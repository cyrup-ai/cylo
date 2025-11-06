// ============================================================================
// File: packages/cylo/src/platform/ramdisk.rs
// ----------------------------------------------------------------------------
// Platform-specific ramdisk operations trait for Cylo.
//
// Defines the interface for platform-specific ramdisk management.
// Implementations provide platform-optimized methods for creating,
// mounting, and managing temporary in-memory filesystems.
// ============================================================================

use std::path::Path;

use crate::error::StorageError;

/// Platform-specific ramdisk operations trait
///
/// Defines the interface for platform-specific ramdisk management.
/// Implementations provide platform-optimized methods for creating,
/// mounting, and managing temporary in-memory filesystems.
pub trait RamdiskPlatform {
    /// Create a new platform-specific ramdisk implementation
    fn new() -> Self;

    /// Check if a ramdisk is mounted at the given path
    ///
    /// # Arguments
    /// * `mount_point` - Path to check for ramdisk mount
    ///
    /// # Returns
    /// True if a ramdisk is mounted at the path, false otherwise
    fn is_mounted(&self, mount_point: &Path) -> Result<bool, StorageError>;

    /// Create a ramdisk with the specified configuration
    ///
    /// # Arguments
    /// * `config` - Ramdisk configuration
    ///
    /// # Returns
    /// Success or storage error
    fn create(&mut self, config: &crate::config::RamdiskConfig) -> Result<(), StorageError>;

    /// Remove a ramdisk at the specified mount point
    ///
    /// # Arguments
    /// * `mount_point` - Path to the ramdisk mount point
    ///
    /// # Returns
    /// Success or storage error
    fn remove(&self, mount_point: &Path) -> Result<(), StorageError>;
}
