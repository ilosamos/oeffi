use std::iter;

use chrono::NaiveDate;
use clap::{CommandFactory, Parser, Subcommand, error::ErrorKind};

pub const DEFAULT_GTFS_PATH: &str = "data/combined-vienna";
pub const DEFAULT_CACHE_PATH: &str = "gtfs.cache.bin";

#[derive(Debug)]
pub enum Command {
    Summary,
    ListRoutes,
    ListStops,
    Route {
        from: String,
        to: String,
        debug: bool,
        alternatives: usize,
        depart_secs: Option<usize>,
        service_date: Option<NaiveDate>,
    },
    RouteCoords {
        from_lat: f64,
        from_lon: f64,
        to_lat: f64,
        to_lon: f64,
        debug: bool,
        alternatives: usize,
        depart_secs: Option<usize>,
        service_date: Option<NaiveDate>,
    },
    Line {
        route: String,
    },
    Inspect {
        query: String,
    },
    CacheBuild {
        source_path: String,
        cache_path: String,
    },
    Help,
}

#[derive(Debug, Parser)]
#[command(
    name = "oeffi",
    about = "Wien Öffi CLI",
    disable_help_subcommand = true,
    next_line_help = true,
    after_help = "Examples:
  oeffi route \"Karlsplatz\" \"Praterstern\"
  oeffi route \"Herrengasse\" \"Praterstern\" --debug --alts 3
  oeffi route \"Herrengasse\" \"Praterstern\" --depart 22:15
  oeffi route \"Herrengasse\" \"Praterstern\" --date 2026-03-02 --depart 08:15
  oeffi route-coords 48.2066 16.3707 48.1850 16.3747
  oeffi line U1
  oeffi stops
  oeffi inspect Karlsplatz
  oeffi routes
  oeffi summary
  oeffi cache-build"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<CliCommand>,
}

#[derive(Debug, Subcommand)]
enum CliCommand {
    #[command(about = "Plan a route between two stops (id/name)")]
    Route {
        from: String,
        to: String,
        #[arg(short = 'd', long = "debug")]
        debug: bool,
        #[arg(long = "alts", default_value_t = 3, value_parser = parse_positive_usize)]
        alternatives: usize,
        #[arg(long = "depart", value_name = "HH:MM", value_parser = parse_depart_hhmm)]
        depart_secs: Option<usize>,
        #[arg(long = "date", value_name = "YYYY-MM-DD", value_parser = parse_service_date)]
        service_date: Option<NaiveDate>,
    },
    #[command(name = "route-coords", about = "Plan a route between two coordinates")]
    RouteCoords {
        #[arg(value_parser = parse_lat)]
        from_lat: f64,
        #[arg(value_parser = parse_lon)]
        from_lon: f64,
        #[arg(value_parser = parse_lat)]
        to_lat: f64,
        #[arg(value_parser = parse_lon)]
        to_lon: f64,
        #[arg(short = 'd', long = "debug")]
        debug: bool,
        #[arg(long = "alts", default_value_t = 3, value_parser = parse_positive_usize)]
        alternatives: usize,
        #[arg(long = "depart", value_name = "HH:MM", value_parser = parse_depart_hhmm)]
        depart_secs: Option<usize>,
        #[arg(long = "date", value_name = "YYYY-MM-DD", value_parser = parse_service_date)]
        service_date: Option<NaiveDate>,
    },
    #[command(about = "List all stops in order for a line (all variants)")]
    Line { route: String },
    #[command(about = "List all clustered stops (name + cluster key)")]
    Stops,
    #[command(about = "Inspect stop by id/code/name and list serving routes")]
    Inspect { query: String },
    #[command(about = "List all routes (id, short name, long name)")]
    Routes,
    #[command(about = "Rebuild snapshot + planner caches (default snapshot: gtfs.cache.bin)")]
    CacheBuild {
        source_path: Option<String>,
        cache_path: Option<String>,
    },
    #[command(about = "Show high-level GTFS dataset stats")]
    Summary,
    #[command(about = "Show help message")]
    Help,
}

