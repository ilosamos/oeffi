use std::cmp::Ordering;

use crate::matcher::{GENERIC_QUERY_TOKENS, exact_key_case_insensitive, match_name_candidates};
use chrono::{Local, NaiveDate, Timelike};
use raptor::Journey;
use raptor::Timetable;

use super::model::{
    EvaluatedPair, LegTiming, MAX_TRANSFERS, MIN_TRANSFER_SECONDS, PlannerCache, RouteOption,
    RoutePlanResult, STOP_FUZZY_THRESHOLD,
};
use super::raptor_adapter::PlannerTimetable;

const EARTH_RADIUS_M: f64 = 6_371_000.0;
const WALK_METERS_PER_SECOND: f64 = 1.2;
const MAX_COORD_CANDIDATES: usize = 12;
const MAX_ACCESS_DISTANCE_METERS: f64 = 1_200.0;
const TRANSFER_PENALTY_SECONDS: usize = 240;

#[derive(Debug, Clone)]
struct StationCandidate {
    station_idx: u32,
    walk_secs: usize,
    distance_meters: f64,
}

fn better_journey(candidate: &Journey<u32, u32>, current_best: &Journey<u32, u32>) -> bool {
    match candidate.arrival.cmp(&current_best.arrival) {
        Ordering::Less => true,
        Ordering::Equal => candidate.plan.len() < current_best.plan.len(),
        Ordering::Greater => false,
    }
}

fn haversine_meters(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let dlat = (lat2 - lat1).to_radians();
    let dlon = (lon2 - lon1).to_radians();
    let lat1 = lat1.to_radians();
    let lat2 = lat2.to_radians();

    let a = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());
    EARTH_RADIUS_M * c
}

fn walk_secs_for_distance(distance_meters: f64) -> usize {
    (distance_meters / WALK_METERS_PER_SECOND).ceil() as usize
}

fn nearest_station_candidates(
    cache: &PlannerCache,
    lat: f64,
    lon: f64,
    max_results: usize,
    max_distance_meters: f64,
) -> Vec<StationCandidate> {
    let mut candidates: Vec<StationCandidate> = cache
        .stations
        .iter()
        .enumerate()
        .filter_map(|(idx, station)| {
            let (Some(st_lat), Some(st_lon)) = (station.centroid_lat, station.centroid_lon) else {
                return None;
            };
            let distance_meters = haversine_meters(lat, lon, st_lat, st_lon);
            if distance_meters > max_distance_meters {
                return None;
            }
            Some(StationCandidate {
                station_idx: idx as u32,
                walk_secs: walk_secs_for_distance(distance_meters),
                distance_meters,
            })
        })
        .collect();

    candidates.sort_by(|a, b| {
        a.walk_secs
            .cmp(&b.walk_secs)
            .then_with(|| a.distance_meters.total_cmp(&b.distance_meters))
            .then(a.station_idx.cmp(&b.station_idx))
    });
    candidates.truncate(max_results);
    candidates
}

pub fn match_station_idxs(cache: &PlannerCache, query: &str) -> Vec<u32> {
    if let Some(idx) = cache.station_idx_by_key.get(query).copied() {
        return vec![idx];
    }
    if let Some((_, idx)) = exact_key_case_insensitive(&cache.station_idx_by_key, query) {
        return vec![*idx];
    }

    if let Some(idx) = cache.station_idx_by_stop_id.get(query).copied() {
        return vec![idx];
    }
    if let Some((_, idx)) = exact_key_case_insensitive(&cache.station_idx_by_stop_id, query) {
        return vec![*idx];
    }

    let query_upper = query.to_ascii_uppercase();
    if let Some(v) = cache.station_idxs_by_code_upper.get(&query_upper) {
        return v.clone();
    }
    let (matches, _) = match_name_candidates(
        &cache.station_idxs_by_name_upper,
        query,
        STOP_FUZZY_THRESHOLD,
        &GENERIC_QUERY_TOKENS,
        24,
    );
    if !matches.is_empty() {
        return matches;
    }

    Vec::new()
}

