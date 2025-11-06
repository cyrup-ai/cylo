use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use log::{error, info};

use crate::config::RamdiskConfig;
use crate::ramdisk::get_watched_dir;

/// Helper function to check if any of the commands exist in path
pub fn find_command<'a>(candidates: &[&'a str]) -> Option<&'a str> {
    for cmd in candidates {
        // First check if it's a direct path we can execute
        if Path::new(cmd).exists() {
            info!("Found executable directly at path: {}", cmd);
            return Some(cmd);
        }

        // Then try to find it in the PATH
        let exists = Command::new("which")
            .arg(cmd)
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false);

        if exists {
            info!("Found executable in PATH: {}", cmd);
            return Some(cmd);
        }
    }
    info!("Could not find any of these executables: {:?}", candidates);
    None
}

// Check if a specific command exists in path
#[allow(dead_code)]
pub fn command_exists(cmd: &str) -> bool {
    find_command(&[cmd]).is_some()
}

/// Get a safe watched directory path that actually exists
pub fn get_safe_watched_dir(config: &RamdiskConfig) -> PathBuf {
    // First try the ramdisk path
    let ramdisk_path = get_watched_dir(config);

    // Check if the ramdisk path exists
    if ramdisk_path.exists() {
        info!("Using ramdisk watched directory at {:?}", ramdisk_path);
        return ramdisk_path;
    }

    // Fall back to local watched_dir if the ramdisk path doesn't exist
    let local_path = PathBuf::from("./watched_dir");

    // Ensure the local watched_dir exists
    if !local_path.exists() {
        match fs::create_dir_all(&local_path) {
            Ok(_) => info!("Created local watched directory at {:?}", local_path),
            Err(e) => error!("Failed to create local watched directory: {}", e),
        }
    } else {
        info!("Using local watched directory at {:?}", local_path);
    }

    local_path
}
