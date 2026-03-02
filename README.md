# Wien Öffi CLI

CLI for inspecting Vienna GTFS data and planning routes.

## Data Source
- GTFS Wiener Linien (data.gv.at):  
  `https://www.data.gv.at/datasets/ab4a73b6-1c2d-42e1-b4d9-049e04889cf0?locale=de`
- GTFS ÖBB:
  `https://static.web.oebb.at/open-data/soll-fahrplan-gtfs/GTFS_Fahrplan_2026.zip`

At runtime, the CLI builds and uses a merged source at `data/combined-vienna`:
- Wiener Linien (full source feed)
- ÖBB (Vienna-only `at:49:*` stops, commuter rail routes only: `S*`, `REX*`, `R*`)

## Main Commands
- `oeffi route <from> <to> [--debug] [--alts N] [--depart HH:MM] [--date YYYY-MM-DD]`
- `oeffi route-coords <from_lat> <from_lon> <to_lat> <to_lon> [--debug] [--alts N] [--depart HH:MM] [--date YYYY-MM-DD]`
- `oeffi line <route>`
- `oeffi inspect <query>`
- `oeffi routes`
- `oeffi summary`
- `oeffi cache-build [gtfs_path] [cache_file]`

Notes:
- `route` now applies `calendar.txt` + `calendar_dates.txt` for the current service day.

## Cache Files
- `gtfs.cache.bin`: snapshot cache for inspect/list commands.
- `planner.cache.bin`: planner cache with station-normalized RAPTOR model.
- `cache-build` always rebuilds both caches.

## Todo
- Support trip planning by two arbitrary coordinates or adresses (calculate nearest stations)
- Add onboarding flow (download, cache build) with commands setup, refresh, wipe-cache etc.
