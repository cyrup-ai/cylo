// ============================================================================
// File: packages/cylo/src/backends/firecracker/mod.rs
// ----------------------------------------------------------------------------
// FireCracker backend module - organizes all firecracker components.
//
// This module decomposes the original monolithic firecracker.rs (1,648 lines)
// into logical separation of concerns:
// - api_client: HTTP API client for VM management (390 lines)
// - config: Configuration structures and validation (121 lines)
// - ssh: SSH configuration and session management (86 lines)
// - vm_instance: VM struct and basic operations (170 lines)
// - vm_lifecycle: VM startup and configuration (246 lines)
// - vm_execution: Code execution in VM (245 lines)
// - backend: ExecutionBackend trait implementation (295 lines)
//
// Total: 1,553 lines (no single module >= 500 lines)
// ============================================================================

// Firecracker backend is Linux-only (requires KVM)
#![cfg(target_os = "linux")]

mod api_client;
mod config;
mod ssh;
mod vm_instance;
mod vm_lifecycle;
mod vm_execution;
mod backend;

// Re-export main backend for external use
pub use backend::FireCrackerBackend;

// Re-export commonly used types
pub use api_client::{FireCrackerApiClient, ResourceStats, SecurityPolicy, FilesystemRestrictions};
pub use config::FireCrackerConfig;
pub use vm_instance::VMInstance;
pub use ssh::{SshConfig, SshAuth};
