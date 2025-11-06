use std::{fs, io::Write, process::Command};

use log::{error, info, warn};
use tempfile::Builder as TempFileBuilder;

use crate::config::RamdiskConfig;
use crate::error::{ExecError, Result};
use crate::metadata::MetadataManager;
use crate::sandbox::safe_path_to_string;

use super::utils::{find_command, get_safe_watched_dir};

/// Executes Bash shell scripts in a sandboxed environment
pub fn exec_bash(code: &str, config: &RamdiskConfig) -> Result<()> {
    let watched_dir = get_safe_watched_dir(config);

    // Write code to a temporary file
    let mut tmpfile = TempFileBuilder::new()
        .prefix("inline-bash-")
        .suffix(".sh")
        .tempfile_in(&watched_dir)?;

    write!(tmpfile, "{code}")?;
    info!("Created Bash script: {:?}", tmpfile.path());

    // Make the script executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(tmpfile.path())?.permissions();
        perms.set_mode(0o755); // rwx for owner, rx for group and others
        fs::set_permissions(tmpfile.path(), perms)?;
    }

    // Find bash executable
    let bash_cmd = find_command(&["/usr/bin/bash", "/bin/bash", "bash"]);
    let bash_executable = match bash_cmd {
        Some(cmd) => cmd,
        None => {
            return Err(ExecError::CommandFailed(
                "No Bash interpreter found for execution".into(),
            ));
        }
    };

    info!("Using Bash interpreter at {}", bash_executable);

    // Execute the script in a controlled environment
    let mut cmd = Command::new(bash_executable);
    cmd.arg(tmpfile.path());

    // Add environment variables for isolation
    // Note: We're not creating a specific sandbox environment for bash yet
    // but we could implement a more specialized sandboxed bash environment in the future
    let mut safe_env = std::collections::HashMap::new();
    safe_env.insert("PATH".to_string(), "/usr/bin:/bin".to_string());
    let watched_dir_str = safe_path_to_string(&watched_dir)?;
    safe_env.insert("HOME".to_string(), watched_dir_str.clone());
    safe_env.insert("TEMP".to_string(), watched_dir_str.clone());
    safe_env.insert("TMP".to_string(), watched_dir_str);

    // Apply the safe environment
    for (key, value) in &safe_env {
        cmd.env(key, value);
    }

    // Execute the command
    let output = cmd.output().map_err(|e| {
        error!("Failed to execute Bash script: {}", e);
        ExecError::CommandFailed(format!("Failed to execute Bash script: {e}"))
    })?;

    // Update metadata for the executed file
    if let Some(parent_dir) = watched_dir.parent() {
        let metadata_manager = MetadataManager::new(parent_dir);
        if let Err(e) = metadata_manager.update_metadata(tmpfile.path(), "bash") {
            warn!("Failed to update metadata: {}", e);
        }
    }

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        info!("Bash output (from sandbox): {}", stdout);
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        error!("Bash execution failed: {}", stderr);
        Err(ExecError::CommandFailed(format!(
            "Bash execution failed: {stderr}"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RamdiskConfig;

    fn default_config() -> RamdiskConfig {
        RamdiskConfig::default()
    }

    #[test]
    fn test_exec_bash() {
        // Skip this test in CI environments
        if std::env::var("CI").is_ok() {
            return;
        }

        // Check for bash which is needed for the execution
        if !command_exists("bash") {
            return; // Skip test if bash isn't installed
        }

        let config = default_config();
        let valid_code = r#"
            #!/bin/bash
            echo "Hello from Bash"
            exit 0
        "#;

        match exec_bash(valid_code, &config) {
            Ok(_) => (),
            Err(e) => {
                panic!("Expected success but got error: {}", e);
            }
        }

        // Test with a script that produces an error
        let error_code = r#"
            #!/bin/bash
            echo "This script will fail" >&2
            exit 1
        "#;
        assert!(exec_bash(error_code, &config).is_err());

        // Test with invalid bash syntax
        let invalid_code = "if then fi malformed";
        assert!(exec_bash(invalid_code, &config).is_err());
    }
}
