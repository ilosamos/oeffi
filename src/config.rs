use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

pub const ENV_CONFIG_PATH: &str = "OEFFI_CONFIG_PATH";
pub const ENV_MERGED_GTFS_PATH: &str = "OEFFI_MERGED_GTFS_PATH";
pub const ENV_SNAPSHOT_CACHE_PATH: &str = "OEFFI_SNAPSHOT_CACHE_PATH";
pub const ENV_PLANNER_CACHE_PATH: &str = "OEFFI_PLANNER_CACHE_PATH";
pub const ENV_RAW_DATA_ROOT: &str = "OEFFI_RAW_DATA_ROOT";
pub const ENV_WIENER_LINIEN_SOURCE_DIR: &str = "OEFFI_WIENER_LINIEN_SOURCE_DIR";
pub const ENV_OEBB_SOURCE_DIR: &str = "OEFFI_OEBB_SOURCE_DIR";
pub const ENV_WIENER_LINIEN_GTFS_URL: &str = "OEFFI_WIENER_LINIEN_GTFS_URL";
pub const ENV_OEBB_GTFS_URL: &str = "OEFFI_OEBB_GTFS_URL";
pub const ENV_AUSTRIA_OSM_PBF_PATH: &str = "OEFFI_AUSTRIA_OSM_PBF_PATH";
pub const ENV_GEOCODE_CACHE_PATH: &str = "OEFFI_GEOCODE_CACHE_PATH";

const DEFAULT_WIENER_LINIEN_GTFS_URL: &str =
    "http://www.wienerlinien.at/ogd_realtime/doku/ogd/gtfs/gtfs.zip";
const DEFAULT_OEBB_GTFS_URL: &str =
    "https://static.web.oebb.at/open-data/soll-fahrplan-gtfs/GTFS_Fahrplan_2026.zip";
const DEFAULT_AUSTRIA_OSM_PBF_PATH: &str = "data/austria.osm.bpf";
const DEFAULT_GEOCODE_CACHE_PATH: &str = "data/vienna-addresses.cache.bin";

fn default_austria_osm_pbf_path() -> String {
    DEFAULT_AUSTRIA_OSM_PBF_PATH.to_string()
}

fn default_geocode_cache_path() -> String {
    DEFAULT_GEOCODE_CACHE_PATH.to_string()
}

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub config_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub merged_gtfs_path: String,
    pub snapshot_cache_path: String,
    pub planner_cache_path: String,
    pub raw_data_root: String,
    pub wiener_linien_source_dir: String,
    pub oebb_source_dir: String,
    pub wiener_linien_gtfs_url: String,
    pub oebb_gtfs_url: String,
    #[serde(default = "default_austria_osm_pbf_path")]
    pub austria_osm_pbf_path: String,
    #[serde(default = "default_geocode_cache_path")]
    pub geocode_cache_path: String,
}

#[derive(Debug, Clone)]
pub struct LoadedConfig {
    pub paths: AppPaths,
    pub file_config: AppConfig,
    pub effective_config: AppConfig,
    pub env_overrides: HashSet<&'static str>,
}

fn default_paths() -> Result<AppPaths, String> {
    let project_dirs = ProjectDirs::from("com", "github", "oeffi")
        .ok_or_else(|| "Could not resolve OS app directories for oeffi".to_string())?;

    let config_dir = project_dirs.config_dir().to_path_buf();
    let data_dir = project_dirs.data_dir().to_path_buf();
    let cache_dir = project_dirs.cache_dir().to_path_buf();
    let config_path = env::var(ENV_CONFIG_PATH)
        .map(PathBuf::from)
        .unwrap_or_else(|_| config_dir.join("config.json"));

    Ok(AppPaths {
        config_dir,
        data_dir,
        cache_dir,
        config_path,
    })
}

fn as_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn default_config(paths: &AppPaths) -> AppConfig {
    let raw_root = paths.data_dir.join("raw");
    let wiener_linien_source_dir = raw_root.join("wiener-linien");
    let oebb_source_dir = raw_root.join("oebb");
    let merged_gtfs_path = paths.data_dir.join("combined-vienna");
    let snapshot_cache_path = paths.cache_dir.join("gtfs.cache.bin");
    let planner_cache_path = paths.cache_dir.join("planner.cache.bin");

    AppConfig {
        merged_gtfs_path: as_string(&merged_gtfs_path),
        snapshot_cache_path: as_string(&snapshot_cache_path),
        planner_cache_path: as_string(&planner_cache_path),
        raw_data_root: as_string(&raw_root),
        wiener_linien_source_dir: as_string(&wiener_linien_source_dir),
        oebb_source_dir: as_string(&oebb_source_dir),
        wiener_linien_gtfs_url: DEFAULT_WIENER_LINIEN_GTFS_URL.to_string(),
        oebb_gtfs_url: DEFAULT_OEBB_GTFS_URL.to_string(),
        austria_osm_pbf_path: DEFAULT_AUSTRIA_OSM_PBF_PATH.to_string(),
        geocode_cache_path: DEFAULT_GEOCODE_CACHE_PATH.to_string(),
    }
}

