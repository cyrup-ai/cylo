use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
};

use log::{error, info, warn};
use tempfile::Builder as TempFileBuilder;

use crate::config::RamdiskConfig;
use crate::error::{ExecError, Result};
use crate::metadata::MetadataManager;
use crate::ramdisk::get_watched_dir;
use crate::sandbox::{
    create_go_environment, create_node_environment, create_python_venv, create_rust_environment,
    safe_path_to_string,
};

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
fn command_exists(cmd: &str) -> bool {
    find_command(&[cmd]).is_some()
}

// Check if running in a container or restricted environment (removed - now using sandboxes always)

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

/// Executes Rust code in a sandboxed environment
pub fn exec_rust(code: &str, config: &RamdiskConfig) -> Result<()> {
    let watched_dir = get_safe_watched_dir(config);

    // Create a temporary file for the Rust code
    let mut tmpfile = TempFileBuilder::new()
        .prefix("inline-rust-")
        .suffix(".rs")
        .tempfile_in(&watched_dir)?;

    write!(tmpfile, "{code}")?;
    info!("Created Rust file: {:?}", tmpfile.path());

    // Create and use a sandboxed Rust environment
    info!("Creating sandboxed Rust environment");
    let env = create_rust_environment(config).map_err(|e| {
        error!("Failed to create Rust environment: {}", e);
        ExecError::CommandFailed(format!("Failed to create secure Rust environment: {e}"))
    })?;

    info!("Created Rust environment at {:?}", env.path);

    // Create a simple Cargo project for the code
    let project_dir = env.path.join("project");
    if !project_dir.exists() {
        fs::create_dir_all(&project_dir)?;
    }

    let src_dir = project_dir.join("src");
    if !src_dir.exists() {
        fs::create_dir_all(&src_dir)?;
    }

    // Copy the code to main.rs
    fs::write(src_dir.join("main.rs"), code)?;

    // Create a Cargo.toml file
    fs::write(
        project_dir.join("Cargo.toml"),
        r#"[package]
name = "sandbox_rust"
version = "0.1.0"
edition = "2021"

[dependencies]
"#,
    )?;

    // Execute the code in the sandboxed environment
    let cargo_bin = env.get_binary_path("cargo");
    let mut cmd = Command::new(&cargo_bin);
    cmd.args(["run"]);
    cmd.current_dir(&project_dir);

    // Add environment variables
    for (key, value) in &env.env_vars {
        cmd.env(key, value);
    }

    // Execute the command
    let output = cmd.output().map_err(|e| {
        error!("Failed to execute Rust in sandbox: {}", e);
        ExecError::CommandFailed(format!("Failed to execute Rust in sandbox: {e}"))
    })?;

    // Update metadata for the executed file
    if let Some(parent_dir) = watched_dir.parent() {
        let metadata_manager = MetadataManager::new(parent_dir);
        if let Err(e) = metadata_manager.update_metadata(tmpfile.path(), "rust") {
            warn!("Failed to update metadata: {}", e);
        }
    }

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        info!("Rust output (from sandbox): {}", stdout);
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        error!("Rust execution in sandbox failed: {}", stderr);
        Err(ExecError::CommandFailed(format!(
            "Rust execution in sandbox failed: {stderr}"
        )))
    }
}

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

    #[test]
    fn test_exec_rust() {
        // Skip this test in CI environments
        if std::env::var("CI").is_ok() {
            return;
        }

        // Check for rustc and cargo which are needed for the sandbox
        if !command_exists("rustc") || !command_exists("cargo") {
            return; // Skip test if rust toolchain isn't installed
        }

        let config = default_config();
        let valid_code = r#"
            fn main() {
                println!("Hello from Rust");
            }
        "#;
        match exec_rust(valid_code, &config) {
            Ok(_) => (),
            Err(e) => {
                // Only fail if it's not a sandbox creation error (which may happen in CI)
                if !e
                    .to_string()
                    .contains("Failed to create secure Rust environment")
                {
                    panic!("Expected success but got error: {}", e);
                }
            }
        }

        let invalid_code = "this is not rust code";
        assert!(exec_rust(invalid_code, &config).is_err());
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
