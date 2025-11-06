// ============================================================================
// File: packages/cylo/src/platform/capabilities.rs
// ----------------------------------------------------------------------------
// Platform capability detection for Cylo.
//
// Provides functions to detect various platform capabilities including:
// - Virtualization support (KVM, Hyper-V, Hypervisor.framework)
// - Container runtime availability
// - Security features (LandLock, SELinux, AppArmor, etc.)
// - Network capabilities
// - Filesystem features
// ============================================================================

use super::types::*;

/// Detect virtualization support for the given OS
pub(crate) fn detect_virtualization_support(_os: &OperatingSystem) -> VirtualizationSupport {
    VirtualizationSupport {
        hardware_virtualization: has_hardware_virtualization(),
        kvm_available: has_kvm_support(),
        hyperv_available: has_hyperv_support(),
        hypervisor_framework: has_hypervisor_framework(),
        nested_virtualization: false, // Complex to detect
    }
}

/// Detect container runtime support for the given OS
pub(crate) fn detect_container_support(os: &OperatingSystem) -> ContainerSupport {
    ContainerSupport {
        docker_available: is_command_available("docker"),
        podman_available: is_command_available("podman"),
        apple_containers: is_command_available("container")
            && matches!(os, OperatingSystem::MacOS { .. }),
        native_runtimes: detect_native_runtimes(),
    }
}

/// Detect security features for the given OS
pub(crate) fn detect_security_features(os: &OperatingSystem) -> SecurityFeatures {
    SecurityFeatures {
        landlock: has_landlock_support(),
        selinux: has_selinux_support(),
        apparmor: has_apparmor_support(),
        app_sandbox: matches!(os, OperatingSystem::MacOS { .. }),
        secure_enclave: matches!(os, OperatingSystem::MacOS { .. }) && has_secure_enclave(),
    }
}

/// Detect network capabilities
pub(crate) fn detect_network_capabilities() -> NetworkCapabilities {
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

/// Detect filesystem features
pub(crate) fn detect_filesystem_features() -> FilesystemFeatures {
    // Simplified detection
    FilesystemFeatures {
        filesystem_type: "unknown".to_string(),
        case_sensitive: cfg!(not(target_os = "windows")),
        journaling_enabled: true,
        copy_on_write: false,
        encryption_enabled: false,
    }
}

/// Measure actual DNS resolution time by testing against reliable domains
///
/// Tests DNS lookup against multiple domains with timeout protection.
/// Returns average time in milliseconds, or conservative fallback if all fail.
#[allow(dead_code)]
pub(crate) async fn measure_dns_resolution() -> u32 {
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
    if is_command_available("python3") {
        runtimes.push("python".to_string());
    }
    if is_command_available("node") {
        runtimes.push("javascript".to_string());
    }
    runtimes
}
