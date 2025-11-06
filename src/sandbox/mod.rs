mod environment;
mod manager;
mod path_utils;

pub use environment::SandboxedEnvironment;
pub use manager::SandboxManager;
pub use path_utils::{safe_path_to_str, safe_path_to_string};

use log::info;

use crate::{config::RamdiskConfig, error::Result};

/// Helper function to create a Python virtual environment
///
/// Creates an isolated Python environment with its own site-packages and Python interpreter
/// within the secure ramdisk.
///
/// # Arguments
/// * `config` - Ramdisk configuration with mount point
///
/// # Returns
/// * A configured SandboxedEnvironment with Python-specific environment variables
/// * Error if environment creation fails
pub fn create_python_venv(config: &RamdiskConfig) -> Result<SandboxedEnvironment> {
    // Always use the ramdisk path for security
    let ramdisk_path = config.mount_point.clone();

    info!(
        "Creating Python virtual environment inside ramdisk at: {}",
        ramdisk_path.display()
    );

    let mut sandbox_manager = SandboxManager::new(ramdisk_path);
    match sandbox_manager.create_python_environment("python_venv") {
        Ok(env) => {
            let mut env_copy = SandboxedEnvironment::new("python", env.path.clone());
            env_copy.is_valid = env.is_valid;
            env_copy.env_vars = env.env_vars.clone();
            Ok(env_copy)
        }
        Err(e) => Err(e),
    }
}

/// Helper function to create a Node.js environment
///
/// Creates an isolated Node.js environment with its own node_modules directory
/// within the secure ramdisk.
///
/// # Arguments
/// * `config` - Ramdisk configuration with mount point
///
/// # Returns
/// * A configured SandboxedEnvironment with Node.js-specific environment variables
/// * Error if environment creation fails
pub fn create_node_environment(config: &RamdiskConfig) -> Result<SandboxedEnvironment> {
    // Always use the ramdisk path for security
    let ramdisk_path = config.mount_point.clone();

    info!(
        "Creating Node.js environment inside ramdisk at: {}",
        ramdisk_path.display()
    );

    let mut sandbox_manager = SandboxManager::new(ramdisk_path);
    match sandbox_manager.create_node_environment("node_env") {
        Ok(env) => {
            let mut env_copy = SandboxedEnvironment::new("node", env.path.clone());
            env_copy.is_valid = env.is_valid;
            env_copy.env_vars = env.env_vars.clone();
            Ok(env_copy)
        }
        Err(e) => Err(e),
    }
}

/// Helper function to create a Rust environment
///
/// Creates an isolated Rust environment with its own Cargo home directory
/// within the secure ramdisk.
///
/// # Arguments
/// * `config` - Ramdisk configuration with mount point
///
/// # Returns
/// * A configured SandboxedEnvironment with Rust-specific environment variables
/// * Error if environment creation fails
pub fn create_rust_environment(config: &RamdiskConfig) -> Result<SandboxedEnvironment> {
    // Always use the ramdisk path for security
    let ramdisk_path = config.mount_point.clone();

    info!(
        "Creating Rust environment inside ramdisk at: {}",
        ramdisk_path.display()
    );

    let mut sandbox_manager = SandboxManager::new(ramdisk_path);
    match sandbox_manager.create_rust_environment("rust_env") {
        Ok(env) => {
            let mut env_copy = SandboxedEnvironment::new("rust", env.path.clone());
            env_copy.is_valid = env.is_valid;
            env_copy.env_vars = env.env_vars.clone();
            Ok(env_copy)
        }
        Err(e) => Err(e),
    }
}

/// Helper function to create a Go environment
///
/// Creates an isolated Go environment with its own GOPATH and temporary workspace
/// within the secure ramdisk.
///
/// # Arguments
/// * `config` - Ramdisk configuration with mount point
///
/// # Returns
/// * A configured SandboxedEnvironment with Go-specific environment variables
/// * Error if environment creation fails
pub fn create_go_environment(config: &RamdiskConfig) -> Result<SandboxedEnvironment> {
    // Always use the ramdisk path for security
    let ramdisk_path = config.mount_point.clone();

    info!(
        "Creating Go environment inside ramdisk at: {}",
        ramdisk_path.display()
    );

    let mut sandbox_manager = SandboxManager::new(ramdisk_path);
    match sandbox_manager.create_go_environment("go_env") {
        Ok(env) => {
            let mut env_copy = SandboxedEnvironment::new("go", env.path.clone());
            env_copy.is_valid = env.is_valid;
            env_copy.env_vars = env.env_vars.clone();
            Ok(env_copy)
        }
        Err(e) => Err(e),
    }
}
