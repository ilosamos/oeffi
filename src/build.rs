use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use gtfs_structures::GtfsReader;

use crate::snapshot::{
    RouteEntry, SNAPSHOT_VERSION, Snapshot, SnapshotSummary, SourceFingerprint, StopEntry,
};

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn file_mtime_unix(meta: &fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn compute_source_fingerprint(source_path: &str) -> Result<SourceFingerprint, String> {
    let path = Path::new(source_path);
    let meta = fs::metadata(path)
        .map_err(|err| format!("Cannot read GTFS source metadata '{source_path}': {err}"))?;

    if meta.is_file() {
        return Ok(SourceFingerprint {
            source_path: source_path.to_string(),
            file_count: 1,
            total_size_bytes: meta.len(),
            newest_modified_unix_secs: file_mtime_unix(&meta),
        });
    }

    if !meta.is_dir() {
        return Err(format!(
            "GTFS source '{source_path}' is neither a file nor a directory."
        ));
    }

    let mut file_count = 0usize;
    let mut total_size_bytes = 0u64;
    let mut newest_modified_unix_secs = 0u64;

    let entries = fs::read_dir(path)
        .map_err(|err| format!("Cannot list GTFS source directory '{source_path}': {err}"))?;

    for entry in entries {
        let entry = entry.map_err(|err| format!("Error reading directory entry: {err}"))?;
        let entry_meta = entry.metadata().map_err(|err| {
            format!(
                "Cannot read metadata for '{}': {err}",
                entry.path().display()
            )
        })?;

        if entry_meta.is_file() {
            file_count += 1;
            total_size_bytes = total_size_bytes.saturating_add(entry_meta.len());
            let mtime = file_mtime_unix(&entry_meta);
            if mtime > newest_modified_unix_secs {
                newest_modified_unix_secs = mtime;
            }
        }
    }

    Ok(SourceFingerprint {
        source_path: source_path.to_string(),
        file_count,
        total_size_bytes,
        newest_modified_unix_secs,
    })
}

pub fn build_snapshot(source_path: &str) -> Result<Snapshot, String> {
    let fingerprint = compute_source_fingerprint(source_path)?;

    let gtfs = GtfsReader::default()
        .read_shapes(false)
        .trim_fields(false)
        .read(source_path)
        .map_err(|err| format!("Failed to load GTFS from '{source_path}': {err}"))?;

    let summary = SnapshotSummary {
        agencies: gtfs.agencies.len(),
        routes: gtfs.routes.len(),
        trips: gtfs.trips.len(),
        stops: gtfs.stops.len(),
        calendars: gtfs.calendar.len(),
        calendar_dates: gtfs.calendar_dates.len(),
    };

    let mut routes: Vec<RouteEntry> = gtfs
        .routes
        .values()
        .map(|route| RouteEntry {
            id: route.id.clone(),
            short_name: route.short_name.clone().unwrap_or_else(|| "-".to_string()),
            long_name: route.long_name.clone().unwrap_or_else(|| "-".to_string()),
        })
        .collect();

    routes.sort_by(|a, b| a.short_name.cmp(&b.short_name).then(a.id.cmp(&b.id)));

    let mut route_ids_by_short_name_upper: HashMap<String, Vec<String>> = HashMap::new();
    for route in &routes {
        if route.short_name != "-" {
            route_ids_by_short_name_upper
                .entry(route.short_name.to_ascii_uppercase())
                .or_default()
                .push(route.id.clone());
        }
    }

    for route_ids in route_ids_by_short_name_upper.values_mut() {
        route_ids.sort();
        route_ids.dedup();
    }

    let mut route_stop_acc: HashMap<String, HashMap<String, (u32, HashSet<String>)>> =
        HashMap::new();

    for trip in gtfs.trips.values() {
        let stop_map = route_stop_acc.entry(trip.route_id.clone()).or_default();

        for stop_time in &trip.stop_times {
            let stop_name = stop_time
                .stop
                .name
                .clone()
                .unwrap_or_else(|| format!("<unknown stop {}>", stop_time.stop.id));

            let seq = stop_time.stop_sequence;
            let entry = stop_map
                .entry(stop_name)
                .or_insert_with(|| (seq, HashSet::new()));

            if seq < entry.0 {
                entry.0 = seq;
            }
            entry.1.insert(stop_time.stop.id.clone());
        }
    }

    let mut route_stops_by_route_id: HashMap<String, Vec<StopEntry>> = HashMap::new();

    for (route_id, stop_map) in route_stop_acc {
        let mut stops: Vec<(u32, String, usize)> = stop_map
            .into_iter()
            .map(|(name, (seq, stop_ids))| (seq, name, stop_ids.len()))
            .collect();

        stops.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));

        let mapped: Vec<StopEntry> = stops
            .into_iter()
            .map(|(_, name, stop_ids_count)| StopEntry {
                name,
                stop_ids_count,
            })
            .collect();

        route_stops_by_route_id.insert(route_id, mapped);
    }

    Ok(Snapshot {
        version: SNAPSHOT_VERSION,
        built_unix_secs: now_unix_secs(),
        fingerprint,
        summary,
        routes,
        route_ids_by_short_name_upper,
        route_stops_by_route_id,
    })
}
