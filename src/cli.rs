use chrono::NaiveDate;

pub const DEFAULT_GTFS_PATH: &str = "data/combined-vienna";
pub const DEFAULT_CACHE_PATH: &str = "gtfs.cache.bin";

pub const USAGE: &str = r#"oeffi - Wien Öffi CLI

Usage:
  oeffi <command>

Commands:
  route <from> <to> [--debug] [--alts N] [--depart HH:MM] [--date YYYY-MM-DD]
                                              Plan a route between two stops (id/name)
  route-coords <from_lat> <from_lon> <to_lat> <to_lon> [--debug] [--alts N] [--depart HH:MM] [--date YYYY-MM-DD]
                                              Plan a route between two coordinates
  line <route>                                List all stops in order for a line (all variants)
  inspect <query>                             Inspect stop by id/code/name and list serving routes
  routes                                      List all routes (id, short name, long name)
  cache-build [gtfs_path] [cache_file]        Rebuild snapshot + planner caches (default snapshot: gtfs.cache.bin)
  summary                                     Show high-level GTFS dataset stats
  help                                        Show this help message

Options:
  -h, --help   Show this help message

Examples:
  oeffi route "Karlsplatz" "Praterstern"
  oeffi route "Herrengasse" "Praterstern" --debug --alts 3
  oeffi route "Herrengasse" "Praterstern" --depart 22:15
  oeffi route "Herrengasse" "Praterstern" --date 2026-03-02 --depart 08:15
  oeffi route-coords 48.2066 16.3707 48.1850 16.3747
  oeffi line U1
  oeffi inspect Karlsplatz
  oeffi routes
  oeffi summary
  oeffi cache-build
"#;

#[derive(Debug)]
pub enum Command {
    Summary,
    ListRoutes,
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

fn default_path() -> String {
    DEFAULT_GTFS_PATH.to_string()
}

fn default_cache_path() -> String {
    DEFAULT_CACHE_PATH.to_string()
}

fn parse_depart_hhmm(value: &str) -> Result<usize, String> {
    let mut parts = value.split(':');
    let (Some(h), Some(m), None) = (parts.next(), parts.next(), parts.next()) else {
        return Err(format!(
            "Invalid value for '--depart': '{value}'. Expected HH:MM (24h)."
        ));
    };
    let hour = h
        .parse::<usize>()
        .map_err(|_| format!("Invalid value for '--depart': '{value}'. Expected HH:MM (24h)."))?;
    let minute = m
        .parse::<usize>()
        .map_err(|_| format!("Invalid value for '--depart': '{value}'. Expected HH:MM (24h)."))?;
    if hour > 23 || minute > 59 {
        return Err(format!(
            "Invalid value for '--depart': '{value}'. Expected HH:MM (24h)."
        ));
    }
    Ok(hour * 3600 + minute * 60)
}

fn parse_service_date(value: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| format!("Invalid value for '--date': '{value}'. Expected YYYY-MM-DD."))
}

fn parse_lat(value: &str, flag: &str) -> Result<f64, String> {
    let lat = value
        .parse::<f64>()
        .map_err(|_| format!("Invalid value for '{flag}': '{value}'. Expected latitude."))?;
    if !(-90.0..=90.0).contains(&lat) {
        return Err(format!(
            "Invalid value for '{flag}': '{value}'. Expected latitude in [-90, 90]."
        ));
    }
    Ok(lat)
}

fn parse_lon(value: &str, flag: &str) -> Result<f64, String> {
    let lon = value
        .parse::<f64>()
        .map_err(|_| format!("Invalid value for '{flag}': '{value}'. Expected longitude."))?;
    if !(-180.0..=180.0).contains(&lon) {
        return Err(format!(
            "Invalid value for '{flag}': '{value}'. Expected longitude in [-180, 180]."
        ));
    }
    Ok(lon)
}

