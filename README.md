# Wien Öffi CLI

CLI for inspecting Wiener Linien GTFS data and planning routes.

## Data Source
- GTFS Wiener Linien (data.gv.at):  
  `https://www.data.gv.at/datasets/ab4a73b6-1c2d-42e1-b4d9-049e04889cf0?locale=de`

## Main Commands
- `oeffi gtfs-summary`
- `oeffi routes`
- `oeffi route-stops <route> [--all]`
- `oeffi stop-inspect <query>`
- `oeffi route-plan <from> <to> [--debug] [--alts N]`
- `oeffi cache-build [gtfs_path] [cache_file]`

Notes:
- `route-plan` now applies `calendar.txt` + `calendar_dates.txt` for the current service day.

## Cache Files
- `gtfs.cache.bin`: snapshot cache for inspect/list commands.
- `planner.cache.bin`: planner cache with station-normalized RAPTOR model.
- `cache-build` always rebuilds both caches.
