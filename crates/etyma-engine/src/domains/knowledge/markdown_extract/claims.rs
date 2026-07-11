use super::*;

pub(crate) fn extract_markdown_claim_candidates(
    markdown: &str,
    source_path: &str,
    source_id: Option<&str>,
    node_candidates: &[MarkdownNodeCandidate],
) -> Vec<MarkdownClaimCandidate> {
    let mut candidates = Vec::new();
    let mut seen = BTreeSet::<String>::new();
    let mut in_frontmatter = false;

    for (line_index, raw_line) in markdown.lines().enumerate() {
        let line_start = line_index + 1;
        let trimmed = raw_line.trim();
        if line_index == 0 && trimmed == "---" {
            in_frontmatter = true;
            continue;
        }
        if in_frontmatter {
            if trimmed == "---" {
                in_frontmatter = false;
            }
            continue;
        }
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("![") {
            continue;
        }

        let Some(statement) = normalize_claim_statement(trimmed) else {
            continue;
        };
        let claim_key = bounded_artifact_key(&statement, 80);
        let evidence_scope_key = source_id
            .map(|source_id| bounded_artifact_key(source_id, 48))
            .unwrap_or_else(|| bounded_artifact_key(source_path, 48));
        if claim_key.is_empty() || !seen.insert(claim_key.clone()) {
            continue;
        }
        let char_start = raw_line
            .find(statement.as_str())
            .or_else(|| raw_line.find(trimmed))
            .unwrap_or(0);
        let char_end = char_start + statement.len();
        let evidence_snippet = excerpt(&statement, 220);
        let mentions = relationship_mentions_in_line(&statement, node_candidates);
        let subject_labels = mentions
            .iter()
            .take(4)
            .map(|mention| mention.label.clone())
            .collect::<Vec<_>>();
        let mut subject_refs = mentions
            .iter()
            .take(4)
            .map(|mention| mention.resolved_node_id.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        if subject_refs.is_empty() {
            if let Some(label) = derive_concept_label(&statement) {
                subject_refs.push(format!("concept-{}", normalize_key(&label)));
            }
        }

        let confidence = claim_candidate_confidence(&statement, !subject_refs.is_empty());
        let classification = classify_markdown_claim_statement(&statement);
        candidates.push(MarkdownClaimCandidate {
            candidate_id: format!("claim-candidate-{evidence_scope_key}-{line_start}-{claim_key}"),
            evidence_id: format!("ev-claim-{evidence_scope_key}-{line_start}-{claim_key}"),
            statement: statement.clone(),
            classification,
            durable: true,
            memory_candidate: markdown_claim_should_be_memory_candidate(&statement, classification),
            source_path: source_path.to_string(),
            source_id: source_id.map(ToString::to_string),
            source_refs: source_id
                .map(|source_id| vec![source_id.to_string()])
                .unwrap_or_default(),
            line_start,
            line_end: line_start,
            char_start,
            char_end,
            evidence_span: MarkdownEvidenceSpan {
                source_path: source_path.to_string(),
                source_id: source_id.map(ToString::to_string),
                line_start,
                line_end: line_start,
                char_start,
                char_end,
                snippet: evidence_snippet.clone(),
            },
            evidence_snippet,
            subject_labels,
            subject_refs,
            confidence,
            reason: claim_candidate_reason(&statement),
        });
        if candidates.len() >= 32 {
            break;
        }
    }

    candidates
}

pub(crate) fn classify_markdown_claim_statement(statement: &str) -> MarkdownClaimClassification {
    let lower = format!(" {} ", statement.to_ascii_lowercase());
    if [
        " decision ",
        " decided ",
        " chose ",
        " chosen ",
        " approved ",
        " accepted ",
        " source of truth ",
        " must ",
        " should ",
        " will ",
        " no human approval ",
        " records approved ",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
    {
        MarkdownClaimClassification::Decision
    } else {
        MarkdownClaimClassification::DurableFact
    }
}

pub(crate) fn markdown_claim_should_be_memory_candidate(
    statement: &str,
    classification: MarkdownClaimClassification,
) -> bool {
    if classification == MarkdownClaimClassification::Decision {
        return true;
    }
    let lower = statement.to_ascii_lowercase();
    [
        "remember",
        "retain",
        "persistent",
        "durable memory",
        "agent memory",
        "memory candidate",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

pub(crate) fn normalize_claim_statement(line: &str) -> Option<String> {
    let statement = clean_candidate_line(line)
        .trim_start_matches('>')
        .trim_start_matches(['-', '*'])
        .trim()
        .trim_end_matches(';')
        .trim()
        .to_string();
    if statement.len() < 18 || statement.split_whitespace().count() < 4 {
        return None;
    }
    if !line_looks_like_claim(&statement) {
        return None;
    }
    Some(statement)
}

pub(crate) fn line_looks_like_claim(statement: &str) -> bool {
    let lower = format!(" {} ", statement.to_ascii_lowercase());
    [
        " is ",
        " are ",
        " was ",
        " were ",
        " has ",
        " have ",
        " had ",
        " can ",
        " should ",
        " must ",
        " will ",
        " remains ",
        " keeps ",
        " records ",
        " stores ",
        " supports ",
        " depends on ",
        " relies on ",
        " requires ",
        " enables ",
        " blocks ",
        " contradicts ",
        " supersedes ",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

pub(crate) fn claim_candidate_confidence(statement: &str, has_subject: bool) -> f32 {
    let lower = statement.to_ascii_lowercase();
    let explicit = lower.contains(" is ")
        || lower.contains(" are ")
        || lower.contains(" must ")
        || lower.contains(" should ")
        || lower.contains(" depends on ")
        || lower.contains(" supports ");
    match (explicit, has_subject) {
        (true, true) => 0.84,
        (true, false) => 0.76,
        (false, true) => 0.72,
        (false, false) => 0.64,
    }
}

pub(crate) fn claim_candidate_reason(statement: &str) -> String {
    match classify_markdown_claim_statement(statement) {
        MarkdownClaimClassification::Decision => {
            return "the line states a durable decision or operating rule with source evidence"
                .into();
        }
        MarkdownClaimClassification::DurableFact => {}
    }
    if infer_markdown_relation_kind(statement).is_some() {
        return "the line states an explicit relation that can be audited as a claim".into();
    }
    "the line contains a factual modal or copular assertion with source evidence".into()
}

pub(crate) fn bounded_artifact_key(value: &str, max_chars: usize) -> String {
    normalize_key(value)
        .chars()
        .take(max_chars)
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}
