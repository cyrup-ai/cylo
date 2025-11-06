//! ============================================================================
//! File: packages/cylo/src/executor/types.rs
//! ----------------------------------------------------------------------------
//! Type definitions for executor routing and configuration.
//! ============================================================================

use std::collections::HashMap;
use std::time::{Duration, SystemTime};

/// Routing strategy for execution requests
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoutingStrategy {
    /// Always use the fastest available backend
    Performance,
    /// Prioritize maximum security isolation
    Security,
    /// Balance performance and security
    Balanced,
    /// Use specific backend if available, fallback to balanced
    PreferBackend(String),
    /// Only use explicitly specified backends
    ExplicitOnly,
}

/// Backend selection preferences and weights
#[derive(Debug, Clone)]
pub struct BackendPreferences {
    /// Preferred backends in order of preference
    pub preferred_order: Vec<String>,
    /// Backend-specific weight multipliers (0.0-1.0)
    pub weight_multipliers: HashMap<String, f32>,
    /// Maximum concurrent executions per backend
    pub max_concurrent: HashMap<String, u32>,
    /// Backend exclusion list
    pub excluded_backends: Vec<String>,
}

impl Default for BackendPreferences {
    fn default() -> Self {
        let mut weight_multipliers = HashMap::new();
        weight_multipliers.insert("Apple".to_string(), 1.0);
        weight_multipliers.insert("LandLock".to_string(), 1.0);
        weight_multipliers.insert("FireCracker".to_string(), 1.0);

        let mut max_concurrent = HashMap::new();
        max_concurrent.insert("Apple".to_string(), 10);
        max_concurrent.insert("LandLock".to_string(), 20);
        max_concurrent.insert("FireCracker".to_string(), 50);

        Self {
            preferred_order: vec![
                "FireCracker".to_string(),
                "LandLock".to_string(),
                "Apple".to_string(),
            ],
            weight_multipliers,
            max_concurrent,
            excluded_backends: Vec::new(),
        }
    }
}

/// Performance optimization configuration
#[derive(Debug, Clone)]
pub struct OptimizationConfig {
    /// Enable instance reuse for repeated executions
    pub instance_reuse: bool,
    /// Instance pool size per backend
    pub instance_pool_size: u32,
    /// Maximum idle time before instance cleanup
    pub max_idle_time: Duration,
    /// Enable load balancing across instances
    pub load_balancing: bool,
    /// Resource usage monitoring interval
    pub monitoring_interval: Duration,
}

impl Default for OptimizationConfig {
    fn default() -> Self {
        Self {
            instance_reuse: true,
            instance_pool_size: 5,
            max_idle_time: Duration::from_secs(300),
            load_balancing: true,
            monitoring_interval: Duration::from_secs(60),
        }
    }
}

/// Cached platform information for fast routing decisions
#[derive(Debug, Clone)]
pub struct PlatformCache {
    /// Available backends with performance ratings
    pub available_backends: Vec<(String, u8)>,
    /// Platform capabilities hash for cache invalidation
    pub capabilities_hash: u64,
    /// Cache timestamp
    pub cached_at: SystemTime,
    /// Cache validity duration
    pub cache_duration: Duration,
}

/// Execution metrics and performance statistics
#[derive(Debug, Clone, Default)]
pub struct ExecutionMetrics {
    /// Total executions per backend
    pub executions_per_backend: HashMap<String, u64>,
    /// Average execution time per backend
    pub avg_execution_time: HashMap<String, Duration>,
    /// Success rate per backend
    pub success_rate: HashMap<String, f32>,
    /// Resource usage statistics
    pub resource_usage: HashMap<String, ResourceStats>,
    /// Last update timestamp
    pub last_updated: Option<SystemTime>,
}

/// Resource usage statistics for a backend
#[derive(Debug, Clone, Default)]
pub struct ResourceStats {
    /// Average memory usage in bytes
    pub avg_memory: u64,
    /// Average CPU time in milliseconds
    pub avg_cpu_time: u64,
    /// Average execution duration
    pub avg_duration: Duration,
    /// Peak resource usage
    pub peak_memory: u64,
    /// Total resource usage over time
    pub cumulative_cpu_time: u64,
}
