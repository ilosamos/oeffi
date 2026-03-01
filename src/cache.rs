use std::fs::File;
use std::io::{BufReader, BufWriter};

use crate::build::{build_snapshot, compute_source_fingerprint};
use crate::snapshot::{SNAPSHOT_VERSION, Snapshot};

pub fn save_snapshot(path: &str, snapshot: &Snapshot) -> Result<(), String> {
    let file =
        File::create(path).map_err(|err| format!("Failed to create cache file '{path}': {err}"))?;
    let mut writer = BufWriter::new(file);
    bincode::serialize_into(&mut writer, snapshot)
        .map_err(|err| format!("Failed to serialize cache '{path}': {err}"))
}

pub fn load_snapshot(path: &str) -> Result<Snapshot, String> {
    let file =
        File::open(path).map_err(|err| format!("Failed to open cache file '{path}': {err}"))?;
    let mut reader = BufReader::new(file);
    bincode::deserialize_from(&mut reader)
        .map_err(|err| format!("Failed to deserialize cache '{path}': {err}"))
}

pub fn cache_is_fresh(snapshot: &Snapshot, source_path: &str) -> Result<bool, String> {
    if snapshot.version != SNAPSHOT_VERSION {
        return Ok(false);
    }

    let current = compute_source_fingerprint(source_path)?;
    Ok(snapshot.fingerprint == current)
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
