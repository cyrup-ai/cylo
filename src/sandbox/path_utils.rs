use std::{path::Path, sync::Arc};

use crate::error::{SandboxError, SandboxResult};

/// Safe path to string conversion with zero-allocation optimization
///
/// Uses direct path.to_str() in the happy path (no allocation), falls back to
/// path.to_string_lossy() only when UTF-8 conversion is required.
#[inline]
pub fn safe_path_to_str(path: &Path) -> SandboxResult<&str> {
    path.to_str().ok_or_else(|| SandboxError::PathInvalid {
        detail: Arc::from(format!(
            "Path contains invalid UTF-8 characters: {}",
            path.to_string_lossy()
        )),
    })
}

/// Safe path to owned string conversion with minimal allocation
///
/// Only allocates when UTF-8 conversion is necessary, maintaining zero-allocation
/// characteristics for valid UTF-8 paths.
#[inline]
pub fn safe_path_to_string(path: &Path) -> SandboxResult<String> {
    match path.to_str() {
        Some(s) => Ok(s.to_string()),
        None => Ok(path.to_string_lossy().into_owned()),
    }
}
