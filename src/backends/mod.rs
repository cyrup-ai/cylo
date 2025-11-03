// ============================================================================
// File: packages/cylo/src/backends/mod.rs
// ----------------------------------------------------------------------------
// Backend trait definitions and module organization for Cylo execution environments.
//
// Provides a unified interface for different secure execution backends:
// - ExecutionBackend trait for common operations
// - Backend-specific error types and configurations
// - Platform-conditional module loading
// - AsyncTask-based async patterns (never async fn)
// ============================================================================

use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};
// Local AsyncTask type alias to avoid circular dependency with fluent_ai_domain
pub type AsyncTask<T> = tokio::task::JoinHandle<T>;
use crate::execution_env::{CyloError, CyloResult};

/// Core execution backend trait
///
/// All backends must implement this trait to provide secure code execution
/// capabilities. Uses AsyncTask pattern throughout for zero-allocation async.
pub trait ExecutionBackend: Send + Sync + std::fmt::Debug {
    /// Execute code in this backend environment
    ///
    /// # Arguments
    /// * `request` - Execution request with code, language, and configuration
    ///
    /// # Returns
    /// AsyncTask that resolves to execution result
    fn execute_code(&self, request: ExecutionRequest) -> AsyncTask<ExecutionResult>;

    /// Perform health check on this backend
    ///
    /// Verifies that the backend is available and functional.
    /// Should be fast and non-destructive.
    ///
    /// # Returns
    /// AsyncTask that resolves to health status
    fn health_check(&self) -> AsyncTask<HealthStatus>;

    /// Clean up resources for this backend
    ///
    /// Called when the backend instance is no longer needed.
    /// Should clean up any persistent resources, containers, or processes.
    ///
    /// # Returns
    /// AsyncTask that resolves when cleanup is complete
    fn cleanup(&self) -> AsyncTask<CyloResult<()>>;

    /// Get backend-specific configuration
    ///
    /// Returns the current configuration for this backend instance.
    fn get_config(&self) -> &BackendConfig;

    /// Get the backend type identifier
    fn backend_type(&self) -> &'static str;

    /// Check if this backend supports the requested language
    ///
    /// # Arguments
    /// * `language` - Programming language to check
    ///
    /// # Returns
    /// true if language is supported, false otherwise
    fn supports_language(&self, language: &str) -> bool;

    /// Get supported languages for this backend
    ///
    /// # Returns
    /// List of supported programming languages
    fn supported_languages(&self) -> &[&'static str];
}

/// Execution request parameters
///
/// Contains all information needed to execute code in a secure environment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionRequest {
    /// Source code to execute
    pub code: String,

    /// Programming language (rust, python, javascript, etc.)
    pub language: String,

    /// Optional input data for the code
    pub input: Option<String>,

    /// Environment variables to set
    pub env_vars: HashMap<String, String>,

    /// Working directory (relative to sandbox)
    pub working_dir: Option<String>,

    /// Execution timeout
    pub timeout: Duration,

    /// Resource limits
    pub limits: ResourceLimits,

    /// Backend-specific configuration
    pub backend_config: HashMap<String, String>,
}

impl ExecutionRequest {
    /// Create a new execution request
    ///
    /// # Arguments
    /// * `code` - Source code to execute
    /// * `language` - Programming language
    pub fn new<C: Into<String>, L: Into<String>>(code: C, language: L) -> Self {
        Self {
            code: code.into(),
            language: language.into(),
            input: None,
            env_vars: HashMap::new(),
            working_dir: None,
            timeout: Duration::from_secs(30),
            limits: ResourceLimits::default(),
            backend_config: HashMap::new(),
        }
    }

    /// Set input data for the execution
    pub fn with_input<I: Into<String>>(mut self, input: I) -> Self {
        self.input = Some(input.into());
        self
    }

    /// Add environment variable
    pub fn with_env<K: Into<String>, V: Into<String>>(mut self, key: K, value: V) -> Self {
        self.env_vars.insert(key.into(), value.into());
        self
    }

    /// Set working directory
    pub fn with_working_dir<W: Into<String>>(mut self, dir: W) -> Self {
        self.working_dir = Some(dir.into());
        self
    }

    /// Set execution timeout
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Set resource limits
    pub fn with_limits(mut self, limits: ResourceLimits) -> Self {
        self.limits = limits;
        self
    }

    /// Add backend-specific configuration
    pub fn with_backend_config<K: Into<String>, V: Into<String>>(
        mut self,
        key: K,
        value: V,
    ) -> Self {
        self.backend_config.insert(key.into(), value.into());
        self
    }
}

