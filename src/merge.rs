use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use csv::{ReaderBuilder, StringRecord, WriterBuilder};
const META_FILE: &str = ".merge_fingerprint";
const MERGE_SCHEMA_VERSION: u32 = 3;

const VIENNA_CENTER_LAT: f64 = 48.2082;
const VIENNA_CENTER_LON: f64 = 16.3738;
const NEAR_VIENNA_RADIUS_METERS: f64 = 60_000.0;
const EARTH_RADIUS_METERS: f64 = 6_371_000.0;

const AGENCY_FILE: &str = "agency.txt";
const ROUTES_FILE: &str = "routes.txt";
const STOPS_FILE: &str = "stops.txt";
const TRIPS_FILE: &str = "trips.txt";
const STOP_TIMES_FILE: &str = "stop_times.txt";
const CALENDAR_FILE: &str = "calendar.txt";
const CALENDAR_DATES_FILE: &str = "calendar_dates.txt";

const REQUIRED_FILES: [&str; 7] = [
    AGENCY_FILE,
    ROUTES_FILE,
    STOPS_FILE,
    TRIPS_FILE,
    STOP_TIMES_FILE,
    CALENDAR_FILE,
    CALENDAR_DATES_FILE,
];

#[derive(Clone, Copy)]
struct MergeSources<'a> {
    wiener_linien_source: &'a str,
    oebb_source: &'a str,
}

// Minimal metadata we keep from ÖBB routes in the merged Vienna feed.
#[derive(Clone)]
struct OebbRoute {
    agency_id: String,
    short_name: String,
    route_type: String,
}

#[derive(Clone)]
struct OebbTrip {
    route_id: String,
    service_id: String,
    trip_id: String,
    shape_id: String,
    trip_headsign: String,
    direction_id: String,
    block_id: String,
}

#[derive(Clone)]
struct OebbStop {
    stop_id: String,
    stop_name: String,
    stop_lat: String,
    stop_lon: String,
    zone_id: String,
    location_type: String,
    parent_station: String,
    level_id: String,
    platform_code: String,
}

struct StopScope {
    scoped_stop_ids: HashSet<String>,
    vienna_stop_ids: HashSet<String>,
}

fn path_for(source_root: &str, file_name: &str) -> PathBuf {
    Path::new(source_root).join(file_name)
}

fn prefixed_oebb_id(id: &str) -> String {
    // Namespace ÖBB ids so they never collide with Wiener Linien ids.
    format!("oebb:{id}")
}

fn haversine_meters(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let dlat = (lat2 - lat1).to_radians();
    let dlon = (lon2 - lon1).to_radians();
    let lat1 = lat1.to_radians();
    let lat2 = lat2.to_radians();

    let a = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());
    EARTH_RADIUS_METERS * c
}

fn parse_coord(value: &str) -> Option<f64> {
    value.parse::<f64>().ok()
}

fn stop_in_scope(stop_id: &str, stop_lat: &str, stop_lon: &str) -> bool {
    if stop_id.starts_with("at:49:") {
        return true;
    }

    let (Some(lat), Some(lon)) = (parse_coord(stop_lat), parse_coord(stop_lon)) else {
        return false;
    };

    haversine_meters(VIENNA_CENTER_LAT, VIENNA_CENTER_LON, lat, lon) <= NEAR_VIENNA_RADIUS_METERS
}

fn header_index(headers: &StringRecord, name: &str) -> Result<usize, String> {
    headers
        .iter()
        .position(|h| h.trim_start_matches('\u{feff}') == name)
        .ok_or_else(|| format!("Missing required CSV column '{name}'."))
}

fn csv_reader(path: &Path) -> Result<csv::Reader<std::fs::File>, String> {
    ReaderBuilder::new()
        .flexible(true)
        .from_path(path)
        .map_err(|err| format!("Failed to open CSV '{}': {err}", path.display()))
}

fn csv_writer(path: &Path) -> Result<csv::Writer<std::fs::File>, String> {
    WriterBuilder::new()
        .from_path(path)
        .map_err(|err| format!("Failed to create CSV '{}': {err}", path.display()))
}

fn required_sources_fingerprint(sources: MergeSources<'_>) -> Result<String, String> {
    let mut lines = vec![format!("schema_version={MERGE_SCHEMA_VERSION}")];

    for source in [sources.wiener_linien_source, sources.oebb_source] {
        for file in REQUIRED_FILES {
            let path = path_for(source, file);
            let meta = fs::metadata(&path)
                .map_err(|err| format!("Cannot read source file '{}': {err}", path.display()))?;
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            lines.push(format!("{}|{}|{}", path.display(), meta.len(), mtime));
        }
    }

    lines.sort();
    Ok(lines.join("\n"))
}

fn combined_outputs_exist(output_root: &str) -> bool {
    REQUIRED_FILES
        .iter()
        .all(|file| path_for(output_root, file).is_file())
}

