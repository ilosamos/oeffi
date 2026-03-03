mod model;
mod normalize;

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufReader, BufWriter};
use std::path::Path;

use chrono::Utc;
use geo::{Contains, LineString, MultiPolygon, Point, Polygon};
use geojson::{GeoJson, Geometry, Value};
use osmpbfreader::{NodeId, OsmId, OsmObj, OsmPbfReader, WayId};
use strsim::levenshtein;

use self::model::{
    AddressRecord, GEOCODE_CACHE_VERSION, GeocodeBuildStats, GeocodeCache, LandmarkRecord,
};
use self::normalize::{
    canonical_street, normalize_ascii, normalized_address_key, strip_house_number_unit,
};

const EMBEDDED_VIENNA_POLYGON_PATH: &str = "embedded:assets/vienna-polygon.json";
const EMBEDDED_VIENNA_POLYGON_GEOJSON: &str = include_str!("../../assets/vienna-polygon.json");

#[derive(Debug, Clone)]
struct AddressAgg {
    street: String,
    house_number: String,
    postcode: Option<String>,
    city: Option<String>,
    lat_sum: f64,
    lon_sum: f64,
    count: u32,
}

#[derive(Debug, Clone)]
struct LandmarkAgg {
    name: String,
    kind: String,
    normalized_name: String,
    lat_sum: f64,
    lon_sum: f64,
    count: u32,
}

#[derive(Debug, Clone)]
struct PendingWayLandmark {
    way_id: WayId,
    name: String,
    kind: String,
}

#[derive(Debug, Clone)]
struct PendingRelationLandmark {
    name: String,
    kind: String,
    node_members: Vec<NodeId>,
    way_members: Vec<WayId>,
}

impl AddressAgg {
    fn add_sample(&mut self, lat: f64, lon: f64) {
        self.lat_sum += lat;
        self.lon_sum += lon;
        self.count += 1;
    }

    fn to_record(&self, normalized_key: String) -> AddressRecord {
        let count = self.count.max(1);
        AddressRecord {
            street: self.street.clone(),
            house_number: self.house_number.clone(),
            postcode: self.postcode.clone(),
            city: self.city.clone(),
            normalized_key,
            lat: self.lat_sum / count as f64,
            lon: self.lon_sum / count as f64,
            count,
        }
    }
}

impl LandmarkAgg {
    fn add_sample(&mut self, lat: f64, lon: f64) {
        self.lat_sum += lat;
        self.lon_sum += lon;
        self.count += 1;
    }

    fn to_record(&self) -> LandmarkRecord {
        let count = self.count.max(1);
        LandmarkRecord {
            name: self.name.clone(),
            kind: self.kind.clone(),
            normalized_name: self.normalized_name.clone(),
            lat: self.lat_sum / count as f64,
            lon: self.lon_sum / count as f64,
            count,
        }
    }
}

fn close_ring_if_needed(mut ring: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    if ring.len() >= 2 && ring.first() != ring.last() {
        ring.push(ring[0]);
    }
    ring
}

fn line_string_from_ring(ring: &[Vec<f64>]) -> Result<LineString<f64>, String> {
    if ring.len() < 3 {
        return Err("Polygon ring has fewer than 3 coordinates".to_string());
    }
    let mut coords = Vec::with_capacity(ring.len());
    for coord in ring {
        if coord.len() < 2 {
            return Err("Polygon coordinate must contain [lon, lat]".to_string());
        }
        coords.push((coord[0], coord[1]));
    }
    Ok(LineString::from(close_ring_if_needed(coords)))
}

fn polygon_from_coords(coords: &[Vec<Vec<f64>>]) -> Result<Polygon<f64>, String> {
    if coords.is_empty() {
        return Err("Polygon has no rings".to_string());
    }
    let exterior = line_string_from_ring(&coords[0])?;
    let mut interiors = Vec::new();
    for inner in coords.iter().skip(1) {
        interiors.push(line_string_from_ring(inner)?);
    }
    Ok(Polygon::new(exterior, interiors))
}

fn extract_polygons_from_geometry(geometry: &Geometry) -> Result<Vec<Polygon<f64>>, String> {
    match &geometry.value {
        Value::Polygon(coords) => Ok(vec![polygon_from_coords(coords)?]),
        Value::MultiPolygon(multi) => {
            let mut out = Vec::new();
            for coords in multi {
                out.push(polygon_from_coords(coords)?);
            }
            Ok(out)
        }
        other => Err(format!(
            "Unsupported geometry for boundary: {:?} (expected Polygon or MultiPolygon)",
            other
        )),
    }
}

