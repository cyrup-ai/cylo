use std::{fs, os::unix::fs::PermissionsExt};

use log::{info, warn};

use crate::{
    error::{ExecError, Result},
    exec::find_command,
    sandbox::{environment::SandboxedEnvironment, path_utils::safe_path_to_str},
};

use super::SandboxManager;

/// Create a Rust environment with its own cargo directory
pub fn create_rust_environment_impl<'a>(
    manager: &'a mut SandboxManager,
    name: &str,
) -> Result<&'a SandboxedEnvironment> {
    let env_path = manager.base_dir().join(name);
    let mut env = SandboxedEnvironment::new("rust", env_path.clone());

    if env_path.exists() {
        info!("Rust environment already exists at {:?}", env_path);
        env.is_valid = true;
        manager.add_environment(env);
        return manager.get_environment("rust").ok_or_else(|| {
            ExecError::RuntimeError(
                "Failed to retrieve Rust environment after adding it to sandbox".to_string(),
            )
        });
    }

    info!("Creating Rust environment at {:?}", env_path);

    // Create directory structure
    if let Err(e) = fs::create_dir_all(env_path.join("bin")) {
        warn!("Failed to create Rust env directory structure: {}", e);
        return Err(ExecError::RuntimeError(format!(
            "Failed to create Rust environment directory: {e}"
        )));
    }

    // Create a Cargo.toml for the environment
    if let Err(e) = fs::write(
        env_path.join("Cargo.toml"),
        r#"[package]
name = "sandbox"
version = "0.1.0"
edition = "2021"

[dependencies]
"#,
    ) {
        warn!("Failed to create Cargo.toml: {}", e);
    }

    // Create src directory with main.rs
    if let Err(e) = fs::create_dir_all(env_path.join("src")) {
        warn!("Failed to create src directory: {}", e);
    }

    if let Err(e) = fs::write(
        env_path.join("src").join("main.rs"),
        r#"fn main() {
    println!("Rust environment initialized");
}
"#,
    ) {
        warn!("Failed to create main.rs: {}", e);
    }

    // Find rustc and cargo - check absolute paths first
    let rustc_candidates = &[
        "/usr/bin/rustc",
        "/bin/rustc",
        "/usr/local/bin/rustc",
        "/home/user/.cargo/bin/rustc",
        "rustc",
    ];

    let cargo_candidates = &[
        "/usr/bin/cargo",
        "/bin/cargo",
        "/usr/local/bin/cargo",
        "/home/user/.cargo/bin/cargo",
        "cargo",
    ];

    let rustc_cmd = find_command(rustc_candidates);
    let cargo_cmd = find_command(cargo_candidates);

    if rustc_cmd.is_none() || cargo_cmd.is_none() {
        return Err(ExecError::RuntimeError(format!(
            "Rust toolchain not found. Tried rustc: {rustc_candidates:?}, cargo: {cargo_candidates:?}"
        )));
    }

    let rustc = rustc_cmd.ok_or_else(|| {
        ExecError::RuntimeError(
            "Rust compiler command unexpectedly became None after validation".to_string(),
        )
    })?;
    let cargo = cargo_cmd.ok_or_else(|| {
        ExecError::RuntimeError(
            "Cargo command unexpectedly became None after validation".to_string(),
        )
    })?;

    // Create wrapper scripts
    let env_path_str = safe_path_to_str(&env_path)?;
    let rustc_wrapper = format!(
        "#!/bin/sh\nexport CARGO_HOME=\"{env_path_str}\"\nexport RUSTUP_HOME=\"{env_path_str}\"\n{rustc} \"$@\"\n"
    );

    let cargo_wrapper = format!(
        "#!/bin/sh\nexport CARGO_HOME=\"{env_path_str}\"\nexport RUSTUP_HOME=\"{env_path_str}\"\n{cargo} \"$@\"\n"
    );

    let rustc_bin_path = env_path.join("bin").join("rustc");
    if let Err(e) = fs::write(&rustc_bin_path, rustc_wrapper) {
        warn!("Failed to create rustc wrapper script: {}", e);
        return Err(ExecError::RuntimeError(format!(
            "Failed to create rustc wrapper script: {e}"
        )));
    }

    let cargo_bin_path = env_path.join("bin").join("cargo");
    if let Err(e) = fs::write(&cargo_bin_path, cargo_wrapper) {
        warn!("Failed to create cargo wrapper script: {}", e);
        return Err(ExecError::RuntimeError(format!(
            "Failed to create cargo wrapper script: {e}"
        )));
    }

    // Make them executable
    if let Err(e) = fs::set_permissions(&rustc_bin_path, fs::Permissions::from_mode(0o755)) {
        warn!("Failed to make rustc wrapper executable: {}", e);
    }

    if let Err(e) = fs::set_permissions(&cargo_bin_path, fs::Permissions::from_mode(0o755)) {
        warn!("Failed to make cargo wrapper executable: {}", e);
    }

    info!("Created minimal Rust environment with wrapper scripts");
    env.is_valid = true;

    // Add environment variables
    let env_path_str = safe_path_to_str(&env_path)?;
    let bin_path = env_path.join("bin");
    let bin_path_str = safe_path_to_str(&bin_path)?;
    env.add_env_var("CARGO_HOME", env_path_str);
    env.add_env_var("RUSTUP_HOME", env_path_str);
    env.add_env_var(
        "PATH",
        &format!(
            "{}:{}",
            bin_path_str,
            std::env::var("PATH").unwrap_or_else(|_| String::new())
        ),
    );

    manager.add_environment(env);
    manager.get_environment("rust").ok_or_else(|| {
        ExecError::RuntimeError("Failed to retrieve Rust environment after creation".to_string())
    })
}
