// ============================================================================
// File: packages/cylo/src/platform_utils.rs
// ----------------------------------------------------------------------------
// Cross-platform utilities for file operations
// ============================================================================

use std::path::Path;
use std::io;
#[cfg(unix)]
use std::fs;

/// Set executable permissions on a file
///
/// On Unix systems, sets mode to 0o755 (rwxr-xr-x)
/// On Windows, this is a no-op since .exe files are automatically executable
///
/// # Arguments
/// * `path` - Path to the file to make executable
///
/// # Returns
/// Ok(()) if successful, io::Error otherwise
#[cfg(unix)]
pub fn set_executable(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
}

#[cfg(not(unix))]
pub fn set_executable(_path: &Path) -> io::Result<()> {
    // Windows .exe files are automatically executable
    // No permissions need to be set
    Ok(())
}
