use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::build::build_snapshot;
use crate::cache::{load_or_build_snapshot, save_snapshot};
use crate::config::{
    AppConfig, LoadedConfig, config_keys, env_var_for_key, get_config_value, persist_file_config,
    set_config_value,
};
use crate::download::download_gtfs_zip_to_dir;
use crate::matcher::{
    GENERIC_QUERY_TOKENS, NameMatchMode, exact_key_case_insensitive, match_name_candidates,
    normalize_for_match, relaxed_name_matches,
};
use crate::merge::{ensure_combined_source_ready, validate_raw_sources};
use crate::route_planner::rebuild_planner_cache;
use crate::snapshot::{StopCluster, StopRecord};

const STOP_FUZZY_THRESHOLD: f64 = 0.94;

pub fn cmd_cache_build(
    config: &AppConfig,
    source_path_override: Option<&str>,
    cache_path_override: Option<&str>,
    download: bool,
) -> Result<(), String> {
    let source_path = source_path_override.unwrap_or(&config.merged_gtfs_path);
    let cache_path = cache_path_override.unwrap_or(&config.snapshot_cache_path);
    let planner_cache_path = &config.planner_cache_path;

    if download {
        download_raw_data(config, true)?;
    } else {
        ensure_raw_sources_exist(config).map_err(|err| {
            format!("{err}\nHint: run `oeffi cache-build --download` for first-time setup.")
        })?;
    }

    eprintln!("Preprocessing raw GTFS data (merge) ...");
    ensure_combined_source_ready(
        source_path,
        &config.wiener_linien_source_dir,
        &config.oebb_source_dir,
    )?;
    // Always rebuild both caches from source GTFS files.
    eprintln!("Rebuilding snapshot cache: {cache_path}");
    let snapshot = build_snapshot(source_path)?;
    save_snapshot(cache_path, &snapshot)?;
    eprintln!("Rebuilding planner cache: {planner_cache_path}");
    let planner = rebuild_planner_cache(source_path, planner_cache_path)?;

    println!("Built snapshot cache: {cache_path}");
    println!("  source: {}", snapshot.fingerprint.source_path);
    println!("  files: {}", snapshot.fingerprint.file_count);
    println!("  size: {} bytes", snapshot.fingerprint.total_size_bytes);
    println!("  routes: {}", snapshot.summary.routes);
    println!("  trips: {}", snapshot.summary.trips);
    println!("Built planner cache: {planner_cache_path}");
    println!("  stations: {}", planner.stations_count());
    println!("  routes: {}", planner.routes_count());
    println!("  trips: {}", planner.trips_count());

    Ok(())
}

pub fn cmd_init(config: &AppConfig, force: bool) -> Result<(), String> {
    let raw_sources_present =
        validate_raw_sources(&config.wiener_linien_source_dir, &config.oebb_source_dir).is_ok();

    if raw_sources_present && !force {
        return Err(
            "Raw GTFS data already exists. Re-run with `oeffi init --force` to overwrite."
                .to_string(),
        );
    }

    if force && raw_sources_present {
        eprintln!("`--force` set: existing raw GTFS data will be overwritten.");
    }

    eprintln!("Initializing local data and caches ...");
    download_raw_data(config, true)?;
    cmd_cache_build(config, None, None, false)?;
    eprintln!("Initialization complete.");
    Ok(())
}

pub fn cmd_gtfs_summary(config: &AppConfig) -> Result<(), String> {
    let source_path = &config.merged_gtfs_path;
    let cache_path = &config.snapshot_cache_path;
    ensure_combined_source_ready(
        source_path,
        &config.wiener_linien_source_dir,
        &config.oebb_source_dir,
    )?;
    // Reuse cache when possible, otherwise build it once transparently.
    let snapshot = load_or_build_snapshot(source_path, cache_path)?;

    println!("GTFS summary for {source_path} (via cache: {cache_path})");
    println!("  agencies: {}", snapshot.summary.agencies);
    println!("  routes: {}", snapshot.summary.routes);
    println!("  trips: {}", snapshot.summary.trips);
    println!("  stops: {}", snapshot.summary.stops);
    println!("  calendars: {}", snapshot.summary.calendars);
    println!("  calendar_dates: {}", snapshot.summary.calendar_dates);

    Ok(())
}

