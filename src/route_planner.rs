mod cache;
mod model;
mod output;
mod query;
mod raptor_adapter;

use crate::config::AppConfig;
use crate::geocode;
use crate::merge::ensure_combined_source_ready;
use chrono::NaiveDate;

pub use cache::rebuild_planner_cache;

#[derive(Debug, Clone)]
enum EndpointResolution {
    Coord { lat: f64, lon: f64 },
    Station { station_name: String, lat: f64, lon: f64 },
    Geocode {
        label: String,
        lat: f64,
        lon: f64,
        source: &'static str,
    },
}

fn parse_coord_pair(input: &str) -> Option<(f64, f64)> {
    let parts: Vec<&str> = input.split_whitespace().collect();
    if parts.len() != 2 {
        return None;
    }
    let lat = parts[0].parse::<f64>().ok()?;
    let lon = parts[1].parse::<f64>().ok()?;
    if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
        return None;
    }
    Some((lat, lon))
}

fn station_endpoint(
    cache: &model::PlannerCache,
    station_idxs: &[u32],
) -> Option<EndpointResolution> {
    for idx in station_idxs {
        let station = cache.stations.get(*idx as usize)?;
        if let (Some(lat), Some(lon)) = (station.centroid_lat, station.centroid_lon) {
            return Some(EndpointResolution::Station {
                station_name: station.name.clone(),
                lat,
                lon,
            });
        }
    }
    None
}

fn resolve_endpoint(
    cache: &model::PlannerCache,
    query_text: &str,
    station_idxs: &[u32],
    geocode_cache_path: &str,
) -> Result<EndpointResolution, String> {
    if let Some((lat, lon)) = parse_coord_pair(query_text) {
        return Ok(EndpointResolution::Coord { lat, lon });
    }

    if !station_idxs.is_empty() {
        return station_endpoint(cache, station_idxs).ok_or_else(|| {
            format!(
                "Route could not be planned because '{query_text}' matched a station but no station coordinates are available."
            )
        });
    }

    match geocode::lookup_first(geocode_cache_path, query_text) {
        Ok(Some(hit)) => Ok(EndpointResolution::Geocode {
            label: hit.label,
            lat: hit.lat,
            lon: hit.lon,
            source: hit.source,
        }),
        Ok(None) => Err(format!(
            "Route could not be planned because no station and no geocode match were found for '{query_text}'."
        )),
        Err(err) => Err(format!(
            "Route could not be planned because no station match was found for '{query_text}' and geocoding failed: {err}"
        )),
    }
}

pub fn cmd_route_plan(
    config: &AppConfig,
    from_query: &str,
    to_query: &str,
    debug: bool,
    verbose: bool,
    alternatives: usize,
    depart_secs: Option<usize>,
    service_date: Option<NaiveDate>,
) -> Result<(), String> {
    ensure_combined_source_ready(
        &config.merged_gtfs_path,
        &config.wiener_linien_source_dir,
        &config.oebb_source_dir,
    )?;
    let cache =
        cache::load_or_build_planner_cache(&config.merged_gtfs_path, &config.planner_cache_path)?;

    let from_station_idxs = query::match_station_idxs(&cache, from_query);
    let to_station_idxs = query::match_station_idxs(&cache, to_query);

    let mut result = if !from_station_idxs.is_empty() && !to_station_idxs.is_empty() {
        query::plan_route(
            &cache,
            from_query,
            to_query,
            alternatives,
            depart_secs,
            service_date,
        )?
    } else {
        let from_endpoint = resolve_endpoint(
            &cache,
            from_query,
            &from_station_idxs,
            &config.geocode_cache_path,
        )?;
        let to_endpoint = resolve_endpoint(
            &cache,
            to_query,
            &to_station_idxs,
            &config.geocode_cache_path,
        )?;

        if verbose {
            if let EndpointResolution::Geocode {
                label,
                lat,
                lon,
                source,
            } = &from_endpoint
            {
                eprintln!(
                    "Origin resolved via {source}: {label} -> {:.6} {:.6}",
                    lat, lon
                );
            }
            if let EndpointResolution::Station { station_name, .. } = &from_endpoint {
                eprintln!("Origin resolved as station: {station_name}");
            }
            if let EndpointResolution::Coord { lat, lon } = &from_endpoint {
                eprintln!("Origin resolved as coordinates: {:.6} {:.6}", lat, lon);
            }
            if let EndpointResolution::Geocode {
                label,
                lat,
                lon,
                source,
            } = &to_endpoint
            {
                eprintln!(
                    "Destination resolved via {source}: {label} -> {:.6} {:.6}",
                    lat, lon
                );
            }
            if let EndpointResolution::Station { station_name, .. } = &to_endpoint {
                eprintln!("Destination resolved as station: {station_name}");
            }
            if let EndpointResolution::Coord { lat, lon } = &to_endpoint {
                eprintln!("Destination resolved as coordinates: {:.6} {:.6}", lat, lon);
            }
        }

        let (from_lat, from_lon) = match from_endpoint {
            EndpointResolution::Coord { lat, lon } => (lat, lon),
            EndpointResolution::Station { lat, lon, .. } => (lat, lon),
            EndpointResolution::Geocode { lat, lon, .. } => (lat, lon),
        };
        let (to_lat, to_lon) = match to_endpoint {
            EndpointResolution::Coord { lat, lon } => (lat, lon),
            EndpointResolution::Station { lat, lon, .. } => (lat, lon),
            EndpointResolution::Geocode { lat, lon, .. } => (lat, lon),
        };

        query::plan_route_from_coords(
            &cache,
            from_lat,
            from_lon,
            to_lat,
            to_lon,
            alternatives,
            depart_secs,
            service_date,
        )?
    };

    result.from_query = from_query.to_string();
    result.to_query = to_query.to_string();
    output::print_route_plan(&cache, &result, debug, verbose);
    Ok(())
}
