//! Firecracker-based secure execution environment

use std::path::PathBuf;
use std::process::Command;

use anyhow::Result;
use log::{error, info};

use crate::config::RamdiskConfig;
use crate::error::StorageError;

// Internal modules
mod api_client;
mod api_types;
mod config;
mod ssh_execution;
mod vm_lifecycle;

// Public re-exports
pub use api_types::{
    BootSource, Drive, FirecrackerError, InstanceActionInfo, MachineConfiguration,
    NetworkInterface,
};
pub use config::{FirecrackerConfig, NetworkConfig, SshAuth, SshConfig};
pub use vm_lifecycle::FirecrackerVM;

/// Create a Firecracker-based execution environment
pub async fn create_firecracker_environment(
    config: &RamdiskConfig,
) -> Result<FirecrackerVM, StorageError> {
    // Convert RamdiskConfig to FirecrackerConfig
    let fc_config = FirecrackerConfig {
        binary_path: PathBuf::from("/usr/bin/firecracker"),
        kernel_path: PathBuf::from("/var/lib/firecracker/vmlinux"),
        rootfs_path: PathBuf::from("/var/lib/firecracker/rootfs.ext4"),
        mem_size_mib: (config.size_gb * 1024) as u32,
        vcpu_count: 1,
        network_config: None,
        ssh_config: None,
    };

    let vm_id = format!("cylo-{}", uuid::Uuid::new_v4());
    let mut vm = FirecrackerVM::new(fc_config, vm_id);

    match vm.start().await {
        Ok(_) => {
            info!("Firecracker VM started successfully");
            Ok(vm)
        }
        Err(e) => {
            error!("Failed to start Firecracker VM: {}", e);
            Err(StorageError::Other(e))
        }
    }
}

/// Check if Firecracker is available on the system
pub fn is_firecracker_available() -> bool {
    Command::new("which")
        .arg("firecracker")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}