pub fn parse_command(args: &[String]) -> Result<Command, String> {
    if args.is_empty() {
        return Ok(Command::Help);
    }

    if args.len() == 1 && (args[0] == "-h" || args[0] == "--help") {
        return Ok(Command::Help);
    }

    match args[0].as_str() {
        "help" if args.len() == 1 => Ok(Command::Help),
        "summary" if args.len() == 1 => Ok(Command::Summary),
        "routes" if args.len() == 1 => Ok(Command::ListRoutes),
        "route" => {
            if args.len() < 3 {
                return Err(
                    "Invalid arguments for 'route'. Usage: oeffi route <from> <to> [--debug] [--alts N] [--depart HH:MM] [--date YYYY-MM-DD]"
                        .to_string(),
                );
            }

            let from = args[1].clone();
            let to = args[2].clone();
            let mut debug = false;
            let mut alternatives: usize = 3;
            let mut depart_secs: Option<usize> = None;
            let mut service_date: Option<NaiveDate> = None;

            let mut i = 3usize;
            while i < args.len() {
                let arg = &args[i];
                if arg == "--debug" || arg == "-d" {
                    debug = true;
                    i += 1;
                    continue;
                }
                if arg == "--alts" {
                    let value = args.get(i + 1).ok_or_else(|| {
                        "Missing value for '--alts'. Usage: oeffi route <from> <to> [--debug] [--alts N] [--depart HH:MM] [--date YYYY-MM-DD]"
                            .to_string()
                    })?;
                    alternatives = value.parse::<usize>().map_err(|_| {
                        format!("Invalid value for '--alts': '{value}'. Expected a positive integer.")
                    })?;
                    if alternatives == 0 {
                        return Err(
                            "Invalid value for '--alts'. Expected a positive integer."
                                .to_string(),
                        );
                    }
                    i += 2;
                    continue;
                }
                if let Some(value) = arg.strip_prefix("--alts=") {
                    alternatives = value.parse::<usize>().map_err(|_| {
                        format!("Invalid value for '--alts': '{value}'. Expected a positive integer.")
                    })?;
                    if alternatives == 0 {
                        return Err(
                            "Invalid value for '--alts'. Expected a positive integer."
                                .to_string(),
                        );
                    }
                    i += 1;
                    continue;
                }
                if arg == "--depart" {
                    let value = args.get(i + 1).ok_or_else(|| {
                        "Missing value for '--depart'. Usage: oeffi route <from> <to> [--debug] [--alts N] [--depart HH:MM] [--date YYYY-MM-DD]"
                            .to_string()
                    })?;
                    depart_secs = Some(parse_depart_hhmm(value)?);
                    i += 2;
                    continue;
                }
                if let Some(value) = arg.strip_prefix("--depart=") {
                    depart_secs = Some(parse_depart_hhmm(value)?);
                    i += 1;
                    continue;
                }
                if arg == "--date" {
                    let value = args.get(i + 1).ok_or_else(|| {
                        "Missing value for '--date'. Usage: oeffi route <from> <to> [--debug] [--alts N] [--depart HH:MM] [--date YYYY-MM-DD]"
                            .to_string()
                    })?;
                    service_date = Some(parse_service_date(value)?);
                    i += 2;
                    continue;
                }
                if let Some(value) = arg.strip_prefix("--date=") {
                    service_date = Some(parse_service_date(value)?);
                    i += 1;
                    continue;
                }

                return Err(
                    "Invalid arguments for 'route'. Usage: oeffi route <from> <to> [--debug] [--alts N] [--depart HH:MM] [--date YYYY-MM-DD]"
                        .to_string(),
                );
            }

            Ok(Command::Route {
                from,
                to,
                debug,
                alternatives,
                depart_secs,
                service_date,
            })
        }
        "route-coords" => {
            if args.len() < 5 {
                return Err(
                    "Invalid arguments for 'route-coords'. Usage: oeffi route-coords <from_lat> <from_lon> <to_lat> <to_lon> [--debug] [--alts N] [--depart HH:MM] [--date YYYY-MM-DD]"
                        .to_string(),
                );
            }

            let from_lat = parse_lat(&args[1], "<from_lat>")?;
            let from_lon = parse_lon(&args[2], "<from_lon>")?;
            let to_lat = parse_lat(&args[3], "<to_lat>")?;
            let to_lon = parse_lon(&args[4], "<to_lon>")?;

            let mut debug = false;
            let mut alternatives: usize = 3;
            let mut depart_secs: Option<usize> = None;
            let mut service_date: Option<NaiveDate> = None;

            let mut i = 5usize;
            while i < args.len() {
                let arg = &args[i];
                if arg == "--debug" || arg == "-d" {
                    debug = true;
                    i += 1;
                    continue;
                }
                if arg == "--alts" {
                    let value = args.get(i + 1).ok_or_else(|| {
                        "Missing value for '--alts'. Usage: oeffi route-coords <from_lat> <from_lon> <to_lat> <to_lon> [--debug] [--alts N] [--depart HH:MM] [--date YYYY-MM-DD]"
                            .to_string()
                    })?;
                    alternatives = value.parse::<usize>().map_err(|_| {
                        format!("Invalid value for '--alts': '{value}'. Expected a positive integer.")
                    })?;
                    if alternatives == 0 {
                        return Err(
                            "Invalid value for '--alts'. Expected a positive integer."
                                .to_string(),
                        );
                    }
                    i += 2;
                    continue;
                }
                if let Some(value) = arg.strip_prefix("--alts=") {
                    alternatives = value.parse::<usize>().map_err(|_| {
                        format!("Invalid value for '--alts': '{value}'. Expected a positive integer.")
                    })?;
                    if alternatives == 0 {
                        return Err(
                            "Invalid value for '--alts'. Expected a positive integer."
                                .to_string(),
                        );
                    }
                    i += 1;
                    continue;
                }
                if arg == "--depart" {
                    let value = args.get(i + 1).ok_or_else(|| {
                        "Missing value for '--depart'. Usage: oeffi route-coords <from_lat> <from_lon> <to_lat> <to_lon> [--debug] [--alts N] [--depart HH:MM] [--date YYYY-MM-DD]"
                            .to_string()
                    })?;
                    depart_secs = Some(parse_depart_hhmm(value)?);
                    i += 2;
                    continue;
                }
                if let Some(value) = arg.strip_prefix("--depart=") {
                    depart_secs = Some(parse_depart_hhmm(value)?);
                    i += 1;
                    continue;
                }
                if arg == "--date" {
                    let value = args.get(i + 1).ok_or_else(|| {
                        "Missing value for '--date'. Usage: oeffi route-coords <from_lat> <from_lon> <to_lat> <to_lon> [--debug] [--alts N] [--depart HH:MM] [--date YYYY-MM-DD]"
                            .to_string()
                    })?;
                    service_date = Some(parse_service_date(value)?);
                    i += 2;
                    continue;
                }
                if let Some(value) = arg.strip_prefix("--date=") {
                    service_date = Some(parse_service_date(value)?);
                    i += 1;
                    continue;
                }

                return Err(
                    "Invalid arguments for 'route-coords'. Usage: oeffi route-coords <from_lat> <from_lon> <to_lat> <to_lon> [--debug] [--alts N] [--depart HH:MM] [--date YYYY-MM-DD]"
                        .to_string(),
                );
            }

            Ok(Command::RouteCoords {
                from_lat,
                from_lon,
                to_lat,
                to_lon,
                debug,
                alternatives,
                depart_secs,
                service_date,
            })
        }
        "line" => {
            if args.len() != 2 {
                return Err(
                    "Invalid arguments for 'line'. Usage: oeffi line <route>"
                        .to_string(),
                );
            }

            let route = args[1].clone();
            Ok(Command::Line { route })
        }
        "inspect" => {
            if args.len() != 2 {
                return Err(
                    "Invalid arguments for 'inspect'. Usage: oeffi inspect <query>"
                        .to_string(),
                );
            }

            let query = args[1].clone();
            Ok(Command::Inspect { query })
        }
        "cache-build" if args.len() == 1 => Ok(Command::CacheBuild {
            source_path: default_path(),
            cache_path: default_cache_path(),
        }),
        "cache-build" if args.len() == 2 => Ok(Command::CacheBuild {
            source_path: args[1].clone(),
            cache_path: default_cache_path(),
        }),
        "cache-build" if args.len() == 3 => Ok(Command::CacheBuild {
            source_path: args[1].clone(),
            cache_path: args[2].clone(),
        }),
        "cache-build" => Err(
            "Invalid arguments for 'cache-build'. Usage: oeffi cache-build [gtfs_path] [cache_file]"
                .to_string(),
        ),
        "help" | "summary" | "routes" => {
            Err(format!("Too many arguments for command '{}'.", args[0]))
        }
        unknown => Err(format!("Unknown command: '{unknown}'")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_summary_with_default_file() {
        let args = vec!["summary".to_string()];
        assert!(matches!(parse_command(&args), Ok(Command::Summary)));
    }

    #[test]
    fn parses_line() {
        let args = vec!["line".to_string(), "U1".to_string()];
        assert!(matches!(
            parse_command(&args),
            Ok(Command::Line { route }) if route == "U1"
        ));
    }

    #[test]
    fn parses_route() {
        let args = vec![
            "route".to_string(),
            "Karlsplatz".to_string(),
            "Praterstern".to_string(),
        ];
        assert!(matches!(
            parse_command(&args),
            Ok(Command::Route { from, to, debug, alternatives, depart_secs, service_date })
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
        let args = vec![
            "route".to_string(),
            "Herrengasse".to_string(),
            "Praterstern".to_string(),
            "--debug".to_string(),
            "--alts".to_string(),
            "5".to_string(),
        ];
        assert!(matches!(
            parse_command(&args),
            Ok(Command::Route { from, to, debug, alternatives, depart_secs, service_date })
                if from == "Herrengasse"
                    && to == "Praterstern"
                    && debug
                    && alternatives == 5
                    && depart_secs.is_none()
                    && service_date.is_none()
        ));
    }

    #[test]
    fn parses_route_with_depart_time() {
        let args = vec![
            "route".to_string(),
            "Herrengasse".to_string(),
            "Praterstern".to_string(),
            "--depart".to_string(),
            "22:15".to_string(),
        ];
        assert!(matches!(
            parse_command(&args),
            Ok(Command::Route { depart_secs, service_date, .. })
                if depart_secs == Some(80_100) && service_date.is_none()
        ));
    }

    #[test]
    fn parses_route_with_service_date() {
        let args = vec![
            "route".to_string(),
            "Herrengasse".to_string(),
            "Praterstern".to_string(),
            "--date".to_string(),
            "2026-03-02".to_string(),
        ];
        assert!(matches!(
            parse_command(&args),
            Ok(Command::Route { depart_secs, service_date, .. })
                if depart_secs.is_none()
                    && service_date == NaiveDate::from_ymd_opt(2026, 3, 2)
        ));
    }

    #[test]
    fn parses_route_coords() {
        let args = vec![
            "route-coords".to_string(),
            "48.2066".to_string(),
            "16.3707".to_string(),
            "48.1850".to_string(),
            "16.3747".to_string(),
            "--alts".to_string(),
            "2".to_string(),
        ];
        assert!(matches!(
            parse_command(&args),
            Ok(Command::RouteCoords {
                from_lat,
                from_lon,
                to_lat,
                to_lon,
                debug,
                alternatives,
                depart_secs,
                service_date
            })
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
    fn rejects_route_plan_with_invalid_depart_time() {
        let args = vec![
            "route".to_string(),
            "Herrengasse".to_string(),
            "Praterstern".to_string(),
            "--depart".to_string(),
            "25:99".to_string(),
        ];
        let err = parse_command(&args).expect_err("expected parse failure");
        assert!(err.contains("Invalid value for '--depart'"));
    }

    #[test]
    fn rejects_route_plan_with_invalid_service_date() {
        let args = vec![
            "route".to_string(),
            "Herrengasse".to_string(),
            "Praterstern".to_string(),
            "--date".to_string(),
            "2026-99-99".to_string(),
        ];
        let err = parse_command(&args).expect_err("expected parse failure");
        assert!(err.contains("Invalid value for '--date'"));
    }

    #[test]
    fn parses_cache_build_defaults() {
        let args = vec!["cache-build".to_string()];
        assert!(matches!(
            parse_command(&args),
            Ok(Command::CacheBuild { source_path, cache_path }) if source_path == DEFAULT_GTFS_PATH && cache_path == DEFAULT_CACHE_PATH
        ));
    }

    #[test]
    fn parses_inspect_defaults() {
        let args = vec!["inspect".to_string(), "Karlsplatz".to_string()];
        assert!(matches!(
            parse_command(&args),
            Ok(Command::Inspect { query }) if query == "Karlsplatz"
        ));
    }

    #[test]
    fn rejects_inspect_extra_arg() {
        let args = vec![
            "inspect".to_string(),
            "Karlsplatz".to_string(),
            "--x".to_string(),
        ];
        let err = parse_command(&args).unwrap_err();
        assert!(err.contains("Invalid arguments for 'inspect'"));
    }

    #[test]
    fn rejects_line_with_extra_arg() {
        let args = vec!["line".to_string(), "U1".to_string(), "data".to_string()];
        let err = parse_command(&args).unwrap_err();
        assert!(err.contains("Invalid arguments for 'line'"));
    }

    #[test]
    fn rejects_unknown_command() {
        let args = vec!["nope".to_string()];
        let err = parse_command(&args).unwrap_err();
        assert!(err.contains("Unknown command"));
    }
}
