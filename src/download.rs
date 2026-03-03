use std::fs;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use zip::ZipArchive;

const REQUIRED_GTFS_FILES: [&str; 7] = [
    "agency.txt",
    "routes.txt",
    "stops.txt",
    "trips.txt",
    "stop_times.txt",
    "calendar.txt",
    "calendar_dates.txt",
];

fn unique_temp_dir(target_dir: &Path) -> PathBuf {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let pid = std::process::id();
    let name = format!(".download_tmp_{pid}_{ts}");
    target_dir.with_file_name(name)
}

fn is_zip_payload(bytes: &[u8], url: &str, content_type: Option<&str>) -> bool {
    if bytes.len() >= 4 && bytes[0..4] == [0x50, 0x4B, 0x03, 0x04] {
        return true;
    }
    if url.to_ascii_lowercase().ends_with(".zip") {
        return true;
    }
    content_type
        .map(|ct| ct.to_ascii_lowercase().contains("zip"))
        .unwrap_or(false)
}

fn extract_zip_link_from_html(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let mut start = 0usize;
    while let Some(idx) = lower[start..].find("href=") {
        let attr_start = start + idx + 5;
        let quote = lower[attr_start..].chars().next()?;
        if quote != '"' && quote != '\'' {
            start = attr_start + 1;
            continue;
        }
        let val_start = attr_start + 1;
        let rel_end = lower[val_start..].find(quote)?;
        let val_end = val_start + rel_end;
        let href = &html[val_start..val_end];
        if href.to_ascii_lowercase().contains(".zip") {
            return Some(href.to_string());
        }
        start = val_end + 1;
    }
    None
}

fn resolve_link(base_url: &str, link: &str) -> Option<String> {
    if link.starts_with("http://") || link.starts_with("https://") {
        return Some(link.to_string());
    }
    if link.starts_with("//") {
        return Some(format!("https:{link}"));
    }
    if link.starts_with('/') {
        let scheme_end = base_url.find("://")?;
        let host_start = scheme_end + 3;
        let host_end = base_url[host_start..]
            .find('/')
            .map(|i| host_start + i)
            .unwrap_or(base_url.len());
        return Some(format!("{}{}", &base_url[..host_end], link));
    }
    None
}

fn download_bytes(url: &str) -> Result<(Vec<u8>, Option<String>), String> {
    let response = ureq::get(url)
        .call()
        .map_err(|err| format!("Failed to download '{url}': {err}"))?;
    let content_type = response.header("content-type").map(|v| v.to_string());

    let mut bytes = Vec::new();
    let mut reader = response.into_reader();
    reader
        .read_to_end(&mut bytes)
        .map_err(|err| format!("Failed reading response body for '{url}': {err}"))?;
    Ok((bytes, content_type))
}

pub fn download_file_to_path(url: &str, target_path: &str, label: &str) -> Result<(), String> {
    let target = Path::new(target_path);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("Failed creating directory '{}': {err}", parent.display()))?;
    }

    let tmp_path = target.with_extension("download_tmp");
    if tmp_path.exists() {
        let _ = fs::remove_file(&tmp_path);
    }

    eprintln!("Downloading {label} from {url} ...");
    let response = ureq::get(url)
        .call()
        .map_err(|err| format!("Failed to download '{url}': {err}"))?;
    let mut reader = response.into_reader();
    let mut out = fs::File::create(&tmp_path)
        .map_err(|err| format!("Failed creating file '{}': {err}", tmp_path.display()))?;
    std::io::copy(&mut reader, &mut out)
        .map_err(|err| format!("Failed writing file '{}': {err}", tmp_path.display()))?;

    fs::rename(&tmp_path, target).map_err(|err| {
        format!(
            "Failed replacing '{}' with downloaded file '{}': {err}",
            target.display(),
            tmp_path.display()
        )
    })?;

    Ok(())
}

fn has_required_gtfs_files(dir: &Path) -> bool {
    REQUIRED_GTFS_FILES
        .iter()
        .all(|name| dir.join(name).is_file())
}

