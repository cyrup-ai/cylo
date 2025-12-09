// ============================================================================
// File: packages/cylo/src/backends/windows/mod.rs
// ----------------------------------------------------------------------------
// Windows Job Objects backend for secure code execution using Windows sandboxing.
//
// Implements ExecutionBackend trait using Windows Job Objects for:
// - Process isolation and grouping
// - Resource limits (memory, CPU, process count)
// - Automatic cleanup via kill-on-job-close
// - Windows-native sandboxing
// ============================================================================

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Instant;

use crate::async_task::AsyncTaskBuilder;
use crate::backends::AsyncTask;
use crate::backends::{
    BackendConfig, BackendError, BackendResult, ExecutionBackend, ExecutionRequest,
    ExecutionResult, HealthStatus,
};

mod job;
mod limits;

use job::JobManager;
use limits::WindowsLimits;

/// Windows Job Objects backend for secure code execution
///
/// Uses Windows Job Objects to provide process sandboxing and resource
/// control for untrusted code execution.
#[derive(Debug, Clone)]
pub struct WindowsJobBackend {
    /// Workspace name for execution isolation
    workspace_name: String,

    /// Backend configuration
    config: BackendConfig,
}

impl WindowsJobBackend {
    /// Create a new Windows Job backend instance
    ///
    /// # Arguments
    /// * `workspace_name` - Name of the workspace for isolation
    /// * `config` - Backend configuration
    ///
    /// # Returns
    /// New Windows Job backend instance or error if platform is unsupported
    pub fn new(workspace_name: String, config: BackendConfig) -> BackendResult<Self> {
        // Platform validation - WindowsJob requires Windows
        if cfg!(not(target_os = "windows")) {
            return Err(BackendError::NotAvailable {
                backend: "windows",
                reason: "WindowsJob backend is only available on Windows".to_string(),
            });
        }

        // Validate workspace name
        if workspace_name.is_empty() {
            return Err(BackendError::InvalidConfig {
                backend: "windows",
                details: "Workspace name cannot be empty".to_string(),
            });
        }

        Ok(Self {
            workspace_name,
            config,
        })
    }

    /// Get the language-specific command to execute code
    ///
    /// # Arguments
    /// * `language` - Programming language
    /// * `file_path` - Path to the code file
    ///
    /// # Returns
    /// Command to execute the code, or error if language is unsupported
    fn get_execution_command(language: &str, file_path: &PathBuf) -> BackendResult<Command> {
        let mut cmd = match language.to_lowercase().as_str() {
            "python" | "python3" => {
                let mut c = Command::new("python");
                c.arg(file_path);
                c
            }
            "rust" => {
                // Compile Rust source to Windows executable
                let exe_path = file_path.with_extension("exe");

                log::debug!(
                    "Compiling Rust code: {:?} -> {:?}",
                    file_path,
                    exe_path
                );

                // Execute rustc to compile the code
                let compile_output = Command::new("rustc")
                    .arg(file_path)
                    .arg("-o")
                    .arg(&exe_path)
                    .output()
                    .map_err(|e| BackendError::ProcessFailed {
                        details: format!("Failed to execute rustc (is Rust installed?): {}", e)
                    })?;

                // Check compilation result
                if !compile_output.status.success() {
                    let stderr = String::from_utf8_lossy(&compile_output.stderr);
                    let stdout = String::from_utf8_lossy(&compile_output.stdout);
                    let combined = if stdout.is_empty() {
                        stderr.to_string()
                    } else {
                        format!("{}\n{}", stdout, stderr)
                    };

                    log::error!("Rust compilation failed: {}", combined);

                    return Err(BackendError::ProcessFailed {
                        details: format!("Rust compilation failed:\n{}", combined)
                    });
                }

                log::debug!("Rust compilation successful, executable: {:?}", exe_path);

                // Return command to execute the compiled binary
                let mut c = Command::new(&exe_path);
                c
            }
            "javascript" | "js" | "node" => {
                let mut c = Command::new("node");
                c.arg(file_path);
                c
            }
            "bash" | "sh" => {
                let mut c = Command::new("powershell");
                c.arg("-File").arg(file_path);
                c
            }
            _ => {
                return Err(BackendError::NotAvailable {
                    backend: "windows",
                    reason: format!("Language '{}' not supported", language),
                });
            }
        };

        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        Ok(cmd)
    }

