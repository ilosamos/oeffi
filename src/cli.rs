use std::iter;

use chrono::NaiveDate;
use clap::{CommandFactory, Parser, Subcommand, error::ErrorKind};

pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug)]
pub enum Command {
    Init {
        force: bool,
    },
    Route {
        from: String,
        to: String,
        debug: bool,
        verbose: bool,
        alternatives: usize,
        depart_secs: Option<usize>,
        service_date: Option<NaiveDate>,
    },
    Geocode {
        query: String,
        cache_path: String,
        limit: usize,
    },
    Inspect {
        query: String,
    },
    ListStops,
    Line {
        route: String,
    },
    Summary,
    ListRoutes,
    CacheBuild {
        download: bool,
    },
    CacheErase,
    ConfigList,
    ConfigGet {
        key: String,
    },
    ConfigSet {
        key: String,
        value: String,
    },
    ConfigReset,
    Version,
    Help,
}

#[derive(Debug, Parser)]
#[command(
    name = "oeffi",
    about = "Wien Öffi CLI",
    version = APP_VERSION,
    disable_help_subcommand = true,
    next_line_help = true,
    after_help = "Examples:
  oeffi init
  oeffi route \"Karlsplatz\" \"Praterstern\"
  oeffi route \"Praterstern\" \"Mochi Ramen Bar\" --verbose
  oeffi route \"48.2066 16.3707\" \"48.1850 16.3747\"
  oeffi geocode \"mariahilfer strasse 1\"
  oeffi geocode \"stephansdom\" --limit 3
  oeffi inspect Karlsplatz
  oeffi stops
  oeffi route \"Herrengasse\" \"Praterstern\" --debug --alts 3
  oeffi route \"Herrengasse\" \"Praterstern\" --depart 22:15
  oeffi route \"Herrengasse\" \"Praterstern\" --date 2026-03-02 --depart 08:15
  oeffi line U1
  oeffi routes
  oeffi summary
  oeffi cache build
  oeffi cache build --download
  oeffi cache erase
  oeffi version
  oeffi config list
  oeffi config get merged_gtfs_path
  oeffi config set planner_cache_path /tmp/planner.cache.bin"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<CliCommand>,
}

#[derive(Debug, Subcommand)]
enum CliCommand {
    #[command(about = "First-run setup: download feeds + map data, merge, and build caches")]
    Init {
        #[arg(short = 'f', long = "force", help = "Overwrite existing raw GTFS data")]
        force: bool,
    },
    #[command(about = "Plan a route between two locations (stop/geocode/\"lat lon\")")]
    Route {
        from: String,
        to: String,
        #[arg(short = 'd', long = "debug")]
        debug: bool,
        #[arg(short = 'v', long = "verbose")]
        verbose: bool,
        #[arg(long = "alts", default_value_t = 3, value_parser = parse_positive_usize)]
        alternatives: usize,
        #[arg(long = "depart", value_name = "HH:MM", value_parser = parse_depart_hhmm)]
        depart_secs: Option<usize>,
        #[arg(long = "date", value_name = "YYYY-MM-DD", value_parser = parse_service_date)]
        service_date: Option<NaiveDate>,
    },
    #[command(
        about = "Find coordinates for addresses, landmarks, restaurants, and other named places"
    )]
    Geocode {
        query: String,
        #[arg(long = "cache", default_value = "data/vienna-addresses.cache.bin")]
        cache_path: String,
        #[arg(long = "limit", default_value_t = 10, value_parser = parse_positive_usize)]
        limit: usize,
    },
    #[command(about = "Inspect stop by id/code/name and list serving routes")]
    Inspect { query: String },
    #[command(about = "List all clustered stops (name + cluster key)")]
    Stops,
    #[command(about = "List all stops in order for a line (all variants)")]
    Line { route: String },
    #[command(about = "List all routes (id, short name, long name)")]
    Routes,
    #[command(about = "Show high-level GTFS dataset stats")]
    Summary,
    #[command(about = "Manage local caches and raw data")]
    Cache {
        #[command(subcommand)]
        command: CacheSubcommand,
    },
    #[command(about = "Read and write persistent configuration")]
    Config {
        #[command(subcommand)]
        command: ConfigSubcommand,
    },
    #[command(about = "Print version")]
    Version,
    #[command(about = "Show help message")]
    Help,
}

#[derive(Debug, Subcommand)]
enum ConfigSubcommand {
    #[command(about = "List effective config values")]
    List,
    #[command(about = "Read one config value by key")]
    Get { key: String },
    #[command(about = "Write one config value to config.json")]
    Set { key: String, value: String },
    #[command(about = "Reset config.json to default values")]
    Reset,
}

