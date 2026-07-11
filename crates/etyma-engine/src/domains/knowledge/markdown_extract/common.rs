use std::collections::BTreeMap;

pub(crate) fn normalize_key(value: &str) -> String {
    let mut normalized = String::new();
    let mut last_dash = false;
    for char in value.chars() {
        if char.is_ascii_alphanumeric() {
            normalized.push(char.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            normalized.push('-');
            last_dash = true;
        }
    }
    normalized.trim_matches('-').to_string()
}

pub(crate) fn markdown_search_token_frequencies(text: &str) -> BTreeMap<String, usize> {
    let mut frequencies = BTreeMap::new();
    for token in text
        .split(|char: char| !char.is_ascii_alphanumeric())
        .filter_map(normalize_extract_search_token)
    {
        *frequencies.entry(token).or_insert(0) += 1;
    }
    frequencies
}

pub(crate) fn normalize_extract_search_token(raw: &str) -> Option<String> {
    let mut token = raw.trim().to_ascii_lowercase();
    if token.len() <= 1 {
        return None;
    }
    if token.ends_with("ies") && token.len() > 4 {
        token.truncate(token.len() - 3);
        token.push('y');
    } else if token.ends_with("ing") && token.len() > 5 {
        token.truncate(token.len() - 3);
    } else if (token.ends_with("ed") || token.ends_with("es") && !token.ends_with("ses"))
        && token.len() > 4
    {
        token.truncate(token.len() - 2);
    } else if token.ends_with('s')
        && token.len() > 4
        && !token.ends_with("ss")
        && !token.ends_with("us")
    {
        token.truncate(token.len() - 1);
    }
    (token.len() > 1).then_some(token)
}

pub(crate) fn excerpt(value: &str, max_length: usize) -> String {
    let compact = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.is_empty() {
        return "No visible evidence snippet is available yet.".into();
    }
    let compact_chars = compact.chars().count();
    if compact_chars <= max_length {
        return compact;
    }
    let truncated = compact
        .chars()
        .take(max_length.saturating_sub(1))
        .collect::<String>();
    format!("{}…", truncated.trim_end())
}