fn default_path() -> String {
    DEFAULT_GTFS_PATH.to_string()
}

fn default_cache_path() -> String {
    DEFAULT_CACHE_PATH.to_string()
}

fn parse_depart_hhmm(value: &str) -> Result<usize, String> {
    let mut parts = value.split(':');
    let (Some(h), Some(m), None) = (parts.next(), parts.next(), parts.next()) else {
        return Err(format!("invalid time '{value}', expected HH:MM (24h)"));
    };
    let hour = h
        .parse::<usize>()
        .map_err(|_| format!("invalid time '{value}', expected HH:MM (24h)"))?;
    let minute = m
        .parse::<usize>()
        .map_err(|_| format!("invalid time '{value}', expected HH:MM (24h)"))?;
    if hour > 23 || minute > 59 {
        return Err(format!("invalid time '{value}', expected HH:MM (24h)"));
    }
    Ok(hour * 3600 + minute * 60)
}

fn parse_positive_usize(value: &str) -> Result<usize, String> {
    let parsed = value
        .parse::<usize>()
        .map_err(|_| format!("invalid integer '{value}', expected a positive integer"))?;
    if parsed == 0 {
        return Err("expected a positive integer (> 0)".to_string());
    }
    Ok(parsed)
}

fn parse_service_date(value: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| format!("invalid date '{value}', expected YYYY-MM-DD"))
}

fn parse_lat(value: &str) -> Result<f64, String> {
    let lat = value
        .parse::<f64>()
        .map_err(|_| format!("invalid latitude '{value}'"))?;
    if !(-90.0..=90.0).contains(&lat) {
        return Err(format!(
            "invalid latitude '{value}', expected range [-90, 90]"
        ));
    }
    Ok(lat)
}

fn parse_lon(value: &str) -> Result<f64, String> {
    let lon = value
        .parse::<f64>()
        .map_err(|_| format!("invalid longitude '{value}'"))?;
    if !(-180.0..=180.0).contains(&lon) {
        return Err(format!(
            "invalid longitude '{value}', expected range [-180, 180]"
        ));
    }
    Ok(lon)
}

pub fn parse_command(args: &[String]) -> Result<Command, clap::Error> {
    let parsed = Cli::try_parse_from(iter::once("oeffi").chain(args.iter().map(String::as_str)))?;
    Ok(match parsed.command {
        Some(CliCommand::Route {
            from,
            to,
            debug,
            alternatives,
            depart_secs,
            service_date,
        }) => Command::Route {
            from,
            to,
            debug,
            alternatives,
            depart_secs,
            service_date,
        },
        Some(CliCommand::RouteCoords {
            from_lat,
            from_lon,
            to_lat,
            to_lon,
            debug,
            alternatives,
            depart_secs,
            service_date,
        }) => Command::RouteCoords {
            from_lat,
            from_lon,
            to_lat,
            to_lon,
            debug,
            alternatives,
            depart_secs,
            service_date,
        },
        Some(CliCommand::Line { route }) => Command::Line { route },
        Some(CliCommand::Stops) => Command::ListStops,
        Some(CliCommand::Inspect { query }) => Command::Inspect { query },
        Some(CliCommand::Routes) => Command::ListRoutes,
        Some(CliCommand::CacheBuild {
            source_path,
            cache_path,
        }) => Command::CacheBuild {
            source_path: source_path.unwrap_or_else(default_path),
            cache_path: cache_path.unwrap_or_else(default_cache_path),
        },
        Some(CliCommand::Summary) => Command::Summary,
        Some(CliCommand::Help) | None => Command::Help,
    })
}

pub fn is_help_error(err: &clap::Error) -> bool {
    matches!(
        err.kind(),
        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
    )
}

