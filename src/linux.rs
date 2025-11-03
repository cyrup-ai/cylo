use crate::error::StorageError;
use nix::errno::Errno;
use nix::libc::{chdir, mount, CLONE_NEWNS, CLONE_NEWUSER};
use std::ffi::CString;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use log::{error, info, warn};

use crate::sandbox::safe_path_to_string;

pub struct LinuxRamdisk;

impl LinuxRamdisk {
    // Try to run a command with sudo if available, otherwise try without sudo
    fn run_with_sudo(cmd: &str, args: &[&str]) -> Result<bool, StorageError> {
        // First try running without sudo
        info!("Attempting to run '{}' without sudo first", cmd);
        let result = Command::new(cmd).args(args).output();

        if let Ok(output) = result {
            if output.status.success() {
                info!("Command succeeded without sudo");
                return Ok(true);
            }
        }

        // Format the full command for logging/display
        let full_cmd = format!("{} {}", cmd, args.join(" "));

        // Check if we can use sudo non-interactively
        info!("Checking if sudo is available non-interactively");
        let sudo_check = Command::new("sudo")
            .arg("-n") // Non-interactive check
            .arg("true")
            .status();

        let sudo_available = sudo_check.map(|s| s.success()).unwrap_or(false);

        if sudo_available {
            // Try the command with sudo non-interactively
            info!("Sudo is available, trying command with sudo");
            let sudo_result = Command::new("sudo")
                .arg("-n") // Non-interactive mode
                .arg(cmd)
                .args(args)
                .output();

            match sudo_result {
                Ok(output) => {
                    if output.status.success() {
                        info!("Successfully executed command with sudo: {}", full_cmd);
                        return Ok(true);
                    } else {
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        warn!("Command failed with sudo: {}", stderr);
                    }
                }
                Err(e) => {
                    warn!("Failed to execute command with sudo: {}", e);
                }
            }
        } else {
            // Going to need an interactive sudo prompt
            info!("\nSecure code execution requires creating an isolated ramdisk environment.");
            info!("This requires elevated privileges to execute the following command:");
            info!("    sudo {}", full_cmd);
            info!("This operation provides secure isolation for the code you're about to run.");

            // Try with interactive sudo
            let sudo_interactive = Command::new("sudo").arg(cmd).args(args).status();

            match sudo_interactive {
                Ok(status) => {
                    if status.success() {
                        info!("Successfully executed command with sudo");
                        return Ok(true);
                    } else {
                        warn!("Command failed with interactive sudo");
                    }
                }
                Err(e) => {
                    warn!("Failed to execute command with interactive sudo: {}", e);
                }
            }
        }

        // If everything failed, try one more time as the current user
        info!("Trying alternative approach without elevated privileges");
        let fallback_result = Command::new(cmd).args(args).output();

        match fallback_result {
            Ok(output) => {
                if output.status.success() {
                    info!("Command succeeded in fallback mode");
                    Ok(true)
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    warn!("Fallback command also failed: {}", stderr);
                    Ok(false)
                }
            }
            Err(e) => {
                warn!("Fallback command error: {}", e);
                Ok(false) // Return false instead of error to allow graceful fallback
            }
        }
    }

    pub fn new() -> Self {
        Self
    }

