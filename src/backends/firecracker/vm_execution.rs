// ============================================================================
// File: packages/cylo/src/backends/firecracker/vm_execution.rs
// ----------------------------------------------------------------------------
// Code execution inside VM via SSH and script preparation.
// ============================================================================

use std::fs;
use std::io::Read;
use std::path::Path;
use std::time::Instant;

use crate::async_task::AsyncTaskBuilder;
use crate::backends::{AsyncTask, BackendError, BackendResult, ExecutionRequest, ExecutionResult, ResourceUsage};

use super::vm_instance::VMInstance;

impl VMInstance {
    /// Execute code in FireCracker VM
    pub fn execute(self, request: ExecutionRequest) -> AsyncTask<BackendResult<ExecutionResult>> {
        AsyncTaskBuilder::new(async move {
            let start_time = Instant::now();

            let exec_script = prepare_execution_script(&request)?;

            let ssh_config = self
                .ssh_config
                .as_ref()
                .ok_or_else(|| BackendError::InvalidConfig {
                    backend: "FireCracker",
                    details: "SSH configuration not available for VM".to_string(),
                })?;

            let script_path = format!("/tmp/exec-{}.sh", self.vm_id);
            let guest_script_path = format!("/tmp/exec-{}.sh", self.vm_id);

            fs::write(&script_path, &exec_script).map_err(|e| BackendError::FileSystemFailed {
                details: format!("Failed to write script: {}", e),
            })?;

            copy_script_to_vm(&ssh_config, &script_path, &guest_script_path).await?;

            let (exit_code, stdout, stderr) = execute_script_in_vm(&ssh_config, &guest_script_path).await?;

            let _ = fs::remove_file(&script_path);

            let resource_usage = collect_resource_metrics(&self).await;

            let duration = start_time.elapsed();

            Ok(ExecutionResult {
                exit_code,
                stdout,
                stderr,
                duration,
                resource_usage,
                metadata: {
                    let mut meta = std::collections::HashMap::new();
                    meta.insert("backend".to_string(), "FireCracker".to_string());
                    meta.insert("vm_id".to_string(), self.vm_id.clone());
                    meta.insert("execution_method".to_string(), "SSH".to_string());
                    meta
                },
            })
        }).spawn()
    }
}

/// Copy script to VM via SCP
async fn copy_script_to_vm(
    ssh_config: &super::ssh::SshConfig,
    script_path: &str,
    guest_script_path: &str,
) -> BackendResult<()> {
    tokio::task::spawn_blocking({
        let ssh_cfg = ssh_config.clone();
        let script = script_path.to_string();
        let guest_script = guest_script_path.to_string();
        move || -> BackendResult<()> {
            let session = ssh_cfg.create_session()?;
            let metadata = fs::metadata(&script).map_err(|e| BackendError::FileSystemFailed {
                details: format!("Failed to read script metadata: {}", e),
            })?;

            let mut local_file = std::fs::File::open(&script).map_err(|e| {
                BackendError::FileSystemFailed {
                    details: format!("Failed to open script: {}", e),
                }
            })?;

            let mut remote_file = session
                .scp_send(Path::new(&guest_script), 0o755, metadata.len(), None)
                .map_err(|e| BackendError::ProcessFailed {
                    details: format!("SCP failed: {}", e),
                })?;

            std::io::copy(&mut local_file, &mut remote_file).map_err(|e| {
                BackendError::ProcessFailed {
                    details: format!("File copy failed: {}", e),
                }
            })?;

            remote_file.send_eof().map_err(|e| BackendError::ProcessFailed {
                details: format!("EOF failed: {}", e),
            })?;
            remote_file.wait_close().map_err(|e| BackendError::ProcessFailed {
                details: format!("Wait close failed: {}", e),
            })?;

            Ok(())
        }
    })
    .await
    .map_err(|e| BackendError::ProcessFailed {
        details: format!("Task join failed: {}", e),
    })??;

    Ok(())
}

/// Execute script in VM via SSH
async fn execute_script_in_vm(
    ssh_config: &super::ssh::SshConfig,
    guest_script_path: &str,
) -> BackendResult<(i32, String, String)> {
    tokio::task::spawn_blocking({
        let ssh_cfg = ssh_config.clone();
        let guest_script = guest_script_path.to_string();
        move || -> BackendResult<(i32, String, String)> {
            let session = ssh_cfg.create_session()?;
            let mut channel = session
                .channel_session()
                .map_err(|e| BackendError::ProcessFailed {
                    details: format!("Failed to create channel: {}", e),
                })?;

            channel
                .exec(&format!("bash {}", guest_script))
                .map_err(|e| BackendError::ProcessFailed {
                    details: format!("Exec failed: {}", e),
                })?;

            let mut stdout = String::new();
            channel.read_to_string(&mut stdout).map_err(|e| {
                BackendError::ProcessFailed {
                    details: format!("Read stdout failed: {}", e),
                }
            })?;

            let mut stderr = String::new();
            channel.stderr().read_to_string(&mut stderr).map_err(|e| {
                BackendError::ProcessFailed {
                    details: format!("Read stderr failed: {}", e),
                }
            })?;

            channel.wait_close().map_err(|e| BackendError::ProcessFailed {
                details: format!("Wait close failed: {}", e),
            })?;

            let exit_code = channel.exit_status().map_err(|e| BackendError::ProcessFailed {
                details: format!("Get exit status failed: {}", e),
            })?;

            Ok((exit_code, stdout, stderr))
        }
    })
    .await
    .map_err(|e| BackendError::ProcessFailed {
        details: format!("Task join failed: {}", e),
    })?
}

/// Collect resource metrics from VM
async fn collect_resource_metrics(vm: &VMInstance) -> ResourceUsage {
    if let Some(ref api_client) = vm.api_client {
        match api_client.get_vm_metrics().await {
            Ok(metrics) => ResourceUsage {
                peak_memory: metrics
                    .get("memory_usage_bytes")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
                cpu_time_ms: metrics
                    .get("cpu_usage_us")
                    .and_then(|v| v.as_u64())
                    .map(|us| us / 1000)
                    .unwrap_or(0),
                process_count: 1,
                disk_bytes_written: metrics
                    .get("disk_write_bytes")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
                disk_bytes_read: metrics
                    .get("disk_read_bytes")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
                network_bytes_sent: 0,
                network_bytes_received: 0,
            },
            Err(_) => ResourceUsage::default(),
        }
    } else {
        ResourceUsage::default()
    }
}

/// Prepare execution script for the VM
fn prepare_execution_script(request: &ExecutionRequest) -> BackendResult<String> {
    let script = match request.language.as_str() {
        "python" | "python3" => {
            format!(
                "#!/bin/bash\necho '{}' | python3",
                request.code.replace('\'', "'\"'\"'")
            )
        }
        "javascript" | "js" | "node" => {
            format!(
                "#!/bin/bash\necho '{}' | node",
                request.code.replace('\'', "'\"'\"'")
            )
        }
        "rust" => {
            format!(
                "#!/bin/bash\necho '{}' > /tmp/main.rs && cd /tmp && rustc main.rs && ./main",
                request.code.replace('\'', "'\"'\"'")
            )
        }
        "bash" | "sh" => {
            format!("#!/bin/bash\n{}", request.code)
        }
        "go" => {
            format!(
                "#!/bin/bash\necho '{}' > /tmp/main.go && cd /tmp && go run main.go",
                request.code.replace('\'', "'\"'\"'")
            )
        }
        _ => {
            return Err(BackendError::UnsupportedLanguage {
                backend: "FireCracker",
                language: request.language.clone(),
            });
        }
    };

    Ok(script)
}
