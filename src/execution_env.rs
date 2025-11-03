// ============================================================================
// File: packages/cylo/src/execution_env.rs
// ----------------------------------------------------------------------------
// Core execution environment types and instance management for Cylo.
//
// Provides a fluent API for specifying execution environments:
// - Cylo::LandLock("/path/to/jail").instance("name")
// - Cylo::FireCracker("rust:alpine3.20").instance("name")
// - Cylo::Apple("python:alpine3.20").instance("name")
//
// Zero allocation patterns with string interning and efficient enum dispatch.
// ============================================================================

use std::fmt;

use serde::{Deserialize, Serialize};

/// Core execution environment specification
///
/// Each variant represents a different secure execution backend:
/// - LandLock: Linux kernel-based sandboxing with filesystem restrictions
/// - FireCracker: Lightweight microVMs for complete isolation
/// - Apple: Apple's containerization framework for macOS
/// - SweetMcpPlugin: WASM-based SweetMCP plugin execution
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Cylo {
    /// LandLock backend with jail directory path
    /// Example: Cylo::LandLock("/tmp/sandbox")
    LandLock(String),

    /// FireCracker backend with container image specification
    /// Example: Cylo::FireCracker("rust:alpine3.20")
    FireCracker(String),

    /// Apple containerization backend with image specification
    /// Example: Cylo::Apple("python:alpine3.20")
    Apple(String),

    /// SweetMCP plugin execution with plugin path
    /// Example: Cylo::SweetMcpPlugin("./plugins/eval-py.wasm")
    SweetMcpPlugin(String),
}

impl Cylo {
    /// Create a named instance of this execution environment
    ///
    /// This allows mapping arbitrary names to specific execution configurations
    /// for reuse across multiple tool invocations.
    ///
    /// # Arguments
    /// * `name` - Unique identifier for this instance
    ///
    /// # Returns
    /// CyloInstance configured with this environment and name
    ///
    /// # Example
    /// ```rust
    /// let instance = Cylo::Apple("python:alpine3.20".to_string()).instance("python_env");
    /// ```
    #[inline]
    pub fn instance<N: Into<String>>(self, name: N) -> CyloInstance {
        CyloInstance {
            env: self,
            name: name.into(),
        }
    }

    /// Validate the configuration string for this backend
    ///
    /// Performs backend-specific validation:
    /// - LandLock: Validates path exists and is accessible
    /// - FireCracker: Validates image format and registry accessibility
    /// - Apple: Validates image format and platform compatibility
    ///
    /// # Returns
    /// Ok(()) if configuration is valid, Err(CyloError) otherwise
    pub fn validate(&self) -> Result<(), CyloError> {
        match self {
            Cylo::LandLock(path) => {
                if path.is_empty() {
                    return Err(CyloError::InvalidConfiguration {
                        backend: "LandLock",
                        message: "Path cannot be empty",
                    });
                }

                // Validate path format - must be absolute for security
                if !path.starts_with('/') {
                    return Err(CyloError::InvalidConfiguration {
                        backend: "LandLock",
                        message: "LandLock path must be absolute",
                    });
                }

                Ok(())
            }

            Cylo::FireCracker(image) => {
                if image.is_empty() {
                    return Err(CyloError::InvalidConfiguration {
                        backend: "FireCracker",
                        message: "Image specification cannot be empty",
                    });
                }

                // Validate basic image format: name:tag or registry/name:tag
                if !image.contains(':') {
                    return Err(CyloError::InvalidConfiguration {
                        backend: "FireCracker",
                        message: "Image must include tag (e.g., 'rust:alpine3.20')",
                    });
                }

                Ok(())
            }

            Cylo::Apple(image) => {
                if image.is_empty() {
                    return Err(CyloError::InvalidConfiguration {
                        backend: "Apple",
                        message: "Image specification cannot be empty",
                    });
                }

                // Validate basic image format for Apple containerization
                if !image.contains(':') {
                    return Err(CyloError::InvalidConfiguration {
                        backend: "Apple",
                        message: "Image must include tag (e.g., 'python:alpine3.20')",
                    });
                }

                Ok(())
            }

            Cylo::SweetMcpPlugin(plugin_path) => {
                if plugin_path.is_empty() {
                    return Err(CyloError::InvalidConfiguration {
                        backend: "SweetMcpPlugin",
                        message: "Plugin path cannot be empty",
                    });
                }

                // Validate plugin file extension
                if !plugin_path.ends_with(".wasm") {
                    return Err(CyloError::InvalidConfiguration {
                        backend: "SweetMcpPlugin",
                        message: "Plugin file must have .wasm extension",
                    });
                }

                Ok(())
            }
        }
    }

