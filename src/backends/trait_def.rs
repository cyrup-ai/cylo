// ============================================================================
// File: packages/cylo/src/backends/trait_def.rs
// ----------------------------------------------------------------------------
// ExecutionBackend trait definition
// ============================================================================

use crate::backends::config::BackendConfig;
use crate::backends::types::{ExecutionRequest, ExecutionResult, HealthStatus};
use crate::execution_env::CyloResult;

// Local AsyncTask type alias to avoid circular dependency with fluent_ai_domain
pub type AsyncTask<T> = tokio::task::JoinHandle<T>;

/// Core execution backend trait
///
/// All backends must implement this trait to provide secure code execution
/// capabilities. Uses AsyncTask pattern throughout for zero-allocation async.
pub trait ExecutionBackend: Send + Sync + std::fmt::Debug {
    /// Execute code in this backend environment
    ///
    /// # Arguments
    /// * `request` - Execution request with code, language, and configuration
    ///
    /// # Returns
    /// AsyncTask that resolves to execution result
    fn execute_code(&self, request: ExecutionRequest) -> AsyncTask<ExecutionResult>;

    /// Perform health check on this backend
    ///
    /// Verifies that the backend is available and functional.
    /// Should be fast and non-destructive.
    ///
    /// # Returns
    /// AsyncTask that resolves to health status
    fn health_check(&self) -> AsyncTask<HealthStatus>;

    /// Clean up resources for this backend
    ///
    /// Called when the backend instance is no longer needed.
    /// Should clean up any persistent resources, containers, or processes.
    ///
    /// # Returns
    /// AsyncTask that resolves when cleanup is complete
    fn cleanup(&self) -> AsyncTask<CyloResult<()>>;

    /// Get backend-specific configuration
    ///
    /// Returns the current configuration for this backend instance.
    fn get_config(&self) -> &BackendConfig;

    /// Get the backend type identifier
    fn backend_type(&self) -> &'static str;

    /// Check if this backend supports the requested language
    ///
    /// # Arguments
    /// * `language` - Programming language to check
    ///
    /// # Returns
    /// true if language is supported, false otherwise
    fn supports_language(&self, language: &str) -> bool;

    /// Get supported languages for this backend
    ///
    /// # Returns
    /// List of supported programming languages
    fn supported_languages(&self) -> &[&'static str];
}
