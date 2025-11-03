//! ============================================================================
//! File: packages/cylo/src/executor.rs
//! ----------------------------------------------------------------------------
//! High-performance execution routing and orchestration for Cylo environments.
//!
//! Provides intelligent routing of execution requests to optimal backends based on:
//! - Platform capabilities and backend availability
//! - Resource requirements and performance characteristics  
//! - Security policies and isolation levels
//! - Load balancing and instance health monitoring
//! ============================================================================

use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime};
use crate::async_task::{AsyncTask, AsyncTaskBuilder};
use crate::execution_env::{Cylo, CyloInstance, CyloError, CyloResult};
use crate::backends::{
    ExecutionBackend, ExecutionRequest, ExecutionResult, BackendConfig,
    BackendError, BackendResult, HealthStatus, create_backend
};
use crate::platform::{detect_platform, get_recommended_backend, get_available_backends};
use crate::instance_manager::{global_instance_manager, InstanceManager};

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
    metrics: Arc<RwLock<ExecutionMetrics>>}

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
    ExplicitOnly}

/// Backend selection preferences and weights
#[derive(Debug, Clone)]
pub struct BackendPreferences {
    /// Preferred backends in order of preference
    preferred_order: Vec<String>,
    /// Backend-specific weight multipliers (0.0-1.0)
    weight_multipliers: HashMap<String, f32>,
    /// Maximum concurrent executions per backend
    max_concurrent: HashMap<String, u32>,
    /// Backend exclusion list
    excluded_backends: Vec<String>}

/// Performance optimization configuration
#[derive(Debug, Clone)]
pub struct OptimizationConfig {
    /// Enable instance reuse for repeated executions
    instance_reuse: bool,
    /// Instance pool size per backend
    instance_pool_size: u32,
    /// Maximum idle time before instance cleanup
    max_idle_time: Duration,
    /// Enable load balancing across instances
    load_balancing: bool,
    /// Resource usage monitoring interval
    monitoring_interval: Duration}

/// Cached platform information for fast routing decisions
#[derive(Debug, Clone)]
struct PlatformCache {
    /// Available backends with performance ratings
    available_backends: Vec<(String, u8)>,
    /// Platform capabilities hash for cache invalidation
    capabilities_hash: u64,
    /// Cache timestamp
    cached_at: SystemTime,
    /// Cache validity duration
    cache_duration: Duration}

/// Execution metrics and performance statistics
#[derive(Debug, Clone, Default)]
pub struct ExecutionMetrics {
    /// Total executions per backend
    executions_per_backend: HashMap<String, u64>,
    /// Average execution time per backend
    avg_execution_time: HashMap<String, Duration>,
    /// Success rate per backend
    success_rate: HashMap<String, f32>,
    /// Resource usage statistics
    resource_usage: HashMap<String, ResourceStats>,
    /// Last update timestamp
    last_updated: Option<SystemTime>}