    /// Get the backend type as a string
    #[inline]
    pub fn backend_type(&self) -> &'static str {
        match self {
            Cylo::LandLock(_) => "LandLock",
            Cylo::FireCracker(_) => "FireCracker",
            Cylo::Apple(_) => "Apple",
            Cylo::SweetMcpPlugin(_) => "SweetMcpPlugin",
        }
    }

    /// Get the configuration value
    #[inline]
    pub fn config(&self) -> &str {
        match self {
            Cylo::LandLock(path) => path,
            Cylo::FireCracker(image) => image,
            Cylo::Apple(image) => image,
            Cylo::SweetMcpPlugin(plugin_path) => plugin_path,
        }
    }
}

impl fmt::Display for Cylo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Cylo::LandLock(path) => write!(f, "LandLock({path})"),
            Cylo::FireCracker(image) => write!(f, "FireCracker({image})"),
            Cylo::Apple(image) => write!(f, "Apple({image})"),
            Cylo::SweetMcpPlugin(plugin_path) => write!(f, "SweetMcpPlugin({plugin_path})"),
        }
    }
}

/// Named instance of a Cylo execution environment
///
/// Combines an execution environment specification with a unique name
/// for tracking and reuse across multiple executions.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CyloInstance {
    /// The execution environment configuration
    pub env: Cylo,
    /// Unique name for this instance
    pub name: String,
}

impl CyloInstance {
    /// Create a new CyloInstance
    ///
    /// # Arguments
    /// * `env` - The execution environment specification
    /// * `name` - Unique identifier for this instance
    pub fn new<N: Into<String>>(env: Cylo, name: N) -> Self {
        Self {
            env,
            name: name.into(),
        }
    }

    /// Validate this instance configuration
    ///
    /// Performs validation on both the environment configuration
    /// and instance name requirements.
    pub fn validate(&self) -> Result<(), CyloError> {
        // Validate environment configuration
        self.env.validate()?;

        // Validate instance name
        if self.name.is_empty() {
            return Err(CyloError::InvalidConfiguration {
                backend: self.env.backend_type(),
                message: "Instance name cannot be empty",
            });
        }

        // Instance names must be valid identifiers for security
        if !self
            .name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
        {
            return Err(CyloError::InvalidConfiguration {
                backend: self.env.backend_type(),
                message: "Instance name must contain only alphanumeric characters, hyphens, and underscores",
            });
        }

        Ok(())
    }

    /// Get the instance identifier for tracking
    #[inline]
    pub fn id(&self) -> String {
        format!("{}:{}", self.env.backend_type(), self.name)
    }
}

impl fmt::Display for CyloInstance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.instance(\"{}\")", self.env, self.name)
    }
}

/// Comprehensive error type for Cylo operations
///
/// Covers all error scenarios across different backends and operations
/// with detailed context for debugging and user feedback.
#[derive(Debug, Clone, thiserror::Error)]
pub enum CyloError {
    /// Invalid configuration for a specific backend
    #[error("Invalid {backend} configuration: {message}")]
    InvalidConfiguration {
        backend: &'static str,
        message: &'static str,
    },

    /// Platform does not support the requested backend
    #[error("Platform does not support {backend} backend: {details}")]
    PlatformUnsupported {
        backend: &'static str,
        details: String,
    },

    /// Backend is available but not currently functional
    #[error("Backend {backend} is unavailable: {reason}")]
    BackendUnavailable {
        backend: &'static str,
        reason: String,
    },

