// ============================================================================
// File: packages/cylo/src/backends/apple/validation.rs
// ----------------------------------------------------------------------------
// Platform and image validation for Apple containerization backend.
// ============================================================================

/// Check if platform supports Apple containerization
///
/// # Returns
/// true if running on macOS with Apple Silicon, false otherwise
pub(super) fn is_platform_supported() -> bool {
    #[cfg(target_os = "macos")]
    {
        // Check for Apple Silicon architecture
        std::env::consts::ARCH == "aarch64"
    }

    #[cfg(not(target_os = "macos"))]
    false
}

/// Validate container image format
///
/// # Arguments
/// * `image` - Image specification to validate
///
/// # Returns
/// true if format is valid, false otherwise
pub(super) fn is_valid_image_format(image: &str) -> bool {
    // Basic validation: must contain ':' for tag
    if !image.contains(':') {
        return false;
    }

    // Split into name and tag
    let parts: Vec<&str> = image.splitn(2, ':').collect();
    if parts.len() != 2 {
        return false;
    }

    let (name, tag) = (parts[0], parts[1]);

    // Name must not be empty and contain valid characters
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '/' || c == '-' || c == '_' || c == '.')
    {
        return false;
    }

    // Tag must not be empty and contain valid characters
    if tag.is_empty()
        || !tag
            .chars()
            .all(|c| c.is_alphanumeric() || c == '.' || c == '-' || c == '_')
    {
        return false;
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_format_validation() {
        assert!(is_valid_image_format("python:3.11"));
        assert!(is_valid_image_format("rust:alpine3.20"));
        assert!(is_valid_image_format("node:18-alpine"));
        assert!(is_valid_image_format("registry.io/user/image:tag"));

        assert!(!is_valid_image_format("python"));
        assert!(!is_valid_image_format(""));
        assert!(!is_valid_image_format(":tag"));
        assert!(!is_valid_image_format("image:"));
        assert!(!is_valid_image_format("image:tag:extra"));
    }
}
