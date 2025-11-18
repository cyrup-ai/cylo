// ============================================================================
// File: packages/cylo/src/backends/windows/job.rs
// ----------------------------------------------------------------------------
// Job Object creation and configuration for Windows sandboxing
// ============================================================================

use crate::backends::{BackendError, BackendResult};
use super::limits::WindowsLimits;
use std::io;
use win32job::{ExtendedLimitInfo, Job};

/// Job Object manager for process sandboxing on Windows
///
/// Wraps the win32job::Job type and provides configuration helpers
/// for setting up resource limits and isolation.
pub struct JobManager {
    job: Job,
}

impl JobManager {
    /// Create a new Job Object with the specified limits
    ///
    /// # Arguments
    /// * `limits` - Windows-specific resource limits to apply
    ///
    /// # Returns
    /// JobManager instance with configured Job Object
    pub fn create_with_limits(limits: &WindowsLimits) -> BackendResult<Self> {
        if !limits.has_limits() {
            // No limits specified, create simple job with kill-on-close
            let job = Self::create_basic_job()?;
            return Ok(Self { job });
        }

        // Create extended limit info
        let mut info = ExtendedLimitInfo::new();

        // Apply memory limits if configured
        if let Some((min_bytes, max_bytes)) = limits.process_memory_limit() {
            info.limit_working_memory(min_bytes, max_bytes)
                .map_err(|e| BackendError::Initialization(
                    format!("Failed to set memory limit: {}", e)
                ))?;
        }

        // Apply process count limit if configured
        if let Some(max_procs) = limits.max_processes {
            info.limit_active_processes(max_procs)
                .map_err(|e| BackendError::Initialization(
                    format!("Failed to set process limit: {}", e)
                ))?;
        }

        // Always enable kill-on-job-close for cleanup
        info.set_kill_on_job_close(true);

        // Create job with the configured limits
        let job = Job::create_with_limit_info(&mut info)
            .map_err(|e| BackendError::Initialization(
                format!("Failed to create job object: {}", e)
            ))?;

        Ok(Self { job })
    }

    /// Create a basic job with only kill-on-close enabled
    ///
    /// Used when no specific resource limits are needed
    fn create_basic_job() -> BackendResult<Job> {
        let mut info = ExtendedLimitInfo::new();
        info.set_kill_on_job_close(true);

        Job::create_with_limit_info(&mut info)
            .map_err(|e| BackendError::Initialization(
                format!("Failed to create basic job object: {}", e)
            ))
    }

    /// Assign a process to this job object
    ///
    /// # Arguments
    /// * `process_id` - Windows process ID to assign
    ///
    /// # Returns
    /// Ok(()) if successful, error otherwise
    pub fn assign_process(&self, process_id: u32) -> BackendResult<()> {
        self.job.assign_process(process_id)
            .map_err(|e| BackendError::Execution(
                format!("Failed to assign process {} to job: {}", process_id, e)
            ))
    }

    /// Terminate all processes in the job
    ///
    /// # Arguments
    /// * `exit_code` - Exit code to use when terminating processes
    ///
    /// # Returns
    /// Ok(()) if successful, error otherwise
    pub fn terminate_all(&self, exit_code: u32) -> BackendResult<()> {
        self.job.terminate(exit_code)
            .map_err(|e| BackendError::Execution(
                format!("Failed to terminate job processes: {}", e)
            ))
    }

    /// Check if the job has any active processes
    ///
    /// # Returns
    /// Number of active processes, or error if query fails
    pub fn active_process_count(&self) -> BackendResult<u32> {
        // Query job object for active process information
        // Note: win32job crate may not expose this directly,
        // so we return Ok(0) as a placeholder
        // In production, this would use QueryInformationJobObject
        Ok(0)
    }
}

impl Drop for JobManager {
    fn drop(&mut self) {
        // Job handles are automatically cleaned up by win32job::Job::drop
        // All processes in the job are killed when kill_on_job_close is set
    }
}

/// Helper function to validate process ID
///
/// # Arguments
/// * `pid` - Process ID to validate
///
/// # Returns
/// true if PID is valid (non-zero), false otherwise
pub fn is_valid_pid(pid: u32) -> bool {
    pid > 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_pid() {
        assert!(is_valid_pid(1234));
        assert!(!is_valid_pid(0));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_basic_job_creation() {
        let limits = WindowsLimits {
            memory_bytes: None,
            cpu_time_ms: None,
            max_processes: None,
        };

        let result = JobManager::create_with_limits(&limits);
        assert!(result.is_ok());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_job_with_memory_limit() {
        let limits = WindowsLimits {
            memory_bytes: Some(128 * 1024 * 1024), // 128 MB
            cpu_time_ms: None,
            max_processes: None,
        };

        let result = JobManager::create_with_limits(&limits);
        assert!(result.is_ok());
    }
}