pub fn render_help() -> String {
    let mut cmd = Cli::command();
    let mut out = Vec::new();
    let _ = cmd.write_long_help(&mut out);
    String::from_utf8(out).unwrap_or_else(|_| "oeffi help unavailable".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_to_command(args: &[&str]) -> Command {
        let args = args.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        parse_command(&args).expect("command parses")
    }

    #[test]
    fn parses_summary() {
        assert!(matches!(parse_to_command(&["summary"]), Command::Summary));
    }

    #[test]
    fn parses_line() {
        assert!(matches!(
            parse_to_command(&["line", "U1"]),
            Command::Line { route } if route == "U1"
        ));
    }

    #[test]
    fn parses_stops() {
        assert!(matches!(parse_to_command(&["stops"]), Command::ListStops));
    }

    #[test]
    fn parses_route() {
        assert!(matches!(
            parse_to_command(&["route", "Karlsplatz", "Praterstern"]),
            Command::Route { from, to, debug, alternatives, depart_secs, service_date }
                if from == "Karlsplatz"
                    && to == "Praterstern"
                    && !debug
                    && alternatives == 3
                    && depart_secs.is_none()
                    && service_date.is_none()
        ));
    }

    #[test]
    fn parses_route_debug_with_alts() {
        assert!(matches!(
            parse_to_command(&["route", "Herrengasse", "Praterstern", "--debug", "--alts", "5"]),
            Command::Route { debug, alternatives, .. } if debug && alternatives == 5
        ));
    }

    #[test]
    fn parses_route_with_depart_time() {
        assert!(matches!(
            parse_to_command(&["route", "Herrengasse", "Praterstern", "--depart", "22:15"]),
            Command::Route { depart_secs, service_date, .. }
                if depart_secs == Some(80_100) && service_date.is_none()
        ));
    }

    #[test]
    fn parses_route_with_service_date() {
        assert!(matches!(
            parse_to_command(&["route", "Herrengasse", "Praterstern", "--date", "2026-03-02"]),
            Command::Route { depart_secs, service_date, .. }
                if depart_secs.is_none() && service_date == NaiveDate::from_ymd_opt(2026, 3, 2)
        ));
    }

    #[test]
    fn parses_route_coords() {
        assert!(matches!(
            parse_to_command(&[
                "route-coords",
                "48.2066",
                "16.3707",
                "48.1850",
                "16.3747",
                "--alts",
                "2"
            ]),
            Command::RouteCoords {
                from_lat,
                from_lon,
                to_lat,
                to_lon,
                debug,
                alternatives,
                depart_secs,
                service_date
            }
            if (from_lat - 48.2066).abs() < f64::EPSILON
                && (from_lon - 16.3707).abs() < f64::EPSILON
                && (to_lat - 48.1850).abs() < f64::EPSILON
                && (to_lon - 16.3747).abs() < f64::EPSILON
                && !debug
                && alternatives == 2
                && depart_secs.is_none()
                && service_date.is_none()
        ));
    }

    #[test]
    fn parses_cache_build_defaults() {
        assert!(matches!(
            parse_to_command(&["cache-build"]),
            Command::CacheBuild { source_path, cache_path }
                if source_path == DEFAULT_GTFS_PATH && cache_path == DEFAULT_CACHE_PATH
        ));
    }

    #[test]
    fn parses_inspect() {
        assert!(matches!(
            parse_to_command(&["inspect", "Karlsplatz"]),
            Command::Inspect { query } if query == "Karlsplatz"
        ));
    }

    #[test]
    fn rejects_invalid_depart_time() {
        let args = ["route", "Herrengasse", "Praterstern", "--depart", "25:99"]
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>();
        let err = parse_command(&args).expect_err("expected parse failure");
        assert!(err.to_string().contains("invalid time"));
    }

    #[test]
    fn rejects_unknown_command() {
        let args = vec!["nope".to_string()];
        let err = parse_command(&args).expect_err("expected parse failure");
        assert!(err.to_string().contains("unrecognized subcommand"));
    }
}
