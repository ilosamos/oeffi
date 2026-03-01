use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::Datelike;
use gtfs_structures::{Exception, GtfsReader, TransferType};

use crate::build::compute_source_fingerprint;
use crate::cache_meta::fingerprint_is_fresh;
use crate::clustering::{ClusterStopAccessor, build_stop_clusters};

use super::model::{
    DEFAULT_TRANSFER_SECONDS, MIN_TRANSFER_SECONDS, PLANNER_CACHE_DECODE_LIMIT_BYTES,
    PLANNER_CACHE_PATH, PLANNER_CACHE_VERSION, PlannerCache, PlannerRoute, PlannerServiceCalendar,
    PlannerStation, PlannerStop, PlannerTrip,
};

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

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
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

    let mut service_ids: Vec<String> = Vec::new();
    let mut service_idx_by_id: HashMap<String, u32> = HashMap::new();
    let mut ensure_service_idx = |service_id: &str| -> u32 {
        if let Some(idx) = service_idx_by_id.get(service_id).copied() {
            return idx;
        }
        let idx = service_ids.len() as u32;
        service_ids.push(service_id.to_string());
        service_idx_by_id.insert(service_id.to_string(), idx);
        idx
    };

    for calendar in gtfs.calendar.values() {
        ensure_service_idx(&calendar.id);
    }
    for service_id in gtfs.calendar_dates.keys() {
        ensure_service_idx(service_id);
    }

    let mut service_calendars: HashMap<u32, PlannerServiceCalendar> = HashMap::new();
    for calendar in gtfs.calendar.values() {
        let service_idx = ensure_service_idx(&calendar.id);
        let mut weekday_mask = 0u8;
        if calendar.monday {
            weekday_mask |= 1 << 0;
        }
        if calendar.tuesday {
            weekday_mask |= 1 << 1;
        }
        if calendar.wednesday {
            weekday_mask |= 1 << 2;
        }
        if calendar.thursday {
            weekday_mask |= 1 << 3;
        }
        if calendar.friday {
            weekday_mask |= 1 << 4;
        }
        if calendar.saturday {
            weekday_mask |= 1 << 5;
        }
        if calendar.sunday {
            weekday_mask |= 1 << 6;
        }

        let start_date_yyyymmdd = calendar.start_date.year() * 10_000
            + calendar.start_date.month() as i32 * 100
            + calendar.start_date.day() as i32;
        let end_date_yyyymmdd = calendar.end_date.year() * 10_000
            + calendar.end_date.month() as i32 * 100
            + calendar.end_date.day() as i32;

        service_calendars.insert(
            service_idx,
            PlannerServiceCalendar {
                weekday_mask,
                start_date_yyyymmdd,
                end_date_yyyymmdd,
            },
        );
    }

    let mut services_added_by_date: HashMap<i32, Vec<u32>> = HashMap::new();
    let mut services_removed_by_date: HashMap<i32, Vec<u32>> = HashMap::new();
    for calendar_dates in gtfs.calendar_dates.values() {
        for calendar_date in calendar_dates {
            let service_idx = ensure_service_idx(&calendar_date.service_id);
            let date_key = calendar_date.date.year() * 10_000
                + calendar_date.date.month() as i32 * 100
                + calendar_date.date.day() as i32;
            match calendar_date.exception_type {
                Exception::Added => services_added_by_date
                    .entry(date_key)
                    .or_default()
                    .push(service_idx),
                Exception::Deleted => services_removed_by_date
                    .entry(date_key)
                    .or_default()
                    .push(service_idx),
            }
        }
    }
    for service_idxs in services_added_by_date.values_mut() {
        service_idxs.sort();
        service_idxs.dedup();
    }
    for service_idxs in services_removed_by_date.values_mut() {
        service_idxs.sort();
        service_idxs.dedup();
    }

    let mut route_variant_idx: HashMap<(String, Vec<u32>), u32> = HashMap::new();
    let mut routes: Vec<PlannerRoute> = Vec::new();
    let mut trips: Vec<PlannerTrip> = Vec::new();
    let mut trip_idxs_by_route: Vec<Vec<u32>> = Vec::new();

    for trip in gtfs.trips.values() {
        let service_idx = ensure_service_idx(&trip.service_id);
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
        trips.push(PlannerTrip {
            route_idx,
            service_idx,
            times,
        });
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

    for station_idx in 0..stations.len() as u32 {
        footpaths.entry(station_idx).or_default().push(station_idx);
        transfer_times
            .entry((station_idx, station_idx))
            .or_insert(MIN_TRANSFER_SECONDS);
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
        service_ids,
        service_calendars,
        services_added_by_date,
        services_removed_by_date,
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
    fingerprint_is_fresh(&cache.fingerprint, source_path)
}

pub fn load_or_build_planner_cache(source_path: &str) -> Result<PlannerCache, String> {
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
