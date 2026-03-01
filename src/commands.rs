use std::collections::{HashMap, HashSet};

use crate::build::build_snapshot;
use crate::cache::{load_or_build_snapshot, save_snapshot};
use crate::cli::DEFAULT_CACHE_PATH;
use crate::snapshot::StopRecord;

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

fn route_labels_for_ids(
    route_ids: &[String],
    route_by_id: &HashMap<&str, &crate::snapshot::RouteEntry>,
) -> Vec<String> {
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

fn collect_route_ids_for_stop_with_children(
    stop: &StopRecord,
    children_by_parent: &HashMap<&str, Vec<&StopRecord>>,
    route_ids_by_stop_id: &HashMap<String, Vec<String>>,
) -> Vec<String> {
    let mut route_ids: HashSet<String> = route_ids_by_stop_id
        .get(&stop.id)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .collect();

    if let Some(children) = children_by_parent.get(stop.id.as_str()) {
        for child in children {
            if let Some(child_route_ids) = route_ids_by_stop_id.get(&child.id) {
                for route_id in child_route_ids {
                    route_ids.insert(route_id.clone());
                }
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

    let mut children_by_parent: HashMap<&str, Vec<&StopRecord>> = HashMap::new();
    for stop in &snapshot.stops {
        if let Some(parent_id) = &stop.parent_station {
            children_by_parent
                .entry(parent_id.as_str())
                .or_default()
                .push(stop);
        }
    }

    let mut match_mode = "partial match (name/id/code)";
    let mut matched_ids: Vec<String> = snapshot
        .stops
        .iter()
        .filter(|stop| stop.id.eq_ignore_ascii_case(query))
        .map(|stop| stop.id.clone())
        .collect();

    if matched_ids.is_empty() {
        if let Some(ids) = snapshot.stop_ids_by_code_upper.get(&query_upper) {
            match_mode = "exact stop code";
            matched_ids = ids.clone();
        }
    } else {
        match_mode = "exact stop id";
    }

    if matched_ids.is_empty() {
        if let Some(ids) = snapshot.stop_ids_by_name_upper.get(&query_upper) {
            match_mode = "exact stop name";
            matched_ids = ids.clone();
        }
    }

    if matched_ids.is_empty() {
        matched_ids = snapshot
            .stops
            .iter()
            .filter(|stop| {
                stop.id.to_ascii_uppercase().contains(&query_upper)
                    || stop.name.to_ascii_uppercase().contains(&query_upper)
                    || stop
                        .code
                        .as_ref()
                        .map(|code| code.to_ascii_uppercase().contains(&query_upper))
                        .unwrap_or(false)
            })
            .map(|stop| stop.id.clone())
            .collect();
    }

    if matched_ids.is_empty() {
        let query_upper = query.to_ascii_uppercase();

        let mut suggestions: Vec<String> = snapshot
            .stops
            .iter()
            .filter(|stop| stop.name.to_ascii_uppercase().starts_with(&query_upper))
            .map(|stop| stop.name.clone())
            .collect();

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
            "No stops found for query '{query}'.\nSearched fields: stop id, stop code, and stop name (exact and partial).\n{suggestion_text}\nExamples:\n  oeffi stop-inspect \"Karlsplatz\"\n  oeffi stop-inspect \"at:49:657:0:8\""
        ));
    }

    matched_ids.sort();
    matched_ids.dedup();

    let mut ranked: Vec<(usize, String)> = matched_ids
        .into_iter()
        .filter_map(|stop_id| {
            stop_by_id.get(stop_id.as_str()).map(|stop| {
                let route_count = collect_route_ids_for_stop_with_children(
                    stop,
                    &children_by_parent,
                    &snapshot.route_ids_by_stop_id,
                )
                .len();
                (route_count, stop_id)
            })
        })
        .collect();

    ranked.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));

    let selected_stop_ids: Vec<String> = ranked.into_iter().map(|(_, stop_id)| stop_id).collect();

    let route_by_id: HashMap<&str, &crate::snapshot::RouteEntry> = snapshot
        .routes
        .iter()
        .map(|route| (route.id.as_str(), route))
        .collect();

    println!("Stop inspect for '{query}' in {source_path} (via cache: {DEFAULT_CACHE_PATH})");
    println!("Match mode: {match_mode}");

    for stop_id in selected_stop_ids {
        if let Some(stop) = stop_by_id.get(stop_id.as_str()) {
            let route_ids = collect_route_ids_for_stop_with_children(
                stop,
                &children_by_parent,
                &snapshot.route_ids_by_stop_id,
            );
            let route_labels = route_labels_for_ids(&route_ids, &route_by_id);

            println!();
            println!("  Stop: {} ({})", stop.name, stop.id);
            if let Some(code) = &stop.code {
                println!("  Code: {code}");
            }
            if let Some(parent_station) = &stop.parent_station {
                println!("  Parent station: {parent_station}");
            }
            println!("  Routes: {}", route_labels.len());
            for label in route_labels {
                println!("    - {label}");
            }
        }
    }

    Ok(())
}
