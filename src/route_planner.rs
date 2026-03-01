use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{Local, Timelike};
use gtfs_structures::{GtfsReader, TransferType};
use raptor::{Journey, Timetable};
use serde::{Deserialize, Serialize};
use strsim::jaro_winkler;

use crate::build::compute_source_fingerprint;
use crate::cli::DEFAULT_GTFS_PATH;
use crate::clustering::{ClusterStopAccessor, build_stop_clusters};
use crate::snapshot::SourceFingerprint;

const STOP_FUZZY_THRESHOLD: f64 = 0.94;
const MAX_TRANSFERS: usize = 6;
const SAME_STATION_TRANSFER_SECONDS: usize = 120;
const MIN_TRANSFER_BETWEEN_LEGS_SECONDS: usize = 120;
const DEFAULT_TRANSFER_SECONDS: usize = 300;
const PLANNER_CACHE_PATH: &str = "planner.cache.bin";
const PLANNER_CACHE_VERSION: u32 = 1;
const PLANNER_CACHE_DECODE_LIMIT_BYTES: u64 = 1024 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PlannerStop {
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
struct PlannerStation {
    key: String,
    name: String,
    member_stop_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PlannerRoute {
    base_route_id: String,
    short_name: String,
    long_name: String,
    stations: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PlannerTrip {
    route_idx: u32,
    times: Vec<(usize, usize)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PlannerCache {
    version: u32,
    built_unix_secs: u64,
    fingerprint: SourceFingerprint,
    stations: Vec<PlannerStation>,
    station_idx_by_key: HashMap<String, u32>,
    station_idxs_by_name_upper: HashMap<String, Vec<u32>>,
    station_idx_by_stop_id: HashMap<String, u32>,
    station_idxs_by_code_upper: HashMap<String, Vec<u32>>,
    routes: Vec<PlannerRoute>,
    route_station_pos: Vec<HashMap<u32, usize>>,
    trips: Vec<PlannerTrip>,
    trip_idxs_by_route: Vec<Vec<u32>>,
    routes_serving_station: HashMap<u32, Vec<u32>>,
    footpaths: HashMap<u32, Vec<u32>>,
    transfer_times: HashMap<(u32, u32), usize>,
}

impl PlannerCache {
    pub fn stations_count(&self) -> usize {
        self.stations.len()
    }

    pub fn routes_count(&self) -> usize {
        self.routes.len()
    }

    pub fn trips_count(&self) -> usize {
        self.trips.len()
    }
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
                .routes_serving_station
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
        let left_pos = self.cache.route_station_pos[route_idx]
            .get(&left)
            .copied()
            .unwrap_or(usize::MAX);
        let right_pos = self.cache.route_station_pos[route_idx]
            .get(&right)
            .copied()
            .unwrap_or(usize::MAX);
        if left_pos <= right_pos { left } else { right }
    }

    fn get_stops_after(&self, route: Self::Route, stop: Self::Stop) -> Cow<'_, [Self::Stop]> {
        let route_idx = route as usize;
        let pos = self.cache.route_station_pos[route_idx]
            .get(&stop)
            .copied()
            .unwrap_or(0);
        Cow::Owned(self.cache.routes[route_idx].stations[pos..].to_vec())
    }

    fn get_earliest_trip(
        &self,
        route: Self::Route,
        at: raptor::Tau,
        stop: Self::Stop,
    ) -> Option<Self::Trip> {
        let route_idx = route as usize;
        let stop_pos = self.cache.route_station_pos[route_idx]
            .get(&stop)
            .copied()?;

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
        let stop_pos = self.cache.route_station_pos[route_idx][&stop];
        self.cache.trips[trip_idx].times[stop_pos].0
    }

    fn get_departure_time(&self, trip: Self::Trip, stop: Self::Stop) -> raptor::Tau {
        let trip_idx = trip as usize;
        let route_idx = self.cache.trips[trip_idx].route_idx as usize;
        let stop_pos = self.cache.route_station_pos[route_idx][&stop];
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

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
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

fn format_delta_secs(secs: usize) -> String {
    let m = secs / 60;
    let s = secs % 60;
    if m > 0 {
        format!("{m}m {s:02}s")
    } else {
        format!("{s}s")
    }
}

pub(crate) fn build_planner_cache(source_path: &str) -> Result<PlannerCache, String> {
    let gtfs = GtfsReader::default()
        .read_shapes(false)
        .trim_fields(false)
        .read(source_path)
        .map_err(|err| format!("Failed to load GTFS from '{source_path}': {err}"))?;

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

    let clustered_stops = build_stop_clusters(&stops, &stop_idx_by_id);
    let mut stations: Vec<PlannerStation> = Vec::with_capacity(clustered_stops.clusters.len());
    let mut station_idx_by_stop_idx: Vec<u32> = vec![0; stops.len()];
    let mut station_idx_by_stop_id: HashMap<String, u32> = HashMap::new();
    let mut station_idxs_by_code_upper: HashMap<String, Vec<u32>> = HashMap::new();

    for (station_idx, cluster) in clustered_stops.clusters.iter().enumerate() {
        let mut member_stop_ids: Vec<String> = cluster
            .member_stop_idxs
            .iter()
            .filter_map(|idx| stops.get(*idx as usize).map(|s| s.id.clone()))
            .collect();
        member_stop_ids.sort();
        member_stop_ids.dedup();

        let station_idx_u32 = station_idx as u32;
        for stop_idx in &cluster.member_stop_idxs {
            station_idx_by_stop_idx[*stop_idx as usize] = station_idx_u32;
        }
        for stop_id in &member_stop_ids {
            station_idx_by_stop_id.insert(stop_id.clone(), station_idx_u32);
        }
        for stop_idx in &cluster.member_stop_idxs {
            if let Some(code) = stops[*stop_idx as usize].code.as_ref() {
                station_idxs_by_code_upper
                    .entry(code.to_ascii_uppercase())
                    .or_default()
                    .push(station_idx_u32);
            }
        }

        stations.push(PlannerStation {
            key: cluster.key.clone(),
            name: cluster.name.clone(),
            member_stop_ids,
        });
    }

    for station_idxs in station_idxs_by_code_upper.values_mut() {
        station_idxs.sort();
        station_idxs.dedup();
    }

    let station_idx_by_key = clustered_stops.cluster_idx_by_key;
    let station_idxs_by_name_upper = clustered_stops.cluster_idxs_by_name_upper;

    let mut route_variant_idx: HashMap<(String, Vec<u32>), u32> = HashMap::new();
    let mut routes: Vec<PlannerRoute> = Vec::new();
    let mut trips: Vec<PlannerTrip> = Vec::new();
    let mut trip_idxs_by_route: Vec<Vec<u32>> = Vec::new();

    for trip in gtfs.trips.values() {
        let mut station_pattern: Vec<u32> = Vec::new();
        let mut times: Vec<(usize, usize)> = Vec::new();

        for st in &trip.stop_times {
            let arr = st.arrival_time.or(st.departure_time);
            let dep = st.departure_time.or(st.arrival_time);
            let (Some(arr), Some(dep)) = (arr, dep) else {
                station_pattern.clear();
                times.clear();
                break;
            };

            let Some(stop_idx) = stop_idx_by_id.get(&st.stop.id).copied() else {
                continue;
            };
            let station_idx = station_idx_by_stop_idx[stop_idx as usize];
            let arr = arr as usize;
            let dep = dep as usize;

            if let Some(last_station) = station_pattern.last().copied() {
                if last_station == station_idx {
                    if let Some(last_times) = times.last_mut() {
                        last_times.0 = last_times.0.min(arr);
                        last_times.1 = last_times.1.max(dep);
                    }
                    continue;
                }
            }

            station_pattern.push(station_idx);
            times.push((arr, dep));
        }

        if station_pattern.len() < 2 || station_pattern.len() != times.len() {
            continue;
        }

        let route_key = (trip.route_id.clone(), station_pattern.clone());
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
                stations: station_pattern,
            });
            trip_idxs_by_route.push(Vec::new());
            idx
        };

        let trip_idx = trips.len() as u32;
        trips.push(PlannerTrip { route_idx, times });
        trip_idxs_by_route[route_idx as usize].push(trip_idx);
    }

    for trip_idxs in &mut trip_idxs_by_route {
        trip_idxs.sort_by_key(|trip_idx| {
            let trip = &trips[*trip_idx as usize];
            trip.times.first().map(|t| t.1).unwrap_or(usize::MAX)
        });
    }

    let mut route_station_pos: Vec<HashMap<u32, usize>> = Vec::with_capacity(routes.len());
    let mut routes_serving_station: HashMap<u32, Vec<u32>> = HashMap::new();

    for (route_idx, route) in routes.iter().enumerate() {
        let route_idx_u32 = route_idx as u32;
        let mut pos_map: HashMap<u32, usize> = HashMap::new();
        for (pos, station_idx) in route.stations.iter().copied().enumerate() {
            pos_map.entry(station_idx).or_insert(pos);
        }

        let mut seen = HashSet::new();
        for station_idx in &route.stations {
            if seen.insert(*station_idx) {
                routes_serving_station
                    .entry(*station_idx)
                    .or_default()
                    .push(route_idx_u32);
            }
        }

        route_station_pos.push(pos_map);
    }

    for route_idxs in routes_serving_station.values_mut() {
        route_idxs.sort();
        route_idxs.dedup();
    }

    let mut footpaths: HashMap<u32, Vec<u32>> = HashMap::new();
    let mut transfer_times: HashMap<(u32, u32), usize> = HashMap::new();

    // Explicit GTFS transfers mapped stop->station.
    for stop in gtfs.stops.values() {
        let Some(from_stop_idx) = stop_idx_by_id.get(&stop.id).copied() else {
            continue;
        };
        let from_station_idx = station_idx_by_stop_idx[from_stop_idx as usize];
        for tr in &stop.transfers {
            if tr.transfer_type == TransferType::Impossible {
                continue;
            }
            let Some(to_stop_idx) = stop_idx_by_id.get(&tr.to_stop_id).copied() else {
                continue;
            };
            let to_station_idx = station_idx_by_stop_idx[to_stop_idx as usize];
            if from_station_idx == to_station_idx {
                continue;
            }
            let secs = tr
                .min_transfer_time
                .map(|v| v as usize)
                .unwrap_or(DEFAULT_TRANSFER_SECONDS);
            footpaths
                .entry(from_station_idx)
                .or_default()
                .push(to_station_idx);
            transfer_times
                .entry((from_station_idx, to_station_idx))
                .and_modify(|cur| {
                    if secs < *cur {
                        *cur = secs;
                    }
                })
                .or_insert(secs);
        }
    }

    // Keep station interchange possible by adding self-footpath.
    for station_idx in 0..stations.len() as u32 {
        footpaths.entry(station_idx).or_default().push(station_idx);
        transfer_times
            .entry((station_idx, station_idx))
            .or_insert(SAME_STATION_TRANSFER_SECONDS);
    }

    for tos in footpaths.values_mut() {
        tos.sort();
        tos.dedup();
    }

    let fingerprint = compute_source_fingerprint(source_path)?;
    Ok(PlannerCache {
        version: PLANNER_CACHE_VERSION,
        built_unix_secs: now_unix_secs(),
        fingerprint,
        stations,
        station_idx_by_key,
        station_idxs_by_name_upper,
        station_idx_by_stop_id,
        station_idxs_by_code_upper,
        routes,
        route_station_pos,
        trips,
        trip_idxs_by_route,
        routes_serving_station,
        footpaths,
        transfer_times,
    })
}

fn save_planner_cache(path: &str, cache: &PlannerCache) -> Result<(), String> {
    let file = File::create(path)
        .map_err(|err| format!("Failed to create planner cache '{path}': {err}"))?;
    let mut writer = BufWriter::new(file);
    bincode::serialize_into(&mut writer, cache)
        .map_err(|err| format!("Failed to serialize planner cache '{path}': {err}"))
}

fn load_planner_cache(path: &str) -> Result<PlannerCache, String> {
    let file_size = std::fs::metadata(Path::new(path))
        .map(|m| m.len())
        .map_err(|err| format!("Failed to read planner cache metadata '{path}': {err}"))?;
    if file_size > PLANNER_CACHE_DECODE_LIMIT_BYTES {
        return Err(format!(
            "Planner cache '{path}' is too large to load safely ({} bytes > {} bytes)",
            file_size, PLANNER_CACHE_DECODE_LIMIT_BYTES
        ));
    }

    let file =
        File::open(path).map_err(|err| format!("Failed to open planner cache '{path}': {err}"))?;
    let mut reader = BufReader::new(file);
    bincode::deserialize_from(&mut reader)
        .map_err(|err| format!("Failed to deserialize planner cache '{path}': {err}"))
}

fn planner_cache_fresh(cache: &PlannerCache, source_path: &str) -> Result<bool, String> {
    if cache.version != PLANNER_CACHE_VERSION {
        return Ok(false);
    }
    let current = compute_source_fingerprint(source_path)?;
    Ok(cache.fingerprint == current)
}

fn load_or_build_planner_cache(source_path: &str) -> Result<PlannerCache, String> {
    if let Ok(cache) = load_planner_cache(PLANNER_CACHE_PATH) {
        if planner_cache_fresh(&cache, source_path)? {
            return Ok(cache);
        }
    }
    rebuild_planner_cache(source_path)
}

pub fn rebuild_planner_cache(source_path: &str) -> Result<PlannerCache, String> {
    let cache = build_planner_cache(source_path)?;
    save_planner_cache(PLANNER_CACHE_PATH, &cache)?;
    Ok(cache)
}

fn match_station_idxs(cache: &PlannerCache, query: &str) -> Vec<u32> {
    if let Some(idx) = cache.station_idx_by_key.get(query).copied() {
        return vec![idx];
    }
    for (key, idx) in &cache.station_idx_by_key {
        if key.eq_ignore_ascii_case(query) {
            return vec![*idx];
        }
    }

    if let Some(idx) = cache.station_idx_by_stop_id.get(query).copied() {
        return vec![idx];
    }
    for (stop_id, idx) in &cache.station_idx_by_stop_id {
        if stop_id.eq_ignore_ascii_case(query) {
            return vec![*idx];
        }
    }

    let query_upper = query.to_ascii_uppercase();
    if let Some(v) = cache.station_idxs_by_code_upper.get(&query_upper) {
        return v.clone();
    }
    if let Some(v) = cache.station_idxs_by_name_upper.get(&query_upper) {
        return v.clone();
    }

    let mut best_name_upper: Option<String> = None;
    let mut best_score = 0.0;
    for name_upper in cache.station_idxs_by_name_upper.keys() {
        let score = jaro_winkler(&query_upper, name_upper);
        if score > best_score {
            best_score = score;
            best_name_upper = Some(name_upper.clone());
        }
    }
    if best_score >= STOP_FUZZY_THRESHOLD {
        if let Some(name) = best_name_upper {
            return cache
                .station_idxs_by_name_upper
                .get(&name)
                .cloned()
                .unwrap_or_default();
        }
    }

    Vec::new()
}

fn station_label_debug(cache: &PlannerCache, station_idx: u32) -> String {
    let station = &cache.stations[station_idx as usize];
    let preview: Vec<&str> = station
        .member_stop_ids
        .iter()
        .take(3)
        .map(|s| s.as_str())
        .collect();
    let suffix = if station.member_stop_ids.len() > 3 {
        format!(" +{} more", station.member_stop_ids.len() - 3)
    } else {
        String::new()
    };
    format!("{} [{}{}]", station.name, preview.join(", "), suffix)
}

fn station_label(cache: &PlannerCache, station_idx: u32, debug: bool) -> String {
    if debug {
        station_label_debug(cache, station_idx)
    } else {
        cache.stations[station_idx as usize].name.clone()
    }
}

fn better_journey(candidate: &Journey<u32, u32>, current_best: &Journey<u32, u32>) -> bool {
    match candidate.arrival.cmp(&current_best.arrival) {
        Ordering::Less => true,
        Ordering::Equal => candidate.plan.len() < current_best.plan.len(),
        Ordering::Greater => false,
    }
}

#[derive(Debug, Clone)]
struct LegTiming {
    route_idx: u32,
    from_station_idx: u32,
    to_station_idx: u32,
    departure: usize,
    arrival: usize,
    stops_count: usize,
}

fn compute_stops_count(cache: &PlannerCache, route_idx: u32, from: u32, to: u32) -> usize {
    let map = &cache.route_station_pos[route_idx as usize];
    let from_pos = map.get(&from).copied().unwrap_or(0);
    let to_pos = map.get(&to).copied().unwrap_or(from_pos);
    to_pos.saturating_sub(from_pos)
}

fn build_leg_timings(
    cache: &PlannerCache,
    start_station_idx: u32,
    journey: &Journey<u32, u32>,
    depart_secs: usize,
    min_transfer_between_legs_secs: usize,
) -> Result<Vec<LegTiming>, String> {
    let timetable = PlannerTimetable { cache };
    let mut out = Vec::new();
    let mut current_from = start_station_idx;
    let mut earliest_departure = depart_secs;

    for (route_idx, drop_station_idx) in &journey.plan {
        let Some(trip_idx) =
            timetable.get_earliest_trip(*route_idx, earliest_departure, current_from)
        else {
            return Err(format!(
                "Cannot reconstruct leg timing for route {} from {}.",
                route_idx,
                station_label_debug(cache, current_from)
            ));
        };

        let departure = timetable.get_departure_time(trip_idx, current_from);
        let arrival = timetable.get_arrival_time(trip_idx, *drop_station_idx);
        let stops_count = compute_stops_count(cache, *route_idx, current_from, *drop_station_idx);

        out.push(LegTiming {
            route_idx: *route_idx,
            from_station_idx: current_from,
            to_station_idx: *drop_station_idx,
            departure,
            arrival,
            stops_count,
        });

        current_from = *drop_station_idx;
        earliest_departure = arrival.saturating_add(min_transfer_between_legs_secs);
    }

    Ok(out)
}

fn evaluate_journey_arrival_with_transfer_slack(
    cache: &PlannerCache,
    start_station_idx: u32,
    journey: &Journey<u32, u32>,
    depart_secs: usize,
) -> Option<usize> {
    let legs = build_leg_timings(
        cache,
        start_station_idx,
        journey,
        depart_secs,
        MIN_TRANSFER_BETWEEN_LEGS_SECONDS,
    )
    .ok()?;
    Some(legs.last().map(|l| l.arrival).unwrap_or(depart_secs))
}

fn print_journey(cache: &PlannerCache, leg_timings: &[LegTiming], debug: bool) {
    for (idx, leg) in leg_timings.iter().enumerate() {
        let route = &cache.routes[leg.route_idx as usize];
        let from_label = station_label(cache, leg.from_station_idx, debug);
        let to_label = station_label(cache, leg.to_station_idx, debug);
        println!(
            "  {}. Ride {} [{}] {} -> {}",
            idx + 1,
            route.short_name,
            route.base_route_id,
            from_label,
            to_label
        );
        println!(
            "     dep {} | arr {} | {} stops",
            format_secs_hhmm(leg.departure),
            format_secs_hhmm(leg.arrival),
            leg.stops_count
        );

        if leg_timings.get(idx + 1).is_some() {
            println!(
                "     transfer at {}",
                station_label(cache, leg.to_station_idx, debug)
            );
        }
    }
}

fn print_journey_compact(
    cache: &PlannerCache,
    start_station_idx: u32,
    journey: &Journey<u32, u32>,
) {
    let mut current_from = start_station_idx;
    for (idx, (route_idx, drop_station_idx)) in journey.plan.iter().enumerate() {
        let route = &cache.routes[*route_idx as usize];
        println!(
            "  {}. Ride {} [{}] {} -> {}",
            idx + 1,
            route.short_name,
            route.base_route_id,
            station_label_debug(cache, current_from),
            station_label_debug(cache, *drop_station_idx)
        );
        current_from = *drop_station_idx;
    }
}

#[derive(Debug, Clone)]
struct EvaluatedPair {
    from_idx: u32,
    to_idx: u32,
    pareto_count: usize,
    best_journey: Option<Journey<u32, u32>>,
}

#[derive(Debug, Clone)]
struct CandidateJourney {
    from_idx: u32,
    to_idx: u32,
    journey: Journey<u32, u32>,
    adjusted_arrival: usize,
}

pub fn cmd_route_plan(
    from_query: &str,
    to_query: &str,
    debug: bool,
    alternatives: usize,
) -> Result<(), String> {
    let now = Local::now();
    let query_date = now.date_naive();
    let depart_secs = now.time().num_seconds_from_midnight() as usize;

    let cache = load_or_build_planner_cache(DEFAULT_GTFS_PATH)?;

    let from_station_idxs = match_station_idxs(&cache, from_query);
    if from_station_idxs.is_empty() {
        return Err(format!(
            "No origin stop match for '{from_query}'. Use stop id or a close stop name."
        ));
    }
    let to_station_idxs = match_station_idxs(&cache, to_query);
    if to_station_idxs.is_empty() {
        return Err(format!(
            "No destination stop match for '{to_query}'. Use stop id or a close stop name."
        ));
    }

    let timetable = PlannerTimetable { cache: &cache };
    let mut best: Option<(u32, u32, Journey<u32, u32>, usize)> = None;
    let mut pair_stats: Vec<EvaluatedPair> = Vec::new();
    let mut candidates: Vec<CandidateJourney> = Vec::new();

    for from_idx in &from_station_idxs {
        for to_idx in &to_station_idxs {
            let journeys = timetable.raptor(MAX_TRANSFERS, depart_secs, *from_idx, *to_idx);
            if journeys.is_empty() {
                if debug {
                    pair_stats.push(EvaluatedPair {
                        from_idx: *from_idx,
                        to_idx: *to_idx,
                        pareto_count: 0,
                        best_journey: None,
                    });
                }
                continue;
            }

            let mut local_best: Option<(Journey<u32, u32>, usize)> = None;
            for journey in journeys {
                let Some(adjusted_arrival) = evaluate_journey_arrival_with_transfer_slack(
                    &cache,
                    *from_idx,
                    &journey,
                    depart_secs,
                ) else {
                    continue;
                };

                if debug {
                    candidates.push(CandidateJourney {
                        from_idx: *from_idx,
                        to_idx: *to_idx,
                        journey: journey.clone(),
                        adjusted_arrival,
                    });
                }

                match &local_best {
                    None => local_best = Some((journey, adjusted_arrival)),
                    Some((current_best_journey, current_adjusted_arrival)) => {
                        if adjusted_arrival < *current_adjusted_arrival
                            || (adjusted_arrival == *current_adjusted_arrival
                                && better_journey(&journey, current_best_journey))
                        {
                            local_best = Some((journey, adjusted_arrival));
                        }
                    }
                }
            }

            let Some((local_best, local_best_adjusted_arrival)) = local_best else {
                if debug {
                    pair_stats.push(EvaluatedPair {
                        from_idx: *from_idx,
                        to_idx: *to_idx,
                        pareto_count: 0,
                        best_journey: None,
                    });
                }
                continue;
            };

            if debug {
                pair_stats.push(EvaluatedPair {
                    from_idx: *from_idx,
                    to_idx: *to_idx,
                    pareto_count: 1,
                    best_journey: Some(local_best.clone()),
                });
            }

            match &best {
                None => best = Some((*from_idx, *to_idx, local_best, local_best_adjusted_arrival)),
                Some((_, _, current_best, current_adjusted_arrival))
                    if local_best_adjusted_arrival < *current_adjusted_arrival
                        || (local_best_adjusted_arrival == *current_adjusted_arrival
                            && better_journey(&local_best, current_best)) =>
                {
                    best = Some((*from_idx, *to_idx, local_best, local_best_adjusted_arrival))
                }
                _ => {}
            }
        }
    }

    let Some((best_from, best_to, best_journey, best_adjusted_arrival)) = best else {
        if debug {
            let reachable = pair_stats.iter().filter(|p| p.pareto_count > 0).count();
            let unreachable = pair_stats.iter().filter(|p| p.pareto_count == 0).count();
            println!("Route plan debug: '{from_query}' -> '{to_query}'");
            println!(
                "  evaluated station pairs: {} | reachable: {} | unreachable: {}",
                pair_stats.len(),
                reachable,
                unreachable
            );
        }
        return Err(format!(
            "No route found from '{from_query}' to '{to_query}' for {query_date} after {}.",
            format_secs_hhmm(depart_secs)
        ));
    };

    println!("Route plan: '{from_query}' -> '{to_query}'");
    println!("Service day: {query_date}");
    println!("Departure (query time): {}", format_secs_hhmm(depart_secs));
    println!("Arrival: {}", format_secs_hhmm(best_adjusted_arrival));
    if debug {
        println!("Model: station-normalized planning (hybrid stop->station cache)");
        println!(
            "Minimum transfer time between legs: {}",
            format_delta_secs(MIN_TRANSFER_BETWEEN_LEGS_SECONDS)
        );

        println!("\nMatched origin stations:");
        for station_idx in &from_station_idxs {
            println!("  - {}", station_label(&cache, *station_idx, debug));
        }
        println!("Matched destination stations:");
        for station_idx in &to_station_idxs {
            println!("  - {}", station_label(&cache, *station_idx, debug));
        }

        println!("\nChosen station pair:");
        println!("  from: {}", station_label(&cache, best_from, debug));
        println!("  to:   {}", station_label(&cache, best_to, debug));
    }

    if debug {
        let reachable = pair_stats.iter().filter(|p| p.pareto_count > 0).count();
        let unreachable = pair_stats.iter().filter(|p| p.pareto_count == 0).count();
        println!("\nDebug summary:");
        println!(
            "  evaluated station pairs: {} | reachable: {} | unreachable: {}",
            pair_stats.len(),
            reachable,
            unreachable
        );

        let mut best_pairs: Vec<&EvaluatedPair> = pair_stats
            .iter()
            .filter(|p| p.best_journey.is_some())
            .collect();
        best_pairs.sort_by(|a, b| {
            let a_best = a.best_journey.as_ref().expect("filtered");
            let b_best = b.best_journey.as_ref().expect("filtered");
            a_best
                .arrival
                .cmp(&b_best.arrival)
                .then(a_best.plan.len().cmp(&b_best.plan.len()))
        });

        if !best_pairs.is_empty() {
            println!("  top reachable station pairs:");
            for pair in best_pairs.into_iter().take(5) {
                let j = pair.best_journey.as_ref().expect("exists");
                println!(
                    "    - {} -> {} | arrival {} | legs {} | pareto {}",
                    station_label_debug(&cache, pair.from_idx),
                    station_label_debug(&cache, pair.to_idx),
                    format_secs_hhmm(j.arrival),
                    j.plan.len(),
                    pair.pareto_count
                );
            }
        }
    }

    if best_journey.plan.is_empty() {
        println!("\nNo transit legs needed (already at destination station).\n");
        return Ok(());
    }

    println!("\nItinerary (RAPTOR plan):");
    match build_leg_timings(
        &cache,
        best_from,
        &best_journey,
        depart_secs,
        MIN_TRANSFER_BETWEEN_LEGS_SECONDS,
    ) {
        Ok(legs) => print_journey(&cache, &legs, debug),
        Err(_) => print_journey_compact(&cache, best_from, &best_journey),
    }

    if debug && !candidates.is_empty() {
        candidates.sort_by(|a, b| {
            a.adjusted_arrival
                .cmp(&b.adjusted_arrival)
                .then(a.journey.arrival.cmp(&b.journey.arrival))
                .then(a.journey.plan.len().cmp(&b.journey.plan.len()))
        });

        let best_journey_signature = (
            best_from,
            best_to,
            best_journey.plan.clone(),
            best_adjusted_arrival,
        );

        let mut shown = 0usize;
        println!("\nAlternatives (why not picked):");
        for alt in candidates {
            let is_same_as_best = (
                alt.from_idx,
                alt.to_idx,
                alt.journey.plan.clone(),
                alt.adjusted_arrival,
            ) == best_journey_signature;
            if is_same_as_best {
                continue;
            }

            shown += 1;
            let delay = alt.adjusted_arrival.saturating_sub(best_adjusted_arrival);
            println!(
                "  Alternative {}: arrival {} ({} later), legs {}",
                shown,
                format_secs_hhmm(alt.adjusted_arrival),
                format_delta_secs(delay),
                alt.journey.plan.len()
            );
            println!(
                "    pair: {} -> {}",
                station_label_debug(&cache, alt.from_idx),
                station_label_debug(&cache, alt.to_idx)
            );

            match build_leg_timings(
                &cache,
                alt.from_idx,
                &alt.journey,
                depart_secs,
                MIN_TRANSFER_BETWEEN_LEGS_SECONDS,
            ) {
                Ok(legs) => print_journey(&cache, &legs, true),
                Err(_) => print_journey_compact(&cache, alt.from_idx, &alt.journey),
            }

            if shown >= alternatives {
                break;
            }
        }
        if shown == 0 {
            println!("  (No additional alternatives found.)");
        }
    }

    Ok(())
}
