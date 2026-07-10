//! Query term extraction and search-text normalization (including Hangul jamo composition).

pub(crate) fn db_search_terms(query: &str) -> Vec<String> {
    let mut terms = Vec::new();
    for term in query
        .split(|ch: char| !ch.is_alphanumeric())
        .map(|term| normalize_search_text(term.trim()))
    {
        for candidate in query_term_candidates(&term) {
            if !candidate.is_empty()
                && !is_query_stopword(&candidate)
                && !terms.contains(&candidate)
            {
                terms.push(candidate);
            }
        }
    }
    terms
}

fn query_term_candidates(term: &str) -> Vec<String> {
    if term.is_empty() || is_query_stopword(term) {
        return Vec::new();
    }
    vec![term.into()]
}

fn is_query_stopword(term: &str) -> bool {
    matches!(
        term,
        "a" | "an"
            | "the"
            | "about"
            | "what"
            | "is"
            | "are"
            | "tell"
            | "me"
            | "please"
            | "내용"
            | "설명"
            | "정리"
            | "요약"
            | "무엇"
            | "뭐야"
            | "어떤"
            | "있어"
            | "있나요"
            | "없어"
            | "없나요"
    )
}

pub(crate) fn normalize_search_text(text: &str) -> String {
    compose_hangul_jamo(text).to_lowercase()
}

fn compose_hangul_jamo(text: &str) -> String {
    const S_BASE: u32 = 0xAC00;
    const L_BASE: u32 = 0x1100;
    const V_BASE: u32 = 0x1161;
    const T_BASE: u32 = 0x11A7;
    const L_COUNT: u32 = 19;
    const V_COUNT: u32 = 21;
    const T_COUNT: u32 = 28;
    const N_COUNT: u32 = V_COUNT * T_COUNT;

    fn l_index(ch: char) -> Option<u32> {
        let value = ch as u32;
        (L_BASE..L_BASE + L_COUNT)
            .contains(&value)
            .then(|| value - L_BASE)
    }
    fn v_index(ch: char) -> Option<u32> {
        let value = ch as u32;
        (V_BASE..V_BASE + V_COUNT)
            .contains(&value)
            .then(|| value - V_BASE)
    }
    fn t_index(ch: char) -> Option<u32> {
        let value = ch as u32;
        (T_BASE + 1..T_BASE + T_COUNT)
            .contains(&value)
            .then(|| value - T_BASE)
    }

    let chars = text.chars().collect::<Vec<_>>();
    let mut output = String::with_capacity(text.len());
    let mut index = 0;
    while index < chars.len() {
        if let (Some(l), Some(v)) = (
            l_index(chars[index]),
            chars.get(index + 1).and_then(|ch| v_index(*ch)),
        ) {
            let mut consumed = 2;
            let t = chars
                .get(index + 2)
                .and_then(|ch| t_index(*ch))
                .inspect(|_| consumed = 3)
                .unwrap_or(0);
            let syllable = S_BASE + (l * N_COUNT) + (v * T_COUNT) + t;
            if let Some(ch) = char::from_u32(syllable) {
                output.push(ch);
                index += consumed;
                continue;
            }
        }
        output.push(chars[index]);
        index += 1;
    }
    output
}

#[allow(dead_code)]
pub(crate) fn fts_phrase_query(query: &str) -> String {
    db_search_terms(query)
        .into_iter()
        .map(|term| format!("\"{}\"", term.replace('\"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" ")
}