#[derive(Debug, Subcommand)]
enum CacheSubcommand {
    #[command(about = "Rebuild snapshot + planner + geocode caches")]
    Build {
        #[arg(
            long = "download",
            help = "Download GTFS feeds and OSM PBF map data before rebuild"
        )]
        download: bool,
    },
    #[command(about = "Erase all local raw data and cache files")]
    Erase,
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

pub fn parse_command(args: &[String]) -> Result<Command, clap::Error> {
    let parsed = Cli::try_parse_from(iter::once("oeffi").chain(args.iter().map(String::as_str)))?;
    Ok(match parsed.command {
        Some(CliCommand::Route {
            from,
            to,
            debug,
            verbose,
            alternatives,
            depart_secs,
            service_date,
        }) => Command::Route {
            from,
            to,
            debug,
            verbose,
            alternatives,
            depart_secs,
            service_date,
        },
        Some(CliCommand::Init { force }) => Command::Init { force },
        Some(CliCommand::Line { route }) => Command::Line { route },
        Some(CliCommand::Stops) => Command::ListStops,
        Some(CliCommand::Inspect { query }) => Command::Inspect { query },
        Some(CliCommand::Geocode {
            query,
            cache_path,
            limit,
        }) => Command::Geocode {
            query,
            cache_path,
            limit,
        },
        Some(CliCommand::Routes) => Command::ListRoutes,
        Some(CliCommand::Summary) => Command::Summary,
        Some(CliCommand::Cache { command }) => match command {
            CacheSubcommand::Build { download } => Command::CacheBuild { download },
            CacheSubcommand::Erase => Command::CacheErase,
        },
        Some(CliCommand::Config { command }) => match command {
            ConfigSubcommand::List => Command::ConfigList,
            ConfigSubcommand::Get { key } => Command::ConfigGet { key },
            ConfigSubcommand::Set { key, value } => Command::ConfigSet { key, value },
            ConfigSubcommand::Reset => Command::ConfigReset,
        },
        Some(CliCommand::Version) => Command::Version,
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
            Command::Route { from, to, debug, verbose, alternatives, depart_secs, service_date }
                if from == "Karlsplatz"
                    && to == "Praterstern"
                    && !debug
                    && !verbose
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
    fn parses_route_with_coord_pair_strings() {
        assert!(matches!(
            parse_to_command(&["route", "48.2066 16.3707", "48.1850 16.3747", "--alts", "2"]),
            Command::Route {
                from,
                to,
                debug,
                verbose,
                alternatives,
                depart_secs,
                service_date
            }
            if from == "48.2066 16.3707"
                && to == "48.1850 16.3747"
                && !debug
                && !verbose
                && alternatives == 2
                && depart_secs.is_none()
                && service_date.is_none()
        ));
    }

    #[test]
    fn parses_route_verbose() {
        assert!(matches!(
            parse_to_command(&["route", "Praterstern", "Mochi Ramen Bar", "--verbose"]),
            Command::Route { verbose, .. } if verbose
        ));
    }

    #[test]
    fn parses_cache_build_defaults() {
        assert!(matches!(
            parse_to_command(&["cache", "build"]),
            Command::CacheBuild { download } if !download
        ));
    }

    #[test]
    fn parses_cache_build_with_download() {
        assert!(matches!(
            parse_to_command(&["cache", "build", "--download"]),
            Command::CacheBuild { download, .. } if download
        ));
    }

    #[test]
    fn parses_cache_erase() {
        assert!(matches!(
            parse_to_command(&["cache", "erase"]),
            Command::CacheErase
        ));
    }

    #[test]
    fn parses_init_with_force() {
        assert!(matches!(
            parse_to_command(&["init", "--force"]),
            Command::Init { force } if force
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
    fn parses_config_list() {
        assert!(matches!(
            parse_to_command(&["config", "list"]),
            Command::ConfigList
        ));
    }

    #[test]
    fn parses_config_set() {
        assert!(matches!(
            parse_to_command(&["config", "set", "merged_gtfs_path", "/tmp/gtfs"]),
            Command::ConfigSet { key, value }
            if key == "merged_gtfs_path" && value == "/tmp/gtfs"
        ));
    }

    #[test]
    fn parses_config_reset() {
        assert!(matches!(
            parse_to_command(&["config", "reset"]),
            Command::ConfigReset
        ));
    }

    #[test]
    fn parses_version() {
        assert!(matches!(parse_to_command(&["version"]), Command::Version));
    }

    #[test]
    fn parses_geocode_with_limit() {
        assert!(matches!(
            parse_to_command(&["geocode", "karntner strasse 1", "--limit", "5"]),
            Command::Geocode { query, limit, .. }
                if query == "karntner strasse 1" && limit == 5
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
