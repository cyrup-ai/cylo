// ============================================================================
// File: packages/cylo/src/backends/apple.rs
// ----------------------------------------------------------------------------
// Apple containerization backend for macOS secure code execution.
//
// Implements ExecutionBackend trait using Apple's containerization framework
// via CLI wrapper. Provides:
// - VM-level isolation with hardware security
// - Sub-second startup times
// - OCI-compliant container support
// - Apple Silicon optimization
// ============================================================================

use std::collections::HashMap;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::AsyncTaskBuilder;
use crate::backends::AsyncTask;
use crate::backends::{
    BackendConfig, BackendError, BackendResult, ExecutionBackend, ExecutionRequest,
    ExecutionResult, HealthStatus, ResourceUsage,
};

/// Apple containerization backend
///
/// Uses Apple's containerization framework for secure code execution
/// on macOS with Apple Silicon. Provides VM-level isolation and
/// hardware-backed security features.
#[derive(Debug, Clone)]
pub struct AppleBackend {
    /// Container image specification (e.g., "python:alpine3.20")
    image: String,

    /// Backend configuration
    config: BackendConfig,
}

impl AppleBackend {
    /// Create a new Apple backend instance
    ///
    /// # Arguments
    /// * `image` - Container image specification
    /// * `config` - Backend configuration
    ///
    /// # Returns
    /// New Apple backend instance or error if platform is unsupported
    pub fn new(image: String, config: BackendConfig) -> BackendResult<Self> {
        // Platform validation - Apple containerization requires macOS with Apple Silicon
        if !Self::is_platform_supported() {
            return Err(BackendError::NotAvailable {
                backend: "Apple",
                reason: "Apple containerization requires macOS with Apple Silicon".to_string(),
            });
        }

        // Validate image format
        if !Self::is_valid_image_format(&image) {
            return Err(BackendError::InvalidConfig {
                backend: "Apple",
                details: format!("Invalid image format: {image}. Expected format: 'name:tag'"),
            });
        }

        Ok(Self { image, config })
    }

    /// Check if platform supports Apple containerization
    ///
    /// # Returns
    /// true if running on macOS with Apple Silicon, false otherwise
    fn is_platform_supported() -> bool {
        #[cfg(target_os = "macos")]
        {
            // Check for Apple Silicon architecture
            std::env::consts::ARCH == "aarch64"
        }

        #[cfg(not(target_os = "macos"))]
        false
    }

    /// Validate container image format
    ///
    /// # Arguments
    /// * `image` - Image specification to validate
    ///
    /// # Returns
    /// true if format is valid, false otherwise
    fn is_valid_image_format(image: &str) -> bool {
        // Basic validation: must contain ':' for tag
        if !image.contains(':') {
            return false;
        }

        // Split into name and tag
        let parts: Vec<&str> = image.splitn(2, ':').collect();
        if parts.len() != 2 {
            return false;
        }

        let (name, tag) = (parts[0], parts[1]);

        // Name must not be empty and contain valid characters
        if name.is_empty()
            || !name
                .chars()
                .all(|c| c.is_alphanumeric() || c == '/' || c == '-' || c == '_' || c == '.')
        {
            return false;
        }

        // Tag must not be empty and contain valid characters
        if tag.is_empty()
            || !tag
                .chars()
                .all(|c| c.is_alphanumeric() || c == '.' || c == '-' || c == '_')
        {
            return false;
        }

        true
    }

    /// Check if Apple containerization CLI is available
    ///
    /// # Returns
    /// AsyncTask that resolves to availability status
    fn check_cli_availability() -> AsyncTask<bool> {
        AsyncTaskBuilder::new(async move {
            let result = Command::new("container")
                .arg("--version")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();

            match result {
                Ok(status) => status.success(),
                Err(_) => false,
            }
        })
        .spawn()
    }

    /// Pull container image if not already available
    ///
    /// # Arguments
    /// * `image` - Image to pull
    ///
    /// # Returns
    /// AsyncTask that resolves when image is available
    fn ensure_image_available(image: String) -> AsyncTask<BackendResult<()>> {
        AsyncTaskBuilder::new(async move {
            // Check if image exists locally first
            let check_result = Command::new("container")
                .args(["image", "exists", &image])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();

            match check_result {
                Ok(status) if status.success() => {
                    // Image exists locally
                    return Ok(());
                }
                _ => {
                    // Need to pull image
                }
            }

            // Pull the image
            let pull_result = Command::new("container")
                .args(["pull", &image])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output();

            match pull_result {
                Ok(output) if output.status.success() => Ok(()),
                Ok(output) => {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    Err(BackendError::ContainerFailed {
                        details: format!("Failed to pull image {image}: {stderr}"),
                    })
                }
                Err(e) => Err(BackendError::ContainerFailed {
                    details: format!("Failed to execute container pull: {e}"),
                }),
            }
        })
        .spawn()
    }

