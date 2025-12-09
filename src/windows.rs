//! Native Windows ramdisk implementation using VHD and diskpart
//!
//! This module provides production-quality ramdisk support for Windows using:
//! - Virtual Hard Disk (VHD) files backed by system page file
//! - diskpart.exe for disk management operations
//! - Native Windows APIs for privilege detection and drive management

use std::path::{Path, PathBuf};
use std::process::Command;
use std::io::Write;

use log::{info, warn};

use crate::config::RamdiskConfig;
use crate::error::StorageError;
use crate::platform::RamdiskPlatform;

/// Windows ramdisk implementation using VHD files
pub struct WindowsRamdisk {
    /// Path to the VHD file (stored in temp directory)
    vhd_path: Option<PathBuf>,
    /// Assigned drive letter (e.g., 'Z')
    drive_letter: Option<char>,
}

impl RamdiskPlatform for WindowsRamdisk {
    fn new() -> Self {
        Self {
            vhd_path: None,
            drive_letter: None,
        }
    }

    fn create(&mut self, config: &RamdiskConfig) -> Result<(), StorageError> {
        info!("Creating Windows ramdisk with VHD backend");

        // Check for administrator privileges
        if !check_admin_privileges()? {
            return Err(StorageError::InsufficientPrivileges(
                "Creating ramdisk requires administrator privileges. Please run as administrator.".into()
            ));
        }

        // Determine drive letter to use
        let drive_letter = extract_drive_letter(&config.mount_point)
            .or_else(|| get_available_drive_letter().ok())
            .ok_or_else(|| StorageError::Config(
                "No available drive letters and none specified in mount_point".into()
            ))?;

        info!("Using drive letter: {}", drive_letter);

        // Create VHD file in temp directory
        let temp_vhd = std::env::temp_dir()
            .join(format!("cylo_ramdisk_{}.vhd", uuid::Uuid::new_v4()));

        info!("Creating VHD at: {}", temp_vhd.display());

        // Calculate size in MB
        let size_mb = config.size_gb * 1024;

        // Create and execute diskpart script
        let diskpart_commands = format!(
            "create vdisk file=\"{}\" maximum={} type=expandable\n\
             attach vdisk\n\
             convert mbr\n\
             create partition primary\n\
             format fs=NTFS quick label=\"{}\"\n\
             assign letter={}\n\
             exit",
            temp_vhd.display(),
            size_mb,
            config.volume_name,
            drive_letter
        );

        run_diskpart_script(&diskpart_commands)
            .map_err(|e| StorageError::Other(anyhow::anyhow!(
                "Failed to create VHD ramdisk: {}", e
            )))?;

        // Store state
        self.vhd_path = Some(temp_vhd);
        self.drive_letter = Some(drive_letter);

        // Create cylo subdirectory in the new drive
        let cylo_dir = PathBuf::from(format!("{}:\\cylo", drive_letter));
        std::fs::create_dir_all(&cylo_dir)
            .map_err(|e| StorageError::Io(e))?;

        info!("Windows ramdisk created successfully on drive {}:", drive_letter);

        Ok(())
    }

    fn is_mounted(&self, mount_point: &Path) -> Result<bool, StorageError> {
        use windows::Win32::Storage::FileSystem::GetDriveTypeW;
        use windows::core::PCWSTR;

        // Extract drive letter from mount_point
        let drive_letter = extract_drive_letter(mount_point)
            .ok_or_else(|| StorageError::PathInvalid(
                format!("Invalid Windows path format: {}", mount_point.display())
            ))?;

        // Build drive root path (e.g., "Z:\")
        let drive_path = format!("{}:\\", drive_letter);
        let wide_path: Vec<u16> = drive_path.encode_utf16().chain(Some(0)).collect();

        // Check drive type
        unsafe {
            let drive_type = GetDriveTypeW(PCWSTR(wide_path.as_ptr()));
            // DRIVE_FIXED (3) or DRIVE_RAMDISK (6) indicates mounted
            // DRIVE_NO_ROOT_DIR (1) indicates not mounted
            Ok(drive_type >= 3)
        }
    }

    fn remove(&self, mount_point: &Path) -> Result<(), StorageError> {
        // Verify this is the correct mount point before removing
        if !self.is_mounted(mount_point)? {
            warn!("Mount point {} is not mounted, skipping removal", mount_point.display());
            return Ok(());
        }

        if let Some(ref vhd_path) = self.vhd_path {
            info!("Removing Windows ramdisk: {}", vhd_path.display());

            // Detach VHD via diskpart
            let diskpart_commands = format!(
                "select vdisk file=\"{}\"\n\
                 detach vdisk\n\
                 exit",
                vhd_path.display()
            );

            if let Err(e) = run_diskpart_script(&diskpart_commands) {
                warn!("Failed to detach VHD via diskpart: {}", e);
                // Continue to file deletion attempt
            }

            // Delete the VHD file
            if let Err(e) = std::fs::remove_file(vhd_path) {
                warn!("Failed to delete VHD file: {}", e);
                // Non-fatal - file may be locked or already deleted
            } else {
                info!("VHD file deleted successfully");
            }

            Ok(())
        } else {
            warn!("No VHD path stored - cannot remove ramdisk");
            Ok(())
        }
    }
}

