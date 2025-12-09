use std::{fs, process::Command};

use log::{info, warn};

use crate::{
    error::{ExecError, Result},
    exec::find_command,
    platform_utils::set_executable,
    sandbox::{environment::SandboxedEnvironment, path_utils::safe_path_to_str},
};

use super::SandboxManager;

/// Create a Node.js environment using fnm or a simple directory structure
pub fn create_node_environment_impl<'a>(
    manager: &'a mut SandboxManager,
    name: &str,
) -> Result<&'a SandboxedEnvironment> {
    let env_path = manager.base_dir().join(name);
    let mut env = SandboxedEnvironment::new("node", env_path.clone());

    if env_path.exists() {
        info!("Node.js environment already exists at {:?}", env_path);
        env.is_valid = true;
        manager.add_environment(env);
        return manager.get_environment("node").ok_or_else(|| {
            ExecError::RuntimeError(
                "Failed to retrieve Node.js environment after adding it to sandbox".to_string(),
            )
        });
    }

    info!("Creating Node.js environment at {:?}", env_path);

    // Check if fnm is available
    if find_command(&["fnm"]).is_some() {
        info!("Using fnm to create Node.js environment");

        let env_path_str = safe_path_to_str(&env_path)?;
        let output = Command::new("fnm")
            .args(["install", "--fnm-dir", env_path_str, "lts"])
            .output();

        match output {
            Ok(output) => {
                if output.status.success() {
                    info!("Node.js environment created successfully with fnm");
                    env.is_valid = true;

                    // Add environment variables
                    let env_path_str = safe_path_to_str(&env_path)?;
                    let bin_path = env_path.join("bin");
                    let bin_path_str = safe_path_to_str(&bin_path)?;
                    env.add_env_var("FNM_DIR", env_path_str);
                    env.add_env_var(
                        "PATH",
                        &format!(
                            "{}:{}",
                            bin_path_str,
                            std::env::var("PATH").unwrap_or_else(|_| String::new())
                        ),
                    );

                    manager.add_environment(env);
                    return manager.get_environment("node").ok_or_else(|| {
                        ExecError::RuntimeError(
                            "Failed to retrieve Node.js environment after creation".to_string(),
                        )
                    });
                }
                let stderr = String::from_utf8_lossy(&output.stderr);
                warn!("Failed to create Node.js environment with fnm: {}", stderr);
            }
            Err(e) => {
                warn!("Failed to execute fnm: {}", e);
            }
        }
    }

    // Fall back to creating a simple directory structure with Node wrapper
    if let Err(e) = fs::create_dir_all(env_path.join("bin")) {
        warn!("Failed to create Node.js env directory structure: {}", e);
        return Err(ExecError::RuntimeError(format!(
            "Failed to create Node.js environment directory: {e}"
        )));
    }

    // Find a Node executable - check for absolute paths first
    let node_candidates = &[
        "/usr/bin/node",
        "/bin/node",
        "/usr/local/bin/node",
        "/usr/bin/nodejs",
        "/bin/nodejs",
        "/usr/local/bin/nodejs",
        "node",
        "nodejs",
    ];

    let node_cmd = find_command(node_candidates);

    if node_cmd.is_none() {
        return Err(ExecError::RuntimeError(format!(
            "No Node.js runtime found. Tried: {node_candidates:?}"
        )));
    }

    let node = node_cmd.ok_or_else(|| {
        ExecError::RuntimeError(
            "Node.js command unexpectedly became None after validation".to_string(),
        )
    })?;

    // Create a wrapper script for node
    let node_modules_path = env_path.join("node_modules");
    let node_modules_path_str = safe_path_to_str(&node_modules_path)?;
    let node_wrapper = format!(
        "#!/bin/sh\nexport NODE_PATH=\"{node_modules_path_str}:$NODE_PATH\"\n{node} \"$@\"\n"
    );

    let node_bin_path = env_path.join("bin").join("node");
    if let Err(e) = fs::write(&node_bin_path, node_wrapper) {
        warn!("Failed to create Node.js wrapper script: {}", e);
        return Err(ExecError::RuntimeError(format!(
            "Failed to create Node.js wrapper script: {e}"
        )));
    }

    // Make it executable
    if let Err(e) = set_executable(&node_bin_path) {
        warn!("Failed to make Node.js wrapper executable: {}", e);
        return Err(ExecError::RuntimeError(format!(
            "Failed to set permissions on Node.js wrapper: {e}"
        )));
    }

    // Create npm directory
    if let Err(e) = fs::create_dir_all(env_path.join("node_modules")) {
        warn!("Failed to create node_modules directory: {}", e);
    }

    info!("Created minimal Node.js environment with wrapper script");
    env.is_valid = true;

    // Add environment variables
    let node_modules_path = env_path.join("node_modules");
    let node_modules_path_str = safe_path_to_str(&node_modules_path)?;
    let bin_path = env_path.join("bin");
    let bin_path_str = safe_path_to_str(&bin_path)?;
    env.add_env_var(
        "NODE_PATH",
        &format!(
            "{}:{}",
            node_modules_path_str,
            std::env::var("NODE_PATH").unwrap_or_else(|_| String::new())
        ),
    );
    env.add_env_var(
        "PATH",
        &format!(
            "{}:{}",
            bin_path_str,
            std::env::var("PATH").unwrap_or_else(|_| String::new())
        ),
    );

    manager.add_environment(env);
    manager.get_environment("node").ok_or_else(|| {
        ExecError::RuntimeError(
            "Failed to retrieve Node.js environment after creation".to_string(),
        )
    })
}
