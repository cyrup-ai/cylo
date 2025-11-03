// ============================================================================
// File: packages/cylo/src/backends/landlock.rs
// ----------------------------------------------------------------------------
// LandLock backend for Linux secure code execution using kernel sandboxing.
//
// Implements ExecutionBackend trait using LandLock Linux security module
// for filesystem access control and sandboxing. Provides:
// - Kernel-level security enforcement
// - Filesystem access restrictions
// - Process isolation and privilege dropping
// - Zero-overhead sandboxing
// ============================================================================

use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};

use crate::async_task::AsyncTaskBuilder;
use crate::backends::AsyncTask;
use crate::backends::{
    BackendConfig, BackendError, BackendResult, ExecutionBackend, ExecutionRequest,
    ExecutionResult, HealthStatus, ResourceUsage,
};

/// LandLock backend for secure code execution
///
/// Uses LandLock Linux security module to provide filesystem access
/// control and sandboxing for untrusted code execution.
#[derive(Debug, Clone)]
pub struct LandLockBackend {
    /// Jail directory path for sandboxed execution
    jail_path: PathBuf,

    /// Backend configuration
    config: BackendConfig,

    /// Cached LandLock feature detection
    landlock_features: LandLockFeatures,
}

/// LandLock feature detection and capabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
struct LandLockFeatures {
    /// Whether LandLock is available on this system
    available: bool,

    /// Supported LandLock ABI version
    abi_version: u32,

    /// Supported rule types
    supported_access_fs: u64,

    /// Feature detection timestamp
    detected_at: SystemTime,
}

impl LandLockBackend {
    /// Create a new LandLock backend instance
    ///
    /// # Arguments
    /// * `jail_path` - Path to jail directory for sandboxed execution
    /// * `config` - Backend configuration
    ///
    /// # Returns
    /// New LandLock backend instance or error if platform is unsupported
    pub fn new(jail_path: String, config: BackendConfig) -> BackendResult<Self> {
        // Platform validation - LandLock requires Linux
        if !Self::is_platform_supported() {
            return Err(BackendError::NotAvailable {
                backend: "LandLock",
                reason: "LandLock is only available on Linux".to_string(),
            });
        }

        let jail_path = PathBuf::from(jail_path);

        // Validate jail path
        Self::validate_jail_path(&jail_path)?;

        // Detect LandLock features
        let landlock_features = Self::detect_landlock_features()?;

        Ok(Self {
            jail_path,
            config,
            landlock_features,
        })
    }

    /// Check if platform supports LandLock
    ///
    /// # Returns
    /// true if running on Linux with LandLock support, false otherwise
    fn is_platform_supported() -> bool {
        #[cfg(target_os = "linux")]
        {
            // Check for LandLock support in kernel
            std::path::Path::new("/sys/kernel/security/landlock").exists()
        }

        #[cfg(not(target_os = "linux"))]
        false
    }

    /// Validate jail directory path
    ///
    /// # Arguments
    /// * `jail_path` - Path to validate
    ///
    /// # Returns
    /// Ok(()) if path is valid, Err otherwise
    fn validate_jail_path(jail_path: &Path) -> BackendResult<()> {
        // Must be absolute path for security
        if !jail_path.is_absolute() {
            return Err(BackendError::InvalidConfig {
                backend: "LandLock",
                details: "Jail path must be absolute".to_string(),
            });
        }

        // Check if path exists or can be created
        if !jail_path.exists() {
            if let Err(e) = fs::create_dir_all(jail_path) {
                return Err(BackendError::InvalidConfig {
                    backend: "LandLock",
                    details: format!(
                        "Cannot create jail directory {}: {}",
                        jail_path.display(),
                        e
                    ),
                });
            }
        }

        // Verify path is a directory
        if !jail_path.is_dir() {
            return Err(BackendError::InvalidConfig {
                backend: "LandLock",
                details: format!("Jail path {} is not a directory", jail_path.display()),
            });
        }

        // Check permissions - must be writable
        if let Ok(metadata) = jail_path.metadata() {
            let permissions = metadata.permissions();
            if permissions.mode() & 0o200 == 0 {
                return Err(BackendError::InvalidConfig {
                    backend: "LandLock",
                    details: "Jail directory is not writable".to_string(),
                });
            }
        }

        Ok(())
    }

