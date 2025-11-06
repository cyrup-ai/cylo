//! ============================================================================
//! File: packages/cylo/src/executor/metrics.rs
//! ----------------------------------------------------------------------------
//! Execution metrics collection and performance tracking.
//! ============================================================================

use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime};
use crate::execution_env::CyloResult;
use crate::backends::{ExecutionRequest, ExecutionResult};
use super::types::{ExecutionMetrics, ResourceStats};

/// Update execution metrics
pub async fn update_metrics(
    metrics: Arc<RwLock<ExecutionMetrics>>,
    backend_name: &str,
    _request: &ExecutionRequest,
    result: &CyloResult<ExecutionResult>,
) {
    if let Ok(mut metrics) = metrics.write() {
        let executions = metrics
            .executions_per_backend
            .entry(backend_name.to_string())
            .or_insert(0);
        *executions += 1;

        if let Ok(exec_result) = result {
            // Update success rate
            let current_success = metrics
                .success_rate
                .get(backend_name)
                .copied()
                .unwrap_or(0.0);
            let new_success = if exec_result.is_success() {
                (current_success * (*executions as f32 - 1.0) + 1.0) / (*executions as f32)
            } else {
                (current_success * (*executions as f32 - 1.0)) / (*executions as f32)
            };
            metrics
                .success_rate
                .insert(backend_name.to_string(), new_success);

            // Update timing metrics
            let current_avg = metrics
                .avg_execution_time
                .get(backend_name)
                .copied()
                .unwrap_or(Duration::from_secs(0));
            let new_avg = Duration::from_nanos(
                (current_avg.as_nanos() as u64 * (*executions - 1)
                    + exec_result.duration.as_nanos() as u64)
                    / *executions,
            );
            metrics
                .avg_execution_time
                .insert(backend_name.to_string(), new_avg);

            // Update resource usage
            let resource_stats = metrics
                .resource_usage
                .entry(backend_name.to_string())
                .or_insert_with(ResourceStats::default);

            let prev_count = *executions - 1;
            resource_stats.avg_memory = (resource_stats.avg_memory * prev_count
                + exec_result.resource_usage.peak_memory)
                / *executions;
            resource_stats.avg_cpu_time = (resource_stats.avg_cpu_time * prev_count
                + exec_result.resource_usage.cpu_time_ms)
                / *executions;
            resource_stats.avg_duration = Duration::from_nanos(
                (resource_stats.avg_duration.as_nanos() as u64 * prev_count
                    + exec_result.duration.as_nanos() as u64)
                    / *executions,
            );

            if exec_result.resource_usage.peak_memory > resource_stats.peak_memory {
                resource_stats.peak_memory = exec_result.resource_usage.peak_memory;
            }
            resource_stats.cumulative_cpu_time += exec_result.resource_usage.cpu_time_ms;
        }

        metrics.last_updated = Some(SystemTime::now());
    }
}
