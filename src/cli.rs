pub const DEFAULT_GTFS_PATH: &str = "data";
pub const DEFAULT_CACHE_PATH: &str = "gtfs.cache.bin";

pub const USAGE: &str = r#"oeffi - Wiener Linien GTFS inspector

Usage:
  oeffi <command>

Commands:
  hello                                       Print "hello world"
  gtfs-summary [gtfs_path]                    Show high-level GTFS dataset stats
  routes [gtfs_path]                          List all routes (id, short name, long name)
  route-stops <route> [gtfs_path]             List unique stops for a route short name/id (example: U1)
  cache-build [gtfs_path] [cache_file]        Build binary cache file (default: gtfs.cache.bin)
  help                                        Show this help message

Options:
  -h, --help   Show this help message

Examples:
  oeffi cache-build
  oeffi route-stops U1
  oeffi routes
"#;

#[derive(Debug)]
pub enum Command {
    Hello,
    GtfsSummary {
        source_path: String,
    },
    ListRoutes {
        source_path: String,
    },
    RouteStops {
        route: String,
        source_path: String,
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
        "hello" if args.len() == 1 => Ok(Command::Hello),
        "help" if args.len() == 1 => Ok(Command::Help),
        "gtfs-summary" if args.len() == 1 => Ok(Command::GtfsSummary {
            source_path: default_path(),
        }),
        "gtfs-summary" if args.len() == 2 => Ok(Command::GtfsSummary {
            source_path: args[1].clone(),
        }),
        "routes" if args.len() == 1 => Ok(Command::ListRoutes {
            source_path: default_path(),
        }),
        "routes" if args.len() == 2 => Ok(Command::ListRoutes {
            source_path: args[1].clone(),
        }),
        "route-stops" if args.len() == 2 => Ok(Command::RouteStops {
            route: args[1].clone(),
            source_path: default_path(),
        }),
        "route-stops" if args.len() == 3 => Ok(Command::RouteStops {
            route: args[1].clone(),
            source_path: args[2].clone(),
        }),
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
        "route-stops" => Err(
            "Invalid arguments for 'route-stops'. Usage: oeffi route-stops <route> [gtfs_path]"
                .to_string(),
        ),
        "cache-build" => Err(
            "Invalid arguments for 'cache-build'. Usage: oeffi cache-build [gtfs_path] [cache_file]"
                .to_string(),
        ),
        "hello" | "help" | "gtfs-summary" | "routes" => {
            Err(format!("Too many arguments for command '{}'.", args[0]))
        }
        unknown => Err(format!("Unknown command: '{unknown}'")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hello() {
        let args = vec!["hello".to_string()];
        assert!(matches!(parse_command(&args), Ok(Command::Hello)));
    }

    #[test]
    fn parses_gtfs_summary_with_default_file() {
        let args = vec!["gtfs-summary".to_string()];
        assert!(matches!(
            parse_command(&args),
            Ok(Command::GtfsSummary { source_path }) if source_path == DEFAULT_GTFS_PATH
        ));
    }

    #[test]
    fn parses_route_stops() {
        let args = vec!["route-stops".to_string(), "U1".to_string()];
        assert!(matches!(
            parse_command(&args),
            Ok(Command::RouteStops { route, source_path }) if route == "U1" && source_path == DEFAULT_GTFS_PATH
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
    fn rejects_unknown_command() {
        let args = vec!["nope".to_string()];
        let err = parse_command(&args).unwrap_err();
        assert!(err.contains("Unknown command"));
    }
}