/// Resource usage statistics for a backend
#[derive(Debug, Clone, Default)]
pub struct ResourceStats {
    /// Average memory usage in bytes
    avg_memory: u64,
    /// Average CPU time in milliseconds  
    avg_cpu_time: u64,
    /// Average execution duration
    avg_duration: Duration,
    /// Peak resource usage
    peak_memory: u64,
    /// Total resource usage over time
    cumulative_cpu_time: u64}

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
                let rating = platform_info.available_backends
                    .iter()
                    .find(|b| b.name == name)
                    .map(|b| b.performance_rating)
                    .unwrap_or(0);
                (name.to_string(), rating)
            })
            .collect();
        
        let platform_cache = Arc::new(RwLock::new(PlatformCache {
            available_backends,
            capabilities_hash: Self::compute_capabilities_hash(&platform_info),
            cached_at: SystemTime::now(),
            cache_duration: Duration::from_secs(300), // 5 minutes
        }));
        
        Self {
            routing_strategy: strategy,
            backend_preferences: BackendPreferences::default(),
            optimization_config: OptimizationConfig::default(),
            platform_cache,
            metrics: Arc::new(RwLock::new(ExecutionMetrics::default()))}
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
        instance_hint: Option<&CyloInstance>
    ) -> AsyncTask<CyloResult<ExecutionResult>> {
        let strategy = self.routing_strategy.clone();
        let preferences = self.backend_preferences.clone();
        let optimization = self.optimization_config.clone();
        let platform_cache = self.platform_cache.clone();
        let metrics = Arc::clone(&self.metrics);
        let instance_hint = instance_hint.cloned();
        
        AsyncTaskBuilder::new()
            .spawn(move || async move {
                // Route to optimal backend
                let (backend_name, cylo_instance) = match instance_hint {
                    Some(instance) => {
                        // Use explicitly provided instance
                        (Self::backend_name_from_cylo(&instance.env), instance)
                    },
                    None => {
                        // Intelligent backend selection
                        let backend_name = Self::select_optimal_backend(
                            &strategy, 
                            &preferences, 
                            &platform_cache,
                            &request
                        )?;
                        
                        // Create or reuse instance
                        let cylo_env = Self::create_cylo_env(&backend_name, &request)?;
                        let instance_name = Self::generate_instance_name(&backend_name);
                        let cylo_instance = cylo_env.instance(instance_name);
                        
                        (backend_name, cylo_instance)
                    }
                };
                
                // Execute with selected backend
                let result = Self::execute_with_backend(
                    backend_name.clone(),
                    cylo_instance,
                    request.clone(),
                    optimization
                ).await;
                
                // Update metrics
                Self::update_metrics(metrics, &backend_name, &request, &result).await;
                
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
    pub fn execute_code(
        &self,
        code: &str,
        language: &str
    ) -> AsyncTask<CyloResult<ExecutionResult>> {
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
        request: ExecutionRequest
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
        
        AsyncTaskBuilder::new()
            .spawn(move || async move {
                // Check if cache needs refresh
                let should_refresh = {
                    let cache = platform_cache.read()
                        .map_err(|e| CyloError::Other(format!("Cache lock poisoned: {}", e)))?;
                    
                    let current_time = SystemTime::now();
                    let cache_age = current_time.duration_since(cache.cached_at)
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
                        let rating = platform_info.available_backends
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
                let mut cache = platform_cache.write()
                    .map_err(|e| CyloError::Other(format!("Cache lock poisoned: {}", e)))?;
                
                cache.available_backends = available_backends;
                cache.capabilities_hash = capabilities_hash;
                cache.cached_at = SystemTime::now();
                
                Ok(())
            })
    }
    
    // ========================================================================
    // Internal Implementation Methods
    // ========================================================================
    
    /// Select optimal backend based on strategy and requirements
    fn select_optimal_backend(
        strategy: &RoutingStrategy,
        preferences: &BackendPreferences, 
        platform_cache: &Arc<RwLock<PlatformCache>>,
        request: &ExecutionRequest
    ) -> CyloResult<String> {
        let cache = platform_cache.read()
            .map_err(|e| CyloError::Other(format!("Cache lock poisoned: {}", e)))?;
        let available = &cache.available_backends;
        
        if available.is_empty() {
            return Err(CyloError::no_backend_available());
        }
        
        match strategy {
            RoutingStrategy::Performance => {
                // Select backend with highest performance rating
                let best = available.iter()
                    .filter(|(name, _)| !preferences.excluded_backends.contains(name))
                    .max_by_key(|(_, rating)| *rating)
                    .ok_or_else(|| CyloError::no_backend_available())?;
                Ok(best.0.clone())
            },
            
            RoutingStrategy::Security => {
                // Prefer FireCracker > LandLock > Apple for security
                let security_order = ["FireCracker", "LandLock", "Apple"];
                for backend in &security_order {
                    if available.iter().any(|(name, _)| name == backend) &&
                       !preferences.excluded_backends.contains(&backend.to_string()) {
                        return Ok(backend.to_string());
                    }
                }
                Err(CyloError::no_backend_available())
            },
            
            RoutingStrategy::Balanced => {
                // Weight performance with security considerations
                let mut weighted_scores: Vec<(String, f32)> = available.iter()
                    .filter(|(name, _)| !preferences.excluded_backends.contains(name))
                    .map(|(name, rating)| {
                        let base_score = *rating as f32;
                        let security_bonus = match name.as_str() {
                            "FireCracker" => 20.0,
                            "LandLock" => 15.0,
                            "Apple" => 10.0,
                            _ => 0.0};
                        let preference_multiplier = preferences.weight_multipliers
                            .get(name)
                            .copied()
                            .unwrap_or(1.0);
                        
                        let total_score = (base_score + security_bonus) * preference_multiplier;
                        (name.clone(), total_score)
                    })
                    .collect();
                
                weighted_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                
                weighted_scores.first()
                    .map(|(name, _)| name.clone())
                    .ok_or_else(|| CyloError::no_backend_available())
            },
            
            RoutingStrategy::PreferBackend(preferred) => {
                // Use preferred backend if available, otherwise balanced
                if available.iter().any(|(name, _)| name == preferred) &&
                   !preferences.excluded_backends.contains(preferred) {
                    Ok(preferred.clone())
                } else {
                    Self::select_optimal_backend(
                        &RoutingStrategy::Balanced,
                        preferences,
                        platform_cache,
                        request
                    )
                }
            },
            
            RoutingStrategy::ExplicitOnly => {
                Err(CyloError::invalid_configuration(
                    "ExplicitOnly strategy requires instance_hint"
                ))
            }
        }
    }
    
    /// Create Cylo environment for backend
    fn create_cylo_env(backend_name: &str, request: &ExecutionRequest) -> CyloResult<Cylo> {
        match backend_name {
            "Apple" => {
                let image = Self::select_image_for_language(&request.language);
                Ok(Cylo::Apple(image))
            },
            "LandLock" => {
                Ok(Cylo::LandLock("/tmp/cylo_landlock".to_string()))
            },
            "FireCracker" => {
                let image = Self::select_image_for_language(&request.language);
                Ok(Cylo::FireCracker(image))
            },
            _ => Err(CyloError::unsupported_backend(backend_name))
        }
    }
    
    /// Select appropriate container image for programming language
    fn select_image_for_language(language: &str) -> String {
        match language.to_lowercase().as_str() {
            "python" | "python3" => "python:3.11-alpine".to_string(),
            "javascript" | "js" | "node" => "node:18-alpine".to_string(),
            "rust" => "rust:1.75-alpine".to_string(),
            "go" => "golang:1.21-alpine".to_string(),
            _ => "alpine:3.18".to_string(), // Default for bash/sh
        }
    }
    
    /// Generate unique instance name
    fn generate_instance_name(backend_name: &str) -> String {
        format!("{}_{}", 
            backend_name.to_lowercase(),
            uuid::Uuid::new_v4().simple()
        )
    }
    
    /// Get backend name from Cylo environment
    fn backend_name_from_cylo(cylo: &Cylo) -> String {
        match cylo {
            Cylo::Apple(_) => "Apple".to_string(),
            Cylo::LandLock(_) => "LandLock".to_string(),
            Cylo::FireCracker(_) => "FireCracker".to_string()}
    }
    
    /// Execute with specific backend and instance management
    async fn execute_with_backend(
        backend_name: String,
        instance: CyloInstance,
        request: ExecutionRequest,
        optimization: OptimizationConfig
    ) -> CyloResult<ExecutionResult> {
        let manager = global_instance_manager();
        
        // Register instance if using instance reuse
        if optimization.instance_reuse {
            if let Err(e) = manager.register_instance(instance.clone()).await {
                // Instance might already exist, try to get it
                if !matches!(e, CyloError::InstanceConflict { .. }) {
                    return Err(e);
                }
            }
        }
        
        // Get backend instance
        let backend = if optimization.instance_reuse {
            manager.get_instance(&instance.id()).await?
        } else {
            // Create temporary backend
            let config = BackendConfig::new(&format!("temp_{}", backend_name));
            Arc::from(create_backend(&instance.env, config)?)
        };
        
        // Execute code
        let result = backend.execute_code(request).await;
        
        // Clean up if not using instance reuse
        if !optimization.instance_reuse {
            let _ = manager.remove_instance(&instance.id()).await;
        } else {
            // Release reference
            let _ = manager.release_instance(&instance.id());
        }
        
        Ok(result)
    }
    
    /// Update execution metrics
    async fn update_metrics(
        metrics: Arc<RwLock<ExecutionMetrics>>,
        backend_name: &str,
        request: &ExecutionRequest,
        result: &CyloResult<ExecutionResult>
    ) {
        if let Ok(mut metrics) = metrics.write() {
            let executions = metrics.executions_per_backend
                .entry(backend_name.to_string())
                .or_insert(0);
            *executions += 1;
            
            if let Ok(exec_result) = result {
                // Update success rate
                let current_success = metrics.success_rate
                    .get(backend_name)
                    .copied()
                    .unwrap_or(0.0);
                let new_success = if exec_result.is_success() {
                    (current_success * (*executions as f32 - 1.0) + 1.0) / (*executions as f32)
                } else {
                    (current_success * (*executions as f32 - 1.0)) / (*executions as f32)
                };
                metrics.success_rate.insert(backend_name.to_string(), new_success);
                
                // Update timing metrics
                let current_avg = metrics.avg_execution_time
                    .get(backend_name)
                    .copied()
                    .unwrap_or(Duration::from_secs(0));
                let new_avg = Duration::from_nanos(
                    (current_avg.as_nanos() as u64 * (*executions - 1) + 
                     exec_result.duration.as_nanos() as u64) / *executions
                );
                metrics.avg_execution_time.insert(backend_name.to_string(), new_avg);
                
                // Update resource usage
                let resource_stats = metrics.resource_usage
                    .entry(backend_name.to_string())
                    .or_insert_with(ResourceStats::default);
                
                let prev_count = *executions - 1;
                resource_stats.avg_memory = (resource_stats.avg_memory * prev_count + 
                    exec_result.resource_usage.peak_memory) / *executions;
                resource_stats.avg_cpu_time = (resource_stats.avg_cpu_time * prev_count + 
                    exec_result.resource_usage.cpu_time_ms) / *executions;
                resource_stats.avg_duration = Duration::from_nanos(
                    (resource_stats.avg_duration.as_nanos() as u64 * prev_count + 
                     exec_result.duration.as_nanos() as u64) / *executions
                );
                
                if exec_result.resource_usage.peak_memory > resource_stats.peak_memory {
                    resource_stats.peak_memory = exec_result.resource_usage.peak_memory;
                }
                resource_stats.cumulative_cpu_time += exec_result.resource_usage.cpu_time_ms;
            }
            
            metrics.last_updated = Some(SystemTime::now());
        }
    }
    
    /// Compute platform capabilities hash for cache invalidation
    fn compute_capabilities_hash(platform_info: &crate::platform::PlatformInfo) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        
        let mut hasher = DefaultHasher::new();
        platform_info.available_backends.len().hash(&mut hasher);
        for backend in &platform_info.available_backends {
            backend.name.hash(&mut hasher);
            backend.available.hash(&mut hasher);
            backend.performance_rating.hash(&mut hasher);
        }
        hasher.finish()
    }
}

impl Default for CyloExecutor {
    fn default() -> Self {
        Self::new()
    }
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
                "Apple".to_string()
            ],
            weight_multipliers,
            max_concurrent,
            excluded_backends: Vec::new()}
    }
}

