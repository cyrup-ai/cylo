// ============================================================================
// File: packages/cylo/src/backends/firecracker/ssh.rs
// ----------------------------------------------------------------------------
// SSH configuration and session management for VM access.
// ============================================================================

use std::path::PathBuf;
use serde::{Deserialize, Serialize};

use crate::backends::{BackendError, BackendResult};

/// SSH authentication methods for VM access
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SshAuth {
    /// Agent-based authentication
    Agent,
    /// Key-based authentication with path to private key
    Key(PathBuf),
    /// Password authentication
    Password(String),
}

/// SSH configuration for VM communication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshConfig {
    /// SSH host (VM IP address)
    pub host: String,
    /// SSH port
    pub port: u16,
    /// SSH username
    pub username: String,
    /// SSH authentication method
    pub auth: SshAuth,
}

impl SshConfig {
    /// Create SSH session to VM
    pub fn create_session(&self) -> BackendResult<ssh2::Session> {
        let tcp = std::net::TcpStream::connect(format!("{}:{}", self.host, self.port))
            .map_err(|e| BackendError::ProcessFailed {
                details: format!("TCP connection failed: {}", e),
            })?;

        let mut session = ssh2::Session::new().map_err(|e| BackendError::ProcessFailed {
            details: format!("SSH session creation failed: {}", e),
        })?;

        session.set_tcp_stream(tcp);
        session.handshake().map_err(|e| BackendError::ProcessFailed {
            details: format!("SSH handshake failed: {}", e),
        })?;

        match &self.auth {
            SshAuth::Agent => {
                session.userauth_agent(&self.username).map_err(|e| {
                    BackendError::ProcessFailed {
                        details: format!("SSH agent auth failed: {}", e),
                    }
                })?;
            }
            SshAuth::Key(key_path) => {
                session
                    .userauth_pubkey_file(&self.username, None, key_path, None)
                    .map_err(|e| BackendError::ProcessFailed {
                        details: format!("SSH key auth failed: {}", e),
                    })?;
            }
            SshAuth::Password(password) => {
                session
                    .userauth_password(&self.username, password)
                    .map_err(|e| BackendError::ProcessFailed {
                        details: format!("SSH password auth failed: {}", e),
                    })?;
            }
        }

        if !session.authenticated() {
            return Err(BackendError::ProcessFailed {
                details: "SSH authentication failed".to_string(),
            });
        }

        Ok(session)
    }
}
