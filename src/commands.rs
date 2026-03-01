use std::collections::{HashMap, HashSet};

use strsim::jaro_winkler;

use crate::build::build_snapshot;
use crate::cache::{load_or_build_snapshot, save_snapshot};
use crate::cli::DEFAULT_CACHE_PATH;
use crate::route_planner::rebuild_planner_cache;
use crate::snapshot::{StopCluster, StopRecord};

const STOP_FUZZY_THRESHOLD: f64 = 0.93;

pub fn cmd_cache_build(source_path: &str, cache_path: &str) -> Result<(), String> {
    // Always rebuild both caches from source GTFS files.
    let snapshot = build_snapshot(source_path)?;
    save_snapshot(cache_path, &snapshot)?;
    let planner = rebuild_planner_cache(source_path)?;

    println!("Built snapshot cache: {cache_path}");
    println!("  source: {}", snapshot.fingerprint.source_path);
    println!("  files: {}", snapshot.fingerprint.file_count);
    println!("  size: {} bytes", snapshot.fingerprint.total_size_bytes);
    println!("  routes: {}", snapshot.summary.routes);
    println!("  trips: {}", snapshot.summary.trips);
    println!("Built planner cache: planner.cache.bin");
    println!("  stations: {}", planner.stations_count());
    println!("  routes: {}", planner.routes_count());
    println!("  trips: {}", planner.trips_count());

    Ok(())
}

