// ============================================================================
// File: packages/cylo/src/backends/firecracker/backend.rs
// ----------------------------------------------------------------------------
// FireCracker backend implementation of ExecutionBackend trait.
// ============================================================================

use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::async_task::AsyncTaskBuilder;
use crate::backends::{
    AsyncTask, BackendConfig, BackendError, BackendResult, ExecutionBackend, ExecutionRequest,
    ExecutionResult, HealthStatus,
};

use super::config::FireCrackerConfig;
use super::vm_instance::VMInstance;

/// FireCracker backend for secure code execution
#[derive(Debug, Clone)]
pub struct FireCrackerBackend {
    /// Container image specification (e.g., "rust:alpine3.20")
    _image: String,

    /// Backend configuration
    config: BackendConfig,

    /// FireCracker runtime configuration
    firecracker_config: FireCrackerConfig,
}

impl FireCrackerBackend {
    /// Create a new FireCracker backend instance
    pub fn new(image: String, config: BackendConfig) -> BackendResult<Self> {
        if !Self::is_platform_supported() {
            return Err(BackendError::NotAvailable {
                backend: "FireCracker",
                reason: "FireCracker is only available on Linux".to_string(),
            });
        }

        if !Self::is_valid_image_format(&image) {
            return Err(BackendError::InvalidConfig {
                backend: "FireCracker",
                details: format!(
                    "Invalid image format: {}. Expected format: 'name:tag'",
                    image
                ),
            });
        }

        let firecracker_config = FireCrackerConfig::from_backend_config(&config)?;
        firecracker_config.verify_installation()?;

        Ok(Self {
            _image: image,
            config,
            firecracker_config,
        })
    }

    /// Check if platform supports FireCracker
    fn is_platform_supported() -> bool {
        #[cfg(target_os = "linux")]
        {
            Path::new("/dev/kvm").exists() && Path::new("/proc/cpuinfo").exists()
        }

        #[cfg(not(target_os = "linux"))]
        false
    }

    /// Validate container image format
    fn is_valid_image_format(image: &str) -> bool {
        if !image.contains(':') {
            return false;
        }

        let parts: Vec<&str> = image.splitn(2, ':').collect();
        if parts.len() != 2 {
            return false;
        }

        let (name, tag) = (parts[0], parts[1]);

        if name.is_empty()
            || !name
                .chars()
                .all(|c| c.is_alphanumeric() || c == '/' || c == '-' || c == '_' || c == '.')
        {
            return false;
        }

        if tag.is_empty()
            || !tag
                .chars()
                .all(|c| c.is_alphanumeric() || c == '.' || c == '-' || c == '_')
        {
            return false;
        }

        true
    }

    /// Check if FireCracker binary is available
    fn is_firecracker_available() -> bool {
        Command::new("firecracker")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }
}

impl ExecutionBackend for FireCrackerBackend {
    fn execute_code(&self, request: ExecutionRequest) -> AsyncTask<ExecutionResult> {
        let fc_config = self.firecracker_config.clone();
        let backend_config = self.config.clone();
        let backend_name = self.backend_type();

        AsyncTaskBuilder::new(async move {
            let vm = match VMInstance::create(&request, &backend_config) {
                Ok(vm) => vm,
                Err(e) => {
                    return ExecutionResult::failure(
                        -1,
                        format!("Failed to create VM instance: {}", e),
                    );
                }
            };

            if let Err(e) = vm.generate_config(&fc_config, &request) {
                return ExecutionResult::failure(
                    -1,
                    format!("Failed to generate VM config: {}", e),
                );
            }

            let started_vm = match vm.start(fc_config).await {
                Ok(Ok(vm)) => vm,
                Ok(Err(e)) => {
                    return ExecutionResult::failure(-1, format!("Failed to start VM: {}", e));
                }
                Err(e) => {
                    return ExecutionResult::failure(-1, format!("VM start task panicked: {}", e));
                }
            };

            let result = match started_vm.clone().execute(request).await {
                Ok(Ok(result)) => result,
                Ok(Err(e)) => ExecutionResult::failure(
                    -1,
                    format!("{} execution failed: {}", backend_name, e),
                ),
                Err(e) => ExecutionResult::failure(
                    -1,
                    format!("{} execution task panicked: {}", backend_name, e),
                ),
            };

            let _ = started_vm.cleanup().await;

            result
        }).spawn()
    }

    fn health_check(&self) -> AsyncTask<HealthStatus> {
        let fc_config = self.firecracker_config.clone();

        AsyncTaskBuilder::new(async move {
            if !Self::is_platform_supported() {
                return HealthStatus::unhealthy("Platform does not support FireCracker")
                    .with_metric("platform_supported", "false");
            }

            if let Err(e) = fc_config.verify_installation() {
                return HealthStatus::unhealthy(format!("FireCracker installation invalid: {}", e))
                    .with_metric("installation_valid", "false");
            }

            if !Self::is_firecracker_available() {
                return HealthStatus::unhealthy("FireCracker binary not available")
                    .with_metric("firecracker_available", "false");
            }

            HealthStatus::healthy("FireCracker backend operational")
                .with_metric("platform_supported", "true")
                .with_metric("installation_valid", "true")
                .with_metric("firecracker_available", "true")
                .with_metric("memory_size_mb", &fc_config.memory_size_mb.to_string())
                .with_metric("vcpu_count", &fc_config.vcpu_count.to_string())
        }).spawn()
    }

    fn cleanup(&self) -> AsyncTask<crate::execution_env::CyloResult<()>> {
        AsyncTaskBuilder::new(async move {
            let output = Command::new("ps").args(&["aux"]).output();

            if let Ok(output) = output {
                let processes = String::from_utf8_lossy(&output.stdout);
                for line in processes.lines() {
                    if line.contains("firecracker") && line.contains("cylo-") {
                        let fields: Vec<&str> = line.split_whitespace().collect();
                        if fields.len() > 1 {
                            if let Ok(pid) = fields[1].parse::<u32>() {
                                let _ = Command::new("kill")
                                    .args(&["-TERM", &pid.to_string()])
                                    .status();
                            }
                        }
                    }
                }
            }

            if let Ok(entries) = fs::read_dir(std::env::temp_dir()) {
                for entry in entries.filter_map(Result::ok) {
                    if let Ok(file_name) = entry.file_name().into_string() {
                        if file_name.starts_with("cylo-") {
                            let _ = fs::remove_file(entry.path());
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
        "FireCracker"
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
    fn image_format_validation() {
        assert!(FireCrackerBackend::is_valid_image_format("python:3.11"));
        assert!(FireCrackerBackend::is_valid_image_format("rust:alpine3.20"));
        assert!(FireCrackerBackend::is_valid_image_format("node:18-alpine"));

        assert!(!FireCrackerBackend::is_valid_image_format("python"));
        assert!(!FireCrackerBackend::is_valid_image_format(""));
        assert!(!FireCrackerBackend::is_valid_image_format(":tag"));
    }

    #[test]
    fn backend_creation() {
        let config = BackendConfig::new("test_firecracker");

        let result = FireCrackerBackend::new("python:3.11".to_string(), config.clone());

        let invalid_result = FireCrackerBackend::new("invalid".to_string(), config);
        assert!(invalid_result.is_err());
    }

    #[test]
    fn supported_languages() {
        let config = BackendConfig::new("test");
        if let Ok(backend) = FireCrackerBackend::new("python:3.11".to_string(), config) {
            assert!(backend.supports_language("python"));
            assert!(backend.supports_language("rust"));
            assert!(backend.supports_language("javascript"));
            assert!(!backend.supports_language("cobol"));
        }
    }
}