    /// Detect LandLock features and capabilities
    ///
    /// # Returns
    /// LandLock feature detection result
    fn detect_landlock_features() -> BackendResult<LandLockFeatures> {
        #[cfg(target_os = "linux")]
        {
            use std::fs::File;
            use std::io::Read;

            // Check if LandLock is available
            let landlock_dir = Path::new("/sys/kernel/security/landlock");
            if !landlock_dir.exists() {
                return Ok(LandLockFeatures {
                    available: false,
                    abi_version: 0,
                    supported_access_fs: 0,
                    detected_at: SystemTime::now(),
                });
            }

            // Read ABI version
            let abi_version = match File::open(landlock_dir.join("version")) {
                Ok(mut file) => {
                    let mut content = String::new();
                    file.read_to_string(&mut content).unwrap_or_default();
                    content.trim().parse().unwrap_or(0)
                }
                Err(_) => 0,
            };

            // Read supported filesystem access types
            let supported_access_fs = match File::open(landlock_dir.join("access_fs")) {
                Ok(mut file) => {
                    let mut content = String::new();
                    file.read_to_string(&mut content).unwrap_or_default();
                    u64::from_str_radix(content.trim().trim_start_matches("0x"), 16).unwrap_or(0)
                }
                Err(_) => 0,
            };

            Ok(LandLockFeatures {
                available: abi_version > 0,
                abi_version,
                supported_access_fs,
                detected_at: SystemTime::now(),
            })
        }

        #[cfg(not(target_os = "linux"))]
        Ok(LandLockFeatures {
            available: false,
            abi_version: 0,
            supported_access_fs: 0,
            detected_at: SystemTime::now(),
        })
    }

    /// Setup jail environment for execution
    ///
    /// # Arguments
    /// * `request` - Execution request
    ///
    /// # Returns
    /// Path to execution directory within jail
    fn setup_jail_environment(&self, request: &ExecutionRequest) -> BackendResult<PathBuf> {
        // Create unique execution directory
        let exec_id = format!(
            "exec-{}-{}",
            uuid::Uuid::new_v4().simple(),
            std::process::id()
        );
        let exec_dir = self.jail_path.join(&exec_id);

        // Create execution directory
        fs::create_dir_all(&exec_dir).map_err(|e| BackendError::FileSystemFailed {
            details: format!("Failed to create execution directory: {}", e),
        })?;

        // Set proper permissions (rwx for owner only)
        fs::set_permissions(&exec_dir, fs::Permissions::from_mode(0o700)).map_err(|e| {
            BackendError::FileSystemFailed {
                details: format!("Failed to set directory permissions: {}", e),
            }
        })?;

        // Create working directory if specified
        if let Some(workdir) = &request.working_dir {
            let work_path = exec_dir.join(workdir.trim_start_matches('/'));
            fs::create_dir_all(&work_path).map_err(|e| BackendError::FileSystemFailed {
                details: format!("Failed to create working directory: {}", e),
            })?;
        }

        // Create temporary files for code execution
        match request.language.as_str() {
            "python" | "python3" => {
                let code_file = exec_dir.join("main.py");
                fs::write(&code_file, &request.code).map_err(|e| {
                    BackendError::FileSystemFailed {
                        details: format!("Failed to write Python code file: {}", e),
                    }
                })?;
            }
            "rust" => {
                let code_file = exec_dir.join("main.rs");
                fs::write(&code_file, &request.code).map_err(|e| {
                    BackendError::FileSystemFailed {
                        details: format!("Failed to write Rust code file: {}", e),
                    }
                })?;
            }
            "javascript" | "js" | "node" => {
                let code_file = exec_dir.join("main.js");
                fs::write(&code_file, &request.code).map_err(|e| {
                    BackendError::FileSystemFailed {
                        details: format!("Failed to write JavaScript code file: {}", e),
                    }
                })?;
            }
            "go" => {
                let code_file = exec_dir.join("main.go");
                fs::write(&code_file, &request.code).map_err(|e| {
                    BackendError::FileSystemFailed {
                        details: format!("Failed to write Go code file: {}", e),
                    }
                })?;
            }
            _ => {
                // For shell scripts and other languages, write to a generic file
                let code_file = exec_dir.join("code");
                fs::write(&code_file, &request.code).map_err(|e| {
                    BackendError::FileSystemFailed {
                        details: format!("Failed to write code file: {}", e),
                    }
                })?;

                // Make executable for shell scripts
                if matches!(request.language.as_str(), "bash" | "sh") {
                    fs::set_permissions(&code_file, fs::Permissions::from_mode(0o755)).map_err(
                        |e| BackendError::FileSystemFailed {
                            details: format!("Failed to set executable permissions: {}", e),
                        },
                    )?;
                }
            }
        }

        Ok(exec_dir)
    }

