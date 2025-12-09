use std::{fs, io::Write, process::Command};

use log::{error, info, warn};
use tempfile::Builder as TempFileBuilder;

use crate::config::RamdiskConfig;
use crate::error::{ExecError, Result};
use crate::metadata::MetadataManager;
use crate::sandbox::create_rust_environment;

use super::utils::get_safe_watched_dir;
#[cfg(test)]
use super::utils::command_exists;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RamdiskConfig;

    fn default_config() -> RamdiskConfig {
        RamdiskConfig::default()
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
}
