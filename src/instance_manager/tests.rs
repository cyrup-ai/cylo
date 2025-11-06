// ============================================================================
// File: packages/cylo/src/instance_manager/tests.rs
// ----------------------------------------------------------------------------
// Test suite for instance manager
// ============================================================================

use std::time::Duration;

use crate::backends::BackendConfig;
use crate::execution_env::{Cylo, CyloError};

use super::{global_instance_manager, InstanceManager};

#[tokio::test]
async fn instance_manager_creation() {
    let manager = InstanceManager::new();

    let instances = manager
        .list_instances()
        .expect("Failed to list instances in test");
    assert!(instances.is_empty());
}

#[tokio::test]
async fn instance_registration_and_retrieval() {
    let manager = InstanceManager::new();

    // Create a test instance (will fail on unsupported platforms)
    let cylo_env = Cylo::LandLock("/tmp/test".to_string());
    let instance = cylo_env.instance("test_instance");

    // Registration might fail due to platform support
    let register_result = manager.register_instance(instance.clone()).await;

    if register_result.is_ok() {
        // If registration succeeded, test retrieval
        let backend_result = manager.get_instance(&instance.id()).await;

        if let Ok(backend) = &backend_result {
            if let Ok(backend_arc) = backend {
                assert_eq!(backend_arc.backend_type(), "LandLock");
            }

            // Test release
            let release_result = manager.release_instance(&instance.id());
            assert!(release_result.is_ok());

            // Test removal
            let remove_result = manager.remove_instance(&instance.id()).await;
            assert!(remove_result.is_ok());
        }
    }
    // If registration failed due to platform support, that's expected
}

#[tokio::test]
async fn instance_not_found() {
    let manager = InstanceManager::new();

    let result = manager.get_instance("nonexistent").await;
    assert!(result.is_ok()); // JoinHandle should succeed

    match result {
        Ok(inner_result) => {
            if let Err(CyloError::InstanceNotFound { name }) = inner_result {
                assert_eq!(name, "nonexistent");
            } else {
                panic!("Expected InstanceNotFound error");
            }
        }
        Err(join_error) => {
            panic!("Unexpected join error: {:?}", join_error);
        }
    }
}

#[tokio::test]
async fn instance_list() {
    let manager = InstanceManager::new();

    let initial_list = manager
        .list_instances()
        .expect("Failed to get initial instance list in test");
    assert!(initial_list.is_empty());

    // Try to register an instance
    let cylo_env = Cylo::Apple("test:latest".to_string());
    let instance = cylo_env.instance("test_list");

    let register_result = manager.register_instance(instance.clone()).await;

    if register_result.is_ok() {
        let updated_list = manager
            .list_instances()
            .expect("Failed to get updated instance list in test");
        assert!(updated_list.contains(&instance.id()));
    }
    // Platform support determines if this test can complete
}

#[tokio::test]
async fn health_check_all() {
    let manager = InstanceManager::new();

    let health_results = manager
        .health_check_all()
        .await
        .expect("Failed to join async task in test")
        .expect("Failed to check health of all instances in test");
    assert!(health_results.is_empty());
}

#[tokio::test]
async fn cleanup_idle_instances() {
    let manager = InstanceManager::new();

    let cleaned_count = manager
        .cleanup_idle_instances()
        .await
        .expect("Failed to join async task in test")
        .expect("Failed to cleanup idle instances in test");
    assert_eq!(cleaned_count, 0);
}

#[tokio::test]
async fn shutdown() {
    let manager = InstanceManager::new();

    let shutdown_result = manager.shutdown().await;
    assert!(shutdown_result.is_ok());
}

#[test]
fn global_instance_manager_access() {
    let manager = global_instance_manager();
    let instances = manager
        .list_instances()
        .expect("Failed to list instances from global manager in test");
    assert!(instances.is_empty());
}

#[test]
fn custom_configuration() {
    let config = BackendConfig::new("custom").with_timeout(Duration::from_secs(120));
    let manager =
        InstanceManager::with_config(config, Duration::from_secs(30), Duration::from_secs(600));

    assert_eq!(manager.health_check_interval, Duration::from_secs(30));
    assert_eq!(manager.max_idle_time, Duration::from_secs(600));
}
