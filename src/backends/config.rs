// ============================================================================
// File: packages/cylo/src/backends/config.rs
// ----------------------------------------------------------------------------
// Configuration types for execution backends
// ============================================================================

use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
