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
                    title,
                    source_path,
                    line_start,
                    trimmed,
                    0.92,
                    "frontmatter title declares a stable node label",
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
                heading,
                source_path,
                line_start,
                trimmed,
                0.88,
                "markdown heading declares a stable node label",
            );
            continue;
        }
        let cleaned = clean_candidate_line(trimmed);
        if let Some(label) = derive_concept_label(&cleaned) {
            push_markdown_node_candidate(
                &mut candidates,
                &mut seen,
                label,
                source_path,
                line_start,
                trimmed,
                0.68,
                "markdown body line produced a stable candidate label",
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

pub(crate) fn extract_markdown_relationship_evidence(
    markdown: &str,
    source_path: &str,
    source_id: Option<&str>,
    node_candidates: &[MarkdownNodeCandidate],
) -> Vec<MarkdownRelationshipEvidence> {
    let mut evidence = Vec::new();
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
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let relation_kind = infer_markdown_relation_kind(trimmed);
        if relation_kind.is_none() && !line_has_explicit_link_signal(trimmed) {
            continue;
        }

        let mentions = relationship_mentions_in_line(trimmed, node_candidates);
        if mentions.len() < 2 {
            continue;
        }

        for left_index in 0..mentions.len() {
            for right_index in (left_index + 1)..mentions.len() {
                let left = &mentions[left_index];
                let right = &mentions[right_index];
                let key = format!(
                    "{}:{}:{}",
                    line_start,
                    normalize_key(&left.label),
                    normalize_key(&right.label)
                );
                if !seen.insert(key.clone()) {
                    continue;
                }
                let relation_kind = relation_kind.unwrap_or(BrainRelationKind::RelatedTo);
                let candidate_id = format!("edge-candidate-{key}");
                evidence.push(MarkdownRelationshipEvidence {
                    candidate_id,
                    evidence_id: format!("ev-relation-{key}"),
                    source_path: source_path.to_string(),
                    source_id: source_id.map(ToString::to_string),
                    source_refs: source_id
                        .map(|source_id| vec![source_id.to_string()])
                        .unwrap_or_default(),
                    line_start,
                    snippet: excerpt(trimmed, 220),
                    source_label: left.label.clone(),
                    target_label: right.label.clone(),
                    relation_kind,
                    relation_label: markdown_relation_label(relation_kind),
                    confidence: if relation_kind == BrainRelationKind::RelatedTo {
                        0.74
                    } else {
                        0.82
                    },
                    reason: relationship_reason(trimmed, Some(relation_kind)),
                    matched_source_node_id: left.matched_node_id.clone(),
                    matched_target_node_id: right.matched_node_id.clone(),
                    resolved_source_node_id: Some(left.resolved_node_id.clone()),
                    resolved_target_node_id: Some(right.resolved_node_id.clone()),
                    endpoint_resolution: format!(
                        "{} -> {}; {} -> {}",
                        left.label,
                        left.endpoint_resolution,
                        right.label,
                        right.endpoint_resolution
                    ),
                });
                if evidence.len() >= 32 {
                    return evidence;
                }
            }
        }
    }

    evidence
}

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

pub(crate) fn extract_markdown_signals(
    markdown: &str,
    source_path: &str,
    source_id: Option<&str>,
    node_candidates: &[MarkdownNodeCandidate],
) -> MarkdownSignalArtifact {
    let mut title = None;
    let mut headings = Vec::<MarkdownHeadingSignal>::new();
    let mut links = Vec::<MarkdownLinkSignal>::new();
    let mut keyword_counts = BTreeMap::<String, usize>::new();
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
                continue;
            }
            if title.is_none() {
                title = frontmatter_title_candidate(trimmed);
            }
            continue;
        }
        if let Some(heading) = markdown_heading_signal(trimmed, line_start) {
            if title.is_none() && heading.level == 1 {
                title = Some(heading.text.clone());
            }
            headings.push(heading);
        }
        links.extend(markdown_link_signals(trimmed, line_start));
        for term in markdown_signal_terms(trimmed) {
            *keyword_counts.entry(term).or_default() += 1;
        }
    }

    let mut keywords = keyword_counts
        .into_iter()
        .filter(|(_, count)| *count >= 2)
        .map(|(term, count)| MarkdownKeywordSignal { term, count })
        .collect::<Vec<_>>();
    keywords.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.term.cmp(&right.term))
    });
    keywords.truncate(16);

    MarkdownSignalArtifact {
        source_path: source_path.to_string(),
        source_id: source_id.map(ToString::to_string),
        source_refs: source_id
            .map(|source_id| vec![source_id.to_string()])
            .unwrap_or_default(),
        title,
        headings,
        links,
        entities: node_candidates
            .iter()
            .map(|candidate| MarkdownEntitySignal {
                label: candidate.label.clone(),
                line_start: candidate.line_start,
                confidence: candidate.confidence,
                reason: candidate.reason.clone(),
                matched_node_id: candidate.matched_node_id.clone(),
                matched_node_label: candidate.matched_node_label.clone(),
                match_score: candidate.match_score,
            })
            .collect(),
        keywords,
        related_pages: Vec::new(),
    }
}

