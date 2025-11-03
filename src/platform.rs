// ============================================================================
// File: packages/cylo/src/platform.rs
// ----------------------------------------------------------------------------
// Platform detection utilities for Cylo execution environments.
//
// Provides comprehensive platform and capability detection for:
// - Operating system and architecture detection
// - Backend availability and feature support
// - Runtime capability verification
// - Performance optimization hints
// ============================================================================

use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::error::StorageError;

/// Global platform information cache
static PLATFORM_INFO: OnceLock<PlatformInfo> = OnceLock::new();

/// Comprehensive platform information
///
/// Contains detected platform capabilities, available backends,
/// and performance characteristics for optimization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformInfo {
    /// Operating system name
    pub os: OperatingSystem,

    /// CPU architecture
    pub arch: Architecture,

    /// Available execution backends
    pub available_backends: Vec<BackendAvailability>,

    /// Platform capabilities
    pub capabilities: PlatformCapabilities,

    /// Performance characteristics
    pub performance: PerformanceHints,

    /// Detection timestamp
    pub detected_at: SystemTime,
}

/// Operating system enumeration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperatingSystem {
    /// Linux distribution
    Linux {
        /// Distribution name (e.g., "Ubuntu", "Alpine")
        distribution: Option<String>,
        /// Kernel version
        kernel_version: Option<String>,
    },
    /// macOS
    MacOS {
        /// macOS version (e.g., "14.0")
        version: Option<String>,
    },
    /// Windows
    Windows {
        /// Windows version
        version: Option<String>,
    },
    /// Unknown/other OS
    Unknown {
        /// OS name if detectable
        name: String,
    },
}

/// CPU architecture enumeration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Architecture {
    /// ARM64/AArch64 (Apple Silicon, etc.)
    Arm64,
    /// x86_64/AMD64
    X86_64,
    /// ARM32
    Arm,
    /// x86 32-bit
    X86,
    /// Unknown architecture
    Unknown(String),
}

/// Backend availability information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendAvailability {
    /// Backend name
    pub name: String,

    /// Whether backend is available
    pub available: bool,

    /// Availability reason (why available/unavailable)
    pub reason: String,

    /// Backend-specific capabilities
    pub capabilities: HashMap<String, String>,

    /// Performance rating (0-100)
    pub performance_rating: u8,
}

/// Platform capabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformCapabilities {
    /// Virtualization support
    pub virtualization: VirtualizationSupport,

    /// Container runtime support
    pub containers: ContainerSupport,

    /// Security features
    pub security: SecurityFeatures,

    /// Network capabilities
    pub network: NetworkCapabilities,

    /// File system features
    pub filesystem: FilesystemFeatures,
}

/// Virtualization support details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VirtualizationSupport {
    /// Hardware virtualization available
    pub hardware_virtualization: bool,

    /// KVM available (Linux)
    pub kvm_available: bool,

    /// Hyper-V available (Windows)
    pub hyperv_available: bool,

    /// Hypervisor.framework available (macOS)
    pub hypervisor_framework: bool,

    /// Nested virtualization support
    pub nested_virtualization: bool,
}

/// Container runtime support
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerSupport {
    /// Docker available
    pub docker_available: bool,

    /// Podman available
    pub podman_available: bool,

    /// Apple containerization available
    pub apple_containers: bool,

    /// Native language runtimes available
    pub native_runtimes: Vec<String>,
}

/// Security features available
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityFeatures {
    /// LandLock sandboxing (Linux)
    pub landlock: bool,

    /// SELinux support (Linux)
    pub selinux: bool,

    /// AppArmor support (Linux)
    pub apparmor: bool,

    /// App Sandbox (macOS)
    pub app_sandbox: bool,

    /// Secure Enclave (macOS)
    pub secure_enclave: bool,
}

/// Network capabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkCapabilities {
    /// Raw socket access
    pub raw_sockets: bool,

    /// IPv6 support
    pub ipv6_support: bool,

    /// Firewall status
    pub firewall_enabled: bool,

    /// DNS resolution performance (ms)
    pub dns_resolution_ms: u32,
}

/// Filesystem features
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilesystemFeatures {
    /// Filesystem type (e.g., "ext4", "apfs")
    pub filesystem_type: String,

    /// Case sensitive filesystem
    pub case_sensitive: bool,

    /// Journaling enabled
    pub journaling_enabled: bool,

    /// Copy-on-write support
    pub copy_on_write: bool,

    /// Encryption support (e.g., FileVault)
    pub encryption_enabled: bool,
}

/// Performance optimization hints
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceHints {
    /// Number of logical CPU cores
    pub cpu_cores: u32,

    /// Available memory in bytes
    pub available_memory: u64,

    /// Recommended backend for this platform
    pub recommended_backend: Option<String>,

    /// Temporary directory performance
    pub tmpdir_performance: TmpDirPerformance,

    /// I/O characteristics
    pub io_characteristics: IOCharacteristics,
}