    // This function makes a ramdisk, trying unprivileged first, then with sudo if needed
    pub fn create(config: &crate::config::RamdiskConfig) -> Result<(), StorageError> {
        // Check if we're running inside a container (e.g., Docker) which might restrict namespace operations
        if Self::is_in_container() {
            warn!("Detected container environment - namespace operations may be restricted");
        }

        // Check for AppArmor restrictions
        if Self::is_apparmor_active() {
            warn!("AppArmor is active and may restrict mount operations");
        }

        // Step 1: Try to create a sandbox (user namespace + mount namespace) without privileges
        info!("Attempting to create user and mount namespaces with unshare()");
        if unsafe { nix::libc::unshare(CLONE_NEWUSER | CLONE_NEWNS) } != 0 {
            let errno_val = Errno::last();
            error!("Failed to create namespaces: errno: {:?}", errno_val);

            // Check specific error conditions
            if errno_val == Errno::EPERM {
                info!("Operation not permitted - user namespaces are disabled");
                info!("Attempting to use sudo for ramdisk creation...");

                // Try to enable user namespaces with sudo
                if Self::run_with_sudo("sysctl", &["-w", "kernel.unprivileged_userns_clone=1"])? {
                    info!("Successfully enabled unprivileged user namespaces");
                    // Try again with the newly enabled setting
                    if unsafe { nix::libc::unshare(CLONE_NEWUSER | CLONE_NEWNS) } != 0 {
                        error!("Still failed to create namespaces after enabling unprivileged user namespaces");
                    } else {
                        info!("Successfully created namespaces after enabling unprivileged user namespaces");
                        // Continue with the rest of the function
                    }
                } else {
                    // Try direct mount with sudo
                    info!("Attempting to create ramdisk directly with sudo");
                    if Self::create_with_sudo(config)? {
                        info!("Successfully created ramdisk with sudo");
                        return Ok(());
                    }

                    error!("Could not create ramdisk even with sudo");
                    return Err(StorageError::InsufficientPrivileges(
                        "Could not create ramdisk even with sudo. Secure execution requires ramdisk isolation.".into()
                    ));
                }
            } else if errno_val == Errno::EACCES {
                error!("Permission denied - AppArmor or seccomp is blocking namespace creation");
                info!("Attempting to configure AppArmor with sudo...");

                if Self::run_with_sudo("aa-complain", &["/usr/bin/cargo"])? {
                    info!("Successfully set AppArmor to complain mode");
                    // Try again with the new AppArmor setting
                    if unsafe { nix::libc::unshare(CLONE_NEWUSER | CLONE_NEWNS) } != 0 {
                        error!("Still failed to create namespaces after configuring AppArmor");
                    } else {
                        info!("Successfully created namespaces after configuring AppArmor");
                        // Continue with the rest of the function
                    }
                } else {
                    // Try direct mount with sudo
                    info!("Attempting to create ramdisk directly with sudo");
                    if Self::create_with_sudo(config)? {
                        return Ok(());
                    }

                    return Err(StorageError::InsufficientPrivileges(
                        "AppArmor or seccomp is blocking namespace creation. See logs for solutions.".into()
                    ));
                }
            } else if errno_val == Errno::EINVAL {
                error!("Invalid argument - this could be due to kernel configuration or nested container");
                error!("Linux kernel 5.11+ is recommended for full namespace support");

                // Try direct mount with sudo
                info!("Attempting to create ramdisk directly with sudo");
                if Self::create_with_sudo(config)? {
                    return Ok(());
                }

                return Err(StorageError::InsufficientPrivileges(
                    "Kernel configuration issue or nested container limitation. Linux 5.11+ recommended.".into()
                ));
            } else {
                // Try direct mount with sudo as a last resort
                info!("Attempting to create ramdisk directly with sudo");
                if Self::create_with_sudo(config)? {
                    return Ok(());
                }

                return Err(StorageError::Other(anyhow::anyhow!(
                    "Failed to create sandbox: {:?}",
                    errno_val
                )));
            }
        }

        // Step 2: Tell the system "I'm root in this sandbox"
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

        // Step 3: Make the ramdisk mount point and mount it
        let mount_point = &config.mount_point;
        let parent_dir = mount_point.parent().unwrap_or(Path::new("/"));

        // Check parent directory first
        let parent_exists = parent_dir.exists();
        let parent_writable = parent_exists
            && fs::metadata(parent_dir)
                .map(|m| m.permissions().mode() & 0o200 != 0)
                .unwrap_or(false);

        // Check mount point
        let mount_exists = mount_point.exists();

        if !parent_exists || !parent_writable {
            // Get current directory owner and group
            let current_dir_path = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let (user, group) = match fs::metadata(&current_dir_path) {
                Ok(metadata) => {
                    let uid = metadata.uid();
                    let gid = metadata.gid();

                    // Try to get user and group names
                    let username = match std::process::Command::new("id")
                        .args(["-nu", &uid.to_string()])
                        .output()
                    {
                        Ok(output) => String::from_utf8_lossy(&output.stdout).trim().to_string(),
                        Err(_) => format!("{}", uid)};

                    let groupname = match std::process::Command::new("id")
                        .args(["-ng", &gid.to_string()])
                        .output()
                    {
                        Ok(output) => String::from_utf8_lossy(&output.stdout).trim().to_string(),
                        Err(_) => format!("{}", gid)};

                    (username, groupname)
                }
                Err(_) => ("$USER".to_string(), "$GROUP".to_string())};

            // Try to create the directories ourselves with sudo
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

            // Try to execute the command ourselves
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
                            // Directory is now ready, continue with execution
                            return Ok(());
                        }
                    }
                }
            }

            // If we reach here, the sudo attempt failed
            // Build a clear error message
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

        info!("Creating mount point at {}", mount_point.display());
        if !mount_exists {
            match fs::create_dir_all(mount_point) {
                Ok(_) => info!("Mount point directory created successfully"),
                Err(e) => {
                    error!("Failed to create mount point directory: {}", e);

                    // Get current directory owner and group
                    let current_dir_path =
                        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                    let (user, group) = match fs::metadata(&current_dir_path) {
                        Ok(metadata) => {
                            let uid = metadata.uid();
                            let gid = metadata.gid();

                            // Try to get user and group names
                            let username = match std::process::Command::new("id")
                                .args(["-nu", &uid.to_string()])
                                .output()
                            {
                                Ok(output) => {
                                    String::from_utf8_lossy(&output.stdout).trim().to_string()
                                }
                                Err(_) => format!("{}", uid)};

                            let groupname = match std::process::Command::new("id")
                                .args(["-ng", &gid.to_string()])
                                .output()
                            {
                                Ok(output) => {
                                    String::from_utf8_lossy(&output.stdout).trim().to_string()
                                }
                                Err(_) => format!("{}", gid)};

                            (username, groupname)
                        }
                        Err(_) => ("$USER".to_string(), "$GROUP".to_string())};

                    // Try to create the directory ourselves with sudo
                    info!("\nSecure code execution requires creating an isolated ramdisk environment.");
                    info!("Failed to create mount point directory: {}", e);
                    info!("Trying with elevated privileges...");

                    // Try to execute the command ourselves
                    let mkdir_result = Command::new("sudo")
                        .args(["mkdir", "-p", mount_point.to_str().unwrap_or("")])
                        .status();

                    if let Ok(status) = mkdir_result {
                        if status.success() {
                            info!("Successfully created directory with sudo");

                            // Now set permissions
                            let chown_cmd = format!("{}:{}", user, group);
                            let chown_result = Command::new("sudo")
                                .args(["chown", &chown_cmd, mount_point.to_str().unwrap_or("")])
                                .status();

                            if let Ok(status) = chown_result {
                                if status.success() {
                                    info!("Successfully set permissions with sudo");
                                    // Directory is now ready, continue with execution
                                    info!("Mount point directory created successfully with sudo");
                                    return Ok(());
                                }
                            }
                        }
                    }

                    // Provide helpful error message
                    let error_msg = format!(
                        "\nFailed to create mount point directory: {}\n\nPlease run:\n    sudo mkdir -p {} && sudo chown {}:{} {}\n",
                        e, mount_point.display(), user, group, mount_point.display()
                    );

                    return Err(StorageError::InsufficientPrivileges(error_msg));
                }
            }
        }

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

        // Step 4: Move into the ramdisk and set up a folder for code
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

        info!(
            "Ramdisk created and configured successfully at {}",
            config.mount_point.display()
        );
        Ok(())
    }

    // Determine if we're running in a container
    fn is_in_container() -> bool {
        // Check for Docker
        if Path::new("/.dockerenv").exists() {
            return true;
        }

        // Check cgroup
        if let Ok(mut file) = File::open("/proc/1/cgroup") {
            let mut contents = String::new();
            if file.read_to_string(&mut contents).is_ok()
                && (contents.contains("docker") || contents.contains("kubepods"))
            {
                return true;
            }
        }

        false
    }

    // Check if AppArmor is active
    // Create ramdisk using sudo directly
    fn create_with_sudo(config: &crate::config::RamdiskConfig) -> Result<bool, StorageError> {
        // Check if both /ephemeral and the mount point exist and are writable
        let mount_point = &config.mount_point;
        let parent_dir = mount_point.parent().unwrap_or(Path::new("/"));

        // Check parent directory first
        let parent_exists = parent_dir.exists();
        let parent_writable = parent_exists
            && fs::metadata(parent_dir)
                .map(|m| m.permissions().mode() & 0o200 != 0)
                .unwrap_or(false);

        // Check mount point
        let mount_exists = mount_point.exists();
        let mount_writable = mount_exists
            && fs::metadata(mount_point)
                .map(|m| m.permissions().mode() & 0o200 != 0)
                .unwrap_or(false);

        if !parent_exists || !parent_writable || !mount_exists || !mount_writable {
            // Get current directory owner and group
            let current_dir_path = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let (user, group) = match fs::metadata(&current_dir_path) {
                Ok(metadata) => {
                    let uid = metadata.uid();
                    let gid = metadata.gid();

                    // Try to get user and group names
                    let username = match std::process::Command::new("id")
                        .args(["-nu", &uid.to_string()])
                        .output()
                    {
                        Ok(output) => String::from_utf8_lossy(&output.stdout).trim().to_string(),
                        Err(_) => format!("{}", uid)};

                    let groupname = match std::process::Command::new("id")
                        .args(["-ng", &gid.to_string()])
                        .output()
                    {
                        Ok(output) => String::from_utf8_lossy(&output.stdout).trim().to_string(),
                        Err(_) => format!("{}", gid)};

                    (username, groupname)
                }
                Err(_) => ("$USER".to_string(), "$GROUP".to_string())};

            // Try to create the directories ourselves with sudo
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

            // Try to execute the command ourselves
            let mkdir_result = Command::new("sudo")
                .args(["mkdir", "-p", mount_point.to_str().unwrap_or("")])
                .status();

            if let Ok(status) = mkdir_result {
                if status.success() {
                    info!("Successfully created directory with sudo");

                    // Now set permissions
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
                            // Directory is now ready, continue with execution
                            return Ok(true);
                        }
                    }
                }
            }

            // If we get here, the sudo attempts failed
            // Build a clear error message
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

        info!("Creating mount point at {}", mount_point.display());
        // We only reach here if all the necessary directories exist and have proper permissions

        // Mount the tmpfs with sudo
        let size_arg = format!("size={}G", config.size_gb);
        let mount_result = Self::run_with_sudo(
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
        let watched_dir = mount_point.join("watched_dir");
        info!("Creating watched_dir inside ramdisk");
        match fs::create_dir(&watched_dir) {
            Ok(_) => info!("Created watched_dir successfully"),
            Err(e) => {
                error!("Failed to create watched_dir in ramdisk: {}", e);
                // Try to unmount since we failed
                let _ = Self::run_with_sudo("umount", &[mount_point.to_str().unwrap_or("")]);
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

        info!(
            "Ramdisk created and configured successfully with sudo at {}",
            config.mount_point.display()
        );
        Ok(true)
    }

    fn is_apparmor_active() -> bool {
        if let Ok(mut file) = File::open("/sys/module/apparmor/parameters/enabled") {
            let mut contents = String::new();
            if file.read_to_string(&mut contents).is_ok() {
                return contents.trim() == "Y";
            }
        }

        // Look for processes in aa-status
        match Command::new("aa-status").output() {
            Ok(output) => {
                if output.status.success() {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    stdout.contains("apparmor module is loaded")
                } else {
                    false
                }
            }
            Err(_) => false}
    }

    // Keep these helper functions from the old code
    fn get_mounted_filesystems(&self) -> Result<Vec<String>, StorageError> {
        let output = std::process::Command::new("mount")
            .output()
            .map_err(|e| StorageError::CommandFailed(format!("Failed to get mount info: {}", e)))?;
        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|s| s.to_string())
            .collect())
    }

    fn is_mount_point(&self, path: &Path) -> Result<bool, StorageError> {
        if !path.exists() {
            return Ok(false);
        }
        let metadata = fs::metadata(path).map_err(StorageError::Io)?;
        let parent_metadata =
            fs::metadata(path.parent().unwrap_or(Path::new("/"))).map_err(StorageError::Io)?;
        Ok(metadata.dev() != parent_metadata.dev())
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
        let mounts = self.get_mounted_filesystems()?;
        let mount_point_str = safe_path_to_string(mount_point).map_err(|e| StorageError::PathInvalid(e.to_string()))?;
        Ok(mounts.iter().any(|m| m.contains(&mount_point_str))
            || self.is_mount_point(mount_point)?)
    }

    fn create(&mut self, config: &crate::config::RamdiskConfig) -> Result<(), StorageError> {
        Self::create(config)
    }

    fn remove(&self, mount_point: &Path) -> Result<(), StorageError> {
        let mount_point_str = safe_path_to_string(mount_point).map_err(|e| StorageError::PathInvalid(e.to_string()))?;
        info!("Attempting to unmount {}", mount_point_str);

        // First try without sudo
        let status = Command::new("umount").arg(mount_point).status();

        let unmount_success = match status {
            Ok(status) => status.success(),
            Err(_) => false};

        // If that fails, try with sudo
        if !unmount_success {
            info!("Regular unmount failed, trying with sudo");
            let sudo_result = Self::run_with_sudo("umount", &[&mount_point_str])?;

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
