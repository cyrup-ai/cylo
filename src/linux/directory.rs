use crate::error::StorageError;
use log::{error, info};
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Utilities for directory setup and permission management.
pub struct DirectoryManager;

impl DirectoryManager {
    /// Get the current user and group names from the current directory's metadata.
    ///
    /// Returns (username, groupname) as strings. Falls back to "$USER" and "$GROUP"
    /// if the lookup fails.
    pub fn get_current_user_group() -> (String, String) {
        let current_dir_path = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        match fs::metadata(&current_dir_path) {
            Ok(metadata) => {
                let uid = metadata.uid();
                let gid = metadata.gid();

                // Try to get user and group names
                let username = match Command::new("id").args(["-nu", &uid.to_string()]).output() {
                    Ok(output) => String::from_utf8_lossy(&output.stdout).trim().to_string(),
                    Err(_) => format!("{}", uid),
                };

                let groupname = match Command::new("id").args(["-ng", &gid.to_string()]).output() {
                    Ok(output) => String::from_utf8_lossy(&output.stdout).trim().to_string(),
                    Err(_) => format!("{}", gid),
                };

                (username, groupname)
            }
            Err(_) => ("$USER".to_string(), "$GROUP".to_string()),
        }
    }

    /// Check if a directory exists and is writable.
    pub fn is_writable(path: &Path) -> bool {
        path.exists()
            && fs::metadata(path)
                .map(|m| m.permissions().mode() & 0o200 != 0)
                .unwrap_or(false)
    }

    /// Ensure parent directory and mount point exist with proper permissions.
    ///
    /// This function checks if the parent directory exists and is writable.
    /// If not, it attempts to create the directories and set proper ownership
    /// using sudo if necessary.
    ///
    /// Returns Ok(()) if directories are ready, or Err with detailed instructions.
    pub fn ensure_mount_directories(mount_point: &Path) -> Result<(), StorageError> {
        let parent_dir = mount_point.parent().unwrap_or(Path::new("/"));

        // Check parent directory
        let parent_exists = parent_dir.exists();
        let parent_writable = Self::is_writable(parent_dir);

        // Check mount point
        let mount_exists = mount_point.exists();

        if !parent_exists || !parent_writable {
            let (user, group) = Self::get_current_user_group();

            info!("\nSecure code execution requires creating an isolated ramdisk environment.");
            info!("The following directory needs to be created:");
            info!(
                "- {}: [exists: {}] [writable: {}]",
                parent_dir.display(),
                if parent_exists { "yes" } else { "no" },
                if parent_writable { "yes" } else { "no" }
            );

            info!("This requires elevated privileges to execute the following command:");
            info!(
                "    sudo mkdir -p {} && sudo chown {}:{} {}",
                mount_point.display(),
                user,
                group,
                parent_dir.display()
            );
            info!("This operation provides secure isolation for the code you're about to run.");

            // Try to execute the command with sudo
            let mkdir_result = Command::new("sudo")
                .args(["mkdir", "-p", mount_point.to_str().unwrap_or("")])
                .status();

            if let Ok(status) = mkdir_result {
                if status.success() {
                    info!("Successfully created directory with sudo");

                    // Now set permissions
                    let chown_cmd = format!("{}:{}", user, group);
                    let chown_result = Command::new("sudo")
                        .args(["chown", &chown_cmd, parent_dir.to_str().unwrap_or("")])
                        .status();

                    if let Ok(status) = chown_result {
                        if status.success() {
                            info!("Successfully set permissions with sudo");
                            return Ok(());
                        }
                    }
                }
            }

            // Build error message with instructions
            let mut error_msg =
                "\nUnable to create secure ramdisk execution environment.\n\n".to_string();

            error_msg.push_str("Directory status:\n");
            error_msg.push_str(&format!(
                "- {}: [exists: {}] [writable: {}]\n",
                parent_dir.display(),
                if parent_exists { "yes" } else { "no" },
                if parent_writable { "yes" } else { "no" }
            ));

            error_msg.push_str("\nThe secure execution environment requires a ramdisk mounted at /ephemeral/cylo.\n");
            error_msg.push_str("Please run the following command to fix this issue:\n\n");
            error_msg.push_str(&format!(
                "    sudo mkdir -p {} && sudo chown {}:{} {}\n\n",
                mount_point.display(),
                user,
                group,
                parent_dir.display()
            ));

            error_msg.push_str(
                "This command will create the required directories with proper permissions.\n",
            );
            error_msg
                .push_str("After running this command, try executing this application again.\n");
            error_msg.push_str(
                "The application will then securely mount a ramdisk for code execution.\n",
            );

            error!("{}", error_msg);
            return Err(StorageError::InsufficientPrivileges(error_msg));
        }

        // Ensure mount point exists
        if !mount_exists {
            Self::create_mount_point(mount_point)?;
        }

        Ok(())
    }

