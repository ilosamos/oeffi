mod cache;
mod model;
mod output;
mod query;
mod raptor_adapter;

use crate::cli::DEFAULT_GTFS_PATH;

pub use cache::rebuild_planner_cache;

pub fn cmd_route_plan(
    from_query: &str,
    to_query: &str,
    debug: bool,
    alternatives: usize,
) -> Result<(), String> {
    let cache = cache::load_or_build_planner_cache(DEFAULT_GTFS_PATH)?;
    let result = query::plan_route(&cache, from_query, to_query, alternatives)?;
    output::print_route_plan(&cache, &result, debug);
    Ok(())
}
