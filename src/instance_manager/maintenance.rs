// ============================================================================
// File: packages/cylo/src/instance_manager/maintenance.rs
// ----------------------------------------------------------------------------
// Instance maintenance operations:
// - Health check all instances
// - Cleanup idle instances
// - Shutdown all instances
// ============================================================================

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use crate::async_task::{AsyncTask, AsyncTaskBuilder};
use crate::backends::HealthStatus;
use crate::execution_env::{CyloError, CyloResult};

use super::InstanceManager;

impl InstanceManager {
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
