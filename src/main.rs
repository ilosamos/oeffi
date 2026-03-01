mod build;
mod cache;
mod cli;
mod commands;
mod snapshot;

use std::env;
use std::process::ExitCode;

use cli::{Command, DEFAULT_GTFS_PATH, USAGE, parse_command};

fn run(command: Command) -> ExitCode {
    let result = match command {
        Command::Hello => {
            println!("hello world");
            Ok(())
        }
        Command::GtfsSummary => commands::cmd_gtfs_summary(DEFAULT_GTFS_PATH),
        Command::ListRoutes => commands::cmd_list_routes(DEFAULT_GTFS_PATH),
        Command::RouteStops { route, show_all } => {
            commands::cmd_route_stops(DEFAULT_GTFS_PATH, &route, show_all)
        }
        Command::StopInspect { query } => commands::cmd_stop_inspect(DEFAULT_GTFS_PATH, &query),
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
