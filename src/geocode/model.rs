use serde::{Deserialize, Serialize};

pub const GEOCODE_CACHE_VERSION: u32 = 3;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddressRecord {
    pub street: String,
    pub house_number: String,
    pub postcode: Option<String>,
    pub city: Option<String>,
    pub normalized_key: String,
    pub lat: f64,
    pub lon: f64,
    pub count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LandmarkRecord {
    pub name: String,
    pub kind: String,
    pub normalized_name: String,
    pub lat: f64,
    pub lon: f64,
    pub count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeocodeBuildStats {
    pub objects_total: u64,
    pub nodes_total: u64,
    pub ways_total: u64,
    pub relations_total: u64,
    pub addr_nodes_total: u64,
    pub addr_ways_total: u64,
    pub addr_relations_total: u64,
    pub addr_nodes_in_polygon: u64,
    pub unique_addresses: u64,
    pub named_nodes_total: u64,
    pub named_nodes_in_polygon: u64,
    pub landmark_nodes_total: u64,
    pub landmark_nodes_in_polygon: u64,
    pub landmark_ways_total: u64,
    pub landmark_ways_in_polygon: u64,
    pub landmark_relations_total: u64,
    pub landmark_relations_in_polygon: u64,
    pub unique_landmarks: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeocodeCache {
    pub version: u32,
    pub built_unix_ts: i64,
    pub source_pbf: String,
    pub polygon_path: String,
    pub stats: GeocodeBuildStats,
    pub addresses: Vec<AddressRecord>,
    pub landmarks: Vec<LandmarkRecord>,
}
