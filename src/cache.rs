use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::Path;

use crate::build::{build_snapshot, compute_source_fingerprint};
use crate::snapshot::{SNAPSHOT_VERSION, Snapshot};

const SNAPSHOT_DECODE_LIMIT_BYTES: u64 = 256 * 1024 * 1024;

pub fn save_snapshot(path: &str, snapshot: &Snapshot) -> Result<(), String> {
    let file =
        File::create(path).map_err(|err| format!("Failed to create cache file '{path}': {err}"))?;
    let mut writer = BufWriter::new(file);
    bincode::serialize_into(&mut writer, snapshot)
        .map_err(|err| format!("Failed to serialize cache '{path}': {err}"))
}

pub fn load_snapshot(path: &str) -> Result<Snapshot, String> {
    let file_size = std::fs::metadata(Path::new(path))
        .map(|m| m.len())
        .map_err(|err| format!("Failed to read cache metadata '{path}': {err}"))?;
    if file_size > SNAPSHOT_DECODE_LIMIT_BYTES {
        return Err(format!(
            "Cache file '{path}' is too large to load safely ({} bytes > {} bytes)",
            file_size, SNAPSHOT_DECODE_LIMIT_BYTES
        ));
    }

    let file =
        File::open(path).map_err(|err| format!("Failed to open cache file '{path}': {err}"))?;
    let mut reader = BufReader::new(file);
    bincode::deserialize_from(&mut reader)
        .map_err(|err| format!("Failed to deserialize cache '{path}': {err}"))
}

pub fn cache_is_fresh(snapshot: &Snapshot, source_path: &str) -> Result<bool, String> {
    if snapshot.version != SNAPSHOT_VERSION {
        if std::env::var("OEFFI_DEBUG_CACHE").is_ok() {
            eprintln!(
                "cache stale: version mismatch (cache={}, code={})",
                snapshot.version, SNAPSHOT_VERSION
            );
        }
        return Ok(false);
    }

    let current = compute_source_fingerprint(source_path)?;
    if snapshot.fingerprint != current {
        if std::env::var("OEFFI_DEBUG_CACHE").is_ok() {
            eprintln!(
                "cache stale: fingerprint mismatch\n  cache: {:?}\n  curr:  {:?}",
                snapshot.fingerprint, current
            );
        }
        return Ok(false);
    }

    Ok(true)
}

pub fn load_or_build_snapshot(source_path: &str, cache_path: &str) -> Result<Snapshot, String> {
    if let Ok(snapshot) = load_snapshot(cache_path) {
        if cache_is_fresh(&snapshot, source_path)? {
            return Ok(snapshot);
        }
    }

    let snapshot = build_snapshot(source_path)?;
    save_snapshot(cache_path, &snapshot)?;
    Ok(snapshot)
}
