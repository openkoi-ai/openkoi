// src/provider/model_cache.rs — Shared model list caching for provider probes
//
// Caches probed model lists to disk with a configurable TTL (default 1 hour).
// Each provider gets its own JSON file: ~/.openkoi/cache/{provider}-models.json

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use super::ModelInfo;
use crate::infra::paths;

/// Default cache TTL: 1 hour.
const CACHE_TTL: Duration = Duration::from_secs(3600);

/// Cached model list with metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedModels {
    /// When the cache was written (Unix timestamp seconds).
    cached_at: u64,
    /// The probed model list.
    models: Vec<ModelInfo>,
}

/// Return the cache file path for a given provider.
fn cache_path(provider_id: &str) -> PathBuf {
    paths::cache_dir().join(format!("{provider_id}-models.json"))
}

/// Try to load a cached model list. Returns `None` if the cache is missing,
/// corrupt, or expired.
pub fn load_cached(provider_id: &str) -> Option<Vec<ModelInfo>> {
    let path = cache_path(provider_id);
    let data = std::fs::read_to_string(&path).ok()?;
    let cached: CachedModels = serde_json::from_str(&data).ok()?;

    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    if now.saturating_sub(cached.cached_at) > CACHE_TTL.as_secs() {
        tracing::debug!(
            "Model cache for '{provider_id}' expired (age={}s)",
            now.saturating_sub(cached.cached_at)
        );
        return None;
    }

    if cached.models.is_empty() {
        return None;
    }

    tracing::debug!(
        "Loaded {} cached models for '{provider_id}'",
        cached.models.len()
    );
    Some(cached.models)
}

/// Save a probed model list to the cache file.
pub fn save_cache(provider_id: &str, models: &[ModelInfo]) {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let cached = CachedModels {
        cached_at: now,
        models: models.to_vec(),
    };

    let path = cache_path(provider_id);

    // Ensure parent dir exists (best-effort)
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    match serde_json::to_string_pretty(&cached) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&path, json) {
                tracing::warn!("Failed to write model cache for '{provider_id}': {e}");
            } else {
                tracing::debug!("Saved {} models to cache for '{provider_id}'", models.len());
            }
        }
        Err(e) => {
            tracing::warn!("Failed to serialize model cache for '{provider_id}': {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn test_model() -> ModelInfo {
        ModelInfo {
            id: "test-model".into(),
            name: "Test Model".into(),
            context_window: 128_000,
            max_output_tokens: 16_384,
            supports_tools: true,
            supports_streaming: true,
            input_price_per_mtok: 0.0,
            output_price_per_mtok: 0.0,
            ..Default::default()
        }
    }

    #[test]
    fn test_cache_roundtrip() {
        let provider_id = "test-roundtrip";
        let models = vec![test_model()];

        // Ensure cache dir exists
        let _ = fs::create_dir_all(paths::cache_dir());

        save_cache(provider_id, &models);
        let loaded = load_cached(provider_id);
        assert!(loaded.is_some());
        let loaded = loaded.unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "test-model");

        // Clean up
        let _ = fs::remove_file(cache_path(provider_id));
    }

    #[test]
    fn test_cache_empty_returns_none() {
        let provider_id = "test-empty";
        let _ = fs::create_dir_all(paths::cache_dir());

        save_cache(provider_id, &[]);
        let loaded = load_cached(provider_id);
        assert!(loaded.is_none());

        // Clean up
        let _ = fs::remove_file(cache_path(provider_id));
    }

    #[test]
    fn test_cache_missing_returns_none() {
        let loaded = load_cached("nonexistent-provider-xyz");
        assert!(loaded.is_none());
    }

    #[test]
    fn test_cache_expired() {
        let provider_id = "test-expired";
        let _ = fs::create_dir_all(paths::cache_dir());

        // Write a cache entry with a timestamp from 2 hours ago
        let two_hours_ago = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - 7200;

        let cached = CachedModels {
            cached_at: two_hours_ago,
            models: vec![test_model()],
        };

        let path = cache_path(provider_id);
        let json = serde_json::to_string_pretty(&cached).unwrap();
        fs::write(&path, json).unwrap();

        let loaded = load_cached(provider_id);
        assert!(loaded.is_none());

        // Clean up
        let _ = fs::remove_file(path);
    }
}
