use crate::error::StorageError;
use log::{info, warn};
use std::process::Command;

/// Utilities for running commands with privilege escalation (sudo).
pub struct PrivilegeManager;

impl PrivilegeManager {
    /// Try to run a command with sudo if available, otherwise try without sudo.
    ///
    /// This function attempts multiple strategies:
    /// 1. First tries running without sudo
    /// 2. If that fails, checks for non-interactive sudo
    /// 3. Falls back to interactive sudo if needed
    /// 4. Finally tries without privileges as a last resort
    ///
    /// Returns Ok(true) if the command succeeded, Ok(false) if it failed gracefully,
    /// or Err if there was an error that should be propagated.
    pub fn run_with_sudo(cmd: &str, args: &[&str]) -> Result<bool, StorageError> {
        // First try running without sudo
        info!("Attempting to run '{}' without sudo first", cmd);
        let result = Command::new(cmd).args(args).output();

        if let Ok(output) = result {
            if output.status.success() {
                info!("Command succeeded without sudo");
                return Ok(true);
            }
        }

        // Format the full command for logging/display
        let full_cmd = format!("{} {}", cmd, args.join(" "));

        // Check if we can use sudo non-interactively
        info!("Checking if sudo is available non-interactively");
        let sudo_check = Command::new("sudo")
            .arg("-n") // Non-interactive check
            .arg("true")
            .status();

        let sudo_available = sudo_check.map(|s| s.success()).unwrap_or(false);

        if sudo_available {
            // Try the command with sudo non-interactively
            info!("Sudo is available, trying command with sudo");
            let sudo_result = Command::new("sudo")
                .arg("-n") // Non-interactive mode
                .arg(cmd)
                .args(args)
                .output();

            match sudo_result {
                Ok(output) => {
                    if output.status.success() {
                        info!("Successfully executed command with sudo: {}", full_cmd);
                        return Ok(true);
                    } else {
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        warn!("Command failed with sudo: {}", stderr);
                    }
                }
                Err(e) => {
                    warn!("Failed to execute command with sudo: {}", e);
                }
            }
        } else {
            // Going to need an interactive sudo prompt
            info!("\nSecure code execution requires creating an isolated ramdisk environment.");
            info!("This requires elevated privileges to execute the following command:");
            info!("    sudo {}", full_cmd);
            info!("This operation provides secure isolation for the code you're about to run.");

            // Try with interactive sudo
            let sudo_interactive = Command::new("sudo").arg(cmd).args(args).status();

            match sudo_interactive {
                Ok(status) => {
                    if status.success() {
                        info!("Successfully executed command with sudo");
                        return Ok(true);
                    } else {
                        warn!("Command failed with interactive sudo");
                    }
                }
                Err(e) => {
                    warn!("Failed to execute command with interactive sudo: {}", e);
                }
            }
        }

        // If everything failed, try one more time as the current user
        info!("Trying alternative approach without elevated privileges");
        let fallback_result = Command::new(cmd).args(args).output();

        match fallback_result {
            Ok(output) => {
                if output.status.success() {
                    info!("Command succeeded in fallback mode");
                    Ok(true)
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    warn!("Fallback command also failed: {}", stderr);
                    Ok(false)
                }
            }
            Err(e) => {
                warn!("Fallback command error: {}", e);
                Ok(false) // Return false instead of error to allow graceful fallback
            }
        }
    }
}