pub fn validate_raw_sources(wiener_linien_source: &str, oebb_source: &str) -> Result<(), String> {
    for source in [wiener_linien_source, oebb_source] {
        for file in REQUIRED_FILES {
            let path = path_for(source, file);
            if !path.is_file() {
                return Err(format!(
                    "Missing required GTFS source file '{}'.",
                    path.display()
                ));
            }
        }
    }
    Ok(())
}

fn is_commuter_route(short_name: &str) -> bool {
    let short = short_name.to_ascii_uppercase();
    if short.starts_with("REX") {
        return true;
    }
    if let Some(rest) = short.strip_prefix('S') {
        return rest.chars().next().is_some_and(|c| c.is_ascii_digit());
    }
    if let Some(rest) = short.strip_prefix('R') {
        return rest.chars().next().is_some_and(|c| c.is_ascii_digit());
    }
    false
}

fn load_oebb_routes(oebb_source: &str) -> Result<HashMap<String, OebbRoute>, String> {
    let path = path_for(oebb_source, ROUTES_FILE);
    let mut rdr = csv_reader(&path)?;
    let headers = rdr
        .headers()
        .map_err(|err| format!("Failed reading '{}': {err}", path.display()))?
        .clone();

    let route_id_idx = header_index(&headers, "route_id")?;
    let agency_id_idx = header_index(&headers, "agency_id")?;
    let short_name_idx = header_index(&headers, "route_short_name")?;
    let route_type_idx = header_index(&headers, "route_type")?;

    let mut routes = HashMap::new();
    for rec in rdr.records() {
        let rec = rec.map_err(|err| format!("Failed reading '{}': {err}", path.display()))?;
        let route_id = rec.get(route_id_idx).unwrap_or_default().to_string();
        if route_id.is_empty() {
            continue;
        }
        routes.insert(
            route_id,
            OebbRoute {
                agency_id: rec.get(agency_id_idx).unwrap_or_default().to_string(),
                short_name: rec.get(short_name_idx).unwrap_or_default().to_string(),
                route_type: rec.get(route_type_idx).unwrap_or_default().to_string(),
            },
        );
    }

    Ok(routes)
}

fn load_oebb_scoped_stops(oebb_source: &str) -> Result<(Vec<OebbStop>, StopScope), String> {
    let path = path_for(oebb_source, STOPS_FILE);
    let mut rdr = csv_reader(&path)?;
    let headers = rdr
        .headers()
        .map_err(|err| format!("Failed reading '{}': {err}", path.display()))?
        .clone();

    let stop_id_idx = header_index(&headers, "stop_id")?;
    let stop_name_idx = header_index(&headers, "stop_name")?;
    let stop_lat_idx = header_index(&headers, "stop_lat")?;
    let stop_lon_idx = header_index(&headers, "stop_lon")?;
    let zone_id_idx = header_index(&headers, "zone_id")?;
    let location_type_idx = header_index(&headers, "location_type")?;
    let parent_station_idx = header_index(&headers, "parent_station")?;
    let level_id_idx = header_index(&headers, "level_id")?;
    let platform_code_idx = header_index(&headers, "platform_code")?;

    let mut all_stops: HashMap<String, OebbStop> = HashMap::new();
    let mut scoped_stop_ids: HashSet<String> = HashSet::new();
    let mut vienna_stop_ids: HashSet<String> = HashSet::new();

    for rec in rdr.records() {
        let rec = rec.map_err(|err| format!("Failed reading '{}': {err}", path.display()))?;
        let stop_id = rec.get(stop_id_idx).unwrap_or_default().to_string();
        if stop_id.is_empty() {
            continue;
        }

        let stop = OebbStop {
            stop_id,
            stop_name: rec.get(stop_name_idx).unwrap_or_default().to_string(),
            stop_lat: rec.get(stop_lat_idx).unwrap_or_default().to_string(),
            stop_lon: rec.get(stop_lon_idx).unwrap_or_default().to_string(),
            zone_id: rec.get(zone_id_idx).unwrap_or_default().to_string(),
            location_type: rec.get(location_type_idx).unwrap_or_default().to_string(),
            parent_station: rec.get(parent_station_idx).unwrap_or_default().to_string(),
            level_id: rec.get(level_id_idx).unwrap_or_default().to_string(),
            platform_code: rec.get(platform_code_idx).unwrap_or_default().to_string(),
        };

        if stop.stop_id.starts_with("at:49:") {
            vienna_stop_ids.insert(stop.stop_id.clone());
        }
        if stop_in_scope(&stop.stop_id, &stop.stop_lat, &stop.stop_lon) {
            scoped_stop_ids.insert(stop.stop_id.clone());
        }
        all_stops.insert(stop.stop_id.clone(), stop);
    }

    // Keep parent stations for scoped child stops so clustering can still group platforms.
    let mut extra_parent_ids: Vec<String> = Vec::new();
    for stop_id in &scoped_stop_ids {
        if let Some(stop) = all_stops.get(stop_id)
            && !stop.parent_station.is_empty()
        {
            extra_parent_ids.push(stop.parent_station.clone());
        }
    }
    for parent_id in extra_parent_ids {
        if all_stops.contains_key(&parent_id) {
            scoped_stop_ids.insert(parent_id);
        }
    }

    let mut scoped_stops: Vec<OebbStop> = scoped_stop_ids
        .iter()
        .filter_map(|id| all_stops.get(id).cloned())
        .collect();
    scoped_stops.sort_by(|a, b| a.stop_id.cmp(&b.stop_id));

    Ok((
        scoped_stops,
        StopScope {
            scoped_stop_ids,
            vienna_stop_ids,
        },
    ))
}

