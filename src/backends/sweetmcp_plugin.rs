//! SweetMCP Plugin Backend
//!
//! This backend executes SweetMCP WASM plugins directly using the Extism runtime.
//! It provides secure execution of tools via WASM sandboxing while maintaining
//! the same interface as other Cylo backends.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use extism::{Manifest, Plugin, Wasm};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use tokio::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallToolRequest {
    pub method: String,
    pub params: CallToolRequestParams,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallToolRequestParams {
    pub name: String,
    pub arguments: Option<JsonValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallToolResult {
    pub content: Option<Vec<CallToolContent>>,
    #[serde(rename = "isError")]
    pub is_error: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallToolContent {
    #[serde(rename = "type")]
    pub content_type: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginCapabilities {
    pub tools: Vec<ToolInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInfo {
    pub name: String,
    pub description: Option<String>,
}

use super::{
    AsyncTask, BackendConfig, BackendError, BackendResult, ExecutionBackend, ExecutionRequest,
    ExecutionResult, HealthStatus, ResourceUsage,
};
use crate::execution_env::CyloResult;

/// SweetMCP Plugin backend implementation
///
/// Executes SweetMCP WASM plugins using the Extism runtime for secure isolation.
/// Tools are executed via the MCP tool protocol but locally without network calls.
#[derive(Debug)]
pub struct SweetMcpPluginBackend {
    /// Path to the WASM plugin file
    plugin_path: PathBuf,
    /// Backend configuration
    config: BackendConfig,
    /// Shared plugin instance (with interior mutability)
    plugin: Arc<Mutex<Plugin>>,
    /// Supported languages (determined by plugin capabilities)
    supported_languages: Vec<String>,
}

impl SweetMcpPluginBackend {
    /// Create a new SweetMCP Plugin backend
    ///
    /// # Arguments
    /// * `plugin_path` - Path to the WASM plugin file
    /// * `config` - Backend configuration
    ///
    /// # Returns
    /// New backend instance or error if plugin cannot be loaded
    pub fn new(plugin_path: PathBuf, config: BackendConfig) -> BackendResult<Self> {
        // Validate plugin file exists
        if !plugin_path.exists() {
            return Err(BackendError::InvalidConfig {
                backend: "SweetMcpPlugin",
                details: format!("Plugin file not found: {}", plugin_path.display()),
            });
        }

        // Load plugin manifest
        let wasm = Wasm::file(&plugin_path);
        let manifest = Manifest::new([wasm]);

        // Create plugin instance
        let mut plugin = Plugin::new(&manifest, [], true).map_err(|e| BackendError::Internal {
            message: format!("Failed to load plugin: {}", e),
        })?;

        // Query plugin for capabilities using describe() function
        let describe_result =
            plugin
                .call::<(), String>("describe", ())
                .map_err(|e| BackendError::Internal {
                    message: format!("Failed to call describe: {}", e),
                })?;

        let capabilities: PluginCapabilities =
            serde_json::from_str(&describe_result).map_err(|e| BackendError::Internal {
                message: format!("Invalid capabilities JSON: {}", e),
            })?;

        // Extract supported languages from tool names
        let supported_languages = capabilities
            .tools
            .iter()
            .filter_map(|tool| {
                if tool.name.starts_with("eval_") {
                    tool.name.strip_prefix("eval_").map(|s| s.to_string())
                } else {
                    None
                }
            })
            .collect();

        Ok(Self {
            plugin_path,
            config,
            plugin: Arc::new(Mutex::new(plugin)),
            supported_languages,
        })
    }

    /// Convert ExecutionRequest to CallToolRequest
    fn execution_to_tool_request(&self, request: &ExecutionRequest) -> CallToolRequest {
        let mut arguments = serde_json::Map::new();
        arguments.insert("code".to_string(), JsonValue::String(request.code.clone()));

        if let Some(input) = &request.input {
            arguments.insert("input".to_string(), JsonValue::String(input.clone()));
        }

        // Add timeout from Duration (convert to seconds)
        let timeout_secs = request.timeout.as_secs();
        if timeout_secs > 0 {
            arguments.insert(
                "timeout".to_string(),
                JsonValue::Number(timeout_secs.into()),
            );
        }

        // Add environment variables
        if !request.env_vars.is_empty() {
            let env_json = JsonValue::Object(
                request
                    .env_vars
                    .iter()
                    .map(|(k, v)| (k.clone(), JsonValue::String(v.clone())))
                    .collect(),
            );
            arguments.insert("env".to_string(), env_json);
        }

        CallToolRequest {
            method: "tools/call".to_string(),
            params: CallToolRequestParams {
                name: format!("eval_{}", request.language),
                arguments: Some(JsonValue::Object(arguments)),
            },
        }
    }

    /// Convert CallToolResult to ExecutionResult
    fn tool_result_to_execution(
        &self,
        result: CallToolResult,
        duration: Duration,
    ) -> ExecutionResult {
        // Check if result is an error
        if result.is_error.unwrap_or(false) || result.content.is_none() {
            let error_msg = result
                .content
                .and_then(|contents| contents.first().map(|c| c.text.clone()))
                .unwrap_or_else(|| "Unknown plugin error".to_string());

            return ExecutionResult {
                exit_code: 1,
                stdout: String::new(),
                stderr: error_msg,
                duration,
                resource_usage: ResourceUsage::default(),
                metadata: HashMap::new(),
            };
        }

        // Extract content from successful result
        let content_text = result
            .content
            .and_then(|contents| contents.first().map(|c| c.text.clone()))
            .unwrap_or_default();

        // Try to parse as JSON for structured output
        if let Ok(result_json) = serde_json::from_str::<JsonValue>(&content_text) {
            let success = result_json
                .get("success")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);

            let stdout = result_json
                .get("stdout")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let stderr = result_json
                .get("stderr")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            ExecutionResult {
                exit_code: if success { 0 } else { 1 },
                stdout,
                stderr,
                duration,
                resource_usage: ResourceUsage::default(),
                metadata: HashMap::new(),
            }
        } else {
            // Fallback for plain text results
            ExecutionResult {
                exit_code: 0,
                stdout: content_text,
                stderr: String::new(),
                duration,
                resource_usage: ResourceUsage::default(),
                metadata: HashMap::new(),
            }
        }
    }
}

impl ExecutionBackend for SweetMcpPluginBackend {
    fn execute_code(&self, request: ExecutionRequest) -> AsyncTask<ExecutionResult> {
        let plugin = Arc::clone(&self.plugin);
        let backend = self.clone_for_async();

        tokio::spawn(async move {
            let start_time = SystemTime::now();

            // Convert request to tool call
            let tool_request = backend.execution_to_tool_request(&request);

            // Serialize the request
            let request_json = match serde_json::to_string(&tool_request) {
                Ok(json) => json,
                Err(e) => {
                    let duration = start_time.elapsed().unwrap_or_default();
                    return ExecutionResult {
                        exit_code: 1,
                        stdout: String::new(),
                        stderr: format!("Request serialization failed: {}", e),
                        duration,
                        resource_usage: ResourceUsage::default(),
                        metadata: HashMap::new(),
                    };
                }
            };

            // Call the plugin
            let mut plugin_guard = plugin.lock().await;
            let response_str = match plugin_guard.call::<String, String>("call", request_json) {
                Ok(response) => response,
                Err(e) => {
                    let duration = start_time.elapsed().unwrap_or_default();
                    return ExecutionResult {
                        exit_code: 1,
                        stdout: String::new(),
                        stderr: format!("Plugin execution failed: {}", e),
                        duration,
                        resource_usage: ResourceUsage::default(),
                        metadata: HashMap::new(),
                    };
                }
            };
            drop(plugin_guard);

            // Parse response
            let tool_result: CallToolResult = match serde_json::from_str(&response_str) {
                Ok(result) => result,
                Err(e) => {
                    let duration = start_time.elapsed().unwrap_or_default();
                    return ExecutionResult {
                        exit_code: 1,
                        stdout: String::new(),
                        stderr: format!("Response parsing failed: {}: {}", e, response_str),
                        duration,
                        resource_usage: ResourceUsage::default(),
                        metadata: HashMap::new(),
                    };
                }
            };

            let duration = start_time.elapsed().unwrap_or_default();
            backend.tool_result_to_execution(tool_result, duration)
        })
    }

    fn health_check(&self) -> AsyncTask<HealthStatus> {
        let plugin_path = self.plugin_path.clone();
        let plugin = Arc::clone(&self.plugin);

        tokio::spawn(async move {
            // Check if plugin file exists
            if !plugin_path.exists() {
                return HealthStatus::unhealthy(format!(
                    "Plugin file not found: {}",
                    plugin_path.display()
                ));
            }

            // Try calling describe function to verify plugin is functional
            let mut plugin_guard = plugin.lock().await;
            match plugin_guard.call::<(), String>("describe", ()) {
                Ok(_) => HealthStatus::healthy("Plugin is functional")
                    .with_metric("plugin_path", plugin_path.display().to_string().as_str()),
                Err(e) => HealthStatus::unhealthy(format!("Plugin health check failed: {}", e)),
            }
        })
    }

    fn cleanup(&self) -> AsyncTask<CyloResult<()>> {
        tokio::spawn(async move {
            // SweetMCP plugins don't require explicit cleanup
            // The Extism runtime handles WASM instance cleanup automatically
            Ok(())
        })
    }

    fn get_config(&self) -> &BackendConfig {
        &self.config
    }

    fn backend_type(&self) -> &'static str {
        "SweetMcpPlugin"
    }

    fn supports_language(&self, language: &str) -> bool {
        self.supported_languages.iter().any(|lang| lang == language)
    }

    fn supported_languages(&self) -> &[&'static str] {
        // Convert Vec<String> to static slice for trait compatibility
        // This is safe because we're returning references to heap-allocated strings
        unsafe {
            std::slice::from_raw_parts(
                self.supported_languages.as_ptr() as *const &'static str,
                self.supported_languages.len(),
            )
        }
    }
}

// Helper implementation for async cloning
impl SweetMcpPluginBackend {
    fn clone_for_async(&self) -> Self {
        Self {
            plugin_path: self.plugin_path.clone(),
            config: self.config.clone(),
            plugin: Arc::clone(&self.plugin),
            supported_languages: self.supported_languages.clone(),
        }
    }
}
