use crate::error::StorageError;
use crate::sandbox::safe_path_to_string;
use log::{error, info};
use std::path::Path;
use std::process::Command;
use std::{fs};

mod detection;
mod directory;
mod mount;
mod namespace_create;
mod privilege;
mod sudo_create;

pub use detection::EnvironmentDetector;
pub use directory::DirectoryManager;
pub use mount::MountDetector;
pub use privilege::PrivilegeManager;

/// Linux-specific ramdisk implementation using tmpfs and Linux namespaces.
///
/// This implementation provides secure, isolated ramdisk storage for code execution
/// on Linux systems. It attempts to use unprivileged user namespaces first for
/// maximum security and portability, falling back to sudo-based creation when necessary.
pub struct LinuxRamdisk;

impl LinuxRamdisk {
    /// Create a new LinuxRamdisk instance.
    pub fn new() -> Self {
        Self
    }

    /// Create a ramdisk with the given configuration.
    ///
    /// This is the main entry point for ramdisk creation. It delegates to
    /// namespace_create::create_with_namespaces which handles all the logic
    /// including fallbacks to sudo-based creation if needed.
    pub fn create(config: &crate::config::RamdiskConfig) -> Result<(), StorageError> {
        namespace_create::create_with_namespaces(config)
    }
}

impl crate::platform::RamdiskPlatform for LinuxRamdisk {
    fn new() -> Self {
        LinuxRamdisk
    }

    fn is_mounted(&self, mount_point: &Path) -> Result<bool, StorageError> {
        if !mount_point.exists() {
            return Ok(false);
        }

        let mounts = MountDetector::get_mounted_filesystems()?;
        let mount_point_str = safe_path_to_string(mount_point)
            .map_err(|e| StorageError::PathInvalid(e.to_string()))?;

        Ok(mounts.iter().any(|m| m.contains(&mount_point_str))
            || MountDetector::is_mount_point(mount_point)?)
    }

    fn create(&mut self, config: &crate::config::RamdiskConfig) -> Result<(), StorageError> {
        Self::create(config)
    }

    fn remove(&self, mount_point: &Path) -> Result<(), StorageError> {
        let mount_point_str = safe_path_to_string(mount_point)
            .map_err(|e| StorageError::PathInvalid(e.to_string()))?;
        info!("Attempting to unmount {}", mount_point_str);

        // First try without sudo
        let status = Command::new("umount").arg(mount_point).status();

        let unmount_success = match status {
            Ok(status) => status.success(),
            Err(_) => false,
        };

        // If that fails, try with sudo
        if !unmount_success {
            info!("Regular unmount failed, trying with sudo");
            let sudo_result = PrivilegeManager::run_with_sudo("umount", &[&mount_point_str])?;

            if !sudo_result {
                error!("Unmount command failed even with sudo");
                return Err(StorageError::CommandFailed(
                    "Failed to unmount ramdisk".to_string(),
                ));
            }
        }

        info!(
            "Successfully unmounted {}, cleaning up directory",
            mount_point.display()
        );
        fs::remove_dir_all(mount_point).map_err(|e| {
            error!("Failed to remove ramdisk directory: {}", e);
            StorageError::Io(e)
        })?;

        info!("Ramdisk removal completed successfully");
        Ok(())
    }
}
