// ============================================================================
// File: packages/cylo/src/backends/apple/image.rs
// ----------------------------------------------------------------------------
// Container image management for Apple containerization backend.
// ============================================================================

use std::process::{Command, Stdio};

use crate::AsyncTaskBuilder;
use crate::backends::{AsyncTask, BackendError, BackendResult};

/// Check if Apple containerization CLI is available
///
/// # Returns
/// AsyncTask that resolves to availability status
pub(super) fn check_cli_availability() -> AsyncTask<bool> {
    AsyncTaskBuilder::new(async move {
        let result = Command::new("container")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();

        match result {
            Ok(status) => status.success(),
            Err(_) => false,
        }
    })
    .spawn()
}

/// Pull container image if not already available
///
/// # Arguments
/// * `image` - Image to pull
///
/// # Returns
/// AsyncTask that resolves when image is available
pub(super) fn ensure_image_available(image: String) -> AsyncTask<BackendResult<()>> {
    AsyncTaskBuilder::new(async move {
        // Check if image exists locally first
        let check_result = Command::new("container")
            .args(["image", "exists", &image])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();

        match check_result {
            Ok(status) if status.success() => {
                // Image exists locally
                return Ok(());
            }
            _ => {
                // Need to pull image
            }
        }

        // Pull the image
        let pull_result = Command::new("container")
            .args(["pull", &image])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output();

        match pull_result {
            Ok(output) if output.status.success() => Ok(()),
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                Err(BackendError::ContainerFailed {
                    details: format!("Failed to pull image {image}: {stderr}"),
                })
            }
            Err(e) => Err(BackendError::ContainerFailed {
                details: format!("Failed to execute container pull: {e}"),
            }),
        }
    })
    .spawn()
}
