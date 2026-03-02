mod build;
mod cache;
mod cache_meta;
mod cli;
mod clustering;
mod commands;
mod matcher;
mod merge;
mod route_planner;
mod snapshot;

use std::env;
use std::process::ExitCode;

use cli::{Command, DEFAULT_GTFS_PATH, USAGE, parse_command};

fn run(command: Command) -> ExitCode {
    let result = match command {
        Command::Summary => commands::cmd_gtfs_summary(DEFAULT_GTFS_PATH),
        Command::ListRoutes => commands::cmd_list_routes(DEFAULT_GTFS_PATH),
        Command::ListStops => commands::cmd_list_stops(DEFAULT_GTFS_PATH),
        Command::Route {
            from,
            to,
            debug,
            alternatives,
            depart_secs,
            service_date,
        } => route_planner::cmd_route_plan(
            &from,
            &to,
            debug,
            alternatives,
            depart_secs,
            service_date,
        ),
        Command::RouteCoords {
            from_lat,
            from_lon,
            to_lat,
            to_lon,
            debug,
            alternatives,
            depart_secs,
            service_date,
        } => route_planner::cmd_route_plan_coords(
            from_lat,
            from_lon,
            to_lat,
            to_lon,
            debug,
            alternatives,
            depart_secs,
            service_date,
        ),
        Command::Line { route } => commands::cmd_route_stops(DEFAULT_GTFS_PATH, &route),
        Command::Inspect { query } => commands::cmd_stop_inspect(DEFAULT_GTFS_PATH, &query),
        Command::CacheBuild {
            source_path,
            cache_path,
        } => commands::cmd_cache_build(&source_path, &cache_path),
        Command::Help => {
            println!("{USAGE}");
            Ok(())
        }
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("Error: {message}");
            eprintln!("Run `oeffi help` for usage.");
            ExitCode::from(2)
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();

    match parse_command(&args) {
        Ok(command) => run(command),
        Err(message) => {
            eprintln!("Error: {message}");
            eprintln!("Run `oeffi help` for usage.");
            ExitCode::from(2)
        }
    }
}
