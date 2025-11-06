// ============================================================================
// File: packages/cylo/src/backends/apple/tests.rs
// ----------------------------------------------------------------------------
// Tests for Apple containerization backend.
// ============================================================================

use std::time::Duration;

use crate::backends::{BackendConfig, ExecutionBackend};

use super::AppleBackend;

#[test]
fn backend_creation() {
    let config = BackendConfig::new("test_apple").with_timeout(Duration::from_secs(60));

    // Valid image should work
    let _result = AppleBackend::new("python:3.11".to_string(), config.clone());
    // Note: Will fail on non-macOS platforms, which is expected

    // Invalid image should fail
    let invalid_result = AppleBackend::new("invalid".to_string(), config);
    assert!(invalid_result.is_err());
}

#[test]
fn supported_languages() {
    let config = BackendConfig::new("test");
    if let Ok(backend) = AppleBackend::new("python:3.11".to_string(), config) {
        assert!(backend.supports_language("python"));
        assert!(backend.supports_language("javascript"));
        assert!(backend.supports_language("rust"));
        assert!(!backend.supports_language("cobol"));
    }
}
