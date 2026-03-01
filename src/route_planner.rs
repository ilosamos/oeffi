use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use chrono::{Local, Timelike};
use gtfs_structures::{GtfsReader, TransferType};
use raptor::{Journey, Timetable};
use serde::{Deserialize, Serialize};
use strsim::jaro_winkler;

use crate::cache::load_or_build_app_cache;
use crate::cli::{DEFAULT_CACHE_PATH, DEFAULT_GTFS_PATH};
use crate::clustering::{ClusterStopAccessor, build_stop_clusters};

const STOP_FUZZY_THRESHOLD: f64 = 0.94;
const MAX_TRANSFERS: usize = 6;
const SAME_STATION_WALK_SECONDS: usize = 120;
const DEFAULT_TRANSFER_SECONDS: usize = 300;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PlannerStop {
    id: String,
    name: String,
    code: Option<String>,
    parent_station: Option<String>,
}

impl ClusterStopAccessor for PlannerStop {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn parent_station(&self) -> Option<&str> {
        self.parent_station.as_deref()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PlannerRoute {
    base_route_id: String,
    short_name: String,
    long_name: String,
    stops: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PlannerTrip {
    route_idx: u32,
    times: Vec<(usize, usize)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PlannerCluster {
    key: String,
    name: String,
    member_stop_idxs: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PlannerCache {
    stops: Vec<PlannerStop>,
    stop_idx_by_id: HashMap<String, u32>,
    stop_idxs_by_name_upper: HashMap<String, Vec<u32>>,
    stop_idxs_by_code_upper: HashMap<String, Vec<u32>>,
    clusters: Vec<PlannerCluster>,
    cluster_idx_by_key: HashMap<String, u32>,
    cluster_idxs_by_name_upper: HashMap<String, Vec<u32>>,
    stop_idx_to_cluster_idx: Vec<u32>,
    routes: Vec<PlannerRoute>,
    route_stop_pos: Vec<HashMap<u32, usize>>,
    trips: Vec<PlannerTrip>,
    trip_idxs_by_route: Vec<Vec<u32>>,
    routes_serving_stop: HashMap<u32, Vec<u32>>,
    footpaths: HashMap<u32, Vec<u32>>,
    transfer_times: HashMap<(u32, u32), usize>,
}

struct PlannerTimetable<'a> {
    cache: &'a PlannerCache,
}

impl<'a> Timetable for PlannerTimetable<'a> {
    type Stop = u32;
    type Route = u32;
    type Trip = u32;

    fn get_routes_serving_stop(&self, stop: Self::Stop) -> Cow<'_, [Self::Route]> {
        Cow::Owned(
            self.cache
                .routes_serving_stop
                .get(&stop)
                .cloned()
                .unwrap_or_default(),
        )
    }

    fn get_earlier_stop(
        &self,
        route: Self::Route,
        left: Self::Stop,
        right: Self::Stop,
    ) -> Self::Stop {
        let route_idx = route as usize;
        let left_pos = self.cache.route_stop_pos[route_idx]
            .get(&left)
            .copied()
            .unwrap_or(usize::MAX);
        let right_pos = self.cache.route_stop_pos[route_idx]
            .get(&right)
            .copied()
            .unwrap_or(usize::MAX);
        if left_pos <= right_pos { left } else { right }
    }

    fn get_stops_after(&self, route: Self::Route, stop: Self::Stop) -> Cow<'_, [Self::Stop]> {
        let route_idx = route as usize;
        let pos = self.cache.route_stop_pos[route_idx]
            .get(&stop)
            .copied()
            .unwrap_or(0);
        Cow::Owned(self.cache.routes[route_idx].stops[pos..].to_vec())
    }

    fn get_earliest_trip(
        &self,
        route: Self::Route,
        at: raptor::Tau,
        stop: Self::Stop,
    ) -> Option<Self::Trip> {
        let route_idx = route as usize;
        let stop_pos = self.cache.route_stop_pos[route_idx].get(&stop).copied()?;

        self.cache.trip_idxs_by_route[route_idx]
            .iter()
            .copied()
            .filter(|trip_idx| {
                let trip = &self.cache.trips[*trip_idx as usize];
                trip.times[stop_pos].1 >= at
            })
            .min_by_key(|trip_idx| {
                let trip = &self.cache.trips[*trip_idx as usize];
                trip.times[stop_pos].1
            })
    }

    fn get_arrival_time(&self, trip: Self::Trip, stop: Self::Stop) -> raptor::Tau {
        let trip_idx = trip as usize;
        let route_idx = self.cache.trips[trip_idx].route_idx as usize;
        let stop_pos = self.cache.route_stop_pos[route_idx][&stop];
        self.cache.trips[trip_idx].times[stop_pos].0
    }

    fn get_departure_time(&self, trip: Self::Trip, stop: Self::Stop) -> raptor::Tau {
        let trip_idx = trip as usize;
        let route_idx = self.cache.trips[trip_idx].route_idx as usize;
        let stop_pos = self.cache.route_stop_pos[route_idx][&stop];
        self.cache.trips[trip_idx].times[stop_pos].1
    }

    fn get_footpaths_from(&self, stop: Self::Stop) -> Cow<'_, [Self::Stop]> {
        Cow::Owned(self.cache.footpaths.get(&stop).cloned().unwrap_or_default())
    }

