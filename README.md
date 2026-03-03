# oeffi - Wiener Linien Öffi CLI

Fast local CLI for Vienna transit routing using Wiener Linien + ÖBB GTFS data and OSM data for geocoding.

## TL;DR

Simple route plan example:

```console
$ oeffi route westbahnhof "mochi ramen bar"
Route plan: 'westbahnhof' -> 'mochi ramen bar'
Service day: 2026-03-03
Departure (query time): 22:30
Arrival: 22:51
Door-to-door walking: 5m

Itinerary:
  1. Ride U3 Wien Westbahnhof -> Stephansplatz
     dep 22:33 | arr 22:40 | 5 stops
     transfer at Stephansplatz
  2. Ride U1 Stephansplatz -> Vorgartenstraße
     dep 22:43 | arr 22:47 | 4 stops
  Walk from destination station: 5m
```

More examples:

```bash
# first run (download data and build cache) will take a few minutes
oeffi init

# routes work with stops, addresses, landmarks, restaurants, coordinates, etc.
oeffi route "Karlsplatz" "Praterstern"
oeffi route "mochi ramen bar" "haus des meeres"
oeffi route "48.2066 16.3707" "48.1850 16.3747"

# route with date/time
oeffi route "maria hilfer strasse 1" "Praterstern" --date 2026-03-02 --depart 08:15
```

## Features

- Blazingly fast route planning and data inspection
- Support route planning with stops, addresses, coordinates, landmarks, restaurants etc.
- Local caches, no external planner API
- Can be easily integrated as agent skill
- Built on `[raptor-rs](https://github.com/keogami/raptor-rs)` for RAPTOR-based route planning

## Install

### From GitHub Releases

Download the archive for your platform, extract it, and place `oeffi` on your `PATH`.

MacOS:

```bash
VERSION=0.1.5
# macOS Apple Silicon (aarch64)
curl -L -o oeffi.tar.gz "https://github.com/ilosamos/oeffi/releases/download/v${VERSION}/oeffi-${VERSION}-aarch64-apple-darwin.tar.gz"
tar -xzf oeffi.tar.gz
sudo install -m 755 oeffi-${VERSION}-aarch64-apple-darwin/oeffi /usr/local/bin/oeffi
oeffi version
```

Linux:

```bash
VERSION=0.1.5
# Linux x86_64
curl -L -o oeffi.tar.gz "https://github.com/ilosamos/oeffi/releases/download/v${VERSION}/oeffi-${VERSION}-x86_64-unknown-linux-gnu.tar.gz"
tar -xzf oeffi.tar.gz
sudo install -m 755 oeffi-${VERSION}-x86_64-unknown-linux-gnu/oeffi /usr/local/bin/oeffi
oeffi version
```

### From source

```bash
cargo build --release
./target/release/oeffi version
```

## All commands

```bash
oeffi init
oeffi route <from> <to>
oeffi geocode <query>
oeffi inspect <query>
oeffi stops
oeffi line <route>
oeffi routes
oeffi summary
oeffi cache <subcommand>
oeffi config <subcommand>
oeffi version
oeffi help
```

## First run and updates

- `oeffi init`: download raw data, merge feeds, build caches
- `oeffi init --force`: same as above, but overwrites existing raw data
- `oeffi cache build --download`: refresh raw data and rebuild caches

## Compatibility

- macOS Intel: `x86_64-apple-darwin`
- macOS Apple Silicon: `aarch64-apple-darwin`
- Linux x86_64: `x86_64-unknown-linux-gnu`
- No Windows artifacts currently

## Limitations

- No realtime outage/delay integration yet
- Local GTFS data can become stale and should be refreshed regularly

## Data sources

- Wiener Linien GTFS
  - Info: `https://www.data.gv.at/datasets/ab4a73b6-1c2d-42e1-b4d9-049e04889cf0?locale=de`
  - ZIP: `http://www.wienerlinien.at/ogd_realtime/doku/ogd/gtfs/gtfs.zip`
- ÖBB GTFS
  - Info: `https://data.oebb.at/de/datensaetze~soll-fahrplan-gtfs~`
  - ZIP: `https://static.web.oebb.at/open-data/soll-fahrplan-gtfs/GTFS_Fahrplan_2026.zip`
- OSM Austria Map Data:
  - INFO: `https://download.geofabrik.de/europe.html`
  - DATA: `https://download.geofabrik.de/europe/austria-latest.osm.pbf`

Merged dataset includes Wiener Linien feed plus scoped ÖBB commuter rail data around Vienna.

## Release artifacts

Each release includes:

- `oeffi-<version>-x86_64-apple-darwin.tar.gz`
- `oeffi-<version>-aarch64-apple-darwin.tar.gz`
- `oeffi-<version>-x86_64-unknown-linux-gnu.tar.gz`
- `SHA256SUMS.txt`

Verify:

```bash
shasum -a 256 -c SHA256SUMS.txt
```

Config details

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
- `austria_osm_pbf_path`
- `austria_osm_pbf_url`
- `geocode_cache_path`

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
- `OEFFI_AUSTRIA_OSM_PBF_PATH`
- `OEFFI_AUSTRIA_OSM_PBF_URL`
- `OEFFI_GEOCODE_CACHE_PATH`

By default paths are resolved using OS app directories (`directories::ProjectDirs`).

## TODO

- Add realtime data integration (outages, delays, disruptions)
- Add better stale-data checks and refresh reminders

## License

This project is licensed under the terms in [LICENSE](LICENSE).