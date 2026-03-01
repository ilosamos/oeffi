use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::snapshot::SourceFingerprint;

pub const STOP_FUZZY_THRESHOLD: f64 = 0.94;
pub const MAX_TRANSFERS: usize = 6;
pub const MIN_TRANSFER_SECONDS: usize = 150;
pub const DEFAULT_TRANSFER_SECONDS: usize = 300;
pub const PLANNER_CACHE_PATH: &str = "planner.cache.bin";
pub const PLANNER_CACHE_VERSION: u32 = 1;
pub const PLANNER_CACHE_DECODE_LIMIT_BYTES: u64 = 1024 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannerStop {
    pub id: String,
    pub name: String,
    pub code: Option<String>,
    pub parent_station: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannerStation {
    pub key: String,
    pub name: String,
    pub member_stop_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannerRoute {
    pub base_route_id: String,
    pub short_name: String,
    pub long_name: String,
    pub stations: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannerTrip {
    pub route_idx: u32,
    pub times: Vec<(usize, usize)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannerCache {
    pub version: u32,
    pub built_unix_secs: u64,
    pub fingerprint: SourceFingerprint,
    pub stations: Vec<PlannerStation>,
    pub station_idx_by_key: HashMap<String, u32>,
    pub station_idxs_by_name_upper: HashMap<String, Vec<u32>>,
    pub station_idx_by_stop_id: HashMap<String, u32>,
    pub station_idxs_by_code_upper: HashMap<String, Vec<u32>>,
    pub routes: Vec<PlannerRoute>,
    pub route_station_pos: Vec<HashMap<u32, usize>>,
    pub trips: Vec<PlannerTrip>,
    pub trip_idxs_by_route: Vec<Vec<u32>>,
    pub routes_serving_station: HashMap<u32, Vec<u32>>,
    pub footpaths: HashMap<u32, Vec<u32>>,
    pub transfer_times: HashMap<(u32, u32), usize>,
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

#[derive(Debug, Clone)]
pub struct EvaluatedPair {
    pub from_idx: u32,
    pub to_idx: u32,
    pub pareto_count: usize,
    pub best_arrival: Option<usize>,
    pub legs_count: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegTiming {
    pub route_idx: u32,
    pub from_station_idx: u32,
    pub to_station_idx: u32,
    pub departure: usize,
    pub arrival: usize,
    pub stops_count: usize,
}

#[derive(Debug, Clone)]
pub struct RouteOption {
    pub from_idx: u32,
    pub to_idx: u32,
    pub adjusted_arrival: usize,
    pub legs: Vec<LegTiming>,
}

#[derive(Debug, Clone)]
pub struct RoutePlanResult {
    pub from_query: String,
    pub to_query: String,
    pub query_date: String,
    pub depart_secs: usize,
    pub arrival_secs: usize,
    pub from_station_idxs: Vec<u32>,
    pub to_station_idxs: Vec<u32>,
    pub chosen_from_idx: u32,
    pub chosen_to_idx: u32,
    pub chosen_legs: Vec<LegTiming>,
    pub evaluated_pairs: Vec<EvaluatedPair>,
    pub alternatives: Vec<RouteOption>,
}
