use super::model::{MIN_TRANSFER_SECONDS, PlannerCache, RouteOption, RoutePlanResult};

pub fn format_secs_hhmm(secs: usize) -> String {
    let days = secs / 86_400;
    let rem = secs % 86_400;
    let h = rem / 3_600;
    let m = (rem % 3_600) / 60;
    if days > 0 {
        format!("{h:02}:{m:02} (+{days}d)")
    } else {
        format!("{h:02}:{m:02}")
    }
}

fn format_delta_secs(secs: usize) -> String {
    let m = secs / 60;
    let s = secs % 60;
    if m > 0 {
        format!("{m}m {s:02}s")
    } else {
        format!("{s}s")
    }
}

fn station_label_debug(cache: &PlannerCache, station_idx: u32) -> String {
    let station = &cache.stations[station_idx as usize];
    let preview: Vec<&str> = station
        .member_stop_ids
        .iter()
        .take(3)
        .map(|s| s.as_str())
        .collect();
    let suffix = if station.member_stop_ids.len() > 3 {
        format!(" +{} more", station.member_stop_ids.len() - 3)
    } else {
        String::new()
    };
    format!("{} [{}{}]", station.name, preview.join(", "), suffix)
}

fn station_label(cache: &PlannerCache, station_idx: u32, debug: bool) -> String {
    if debug {
        station_label_debug(cache, station_idx)
    } else {
        cache.stations[station_idx as usize].name.clone()
    }
}

fn print_option(cache: &PlannerCache, option: &RouteOption, debug: bool) {
    if option.access_secs > 0 {
        println!(
            "  Walk to start station: {}",
            format_delta_secs(option.access_secs)
        );
    }
    for (idx, leg) in option.legs.iter().enumerate() {
        let route = &cache.routes[leg.route_idx as usize];
        println!(
            "  {}. Ride {} [{}] {} -> {}",
            idx + 1,
            route.short_name,
            route.base_route_id,
            station_label(cache, leg.from_station_idx, debug),
            station_label(cache, leg.to_station_idx, debug)
        );
        println!(
            "     dep {} | arr {} | {} stops",
            format_secs_hhmm(leg.departure),
            format_secs_hhmm(leg.arrival),
            leg.stops_count
        );
        if option.legs.get(idx + 1).is_some() {
            println!(
                "     transfer at {}",
                station_label(cache, leg.to_station_idx, debug)
            );
        }
    }
    if option.egress_secs > 0 {
        println!(
            "  Walk from destination station: {}",
            format_delta_secs(option.egress_secs)
        );
    }
}

pub fn print_route_plan(cache: &PlannerCache, result: &RoutePlanResult, debug: bool) {
    println!(
        "Route plan: '{}' -> '{}'",
        result.from_query, result.to_query
    );
    println!("Service day: {}", result.query_date);
    println!(
        "Departure (query time): {}",
        format_secs_hhmm(result.depart_secs)
    );
    println!("Arrival: {}", format_secs_hhmm(result.arrival_secs));
    if result.chosen_access_secs > 0 || result.chosen_egress_secs > 0 {
        println!(
            "Door-to-door walking: {}",
            format_delta_secs(result.chosen_access_secs + result.chosen_egress_secs)
        );
    }

    if debug {
        println!("Model: station-normalized planning (hybrid stop->station cache)");
        println!(
            "Minimum transfer time between legs: {}",
            format_delta_secs(MIN_TRANSFER_SECONDS)
        );
        println!("\nMatched origin stations:");
        for station_idx in &result.from_station_idxs {
            println!("  - {}", station_label(cache, *station_idx, true));
        }
        println!("Matched destination stations:");
        for station_idx in &result.to_station_idxs {
            println!("  - {}", station_label(cache, *station_idx, true));
        }
        println!("\nChosen station pair:");
        println!(
            "  from: {}",
            station_label(cache, result.chosen_from_idx, true)
        );
        println!(
            "  to:   {}",
            station_label(cache, result.chosen_to_idx, true)
        );

        let reachable = result
            .evaluated_pairs
            .iter()
            .filter(|p| p.best_arrival.is_some())
            .count();
        let unreachable = result.evaluated_pairs.len().saturating_sub(reachable);
        println!("\nDebug summary:");
        println!(
            "  evaluated station pairs: {} | reachable: {} | unreachable: {}",
            result.evaluated_pairs.len(),
            reachable,
            unreachable
        );

        let mut best_pairs: Vec<_> = result
            .evaluated_pairs
            .iter()
            .filter(|p| p.best_arrival.is_some())
            .collect();
        best_pairs.sort_by(|a, b| {
            a.best_arrival
                .unwrap_or(usize::MAX)
                .cmp(&b.best_arrival.unwrap_or(usize::MAX))
                .then(
                    a.legs_count
                        .unwrap_or(usize::MAX)
                        .cmp(&b.legs_count.unwrap_or(usize::MAX)),
                )
        });
        if !best_pairs.is_empty() {
            println!("  top reachable station pairs:");
            for pair in best_pairs.into_iter().take(5) {
                println!(
                    "    - {} -> {} | arrival {} | legs {} | pareto {}",
                    station_label_debug(cache, pair.from_idx),
                    station_label_debug(cache, pair.to_idx),
                    format_secs_hhmm(pair.best_arrival.unwrap_or(0)),
                    pair.legs_count.unwrap_or(0),
                    pair.pareto_count
                );
            }
        }
    }

    if result.chosen_legs.is_empty() {
        println!("\nNo transit legs needed (already at destination station).\n");
        return;
    }

    println!("\nItinerary:");
    let chosen = RouteOption {
        from_idx: result.chosen_from_idx,
        to_idx: result.chosen_to_idx,
        adjusted_arrival: result.arrival_secs,
        access_secs: result.chosen_access_secs,
        egress_secs: result.chosen_egress_secs,
        generalized_cost: 0,
        legs: result.chosen_legs.clone(),
    };
    print_option(cache, &chosen, debug);

    if debug {
        println!("\nAlternatives (why not picked):");
        if result.alternatives.is_empty() {
            println!("  (No additional alternatives found.)");
        } else {
            for (idx, alt) in result.alternatives.iter().enumerate() {
                let delay = alt.adjusted_arrival.saturating_sub(result.arrival_secs);
                println!(
                    "  Alternative {}: arrival {} ({} later), legs {}, walk {}",
                    idx + 1,
                    format_secs_hhmm(alt.adjusted_arrival),
                    format_delta_secs(delay),
                    alt.legs.len(),
                    format_delta_secs(alt.access_secs + alt.egress_secs)
                );
                println!(
                    "    pair: {} -> {}",
                    station_label_debug(cache, alt.from_idx),
                    station_label_debug(cache, alt.to_idx)
                );
                print_option(cache, alt, true);
            }
        }
    }
}
