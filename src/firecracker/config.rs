//! Firecracker VM configuration types

use std::path::PathBuf;

/// Configuration for Firecracker VM
#[derive(Debug, Clone)]
pub struct FirecrackerConfig {
    /// Path to the Firecracker binary
    pub binary_path: PathBuf,
    /// Path to the kernel image
    pub kernel_path: PathBuf,
    /// Path to the root filesystem image
    pub rootfs_path: PathBuf,
    /// Memory size in MB
    pub mem_size_mib: u32,
    /// Number of vCPUs
    pub vcpu_count: u32,
    /// Network configuration (optional)
    pub network_config: Option<NetworkConfig>,
    /// SSH connection details for VM communication
    pub ssh_config: Option<SshConfig>,
}

/// SSH configuration for VM communication
#[derive(Debug, Clone)]
pub struct SshConfig {
    /// SSH host (typically 127.0.0.1 or VM IP)
    pub host: String,
    /// SSH port
    pub port: u16,
    /// SSH username
    pub username: String,
    /// SSH authentication method
    pub auth: SshAuth,
}

/// SSH authentication methods
#[derive(Debug, Clone)]
pub enum SshAuth {
    /// Agent-based authentication
    Agent,
    /// Key-based authentication with path to private key
    Key(PathBuf),
    /// Password authentication
    Password(String),
}

/// Network configuration for Firecracker VM
#[derive(Debug, Clone)]
pub struct NetworkConfig {
    /// Host interface name
    pub host_interface: String,
    /// Guest MAC address
    pub guest_mac: String,
    /// IP configuration
    pub ip_config: String,
}

impl Default for FirecrackerConfig {
    fn default() -> Self {
        Self {
            binary_path: PathBuf::from("/usr/bin/firecracker"),
            kernel_path: PathBuf::from("/var/lib/firecracker/vmlinux"),
            rootfs_path: PathBuf::from("/var/lib/firecracker/rootfs.ext4"),
            mem_size_mib: 512,
            vcpu_count: 1,
            network_config: None,
            ssh_config: None,
        }
    }
}
