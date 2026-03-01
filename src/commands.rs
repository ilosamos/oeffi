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

pub fn cmd_route_stops(source_path: &str, route_name: &str, show_all: bool) -> Result<(), String> {
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

    let mut ordered_route_ids: Vec<String> = route_ids.into_iter().collect();
    ordered_route_ids.sort();

    let route_by_id: HashMap<&str, &crate::snapshot::RouteEntry> = snapshot
        .routes
        .iter()
        .map(|route| (route.id.as_str(), route))
        .collect();

    let mut found_any = false;
    for route_id in &ordered_route_ids {
        if snapshot.route_stops_by_route_id.contains_key(route_id) {
            found_any = true;
            break;
        }
    }

    if !found_any {
        return Err(format!("No stops found for route '{route_name}'."));
    }

    let mut candidates: Vec<(String, usize)> = ordered_route_ids
        .iter()
        .filter_map(|route_id| {
            snapshot
                .route_stops_by_route_id
                .get(route_id)
                .map(|stops| (route_id.clone(), stops.len()))
        })
        .collect();
    candidates.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));

    let selected_ids: Vec<String> = if show_all {
        candidates
            .into_iter()
            .map(|(route_id, _)| route_id)
            .collect()
    } else {
        candidates
            .into_iter()
            .next()
            .map(|(route_id, _)| vec![route_id])
            .unwrap_or_default()
    };

    if show_all {
        println!(
            "Route {route_name} in {source_path} (all variants, via cache: {DEFAULT_CACHE_PATH})"
        );
    } else {
        println!(
            "Route {route_name} in {source_path} (longest variant, via cache: {DEFAULT_CACHE_PATH})"
        );
    }

    for route_id in selected_ids {
        if let Some(stops) = snapshot.route_stops_by_route_id.get(&route_id) {
            let long_name = route_by_id
                .get(route_id.as_str())
                .map(|r| r.long_name.as_str())
                .unwrap_or("-");

            println!();
            println!(
                "  Route ID: {} | {} stops | {}",
                route_id,
                stops.len(),
                long_name
            );

            for (idx, stop) in stops.iter().enumerate() {
                if stop.stop_ids_count > 1 {
                    println!(
                        "    {:>3}. {} ({} stop IDs)",
                        idx + 1,
                        stop.name,
                        stop.stop_ids_count
                    );
                } else {
                    println!("    {:>3}. {}", idx + 1, stop.name);
                }
            }
        }
    }

    Ok(())
}
