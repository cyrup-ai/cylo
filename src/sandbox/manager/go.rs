use std::fs;

use log::{info, warn};

use crate::{
    error::{ExecError, Result},
    exec::find_command,
    platform_utils::set_executable,
    sandbox::{environment::SandboxedEnvironment, path_utils::safe_path_to_str},
};

use super::SandboxManager;

/// Create a Go environment with its own GOPATH and workspace
pub fn create_go_environment_impl<'a>(
    manager: &'a mut SandboxManager,
    name: &str,
) -> Result<&'a SandboxedEnvironment> {
    let env_path = manager.base_dir().join(name);
    let mut env = SandboxedEnvironment::new("go", env_path.clone());

    if env_path.exists() {
        info!("Go environment already exists at {:?}", env_path);
        env.is_valid = true;
        manager.add_environment(env);
        return manager.get_environment("go").ok_or_else(|| {
            ExecError::RuntimeError(
                "Failed to retrieve Go environment after adding it to sandbox".to_string(),
            )
        });
    }

    info!("Creating Go environment at {:?}", env_path);

    // Create directory structure for a proper Go workspace
    let go_paths = [
        env_path.join("bin"),
        env_path.join("pkg"),
        env_path.join("src"),
        env_path.join("tmp"),
    ];

    for path in &go_paths {
        if let Err(e) = fs::create_dir_all(path) {
            warn!(
                "Failed to create Go env directory structure at {:?}: {}",
                path, e
            );
            return Err(ExecError::RuntimeError(format!(
                "Failed to create Go environment directory structure: {e}"
            )));
        }
    }

    // Find a Go executable - check for absolute paths first
    let go_candidates = &[
        "/usr/bin/go",
        "/bin/go",
        "/usr/local/bin/go",
        "/usr/local/go/bin/go",
        "go",
    ];

    let go_cmd = find_command(go_candidates);

    if go_cmd.is_none() {
        return Err(ExecError::RuntimeError(format!(
            "No Go runtime found. Tried: {go_candidates:?}"
        )));
    }

    let go = go_cmd.ok_or_else(|| {
        ExecError::RuntimeError("Go command unexpectedly became None after validation".to_string())
    })?;

    // Create a wrapper script for Go
    let env_path_str = safe_path_to_str(&env_path)?;
    let pkg_path = env_path.join("pkg");
    let pkg_path_str = safe_path_to_str(&pkg_path)?;
    let tmp_path = env_path.join("tmp");
    let tmp_path_str = safe_path_to_str(&tmp_path)?;
    let go_wrapper = format!(
        "#!/bin/sh\nexport GOPATH=\"{env_path_str}\"\nexport GOCACHE=\"{pkg_path_str}\"\nexport GOTMPDIR=\"{tmp_path_str}\"\n{go} \"$@\"\n"
    );

    let go_bin_path = env_path.join("bin").join("go");
    if let Err(e) = fs::write(&go_bin_path, go_wrapper) {
        warn!("Failed to create Go wrapper script: {}", e);
        return Err(ExecError::RuntimeError(format!(
            "Failed to create Go wrapper script: {e}"
        )));
    }

    // Make it executable
    if let Err(e) = set_executable(&go_bin_path) {
        warn!("Failed to make Go wrapper executable: {}", e);
        return Err(ExecError::RuntimeError(format!(
            "Failed to set permissions on Go wrapper: {e}"
        )));
    }

    // Create a simple hello world program to verify the environment
    let hello_dir = env_path.join("src").join("hello");
    if let Err(e) = fs::create_dir_all(&hello_dir) {
        warn!("Failed to create hello directory: {}", e);
    }

    if let Err(e) = fs::write(
        hello_dir.join("main.go"),
        r#"package main

import "fmt"

func main() {
    fmt.Println("Go environment initialized")
}
"#,
    ) {
        warn!("Failed to create hello world Go program: {}", e);
    }

    info!("Created Go environment with workspace structure");
    env.is_valid = true;

    // Add environment variables
    let env_path_str = safe_path_to_str(&env_path)?;
    let pkg_path = env_path.join("pkg");
    let pkg_path_str = safe_path_to_str(&pkg_path)?;
    let tmp_path = env_path.join("tmp");
    let tmp_path_str = safe_path_to_str(&tmp_path)?;
    let bin_path = env_path.join("bin");
    let bin_path_str = safe_path_to_str(&bin_path)?;
    env.add_env_var("GOPATH", env_path_str);
    env.add_env_var("GOCACHE", pkg_path_str);
    env.add_env_var("GOTMPDIR", tmp_path_str);
    env.add_env_var(
        "PATH",
        &format!(
            "{}:{}",
            bin_path_str,
            std::env::var("PATH").unwrap_or_else(|_| String::new())
        ),
    );

    manager.add_environment(env);
    manager.get_environment("go").ok_or_else(|| {
        ExecError::RuntimeError("Failed to retrieve Go environment after creation".to_string())
    })
}
