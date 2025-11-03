use std::{io, sync::Arc};

use anyhow;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ExecError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("Command failed: {0}")]
    CommandFailed(String),

    #[error("Unsupported language: {0}")]
    UnsupportedLanguage(String),

    #[error("Invalid code: {0}")]
    InvalidCode(String),

    #[error("Runtime error: {0}")]
    RuntimeError(String),

    #[error("System error: {0}")]
    SystemError(#[from] anyhow::Error),

    #[error("Storage error: {0}")]
    Storage(#[from] StorageError),
}

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("Command failed: {0}")]
    CommandFailed(String),

    #[error("Unsupported OS: {0}")]
    UnsupportedOs(String),

    #[error("Mount point exists and is mounted: {0}")]
    AlreadyMounted(std::path::PathBuf),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Insufficient privileges: {0}")]
    InsufficientPrivileges(String),

    #[error("Invalid path: {0}")]
    PathInvalid(String),

    #[error("Partial operation failure: {0}")]
    PartialFailure(String),

    #[error("{0}")]
    Other(#[from] anyhow::Error),
}

/// Comprehensive error types for sandbox operations with zero-allocation string sharing
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SandboxError {
    #[error("Configuration failed: {detail}")]
    ConfigurationFailed { detail: Arc<str> },

    #[error("Environment setup failed: {detail}")]
    EnvironmentSetup { detail: Arc<str> },

    #[error("Process launch failed: {detail}")]
    ProcessLaunch { detail: Arc<str> },

    #[error("Resource exhausted: {resource}")]
    ResourceExhausted { resource: Arc<str> },

    #[error("Permission denied: {operation}")]
    PermissionDenied { operation: Arc<str> },

    #[error("IO error ({kind:?}): {detail}")]
    IoError {
        kind: io::ErrorKind,
        detail: Arc<str>,
    },

    #[error("Command not found: {command}")]
    CommandNotFound { command: Arc<str> },

    #[error("Environment invalid: {detail}")]
    EnvironmentInvalid { detail: Arc<str> },

    #[error("Path invalid: {detail}")]
    PathInvalid { detail: Arc<str> },

    #[error("Runtime not found: {runtime}")]
    RuntimeNotFound { runtime: Arc<str> },
}

/// Zero-allocation error conversion from std::io::Error
impl From<io::Error> for SandboxError {
    #[inline]
    fn from(error: io::Error) -> Self {
        SandboxError::IoError {
            kind: error.kind(),
            detail: Arc::from(error.to_string()),
        }
    }
}

/// Conversion from SandboxError to ExecError for compatibility
impl From<SandboxError> for ExecError {
    #[inline]
    fn from(error: SandboxError) -> Self {
        match error {
            SandboxError::ConfigurationFailed { detail } => {
                ExecError::RuntimeError(format!("Configuration failed: {detail}"))
            }
            SandboxError::EnvironmentSetup { detail } => {
                ExecError::RuntimeError(format!("Environment setup failed: {detail}"))
            }
            SandboxError::ProcessLaunch { detail } => {
                ExecError::CommandFailed(format!("Process launch failed: {detail}"))
            }
            SandboxError::ResourceExhausted { resource } => {
                ExecError::RuntimeError(format!("Resource exhausted: {resource}"))
            }
            SandboxError::PermissionDenied { operation } => {
                ExecError::RuntimeError(format!("Permission denied: {operation}"))
            }
            SandboxError::IoError { kind: _, detail } => {
                ExecError::RuntimeError(format!("IO error: {detail}"))
            }
            SandboxError::CommandNotFound { command } => {
                ExecError::RuntimeError(format!("Command not found: {command}"))
            }
            SandboxError::EnvironmentInvalid { detail } => {
                ExecError::RuntimeError(format!("Environment invalid: {detail}"))
            }
            SandboxError::PathInvalid { detail } => {
                ExecError::RuntimeError(format!("Path invalid: {detail}"))
            }
            SandboxError::RuntimeNotFound { runtime } => {
                ExecError::RuntimeError(format!("Runtime not found: {runtime}"))
            }
        }
    }
}

// Generic result type that can be used with either error
pub type Result<T, E = ExecError> = std::result::Result<T, E>;

// Specialized result type for sandbox operations
pub type SandboxResult<T> = std::result::Result<T, SandboxError>;
