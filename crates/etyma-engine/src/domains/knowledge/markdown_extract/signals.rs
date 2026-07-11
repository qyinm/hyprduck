use super::*;

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
            let metadata_frequencies = markdown_search_token_frequencies(&metadata_text);
            let content_frequencies = markdown_search_token_frequencies(&body);
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
