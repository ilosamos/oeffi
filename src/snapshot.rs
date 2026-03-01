use std::collections::HashMap;

use serde::{Deserialize, Serialize};

pub const SNAPSHOT_VERSION: u32 = 3;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotSummary {
    pub agencies: usize,
    pub routes: usize,
    pub trips: usize,
    pub stops: usize,
    pub calendars: usize,
    pub calendar_dates: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteEntry {
    pub id: String,
    pub short_name: String,
    pub long_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StopEntry {
    pub name: String,
    pub stop_ids_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StopRecord {
    pub id: String,
    pub name: String,
    pub code: Option<String>,
    pub parent_station: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceFingerprint {
    pub source_path: String,
    pub file_count: usize,
    pub total_size_bytes: u64,
    pub newest_modified_unix_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub version: u32,
    pub built_unix_secs: u64,
    pub fingerprint: SourceFingerprint,
    pub summary: SnapshotSummary,
    pub routes: Vec<RouteEntry>,
    pub route_ids_by_short_name_upper: HashMap<String, Vec<String>>,
    pub route_stops_by_route_id: HashMap<String, Vec<StopEntry>>,
    pub stops: Vec<StopRecord>,
    pub stop_ids_by_name_upper: HashMap<String, Vec<String>>,
    pub stop_ids_by_code_upper: HashMap<String, Vec<String>>,
    pub route_ids_by_stop_id: HashMap<String, Vec<String>>,
}
