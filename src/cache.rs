use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::build::{build_snapshot, compute_source_fingerprint};
use crate::route_planner::{PlannerCache, build_planner_cache};
use crate::snapshot::{Snapshot, SourceFingerprint};

pub const APP_CACHE_VERSION: u32 = 1;
const APP_CACHE_DECODE_LIMIT_BYTES: u64 = 1024 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppCache {
    pub version: u32,
    pub built_unix_secs: u64,
    pub fingerprint: SourceFingerprint,
    pub snapshot: Snapshot,
    pub planner: PlannerCache,
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn save_app_cache(path: &str, cache: &AppCache) -> Result<(), String> {
    let file =
        File::create(path).map_err(|err| format!("Failed to create cache file '{path}': {err}"))?;
    let mut writer = BufWriter::new(file);
    bincode::serialize_into(&mut writer, cache)
        .map_err(|err| format!("Failed to serialize cache '{path}': {err}"))
}

pub fn load_app_cache(path: &str) -> Result<AppCache, String> {
    let file_size = std::fs::metadata(Path::new(path))
        .map(|m| m.len())
        .map_err(|err| format!("Failed to read cache metadata '{path}': {err}"))?;
    if file_size > APP_CACHE_DECODE_LIMIT_BYTES {
        return Err(format!(
            "Cache file '{path}' is too large to load safely ({} bytes > {} bytes)",
            file_size, APP_CACHE_DECODE_LIMIT_BYTES
        ));
    }

    let file =
        File::open(path).map_err(|err| format!("Failed to open cache file '{path}': {err}"))?;
    let mut reader = BufReader::new(file);
    bincode::deserialize_from(&mut reader)
        .map_err(|err| format!("Failed to deserialize cache '{path}': {err}"))
}

pub fn build_app_cache(source_path: &str) -> Result<AppCache, String> {
    let fingerprint = compute_source_fingerprint(source_path)?;
    let snapshot = build_snapshot(source_path)?;
    let planner = build_planner_cache(source_path)?;

    Ok(AppCache {
        version: APP_CACHE_VERSION,
        built_unix_secs: now_unix_secs(),
        fingerprint,
        snapshot,
        planner,
    })
}

pub fn app_cache_is_fresh(cache: &AppCache, source_path: &str) -> Result<bool, String> {
    if cache.version != APP_CACHE_VERSION {
        return Ok(false);
    }

    let current = compute_source_fingerprint(source_path)?;
    Ok(cache.fingerprint == current)
}

pub fn rebuild_app_cache(source_path: &str, cache_path: &str) -> Result<AppCache, String> {
    let cache = build_app_cache(source_path)?;
    save_app_cache(cache_path, &cache)?;
    Ok(cache)
}

pub fn load_or_build_app_cache(source_path: &str, cache_path: &str) -> Result<AppCache, String> {
    if let Ok(cache) = load_app_cache(cache_path) {
        if app_cache_is_fresh(&cache, source_path)? {
            return Ok(cache);
        }
    }

    rebuild_app_cache(source_path, cache_path)
}