pub fn default_file_config(paths: &AppPaths) -> AppConfig {
    default_config(paths)
}

fn ensure_parent_dirs(paths: &AppPaths, config: &AppConfig) -> Result<(), String> {
    fs::create_dir_all(&paths.config_dir).map_err(|err| {
        format!(
            "Failed to create config dir '{}': {err}",
            paths.config_dir.display()
        )
    })?;
    fs::create_dir_all(&paths.data_dir).map_err(|err| {
        format!(
            "Failed to create data dir '{}': {err}",
            paths.data_dir.display()
        )
    })?;
    fs::create_dir_all(&paths.cache_dir).map_err(|err| {
        format!(
            "Failed to create cache dir '{}': {err}",
            paths.cache_dir.display()
        )
    })?;

    if let Some(parent) = Path::new(&paths.config_path).parent() {
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "Failed to create config parent '{}': {err}",
                parent.display()
            )
        })?;
    }

    for path_str in [
        &config.raw_data_root,
        &config.wiener_linien_source_dir,
        &config.oebb_source_dir,
        &config.merged_gtfs_path,
    ] {
        fs::create_dir_all(path_str)
            .map_err(|err| format!("Failed to create data directory '{}': {err}", path_str))?;
    }

    Ok(())
}

pub fn ensure_dirs_for_config(paths: &AppPaths, config: &AppConfig) -> Result<(), String> {
    ensure_parent_dirs(paths, config)
}

fn apply_env_override(
    value: &mut String,
    env_name: &'static str,
    overrides: &mut HashSet<&'static str>,
) {
    if let Ok(v) = env::var(env_name) {
        *value = v;
        overrides.insert(env_name);
    }
}

fn apply_env_overrides(config: &mut AppConfig) -> HashSet<&'static str> {
    let mut overrides = HashSet::new();

    apply_env_override(
        &mut config.merged_gtfs_path,
        ENV_MERGED_GTFS_PATH,
        &mut overrides,
    );
    apply_env_override(
        &mut config.snapshot_cache_path,
        ENV_SNAPSHOT_CACHE_PATH,
        &mut overrides,
    );
    apply_env_override(
        &mut config.planner_cache_path,
        ENV_PLANNER_CACHE_PATH,
        &mut overrides,
    );
    apply_env_override(&mut config.raw_data_root, ENV_RAW_DATA_ROOT, &mut overrides);
    apply_env_override(
        &mut config.wiener_linien_source_dir,
        ENV_WIENER_LINIEN_SOURCE_DIR,
        &mut overrides,
    );
    apply_env_override(
        &mut config.oebb_source_dir,
        ENV_OEBB_SOURCE_DIR,
        &mut overrides,
    );
    apply_env_override(
        &mut config.wiener_linien_gtfs_url,
        ENV_WIENER_LINIEN_GTFS_URL,
        &mut overrides,
    );
    apply_env_override(&mut config.oebb_gtfs_url, ENV_OEBB_GTFS_URL, &mut overrides);
    apply_env_override(
        &mut config.austria_osm_pbf_path,
        ENV_AUSTRIA_OSM_PBF_PATH,
        &mut overrides,
    );
    apply_env_override(
        &mut config.geocode_cache_path,
        ENV_GEOCODE_CACHE_PATH,
        &mut overrides,
    );

    overrides
}

fn write_config(path: &Path, config: &AppConfig) -> Result<(), String> {
    let payload = serde_json::to_string_pretty(config)
        .map_err(|err| format!("Failed to serialize config '{}': {err}", path.display()))?;
    fs::write(path, payload)
        .map_err(|err| format!("Failed writing config '{}': {err}", path.display()))
}

