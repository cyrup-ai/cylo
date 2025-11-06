use std::{path::PathBuf, process::Command, sync::Arc};

use crate::error::{SandboxError, SandboxResult};

/// Represents a sandboxed environment for a specific language runtime
///
/// A SandboxedEnvironment provides isolation for a language runtime by:
/// 1. Creating a dedicated directory structure for the runtime
/// 2. Setting up environment variables for isolation
/// 3. Providing wrapper scripts that enforce the isolation
/// 4. Offering a standard interface to interact with the environment
///
/// This approach provides security through isolation, ensuring that:
/// - Code execution happens in a contained environment
/// - Language runtimes can't access system libraries or user directories outside the sandbox
/// - Dependencies are localized to the sandboxed environment
/// - Runtime behavior is predictable and repeatable
pub struct SandboxedEnvironment {
    /// Type of environment (python, node, rust, go, etc.)
    pub env_type: String,
    /// Path to the environment directory
    pub path: PathBuf,
    /// Environment variables to set when using this environment
    pub env_vars: Vec<(String, String)>,
    /// Whether the environment was successfully created
    pub is_valid: bool,
}

impl SandboxedEnvironment {
    /// Create a new sandboxed environment with the specified type and path
    pub fn new(env_type: &str, path: PathBuf) -> Self {
        Self {
            env_type: env_type.to_string(),
            path,
            env_vars: Vec::new(),
            is_valid: false,
        }
    }

    /// Add an environment variable to be set when using this environment
    pub fn add_env_var(&mut self, key: &str, value: &str) {
        self.env_vars.push((key.to_string(), value.to_string()));
    }

    /// Get the path to a binary in this environment
    ///
    /// Returns the absolute path to a binary within the sandboxed environment.
    /// This will typically be a wrapper script that sets the appropriate
    /// environment variables before invoking the real binary.
    pub fn get_binary_path(&self, binary_name: &str) -> PathBuf {
        match self.env_type.as_str() {
            "python" => self.path.join("bin").join(binary_name),
            "node" => self.path.join("bin").join(binary_name),
            "rust" => self.path.join("bin").join(binary_name),
            "go" => self.path.join("bin").join(binary_name),
            _ => PathBuf::from(binary_name),
        }
    }

    /// Execute a command with this environment's configuration
    ///
    /// Runs a command within the sandboxed environment, with all the appropriate
    /// environment variables set to ensure isolation.
    ///
    /// # Arguments
    /// * `command` - The name of the command to execute (will be resolved within the environment)
    /// * `args` - Arguments to pass to the command
    ///
    /// # Returns
    /// * The command's stdout output if successful
    /// * An error if the command fails or the environment is invalid
    pub fn execute_command(&self, command: &str, args: &[&str]) -> SandboxResult<String> {
        if !self.is_valid {
            return Err(SandboxError::EnvironmentInvalid {
                detail: Arc::from("Cannot execute command in invalid sandbox environment"),
            });
        }

        let binary_path = self.get_binary_path(command);
        let mut cmd = Command::new(binary_path);
        cmd.args(args);

        // Add environment variables
        for (key, value) in &self.env_vars {
            cmd.env(key, value);
        }

        let output = cmd.output().map_err(|e| SandboxError::ProcessLaunch {
            detail: Arc::from(format!("Failed to launch command '{command}': {e}")),
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(SandboxError::ProcessLaunch {
                detail: Arc::from(format!(
                    "Failed to execute {command} command in sandbox: {stderr}"
                )),
            })
        } else {
            let stdout = String::from_utf8_lossy(&output.stdout);
            Ok(stdout.to_string())
        }
    }
}
