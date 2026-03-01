use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use gtfs_structures::GtfsReader;

use crate::snapshot::{
    RouteEntry, SNAPSHOT_VERSION, Snapshot, SnapshotSummary, SourceFingerprint, StopEntry,
    StopRecord,
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

    let mut stops: Vec<StopRecord> = gtfs
        .stops
        .values()
        .map(|stop| StopRecord {
            id: stop.id.clone(),
            name: stop.name.clone().unwrap_or_else(|| "<unknown>".to_string()),
            code: stop.code.clone(),
            parent_station: stop.parent_station.clone(),
        })
        .collect();
    stops.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));

    let mut stop_ids_by_name_upper: HashMap<String, Vec<String>> = HashMap::new();
    let mut stop_ids_by_code_upper: HashMap<String, Vec<String>> = HashMap::new();
    for stop in &stops {
        stop_ids_by_name_upper
            .entry(stop.name.to_ascii_uppercase())
            .or_default()
            .push(stop.id.clone());

        if let Some(code) = &stop.code {
            stop_ids_by_code_upper
                .entry(code.to_ascii_uppercase())
                .or_default()
                .push(stop.id.clone());
        }
    }

    for stop_ids in stop_ids_by_name_upper.values_mut() {
        stop_ids.sort();
        stop_ids.dedup();
    }
    for stop_ids in stop_ids_by_code_upper.values_mut() {
        stop_ids.sort();
        stop_ids.dedup();
    }

    let mut route_stop_ids_by_name: HashMap<String, HashMap<String, HashSet<String>>> =
        HashMap::new();
    let mut representative_trip_names_by_route: HashMap<String, Vec<String>> = HashMap::new();
    let mut route_ids_by_stop_id_set: HashMap<String, HashSet<String>> = HashMap::new();

    for trip in gtfs.trips.values() {
        let stop_ids_by_name = route_stop_ids_by_name
            .entry(trip.route_id.clone())
            .or_default();

        let mut trip_names: Vec<String> = Vec::new();
        let mut seen_names: HashSet<String> = HashSet::new();

        for stop_time in &trip.stop_times {
            let stop_name = stop_time
                .stop
                .name
                .clone()
                .unwrap_or_else(|| format!("<unknown stop {}>", stop_time.stop.id));

            stop_ids_by_name
                .entry(stop_name.clone())
                .or_default()
                .insert(stop_time.stop.id.clone());

            route_ids_by_stop_id_set
                .entry(stop_time.stop.id.clone())
                .or_default()
                .insert(trip.route_id.clone());

            if seen_names.insert(stop_name.clone()) {
                trip_names.push(stop_name);
            }
        }

        representative_trip_names_by_route
            .entry(trip.route_id.clone())
            .and_modify(|existing| {
                if trip_names.len() > existing.len() {
                    *existing = trip_names.clone();
                }
            })
            .or_insert(trip_names);
    }

    let mut route_stops_by_route_id: HashMap<String, Vec<StopEntry>> = HashMap::new();

    for (route_id, stop_ids_by_name) in route_stop_ids_by_name {
        let mut ordered_names = representative_trip_names_by_route
            .get(&route_id)
            .cloned()
            .unwrap_or_default();

        let ordered_set: HashSet<String> = ordered_names.iter().cloned().collect();
        let mut missing: Vec<String> = stop_ids_by_name
            .keys()
            .filter(|name| !ordered_set.contains(*name))
            .cloned()
            .collect();
        missing.sort();
        ordered_names.extend(missing);

        let mapped: Vec<StopEntry> = ordered_names
            .into_iter()
            .map(|name| StopEntry {
                stop_ids_count: stop_ids_by_name
                    .get(&name)
                    .map(|ids| ids.len())
                    .unwrap_or(0),
                name,
            })
            .collect();

        route_stops_by_route_id.insert(route_id, mapped);
    }

    let mut route_ids_by_stop_id: HashMap<String, Vec<String>> = HashMap::new();
    for (stop_id, route_ids_set) in route_ids_by_stop_id_set {
        let mut route_ids: Vec<String> = route_ids_set.into_iter().collect();
        route_ids.sort();
        route_ids_by_stop_id.insert(stop_id, route_ids);
    }

    Ok(Snapshot {
        version: SNAPSHOT_VERSION,
        built_unix_secs: now_unix_secs(),
        fingerprint,
        summary,
        routes,
        route_ids_by_short_name_upper,
        route_stops_by_route_id,
        stops,
        stop_ids_by_name_upper,
        stop_ids_by_code_upper,
        route_ids_by_stop_id,
    })
}