fn compute_stops_count(cache: &PlannerCache, route_idx: u32, from: u32, to: u32) -> usize {
    let map = &cache.route_station_pos[route_idx as usize];
    let from_pos = map.get(&from).copied().unwrap_or(0);
    let to_pos = map.get(&to).copied().unwrap_or(from_pos);
    to_pos.saturating_sub(from_pos)
}

pub fn build_leg_timings(
    cache: &PlannerCache,
    start_station_idx: u32,
    journey: &Journey<u32, u32>,
    depart_secs: usize,
    min_transfer_between_legs_secs: usize,
    active_trips: Option<&[bool]>,
) -> Result<Vec<LegTiming>, String> {
    let timetable = PlannerTimetable {
        cache,
        active_trips,
    };
    let mut out = Vec::new();
    let mut current_from = start_station_idx;
    let mut earliest_departure = depart_secs;

    for (route_idx, drop_station_idx) in &journey.plan {
        let Some(trip_idx) =
            timetable.get_earliest_trip(*route_idx, earliest_departure, current_from)
        else {
            return Err(format!(
                "Cannot reconstruct leg timing for route {} from station {}.",
                route_idx, current_from
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
    active_trips: Option<&[bool]>,
) -> Option<(usize, Vec<LegTiming>)> {
    let legs = build_leg_timings(
        cache,
        start_station_idx,
        journey,
        depart_secs,
        MIN_TRANSFER_SECONDS,
        active_trips,
    )
    .ok()?;
    let arrival = legs.last().map(|l| l.arrival).unwrap_or(depart_secs);
    Some((arrival, legs))
}

fn generalized_cost(
    depart_secs: usize,
    arrival_secs: usize,
    access_secs: usize,
    egress_secs: usize,
    legs_count: usize,
) -> usize {
    let in_vehicle_and_wait = arrival_secs.saturating_sub(depart_secs);
    let total_walk = access_secs.saturating_add(egress_secs);
    let walk_penalty = total_walk.saturating_mul(8) / 10; // 1.8x walk disutility in total.
    let transfer_penalty = legs_count
        .saturating_sub(1)
        .saturating_mul(TRANSFER_PENALTY_SECONDS);

    in_vehicle_and_wait
        .saturating_add(walk_penalty)
        .saturating_add(transfer_penalty)
}

pub fn plan_route(
    cache: &PlannerCache,
    from_query: &str,
    to_query: &str,
    alternatives: usize,
    depart_secs_override: Option<usize>,
    query_date_override: Option<NaiveDate>,
) -> Result<RoutePlanResult, String> {
    let now = Local::now();
    let query_date = query_date_override.unwrap_or(now.date_naive());
    let depart_secs = match (depart_secs_override, query_date_override) {
        (Some(dep), _) => dep,
        (None, Some(_)) => 0,
        (None, None) => now.time().num_seconds_from_midnight() as usize,
    };

    let from_station_idxs = match_station_idxs(cache, from_query);
    if from_station_idxs.is_empty() {
        return Err(format!(
            "No origin stop match for '{from_query}'. Use stop id or a close stop name."
        ));
    }
    let to_station_idxs = match_station_idxs(cache, to_query);
    if to_station_idxs.is_empty() {
        return Err(format!(
            "No destination stop match for '{to_query}'. Use stop id or a close stop name."
        ));
    }

    let active_trips = cache.active_trip_mask_for_date(query_date);
    let timetable = PlannerTimetable {
        cache,
        active_trips: Some(&active_trips),
    };
    let mut best: Option<RouteOption> = None;
    let mut pair_stats: Vec<EvaluatedPair> = Vec::new();
    let mut all_options: Vec<RouteOption> = Vec::new();

    for from_idx in &from_station_idxs {
        for to_idx in &to_station_idxs {
            let journeys = timetable.raptor(MAX_TRANSFERS, depart_secs, *from_idx, *to_idx);
            if journeys.is_empty() {
                pair_stats.push(EvaluatedPair {
                    from_idx: *from_idx,
                    to_idx: *to_idx,
                    pareto_count: 0,
                    best_arrival: None,
                    legs_count: None,
                });
                continue;
            }

            let mut local_best: Option<(Journey<u32, u32>, usize, Vec<LegTiming>)> = None;
            for journey in journeys {
                let Some((adjusted_arrival, legs)) = evaluate_journey_arrival_with_transfer_slack(
                    cache,
                    *from_idx,
                    &journey,
                    depart_secs,
                    Some(&active_trips),
                ) else {
                    continue;
                };

                let option = RouteOption {
                    from_idx: *from_idx,
                    to_idx: *to_idx,
                    adjusted_arrival,
                    access_secs: 0,
                    egress_secs: 0,
                    generalized_cost: generalized_cost(
                        depart_secs,
                        adjusted_arrival,
                        0,
                        0,
                        legs.len(),
                    ),
                    legs: legs.clone(),
                };
                all_options.push(option);

                match &local_best {
                    None => local_best = Some((journey, adjusted_arrival, legs)),
                    Some((current_best_journey, current_adjusted_arrival, _))
                        if adjusted_arrival < *current_adjusted_arrival
                            || (adjusted_arrival == *current_adjusted_arrival
                                && better_journey(&journey, current_best_journey)) =>
                    {
                        local_best = Some((journey, adjusted_arrival, legs))
                    }
                    _ => {}
                }
            }

            if let Some((_, adjusted_arrival, legs)) = local_best {
                pair_stats.push(EvaluatedPair {
                    from_idx: *from_idx,
                    to_idx: *to_idx,
                    pareto_count: 1,
                    best_arrival: Some(adjusted_arrival),
                    legs_count: Some(legs.len()),
                });

                let option = RouteOption {
                    from_idx: *from_idx,
                    to_idx: *to_idx,
                    adjusted_arrival,
                    access_secs: 0,
                    egress_secs: 0,
                    generalized_cost: generalized_cost(
                        depart_secs,
                        adjusted_arrival,
                        0,
                        0,
                        legs.len(),
                    ),
                    legs,
                };
                match &best {
                    None => best = Some(option),
                    Some(current)
                        if option.generalized_cost < current.generalized_cost
                            || (option.generalized_cost == current.generalized_cost
                                && option.adjusted_arrival < current.adjusted_arrival)
                            || (option.generalized_cost == current.generalized_cost
                                && option.adjusted_arrival == current.adjusted_arrival
                                && option.legs.len() < current.legs.len()) =>
                    {
                        best = Some(option)
                    }
                    _ => {}
                }
            } else {
                pair_stats.push(EvaluatedPair {
                    from_idx: *from_idx,
                    to_idx: *to_idx,
                    pareto_count: 0,
                    best_arrival: None,
                    legs_count: None,
                });
            }
        }
    }

    let Some(best) = best else {
        return Err(format!(
            "No route found from '{from_query}' to '{to_query}' for {query_date} after {:02}:{:02}.",
            depart_secs / 3600,
            (depart_secs % 3600) / 60
        ));
    };

    all_options.sort_by(|a, b| {
        a.generalized_cost
            .cmp(&b.generalized_cost)
            .then(a.adjusted_arrival.cmp(&b.adjusted_arrival))
            .then(a.legs.len().cmp(&b.legs.len()))
    });
    let alternatives: Vec<RouteOption> = all_options
        .into_iter()
        .filter(|o| {
            !(o.from_idx == best.from_idx
                && o.to_idx == best.to_idx
                && o.adjusted_arrival == best.adjusted_arrival
                && o.legs == best.legs)
        })
        .take(alternatives)
        .collect();

    Ok(RoutePlanResult {
        from_query: from_query.to_string(),
        to_query: to_query.to_string(),
        query_date: query_date.to_string(),
        depart_secs,
        arrival_secs: best.adjusted_arrival,
        from_station_idxs,
        to_station_idxs,
        chosen_from_idx: best.from_idx,
        chosen_to_idx: best.to_idx,
        chosen_access_secs: best.access_secs,
        chosen_egress_secs: best.egress_secs,
        chosen_legs: best.legs,
        evaluated_pairs: pair_stats,
        alternatives,
    })
}

pub fn plan_route_from_coords(
    cache: &PlannerCache,
    from_lat: f64,
    from_lon: f64,
    to_lat: f64,
    to_lon: f64,
    alternatives: usize,
    depart_secs_override: Option<usize>,
    query_date_override: Option<NaiveDate>,
) -> Result<RoutePlanResult, String> {
    let now = Local::now();
    let query_date = query_date_override.unwrap_or(now.date_naive());
    let depart_secs = match (depart_secs_override, query_date_override) {
        (Some(dep), _) => dep,
        (None, Some(_)) => 0,
        (None, None) => now.time().num_seconds_from_midnight() as usize,
    };

    let from_candidates = nearest_station_candidates(
        cache,
        from_lat,
        from_lon,
        MAX_COORD_CANDIDATES,
        MAX_ACCESS_DISTANCE_METERS,
    );
    if from_candidates.is_empty() {
        return Err(format!(
            "No origin stations found within {:.0}m of {}, {}.",
            MAX_ACCESS_DISTANCE_METERS, from_lat, from_lon
        ));
    }
    let to_candidates = nearest_station_candidates(
        cache,
        to_lat,
        to_lon,
        MAX_COORD_CANDIDATES,
        MAX_ACCESS_DISTANCE_METERS,
    );
    if to_candidates.is_empty() {
        return Err(format!(
            "No destination stations found within {:.0}m of {}, {}.",
            MAX_ACCESS_DISTANCE_METERS, to_lat, to_lon
        ));
    }

    let from_station_idxs: Vec<u32> = from_candidates.iter().map(|c| c.station_idx).collect();
    let to_station_idxs: Vec<u32> = to_candidates.iter().map(|c| c.station_idx).collect();

    let active_trips = cache.active_trip_mask_for_date(query_date);
    let timetable = PlannerTimetable {
        cache,
        active_trips: Some(&active_trips),
    };

    let mut best: Option<RouteOption> = None;
    let mut pair_stats: Vec<EvaluatedPair> = Vec::new();
    let mut all_options: Vec<RouteOption> = Vec::new();

    for from_candidate in &from_candidates {
        for to_candidate in &to_candidates {
            let effective_depart = depart_secs.saturating_add(from_candidate.walk_secs);
            let journeys = timetable.raptor(
                MAX_TRANSFERS,
                effective_depart,
                from_candidate.station_idx,
                to_candidate.station_idx,
            );
            if journeys.is_empty() {
                pair_stats.push(EvaluatedPair {
                    from_idx: from_candidate.station_idx,
                    to_idx: to_candidate.station_idx,
                    pareto_count: 0,
                    best_arrival: None,
                    legs_count: None,
                });
                continue;
            }

            let mut local_best: Option<(Journey<u32, u32>, RouteOption)> = None;
            for journey in journeys {
                let Some((transit_arrival, legs)) = evaluate_journey_arrival_with_transfer_slack(
                    cache,
                    from_candidate.station_idx,
                    &journey,
                    effective_depart,
                    Some(&active_trips),
                ) else {
                    continue;
                };

                let adjusted_arrival = transit_arrival.saturating_add(to_candidate.walk_secs);
                let option = RouteOption {
                    from_idx: from_candidate.station_idx,
                    to_idx: to_candidate.station_idx,
                    adjusted_arrival,
                    access_secs: from_candidate.walk_secs,
                    egress_secs: to_candidate.walk_secs,
                    generalized_cost: generalized_cost(
                        depart_secs,
                        adjusted_arrival,
                        from_candidate.walk_secs,
                        to_candidate.walk_secs,
                        legs.len(),
                    ),
                    legs: legs.clone(),
                };
                all_options.push(option.clone());

                match &local_best {
                    None => local_best = Some((journey, option)),
                    Some((current_journey, current_option))
                        if option.generalized_cost < current_option.generalized_cost
                            || (option.generalized_cost == current_option.generalized_cost
                                && option.adjusted_arrival < current_option.adjusted_arrival)
                            || (option.generalized_cost == current_option.generalized_cost
                                && option.adjusted_arrival == current_option.adjusted_arrival
                                && better_journey(&journey, current_journey)) =>
                    {
                        local_best = Some((journey, option))
                    }
                    _ => {}
                }
            }

            if let Some((_, option)) = local_best {
                pair_stats.push(EvaluatedPair {
                    from_idx: option.from_idx,
                    to_idx: option.to_idx,
                    pareto_count: 1,
                    best_arrival: Some(option.adjusted_arrival),
                    legs_count: Some(option.legs.len()),
                });

                match &best {
                    None => best = Some(option),
                    Some(current)
                        if option.generalized_cost < current.generalized_cost
                            || (option.generalized_cost == current.generalized_cost
                                && option.adjusted_arrival < current.adjusted_arrival)
                            || (option.generalized_cost == current.generalized_cost
                                && option.adjusted_arrival == current.adjusted_arrival
                                && option.legs.len() < current.legs.len()) =>
                    {
                        best = Some(option)
                    }
                    _ => {}
                }
            } else {
                pair_stats.push(EvaluatedPair {
                    from_idx: from_candidate.station_idx,
                    to_idx: to_candidate.station_idx,
                    pareto_count: 0,
                    best_arrival: None,
                    legs_count: None,
                });
            }
        }
    }

    let Some(best) = best else {
        return Err(format!(
            "No route found from ({}, {}) to ({}, {}) for {} after {:02}:{:02}.",
            from_lat,
            from_lon,
            to_lat,
            to_lon,
            query_date,
            depart_secs / 3600,
            (depart_secs % 3600) / 60
        ));
    };

    all_options.sort_by(|a, b| {
        a.generalized_cost
            .cmp(&b.generalized_cost)
            .then(a.adjusted_arrival.cmp(&b.adjusted_arrival))
            .then(a.legs.len().cmp(&b.legs.len()))
    });
    let alternatives: Vec<RouteOption> = all_options
        .into_iter()
        .filter(|o| {
            !(o.from_idx == best.from_idx
                && o.to_idx == best.to_idx
                && o.adjusted_arrival == best.adjusted_arrival
                && o.access_secs == best.access_secs
                && o.egress_secs == best.egress_secs
                && o.legs == best.legs)
        })
        .take(alternatives)
        .collect();

    Ok(RoutePlanResult {
        from_query: format!("{from_lat:.6},{from_lon:.6}"),
        to_query: format!("{to_lat:.6},{to_lon:.6}"),
        query_date: query_date.to_string(),
        depart_secs,
        arrival_secs: best.adjusted_arrival,
        from_station_idxs,
        to_station_idxs,
        chosen_from_idx: best.from_idx,
        chosen_to_idx: best.to_idx,
        chosen_access_secs: best.access_secs,
        chosen_egress_secs: best.egress_secs,
        chosen_legs: best.legs,
        evaluated_pairs: pair_stats,
        alternatives,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::Path;

    use chrono::NaiveDate;
    use raptor::Timetable;

    use crate::cli::DEFAULT_GTFS_PATH;
    use crate::merge::ensure_combined_source_ready;
    use crate::snapshot::SourceFingerprint;

    use super::*;
    use crate::route_planner::model::{
        PlannerCache, PlannerRoute, PlannerServiceCalendar, PlannerStation, PlannerTrip,
    };

    fn tiny_cache() -> PlannerCache {
        let stations = vec![
            PlannerStation {
                key: "A".to_string(),
                name: "A".to_string(),
                member_stop_ids: vec!["a:1".to_string()],
                centroid_lat: Some(48.2000),
                centroid_lon: Some(16.3600),
            },
            PlannerStation {
                key: "B".to_string(),
                name: "B".to_string(),
                member_stop_ids: vec!["b:1".to_string()],
                centroid_lat: Some(48.2100),
                centroid_lon: Some(16.3700),
            },
            PlannerStation {
                key: "C".to_string(),
                name: "C".to_string(),
                member_stop_ids: vec!["c:1".to_string()],
                centroid_lat: Some(48.2200),
                centroid_lon: Some(16.3800),
            },
        ];

        let routes = vec![
            PlannerRoute {
                base_route_id: "R1".to_string(),
                short_name: "R1".to_string(),
                long_name: "-".to_string(),
                stations: vec![0, 1],
            },
            PlannerRoute {
                base_route_id: "R2".to_string(),
                short_name: "R2".to_string(),
                long_name: "-".to_string(),
                stations: vec![1, 2],
            },
        ];

        let route_station_pos = vec![
            HashMap::from([(0_u32, 0_usize), (1_u32, 1_usize)]),
            HashMap::from([(1_u32, 0_usize), (2_u32, 1_usize)]),
        ];

        // Trip 0: A->B (10:00 -> 10:05)
        // Trip 1: B->C (10:05 -> 10:10) should be rejected with 120s slack.
        // Trip 2: B->C (10:07 -> 10:12) should be accepted with 120s slack.
        let trips = vec![
            PlannerTrip {
                route_idx: 0,
                service_idx: 0,
                times: vec![(36_000, 36_000), (36_300, 36_300)],
            },
            PlannerTrip {
                route_idx: 1,
                service_idx: 0,
                times: vec![(36_300, 36_300), (36_600, 36_600)],
            },
            PlannerTrip {
                route_idx: 1,
                service_idx: 0,
                times: vec![(36_420, 36_420), (36_720, 36_720)],
            },
        ];

        let trip_idxs_by_route = vec![vec![0], vec![1, 2]];
        let routes_serving_station =
            HashMap::from([(0_u32, vec![0_u32]), (1_u32, vec![0, 1]), (2_u32, vec![1])]);

        PlannerCache {
            version: 1,
            built_unix_secs: 0,
            fingerprint: SourceFingerprint {
                source_path: "x".to_string(),
                file_count: 0,
                total_size_bytes: 0,
                newest_modified_unix_secs: 0,
            },
            stations,
            station_idx_by_key: HashMap::from([
                ("A".to_string(), 0_u32),
                ("B".to_string(), 1_u32),
                ("C".to_string(), 2_u32),
            ]),
            station_idxs_by_name_upper: HashMap::from([
                ("A".to_string(), vec![0_u32]),
                ("B".to_string(), vec![1_u32]),
                ("C".to_string(), vec![2_u32]),
            ]),
            station_idx_by_stop_id: HashMap::new(),
            station_idxs_by_code_upper: HashMap::new(),
            routes,
            route_station_pos,
            trips,
            trip_idxs_by_route,
            service_ids: vec!["svc".to_string()],
            service_calendars: HashMap::from([(
                0_u32,
                PlannerServiceCalendar {
                    weekday_mask: 0b111_1111,
                    start_date_yyyymmdd: 20000101,
                    end_date_yyyymmdd: 20991231,
                },
            )]),
            services_added_by_date: HashMap::new(),
            services_removed_by_date: HashMap::new(),
            routes_serving_station,
            footpaths: HashMap::from([
                (0_u32, vec![0_u32]),
                (1_u32, vec![1_u32]),
                (2_u32, vec![2_u32]),
            ]),
            transfer_times: HashMap::from([
                ((0_u32, 0_u32), 0_usize),
                ((1_u32, 1_u32), 0_usize),
                ((2_u32, 2_u32), 0_usize),
            ]),
        }
    }

    #[test]
    fn enforces_min_transfer_slack_in_leg_reconstruction() {
        let cache = tiny_cache();
        let journey = Journey {
            plan: vec![(0_u32, 1_u32), (1_u32, 2_u32)],
            arrival: 36_600,
        };

        let legs = build_leg_timings(&cache, 0, &journey, 35_900, 120, None).expect("legs");
        assert_eq!(legs.len(), 2);
        // second leg should board at 10:07, not 10:05.
        assert_eq!(legs[1].departure, 36_420);
        assert_eq!(legs[1].arrival, 36_720);
    }

    #[test]
    fn timetable_skips_inactive_trips() {
        let cache = tiny_cache();
        let active = vec![true, false, true];
        let timetable = PlannerTimetable {
            cache: &cache,
            active_trips: Some(&active),
        };

        // Route 1 at station B should pick trip 2 (10:07), since trip 1 (10:05) is inactive.
        assert_eq!(timetable.get_earliest_trip(1, 36_300, 1), Some(2));
    }

    #[test]
    fn nearest_station_candidates_prefers_closest_station() {
        let cache = tiny_cache();
        let candidates = nearest_station_candidates(&cache, 48.2099, 16.3699, 3, 2_500.0);
        assert!(!candidates.is_empty());
        assert_eq!(candidates[0].station_idx, 1);
    }

    #[test]
    fn match_station_idxs_supports_partial_multi_word_queries() {
        let mut cache = tiny_cache();
        cache.stations[1].name = "Flughafen Wien Bahnhof".to_string();
        cache.stations[2].name = "Wien Mitte-Landstraße".to_string();
        cache.station_idxs_by_name_upper = HashMap::from([
            ("A".to_string(), vec![0_u32]),
            ("FLUGHAFEN WIEN BAHNHOF".to_string(), vec![1_u32]),
            ("WIEN MITTE-LANDSTRASSE".to_string(), vec![2_u32]),
        ]);

        let matched = match_station_idxs(&cache, "flughafen");
        assert_eq!(matched, vec![1_u32]);

        let matched_mitte = match_station_idxs(&cache, "wien mitte");
        assert_eq!(matched_mitte, vec![2_u32]);

        let matched_generic = match_station_idxs(&cache, "wien");
        assert!(matched_generic.is_empty());
    }

    #[test]
    fn active_services_apply_calendar_dates_overrides() {
        let mut cache = tiny_cache();
        cache.service_ids = vec!["svc".to_string()];
        cache.service_calendars = HashMap::from([(
            0_u32,
            PlannerServiceCalendar {
                // Monday only
                weekday_mask: 0b000_0001,
                start_date_yyyymmdd: 20260101,
                end_date_yyyymmdd: 20261231,
            },
        )]);
        // 2026-01-05 is Monday; remove service there.
        cache.services_removed_by_date = HashMap::from([(20260105_i32, vec![0_u32])]);
        // 2026-01-06 is Tuesday; add service there.
        cache.services_added_by_date = HashMap::from([(20260106_i32, vec![0_u32])]);

        let monday = NaiveDate::from_ymd_opt(2026, 1, 5).expect("valid date");
        let tuesday = NaiveDate::from_ymd_opt(2026, 1, 6).expect("valid date");
        let monday_active = cache.active_services_on(monday);
        let tuesday_active = cache.active_services_on(tuesday);

        assert_eq!(monday_active, vec![false]);
        assert_eq!(tuesday_active, vec![true]);
    }

    #[test]
    fn herrengasse_praterstern_regression_if_dataset_available() {
        if !Path::new("data/wiener-linien/stops.txt").exists()
            || !Path::new("data/oebb/stops.txt").exists()
        {
            return;
        }

        ensure_combined_source_ready(DEFAULT_GTFS_PATH).expect("combined source ready");
        let cache = super::super::cache::load_or_build_planner_cache(DEFAULT_GTFS_PATH)
            .expect("planner cache");
        let result = plan_route(
            &cache,
            "Herrengasse",
            "Praterstern",
            1,
            Some(8 * 3600),
            Some(NaiveDate::from_ymd_opt(2026, 1, 15).expect("date")),
        )
        .expect("route exists");
        assert!(!result.chosen_legs.is_empty());
    }
}
