// ============================================================================
// File: packages/cylo/src/backends/apple/mod.rs
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

mod execution;
mod image;
mod resource_stats;
mod validation;

#[cfg(test)]
mod tests;

use std::process::Command;
use std::time::Duration;

use crate::AsyncTaskBuilder;
use crate::backends::{
    AsyncTask, BackendConfig, BackendError, BackendResult, ExecutionBackend, ExecutionRequest,
    ExecutionResult, HealthStatus,
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
        if !validation::is_platform_supported() {
            return Err(BackendError::NotAvailable {
                backend: "Apple",
                reason: "Apple containerization requires macOS with Apple Silicon".to_string(),
            });
        }

        // Validate image format
        if !validation::is_valid_image_format(&image) {
            return Err(BackendError::InvalidConfig {
                backend: "Apple",
                details: format!("Invalid image format: {image}. Expected format: 'name:tag'"),
            });
        }

        Ok(Self { image, config })
    }
}

impl ExecutionBackend for AppleBackend {
    fn execute_code(&self, request: ExecutionRequest) -> AsyncTask<ExecutionResult> {
        let image = self.image.clone();
        let backend_name = self.backend_type();

        AsyncTaskBuilder::new(async move {
            // Ensure image is available
            match image::ensure_image_available(image.clone()).await {
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
            match execution::execute_in_container(image, request).await {
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
            let cli_available: bool = (image::check_cli_availability().await).unwrap_or_default();
            if !cli_available {
                return HealthStatus::unhealthy("Apple containerization CLI not available")
                    .with_metric("cli_available", "false");
            }

            // Check platform support
            if !validation::is_platform_supported() {
                return HealthStatus::unhealthy("Platform does not support Apple containerization")
                    .with_metric("platform_supported", "false");
            }

            // Test container execution with simple command
            let test_request = ExecutionRequest::new("echo 'health check'", "bash")
                .with_timeout(Duration::from_secs(10));

            match execution::execute_in_container(image.clone(), test_request).await {
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
