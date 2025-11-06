// ============================================================================
// File: packages/cylo/src/backends/firecracker/vm_lifecycle.rs
// ----------------------------------------------------------------------------
// VM startup, configuration, and lifecycle management.
// ============================================================================

use std::process::{Command, Stdio};
use std::time::Duration;

use bytes::Bytes;
use http_body_util::Full;
use hyper::{Method, Request};

use crate::async_task::AsyncTaskBuilder;
use crate::backends::{AsyncTask, BackendError, BackendResult};

use super::api_client::FireCrackerApiClient;
use super::config::FireCrackerConfig;
use super::vm_instance::VMInstance;

impl VMInstance {
    /// Start FireCracker VM
    pub fn start(mut self, fc_config: FireCrackerConfig) -> AsyncTask<BackendResult<Self>> {
        AsyncTaskBuilder::new(async move {
            let mut cmd = Command::new(&fc_config.firecracker_binary);
            cmd.args(&[
                "--api-sock",
                self.socket_path.to_str().unwrap_or(""),
                "--config-file",
                self.config_path.to_str().unwrap_or(""),
            ]);

            cmd.stdout(Stdio::null());
            cmd.stderr(Stdio::piped());

            let child = cmd.spawn().map_err(|e| BackendError::ProcessFailed {
                details: format!("Failed to start FireCracker: {}", e),
            })?;

            self.pid = Some(child.id());

            let api_client = FireCrackerApiClient::new(self.socket_path.clone()).map_err(|e| {
                BackendError::InvalidConfig {
                    backend: "FireCracker",
                    details: format!("Failed to create API client: {}", e),
                }
            })?;

            let machine_config = serde_json::json!({
                "vcpu_count": fc_config.vcpu_count,
                "mem_size_mib": fc_config.memory_size_mb,
                "cpu_template": "C3",
                "track_dirty_pages": false
            });

            api_client.configure_vm(&machine_config).await?;

            Self::configure_boot_source(&api_client, &self, &fc_config).await?;
            Self::configure_rootfs(&api_client, &self, &fc_config).await?;

            if fc_config.network_enabled {
                Self::configure_network(&api_client, &self).await?;
            }

            api_client.start_vm().await?;

            Self::wait_for_vm_ready(&api_client).await?;

            if let Some(ssh_cfg) = &self.ssh_config {
                Self::wait_for_ssh_ready(ssh_cfg).await?;
            }

            self.api_client = Some(api_client);
            Ok(self)
        }).spawn()
    }

    async fn configure_boot_source(
        api_client: &FireCrackerApiClient,
        vm: &VMInstance,
        fc_config: &FireCrackerConfig,
    ) -> BackendResult<()> {
        let boot_source = serde_json::json!({
            "kernel_image_path": fc_config.kernel_path,
            "boot_args": "console=ttyS0 reboot=k panic=1 pci=off"
        });

        let boot_body = serde_json::to_vec(&boot_source).map_err(|e| {
            BackendError::InvalidConfig {
                backend: "FireCracker",
                details: format!("Failed to serialize boot config: {}", e),
            }
        })?;

        let boot_uri = format!("unix://{}:/boot-source", vm.socket_path.display());
        let boot_request = Request::builder()
            .method(Method::PUT)
            .uri(boot_uri)
            .header("Content-Type", "application/json")
            .body(Full::new(Bytes::from(boot_body)))
            .map_err(|e| BackendError::InvalidConfig {
                backend: "FireCracker",
                details: format!("Failed to create boot request: {}", e),
            })?;

        api_client.http_client()
            .request(boot_request)
            .await
            .map_err(|e| BackendError::InvalidConfig {
                backend: "FireCracker",
                details: format!("Boot configuration failed: {}", e),
            })?;

        Ok(())
    }

    async fn configure_rootfs(
        api_client: &FireCrackerApiClient,
        vm: &VMInstance,
        fc_config: &FireCrackerConfig,
    ) -> BackendResult<()> {
        let rootfs_config = serde_json::json!({
            "drive_id": "rootfs",
            "path_on_host": fc_config.rootfs_path,
            "is_root_device": true,
            "is_read_only": false
        });

        let rootfs_body = serde_json::to_vec(&rootfs_config).map_err(|e| {
            BackendError::InvalidConfig {
                backend: "FireCracker",
                details: format!("Failed to serialize rootfs config: {}", e),
            }
        })?;

        let rootfs_uri = format!("unix://{}:/drives/rootfs", vm.socket_path.display());
        let rootfs_request = Request::builder()
            .method(Method::PUT)
            .uri(rootfs_uri)
            .header("Content-Type", "application/json")
            .body(Full::new(Bytes::from(rootfs_body)))
            .map_err(|e| BackendError::InvalidConfig {
                backend: "FireCracker",
                details: format!("Failed to create rootfs request: {}", e),
            })?;

        api_client.http_client()
            .request(rootfs_request)
            .await
            .map_err(|e| BackendError::InvalidConfig {
                backend: "FireCracker",
                details: format!("Rootfs configuration failed: {}", e),
            })?;

        Ok(())
    }

    async fn configure_network(
        api_client: &FireCrackerApiClient,
        vm: &VMInstance,
    ) -> BackendResult<()> {
        let network_config = serde_json::json!({
            "iface_id": "eth0",
            "host_dev_name": "tap0",
            "guest_mac": "AA:FC:00:00:00:01",
            "allow_mmds_requests": true
        });

        let network_body = serde_json::to_vec(&network_config).map_err(|e| {
            BackendError::InvalidConfig {
                backend: "FireCracker",
                details: format!("Failed to serialize network config: {}", e),
            }
        })?;

        let network_uri = format!("unix://{}:/network-interfaces/eth0", vm.socket_path.display());
        let network_request = Request::builder()
            .method(Method::PUT)
            .uri(network_uri)
            .header("Content-Type", "application/json")
            .body(Full::new(Bytes::from(network_body)))
            .map_err(|e| BackendError::InvalidConfig {
                backend: "FireCracker",
                details: format!("Failed to create network request: {}", e),
            })?;

        api_client.http_client()
            .request(network_request)
            .await
            .map_err(|e| BackendError::InvalidConfig {
                backend: "FireCracker",
                details: format!("Network configuration failed: {}", e),
            })?;

        Ok(())
    }

    async fn wait_for_vm_ready(api_client: &FireCrackerApiClient) -> BackendResult<()> {
        for attempt in 0..30 {
            match api_client.get_vm_metrics().await {
                Ok(metrics) => {
                    if let Some(state) = metrics.get("state").and_then(|v| v.as_str()) {
                        if state == "Running" {
                            return Ok(());
                        }
                    }
                }
                Err(_) => {}
            }

            if attempt == 29 {
                return Err(BackendError::ContainerFailed {
                    details: "VM failed to reach running state within timeout".to_string(),
                });
            }

            tokio::time::sleep(Duration::from_millis(1000)).await;
        }

        Ok(())
    }

    async fn wait_for_ssh_ready(ssh_cfg: &super::ssh::SshConfig) -> BackendResult<()> {
        for attempt in 0..30 {
            let addr_str = format!("{}:{}", ssh_cfg.host, ssh_cfg.port);
            if let Ok(addr) = addr_str.parse::<std::net::SocketAddr>() {
                if let Ok(tcp) =
                    std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(1))
                {
                    drop(tcp);
                    return Ok(());
                }
            }
            if attempt == 29 {
                return Err(BackendError::ContainerFailed {
                    details: "SSH not available within timeout".to_string(),
                });
            }
            tokio::time::sleep(Duration::from_millis(1000)).await;
        }

        Ok(())
    }
}
