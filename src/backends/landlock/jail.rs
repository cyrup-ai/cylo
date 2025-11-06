// ============================================================================
// File: packages/cylo/src/backends/landlock/jail.rs
// ----------------------------------------------------------------------------
// Jail environment management for sandboxed code execution.
//
// Provides jail directory setup and validation including:
// - Path validation and security checks
// - Execution directory creation
// - Language-specific code file setup
// - Permission management
// ============================================================================

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::backends::{BackendError, BackendResult, ExecutionRequest};

/// Jail environment manager
pub struct JailEnvironment;

impl JailEnvironment {
    /// Validate jail directory path
    ///
    /// # Arguments
    /// * `jail_path` - Path to validate
    ///
    /// # Returns
    /// Ok(()) if path is valid, Err otherwise
    pub fn validate_path(jail_path: &Path) -> BackendResult<()> {
        // Must be absolute path for security
        if !jail_path.is_absolute() {
            return Err(BackendError::InvalidConfig {
                backend: "LandLock",
                details: "Jail path must be absolute".to_string(),
            });
        }

        // Check if path exists or can be created
        if !jail_path.exists() {
            if let Err(e) = fs::create_dir_all(jail_path) {
                return Err(BackendError::InvalidConfig {
                    backend: "LandLock",
                    details: format!(
                        "Cannot create jail directory {}: {}",
                        jail_path.display(),
                        e
                    ),
                });
            }
        }

        // Verify path is a directory
        if !jail_path.is_dir() {
            return Err(BackendError::InvalidConfig {
                backend: "LandLock",
                details: format!("Jail path {} is not a directory", jail_path.display()),
            });
        }

        // Check permissions - must be writable
        if let Ok(metadata) = jail_path.metadata() {
            let permissions = metadata.permissions();
            if permissions.mode() & 0o200 == 0 {
                return Err(BackendError::InvalidConfig {
                    backend: "LandLock",
                    details: "Jail directory is not writable".to_string(),
                });
            }
        }

        Ok(())
    }

    /// Setup jail environment for execution
    ///
    /// # Arguments
    /// * `jail_path` - Base jail directory
    /// * `request` - Execution request
    ///
    /// # Returns
    /// Path to execution directory within jail
    pub fn setup_environment(
        jail_path: &Path,
        request: &ExecutionRequest,
    ) -> BackendResult<PathBuf> {
        // Create unique execution directory
        let exec_id = format!(
            "exec-{}-{}",
            uuid::Uuid::new_v4().simple(),
            std::process::id()
        );
        let exec_dir = jail_path.join(&exec_id);

        // Create execution directory
        fs::create_dir_all(&exec_dir).map_err(|e| BackendError::FileSystemFailed {
            details: format!("Failed to create execution directory: {}", e),
        })?;

        // Set proper permissions (rwx for owner only)
        fs::set_permissions(&exec_dir, fs::Permissions::from_mode(0o700)).map_err(|e| {
            BackendError::FileSystemFailed {
                details: format!("Failed to set directory permissions: {}", e),
            }
        })?;

        // Create working directory if specified
        if let Some(workdir) = &request.working_dir {
            let work_path = exec_dir.join(workdir.trim_start_matches('/'));
            fs::create_dir_all(&work_path).map_err(|e| BackendError::FileSystemFailed {
                details: format!("Failed to create working directory: {}", e),
            })?;
        }

        // Create language-specific code files
        Self::create_code_file(&exec_dir, request)?;

        Ok(exec_dir)
    }

    /// Create code file for specific language
    ///
    /// # Arguments
    /// * `exec_dir` - Execution directory
    /// * `request` - Execution request
    ///
    /// # Returns
    /// Ok(()) if successful, Err otherwise
    fn create_code_file(exec_dir: &Path, request: &ExecutionRequest) -> BackendResult<()> {
        match request.language.as_str() {
            "python" | "python3" => {
                let code_file = exec_dir.join("main.py");
                fs::write(&code_file, &request.code).map_err(|e| {
                    BackendError::FileSystemFailed {
                        details: format!("Failed to write Python code file: {}", e),
                    }
                })?;
            }
            "rust" => {
                let code_file = exec_dir.join("main.rs");
                fs::write(&code_file, &request.code).map_err(|e| {
                    BackendError::FileSystemFailed {
                        details: format!("Failed to write Rust code file: {}", e),
                    }
                })?;
            }
            "javascript" | "js" | "node" => {
                let code_file = exec_dir.join("main.js");
                fs::write(&code_file, &request.code).map_err(|e| {
                    BackendError::FileSystemFailed {
                        details: format!("Failed to write JavaScript code file: {}", e),
                    }
                })?;
            }
            "go" => {
                let code_file = exec_dir.join("main.go");
                fs::write(&code_file, &request.code).map_err(|e| {
                    BackendError::FileSystemFailed {
                        details: format!("Failed to write Go code file: {}", e),
                    }
                })?;
            }
            _ => {
                // For shell scripts and other languages, write to a generic file
                let code_file = exec_dir.join("code");
                fs::write(&code_file, &request.code).map_err(|e| {
                    BackendError::FileSystemFailed {
                        details: format!("Failed to write code file: {}", e),
                    }
                })?;

                // Make executable for shell scripts
                if matches!(request.language.as_str(), "bash" | "sh") {
                    fs::set_permissions(&code_file, fs::Permissions::from_mode(0o755)).map_err(
                        |e| BackendError::FileSystemFailed {
                            details: format!("Failed to set executable permissions: {}", e),
                        },
                    )?;
                }
            }
        }

        Ok(())
    }

    /// Clean up execution directory
    ///
    /// # Arguments
    /// * `exec_dir` - Execution directory to remove
    pub fn cleanup(exec_dir: &Path) {
        let _ = fs::remove_dir_all(exec_dir);
    }

    /// Clean up leftover execution directories
    ///
    /// # Arguments
    /// * `jail_path` - Base jail directory
    pub fn cleanup_all(jail_path: &Path) {
        if let Ok(entries) = fs::read_dir(jail_path) {
            for entry in entries.filter_map(Result::ok) {
                if let Ok(file_name) = entry.file_name().into_string() {
                    if file_name.starts_with("exec-") {
                        let _ = fs::remove_dir_all(entry.path());
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_validation() {
        // Valid absolute path should pass
        let temp_dir = std::env::temp_dir().join("cylo_test_jail");
        assert!(JailEnvironment::validate_path(&temp_dir).is_ok());
        let _ = fs::remove_dir_all(&temp_dir);

        // Relative path should fail
        let relative_path = PathBuf::from("relative/path");
        assert!(JailEnvironment::validate_path(&relative_path).is_err());
    }
}
