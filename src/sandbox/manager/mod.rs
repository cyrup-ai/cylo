use std::{
    fs,
    path::{Path, PathBuf},
};

use log::{debug, error, warn};

use crate::{error::Result, sandbox::environment::SandboxedEnvironment};

mod go;
mod node;
mod python;
mod rust;

/// Manages sandboxed environments for different language runtimes
pub struct SandboxManager {
    /// Base directory for all sandboxed environments
    base_dir: PathBuf,
    /// Map of created environments
    environments: Vec<SandboxedEnvironment>,
}

impl SandboxManager {
    /// Create a new SandboxManager with the specified base directory
    pub fn new(base_dir: impl AsRef<Path>) -> Self {
        let base_dir = base_dir.as_ref().to_path_buf();

        // Ensure the base directory exists
        if !base_dir.exists()
            && let Err(e) = fs::create_dir_all(&base_dir)
        {
            error!("Failed to create sandbox base directory: {}", e);
        }

        Self {
            base_dir,
            environments: Vec::new(),
        }
    }

    /// Add an environment to the manager
    pub fn add_environment(&mut self, env: SandboxedEnvironment) {
        self.environments.push(env);
    }

    /// Get an environment by type
    pub fn get_environment(&self, env_type: &str) -> Option<&SandboxedEnvironment> {
        self.environments
            .iter()
            .find(|env| env.env_type == env_type)
    }

    /// Get the base directory path
    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    /// Create a Python virtual environment
    pub fn create_python_environment(&mut self, name: &str) -> Result<&SandboxedEnvironment> {
        python::create_python_environment_impl(self, name)
    }

    /// Create a Node.js environment using fnm or a simple directory structure
    pub fn create_node_environment(&mut self, name: &str) -> Result<&SandboxedEnvironment> {
        node::create_node_environment_impl(self, name)
    }

    /// Create a Rust environment with its own cargo directory
    pub fn create_rust_environment(&mut self, name: &str) -> Result<&SandboxedEnvironment> {
        rust::create_rust_environment_impl(self, name)
    }

    /// Create a Go environment with its own GOPATH and workspace
    pub fn create_go_environment(&mut self, name: &str) -> Result<&SandboxedEnvironment> {
        go::create_go_environment_impl(self, name)
    }

    /// Clean up all environments
    pub fn cleanup(&self) -> Result<()> {
        for env in &self.environments {
            debug!("Cleaning up environment at {:?}", env.path);
            if env.path.exists()
                && let Err(e) = fs::remove_dir_all(&env.path)
            {
                warn!("Failed to clean up environment at {:?}: {}", env.path, e);
            }
        }
        Ok(())
    }
}
