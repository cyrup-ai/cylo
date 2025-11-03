use std::fs::{File, create_dir_all};
use std::io::Read;
use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use log::{info, warn};

use crate::error::StorageError;
use crate::state::PipelineEvent;

/// Configuration for the file system jail
pub struct JailConfig {
    /// The directory to allow modifications in
    pub allowed_dir: PathBuf,
    /// Whether to enable Landlock file system restrictions
    pub enable_landlock: bool,
    /// Whether to check for AppArmor restrictions
    pub check_apparmor: bool,
}

impl Default for JailConfig {
    fn default() -> Self {
        Self {
            allowed_dir: PathBuf::from("/tmp/cylo"),
            enable_landlock: true,
            check_apparmor: true,
        }
    }
}

/// Check if AppArmor is loaded and potentially restricting us
fn is_apparmor_enabled() -> bool {
    // Check if AppArmor module is loaded
    if let Ok(mut file) = File::open("/sys/module/apparmor/parameters/enabled") {
        let mut contents = String::new();
        if file.read_to_string(&mut contents).is_ok() {
            return contents.trim() == "Y";
        }
    }

    // Alternative check using the aa-status command
    let output = Command::new("aa-status").output();

    if let Ok(output) = output
        && output.status.success()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        return stdout.contains("apparmor module is loaded");
    }

    false
}

/// Initializes a file system jail using Landlock
///
/// This sets up a Landlock ruleset that restricts file system access to
/// the specified allowed directory.
///
/// # Arguments
/// * `config` - Configuration for the jail, including allowed directory
///
/// # Returns
/// * `Ok(())` if successfully set up
/// * `Err` with a message if the allowed directory cannot be created
pub fn init_jail(config: &JailConfig) -> Result<(), StorageError> {
    // Landlock is preferred but not mandatory
    if !config.enable_landlock {
        warn!("Landlock restrictions disabled - security will be reduced");
    }

    // Check for AppArmor if enabled
    if config.check_apparmor && is_apparmor_enabled() {
        warn!("AppArmor is active on this system. This may affect execution permissions.");
        warn!("If execution fails, you may need to run: sudo aa-complain /usr/bin/cargo");
    }

    // Create the allowed directory if it doesn't exist
    let allowed_dir = config.allowed_dir.to_str().unwrap_or("/tmp/cylo");
    create_dir_all(allowed_dir)
        .with_context(|| format!("Failed to create allowed directory at {allowed_dir}"))?;
    info!("Allowed directory ensured at {}", allowed_dir);

    // Try to set up Landlock rules if enabled
    if config.enable_landlock {
        match apply_landlock_restrictions(allowed_dir) {
            Ok(_) => info!("Landlock restrictions applied successfully"),
            Err(e) => warn!(
                "Failed to apply Landlock restrictions: {}. Continuing with reduced security.",
                e
            ),
        }
    } else {
        warn!("Landlock restrictions not applied - running with reduced security");
    }

    Ok(())
}

/// Applies Landlock restrictions to limit file system access
///
/// This implements the Landlock v0.4.1 API to restrict file system access
/// to only the allowed directory
fn apply_landlock_restrictions(allowed_dir: &str) -> Result<(), StorageError> {
    #[cfg(all(target_os = "linux", feature = "landlock"))]
    {
        use landlock::{
            ABI, Access, AccessFs, PathBeneath, PathFd, Ruleset, RulesetAttr, RulesetCreatedAttr,
            RulesetStatus,
        };

        // Set up Landlock to only allow file access in the specified directory
        let abi = ABI::V1;
        // Create access rights for read, write, and execute operations
        let access_all = AccessFs::from_all(abi);

        let dir_fd = PathFd::new(allowed_dir)
            .context(format!("Failed to open directory: {}", allowed_dir))?;

        // Allow direct execution of Python and other necessary interpreters
        // This is a more secure approach than trying to copy interpreters
        let status = Ruleset::default()
            .handle_access(access_all)
            .context("Failed to set up permissions")?
            .create()
            .context("Failed to make Landlock rules")?
            .add_rule(PathBeneath::new(dir_fd, access_all))
            .context("Failed to add watched directory rule")?
            .restrict_self()
            .context("Failed to turn on Landlock")?;

        // Verify that Landlock is fully enforced
        match status.ruleset {
            RulesetStatus::FullyEnforced => {
                info!("Landlock is fully on—locked to {}", allowed_dir);
                Ok(())
            }
            RulesetStatus::PartiallyEnforced => {
                warn!("Landlock is only partly enforced - security may be reduced");
                Ok(()) // Continue anyway
            }
            RulesetStatus::NotEnforced => {
                warn!("Landlock failed to enforce restrictions - continuing with reduced security");
                Ok(()) // Continue anyway
            }
        }
    }

    #[cfg(not(all(target_os = "linux", feature = "landlock")))]
    {
        warn!(
            "Landlock is not available. Running with reduced security on {}",
            allowed_dir
        );
        Ok(()) // Continue without Landlock
    }
}

/// Watch a directory for file changes and send events through the channel
///
/// This function sets up a file watcher using a simple polling mechanism.
/// It watches the specified directory for changes and sends PipelineEvent::FileChanged
/// events through the provided channel.
///
/// # Arguments
/// * `path` - The directory to watch
/// * `tx` - Sender for pipeline events
///
/// # Returns
/// * `Result<()>` - Ok if the watcher started successfully, Err otherwise
pub fn watch_directory(path: PathBuf, tx: mpsc::Sender<PipelineEvent>) -> Result<(), StorageError> {
    // Start a simple file polling thread
    let path_clone = path.clone();
    thread::spawn(move || {
        // Set up initial state
        let mut last_modified = std::time::SystemTime::now();

        loop {
            // Sleep to avoid excessive CPU usage
            thread::sleep(Duration::from_millis(500));

            // Check if any file in the directory has changed
            if let Ok(entries) = std::fs::read_dir(&path_clone) {
                for entry in entries.filter_map(Result::ok) {
                    if let Ok(metadata) = entry.metadata()
                        && let Ok(modified) = metadata.modified()
                        && modified > last_modified
                    {
                        last_modified = modified;
                        let _ = tx.send(PipelineEvent::FileChanged(entry.path()));
                    }
                }
            }
        }
    });

    Ok(())
}
