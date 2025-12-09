use std::{fs, process::Command};

use log::{info, warn};

use crate::{
    error::{ExecError, Result},
    exec::find_command,
    platform_utils::set_executable,
    sandbox::{environment::SandboxedEnvironment, path_utils::safe_path_to_str},
};

use super::SandboxManager;

/// Create a Python virtual environment
pub fn create_python_environment_impl<'a>(
    manager: &'a mut SandboxManager,
    name: &str,
) -> Result<&'a SandboxedEnvironment> {
    let env_path = manager.base_dir().join(name);
    let mut env = SandboxedEnvironment::new("python", env_path.clone());

    if env_path.exists() {
        info!("Python environment already exists at {:?}", env_path);
        env.is_valid = true;
        manager.add_environment(env);
        return manager.get_environment("python").ok_or_else(|| {
            ExecError::RuntimeError(
                "Failed to retrieve Python environment after adding it to sandbox".to_string(),
            )
        });
    }

    info!("Creating Python virtual environment at {:?}", env_path);
    // Check for Python interpreter using absolute paths first, which is more reliable in containers
    let python_candidates = &[
        "/usr/bin/python3",
        "/bin/python3",
        "/usr/local/bin/python3",
        "/usr/bin/python",
        "/bin/python",
        "/usr/local/bin/python",
        "python3",
        "python",
        "python3.12",
        "python3.11",
        "python3.10",
    ];

    let python_cmd = find_command(python_candidates);

    if python_cmd.is_none() {
        return Err(ExecError::RuntimeError(format!(
            "No Python interpreter found. Tried: {python_candidates:?}"
        )));
    }

    let python = python_cmd.ok_or_else(|| {
        ExecError::RuntimeError("Python interpreter not found despite validation".to_string())
    })?;

    // Try to create a virtual environment
    let env_path_str = env_path.to_str().ok_or_else(|| {
        ExecError::RuntimeError("Invalid path for Python virtual environment".to_string())
    })?;

    let output = Command::new(python)
        .args(["-m", "venv", env_path_str])
        .output();

    match output {
        Ok(output) => {
            if output.status.success() {
                info!("Python virtual environment created successfully");
                env.is_valid = true;

                // Add environment variables
                let virtual_env_path = env_path.to_str().ok_or_else(|| {
                    ExecError::RuntimeError("Invalid virtual environment path".to_string())
                })?;
                env.add_env_var("VIRTUAL_ENV", virtual_env_path);

                let bin_path_buf = env_path.join("bin");
                let bin_path = bin_path_buf.to_str().ok_or_else(|| {
                    ExecError::RuntimeError("Invalid bin path for virtual environment".to_string())
                })?;
                env.add_env_var(
                    "PATH",
                    &format!(
                        "{}:{}",
                        bin_path,
                        std::env::var("PATH").unwrap_or_else(|_| String::new())
                    ),
                );

                manager.add_environment(env);
                manager.get_environment("python").ok_or_else(|| {
                    ExecError::RuntimeError(
                        "Failed to retrieve Python environment after creation".to_string(),
                    )
                })
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                warn!("Failed to create Python virtual environment: {}", stderr);
                Err(ExecError::CommandFailed(format!(
                    "Failed to create Python virtual environment: {stderr}"
                )))
            }
        }
        Err(e) => {
            warn!("Failed to execute Python venv command: {}", e);

            // Try a simpler approach - create directory structure manually
            if let Err(e) = fs::create_dir_all(env_path.join("bin")) {
                warn!("Failed to create Python env directory structure: {}", e);
            }

            // Create an activation script that sets PATH
            let env_path_str = safe_path_to_str(&env_path)?;
            let bin_path = env_path.join("bin");
            let bin_path_str = safe_path_to_str(&bin_path)?;
            let activate_script = format!(
                "#!/bin/sh\nexport VIRTUAL_ENV=\"{env_path_str}\"\nexport PATH=\"{bin_path_str}:$PATH\"\n"
            );

            if let Err(e) = fs::write(env_path.join("bin").join("activate"), activate_script) {
                warn!("Failed to create activation script: {}", e);
            }

            // Create a simple wrapper script for python
            let env_path_str = safe_path_to_str(&env_path)?;
            let python_wrapper = format!(
                "#!/bin/sh\nexport PYTHONUSERBASE=\"{env_path_str}\"\n{python} \"$@\"\n"
            );

            let python_bin_path = env_path.join("bin").join("python");
            if let Err(e) = fs::write(&python_bin_path, python_wrapper) {
                warn!("Failed to create Python wrapper script: {}", e);
            }

            // Make it executable
            if let Err(e) = set_executable(&python_bin_path) {
                warn!("Failed to make Python wrapper executable: {}", e);
            } else {
                info!("Created minimal Python environment with wrapper script");
                env.is_valid = true;

                // Add environment variables
                let env_path_str = safe_path_to_str(&env_path)?;
                let bin_path = env_path.join("bin");
                let bin_path_str = safe_path_to_str(&bin_path)?;
                env.add_env_var("VIRTUAL_ENV", env_path_str);
                env.add_env_var("PYTHONUSERBASE", env_path_str);
                env.add_env_var(
                    "PATH",
                    &format!(
                        "{}:{}",
                        bin_path_str,
                        std::env::var("PATH").unwrap_or_else(|_| String::new())
                    ),
                );

                manager.add_environment(env);
                return manager.get_environment("python").ok_or_else(|| {
                    ExecError::RuntimeError(
                        "Failed to retrieve Python environment after creation".to_string(),
                    )
                });
            }

            Err(ExecError::CommandFailed(format!(
                "Failed to create Python environment: {e}"
            )))
        }
    }
}