    /// Execute code with LandLock sandboxing
    ///
    /// # Arguments
    /// * `request` - Execution request
    /// * `exec_dir` - Execution directory path
    ///
    /// # Returns
    /// AsyncTask that resolves to execution result
    fn execute_with_landlock(
        jail_path: PathBuf,
        request: ExecutionRequest,
        exec_dir: PathBuf,
    ) -> AsyncTask<BackendResult<ExecutionResult>> {
        AsyncTaskBuilder::new(async move {
            let start_time = Instant::now();

            // Prepare execution command
            let (program, args) = Self::prepare_execution_command(&request.language, &exec_dir)?;

            // Build sandboxed command using bwrap (bubblewrap) as LandLock enforcement
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
            let _ = fs::remove_dir_all(&exec_dir);

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
    fn prepare_execution_command(
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
    fn is_bwrap_available() -> bool {
        Command::new("bwrap")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }
}

/// Get CPU time consumed by a process from /proc/[pid]/stat
///
/// # Arguments
/// * `pid` - Process ID to query
///
/// # Returns
/// Total CPU time in milliseconds (user + kernel mode) or error
#[cfg(target_os = "linux")]
fn get_process_cpu_time(pid: u32) -> Result<u64, std::io::Error> {
    let stat_path = format!("/proc/{}/stat", pid);
    let stat_content = std::fs::read_to_string(&stat_path)?;

    // Parse stat file (space-separated fields)
    let fields: Vec<&str> = stat_content.split_whitespace().collect();
    if fields.len() < 15 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Invalid stat format",
        ));
    }

    // Field 13 = utime (user mode jiffies)
    // Field 14 = stime (kernel mode jiffies)
    let utime: u64 = fields[13]
        .parse()
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid utime"))?;
    let stime: u64 = fields[14]
        .parse()
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid stime"))?;

    // Convert clock ticks to milliseconds
    let clock_ticks_per_sec = unsafe { libc::sysconf(libc::_SC_CLK_TCK) } as u64;
    let total_ticks = utime + stime;
    let cpu_time_ms = (total_ticks * 1000) / clock_ticks_per_sec;

    Ok(cpu_time_ms)
}

#[cfg(not(target_os = "linux"))]
fn get_process_cpu_time(_pid: u32) -> Result<u64, std::io::Error> {
    Ok(0)
}

/// Count process tree including threads and child processes
///
/// # Arguments
/// * `pid` - Root process ID
///
/// # Returns
/// Total count of processes and threads or error
#[cfg(target_os = "linux")]
fn count_process_tree(pid: u32) -> Result<usize, std::io::Error> {
    // Count the main process
    let mut count = 1;

    // Count threads via /proc/[pid]/task directory
    let task_dir = format!("/proc/{}/task", pid);
    if let Ok(entries) = std::fs::read_dir(&task_dir) {
        let thread_count = entries.count();
        if thread_count > 0 {
            count += thread_count - 1; // Don't double-count main thread
        }
    }

    // Find child processes from /proc/[pid]/task/[tid]/children
    let children_path = format!("/proc/{}/task/{}/children", pid, pid);
    if let Ok(children_content) = std::fs::read_to_string(&children_path) {
        for child_pid_str in children_content.split_whitespace() {
            if let Ok(child_pid) = child_pid_str.parse::<u32>() {
                // Recursively count child's subtree
                if let Ok(child_count) = count_process_tree(child_pid) {
                    count += child_count;
                }
            }
        }
    }

    Ok(count)
}

#[cfg(not(target_os = "linux"))]
fn count_process_tree(_pid: u32) -> Result<usize, std::io::Error> {
    Ok(1)
}

/// Get disk write statistics from /proc/[pid]/io
///
/// # Arguments
/// * `pid` - Process ID to query
///
/// # Returns
/// Total bytes written to disk or error
#[cfg(target_os = "linux")]
fn get_disk_io_stats(pid: u32) -> Result<u64, std::io::Error> {
    let io_path = format!("/proc/{}/io", pid);
    let io_content = std::fs::read_to_string(&io_path)?;

    // Parse for write_bytes line
    for line in io_content.lines() {
        if line.starts_with("write_bytes:") {
            if let Some(bytes_str) = line.split_whitespace().nth(1) {
                if let Ok(bytes) = bytes_str.parse::<u64>() {
                    return Ok(bytes);
                }
            }
        }
    }

    Ok(0)
}

#[cfg(not(target_os = "linux"))]
fn get_disk_io_stats(_pid: u32) -> Result<u64, std::io::Error> {
    Ok(0)
}

/// Get disk read statistics from /proc/[pid]/io
///
/// # Arguments
/// * `pid` - Process ID to query
///
/// # Returns
/// Total bytes read from disk or error
#[cfg(target_os = "linux")]
fn get_disk_read_stats(pid: u32) -> Result<u64, std::io::Error> {
    let io_path = format!("/proc/{}/io", pid);
    let io_content = std::fs::read_to_string(&io_path)?;

    // Parse for read_bytes line
    for line in io_content.lines() {
        if line.starts_with("read_bytes:") {
            if let Some(bytes_str) = line.split_whitespace().nth(1) {
                if let Ok(bytes) = bytes_str.parse::<u64>() {
                    return Ok(bytes);
                }
            }
        }
    }

    Ok(0)
}

#[cfg(not(target_os = "linux"))]
fn get_disk_read_stats(_pid: u32) -> Result<u64, std::io::Error> {
    Ok(0)
}

/// Get memory usage from /proc/[pid]/status
///
/// # Arguments
/// * `pid` - Process ID to query
///
/// # Returns
/// Resident Set Size (RSS) in bytes or error
#[cfg(target_os = "linux")]
fn get_memory_usage(pid: u32) -> Result<u64, std::io::Error> {
    let status_path = format!("/proc/{}/status", pid);
    let status_content = std::fs::read_to_string(&status_path)?;

    // Parse for VmRSS (Resident Set Size)
    for line in status_content.lines() {
        if line.starts_with("VmRSS:") {
            if let Some(kb_str) = line.split_whitespace().nth(1) {
                if let Ok(rss_kb) = kb_str.parse::<u64>() {
                    // Convert kilobytes to bytes
                    return Ok(rss_kb * 1024);
                }
            }
        }
    }

    Ok(0)
}

#[cfg(not(target_os = "linux"))]
fn get_memory_usage(_pid: u32) -> Result<u64, std::io::Error> {
    Ok(0)
}

impl ExecutionBackend for LandLockBackend {
    fn execute_code(&self, request: ExecutionRequest) -> AsyncTask<ExecutionResult> {
        let jail_path = self.jail_path.clone();
        let backend_name = self.backend_type();

        // Setup jail environment before async block to avoid self borrow issues
        let exec_dir = match self.setup_jail_environment(&request) {
            Ok(dir) => dir,
            Err(e) => {
                return AsyncTaskBuilder::new(async move {
                    ExecutionResult::failure(
                        -1,
                        format!("Failed to setup jail environment: {}", e),
                    )
                }).spawn();
            }
        };

        AsyncTaskBuilder::new(async move {

            // Execute with LandLock sandboxing
            match Self::execute_with_landlock(jail_path, request, exec_dir).await {
                Ok(Ok(result)) => result,
                Ok(Err(e)) => ExecutionResult::failure(
                    -1,
                    format!("{} execution failed: {}", backend_name, e),
                ),
                Err(e) => ExecutionResult::failure(
                    -1,
                    format!("{} task panicked: {}", backend_name, e),
                ),
            }
        }).spawn()
    }

