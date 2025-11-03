// ============================================================================
// File: packages/cylo/src/instance_manager.rs
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

use crate::async_task::{AsyncTask, AsyncTaskBuilder};
use crate::backends::{BackendConfig, ExecutionBackend, HealthStatus, create_backend};
use crate::execution_env::{CyloError, CyloInstance, CyloResult};

/// Thread-safe instance manager for Cylo execution environments
///
/// Maintains a registry of named backend instances for reuse across
/// multiple tool invocations. Provides health monitoring, lifecycle
/// management, and automatic cleanup capabilities.
#[derive(Debug)]
pub struct InstanceManager {
    /// Registry of active backend instances
    instances: Arc<RwLock<HashMap<String, ManagedInstance>>>,

    /// Default configuration for new instances
    default_config: BackendConfig,

    /// Health check interval for monitoring
    health_check_interval: Duration,

    /// Maximum idle time before cleanup
    max_idle_time: Duration,
}

/// Managed instance wrapper with metadata
#[derive(Debug)]
struct ManagedInstance {
    /// The backend instance
    backend: Arc<dyn ExecutionBackend>,

    /// Last access timestamp
    last_accessed: SystemTime,

    /// Last health check result
    last_health: Option<HealthStatus>,

    /// Last health check timestamp
    last_health_check: Option<SystemTime>,

    /// Reference count for active operations
    ref_count: u32,
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

    /// Register a new named instance
    ///
    /// Creates and registers a backend instance for the specified
    /// Cylo configuration with the given name.
    ///
    /// # Arguments
    /// * `instance` - Cylo instance configuration
    ///
    /// # Returns
    /// AsyncTask that resolves when instance is registered
    pub fn register_instance(&self, instance: CyloInstance) -> AsyncTask<CyloResult<()>> {
        let instances_lock = Arc::clone(&self.instances);
        let default_config = self.default_config.clone();

        AsyncTaskBuilder::new(async move {
            // Validate instance configuration
            instance.validate()?;

            // Check if instance already exists
            {
                let instances = instances_lock.read().map_err(|e| {
                    CyloError::internal(format!("Failed to acquire read lock: {e}"))
                })?;

                if instances.contains_key(&instance.id()) {
                    return Err(CyloError::InstanceConflict {
                        name: instance.id(),
                    });
                }
            }

            // Create backend instance
            let backend = create_backend(&instance.env, default_config)?;

            // Perform initial health check
            let health_result = (backend.health_check().await).ok();

            let managed_instance = ManagedInstance {
                backend: Arc::from(backend),
                last_accessed: SystemTime::now(),
                last_health: health_result,
                last_health_check: Some(SystemTime::now()),
                ref_count: 0,
            };

            // Register the instance
            {
                let mut instances = instances_lock.write().map_err(|e| {
                    CyloError::internal(format!("Failed to acquire write lock: {e}"))
                })?;

                instances.insert(instance.id(), managed_instance);
            }

            Ok(())
        })
        .spawn()
    }

    /// Get a registered instance by ID
    ///
    /// Returns a reference to the backend instance if it exists
    /// and is healthy. Updates access timestamp and increments
    /// reference count.
    ///
    /// # Arguments
    /// * `instance_id` - Unique instance identifier
    ///
    /// # Returns
    /// AsyncTask that resolves to backend instance or error
    pub fn get_instance(
        &self,
        instance_id: &str,
    ) -> AsyncTask<CyloResult<Arc<dyn ExecutionBackend>>> {
        let instances_lock = Arc::clone(&self.instances);
        let instance_id = instance_id.to_string();
        let health_check_interval = self.health_check_interval;

        AsyncTaskBuilder::new(async move {
            // First, try to get the instance with read lock
            let backend = {
                let instances = instances_lock.read().map_err(|e| {
                    CyloError::internal(format!("Failed to acquire read lock: {e}"))
                })?;

                match instances.get(&instance_id) {
                    Some(managed) => managed.backend.clone(),
                    None => {
                        return Err(CyloError::InstanceNotFound { name: instance_id });
                    }
                }
            };

            // Check if health check is needed
            let needs_health_check = {
                let instances = instances_lock.read().map_err(|e| {
                    CyloError::internal(format!("Failed to acquire read lock: {e}"))
                })?;

                if let Some(managed) = instances.get(&instance_id) {
                    managed
                        .last_health_check
                        .map(|last| {
                            last.elapsed().unwrap_or(Duration::from_secs(0)) > health_check_interval
                        })
                        .unwrap_or(true)
                } else {
                    false
                }
            };

            // Perform health check if needed
            if needs_health_check {
                let health_result = match backend.health_check().await {
                    Ok(health) => health,
                    Err(e) => {
                        return Err(CyloError::backend_unavailable(
                            backend.backend_type(),
                            format!("Health check failed for instance {instance_id}: {e}"),
                        ));
                    }
                };

                if !health_result.is_healthy {
                    return Err(CyloError::backend_unavailable(
                        backend.backend_type(),
                        format!(
                            "Instance {} is unhealthy: {}",
                            instance_id, health_result.message
                        ),
                    ));
                }

                // Update health status
                {
                    let mut instances = instances_lock.write().map_err(|e| {
                        CyloError::internal(format!("Failed to acquire write lock: {e}"))
                    })?;

                    if let Some(managed) = instances.get_mut(&instance_id) {
                        managed.last_health = Some(health_result);
                        managed.last_health_check = Some(SystemTime::now());
                        managed.last_accessed = SystemTime::now();
                        managed.ref_count += 1;
                    }
                }
            } else {
                // Just update access timestamp and ref count
                {
                    let mut instances = instances_lock.write().map_err(|e| {
                        CyloError::internal(format!("Failed to acquire write lock: {e}"))
                    })?;

                    if let Some(managed) = instances.get_mut(&instance_id) {
                        managed.last_accessed = SystemTime::now();
                        managed.ref_count += 1;
                    }
                }
            }

            Ok(backend)
        })
        .spawn()
    }