    /// Named instance not found in the registry
    #[error("Instance '{name}' not found in registry")]
    InstanceNotFound { name: String },

    /// Instance with the same name already exists
    #[error("Instance '{name}' already exists with different configuration")]
    InstanceConflict { name: String },

    /// Execution failed in the specified environment
    #[error("Execution failed in {backend} environment: {details}")]
    ExecutionFailed {
        backend: &'static str,
        details: String,
    },

    /// Timeout occurred during execution
    #[error("Execution timeout in {backend} environment after {timeout_secs}s")]
    ExecutionTimeout {
        backend: &'static str,
        timeout_secs: u64,
    },

    /// Resource limits exceeded
    #[error("Resource limit exceeded in {backend}: {resource} limit {limit}")]
    ResourceLimitExceeded {
        backend: &'static str,
        resource: String,
        limit: String,
    },

    /// Internal system error
    #[error("Internal system error: {message}")]
    Internal { message: String },

    /// Validation error
    #[error("Validation error: {message}")]
    Validation { message: String },
}

impl CyloError {
    /// Create a platform unsupported error with context
    pub fn platform_unsupported(backend: &'static str, details: impl Into<String>) -> Self {
        Self::PlatformUnsupported {
            backend,
            details: details.into(),
        }
    }

    /// Create a backend unavailable error with reason
    pub fn backend_unavailable(backend: &'static str, reason: impl Into<String>) -> Self {
        Self::BackendUnavailable {
            backend,
            reason: reason.into(),
        }
    }

    /// Create an execution failed error with details
    pub fn execution_failed(backend: &'static str, details: impl Into<String>) -> Self {
        Self::ExecutionFailed {
            backend,
            details: details.into(),
        }
    }

    /// Create an internal error with message
    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal {
            message: message.into(),
        }
    }

    /// Create a validation error with message
    pub fn validation(message: impl Into<String>) -> Self {
        Self::Validation {
            message: message.into(),
        }
    }
}

impl From<tokio::task::JoinError> for CyloError {
    fn from(error: tokio::task::JoinError) -> Self {
        Self::Internal {
            message: format!("Task join error: {error}"),
        }
    }
}

// Result type alias for convenience
pub type CyloResult<T> = Result<T, CyloError>;

/// Validate an instance name for compliance with naming rules
///
/// Instance names must:
/// - Be non-empty
/// - Contain only alphanumeric characters, hyphens, and underscores
/// - Not start or end with hyphens or underscores
///
/// # Arguments
/// * `name` - The instance name to validate
///
/// # Returns
/// * `Ok(())` if the name is valid
/// * `Err(CyloError)` if the name is invalid
pub fn validate_instance_name(name: &str) -> CyloResult<()> {
    if name.is_empty() {
        return Err(CyloError::validation("Instance name cannot be empty"));
    }

    if name.len() > 63 {
        return Err(CyloError::validation(
            "Instance name cannot exceed 63 characters",
        ));
    }

    // Check first and last characters
    let first_char = name
        .chars()
        .next()
        .ok_or_else(|| CyloError::validation("Instance name cannot be empty"))?;
    let last_char = name
        .chars()
        .last()
        .ok_or_else(|| CyloError::validation("Instance name cannot be empty"))?;

    if first_char == '-' || first_char == '_' {
        return Err(CyloError::validation(
            "Instance name cannot start with hyphen or underscore",
        ));
    }

    if last_char == '-' || last_char == '_' {
        return Err(CyloError::validation(
            "Instance name cannot end with hyphen or underscore",
        ));
    }

    // Check all characters are valid
    for ch in name.chars() {
        if !ch.is_alphanumeric() && ch != '-' && ch != '_' {
            return Err(CyloError::validation(
                "Instance name can only contain alphanumeric characters, hyphens, and underscores",
            ));
        }
    }

    Ok(())
}

