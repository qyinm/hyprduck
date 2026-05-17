use super::*;

pub(crate) fn extract_markdown_node_candidates(
    markdown: &str,
    source_path: &str,
) -> Vec<MarkdownNodeCandidate> {
    let mut candidates = Vec::<MarkdownNodeCandidate>::new();
    let mut seen = BTreeSet::<String>::new();
    let mut in_frontmatter = false;
    let mut frontmatter_closed = false;

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
                frontmatter_closed = true;
                continue;
            }
            if let Some(title) = frontmatter_title_candidate(trimmed) {
                push_markdown_node_candidate(
                    &mut candidates,
                    &mut seen,
                    MarkdownNodeCandidateInput {
                        label: title,
                        source_path,
                        line_start,
                        evidence: trimmed,
                        confidence: 0.92,
                        reason: "frontmatter title declares a stable node label",
                    },
                );
            }
            continue;
        }

        if !frontmatter_closed && trimmed == "---" {
            continue;
        }
        if let Some(heading) = markdown_heading_candidate(trimmed) {
            push_markdown_node_candidate(
                &mut candidates,
                &mut seen,
                MarkdownNodeCandidateInput {
                    label: heading,
                    source_path,
                    line_start,
                    evidence: trimmed,
                    confidence: 0.88,
                    reason: "markdown heading declares a stable node label",
                },
            );
            continue;
        }
        let cleaned = clean_candidate_line(trimmed);
        if let Some(label) = derive_concept_label(&cleaned) {
            push_markdown_node_candidate(
                &mut candidates,
                &mut seen,
                MarkdownNodeCandidateInput {
                    label,
                    source_path,
                    line_start,
                    evidence: trimmed,
                    confidence: 0.68,
                    reason: "markdown body line produced a stable candidate label",
                },
            );
        }
        if candidates.len() >= 24 {
            break;
        }
    }

    candidates
}

pub(crate) fn extract_markdown_node_candidates_for_workspace(
    markdown: &str,
    source_path: &str,
    workspace_root: &Path,
) -> Result<Vec<MarkdownNodeCandidate>> {
    let candidates = extract_markdown_node_candidates(markdown, source_path);
    let existing_nodes = read_existing_graph_nodes(workspace_root)?;
    Ok(match_markdown_node_candidates(candidates, &existing_nodes))
}

pub(crate) fn read_existing_graph_nodes(workspace_root: &Path) -> Result<Vec<BrainNodeRecord>> {
    read_optional_json_artifact(&workspace_root.join("graph/nodes.json"))
}

pub(crate) fn read_existing_graph_relations(
    workspace_root: &Path,
) -> Result<Vec<BrainRelationRecord>> {
    read_optional_json_artifact(&workspace_root.join("graph/edges.json"))
}