struct TripStopCounts {
    vienna_count: u16,
    scoped_count: u16,
}

type SelectedOebbTrips = (
    Vec<OebbTrip>,
    HashSet<String>,
    HashSet<String>,
    HashSet<String>,
);

fn count_oebb_stops_per_trip(
    oebb_source: &str,
    scope: &StopScope,
) -> Result<HashMap<String, TripStopCounts>, String> {
    let path = path_for(oebb_source, STOP_TIMES_FILE);
    let mut rdr = csv_reader(&path)?;
    let headers = rdr
        .headers()
        .map_err(|err| format!("Failed reading '{}': {err}", path.display()))?
        .clone();

    let trip_id_idx = header_index(&headers, "trip_id")?;
    let stop_id_idx = header_index(&headers, "stop_id")?;

    let mut counts: HashMap<String, TripStopCounts> = HashMap::new();
    for rec in rdr.records() {
        let rec = rec.map_err(|err| format!("Failed reading '{}': {err}", path.display()))?;
        let stop_id = rec.get(stop_id_idx).unwrap_or_default();
        let trip_id = rec.get(trip_id_idx).unwrap_or_default();
        if trip_id.is_empty() {
            continue;
        }

        let entry = counts.entry(trip_id.to_string()).or_insert(TripStopCounts {
            vienna_count: 0,
            scoped_count: 0,
        });
        if scope.scoped_stop_ids.contains(stop_id) {
            entry.scoped_count = entry.scoped_count.saturating_add(1);
        }
        if scope.vienna_stop_ids.contains(stop_id) {
            entry.vienna_count = entry.vienna_count.saturating_add(1);
        }
    }
    Ok(counts)
}

fn select_oebb_trips(
    oebb_source: &str,
    routes_by_id: &HashMap<String, OebbRoute>,
    counts_by_trip: &HashMap<String, TripStopCounts>,
) -> Result<SelectedOebbTrips, String> {
    let path = path_for(oebb_source, TRIPS_FILE);
    let mut rdr = csv_reader(&path)?;
    let headers = rdr
        .headers()
        .map_err(|err| format!("Failed reading '{}': {err}", path.display()))?
        .clone();

    let route_id_idx = header_index(&headers, "route_id")?;
    let service_id_idx = header_index(&headers, "service_id")?;
    let trip_id_idx = header_index(&headers, "trip_id")?;
    let shape_id_idx = header_index(&headers, "shape_id")?;
    let trip_headsign_idx = header_index(&headers, "trip_headsign")?;
    let direction_id_idx = header_index(&headers, "direction_id")?;
    let block_id_idx = header_index(&headers, "block_id")?;

    let mut trips = Vec::new();
    let mut kept_trip_ids = HashSet::new();
    let mut kept_route_ids = HashSet::new();
    let mut kept_service_ids = HashSet::new();

    for rec in rdr.records() {
        let rec = rec.map_err(|err| format!("Failed reading '{}': {err}", path.display()))?;

        let route_id = rec.get(route_id_idx).unwrap_or_default();
        let service_id = rec.get(service_id_idx).unwrap_or_default();
        let trip_id = rec.get(trip_id_idx).unwrap_or_default();

        if route_id.is_empty() || service_id.is_empty() || trip_id.is_empty() {
            continue;
        }

        let Some(route_meta) = routes_by_id.get(route_id) else {
            continue;
        };
        // Keep only S/REX/R lines from ÖBB in the combined Vienna dataset.
        if !is_commuter_route(&route_meta.short_name) {
            continue;
        }

        let counts = counts_by_trip.get(trip_id);
        let scoped_count = counts.map(|c| c.scoped_count).unwrap_or(0);
        let vienna_count = counts.map(|c| c.vienna_count).unwrap_or(0);
        // Keep trips that actually connect Vienna with nearby regional stops.
        if vienna_count < 1 || scoped_count < 2 {
            continue;
        }

        kept_trip_ids.insert(trip_id.to_string());
        kept_route_ids.insert(route_id.to_string());
        kept_service_ids.insert(service_id.to_string());

        trips.push(OebbTrip {
            route_id: route_id.to_string(),
            service_id: service_id.to_string(),
            trip_id: trip_id.to_string(),
            shape_id: rec.get(shape_id_idx).unwrap_or_default().to_string(),
            trip_headsign: rec.get(trip_headsign_idx).unwrap_or_default().to_string(),
            direction_id: rec.get(direction_id_idx).unwrap_or_default().to_string(),
            block_id: rec.get(block_id_idx).unwrap_or_default().to_string(),
        });
    }

    Ok((trips, kept_trip_ids, kept_route_ids, kept_service_ids))
}