    /// Execute code with Job Object isolation
    ///
    /// # Arguments
    /// * `workspace_name` - Name of the workspace for identification and logging
    /// * `request` - Execution request
    ///
    /// # Returns
    /// Execution result with output and metrics
    async fn execute_with_job(workspace_name: String, request: ExecutionRequest) -> BackendResult<ExecutionResult> {
        log::info!("Executing code in workspace: {}", workspace_name);
        let start_time = Instant::now();

        // Create temporary directory for code execution
        let temp_dir = std::env::temp_dir().join(&format!("cylo_{}_{}", workspace_name, uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir)
            .map_err(|e| BackendError::Internal {
                message: format!("Failed to create temp directory: {}", e)
            })?;

        // Determine file extension
        let extension = match request.language.to_lowercase().as_str() {
            "python" | "python3" => "py",
            "rust" => "rs",
            "javascript" | "js" | "node" => "js",
            "bash" | "sh" => "ps1", // Use PowerShell on Windows
            _ => "txt",
        };

        // Write code to temporary file
        let code_file = temp_dir.join(format!("code.{}", extension));
        let mut file = fs::File::create(&code_file)
            .map_err(|e| BackendError::ProcessFailed {
                details: format!("Failed to create code file: {}", e)
            })?;
        file.write_all(request.code.as_bytes())
            .map_err(|e| BackendError::ProcessFailed {
                details: format!("Failed to write code: {}", e)
            })?;

        // Convert resource limits to Windows limits
        let windows_limits = WindowsLimits::from_resource_limits(&request.limits)?;

        // Create job object with limits
        let job = JobManager::create_with_limits(&windows_limits)?;

        // Get execution command
        let mut cmd = Self::get_execution_command(&request.language, &code_file)?;

        // Set working directory if specified
        if let Some(ref work_dir) = request.working_dir {
            cmd.current_dir(work_dir);
        } else {
            cmd.current_dir(&temp_dir);
        }

        // Set environment variables
        for (key, value) in &request.env_vars {
            cmd.env(key, value);
        }

        // Spawn the process
        let mut child = cmd.spawn()
            .map_err(|e| BackendError::ProcessFailed {
                details: format!("Failed to spawn process: {}", e)
            })?;

        // Get process ID and assign to job
        let process_id = child.id();

        // Validate PID before assignment
        if !job::is_valid_pid(process_id) {
            return Err(BackendError::ProcessFailed {
                details: format!("Child process has invalid PID: {}", process_id)
            });
        }

        job.assign_process(process_id)?;

        // Provide input if specified
        if let Some(ref input_data) = request.input {
            if let Some(ref mut stdin) = child.stdin {
                stdin.write_all(input_data.as_bytes())
                    .map_err(|e| BackendError::ProcessFailed {
                        details: format!("Failed to write stdin: {}", e)
                    })?;
            }
        }

        // Wait for completion with timeout
        let output = if request.timeout.as_secs() > 0 {
            // Use tokio timeout
            match tokio::time::timeout(request.timeout, async move {
                child.wait_with_output()
            }).await {
                Ok(result) => result.map_err(|e| BackendError::ProcessFailed {
                    details: format!("Process execution failed: {}", e)
                })?,
                Err(_) => {
                    // Timeout occurred - terminate the job
                    let _ = job.terminate_all(1);
                    return Err(BackendError::ExecutionTimeout {
                        seconds: request.timeout.as_secs()
                    });
                }
            }
        } else {
            child.wait_with_output()
                .map_err(|e| BackendError::ProcessFailed {
                    details: format!("Process execution failed: {}", e)
                })?
        };

        let duration = start_time.elapsed();

        // Query comprehensive job statistics
        let process_count = job.active_process_count().unwrap_or(1);
        let (cpu_time_ms, disk_read_bytes, disk_write_bytes, network_other_bytes) =
            job.get_cpu_and_io_stats().unwrap_or((0, 0, 0, 0));
        let peak_memory = job.get_memory_usage().unwrap_or(0);

        // Clean up temporary directory
        let _ = fs::remove_dir_all(&temp_dir);

        // Build execution result
        let exit_code = output.status.code().unwrap_or(-1);
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        let mut result = if exit_code == 0 {
            ExecutionResult::success(stdout)
        } else {
            ExecutionResult::failure(exit_code, stderr)
        };

        result.duration = duration;
        result.resource_usage.process_count = process_count;
        result.resource_usage.cpu_time_ms = cpu_time_ms;
        result.resource_usage.peak_memory = peak_memory;
        result.resource_usage.disk_bytes_read = disk_read_bytes;
        result.resource_usage.disk_bytes_written = disk_write_bytes;
        // OtherTransferCount includes network and other non-read/write I/O
        // Split evenly as approximation since Windows doesn't distinguish sent/received
        result.resource_usage.network_bytes_sent = network_other_bytes / 2;
        result.resource_usage.network_bytes_received = network_other_bytes / 2;
        result.metadata.insert("backend".to_string(), "WindowsJob".to_string());
        result.metadata.insert("workspace".to_string(), workspace_name);

        Ok(result)
    }
}

impl ExecutionBackend for WindowsJobBackend {
    fn execute_code(&self, request: ExecutionRequest) -> AsyncTask<ExecutionResult> {
        let workspace_name = self.workspace_name.clone();
        AsyncTaskBuilder::new(async move {
            match Self::execute_with_job(workspace_name, request).await {
                Ok(result) => result,
                Err(e) => ExecutionResult::failure(-1, format!("WindowsJob execution failed: {}", e)),
            }
        }).spawn()
    }

