// ============================================================================
// File: packages/cylo/src/backends/windows/limits.rs
// ----------------------------------------------------------------------------
// Resource limit configuration helpers for Windows Job Objects
// ============================================================================

use crate::backends::{BackendError, BackendResult, ResourceLimits};

/// Convert resource limits to Windows Job Object limit values
///
/// Takes the generic ResourceLimits configuration and converts it to
/// Windows-specific limit values that can be applied to Job Objects.
#[derive(Debug, Clone)]
pub struct WindowsLimits {
    /// Memory limit in bytes (working set)
    pub memory_bytes: Option<u64>,
    
    /// CPU time limit in milliseconds
    pub cpu_time_ms: Option<u64>,
    
    /// Maximum number of processes in the job
    pub max_processes: Option<u32>,
}

impl WindowsLimits {
    /// Create Windows limits from generic resource limits
    ///
    /// # Arguments
    /// * `limits` - Generic resource limits configuration
    ///
    /// # Returns
    /// WindowsLimits instance with converted values
    pub fn from_resource_limits(limits: &ResourceLimits) -> BackendResult<Self> {
        let memory_bytes = if let Some(mem_mb) = limits.memory_mb {
            if mem_mb == 0 {
                return Err(BackendError::Configuration(
                    "Memory limit must be greater than 0".to_string(),
                ));
            }
            Some(mem_mb as u64 * 1024 * 1024)
        } else {
            None
        };

        let cpu_time_ms = if let Some(cpu_secs) = limits.cpu_time_secs {
            if cpu_secs == 0 {
                return Err(BackendError::Configuration(
                    "CPU time limit must be greater than 0".to_string(),
                ));
            }
            Some(cpu_secs as u64 * 1000)
        } else {
            None
        };

        let max_processes = limits.max_processes;

        Ok(Self {
            memory_bytes,
            cpu_time_ms,
            max_processes,
        })
    }

    /// Get the process memory limit (working set minimum and maximum)
    ///
    /// Returns (min_bytes, max_bytes) tuple for Job Object configuration
    /// Min is set to 50% of max to allow some flexibility
    pub fn process_memory_limit(&self) -> Option<(usize, usize)> {
        self.memory_bytes.map(|max| {
            let max_bytes = max as usize;
            let min_bytes = max_bytes / 2;
            (min_bytes, max_bytes)
        })
    }

    /// Check if any limits are configured
    pub fn has_limits(&self) -> bool {
        self.memory_bytes.is_some() 
            || self.cpu_time_ms.is_some() 
            || self.max_processes.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_conversion() {
        let mut limits = ResourceLimits::default();
        limits.memory_mb = Some(256);
        
        let windows_limits = WindowsLimits::from_resource_limits(&limits);
        assert!(windows_limits.is_ok());
        
        if let Ok(wl) = windows_limits {
            assert_eq!(wl.memory_bytes, Some(256 * 1024 * 1024));
            if let Some((min, max)) = wl.process_memory_limit() {
                assert_eq!(max, 256 * 1024 * 1024);
                assert_eq!(min, 128 * 1024 * 1024);
            }
        }
    }

    #[test]
    fn test_cpu_time_conversion() {
        let mut limits = ResourceLimits::default();
        limits.cpu_time_secs = Some(30);
        
        let windows_limits = WindowsLimits::from_resource_limits(&limits);
        assert!(windows_limits.is_ok());
        
        if let Ok(wl) = windows_limits {
            assert_eq!(wl.cpu_time_ms, Some(30_000));
        }
    }

    #[test]
    fn test_zero_limits_rejected() {
        let mut limits = ResourceLimits::default();
        limits.memory_mb = Some(0);
        
        let result = WindowsLimits::from_resource_limits(&limits);
        assert!(result.is_err());
    }

    #[test]
    fn test_has_limits() {
        let mut limits = ResourceLimits::default();
        let wl = WindowsLimits::from_resource_limits(&limits);
        if let Ok(wl) = wl {
            assert!(!wl.has_limits());
        }

        limits.memory_mb = Some(128);
        let wl = WindowsLimits::from_resource_limits(&limits);
        if let Ok(wl) = wl {
            assert!(wl.has_limits());
        }
    }
}