fn load_polygon_from_geojson(raw: &str, label: &str) -> Result<MultiPolygon<f64>, String> {
    let geojson = raw
        .parse::<GeoJson>()
        .map_err(|err| format!("Failed to parse GeoJSON in '{label}': {err}"))?;

    let mut polygons = Vec::new();
    match geojson {
        GeoJson::FeatureCollection(fc) => {
            for feature in fc.features {
                if let Some(geometry) = feature.geometry {
                    polygons.extend(extract_polygons_from_geometry(&geometry)?);
                }
            }
        }
        GeoJson::Feature(feature) => {
            let geometry = feature
                .geometry
                .ok_or_else(|| format!("Feature in '{label}' has no geometry"))?;
            polygons.extend(extract_polygons_from_geometry(&geometry)?);
        }
        GeoJson::Geometry(geometry) => {
            polygons.extend(extract_polygons_from_geometry(&geometry)?);
        }
    }

    if polygons.is_empty() {
        return Err(format!("No polygon geometry found in '{label}'"));
    }
    Ok(MultiPolygon(polygons))
}

fn address_tags(
    tags: &osmpbfreader::Tags,
) -> Option<(String, String, Option<String>, Option<String>)> {
    let street = tags.get("addr:street")?.trim();
    let house_raw = tags.get("addr:housenumber")?.trim();
    if street.is_empty() || house_raw.is_empty() {
        return None;
    }

    let house_number = strip_house_number_unit(house_raw);
    if house_number.is_empty() {
        return None;
    }

    let postcode = tags
        .get("addr:postcode")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let city = tags
        .get("addr:city")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    Some((street.to_string(), house_number, postcode, city))
}

fn landmark_kind(tags: &osmpbfreader::Tags) -> Option<String> {
    for key in [
        "amenity",
        "tourism",
        "historic",
        "shop",
        "leisure",
        "public_transport",
        "railway",
        "building",
    ] {
        if let Some(value) = tags.get(key).map(|v| v.trim()).filter(|v| !v.is_empty()) {
            return Some(format!("{key}:{value}"));
        }
    }
    None
}

fn landmark_tags(tags: &osmpbfreader::Tags) -> Option<(String, String)> {
    let name = tags.get("name")?.trim();
    if name.is_empty() {
        return None;
    }
    let kind = landmark_kind(tags)?;
    Some((name.to_string(), kind))
}

fn collect_way_nodes(
    pbf_path: &str,
    target_way_ids: &HashSet<WayId>,
) -> Result<HashMap<WayId, Vec<NodeId>>, String> {
    if target_way_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let file = File::open(pbf_path)
        .map_err(|err| format!("Failed to open PBF '{pbf_path}' for way scan: {err}"))?;
    let mut reader = OsmPbfReader::new(file);

    let mut out: HashMap<WayId, Vec<NodeId>> = HashMap::new();
    for obj in reader.iter() {
        let obj =
            obj.map_err(|err| format!("Failed while scanning ways in '{pbf_path}': {err}"))?;
        if let OsmObj::Way(way) = obj
            && target_way_ids.contains(&way.id)
        {
            out.insert(way.id, way.nodes);
        }
    }
    Ok(out)
}

fn collect_node_coords(
    pbf_path: &str,
    target_node_ids: &HashSet<NodeId>,
) -> Result<HashMap<NodeId, (f64, f64)>, String> {
    if target_node_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let file = File::open(pbf_path)
        .map_err(|err| format!("Failed to open PBF '{pbf_path}' for node scan: {err}"))?;
    let mut reader = OsmPbfReader::new(file);

    let mut out: HashMap<NodeId, (f64, f64)> = HashMap::new();
    for obj in reader.iter() {
        let obj =
            obj.map_err(|err| format!("Failed while scanning nodes in '{pbf_path}': {err}"))?;
        if let OsmObj::Node(node) = obj
            && target_node_ids.contains(&node.id)
        {
            out.insert(node.id, (node.lat(), node.lon()));
        }
    }
    Ok(out)
}

fn centroid_from_node_ids(
    node_ids: &[NodeId],
    node_coords: &HashMap<NodeId, (f64, f64)>,
) -> Option<(f64, f64)> {
    let mut lat_sum = 0.0;
    let mut lon_sum = 0.0;
    let mut count = 0_u32;

    for node_id in node_ids {
        if let Some((lat, lon)) = node_coords.get(node_id) {
            lat_sum += *lat;
            lon_sum += *lon;
            count += 1;
        }
    }

    if count == 0 {
        None
    } else {
        Some((lat_sum / count as f64, lon_sum / count as f64))
    }
}

