//! ============================================================================
//! File: packages/cylo/src/executor/mod.rs
//! ----------------------------------------------------------------------------
//! High-performance execution routing and orchestration for Cylo environments.
//!
//! Provides intelligent routing of execution requests to optimal backends based on:
//! - Platform capabilities and backend availability
//! - Resource requirements and performance characteristics
//! - Security policies and isolation levels
//! - Load balancing and instance health monitoring
//! ============================================================================

mod types;
mod routing;
mod execution;
mod metrics;
mod factory;

// Re-export public types and functions
pub use types::{
    RoutingStrategy, BackendPreferences, OptimizationConfig, ExecutionMetrics, ResourceStats,
};
pub use factory::{
    create_executor, create_performance_executor, create_security_executor,
    execute_with_routing, global_executor, init_global_executor,
};

use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime};
use crate::async_task::{AsyncTask, AsyncTaskBuilder};
use crate::execution_env::{Cylo, CyloInstance, CyloError, CyloResult};
use crate::backends::{ExecutionRequest, ExecutionResult};
use crate::platform::{detect_platform, get_available_backends};
use types::PlatformCache;

/// High-performance execution orchestrator for Cylo environments
///
/// Provides intelligent routing, load balancing, and resource optimization
/// for code execution across multiple isolation backends.
#[derive(Debug)]
pub struct CyloExecutor {
    /// Execution routing strategy
    routing_strategy: RoutingStrategy,

    /// Backend selection preferences
    backend_preferences: BackendPreferences,

    /// Performance optimization settings
    optimization_config: OptimizationConfig,

    /// Cached platform capabilities (with interior mutability)
    platform_cache: Arc<RwLock<PlatformCache>>,

    /// Execution statistics and metrics
    metrics: Arc<RwLock<ExecutionMetrics>>,
}

impl CyloExecutor {
    /// Create a new high-performance executor with optimal defaults
    ///
    /// # Returns
    /// Configured executor ready for production use
    pub fn new() -> Self {
        Self::with_strategy(RoutingStrategy::Balanced)
    }

    /// Create executor with specific routing strategy
    ///
    /// # Arguments
    /// * `strategy` - Routing strategy for backend selection
    ///
    /// # Returns
    /// Configured executor with specified strategy
    pub fn with_strategy(strategy: RoutingStrategy) -> Self {
        let platform_info = detect_platform();
        let available_backends = get_available_backends()
            .into_iter()
            .map(|name| {
                let rating = platform_info
                    .available_backends
                    .iter()
                    .find(|b| b.name == name)
                    .map(|b| b.performance_rating)
                    .unwrap_or(0);
                (name.to_string(), rating)
            })
            .collect();

        let platform_cache = Arc::new(RwLock::new(PlatformCache {
            available_backends,
            capabilities_hash: routing::compute_capabilities_hash(&platform_info),
            cached_at: SystemTime::now(),
            cache_duration: Duration::from_secs(300), // 5 minutes
        }));

        Self {
            routing_strategy: strategy,
            backend_preferences: BackendPreferences::default(),
            optimization_config: OptimizationConfig::default(),
            platform_cache,
            metrics: Arc::new(RwLock::new(ExecutionMetrics::default())),
        }
    }

    /// Execute code with intelligent backend routing
    ///
    /// # Arguments
    /// * `request` - Execution request with code and requirements
    /// * `instance_hint` - Optional preferred instance for execution
    ///
    /// # Returns
    /// AsyncTask that resolves to execution result
    pub fn execute(
        &self,
        request: ExecutionRequest,
        instance_hint: Option<&CyloInstance>,
    ) -> AsyncTask<CyloResult<ExecutionResult>> {
        let strategy = self.routing_strategy.clone();
        let preferences = self.backend_preferences.clone();
        let optimization = self.optimization_config.clone();
        let platform_cache = self.platform_cache.clone();
        let metrics = Arc::clone(&self.metrics);
        let instance_hint = instance_hint.cloned();

        AsyncTaskBuilder::new().spawn(move || async move {
            // Route to optimal backend
            let (backend_name, cylo_instance) = match instance_hint {
                Some(instance) => {
                    // Use explicitly provided instance
                    (routing::backend_name_from_cylo(&instance.env), instance)
                }
                None => {
                    // Intelligent backend selection
                    let backend_name = routing::select_optimal_backend(
                        &strategy,
                        &preferences,
                        &platform_cache,
                        &request,
                    )?;

                    // Create or reuse instance
                    let cylo_env = routing::create_cylo_env(&backend_name, &request)?;
                    let instance_name = routing::generate_instance_name(&backend_name);
                    let cylo_instance = cylo_env.instance(instance_name);

                    (backend_name, cylo_instance)
                }
            };

            // Execute with selected backend
            let result = execution::execute_with_backend(
                backend_name.clone(),
                cylo_instance,
                request.clone(),
                optimization,
            )
            .await;

            // Update metrics
            metrics::update_metrics(metrics, &backend_name, &request, &result).await;

            result
        })
    }

