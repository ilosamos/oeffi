# oeffi - Wiener Öffi CLI

A Rust cli for planning transit "Öffi" routes in Vienna. Runs fast and local, optimized to be usable by agents like openclaw. Uses both Wiener Linien and ÖBB data.

## TL;DR

```bash
oeffi init # one time download data and building cache
oeffi route "Karlsplatz" "Praterstern"
oeffi route "Herrengasse" "Praterstern" --date 2026-03-02 --depart 08:15
oeffi route-coords 48.2066 16.3707 48.1850 16.3747
```

## Features

- Fast local execution with on-disk caches
- Local-first workflow (no external API needed for planning)
- Human-readable CLI output
- Agent/script-friendly command interface (works well in tool pipelines, including OpenCode/OpenClaw-style agents)
- Configurable paths and data source URLs

## Data sources

- Wiener Linien GTFS
  - Info page: `https://www.data.gv.at/datasets/ab4a73b6-1c2d-42e1-b4d9-049e04889cf0?locale=de`
  - ZIP: `http://www.wienerlinien.at/ogd_realtime/doku/ogd/gtfs/gtfs.zip`
- ÖBB GTFS
  - Info page: `https://data.oebb.at/de/datensaetze~soll-fahrplan-gtfs~`
  - ZIP: `https://static.web.oebb.at/open-data/soll-fahrplan-gtfs/GTFS_Fahrplan_2026.zip`

The merged dataset keeps:

- Wiener Linien feed (full)
- ÖBB stops around Vienna and commuter rail routes (`S*`, `REX*`, `R*`)

## Quick start

Build and run:

```bash
oeffi init
```

This will:

1. Download raw GTFS data
2. Preprocess/merge sources
3. Build snapshot and planner caches

Then try:

```bash
oeffi route "Karlsplatz" "Praterstern"
```

## Common commands

```bash
oeffi summary
oeffi routes
oeffi stops
oeffi inspect "Karlsplatz"
oeffi line U1
oeffi route "Herrengasse" "Praterstern" --date 2026-03-02 --depart 08:15
oeffi route-coords 48.2066 16.3707 48.1850 16.3747
```

Cache and setup:

```bash
oeffi cache-build
oeffi cache-build --download
oeffi init
oeffi init --force
```

## Config

The CLI stores config as JSON and supports env overrides.

Manage config:

```bash
oeffi config list
oeffi config get merged_gtfs_path
oeffi config set planner_cache_path /tmp/planner.cache.bin
```

Config keys:

- `merged_gtfs_path`
- `snapshot_cache_path`
- `planner_cache_path`
- `raw_data_root`
- `wiener_linien_source_dir`
- `oebb_source_dir`
- `wiener_linien_gtfs_url`
- `oebb_gtfs_url`

Environment overrides:

- `OEFFI_CONFIG_PATH`
- `OEFFI_MERGED_GTFS_PATH`
- `OEFFI_SNAPSHOT_CACHE_PATH`
- `OEFFI_PLANNER_CACHE_PATH`
- `OEFFI_RAW_DATA_ROOT`
- `OEFFI_WIENER_LINIEN_SOURCE_DIR`
- `OEFFI_OEBB_SOURCE_DIR`
- `OEFFI_WIENER_LINIEN_GTFS_URL`
- `OEFFI_OEBB_GTFS_URL`

## Where files are stored

By default, paths are resolved via OS app directories (`directories::ProjectDirs`):

- config JSON in the app config directory
- raw GTFS source data in the app data directory
- cache files in the app cache directory

Use `oeffi config list` to see the effective paths on your machine.

## Cache behavior

- Snapshot cache: used for summary/list/inspect/line commands
- Planner cache: used for route planning
- If caches are stale or missing, they are rebuilt automatically

## Notes

- Route planning uses `calendar.txt` and `calendar_dates.txt` for service-day activation.
- `init --force` is a safety-gated overwrite for existing raw data.

## Limitations

- No geocoding yet: arbitrary street addresses are not supported (use stop names/ids or coordinates).
- No realtime integration: service outages, delays, and disruptions are not reflected.
- Local GTFS data can become stale and should be refreshed periodically (`cache-build --download`).
- Merge/build logic assumes current GTFS file/column structure; upstream schema changes can break ingestion.

## TODO

- Add geocoding support for arbitrary addresses.
- Add realtime data integration (outages, delays, disruptions).
- Add staleness checks/reminders and a smoother refresh workflow for outdated local data.
- Make ingestion/merge pipeline more robust against GTFS schema and format changes.