fn push_landmark_sample(
    landmarks_by_key: &mut HashMap<String, LandmarkAgg>,
    name: String,
    kind: String,
    lat: f64,
    lon: f64,
) {
    let normalized_name = normalize_ascii(&name);
    let key = format!("{}|{}", normalized_name, normalize_ascii(&kind));
    if key.trim().is_empty() {
        return;
    }
    let entry = landmarks_by_key.entry(key).or_insert_with(|| LandmarkAgg {
        name,
        kind,
        normalized_name,
        lat_sum: 0.0,
        lon_sum: 0.0,
        count: 0,
    });
    entry.add_sample(lat, lon);
}

fn save_cache(path: &str, cache: &GeocodeCache) -> Result<(), String> {
    if let Some(parent) = Path::new(path).parent() {
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "Failed to create cache directory '{}': {err}",
                parent.display()
            )
        })?;
    }
    let file = File::create(path)
        .map_err(|err| format!("Failed to create geocode cache '{path}': {err}"))?;
    let mut writer = BufWriter::new(file);
    bincode::serialize_into(&mut writer, cache)
        .map_err(|err| format!("Failed to serialize geocode cache '{path}': {err}"))
}

fn load_cache(path: &str) -> Result<GeocodeCache, String> {
    let file =
        File::open(path).map_err(|err| format!("Failed to open geocode cache '{path}': {err}"))?;
    let mut reader = BufReader::new(file);
    let cache: GeocodeCache = bincode::deserialize_from(&mut reader)
        .map_err(|err| format!("Failed to deserialize geocode cache '{path}': {err}"))?;
    if cache.version != GEOCODE_CACHE_VERSION {
        return Err(format!(
            "Unsupported geocode cache version {} in '{}', expected {}",
            cache.version, path, GEOCODE_CACHE_VERSION
        ));
    }
    Ok(cache)
}

