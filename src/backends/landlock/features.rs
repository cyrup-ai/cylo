// ============================================================================
// File: packages/cylo/src/backends/landlock/features.rs
// ----------------------------------------------------------------------------
// LandLock feature detection and platform support.
//
// Provides feature detection for LandLock Linux security module including:
// - Platform availability checks
// - ABI version detection
// - Supported filesystem access types
// - Feature caching with timestamps
// ============================================================================

use std::path::Path;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::backends::{BackendError, BackendResult};

/// LandLock feature detection and capabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LandLockFeatures {
    /// Whether LandLock is available on this system
    pub available: bool,

    /// Supported LandLock ABI version
    pub abi_version: u32,

    /// Supported rule types
    pub supported_access_fs: u64,

    /// Feature detection timestamp
    pub detected_at: SystemTime,
}

impl LandLockFeatures {
    /// Detect LandLock features and capabilities
    ///
    /// # Returns
    /// LandLock feature detection result
    pub fn detect() -> BackendResult<Self> {
        #[cfg(target_os = "linux")]
        {
            use std::fs::File;
            use std::io::Read;

            // Check if LandLock is available
            let landlock_dir = Path::new("/sys/kernel/security/landlock");
            if !landlock_dir.exists() {
                return Ok(Self {
                    available: false,
                    abi_version: 0,
                    supported_access_fs: 0,
                    detected_at: SystemTime::now(),
                });
            }

            // Read ABI version
            let abi_version = match File::open(landlock_dir.join("version")) {
                Ok(mut file) => {
                    let mut content = String::new();
                    file.read_to_string(&mut content).unwrap_or_default();
                    content.trim().parse().unwrap_or(0)
                }
                Err(_) => 0,
            };

            // Read supported filesystem access types
            let supported_access_fs = match File::open(landlock_dir.join("access_fs")) {
                Ok(mut file) => {
                    let mut content = String::new();
                    file.read_to_string(&mut content).unwrap_or_default();
                    u64::from_str_radix(content.trim().trim_start_matches("0x"), 16).unwrap_or(0)
                }
                Err(_) => 0,
            };

            Ok(Self {
                available: abi_version > 0,
                abi_version,
                supported_access_fs,
                detected_at: SystemTime::now(),
            })
        }

        #[cfg(not(target_os = "linux"))]
        Ok(Self {
            available: false,
            abi_version: 0,
            supported_access_fs: 0,
            detected_at: SystemTime::now(),
        })
    }
}

/// Platform support checks for LandLock
pub struct PlatformSupport;

impl PlatformSupport {
    /// Check if platform supports LandLock
    ///
    /// # Returns
    /// true if running on Linux with LandLock support, false otherwise
    pub fn is_supported() -> bool {
        #[cfg(target_os = "linux")]
        {
            // Check for LandLock support in kernel
            Path::new("/sys/kernel/security/landlock").exists()
        }

        #[cfg(not(target_os = "linux"))]
        false
    }

    /// Validate platform or return error
    ///
    /// # Returns
    /// Ok(()) if platform is supported, Err otherwise
    pub fn validate() -> BackendResult<()> {
        if !Self::is_supported() {
            return Err(BackendError::NotAvailable {
                backend: "LandLock",
                reason: "LandLock is only available on Linux".to_string(),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feature_detection() {
        let features = LandLockFeatures::detect();
        assert!(features.is_ok());

        let features = features.expect("test should successfully detect landlock features");
        #[cfg(target_os = "linux")]
        {
            // On Linux, should at least attempt detection
            assert!(features.detected_at <= SystemTime::now());
        }

        #[cfg(not(target_os = "linux"))]
        {
            assert!(!features.available);
            assert_eq!(features.abi_version, 0);
        }
    }

    #[test]
    fn platform_support() {
        #[cfg(target_os = "linux")]
        {
            // On Linux, should check for LandLock directory
            let supported = PlatformSupport::is_supported();
            assert_eq!(
                supported,
                Path::new("/sys/kernel/security/landlock").exists()
            );
        }

        #[cfg(not(target_os = "linux"))]
        {
            assert!(!PlatformSupport::is_supported());
        }
    }
}
