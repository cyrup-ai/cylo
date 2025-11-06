// ============================================================================
// File: packages/cylo/src/backends/types.rs
// ----------------------------------------------------------------------------
// Execution request and result types
// ============================================================================

use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::backends::config::ResourceLimits;

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
}