pub fn cmd_geocode_build(pbf_path: &str, out_path: &str) -> Result<(), String> {
    eprintln!("Loading embedded Vienna polygon ({EMBEDDED_VIENNA_POLYGON_PATH}) ...");
    let polygon = load_polygon_from_geojson(
        EMBEDDED_VIENNA_POLYGON_GEOJSON,
        EMBEDDED_VIENNA_POLYGON_PATH,
    )?;

    eprintln!("Scanning OSM PBF from {pbf_path} ...");
    let file =
        File::open(pbf_path).map_err(|err| format!("Failed to open PBF '{pbf_path}': {err}"))?;
    let mut reader = OsmPbfReader::new(file);

    let mut stats = GeocodeBuildStats {
        objects_total: 0,
        nodes_total: 0,
        ways_total: 0,
        relations_total: 0,
        addr_nodes_total: 0,
        addr_ways_total: 0,
        addr_relations_total: 0,
        addr_nodes_in_polygon: 0,
        unique_addresses: 0,
        named_nodes_total: 0,
        named_nodes_in_polygon: 0,
        landmark_nodes_total: 0,
        landmark_nodes_in_polygon: 0,
        landmark_ways_total: 0,
        landmark_ways_in_polygon: 0,
        landmark_relations_total: 0,
        landmark_relations_in_polygon: 0,
        unique_landmarks: 0,
    };

    let mut by_key: HashMap<String, AddressAgg> = HashMap::new();
    let mut landmarks_by_key: HashMap<String, LandmarkAgg> = HashMap::new();
    let mut pending_way_landmarks: Vec<PendingWayLandmark> = Vec::new();
    let mut pending_relation_landmarks: Vec<PendingRelationLandmark> = Vec::new();
    let mut way_ids_needed: HashSet<WayId> = HashSet::new();

    // First version indexes addr:* from nodes only; ways/relations are counted for visibility.
    for obj in reader.iter() {
        let obj = obj.map_err(|err| format!("Failed while reading PBF '{pbf_path}': {err}"))?;
        stats.objects_total += 1;

        match obj {
            OsmObj::Node(node) => {
                stats.nodes_total += 1;
                let point = Point::new(node.lon(), node.lat());

                if node.tags.get("name").is_some() {
                    stats.named_nodes_total += 1;
                    if polygon.contains(&point) {
                        stats.named_nodes_in_polygon += 1;
                    }
                }

                if let Some((name, kind)) = landmark_tags(&node.tags) {
                    stats.landmark_nodes_total += 1;
                    if polygon.contains(&point) {
                        stats.landmark_nodes_in_polygon += 1;
                        push_landmark_sample(
                            &mut landmarks_by_key,
                            name,
                            kind,
                            point.y(),
                            point.x(),
                        );
                    }
                }

                let Some((street, house_number, postcode, city)) = address_tags(&node.tags) else {
                    continue;
                };
                stats.addr_nodes_total += 1;
                if !polygon.contains(&point) {
                    continue;
                }
                stats.addr_nodes_in_polygon += 1;

                let key = normalized_address_key(&street, &house_number, postcode.as_deref());
                if key.is_empty() {
                    continue;
                }

                // Deduplicate by normalized address key and average coordinates for stable lookup.
                let entry = by_key.entry(key).or_insert_with(|| AddressAgg {
                    street,
                    house_number,
                    postcode,
                    city,
                    lat_sum: 0.0,
                    lon_sum: 0.0,
                    count: 0,
                });
                entry.add_sample(point.y(), point.x());
            }
            OsmObj::Way(way) => {
                stats.ways_total += 1;
                if address_tags(&way.tags).is_some() {
                    stats.addr_ways_total += 1;
                }
                if let Some((name, kind)) = landmark_tags(&way.tags) {
                    stats.landmark_ways_total += 1;
                    way_ids_needed.insert(way.id);
                    pending_way_landmarks.push(PendingWayLandmark {
                        way_id: way.id,
                        name,
                        kind,
                    });
                }
            }
            OsmObj::Relation(relation) => {
                stats.relations_total += 1;
                if address_tags(&relation.tags).is_some() {
                    stats.addr_relations_total += 1;
                }
                if let Some((name, kind)) = landmark_tags(&relation.tags) {
                    stats.landmark_relations_total += 1;
                    let mut node_members: Vec<NodeId> = Vec::new();
                    let mut way_members: Vec<WayId> = Vec::new();
                    for member in &relation.refs {
                        match member.member {
                            OsmId::Node(node_id) => node_members.push(node_id),
                            OsmId::Way(way_id) => {
                                way_ids_needed.insert(way_id);
                                way_members.push(way_id);
                            }
                            OsmId::Relation(_) => {}
                        }
                    }
                    pending_relation_landmarks.push(PendingRelationLandmark {
                        name,
                        kind,
                        node_members,
                        way_members,
                    });
                }
            }
        }
    }

    eprintln!(
        "Resolving landmark geometry for {} ways and {} relations ...",
        pending_way_landmarks.len(),
        pending_relation_landmarks.len()
    );
    let way_nodes = collect_way_nodes(pbf_path, &way_ids_needed)?;

    let mut needed_node_ids: HashSet<NodeId> = HashSet::new();
    for node_ids in way_nodes.values() {
        needed_node_ids.extend(node_ids.iter().copied());
    }
    for relation in &pending_relation_landmarks {
        needed_node_ids.extend(relation.node_members.iter().copied());
    }
    let node_coords = collect_node_coords(pbf_path, &needed_node_ids)?;

    for pending in pending_way_landmarks {
        let Some(node_ids) = way_nodes.get(&pending.way_id) else {
            continue;
        };
        let Some((lat, lon)) = centroid_from_node_ids(node_ids, &node_coords) else {
            continue;
        };
        if polygon.contains(&Point::new(lon, lat)) {
            stats.landmark_ways_in_polygon += 1;
            push_landmark_sample(&mut landmarks_by_key, pending.name, pending.kind, lat, lon);
        }
    }

    for pending in pending_relation_landmarks {
        let mut coords: Vec<(f64, f64)> = Vec::new();

        for node_id in &pending.node_members {
            if let Some((lat, lon)) = node_coords.get(node_id) {
                coords.push((*lat, *lon));
            }
        }
        for way_id in &pending.way_members {
            if let Some(node_ids) = way_nodes.get(way_id) {
                for node_id in node_ids {
                    if let Some((lat, lon)) = node_coords.get(node_id) {
                        coords.push((*lat, *lon));
                    }
                }
            }
        }

        if coords.is_empty() {
            continue;
        }
        let (mut lat_sum, mut lon_sum) = (0.0_f64, 0.0_f64);
        for (lat, lon) in &coords {
            lat_sum += *lat;
            lon_sum += *lon;
        }
        let lat = lat_sum / coords.len() as f64;
        let lon = lon_sum / coords.len() as f64;

        if polygon.contains(&Point::new(lon, lat)) {
            stats.landmark_relations_in_polygon += 1;
            push_landmark_sample(&mut landmarks_by_key, pending.name, pending.kind, lat, lon);
        }
    }

    let mut records: Vec<AddressRecord> = by_key
        .into_iter()
        .map(|(key, agg)| agg.to_record(key))
        .collect();

    let mut landmarks: Vec<LandmarkRecord> = landmarks_by_key
        .into_values()
        .map(|agg| agg.to_record())
        .collect();

    records.sort_by(|a, b| a.normalized_key.cmp(&b.normalized_key));
    landmarks.sort_by(|a, b| a.normalized_name.cmp(&b.normalized_name));
    stats.unique_addresses = records.len() as u64;
    stats.unique_landmarks = landmarks.len() as u64;

    let cache = GeocodeCache {
        version: GEOCODE_CACHE_VERSION,
        built_unix_ts: Utc::now().timestamp(),
        source_pbf: pbf_path.to_string(),
        polygon_path: EMBEDDED_VIENNA_POLYGON_PATH.to_string(),
        stats,
        addresses: records,
        landmarks,
    };

    save_cache(out_path, &cache)?;

    println!("Built geocode cache: {out_path}");
    println!("  unique addresses: {}", cache.stats.unique_addresses);
    println!("  unique landmarks: {}", cache.stats.unique_landmarks);
    println!(
        "  addr nodes in polygon: {}",
        cache.stats.addr_nodes_in_polygon
    );
    println!("  addr nodes total: {}", cache.stats.addr_nodes_total);
    println!(
        "  landmark nodes in polygon: {}",
        cache.stats.landmark_nodes_in_polygon
    );
    println!(
        "  landmark nodes total: {}",
        cache.stats.landmark_nodes_total
    );
    println!(
        "  landmark ways in polygon: {}",
        cache.stats.landmark_ways_in_polygon
    );
    println!("  landmark ways total: {}", cache.stats.landmark_ways_total);
    println!(
        "  landmark relations in polygon: {}",
        cache.stats.landmark_relations_in_polygon
    );
    println!(
        "  landmark relations total: {}",
        cache.stats.landmark_relations_total
    );
    println!(
        "  addr ways total (not indexed yet): {}",
        cache.stats.addr_ways_total
    );
    println!(
        "  addr relations total (not indexed yet): {}",
        cache.stats.addr_relations_total
    );

    Ok(())
}

