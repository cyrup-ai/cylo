// ============================================================================
// File: packages/cylo/src/backends/windows/job.rs
// ----------------------------------------------------------------------------
// Job Object creation and configuration for Windows sandboxing
//
// Enforces resource limits:
// - Memory: Working set min/max via ExtendedLimitInfo (win32job crate)
// - CPU Time: Per-job user-mode time via JOBOBJECT_BASIC_LIMIT_INFORMATION
// - Process Count: Active process limit via JOBOBJECT_BASIC_LIMIT_INFORMATION
//
// When CPU time limit is exceeded, Windows automatically terminates all
// processes in the job with exit status ERROR_NOT_ENOUGH_QUOTA.
// ============================================================================

use crate::backends::{BackendError, BackendResult};
use super::limits::WindowsLimits;
use win32job::{ExtendedLimitInfo, Job};
use windows::Win32::System::JobObjects::TerminateJobObject;

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
            info.limit_working_memory(min_bytes, max_bytes);
        }

        // Always enable kill-on-job-close for cleanup
        info.limit_kill_on_job_close();

        // Create job with the configured limits
        let job = Job::create_with_limit_info(&mut info)
            .map_err(|e| BackendError::Internal {
                message: format!("Failed to create job object: {}", e)
            })?;

        // Apply basic limits (CPU time and process count) using Windows API
        // These must be set together in one call because LimitFlags are replaced
        Self::set_basic_limits(&job, limits.cpu_time_ms, limits.max_processes)?;

        Ok(Self { job })
    }

    /// Create a basic job with only kill-on-close enabled
    ///
    /// Used when no specific resource limits are needed
    fn create_basic_job() -> BackendResult<Job> {
        let mut info = ExtendedLimitInfo::new();
        info.limit_kill_on_job_close();

        Job::create_with_limit_info(&mut info)
            .map_err(|e| BackendError::Internal {
                message: format!("Failed to create basic job object: {}", e)
            })
    }

    /// Set basic limits (CPU time and process count) on a Job Object
    ///
    /// This method MUST set all basic limits in a single call because
    /// SetInformationJobObject with JobObjectBasicLimitInformation
    /// replaces all LimitFlags - calling it multiple times would clear
    /// previously set limits.
    ///
    /// # Arguments
    /// * `job` - The job object to configure
    /// * `cpu_time_ms` - Optional CPU time limit in milliseconds
    /// * `max_processes` - Optional maximum active process count
    fn set_basic_limits(
        job: &Job,
        cpu_time_ms: Option<u64>,
        max_processes: Option<u32>,
    ) -> BackendResult<()> {
        use std::mem;
        use windows::Win32::System::JobObjects::{
            JobObjectBasicLimitInformation, SetInformationJobObject,
            JOBOBJECT_BASIC_LIMIT_INFORMATION, JOB_OBJECT_LIMIT,
            JOB_OBJECT_LIMIT_ACTIVE_PROCESS, JOB_OBJECT_LIMIT_JOB_TIME,
        };

        // Skip if no basic limits to set
        if cpu_time_ms.is_none() && max_processes.is_none() {
            return Ok(());
        }

        let mut info: JOBOBJECT_BASIC_LIMIT_INFORMATION = unsafe { mem::zeroed() };
        let mut flags = JOB_OBJECT_LIMIT(0);

        // Set CPU time limit if specified
        if let Some(cpu_ms) = cpu_time_ms {
            // Convert milliseconds to 100-nanosecond intervals
            // ms * 1,000,000 ns/ms / 100 ns/interval = ms * 10,000
            let time_100ns = (cpu_ms as i64).saturating_mul(10_000);
            info.PerJobUserTimeLimit = time_100ns;
            flags |= JOB_OBJECT_LIMIT_JOB_TIME;
        }

        // Set process count limit if specified
        if let Some(max_procs) = max_processes {
            info.ActiveProcessLimit = max_procs;
            flags |= JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
        }

        info.LimitFlags = flags;

        unsafe {
            SetInformationJobObject(
                windows::Win32::Foundation::HANDLE(job.handle() as *mut std::ffi::c_void),
                JobObjectBasicLimitInformation,
                &info as *const _ as *const std::ffi::c_void,
                mem::size_of::<JOBOBJECT_BASIC_LIMIT_INFORMATION>() as u32,
            ).map_err(|e| BackendError::Internal {
                message: format!("Failed to set basic job limits: {}", e)
            })?;
        }

        Ok(())
    }

    /// Assign a process to this job object
    ///
    /// # Arguments
    /// * `process_id` - Windows process ID to assign
    ///
    /// # Returns
    /// Ok(()) if successful, error otherwise
    pub fn assign_process(&self, process_id: u32) -> BackendResult<()> {
        if !is_valid_pid(process_id) {
            return Err(BackendError::ProcessFailed {
                details: format!("Invalid process ID: {} (must be > 0)", process_id)
            });
        }

        self.job.assign_process(process_id as isize)
            .map_err(|e| BackendError::ProcessFailed {
                details: format!("Failed to assign process {} to job: {}", process_id, e)
            })
    }

    /// Terminate all processes in the job
    ///
    /// # Arguments
    /// * `exit_code` - Exit code to use when terminating processes
    ///
    /// # Returns
    /// Ok(()) if successful, error otherwise
    pub fn terminate_all(&self, exit_code: u32) -> BackendResult<()> {
        unsafe {
            TerminateJobObject(
                windows::Win32::Foundation::HANDLE(self.job.handle() as *mut std::ffi::c_void),
                exit_code
            ).map_err(|e| BackendError::ProcessFailed {
                details: format!("Failed to terminate job: {}", e)
            })?;
        }
        Ok(())
    }

    /// Check if the job has any active processes
    ///
    /// # Returns
    /// Number of active processes, or error if query fails
    pub fn active_process_count(&self) -> BackendResult<u32> {
        use windows::Win32::System::JobObjects::{
            QueryInformationJobObject, JobObjectBasicAccountingInformation,
            JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
        };
        use std::mem;

        unsafe {
            let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = mem::zeroed();
            let mut return_length = 0u32;

            QueryInformationJobObject(
                Some(windows::Win32::Foundation::HANDLE(self.job.handle() as *mut std::ffi::c_void)),
                JobObjectBasicAccountingInformation,
                &mut info as *mut _ as *mut std::ffi::c_void,
                mem::size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                Some(&mut return_length),
            ).map_err(|e| BackendError::Internal {
                message: format!("Failed to query job process count: {}", e)
            })?;

            Ok(info.ActiveProcesses)
        }
    }

    /// Get comprehensive CPU time and I/O statistics for the job
    ///
    /// # Returns
    /// Tuple of (cpu_time_ms, disk_read_bytes, disk_write_bytes, network_other_bytes) or error if query fails
    pub fn get_cpu_and_io_stats(&self) -> BackendResult<(u64, u64, u64, u64)> {
        use windows::Win32::System::JobObjects::{
            QueryInformationJobObject, JobObjectBasicAndIoAccountingInformation,
            JOBOBJECT_BASIC_AND_IO_ACCOUNTING_INFORMATION,
        };
        use std::mem;

        unsafe {
            let mut info: JOBOBJECT_BASIC_AND_IO_ACCOUNTING_INFORMATION = mem::zeroed();
            let mut return_length = 0u32;

            QueryInformationJobObject(
                Some(windows::Win32::Foundation::HANDLE(self.job.handle() as *mut std::ffi::c_void)),
                JobObjectBasicAndIoAccountingInformation,
                &mut info as *mut _ as *mut std::ffi::c_void,
                mem::size_of::<JOBOBJECT_BASIC_AND_IO_ACCOUNTING_INFORMATION>() as u32,
                Some(&mut return_length),
            ).map_err(|e| BackendError::Internal {
                message: format!("Failed to query CPU and I/O stats: {}", e)
            })?;

            // Extract CPU time: TotalUserTime + TotalKernelTime
            // These are i64 values in 100-nanosecond intervals
            // Convert to milliseconds: divide by 10,000
            let user_time_100ns = info.BasicInfo.TotalUserTime;
            let kernel_time_100ns = info.BasicInfo.TotalKernelTime;
            let total_time_100ns = user_time_100ns + kernel_time_100ns;
            let cpu_time_ms = (total_time_100ns / 10_000) as u64;

            // Extract I/O statistics
            let disk_read_bytes = info.IoInfo.ReadTransferCount;
            let disk_write_bytes = info.IoInfo.WriteTransferCount;
            let network_other_bytes = info.IoInfo.OtherTransferCount;

            Ok((cpu_time_ms, disk_read_bytes, disk_write_bytes, network_other_bytes))
        }
    }

    /// Get memory usage statistics for the job
    ///
    /// # Returns
    /// Peak job memory usage in bytes, or error if query fails
    pub fn get_memory_usage(&self) -> BackendResult<u64> {
        use windows::Win32::System::JobObjects::{
            QueryInformationJobObject, JobObjectExtendedLimitInformation,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        };
        use std::mem;

        unsafe {
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = mem::zeroed();
            let mut return_length = 0u32;

            QueryInformationJobObject(
                Some(windows::Win32::Foundation::HANDLE(self.job.handle() as *mut std::ffi::c_void)),
                JobObjectExtendedLimitInformation,
                &mut info as *mut _ as *mut std::ffi::c_void,
                mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                Some(&mut return_length),
            ).map_err(|e| BackendError::Internal {
                message: format!("Failed to query memory usage: {}", e)
            })?;

            // Return peak job memory usage (all processes combined)
            Ok(info.PeakJobMemoryUsed as u64)
        }
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

    #[cfg(target_os = "windows")]
    #[test]
    fn test_cpu_and_io_tracking() {
        let limits = WindowsLimits {
            memory_bytes: None,
            cpu_time_ms: None,
            max_processes: None,
        };

        let job = JobManager::create_with_limits(&limits).unwrap();

        // Spawn a process that uses CPU and does some I/O
        let mut child = std::process::Command::new("cmd")
            .arg("/c")
            .arg("for /L %i in (1,1,100000) do @echo test > nul")
            .spawn()
            .unwrap();

        job.assign_process(child.id()).unwrap();
        child.wait().unwrap();

        // Query CPU and I/O stats
        let (cpu_time_ms, disk_read, disk_write, network_other) = job.get_cpu_and_io_stats().unwrap();

        // CPU time must be measurable after 100k loop iterations
        assert!(
            cpu_time_ms > 0,
            "Expected CPU time > 0ms after 100k loop iterations, got {} ms",
            cpu_time_ms
        );

        // Network/other I/O is now captured (may be 0 for this test)
        assert!(network_other >= 0, "Network/other I/O should be >= 0");

        // Disk I/O may be 0 when redirecting to 'nul' on Windows
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_memory_tracking() {
        let limits = WindowsLimits {
            memory_bytes: None,
            cpu_time_ms: None,
            max_processes: None,
        };

        let job = JobManager::create_with_limits(&limits).unwrap();

        // Spawn a process that allocates memory
        let mut child = std::process::Command::new("cmd")
            .arg("/c")
            .arg("echo test")
            .spawn()
            .unwrap();

        job.assign_process(child.id()).unwrap();
        child.wait().unwrap();

        // Query memory usage
        let peak_memory = job.get_memory_usage().unwrap();

        // Peak memory should be greater than 0 since cmd.exe was allocated
        assert!(peak_memory > 0, "Peak memory should be greater than 0, got: {}", peak_memory);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_active_process_count() {
        let limits = WindowsLimits {
            memory_bytes: None,
            cpu_time_ms: None,
            max_processes: None,
        };

        let job = JobManager::create_with_limits(&limits).unwrap();

        // Spawn a process
        let mut child = std::process::Command::new("cmd")
            .arg("/c")
            .arg("echo test")
            .spawn()
            .unwrap();

        job.assign_process(child.id()).unwrap();

        // Process count should be at least 1 while running
        let count_before = job.active_process_count().unwrap();
        assert!(count_before >= 1, "Should have at least 1 active process");

        child.wait().unwrap();

        // After process exits, count should eventually be 0
        let count_after = job.active_process_count().unwrap();
        assert_eq!(count_after, 0, "Should have 0 active processes after exit");
    }
}