    /// Create the mount point directory, trying with sudo if necessary.
    fn create_mount_point(mount_point: &Path) -> Result<(), StorageError> {
        info!("Creating mount point at {}", mount_point.display());

        match fs::create_dir_all(mount_point) {
            Ok(_) => {
                info!("Mount point directory created successfully");
                Ok(())
            }
            Err(e) => {
                error!("Failed to create mount point directory: {}", e);

                let (user, group) = Self::get_current_user_group();

                info!("\nSecure code execution requires creating an isolated ramdisk environment.");
                info!("Failed to create mount point directory: {}", e);
                info!("Trying with elevated privileges...");

                // Try with sudo
                let mkdir_result = Command::new("sudo")
                    .args(["mkdir", "-p", mount_point.to_str().unwrap_or("")])
                    .status();

                if let Ok(status) = mkdir_result {
                    if status.success() {
                        info!("Successfully created directory with sudo");

                        // Set permissions
                        let chown_cmd = format!("{}:{}", user, group);
                        let chown_result = Command::new("sudo")
                            .args(["chown", &chown_cmd, mount_point.to_str().unwrap_or("")])
                            .status();

                        if let Ok(status) = chown_result {
                            if status.success() {
                                info!("Successfully set permissions with sudo");
                                info!("Mount point directory created successfully with sudo");
                                return Ok(());
                            }
                        }
                    }
                }

                // Failed - provide error message
                let error_msg = format!(
                    "\nFailed to create mount point directory: {}\n\nPlease run:\n    sudo mkdir -p {} && sudo chown {}:{} {}\n",
                    e, mount_point.display(), user, group, mount_point.display()
                );

                Err(StorageError::InsufficientPrivileges(error_msg))
            }
        }
    }

    /// Ensure directories exist and are accessible for sudo-based ramdisk creation.
    ///
    /// Similar to ensure_mount_directories but with slightly different checks
    /// for the sudo-based creation path.
    pub fn ensure_sudo_mount_directories(mount_point: &Path) -> Result<(), StorageError> {
        let parent_dir = mount_point.parent().unwrap_or(Path::new("/"));

        // Check parent directory
        let parent_exists = parent_dir.exists();
        let parent_writable = Self::is_writable(parent_dir);

        // Check mount point
        let mount_exists = mount_point.exists();
        let mount_writable = Self::is_writable(mount_point);

        if !parent_exists || !parent_writable || !mount_exists || !mount_writable {
            let (user, group) = Self::get_current_user_group();

            info!("\nSecure code execution requires creating an isolated ramdisk environment.");
            info!("The following directories need to be created:");
            info!(
                "- {}: [exists: {}] [writable: {}]",
                parent_dir.display(),
                if parent_exists { "yes" } else { "no" },
                if parent_writable { "yes" } else { "no" }
            );
            info!(
                "- {}: [exists: {}] [writable: {}]",
                mount_point.display(),
                if mount_exists { "yes" } else { "no" },
                if mount_writable { "yes" } else { "no" }
            );

            info!("This requires elevated privileges to execute the following command:");
            info!(
                "    sudo mkdir -p {} && sudo chown {}:{} {} {}",
                mount_point.display(),
                user,
                group,
                parent_dir.display(),
                mount_point.display()
            );
            info!("This operation provides secure isolation for the code you're about to run.");

            // Try to execute the command with sudo
            let mkdir_result = Command::new("sudo")
                .args(["mkdir", "-p", mount_point.to_str().unwrap_or("")])
                .status();

            if let Ok(status) = mkdir_result {
                if status.success() {
                    info!("Successfully created directory with sudo");

                    // Set permissions on both directories
                    let chown_cmd = format!("{}:{}", user, group);
                    let chown_result = Command::new("sudo")
                        .args([
                            "chown",
                            &chown_cmd,
                            parent_dir.to_str().unwrap_or(""),
                            mount_point.to_str().unwrap_or(""),
                        ])
                        .status();

                    if let Ok(status) = chown_result {
                        if status.success() {
                            info!("Successfully set permissions with sudo");
                            return Ok(());
                        }
                    }
                }
            }

            // Build error message
            let mut error_msg =
                "\nUnable to create secure ramdisk execution environment.\n\n".to_string();

            error_msg.push_str("Directory status:\n");
            error_msg.push_str(&format!(
                "- {}: [exists: {}] [writable: {}]\n",
                parent_dir.display(),
                if parent_exists { "yes" } else { "no" },
                if parent_writable { "yes" } else { "no" }
            ));
            error_msg.push_str(&format!(
                "- {}: [exists: {}] [writable: {}]\n\n",
                mount_point.display(),
                if mount_exists { "yes" } else { "no" },
                if mount_writable { "yes" } else { "no" }
            ));

            error_msg.push_str("The secure execution environment requires a ramdisk mounted at the location above.\n");
            error_msg.push_str("Please run the following command to fix this issue:\n\n");
            error_msg.push_str(&format!(
                "    sudo mkdir -p {} && sudo chown {}:{} {} {}\n\n",
                mount_point.display(),
                user,
                group,
                parent_dir.display(),
                mount_point.display()
            ));

            error_msg.push_str(
                "This command will create the required directories with proper permissions.\n",
            );
            error_msg
                .push_str("After running this command, try executing this application again.\n");

            error!("{}", error_msg);
            return Err(StorageError::InsufficientPrivileges(error_msg));
        }

        Ok(())
    }
}