pub fn load_or_init_config() -> Result<LoadedConfig, String> {
    let paths = default_paths()?;
    let defaults = default_config(&paths);
    ensure_parent_dirs(&paths, &defaults)?;

    let file_config = if paths.config_path.exists() {
        let raw = fs::read_to_string(&paths.config_path).map_err(|err| {
            format!(
                "Failed reading config '{}': {err}",
                paths.config_path.display()
            )
        })?;
        serde_json::from_str::<AppConfig>(&raw).map_err(|err| {
            format!(
                "Failed parsing config '{}': {err}. Fix the file or remove it to regenerate defaults.",
                paths.config_path.display()
            )
        })?
    } else {
        write_config(&paths.config_path, &defaults)?;
        defaults.clone()
    };

    ensure_parent_dirs(&paths, &file_config)?;

    let mut effective_config = file_config.clone();
    let env_overrides = apply_env_overrides(&mut effective_config);

    Ok(LoadedConfig {
        paths,
        file_config,
        effective_config,
        env_overrides,
    })
}

pub fn persist_file_config(paths: &AppPaths, config: &AppConfig) -> Result<(), String> {
    write_config(&paths.config_path, config)
}

pub fn config_keys() -> &'static [&'static str] {
    &[
        "merged_gtfs_path",
        "snapshot_cache_path",
        "planner_cache_path",
        "raw_data_root",
        "wiener_linien_source_dir",
        "oebb_source_dir",
        "wiener_linien_gtfs_url",
        "oebb_gtfs_url",
        "austria_osm_pbf_path",
        "geocode_cache_path",
    ]
}

pub fn env_var_for_key(key: &str) -> Option<&'static str> {
    match key {
        "merged_gtfs_path" => Some(ENV_MERGED_GTFS_PATH),
        "snapshot_cache_path" => Some(ENV_SNAPSHOT_CACHE_PATH),
        "planner_cache_path" => Some(ENV_PLANNER_CACHE_PATH),
        "raw_data_root" => Some(ENV_RAW_DATA_ROOT),
        "wiener_linien_source_dir" => Some(ENV_WIENER_LINIEN_SOURCE_DIR),
        "oebb_source_dir" => Some(ENV_OEBB_SOURCE_DIR),
        "wiener_linien_gtfs_url" => Some(ENV_WIENER_LINIEN_GTFS_URL),
        "oebb_gtfs_url" => Some(ENV_OEBB_GTFS_URL),
        "austria_osm_pbf_path" => Some(ENV_AUSTRIA_OSM_PBF_PATH),
        "geocode_cache_path" => Some(ENV_GEOCODE_CACHE_PATH),
        _ => None,
    }
}

pub fn get_config_value<'a>(cfg: &'a AppConfig, key: &str) -> Option<&'a str> {
    match key {
        "merged_gtfs_path" => Some(&cfg.merged_gtfs_path),
        "snapshot_cache_path" => Some(&cfg.snapshot_cache_path),
        "planner_cache_path" => Some(&cfg.planner_cache_path),
        "raw_data_root" => Some(&cfg.raw_data_root),
        "wiener_linien_source_dir" => Some(&cfg.wiener_linien_source_dir),
        "oebb_source_dir" => Some(&cfg.oebb_source_dir),
        "wiener_linien_gtfs_url" => Some(&cfg.wiener_linien_gtfs_url),
        "oebb_gtfs_url" => Some(&cfg.oebb_gtfs_url),
        "austria_osm_pbf_path" => Some(&cfg.austria_osm_pbf_path),
        "geocode_cache_path" => Some(&cfg.geocode_cache_path),
        _ => None,
    }
}

pub fn set_config_value(cfg: &mut AppConfig, key: &str, value: String) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err("config value must not be empty".to_string());
    }

    match key {
        "merged_gtfs_path" => cfg.merged_gtfs_path = value,
        "snapshot_cache_path" => cfg.snapshot_cache_path = value,
        "planner_cache_path" => cfg.planner_cache_path = value,
        "raw_data_root" => cfg.raw_data_root = value,
        "wiener_linien_source_dir" => cfg.wiener_linien_source_dir = value,
        "oebb_source_dir" => cfg.oebb_source_dir = value,
        "wiener_linien_gtfs_url" => cfg.wiener_linien_gtfs_url = value,
        "oebb_gtfs_url" => cfg.oebb_gtfs_url = value,
        "austria_osm_pbf_path" => cfg.austria_osm_pbf_path = value,
        "geocode_cache_path" => cfg.geocode_cache_path = value,
        _ => {
            return Err(format!(
                "unknown config key '{key}'. Valid keys: {}",
                config_keys().join(", ")
            ));
        }
    }

    Ok(())
}
