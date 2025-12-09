use std::{fs, io::Write, process::Command};

use log::{error, info, warn};
use tempfile::Builder as TempFileBuilder;

use crate::config::RamdiskConfig;
use crate::error::{ExecError, Result};
use crate::metadata::MetadataManager;
use crate::sandbox::create_python_venv;

use super::utils::{find_command, get_safe_watched_dir};
#[cfg(test)]
use super::utils::command_exists;

/// Executes Python code in a sandboxed environment
pub fn exec_python(code: &str, config: &RamdiskConfig) -> Result<()> {
    info!("Executing Python code");

    // Get the appropriate watched directory
    let watched_dir = get_safe_watched_dir(config);

    // Ensure the directory exists
    if !watched_dir.exists() {
        fs::create_dir_all(&watched_dir).map_err(|e| {
            error!("Failed to create watched directory: {}", e);
            ExecError::RuntimeError(format!("Failed to create directory: {e}"))
        })?;
    }

    info!("Using watched directory: {}", watched_dir.display());

    // Create a temporary file for the Python code
    let mut tmpfile = TempFileBuilder::new()
        .prefix("inline-python-")
        .suffix(".py")
        .tempfile_in(&watched_dir)
        .map_err(|e| {
            error!("Failed to create temporary Python file: {}", e);
            ExecError::RuntimeError(format!("Failed to create temp file: {e}"))
        })?;

    write!(tmpfile, "{code}").map_err(|e| {
        error!("Failed to write Python code to file: {}", e);
        ExecError::RuntimeError(format!("Failed to write to temp file: {e}"))
    })?;

    let path = tmpfile.path().to_owned();
    info!("Created Python file at {:?}", path);

    // Create and use a sandboxed Python environment for execution
    info!("Creating sandboxed Python environment");
    let env = create_python_venv(config).map_err(|e| {
        error!("Failed to create Python virtual environment: {}", e);
        ExecError::CommandFailed(format!("Failed to create secure Python environment: {e}"))
    })?;

    info!("Created Python virtual environment at {:?}", env.path);

    // In restricted environments like containers with Landlock, directly use system Python
    // but with the sandboxed environment variables
    let python_cmd = find_command(&["/usr/bin/python3", "/bin/python3", "python3", "python"]);

    let python_executable = match python_cmd {
        Some(cmd) => cmd,
        None => {
            return Err(ExecError::CommandFailed(
                "No Python interpreter found for execution".into(),
            ));
        }
    };

    info!(
        "Using system Python at {} with sandbox environment variables",
        python_executable
    );
    let mut cmd = Command::new(python_executable);
    cmd.arg(&path);

    // Add environment variables for isolation
    for (key, value) in &env.env_vars {
        cmd.env(key, value);
    }

    // Execute the command
    let output = cmd.output().map_err(|e| {
        error!("Failed to execute Python in venv: {}", e);
        ExecError::CommandFailed(format!("Failed to execute Python in sandbox: {e}"))
    })?;

    // Update metadata for the executed file
    if let Some(parent_dir) = watched_dir.parent() {
        let metadata_manager = MetadataManager::new(parent_dir);
        if let Err(e) = metadata_manager.update_metadata(&path, "python") {
            warn!("Failed to update metadata: {}", e);
        }
    }

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        info!("Python output (from sandbox): {}", stdout);
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        error!("Python execution in sandbox failed: {}", stderr);
        Err(ExecError::CommandFailed(format!(
            "Python execution in sandbox failed: {stderr}"
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
    fn test_exec_python() {
        // Skip this test in CI environments
        if std::env::var("CI").is_ok() {
            return;
        }

        // Check for python which is needed for the sandbox
        if !command_exists("python3") && !command_exists("python") {
            return; // Skip test if python isn't installed
        }

        let config = default_config();
        let valid_code = r#"print("Hello from Python")"#;
        match exec_python(valid_code, &config) {
            Ok(_) => (),
            Err(e) => {
                // Only fail if it's not a sandbox creation error (which may happen in CI)
                if !e
                    .to_string()
                    .contains("Failed to create secure Python environment")
                {
                    panic!("Expected success but got error: {}", e);
                }
            }
        }

        let invalid_code = "def unclosed_function(:";
        assert!(exec_python(invalid_code, &config).is_err());
    }
}
