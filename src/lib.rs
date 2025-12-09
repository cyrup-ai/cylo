//! ============================================================================
//! Cylo: Secure Multi-Platform Code Execution Framework
//! ============================================================================
//!
//! High-performance secure code execution with multiple isolation backends:
//! - Apple containerization for macOS with Apple Silicon
//! - `LandLock` sandboxing for Linux with kernel-level security
//! - `FireCracker` microVMs for ultra-lightweight virtualization
//!
//! Features:
//! - Zero allocation in hot paths
//! - Lock-free concurrent operations
//! - Platform-aware backend selection
//! - Instance lifecycle management
//! - Comprehensive health monitoring
//!
//! ## Quick Start
//!
//! ```rust
//! use fluent_ai_cylo::{Cylo, global_instance_manager};
//!
//! // Create execution environment
//! let cylo_env = Cylo::Apple("python:alpine3.20".to_string());
//! let instance = cylo_env.instance("my_python_env");
//!
//! // Register with global manager
//! let manager = global_instance_manager();
//! manager.register_instance(instance).await?;
//!
//! // Execute code
//! let request = ExecutionRequest::new("print('Hello, World!')", "python");
//! let result = manager.get_instance("my_python_env").await?
//!     .execute_code(request).await;
//! ```

// ============================================================================
// Core execution environment types
// ============================================================================

pub mod execution_env;
pub use execution_env::{
    Cylo, CyloError, CyloInstance, CyloResult, validate_environment_spec, validate_instance_name,
};

// ============================================================================
// Backend implementations and traits
// ============================================================================

pub mod backends;
pub use backends::{
    // Backend implementations
    BackendConfig,
    // Trait
    ExecutionBackend,
    ExecutionRequest,
    ExecutionResult,
    HealthStatus,
    // Factory function
    create_backend,
};
// Platform-specific backends
#[cfg(target_os = "macos")]
pub use backends::AppleBackend;
#[cfg(target_os = "linux")]
pub use backends::{FireCrackerBackend, LandLockBackend};

// ============================================================================
// Platform detection and capabilities
// ============================================================================

pub mod platform;
pub use platform::{
    // Structs
    Architecture,
    BackendAvailability,
    OperatingSystem,
    PerformanceHints,
    PlatformInfo,
    get_available_backends,
    get_recommended_backend,
    has_kvm,
    has_landlock,
    is_apple_silicon,
    is_linux,
};

// ============================================================================
// Error handling
// ============================================================================

pub mod error;
pub use error::{ExecError, StorageError};

// ============================================================================
// Configuration and utilities
// ============================================================================

pub mod config;
pub use config::{FileSystem, RamdiskConfig};

pub mod platform_utils;
pub use platform_utils::set_executable;

pub mod exec;
pub use exec::{exec_bash, exec_go, exec_js, exec_python, exec_rust};

pub mod ramdisk;
pub use ramdisk::{create_ramdisk, create_secure_ramdisk, get_watched_dir, remove_ramdisk};

pub mod metadata;
pub use metadata::MetadataManager;

pub mod sandbox;

pub mod jail;

pub mod state;
pub use state::PipelineEvent;

#[cfg(target_os = "linux")]
pub mod firecracker;
#[cfg(target_os = "linux")]
pub use firecracker::{FirecrackerVM, create_firecracker_environment, is_firecracker_available};

pub mod task;
pub use task::{ExecutionPool, ExecutionTask};

// Platform-specific modules
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "macos")]
pub use macos::MacosRamdisk;

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "linux")]
pub use linux::LinuxRamdisk;

#[cfg(target_os = "windows")]
pub mod windows;
#[cfg(target_os = "windows")]
pub use windows::WindowsRamdisk;

// ============================================================================
// Global instance manager
// ============================================================================

pub mod instance_manager;
pub use instance_manager::{
    InstanceManager, global_instance_manager, init_global_instance_manager,
};
// ============================================================================
// Asynchronous task utilities
// ============================================================================
use serde::{Deserialize, Serialize};

// Use our own async task implementations
pub use crate::async_task::{AsyncTask, AsyncTaskBuilder};

// ============================================================================
// Performance monitoring and diagnostics
// ============================================================================

/// Get comprehensive platform and backend diagnostics
pub fn get_diagnostics() -> AsyncTask<DiagnosticsReport> {
    AsyncTaskBuilder::new(async move {
        let platform_info = platform::detect_platform();
        let available_backends = get_available_backends();
        let manager = global_instance_manager();

        let health_results = match manager.health_check_all().await {
            Ok(Ok(results)) => results,
            Ok(Err(_)) => std::collections::HashMap::new(),
            Err(_) => std::collections::HashMap::new(),
        };

        let instance_list = manager.list_instances().unwrap_or_default();

        DiagnosticsReport {
            platform: platform_info.clone(),
            available_backends,
            backend_health: health_results,
            active_instances: instance_list,
            performance_hints: platform_info.performance.clone(),
        }
    })
    .spawn()
}

/// Comprehensive diagnostics report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticsReport {
    /// Platform information
    pub platform: PlatformInfo,
    /// Available execution backends
    pub available_backends: Vec<String>,
    /// Backend health status
    pub backend_health: std::collections::HashMap<String, HealthStatus>,
    /// Currently active instances
    pub active_instances: Vec<String>,
    /// Performance optimization hints
    pub performance_hints: PerformanceHints,
}

// ============================================================================
// AsyncTask module - simple wrapper around tokio for backend compatibility
// ============================================================================

pub mod async_task {
    /// AsyncTask is a type alias for tokio::task::JoinHandle
    pub type AsyncTask<T> = tokio::task::JoinHandle<T>;

    /// Simple AsyncTaskBuilder for fluent construction
    pub struct AsyncTaskBuilder<F> {
        future: F,
    }

    impl<F, T> AsyncTaskBuilder<F>
    where
        F: std::future::Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        /// Create a new AsyncTaskBuilder
        pub fn new(future: F) -> Self {
            Self { future }
        }

        /// Spawn the task and return the AsyncTask handle
        pub fn spawn(self) -> AsyncTask<T> {
            tokio::spawn(self.future)
        }
    }

    /// Convenience function to spawn an async task
    pub fn spawn_async<F, T>(future: F) -> AsyncTask<T>
    where
        F: std::future::Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        tokio::spawn(future)
    }
}