    fn health_check(&self) -> AsyncTask<HealthStatus> {
        AsyncTaskBuilder::new(async move {
            // Check if we can create a basic job object
            let limits = WindowsLimits {
                memory_bytes: None,
                cpu_time_ms: None,
                max_processes: None,
            };

            match JobManager::create_with_limits(&limits) {
                Ok(_) => {
                    HealthStatus::healthy("WindowsJob backend operational")
                        .with_metric("job_creation", "success")
                }
                Err(e) => {
                    HealthStatus::unhealthy(format!("Job creation failed: {}", e))
                        .with_metric("job_creation", "failed")
                }
            }
        }).spawn()
    }

    fn cleanup(&self) -> AsyncTask<crate::execution_env::CyloResult<()>> {
        AsyncTaskBuilder::new(async move {
            // Clean up any leftover temporary directories
            let temp_base = std::env::temp_dir();
            if let Ok(entries) = fs::read_dir(&temp_base) {
                for entry in entries.flatten() {
                    if let Ok(name) = entry.file_name().into_string() {
                        if name.starts_with("cylo_") {
                            let _ = fs::remove_dir_all(entry.path());
                        }
                    }
                }
            }
            Ok(())
        }).spawn()
    }

    fn get_config(&self) -> &BackendConfig {
        &self.config
    }

    fn backend_type(&self) -> &'static str {
        "WindowsJob"
    }

    fn supports_language(&self, language: &str) -> bool {
        self.supported_languages().contains(&language)
    }

