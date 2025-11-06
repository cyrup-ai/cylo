// ============================================================================
// File: packages/cylo/src/backends/apple/resource_stats.rs
// ----------------------------------------------------------------------------
// Resource usage statistics parsing for Apple containerization backend.
// ============================================================================

use std::process::Command;

use crate::backends::ResourceUsage;

/// Parse resource usage from container stats
///
/// # Arguments
/// * `container_name` - Name of the container
///
/// # Returns
/// Resource usage statistics or None if unavailable
pub(super) async fn parse_resource_usage(container_name: &str) -> Option<ResourceUsage> {
    let stats_result = Command::new("container")
        .args(["stats", "--no-stream", "--format", "json", container_name])
        .output();

    match stats_result {
        Ok(output) if output.status.success() => {
            let stats_json = String::from_utf8_lossy(&output.stdout);
            if let Ok(stats) = serde_json::from_str::<serde_json::Value>(&stats_json) {
                Some(ResourceUsage {
                    peak_memory: stats["memory"]["usage"].as_u64().unwrap_or(0),
                    cpu_time_ms: stats["cpu"]["total_usage"].as_u64().unwrap_or(0) / 1_000_000,
                    process_count: stats["pids"]["current"].as_u64().unwrap_or(0) as u32,
                    disk_bytes_written: stats["blkio"]["io_service_bytes_recursive"]
                        .as_array()
                        .and_then(|arr| arr.iter().find(|entry| entry["op"] == "Write"))
                        .and_then(|entry| entry["value"].as_u64())
                        .unwrap_or(0),
                    disk_bytes_read: stats["blkio"]["io_service_bytes_recursive"]
                        .as_array()
                        .and_then(|arr| arr.iter().find(|entry| entry["op"] == "Read"))
                        .and_then(|entry| entry["value"].as_u64())
                        .unwrap_or(0),
                    network_bytes_sent: stats["network"]["tx_bytes"].as_u64().unwrap_or(0),
                    network_bytes_received: stats["network"]["rx_bytes"].as_u64().unwrap_or(0),
                })
            } else {
                None
            }
        }
        _ => None,
    }
}