/// Temporary directory performance characteristics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TmpDirPerformance {
    /// Path to temporary directory
    pub path: String,

    /// Whether temporary directory is in-memory (e.g., tmpfs)
    pub in_memory: bool,

    /// Estimated throughput in MB/s
    pub estimated_throughput: u32,
}

/// I/O performance characteristics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IOCharacteristics {
    /// Disk type (e.g., "SSD", "HDD")
    pub disk_type: String,

    /// Sequential read performance (MB/s)
    pub sequential_read_mbps: u32,

    /// Sequential write performance (MB/s)
    pub sequential_write_mbps: u32,

    /// Random I/O operations per second
    pub random_iops: u32,
}

/// Platform-specific ramdisk operations trait
///
/// Defines the interface for platform-specific ramdisk management.
/// Implementations provide platform-optimized methods for creating,
/// mounting, and managing temporary in-memory filesystems.
pub trait RamdiskPlatform {
    /// Create a new platform-specific ramdisk implementation
    fn new() -> Self;

    /// Check if a ramdisk is mounted at the given path
    ///
    /// # Arguments
    /// * `mount_point` - Path to check for ramdisk mount
    ///
    /// # Returns
    /// True if a ramdisk is mounted at the path, false otherwise
    fn is_mounted(&self, mount_point: &Path) -> Result<bool, StorageError>;

    /// Create a ramdisk with the specified configuration
    ///
    /// # Arguments
    /// * `config` - Ramdisk configuration
    ///
    /// # Returns
    /// Success or storage error
    fn create(&mut self, config: &crate::config::RamdiskConfig) -> Result<(), StorageError>;