fn find_gtfs_root(base: &Path) -> Option<PathBuf> {
    if has_required_gtfs_files(base) {
        return Some(base.to_path_buf());
    }

    let mut stack = vec![base.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = fs::read_dir(&dir).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            if has_required_gtfs_files(&path) {
                return Some(path);
            }
            stack.push(path);
        }
    }

    None
}

fn extract_zip_bytes(bytes: &[u8], out_dir: &Path) -> Result<(), String> {
    let cursor = Cursor::new(bytes);
    let mut zip = ZipArchive::new(cursor).map_err(|err| format!("Failed to read zip: {err}"))?;

    for i in 0..zip.len() {
        let mut file = zip
            .by_index(i)
            .map_err(|err| format!("Failed to read zip entry #{i}: {err}"))?;
        let Some(safe_name) = file.enclosed_name().map(|p| p.to_path_buf()) else {
            continue;
        };

        let out_path = out_dir.join(safe_name);
        if file.name().ends_with('/') {
            fs::create_dir_all(&out_path).map_err(|err| {
                format!(
                    "Failed creating directory from zip '{}': {err}",
                    out_path.display()
                )
            })?;
            continue;
        }

        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent).map_err(|err| {
                format!("Failed creating directory '{}': {err}", parent.display())
            })?;
        }

        let mut out = fs::File::create(&out_path)
            .map_err(|err| format!("Failed creating file '{}': {err}", out_path.display()))?;
        std::io::copy(&mut file, &mut out)
            .map_err(|err| format!("Failed extracting '{}': {err}", out_path.display()))?;
    }

    Ok(())
}

fn replace_directory(target_dir: &Path, source_dir: &Path) -> Result<(), String> {
    if let Some(parent) = target_dir.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("Failed creating directory '{}': {err}", parent.display()))?;
    }

    if target_dir.exists() {
        fs::remove_dir_all(target_dir).map_err(|err| {
            format!(
                "Failed removing existing directory '{}': {err}",
                target_dir.display()
            )
        })?;
    }

    fs::rename(source_dir, target_dir).map_err(|err| {
        format!(
            "Failed replacing '{}' with '{}': {err}",
            target_dir.display(),
            source_dir.display()
        )
    })
}

pub fn download_gtfs_zip_to_dir(
    url: &str,
    target_dir: &str,
    feed_name: &str,
) -> Result<(), String> {
    let target_path = Path::new(target_dir);
    let temp_root = unique_temp_dir(target_path);

    if temp_root.exists() {
        fs::remove_dir_all(&temp_root).map_err(|err| {
            format!(
                "Failed cleaning old temp dir '{}': {err}",
                temp_root.display()
            )
        })?;
    }
    fs::create_dir_all(&temp_root)
        .map_err(|err| format!("Failed creating temp dir '{}': {err}", temp_root.display()))?;

    eprintln!("Downloading {feed_name} GTFS from {url} ...");
    let (mut bytes, mut content_type) = download_bytes(url)?;
    if !is_zip_payload(&bytes, url, content_type.as_deref()) {
        let html = String::from_utf8_lossy(&bytes);
        if let Some(zip_url) =
            extract_zip_link_from_html(&html).and_then(|href| resolve_link(url, &href))
        {
            eprintln!("Found ZIP link on page, following: {zip_url}");
            let fetched = download_bytes(&zip_url)?;
            bytes = fetched.0;
            content_type = fetched.1;
        }
    }

    if !is_zip_payload(&bytes, url, content_type.as_deref()) {
        return Err(format!(
            "URL '{url}' did not return a GTFS zip for {feed_name}, and no ZIP link was detected. Set a direct zip URL in config and try again."
        ));
    }

    eprintln!("Extracting {feed_name} GTFS archive ...");
    extract_zip_bytes(&bytes, &temp_root)?;

    let gtfs_root = find_gtfs_root(&temp_root).ok_or_else(|| {
        format!(
            "Downloaded archive for {feed_name} does not contain required GTFS files: {}",
            REQUIRED_GTFS_FILES.join(", ")
        )
    })?;

    eprintln!("Replacing local raw data for {feed_name} ...");
    replace_directory(target_path, &gtfs_root)?;

    if temp_root.exists() {
        let _ = fs::remove_dir_all(&temp_root);
    }

    Ok(())
}
