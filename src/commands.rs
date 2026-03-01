use std::collections::{HashMap, HashSet};

use crate::build::build_snapshot;
use crate::cache::{load_or_build_snapshot, save_snapshot};
use crate::cli::DEFAULT_CACHE_PATH;

pub fn cmd_cache_build(source_path: &str, cache_path: &str) -> Result<(), String> {
    let snapshot = build_snapshot(source_path)?;
    save_snapshot(cache_path, &snapshot)?;

    println!("Built cache: {cache_path}");
    println!("  source: {}", snapshot.fingerprint.source_path);
    println!("  files: {}", snapshot.fingerprint.file_count);
    println!("  size: {} bytes", snapshot.fingerprint.total_size_bytes);
    println!("  routes: {}", snapshot.summary.routes);
    println!("  trips: {}", snapshot.summary.trips);

    Ok(())
}

pub fn cmd_gtfs_summary(source_path: &str) -> Result<(), String> {
    let snapshot = load_or_build_snapshot(source_path, DEFAULT_CACHE_PATH)?;

    println!("GTFS summary for {source_path} (via cache: {DEFAULT_CACHE_PATH})");
    println!("  agencies: {}", snapshot.summary.agencies);
    println!("  routes: {}", snapshot.summary.routes);
    println!("  trips: {}", snapshot.summary.trips);
    println!("  stops: {}", snapshot.summary.stops);
    println!("  calendars: {}", snapshot.summary.calendars);
    println!("  calendar_dates: {}", snapshot.summary.calendar_dates);

    Ok(())
}

pub fn cmd_list_routes(source_path: &str) -> Result<(), String> {
    let snapshot = load_or_build_snapshot(source_path, DEFAULT_CACHE_PATH)?;

    println!(
        "Routes in {source_path} ({} total, via cache: {DEFAULT_CACHE_PATH}):",
        snapshot.routes.len()
    );

    for route in &snapshot.routes {
        println!(
            "  {: <8} | {: <16} | {}",
            route.short_name, route.id, route.long_name
        );
    }

    Ok(())
}

pub fn cmd_route_stops(source_path: &str, route_name: &str) -> Result<(), String> {
    let snapshot = load_or_build_snapshot(source_path, DEFAULT_CACHE_PATH)?;

    let query_upper = route_name.to_ascii_uppercase();

    let mut route_ids: HashSet<String> = snapshot
        .route_ids_by_short_name_upper
        .get(&query_upper)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .collect();

    for route in &snapshot.routes {
        if route.id.eq_ignore_ascii_case(route_name) {
            route_ids.insert(route.id.clone());
        }
    }

    if route_ids.is_empty() {
        return Err(format!(
            "No route found for '{route_name}'. Try `oeffi routes` to list available routes."
        ));
    }

    let mut combined: HashMap<String, (usize, usize)> = HashMap::new();

    for route_id in &route_ids {
        if let Some(stops) = snapshot.route_stops_by_route_id.get(route_id) {
            for (index, stop) in stops.iter().enumerate() {
                let order = index;
                combined
                    .entry(stop.name.clone())
                    .and_modify(|(min_order, max_stop_ids_count)| {
                        if order < *min_order {
                            *min_order = order;
                        }
                        if stop.stop_ids_count > *max_stop_ids_count {
                            *max_stop_ids_count = stop.stop_ids_count;
                        }
                    })
                    .or_insert((order, stop.stop_ids_count));
            }
        }
    }

    if combined.is_empty() {
        return Err(format!("No stops found for route '{route_name}'."));
    }

    let mut rows: Vec<(usize, String, usize)> = combined
        .into_iter()
        .map(|(name, (order, stop_ids_count))| (order, name, stop_ids_count))
        .collect();

    rows.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));

    println!(
        "Route {route_name} in {source_path}: {} unique stops (via cache: {DEFAULT_CACHE_PATH})",
        rows.len()
    );

    for (idx, (_, name, stop_ids_count)) in rows.iter().enumerate() {
        if *stop_ids_count > 1 {
            println!("  {:>3}. {} ({} stop IDs)", idx + 1, name, stop_ids_count);
        } else {
            println!("  {:>3}. {}", idx + 1, name);
        }
    }

    Ok(())
}
