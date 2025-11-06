// ============================================================================
// File: packages/cylo/src/instance_manager/queries.rs
// ----------------------------------------------------------------------------
// Instance query operations:
// - List all instances
// - Get instance health status
// ============================================================================

use crate::backends::HealthStatus;
use crate::execution_env::{CyloError, CyloResult};

use super::InstanceManager;

impl InstanceManager {
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
}
