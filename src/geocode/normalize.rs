pub fn strip_house_number_unit(raw: &str) -> String {
    let head = raw
        .split([';', ','])
        .next()
        .unwrap_or("")
        .trim();
    if head.is_empty() {
        return String::new();
    }

    let no_space_suffix = head.split_whitespace().next().unwrap_or("");
    let no_slash_suffix = no_space_suffix.split('/').next().unwrap_or("").trim();
    no_slash_suffix.to_string()
}

pub fn normalize_ascii(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut last_was_space = true;

    for ch in raw.chars() {
        let lower = ch.to_ascii_lowercase();
        match lower {
            'a'..='z' | '0'..='9' => {
                out.push(lower);
                last_was_space = false;
            }
            'ä' => {
                out.push('a');
                out.push('e');
                last_was_space = false;
            }
            'ö' => {
                out.push('o');
                out.push('e');
                last_was_space = false;
            }
            'ü' => {
                out.push('u');
                out.push('e');
                last_was_space = false;
            }
            'ß' => {
                out.push('s');
                out.push('s');
                last_was_space = false;
            }
            _ => {
                if !last_was_space {
                    out.push(' ');
                    last_was_space = true;
                }
            }
        }
    }

    out.trim().to_string()
}

pub fn canonical_street(raw: &str) -> String {
    let norm = normalize_ascii(raw);
    let mut out = Vec::new();

    for token in norm.split_whitespace() {
        let canonical = if token == "str" || token == "str." || token == "strasse" {
            "strasse".to_string()
        } else if token.ends_with("str") && token.len() > 3 {
            format!("{}strasse", &token[..token.len() - 3])
        } else {
            token.to_string()
        };
        out.push(canonical);
    }

    out.join(" ")
}

pub fn normalized_address_key(street: &str, house_number: &str, postcode: Option<&str>) -> String {
    let mut key = canonical_street(street);
    let number = normalize_ascii(house_number);
    if !number.is_empty() {
        if !key.is_empty() {
            key.push(' ');
        }
        key.push_str(&number);
    }

    if let Some(postcode) = postcode {
        let postcode_norm = normalize_ascii(postcode);
        if !postcode_norm.is_empty() {
            key.push(' ');
            key.push_str(&postcode_norm);
        }
    }

    key.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_apartment_suffixes() {
        assert_eq!(strip_house_number_unit("12/3"), "12");
        assert_eq!(strip_house_number_unit("12A Stiege 2"), "12A");
    }

    #[test]
    fn normalizes_umlauts_and_spacing() {
        assert_eq!(normalize_ascii("Währinger Straße"), "waehringer strasse");
        assert_eq!(normalize_ascii("  Wien, 1090  "), "wien 1090");
    }

    #[test]
    fn canonicalizes_street_variants() {
        assert_eq!(
            canonical_street("Prinz-Eugen-Straße"),
            "prinz eugen strasse"
        );
        assert_eq!(canonical_street("Prinz Eugen Str."), "prinz eugen strasse");
        assert_eq!(canonical_street("Lassallestr"), "lassallestrasse");
    }
}
