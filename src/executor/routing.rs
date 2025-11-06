//! ============================================================================
//! File: packages/cylo/src/executor/routing.rs
//! ----------------------------------------------------------------------------
//! Backend selection and routing logic for optimal execution placement.
//! ============================================================================

use std::sync::{Arc, RwLock};
use crate::execution_env::{Cylo, CyloError, CyloResult};
use crate::backends::ExecutionRequest;
use super::types::{RoutingStrategy, BackendPreferences, PlatformCache};

/// Select optimal backend based on strategy and requirements
pub fn select_optimal_backend(
    strategy: &RoutingStrategy,
    preferences: &BackendPreferences,
    platform_cache: &Arc<RwLock<PlatformCache>>,
    _request: &ExecutionRequest,
) -> CyloResult<String> {
    let cache = platform_cache
        .read()
        .map_err(|e| CyloError::Other(format!("Cache lock poisoned: {}", e)))?;
    let available = &cache.available_backends;

    if available.is_empty() {
        return Err(CyloError::no_backend_available());
    }

    match strategy {
        RoutingStrategy::Performance => {
            // Select backend with highest performance rating
            let best = available
                .iter()
                .filter(|(name, _)| !preferences.excluded_backends.contains(name))
                .max_by_key(|(_, rating)| *rating)
                .ok_or_else(|| CyloError::no_backend_available())?;
            Ok(best.0.clone())
        }

        RoutingStrategy::Security => {
            // Prefer FireCracker > LandLock > Apple for security
            let security_order = ["FireCracker", "LandLock", "Apple"];
            for backend in &security_order {
                if available.iter().any(|(name, _)| name == backend)
                    && !preferences
                        .excluded_backends
                        .contains(&backend.to_string())
                {
                    return Ok(backend.to_string());
                }
            }
            Err(CyloError::no_backend_available())
        }

        RoutingStrategy::Balanced => {
            // Weight performance with security considerations
            let mut weighted_scores: Vec<(String, f32)> = available
                .iter()
                .filter(|(name, _)| !preferences.excluded_backends.contains(name))
                .map(|(name, rating)| {
                    let base_score = *rating as f32;
                    let security_bonus = match name.as_str() {
                        "FireCracker" => 20.0,
                        "LandLock" => 15.0,
                        "Apple" => 10.0,
                        _ => 0.0,
                    };
                    let preference_multiplier = preferences
                        .weight_multipliers
                        .get(name)
                        .copied()
                        .unwrap_or(1.0);

                    let total_score = (base_score + security_bonus) * preference_multiplier;
                    (name.clone(), total_score)
                })
                .collect();

            weighted_scores.sort_by(|a, b| {
                b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal)
            });

            weighted_scores
                .first()
                .map(|(name, _)| name.clone())
                .ok_or_else(|| CyloError::no_backend_available())
        }

        RoutingStrategy::PreferBackend(preferred) => {
            // Use preferred backend if available, otherwise balanced
            if available.iter().any(|(name, _)| name == preferred)
                && !preferences.excluded_backends.contains(preferred)
            {
                Ok(preferred.clone())
            } else {
                select_optimal_backend(
                    &RoutingStrategy::Balanced,
                    preferences,
                    platform_cache,
                    _request,
                )
            }
        }

        RoutingStrategy::ExplicitOnly => Err(CyloError::invalid_configuration(
            "ExplicitOnly strategy requires instance_hint",
        )),
    }
}

/// Create Cylo environment for backend
pub fn create_cylo_env(backend_name: &str, request: &ExecutionRequest) -> CyloResult<Cylo> {
    match backend_name {
        "Apple" => {
            let image = select_image_for_language(&request.language);
            Ok(Cylo::Apple(image))
        }
        "LandLock" => Ok(Cylo::LandLock("/tmp/cylo_landlock".to_string())),
        "FireCracker" => {
            let image = select_image_for_language(&request.language);
            Ok(Cylo::FireCracker(image))
        }
        _ => Err(CyloError::unsupported_backend(backend_name)),
    }
}

/// Select appropriate container image for programming language
pub fn select_image_for_language(language: &str) -> String {
    match language.to_lowercase().as_str() {
        "python" | "python3" => "python:3.11-alpine".to_string(),
        "javascript" | "js" | "node" => "node:18-alpine".to_string(),
        "rust" => "rust:1.75-alpine".to_string(),
        "go" => "golang:1.21-alpine".to_string(),
        _ => "alpine:3.18".to_string(), // Default for bash/sh
    }
}

/// Get backend name from Cylo environment
pub fn backend_name_from_cylo(cylo: &Cylo) -> String {
    match cylo {
        Cylo::Apple(_) => "Apple".to_string(),
        Cylo::LandLock(_) => "LandLock".to_string(),
        Cylo::FireCracker(_) => "FireCracker".to_string(),
    }
}

/// Generate unique instance name
pub fn generate_instance_name(backend_name: &str) -> String {
    format!(
        "{}_{}",
        backend_name.to_lowercase(),
        uuid::Uuid::new_v4().simple()
    )
}

/// Compute platform capabilities hash for cache invalidation
pub fn compute_capabilities_hash(platform_info: &crate::platform::PlatformInfo) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    platform_info.available_backends.len().hash(&mut hasher);
    for backend in &platform_info.available_backends {
        backend.name.hash(&mut hasher);
        backend.available.hash(&mut hasher);
        backend.performance_rating.hash(&mut hasher);
    }
    hasher.finish()
}
