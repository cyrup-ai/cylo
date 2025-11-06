// ============================================================================
// File: packages/cylo/src/platform/performance.rs
// ----------------------------------------------------------------------------
// Performance detection for Cylo execution environments.
//
// Provides functions to detect system performance characteristics:
// - CPU core count
// - Available memory
// - Temporary directory performance
// - I/O characteristics (disk type, throughput, IOPS)
// ============================================================================

use super::types::*;

/// Detect performance hints for the current system
pub(crate) fn detect_performance_hints() -> PerformanceHints {
    PerformanceHints {
        cpu_cores: detect_cpu_cores(),
        available_memory: detect_available_memory(),
        recommended_backend: None, // Logic to determine this would be complex
        tmpdir_performance: detect_tmpdir_performance(),
        io_characteristics: detect_io_characteristics(),
    }
}

/// Detect number of CPU cores
pub(crate) fn detect_cpu_cores() -> u32 {
    num_cpus::get() as u32
}

/// Detect available memory in bytes
pub(crate) fn detect_available_memory() -> u64 {
    #[cfg(target_os = "linux")]
    {
        use std::fs;

        if let Ok(content) = fs::read_to_string("/proc/meminfo") {
            for line in content.lines() {
                if line.starts_with("MemAvailable:") {
                    if let Some(kb_str) = line.split_whitespace().nth(1) {
                        if let Ok(kb) = kb_str.parse::<u64>() {
                            return kb * 1024; // Convert to bytes
                        }
                    }
                }
            }
        }
    }

    // Fallback: return a reasonable default
    4 * 1024 * 1024 * 1024 // 4GB
}

/// Detect temporary directory performance characteristics
pub(crate) fn detect_tmpdir_performance() -> TmpDirPerformance {
    let tmp_path = std::env::temp_dir();
    let path = tmp_path.display().to_string();

    // Check if it's likely in-memory
    let in_memory = path.contains("/tmp");

    let estimated_throughput = if in_memory {
        5000 // 5GB/s for RAM
    } else {
        500 // 500MB/s for SSD
    };

    TmpDirPerformance {
        path,
        in_memory,
        estimated_throughput,
    }
}

/// Detect I/O characteristics
pub(crate) fn detect_io_characteristics() -> IOCharacteristics {
    // This is a simplified implementation
    // Real implementation would benchmark I/O performance
    IOCharacteristics {
        disk_type: "SSD".to_string(),
        sequential_read_mbps: 500,
        sequential_write_mbps: 400,
        random_iops: 50000,
    }
}
