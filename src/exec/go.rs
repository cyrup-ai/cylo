use std::{io::Write, process::Command};

use log::{error, info, warn};
use tempfile::Builder as TempFileBuilder;

use crate::config::RamdiskConfig;
use crate::error::{ExecError, Result};
use crate::metadata::MetadataManager;
use crate::sandbox::create_go_environment;

use super::utils::get_safe_watched_dir;

/// Executes Go code in a sandboxed environment
pub fn exec_go(code: &str, config: &RamdiskConfig) -> Result<()> {
    let watched_dir = get_safe_watched_dir(config);

    // Create a temporary file for the Go code
    let mut tmpfile = TempFileBuilder::new()
        .prefix("inline-go-")
        .suffix(".go")
        .tempfile_in(&watched_dir)?;

    write!(tmpfile, "{code}")?;
    info!("Created Go file: {:?}", tmpfile.path());

    // Create and use a sandboxed Go environment
    info!("Creating sandboxed Go environment");
    let env = create_go_environment(config).map_err(|e| {
        error!("Failed to create Go environment: {}", e);
        ExecError::CommandFailed(format!("Failed to create secure Go environment: {e}"))
    })?;

    info!("Created Go environment at {:?}", env.path);

    // Execute the code in the sandboxed environment
    let go_bin = env.get_binary_path("go");
    let mut cmd = Command::new(&go_bin);
    let tmpfile_path_str = tmpfile.path().to_str().ok_or_else(|| {
        ExecError::RuntimeError("Temporary file path contains invalid UTF-8".to_string())
    })?;
    cmd.args(["run", tmpfile_path_str]);

    // Add environment variables
    for (key, value) in &env.env_vars {
        cmd.env(key, value);
    }

    // Execute the command
    let output = cmd.output().map_err(|e| {
        error!("Failed to execute Go in sandbox: {}", e);
        ExecError::CommandFailed(format!("Failed to execute Go in sandbox: {e}"))
    })?;

    // Update metadata for the executed file
    if let Some(parent_dir) = watched_dir.parent() {
        let metadata_manager = MetadataManager::new(parent_dir);
        if let Err(e) = metadata_manager.update_metadata(tmpfile.path(), "go") {
            warn!("Failed to update metadata: {}", e);
        }
    }

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        info!("Go output (from sandbox): {}", stdout);
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        error!("Go execution in sandbox failed: {}", stderr);
        Err(ExecError::CommandFailed(format!(
            "Go execution in sandbox failed: {stderr}"
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
    fn test_exec_go() {
        // Skip this test in CI environments
        if std::env::var("CI").is_ok() {
            return;
        }

        // Check for go which is needed for the sandbox
        if !command_exists("go") {
            return; // Skip test if go isn't installed
        }

        let config = default_config();
        let valid_code = r#"
            package main
            import "fmt"
            func main() {
                fmt.Println("Hello from Go")
            }
        "#;
        match exec_go(valid_code, &config) {
            Ok(_) => (),
            Err(e) => {
                // Only fail if it's not a sandbox creation error (which may happen in CI)
                if !e
                    .to_string()
                    .contains("Failed to create secure Go environment")
                {
                    panic!("Expected success but got error: {}", e);
                }
            }
        }

        let invalid_code = "this is not go code";
        assert!(exec_go(invalid_code, &config).is_err());
    }
}
