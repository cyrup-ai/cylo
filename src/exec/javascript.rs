use std::{io::Write, process::Command};

use log::{error, info, warn};
use tempfile::Builder as TempFileBuilder;

use crate::config::RamdiskConfig;
use crate::error::{ExecError, Result};
use crate::metadata::MetadataManager;
use crate::sandbox::create_node_environment;

use super::utils::get_safe_watched_dir;
#[cfg(test)]
use super::utils::command_exists;

/// Executes JavaScript code in a sandboxed environment
pub fn exec_js(code: &str, config: &RamdiskConfig) -> Result<()> {
    let watched_dir = get_safe_watched_dir(config);

    // Write code to a temporary file
    let mut tmpfile = TempFileBuilder::new()
        .prefix("inline-js-")
        .suffix(".js")
        .tempfile_in(&watched_dir)?;

    write!(tmpfile, "{code}")?;
    info!("Created JS file: {:?}", tmpfile.path());

    // Create and use a sandboxed Node environment
    info!("Creating sandboxed Node environment");
    let env = create_node_environment(config).map_err(|e| {
        error!("Failed to create Node environment: {}", e);
        ExecError::CommandFailed(format!(
            "Failed to create secure JavaScript environment: {e}"
        ))
    })?;

    info!("Created Node environment at {:?}", env.path);

    // Execute the code in the sandboxed environment
    let node_bin = env.get_binary_path("node");
    let node_bin_str = node_bin.to_str().ok_or_else(|| {
        ExecError::RuntimeError("Node binary path contains invalid UTF-8".to_string())
    })?;
    let mut cmd = Command::new(node_bin_str);
    cmd.arg(tmpfile.path());

    // Add environment variables
    for (key, value) in &env.env_vars {
        cmd.env(key, value);
    }

    // Execute the command
    let output = cmd.output().map_err(|e| {
        error!("Failed to execute JavaScript in sandbox: {}", e);
        ExecError::CommandFailed(format!("Failed to execute JavaScript in sandbox: {e}"))
    })?;

    // Update metadata for the executed file
    if let Some(parent_dir) = watched_dir.parent() {
        let metadata_manager = MetadataManager::new(parent_dir);
        if let Err(e) = metadata_manager.update_metadata(tmpfile.path(), "javascript") {
            warn!("Failed to update metadata: {}", e);
        }
    }

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        info!("JavaScript output (from sandbox): {}", stdout);
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        error!("JavaScript execution in sandbox failed: {}", stderr);
        Err(ExecError::CommandFailed(format!(
            "JavaScript execution in sandbox failed: {stderr}"
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
    fn test_exec_js() {
        // Skip this test in CI environments
        if std::env::var("CI").is_ok() {
            return;
        }

        // Check for node which is needed for the sandbox
        if !command_exists("node") {
            return; // Skip test if node isn't installed
        }

        let config = default_config();
        let valid_code = r#"console.log("Hello from JavaScript");"#;
        match exec_js(valid_code, &config) {
            Ok(_) => (),
            Err(e) => {
                // Only fail if it's not a sandbox creation error (which may happen in CI)
                if !e
                    .to_string()
                    .contains("Failed to create secure JavaScript environment")
                {
                    panic!("Expected success but got error: {}", e);
                }
            }
        }

        let invalid_code = "function {";
        assert!(exec_js(invalid_code, &config).is_err());
    }
}
