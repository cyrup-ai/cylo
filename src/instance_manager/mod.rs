// ============================================================================
// File: packages/cylo/src/instance_manager/mod.rs
// ----------------------------------------------------------------------------
// Thread-safe instance manager for named Cylo execution environments.
//
// Provides centralized management of execution backend instances with:
// - Named instance registration and lookup
// - Thread-safe access with lock-free operations where possible
// - Instance lifecycle management and health monitoring
// - Automatic cleanup and resource management
// ============================================================================

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime};

use crate::backends::{BackendConfig, ExecutionBackend, HealthStatus};

// Submodules
mod lifecycle;
mod queries;
mod maintenance;
mod global;

#[cfg(test)]
mod tests;

// Re-exports
pub use global::{global_instance_manager, init_global_instance_manager};

/// Thread-safe instance manager for Cylo execution environments
///
/// Maintains a registry of named backend instances for reuse across
/// multiple tool invocations. Provides health monitoring, lifecycle
/// management, and automatic cleanup capabilities.
#[derive(Debug)]
pub struct InstanceManager {
    /// Registry of active backend instances
    pub(crate) instances: Arc<RwLock<HashMap<String, ManagedInstance>>>,

    /// Default configuration for new instances
    pub(crate) default_config: BackendConfig,

    /// Health check interval for monitoring
    pub(crate) health_check_interval: Duration,

    /// Maximum idle time before cleanup
    pub(crate) max_idle_time: Duration,
}

/// Managed instance wrapper with metadata
#[derive(Debug)]
pub(crate) struct ManagedInstance {
    /// The backend instance
    pub(crate) backend: Arc<dyn ExecutionBackend>,

    /// Last access timestamp
    pub(crate) last_accessed: SystemTime,

    /// Last health check result
    pub(crate) last_health: Option<HealthStatus>,

    /// Last health check timestamp
    pub(crate) last_health_check: Option<SystemTime>,

    /// Reference count for active operations
    pub(crate) ref_count: u32,
}

impl InstanceManager {
    /// Create a new instance manager
    ///
    /// # Returns
    /// New instance manager with default configuration
    pub fn new() -> Self {
        Self {
            instances: Arc::new(RwLock::new(HashMap::new())),
            default_config: BackendConfig::new("default"),
            health_check_interval: Duration::from_secs(60),
            max_idle_time: Duration::from_secs(300), // 5 minutes
        }
    }

    /// Create instance manager with custom configuration
    ///
    /// # Arguments
    /// * `config` - Default configuration for instances
    /// * `health_check_interval` - How often to check instance health
    /// * `max_idle_time` - Maximum idle time before cleanup
    ///
    /// # Returns
    /// Configured instance manager
    pub fn with_config(
        config: BackendConfig,
        health_check_interval: Duration,
        max_idle_time: Duration,
    ) -> Self {
        Self {
            instances: Arc::new(RwLock::new(HashMap::new())),
            default_config: config,
            health_check_interval,
            max_idle_time,
        }
    }
}

impl Default for InstanceManager {
    fn default() -> Self {
        Self::new()
    }
}
