pub const DEFAULT_GTFS_PATH: &str = "data";
pub const DEFAULT_CACHE_PATH: &str = "gtfs.cache.bin";

pub const USAGE: &str = r#"oeffi - Wiener Linien CLI

Usage:
  oeffi <command>

Commands:
  gtfs-summary                                Show high-level GTFS dataset stats
  routes                                      List all routes (id, short name, long name)
  route-plan <from> <to> [--debug] [--alts N] Plan a route between two stops (id/name)
  route-stops <route> [--all]                 List stops in order for a route (default: longest variant only)
  stop-inspect <query>                        Inspect stop by id/code/name and list serving routes
  cache-build [gtfs_path] [cache_file]        Rebuild snapshot + planner caches (default snapshot: gtfs.cache.bin)
  help                                        Show this help message

Options:
  -h, --help   Show this help message

Examples:
  oeffi cache-build
  oeffi route-plan "Karlsplatz" "Praterstern"
  oeffi route-plan "Herrengasse" "Praterstern" --debug --alts 3
  oeffi route-stops U1
  oeffi route-stops U1 --all
  oeffi stop-inspect Karlsplatz
  oeffi routes
"#;

#[derive(Debug)]
pub enum Command {
    GtfsSummary,
    ListRoutes,
    RoutePlan {
        from: String,
        to: String,
        debug: bool,
        alternatives: usize,
    },
    RouteStops {
        route: String,
        show_all: bool,
    },
    StopInspect {
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

pub fn parse_command(args: &[String]) -> Result<Command, String> {
    if args.is_empty() {
        return Ok(Command::Help);
    }

    if args.len() == 1 && (args[0] == "-h" || args[0] == "--help") {
        return Ok(Command::Help);
    }

    match args[0].as_str() {
        "help" if args.len() == 1 => Ok(Command::Help),
        "gtfs-summary" if args.len() == 1 => Ok(Command::GtfsSummary),
        "routes" if args.len() == 1 => Ok(Command::ListRoutes),
        "route-plan" => {
            if args.len() < 3 {
                return Err(
                    "Invalid arguments for 'route-plan'. Usage: oeffi route-plan <from> <to> [--debug] [--alts N]"
                        .to_string(),
                );
            }

            let from = args[1].clone();
            let to = args[2].clone();
            let mut debug = false;
            let mut alternatives: usize = 3;

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
                        "Missing value for '--alts'. Usage: oeffi route-plan <from> <to> [--debug] [--alts N]"
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

                return Err(
                    "Invalid arguments for 'route-plan'. Usage: oeffi route-plan <from> <to> [--debug] [--alts N]"
                        .to_string(),
                );
            }

            Ok(Command::RoutePlan {
                from,
                to,
                debug,
                alternatives,
            })
        }
        "route-stops" => {
            if args.len() < 2 || args.len() > 3 {
                return Err(
                    "Invalid arguments for 'route-stops'. Usage: oeffi route-stops <route> [--all]"
                        .to_string(),
                );
            }

            let route = args[1].clone();
            let mut show_all = false;

            for arg in args.iter().skip(2) {
                if arg == "--all" || arg == "-a" {
                    show_all = true;
                } else {
                    return Err(
                        "Invalid arguments for 'route-stops'. Usage: oeffi route-stops <route> [--all]"
                            .to_string(),
                    );
                }
            }

            Ok(Command::RouteStops { route, show_all })
        }
        "stop-inspect" => {
            if args.len() != 2 {
                return Err(
                    "Invalid arguments for 'stop-inspect'. Usage: oeffi stop-inspect <query>"
                        .to_string(),
                );
            }

            let query = args[1].clone();
            Ok(Command::StopInspect { query })
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
        "help" | "gtfs-summary" | "routes" => {
            Err(format!("Too many arguments for command '{}'.", args[0]))
        }
        unknown => Err(format!("Unknown command: '{unknown}'")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_gtfs_summary_with_default_file() {
        let args = vec!["gtfs-summary".to_string()];
        assert!(matches!(parse_command(&args), Ok(Command::GtfsSummary)));
    }

    #[test]
    fn parses_route_stops() {
        let args = vec!["route-stops".to_string(), "U1".to_string()];
        assert!(matches!(
            parse_command(&args),
            Ok(Command::RouteStops { route, show_all }) if route == "U1" && !show_all
        ));
    }

    #[test]
    fn parses_route_plan() {
        let args = vec![
            "route-plan".to_string(),
            "Karlsplatz".to_string(),
            "Praterstern".to_string(),
        ];
        assert!(matches!(
            parse_command(&args),
            Ok(Command::RoutePlan { from, to, debug, alternatives })
                if from == "Karlsplatz" && to == "Praterstern" && !debug && alternatives == 3
        ));
    }

    #[test]
    fn parses_route_plan_debug_with_alts() {
        let args = vec![
            "route-plan".to_string(),
            "Herrengasse".to_string(),
            "Praterstern".to_string(),
            "--debug".to_string(),
            "--alts".to_string(),
            "5".to_string(),
        ];
        assert!(matches!(
            parse_command(&args),
            Ok(Command::RoutePlan { from, to, debug, alternatives })
                if from == "Herrengasse" && to == "Praterstern" && debug && alternatives == 5
        ));
    }

    #[test]
    fn parses_route_stops_all_flag() {
        let args = vec![
            "route-stops".to_string(),
            "U1".to_string(),
            "--all".to_string(),
        ];
        assert!(matches!(
            parse_command(&args),
            Ok(Command::RouteStops { route, show_all }) if route == "U1" && show_all
        ));
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
    fn parses_stop_inspect_defaults() {
        let args = vec!["stop-inspect".to_string(), "Karlsplatz".to_string()];
        assert!(matches!(
            parse_command(&args),
            Ok(Command::StopInspect { query }) if query == "Karlsplatz"
        ));
    }

    #[test]
    fn rejects_stop_inspect_all_flag() {
        let args = vec![
            "stop-inspect".to_string(),
            "Karlsplatz".to_string(),
            "--all".to_string(),
        ];
        let err = parse_command(&args).unwrap_err();
        assert!(err.contains("Invalid arguments for 'stop-inspect'"));
    }

    #[test]
    fn rejects_route_stops_with_path_arg() {
        let args = vec![
            "route-stops".to_string(),
            "U1".to_string(),
            "data".to_string(),
        ];
        let err = parse_command(&args).unwrap_err();
        assert!(err.contains("Invalid arguments for 'route-stops'"));
    }

    #[test]
    fn rejects_unknown_command() {
        let args = vec!["nope".to_string()];
        let err = parse_command(&args).unwrap_err();
        assert!(err.contains("Unknown command"));
    }
}
