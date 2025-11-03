use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::StorageError;

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum FileSystem {
    APFS,
    HFSPlus,
}

#[cfg(not(target_os = "macos"))]
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum FileSystem {
    Ext4,
    Tmpfs,
}

impl std::fmt::Display for FileSystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            #[cfg(target_os = "macos")]
            Self::APFS => write!(f, "APFS"),
            #[cfg(target_os = "macos")]
            Self::HFSPlus => write!(f, "HFS+"),
            #[cfg(not(target_os = "macos"))]
            Self::Ext4 => write!(f, "Ext4"),
            #[cfg(not(target_os = "macos"))]
            Self::Tmpfs => write!(f, "Tmpfs"),
        }
    }
}

/// Configuration for the ramdisk
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RamdiskConfig {
    /// Whether to use a ramdisk for execution (recommended)
    pub use_ramdisk: bool,

    /// Size of ramdisk in gigabytes
    pub size_gb: u64,

    /// Mount point (path to mount the ramdisk)
    pub mount_point: PathBuf,

    /// Volume name for the ramdisk
    pub volume_name: String,

    /// Whether to enable Landlock restrictions
    pub landlock_enabled: bool,

    /// Whether to check for AppArmor restrictions
    pub check_apparmor: bool,

    #[cfg(target_os = "macos")]
    /// File system to use for macOS
    pub filesystem: FileSystem,
}

/// Default implementation for RamdiskConfig
impl Default for RamdiskConfig {
    fn default() -> Self {
        Self {
            use_ramdisk: true,
            size_gb: 1, // 1GB by default
            mount_point: PathBuf::from("/ephemeral/cylo"),
            volume_name: "IRunExecRAM".to_string(),
            landlock_enabled: true,
            check_apparmor: true,
            #[cfg(target_os = "macos")]
            filesystem: FileSystem::APFS,
        }
    }
}

impl TryFrom<&str> for RamdiskConfig {
    type Error = StorageError;

    fn try_from(s: &str) -> Result<Self, StorageError> {
        let parts: Vec<&str> = s.split(',').collect();
        if parts.len() < 3 {
            return Err(StorageError::Config(
                "Expected format: size_gb,mount_point,volume_name[,filesystem]".into(),
            ));
        }

        let size_gb = parts[0]
            .parse()
            .map_err(|_| StorageError::Config("Invalid size_gb value".into()))?;

        #[cfg(target_os = "macos")]
        let filesystem = if parts.len() > 3 {
            match parts[3] {
                "APFS" => FileSystem::APFS,
                "HFSPlus" => FileSystem::HFSPlus,
                _ => FileSystem::APFS,
            }
        } else {
            FileSystem::APFS
        };

        #[cfg(not(target_os = "macos"))]
        let _filesystem = if parts.len() > 3 {
            match parts[3] {
                "Ext4" => FileSystem::Ext4,
                "Tmpfs" => FileSystem::Tmpfs,
                _ => FileSystem::Tmpfs,
            }
        } else {
            FileSystem::Tmpfs
        };

        Ok(Self {
            use_ramdisk: true, // Add default value
            size_gb,
            mount_point: PathBuf::from(parts[1]),
            volume_name: parts[2].to_string(),
            landlock_enabled: true, // Add default value
            check_apparmor: true,   // Add default value
            #[cfg(target_os = "macos")]
            filesystem,
        })
    }
}
