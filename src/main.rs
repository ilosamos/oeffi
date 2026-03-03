mod build;
mod cache;
mod cache_meta;
mod cli;
mod clustering;
mod commands;
mod config;
mod download;
mod geocode;
mod matcher;
mod merge;
mod route_planner;
mod snapshot;

use std::env;
use std::process::ExitCode;

use cli::{APP_VERSION, Command, is_help_error, parse_command, render_help};
use config::load_or_init_config;

fn run(command: Command) -> ExitCode {
    let loaded = match load_or_init_config() {
        Ok(cfg) => cfg,
        Err(message) => {
            eprintln!("Error: {message}");
            return ExitCode::from(2);
        }
    };
    let cfg = &loaded.effective_config;

    let result = match command {
        Command::Summary => commands::cmd_gtfs_summary(cfg),
        Command::ListRoutes => commands::cmd_list_routes(cfg),
        Command::ListStops => commands::cmd_list_stops(cfg),
        Command::Route {
            from,
            to,
            debug,
            verbose,
            alternatives,
            depart_secs,
            service_date,
        } => route_planner::cmd_route_plan(
            cfg,
            route_planner::RoutePlanRequest {
                from_query: &from,
                to_query: &to,
                debug,
                verbose,
                alternatives,
                depart_secs,
                service_date,
            },
        ),
        Command::Line { route } => commands::cmd_route_stops(cfg, &route),
        Command::Inspect { query } => commands::cmd_stop_inspect(cfg, &query),
        Command::CacheBuild { download } => commands::cmd_cache_build(cfg, download),
        Command::CacheErase => commands::cmd_cache_erase(cfg),
        Command::Init { force } => commands::cmd_init(cfg, force),
        Command::ConfigList => commands::cmd_config_list(&loaded),
        Command::ConfigGet { key } => commands::cmd_config_get(&loaded, &key),
        Command::ConfigSet { key, value } => commands::cmd_config_set(&loaded, &key, &value),
        Command::ConfigReset => commands::cmd_config_reset(&loaded),
        Command::Geocode {
            query,
            cache_path,
            limit,
        } => geocode::cmd_geocode_find(&cache_path, &query, limit),
        Command::Version => {
            println!("oeffi {APP_VERSION}");
            Ok(())
        }
        Command::Help => {
            print!("{}", render_help());
            Ok(())
        }
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("Error: {message}");
            if message.to_ascii_lowercase().contains("cache") && !message.contains("Hint:") {
                eprintln!(
                    "Hint: run `oeffi cache build --download` to rebuild and refresh local caches."
                );
            }
            if commands::is_missing_local_data_error(&message) && !message.contains("Hint:") {
                eprintln!("Hint: run `oeffi init` for first-time setup.");
                eprintln!(
                    "Hint: or run `oeffi cache build --download` to fetch raw GTFS + map data and rebuild caches."
                );
            }
            eprintln!("Run `oeffi help` for usage.");
            ExitCode::from(2)
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();

    match parse_command(&args) {
        Ok(command) => run(command),
        Err(err) => {
            if is_help_error(&err) {
                print!("{err}");
                return ExitCode::SUCCESS;
            }
            eprintln!("{err}");
            ExitCode::from(2)
        }
    }
}
