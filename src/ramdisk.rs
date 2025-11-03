#[cfg(not(any(target_os = "macos", target_os = "linux")))]
use std::env;
use std::path::{Path, PathBuf};

use log::{info, warn};

#[cfg(target_os = "linux")]
use crate::linux::LinuxRamdisk;
#[cfg(target_os = "macos")]
use crate::macos::MacosRamdisk;
use crate::{config::RamdiskConfig, error::StorageError, platform::RamdiskPlatform};

/// Returns the path to the watched directory within the ramdisk
pub fn get_watched_dir(config: &RamdiskConfig) -> PathBuf {
    PathBuf::from(&config.mount_point).join("watched_dir")
}

/// Creates a ramdisk with the specified configuration
///
/// # Arguments
/// * `config` - Configuration specifying size, mount point, and other options
///
/// # Returns
/// * `Ok(())` if successful
/// * `Err(StorageError)` if creation fails
pub fn create_ramdisk(config: &RamdiskConfig) -> Result<(), StorageError> {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        let mut platform = get_platform_impl()?;
        platform.create(config)
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    Err(StorageError::UnsupportedOs(env::consts::OS.to_string()))
}

/// Removes a ramdisk at the specified mount point
///
/// # Arguments
/// * `mount_point` - Path where the ramdisk is mounted
///
/// # Returns
/// * `Ok(())` if successful
/// * `Err(StorageError)` if removal fails
pub fn remove_ramdisk(mount_point: &Path) -> Result<(), StorageError> {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        let platform = get_platform_impl()?;
        platform.remove(mount_point)
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    Err(StorageError::UnsupportedOs(env::consts::OS.to_string()))
}

/// Checks if a path is mounted as a ramdisk
///
/// # Arguments
/// * `mount_point` - Path to check
///
/// # Returns
/// * `Ok(true)` if path is mounted as a ramdisk
/// * `Ok(false)` if not
/// * `Err(StorageError)` if there's an error checking
pub fn is_mounted(mount_point: &Path) -> Result<bool, StorageError> {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        let platform = get_platform_impl()?;
        platform.is_mounted(mount_point)
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    Err(StorageError::UnsupportedOs(env::consts::OS.to_string()))
}

/// Creates a secure execution environment
///
/// This function performs the following steps:
/// 1. Creates the watched_dir in the current directory
/// 2. Attempts to create a ramdisk at the configured mount point (if possible)
///    - Will prompt for sudo access if needed and user allows it
/// 3. Applies Landlock restrictions to the watched_dir
///
/// The ramdisk is optional - if it can't be created, we'll still use Landlock
/// to restrict file operations to the watched_dir.
///
/// # Arguments
/// * `config` - Configuration for the ramdisk
///
/// # Returns
/// * `Ok(())` if successful
/// * `Err(StorageError)` if creation fails
pub fn create_secure_ramdisk(config: &RamdiskConfig) -> Result<(), StorageError> {
    let watched_dir = get_watched_dir(config);
    let mut ramdisk_created = false;

    #[cfg(target_os = "linux")]
    {
        info!("Attempting to create secure ramdisk with Linux-specific implementation");
        info!("This may prompt for sudo access if needed for optimal security");

        match crate::linux::LinuxRamdisk::create(config) {
            Ok(_) => {
                info!(
                    "Successfully created ramdisk at {}",
                    config.mount_point.display()
                );
                ramdisk_created = true;
            }
            Err(e) => {
                warn!(
                    "Could not create ramdisk: {}. Falling back to local dir.",
                    e
                );
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        let mut platform = crate::macos::MacosRamdisk::new();
        match platform.create(config) {
            Ok(_) => {
                info!(
                    "Successfully created ramdisk at {}",
                    config.mount_point.display()
                );
                ramdisk_created = true;
            }
            Err(e) => {
                warn!(
                    "Could not create ramdisk: {}. Falling back to local dir.",
                    e
                );
            }
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        warn!("Ramdisk not supported on this OS: {}", std::env::consts::OS);
        warn!("Using local directory instead");
    }

    if !ramdisk_created {
        std::fs::create_dir_all(&watched_dir).map_err(StorageError::Io)?;
        info!(
            "Created fallback watched directory at {}",
            watched_dir.display()
        );
    } else {
        info!(
            "Using ramdisk watched directory at {}",
            watched_dir.display()
        );
    }

    let jail_config = crate::jail::JailConfig {
        allowed_dir: watched_dir,
        enable_landlock: config.landlock_enabled,
        check_apparmor: config.check_apparmor,
    };

    if let Err(e) = crate::jail::init_jail(&jail_config) {
        warn!("Landlock failed: {}. Security may be reduced.", e);
    }

    Ok(())
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn get_platform_impl() -> Result<impl RamdiskPlatform, StorageError> {
    #[cfg(target_os = "linux")]
    {
        Ok(LinuxRamdisk::new())
    }

    #[cfg(target_os = "macos")]
    {
        Ok(MacosRamdisk::new())
    }
}
