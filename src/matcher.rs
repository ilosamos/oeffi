use std::collections::HashMap;

use strsim::jaro_winkler;

pub fn exact_key_case_insensitive<'a, T>(
    map: &'a HashMap<String, T>,
    query: &str,
) -> Option<(&'a String, &'a T)> {
    if let Some((k, v)) = map.get_key_value(query) {
        return Some((k, v));
    }
    map.iter().find(|(k, _)| k.eq_ignore_ascii_case(query))
}

pub fn fuzzy_best_key(
    query_upper: &str,
    keys: impl Iterator<Item = String>,
    threshold: f64,
) -> Option<String> {
    let mut best_key: Option<String> = None;
    let mut best_score = 0.0;
    for key in keys {
        let score = jaro_winkler(query_upper, &key);
        if score > best_score {
            best_score = score;
            best_key = Some(key);
        }
    }
    if best_score >= threshold {
        best_key
    } else {
        None
    }
}