impl Default for OptimizationConfig {
    fn default() -> Self {
        Self {
            instance_reuse: true,
            instance_pool_size: 5,
            max_idle_time: Duration::from_secs(300),
            load_balancing: true,
            monitoring_interval: Duration::from_secs(60)}
    }
}

// ============================================================================
// Convenience Functions and High-Level API
// ============================================================================

/// Create a new executor with optimal configuration for the current platform
#[inline]
pub fn create_executor() -> CyloExecutor {
    CyloExecutor::new()
}

/// Create a performance-optimized executor
#[inline]
pub fn create_performance_executor() -> CyloExecutor {
    CyloExecutor::with_strategy(RoutingStrategy::Performance)
}

/// Create a security-focused executor
#[inline]
pub fn create_security_executor() -> CyloExecutor {
    CyloExecutor::with_strategy(RoutingStrategy::Security)
}

/// Execute code with automatic backend selection and optimal routing
#[inline]
pub fn execute_with_routing(code: &str, language: &str) -> AsyncTask<CyloResult<ExecutionResult>> {
    let executor = create_executor();
    executor.execute_code(code, language)
}

/// Global executor instance for high-performance shared usage
static GLOBAL_EXECUTOR: std::sync::OnceLock<CyloExecutor> = std::sync::OnceLock::new();

/// Get the global executor instance
#[inline]
pub fn global_executor() -> &'static CyloExecutor {
    GLOBAL_EXECUTOR.get_or_init(CyloExecutor::new)
}

/// Initialize global executor with specific configuration
pub fn init_global_executor(executor: CyloExecutor) -> Result<(), CyloError> {
    GLOBAL_EXECUTOR.set(executor)
        .map_err(|_| CyloError::internal("Global executor already initialized".to_string()))
}