    /// Execute code in Apple container
    ///
    /// # Arguments
    /// * `request` - Execution request with code and configuration
    ///
    /// # Returns
    /// AsyncTask that resolves to execution result
    fn execute_in_container(
        image: String,
        request: ExecutionRequest,
    ) -> AsyncTask<BackendResult<ExecutionResult>> {
        AsyncTaskBuilder::new(async move {
            let start_time = Instant::now();

            // Create unique container name
            let container_name = format!(
                "cylo-{}-{}",
                uuid::Uuid::new_v4().simple(),
                std::process::id()
            );

            // Prepare execution command based on language
            let exec_cmd = Self::prepare_execution_command(&request.language, &request.code)?;

            // Build container run command
            let mut cmd = Command::new("container");
            cmd.args(["run", "--rm", "--name", &container_name]);

            // Add resource limits
            if let Some(memory) = request.limits.max_memory {
                cmd.args(["--memory", &format!("{memory}b")]);
            }

            if let Some(cpu_time) = request.limits.max_cpu_time {
                cmd.args(["--cpus", &format!("{cpu_time}")]);
            }

            // Add environment variables
            for (key, value) in &request.env_vars {
                cmd.args(["-e", &format!("{key}={value}")]);
            }

            // Set working directory if specified
            if let Some(workdir) = &request.working_dir {
                cmd.args(["-w", workdir]);
            }

            // Add timeout handling
            cmd.args(["--timeout", &format!("{}s", request.timeout.as_secs())]);

            // Specify image and command
            cmd.arg(&image);
            cmd.args(&exec_cmd);

            // Set up stdio
            cmd.stdout(Stdio::piped());
            cmd.stderr(Stdio::piped());
            cmd.stdin(Stdio::piped());

            // Execute the container
            let mut child = cmd.spawn().map_err(|e| BackendError::ProcessFailed {
                details: format!("Failed to spawn container: {e}"),
            })?;

            // Write input if provided
            if let Some(input) = &request.input
                && let Some(stdin) = child.stdin.take()
            {
                use std::io::Write;
                let mut stdin = stdin;
                stdin
                    .write_all(input.as_bytes())
                    .map_err(|e| BackendError::ProcessFailed {
                        details: format!("Failed to write to container stdin: {e}"),
                    })?;
            }

            // Wait for completion with timeout
            let timeout_duration = request.timeout;

            // Use a different approach - spawn a task that can kill the process
            let child_handle = tokio::spawn(async move { child.wait_with_output() });

            let output = match tokio::time::timeout(timeout_duration, child_handle).await {
                Ok(Ok(Ok(output))) => output,
                Ok(Ok(Err(e))) => {
                    return Err(BackendError::ProcessFailed {
                        details: format!("Container execution failed: {e}"),
                    });
                }
                Ok(Err(_)) => {
                    return Err(BackendError::ProcessFailed {
                        details: "Container process task failed".to_string(),
                    });
                }
                Err(_) => {
                    // Timeout occurred - the process is still running but we can't kill it
                    // from here since it's been moved into the task
                    return Err(BackendError::ExecutionTimeout {
                        seconds: timeout_duration.as_secs(),
                    });
                }
            };

            let duration = start_time.elapsed();

            // Parse resource usage from container stats (if available)
            let resource_usage = Self::parse_resource_usage(&container_name)
                .await
                .unwrap_or_default();

            Ok(ExecutionResult {
                exit_code: output.status.code().unwrap_or(-1),
                stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
                duration,
                resource_usage,
                metadata: {
                    let mut meta = HashMap::new();
                    meta.insert("backend".to_string(), "Apple".to_string());
                    meta.insert("image".to_string(), image);
                    meta.insert("container_name".to_string(), container_name);
                    meta
                },
            })
        })
        .spawn()
    }

