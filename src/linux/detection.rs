use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::process::Command;

/// Utilities for detecting container environments and security systems.
pub struct EnvironmentDetector;

impl EnvironmentDetector {
    /// Determine if we're running in a container (Docker, Kubernetes, etc.).
    ///
    /// Checks for common container indicators:
    /// - Presence of /.dockerenv file
    /// - Container-specific entries in /proc/1/cgroup
    pub fn is_in_container() -> bool {
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

    /// Check if AppArmor is active on the system.
    ///
    /// AppArmor is a Linux kernel security module that can restrict
    /// program capabilities with per-program profiles. This function
    /// checks if AppArmor is loaded and active.
    pub fn is_apparmor_active() -> bool {
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
            Err(_) => false,
        }
    }
}