fn format_record(record: &AddressRecord) -> String {
    match (&record.postcode, &record.city) {
        (Some(postcode), Some(city)) => format!(
            "{} {} | {} {}",
            record.street, record.house_number, postcode, city
        ),
        (Some(postcode), None) => {
            format!("{} {} | {}", record.street, record.house_number, postcode)
        }
        _ => format!("{} {}", record.street, record.house_number),
    }
}

#[derive(Debug, Clone)]
struct ParsedHouse {
    normalized: String,
    digits: Option<String>,
    suffix: Option<String>,
}

#[derive(Debug, Clone)]
struct ParsedQuery {
    street: String,
    house: Option<ParsedHouse>,
    raw_normalized: String,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
enum StreetMatch {
    Exact = 0,
    Fuzzy = 1,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
enum HouseMatch {
    Exact = 0,
    Family = 1,
}

fn split_house_parts(value: &str) -> (Option<String>, Option<String>) {
    let norm = normalize_ascii(value);
    if norm.is_empty() {
        return (None, None);
    }

    let compact = norm.replace(' ', "");
    let mut digits_end = 0;
    for (idx, ch) in compact.char_indices() {
        if ch.is_ascii_digit() {
            digits_end = idx + ch.len_utf8();
        } else {
            break;
        }
    }

    if digits_end == 0 {
        return (None, Some(compact));
    }

    let digits = compact[..digits_end].to_string();
    let suffix = compact[digits_end..].trim();
    let suffix = if suffix.is_empty() {
        None
    } else {
        Some(suffix.to_string())
    };
    (Some(digits), suffix)
}

fn parse_house(value: &str) -> Option<ParsedHouse> {
    let normalized = normalize_ascii(value);
    if normalized.is_empty() {
        return None;
    }
    let (digits, suffix) = split_house_parts(&normalized);
    Some(ParsedHouse {
        normalized: normalized.replace(' ', ""),
        digits,
        suffix,
    })
}

fn parse_query(query: &str) -> Option<ParsedQuery> {
    let norm = normalize_ascii(query);
    if norm.is_empty() {
        return None;
    }

    let tokens: Vec<&str> = norm.split_whitespace().collect();
    if tokens.is_empty() {
        return None;
    }

    let last = *tokens.last().unwrap_or(&"");
    let house = parse_house(last).filter(|h| h.digits.is_some());

    let street_raw = if house.is_some() && tokens.len() > 1 {
        tokens[..tokens.len() - 1].join(" ")
    } else {
        norm.clone()
    };
    let street = canonical_street(&street_raw);
    if street.is_empty() {
        return None;
    }

    Some(ParsedQuery {
        street,
        house,
        raw_normalized: norm,
    })
}

fn street_match_kind(query_street: &str, record_street: &str) -> Option<StreetMatch> {
    let record_street_norm = canonical_street(record_street);
    if record_street_norm == query_street {
        return Some(StreetMatch::Exact);
    }
    if record_street_norm.starts_with(query_street) || query_street.starts_with(&record_street_norm)
    {
        return Some(StreetMatch::Fuzzy);
    }

    let dist = levenshtein(query_street, &record_street_norm);
    let max_dist = if query_street.len() >= 12 { 2 } else { 1 };
    if dist <= max_dist {
        Some(StreetMatch::Fuzzy)
    } else {
        None
    }
}

fn house_match_kind(query_house: &ParsedHouse, record_house_raw: &str) -> Option<HouseMatch> {
    let record_house = parse_house(record_house_raw)?;

    if record_house.normalized == query_house.normalized {
        return Some(HouseMatch::Exact);
    }

    match (
        query_house.digits.as_deref(),
        query_house.suffix.as_deref(),
        record_house.digits.as_deref(),
        record_house.suffix.as_deref(),
    ) {
        (Some(qd), None, Some(rd), Some(rs)) if qd == rd && !rs.is_empty() => {
            Some(HouseMatch::Family)
        }
        _ => None,
    }
}

fn rank_record(record: &AddressRecord, query: &ParsedQuery) -> Option<(u8, u32)> {
    let street = street_match_kind(&query.street, &record.street)?;

    match &query.house {
        Some(house_query) => {
            let house = house_match_kind(house_query, &record.house_number)?;
            let tier = match (street, house) {
                (StreetMatch::Exact, HouseMatch::Exact) => 0,
                (StreetMatch::Exact, HouseMatch::Family) => 1,
                (StreetMatch::Fuzzy, HouseMatch::Exact) => 2,
                (StreetMatch::Fuzzy, HouseMatch::Family) => 3,
            };
            Some((tier, record.count))
        }
        None => {
            let tier = match street {
                StreetMatch::Exact => 4,
                StreetMatch::Fuzzy => 5,
            };
            Some((tier, record.count))
        }
    }
}

fn rank_landmark_strict(record: &LandmarkRecord, query_norm: &str) -> Option<(u8, u32)> {
    if query_norm.is_empty() {
        return None;
    }

    if record.normalized_name == query_norm {
        return Some((0, record.count));
    }

    if record
        .normalized_name
        .split_whitespace()
        .last()
        .is_some_and(|last| last == query_norm)
    {
        return Some((1, record.count));
    }
    None
}

fn rank_landmark_fuzzy(record: &LandmarkRecord, query_norm: &str) -> Option<(u8, u32)> {
    if query_norm.is_empty() {
        return None;
    }

    if record.normalized_name == query_norm {
        return Some((2, record.count));
    }
    if record.normalized_name.starts_with(query_norm) {
        return Some((3, record.count));
    }
    if record.normalized_name.contains(query_norm) {
        return Some((4, record.count));
    }

    let dist = levenshtein(query_norm, &record.normalized_name);
    let max_dist = if query_norm.len() >= 10 { 2 } else { 1 };
    if dist <= max_dist {
        Some((5, record.count))
    } else {
        None
    }
}

fn format_landmark(record: &LandmarkRecord) -> String {
    format!("{} [{}]", record.name, record.kind)
}

#[derive(Debug, Clone)]
pub struct GeocodeLookupHit {
    pub label: String,
    pub lat: f64,
    pub lon: f64,
    pub source: &'static str,
}

#[derive(Debug, Clone)]
pub struct GeocodeSummary {
    pub version: u32,
    pub unique_addresses: u64,
    pub unique_landmarks: u64,
    pub source_pbf: String,
    pub polygon_path: String,
}

pub fn load_summary(cache_path: &str) -> Result<GeocodeSummary, String> {
    let cache = load_cache(cache_path)?;
    Ok(GeocodeSummary {
        version: cache.version,
        unique_addresses: cache.stats.unique_addresses,
        unique_landmarks: cache.stats.unique_landmarks,
        source_pbf: cache.source_pbf,
        polygon_path: cache.polygon_path,
    })
}

fn sorted_address_matches<'a>(
    cache: &'a GeocodeCache,
    parsed_query: &ParsedQuery,
) -> Vec<&'a AddressRecord> {
    let mut matches: Vec<(u8, &AddressRecord)> = cache
        .addresses
        .iter()
        .filter_map(|record| rank_record(record, parsed_query).map(|(score, _)| (score, record)))
        .collect();

