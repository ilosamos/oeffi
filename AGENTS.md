# AGENTS.md

## Project Summary
- Rust CLI tool for inspecting and routing on Wiener Linien GTFS data.
- Uses two cache files built from `data/` GTFS text files:
  - `gtfs.cache.bin` for snapshot/inspect commands
  - `planner.cache.bin` for route planning
- Main commands: `gtfs-summary`, `routes`, `route-stops`, `stop-inspect`, `route-plan`, `cache-build`.

## Quick File Tree
```text
.
├── Cargo.toml
├── README.md
├── AGENTS.md
├── data/
│   ├── agency.txt
│   ├── calendar.txt
│   ├── calendar_dates.txt
│   ├── routes.txt
│   ├── stops.txt
│   ├── trips.txt
│   ├── stop_times.txt
│   └── shapes.txt
├── src/
│   ├── main.rs
│   ├── cli.rs
│   ├── cache.rs
│   ├── cache_meta.rs
│   ├── build.rs
│   ├── commands.rs
│   ├── matcher.rs
│   ├── route_planner.rs
│   ├── route_planner/
│   │   ├── model.rs
│   │   ├── cache.rs
│   │   ├── query.rs
│   │   ├── output.rs
│   │   └── raptor_adapter.rs
│   ├── snapshot.rs
│   └── clustering.rs
├── gtfs.cache.bin
└── planner.cache.bin
```

## GTFS TXT Structure (Samples)
Notes:
- Several files contain a UTF-8 BOM on the header line.
- Values are CSV with quoted strings.

### `data/agency.txt`
```csv
agency_id,agency_name,agency_url,agency_timezone,agency_lang,agency_fare_url
"04","Wiener Linien GmbH & Co KG","https://www.wienerlinien.at","Europe/Vienna","DE","https://shop.wienmobil.at/products"
"03","Wiener Lokalbahnen GmbH","https://www.wlb.at","Europe/Vienna","DE",""
```

### `data/routes.txt`
```csv
route_id,agency_id,route_short_name,route_long_name,route_type,route_color,route_text_color
"11-WLB-j26-1","03","BB","Wien Oper - Aßmayergasse - Inzersdorf Lokalbahn - Wiener Neudorf - Baden Landesklinikum - Baden Josefsplatz","0","0A295D","FFFFFF"
"11-WLB-j26-2","03","BB","Oper/Karlsplatz U - Aßmayergasse - Inzersdorf Lokalbahn - Baden Landesklinikum - Baden Josefsplatz","0","0A295D","FFFFFF"
```

### `data/stops.txt`
```csv
stop_id,stop_name,stop_lat,stop_lon,zone_id
"at:43:3121:0:1","Baden Josefsplatz","48.00598750","16.23376093","3045"
"at:43:3134:0:1","Baden Viadukt","48.00383581","16.24096542","3045"
```

### `data/trips.txt`
```csv
route_id,service_id,trip_id,shape_id,trip_headsign,direction_id,block_id
"11-WLB-j26-1","T0+bb02","1.T0.11-WLB-j26-1.35.H","11-WLB-j26-1.35.H","Baden Josefsplatz","0",""
"11-WLB-j26-1","T3","10.T3.11-WLB-j26-1.11.H","11-WLB-j26-1.11.H","Baden Josefsplatz","0",""
```

### `data/stop_times.txt`
```csv
trip_id,arrival_time,departure_time,stop_id,stop_sequence,pickup_type,drop_off_type,shape_dist_traveled
"1.T0.23-13A-j26-2.3.H","05:15:00","05:15:00","at:49:1267:0:5","1","0","0","0.00"
"1.T0.23-13A-j26-2.3.H","05:16:00","05:16:00","at:49:753:0:4","2","0","0","192.20"
```

### `data/calendar.txt`
```csv
service_id,monday,tuesday,wednesday,thursday,friday,saturday,sunday,start_date,end_date
"T0","1","1","1","1","1","0","0","20251214","20251220"
"T0#1","1","1","1","1","1","0","0","20251214","20261212"
```

### `data/calendar_dates.txt`
```csv
service_id,date,exception_type
"T0#1","20251224","2"
"T0#1","20251225","2"
```