/// Validate an environment specification for the given backend type
///
/// Different backends have different validation requirements:
/// - LandLock: Path must be absolute and exist
/// - FireCracker/Apple: Image specification must include tag
///
/// # Arguments
/// * `env` - The Cylo environment to validate
///
/// # Returns
/// * `Ok(())` if the environment specification is valid
/// * `Err(CyloError)` if the specification is invalid
pub fn validate_environment_spec(env: &Cylo) -> CyloResult<()> {
    match env {
        Cylo::LandLock(path) => {
            if path.is_empty() {
                return Err(CyloError::validation("LandLock path cannot be empty"));
            }

            if !path.starts_with('/') {
                return Err(CyloError::validation("LandLock path must be absolute"));
            }

            Ok(())
        }
        Cylo::FireCracker(image) | Cylo::Apple(image) => {
            if image.is_empty() {
                return Err(CyloError::validation(
                    "Container image specification cannot be empty",
                ));
            }

            if !image.contains(':') {
                return Err(CyloError::validation(
                    "Container image must include tag (e.g., 'image:tag')",
                ));
            }

            let parts: Vec<&str> = image.split(':').collect();
            if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() {
                return Err(CyloError::validation(
                    "Invalid container image format (expected 'name:tag')",
                ));
            }

            Ok(())
        }
        Cylo::SweetMcpPlugin(plugin_path) => {
            if plugin_path.is_empty() {
                return Err(CyloError::validation("Plugin path cannot be empty"));
            }

            if !plugin_path.ends_with(".wasm") {
                return Err(CyloError::validation(
                    "Plugin file must have .wasm extension",
                ));
            }

            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cylo_landlock_creation() {
        let cylo = Cylo::LandLock("/tmp/sandbox".to_string());
        assert_eq!(cylo.backend_type(), "LandLock");
        assert_eq!(cylo.config(), "/tmp/sandbox");
    }

    #[test]
    fn cylo_firecracker_creation() {
        let cylo = Cylo::FireCracker("rust:alpine3.20".to_string());
        assert_eq!(cylo.backend_type(), "FireCracker");
        assert_eq!(cylo.config(), "rust:alpine3.20");
    }

    #[test]
    fn cylo_apple_creation() {
        let cylo = Cylo::Apple("python:alpine3.20".to_string());
        assert_eq!(cylo.backend_type(), "Apple");
        assert_eq!(cylo.config(), "python:alpine3.20");
    }

    #[test]
    fn instance_creation() {
        let instance = Cylo::Apple("python:alpine3.20".to_string()).instance("test_env");
        assert_eq!(instance.name, "test_env");
        assert_eq!(instance.env.backend_type(), "Apple");
        assert_eq!(instance.id(), "Apple:test_env");
    }

    #[test]
    fn landlock_path_validation() {
        // Valid absolute path
        let valid = Cylo::LandLock("/tmp/sandbox".to_string());
        assert!(valid.validate().is_ok());

        // Invalid relative path
        let invalid = Cylo::LandLock("relative/path".to_string());
        assert!(invalid.validate().is_err());

        // Empty path
        let empty = Cylo::LandLock("".to_string());
        assert!(empty.validate().is_err());
    }

    #[test]
    fn image_validation() {
        // Valid image with tag
        let valid = Cylo::FireCracker("rust:alpine3.20".to_string());
        assert!(valid.validate().is_ok());

        // Invalid image without tag
        let invalid = Cylo::FireCracker("rust".to_string());
        assert!(invalid.validate().is_err());

        // Empty image
        let empty = Cylo::Apple("".to_string());
        assert!(empty.validate().is_err());
    }

    #[test]
    fn instance_name_validation() {
        let env = Cylo::Apple("python:alpine3.20".to_string());

        // Valid name
        let valid = CyloInstance::new(env.clone(), "valid_name-123");
        assert!(valid.validate().is_ok());

        // Invalid name with special characters
        let invalid = CyloInstance::new(env.clone(), "invalid@name!");
        assert!(invalid.validate().is_err());

        // Empty name
        let empty = CyloInstance::new(env, "");
        assert!(empty.validate().is_err());
    }

    #[test]
    fn display_formatting() {
        let cylo = Cylo::Apple("python:alpine3.20".to_string());
        let instance = cylo.instance("test_env");

        assert_eq!(
            format!("{}", instance),
            "Apple(python:alpine3.20).instance(\"test_env\")"
        );
    }
}