pub fn cmd_gtfs_summary(source_path: &str) -> Result<(), String> {
    // Reuse cache when possible, otherwise build it once transparently.
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
    // Load snapshot and print a compact route table.
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

    // Accept either route short name (e.g. "U1") or explicit route id.
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

    // Stable ordering for deterministic output.
    let mut ordered_route_ids: Vec<String> = route_ids.into_iter().collect();
    ordered_route_ids.sort();

    let route_by_id: HashMap<&str, &crate::snapshot::RouteEntry> = snapshot
        .routes
        .iter()
        .map(|route| (route.id.as_str(), route))
        .collect();

    // Skip routes that have no stop sequence in the snapshot.
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

    // Prefer variants with more stops; this approximates the "main" variant.
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

    // Either print all variants or just the best (longest) one.
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

fn route_labels_for_ids(
    route_ids: &[String],
    route_by_id: &HashMap<&str, &crate::snapshot::RouteEntry>,
) -> Vec<String> {
    // Resolve ids to readable labels and keep them consistently sorted.
    let mut rows: Vec<(String, String, String)> = route_ids
        .iter()
        .map(|route_id| {
            let route = route_by_id.get(route_id.as_str());
            let short_name = route
                .map(|r| r.short_name.clone())
                .unwrap_or_else(|| "-".to_string());
            let long_name = route
                .map(|r| r.long_name.clone())
                .unwrap_or_else(|| "-".to_string());
            (short_name, route_id.clone(), long_name)
        })
        .collect();

    rows.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    rows.into_iter()
        .map(|(short_name, id, long_name)| format!("{short_name} ({id}) - {long_name}"))
        .collect()
}

fn collect_route_ids_for_cluster(
    cluster: &StopCluster,
    route_ids_by_stop_id: &HashMap<String, Vec<String>>,
) -> Vec<String> {
    let mut route_ids: HashSet<String> = HashSet::new();
    for stop_id in &cluster.member_stop_ids {
        if let Some(ids) = route_ids_by_stop_id.get(stop_id) {
            for route_id in ids {
                route_ids.insert(route_id.clone());
            }
        }
    }

    let mut sorted: Vec<String> = route_ids.into_iter().collect();
    sorted.sort();
    sorted
}

pub fn cmd_stop_inspect(source_path: &str, query: &str) -> Result<(), String> {
    let snapshot = load_or_build_snapshot(source_path, DEFAULT_CACHE_PATH)?;
    let query_upper = query.to_ascii_uppercase();

    let stop_by_id: HashMap<&str, &StopRecord> = snapshot
        .stops
        .iter()
        .map(|stop| (stop.id.as_str(), stop))
        .collect();

    // Matching strategy (in order): cluster key, exact stop id, exact code, exact cluster name,
    // exact stop name, fuzzy cluster/stop name.
    let mut match_mode = "partial match (name/id/code)";
    let mut matched_cluster_idxs: Vec<u32> = snapshot
        .stop_cluster_idx_by_key
        .get(query)
        .copied()
        .map(|idx| vec![idx])
        .unwrap_or_default();

    if matched_cluster_idxs.is_empty() {
        for (key, idx) in &snapshot.stop_cluster_idx_by_key {
            if key.eq_ignore_ascii_case(query) {
                matched_cluster_idxs = vec![*idx];
                break;
            }
        }
        if !matched_cluster_idxs.is_empty() {
            match_mode = "exact cluster key";
        }
    } else {
        match_mode = "exact cluster key";
    }

    if matched_cluster_idxs.is_empty() {
        let matched_ids: Vec<String> = snapshot
            .stops
            .iter()
            .filter(|stop| stop.id.eq_ignore_ascii_case(query))
            .map(|stop| stop.id.clone())
            .collect();

        if !matched_ids.is_empty() {
            match_mode = "exact stop id";
            matched_cluster_idxs = matched_ids
                .into_iter()
                .filter_map(|stop_id| snapshot.stop_id_to_cluster_idx.get(&stop_id).copied())
                .collect();
        }
    }

    if matched_cluster_idxs.is_empty() {
        if let Some(ids) = snapshot.stop_ids_by_code_upper.get(&query_upper) {
            match_mode = "exact stop code";
            matched_cluster_idxs = ids
                .iter()
                .filter_map(|stop_id| snapshot.stop_id_to_cluster_idx.get(stop_id).copied())
                .collect();
        }
    }

    if matched_cluster_idxs.is_empty() {
        if let Some(idxs) = snapshot.stop_cluster_idxs_by_name_upper.get(&query_upper) {
            match_mode = "exact stop/station name";
            matched_cluster_idxs = idxs.clone();
        }
    }

    if matched_cluster_idxs.is_empty() {
        if let Some(ids) = snapshot.stop_ids_by_name_upper.get(&query_upper) {
            match_mode = "exact stop name";
            matched_cluster_idxs = ids
                .iter()
                .filter_map(|stop_id| snapshot.stop_id_to_cluster_idx.get(stop_id).copied())
                .collect();
        }
    }

    if matched_cluster_idxs.is_empty() {
        let mut best_name_upper: Option<&String> = None;
        let mut best_score = 0.0f64;

        for cluster_name_upper in snapshot.stop_cluster_idxs_by_name_upper.keys() {
            let score = jaro_winkler(&query_upper, cluster_name_upper);
            if score > best_score {
                best_score = score;
                best_name_upper = Some(cluster_name_upper);
            }
        }

        if best_score >= STOP_FUZZY_THRESHOLD {
            if let Some(name_upper) = best_name_upper {
                matched_cluster_idxs = snapshot
                    .stop_cluster_idxs_by_name_upper
                    .get(name_upper)
                    .cloned()
                    .unwrap_or_default();
                match_mode = "fuzzy stop/station name";
            }
        }
    }

    if matched_cluster_idxs.is_empty() {
        // Fallback fuzzy on stop names if cluster names had no match.
        let mut best_stop_name_upper: Option<&String> = None;
        let mut best_score = 0.0f64;

        for stop_name_upper in snapshot.stop_ids_by_name_upper.keys() {
            let score = jaro_winkler(&query_upper, stop_name_upper);
            if score > best_score {
                best_score = score;
                best_stop_name_upper = Some(stop_name_upper);
            }
        }

        if best_score >= STOP_FUZZY_THRESHOLD {
            if let Some(name_upper) = best_stop_name_upper {
                matched_cluster_idxs = snapshot
                    .stop_ids_by_name_upper
                    .get(name_upper)
                    .into_iter()
                    .flat_map(|ids| ids.iter())
                    .filter_map(|stop_id| snapshot.stop_id_to_cluster_idx.get(stop_id).copied())
                    .collect();
                match_mode = "fuzzy stop name";
            }
        }
    }

    if matched_cluster_idxs.is_empty() {
        // Final fallback: show a few human-friendly suggestions.
        let mut suggestions: Vec<String> = snapshot
            .stop_clusters
            .iter()
            .filter(|cluster| cluster.name.to_ascii_uppercase().starts_with(&query_upper))
            .map(|cluster| cluster.name.clone())
            .collect();

        if suggestions.is_empty() {
            suggestions = snapshot
                .stop_clusters
                .iter()
                .filter(|cluster| cluster.name.to_ascii_uppercase().contains(&query_upper))
                .map(|cluster| cluster.name.clone())
                .collect();
        }

        if suggestions.is_empty() {
            suggestions = snapshot
                .stops
                .iter()
                .filter(|stop| stop.name.to_ascii_uppercase().contains(&query_upper))
                .map(|stop| stop.name.clone())
                .collect();
        }

        suggestions.sort();
        suggestions.dedup();
        let suggestions = suggestions.into_iter().take(5).collect::<Vec<_>>();

        let suggestion_text = if suggestions.is_empty() {
            "No similar stop names found.".to_string()
        } else {
            format!("Did you mean: {}", suggestions.join(", "))
        };

        return Err(format!(
            "No stops found for query '{query}'.\nSearched fields: cluster key, stop id, stop code, exact stop/station name, and high-confidence fuzzy stop/station name.\n{suggestion_text}\nExamples:\n  oeffi stop-inspect \"Karlsplatz\"\n  oeffi stop-inspect \"at:49:657:0:8\""
        ));
    }

    matched_cluster_idxs.sort();
    matched_cluster_idxs.dedup();

    // Rank matches by number of routes served at cluster level.
    let mut ranked: Vec<(usize, u32)> = matched_cluster_idxs
        .into_iter()
        .filter_map(|cluster_idx| {
            snapshot
                .stop_clusters
                .get(cluster_idx as usize)
                .map(|cluster| {
                    let route_count =
                        collect_route_ids_for_cluster(cluster, &snapshot.route_ids_by_stop_id)
                            .len();
                    (route_count, cluster_idx)
                })
        })
        .collect();

    ranked.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));

    let selected_cluster_idxs: Vec<u32> = ranked
        .into_iter()
        .map(|(_, cluster_idx)| cluster_idx)
        .collect();

    let route_by_id: HashMap<&str, &crate::snapshot::RouteEntry> = snapshot
        .routes
        .iter()
        .map(|route| (route.id.as_str(), route))
        .collect();

    println!("Stop inspect for '{query}' in {source_path} (via cache: {DEFAULT_CACHE_PATH})");
    println!("Match mode: {match_mode}");

    // Print each matched logical station cluster with platform details and serving routes.
    for cluster_idx in selected_cluster_idxs {
        let Some(cluster) = snapshot.stop_clusters.get(cluster_idx as usize) else {
            continue;
        };

        let route_ids = collect_route_ids_for_cluster(cluster, &snapshot.route_ids_by_stop_id);
        let route_labels = route_labels_for_ids(&route_ids, &route_by_id);

        println!();
        println!(
            "  Station: {} ({} stop IDs)",
            cluster.name,
            cluster.member_stop_ids.len()
        );
        println!("  Cluster key: {}", cluster.key);
        println!("  Routes: {}", route_labels.len());
        for label in route_labels {
            println!("    - {label}");
        }

        println!("  Platforms/stops:");
        for stop_id in &cluster.member_stop_ids {
            if let Some(stop) = stop_by_id.get(stop_id.as_str()) {
                let code = stop.code.clone().unwrap_or_else(|| "-".to_string());
                let parent = stop
                    .parent_station
                    .clone()
                    .unwrap_or_else(|| "-".to_string());
                println!(
                    "    - {} | id={} | code={} | parent={}",
                    stop.name, stop.id, code, parent
                );
            }
        }
    }

    Ok(())
}
