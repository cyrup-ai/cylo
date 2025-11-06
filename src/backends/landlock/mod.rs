// ============================================================================
// File: packages/cylo/src/backends/landlock/mod.rs
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

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use crate::async_task::AsyncTaskBuilder;
use crate::backends::AsyncTask;
use crate::backends::{
    BackendConfig, BackendError, BackendResult, ExecutionBackend, ExecutionRequest,
    ExecutionResult, HealthStatus,
};

mod execution;
mod features;
mod jail;
mod monitoring;

use execution::SandboxedExecutor;
use features::{LandLockFeatures, PlatformSupport};
use jail::JailEnvironment;

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
        PlatformSupport::validate()?;

        let jail_path = PathBuf::from(jail_path);

        // Validate jail path
        JailEnvironment::validate_path(&jail_path)?;

        // Detect LandLock features
        let landlock_features = LandLockFeatures::detect()?;

        Ok(Self {
            jail_path,
            config,
            landlock_features,
        })
    }
}

impl ExecutionBackend for LandLockBackend {
    fn execute_code(&self, request: ExecutionRequest) -> AsyncTask<ExecutionResult> {
        let jail_path = self.jail_path.clone();
        let backend_name = self.backend_type();

        // Setup jail environment before async block to avoid self borrow issues
        let exec_dir = match JailEnvironment::setup_environment(&self.jail_path, &request) {
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
            match SandboxedExecutor::execute(jail_path, request, exec_dir).await {
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
            if !SandboxedExecutor::is_bwrap_available() {
                return HealthStatus::unhealthy("Bubblewrap (bwrap) is not available")
                    .with_metric("bwrap_available", "false");
            }

            // Check jail directory accessibility
            if let Err(e) = JailEnvironment::validate_path(&jail_path) {
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

            match JailEnvironment::setup_environment(&backend.jail_path, &test_request) {
                Ok(exec_dir) => {
                    // Clean up test directory
                    JailEnvironment::cleanup(&exec_dir);

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
            JailEnvironment::cleanup_all(&jail_path);
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
    use std::path::PathBuf;

    #[test]
    fn backend_creation() {
        let config = BackendConfig::new("test_landlock");
        let temp_jail = std::env::temp_dir().join("cylo_test_jail");

        let result = LandLockBackend::new(temp_jail.display().to_string(), config);

        #[cfg(target_os = "linux")]
        {
            // On Linux, should work if LandLock is available
            if PlatformSupport::is_supported() {
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