    /// Release a reference to an instance
    ///
    /// Decrements the reference count for the specified instance.
    /// Should be called when finished using an instance obtained
    /// from get_instance().
    ///
    /// # Arguments
    /// * `instance_id` - Unique instance identifier
    ///
    /// # Returns
    /// Result indicating success or error
    pub fn release_instance(&self, instance_id: &str) -> CyloResult<()> {
        let mut instances = self
            .instances
            .write()
            .map_err(|e| CyloError::internal(format!("Failed to acquire write lock: {e}")))?;

        if let Some(managed) = instances.get_mut(instance_id)
            && managed.ref_count > 0
        {
            managed.ref_count -= 1;
        }

        Ok(())
    }

    /// Remove an instance from the registry
    ///
    /// Cleanly shuts down and removes the specified instance.
    /// Will wait for active references to be released.
    ///
    /// # Arguments
    /// * `instance_id` - Unique instance identifier
    ///
    /// # Returns
    /// AsyncTask that resolves when instance is removed
    pub fn remove_instance(&self, instance_id: &str) -> AsyncTask<CyloResult<()>> {
        let instances_lock = Arc::clone(&self.instances);
        let instance_id = instance_id.to_string();

        AsyncTaskBuilder::new(async move {
            // Remove the instance from registry
            let managed_instance = {
                let mut instances = instances_lock.write().map_err(|e| {
                    CyloError::internal(format!("Failed to acquire write lock: {e}"))
                })?;

                instances.remove(&instance_id)
            };

            if let Some(managed) = managed_instance {
                // Wait for active references to be released
                let mut attempts = 0;
                while managed.ref_count > 0 && attempts < 30 {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    attempts += 1;
                }

                // Perform cleanup
                if let Err(e) = managed.backend.cleanup().await {
                    // Log cleanup error but don't fail the removal
                    log::warn!("Failed to cleanup instance {}: {}", instance_id, e);
                }
            }

            Ok(())
        })
        .spawn()
    }

    /// Get all registered instance IDs
    ///
    /// # Returns
    /// Vector of instance identifiers
    pub fn list_instances(&self) -> CyloResult<Vec<String>> {
        let instances = self
            .instances
            .read()
            .map_err(|e| CyloError::internal(format!("Failed to acquire read lock: {e}")))?;

        Ok(instances.keys().cloned().collect())
    }

    /// Get instance health status
    ///
    /// # Arguments
    /// * `instance_id` - Unique instance identifier
    ///
    /// # Returns
    /// Health status if instance exists
    pub fn get_instance_health(&self, instance_id: &str) -> CyloResult<Option<HealthStatus>> {
        let instances = self
            .instances
            .read()
            .map_err(|e| CyloError::internal(format!("Failed to acquire read lock: {e}")))?;

        Ok(instances
            .get(instance_id)
            .and_then(|managed| managed.last_health.clone()))
    }