pub(crate) fn rank_related_wiki_pages_for_signals(
    workspace_root: &Path,
    workspace_id: &str,
    signals: &MarkdownSignalArtifact,
) -> Result<Vec<MarkdownRelatedPageSignal>> {
    if !workspace_root.join("brain-manifest.json").exists() {
        return Ok(Vec::new());
    }

    let snapshot = read_materialized_brain_snapshot(workspace_root, workspace_id)?;
    let weighted_terms = weighted_markdown_signal_terms(signals);
    if weighted_terms.is_empty() {
        return Ok(Vec::new());
    }

    let mut related_pages = snapshot
        .wiki_pages
        .iter()
        .filter_map(|page| {
            let metadata_text = wiki_page_metadata_text(page);
            let body = fs::read_to_string(workspace_root.join(&page.path))
                .unwrap_or_else(|_| materialized_wiki_page_body(page, &snapshot));
            let metadata_frequencies = search_token_frequencies(&metadata_text);
            let content_frequencies = search_token_frequencies(&body);
            let mut metadata_score = 0usize;
            let mut content_score = 0usize;
            let mut matched_terms = Vec::<String>::new();

            for (term, weight) in &weighted_terms {
                let metadata_count = metadata_frequencies.get(term).copied().unwrap_or(0);
                let content_count = content_frequencies.get(term).copied().unwrap_or(0);
                if metadata_count == 0 && content_count == 0 {
                    continue;
                }
                metadata_score += metadata_count.saturating_mul(*weight).saturating_mul(8);
                content_score += content_count.saturating_mul(*weight).saturating_mul(2);
                matched_terms.push(term.clone());
            }

            let score = metadata_score + content_score + matched_terms.len().saturating_mul(3);
            if score == 0 {
                return None;
            }
            let reason = match (metadata_score > 0, content_score > 0) {
                (true, true) => {
                    "ranked by overlap with existing wiki page metadata and content".into()
                }
                (true, false) => "ranked by overlap with existing wiki page metadata".into(),
                (false, true) => "ranked by overlap with existing wiki page content".into(),
                (false, false) => "ranked by signal overlap".into(),
            };
            Some(MarkdownRelatedPageSignal {
                page_id: page.page_id.clone(),
                path: page.path.clone(),
                title: page.title.clone(),
                score,
                matched_terms,
                reason,
            })
        })
        .collect::<Vec<_>>();

    related_pages.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.path.cmp(&right.path))
    });
    related_pages.truncate(8);
    Ok(related_pages)
}

pub(crate) fn weighted_markdown_signal_terms(
    signals: &MarkdownSignalArtifact,
) -> BTreeMap<String, usize> {
    let mut terms = BTreeMap::<String, usize>::new();
    if let Some(title) = &signals.title {
        add_weighted_terms(&mut terms, title, 8);
    }
    for heading in &signals.headings {
        let weight = if heading.level == 1 { 7 } else { 5 };
        add_weighted_terms(&mut terms, &heading.text, weight);
    }
    for entity in &signals.entities {
        add_weighted_terms(&mut terms, &entity.label, 6);
        if let Some(label) = &entity.matched_node_label {
            add_weighted_terms(&mut terms, label, 6);
        }
    }
    for link in &signals.links {
        add_weighted_terms(&mut terms, &link.label, 4);
        add_weighted_terms(&mut terms, &link.target, 3);
    }
    for keyword in &signals.keywords {
        *terms.entry(keyword.term.clone()).or_default() += keyword.count.min(4);
    }
    terms
}

pub(crate) fn add_weighted_terms(terms: &mut BTreeMap<String, usize>, text: &str, weight: usize) {
    for term in markdown_signal_terms(text) {
        *terms.entry(term).or_default() += weight;
    }
}

pub(crate) fn wiki_page_metadata_text(page: &WikiPage) -> String {
    [
        page.page_id.as_str(),
        page.path.as_str(),
        page.title.as_str(),
        &page.node_refs.join(" "),
        &page.source_refs.join(" "),
        &page.evidence_refs.join(" "),
    ]
    .join(" ")
}

pub(crate) fn markdown_heading_signal(
    line: &str,
    line_start: usize,
) -> Option<MarkdownHeadingSignal> {
    if !line.starts_with('#') {
        return None;
    }
    let level = line.chars().take_while(|char| *char == '#').count();
    if level == 0 || level > 6 {
        return None;
    }
    let text = markdown_scalar_label(line[level..].trim())?;
    Some(MarkdownHeadingSignal {
        text,
        level,
        line_start,
    })
}

pub(crate) fn markdown_link_signals(line: &str, line_start: usize) -> Vec<MarkdownLinkSignal> {
    let mut links = Vec::new();
    let mut rest = line;
    while let Some(start) = rest.find("[[") {
        let after_start = &rest[start + 2..];
        let Some(end) = after_start.find("]]") else {
            break;
        };
        let target = after_start[..end].trim();
        if !target.is_empty() {
            let label = target
                .split('|')
                .next_back()
                .unwrap_or(target)
                .trim()
                .to_string();
            links.push(MarkdownLinkSignal {
                label,
                target: target.to_string(),
                kind: "wiki".into(),
                line_start,
            });
        }
        rest = &after_start[end + 2..];
    }

    let mut rest = line;
    while let Some(label_start) = rest.find('[') {
        if rest[..label_start].ends_with('!') {
            rest = &rest[label_start + 1..];
            continue;
        }
        if rest[label_start..].starts_with("[[") {
            let after_wiki_start = &rest[label_start + 2..];
            rest = match after_wiki_start.find("]]") {
                Some(wiki_end) => &after_wiki_start[wiki_end + 2..],
                None => &rest[label_start + 2..],
            };
            continue;
        }
        let after_label_start = &rest[label_start + 1..];
        let Some(label_end) = after_label_start.find("](") else {
            break;
        };
        let label = after_label_start[..label_end].trim();
        let after_target_start = &after_label_start[label_end + 2..];
        let Some(target_end) = after_target_start.find(')') else {
            break;
        };
        let target = after_target_start[..target_end].trim();
        if !label.is_empty() && !target.is_empty() {
            links.push(MarkdownLinkSignal {
                label: label.to_string(),
                target: target.to_string(),
                kind: "markdown".into(),
                line_start,
            });
        }
        rest = &after_target_start[target_end + 1..];
    }

    links
}

