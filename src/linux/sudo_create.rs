use crate::config::RamdiskConfig;
use crate::error::StorageError;
use log::{error, info};
use std::fs;
use std::os::unix::fs::PermissionsExt;

use super::directory::DirectoryManager;
use super::privilege::PrivilegeManager;

/// Create a ramdisk using sudo for privileged operations.
///
/// This is a fallback method when unprivileged user namespace creation
/// is not available or fails. It requires sudo access to:
/// 1. Create necessary directories
/// 2. Mount the tmpfs filesystem
/// 3. Set up the watched_dir for code execution
///
/// Returns Ok(true) if successful, Ok(false) if it failed but should
/// continue trying other methods, or Err for hard failures.
pub fn create_with_sudo(config: &RamdiskConfig) -> Result<bool, StorageError> {
    let mount_point = &config.mount_point;

    // Ensure directories exist with proper permissions
    DirectoryManager::ensure_sudo_mount_directories(mount_point)?;

    info!("Creating mount point at {}", mount_point.display());

    // Mount the tmpfs with sudo
    let size_arg = format!("size={}G", config.size_gb);
    let mount_result = PrivilegeManager::run_with_sudo(
        "mount",
        &[
            "-t",
            "tmpfs",
            "-o",
            &size_arg,
            "none",
            mount_point.to_str().unwrap_or(""),
        ],
    )?;

    if !mount_result {
        error!("Failed to mount tmpfs with sudo");
        return Ok(false);
    }

    // Create watched_dir inside the ramdisk
    setup_watched_dir_with_sudo(mount_point)?;

    info!(
        "Ramdisk created and configured successfully with sudo at {}",
        config.mount_point.display()
    );
    Ok(true)
}

/// Set up the watched_dir inside the ramdisk.
///
/// Creates the directory and sets appropriate permissions for secure code execution.
fn setup_watched_dir_with_sudo(mount_point: &std::path::Path) -> Result<(), StorageError> {
    let watched_dir = mount_point.join("watched_dir");

    info!("Creating watched_dir inside ramdisk");
    match fs::create_dir(&watched_dir) {
        Ok(_) => info!("Created watched_dir successfully"),
        Err(e) => {
            error!("Failed to create watched_dir in ramdisk: {}", e);
            // Try to unmount since we failed
            let _ = PrivilegeManager::run_with_sudo("umount", &[mount_point.to_str().unwrap_or("")]);
            return Err(StorageError::Io(e));
        }
    }

    // Set permissions on watched_dir
    info!("Setting watched_dir permissions to 0700");
    match fs::set_permissions(&watched_dir, fs::Permissions::from_mode(0o700)) {
        Ok(_) => info!("Set watched_dir permissions successfully"),
        Err(e) => {
            error!("Failed to set watched_dir permissions: {}", e);
            // Continue anyway, this is not critical
        }
    }

    Ok(())
}
