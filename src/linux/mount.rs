use crate::error::StorageError;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

/// Utilities for detecting and managing mount points.
pub struct MountDetector;

impl MountDetector {
    /// Get a list of all mounted filesystems from the `mount` command.
    pub fn get_mounted_filesystems() -> Result<Vec<String>, StorageError> {
        let output = std::process::Command::new("mount")
            .output()
            .map_err(|e| StorageError::CommandFailed(format!("Failed to get mount info: {}", e)))?;
        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|s| s.to_string())
            .collect())
    }

    /// Check if a path is a mount point by comparing device IDs.
    ///
    /// A directory is a mount point if its device ID differs from its parent's device ID.
    pub fn is_mount_point(path: &Path) -> Result<bool, StorageError> {
        if !path.exists() {
            return Ok(false);
        }
        let metadata = fs::metadata(path).map_err(StorageError::Io)?;
        let parent_metadata =
            fs::metadata(path.parent().unwrap_or(Path::new("/"))).map_err(StorageError::Io)?;
        Ok(metadata.dev() != parent_metadata.dev())
    }
}