    /// Prepare execution command for specific language
    ///
    /// # Arguments
    /// * `language` - Programming language
    /// * `code` - Source code to execute
    ///
    /// # Returns
    /// Command arguments for container execution
    fn prepare_execution_command(language: &str, code: &str) -> BackendResult<Vec<String>> {
        match language.to_lowercase().as_str() {
            "python" | "python3" => Ok(vec![
                "python3".to_string(),
                "-c".to_string(),
                code.to_string(),
            ]),
            "javascript" | "js" | "node" => {
                Ok(vec!["node".to_string(), "-e".to_string(), code.to_string()])
            }
            "rust" => {
                // For Rust, we need to create a temporary file and compile
                Ok(vec![
                    "sh".to_string(),
                    "-c".to_string(),
                    format!(
                        "echo '{}' > /tmp/main.rs && cd /tmp && rustc main.rs && ./main",
                        code.replace('\'', "'\"'\"'")
                    ),
                ])
            }
            "bash" | "sh" => Ok(vec!["sh".to_string(), "-c".to_string(), code.to_string()]),
            "go" => Ok(vec![
                "sh".to_string(),
                "-c".to_string(),
                format!(
                    "echo '{}' > /tmp/main.go && cd /tmp && go run main.go",
                    code.replace('\'', "'\"'\"'")
                ),
            ]),
            _ => Err(BackendError::UnsupportedLanguage {
                backend: "Apple",
                language: language.to_string(),
            }),
        }
    }

