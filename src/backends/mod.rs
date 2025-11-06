// ============================================================================
// File: packages/cylo/src/backends/mod.rs
// ----------------------------------------------------------------------------
// Backend trait definitions and module organization for Cylo execution environments.
//
// Provides a unified interface for different secure execution backends:
// - ExecutionBackend trait for common operations
// - Backend-specific error types and configurations
// - Platform-conditional module loading
// - AsyncTask-based async patterns (never async fn)
// ============================================================================

// Core modules (extracted from monolithic mod.rs)
mod trait_def;
mod types;
mod config;
mod errors;
mod factory;

// Re-export core types and traits
pub use trait_def::{AsyncTask, ExecutionBackend};
pub use types::{ExecutionRequest, ExecutionResult, HealthStatus, ResourceUsage};
pub use config::{BackendConfig, ResourceLimits};
pub use errors::{BackendError, BackendResult};
pub use factory::{available_backends, create_backend};

// Platform-conditional module imports
#[cfg(target_os = "macos")]
pub mod apple;
#[cfg(target_os = "macos")]
pub use apple::AppleBackend;

#[cfg(target_os = "linux")]
pub mod landlock;
#[cfg(target_os = "linux")]
pub use landlock::LandLockBackend;

#[cfg(target_os = "linux")]
pub mod firecracker;
#[cfg(target_os = "linux")]
pub use firecracker::FireCrackerBackend;

// SweetMCP plugin backend (available on all platforms)
pub mod sweetmcp_plugin;
pub use sweetmcp_plugin::SweetMcpPluginBackend;
