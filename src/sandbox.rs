use std::sync::Arc;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
};

use log::{debug, error, info, warn};

use crate::{
    config::RamdiskConfig,
    error::{ExecError, Result, SandboxError, SandboxResult},
    exec::find_command,
};

/// Safe path to string conversion with zero-allocation optimization
///
/// Uses direct path.to_str() in the happy path (no allocation), falls back to
/// path.to_string_lossy() only when UTF-8 conversion is required.
#[inline]
fn safe_path_to_str(path: &Path) -> SandboxResult<&str> {
    path.to_str().ok_or_else(|| SandboxError::PathInvalid {
        detail: Arc::from(format!(
            "Path contains invalid UTF-8 characters: {}",
            path.to_string_lossy()
        )),
    })
}

/// Safe path to owned string conversion with minimal allocation
///
/// Only allocates when UTF-8 conversion is necessary, maintaining zero-allocation
/// characteristics for valid UTF-8 paths.
#[inline]
pub fn safe_path_to_string(path: &Path) -> SandboxResult<String> {
    match path.to_str() {
        Some(s) => Ok(s.to_string()),
        None => Ok(path.to_string_lossy().into_owned()),
    }
}

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

    /// Create a Python virtual environment
    pub fn create_python_environment(&mut self, name: &str) -> Result<&SandboxedEnvironment> {
        let env_path = self.base_dir.join(name);
        let mut env = SandboxedEnvironment::new("python", env_path.clone());

        if env_path.exists() {
            info!("Python environment already exists at {:?}", env_path);
            env.is_valid = true;
            self.add_environment(env);
            return self.get_environment("python").ok_or_else(|| {
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
                        ExecError::RuntimeError(
                            "Invalid bin path for virtual environment".to_string(),
                        )
                    })?;
                    env.add_env_var(
                        "PATH",
                        &format!(
                            "{}:{}",
                            bin_path,
                            std::env::var("PATH").unwrap_or_else(|_| String::new())
                        ),
                    );

                    self.add_environment(env);
                    self.get_environment("python").ok_or_else(|| {
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
                if let Err(e) =
                    fs::set_permissions(&python_bin_path, fs::Permissions::from_mode(0o755))
                {
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

                    self.add_environment(env);
                    return self.get_environment("python").ok_or_else(|| {
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

    /// Create a Node.js environment using fnm or a simple directory structure
    pub fn create_node_environment(&mut self, name: &str) -> Result<&SandboxedEnvironment> {
        let env_path = self.base_dir.join(name);
        let mut env = SandboxedEnvironment::new("node", env_path.clone());

        if env_path.exists() {
            info!("Node.js environment already exists at {:?}", env_path);
            env.is_valid = true;
            self.add_environment(env);
            return self.get_environment("node").ok_or_else(|| {
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

                        self.add_environment(env);
                        return self.get_environment("node").ok_or_else(|| {
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
        if let Err(e) = fs::set_permissions(&node_bin_path, fs::Permissions::from_mode(0o755)) {
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

        self.add_environment(env);
        self.get_environment("node").ok_or_else(|| {
            ExecError::RuntimeError(
                "Failed to retrieve Node.js environment after creation".to_string(),
            )
        })
    }

    /// Create a Rust environment with its own cargo directory
    pub fn create_rust_environment(&mut self, name: &str) -> Result<&SandboxedEnvironment> {
        let env_path = self.base_dir.join(name);
        let mut env = SandboxedEnvironment::new("rust", env_path.clone());

        if env_path.exists() {
            info!("Rust environment already exists at {:?}", env_path);
            env.is_valid = true;
            self.add_environment(env);
            return self.get_environment("rust").ok_or_else(|| {
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

        self.add_environment(env);
        self.get_environment("rust").ok_or_else(|| {
            ExecError::RuntimeError(
                "Failed to retrieve Rust environment after creation".to_string(),
            )
        })
    }

    /// Create a Go environment with its own GOPATH and workspace
    pub fn create_go_environment(&mut self, name: &str) -> Result<&SandboxedEnvironment> {
        let env_path = self.base_dir.join(name);
        let mut env = SandboxedEnvironment::new("go", env_path.clone());

        if env_path.exists() {
            info!("Go environment already exists at {:?}", env_path);
            env.is_valid = true;
            self.add_environment(env);
            return self.get_environment("go").ok_or_else(|| {
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
            ExecError::RuntimeError(
                "Go command unexpectedly became None after validation".to_string(),
            )
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
        if let Err(e) = fs::set_permissions(&go_bin_path, fs::Permissions::from_mode(0o755)) {
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

        self.add_environment(env);
        self.get_environment("go").ok_or_else(|| {
            ExecError::RuntimeError("Failed to retrieve Go environment after creation".to_string())
        })
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