pub(crate) fn match_markdown_node_candidates(
    candidates: Vec<MarkdownNodeCandidate>,
    existing_nodes: &[BrainNodeRecord],
) -> Vec<MarkdownNodeCandidate> {
    candidates
        .into_iter()
        .map(|mut candidate| {
            if let Some(node_match) = best_existing_node_match(&candidate, existing_nodes) {
                candidate.matched_node_id = Some(node_match.node_id);
                candidate.matched_node_label = Some(node_match.label);
                candidate.match_score = Some(node_match.score);
                candidate.match_reason = Some(node_match.reason);
            }
            candidate
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ExistingNodeMatch {
    pub(crate) node_id: String,
    pub(crate) label: String,
    pub(crate) score: f32,
    pub(crate) reason: String,
}

pub(crate) fn best_existing_node_match(
    candidate: &MarkdownNodeCandidate,
    existing_nodes: &[BrainNodeRecord],
) -> Option<ExistingNodeMatch> {
    existing_nodes
        .iter()
        .filter(|node| matches!(node.kind, BrainNodeKind::Concept | BrainNodeKind::Topic))
        .filter_map(|node| score_existing_node_match(candidate, node))
        .max_by(|left, right| {
            left.score
                .partial_cmp(&right.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| right.node_id.cmp(&left.node_id))
        })
        .filter(|node_match| node_match.score >= 0.72)
}

pub(crate) fn score_existing_node_match(
    candidate: &MarkdownNodeCandidate,
    node: &BrainNodeRecord,
) -> Option<ExistingNodeMatch> {
    let candidate_key = normalize_key(&candidate.label);
    if candidate_key.is_empty() {
        return None;
    }

    let mut identity_labels = vec![node.label.as_str()];
    identity_labels.extend(node.aliases.iter().map(String::as_str));
    for label in &identity_labels {
        if normalize_key(label) == candidate_key {
            return Some(ExistingNodeMatch {
                node_id: node.node_id.clone(),
                label: node.label.clone(),
                score: 1.0,
                reason: "candidate label exactly matched an existing graph node label or alias"
                    .into(),
            });
        }
    }

    let candidate_terms = candidate_label_terms(&candidate.label);
    if candidate_terms.len() < 2 {
        return None;
    }
    let node_terms = identity_labels
        .iter()
        .flat_map(|label| candidate_label_terms(label))
        .collect::<BTreeSet<_>>();
    if node_terms.is_empty() {
        return None;
    }
    let intersection_count = candidate_terms.intersection(&node_terms).count();
    if intersection_count == 0 {
        return None;
    }
    let union_count = candidate_terms.union(&node_terms).count();
    let score = if candidate_terms.is_subset(&node_terms) || node_terms.is_subset(&candidate_terms)
    {
        0.86
    } else {
        intersection_count as f32 / union_count as f32
    };
    (score >= 0.72).then(|| ExistingNodeMatch {
        node_id: node.node_id.clone(),
        label: node.label.clone(),
        score,
        reason: "candidate label strongly overlapped an existing graph node label or alias".into(),
    })
}

pub(crate) fn candidate_label_terms(label: &str) -> BTreeSet<String> {
    label
        .split(|char: char| !char.is_ascii_alphanumeric())
        .filter_map(normalize_extract_search_token)
        .collect()
}

pub(crate) struct MarkdownNodeCandidateInput<'a> {
    pub(crate) label: String,
    pub(crate) source_path: &'a str,
    pub(crate) line_start: usize,
    pub(crate) evidence: &'a str,
    pub(crate) confidence: f32,
    pub(crate) reason: &'a str,
}

pub(crate) fn push_markdown_node_candidate(
    candidates: &mut Vec<MarkdownNodeCandidate>,
    seen: &mut BTreeSet<String>,
    input: MarkdownNodeCandidateInput<'_>,
) {
    let label = normalize_candidate_label(&input.label);
    let key = normalize_key(&label);
    if key.is_empty() || !seen.insert(key.clone()) {
        return;
    }
    candidates.push(MarkdownNodeCandidate {
        candidate_id: format!("candidate-{key}"),
        label,
        kind: BrainNodeKind::Concept,
        source_path: input.source_path.to_string(),
        line_start: input.line_start,
        evidence_snippet: excerpt(input.evidence, 180),
        confidence: input.confidence,
        reason: input.reason.into(),
        matched_node_id: None,
        matched_node_label: None,
        match_score: None,
        match_reason: None,
    });
}

pub(crate) fn frontmatter_title_candidate(line: &str) -> Option<String> {
    let value = line.strip_prefix("title:")?.trim();
    markdown_scalar_label(value)
}

pub(crate) fn markdown_heading_candidate(line: &str) -> Option<String> {
    if !line.starts_with('#') {
        return None;
    }
    let hash_count = line.chars().take_while(|char| *char == '#').count();
    if hash_count == 0 || hash_count > 4 {
        return None;
    }
    let value = line[hash_count..].trim();
    if value.to_ascii_lowercase().starts_with("page ") {
        return None;
    }
    markdown_scalar_label(value)
}

pub(crate) fn markdown_scalar_label(value: &str) -> Option<String> {
    let label = normalize_candidate_label(value.trim_matches(['"', '\'']));
    let word_count = label.split_whitespace().count();
    (label.len() >= 4 && word_count <= 10).then_some(label)
}

pub(crate) fn normalize_candidate_label(value: &str) -> String {
    value
        .trim()
        .trim_matches(|char: char| !char.is_alphanumeric())
        .replace(['`', '*'], "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn concept_candidates(content: &str) -> Vec<String> {
    let mut labels: Vec<String> = Vec::new();
    for line in content.lines() {
        let cleaned = clean_candidate_line(line);
        if cleaned.is_empty() {
            continue;
        }
        if let Some(label) = derive_concept_label(&cleaned) {
            if !labels
                .iter()
                .any(|existing| normalize_key(existing) == normalize_key(&label))
            {
                labels.push(label);
            }
        }
        if labels.len() >= 3 {
            break;
        }
    }
    labels
}

pub(crate) fn clean_candidate_line(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed.starts_with("![")
        || trimmed.starts_with("_AI analysis unavailable")
        || trimmed.starts_with("# ")
        || trimmed.starts_with("## Page ")
    {
        return String::new();
    }

    trimmed
        .trim_start_matches('#')
        .trim_start_matches('-')
        .trim_start_matches('*')
        .trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == ')')
        .trim()
        .replace(['`', '*'], "")
}

pub(crate) fn derive_concept_label(value: &str) -> Option<String> {
    let first_clause = value
        .split(['.', ':', ';', '(', ')', '[', ']'])
        .next()
        .unwrap_or(value)
        .trim();
    let mut words = first_clause
        .split_whitespace()
        .map(|word| {
            word.trim_matches(|char: char| !char.is_alphanumeric() && char != '-' && char != '/')
        })
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>();

    while matches!(words.first(), Some(word) if is_leading_stopword(word)) {
        words.remove(0);
    }

    if words.len() < 2 {
        return None;
    }

    let label = words.into_iter().take(6).collect::<Vec<_>>().join(" ");
    if label.len() < 10 {
        return None;
    }

    Some(label)
}

pub(crate) fn is_leading_stopword(value: &str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "a" | "an"
            | "and"
            | "as"
            | "for"
            | "from"
            | "in"
            | "into"
            | "of"
            | "on"
            | "or"
            | "the"
            | "this"
            | "that"
            | "to"
            | "with"
    )
}

pub(crate) fn fallback_concept_label(content: &str, page_label: &str) -> String {
    derive_concept_label(content).unwrap_or_else(|| format!("{page_label} summary"))
}