    matches.sort_by(|(score_a, a), (score_b, b)| {
        score_a
            .cmp(score_b)
            .then_with(|| b.count.cmp(&a.count))
            .then_with(|| a.normalized_key.cmp(&b.normalized_key))
    });

    matches.into_iter().map(|(_, r)| r).collect()
}

fn sorted_landmark_matches<'a>(
    cache: &'a GeocodeCache,
    query_norm: &str,
) -> Vec<&'a LandmarkRecord> {
    let strict_landmark_matches: Vec<(u8, &LandmarkRecord)> = cache
        .landmarks
        .iter()
        .filter_map(|record| {
            rank_landmark_strict(record, query_norm).map(|(score, _)| (score, record))
        })
        .collect();

    let has_exact_landmark = strict_landmark_matches.iter().any(|(score, _)| *score == 0);

    let mut landmark_matches: Vec<(u8, &LandmarkRecord)> = if has_exact_landmark {
        strict_landmark_matches
            .into_iter()
            .filter(|(score, _)| *score == 0)
            .collect()
    } else {
        strict_landmark_matches
    };

    if landmark_matches.is_empty() {
        landmark_matches = cache
            .landmarks
            .iter()
            .filter_map(|record| {
                rank_landmark_fuzzy(record, query_norm).map(|(score, _)| (score, record))
            })
            .collect();
    }

    landmark_matches.sort_by(|(score_a, a), (score_b, b)| {
        score_a
            .cmp(score_b)
            .then_with(|| b.count.cmp(&a.count))
            .then_with(|| a.normalized_name.cmp(&b.normalized_name))
    });

    landmark_matches.into_iter().map(|(_, r)| r).collect()
}