fn write_agency(
    output_root: &str,
    sources: MergeSources<'_>,
    kept_oebb_agency_ids: &HashSet<String>,
) -> Result<(), String> {
    let out_path = path_for(output_root, AGENCY_FILE);
    let mut wtr = csv_writer(&out_path)?;
    wtr.write_record([
        "agency_id",
        "agency_name",
        "agency_url",
        "agency_timezone",
        "agency_lang",
        "agency_fare_url",
    ])
    .map_err(|err| format!("Failed writing '{}': {err}", out_path.display()))?;

    for source in [sources.wiener_linien_source, sources.oebb_source] {
        let path = path_for(source, AGENCY_FILE);
        let mut rdr = csv_reader(&path)?;
        let headers = rdr
            .headers()
            .map_err(|err| format!("Failed reading '{}': {err}", path.display()))?
            .clone();

        let agency_id_idx = header_index(&headers, "agency_id")?;
        let agency_name_idx = header_index(&headers, "agency_name")?;
        let agency_url_idx = header_index(&headers, "agency_url")?;
        let agency_timezone_idx = header_index(&headers, "agency_timezone")?;
        let agency_lang_idx = header_index(&headers, "agency_lang")?;
        let fare_idx = headers
            .iter()
            .position(|h| h.trim_start_matches('\u{feff}') == "agency_fare_url");

        for rec in rdr.records() {
            let rec = rec.map_err(|err| format!("Failed reading '{}': {err}", path.display()))?;
            let agency_id = rec.get(agency_id_idx).unwrap_or_default();
            if agency_id.is_empty() {
                continue;
            }

            if source == sources.oebb_source && !kept_oebb_agency_ids.contains(agency_id) {
                continue;
            }

            let out_agency_id = if source == sources.oebb_source {
                prefixed_oebb_id(agency_id)
            } else {
                agency_id.to_string()
            };

            let fare = fare_idx
                .and_then(|idx| rec.get(idx))
                .unwrap_or_default()
                .to_string();

            wtr.write_record([
                out_agency_id,
                rec.get(agency_name_idx).unwrap_or_default().to_string(),
                rec.get(agency_url_idx).unwrap_or_default().to_string(),
                rec.get(agency_timezone_idx).unwrap_or_default().to_string(),
                rec.get(agency_lang_idx).unwrap_or_default().to_string(),
                fare,
            ])
            .map_err(|err| format!("Failed writing '{}': {err}", out_path.display()))?;
        }
    }

    wtr.flush()
        .map_err(|err| format!("Failed flushing '{}': {err}", out_path.display()))
}