/// Resource limits for execution
///
/// Defines constraints on resource usage during code execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimits {
    /// Maximum memory usage in bytes
    pub max_memory: Option<u64>,

    /// Maximum CPU time in seconds
    pub max_cpu_time: Option<u64>,

    /// Maximum number of processes/threads
    pub max_processes: Option<u32>,

    /// Maximum file size in bytes
    pub max_file_size: Option<u64>,

    /// Maximum network bandwidth in bytes/sec
    pub max_network_bandwidth: Option<u64>,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            max_memory: Some(512 * 1024 * 1024),           // 512MB
            max_cpu_time: Some(30),                        // 30 seconds
            max_processes: Some(10),                       // 10 processes
            max_file_size: Some(100 * 1024 * 1024),        // 100MB
            max_network_bandwidth: Some(10 * 1024 * 1024), // 10MB/s
        }
    }
}

/// Execution result from backend
///
/// Contains all output and metadata from code execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    /// Exit code from execution (0 = success)
    pub exit_code: i32,

    /// Standard output from execution
    pub stdout: String,

    /// Standard error from execution  
    pub stderr: String,

    /// Execution duration
    pub duration: Duration,

    /// Resource usage statistics
    pub resource_usage: ResourceUsage,

    /// Any backend-specific metadata
    pub metadata: HashMap<String, String>,
}

impl ExecutionResult {
    /// Create a successful execution result
    pub fn success<O: Into<String>>(stdout: O) -> Self {
        Self {
            exit_code: 0,
            stdout: stdout.into(),
            stderr: String::new(),
            duration: Duration::from_millis(0),
            resource_usage: ResourceUsage::default(),
            metadata: HashMap::new(),
        }
    }

    /// Create a failed execution result
    pub fn failure<E: Into<String>>(exit_code: i32, stderr: E) -> Self {
        Self {
            exit_code,
            stdout: String::new(),
            stderr: stderr.into(),
            duration: Duration::from_millis(0),
            resource_usage: ResourceUsage::default(),
            metadata: HashMap::new(),
        }
    }

    /// Check if execution was successful
    pub fn is_success(&self) -> bool {
        self.exit_code == 0
    }

    /// Get combined output (stdout + stderr)
    pub fn combined_output(&self) -> String {
        if self.stderr.is_empty() {
            self.stdout.clone()
        } else if self.stdout.is_empty() {
            self.stderr.clone()
        } else {
            format!("{}\n{}", self.stdout, self.stderr)
        }
    }
}

/// Resource usage statistics
///
/// Tracks actual resource consumption during execution.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResourceUsage {
    /// Peak memory usage in bytes
    pub peak_memory: u64,

    /// CPU time consumed in milliseconds
    pub cpu_time_ms: u64,

    /// Number of processes created
    pub process_count: u32,

    /// Total bytes written to disk
    pub disk_bytes_written: u64,

    /// Total bytes read from disk
    pub disk_bytes_read: u64,

    /// Network bytes sent
    pub network_bytes_sent: u64,

    /// Network bytes received
    pub network_bytes_received: u64,
}

/// Backend health status
///
/// Indicates the current health and availability of a backend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthStatus {
    /// Whether the backend is healthy and available
    pub is_healthy: bool,

    /// Human-readable status message
    pub message: String,

    /// Last health check timestamp
    pub last_check: std::time::SystemTime,

    /// Backend-specific health metrics
    pub metrics: HashMap<String, String>,
}

impl HealthStatus {
    /// Create a healthy status
    pub fn healthy<M: Into<String>>(message: M) -> Self {
        Self {
            is_healthy: true,
            message: message.into(),
            last_check: std::time::SystemTime::now(),
            metrics: HashMap::new(),
        }
    }

    /// Create an unhealthy status
    pub fn unhealthy<M: Into<String>>(message: M) -> Self {
        Self {
            is_healthy: false,
            message: message.into(),
            last_check: std::time::SystemTime::now(),
            metrics: HashMap::new(),
        }
    }

    /// Add a health metric
    pub fn with_metric<K: Into<String>, V: Into<String>>(mut self, key: K, value: V) -> Self {
        self.metrics.insert(key.into(), value.into());
        self
    }
}

/// Backend configuration
///
/// Common configuration options for all backends.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendConfig {
    /// Backend name/identifier
    pub name: String,

    /// Whether this backend is enabled
    pub enabled: bool,

    /// Default timeout for executions
    pub default_timeout: Duration,

    /// Default resource limits
    pub default_limits: ResourceLimits,

    /// Backend-specific configuration
    pub backend_specific: HashMap<String, String>,
}

