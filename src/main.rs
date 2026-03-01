use std::collections::{HashMap, HashSet};
use std::env;
use std::process::ExitCode;

use gtfs_structures::Gtfs;

const DEFAULT_GTFS_PATH: &str = "data";
const USAGE: &str = r#"oeffi - Wiener Linien GTFS inspector

Usage:
  oeffi <command>

Commands:
  hello                               Print "hello world"
  gtfs-summary [gtfs_zip]             Show high-level GTFS dataset stats
  routes [gtfs_zip]                   List all routes (id, short name, long name)
  route-stops <route> [gtfs_zip]      List unique stops for a route short name (example: U1)
  help                                Show this help message

Options:
  -h, --help   Show this help message

Examples:
  oeffi route-stops U1
  oeffi routes
  oeffi gtfs-summary ./gtfs.zip
"#;

#[derive(Debug)]
enum Command {
    Hello,
    GtfsSummary { path: String },
    ListRoutes { path: String },
    RouteStops { route: String, path: String },
    Help,
}

fn default_path() -> String {
    DEFAULT_GTFS_PATH.to_string()
}

fn parse_command(args: &[String]) -> Result<Command, String> {
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
            path: default_path(),
        }),
        "gtfs-summary" if args.len() == 2 => Ok(Command::GtfsSummary {
            path: args[1].clone(),
        }),
        "routes" if args.len() == 1 => Ok(Command::ListRoutes {
            path: default_path(),
        }),
        "routes" if args.len() == 2 => Ok(Command::ListRoutes {
            path: args[1].clone(),
        }),
        "route-stops" if args.len() == 2 => Ok(Command::RouteStops {
            route: args[1].clone(),
            path: default_path(),
        }),
        "route-stops" if args.len() == 3 => Ok(Command::RouteStops {
            route: args[1].clone(),
            path: args[2].clone(),
        }),
        "route-stops" => {
            Err("Invalid arguments for 'route-stops'. Usage: oeffi route-stops <route> [gtfs_zip]".to_string())
        }
        "hello" | "help" | "gtfs-summary" | "routes" => {
            Err(format!("Too many arguments for command '{}'.", args[0]))
        }
        unknown => Err(format!("Unknown command: '{unknown}'")),
    }
}

fn load_gtfs(path: &str) -> Result<Gtfs, String> {
    Gtfs::new(path).map_err(|err| format!("Failed to load GTFS from '{path}': {err}"))
}

fn cmd_gtfs_summary(path: &str) -> Result<(), String> {
    let gtfs = load_gtfs(path)?;

    println!("GTFS summary for {path}");
    println!("  agencies: {}", gtfs.agencies.len());
    println!("  routes: {}", gtfs.routes.len());
    println!("  trips: {}", gtfs.trips.len());
    println!("  stops: {}", gtfs.stops.len());
    println!("  calendars: {}", gtfs.calendar.len());
    println!("  calendar_dates: {}", gtfs.calendar_dates.len());

    Ok(())
}

fn cmd_list_routes(path: &str) -> Result<(), String> {
    let gtfs = load_gtfs(path)?;

    let mut rows: Vec<(String, String, String)> = gtfs
        .routes
        .values()
        .map(|route| {
            (
                route.id.clone(),
                route.short_name.clone().unwrap_or_else(|| "-".to_string()),
                route.long_name.clone().unwrap_or_else(|| "-".to_string()),
            )
        })
        .collect();

    rows.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));

    println!("Routes in {path} ({} total):", rows.len());
    for (id, short_name, long_name) in rows {
        println!("  {short_name: <8} | {id: <16} | {long_name}");
    }

    Ok(())
}

fn cmd_route_stops(path: &str, route_name: &str) -> Result<(), String> {
    let gtfs = load_gtfs(path)?;

    let route_ids: HashSet<String> = gtfs
        .routes
        .values()
        .filter(|route| {
            route
                .short_name
                .as_deref()
                .map(|n| n.eq_ignore_ascii_case(route_name))
                .unwrap_or(false)
                || route.id.eq_ignore_ascii_case(route_name)
        })
        .map(|route| route.id.clone())
        .collect();

    if route_ids.is_empty() {
        return Err(format!(
            "No route found for '{route_name}'. Try `oeffi routes` to list available routes."
        ));
    }

    let mut stop_order: HashMap<String, u32> = HashMap::new();
    let mut stop_ids_by_name: HashMap<String, HashSet<String>> = HashMap::new();
    let mut trip_count = 0usize;

    for trip in gtfs.trips.values().filter(|trip| route_ids.contains(&trip.route_id)) {
        trip_count += 1;

        for stop_time in &trip.stop_times {
            let stop_name = stop_time
                .stop
                .name
                .clone()
                .unwrap_or_else(|| format!("<unknown stop {}>", stop_time.stop.id));

            stop_order
                .entry(stop_name.clone())
                .and_modify(|min_seq| {
                    if stop_time.stop_sequence < *min_seq {
                        *min_seq = stop_time.stop_sequence;
                    }
                })
                .or_insert(stop_time.stop_sequence);

            stop_ids_by_name
                .entry(stop_name)
                .or_default()
                .insert(stop_time.stop.id.clone());
        }
    }

    if stop_order.is_empty() {
        return Err(format!(
            "No stop times found for route '{route_name}' in {path}."
        ));
    }

    let mut stops: Vec<(u32, String, usize)> = stop_order
        .into_iter()
        .map(|(stop_name, seq)| {
            let stop_ids = stop_ids_by_name
                .get(&stop_name)
                .map(|ids| ids.len())
                .unwrap_or(0);
            (seq, stop_name, stop_ids)
        })
        .collect();

    stops.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));

    println!(
        "Route {route_name} in {path}: {} unique stops across {} trips",
        stops.len(),
        trip_count
    );

    for (index, (_, stop_name, stop_ids)) in stops.iter().enumerate() {
        if *stop_ids > 1 {
            println!("  {:>3}. {} ({} stop IDs)", index + 1, stop_name, stop_ids);
        } else {
            println!("  {:>3}. {}", index + 1, stop_name);
        }
    }

    Ok(())
}

fn run(command: Command) -> ExitCode {
    let result = match command {
        Command::Hello => {
            println!("hello world");
            Ok(())
        }
        Command::GtfsSummary { path } => cmd_gtfs_summary(&path),
        Command::ListRoutes { path } => cmd_list_routes(&path),
        Command::RouteStops { route, path } => cmd_route_stops(&path, &route),
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
            Ok(Command::GtfsSummary { path }) if path == DEFAULT_GTFS_PATH
        ));
    }

    #[test]
    fn parses_route_stops() {
        let args = vec!["route-stops".to_string(), "U1".to_string()];
        assert!(matches!(
            parse_command(&args),
            Ok(Command::RouteStops { route, path }) if route == "U1" && path == DEFAULT_GTFS_PATH
        ));
    }

    #[test]
    fn rejects_unknown_command() {
        let args = vec!["nope".to_string()];
        let err = parse_command(&args).unwrap_err();
        assert!(err.contains("Unknown command"));
    }
}