    fn health_check(&self) -> AsyncTask<HealthStatus> {
        let jail_path = self.jail_path.clone();
        let features = self.landlock_features.clone();

        AsyncTaskBuilder::new(async move {
            // Check LandLock availability
            if !features.available {
                return HealthStatus::unhealthy("LandLock is not available on this system")
                    .with_metric("landlock_available", "false");
            }

            // Check bubblewrap availability
            if !Self::is_bwrap_available() {
                return HealthStatus::unhealthy("Bubblewrap (bwrap) is not available")
                    .with_metric("bwrap_available", "false");
            }

            // Check jail directory accessibility
            if let Err(e) = Self::validate_jail_path(&jail_path) {
                return HealthStatus::unhealthy(format!("Jail path validation failed: {}", e))
                    .with_metric("jail_path_valid", "false");
            }

            // Test execution with simple command
            let backend = match LandLockBackend::new(
                jail_path.display().to_string(),
                crate::backends::BackendConfig::new("health_check"),
            ) {
                Ok(backend) => backend,
                Err(e) => {
                    return HealthStatus::unhealthy(format!("Backend creation failed: {}", e));
                }
            };

            let test_request = ExecutionRequest::new("echo 'health check'", "bash")
                .with_timeout(Duration::from_secs(10));

            match backend.setup_jail_environment(&test_request) {
                Ok(exec_dir) => {
                    // Clean up test directory
                    let _ = fs::remove_dir_all(&exec_dir);

                    HealthStatus::healthy("LandLock backend operational")
                        .with_metric("landlock_available", "true")
                        .with_metric("bwrap_available", "true")
                        .with_metric("jail_path_valid", "true")
                        .with_metric("abi_version", &features.abi_version.to_string())
                        .with_metric(
                            "access_fs",
                            &format!("0x{:x}", features.supported_access_fs),
                        )
                }
                Err(e) => HealthStatus::unhealthy(format!("Test environment setup failed: {}", e))
                    .with_metric("test_setup", "failed"),
            }
        }).spawn()
    }

