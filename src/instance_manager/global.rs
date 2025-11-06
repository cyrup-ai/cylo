// ============================================================================
// File: packages/cylo/src/instance_manager/global.rs
// ----------------------------------------------------------------------------
// Global instance manager singleton
// ============================================================================

use std::time::Duration;

use crate::backends::BackendConfig;

use super::InstanceManager;

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
