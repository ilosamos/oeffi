# oeffi - Wiener Linien Öffi CLI

Fast local CLI for Vienna transit routing using Wiener Linien + ÖBB GTFS data.

## TL;DR

Route plan example:

```console
$ oeffi route praterstern westbahnhof
Route plan: 'praterstern' -> 'westbahnhof'
Service day: 2026-03-03
Departure (query time): 08:33
Arrival: 08:48

Itinerary:
  1. Ride U1 [21-U1-j26-1] Wien Praterstern -> Stephansplatz
     dep 08:34 | arr 08:37 | 3 stops
     transfer at Stephansplatz
  2. Ride U3 [21-U3-j26-1] Stephansplatz -> Wien Westbahnhof
     dep 08:41 | arr 08:48 | 5 stops
```

Other commands:

```bash
# first run (download data and build cache)
oeffi init

# route by station names
oeffi route "Karlsplatz" "Praterstern"

# route with date/time
oeffi route "Herrengasse" "Praterstern" --date 2026-03-02 --depart 08:15

# route by coordinates
oeffi route-coords 48.2066 16.3707 48.1850 16.3747
```

## Features

- Blazingly fast route planning and data inspection
- Local caches, no external planner API
- Can be easily integrated as agent skill
- Built on [`raptor-rs`](https://github.com/keogami/raptor-rs) for RAPTOR-based route planning

## Install

### From GitHub Releases

Download the archive for your platform, extract it, and place `oeffi` on your `PATH`.

```bash
VERSION=0.1.0
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

## Common commands

```bash
oeffi summary
oeffi routes
oeffi stops
oeffi inspect "Karlsplatz"
oeffi line U1
oeffi cache-build
oeffi cache-build --download
oeffi init
oeffi init --force
oeffi config list
```

## First run and updates

- `oeffi init`: download raw data, merge feeds, build caches
- `oeffi init --force`: same as above, but overwrites existing raw data
- `oeffi cache-build --download`: refresh raw data and rebuild caches

## Compatibility

- macOS Intel: `x86_64-apple-darwin`
- macOS Apple Silicon: `aarch64-apple-darwin`
- Linux x86_64: `x86_64-unknown-linux-gnu`
- No Windows artifacts currently

## Limitations

- No geocoding for arbitrary addresses yet
- No realtime outage/delay integration yet
- Local GTFS data can become stale and should be refreshed regularly
- Merge/build logic assumes current GTFS structure

## Data sources

- Wiener Linien GTFS
  - Info: `https://www.data.gv.at/datasets/ab4a73b6-1c2d-42e1-b4d9-049e04889cf0?locale=de`
  - ZIP: `http://www.wienerlinien.at/ogd_realtime/doku/ogd/gtfs/gtfs.zip`
- ÖBB GTFS
  - Info: `https://data.oebb.at/de/datensaetze~soll-fahrplan-gtfs~`
  - ZIP: `https://static.web.oebb.at/open-data/soll-fahrplan-gtfs/GTFS_Fahrplan_2026.zip`

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

By default paths are resolved using OS app directories (`directories::ProjectDirs`).

## TODO

- Add geocoding support for arbitrary addresses
- Add realtime data integration (outages, delays, disruptions)
- Add better stale-data checks and refresh reminders
- Improve resilience against GTFS schema changes

## License

This project is licensed under the terms in [LICENSE](LICENSE).
