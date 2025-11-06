// ============================================================================
// File: packages/cylo/src/backends/factory.rs
// ----------------------------------------------------------------------------
// Backend factory functions
// ============================================================================

use crate::backends::config::BackendConfig;
use crate::backends::trait_def::ExecutionBackend;
use crate::execution_env::{CyloError, CyloResult};

#[cfg(target_os = "macos")]
use crate::backends::AppleBackend;
#[cfg(target_os = "linux")]
use crate::backends::{FireCrackerBackend, LandLockBackend};
use crate::backends::SweetMcpPluginBackend;

/// Create a backend instance from configuration
///
/// Factory function that creates the appropriate backend based on
/// the execution environment specification.
///
/// # Arguments
/// * `env` - Execution environment specification
/// * `config` - Backend configuration
///
/// # Returns
/// Boxed backend instance or error if backend is not available
pub fn create_backend(
    env: &crate::execution_env::Cylo,
    config: BackendConfig,
) -> CyloResult<Box<dyn ExecutionBackend>> {
    match env {
        #[cfg(target_os = "macos")]
        crate::execution_env::Cylo::Apple(image) => {
            let backend = AppleBackend::new(image.clone(), config)?;
            Ok(Box::new(backend))
        }

        #[cfg(target_os = "linux")]
        crate::execution_env::Cylo::LandLock(path) => {
            let backend = LandLockBackend::new(path.clone(), config)?;
            Ok(Box::new(backend))
        }

        #[cfg(target_os = "linux")]
        crate::execution_env::Cylo::FireCracker(image) => {
            let backend = FireCrackerBackend::new(image.clone(), config)?;
            Ok(Box::new(backend))
        }

        // Platform-specific error handling
        #[cfg(not(target_os = "macos"))]
        crate::execution_env::Cylo::Apple(_) => Err(CyloError::platform_unsupported(
            "Apple",
            "Apple containerization is only available on macOS",
        )),

        #[cfg(not(target_os = "linux"))]
        crate::execution_env::Cylo::LandLock(_) => Err(CyloError::platform_unsupported(
            "LandLock",
            "LandLock is only available on Linux",
        )),

        #[cfg(not(target_os = "linux"))]
        crate::execution_env::Cylo::FireCracker(_) => Err(CyloError::platform_unsupported(
            "FireCracker",
            "FireCracker is only available on Linux",
        )),

        crate::execution_env::Cylo::SweetMcpPlugin(plugin_path) => {
            let backend = SweetMcpPluginBackend::new(plugin_path.clone().into(), config)?;
            Ok(Box::new(backend))
        }
    }
}

/// Get all available backends for the current platform
///
/// # Returns
/// List of backend types available on this platform
pub fn available_backends() -> Vec<&'static str> {
    let mut backends = vec!["SweetMcpPlugin"];

    #[cfg(target_os = "macos")]
    backends.push("Apple");

    #[cfg(target_os = "linux")]
    {
        backends.push("LandLock");
        backends.push("FireCracker");
    }

    backends
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn available_backends_list() {
        let backends = available_backends();
        assert!(!backends.is_empty());

        #[cfg(target_os = "macos")]
        assert!(backends.contains(&"Apple"));

        #[cfg(target_os = "linux")]
        {
            assert!(backends.contains(&"LandLock"));
            assert!(backends.contains(&"FireCracker"));
        }
    }
}