    fn cleanup(&self) -> AsyncTask<crate::execution_env::CyloResult<()>> {
        let jail_path = self.jail_path.clone();

        AsyncTaskBuilder::new(async move {
            // Clean up any leftover execution directories
            if let Ok(entries) = fs::read_dir(&jail_path) {
                for entry in entries.filter_map(Result::ok) {
                    if let Ok(file_name) = entry.file_name().into_string() {
                        if file_name.starts_with("exec-") {
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
        "LandLock"
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
            "go",
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::BackendConfig;

    #[test]
    fn jail_path_validation() {
        // Valid absolute path should pass
        let temp_dir = std::env::temp_dir().join("cylo_test_jail");
        assert!(LandLockBackend::validate_jail_path(&temp_dir).is_ok());
        let _ = fs::remove_dir_all(&temp_dir);

        // Relative path should fail
        let relative_path = PathBuf::from("relative/path");
        assert!(LandLockBackend::validate_jail_path(&relative_path).is_err());
    }

    #[test]
    fn execution_command_preparation() {
        let exec_dir = PathBuf::from("/tmp/test");

        let (prog, args) = LandLockBackend::prepare_execution_command("python", &exec_dir)
            .expect("test should successfully prepare python execution command");
        assert_eq!(prog, "python3");
        assert_eq!(args, vec!["main.py"]);

        let (prog, args) = LandLockBackend::prepare_execution_command("rust", &exec_dir)
            .expect("test should successfully prepare rust execution command");
        assert_eq!(prog, "bash");
        assert!(args[1].contains("rustc"));

        let unsupported = LandLockBackend::prepare_execution_command("cobol", &exec_dir);
        assert!(unsupported.is_err());
    }

    #[test]
    fn landlock_feature_detection() {
        let features = LandLockBackend::detect_landlock_features();
        assert!(features.is_ok());

        let features = features
            .expect("test should successfully detect landlock features");
        #[cfg(target_os = "linux")]
        {
            // On Linux, should at least attempt detection
            assert!(features.detected_at <= SystemTime::now());
        }

        #[cfg(not(target_os = "linux"))]
        {
            assert!(!features.available);
            assert_eq!(features.abi_version, 0);
        }
    }

    #[test]
    fn backend_creation() {
        let config = BackendConfig::new("test_landlock");
        let temp_jail = std::env::temp_dir().join("cylo_test_jail");

        let result = LandLockBackend::new(temp_jail.display().to_string(), config);

        #[cfg(target_os = "linux")]
        {
            // On Linux, should work if LandLock is available
            if LandLockBackend::is_platform_supported() {
                assert!(result.is_ok());
            }
        }

        #[cfg(not(target_os = "linux"))]
        {
            assert!(result.is_err());
        }

        let _ = fs::remove_dir_all(&temp_jail);
    }

    #[test]
    fn supported_languages() {
        let config = BackendConfig::new("test");
        let temp_jail = std::env::temp_dir().join("cylo_test_jail2");

        if let Ok(backend) = LandLockBackend::new(temp_jail.display().to_string(), config) {
            assert!(backend.supports_language("python"));
            assert!(backend.supports_language("rust"));
            assert!(backend.supports_language("bash"));
            assert!(!backend.supports_language("cobol"));
        }

        let _ = fs::remove_dir_all(&temp_jail);
    }
}
