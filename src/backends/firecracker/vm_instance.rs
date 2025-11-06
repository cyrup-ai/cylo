// ============================================================================
// File: packages/cylo/src/backends/firecracker/vm_instance.rs
// ----------------------------------------------------------------------------
// VM instance struct and basic operations (create, config, cleanup).
// ============================================================================

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use crate::async_task::AsyncTaskBuilder;
use crate::backends::{AsyncTask, BackendConfig, BackendError, BackendResult, ExecutionRequest};

use super::api_client::FireCrackerApiClient;
use super::config::FireCrackerConfig;
use super::ssh::SshConfig;

/// VM instance information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VMInstance {
    /// Unique VM ID
    pub vm_id: String,

    /// VM socket path
    pub socket_path: PathBuf,

    /// VM configuration file path
    pub config_path: PathBuf,

    /// VM process ID
    pub pid: Option<u32>,

    /// API client for VM management
    #[serde(skip)]
    pub api_client: Option<FireCrackerApiClient>,

    /// Creation timestamp
    pub created_at: SystemTime,

    /// SSH configuration for guest access
    pub ssh_config: Option<SshConfig>,
}

impl VMInstance {
    /// Create VM instance for execution
    pub fn create(_request: &ExecutionRequest, backend_config: &BackendConfig) -> BackendResult<Self> {
        let vm_id = format!(
            "cylo-{}-{}",
            uuid::Uuid::new_v4().simple(),
            std::process::id()
        );

        let socket_path = std::env::temp_dir().join(format!("{}.sock", vm_id));
        let config_path = std::env::temp_dir().join(format!("{}.json", vm_id));

        let ssh_config = Self::build_ssh_config(backend_config);

        Ok(VMInstance {
            vm_id,
            socket_path,
            config_path,
            pid: None,
            api_client: None,
            created_at: SystemTime::now(),
            ssh_config,
        })
    }

    fn build_ssh_config(backend_config: &BackendConfig) -> Option<SshConfig> {
        if !backend_config.backend_specific.contains_key("ssh_host") {
            return None;
        }

        let host = backend_config
            .backend_specific
            .get("ssh_host")
            .cloned()
            .unwrap_or_else(|| "172.16.0.2".to_string());
        let port = backend_config
            .backend_specific
            .get("ssh_port")
            .and_then(|p| p.parse().ok())
            .unwrap_or(22);
        let username = backend_config
            .backend_specific
            .get("ssh_username")
            .cloned()
            .unwrap_or_else(|| "root".to_string());

        let auth = if let Some(key_path) = backend_config.backend_specific.get("ssh_key_path") {
            super::ssh::SshAuth::Key(PathBuf::from(key_path))
        } else if let Some(password) = backend_config.backend_specific.get("ssh_password") {
            super::ssh::SshAuth::Password(password.clone())
        } else {
            super::ssh::SshAuth::Agent
        };

        Some(SshConfig {
            host,
            port,
            username,
            auth,
        })
    }

    /// Generate VM configuration file
    pub fn generate_config(&self, fc_config: &FireCrackerConfig, _request: &ExecutionRequest) -> BackendResult<()> {
        let vm_config = serde_json::json!({
            "boot-source": {
                "kernel_image_path": fc_config.kernel_path.display().to_string(),
                "boot_args": "console=ttyS0 reboot=k panic=1 pci=off"
            },
            "drives": [
                {
                    "drive_id": "rootfs",
                    "path_on_host": fc_config.rootfs_path.display().to_string(),
                    "is_root_device": true,
                    "is_read_only": false
                }
            ],
            "machine-config": {
                "vcpu_count": fc_config.vcpu_count,
                "mem_size_mib": fc_config.memory_size_mb,
                "ht_enabled": false
            },
            "logger": {
                "log_path": format!("/tmp/{}.log", self.vm_id),
                "level": "Info"
            }
        });

        let config_content =
            serde_json::to_string_pretty(&vm_config).map_err(|e| BackendError::Internal {
                message: format!("Failed to serialize VM config: {}", e),
            })?;

        fs::write(&self.config_path, config_content).map_err(|e| BackendError::FileSystemFailed {
            details: format!("Failed to write VM config: {}", e),
        })?;

        Ok(())
    }

    /// Stop and cleanup VM
    pub fn cleanup(self) -> AsyncTask<BackendResult<()>> {
        AsyncTaskBuilder::new(async move {
            if let Some(pid) = self.pid {
                let _ = Command::new("kill")
                    .args(&["-TERM", &pid.to_string()])
                    .status();

                tokio::time::sleep(Duration::from_secs(1)).await;

                let _ = Command::new("kill")
                    .args(&["-KILL", &pid.to_string()])
                    .status();
            }

            let _ = fs::remove_file(&self.socket_path);
            let _ = fs::remove_file(&self.config_path);
            let _ = fs::remove_file(format!("/tmp/{}.log", self.vm_id));

            Ok(())
        }).spawn()
    }
}
