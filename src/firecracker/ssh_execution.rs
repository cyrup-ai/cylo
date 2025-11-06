//! SSH-based code execution in Firecracker VM

use std::fs::{self, File};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;

use anyhow::{Context, Result};
use log::info;
use ssh2::Session;

use super::config::{SshAuth, SshConfig};

/// Execute code in the VM via SSH
pub fn execute_code(
    ssh_config: &SshConfig,
    vm_id: &str,
    language: &str,
    code: &str,
) -> Result<String> {
    info!("Executing {} code in Firecracker VM", language);

    // Create a temporary file with the code
    let tmp_dir = "/tmp";
    let file_name = format!(
        "code-{}-{}.{}",
        vm_id,
        language,
        get_file_extension(language)
    );
    let file_path = format!("{tmp_dir}/{file_name}");

    let mut file = File::create(&file_path)?;
    file.write_all(code.as_bytes())?;

    // Copy the file to the VM
    copy_to_vm(ssh_config, &file_path, &format!("/tmp/{file_name}"))?;

    // Execute the code in the VM
    let cmd = get_execution_command(language, &format!("/tmp/{file_name}"));
    let output = execute_command(ssh_config, &cmd)?;

    // Clean up
    fs::remove_file(file_path)?;

    Ok(output)
}

/// Copy a file to the VM using SSH/SCP
pub fn copy_to_vm(ssh_config: &SshConfig, host_path: &str, guest_path: &str) -> Result<()> {
    info!("Copying {} to VM at {}", host_path, guest_path);

    let session = create_ssh_session(ssh_config)?;

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
pub fn execute_command(ssh_config: &SshConfig, command: &str) -> Result<String> {
    info!("Executing command in VM: {}", command);

    let session = create_ssh_session(ssh_config)?;

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
pub fn create_ssh_session(ssh_config: &SshConfig) -> Result<Session> {
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