impl BackendConfig {
    /// Create a new backend configuration
    pub fn new<N: Into<String>>(name: N) -> Self {
        Self {
            name: name.into(),
            enabled: true,
            default_timeout: Duration::from_secs(30),
            default_limits: ResourceLimits::default(),
            backend_specific: HashMap::new(),
        }
    }

    /// Set enabled status
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Set default timeout
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.default_timeout = timeout;
        self
    }

    /// Set default resource limits
    pub fn with_limits(mut self, limits: ResourceLimits) -> Self {
        self.default_limits = limits;
        self
    }

    /// Add backend-specific configuration
    pub fn with_config<K: Into<String>, V: Into<String>>(mut self, key: K, value: V) -> Self {
        self.backend_specific.insert(key.into(), value.into());
        self
    }
}

impl Default for BackendConfig {
    fn default() -> Self {
        Self::new("default")
    }
}

/// Backend-specific error types
///
/// Covers errors that can occur during backend operations.
#[derive(Debug, Clone, thiserror::Error)]
pub enum BackendError {
    /// Backend is not available on this platform
    #[error("Backend {backend} is not available on this platform: {reason}")]
    NotAvailable {
        backend: &'static str,
        reason: String,
    },

    /// Backend configuration is invalid
    #[error("Invalid configuration for {backend}: {details}")]
    InvalidConfig {
        backend: &'static str,
        details: String,
    },

    /// Language is not supported by this backend
    #[error("Language '{language}' is not supported by {backend}")]
    UnsupportedLanguage {
        backend: &'static str,
        language: String,
    },

    /// Resource limit exceeded during execution
    #[error("Resource limit exceeded: {resource} exceeded {limit}")]
    ResourceLimitExceeded { resource: String, limit: String },

    /// Execution timeout
    #[error("Execution timed out after {seconds} seconds")]
    ExecutionTimeout { seconds: u64 },

    /// Process execution failed
    #[error("Process execution failed: {details}")]
    ProcessFailed { details: String },

    /// Container/sandbox creation failed
    #[error("Container creation failed: {details}")]
    ContainerFailed { details: String },

    /// Network operation failed
    #[error("Network operation failed: {details}")]
    NetworkFailed { details: String },

    /// File system operation failed
    #[error("File system operation failed: {details}")]
    FileSystemFailed { details: String },

    /// Internal backend error
    #[error("Internal backend error: {message}")]
    Internal { message: String },
}

impl From<BackendError> for CyloError {
    fn from(err: BackendError) -> Self {
        match err {
            BackendError::NotAvailable { backend, reason } => {
                CyloError::backend_unavailable(backend, reason)
            }
            BackendError::InvalidConfig { backend, details } => CyloError::InvalidConfiguration {
                backend,
                message: Box::leak(details.into_boxed_str()),
            },
            BackendError::UnsupportedLanguage { backend, language } => {
                CyloError::execution_failed(backend, format!("Unsupported language: {language}"))
            }
            BackendError::ExecutionTimeout { seconds } => CyloError::ExecutionTimeout {
                backend: "unknown",
                timeout_secs: seconds,
            },
            BackendError::ResourceLimitExceeded { resource, limit } => {
                CyloError::ResourceLimitExceeded {
                    backend: "unknown",
                    resource,
                    limit,
                }
            }
            _ => CyloError::internal(err.to_string()),
        }
    }
}

/// Result type for backend operations
pub type BackendResult<T> = Result<T, BackendError>;

// Platform-conditional module imports
#[cfg(target_os = "macos")]
pub mod apple;
#[cfg(target_os = "macos")]
pub use apple::AppleBackend;

#[cfg(target_os = "linux")]
pub mod landlock;
#[cfg(target_os = "linux")]
pub use landlock::LandLockBackend;

#[cfg(target_os = "linux")]
pub mod firecracker;
#[cfg(target_os = "linux")]
pub use firecracker::FireCrackerBackend;

// SweetMCP plugin backend (available on all platforms)
pub mod sweetmcp_plugin;
pub use sweetmcp_plugin::SweetMcpPluginBackend;