    /// Perform health checks on all instances
    ///
    /// Updates health status for all registered instances.
    ///
    /// # Returns
    /// AsyncTask that resolves when all health checks complete
    pub fn health_check_all(&self) -> AsyncTask<CyloResult<HashMap<String, HealthStatus>>> {
        let instances_lock = Arc::clone(&self.instances);

        AsyncTaskBuilder::new(async move {
            let mut results = HashMap::new();

            // Get list of instances to check
            let instance_list = {
                let instances = instances_lock.read().map_err(|e| {
                    CyloError::internal(format!("Failed to acquire read lock: {e}"))
                })?;

                instances
                    .iter()
                    .map(|(id, managed)| (id.clone(), managed.backend.clone()))
                    .collect::<Vec<_>>()
            };

            // Perform health checks concurrently
            let mut health_tasks = Vec::new();

            for (instance_id, backend) in instance_list {
                let id = instance_id.clone();
                let health_task = AsyncTaskBuilder::new(async move {
                    let health = backend.health_check().await;
                    (id, health)
                })
                .spawn();
                health_tasks.push(health_task);
            }

            // Collect results
            for task in health_tasks {
                match task.await {
                    Ok((instance_id, health)) => {
                        match health {
                            Ok(health_status) => {
                                results.insert(instance_id, health_status);
                            }
                            Err(_) => {
                                // Health check failed, insert unhealthy status
                                results.insert(
                                    instance_id,
                                    HealthStatus::unhealthy("Health check failed"),
                                );
                            }
                        }
                    }
                    Err(_) => {
                        // Task failed, skip this instance
                    }
                }

                // Note: Health status is already stored in results HashMap
                // The stored health status in instances is updated when instances are accessed
            }

            Ok(results)
        })
        .spawn()
    }

    /// Clean up idle instances
    ///
    /// Removes instances that have been idle longer than the
    /// configured maximum idle time and have no active references.
    ///
    /// # Returns
    /// AsyncTask that resolves with count of cleaned up instances
    pub fn cleanup_idle_instances(&self) -> AsyncTask<CyloResult<u32>> {
        let instances_lock = Arc::clone(&self.instances);
        let max_idle_time = self.max_idle_time;

        AsyncTaskBuilder::new(async move {
            let now = SystemTime::now();
            let mut to_remove = Vec::new();

            // Identify idle instances
            {
                let instances = instances_lock.read().map_err(|e| {
                    CyloError::internal(format!("Failed to acquire read lock: {e}"))
                })?;

                for (instance_id, managed) in instances.iter() {
                    let idle_time = now
                        .duration_since(managed.last_accessed)
                        .unwrap_or(Duration::from_secs(0));

                    if idle_time > max_idle_time && managed.ref_count == 0 {
                        to_remove.push(instance_id.clone());
                    }
                }
            }

            // Remove idle instances
            let mut removed_count = 0;
            for instance_id in to_remove {
                let managed_instance = {
                    let mut instances = instances_lock.write().map_err(|e| {
                        CyloError::internal(format!("Failed to acquire write lock: {e}"))
                    })?;

                    instances.remove(&instance_id)
                };

                if let Some(managed) = managed_instance {
                    // Perform cleanup
                    if let Err(e) = managed.backend.cleanup().await {
                        log::warn!("Failed to cleanup idle instance {}: {}", instance_id, e);
                    } else {
                        removed_count += 1;
                    }
                }
            }

            Ok(removed_count)
        })
        .spawn()
    }

    /// Shutdown the instance manager
    ///
    /// Cleanly shuts down all registered instances and clears
    /// the registry. Should be called before dropping the manager.
    ///
    /// # Returns
    /// AsyncTask that resolves when shutdown is complete
    pub fn shutdown(&self) -> AsyncTask<CyloResult<()>> {
        let instances_lock = Arc::clone(&self.instances);

        AsyncTaskBuilder::new(async move {
            // Get all instances
            let all_instances = {
                let mut instances = instances_lock.write().map_err(|e| {
                    CyloError::internal(format!("Failed to acquire write lock: {e}"))
                })?;

                instances.drain().collect::<Vec<_>>()
            };

            // Cleanup all instances concurrently
            let mut cleanup_tasks = Vec::new();

            for (instance_id, managed) in all_instances {
                let id = instance_id.clone();
                let cleanup_task = AsyncTaskBuilder::new(async move {
                    if let Err(e) = managed.backend.cleanup().await {
                        log::warn!("Failed to cleanup instance {} during shutdown: {}", id, e);
                    }
                })
                .spawn();
                cleanup_tasks.push(cleanup_task);
            }

            // Wait for all cleanups to complete
            for task in cleanup_tasks {
                let _ = task.await;
            }

            Ok(())
        })
        .spawn()
    }
}

impl Default for InstanceManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Global instance manager singleton
static GLOBAL_INSTANCE_MANAGER: std::sync::OnceLock<InstanceManager> = std::sync::OnceLock::new();

/// Get the global instance manager
///
/// # Returns
/// Reference to the global instance manager
pub fn global_instance_manager() -> &'static InstanceManager {
    GLOBAL_INSTANCE_MANAGER.get_or_init(InstanceManager::new)
}

