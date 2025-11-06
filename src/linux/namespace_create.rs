use crate::config::RamdiskConfig;
use crate::error::StorageError;
use log::{error, info, warn};
use nix::errno::Errno;
use nix::libc::{chdir, mount, CLONE_NEWNS, CLONE_NEWUSER};
use std::ffi::CString;
use std::fs::{self, File};
use std::io::{self, Write};
use std::os::unix::fs::PermissionsExt;

use super::detection::EnvironmentDetector;
use super::directory::DirectoryManager;
use super::privilege::PrivilegeManager;
use super::sudo_create;

/// Create a ramdisk using unprivileged user namespaces.
///
/// This function attempts to create a secure isolated ramdisk environment
/// without requiring root privileges by using Linux user and mount namespaces.
///
/// The process involves:
/// 1. Creating user and mount namespaces with unshare()
/// 2. Setting up UID/GID mappings for the namespace
/// 3. Mounting a tmpfs ramdisk at the specified mount point
/// 4. Creating and configuring a watched_dir for code execution
///
/// If namespace creation fails (due to kernel restrictions, AppArmor, etc.),
/// this function will attempt various fallback strategies including enabling
/// unprivileged user namespaces and ultimately falling back to sudo-based creation.
pub fn create_with_namespaces(config: &RamdiskConfig) -> Result<(), StorageError> {
    // Check for container/security restrictions
    if EnvironmentDetector::is_in_container() {
        warn!("Detected container environment - namespace operations may be restricted");
    }

    if EnvironmentDetector::is_apparmor_active() {
        warn!("AppArmor is active and may restrict mount operations");
    }

    // Step 1: Try to create a sandbox (user namespace + mount namespace) without privileges
    info!("Attempting to create user and mount namespaces with unshare()");
    if unsafe { nix::libc::unshare(CLONE_NEWUSER | CLONE_NEWNS) } != 0 {
        let errno_val = Errno::last();
        error!("Failed to create namespaces: errno: {:?}", errno_val);

        // Handle specific error conditions
        return handle_namespace_error(errno_val, config);
    }

    // Step 2: Set up UID/GID mappings
    setup_namespace_mappings()?;

    // Step 3: Ensure directories are ready
    DirectoryManager::ensure_mount_directories(&config.mount_point)?;

    // Step 4: Mount the tmpfs
    mount_tmpfs(config)?;

    // Step 5: Change into the ramdisk and create watched_dir
    setup_watched_dir(&config.mount_point)?;

    info!(
        "Ramdisk created and configured successfully at {}",
        config.mount_point.display()
    );
    Ok(())
}

/// Handle errors from namespace creation and attempt recovery strategies.
fn handle_namespace_error(errno_val: Errno, config: &RamdiskConfig) -> Result<(), StorageError> {
    match errno_val {
        Errno::EPERM => {
            info!("Operation not permitted - user namespaces are disabled");
            info!("Attempting to use sudo for ramdisk creation...");

            // Try to enable user namespaces with sudo
            if PrivilegeManager::run_with_sudo("sysctl", &["-w", "kernel.unprivileged_userns_clone=1"])? {
                info!("Successfully enabled unprivileged user namespaces");
                // Try again with the newly enabled setting
                if unsafe { nix::libc::unshare(CLONE_NEWUSER | CLONE_NEWNS) } != 0 {
                    error!("Still failed to create namespaces after enabling unprivileged user namespaces");
                } else {
                    info!("Successfully created namespaces after enabling unprivileged user namespaces");
                    // Continue with setup
                    setup_namespace_mappings()?;
                    DirectoryManager::ensure_mount_directories(&config.mount_point)?;
                    mount_tmpfs(config)?;
                    setup_watched_dir(&config.mount_point)?;
                    return Ok(());
                }
            }

            // Fall back to sudo-based creation
            info!("Attempting to create ramdisk directly with sudo");
            if sudo_create::create_with_sudo(config)? {
                info!("Successfully created ramdisk with sudo");
                return Ok(());
            }

            error!("Could not create ramdisk even with sudo");
            Err(StorageError::InsufficientPrivileges(
                "Could not create ramdisk even with sudo. Secure execution requires ramdisk isolation.".into()
            ))
        }

        Errno::EACCES => {
            error!("Permission denied - AppArmor or seccomp is blocking namespace creation");
            info!("Attempting to configure AppArmor with sudo...");

            if PrivilegeManager::run_with_sudo("aa-complain", &["/usr/bin/cargo"])? {
                info!("Successfully set AppArmor to complain mode");
                // Try again with the new AppArmor setting
                if unsafe { nix::libc::unshare(CLONE_NEWUSER | CLONE_NEWNS) } != 0 {
                    error!("Still failed to create namespaces after configuring AppArmor");
                } else {
                    info!("Successfully created namespaces after configuring AppArmor");
                    // Continue with setup
                    setup_namespace_mappings()?;
                    DirectoryManager::ensure_mount_directories(&config.mount_point)?;
                    mount_tmpfs(config)?;
                    setup_watched_dir(&config.mount_point)?;
                    return Ok(());
                }
            }

            // Fall back to sudo-based creation
            info!("Attempting to create ramdisk directly with sudo");
            if sudo_create::create_with_sudo(config)? {
                return Ok(());
            }

            Err(StorageError::InsufficientPrivileges(
                "AppArmor or seccomp is blocking namespace creation. See logs for solutions.".into()
            ))
        }

        Errno::EINVAL => {
            error!("Invalid argument - this could be due to kernel configuration or nested container");
            error!("Linux kernel 5.11+ is recommended for full namespace support");

            // Try direct mount with sudo
            info!("Attempting to create ramdisk directly with sudo");
            if sudo_create::create_with_sudo(config)? {
                return Ok(());
            }

            Err(StorageError::InsufficientPrivileges(
                "Kernel configuration issue or nested container limitation. Linux 5.11+ recommended.".into()
            ))
        }

        _ => {
            // Try direct mount with sudo as a last resort
            info!("Attempting to create ramdisk directly with sudo");
            if sudo_create::create_with_sudo(config)? {
                return Ok(());
            }

            Err(StorageError::Other(anyhow::anyhow!(
                "Failed to create sandbox: {:?}",
                errno_val
            )))
        }
    }
}