/// Execute a diskpart script with the given commands
///
/// # Arguments
/// * `script` - Multi-line diskpart script with one command per line
///
/// # Returns
/// Ok(()) if diskpart executed successfully, error otherwise
fn run_diskpart_script(script: &str) -> Result<(), StorageError> {
    // Create temporary script file
    let script_path = std::env::temp_dir()
        .join(format!("cylo_diskpart_{}.txt", uuid::Uuid::new_v4()));

    // Write diskpart commands to file
    {
        let mut file = std::fs::File::create(&script_path)
            .map_err(StorageError::Io)?;
        file.write_all(script.as_bytes())
            .map_err(StorageError::Io)?;
    }

    info!("Running diskpart script: {}", script_path.display());

    // Execute diskpart
    let output = Command::new("diskpart")
        .arg("/s")
        .arg(&script_path)
        .output()
        .map_err(|e| StorageError::CommandFailed(
            format!("Failed to execute diskpart: {}", e)
        ))?;

    // Clean up script file
    let _ = std::fs::remove_file(&script_path);

    // Check if diskpart succeeded
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(StorageError::CommandFailed(
            format!("diskpart failed: {}", stderr)
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    info!("diskpart output: {}", stdout);

    Ok(())
}

/// Extract drive letter from a Windows path
///
/// # Arguments
/// * `path` - Path that may contain a drive letter
///
/// # Returns
/// Some(char) if drive letter found (e.g., 'C'), None otherwise
///
/// # Examples
/// - "C:\\cylo" -> Some('C')
/// - "X:/data" -> Some('X')
/// - "/ephemeral/cylo" -> None
fn extract_drive_letter(path: &Path) -> Option<char> {
    let path_str = path.to_str()?;

    if path_str.len() >= 2 {
        let first_char = path_str.chars().next()?;
        let second_char = path_str.chars().nth(1)?;

        if first_char.is_ascii_alphabetic() && (second_char == ':' || second_char == '/') {
            return Some(first_char.to_ascii_uppercase());
        }
    }

    None
}

/// Find an available (unused) drive letter
///
/// # Returns
/// Ok(char) with available drive letter, or error if none available
///
/// # Note
/// Checks drives from Z to D (reserves A, B, C for system use)
fn get_available_drive_letter() -> Result<char, StorageError> {
    use windows::Win32::Storage::FileSystem::GetLogicalDrives;

    unsafe {
        let drives = GetLogicalDrives();

        // Check Z to D (reserve A, B, C for floppy/system)
        for letter in (b'D'..=b'Z').rev() {
            let bit = 1 << (letter - b'A');
            if drives & bit == 0 {
                return Ok(letter as char);
            }
        }
    }

    Err(StorageError::Other(anyhow::anyhow!(
        "No available drive letters (D-Z all in use)"
    )))
}

/// Check if current process has administrator privileges
///
/// # Returns
/// Ok(true) if running as administrator, Ok(false) otherwise
fn check_admin_privileges() -> Result<bool, StorageError> {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::{
        GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    unsafe {
        let mut token = HANDLE::default();
        let process = GetCurrentProcess();

        // Open process token
        if OpenProcessToken(process, TOKEN_QUERY, &mut token).is_err() {
            return Ok(false);
        }

        // Query elevation information
        let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut return_length = 0u32;

        let result = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut std::ffi::c_void),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut return_length,
        );

        CloseHandle(token).ok();

        Ok(result.is_ok() && elevation.TokenIsElevated != 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_drive_letter_windows_style() {
        let path = Path::new("C:\\Users\\test");
        assert_eq!(extract_drive_letter(path), Some('C'));

        let path = Path::new("Z:\\data");
        assert_eq!(extract_drive_letter(path), Some('Z'));
    }

    #[test]
    fn test_extract_drive_letter_forward_slash() {
        let path = Path::new("X:/cylo");
        assert_eq!(extract_drive_letter(path), Some('X'));
    }

    #[test]
    fn test_extract_drive_letter_unix_style() {
        let path = Path::new("/ephemeral/cylo");
        assert_eq!(extract_drive_letter(path), None);
    }

    #[test]
    fn test_extract_drive_letter_relative() {
        let path = Path::new("relative/path");
        assert_eq!(extract_drive_letter(path), None);
    }
}
