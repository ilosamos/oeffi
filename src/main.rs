mod build;
mod cache;
mod cli;
mod commands;
mod snapshot;

use std::env;
use std::process::ExitCode;

use cli::{Command, USAGE, parse_command};

fn run(command: Command) -> ExitCode {
    let result = match command {
        Command::Hello => {
            println!("hello world");
            Ok(())
        }
        Command::GtfsSummary { source_path } => commands::cmd_gtfs_summary(&source_path),
        Command::ListRoutes { source_path } => commands::cmd_list_routes(&source_path),
        Command::RouteStops { route, source_path } => {
            commands::cmd_route_stops(&source_path, &route)
        }
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
            eprintln!("Error: {message}\n");
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();

    match parse_command(&args) {
        Ok(command) => run(command),
        Err(message) => {
            eprintln!("Error: {message}\n");
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}