pub fn lookup_first(cache_path: &str, query: &str) -> Result<Option<GeocodeLookupHit>, String> {
    let cache = load_cache(cache_path)?;
    let parsed_query =
        parse_query(query).ok_or_else(|| "query normalizes to empty string".to_string())?;
    let query_norm = normalize_ascii(query);

    let address_matches = sorted_address_matches(&cache, &parsed_query);
    if let Some(first) = address_matches.first() {
        return Ok(Some(GeocodeLookupHit {
            label: format_record(first),
            lat: first.lat,
            lon: first.lon,
            source: "address",
        }));
    }

    let landmark_matches = sorted_landmark_matches(&cache, &query_norm);
    if let Some(first) = landmark_matches.first() {
        return Ok(Some(GeocodeLookupHit {
            label: format_landmark(first),
            lat: first.lat,
            lon: first.lon,
            source: "landmark",
        }));
    }

    Ok(None)
}

pub fn cmd_geocode_find(cache_path: &str, query: &str, limit: usize) -> Result<(), String> {
    let cache = load_cache(cache_path)?;
    if limit == 0 {
        return Err("--limit must be > 0".to_string());
    }

    let parsed_query =
        parse_query(query).ok_or_else(|| "query normalizes to empty string".to_string())?;

    let matches = sorted_address_matches(&cache, &parsed_query);

    let query_norm = normalize_ascii(query);
    let landmark_matches = sorted_landmark_matches(&cache, &query_norm);

    if matches.is_empty() && landmark_matches.is_empty() {
        println!("No geocode match for '{query}' in {cache_path}");
        return Ok(());
    }

    if !matches.is_empty() {
        println!(
            "Address matches for '{query}' (normalized: '{}') in {cache_path}:",
            parsed_query.raw_normalized
        );

        for (idx, record) in matches.into_iter().take(limit).enumerate() {
            println!(
                "  {}. {} -> {:.6} {:.6} (samples: {})",
                idx + 1,
                format_record(record),
                record.lat,
                record.lon,
                record.count
            );
        }
    }

    if !landmark_matches.is_empty() {
        println!("Landmark matches for '{query}' (normalized: '{query_norm}') in {cache_path}:");

        for (idx, record) in landmark_matches.into_iter().take(limit).enumerate() {
            println!(
                "  {}. {} -> {:.6} {:.6} (samples: {})",
                idx + 1,
                format_landmark(record),
                record.lat,
                record.lon,
                record.count
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(street: &str, house: &str, count: u32) -> AddressRecord {
        AddressRecord {
            street: street.to_string(),
            house_number: house.to_string(),
            postcode: Some("1040".to_string()),
            city: Some("Wien".to_string()),
            normalized_key: normalized_address_key(street, house, Some("1040")),
            lat: 48.0,
            lon: 16.0,
            count,
        }
    }

    fn ranked<'a>(records: &'a [AddressRecord], query: &str) -> Vec<&'a AddressRecord> {
        let q = parse_query(query).expect("query parse");
        let mut v: Vec<(u8, &AddressRecord)> = records
            .iter()
            .filter_map(|r| rank_record(r, &q).map(|(tier, _)| (tier, r)))
            .collect();
        v.sort_by(|(tier_a, a), (tier_b, b)| {
            tier_a
                .cmp(tier_b)
                .then_with(|| b.count.cmp(&a.count))
                .then_with(|| a.normalized_key.cmp(&b.normalized_key))
        });
        v.into_iter().map(|(_, r)| r).collect()
    }

    fn landmark(name: &str, kind: &str, count: u32) -> LandmarkRecord {
        LandmarkRecord {
            name: name.to_string(),
            kind: kind.to_string(),
            normalized_name: normalize_ascii(name),
            lat: 48.0,
            lon: 16.0,
            count,
        }
    }

    #[test]
    fn strict_house_number_avoids_24_for_query_2() {
        let records = vec![
            rec("Prinz-Eugen-Straße", "2", 10),
            rec("Prinz-Eugen-Straße", "24", 10),
            rec("Prinz-Eugen-Straße", "28", 10),
            rec("Prinz-Eugen-Straße", "2A", 8),
        ];

        let out = ranked(&records, "prinz-eugen-straße 2");
        assert_eq!(out[0].house_number, "2");
        assert!(
            out.iter()
                .all(|r| r.house_number != "24" && r.house_number != "28")
        );
    }

    #[test]
    fn dashed_and_spaced_street_variants_match() {
        let records = vec![rec("Prinz-Eugen-Straße", "2", 10)];
        assert!(!ranked(&records, "prinz-eugen-straße 2").is_empty());
        assert!(!ranked(&records, "prinz eugen straße 2").is_empty());
        assert!(!ranked(&records, "prinz-eugen straße 2").is_empty());
    }

    #[test]
    fn exact_house_beats_suffix_variants() {
        let records = vec![
            rec("Lassallestraße", "7A", 50),
            rec("Lassallestraße", "7B", 40),
            rec("Lassallestraße", "7", 5),
        ];
        let out = ranked(&records, "lassallestraße 7");
        assert_eq!(out[0].house_number, "7");
    }

    #[test]
    fn supports_common_typos_and_abbreviation() {
        let records = vec![rec("Lassallestraße", "7", 10)];
        assert_eq!(ranked(&records, "lasallestraße 7")[0].house_number, "7");
        assert_eq!(ranked(&records, "lassallestr 7")[0].house_number, "7");
    }

    #[test]
    fn matches_landmark_name_with_typo() {
        let l = landmark("Stephansdom", "tourism:attraction", 1);
        assert!(rank_landmark_strict(&l, "stephansdom").is_some());
        assert!(rank_landmark_fuzzy(&l, "stephansdum").is_some());
    }

    #[test]
    fn strict_landmark_mode_prefers_exact_when_available() {
        let main = landmark("Stephansdom", "amenity:place_of_worship", 1);
        let child = landmark("Stephansdom Krypta", "tourism:attraction", 1);
        let unrelated_prefix = landmark("Stephansdomplatz", "highway:pedestrian", 1);

        assert!(rank_landmark_strict(&main, "stephansdom").is_some());
        assert!(rank_landmark_strict(&child, "stephansdom").is_none());
        assert!(rank_landmark_strict(&unrelated_prefix, "stephansdom").is_none());
    }
}
