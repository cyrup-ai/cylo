//! Firecracker VM lifecycle management

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{Context, Result};
use log::{info, warn};

use crate::error::StorageError;

use super::api_client;
use super::api_types::{BootSource, Drive, InstanceActionInfo, MachineConfiguration, NetworkInterface};
use super::config::FirecrackerConfig;
use super::ssh_execution;

/// Firecracker VM manager
pub struct FirecrackerVM {
    config: FirecrackerConfig,
    socket_path: PathBuf,
    api_socket: Option<PathBuf>,
    vm_id: String,
}

impl FirecrackerVM {
    /// Create a new Firecracker VM manager
    pub fn new(config: FirecrackerConfig, vm_id: impl Into<String>) -> Self {
        let vm_id = vm_id.into();
        let socket_path = PathBuf::from(format!("/tmp/firecracker-{vm_id}.sock"));

        Self {
            config,
            socket_path,
            api_socket: None,
            vm_id,
        }
    }

    /// Start the Firecracker VM
    pub async fn start(&mut self) -> Result<()> {
        info!("Starting Firecracker VM with ID: {}", self.vm_id);

        // Check if Firecracker binary exists
        if !self.config.binary_path.exists() {
            return Err(anyhow::anyhow!(
                "Firecracker binary not found at {:?}",
                self.config.binary_path
            ));
        }

        // Remove socket file if it exists
        if self.socket_path.exists() {
            fs::remove_file(&self.socket_path)?;
        }

        // Start Firecracker process
        let _cmd = Command::new(&self.config.binary_path)
            .arg("--api-sock")
            .arg(&self.socket_path)
            .arg("--id")
            .arg(&self.vm_id)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("Failed to start Firecracker process")?;

        // Wait for socket to be created
        let mut attempts = 0;
        while !self.socket_path.exists() && attempts < 10 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            attempts += 1;
        }

        if !self.socket_path.exists() {
            return Err(anyhow::anyhow!("Firecracker API socket not created"));
        }

        self.api_socket = Some(self.socket_path.clone());

        // Configure the VM (now async)
        self.configure_vm().await?;

        info!("Firecracker VM started successfully");
        Ok(())
    }

    /// Configure the VM using the Firecracker API
    async fn configure_vm(&self) -> Result<()> {
        info!("Configuring Firecracker VM via API");

        let api_socket = self
            .api_socket
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("API socket not available"))?;

        // Configure boot source
        let boot_source = BootSource {
            kernel_image_path: self.config.kernel_path.display().to_string(),
            boot_args: Some("console=ttyS0 reboot=k panic=1 pci=off".to_string()),
            initrd_path: None, // Can be added from config if needed
        };

        api_client::api_put(api_socket, "boot-source", &boot_source)
            .await
            .context("Failed to configure boot source")?;

        info!("Boot source configured");

        // Configure machine config (vCPU and memory)
        let machine_config = MachineConfiguration {
            vcpu_count: self.config.vcpu_count,
            mem_size_mib: self.config.mem_size_mib,
            smt: Some(false), // Disable hyperthreading
        };

        api_client::api_put(api_socket, "machine-config", &machine_config)
            .await
            .context("Failed to configure machine")?;

        info!(
            "Machine config set: {} vCPUs, {} MiB memory",
            self.config.vcpu_count, self.config.mem_size_mib
        );

        // Configure rootfs drive
        let drive = Drive {
            drive_id: "rootfs".to_string(),
            path_on_host: self.config.rootfs_path.display().to_string(),
            is_root_device: true,
            is_read_only: Some(false),
        };

        api_client::api_put(api_socket, "drives/rootfs", &drive)
            .await
            .context("Failed to configure root filesystem")?;

        info!("Root filesystem configured");

        // Configure network if provided
        if let Some(net_config) = &self.config.network_config {
            let network_interface = NetworkInterface {
                iface_id: "eth0".to_string(),
                host_dev_name: net_config.host_interface.clone(),
                guest_mac: net_config.guest_mac.clone(),
            };

            api_client::api_put(api_socket, "network-interfaces/eth0", &network_interface)
                .await
                .context("Failed to configure network interface")?;

            info!("Network interface configured");
        }

        // Start the VM instance
        let start_action = InstanceActionInfo {
            action_type: "InstanceStart".to_string(),
        };

        api_client::api_put(api_socket, "actions", &start_action)
            .await
            .context("Failed to start VM instance")?;

        info!("VM instance started successfully");

        Ok(())
    }

    /// Stop the Firecracker VM
    pub async fn stop(&self) -> Result<(), StorageError> {
        info!("Stopping Firecracker VM with ID: {}", self.vm_id);

        if let Some(socket) = &self.api_socket {
            // Send shutdown request
            let shutdown_action = InstanceActionInfo {
                action_type: "SendCtrlAltDel".to_string(),
            };

            if let Err(e) = api_client::api_put(socket, "actions", &shutdown_action).await {
                warn!("Failed to send shutdown request: {}", e);
                // Continue with cleanup even if shutdown request fails
            }

            // Wait for VM to shut down
            tokio::time::sleep(Duration::from_secs(2)).await;

            // Clean up socket file
            if socket.exists()
                && let Err(e) = fs::remove_file(socket)
            {
                warn!("Failed to remove socket file: {}", e);
                // Continue anyway
            }
        }

        info!("Firecracker VM stopped successfully");
        Ok(())
    }

    /// Execute code in the Firecracker VM
    pub fn execute_code(&self, language: &str, code: &str) -> Result<String> {
        let ssh_config = self
            .config
            .ssh_config
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("SSH configuration not provided"))?;

        ssh_execution::execute_code(ssh_config, &self.vm_id, language, code)
    }
}
