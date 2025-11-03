use std::{fs, path::Path, process::Command};

use crate::{
    config::{FileSystem, RamdiskConfig},
    error::StorageError,
    platform::RamdiskPlatform,
    sandbox::safe_path_to_string,
};

/// Implements ramdisk functionality for macOS systems using hdiutil and diskutil
pub struct MacosRamdisk;

impl MacosRamdisk {
    /// Gets a list of all currently mounted volumes by running the 'mount' command
    fn get_mounted_volumes(&self) -> Result<Vec<String>, StorageError> {
        let output = Command::new("mount")
            .output()
            .map_err(|e| StorageError::CommandFailed(format!("Failed to get mount info: {e}")))?;

        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|s| s.to_string())
            .collect())
    }

    /// Attaches a new ramdisk device of the specified size using hdiutil
    /// Returns the device path on success
    fn attach_disk(&mut self, size_sectors: u64) -> Result<String, StorageError> {
        let output = Command::new("hdiutil")
            .args(["attach", "-nomount", &format!("ram://{size_sectors}")])
            .output()
            .map_err(|e| StorageError::CommandFailed(format!("hdiutil failed: {e}")))?;

        if !output.status.success() {
            return Err(StorageError::CommandFailed(
                String::from_utf8_lossy(&output.stderr).to_string(),
            ));
        }

        let device = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(device)
    }

    /// Formats the attached device with the specified filesystem type and volume name
    /// Will attempt to detach device on failure
    fn format_disk(&self, device: &str, config: &RamdiskConfig) -> Result<(), StorageError> {
        let fs_type = match config.filesystem {
            FileSystem::APFS => "APFS",
            FileSystem::HFSPlus => "HFS+",
        };

        let output = Command::new("diskutil")
            .args(["erasevolume", fs_type, &config.volume_name, device])
            .output()
            .map_err(|e| StorageError::CommandFailed(format!("diskutil format failed: {e}")))?;

        if !output.status.success() {
            // Attempt to detach the device since format failed
            let _ = Command::new("hdiutil").args(["detach", device]).output();

            return Err(StorageError::PartialFailure(format!(
                "Failed to format disk: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        Ok(())
    }

    /// Gets the device path associated with a mount point using diskutil info
    fn get_device_for_mount(&self, mount_point: &str) -> Result<String, StorageError> {
        let output = Command::new("diskutil")
            .args(["info", mount_point])
            .output()
            .map_err(|e| StorageError::CommandFailed(format!("diskutil info failed: {e}")))?;

        if !output.status.success() {
            return Err(StorageError::CommandFailed(
                String::from_utf8_lossy(&output.stderr).to_string(),
            ));
        }

        let info = String::from_utf8_lossy(&output.stdout);
        info.lines()
            .find(|line| line.contains("Device Node:"))
            .and_then(|line| line.split(':').nth(1))
            .map(|s| s.trim().to_string())
            .ok_or_else(|| {
                StorageError::CommandFailed(format!(
                    "Could not find device for mount point {mount_point}"
                ))
            })
    }
}

impl RamdiskPlatform for MacosRamdisk {
    /// Creates a new MacosRamdisk instance
    fn new() -> Self {
        Self
    }

    /// Checks if a mount point is currently in use
    fn is_mounted(&self, mount_point: &Path) -> Result<bool, StorageError> {
        let mount_point_str = safe_path_to_string(mount_point)
            .map_err(|e| StorageError::PathInvalid(e.to_string()))?;
        let volumes = self.get_mounted_volumes()?;
        Ok(volumes.iter().any(|v| v.contains(&mount_point_str)))
    }

    /// Creates a new ramdisk with the specified configuration
    /// 1. Verifies mount point is not in use
    /// 2. Creates mount point directory if needed
    /// 3. Attaches and formats the ramdisk device
    fn create(&mut self, config: &RamdiskConfig) -> Result<(), StorageError> {
        if self.is_mounted(&config.mount_point)? {
            return Err(StorageError::AlreadyMounted(config.mount_point.clone()));
        }

        // Calculate size in 512-byte sectors
        let size_sectors = config.size_gb * 1024 * 1024 * 2;

        // Create mount point if needed
        if !config.mount_point.exists() {
            fs::create_dir_all(&config.mount_point)?;
        }

        // Attach and format the disk
        let device = self.attach_disk(size_sectors)?;
        self.format_disk(&device, config)?;

        Ok(())
    }

    /// Removes an existing ramdisk:
    /// 1. Unmounts the filesystem
    /// 2. Detaches the device
    /// 3. Optionally removes the empty mount point directory
    fn remove(&self, mount_point: &Path) -> Result<(), StorageError> {
        if !self.is_mounted(mount_point)? {
            return Ok(());
        }

        let mount_point_str = safe_path_to_string(mount_point)
            .map_err(|e| StorageError::PathInvalid(e.to_string()))?;
        let device = self.get_device_for_mount(&mount_point_str)?;

        // First unmount
        let unmount_output = Command::new("diskutil")
            .args(["unmountDisk", &device])
            .output()
            .map_err(|e| StorageError::CommandFailed(format!("unmount failed: {e}")))?;

        if !unmount_output.status.success() {
            return Err(StorageError::CommandFailed(
                String::from_utf8_lossy(&unmount_output.stderr).to_string(),
            ));
        }

        // Then detach
        let detach_output = Command::new("hdiutil")
            .args(["detach", &device])
            .output()
            .map_err(|e| StorageError::CommandFailed(format!("detach failed: {e}")))?;

        if !detach_output.status.success() {
            return Err(StorageError::CommandFailed(
                String::from_utf8_lossy(&detach_output.stderr).to_string(),
            ));
        }

        // Remove mount point if empty
        if mount_point.exists() && fs::read_dir(mount_point)?.next().is_none() {
            fs::remove_dir(mount_point)?;
        }

        Ok(())
    }
}
