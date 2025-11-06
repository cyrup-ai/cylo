// ============================================================================
// File: packages/cylo/src/platform/api.rs
// ----------------------------------------------------------------------------
// Public API functions for platform detection.
//
// Provides convenient functions for common platform queries:
// - Platform information access
// - OS and architecture checks
// - Backend availability queries
// - Capability checks
// ============================================================================

use super::types::*;

/// Get current platform information
pub fn detect_platform() -> &'static PlatformInfo {
    PlatformInfo::get()
}

/// Check if running on Apple Silicon
pub fn is_apple_silicon() -> bool {
    let info = detect_platform();
    matches!(info.os, OperatingSystem::MacOS { .. }) && matches!(info.arch, Architecture::Arm64)
}

/// Check if running on Linux
pub fn is_linux() -> bool {
    matches!(detect_platform().os, OperatingSystem::Linux { .. })
}

/// Check if LandLock is available
pub fn has_landlock() -> bool {
    detect_platform().capabilities.security.landlock
}

/// Check if KVM is available
pub fn has_kvm() -> bool {
    detect_platform().capabilities.virtualization.kvm_available
}

/// Get recommended backend for current platform
pub fn get_recommended_backend() -> Option<String> {
    detect_platform().performance.recommended_backend.clone()
}

/// Get available backends for current platform
pub fn get_available_backends() -> Vec<String> {
    detect_platform()
        .available_backends
        .iter()
        .filter(|b| b.available)
        .map(|b| b.name.clone())
        .collect()
}
