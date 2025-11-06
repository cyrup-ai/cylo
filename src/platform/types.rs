// ============================================================================
// File: packages/cylo/src/platform/types.rs
// ----------------------------------------------------------------------------
// Platform type definitions for Cylo execution environments.
//
// Defines all data structures for platform information including:
// - Operating system and architecture enumerations
// - Platform capabilities and features
// - Performance characteristics
// - Backend availability information
// ============================================================================

use std::collections::HashMap;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

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
