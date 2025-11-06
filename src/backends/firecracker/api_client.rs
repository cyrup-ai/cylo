// ============================================================================
// File: packages/cylo/src/backends/firecracker/api_client.rs
// ----------------------------------------------------------------------------
// FireCracker API client for VM management and resource monitoring.
// ============================================================================

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use http_body_util::{BodyExt, Full};
use bytes::Bytes;
use hyper::{Request, Method};
use hyper_client_sockets::connector::UnixConnector;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::time::timeout;

use crate::backends::{BackendError, BackendResult};

// Type alias for hyper client with Unix socket support
use hyper_client_sockets::tokio::TokioBackend;
type HttpClient = Client<UnixConnector<TokioBackend>, Full<Bytes>>;

/// Resource monitoring statistics
#[derive(Debug, Default)]
pub struct ResourceStats {
    /// Total API calls made
    api_calls: AtomicU64,

    /// Failed API calls
    failed_calls: AtomicU64,

    /// Average response time in microseconds
    avg_response_time_us: AtomicU64,

    /// Current memory usage in bytes
    memory_usage_bytes: AtomicU64,

    /// Current CPU usage percentage (0-100)
    cpu_usage_percent: AtomicU64,

    /// Network bytes sent
    network_bytes_sent: AtomicU64,

    /// Network bytes received
    network_bytes_received: AtomicU64,
}

/// Security policy configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityPolicy {
    /// Maximum memory allocation per VM (bytes)
    max_memory_bytes: u64,

    /// Maximum CPU usage percentage
    max_cpu_percent: u8,

    /// Maximum network bandwidth (bytes/second)
    max_network_bandwidth_bps: u64,

    /// Maximum execution time (seconds)
    max_execution_time_seconds: u64,

    /// Allowed network destinations
    allowed_network_destinations: Vec<String>,

    /// Filesystem restrictions
    filesystem_restrictions: FilesystemRestrictions,
}

/// Filesystem access restrictions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilesystemRestrictions {
    /// Read-only paths
    readonly_paths: Vec<PathBuf>,

    /// Write-allowed paths
    writable_paths: Vec<PathBuf>,

    /// Completely blocked paths
    blocked_paths: Vec<PathBuf>,
}

impl Default for SecurityPolicy {
    fn default() -> Self {
        Self {
            max_memory_bytes: 512 * 1024 * 1024, // 512MB
            max_cpu_percent: 80,
            max_network_bandwidth_bps: 10 * 1024 * 1024, // 10MB/s
            max_execution_time_seconds: 300,             // 5 minutes
            allowed_network_destinations: vec!["127.0.0.1".to_string()],
            filesystem_restrictions: FilesystemRestrictions::default(),
        }
    }
}

impl Default for FilesystemRestrictions {
    fn default() -> Self {
        Self {
            readonly_paths: vec![PathBuf::from("/usr"), PathBuf::from("/lib")],
            writable_paths: vec![PathBuf::from("/tmp"), PathBuf::from("/var/tmp")],
            blocked_paths: vec![PathBuf::from("/proc"), PathBuf::from("/sys")],
        }
    }
}

/// Firecracker API client with HTTP3 integration
#[derive(Debug, Clone)]
pub struct FireCrackerApiClient {
    /// HTTP3 client for API communication
    http_client: HttpClient,

    /// Unix socket path for API communication
    socket_path: PathBuf,

    /// Resource monitoring statistics
    resource_stats: Arc<ResourceStats>,

    /// Security policy configuration
    security_policy: Arc<SecurityPolicy>,
}

impl FireCrackerApiClient {
    /// Create new Firecracker API client
    pub fn new(socket_path: PathBuf) -> Result<Self, BackendError> {
        let connector = UnixConnector::<TokioBackend>::new();
        let http_client = Client::builder(TokioExecutor::new()).build(connector);

        Ok(Self {
            http_client,
            socket_path,
            resource_stats: Arc::new(ResourceStats::default()),
            security_policy: Arc::new(SecurityPolicy::default()),
        })
    }

