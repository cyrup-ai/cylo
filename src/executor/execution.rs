//! ============================================================================
//! File: packages/cylo/src/executor/execution.rs
//! ----------------------------------------------------------------------------
//! Core execution orchestration and instance management.
//! ============================================================================

use std::sync::Arc;
use crate::execution_env::{CyloInstance, CyloError, CyloResult};
use crate::backends::{
    ExecutionRequest, ExecutionResult, BackendConfig, create_backend,
};
use crate::instance_manager::global_instance_manager;
use super::types::OptimizationConfig;

/// Execute with specific backend and instance management
pub async fn execute_with_backend(
    backend_name: String,
    instance: CyloInstance,
    request: ExecutionRequest,
    optimization: OptimizationConfig,
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
