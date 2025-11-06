// ============================================================================
// File: packages/cylo/src/backends/landlock/execution.rs
// ----------------------------------------------------------------------------
// Core sandboxed execution logic using bubblewrap and LandLock.
//
// Provides secure code execution including:
// - Sandboxed process spawning with bubblewrap
// - Language-specific command preparation
// - Resource limiting and monitoring
// - Timeout handling and process cleanup
// ============================================================================

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::async_task::AsyncTaskBuilder;
use crate::backends::AsyncTask;
use crate::backends::{BackendError, BackendResult, ExecutionRequest, ExecutionResult, ResourceUsage};

use super::jail::JailEnvironment;
use super::monitoring::{
    count_process_tree, get_disk_io_stats, get_disk_read_stats, get_memory_usage,
    get_process_cpu_time,
};

/// Sandboxed code executor using bubblewrap and LandLock
pub struct SandboxedExecutor;

impl SandboxedExecutor {
    /// Execute code with LandLock sandboxing
    ///
    /// # Arguments
    /// * `jail_path` - Base jail directory
    /// * `request` - Execution request
    /// * `exec_dir` - Execution directory path
    ///
    /// # Returns
    /// AsyncTask that resolves to execution result
    pub fn execute(
        jail_path: PathBuf,
        request: ExecutionRequest,
        exec_dir: PathBuf,
    ) -> AsyncTask<BackendResult<ExecutionResult>> {
        AsyncTaskBuilder::new(async move {
            let start_time = Instant::now();

            // Prepare execution command
            let (program, args) = Self::prepare_command(&request.language, &exec_dir)?;

            // Build sandboxed command using bwrap (bubblewrap)
            let mut cmd = Command::new("bwrap");

            // Basic sandboxing arguments
            cmd.args(&[
                "--ro-bind",
                "/usr",
                "/usr", // Read-only system binaries
                "--ro-bind",
                "/lib",
                "/lib", // Read-only system libraries
                "--ro-bind",
                "/lib64",
                "/lib64", // Read-only system libraries
                "--ro-bind",
                "/bin",
                "/bin", // Read-only system binaries
                "--ro-bind",
                "/sbin",
                "/sbin", // Read-only system binaries
                "--tmpfs",
                "/tmp", // Temporary filesystem
                "--proc",
                "/proc", // Process filesystem
                "--dev",
                "/dev", // Device filesystem
                "--bind",
                exec_dir.to_str().unwrap_or(""),
                "/workspace", // Writable workspace
                "--chdir",
                "/workspace",    // Change to workspace
                "--unshare-all", // Unshare all namespaces
                "--share-net",   // Share network (if needed)
            ]);

            // Add resource limits
            if let Some(memory) = request.limits.max_memory {
                // Convert to MB for ulimit
                let memory_mb = memory / (1024 * 1024);
                cmd.args(&[
                    "--",
                    "bash",
                    "-c",
                    &format!(
                        "ulimit -v {} && exec {} {}",
                        memory_mb,
                        program,
                        args.join(" ")
                    ),
                ]);
            } else {
                cmd.arg("--");
                cmd.arg(&program);
                cmd.args(&args);
            }

            // Set environment variables
            for (key, value) in &request.env_vars {
                cmd.env(key, value);
            }

            // Configure stdio
            cmd.stdout(Stdio::piped());
            cmd.stderr(Stdio::piped());
            cmd.stdin(Stdio::piped());

            // Spawn the process
            let mut child = cmd.spawn().map_err(|e| BackendError::ProcessFailed {
                details: format!("Failed to spawn sandboxed process: {}", e),
            })?;

            // Start background resource monitoring task
            let pid = child.id();
            let (tx, mut rx) = tokio::sync::oneshot::channel();

            #[cfg(target_os = "linux")]
            let monitor_handle = tokio::spawn(async move {
                let mut peak_memory = 0u64;
                let mut final_cpu_time = 0u64;
                let mut final_process_count = 1usize;
                let mut final_disk_written = 0u64;
                let mut final_disk_read = 0u64;

                loop {
                    // Poll every 100ms
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_millis(100)) => {
                            // Track peak memory
                            if let Ok(mem) = get_memory_usage(pid) {
                                peak_memory = peak_memory.max(mem);
                            }

                            // Track latest CPU time (cumulative)
                            if let Ok(cpu) = get_process_cpu_time(pid) {
                                final_cpu_time = cpu;
                            }

                            // Track process count
                            if let Ok(count) = count_process_tree(pid) {
                                final_process_count = count;
                            }

                            // Track disk I/O
                            if let Ok(written) = get_disk_io_stats(pid) {
                                final_disk_written = written;
                            }
                            if let Ok(read) = get_disk_read_stats(pid) {
                                final_disk_read = read;
                            }
                        }
                        _ = &mut rx => {
                            // Stop signal received
                            break;
                        }
                    }
                }

                ResourceUsage {
                    peak_memory,
                    cpu_time_ms: final_cpu_time,
                    process_count: final_process_count as u32,
                    disk_bytes_written: final_disk_written,
                    disk_bytes_read: final_disk_read,
                    network_bytes_sent: 0,
                    network_bytes_received: 0,
                }
            });

            #[cfg(not(target_os = "linux"))]
            let monitor_handle = tokio::spawn(async move {
                let _ = rx.await;
                ResourceUsage::default()
            });

            // Write input if provided
            if let Some(input) = &request.input {
                if let Some(stdin) = child.stdin.take() {
                    use std::io::Write;
                    let mut stdin = stdin;
                    stdin
                        .write_all(input.as_bytes())
                        .map_err(|e| BackendError::ProcessFailed {
                            details: format!("Failed to write to process stdin: {}", e),
                        })?;
                }
            }

            // Wait for completion with timeout
            let timeout_duration = request.timeout;
            let child_id = child.id();
            let result =
                tokio::time::timeout(timeout_duration, async { child.wait_with_output() }).await;

            let output = match result {
                Ok(Ok(output)) => output,
                Ok(Err(e)) => {
                    return Err(BackendError::ProcessFailed {
                        details: format!("Process execution failed: {}", e),
                    });
                }
                Err(_) => {
                    // Kill the process on timeout using saved PID
                    #[cfg(target_os = "linux")]
                    {
                        use nix::sys::signal::{kill, Signal};
                        use nix::unistd::Pid;
                        let _ = kill(Pid::from_raw(child_id as i32), Signal::SIGKILL);
                    }
                    #[cfg(not(target_os = "linux"))]
                    {
                        let _ = child_id; // Suppress unused warning
                    }
                    return Err(BackendError::ExecutionTimeout {
                        seconds: timeout_duration.as_secs(),
                    });
                }
            };

            let duration = start_time.elapsed();

            // Stop monitoring and collect final resource statistics
            let _ = tx.send(());
            let resource_usage = match monitor_handle.await {
                Ok(usage) => usage,
                Err(_) => {
                    // Monitoring task failed, return defaults
                    ResourceUsage {
                        peak_memory: 0,
                        cpu_time_ms: 0,
                        process_count: 1,
                        disk_bytes_written: 0,
                        disk_bytes_read: 0,
                        network_bytes_sent: 0,
                        network_bytes_received: 0,
                    }
                }
            };

            // Clean up execution directory
            JailEnvironment::cleanup(&exec_dir);

            Ok(ExecutionResult {
                exit_code: output.status.code().unwrap_or(-1),
                stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
                duration,
                resource_usage,
                metadata: {
                    let mut meta = HashMap::new();
                    meta.insert("backend".to_string(), "LandLock".to_string());
                    meta.insert("jail_path".to_string(), jail_path.display().to_string());
                    meta.insert("exec_dir".to_string(), exec_dir.display().to_string());
                    meta
                },
            })
        }).spawn()
    }

    /// Prepare execution command for specific language
    ///
    /// # Arguments
    /// * `language` - Programming language
    /// * `exec_dir` - Execution directory path
    ///
    /// # Returns
    /// Command program and arguments
    fn prepare_command(
        language: &str,
        _exec_dir: &Path,
    ) -> BackendResult<(String, Vec<String>)> {
        match language.to_lowercase().as_str() {
            "python" | "python3" => Ok(("python3".to_string(), vec!["main.py".to_string()])),
            "javascript" | "js" | "node" => Ok(("node".to_string(), vec!["main.js".to_string()])),
            "rust" => {
                // Compile and run Rust code
                Ok((
                    "bash".to_string(),
                    vec![
                        "-c".to_string(),
                        "rustc main.rs -o main && ./main".to_string(),
                    ],
                ))
            }
            "bash" | "sh" => Ok(("bash".to_string(), vec!["code".to_string()])),
            "go" => Ok((
                "bash".to_string(),
                vec!["-c".to_string(), "go run main.go".to_string()],
            )),
            _ => Err(BackendError::UnsupportedLanguage {
                backend: "LandLock",
                language: language.to_string(),
            }),
        }
    }

    /// Check if bubblewrap is available for sandboxing
    ///
    /// # Returns
    /// true if bwrap is available, false otherwise
    pub fn is_bwrap_available() -> bool {
        Command::new("bwrap")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_preparation() {
        let exec_dir = PathBuf::from("/tmp/test");

        let (prog, args) = SandboxedExecutor::prepare_command("python", &exec_dir)
            .expect("test should successfully prepare python execution command");
        assert_eq!(prog, "python3");
        assert_eq!(args, vec!["main.py"]);

        let (prog, args) = SandboxedExecutor::prepare_command("rust", &exec_dir)
            .expect("test should successfully prepare rust execution command");
        assert_eq!(prog, "bash");
        assert!(args[1].contains("rustc"));

        let unsupported = SandboxedExecutor::prepare_command("cobol", &exec_dir);
        assert!(unsupported.is_err());
    }
}