pub fn cmd_list_routes(config: &AppConfig) -> Result<(), String> {
    let source_path = &config.merged_gtfs_path;
    let cache_path = &config.snapshot_cache_path;
    ensure_combined_source_ready(
        source_path,
        &config.wiener_linien_source_dir,
        &config.oebb_source_dir,
    )?;
    // Load snapshot and print a compact route table.
    let snapshot = load_or_build_snapshot(source_path, cache_path)?;

    println!(
        "Routes in {source_path} ({} total, via cache: {cache_path}):",
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

pub fn cmd_list_stops(config: &AppConfig) -> Result<(), String> {
    let source_path = &config.merged_gtfs_path;
    let cache_path = &config.snapshot_cache_path;
    ensure_combined_source_ready(
        source_path,
        &config.wiener_linien_source_dir,
        &config.oebb_source_dir,
    )?;
    let snapshot = load_or_build_snapshot(source_path, cache_path)?;

    println!(
        "Clustered stops in {source_path} ({} total, via cache: {cache_path}):",
        snapshot.stop_clusters.len()
    );

    for cluster in &snapshot.stop_clusters {
        println!("  {} | {}", cluster.name, cluster.key);
    }

    Ok(())
}

pub fn cmd_route_stops(config: &AppConfig, route_name: &str) -> Result<(), String> {
    let source_path = &config.merged_gtfs_path;
    let cache_path = &config.snapshot_cache_path;
    ensure_combined_source_ready(
        source_path,
        &config.wiener_linien_source_dir,
        &config.oebb_source_dir,
    )?;
    let snapshot = load_or_build_snapshot(source_path, cache_path)?;

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

    let mut selected_ids: Vec<String> = ordered_route_ids
        .iter()
        .filter_map(|route_id| {
            snapshot
                .route_stops_by_route_id
                .get(route_id)
                .map(|_| route_id.clone())
        })
        .collect();
    selected_ids.sort();
    selected_ids.dedup();

    println!("Line {route_name} in {source_path} (via cache: {cache_path})");

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

pub fn cmd_stop_inspect(config: &AppConfig, query: &str) -> Result<(), String> {
    let source_path = &config.merged_gtfs_path;
    let cache_path = &config.snapshot_cache_path;
    ensure_combined_source_ready(
        source_path,
        &config.wiener_linien_source_dir,
        &config.oebb_source_dir,
    )?;
    let snapshot = load_or_build_snapshot(source_path, cache_path)?;
    let query_upper = query.to_ascii_uppercase();

    let stop_by_id: HashMap<&str, &StopRecord> = snapshot
        .stops
        .iter()
        .map(|stop| (stop.id.as_str(), stop))
        .collect();

    // Matching strategy mirrors route planning: key/id/code, then exact/fuzzy/relaxed station name.
    let mut match_mode = "partial match (name/id/code)";
    let mut matched_cluster_idxs: Vec<u32> = snapshot
        .stop_cluster_idx_by_key
        .get(query)
        .copied()
        .map(|idx| vec![idx])
        .unwrap_or_default();

    if matched_cluster_idxs.is_empty() {
        if let Some((_, idx)) = exact_key_case_insensitive(&snapshot.stop_cluster_idx_by_key, query)
        {
            matched_cluster_idxs = vec![*idx];
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

    if matched_cluster_idxs.is_empty()
        && let Some(ids) = snapshot.stop_ids_by_code_upper.get(&query_upper)
    {
        match_mode = "exact stop code";
        matched_cluster_idxs = ids
            .iter()
            .filter_map(|stop_id| snapshot.stop_id_to_cluster_idx.get(stop_id).copied())
            .collect();
    }

    if matched_cluster_idxs.is_empty() {
        let (matches, mode) = match_name_candidates(
            &snapshot.stop_cluster_idxs_by_name_upper,
            query,
            STOP_FUZZY_THRESHOLD,
            &GENERIC_QUERY_TOKENS,
            24,
        );
        matched_cluster_idxs = matches;
        match_mode = match mode {
            NameMatchMode::Exact => "exact stop/station name",
            NameMatchMode::Fuzzy => "fuzzy stop/station name",
            NameMatchMode::Relaxed => "relaxed stop/station name",
            NameMatchMode::None => match_mode,
        };
    }

    if matched_cluster_idxs.is_empty() {
        // Secondary fallback: raw stop-name lookup maps to its containing station cluster.
        let (stop_name_matches, mode) = match_name_candidates(
            &snapshot.stop_ids_by_name_upper,
            query,
            STOP_FUZZY_THRESHOLD,
            &GENERIC_QUERY_TOKENS,
            24,
        );
        matched_cluster_idxs = stop_name_matches
            .into_iter()
            .filter_map(|stop_id| snapshot.stop_id_to_cluster_idx.get(&stop_id).copied())
            .collect();
        match_mode = match mode {
            NameMatchMode::Exact => "exact stop name",
            NameMatchMode::Fuzzy => "fuzzy stop name",
            NameMatchMode::Relaxed => "relaxed stop name",
            NameMatchMode::None => match_mode,
        };
    }

    if matched_cluster_idxs.is_empty() {
        let mut suggestions = relaxed_name_matches(
            &snapshot.stop_cluster_idxs_by_name_upper,
            query,
            &GENERIC_QUERY_TOKENS,
            5,
        )
        .into_iter()
        .filter_map(|cluster_idx| snapshot.stop_clusters.get(cluster_idx as usize))
        .map(|cluster| cluster.name.clone())
        .collect::<Vec<_>>();

        if suggestions.is_empty() {
            let normalized_query = normalize_for_match(query);
            suggestions = snapshot
                .stop_clusters
                .iter()
                .filter(|cluster| normalize_for_match(&cluster.name).contains(&normalized_query))
                .map(|cluster| cluster.name.clone())
                .collect();
        }

        if suggestions.is_empty() {
            let normalized_query = normalize_for_match(query);
            suggestions = snapshot
                .stops
                .iter()
                .filter(|stop| normalize_for_match(&stop.name).contains(&normalized_query))
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
            "No stops found for query '{query}'.\nSearched fields: cluster key, stop id, stop code, and route-style stop/station name matching.\n{suggestion_text}\nExamples:\n  oeffi inspect \"Karlsplatz\"\n  oeffi inspect \"at:49:657:0:8\""
        ));
    }

    let mut seen_cluster_idxs: HashSet<u32> = HashSet::new();
    matched_cluster_idxs.retain(|idx| seen_cluster_idxs.insert(*idx));
    let selected_cluster_idxs = matched_cluster_idxs;

    let route_by_id: HashMap<&str, &crate::snapshot::RouteEntry> = snapshot
        .routes
        .iter()
        .map(|route| (route.id.as_str(), route))
        .collect();

    println!("Stop inspect for '{query}' in {source_path} (via cache: {cache_path})");
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

pub fn cmd_config_list(loaded: &LoadedConfig) -> Result<(), String> {
    println!("Config file: {}", loaded.paths.config_path.display());
    println!();

    for key in config_keys() {
        let value = get_config_value(&loaded.effective_config, key)
            .ok_or_else(|| format!("Missing config key '{key}'"))?;
        if let Some(env_name) = env_var_for_key(key)
            && loaded.env_overrides.contains(env_name)
        {
            println!("{key}={value}  (env: {env_name})");
            continue;
        }
        println!("{key}={value}");
    }

    Ok(())
}

pub fn cmd_config_get(loaded: &LoadedConfig, key: &str) -> Result<(), String> {
    let value = get_config_value(&loaded.effective_config, key).ok_or_else(|| {
        format!(
            "unknown config key '{key}'. Valid keys: {}",
            config_keys().join(", ")
        )
    })?;

    println!("{value}");
    Ok(())
}

pub fn cmd_config_set(loaded: &LoadedConfig, key: &str, value: &str) -> Result<(), String> {
    let mut updated = loaded.file_config.clone();
    set_config_value(&mut updated, key, value.to_string())?;
    persist_file_config(&loaded.paths, &updated)?;

    // Keep directory structure usable after changes to path keys.
    if key.ends_with("_path") || key.ends_with("_dir") || key == "raw_data_root" {
        let to_create = Path::new(value);
        if key.ends_with("_path") {
            if let Some(parent) = to_create.parent() {
                std::fs::create_dir_all(parent).map_err(|err| {
                    format!(
                        "Saved config but failed to create parent directory '{}': {err}",
                        parent.display()
                    )
                })?;
            }
        } else {
            std::fs::create_dir_all(to_create).map_err(|err| {
                format!(
                    "Saved config but failed to create directory '{}': {err}",
                    to_create.display()
                )
            })?;
        }
    }

    println!("Updated '{key}' in {}", loaded.paths.config_path.display());
    if let Some(env_name) = env_var_for_key(key)
        && loaded.env_overrides.contains(env_name)
    {
        println!("Note: currently overridden by env var {env_name}");
    }
    Ok(())
}

fn ensure_raw_sources_exist(config: &AppConfig) -> Result<(), String> {
    validate_raw_sources(&config.wiener_linien_source_dir, &config.oebb_source_dir)
}

fn download_raw_data(config: &AppConfig, replace_existing: bool) -> Result<(), String> {
    if !replace_existing {
        ensure_raw_sources_exist(config)?;
        return Ok(());
    }

    eprintln!("Refreshing raw GTFS data sources ...");
    download_gtfs_zip_to_dir(
        &config.wiener_linien_gtfs_url,
        &config.wiener_linien_source_dir,
        "Wiener Linien",
    )?;
    download_gtfs_zip_to_dir(&config.oebb_gtfs_url, &config.oebb_source_dir, "ÖBB")?;
    Ok(())
}

pub fn is_missing_local_data_error(message: &str) -> bool {
    message.contains("Missing required GTFS source file")
        || message.contains("Failed to load GTFS")
        || message.contains("Cannot read source file")
}
