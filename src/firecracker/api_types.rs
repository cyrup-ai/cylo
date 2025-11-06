//! Firecracker API request/response types

use serde::{Deserialize, Serialize};

/// Boot source configuration for Firecracker VM
/// API: PUT /boot-source
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootSource {
    /// Host level path to the kernel image used to boot the guest (required)
    pub kernel_image_path: String,

    /// Kernel boot arguments (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub boot_args: Option<String>,

    /// Host level path to the initrd image used to boot the guest (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initrd_path: Option<String>,
}

/// Machine configuration for Firecracker VM
/// API: PUT /machine-config
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MachineConfiguration {
    /// Number of vCPUs (required, 1 or even number, max 32)
    pub vcpu_count: u32,

    /// Memory size in MiB (required)
    pub mem_size_mib: u32,

    /// Enable simultaneous multithreading (optional, default: false)
    /// Can only be enabled on x86
    #[serde(skip_serializing_if = "Option::is_none")]
    pub smt: Option<bool>,
}

/// Drive configuration for Firecracker VM
/// API: PUT /drives/{drive_id}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Drive {
    /// Drive identifier (required)
    pub drive_id: String,

    /// Host level path for the guest drive (required for virtio-block)
    pub path_on_host: String,

    /// Is this the root device (required)
    pub is_root_device: bool,

    /// Is the drive read-only (optional, default: false)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_read_only: Option<bool>,
}

/// Network interface configuration for Firecracker VM
/// API: PUT /network-interfaces/{iface_id}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkInterface {
    /// Network interface ID (required)
    pub iface_id: String,

    /// Host device name for the tap interface (required)
    pub host_dev_name: String,

    /// Guest MAC address (required)
    pub guest_mac: String,
}

/// Instance action request
/// API: PUT /actions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceActionInfo {
    /// Action type (required)
    /// Valid values: "InstanceStart", "SendCtrlAltDel", "FlushMetrics"
    pub action_type: String,
}

/// Firecracker API error response
#[derive(Debug, Clone, Deserialize)]
pub struct FirecrackerError {
    /// Error description from Firecracker API
    pub fault_message: String,
}
