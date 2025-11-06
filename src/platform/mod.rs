// ============================================================================
// File: packages/cylo/src/platform/mod.rs
// ----------------------------------------------------------------------------
// Platform detection module for Cylo execution environments.
//
// Provides comprehensive platform and capability detection for:
// - Operating system and architecture detection
// - Backend availability and feature support
// - Runtime capability verification
// - Performance optimization hints
// ============================================================================

// Module declarations
mod api;
mod capabilities;
mod detection;
mod performance;
mod ramdisk;
mod types;

// Re-export public API
pub use api::*;
pub use ramdisk::RamdiskPlatform;
pub use types::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_detection() {
        let info = detect_platform();

        // Basic sanity checks
        assert!(info.performance.cpu_cores > 0);
        assert!(info.performance.available_memory > 0);
        assert!(!info.performance.tmpdir_performance.path.is_empty());

        // Should have at least some architecture detection
        assert!(!matches!(info.arch, Architecture::Unknown(_)));

        // Should detect current OS correctly
        #[cfg(target_os = "linux")]
        assert!(matches!(info.os, OperatingSystem::Linux { .. }));

        #[cfg(target_os = "macos")]
        assert!(matches!(info.os, OperatingSystem::MacOS { .. }));
    }

    #[test]
    fn backend_availability() {
        let backends = get_available_backends();

        // Should have at least one backend available or give reasonable reasons
        if backends.is_empty() {
            let info = detect_platform();
            for backend in &info.available_backends {
                assert!(!backend.reason.is_empty());
            }
        }
    }

    #[test]
    fn utility_functions() {
        // These should not panic
        let _ = is_apple_silicon();
        let _ = is_linux();
        let _ = has_landlock();
        let _ = has_kvm();
        let _ = get_recommended_backend();
    }

    #[test]
    fn platform_specific_detection() {
        let info = detect_platform();

        #[cfg(target_os = "macos")]
        {
            if is_apple_silicon() {
                assert!(info.available_backends.iter().any(|b| b.name == "Apple"));
            }
        }

        #[cfg(target_os = "linux")]
        {
            assert!(info.available_backends.iter().any(|b| b.name == "LandLock"));
            assert!(
                info.available_backends
                    .iter()
                    .any(|b| b.name == "FireCracker")
            );
        }
    }
}