    fn supported_languages(&self) -> &[&'static str] {
        &[
            "python",
            "python3",
            "javascript",
            "js",
            "node",
            "rust",
            "bash",
            "sh",
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_creation() {
        let config = BackendConfig::new("test_windows");
        let result = WindowsJobBackend::new("test_workspace".to_string(), config);

        #[cfg(target_os = "windows")]
        {
            assert!(result.is_ok());
        }

        #[cfg(not(target_os = "windows"))]
        {
            assert!(result.is_err());
        }
    }

    #[test]
    fn supported_languages() {
        let config = BackendConfig::new("test");
        if let Ok(backend) = WindowsJobBackend::new("test".to_string(), config) {
            assert!(backend.supports_language("python"));
            assert!(backend.supports_language("javascript"));
            assert!(backend.supports_language("rust"));
            assert!(backend.supports_language("bash"));
            assert!(!backend.supports_language("cobol"));
        }
    }

    #[cfg(target_os = "windows")]
    #[tokio::test]
    async fn test_rust_compilation_and_execution() {
        use std::time::Duration;

        let config = BackendConfig::new("test_rust");
        let backend = match WindowsJobBackend::new("test_rust".to_string(), config) {
            Ok(b) => b,
            Err(_) => return, // Skip if backend unavailable
        };

        // Test 1: Valid Rust code that prints to stdout
        let rust_code = r#"
fn main() {
    println!("Hello from Rust on Windows!");
    println!("Job Object execution successful");
}
"#;

        let request = ExecutionRequest::new(rust_code, "rust")
            .with_timeout(Duration::from_secs(30));

        let result = backend.execute_code(request).await;

        match result {
            ExecutionResult { exit_code: 0, stdout, .. } => {
                assert!(
                    stdout.contains("Hello from Rust on Windows!"),
                    "Expected greeting in stdout, got: {}",
                    stdout
                );
                assert!(
                    stdout.contains("Job Object execution successful"),
                    "Expected success message in stdout, got: {}",
                    stdout
                );
            }
            ExecutionResult { exit_code, stderr, .. } => {
                // If rustc is not installed, skip test
                if stderr.contains("Failed to execute rustc") {
                    eprintln!("Skipping test: rustc not installed");
                    return;
                }
                panic!(
                    "Rust execution failed with exit code {}: {}",
                    exit_code, stderr
                );
            }
        }
    }

    #[cfg(target_os = "windows")]
    #[tokio::test]
    async fn test_rust_compilation_error() {
        use std::time::Duration;

        let config = BackendConfig::new("test_rust_error");
        let backend = match WindowsJobBackend::new("test_rust_error".to_string(), config) {
            Ok(b) => b,
            Err(_) => return,
        };

        // Test 2: Invalid Rust code should fail compilation
        let invalid_rust = r#"
fn main() {
    this_function_does_not_exist();
    let x: u32 = "not a number";
}
"#;

        let request = ExecutionRequest::new(invalid_rust, "rust")
            .with_timeout(Duration::from_secs(30));

        let result = backend.execute_code(request).await;

        // Should fail (non-zero exit code or error in stderr)
        assert!(
            result.exit_code != 0 || !result.stderr.is_empty(),
            "Expected compilation error, but execution succeeded"
        );

        // Error message should mention compilation failure
        let error_output = result.combined_output();
        assert!(
            error_output.contains("error") || error_output.contains("failed"),
            "Expected error message in output, got: {}",
            error_output
        );
    }

    #[cfg(target_os = "windows")]
    #[tokio::test]
    async fn test_rust_resource_limits() {
        use std::time::Duration;
        use crate::backends::config::ResourceLimits;

        let config = BackendConfig::new("test_rust_limits");
        let backend = match WindowsJobBackend::new("test_rust_limits".to_string(), config) {
            Ok(b) => b,
            Err(_) => return,
        };

        // Test 3: Rust code with resource usage tracking
        let rust_code = r#"
fn main() {
    let v: Vec<u32> = (0..1000).collect();
    println!("Allocated vector with {} elements", v.len());
}
"#;

        let limits = ResourceLimits {
            memory_bytes: Some(100 * 1024 * 1024), // 100MB
            cpu_time_ms: Some(10_000),              // 10 seconds
            max_processes: Some(5),
            ..Default::default()
        };

        let request = ExecutionRequest::new(rust_code, "rust")
            .with_timeout(Duration::from_secs(30))
            .with_limits(limits);

        let result = backend.execute_code(request).await;

        if result.exit_code == 0 {
            // Verify resource tracking works
            assert!(
                result.resource_usage.peak_memory > 0,
                "Expected non-zero memory usage"
            );
            assert!(
                result.resource_usage.process_count > 0,
                "Expected at least one process"
            );
            println!(
                "Rust execution metrics - Memory: {}KB, Processes: {}, CPU: {}ms",
                result.resource_usage.peak_memory / 1024,
                result.resource_usage.process_count,
                result.resource_usage.cpu_time_ms
            );
        } else if result.stderr.contains("Failed to execute rustc") {
            eprintln!("Skipping test: rustc not installed");
        }
    }
}
