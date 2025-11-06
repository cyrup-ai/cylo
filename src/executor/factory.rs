//! ============================================================================
//! File: packages/cylo/src/executor/factory.rs
//! ----------------------------------------------------------------------------
//! Convenience functions and global executor singleton.
//! ============================================================================

use crate::async_task::AsyncTask;
use crate::execution_env::{CyloError, CyloResult};
use crate::backends::ExecutionResult;
use super::{CyloExecutor, RoutingStrategy};

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
pub fn execute_with_routing(
    code: &str,
    language: &str,
) -> AsyncTask<CyloResult<ExecutionResult>> {
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
    GLOBAL_EXECUTOR
        .set(executor)
        .map_err(|_| CyloError::internal("Global executor already initialized".to_string()))
}