    /// Parse resource usage from container stats
    ///
    /// # Arguments
    /// * `container_name` - Name of the container
    ///
    /// # Returns
    /// Resource usage statistics or default values
    async fn parse_resource_usage(container_name: &str) -> Option<ResourceUsage> {
        let stats_result = Command::new("container")
            .args(["stats", "--no-stream", "--format", "json", container_name])
            .output();

        match stats_result {
            Ok(output) if output.status.success() => {
                let stats_json = String::from_utf8_lossy(&output.stdout);
                if let Ok(stats) = serde_json::from_str::<serde_json::Value>(&stats_json) {
                    Some(ResourceUsage {
                        peak_memory: stats["memory"]["usage"].as_u64().unwrap_or(0),
                        cpu_time_ms: stats["cpu"]["total_usage"].as_u64().unwrap_or(0) / 1_000_000,
                        process_count: stats["pids"]["current"].as_u64().unwrap_or(0) as u32,
                        disk_bytes_written: stats["blkio"]["io_service_bytes_recursive"]
                            .as_array()
                            .and_then(|arr| arr.iter().find(|entry| entry["op"] == "Write"))
                            .and_then(|entry| entry["value"].as_u64())
                            .unwrap_or(0),
                        disk_bytes_read: stats["blkio"]["io_service_bytes_recursive"]
                            .as_array()
                            .and_then(|arr| arr.iter().find(|entry| entry["op"] == "Read"))
                            .and_then(|entry| entry["value"].as_u64())
                            .unwrap_or(0),
                        network_bytes_sent: stats["network"]["tx_bytes"].as_u64().unwrap_or(0),
                        network_bytes_received: stats["network"]["rx_bytes"].as_u64().unwrap_or(0),
                    })
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

impl ExecutionBackend for AppleBackend {
    fn execute_code(&self, request: ExecutionRequest) -> AsyncTask<ExecutionResult> {
        let image = self.image.clone();
        let backend_name = self.backend_type();

        AsyncTaskBuilder::new(async move {
            // Ensure image is available
            match Self::ensure_image_available(image.clone()).await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    return ExecutionResult::failure(-1, format!("Failed to prepare image: {e}"));
                }
                Err(e) => {
                    return ExecutionResult::failure(
                        -1,
                        format!("Failed to prepare image task: {e}"),
                    );
                }
            }

            // Execute in container
            match Self::execute_in_container(image, request).await {
                Ok(Ok(result)) => result,
                Ok(Err(e)) => {
                    ExecutionResult::failure(-1, format!("{backend_name} execution failed: {e}"))
                }
                Err(e) => ExecutionResult::failure(
                    -1,
                    format!("{backend_name} execution task failed: {e}"),
                ),
            }
        })
        .spawn()
    }

    fn health_check(&self) -> AsyncTask<HealthStatus> {
        let image = self.image.clone();

        AsyncTaskBuilder::new(async move {
            // Check CLI availability
            let cli_available: bool = (Self::check_cli_availability().await).unwrap_or_default();
            if !cli_available {
                return HealthStatus::unhealthy("Apple containerization CLI not available")
                    .with_metric("cli_available", "false");
            }

            // Check platform support
            if !Self::is_platform_supported() {
                return HealthStatus::unhealthy("Platform does not support Apple containerization")
                    .with_metric("platform_supported", "false");
            }

            // Test container execution with simple command
            let test_request = ExecutionRequest::new("echo 'health check'", "bash")
                .with_timeout(Duration::from_secs(10));

            match Self::execute_in_container(image.clone(), test_request).await {
                Ok(Ok(result)) if result.is_success() => {
                    HealthStatus::healthy("Apple containerization backend operational")
                        .with_metric("cli_available", "true")
                        .with_metric("platform_supported", "true")
                        .with_metric("test_execution", "success")
                        .with_metric("image", &image)
                }
                Ok(Ok(result)) => {
                    HealthStatus::unhealthy(format!("Test execution failed: {}", result.stderr))
                        .with_metric("test_execution", "failed")
                        .with_metric("exit_code", result.exit_code.to_string())
                }
                Ok(Err(e)) => HealthStatus::unhealthy(format!("Health check execution error: {e}"))
                    .with_metric("test_execution", "error"),
                Err(e) => HealthStatus::unhealthy(format!("Health check task error: {e}"))
                    .with_metric("test_execution", "task_error"),
            }
        })
        .spawn()
    }

    fn cleanup(&self) -> AsyncTask<crate::execution_env::CyloResult<()>> {
        AsyncTaskBuilder::new(async move {
            // Clean up any dangling containers with our prefix
            let cleanup_result = Command::new("container")
                .args([
                    "ps",
                    "-a",
                    "--filter",
                    "name=cylo-",
                    "--format",
                    "{{.Names}}",
                ])
                .output();

            if let Ok(output) = cleanup_result
                && output.status.success()
            {
                let container_names = String::from_utf8_lossy(&output.stdout);
                for name in container_names.lines() {
                    if !name.trim().is_empty() {
                        let _ = Command::new("container")
                            .args(["rm", "-f", name.trim()])
                            .status();
                    }
                }
            }

            Ok(())
        })
        .spawn()
    }

    fn get_config(&self) -> &BackendConfig {
        &self.config
    }

    fn backend_type(&self) -> &'static str {
        "Apple"
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
    use std::time::Duration;

    use super::*;
    use crate::backends::BackendConfig;

    #[test]
    fn image_format_validation() {
        assert!(AppleBackend::is_valid_image_format("python:3.11"));
        assert!(AppleBackend::is_valid_image_format("rust:alpine3.20"));
        assert!(AppleBackend::is_valid_image_format("node:18-alpine"));
        assert!(AppleBackend::is_valid_image_format(
            "registry.io/user/image:tag"
        ));

        assert!(!AppleBackend::is_valid_image_format("python"));
        assert!(!AppleBackend::is_valid_image_format(""));
        assert!(!AppleBackend::is_valid_image_format(":tag"));
        assert!(!AppleBackend::is_valid_image_format("image:"));
        assert!(!AppleBackend::is_valid_image_format("image:tag:extra"));
    }

    #[test]
    fn execution_command_preparation() {
        let python_cmd =
            AppleBackend::prepare_execution_command("python", "print('hello')")
                .expect("test should successfully prepare python execution command");
        assert_eq!(python_cmd, vec!["python3", "-c", "print('hello')"]);

        let js_cmd =
            AppleBackend::prepare_execution_command("javascript", "console.log('hello')")
                .expect("test should successfully prepare javascript execution command");
        assert_eq!(js_cmd, vec!["node", "-e", "console.log('hello')"]);

        let bash_cmd = AppleBackend::prepare_execution_command("bash", "echo hello")
            .expect("test should successfully prepare bash execution command");
        assert_eq!(bash_cmd, vec!["sh", "-c", "echo hello"]);

        let unsupported = AppleBackend::prepare_execution_command("cobol", "some code");
        assert!(unsupported.is_err());
    }

    #[test]
    fn backend_creation() {
        let config = BackendConfig::new("test_apple").with_timeout(Duration::from_secs(60));

        // Valid image should work
        let _result = AppleBackend::new("python:3.11".to_string(), config.clone());
        // Note: Will fail on non-macOS platforms, which is expected

        // Invalid image should fail
        let invalid_result = AppleBackend::new("invalid".to_string(), config);
        assert!(invalid_result.is_err());
    }

    #[test]
    fn supported_languages() {
        let config = BackendConfig::new("test");
        if let Ok(backend) = AppleBackend::new("python:3.11".to_string(), config) {
            assert!(backend.supports_language("python"));
            assert!(backend.supports_language("javascript"));
            assert!(backend.supports_language("rust"));
            assert!(!backend.supports_language("cobol"));
        }
    }
}
