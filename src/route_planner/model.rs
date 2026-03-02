use std::collections::HashMap;

use chrono::{Datelike, NaiveDate, Weekday};
use serde::{Deserialize, Serialize};

use crate::snapshot::SourceFingerprint;

pub const STOP_FUZZY_THRESHOLD: f64 = 0.94;
pub const MAX_TRANSFERS: usize = 6;
pub const MIN_TRANSFER_SECONDS: usize = 150;
pub const DEFAULT_TRANSFER_SECONDS: usize = 300;
pub const PLANNER_CACHE_VERSION: u32 = 6;
pub const PLANNER_CACHE_DECODE_LIMIT_BYTES: u64 = 1024 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannerStop {
    pub id: String,
    pub name: String,
    pub code: Option<String>,
    pub parent_station: Option<String>,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannerStation {
    pub key: String,
    pub name: String,
    pub member_stop_ids: Vec<String>,
    pub centroid_lat: Option<f64>,
    pub centroid_lon: Option<f64>,
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
    pub service_idx: u32,
    pub times: Vec<(usize, usize)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannerServiceCalendar {
    pub weekday_mask: u8,
    pub start_date_yyyymmdd: i32,
    pub end_date_yyyymmdd: i32,
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
    pub service_ids: Vec<String>,
    pub service_calendars: HashMap<u32, PlannerServiceCalendar>,
    pub services_added_by_date: HashMap<i32, Vec<u32>>,
    pub services_removed_by_date: HashMap<i32, Vec<u32>>,
    pub routes_serving_station: HashMap<u32, Vec<u32>>,
    pub footpaths: HashMap<u32, Vec<u32>>,
    pub transfer_times: HashMap<(u32, u32), usize>,
}

impl PlannerCache {
    fn weekday_bit(weekday: Weekday) -> u8 {
        match weekday {
            Weekday::Mon => 1 << 0,
            Weekday::Tue => 1 << 1,
            Weekday::Wed => 1 << 2,
            Weekday::Thu => 1 << 3,
            Weekday::Fri => 1 << 4,
            Weekday::Sat => 1 << 5,
            Weekday::Sun => 1 << 6,
        }
    }

    fn date_to_yyyymmdd(date: NaiveDate) -> i32 {
        date.year() * 10_000 + date.month() as i32 * 100 + date.day() as i32
    }

    pub fn stations_count(&self) -> usize {
        self.stations.len()
    }

    pub fn routes_count(&self) -> usize {
        self.routes.len()
    }

    pub fn trips_count(&self) -> usize {
        self.trips.len()
    }

    pub fn active_services_on(&self, date: NaiveDate) -> Vec<bool> {
        let mut active = vec![false; self.service_ids.len()];
        let date_key = Self::date_to_yyyymmdd(date);
        let weekday_bit = Self::weekday_bit(date.weekday());

        for (service_idx, cal) in &self.service_calendars {
            if date_key >= cal.start_date_yyyymmdd
                && date_key <= cal.end_date_yyyymmdd
                && (cal.weekday_mask & weekday_bit) != 0
            {
                active[*service_idx as usize] = true;
            }
        }

        if let Some(removed) = self.services_removed_by_date.get(&date_key) {
            for service_idx in removed {
                active[*service_idx as usize] = false;
            }
        }
        if let Some(added) = self.services_added_by_date.get(&date_key) {
            for service_idx in added {
                active[*service_idx as usize] = true;
            }
        }

        active
    }

    pub fn active_trip_mask_for_date(&self, date: NaiveDate) -> Vec<bool> {
        let active_services = self.active_services_on(date);
        self.trips
            .iter()
            .map(|trip| {
                active_services
                    .get(trip.service_idx as usize)
                    .copied()
                    .unwrap_or(false)
            })
            .collect()
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
    pub access_secs: usize,
    pub egress_secs: usize,
    pub generalized_cost: usize,
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
    pub chosen_access_secs: usize,
    pub chosen_egress_secs: usize,
    pub chosen_legs: Vec<LegTiming>,
    pub evaluated_pairs: Vec<EvaluatedPair>,
    pub alternatives: Vec<RouteOption>,
}
