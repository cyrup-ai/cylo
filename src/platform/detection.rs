// ============================================================================
// File: packages/cylo/src/platform/detection.rs
// ----------------------------------------------------------------------------
// Main platform detection logic for Cylo.
//
// Orchestrates detection of platform information including:
// - Operating system and architecture
// - Available backends and their capabilities
// - Platform capabilities (virtualization, containers, security)
// - Performance characteristics
// ============================================================================

use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::SystemTime;

use super::capabilities::*;
use super::performance::*;
use super::types::*;

/// Global platform information cache
static PLATFORM_INFO: OnceLock<PlatformInfo> = OnceLock::new();

impl PlatformInfo {
    /// Get or detect platform information
    ///
    /// Uses cached detection results for performance.
    ///
    /// # Returns
    /// Platform information
    pub fn get() -> &'static PlatformInfo {
        PLATFORM_INFO.get_or_init(Self::detect)
    }

    /// Force re-detection of platform information
    ///
    /// # Returns
    /// Newly detected platform information
    pub fn detect() -> PlatformInfo {
        let os = Self::detect_operating_system();
        let arch = Self::detect_architecture();
        let capabilities = Self::detect_capabilities(&os);
        let available_backends = Self::detect_available_backends(&os, &arch, &capabilities);

        PlatformInfo {
            os,
            arch,
            capabilities,
            available_backends,
            performance: detect_performance_hints(),
            detected_at: SystemTime::now(),
        }
    }

    /// Detect operating system
    fn detect_operating_system() -> OperatingSystem {
        #[cfg(target_os = "linux")]
        {
            let distribution = Some(Self::detect_linux_distribution());
            let kernel_version = Some(Self::detect_kernel_version());
            OperatingSystem::Linux {
                distribution,
                kernel_version,
            }
        }

        #[cfg(target_os = "macos")]
        {
            let version = Self::detect_macos_version();
            OperatingSystem::MacOS { version }
        }

        #[cfg(target_os = "windows")]
        {
            let version = Self::detect_windows_version();
            OperatingSystem::Windows { version }
        }

        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        {
            OperatingSystem::Unknown {
                name: std::env::consts::OS.to_string(),
            }
        }
    }

    /// Detect CPU architecture
    fn detect_architecture() -> Architecture {
        match std::env::consts::ARCH {
            "aarch64" => Architecture::Arm64,
            "x86_64" => Architecture::X86_64,
            "arm" => Architecture::Arm,
            "x86" => Architecture::X86,
            other => Architecture::Unknown(other.to_string()),
        }
    }

    /// Detect platform capabilities
    fn detect_capabilities(os: &OperatingSystem) -> PlatformCapabilities {
        PlatformCapabilities {
            virtualization: detect_virtualization_support(os),
            containers: detect_container_support(os),
            security: detect_security_features(os),
            network: detect_network_capabilities(),
            filesystem: detect_filesystem_features(),
        }
    }

    /// Detect available backends
    fn detect_available_backends(
        os: &OperatingSystem,
        arch: &Architecture,
        capabilities: &PlatformCapabilities,
    ) -> Vec<BackendAvailability> {
        let mut backends = Vec::new();

        // Apple backend
        if matches!(os, OperatingSystem::MacOS { .. }) && *arch == Architecture::Arm64 {
            backends.push(BackendAvailability {
                name: "Apple".to_string(),
                available: true,
                reason: "Running on macOS with Apple Silicon".to_string(),
                capabilities: HashMap::new(),
                performance_rating: 95,
            });
        }

        // LandLock backend
        if capabilities.security.landlock {
            backends.push(BackendAvailability {
                name: "LandLock".to_string(),
                available: true,
                reason: "LandLock is supported by the kernel".to_string(),
                capabilities: HashMap::new(),
                performance_rating: 85,
            });
        }

        // FireCracker backend
        if capabilities.virtualization.kvm_available {
            backends.push(BackendAvailability {
                name: "FireCracker".to_string(),
                available: true,
                reason: "KVM is available for hardware virtualization".to_string(),
                capabilities: HashMap::new(),
                performance_rating: 90,
            });
        }

        backends
    }

    // --- OS-specific version detection ---

    #[cfg(target_os = "macos")]
    fn detect_macos_version() -> Option<String> {
        use std::process::Command;

        Command::new("sw_vers")
            .arg("-productVersion")
            .output()
            .ok()
            .and_then(|output| {
                if output.status.success() {
                    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
                } else {
                    None
                }
            })
    }

    #[cfg(not(target_os = "macos"))]
    #[allow(dead_code)]
    fn detect_macos_version() -> Option<String> {
        None
    }

    #[cfg(target_os = "windows")]
    #[allow(dead_code)]
    fn detect_windows_version() -> Option<String> {
        // Windows version detection would go here
        None
    }

    #[cfg(not(target_os = "windows"))]
    #[allow(dead_code)]
    fn detect_windows_version() -> Option<String> {
        None
    }

    #[cfg(target_os = "linux")]
    fn detect_linux_distribution() -> String {
        std::fs::read_to_string("/etc/os-release")
            .ok()
            .and_then(|content| {
                content
                    .lines()
                    .find(|line| line.starts_with("ID="))
                    .map(|line| line.trim_start_matches("ID=").trim_matches('"').to_string())
            })
            .unwrap_or_else(|| "unknown".to_string())
    }

    #[cfg(not(target_os = "linux"))]
    #[allow(dead_code)]
    fn detect_linux_distribution() -> String {
        "unknown".to_string()
    }

    #[cfg(target_os = "linux")]
    fn detect_kernel_version() -> String {
        std::process::Command::new("uname")
            .arg("-r")
            .output()
            .ok()
            .and_then(|output| {
                if output.status.success() {
                    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| "unknown".to_string())
    }

    #[cfg(not(target_os = "linux"))]
    #[allow(dead_code)]
    fn detect_kernel_version() -> String {
        "unknown".to_string()
    }
}