/// Set up UID/GID mappings for the user namespace.
///
/// This tells the kernel to map the current user's UID/GID to root (0)
/// within the namespace, allowing unprivileged mount operations.
fn setup_namespace_mappings() -> Result<(), StorageError> {
    info!("Setting up UID/GID mappings for user namespace");
    let uid = nix::unistd::geteuid().as_raw();
    let gid = nix::unistd::getegid().as_raw();
    let uid_map = format!("0 {} 1", uid);
    let gid_map = format!("0 {} 1", gid);

    match File::create("/proc/self/uid_map").and_then(|mut f| f.write_all(uid_map.as_bytes())) {
        Ok(_) => info!("UID mapping successful: {}", uid_map),
        Err(e) => {
            error!("Failed to set UID mapping: {}", e);
            return Err(StorageError::Other(anyhow::anyhow!(
                "UID map failed: {}",
                e
            )));
        }
    }

    match File::create("/proc/self/setgroups").and_then(|mut f| f.write_all(b"deny")) {
        Ok(_) => info!("Setgroups deny successful"),
        Err(e) => {
            error!("Failed to deny setgroups: {}", e);
            return Err(StorageError::Other(anyhow::anyhow!(
                "Setgroups failed: {}",
                e
            )));
        }
    }

    match File::create("/proc/self/gid_map").and_then(|mut f| f.write_all(gid_map.as_bytes())) {
        Ok(_) => info!("GID mapping successful: {}", gid_map),
        Err(e) => {
            error!("Failed to set GID mapping: {}", e);
            return Err(StorageError::Other(anyhow::anyhow!(
                "GID map failed: {}",
                e
            )));
        }
    }

    Ok(())
}

/// Mount a tmpfs filesystem at the configured mount point.
fn mount_tmpfs(config: &RamdiskConfig) -> Result<(), StorageError> {
    let mount_point = &config.mount_point;

    let mp_cstr = CString::new(mount_point.to_str().unwrap_or(""))
        .map_err(|e| StorageError::PathInvalid(format!("Mount point path contains null byte: {}", e)))?;
    let source = CString::new("none")
        .map_err(|e| StorageError::PathInvalid(format!("Source string contains null byte: {}", e)))?;
    let fstype = CString::new("tmpfs")
        .map_err(|e| StorageError::PathInvalid(format!("Filesystem type contains null byte: {}", e)))?;
    let size = format!("size={}G", config.size_gb);
    let data = CString::new(size.as_str())
        .map_err(|e| StorageError::PathInvalid(format!("Size parameter contains null byte: {}", e)))?;

    info!(
        "Mounting tmpfs with size {} at {}",
        size,
        mount_point.display()
    );

    unsafe {
        if mount(
            source.as_ptr(),
            mp_cstr.as_ptr(),
            fstype.as_ptr(),
            0,
            data.as_ptr() as *const _,
        ) != 0
        {
            let err = io::Error::last_os_error();
            let errno_val = Errno::last();
            error!("Mount failed: {} (errno: {})", err, errno_val);
            return Err(StorageError::CommandFailed(format!(
                "Couldn't mount ramdisk: {}",
                err
            )));
        }
    }

    Ok(())
}

/// Set up the watched_dir inside the ramdisk for code execution.
fn setup_watched_dir(mount_point: &std::path::Path) -> Result<(), StorageError> {
    let mp_cstr = CString::new(mount_point.to_str().unwrap_or(""))
        .map_err(|e| StorageError::PathInvalid(format!("Mount point path contains null byte: {}", e)))?;

    info!("Changing directory to {}", mount_point.display());
    unsafe {
        if chdir(mp_cstr.as_ptr()) != 0 {
            let err = io::Error::last_os_error();
            error!("Failed to chdir to ramdisk: {}", err);
            return Err(StorageError::Other(anyhow::anyhow!(
                "Failed to move into ramdisk: {}",
                err
            )));
        }
    }

    info!("Creating watched_dir inside ramdisk");
    match fs::create_dir("watched_dir") {
        Ok(_) => info!("Created watched_dir successfully"),
        Err(e) => {
            error!("Failed to create watched_dir in ramdisk: {}", e);
            return Err(StorageError::Io(e));
        }
    }

    info!("Setting watched_dir permissions to 0700");
    match fs::set_permissions("watched_dir", fs::Permissions::from_mode(0o700)) {
        Ok(_) => info!("Set watched_dir permissions successfully"),
        Err(e) => {
            error!("Failed to set watched_dir permissions: {}", e);
            return Err(StorageError::Io(e));
        }
    }

    Ok(())
}