pub(crate) fn markdown_signal_terms(line: &str) -> Vec<String> {
    let line = strip_inline_markdown_targets(line);
    search_terms(&line)
        .into_iter()
        .filter(|term| !is_markdown_signal_stopword(term))
        .collect()
}

pub(crate) fn strip_inline_markdown_targets(line: &str) -> String {
    line.replace("[[", " ")
        .replace("]]", " ")
        .replace("](", " ")
        .replace(['[', ']', '(', ')', '#', '`', '*'], " ")
}

pub(crate) fn is_markdown_signal_stopword(term: &str) -> bool {
    matches!(
        term,
        "and"
            | "are"
            | "but"
            | "for"
            | "from"
            | "into"
            | "the"
            | "this"
            | "that"
            | "with"
            | "without"
            | "source"
            | "evidence"
            | "remain"
            | "keep"
            | "keeps"
            | "durable"
    )
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
        .trim_start_matches(|char: char| char == '-' || char == '*')
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

#[derive(Debug, Clone)]
pub(crate) struct RelationshipMention {
    pub(crate) label: String,
    pub(crate) position: usize,
    pub(crate) matched_node_id: Option<String>,
    pub(crate) resolved_node_id: String,
    pub(crate) endpoint_resolution: String,
}

pub(crate) fn relationship_mentions_in_line(
    line: &str,
    node_candidates: &[MarkdownNodeCandidate],
) -> Vec<RelationshipMention> {
    let lower_line = line.to_ascii_lowercase();
    let mut mentions = Vec::new();
    let mut seen = BTreeSet::<String>::new();
    for candidate in node_candidates {
        let labels = candidate
            .matched_node_label
            .iter()
            .chain(std::iter::once(&candidate.label));
        for label in labels {
            let needle = label.to_ascii_lowercase();
            if needle.len() < 4 {
                continue;
            }
            let Some(position) = lower_line.find(&needle) else {
                continue;
            };
            let key = candidate
                .matched_node_id
                .clone()
                .unwrap_or_else(|| normalize_key(label));
            if !seen.insert(key) {
                continue;
            }
            let resolved_node_id = candidate
                .matched_node_id
                .clone()
                .unwrap_or_else(|| format!("concept-{}", normalize_key(&candidate.label)));
            let endpoint_resolution = if candidate.matched_node_id.is_some() {
                "existing_node".into()
            } else {
                "proposed_node".into()
            };
            mentions.push(RelationshipMention {
                label: label.clone(),
                position,
                matched_node_id: candidate.matched_node_id.clone(),
                resolved_node_id,
                endpoint_resolution,
            });
        }
    }
    mentions.sort_by(|left, right| {
        left.position
            .cmp(&right.position)
            .then_with(|| left.label.cmp(&right.label))
    });
    mentions
}

pub(crate) fn infer_markdown_relation_kind(line: &str) -> Option<BrainRelationKind> {
    let lower = line.to_ascii_lowercase();
    if lower.contains(" depends on ")
        || lower.contains(" relies on ")
        || lower.contains(" requires ")
        || lower.contains(" blocked by ")
    {
        return Some(BrainRelationKind::DependsOn);
    }
    if lower.contains(" supports ")
        || lower.contains(" enables ")
        || lower.contains(" grounds ")
        || lower.contains(" backs ")
        || lower.contains(" cites ")
    {
        return Some(BrainRelationKind::Supports);
    }
    if lower.contains(" contradicts ") || lower.contains(" conflicts with ") {
        return Some(BrainRelationKind::Contradicts);
    }
    if lower.contains(" supersedes ") || lower.contains(" replaces ") {
        return Some(BrainRelationKind::Supersedes);
    }
    if lower.contains(" same as ") || lower.contains(" alias of ") {
        return Some(BrainRelationKind::SameAs);
    }
    if line.contains("->") || line.contains("<->") || lower.contains(" links ") {
        return Some(BrainRelationKind::RelatedTo);
    }
    None
}

pub(crate) fn markdown_relation_label(kind: BrainRelationKind) -> String {
    match kind {
        BrainRelationKind::Supports => "Supports".into(),
        BrainRelationKind::Contradicts => "Contradicts".into(),
        BrainRelationKind::Supersedes => "Supersedes".into(),
        BrainRelationKind::SameAs => "Same as".into(),
        BrainRelationKind::DependsOn => "Depends on".into(),
        _ => "Related in source".into(),
    }
}

pub(crate) fn line_has_explicit_link_signal(line: &str) -> bool {
    line.contains("[[") || line.contains("](") || line.contains("->") || line.contains("<->")
}

pub(crate) fn relationship_reason(line: &str, relation_kind: Option<BrainRelationKind>) -> String {
    if let Some(kind) = relation_kind {
        return format!("the line contains an explicit {:?} relationship cue", kind);
    }
    if line.contains("[[") {
        return "the line contains wiki-link syntax connecting mentioned nodes".into();
    }
    if line.contains("](") {
        return "the line contains markdown-link syntax connecting mentioned nodes".into();
    }
    "the line contains an explicit link signal connecting mentioned nodes".into()
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
        .filter_map(normalize_search_token)
        .collect()
}

pub(crate) fn push_markdown_node_candidate(
    candidates: &mut Vec<MarkdownNodeCandidate>,
    seen: &mut BTreeSet<String>,
    label: String,
    source_path: &str,
    line_start: usize,
    evidence: &str,
    confidence: f32,
    reason: &str,
) {
    let label = normalize_candidate_label(&label);
    let key = normalize_key(&label);
    if key.is_empty() || !seen.insert(key.clone()) {
        return;
    }
    candidates.push(MarkdownNodeCandidate {
        candidate_id: format!("candidate-{key}"),
        label,
        kind: BrainNodeKind::Concept,
        source_path: source_path.to_string(),
        line_start,
        evidence_snippet: excerpt(evidence, 180),
        confidence,
        reason: reason.into(),
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
        .replace('`', "")
        .replace('*', "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn page_section_for_candidate<'a>(
    sections: &'a [PageSection],
    candidate: &MarkdownNodeCandidate,
) -> Option<&'a PageSection> {
    sections.iter().find(|section| {
        section.content.contains(&candidate.evidence_snippet)
            || section.content.contains(&candidate.label)
    })
}

pub(crate) fn page_section_for_line(
    sections: &[PageSection],
    _line_start: usize,
) -> Option<&PageSection> {
    sections.first()
}

pub(crate) fn build_extraction_artifact(
    page_sections: &[PageSection],
    markdown: &str,
    source_path: &str,
    source_id: Option<&str>,
    node_candidates: &[MarkdownNodeCandidate],
    claim_candidates: &[MarkdownClaimCandidate],
) -> ExtractionArtifact {
    let mut concepts = BTreeMap::<String, ExtractedConcept>::new();
    let mut claims = Vec::new();
    let mut evidence_refs = BTreeMap::new();
    let mut concept_ids_by_page = Vec::<(String, Vec<String>, Vec<String>)>::new();

    for candidate in node_candidates.iter().take(20) {
        let matched_label = candidate
            .matched_node_label
            .as_deref()
            .unwrap_or(&candidate.label);
        let key = candidate
            .matched_node_id
            .as_deref()
            .and_then(|node_id| node_id.strip_prefix("concept-"))
            .map(ToString::to_string)
            .unwrap_or_else(|| normalize_key(matched_label));
        if key.is_empty() {
            continue;
        }
        let concept_id = candidate
            .matched_node_id
            .clone()
            .unwrap_or_else(|| format!("concept-{key}"));
        let evidence_id = format!("ev-candidate-{key}");
        let section =
            page_section_for_candidate(page_sections, candidate).or_else(|| page_sections.first());
        let page_index = section.map(|section| section.page_index).unwrap_or(0);
        let page_label = section
            .map(|section| section.page_label.clone())
            .unwrap_or_else(|| "Imported text".into());
        let markdown_path = section.and_then(|section| section.markdown_path.clone());
        let image_path = section.and_then(|section| section.image_path.clone());
        let concept = concepts
            .entry(key.clone())
            .or_insert_with(|| ExtractedConcept {
                id: concept_id.clone(),
                label: matched_label.to_string(),
                aliases: BTreeSet::new(),
                evidence_ids: Vec::new(),
                page_labels: BTreeSet::new(),
            });
        if concept.label != candidate.label {
            concept.aliases.insert(candidate.label.clone());
        }
        if let Some(matched_label) = &candidate.matched_node_label {
            if concept.label != *matched_label {
                concept.aliases.insert(matched_label.clone());
            }
        }
        concept.page_labels.insert(page_label.clone());
        if !concept.evidence_ids.iter().any(|id| id == &evidence_id) {
            concept.evidence_ids.push(evidence_id.clone());
        }
        evidence_refs
            .entry(evidence_id.clone())
            .or_insert_with(|| ExtractionEvidenceRef {
                id: evidence_id.clone(),
                page_index,
                page_label: page_label.clone(),
                snippet: candidate.evidence_snippet.clone(),
                source_path: source_path.to_string(),
                source_id: source_id.map(ToString::to_string),
                markdown_path,
                image_path,
                provenance: format!(
                    "Node candidate '{}' was extracted from the markdown source at line {} because {}{}.",
                    candidate.label,
                    candidate.line_start,
                    candidate.reason,
                    candidate
                        .matched_node_id
                        .as_ref()
                        .map(|node_id| format!(" It matched existing graph node {node_id}"))
                        .unwrap_or_default()
                ),
            });
        claims.push(ExtractedClaim {
            id: format!("claim-candidate-{}", key),
            text: matched_label.to_string(),
            subject_concept_id: concept_id.clone(),
            evidence_id: evidence_id.clone(),
        });
        concept_ids_by_page.push((page_label, vec![concept_id], vec![evidence_id]));
    }

    for section in page_sections {
        let mut seen_on_page = BTreeSet::new();
        let mut page_concept_ids = Vec::new();
        let mut page_evidence_ids = Vec::new();
        let candidates = concept_candidates(&section.content);
        for candidate in candidates {
            let key = normalize_key(&candidate);
            if key.is_empty() || !seen_on_page.insert(key.clone()) {
                continue;
            }
            let concept_id = format!("concept-{key}");
            let concept = concepts
                .entry(key.clone())
                .or_insert_with(|| ExtractedConcept {
                    id: concept_id.clone(),
                    label: candidate.clone(),
                    aliases: BTreeSet::new(),
                    evidence_ids: Vec::new(),
                    page_labels: BTreeSet::new(),
                });
            if concept.label != candidate {
                concept.aliases.insert(candidate.clone());
            }
            concept.page_labels.insert(section.page_label.clone());
            let evidence_id = format!("ev-{}-{}", key, concept.evidence_ids.len() + 1);
            evidence_refs.insert(
                evidence_id.clone(),
                ExtractionEvidenceRef {
                    id: evidence_id.clone(),
                    page_index: section.page_index,
                    page_label: section.page_label.clone(),
                    snippet: excerpt(&section.content, 180),
                    source_path: source_path.to_string(),
                    source_id: source_id.map(ToString::to_string),
                    markdown_path: section.markdown_path.clone(),
                    image_path: section.image_path.clone(),
                    provenance: format!(
                        "Concept '{}' was extracted from {} because the page text produced a stable candidate label.",
                        candidate, section.page_label
                    ),
                },
            );
            concept.evidence_ids.push(evidence_id.clone());
            page_evidence_ids.push(evidence_id.clone());
            claims.push(ExtractedClaim {
                id: format!("claim-{}-{}", key, claims.len() + 1),
                text: candidate.clone(),
                subject_concept_id: concept_id.clone(),
                evidence_id,
            });
            page_concept_ids.push(concept_id);
        }
        if !page_concept_ids.is_empty() {
            concept_ids_by_page.push((
                section.page_label.clone(),
                page_concept_ids,
                page_evidence_ids,
            ));
        }
    }

    for candidate in claim_candidates {
        let section = page_section_for_line(page_sections, candidate.line_start)
            .or_else(|| page_sections.first());
        let page_index = section.map(|section| section.page_index).unwrap_or(0);
        let page_label = section
            .map(|section| section.page_label.clone())
            .unwrap_or_else(|| "Imported text".into());
        let markdown_path = section.and_then(|section| section.markdown_path.clone());
        let image_path = section.and_then(|section| section.image_path.clone());
        evidence_refs
            .entry(candidate.evidence_id.clone())
            .or_insert_with(|| ExtractionEvidenceRef {
                id: candidate.evidence_id.clone(),
                page_index,
                page_label: page_label.clone(),
                snippet: candidate.evidence_snippet.clone(),
                source_path: source_path.to_string(),
                source_id: source_id.map(ToString::to_string),
                markdown_path,
                image_path,
                provenance: format!(
                    "Claim candidate was extracted from markdown line {} because {}.",
                    candidate.line_start, candidate.reason
                ),
            });

        let mut claim_subjects = candidate
            .subject_refs
            .iter()
            .filter(|subject_ref| concepts.values().any(|concept| &concept.id == *subject_ref))
            .cloned()
            .collect::<Vec<_>>();
        if claim_subjects.is_empty() {
            if let Some(label) = derive_concept_label(&candidate.statement) {
                let key = normalize_key(&label);
                let concept_id = format!("concept-{key}");
                if concepts.contains_key(&key) {
                    claim_subjects.push(concept_id);
                }
            }
        }
        for subject_concept_id in claim_subjects {
            claims.push(ExtractedClaim {
                id: candidate.candidate_id.clone(),
                text: candidate.statement.clone(),
                subject_concept_id,
                evidence_id: candidate.evidence_id.clone(),
            });
        }
    }

    if concepts.is_empty() {
        for (index, section) in page_sections.iter().enumerate() {
            let label = fallback_concept_label(&section.content, &section.page_label);
            let key = normalize_key(&label);
            let concept_id = format!("concept-{key}");
            concepts.insert(
                key.clone(),
                ExtractedConcept {
                    id: concept_id.clone(),
                    label,
                    aliases: BTreeSet::new(),
                    evidence_ids: vec![format!("ev-fallback-{}", index + 1)],
                    page_labels: [section.page_label.clone()].into_iter().collect(),
                },
            );
            let evidence_id = format!("ev-fallback-{}", index + 1);
            evidence_refs.insert(
                evidence_id.clone(),
                ExtractionEvidenceRef {
                    id: evidence_id.clone(),
                    page_index: section.page_index,
                    page_label: section.page_label.clone(),
                    snippet: excerpt(&section.content, 180),
                    source_path: source_path.to_string(),
                    source_id: source_id.map(ToString::to_string),
                    markdown_path: section.markdown_path.clone(),
                    image_path: section.image_path.clone(),
                    provenance: format!(
                        "Fallback concept extracted from {} because no stronger concept candidates were found.",
                        section.page_label
                    ),
                },
            );
            claims.push(ExtractedClaim {
                id: format!("claim-fallback-{}", index + 1),
                text: fallback_concept_label(&section.content, &section.page_label),
                subject_concept_id: concept_id.clone(),
                evidence_id: evidence_id.clone(),
            });
            concept_ids_by_page.push((
                section.page_label.clone(),
                vec![concept_id],
                vec![evidence_id],
            ));
        }
    }

    let concepts = concepts.into_values().take(20).collect::<Vec<_>>();
    let allowed_ids = concepts
        .iter()
        .map(|concept| concept.id.clone())
        .collect::<BTreeSet<_>>();
    let mut relations = Vec::new();
    let relationship_evidence =
        extract_markdown_relationship_evidence(markdown, source_path, source_id, node_candidates);
    for evidence in relationship_evidence {
        let source_key = normalize_key(&evidence.source_label);
        let target_key = normalize_key(&evidence.target_label);
        if source_key.is_empty() || target_key.is_empty() || source_key == target_key {
            continue;
        }
        let source_concept_id = evidence
            .resolved_source_node_id
            .clone()
            .or_else(|| evidence.matched_source_node_id.clone())
            .unwrap_or_else(|| format!("concept-{source_key}"));
        let target_concept_id = evidence
            .resolved_target_node_id
            .clone()
            .or_else(|| evidence.matched_target_node_id.clone())
            .unwrap_or_else(|| format!("concept-{target_key}"));
        if source_concept_id == target_concept_id {
            continue;
        }
        if !allowed_ids.contains(&source_concept_id) || !allowed_ids.contains(&target_concept_id) {
            continue;
        }
        let section = page_section_for_line(page_sections, evidence.line_start)
            .or_else(|| page_sections.first());
        let page_index = section.map(|section| section.page_index).unwrap_or(0);
        let page_label = section
            .map(|section| section.page_label.clone())
            .unwrap_or_else(|| "Imported text".into());
        let markdown_path = section.and_then(|section| section.markdown_path.clone());
        let image_path = section.and_then(|section| section.image_path.clone());
        evidence_refs
            .entry(evidence.evidence_id.clone())
            .or_insert_with(|| ExtractionEvidenceRef {
                id: evidence.evidence_id.clone(),
                page_index,
                page_label: page_label.clone(),
                snippet: evidence.snippet.clone(),
                source_path: source_path.to_string(),
                source_id: source_id.map(ToString::to_string),
                markdown_path,
                image_path,
                provenance: format!(
                    "Relationship evidence was extracted from markdown line {} because {}. Endpoints resolved as {}.",
                    evidence.line_start, evidence.reason, evidence.endpoint_resolution
                ),
            });
        relations.push(ExtractedRelation {
            source_concept_id,
            target_concept_id,
            relation_kind: evidence.relation_kind,
            confidence: evidence.confidence,
            evidence_ids: vec![evidence.evidence_id],
            page_labels: [page_label].into_iter().collect(),
        });
    }
    for (page_label, mut concept_ids, evidence_ids) in concept_ids_by_page {
        concept_ids.retain(|id| allowed_ids.contains(id));
        concept_ids.sort();
        concept_ids.dedup();
        for left_index in 0..concept_ids.len() {
            for right_index in (left_index + 1)..concept_ids.len() {
                let (source_concept_id, target_concept_id) =
                    if concept_ids[left_index] <= concept_ids[right_index] {
                        (
                            concept_ids[left_index].clone(),
                            concept_ids[right_index].clone(),
                        )
                    } else {
                        (
                            concept_ids[right_index].clone(),
                            concept_ids[left_index].clone(),
                        )
                    };
                relations.push(ExtractedRelation {
                    source_concept_id,
                    target_concept_id,
                    relation_kind: BrainRelationKind::RelatedTo,
                    confidence: 0.0,
                    evidence_ids: evidence_ids.clone(),
                    page_labels: [page_label.clone()].into_iter().collect(),
                });
            }
        }
    }

    ExtractionArtifact {
        concepts,
        claims,
        relations,
        evidence_refs,
    }
}

pub(crate) fn collected_concepts_from_artifact(artifact: &ExtractionArtifact) -> CollectedConcepts {
    let allowed_ids = artifact
        .concepts
        .iter()
        .map(|concept| concept.id.clone())
        .collect::<BTreeSet<_>>();
    let concepts = artifact
        .concepts
        .iter()
        .map(|concept| ConceptAccumulator {
            id: concept.id.clone(),
            label: concept.label.clone(),
            aliases: concept.aliases.clone(),
            evidence: concept
                .evidence_ids
                .iter()
                .filter_map(|id| artifact.evidence_refs.get(id))
                .map(evidence_ref_from_extraction)
                .collect(),
            page_labels: concept.page_labels.clone(),
        })
        .collect::<Vec<_>>();
    let mut page_concepts_by_label = BTreeMap::<String, PageConceptSet>::new();
    let mut claims = artifact.claims.iter().collect::<Vec<_>>();
    claims.sort_by(|left, right| left.id.cmp(&right.id));
    for claim in claims {
        if !allowed_ids.contains(&claim.subject_concept_id) {
            continue;
        }
        let Some(evidence) = artifact.evidence_refs.get(&claim.evidence_id) else {
            continue;
        };
        let page = page_concepts_by_label
            .entry(evidence.page_label.clone())
            .or_insert_with(|| PageConceptSet {
                page_index: evidence.page_index,
                page_label: evidence.page_label.clone(),
                concept_ids: Vec::new(),
                snippet: evidence.snippet.clone(),
                markdown_path: evidence.markdown_path.clone(),
                image_path: evidence.image_path.clone(),
            });
        page.concept_ids.push(claim.subject_concept_id.clone());
        if claim.text.len() > page.snippet.len() {
            page.snippet = claim.text.clone();
        }
    }
    let mut relation_candidates = Vec::new();
    for relation in &artifact.relations {
        if !allowed_ids.contains(&relation.source_concept_id)
            || !allowed_ids.contains(&relation.target_concept_id)
        {
            continue;
        }
        let relation_evidence_refs = relation
            .evidence_ids
            .iter()
            .filter_map(|id| artifact.evidence_refs.get(id))
            .collect::<Vec<_>>();
        let relation_evidence = relation_evidence_refs
            .iter()
            .map(|evidence| evidence_ref_from_extraction(evidence))
            .collect::<Vec<_>>();
        relation_candidates.push(RelationCandidateAccumulator {
            source_node_id: relation.source_concept_id.clone(),
            target_node_id: relation.target_concept_id.clone(),
            relation_kind: relation.relation_kind,
            confidence: relation.confidence,
            evidence: relation_evidence,
            page_labels: relation.page_labels.clone(),
        });
        for page_label in &relation.page_labels {
            let relation_evidence = relation_evidence_refs
                .iter()
                .find(|evidence| &evidence.page_label == page_label)
                .copied()
                .or_else(|| relation_evidence_refs.first().copied());
            let Some(evidence) = relation_evidence else {
                continue;
            };
            let page = page_concepts_by_label
                .entry(page_label.clone())
                .or_insert_with(|| PageConceptSet {
                    page_index: evidence.page_index,
                    page_label: page_label.clone(),
                    concept_ids: Vec::new(),
                    snippet: evidence.snippet.clone(),
                    markdown_path: evidence.markdown_path.clone(),
                    image_path: evidence.image_path.clone(),
                });
            page.concept_ids.push(relation.source_concept_id.clone());
            page.concept_ids.push(relation.target_concept_id.clone());
            if evidence.snippet.len() > page.snippet.len()
                || evidence.id.starts_with("ev-relation-")
            {
                page.snippet = evidence.snippet.clone();
            }
        }
    }
    let page_concepts = page_concepts_by_label
        .into_values()
        .filter_map(|mut page| {
            page.concept_ids.sort();
            page.concept_ids.dedup();
            (!page.concept_ids.is_empty()).then_some(page)
        })
        .collect();
    CollectedConcepts {
        concepts,
        page_concepts,
        relation_candidates,
    }
}

pub(crate) fn evidence_ref_from_extraction(evidence: &ExtractionEvidenceRef) -> EvidenceRef {
    EvidenceRef {
        id: evidence.id.clone(),
        page_label: evidence.page_label.clone(),
        page_index: Some(evidence.page_index),
        snippet: evidence.snippet.clone(),
        source_path: Some(evidence.source_path.clone()),
        source_id: evidence.source_id.clone(),
        markdown_path: evidence.markdown_path.clone(),
        image_path: evidence.image_path.clone(),
        provenance: Some(evidence.provenance.clone()),
    }
}

pub(crate) fn build_relation_edges(
    document_node: &GraphNodeSummary,
    concept_accumulators: &[ConceptAccumulator],
    page_concepts: &[PageConceptSet],
    relation_candidates: &[RelationCandidateAccumulator],
    source_path: &str,
    source_id: Option<&str>,
) -> (
    Vec<RelationEdgeSummary>,
    BTreeMap<String, RelationEdgeDetail>,
    BTreeMap<String, usize>,
    BTreeMap<String, BTreeSet<String>>,
) {
    let mut edges = Vec::new();
    let mut edge_details_by_id = BTreeMap::new();
    let mut related_count_by_node_id = BTreeMap::<String, usize>::new();
    let mut connected_node_ids_by_node_id = BTreeMap::<String, BTreeSet<String>>::new();
    let concept_by_id = concept_accumulators
        .iter()
        .map(|concept| (concept.id.clone(), concept))
        .collect::<BTreeMap<_, _>>();

    for concept in concept_accumulators {
        let edge = RelationEdgeSummary {
            id: relation_edge_id(RelationKind::SourceDocument, &document_node.id, &concept.id),
            source_node_id: document_node.id.clone(),
            target_node_id: concept.id.clone(),
            kind: RelationKind::SourceDocument,
            label: "Compiled from source".into(),
            confidence: Some(0.94),
            evidence_count: concept.evidence.iter().take(2).count(),
        };
        let evidence = concept.evidence.iter().take(2).cloned().collect::<Vec<_>>();
        edge_details_by_id.insert(
            edge.id.clone(),
            RelationEdgeDetail {
                edge: edge.clone(),
                explanation: format!(
                    "HyprDuck linked the source document to {} because this concept was compiled from cited snippets in the import.",
                    concept.label
                ),
                evidence,
            },
        );
        note_relation(
            &mut related_count_by_node_id,
            &mut connected_node_ids_by_node_id,
            &edge.source_node_id,
            &edge.target_node_id,
        );
        edges.push(edge);
    }

    let mut concept_edge_accumulators = BTreeMap::<(String, String), EdgeAccumulator>::new();
    for candidate in relation_candidates {
        let (source_node_id, target_node_id) =
            if candidate.source_node_id <= candidate.target_node_id {
                (
                    candidate.source_node_id.clone(),
                    candidate.target_node_id.clone(),
                )
            } else {
                (
                    candidate.target_node_id.clone(),
                    candidate.source_node_id.clone(),
                )
            };
        let accumulator = concept_edge_accumulators
            .entry((source_node_id.clone(), target_node_id.clone()))
            .or_insert_with(|| EdgeAccumulator {
                source_node_id: source_node_id.clone(),
                target_node_id: target_node_id.clone(),
                relation_kind: candidate.relation_kind,
                label: markdown_relation_label(candidate.relation_kind),
                confidence: Some(candidate.confidence),
                evidence: Vec::new(),
                page_labels: BTreeSet::new(),
            });
        if accumulator.relation_kind == BrainRelationKind::RelatedTo
            && candidate.relation_kind != BrainRelationKind::RelatedTo
        {
            accumulator.relation_kind = candidate.relation_kind;
            accumulator.label = markdown_relation_label(candidate.relation_kind);
        }
        accumulator.confidence = match (accumulator.confidence, Some(candidate.confidence)) {
            (Some(left), Some(right)) => Some(left.max(right).min(0.94)),
            (Some(left), None) => Some(left),
            (None, Some(right)) => Some(right),
            (None, None) => None,
        };
        accumulator
            .page_labels
            .extend(candidate.page_labels.iter().cloned());
        accumulator
            .evidence
            .extend(candidate.evidence.iter().cloned());
    }
    for page in page_concepts {
        if page.concept_ids.len() < 2 {
            continue;
        }
        for left_index in 0..page.concept_ids.len() {
            for right_index in (left_index + 1)..page.concept_ids.len() {
                let left_id = &page.concept_ids[left_index];
                let right_id = &page.concept_ids[right_index];
                let (source_node_id, target_node_id) = if left_id <= right_id {
                    (left_id.clone(), right_id.clone())
                } else {
                    (right_id.clone(), left_id.clone())
                };
                let accumulator = concept_edge_accumulators
                    .entry((source_node_id.clone(), target_node_id.clone()))
                    .or_insert_with(|| EdgeAccumulator {
                        source_node_id: source_node_id.clone(),
                        target_node_id: target_node_id.clone(),
                        relation_kind: BrainRelationKind::RelatedTo,
                        label: "Related in source".into(),
                        confidence: None,
                        evidence: Vec::new(),
                        page_labels: BTreeSet::new(),
                    });
                accumulator.page_labels.insert(page.page_label.clone());
                accumulator.evidence.push(EvidenceRef {
                    id: format!(
                        "ev-edge-{}-{}-{}",
                        source_node_id,
                        target_node_id,
                        accumulator.evidence.len() + 1
                    ),
                    page_label: page.page_label.clone(),
                    page_index: Some(page.page_index),
                    snippet: page.snippet.clone(),
                    source_path: Some(source_path.to_string()),
                    source_id: source_id.map(ToString::to_string),
                    markdown_path: page.markdown_path.clone(),
                    image_path: page.image_path.clone(),
                    provenance: Some(format!(
                        "Relation evidence extracted because both concepts appeared in {}.",
                        page.page_label
                    )),
                });
            }
        }
    }

    let mut concept_edges = concept_edge_accumulators.into_values().collect::<Vec<_>>();
    concept_edges.sort_by(|left, right| {
        right
            .page_labels
            .len()
            .cmp(&left.page_labels.len())
            .then_with(|| left.source_node_id.cmp(&right.source_node_id))
            .then_with(|| left.target_node_id.cmp(&right.target_node_id))
    });

    for accumulator in concept_edges.into_iter().take(16) {
        let source_label = concept_by_id
            .get(&accumulator.source_node_id)
            .map(|concept| concept.label.clone())
            .unwrap_or_else(|| accumulator.source_node_id.clone());
        let target_label = concept_by_id
            .get(&accumulator.target_node_id)
            .map(|concept| concept.label.clone())
            .unwrap_or_else(|| accumulator.target_node_id.clone());
        let edge = RelationEdgeSummary {
            id: format!(
                "edge-{}-{}",
                accumulator.source_node_id, accumulator.target_node_id
            ),
            source_node_id: accumulator.source_node_id.clone(),
            target_node_id: accumulator.target_node_id.clone(),
            kind: RelationKind::RelatedTo,
            label: accumulator.label.clone(),
            confidence: accumulator.confidence.or_else(|| {
                Some((0.56 + (accumulator.page_labels.len().min(3) as f32 * 0.08)).min(0.84))
            }),
            evidence_count: accumulator.evidence.len(),
        };
        edge_details_by_id.insert(
            edge.id.clone(),
            RelationEdgeDetail {
                edge: edge.clone(),
                explanation: format!(
                    "HyprDuck linked {} and {} because they appeared together in {} page section(s).",
                    source_label,
                    target_label,
                    accumulator.page_labels.len()
                ),
                evidence: accumulator.evidence.clone(),
            },
        );
        note_relation(
            &mut related_count_by_node_id,
            &mut connected_node_ids_by_node_id,
            &edge.source_node_id,
            &edge.target_node_id,
        );
        edges.push(edge);
    }

    (
        edges,
        edge_details_by_id,
        related_count_by_node_id,
        connected_node_ids_by_node_id,
    )
}

pub(crate) fn note_relation(
    related_count_by_node_id: &mut BTreeMap<String, usize>,
    connected_node_ids_by_node_id: &mut BTreeMap<String, BTreeSet<String>>,
    source_node_id: &str,
    target_node_id: &str,
) {
    *related_count_by_node_id
        .entry(source_node_id.to_string())
        .or_default() += 1;
    *related_count_by_node_id
        .entry(target_node_id.to_string())
        .or_default() += 1;
    connected_node_ids_by_node_id
        .entry(source_node_id.to_string())
        .or_default()
        .insert(target_node_id.to_string());
    connected_node_ids_by_node_id
        .entry(target_node_id.to_string())
        .or_default()
        .insert(source_node_id.to_string());
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
        .replace('`', "")
        .replace('*', "")
}

pub(crate) fn derive_concept_label(value: &str) -> Option<String> {
    let first_clause = value
        .split(|char| matches!(char, '.' | ':' | ';' | '(' | ')' | '[' | ']'))
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

pub(crate) fn extract_page_sections(markdown: &str) -> Vec<PageSection> {
    let normalized = markdown.replace("\r\n", "\n");
    let headers = regex_like_page_headers(&normalized);
    if headers.is_empty() {
        return vec![PageSection {
            page_index: 0,
            page_label: "Imported text".into(),
            content: normalized,
            markdown_path: None,
            image_path: None,
        }];
    }

    let mut sections = Vec::with_capacity(headers.len());
    for index in 0..headers.len() {
        let (page_label, _, content_start) = &headers[index];
        let next_start = headers
            .get(index + 1)
            .map(|(_, next_start, _)| *next_start)
            .unwrap_or(normalized.len());
        sections.push(PageSection {
            page_index: index,
            page_label: page_label.clone(),
            content: normalized[*content_start..next_start].trim().to_string(),
            markdown_path: None,
            image_path: None,
        });
    }
    sections
}

pub(crate) fn attach_page_artifacts_to_sections(
    sections: &mut [PageSection],
    source_manifest: Option<&SourceArtifactManifest>,
) {
    let Some(manifest) = source_manifest else {
        return;
    };
    for section in sections {
        let artifact = manifest
            .pages
            .iter()
            .find(|page| page.label == section.page_label)
            .or_else(|| manifest.pages.get(section.page_index));
        if let Some(artifact) = artifact {
            section.page_index = artifact.index;
            section.markdown_path = artifact.markdown_path.clone();
            section.image_path = artifact.image_path.clone();
        }
    }
}

pub(crate) fn regex_like_page_headers(markdown: &str) -> Vec<(String, usize, usize)> {
    let mut headers = Vec::new();
    let mut offset = 0usize;
    for line in markdown.lines() {
        let line_len = line.len();
        if let Some(page_label) = line
            .strip_prefix("## Page ")
            .map(|page| format!("Page {}", page.trim()))
        {
            headers.push((page_label, offset, offset + line_len + 1));
        }
        offset += line_len + 1;
    }
    headers
}

pub(crate) fn infer_markdown_title(markdown_path: &str, markdown: &str) -> String {
    if let Some(heading) = markdown
        .lines()
        .find_map(|line| line.strip_prefix("# ").map(str::trim))
        .filter(|value| !value.is_empty())
    {
        return heading.to_string();
    }

    Path::new(markdown_path)
        .file_stem()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| "HyprDuck import".into())
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
