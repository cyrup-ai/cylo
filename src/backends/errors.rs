// ============================================================================
// File: packages/cylo/src/backends/errors.rs
// ----------------------------------------------------------------------------
// Backend-specific error types
// ============================================================================

use crate::execution_env::CyloError;

/// Backend-specific error types
///
/// Covers errors that can occur during backend operations.
#[derive(Debug, Clone, thiserror::Error)]
pub enum BackendError {
    /// Backend is not available on this platform
    #[error("Backend {backend} is not available on this platform: {reason}")]
    NotAvailable {
        backend: &'static str,
        reason: String,
    },

    /// Backend configuration is invalid
    #[error("Invalid configuration for {backend}: {details}")]
    InvalidConfig {
        backend: &'static str,
        details: String,
    },

    /// Language is not supported by this backend
    #[error("Language '{language}' is not supported by {backend}")]
    UnsupportedLanguage {
        backend: &'static str,
        language: String,
    },

    /// Resource limit exceeded during execution
    #[error("Resource limit exceeded: {resource} exceeded {limit}")]
    ResourceLimitExceeded { resource: String, limit: String },

    /// Execution timeout
    #[error("Execution timed out after {seconds} seconds")]
    ExecutionTimeout { seconds: u64 },

    /// Process execution failed
    #[error("Process execution failed: {details}")]
    ProcessFailed { details: String },

    /// Container/sandbox creation failed
    #[error("Container creation failed: {details}")]
    ContainerFailed { details: String },

    /// Network operation failed
    #[error("Network operation failed: {details}")]
    NetworkFailed { details: String },

    /// File system operation failed
    #[error("File system operation failed: {details}")]
    FileSystemFailed { details: String },

    /// Internal backend error
    #[error("Internal backend error: {message}")]
    Internal { message: String },
}

impl From<BackendError> for CyloError {
    fn from(err: BackendError) -> Self {
        match err {
            BackendError::NotAvailable { backend, reason } => {
                CyloError::backend_unavailable(backend, reason)
            }
            BackendError::InvalidConfig { backend, details } => CyloError::InvalidConfiguration {
                backend,
                message: Box::leak(details.into_boxed_str()),
            },
            BackendError::UnsupportedLanguage { backend, language } => {
                CyloError::execution_failed(backend, format!("Unsupported language: {language}"))
            }
            BackendError::ExecutionTimeout { seconds } => CyloError::ExecutionTimeout {
                backend: "unknown",
                timeout_secs: seconds,
            },
            BackendError::ResourceLimitExceeded { resource, limit } => {
                CyloError::ResourceLimitExceeded {
                    backend: "unknown",
                    resource,
                    limit,
                }
            }
            _ => CyloError::internal(err.to_string()),
        }
    }
}

/// Result type for backend operations
pub type BackendResult<T> = Result<T, BackendError>;
