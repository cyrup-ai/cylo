// ============================================================================
// File: packages/cylo/src/backends/landlock/monitoring.rs
// ----------------------------------------------------------------------------
// Resource monitoring for sandboxed processes.
//
// Provides Linux /proc filesystem monitoring including:
// - CPU time tracking (user + kernel mode)
// - Memory usage (RSS) tracking
// - Disk I/O statistics (read/write bytes)
// - Process tree counting (threads + children)
// ============================================================================

/// Get CPU time consumed by a process from /proc/[pid]/stat
///
/// # Arguments
/// * `pid` - Process ID to query
///
/// # Returns
/// Total CPU time in milliseconds (user + kernel mode) or error
#[cfg(target_os = "linux")]
pub fn get_process_cpu_time(pid: u32) -> Result<u64, std::io::Error> {
    let stat_path = format!("/proc/{}/stat", pid);
    let stat_content = std::fs::read_to_string(&stat_path)?;

    // Parse stat file (space-separated fields)
    let fields: Vec<&str> = stat_content.split_whitespace().collect();
    if fields.len() < 15 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Invalid stat format",
        ));
    }

    // Field 13 = utime (user mode jiffies)
    // Field 14 = stime (kernel mode jiffies)
    let utime: u64 = fields[13]
        .parse()
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid utime"))?;
    let stime: u64 = fields[14]
        .parse()
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid stime"))?;

    // Convert clock ticks to milliseconds
    let clock_ticks_per_sec = unsafe { libc::sysconf(libc::_SC_CLK_TCK) } as u64;
    let total_ticks = utime + stime;
    let cpu_time_ms = (total_ticks * 1000) / clock_ticks_per_sec;

    Ok(cpu_time_ms)
}

#[cfg(not(target_os = "linux"))]
pub fn get_process_cpu_time(_pid: u32) -> Result<u64, std::io::Error> {
    Ok(0)
}

/// Count process tree including threads and child processes
///
/// # Arguments
/// * `pid` - Root process ID
///
/// # Returns
/// Total count of processes and threads or error
#[cfg(target_os = "linux")]
pub fn count_process_tree(pid: u32) -> Result<usize, std::io::Error> {
    // Count the main process
    let mut count = 1;

    // Count threads via /proc/[pid]/task directory
    let task_dir = format!("/proc/{}/task", pid);
    if let Ok(entries) = std::fs::read_dir(&task_dir) {
        let thread_count = entries.count();
        if thread_count > 0 {
            count += thread_count - 1; // Don't double-count main thread
        }
    }

    // Find child processes from /proc/[pid]/task/[tid]/children
    let children_path = format!("/proc/{}/task/{}/children", pid, pid);
    if let Ok(children_content) = std::fs::read_to_string(&children_path) {
        for child_pid_str in children_content.split_whitespace() {
            if let Ok(child_pid) = child_pid_str.parse::<u32>() {
                // Recursively count child's subtree
                if let Ok(child_count) = count_process_tree(child_pid) {
                    count += child_count;
                }
            }
        }
    }

    Ok(count)
}

#[cfg(not(target_os = "linux"))]
pub fn count_process_tree(_pid: u32) -> Result<usize, std::io::Error> {
    Ok(1)
}

/// Get disk write statistics from /proc/[pid]/io
///
/// # Arguments
/// * `pid` - Process ID to query
///
/// # Returns
/// Total bytes written to disk or error
#[cfg(target_os = "linux")]
pub fn get_disk_io_stats(pid: u32) -> Result<u64, std::io::Error> {
    let io_path = format!("/proc/{}/io", pid);
    let io_content = std::fs::read_to_string(&io_path)?;

    // Parse for write_bytes line
    for line in io_content.lines() {
        if line.starts_with("write_bytes:") {
            if let Some(bytes_str) = line.split_whitespace().nth(1) {
                if let Ok(bytes) = bytes_str.parse::<u64>() {
                    return Ok(bytes);
                }
            }
        }
    }

    Ok(0)
}

#[cfg(not(target_os = "linux"))]
pub fn get_disk_io_stats(_pid: u32) -> Result<u64, std::io::Error> {
    Ok(0)
}

/// Get disk read statistics from /proc/[pid]/io
///
/// # Arguments
/// * `pid` - Process ID to query
///
/// # Returns
/// Total bytes read from disk or error
#[cfg(target_os = "linux")]
pub fn get_disk_read_stats(pid: u32) -> Result<u64, std::io::Error> {
    let io_path = format!("/proc/{}/io", pid);
    let io_content = std::fs::read_to_string(&io_path)?;

    // Parse for read_bytes line
    for line in io_content.lines() {
        if line.starts_with("read_bytes:") {
            if let Some(bytes_str) = line.split_whitespace().nth(1) {
                if let Ok(bytes) = bytes_str.parse::<u64>() {
                    return Ok(bytes);
                }
            }
        }
    }

    Ok(0)
}

#[cfg(not(target_os = "linux"))]
pub fn get_disk_read_stats(_pid: u32) -> Result<u64, std::io::Error> {
    Ok(0)
}

/// Get memory usage from /proc/[pid]/status
///
/// # Arguments
/// * `pid` - Process ID to query
///
/// # Returns
/// Resident Set Size (RSS) in bytes or error
#[cfg(target_os = "linux")]
pub fn get_memory_usage(pid: u32) -> Result<u64, std::io::Error> {
    let status_path = format!("/proc/{}/status", pid);
    let status_content = std::fs::read_to_string(&status_path)?;

    // Parse for VmRSS (Resident Set Size)
    for line in status_content.lines() {
        if line.starts_with("VmRSS:") {
            if let Some(kb_str) = line.split_whitespace().nth(1) {
                if let Ok(rss_kb) = kb_str.parse::<u64>() {
                    // Convert kilobytes to bytes
                    return Ok(rss_kb * 1024);
                }
            }
        }
    }

    Ok(0)
}

#[cfg(not(target_os = "linux"))]
pub fn get_memory_usage(_pid: u32) -> Result<u64, std::io::Error> {
    Ok(0)
}