fn write_routes(
    output_root: &str,
    wiener_linien_source: &str,
    routes_by_id: &HashMap<String, OebbRoute>,
    kept_route_ids: &HashSet<String>,
) -> Result<HashSet<String>, String> {
    let out_path = path_for(output_root, ROUTES_FILE);
    let mut wtr = csv_writer(&out_path)?;
    wtr.write_record([
        "route_id",
        "agency_id",
        "route_short_name",
        "route_long_name",
        "route_type",
        "route_color",
        "route_text_color",
    ])
    .map_err(|err| format!("Failed writing '{}': {err}", out_path.display()))?;

    let wl_path = path_for(wiener_linien_source, ROUTES_FILE);
    let mut wl_rdr = csv_reader(&wl_path)?;
    let wl_headers = wl_rdr
        .headers()
        .map_err(|err| format!("Failed reading '{}': {err}", wl_path.display()))?
        .clone();

    let wl_route_id_idx = header_index(&wl_headers, "route_id")?;
    let wl_agency_id_idx = header_index(&wl_headers, "agency_id")?;
    let wl_short_idx = header_index(&wl_headers, "route_short_name")?;
    let wl_long_idx = header_index(&wl_headers, "route_long_name")?;
    let wl_type_idx = header_index(&wl_headers, "route_type")?;
    let wl_color_idx = header_index(&wl_headers, "route_color")?;
    let wl_text_color_idx = header_index(&wl_headers, "route_text_color")?;

    for rec in wl_rdr.records() {
        let rec = rec.map_err(|err| format!("Failed reading '{}': {err}", wl_path.display()))?;
        wtr.write_record([
            rec.get(wl_route_id_idx).unwrap_or_default().to_string(),
            rec.get(wl_agency_id_idx).unwrap_or_default().to_string(),
            rec.get(wl_short_idx).unwrap_or_default().to_string(),
            rec.get(wl_long_idx).unwrap_or_default().to_string(),
            rec.get(wl_type_idx).unwrap_or_default().to_string(),
            rec.get(wl_color_idx).unwrap_or_default().to_string(),
            rec.get(wl_text_color_idx).unwrap_or_default().to_string(),
        ])
        .map_err(|err| format!("Failed writing '{}': {err}", out_path.display()))?;
    }

    let mut kept_oebb_agency_ids = HashSet::new();
    let mut ordered_kept_routes: Vec<&String> = kept_route_ids.iter().collect();
    ordered_kept_routes.sort();

    for route_id in ordered_kept_routes {
        let Some(route) = routes_by_id.get(route_id) else {
            continue;
        };
        kept_oebb_agency_ids.insert(route.agency_id.clone());

        // ÖBB route_long_name in this feed is not reliable for Vienna commuter routes.
        // Use a neutral placeholder instead.
        wtr.write_record([
            prefixed_oebb_id(route_id),
            prefixed_oebb_id(&route.agency_id),
            route.short_name.clone(),
            "-".to_string(),
            route.route_type.clone(),
            String::new(),
            String::new(),
        ])
        .map_err(|err| format!("Failed writing '{}': {err}", out_path.display()))?;
    }

    wtr.flush()
        .map_err(|err| format!("Failed flushing '{}': {err}", out_path.display()))?;

    Ok(kept_oebb_agency_ids)
}