    /// Remove a ramdisk at the specified mount point
    ///
    /// # Arguments
    /// * `mount_point` - Path to the ramdisk mount point
    ///
    /// # Returns
    /// Success or storage error
    fn remove(&self, mount_point: &Path) -> Result<(), StorageError>;
}

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
            performance: Self::detect_performance_hints(),
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
            virtualization: Self::detect_virtualization_support(os),
            containers: Self::detect_container_support(os),
            security: Self::detect_security_features(os),
            network: Self::detect_network_capabilities(),
            filesystem: Self::detect_filesystem_features(),
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

    /// Detect performance hints
    fn detect_performance_hints() -> PerformanceHints {
        PerformanceHints {
            cpu_cores: Self::detect_cpu_cores(),
            available_memory: Self::detect_available_memory(),
            recommended_backend: None, // Logic to determine this would be complex
            tmpdir_performance: Self::detect_tmpdir_performance(),
            io_characteristics: Self::detect_io_characteristics(),
        }
    }

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

    fn detect_virtualization_support(_os: &OperatingSystem) -> VirtualizationSupport {
        VirtualizationSupport {
            hardware_virtualization: Self::has_hardware_virtualization(),
            kvm_available: Self::has_kvm_support(),
            hyperv_available: Self::has_hyperv_support(),
            hypervisor_framework: Self::has_hypervisor_framework(),
            nested_virtualization: false, // Complex to detect
        }
    }

    fn detect_container_support(os: &OperatingSystem) -> ContainerSupport {
        ContainerSupport {
            docker_available: Self::is_command_available("docker"),
            podman_available: Self::is_command_available("podman"),
            apple_containers: Self::is_command_available("container")
                && matches!(os, OperatingSystem::MacOS { .. }),
            native_runtimes: Self::detect_native_runtimes(),
        }
    }

    fn detect_security_features(os: &OperatingSystem) -> SecurityFeatures {
        SecurityFeatures {
            landlock: Self::has_landlock_support(),
            selinux: Self::has_selinux_support(),
            apparmor: Self::has_apparmor_support(),
            app_sandbox: matches!(os, OperatingSystem::MacOS { .. }),
            secure_enclave: matches!(os, OperatingSystem::MacOS { .. })
                && Self::has_secure_enclave(),
        }
    }

    /// Measure actual DNS resolution time by testing against reliable domains
    ///
    /// Tests DNS lookup against multiple domains with timeout protection.
    /// Returns average time in milliseconds, or conservative fallback if all fail.
    #[allow(dead_code)]
    async fn measure_dns_resolution() -> u32 {
        use std::time::{Duration, Instant};
        use tokio::net::lookup_host;
        use tokio::time::timeout;

        // Test domains with port specifications for lookup_host
        let test_domains = ["google.com:80", "cloudflare.com:80", "1.1.1.1:80"];

        let mut successful_timings: Vec<u32> = Vec::new();

        // Test each domain with 2-second timeout
        for domain in test_domains.iter() {
            let start = Instant::now();

            // Attempt DNS lookup with timeout protection
            let lookup_result = timeout(Duration::from_secs(2), lookup_host(domain)).await;

            match lookup_result {
                Ok(Ok(mut addrs)) => {
                    // Force DNS resolution by consuming iterator
                    if addrs.next().is_some() {
                        let elapsed = start.elapsed().as_millis() as u32;
                        successful_timings.push(elapsed);
                    }
                }
                Ok(Err(_)) => {
                    // DNS lookup failed (network error, invalid domain)
                    continue;
                }
                Err(_) => {
                    // Timeout occurred
                    continue;
                }
            }
        }

        // Calculate average of successful lookups
        if successful_timings.is_empty() {
            // Conservative fallback if all lookups failed
            100
        } else {
            let sum: u32 = successful_timings.iter().sum();
            sum / successful_timings.len() as u32
        }
    }

    fn detect_network_capabilities() -> NetworkCapabilities {
        // Use conservative default for DNS resolution time.
        // The measure_dns_resolution() async function remains available
        // for future use if accurate measurement becomes necessary.
        // Default of 100ms represents typical DNS resolution on modern networks.

        NetworkCapabilities {
            raw_sockets: true,       // Assume available
            ipv6_support: true,      // Assume available
            firewall_enabled: false, // Assume disabled for simplicity
            dns_resolution_ms: 100,  // Conservative default
        }
    }

    fn detect_filesystem_features() -> FilesystemFeatures {
        // Simplified detection
        FilesystemFeatures {
            filesystem_type: "unknown".to_string(),
            case_sensitive: cfg!(not(target_os = "windows")),
            journaling_enabled: true,
            copy_on_write: false,
            encryption_enabled: false,
        }
    }

    // --- Private helper functions for capability detection ---

    fn has_hardware_virtualization() -> bool {
        #[cfg(target_os = "linux")]
        {
            if let Ok(cpuinfo) = std::fs::read_to_string("/proc/cpuinfo") {
                return cpuinfo.contains("vmx") || cpuinfo.contains("svm");
            }
        }
        false
    }

    fn has_kvm_support() -> bool {
        std::path::Path::new("/dev/kvm").exists()
    }

    fn has_hyperv_support() -> bool {
        // Windows-specific detection would go here
        false
    }

    fn has_hypervisor_framework() -> bool {
        // macOS-specific detection would go here
        cfg!(target_os = "macos")
    }

    fn has_landlock_support() -> bool {
        // Linux-specific detection using syscalls
        false
    }

    fn has_selinux_support() -> bool {
        std::path::Path::new("/sys/fs/selinux").exists()
    }

    fn has_apparmor_support() -> bool {
        std::path::Path::new("/sys/kernel/security/apparmor").exists()
    }

    fn has_secure_enclave() -> bool {
        // macOS-specific detection
        false
    }

    fn is_command_available(command: &str) -> bool {
        std::process::Command::new(command)
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok()
    }

    fn detect_native_runtimes() -> Vec<String> {
        let mut runtimes = Vec::new();
        if Self::is_command_available("python3") {
            runtimes.push("python".to_string());
        }
        if Self::is_command_available("node") {
            runtimes.push("javascript".to_string());
        }
        runtimes
    }

    fn detect_cpu_cores() -> u32 {
        num_cpus::get() as u32
    }

    fn detect_available_memory() -> u64 {
        #[cfg(target_os = "linux")]
        {
            use std::fs;

            if let Ok(content) = fs::read_to_string("/proc/meminfo") {
                for line in content.lines() {
                    if line.starts_with("MemAvailable:") {
                        if let Some(kb_str) = line.split_whitespace().nth(1) {
                            if let Ok(kb) = kb_str.parse::<u64>() {
                                return kb * 1024; // Convert to bytes
                            }
                        }
                    }
                }
            }
        }

        // Fallback: return a reasonable default
        4 * 1024 * 1024 * 1024 // 4GB
    }

    fn detect_tmpdir_performance() -> TmpDirPerformance {
        let tmp_path = std::env::temp_dir();
        let path = tmp_path.display().to_string();

        // Check if it's likely in-memory
        let in_memory = path.contains("/tmp");

        let estimated_throughput = if in_memory {
            5000 // 5GB/s for RAM
        } else {
            500 // 500MB/s for SSD
        };

        TmpDirPerformance {
            path,
            in_memory,
            estimated_throughput,
        }
    }

    fn detect_io_characteristics() -> IOCharacteristics {
        // This is a simplified implementation
        // Real implementation would benchmark I/O performance
        IOCharacteristics {
            disk_type: "SSD".to_string(),
            sequential_read_mbps: 500,
            sequential_write_mbps: 400,
            random_iops: 50000,
        }
    }
}

/// Public API functions
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