    /// Execute code with automatic instance management
    ///
    /// # Arguments
    /// * `code` - Source code to execute
    /// * `language` - Programming language
    ///
    /// # Returns
    /// AsyncTask that resolves to execution result
    #[inline]
    pub fn execute_code(&self, code: &str, language: &str) -> AsyncTask<CyloResult<ExecutionResult>> {
        let request = ExecutionRequest::new(code, language);
        self.execute(request, None)
    }

    /// Execute with specific Cylo instance
    ///
    /// # Arguments
    /// * `instance` - Cylo instance to use for execution
    /// * `request` - Execution request
    ///
    /// # Returns
    /// AsyncTask that resolves to execution result
    pub fn execute_with_instance(
        &self,
        instance: &CyloInstance,
        request: ExecutionRequest,
    ) -> AsyncTask<CyloResult<ExecutionResult>> {
        self.execute(request, Some(instance))
    }

    /// Get execution metrics and performance statistics
    ///
    /// # Returns
    /// Current execution metrics
    pub fn get_metrics(&self) -> CyloResult<ExecutionMetrics> {
        let metrics = self.metrics.read().map_err(|e| {
            CyloError::internal(format!("Failed to read metrics: {}", e))
        })?;
        Ok(metrics.clone())
    }

    /// Update executor configuration
    ///
    /// # Arguments
    /// * `config` - New optimization configuration
    pub fn update_config(&mut self, config: OptimizationConfig) {
        self.optimization_config = config;
    }

    /// Update backend preferences
    ///
    /// # Arguments
    /// * `preferences` - New backend preferences
    pub fn update_preferences(&mut self, preferences: BackendPreferences) {
        self.backend_preferences = preferences;
    }

    /// Refresh platform cache if needed
    ///
    /// # Returns
    /// AsyncTask that resolves when cache is refreshed
    pub fn refresh_platform_cache(&self) -> AsyncTask<CyloResult<()>> {
        let platform_cache = Arc::clone(&self.platform_cache);

        AsyncTaskBuilder::new().spawn(move || async move {
            // Check if cache needs refresh
            let should_refresh = {
                let cache = platform_cache
                    .read()
                    .map_err(|e| CyloError::Other(format!("Cache lock poisoned: {}", e)))?;

                let current_time = SystemTime::now();
                let cache_age = current_time
                    .duration_since(cache.cached_at)
                    .unwrap_or(Duration::from_secs(0));

                cache_age >= cache.cache_duration
            };

            if !should_refresh {
                return Ok(());
            }

            // Detect current platform capabilities
            let platform_info = detect_platform();
            let available_backends: Vec<(String, u8)> = get_available_backends()
                .into_iter()
                .map(|name| {
                    let rating = platform_info
                        .available_backends
                        .iter()
                        .find(|b| b.name == name)
                        .map(|b| b.performance_rating)
                        .unwrap_or(0);
                    (name, rating)
                })
                .collect();

            let capabilities_hash = {
                use std::collections::hash_map::DefaultHasher;
                use std::hash::{Hash, Hasher};
                let mut hasher = DefaultHasher::new();
                platform_info.os.hash(&mut hasher);
                platform_info.arch.hash(&mut hasher);
                hasher.finish()
            };

            // Update cache with write lock
            let mut cache = platform_cache
                .write()
                .map_err(|e| CyloError::Other(format!("Cache lock poisoned: {}", e)))?;

            cache.available_backends = available_backends;
            cache.capabilities_hash = capabilities_hash;
            cache.cached_at = SystemTime::now();

            Ok(())
        })
    }
}

impl Default for CyloExecutor {
    fn default() -> Self {
        Self::new()
    }
}