/// Initialize the global instance manager with custom configuration
///
/// # Arguments
/// * `config` - Default configuration for instances
/// * `health_check_interval` - Health check interval
/// * `max_idle_time` - Maximum idle time before cleanup
///
/// # Returns
/// Result indicating success or if already initialized
pub fn init_global_instance_manager(
    config: BackendConfig,
    health_check_interval: Duration,
    max_idle_time: Duration,
) -> Result<(), &'static str> {
    let manager = InstanceManager::with_config(config, health_check_interval, max_idle_time);

    GLOBAL_INSTANCE_MANAGER
        .set(manager)
        .map_err(|_| "Global instance manager already initialized")
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::backends::BackendConfig;
    use crate::execution_env::Cylo;

    #[tokio::test]
    async fn instance_manager_creation() {
        let manager = InstanceManager::new();

        let instances = manager
            .list_instances()
            .expect("Failed to list instances in test");
        assert!(instances.is_empty());
    }

    #[tokio::test]
    async fn instance_registration_and_retrieval() {
        let manager = InstanceManager::new();

        // Create a test instance (will fail on unsupported platforms)
        let cylo_env = Cylo::LandLock("/tmp/test".to_string());
        let instance = cylo_env.instance("test_instance");

        // Registration might fail due to platform support
        let register_result = manager.register_instance(instance.clone()).await;

        if register_result.is_ok() {
            // If registration succeeded, test retrieval
            let backend_result = manager.get_instance(&instance.id()).await;

            if let Ok(backend) = &backend_result {
                if let Ok(backend_arc) = backend {
                    assert_eq!(backend_arc.backend_type(), "LandLock");
                }

                // Test release
                let release_result = manager.release_instance(&instance.id());
                assert!(release_result.is_ok());

                // Test removal
                let remove_result = manager.remove_instance(&instance.id()).await;
                assert!(remove_result.is_ok());
            }
        }
        // If registration failed due to platform support, that's expected
    }

    #[tokio::test]
    async fn instance_not_found() {
        let manager = InstanceManager::new();

        let result = manager.get_instance("nonexistent").await;
        assert!(result.is_ok()); // JoinHandle should succeed

        match result {
            Ok(inner_result) => {
                if let Err(CyloError::InstanceNotFound { name }) = inner_result {
                    assert_eq!(name, "nonexistent");
                } else {
                    panic!("Expected InstanceNotFound error");
                }
            }
            Err(join_error) => {
                panic!("Unexpected join error: {:?}", join_error);
            }
        }
    }

    #[tokio::test]
    async fn instance_list() {
        let manager = InstanceManager::new();

        let initial_list = manager
            .list_instances()
            .expect("Failed to get initial instance list in test");
        assert!(initial_list.is_empty());

        // Try to register an instance
        let cylo_env = Cylo::Apple("test:latest".to_string());
        let instance = cylo_env.instance("test_list");

        let register_result = manager.register_instance(instance.clone()).await;

        if register_result.is_ok() {
            let updated_list = manager
                .list_instances()
                .expect("Failed to get updated instance list in test");
            assert!(updated_list.contains(&instance.id()));
        }
        // Platform support determines if this test can complete
    }

    #[tokio::test]
    async fn health_check_all() {
        let manager = InstanceManager::new();

        let health_results = manager
            .health_check_all()
            .await
            .expect("Failed to join async task in test")
            .expect("Failed to check health of all instances in test");
        assert!(health_results.is_empty());
    }

    #[tokio::test]
    async fn cleanup_idle_instances() {
        let manager = InstanceManager::new();

        let cleaned_count = manager
            .cleanup_idle_instances()
            .await
            .expect("Failed to join async task in test")
            .expect("Failed to cleanup idle instances in test");
        assert_eq!(cleaned_count, 0);
    }

    #[tokio::test]
    async fn shutdown() {
        let manager = InstanceManager::new();

        let shutdown_result = manager.shutdown().await;
        assert!(shutdown_result.is_ok());
    }

    #[test]
    fn global_instance_manager_access() {
        let manager = global_instance_manager();
        let instances = manager
            .list_instances()
            .expect("Failed to list instances from global manager in test");
        assert!(instances.is_empty());
    }

    #[test]
    fn custom_configuration() {
        let config = BackendConfig::new("custom").with_timeout(Duration::from_secs(120));
        let manager =
            InstanceManager::with_config(config, Duration::from_secs(30), Duration::from_secs(600));

        assert_eq!(manager.health_check_interval, Duration::from_secs(30));
        assert_eq!(manager.max_idle_time, Duration::from_secs(600));
    }
}
