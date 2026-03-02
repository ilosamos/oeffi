use std::collections::HashMap;

use strsim::jaro_winkler;

pub const GENERIC_QUERY_TOKENS: [&str; 1] = ["WIEN"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameMatchMode {
    Exact,
    Fuzzy,
    Relaxed,
    None,
}

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

pub fn normalize_for_match(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            'ä' | 'Ä' => out.push_str("AE"),
            'ö' | 'Ö' => out.push_str("OE"),
            'ü' | 'Ü' => out.push_str("UE"),
            'ß' => out.push_str("SS"),
            _ => {
                if ch.is_alphanumeric() {
                    out.extend(ch.to_uppercase());
                } else {
                    out.push(' ');
                }
            }
        }
    }

    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn significant_query_tokens<'a>(
    normalized_query: &'a str,
    generic_tokens: &[&str],
) -> Vec<&'a str> {
    normalized_query
        .split_whitespace()
        .filter(|token| !generic_tokens.contains(token))
        .collect()
}

fn contains_token_match(normalized_name: &str, query_tokens: &[&str]) -> bool {
    if query_tokens.is_empty() || query_tokens.iter().all(|token| token.len() < 2) {
        return false;
    }

    query_tokens
        .iter()
        .filter(|token| token.len() >= 2)
        .all(|token| normalized_name.contains(token))
}

pub fn relaxed_name_matches<T: Clone + Ord>(
    name_idxs_by_name_upper: &HashMap<String, Vec<T>>,
    query: &str,
    generic_tokens: &[&str],
    max_results: usize,
) -> Vec<T> {
    let normalized_query = normalize_for_match(query);
    if normalized_query.is_empty() {
        return Vec::new();
    }

    let raw_tokens: Vec<&str> = normalized_query.split_whitespace().collect();
    if raw_tokens
        .iter()
        .all(|token| generic_tokens.contains(token))
    {
        return Vec::new();
    }

    let significant_tokens = significant_query_tokens(&normalized_query, generic_tokens);
    let significant_phrase = significant_tokens.join(" ");

    let mut scored: Vec<(u16, String, T)> = Vec::new();
    for (name_upper, idxs) in name_idxs_by_name_upper {
        let normalized_name = normalize_for_match(name_upper);
        let score = if normalized_name == normalized_query {
            350
        } else if normalized_name.starts_with(&normalized_query) {
            320
        } else if normalized_name.contains(&normalized_query) {
            260
        } else if !significant_phrase.is_empty()
            && significant_phrase != normalized_query
            && normalized_name.starts_with(&significant_phrase)
        {
            240
        } else if !significant_phrase.is_empty()
            && significant_phrase != normalized_query
            && normalized_name.contains(&significant_phrase)
        {
            190
        } else if contains_token_match(&normalized_name, &significant_tokens) {
            150
        } else {
            0
        };

        if score == 0 {
            continue;
        }

        for idx in idxs {
            scored.push((score, name_upper.clone(), idx.clone()));
        }
    }

    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    let best_score = scored.first().map(|(score, _, _)| *score).unwrap_or(0);
    if best_score == 0 {
        return Vec::new();
    }

    // Keep only the strongest score bucket to avoid noisy results for terms like "wien mitte".
    let mut out: Vec<T> = scored
        .into_iter()
        .filter(|(score, _, _)| *score == best_score)
        .map(|(_, _, idx)| idx)
        .collect();
    out.dedup();
    out.truncate(max_results);
    out
}

pub fn match_name_candidates<T: Clone + Ord>(
    name_idxs_by_name_upper: &HashMap<String, Vec<T>>,
    query: &str,
    fuzzy_threshold: f64,
    generic_tokens: &[&str],
    max_relaxed_results: usize,
) -> (Vec<T>, NameMatchMode) {
    let query_upper = query.to_ascii_uppercase();
    if let Some(v) = name_idxs_by_name_upper.get(&query_upper) {
        return (v.clone(), NameMatchMode::Exact);
    }

    if let Some(name) = fuzzy_best_key(
        &query_upper,
        name_idxs_by_name_upper.keys().cloned(),
        fuzzy_threshold,
    ) {
        let candidates = name_idxs_by_name_upper
            .get(&name)
            .cloned()
            .unwrap_or_default();
        if !candidates.is_empty() {
            return (candidates, NameMatchMode::Fuzzy);
        }
    }

    let relaxed = relaxed_name_matches(
        name_idxs_by_name_upper,
        query,
        generic_tokens,
        max_relaxed_results,
    );
    if !relaxed.is_empty() {
        return (relaxed, NameMatchMode::Relaxed);
    }

    (Vec::new(), NameMatchMode::None)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{
        GENERIC_QUERY_TOKENS, NameMatchMode, match_name_candidates, normalize_for_match,
        relaxed_name_matches,
    };

    #[test]
    fn normalize_for_match_handles_punctuation_and_german_letters() {
        assert_eq!(
            normalize_for_match("Wien-Mitte-Landstraße"),
            "WIEN MITTE LANDSTRASSE"
        );
    }

    #[test]
    fn relaxed_name_matching_supports_partial_multi_word_queries() {
        let name_index = HashMap::from([
            ("FLUGHAFEN WIEN BAHNHOF".to_string(), vec![1_u32]),
            ("WIEN MITTE-LANDSTRASSE".to_string(), vec![2_u32]),
        ]);

        let flughafen =
            relaxed_name_matches(&name_index, "flughafen-bahnhof", &GENERIC_QUERY_TOKENS, 10);
        assert_eq!(flughafen, vec![1_u32]);

        let mitte = relaxed_name_matches(&name_index, "wien mitte", &GENERIC_QUERY_TOKENS, 10);
        assert_eq!(mitte, vec![2_u32]);

        let generic = relaxed_name_matches(&name_index, "wien", &GENERIC_QUERY_TOKENS, 10);
        assert!(generic.is_empty());
    }

    #[test]
    fn match_name_candidates_reports_mode() {
        let name_index = HashMap::from([("WIEN MITTE-LANDSTRASSE".to_string(), vec![2_u32])]);
        let (matches, mode) =
            match_name_candidates(&name_index, "landstrasse", 0.94, &GENERIC_QUERY_TOKENS, 10);
        assert_eq!(matches, vec![2_u32]);
        assert_eq!(mode, NameMatchMode::Relaxed);
    }
}
