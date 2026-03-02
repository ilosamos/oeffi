mod cache;
mod model;
mod output;
mod query;
mod raptor_adapter;

use crate::cli::DEFAULT_GTFS_PATH;
use crate::merge::ensure_combined_source_ready;
use chrono::NaiveDate;

pub use cache::rebuild_planner_cache;

pub fn cmd_route_plan(
    from_query: &str,
    to_query: &str,
    debug: bool,
    alternatives: usize,
    depart_secs: Option<usize>,
    service_date: Option<NaiveDate>,
) -> Result<(), String> {
    ensure_combined_source_ready(DEFAULT_GTFS_PATH)?;
    let cache = cache::load_or_build_planner_cache(DEFAULT_GTFS_PATH)?;
    let result = query::plan_route(
        &cache,
        from_query,
        to_query,
        alternatives,
        depart_secs,
        service_date,
    )?;
    output::print_route_plan(&cache, &result, debug);
    Ok(())
}

pub fn cmd_route_plan_coords(
    from_lat: f64,
    from_lon: f64,
    to_lat: f64,
    to_lon: f64,
    debug: bool,
    alternatives: usize,
    depart_secs: Option<usize>,
    service_date: Option<NaiveDate>,
) -> Result<(), String> {
    ensure_combined_source_ready(DEFAULT_GTFS_PATH)?;
    let cache = cache::load_or_build_planner_cache(DEFAULT_GTFS_PATH)?;
    let result = query::plan_route_from_coords(
        &cache,
        from_lat,
        from_lon,
        to_lat,
        to_lon,
        alternatives,
        depart_secs,
        service_date,
    )?;
    output::print_route_plan(&cache, &result, debug);
    Ok(())
}