fn write_stops(
    output_root: &str,
    wiener_linien_source: &str,
    oebb_stops: &[OebbStop],
) -> Result<(), String> {
    let out_path = path_for(output_root, STOPS_FILE);
    let mut wtr = csv_writer(&out_path)?;
    wtr.write_record([
        "stop_id",
        "stop_name",
        "stop_lat",
        "stop_lon",
        "zone_id",
        "location_type",
        "parent_station",
        "level_id",
        "platform_code",
    ])
    .map_err(|err| format!("Failed writing '{}': {err}", out_path.display()))?;

    let wl_path = path_for(wiener_linien_source, STOPS_FILE);
    let mut wl_rdr = csv_reader(&wl_path)?;
    let wl_headers = wl_rdr
        .headers()
        .map_err(|err| format!("Failed reading '{}': {err}", wl_path.display()))?
        .clone();

    let wl_stop_id_idx = header_index(&wl_headers, "stop_id")?;
    let wl_stop_name_idx = header_index(&wl_headers, "stop_name")?;
    let wl_stop_lat_idx = header_index(&wl_headers, "stop_lat")?;
    let wl_stop_lon_idx = header_index(&wl_headers, "stop_lon")?;
    let wl_zone_id_idx = header_index(&wl_headers, "zone_id")?;

    for rec in wl_rdr.records() {
        let rec = rec.map_err(|err| format!("Failed reading '{}': {err}", wl_path.display()))?;
        wtr.write_record([
            rec.get(wl_stop_id_idx).unwrap_or_default().to_string(),
            rec.get(wl_stop_name_idx).unwrap_or_default().to_string(),
            rec.get(wl_stop_lat_idx).unwrap_or_default().to_string(),
            rec.get(wl_stop_lon_idx).unwrap_or_default().to_string(),
            rec.get(wl_zone_id_idx).unwrap_or_default().to_string(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
        ])
        .map_err(|err| format!("Failed writing '{}': {err}", out_path.display()))?;
    }

    for stop in oebb_stops {
        wtr.write_record([
            stop.stop_id.clone(),
            stop.stop_name.clone(),
            stop.stop_lat.clone(),
            stop.stop_lon.clone(),
            stop.zone_id.clone(),
            stop.location_type.clone(),
            stop.parent_station.clone(),
            stop.level_id.clone(),
            stop.platform_code.clone(),
        ])
        .map_err(|err| format!("Failed writing '{}': {err}", out_path.display()))?;
    }

    wtr.flush()
        .map_err(|err| format!("Failed flushing '{}': {err}", out_path.display()))
}

fn write_trips(
    output_root: &str,
    wiener_linien_source: &str,
    oebb_trips: &[OebbTrip],
) -> Result<(), String> {
    let out_path = path_for(output_root, TRIPS_FILE);
    let mut wtr = csv_writer(&out_path)?;
    wtr.write_record([
        "route_id",
        "service_id",
        "trip_id",
        "shape_id",
        "trip_headsign",
        "direction_id",
        "block_id",
    ])
    .map_err(|err| format!("Failed writing '{}': {err}", out_path.display()))?;

    let wl_path = path_for(wiener_linien_source, TRIPS_FILE);
    let mut wl_rdr = csv_reader(&wl_path)?;
    let wl_headers = wl_rdr
        .headers()
        .map_err(|err| format!("Failed reading '{}': {err}", wl_path.display()))?
        .clone();

    let wl_route_id_idx = header_index(&wl_headers, "route_id")?;
    let wl_service_id_idx = header_index(&wl_headers, "service_id")?;
    let wl_trip_id_idx = header_index(&wl_headers, "trip_id")?;
    let wl_shape_id_idx = header_index(&wl_headers, "shape_id")?;
    let wl_headsign_idx = header_index(&wl_headers, "trip_headsign")?;
    let wl_direction_idx = header_index(&wl_headers, "direction_id")?;
    let wl_block_idx = header_index(&wl_headers, "block_id")?;

    for rec in wl_rdr.records() {
        let rec = rec.map_err(|err| format!("Failed reading '{}': {err}", wl_path.display()))?;
        wtr.write_record([
            rec.get(wl_route_id_idx).unwrap_or_default().to_string(),
            rec.get(wl_service_id_idx).unwrap_or_default().to_string(),
            rec.get(wl_trip_id_idx).unwrap_or_default().to_string(),
            rec.get(wl_shape_id_idx).unwrap_or_default().to_string(),
            rec.get(wl_headsign_idx).unwrap_or_default().to_string(),
            rec.get(wl_direction_idx).unwrap_or_default().to_string(),
            rec.get(wl_block_idx).unwrap_or_default().to_string(),
        ])
        .map_err(|err| format!("Failed writing '{}': {err}", out_path.display()))?;
    }

    for trip in oebb_trips {
        wtr.write_record([
            prefixed_oebb_id(&trip.route_id),
            prefixed_oebb_id(&trip.service_id),
            prefixed_oebb_id(&trip.trip_id),
            trip.shape_id.clone(),
            trip.trip_headsign.clone(),
            trip.direction_id.clone(),
            trip.block_id.clone(),
        ])
        .map_err(|err| format!("Failed writing '{}': {err}", out_path.display()))?;
    }

    wtr.flush()
        .map_err(|err| format!("Failed flushing '{}': {err}", out_path.display()))
}

fn write_stop_times(
    output_root: &str,
    sources: MergeSources<'_>,
    kept_oebb_trip_ids: &HashSet<String>,
    scoped_stop_ids: &HashSet<String>,
) -> Result<(), String> {
    let out_path = path_for(output_root, STOP_TIMES_FILE);
    let mut wtr = csv_writer(&out_path)?;
    wtr.write_record([
        "trip_id",
        "arrival_time",
        "departure_time",
        "stop_id",
        "stop_sequence",
        "pickup_type",
        "drop_off_type",
        "shape_dist_traveled",
    ])
    .map_err(|err| format!("Failed writing '{}': {err}", out_path.display()))?;

    let wl_path = path_for(sources.wiener_linien_source, STOP_TIMES_FILE);
    let mut wl_rdr = csv_reader(&wl_path)?;
    let wl_headers = wl_rdr
        .headers()
        .map_err(|err| format!("Failed reading '{}': {err}", wl_path.display()))?
        .clone();

    let wl_trip_id_idx = header_index(&wl_headers, "trip_id")?;
    let wl_arrival_idx = header_index(&wl_headers, "arrival_time")?;
    let wl_departure_idx = header_index(&wl_headers, "departure_time")?;
    let wl_stop_id_idx = header_index(&wl_headers, "stop_id")?;
    let wl_sequence_idx = header_index(&wl_headers, "stop_sequence")?;
    let wl_pickup_idx = header_index(&wl_headers, "pickup_type")?;
    let wl_dropoff_idx = header_index(&wl_headers, "drop_off_type")?;
    let wl_dist_idx = header_index(&wl_headers, "shape_dist_traveled")?;

    for rec in wl_rdr.records() {
        let rec = rec.map_err(|err| format!("Failed reading '{}': {err}", wl_path.display()))?;
        wtr.write_record([
            rec.get(wl_trip_id_idx).unwrap_or_default().to_string(),
            rec.get(wl_arrival_idx).unwrap_or_default().to_string(),
            rec.get(wl_departure_idx).unwrap_or_default().to_string(),
            rec.get(wl_stop_id_idx).unwrap_or_default().to_string(),
            rec.get(wl_sequence_idx).unwrap_or_default().to_string(),
            rec.get(wl_pickup_idx).unwrap_or_default().to_string(),
            rec.get(wl_dropoff_idx).unwrap_or_default().to_string(),
            rec.get(wl_dist_idx).unwrap_or_default().to_string(),
        ])
        .map_err(|err| format!("Failed writing '{}': {err}", out_path.display()))?;
    }

    let oebb_path = path_for(sources.oebb_source, STOP_TIMES_FILE);
    let mut oebb_rdr = csv_reader(&oebb_path)?;
    let oebb_headers = oebb_rdr
        .headers()
        .map_err(|err| format!("Failed reading '{}': {err}", oebb_path.display()))?
        .clone();

    let oebb_trip_id_idx = header_index(&oebb_headers, "trip_id")?;
    let oebb_arrival_idx = header_index(&oebb_headers, "arrival_time")?;
    let oebb_departure_idx = header_index(&oebb_headers, "departure_time")?;
    let oebb_stop_id_idx = header_index(&oebb_headers, "stop_id")?;
    let oebb_sequence_idx = header_index(&oebb_headers, "stop_sequence")?;
    let oebb_pickup_idx = header_index(&oebb_headers, "pickup_type")?;
    let oebb_dropoff_idx = header_index(&oebb_headers, "drop_off_type")?;
    let oebb_dist_idx = header_index(&oebb_headers, "shape_dist_traveled")?;

    for rec in oebb_rdr.records() {
        let rec = rec.map_err(|err| format!("Failed reading '{}': {err}", oebb_path.display()))?;
        let trip_id = rec.get(oebb_trip_id_idx).unwrap_or_default();
        if !kept_oebb_trip_ids.contains(trip_id) {
            continue;
        }

        let stop_id = rec.get(oebb_stop_id_idx).unwrap_or_default();
        if !scoped_stop_ids.contains(stop_id) {
            continue;
        }

        wtr.write_record([
            prefixed_oebb_id(trip_id),
            rec.get(oebb_arrival_idx).unwrap_or_default().to_string(),
            rec.get(oebb_departure_idx).unwrap_or_default().to_string(),
            stop_id.to_string(),
            rec.get(oebb_sequence_idx).unwrap_or_default().to_string(),
            rec.get(oebb_pickup_idx).unwrap_or_default().to_string(),
            rec.get(oebb_dropoff_idx).unwrap_or_default().to_string(),
            rec.get(oebb_dist_idx).unwrap_or_default().to_string(),
        ])
        .map_err(|err| format!("Failed writing '{}': {err}", out_path.display()))?;
    }

    wtr.flush()
        .map_err(|err| format!("Failed flushing '{}': {err}", out_path.display()))
}

fn write_calendar(
    output_root: &str,
    sources: MergeSources<'_>,
    kept_oebb_service_ids: &HashSet<String>,
) -> Result<(), String> {
    let out_path = path_for(output_root, CALENDAR_FILE);
    let mut wtr = csv_writer(&out_path)?;
    wtr.write_record([
        "service_id",
        "monday",
        "tuesday",
        "wednesday",
        "thursday",
        "friday",
        "saturday",
        "sunday",
        "start_date",
        "end_date",
    ])
    .map_err(|err| format!("Failed writing '{}': {err}", out_path.display()))?;

    for source in [sources.wiener_linien_source, sources.oebb_source] {
        let path = path_for(source, CALENDAR_FILE);
        let mut rdr = csv_reader(&path)?;
        let headers = rdr
            .headers()
            .map_err(|err| format!("Failed reading '{}': {err}", path.display()))?
            .clone();

        let service_id_idx = header_index(&headers, "service_id")?;
        let monday_idx = header_index(&headers, "monday")?;
        let tuesday_idx = header_index(&headers, "tuesday")?;
        let wednesday_idx = header_index(&headers, "wednesday")?;
        let thursday_idx = header_index(&headers, "thursday")?;
        let friday_idx = header_index(&headers, "friday")?;
        let saturday_idx = header_index(&headers, "saturday")?;
        let sunday_idx = header_index(&headers, "sunday")?;
        let start_idx = header_index(&headers, "start_date")?;
        let end_idx = header_index(&headers, "end_date")?;

        for rec in rdr.records() {
            let rec = rec.map_err(|err| format!("Failed reading '{}': {err}", path.display()))?;
            let service_id = rec.get(service_id_idx).unwrap_or_default();
            if service_id.is_empty() {
                continue;
            }

            if source == sources.oebb_source && !kept_oebb_service_ids.contains(service_id) {
                continue;
            }

            let out_service_id = if source == sources.oebb_source {
                prefixed_oebb_id(service_id)
            } else {
                service_id.to_string()
            };

            wtr.write_record([
                out_service_id,
                rec.get(monday_idx).unwrap_or_default().to_string(),
                rec.get(tuesday_idx).unwrap_or_default().to_string(),
                rec.get(wednesday_idx).unwrap_or_default().to_string(),
                rec.get(thursday_idx).unwrap_or_default().to_string(),
                rec.get(friday_idx).unwrap_or_default().to_string(),
                rec.get(saturday_idx).unwrap_or_default().to_string(),
                rec.get(sunday_idx).unwrap_or_default().to_string(),
                rec.get(start_idx).unwrap_or_default().to_string(),
                rec.get(end_idx).unwrap_or_default().to_string(),
            ])
            .map_err(|err| format!("Failed writing '{}': {err}", out_path.display()))?;
        }
    }

    wtr.flush()
        .map_err(|err| format!("Failed flushing '{}': {err}", out_path.display()))
}

fn write_calendar_dates(
    output_root: &str,
    sources: MergeSources<'_>,
    kept_oebb_service_ids: &HashSet<String>,
) -> Result<(), String> {
    let out_path = path_for(output_root, CALENDAR_DATES_FILE);
    let mut wtr = csv_writer(&out_path)?;
    wtr.write_record(["service_id", "date", "exception_type"])
        .map_err(|err| format!("Failed writing '{}': {err}", out_path.display()))?;

    for source in [sources.wiener_linien_source, sources.oebb_source] {
        let path = path_for(source, CALENDAR_DATES_FILE);
        let mut rdr = csv_reader(&path)?;
        let headers = rdr
            .headers()
            .map_err(|err| format!("Failed reading '{}': {err}", path.display()))?
            .clone();

        let service_id_idx = header_index(&headers, "service_id")?;
        let date_idx = header_index(&headers, "date")?;
        let exception_idx = header_index(&headers, "exception_type")?;

        for rec in rdr.records() {
            let rec = rec.map_err(|err| format!("Failed reading '{}': {err}", path.display()))?;
            let service_id = rec.get(service_id_idx).unwrap_or_default();
            if service_id.is_empty() {
                continue;
            }
            if source == sources.oebb_source && !kept_oebb_service_ids.contains(service_id) {
                continue;
            }

            let out_service_id = if source == sources.oebb_source {
                prefixed_oebb_id(service_id)
            } else {
                service_id.to_string()
            };

            wtr.write_record([
                out_service_id,
                rec.get(date_idx).unwrap_or_default().to_string(),
                rec.get(exception_idx).unwrap_or_default().to_string(),
            ])
            .map_err(|err| format!("Failed writing '{}': {err}", out_path.display()))?;
        }
    }

    wtr.flush()
        .map_err(|err| format!("Failed flushing '{}': {err}", out_path.display()))
}

fn rebuild_combined_vienna_gtfs(
    output_root: &str,
    sources: MergeSources<'_>,
) -> Result<(), String> {
    fs::create_dir_all(output_root)
        .map_err(|err| format!("Failed to create output directory '{output_root}': {err}"))?;

    // Build filtered ÖBB subset first, then append it to Wiener Linien files.
    let (oebb_scoped_stops, scope) = load_oebb_scoped_stops(sources.oebb_source)?;
    let oebb_routes = load_oebb_routes(sources.oebb_source)?;
    let counts_by_trip = count_oebb_stops_per_trip(sources.oebb_source, &scope)?;
    let (oebb_trips, kept_trip_ids, kept_route_ids, kept_service_ids) =
        select_oebb_trips(sources.oebb_source, &oebb_routes, &counts_by_trip)?;

    let kept_oebb_agency_ids = write_routes(
        output_root,
        sources.wiener_linien_source,
        &oebb_routes,
        &kept_route_ids,
    )?;
    write_agency(output_root, sources, &kept_oebb_agency_ids)?;
    write_stops(
        output_root,
        sources.wiener_linien_source,
        &oebb_scoped_stops,
    )?;
    write_trips(output_root, sources.wiener_linien_source, &oebb_trips)?;
    write_stop_times(output_root, sources, &kept_trip_ids, &scope.scoped_stop_ids)?;
    write_calendar(output_root, sources, &kept_service_ids)?;
    write_calendar_dates(output_root, sources, &kept_service_ids)?;

    Ok(())
}

pub fn ensure_combined_source_ready(
    source_path: &str,
    wiener_linien_source: &str,
    oebb_source: &str,
) -> Result<(), String> {
    validate_raw_sources(wiener_linien_source, oebb_source)?;
    let sources = MergeSources {
        wiener_linien_source,
        oebb_source,
    };
    let fingerprint = required_sources_fingerprint(sources)?;
    let meta_path = path_for(source_path, META_FILE);
    // Rebuild only when source files changed or merged files are missing.
    if combined_outputs_exist(source_path)
        && meta_path.is_file()
        && fs::read_to_string(&meta_path).ok().as_deref() == Some(fingerprint.as_str())
    {
        return Ok(());
    }

    rebuild_combined_vienna_gtfs(source_path, sources)?;
    fs::write(&meta_path, fingerprint)
        .map_err(|err| format!("Failed writing '{}': {err}", meta_path.display()))
}
