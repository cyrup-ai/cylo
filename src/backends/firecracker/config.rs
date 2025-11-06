// ============================================================================
// File: packages/cylo/src/backends/firecracker/config.rs
// ----------------------------------------------------------------------------
// FireCracker configuration structures and initialization.
// ============================================================================

use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};

use crate::backends::{BackendConfig, BackendError, BackendResult};

/// FireCracker-specific configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FireCrackerConfig {
    /// Path to FireCracker binary
    pub firecracker_binary: PathBuf,

    /// Path to kernel image
    pub kernel_path: PathBuf,

    /// Path to root filesystem
    pub rootfs_path: PathBuf,

    /// VM memory size in MB
    pub memory_size_mb: u32,

    /// Number of vCPUs
    pub vcpu_count: u8,

    /// Network configuration
    pub network_enabled: bool,

    /// Metadata configuration
    pub metadata_enabled: bool,
}

impl Default for FireCrackerConfig {
    fn default() -> Self {
        Self {
            firecracker_binary: PathBuf::from("/usr/bin/firecracker"),
            kernel_path: PathBuf::from("/var/lib/firecracker/vmlinux.bin"),
            rootfs_path: PathBuf::from("/var/lib/firecracker/rootfs.ext4"),
            memory_size_mb: 512,
            vcpu_count: 1,
            network_enabled: false,
            metadata_enabled: true,
        }
    }
}

impl FireCrackerConfig {
    /// Initialize FireCracker configuration from backend config
    pub fn from_backend_config(config: &BackendConfig) -> BackendResult<Self> {
        let mut fc_config = FireCrackerConfig::default();

        if let Some(binary_path) = config.backend_specific.get("firecracker_binary") {
            fc_config.firecracker_binary = PathBuf::from(binary_path);
        }

        if let Some(kernel_path) = config.backend_specific.get("kernel_path") {
            fc_config.kernel_path = PathBuf::from(kernel_path);
        }

        if let Some(rootfs_path) = config.backend_specific.get("rootfs_path") {
            fc_config.rootfs_path = PathBuf::from(rootfs_path);
        }

        if let Some(memory_size) = config.backend_specific.get("memory_size_mb") {
            fc_config.memory_size_mb = memory_size.parse().unwrap_or(512);
        }

        if let Some(vcpu_count) = config.backend_specific.get("vcpu_count") {
            fc_config.vcpu_count = vcpu_count.parse().unwrap_or(1);
        }

        if let Some(network_enabled) = config.backend_specific.get("network_enabled") {
            fc_config.network_enabled = network_enabled.parse().unwrap_or(false);
        }

        Ok(fc_config)
    }

    /// Verify FireCracker installation and requirements
    pub fn verify_installation(&self) -> BackendResult<()> {
        if !self.firecracker_binary.exists() {
            return Err(BackendError::NotAvailable {
                backend: "FireCracker",
                reason: format!(
                    "FireCracker binary not found at {}",
                    self.firecracker_binary.display()
                ),
            });
        }

        if !self.kernel_path.exists() {
            return Err(BackendError::NotAvailable {
                backend: "FireCracker",
                reason: format!("Kernel image not found at {}", self.kernel_path.display()),
            });
        }

        if !self.rootfs_path.exists() {
            return Err(BackendError::NotAvailable {
                backend: "FireCracker",
                reason: format!(
                    "Root filesystem not found at {}",
                    self.rootfs_path.display()
                ),
            });
        }

        if !Path::new("/dev/kvm").exists() {
            return Err(BackendError::NotAvailable {
                backend: "FireCracker",
                reason: "KVM device not available (/dev/kvm)".to_string(),
            });
        }

        Ok(())
    }
}