    fn get_transfer_time(&self, from: Self::Stop, to: Self::Stop) -> raptor::Tau {
        self.cache
            .transfer_times
            .get(&(from, to))
            .copied()
            .unwrap_or(DEFAULT_TRANSFER_SECONDS)
    }
}

fn format_secs_hhmm(secs: usize) -> String {
    let days = secs / 86_400;
    let rem = secs % 86_400;
    let h = rem / 3_600;
    let m = (rem % 3_600) / 60;
    if days > 0 {
        format!("{h:02}:{m:02} (+{days}d)")
    } else {
        format!("{h:02}:{m:02}")
    }
}

pub(crate) fn build_planner_cache(source_path: &str) -> Result<PlannerCache, String> {
    let gtfs = GtfsReader::default()
        .read_shapes(false)
        .trim_fields(false)
        .read(source_path)
        .map_err(|err| format!("Failed to load GTFS from '{}': {err}", source_path))?;

    let mut stop_ids: Vec<String> = gtfs.stops.keys().cloned().collect();
    stop_ids.sort();

    let mut stops = Vec::with_capacity(stop_ids.len());
    let mut stop_idx_by_id = HashMap::new();
    for (idx, stop_id) in stop_ids.iter().enumerate() {
        let stop = gtfs.stops.get(stop_id).ok_or_else(|| {
            format!(
                "Stop id '{}' vanished while building planner cache",
                stop_id
            )
        })?;
        let idx_u32 = idx as u32;
        stop_idx_by_id.insert(stop_id.clone(), idx_u32);
        stops.push(PlannerStop {
            id: stop_id.clone(),
            name: stop.name.clone().unwrap_or_else(|| "<unknown>".to_string()),
            code: stop.code.clone(),
            parent_station: stop.parent_station.clone(),
        });
    }

    let mut stop_idxs_by_name_upper: HashMap<String, Vec<u32>> = HashMap::new();
    let mut stop_idxs_by_code_upper: HashMap<String, Vec<u32>> = HashMap::new();
    for (idx, stop) in stops.iter().enumerate() {
        let idx_u32 = idx as u32;
        stop_idxs_by_name_upper
            .entry(stop.name.to_ascii_uppercase())
            .or_default()
            .push(idx_u32);
        if let Some(code) = &stop.code {
            stop_idxs_by_code_upper
                .entry(code.to_ascii_uppercase())
                .or_default()
                .push(idx_u32);
        }
    }

    let clustered_stops = build_stop_clusters(&stops, &stop_idx_by_id);
    let clusters: Vec<PlannerCluster> = clustered_stops
        .clusters
        .into_iter()
        .map(|cluster| PlannerCluster {
            key: cluster.key,
            name: cluster.name,
            member_stop_idxs: cluster.member_stop_idxs,
        })
        .collect();
    let cluster_idx_by_key = clustered_stops.cluster_idx_by_key;
    let cluster_idxs_by_name_upper = clustered_stops.cluster_idxs_by_name_upper;
    let stop_idx_to_cluster_idx = clustered_stops.stop_idx_to_cluster_idx;

    // Route variants are keyed by base route id + exact stop pattern.
    let mut route_variant_idx: HashMap<(String, Vec<u32>), u32> = HashMap::new();
    let mut routes: Vec<PlannerRoute> = Vec::new();
    let mut trips: Vec<PlannerTrip> = Vec::new();
    let mut trip_idxs_by_route: Vec<Vec<u32>> = Vec::new();

    for trip in gtfs.trips.values() {
        let mut stop_pattern: Vec<u32> = Vec::new();
        let mut times: Vec<(usize, usize)> = Vec::new();

        for st in &trip.stop_times {
            let arr = st.arrival_time.or(st.departure_time);
            let dep = st.departure_time.or(st.arrival_time);
            let (Some(arr), Some(dep)) = (arr, dep) else {
                stop_pattern.clear();
                times.clear();
                break;
            };

            let Some(stop_idx) = stop_idx_by_id.get(&st.stop.id).copied() else {
                continue;
            };

            stop_pattern.push(stop_idx);
            times.push((arr as usize, dep as usize));
        }

        if stop_pattern.len() < 2 || stop_pattern.len() != times.len() {
            continue;
        }

        let route_key = (trip.route_id.clone(), stop_pattern.clone());
        let route_idx = if let Some(existing) = route_variant_idx.get(&route_key).copied() {
            existing
        } else {
            let route_meta = gtfs
                .routes
                .get(&trip.route_id)
                .ok_or_else(|| format!("Route '{}' not found in GTFS", trip.route_id))?;

            let idx = routes.len() as u32;
            route_variant_idx.insert(route_key, idx);
            routes.push(PlannerRoute {
                base_route_id: trip.route_id.clone(),
                short_name: route_meta
                    .short_name
                    .clone()
                    .unwrap_or_else(|| route_meta.id.clone()),
                long_name: route_meta
                    .long_name
                    .clone()
                    .unwrap_or_else(|| "-".to_string()),
                stops: stop_pattern,
            });
            trip_idxs_by_route.push(Vec::new());
            idx
        };

        let trip_idx = trips.len() as u32;
        trips.push(PlannerTrip { route_idx, times });
        trip_idxs_by_route[route_idx as usize].push(trip_idx);
    }

    // Sort trips per route by first departure for faster earliest-trip scanning.
    for (route_idx, trip_idxs) in trip_idxs_by_route.iter_mut().enumerate() {
        let _ = route_idx;
        trip_idxs.sort_by_key(|trip_idx| {
            let trip = &trips[*trip_idx as usize];
            trip.times.first().map(|t| t.1).unwrap_or(usize::MAX)
        });
    }

    let mut route_stop_pos: Vec<HashMap<u32, usize>> = Vec::with_capacity(routes.len());
    let mut routes_serving_stop: HashMap<u32, Vec<u32>> = HashMap::new();

    for (route_idx, route) in routes.iter().enumerate() {
        let route_idx_u32 = route_idx as u32;
        let mut pos_map: HashMap<u32, usize> = HashMap::new();
        for (pos, stop_idx) in route.stops.iter().copied().enumerate() {
            pos_map.entry(stop_idx).or_insert(pos);
        }

        let mut seen: HashSet<u32> = HashSet::new();
        for stop_idx in &route.stops {
            if seen.insert(*stop_idx) {
                routes_serving_stop
                    .entry(*stop_idx)
                    .or_default()
                    .push(route_idx_u32);
            }
        }

        route_stop_pos.push(pos_map);
    }

    for route_idxs in routes_serving_stop.values_mut() {
        route_idxs.sort();
        route_idxs.dedup();
    }

    let mut footpaths: HashMap<u32, Vec<u32>> = HashMap::new();
    let mut transfer_times: HashMap<(u32, u32), usize> = HashMap::new();

    // Explicit GTFS transfers.
    for stop in gtfs.stops.values() {
        let Some(from_idx) = stop_idx_by_id.get(&stop.id).copied() else {
            continue;
        };
        for tr in &stop.transfers {
            if tr.transfer_type == TransferType::Impossible {
                continue;
            }
            let Some(to_idx) = stop_idx_by_id.get(&tr.to_stop_id).copied() else {
                continue;
            };
            footpaths.entry(from_idx).or_default().push(to_idx);
            transfer_times.insert(
                (from_idx, to_idx),
                tr.min_transfer_time
                    .map(|v| v as usize)
                    .unwrap_or(DEFAULT_TRANSFER_SECONDS),
            );
        }
    }

    // Parent <-> child station links as fallback walk edges.
    for stop in &stops {
        let Some(from_idx) = stop_idx_by_id.get(&stop.id).copied() else {
            continue;
        };
        if let Some(parent_id) = &stop.parent_station {
            if let Some(parent_idx) = stop_idx_by_id.get(parent_id).copied() {
                footpaths.entry(from_idx).or_default().push(parent_idx);
                footpaths.entry(parent_idx).or_default().push(from_idx);
                transfer_times
                    .entry((from_idx, parent_idx))
                    .or_insert(SAME_STATION_WALK_SECONDS);
                transfer_times
                    .entry((parent_idx, from_idx))
                    .or_insert(SAME_STATION_WALK_SECONDS);
            }
        }
    }

    for tos in footpaths.values_mut() {
        tos.sort();
        tos.dedup();
    }

    Ok(PlannerCache {
        stops,
        stop_idx_by_id,
        stop_idxs_by_name_upper,
        stop_idxs_by_code_upper,
        clusters,
        cluster_idx_by_key,
        cluster_idxs_by_name_upper,
        stop_idx_to_cluster_idx,
        routes,
        route_stop_pos,
        trips,
        trip_idxs_by_route,
        routes_serving_stop,
        footpaths,
        transfer_times,
    })
}

fn match_stop_idxs(cache: &PlannerCache, query: &str) -> Vec<u32> {
    let query_upper = query.to_ascii_uppercase();

    if let Some(idx) = cache.stop_idx_by_id.get(query).copied() {
        return vec![idx];
    }
    for (id, idx) in &cache.stop_idx_by_id {
        if id.eq_ignore_ascii_case(query) {
            return vec![*idx];
        }
    }

    if let Some(v) = cache.stop_idxs_by_code_upper.get(&query_upper) {
        return v.clone();
    }

    if let Some(v) = cache.stop_idxs_by_name_upper.get(&query_upper) {
        return v.clone();
    }

    let mut best_name_upper: Option<String> = None;
    let mut best_score = 0.0;
    for name_upper in cache.stop_idxs_by_name_upper.keys() {
        let score = jaro_winkler(&query_upper, name_upper);
        if score > best_score {
            best_score = score;
            best_name_upper = Some(name_upper.clone());
        }
    }

    if best_score >= STOP_FUZZY_THRESHOLD {
        if let Some(name) = best_name_upper {
            return cache
                .stop_idxs_by_name_upper
                .get(&name)
                .cloned()
                .unwrap_or_default();
        }
    }

    Vec::new()
}

fn stop_idxs_to_cluster_idxs(cache: &PlannerCache, stop_idxs: &[u32]) -> Vec<u32> {
    let mut clusters: Vec<u32> = stop_idxs
        .iter()
        .filter_map(|idx| cache.stop_idx_to_cluster_idx.get(*idx as usize).copied())
        .collect();
    clusters.sort();
    clusters.dedup();
    clusters
}

fn match_cluster_idxs(cache: &PlannerCache, query: &str) -> Vec<u32> {
    if let Some(idx) = cache.cluster_idx_by_key.get(query).copied() {
        return vec![idx];
    }
    for (key, idx) in &cache.cluster_idx_by_key {
        if key.eq_ignore_ascii_case(query) {
            return vec![*idx];
        }
    }

    let query_upper = query.to_ascii_uppercase();
    if let Some(v) = cache.cluster_idxs_by_name_upper.get(&query_upper) {
        return v.clone();
    }

    let from_stop_match = match_stop_idxs(cache, query);
    if !from_stop_match.is_empty() {
        return stop_idxs_to_cluster_idxs(cache, &from_stop_match);
    }

    let mut best_name_upper: Option<String> = None;
    let mut best_score = 0.0;
    for name_upper in cache.cluster_idxs_by_name_upper.keys() {
        let score = jaro_winkler(&query_upper, name_upper);
        if score > best_score {
            best_score = score;
            best_name_upper = Some(name_upper.clone());
        }
    }

    if best_score >= STOP_FUZZY_THRESHOLD {
        if let Some(name) = best_name_upper {
            return cache
                .cluster_idxs_by_name_upper
                .get(&name)
                .cloned()
                .unwrap_or_default();
        }
    }

    Vec::new()
}

fn cluster_stop_idxs(cache: &PlannerCache, cluster_idxs: &[u32]) -> Vec<u32> {
    let mut out: Vec<u32> = cluster_idxs
        .iter()
        .flat_map(|cluster_idx| {
            cache.clusters[*cluster_idx as usize]
                .member_stop_idxs
                .iter()
                .copied()
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

fn stop_label(cache: &PlannerCache, stop_idx: u32) -> String {
    let stop = &cache.stops[stop_idx as usize];
    format!("{} ({})", stop.name, stop.id)
}

fn cluster_label(cache: &PlannerCache, cluster_idx: u32) -> String {
    let c = &cache.clusters[cluster_idx as usize];
    format!("{} ({} stop IDs)", c.name, c.member_stop_idxs.len())
}

fn better_journey(candidate: &Journey<u32, u32>, current_best: &Journey<u32, u32>) -> bool {
    match candidate.arrival.cmp(&current_best.arrival) {
        Ordering::Less => true,
        Ordering::Equal => candidate.plan.len() < current_best.plan.len(),
        Ordering::Greater => false,
    }
}

pub fn cmd_route_plan(from_query: &str, to_query: &str) -> Result<(), String> {
    let now = Local::now();
    let query_date = now.date_naive();
    let depart_secs = now.time().num_seconds_from_midnight() as usize;

    let app_cache = load_or_build_app_cache(DEFAULT_GTFS_PATH, DEFAULT_CACHE_PATH)?;
    let cache = &app_cache.planner;

    let from_cluster_idxs = match_cluster_idxs(&cache, from_query);
    if from_cluster_idxs.is_empty() {
        return Err(format!(
            "No origin stop match for '{from_query}'. Use stop id or a close stop name."
        ));
    }

    let to_cluster_idxs = match_cluster_idxs(&cache, to_query);
    if to_cluster_idxs.is_empty() {
        return Err(format!(
            "No destination stop match for '{to_query}'. Use stop id or a close stop name."
        ));
    }
    let from_idxs = cluster_stop_idxs(&cache, &from_cluster_idxs);
    let to_idxs = cluster_stop_idxs(&cache, &to_cluster_idxs);

    let timetable = PlannerTimetable { cache: &cache };

    let mut best: Option<(u32, u32, Journey<u32, u32>)> = None;

    for from_idx in &from_idxs {
        for to_idx in &to_idxs {
            let journeys = timetable.raptor(MAX_TRANSFERS, depart_secs, *from_idx, *to_idx);
            if journeys.is_empty() {
                continue;
            }

            let mut local_best = journeys[0].clone();
            for j in journeys.iter().skip(1) {
                if better_journey(j, &local_best) {
                    local_best = j.clone();
                }
            }

            match &best {
                None => best = Some((*from_idx, *to_idx, local_best)),
                Some((_, _, current_best)) if better_journey(&local_best, current_best) => {
                    best = Some((*from_idx, *to_idx, local_best))
                }
                _ => {}
            }
        }
    }

    let Some((best_from, best_to, best_journey)) = best else {
        return Err(format!(
            "No route found from '{from_query}' to '{to_query}' for {query_date} after {}.",
            format_secs_hhmm(depart_secs)
        ));
    };

    println!("Route plan: '{from_query}' -> '{to_query}'");
    println!("Service day: {query_date}");
    println!("Departure (query time): {}", format_secs_hhmm(depart_secs));
    println!("Arrival: {}", format_secs_hhmm(best_journey.arrival));
    println!(
        "Walking model: GTFS transfers + parent/child station links (OSM walking not configured)"
    );

    println!("\nMatched origin clusters:");
    for cluster_idx in &from_cluster_idxs {
        println!("  - {}", cluster_label(&cache, *cluster_idx));
    }
    println!("Matched destination clusters:");
    for cluster_idx in &to_cluster_idxs {
        println!("  - {}", cluster_label(&cache, *cluster_idx));
    }

    println!("\nChosen stop pair:");
    println!("  from: {}", stop_label(&cache, best_from));
    println!("  to:   {}", stop_label(&cache, best_to));

    if best_journey.plan.is_empty() {
        println!("\nNo transit legs needed (already at destination cluster).\n");
        return Ok(());
    }

    println!("\nItinerary (RAPTOR plan):");
    let mut current_from = best_from;
    for (idx, (route_idx, drop_stop_idx)) in best_journey.plan.iter().enumerate() {
        let route = &cache.routes[*route_idx as usize];
        println!(
            "  {}. Ride {} [{}] {} -> {}",
            idx + 1,
            route.short_name,
            route.base_route_id,
            stop_label(&cache, current_from),
            stop_label(&cache, *drop_stop_idx)
        );
        current_from = *drop_stop_idx;
    }

    Ok(())
}