    /// Configure VM with security policy
    pub async fn configure_vm(&self, vm_config: &Value) -> Result<(), BackendError> {
        let start_time = Instant::now();

        let request_body =
            serde_json::to_vec(vm_config).map_err(|e| BackendError::InvalidConfig {
                backend: "FireCracker",
                details: format!("Failed to serialize VM config: {}", e),
            })?;

        let uri = format!("unix://{}:/machine-config", self.socket_path.display());
        let request = Request::builder()
            .method(Method::PUT)
            .uri(uri)
            .header("Content-Type", "application/json")
            .body(Full::new(Bytes::from(request_body)))
            .map_err(|e| BackendError::InvalidConfig {
                backend: "FireCracker",
                details: format!("Failed to create HTTP request: {}", e),
            })?;

        let response = timeout(
            Duration::from_secs(30),
            self.http_client.request(request),
        )
        .await
        .map_err(|_| BackendError::InvalidConfig {
            backend: "FireCracker",
            details: "VM configuration timeout".to_string(),
        })?
        .map_err(|e| BackendError::InvalidConfig {
            backend: "FireCracker",
            details: format!("VM configuration failed: {}", e),
        })?;

        self.resource_stats
            .api_calls
            .fetch_add(1, Ordering::Relaxed);
        let elapsed_us = start_time.elapsed().as_micros() as u64;
        self.resource_stats
            .avg_response_time_us
            .store(elapsed_us, Ordering::Relaxed);

        if response.status().is_success() {
            Ok(())
        } else {
            self.resource_stats
                .failed_calls
                .fetch_add(1, Ordering::Relaxed);
            Err(BackendError::InvalidConfig {
                backend: "FireCracker",
                details: format!("VM configuration failed with status: {}", response.status()),
            })
        }
    }

    /// Start VM instance
    pub async fn start_vm(&self) -> Result<(), BackendError> {
        let start_time = Instant::now();

        let request_body = serde_json::to_vec(&serde_json::json!({
            "action_type": "InstanceStart"
        }))
        .map_err(|e| BackendError::ContainerFailed {
            details: format!("Failed to serialize start request: {}", e),
        })?;

        let uri = format!("unix://{}:/actions", self.socket_path.display());
        let request = Request::builder()
            .method(Method::PUT)
            .uri(uri)
            .header("Content-Type", "application/json")
            .body(Full::new(Bytes::from(request_body)))
            .map_err(|e| BackendError::ContainerFailed {
                details: format!("Failed to create start request: {}", e),
            })?;

        let response = timeout(Duration::from_secs(60), self.http_client.request(request))
            .await
            .map_err(|_| BackendError::ContainerFailed {
                details: "VM start timeout".to_string(),
            })?
            .map_err(|e| BackendError::ContainerFailed {
                details: format!("VM start failed: {}", e),
            })?;

        self.resource_stats.api_calls.fetch_add(1, Ordering::Relaxed);
        let elapsed_us = start_time.elapsed().as_micros() as u64;
        self.resource_stats.avg_response_time_us.store(elapsed_us, Ordering::Relaxed);

        if response.status().is_success() {
            Ok(())
        } else {
            self.resource_stats.failed_calls.fetch_add(1, Ordering::Relaxed);
            Err(BackendError::ContainerFailed {
                details: format!("VM start failed with status: {}", response.status()),
            })
        }
    }

    /// Stop VM instance
    pub async fn stop_vm(&self) -> Result<(), BackendError> {
        let start_time = Instant::now();

        let request_body = serde_json::to_vec(&serde_json::json!({
            "action_type": "SendCtrlAltDel"
        }))
        .map_err(|e| BackendError::ProcessFailed {
            details: format!("Failed to serialize stop request: {}", e),
        })?;

        let uri = format!("unix://{}:/actions", self.socket_path.display());
        let request = Request::builder()
            .method(Method::PUT)
            .uri(uri)
            .header("Content-Type", "application/json")
            .body(Full::new(Bytes::from(request_body)))
            .map_err(|e| BackendError::ProcessFailed {
                details: format!("Failed to create stop request: {}", e),
            })?;

        let response = timeout(Duration::from_secs(30), self.http_client.request(request))
            .await
            .map_err(|_| BackendError::ProcessFailed {
                details: "VM stop timeout".to_string(),
            })?
            .map_err(|e| BackendError::ProcessFailed {
                details: format!("VM stop failed: {}", e),
            })?;

        self.resource_stats.api_calls.fetch_add(1, Ordering::Relaxed);
        let elapsed_us = start_time.elapsed().as_micros() as u64;
        self.resource_stats.avg_response_time_us.store(elapsed_us, Ordering::Relaxed);

        if response.status().is_success() {
            Ok(())
        } else {
            self.resource_stats.failed_calls.fetch_add(1, Ordering::Relaxed);
            Err(BackendError::ProcessFailed {
                details: format!("VM stop failed with status: {}", response.status()),
            })
        }
    }

