// ============================================================================
// File: packages/cylo/src/instance_manager/lifecycle.rs
// ----------------------------------------------------------------------------
// Instance lifecycle management operations:
// - Instance registration
// - Instance retrieval with health checking
// - Reference counting and release
// - Instance removal and cleanup
// ============================================================================

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use crate::async_task::{AsyncTask, AsyncTaskBuilder};
use crate::backends::{ExecutionBackend, create_backend};
use crate::execution_env::{CyloError, CyloInstance, CyloResult};

use super::{InstanceManager, ManagedInstance};

impl InstanceManager {
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
}