/// Create a backend instance from configuration
///
/// Factory function that creates the appropriate backend based on
/// the execution environment specification.
///
/// # Arguments
/// * `env` - Execution environment specification
/// * `config` - Backend configuration
///
/// # Returns
/// Boxed backend instance or error if backend is not available
pub fn create_backend(
    env: &crate::execution_env::Cylo,
    config: BackendConfig,
) -> CyloResult<Box<dyn ExecutionBackend>> {
    match env {
        #[cfg(target_os = "macos")]
        crate::execution_env::Cylo::Apple(image) => {
            let backend = AppleBackend::new(image.clone(), config)?;
            Ok(Box::new(backend))
        }

        #[cfg(target_os = "linux")]
        crate::execution_env::Cylo::LandLock(path) => {
            let backend = LandLockBackend::new(path.clone(), config)?;
            Ok(Box::new(backend))
        }

        #[cfg(target_os = "linux")]
        crate::execution_env::Cylo::FireCracker(image) => {
            let backend = FireCrackerBackend::new(image.clone(), config)?;
            Ok(Box::new(backend))
        }

        // Platform-specific error handling
        #[cfg(not(target_os = "macos"))]
        crate::execution_env::Cylo::Apple(_) => Err(CyloError::platform_unsupported(
            "Apple",
            "Apple containerization is only available on macOS",
        )),

        #[cfg(not(target_os = "linux"))]
        crate::execution_env::Cylo::LandLock(_) => Err(CyloError::platform_unsupported(
            "LandLock",
            "LandLock is only available on Linux",
        )),

        #[cfg(not(target_os = "linux"))]
        crate::execution_env::Cylo::FireCracker(_) => Err(CyloError::platform_unsupported(
            "FireCracker",
            "FireCracker is only available on Linux",
        )),

        crate::execution_env::Cylo::SweetMcpPlugin(plugin_path) => {
            let backend = SweetMcpPluginBackend::new(plugin_path.clone().into(), config)?;
            Ok(Box::new(backend))
        }
    }
}

/// Get all available backends for the current platform
///
/// # Returns
/// List of backend types available on this platform
pub fn available_backends() -> Vec<&'static str> {
    let mut backends = vec!["SweetMcpPlugin"];

    #[cfg(target_os = "macos")]
    backends.push("Apple");

    #[cfg(target_os = "linux")]
    {
        backends.push("LandLock");
        backends.push("FireCracker");
    }

    backends
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execution_request_builder() {
        let request = ExecutionRequest::new("println!(\"Hello\");", "rust")
            .with_input("test input")
            .with_env("TEST_VAR", "test_value")
            .with_timeout(Duration::from_secs(60))
            .with_working_dir("/tmp");

        assert_eq!(request.code, "println!(\"Hello\");");
        assert_eq!(request.language, "rust");
        assert_eq!(request.input, Some("test input".to_string()));
        assert_eq!(
            request.env_vars.get("TEST_VAR"),
            Some(&"test_value".to_string())
        );
        assert_eq!(request.timeout, Duration::from_secs(60));
        assert_eq!(request.working_dir, Some("/tmp".to_string()));
    }

    #[test]
    fn execution_result_success() {
        let result = ExecutionResult::success("Hello, World!");
        assert!(result.is_success());
        assert_eq!(result.stdout, "Hello, World!");
        assert_eq!(result.stderr, "");
    }

    #[test]
    fn execution_result_failure() {
        let result = ExecutionResult::failure(1, "Error occurred");
        assert!(!result.is_success());
        assert_eq!(result.exit_code, 1);
        assert_eq!(result.stderr, "Error occurred");
    }

    #[test]
    fn health_status_creation() {
        let healthy = HealthStatus::healthy("All systems operational")
            .with_metric("cpu_usage", "25%")
            .with_metric("memory_usage", "512MB");

        assert!(healthy.is_healthy);
        assert_eq!(healthy.message, "All systems operational");
        assert_eq!(healthy.metrics.get("cpu_usage"), Some(&"25%".to_string()));
    }

    #[test]
    fn backend_config_builder() {
        let config = BackendConfig::new("test_backend")
            .with_enabled(true)
            .with_timeout(Duration::from_secs(120))
            .with_config("custom_option", "value");

        assert_eq!(config.name, "test_backend");
        assert!(config.enabled);
        assert_eq!(config.default_timeout, Duration::from_secs(120));
        assert_eq!(
            config.backend_specific.get("custom_option"),
            Some(&"value".to_string())
        );
    }

    #[test]
    fn available_backends_list() {
        let backends = available_backends();
        assert!(!backends.is_empty());

        #[cfg(target_os = "macos")]
        assert!(backends.contains(&"Apple"));

        #[cfg(target_os = "linux")]
        {
            assert!(backends.contains(&"LandLock"));
            assert!(backends.contains(&"FireCracker"));
        }
    }
}