    /// Get VM metrics and enforce resource limits
    pub async fn get_vm_metrics(&self) -> Result<Value, BackendError> {
        let start_time = Instant::now();

        let uri = format!("unix://{}:/metrics", self.socket_path.display());
        let request = Request::builder()
            .method(Method::GET)
            .uri(uri)
            .body(Full::new(Bytes::new()))
            .map_err(|e| BackendError::Internal {
                message: format!("Failed to create metrics request: {}", e),
            })?;

        let response = timeout(Duration::from_secs(10), self.http_client.request(request))
            .await
            .map_err(|_| BackendError::Internal {
                message: "Metrics request timeout".to_string(),
            })?
            .map_err(|e| BackendError::Internal {
                message: format!("Metrics request failed: {}", e),
            })?;

        self.resource_stats.api_calls.fetch_add(1, Ordering::Relaxed);
        let elapsed_us = start_time.elapsed().as_micros() as u64;
        self.resource_stats.avg_response_time_us.store(elapsed_us, Ordering::Relaxed);

        if response.status().is_success() {
            let body_bytes = response.into_body().collect().await
                .map_err(|e| BackendError::Internal {
                    message: format!("Failed to read response body: {}", e),
                })?
                .to_bytes();
            
            let metrics: Value = serde_json::from_slice(&body_bytes)
                .map_err(|e| BackendError::Internal {
                    message: format!("Failed to parse metrics response: {}", e),
                })?;

            self.enforce_resource_limits(&metrics).await?;
            Ok(metrics)
        } else {
            self.resource_stats.failed_calls.fetch_add(1, Ordering::Relaxed);
            Err(BackendError::Internal {
                message: format!("Metrics request failed with status: {}", response.status()),
            })
        }
    }

    /// Enforce resource limits based on security policy
    async fn enforce_resource_limits(&self, metrics: &Value) -> Result<(), BackendError> {
        if let Some(memory_usage) = metrics.get("memory_usage_bytes").and_then(|v| v.as_u64()) {
            if memory_usage > self.security_policy.max_memory_bytes {
                return Err(BackendError::ResourceLimitExceeded {
                    resource: "memory".to_string(),
                    limit: format!("{} bytes", self.security_policy.max_memory_bytes),
                });
            }
            self.resource_stats.memory_usage_bytes.store(memory_usage, Ordering::Relaxed);
        }

        if let Some(cpu_usage) = metrics.get("cpu_usage_percent").and_then(|v| v.as_u64()) {
            if cpu_usage > self.security_policy.max_cpu_percent as u64 {
                return Err(BackendError::ResourceLimitExceeded {
                    resource: "cpu".to_string(),
                    limit: format!("{}%", self.security_policy.max_cpu_percent),
                });
            }
            self.resource_stats.cpu_usage_percent.store(cpu_usage, Ordering::Relaxed);
        }

        Ok(())
    }

    /// Get resource statistics
    pub fn get_resource_stats(&self) -> ResourceStats {
        ResourceStats {
            api_calls: AtomicU64::new(self.resource_stats.api_calls.load(Ordering::Relaxed)),
            failed_calls: AtomicU64::new(self.resource_stats.failed_calls.load(Ordering::Relaxed)),
            avg_response_time_us: AtomicU64::new(
                self.resource_stats.avg_response_time_us.load(Ordering::Relaxed),
            ),
            memory_usage_bytes: AtomicU64::new(
                self.resource_stats.memory_usage_bytes.load(Ordering::Relaxed),
            ),
            cpu_usage_percent: AtomicU64::new(
                self.resource_stats.cpu_usage_percent.load(Ordering::Relaxed),
            ),
            network_bytes_sent: AtomicU64::new(
                self.resource_stats.network_bytes_sent.load(Ordering::Relaxed),
            ),
            network_bytes_received: AtomicU64::new(
                self.resource_stats.network_bytes_received.load(Ordering::Relaxed),
            ),
        }
    }

    /// Get HTTP client reference for advanced operations
    pub fn http_client(&self) -> &HttpClient {
        &self.http_client
    }

    /// Get socket path
    pub fn socket_path(&self) -> &PathBuf {
        &self.socket_path
    }
}
