//! Firecracker-based secure execution environment

use std::fs::{self, File};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{Context, Result};
use log::{error, info, warn};
use ssh2::Session;

// HTTP client for Firecracker API over Unix sockets
use bytes::Bytes;
use http::{Method, Request, StatusCode};
use http_body_util::{BodyExt, Full};
use hyper_client_sockets::{Backend, tokio::TokioBackend};
use serde::{Deserialize, Serialize};

use crate::config::RamdiskConfig;
use crate::error::StorageError;

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

        // Configure boot source
        let boot_source = BootSource {
            kernel_image_path: self.config.kernel_path.display().to_string(),
            boot_args: Some("console=ttyS0 reboot=k panic=1 pci=off".to_string()),
            initrd_path: None, // Can be added from config if needed
        };

        self.api_put("boot-source", &boot_source)
            .await
            .context("Failed to configure boot source")?;

        info!("Boot source configured");

        // Configure machine config (vCPU and memory)
        let machine_config = MachineConfiguration {
            vcpu_count: self.config.vcpu_count,
            mem_size_mib: self.config.mem_size_mib,
            smt: Some(false), // Disable hyperthreading
        };

        self.api_put("machine-config", &machine_config)
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

        self.api_put("drives/rootfs", &drive)
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

            self.api_put("network-interfaces/eth0", &network_interface)
                .await
                .context("Failed to configure network interface")?;

            info!("Network interface configured");
        }

        // Start the VM instance
        let start_action = InstanceActionInfo {
            action_type: "InstanceStart".to_string(),
        };

        self.api_put("actions", &start_action)
            .await
            .context("Failed to start VM instance")?;

        info!("VM instance started successfully");

        Ok(())
    }

    /// Make a PUT request to the Firecracker API over Unix socket
    async fn api_put<T: Serialize>(&self, path: &str, body: &T) -> Result<()> {
        let socket_path = self
            .api_socket
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("API socket not available"))?;

        // Serialize request body to JSON
        let json_body = serde_json::to_vec(body).context("Failed to serialize request body")?;

        // Connect to Unix socket
        let io = TokioBackend::connect_to_unix_socket(socket_path)
            .await
            .context("Failed to connect to Firecracker API socket")?;

        // Create HTTP/1 connection
        let (mut send_request, conn) = hyper::client::conn::http1::handshake::<_, Full<Bytes>>(io)
            .await
            .context("Failed to perform HTTP handshake")?;

        // Spawn connection handler
        tokio::spawn(async move {
            if let Err(e) = conn.await {
                error!("Firecracker API connection error: {}", e);
            }
        });

        // Build HTTP request
        let uri = format!("http://localhost/{}", path.trim_start_matches('/'));
        let request = Request::builder()
            .method(Method::PUT)
            .uri(&uri)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .body(Full::new(Bytes::from(json_body)))
            .context("Failed to build HTTP request")?;

        // Send request and await response
        let response = send_request
            .send_request(request)
            .await
            .context("Failed to send API request")?;

        // Check response status
        let status = response.status();

        if status == StatusCode::NO_CONTENT {
            // 204 No Content - success
            return Ok(());
        }

        // Read error response body
        let body_bytes = response
            .into_body()
            .collect()
            .await
            .context("Failed to read error response")?
            .to_bytes();

        let error_text = String::from_utf8_lossy(&body_bytes);

        // Try to parse as FirecrackerError for better error messages
        if let Ok(fc_error) = serde_json::from_slice::<FirecrackerError>(&body_bytes) {
            return Err(anyhow::anyhow!(
                "Firecracker API error ({}): {}",
                status,
                fc_error.fault_message
            ));
        }

        // Fallback to raw error text
        Err(anyhow::anyhow!(
            "Firecracker API request failed with status {}: {}",
            status,
            error_text
        ))
    }

    /// Stop the Firecracker VM
    pub async fn stop(&self) -> Result<(), StorageError> {
        info!("Stopping Firecracker VM with ID: {}", self.vm_id);

        if let Some(socket) = &self.api_socket {
            // Send shutdown request
            let shutdown_action = InstanceActionInfo {
                action_type: "SendCtrlAltDel".to_string(),
            };

            if let Err(e) = self.api_put("actions", &shutdown_action).await {
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
        info!("Executing {} code in Firecracker VM", language);

        // Create a temporary file with the code
        let tmp_dir = "/tmp";
        let file_name = format!(
            "code-{}-{}.{}",
            self.vm_id,
            language,
            Self::get_file_extension(language)
        );
        let file_path = format!("{tmp_dir}/{file_name}");

        let mut file = File::create(&file_path)?;
        file.write_all(code.as_bytes())?;

        // Copy the file to the VM
        self.copy_to_vm(&file_path, &format!("/tmp/{file_name}"))?;

        // Execute the code in the VM
        let cmd = Self::get_execution_command(language, &format!("/tmp/{file_name}"));
        let output = self.execute_command(&cmd)?;

        // Clean up
        fs::remove_file(file_path)?;

        Ok(output)
    }

    /// Copy a file to the VM using SSH/SCP
    fn copy_to_vm(&self, host_path: &str, guest_path: &str) -> Result<()> {
        info!("Copying {} to VM at {}", host_path, guest_path);

        let ssh_config = self
            .config
            .ssh_config
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("SSH configuration not provided"))?;

        let session = self.create_ssh_session(ssh_config)?;

        // Get file metadata for size
        let metadata = fs::metadata(host_path).context("Failed to read host file metadata")?;
        let file_size = metadata.len();

        // Open local file
        let mut local_file = File::open(host_path).context("Failed to open host file")?;

        // Create remote file via SCP
        let mut remote_file = session
            .scp_send(Path::new(guest_path), 0o644, file_size, None)
            .context("Failed to initiate SCP transfer")?;

        // Copy file contents
        std::io::copy(&mut local_file, &mut remote_file).context("Failed to copy file contents")?;

        // Send EOF to remote file
        remote_file.send_eof().context("Failed to send EOF")?;
        remote_file.wait_eof().context("Failed to wait for EOF")?;
        remote_file.close().context("Failed to close remote file")?;
        remote_file
            .wait_close()
            .context("Failed to wait for close")?;

        info!("Successfully copied file to VM");
        Ok(())
    }

    /// Execute a command in the VM via SSH
    fn execute_command(&self, command: &str) -> Result<String> {
        info!("Executing command in VM: {}", command);

        let ssh_config = self
            .config
            .ssh_config
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("SSH configuration not provided"))?;

        let session = self.create_ssh_session(ssh_config)?;

        // Create SSH channel
        let mut channel = session
            .channel_session()
            .context("Failed to create SSH channel")?;

        // Execute command
        channel.exec(command).context("Failed to execute command")?;

        // Read output
        let mut output = String::new();
        channel
            .read_to_string(&mut output)
            .context("Failed to read command output")?;

        // Wait for channel to close
        channel
            .wait_close()
            .context("Failed to wait for channel close")?;

        // Check exit status
        let exit_status = channel.exit_status().context("Failed to get exit status")?;

        if exit_status != 0 {
            return Err(anyhow::anyhow!(
                "Command failed with exit code {}: {}",
                exit_status,
                output
            ));
        }

        info!("Command executed successfully");
        Ok(output)
    }

    /// Create an SSH session to the VM
    fn create_ssh_session(&self, ssh_config: &SshConfig) -> Result<Session> {
        // Connect to SSH server
        let tcp = TcpStream::connect(format!("{}:{}", ssh_config.host, ssh_config.port))
            .context("Failed to connect to SSH server")?;

        // Create SSH session
        let mut session = Session::new().context("Failed to create SSH session")?;
        session.set_tcp_stream(tcp);
        session.handshake().context("SSH handshake failed")?;

        // Authenticate based on configured method
        match &ssh_config.auth {
            SshAuth::Agent => {
                session
                    .userauth_agent(&ssh_config.username)
                    .context("SSH agent authentication failed")?;
            }
            SshAuth::Key(key_path) => {
                session
                    .userauth_pubkey_file(&ssh_config.username, None, key_path, None)
                    .context("SSH key authentication failed")?;
            }
            SshAuth::Password(password) => {
                session
                    .userauth_password(&ssh_config.username, password)
                    .context("SSH password authentication failed")?;
            }
        }

        if !session.authenticated() {
            return Err(anyhow::anyhow!("SSH authentication failed"));
        }

        Ok(session)
    }

    /// Get the file extension for a language
    fn get_file_extension(language: &str) -> &'static str {
        match language {
            "go" => "go",
            "rust" => "rs",
            "python" => "py",
            "js" => "js",
            _ => "txt",
        }
    }

    /// Get the execution command for a language
    fn get_execution_command(language: &str, file_path: &str) -> String {
        match language {
            "go" => format!("go run {file_path}"),
            "rust" => format!("rust-script {file_path}"),
            "python" => format!("python3 {file_path}"),
            "js" => format!("node {file_path}"),
            _ => format!("cat {file_path}"),
        }
    }
}

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
