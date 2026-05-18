use super::*;

pub(crate) fn source_like_node_ids_for_concept(
    project: &KnowledgeProject,
    concept_node_id: &str,
) -> BTreeSet<String> {
    let linked_source_ids = project
        .edges
        .iter()
        .filter(|edge| edge.kind == RelationKind::SourceDocument)
        .filter_map(|edge| {
            if edge.target_node_id == concept_node_id {
                Some(edge.source_node_id.clone())
            } else if edge.source_node_id == concept_node_id {
                Some(edge.target_node_id.clone())
            } else {
                None
            }
        })
        .filter(|node_id| {
            project
                .nodes
                .iter()
                .any(|node| node.id == *node_id && is_source_like_node_kind(node.kind))
        })
        .collect::<BTreeSet<_>>();

    if linked_source_ids.is_empty() {
        source_like_node_ids(project)
    } else {
        linked_source_ids
    }
}

const WORKSPACE_ANSWER_CITATION_LIMIT: usize = 4;
const WORKSPACE_ANSWER_CONTEXT_LIMIT: usize = 8;

#[derive(Debug, Clone, Default)]
pub(crate) struct WorkspaceAnswerBias {
    node_ids: BTreeSet<String>,
    relation_ids: BTreeSet<String>,
    source_ids: BTreeSet<String>,
    evidence_ids: BTreeSet<String>,
    terms: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct WorkspaceAnswerCandidate {
    kind_label: &'static str,
    title: String,
    body: String,
    node_ids: BTreeSet<String>,
    relation_ids: BTreeSet<String>,
    source_ids: BTreeSet<String>,
    evidence_ids: BTreeSet<String>,
    citations: Vec<EvidenceRef>,
    rank: usize,
    query_score: usize,
    bias_score: usize,
}

impl WorkspaceAnswerCandidate {
    fn search_text(&self) -> String {
        format!("{} {}", self.title, self.body)
    }

    fn total_score(&self) -> usize {
        self.query_score.saturating_mul(100) + self.bias_score.min(60)
    }
}

pub(crate) fn answer_materialized_workspace_project(
    reader: &BrainReader,
    request: &AnswerProjectRequest,
) -> Result<AnswerResponse> {
    let question = request.question.trim();
    if question.is_empty() {
        return Ok(AnswerResponse {
            status: AnswerStatus::Blocked,
            text: None,
            explanation: "Ask a concrete question before HyprDuck tries to answer from the materialized workspace graph."
                .into(),
            citations: Vec::new(),
            related_node_ids: request.node_id.clone().into_iter().collect(),
            suggested_actions: vec![SuggestedAction {
                kind: SuggestedActionKind::AskDifferentQuestion,
                label: "Ask a concrete question".into(),
                description:
                    "Workspace answers work best when the question names a concept, source, action, or relationship."
                        .into(),
            }],
        });
    }

    if reader.snapshot.nodes.is_empty()
        && reader.snapshot.evidence.is_empty()
        && reader.snapshot.wiki_pages.is_empty()
        && reader.snapshot.memories.is_empty()
        && reader.snapshot.claims.is_empty()
    {
        return Ok(AnswerResponse {
            status: AnswerStatus::Blocked,
            text: None,
            explanation:
                "No materialized workspace graph is available yet. Import a source or rebuild the graph before asking from it."
                    .into(),
            citations: Vec::new(),
            related_node_ids: Vec::new(),
            suggested_actions: vec![SuggestedAction {
                kind: SuggestedActionKind::AskDifferentQuestion,
                label: "Add graph context".into(),
                description:
                    "Workspace chat needs materialized graph, wiki, source, or evidence records to retrieve from."
                        .into(),
            }],
        });
    }

    let query_terms = search_terms(question);
    let bias = workspace_answer_bias(reader, request.node_id.as_deref());
    let mut candidates = materialized_workspace_answer_candidates(reader);
    for candidate in &mut candidates {
        let search_text = candidate.search_text();
        candidate.query_score = match_score(&query_terms, &search_text).unwrap_or(0);
        candidate.bias_score = workspace_candidate_bias_score(candidate, &bias);
    }
    candidates.retain(|candidate| candidate.query_score > 0);
    candidates.sort_by(|left, right| {
        right
            .total_score()
            .cmp(&left.total_score())
            .then_with(|| right.query_score.cmp(&left.query_score))
            .then_with(|| right.bias_score.cmp(&left.bias_score))
            .then_with(|| left.rank.cmp(&right.rank))
            .then_with(|| left.title.cmp(&right.title))
    });

    let mut top_candidates = candidates
        .into_iter()
        .take(WORKSPACE_ANSWER_CONTEXT_LIMIT)
        .collect::<Vec<_>>();
    let citations = materialized_workspace_citations(reader, &top_candidates);
    let status = if !citations.is_empty() {
        AnswerStatus::Grounded
    } else if !top_candidates.is_empty() {
        AnswerStatus::LowConfidence
    } else {
        top_candidates = fallback_workspace_answer_candidates(reader);
        if top_candidates.is_empty() {
            AnswerStatus::Blocked
        } else {
            AnswerStatus::LowConfidence
        }
    };
    let citations = if citations.is_empty() {
        materialized_workspace_citations(reader, &top_candidates)
    } else {
        citations
    };
    let related_node_ids = related_workspace_answer_node_ids(&top_candidates);

    Ok(AnswerResponse {
        status,
        text: materialized_workspace_answer_text(question, status, &top_candidates, &citations),
        explanation: materialized_workspace_answer_explanation(
            question,
            status,
            request.node_id.as_deref(),
            &top_candidates,
            &citations,
        ),
        citations,
        related_node_ids,
        suggested_actions: answer_suggested_actions(status),
    })
}

pub(crate) fn workspace_answer_bias(
    reader: &BrainReader,
    selected_node_id: Option<&str>,
) -> WorkspaceAnswerBias {
    let Some(selected_node_id) = selected_node_id else {
        return WorkspaceAnswerBias::default();
    };
    let Some(node) = reader
        .snapshot
        .nodes
        .iter()
        .find(|node| node.node_id == selected_node_id)
    else {
        return WorkspaceAnswerBias::default();
    };

    let mut bias = WorkspaceAnswerBias::default();
    bias.node_ids.insert(node.node_id.clone());
    bias.source_ids.extend(node.source_ids.iter().cloned());
    bias.evidence_ids.extend(node.evidence_ids.iter().cloned());
    bias.terms = search_terms(&format!("{} {}", node.label, node.aliases.join(" ")));

    for relation in &reader.snapshot.relations {
        if relation.source_node_id == node.node_id || relation.target_node_id == node.node_id {
            bias.relation_ids.insert(relation.relation_id.clone());
            bias.node_ids.insert(relation.source_node_id.clone());
            bias.node_ids.insert(relation.target_node_id.clone());
            bias.evidence_ids
                .extend(relation.evidence_ids.iter().cloned());
        }
    }
    for claim in &reader.snapshot.claims {
        if claim
            .topic_refs
            .iter()
            .any(|node_id| node_id == &node.node_id)
        {
            bias.node_ids.extend(claim.topic_refs.iter().cloned());
            bias.source_ids.extend(claim.source_refs.iter().cloned());
            bias.evidence_ids
                .extend(claim.evidence_refs.iter().cloned());
        }
    }
    for page in &reader.snapshot.wiki_pages {
        if page
            .node_refs
            .iter()
            .any(|node_id| node_id == &node.node_id)
        {
            bias.node_ids.extend(page.node_refs.iter().cloned());
            bias.source_ids.extend(page.source_refs.iter().cloned());
            bias.evidence_ids.extend(page.evidence_refs.iter().cloned());
        }
    }
    bias
}

fn materialized_workspace_answer_candidates(reader: &BrainReader) -> Vec<WorkspaceAnswerCandidate> {
    let mut candidates = Vec::new();
    let evidence_by_id = reader
        .snapshot
        .evidence
        .iter()
        .map(|evidence| (evidence.id.as_str(), evidence))
        .collect::<BTreeMap<_, _>>();
    let node_label_by_id = reader
        .snapshot
        .nodes
        .iter()
        .map(|node| (node.node_id.as_str(), node.label.as_str()))
        .collect::<BTreeMap<_, _>>();

    for chunk in read_workspace_source_chunks(reader.root()).unwrap_or_default() {
        let citation = EvidenceRef {
            id: format!("retrieved:{}:{}", chunk.source_id, chunk.chunk_id),
            page_label: chunk
                .heading_path
                .last()
                .cloned()
                .unwrap_or_else(|| chunk.source_title.clone()),
            page_index: None,
            snippet: excerpt(&chunk.text, 420),
            source_path: Some(chunk.source_path.clone()),
            source_id: Some(chunk.source_id.clone()),
            markdown_path: Some(chunk.markdown_path.clone()),
            image_path: None,
            provenance: Some(format!(
                "Retrieved from workspace source index lines {}-{}.",
                chunk.line_start, chunk.line_end
            )),
        };
        candidates.push(WorkspaceAnswerCandidate {
            kind_label: "source chunk",
            title: chunk.heading_path.join(" / "),
            body: chunk.text,
            node_ids: BTreeSet::new(),
            relation_ids: BTreeSet::new(),
            source_ids: std::iter::once(chunk.source_id).collect(),
            evidence_ids: BTreeSet::new(),
            citations: vec![citation],
            rank: 0,
            query_score: 0,
            bias_score: 0,
        });
    }

    for evidence in &reader.snapshot.evidence {
        candidates.push(WorkspaceAnswerCandidate {
            kind_label: "evidence",
            title: evidence.page_label.clone(),
            body: evidence.snippet.clone(),
            node_ids: BTreeSet::new(),
            relation_ids: BTreeSet::new(),
            source_ids: evidence.source_id.clone().into_iter().collect(),
            evidence_ids: std::iter::once(evidence.id.clone()).collect(),
            citations: vec![evidence.clone()],
            rank: 1,
            query_score: 0,
            bias_score: 0,
        });
    }

    for claim in &reader.snapshot.claims {
        candidates.push(WorkspaceAnswerCandidate {
            kind_label: "claim",
            title: claim.statement.clone(),
            body: format!(
                "{} {} {}",
                claim.statement,
                claim.topic_refs.join(" "),
                claim.source_refs.join(" ")
            ),
            node_ids: claim.topic_refs.iter().cloned().collect(),
            relation_ids: BTreeSet::new(),
            source_ids: claim.source_refs.iter().cloned().collect(),
            evidence_ids: claim.evidence_refs.iter().cloned().collect(),
            citations: evidence_refs_for_ids(&claim.evidence_refs, &evidence_by_id),
            rank: 2,
            query_score: 0,
            bias_score: 0,
        });
    }

    for memory in &reader.snapshot.memories {
        candidates.push(WorkspaceAnswerCandidate {
            kind_label: "memory",
            title: memory.title.clone(),
            body: format!("{} {}", memory.title, memory.body),
            node_ids: BTreeSet::new(),
            relation_ids: BTreeSet::new(),
            source_ids: memory.source_refs.iter().cloned().collect(),
            evidence_ids: memory.evidence_refs.iter().cloned().collect(),
            citations: evidence_refs_for_ids(&memory.evidence_refs, &evidence_by_id),
            rank: 3,
            query_score: 0,
            bias_score: 0,
        });
    }

    for relation in &reader.snapshot.relations {
        let source_label = node_label_by_id
            .get(relation.source_node_id.as_str())
            .copied()
            .unwrap_or(&relation.source_node_id);
        let target_label = node_label_by_id
            .get(relation.target_node_id.as_str())
            .copied()
            .unwrap_or(&relation.target_node_id);
        candidates.push(WorkspaceAnswerCandidate {
            kind_label: "relation",
            title: relation.label.clone(),
            body: format!(
                "{:?}: {} {} {} {} {}",
                relation.kind,
                source_label,
                relation.label,
                target_label,
                relation.source_node_id,
                relation.target_node_id
            ),
            node_ids: [
                relation.source_node_id.clone(),
                relation.target_node_id.clone(),
            ]
            .into_iter()
            .collect(),
            relation_ids: std::iter::once(relation.relation_id.clone()).collect(),
            source_ids: BTreeSet::new(),
            evidence_ids: relation.evidence_ids.iter().cloned().collect(),
            citations: evidence_refs_for_ids(&relation.evidence_ids, &evidence_by_id),
            rank: 4,
            query_score: 0,
            bias_score: 0,
        });
    }

    for node in &reader.snapshot.nodes {
        candidates.push(WorkspaceAnswerCandidate {
            kind_label: "node",
            title: node.label.clone(),
            body: format!(
                "{} {} {:?} {} {}",
                node.node_id,
                node.label,
                node.kind,
                node.aliases.join(" "),
                node.source_ids.join(" ")
            ),
            node_ids: std::iter::once(node.node_id.clone()).collect(),
            relation_ids: BTreeSet::new(),
            source_ids: node.source_ids.iter().cloned().collect(),
            evidence_ids: node.evidence_ids.iter().cloned().collect(),
            citations: evidence_refs_for_ids(&node.evidence_ids, &evidence_by_id),
            rank: 5,
            query_score: 0,
            bias_score: 0,
        });
    }

    for page in reader
        .read_all_wiki_pages()
        .unwrap_or_else(|_| reader.snapshot.wiki_pages.clone())
    {
        candidates.push(WorkspaceAnswerCandidate {
            kind_label: "wiki page",
            title: page.title.clone(),
            body: format!("{} {} {}", page.path, page.title, page.body),
            node_ids: page.node_refs.iter().cloned().collect(),
            relation_ids: BTreeSet::new(),
            source_ids: page.source_refs.iter().cloned().collect(),
            evidence_ids: page.evidence_refs.iter().cloned().collect(),
            citations: evidence_refs_for_ids(&page.evidence_refs, &evidence_by_id),
            rank: 6,
            query_score: 0,
            bias_score: 0,
        });
    }

    for source in &reader.snapshot.sources {
        candidates.push(WorkspaceAnswerCandidate {
            kind_label: "source",
            title: source.source_id.clone(),
            body: format!(
                "{} {} {} {} {} {}",
                source.source_id,
                source.original_path,
                source.source_path,
                source.markdown_path,
                source.description,
                source.user_context
            ),
            node_ids: BTreeSet::new(),
            relation_ids: BTreeSet::new(),
            source_ids: std::iter::once(source.source_id.clone()).collect(),
            evidence_ids: BTreeSet::new(),
            citations: Vec::new(),
            rank: 7,
            query_score: 0,
            bias_score: 0,
        });
    }

    candidates
}

fn evidence_refs_for_ids(
    ids: &[String],
    evidence_by_id: &BTreeMap<&str, &EvidenceRef>,
) -> Vec<EvidenceRef> {
    ids.iter()
        .filter_map(|id| {
            evidence_by_id
                .get(id.as_str())
                .map(|evidence| (*evidence).clone())
        })
        .collect()
}

fn workspace_candidate_bias_score(
    candidate: &WorkspaceAnswerCandidate,
    bias: &WorkspaceAnswerBias,
) -> usize {
    let mut score = 0usize;
    score += candidate
        .node_ids
        .intersection(&bias.node_ids)
        .count()
        .saturating_mul(18);
    score += candidate
        .relation_ids
        .intersection(&bias.relation_ids)
        .count()
        .saturating_mul(12);
    score += candidate
        .source_ids
        .intersection(&bias.source_ids)
        .count()
        .saturating_mul(8);
    score += candidate
        .evidence_ids
        .intersection(&bias.evidence_ids)
        .count()
        .saturating_mul(6);
    if !bias.terms.is_empty() {
        score += match_score(&bias.terms, &candidate.search_text())
            .unwrap_or(0)
            .min(12);
    }
    score
}

fn materialized_workspace_citations(
    reader: &BrainReader,
    candidates: &[WorkspaceAnswerCandidate],
) -> Vec<EvidenceRef> {
    let evidence_by_id = reader
        .snapshot
        .evidence
        .iter()
        .map(|evidence| (evidence.id.as_str(), evidence))
        .collect::<BTreeMap<_, _>>();
    let mut seen = BTreeSet::new();
    let mut citations = Vec::new();
    for candidate in candidates {
        for citation in &candidate.citations {
            if seen.insert(citation.id.clone()) {
                citations.push(citation.clone());
            }
            if citations.len() >= WORKSPACE_ANSWER_CITATION_LIMIT {
                return citations;
            }
        }
        for evidence_id in &candidate.evidence_ids {
            if !seen.insert(evidence_id.clone()) {
                continue;
            }
            if let Some(evidence) = evidence_by_id.get(evidence_id.as_str()) {
                citations.push((*evidence).clone());
            }
            if citations.len() >= WORKSPACE_ANSWER_CITATION_LIMIT {
                return citations;
            }
        }
    }
    citations
}

fn fallback_workspace_answer_candidates(reader: &BrainReader) -> Vec<WorkspaceAnswerCandidate> {
    let mut candidates = materialized_workspace_answer_candidates(reader);
    candidates.sort_by(|left, right| {
        left.rank
            .cmp(&right.rank)
            .then_with(|| left.title.cmp(&right.title))
    });
    candidates
        .into_iter()
        .take(WORKSPACE_ANSWER_CONTEXT_LIMIT.min(3))
        .collect()
}

fn related_workspace_answer_node_ids(candidates: &[WorkspaceAnswerCandidate]) -> Vec<String> {
    candidates
        .iter()
        .flat_map(|candidate| candidate.node_ids.iter().cloned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .take(8)
        .collect()
}

fn materialized_workspace_answer_text(
    question: &str,
    status: AnswerStatus,
    candidates: &[WorkspaceAnswerCandidate],
    citations: &[EvidenceRef],
) -> Option<String> {
    let korean = contains_hangul(question);
    let points = workspace_answer_points(candidates, citations, 7);
    let source_labels = workspace_answer_source_labels(candidates, citations);
    match status {
        AnswerStatus::Grounded => {
            if korean {
                Some(format_workspace_answer(
                    "질문과 관련된 내용을 워크스페이스에서 찾았습니다.",
                    &points,
                    &source_labels,
                    "근거",
                ))
            } else {
                Some(format_workspace_answer(
                    "Here is what the workspace says about your question.",
                    &points,
                    &source_labels,
                    "Sources",
                ))
            }
        }
        AnswerStatus::LowConfidence => {
            if korean {
                Some(format_workspace_answer(
                    "직접 일치는 약하지만, 워크스페이스에서 가장 가까운 내용을 찾았습니다.",
                    &points,
                    &source_labels,
                    "근거",
                ))
            } else {
                Some(format_workspace_answer(
                    "HyprDuck found weak direct overlap, but these workspace notes are closest.",
                    &points,
                    &source_labels,
                    "Sources",
                ))
            }
        }
        AnswerStatus::Blocked => Some(format!(
            "HyprDuck cannot safely answer \"{}\" from the current materialized workspace yet.",
            question
        )),
        AnswerStatus::Stale => {
            Some("HyprDuck is still reading from a stale workspace snapshot.".into())
        }
    }
}

fn format_workspace_answer(
    lead: &str,
    points: &[String],
    source_labels: &[String],
    source_heading: &str,
) -> String {
    let mut lines = vec![lead.to_string()];
    if !points.is_empty() {
        lines.push(String::new());
        lines.extend(points.iter().map(|point| format!("- {point}")));
    }
    if !source_labels.is_empty() {
        lines.push(String::new());
        lines.push(format!("{source_heading}: {}", source_labels.join(", ")));
    }
    lines.join("\n")
}

fn workspace_answer_points(
    candidates: &[WorkspaceAnswerCandidate],
    citations: &[EvidenceRef],
    limit: usize,
) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut points = Vec::new();

    for candidate in candidates {
        for segment in answer_segments(&candidate.body) {
            push_answer_point(&mut points, &mut seen, segment, limit);
            if points.len() >= limit {
                return points;
            }
        }
    }

    for citation in citations {
        for segment in answer_segments(&citation.snippet) {
            push_answer_point(&mut points, &mut seen, segment, limit);
            if points.len() >= limit {
                return points;
            }
        }
    }

    points
}

fn push_answer_point(
    points: &mut Vec<String>,
    seen: &mut BTreeSet<String>,
    segment: String,
    limit: usize,
) {
    if points.len() >= limit || !is_useful_answer_segment(&segment) {
        return;
    }
    let point = excerpt(&segment, 260);
    let key = point.to_lowercase();
    if seen.insert(key) {
        points.push(point);
    }
}

fn answer_segments(value: &str) -> Vec<String> {
    let mut segments = Vec::new();
    for line in value.lines() {
        let line = clean_answer_line(line);
        if line.is_empty() {
            continue;
        }
        for section in split_numbered_sections(&line) {
            if section.chars().count() > 320 {
                segments.extend(split_sentence_segments(&section));
            } else {
                segments.push(section);
            }
        }
    }
    if segments.is_empty() {
        let compact = clean_answer_line(value);
        if !compact.is_empty() {
            segments.extend(split_sentence_segments(&compact));
        }
    }
    segments
}

fn clean_answer_line(value: &str) -> String {
    value
        .replace('\r', " ")
        .replace("**", "")
        .replace('`', "")
        .trim()
        .trim_start_matches(|ch| matches!(ch, '#' | '-' | '*' | '•' | ' '))
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn split_numbered_sections(value: &str) -> Vec<String> {
    let starts = numbered_section_starts(value);
    if starts.len() <= 1 {
        return vec![value.to_string()];
    }
    let mut segments = Vec::new();
    for (index, start) in starts.iter().enumerate() {
        let end = starts.get(index + 1).copied().unwrap_or(value.len());
        let segment = value[*start..end].trim();
        if !segment.is_empty() {
            segments.push(segment.to_string());
        }
    }
    segments
}

fn numbered_section_starts(value: &str) -> Vec<usize> {
    let bytes = value.as_bytes();
    let mut starts = Vec::new();
    let mut index = 0usize;
    while index + 3 < bytes.len() {
        let previous_boundary = index == 0 || bytes[index - 1].is_ascii_whitespace();
        if previous_boundary && bytes[index].is_ascii_digit() {
            let mut cursor = index + 1;
            while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
                cursor += 1;
            }
            if cursor + 2 < bytes.len()
                && bytes[cursor] == b'.'
                && bytes[cursor + 1].is_ascii_digit()
            {
                cursor += 2;
                while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
                    cursor += 1;
                }
                if cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
                    starts.push(index);
                    index = cursor;
                    continue;
                }
            }
        }
        index += 1;
    }
    starts
}

fn split_sentence_segments(value: &str) -> Vec<String> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        current.push(ch);
        let boundary = matches!(ch, '.' | '!' | '?' | '。' | '؟')
            && chars.peek().is_some_and(|next| next.is_whitespace());
        if boundary || current.chars().count() >= 260 {
            let segment = current.trim().to_string();
            if !segment.is_empty() {
                segments.push(segment);
            }
            current.clear();
        }
    }
    let segment = current.trim();
    if !segment.is_empty() {
        segments.push(segment.to_string());
    }
    segments
}

fn is_useful_answer_segment(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return false;
    }
    let lower = trimmed.to_lowercase();
    if lower.starts_with("source evidence prepared from")
        || lower.starts_with("derived graph nodes")
        || lower == "no visible evidence snippet is available yet."
    {
        return false;
    }
    trimmed.chars().count() >= 8
}

fn workspace_answer_source_labels(
    candidates: &[WorkspaceAnswerCandidate],
    citations: &[EvidenceRef],
) -> Vec<String> {
    let mut labels = Vec::new();
    let mut seen = BTreeSet::new();
    for citation in citations {
        let label = citation
            .source_path
            .as_deref()
            .or(citation.markdown_path.as_deref())
            .or(citation.source_id.as_deref())
            .map(file_name_label);
        if let Some(label) = label {
            if seen.insert(label.clone()) {
                labels.push(label);
            }
        }
        if labels.len() >= 3 {
            return labels;
        }
    }
    for candidate in candidates {
        for source_id in &candidate.source_ids {
            if seen.insert(source_id.clone()) {
                labels.push(source_id.clone());
            }
            if labels.len() >= 3 {
                return labels;
            }
        }
    }
    labels
}

fn file_name_label(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path)
        .to_string()
}

fn contains_hangul(value: &str) -> bool {
    value
        .chars()
        .any(|ch| ('\u{ac00}'..='\u{d7a3}').contains(&ch))
}

fn materialized_workspace_answer_explanation(
    question: &str,
    status: AnswerStatus,
    selected_node_id: Option<&str>,
    candidates: &[WorkspaceAnswerCandidate],
    citations: &[EvidenceRef],
) -> String {
    let selected_note = if selected_node_id.is_some() {
        " The selected node was used only as a retrieval ranking hint."
    } else {
        ""
    };
    match status {
        AnswerStatus::Grounded => format!(
            "HyprDuck answered \"{}\" from the materialized workspace graph with {} retrieved context item(s) and {} citation(s).{}",
            question,
            candidates.len(),
            citations.len(),
            selected_note
        ),
        AnswerStatus::LowConfidence => format!(
            "HyprDuck searched the materialized workspace graph for \"{}\", but the retrieved context had weak direct overlap.{}",
            question, selected_note
        ),
        AnswerStatus::Blocked => format!(
            "HyprDuck blocked this answer because it could not find materialized workspace context for \"{}\".{}",
            question, selected_note
        ),
        AnswerStatus::Stale => "HyprDuck is still reading from a stale workspace snapshot.".into(),
    }
}

pub(crate) fn build_answer_for_detail(
    project: &KnowledgeProject,
    detail: &GraphNodeDetail,
    related_node_ids: Vec<String>,
) -> AnswerResponse {
    match detail.node.kind {
        GraphNodeKind::Source | GraphNodeKind::Document => {
            let concept_count = project
                .nodes
                .iter()
                .filter(|node| node.kind == GraphNodeKind::Concept)
                .count();
            let concept_relationship_count = project
                .edges
                .iter()
                .filter(|edge| edge.kind == RelationKind::RelatedTo)
                .count();
            AnswerResponse {
                status: if concept_count > 0 {
                    AnswerStatus::Grounded
                } else {
                    AnswerStatus::LowConfidence
                },
                text: Some(format!(
                    "HyprDuck currently tracks {} concept nodes and {} explainable concept links in this workspace.",
                    concept_count, concept_relationship_count
                )),
                explanation:
                    "This document-level answer reflects the current corrected graph and stays grounded in visible evidence.".into(),
                citations: detail.evidence.iter().take(3).cloned().collect(),
                related_node_ids,
                suggested_actions: vec![
                    SuggestedAction {
                        kind: SuggestedActionKind::InspectEvidence,
                        label: "Inspect evidence".into(),
                        description:
                            "Review the cited snippets before using the workspace-wide answer."
                                .into(),
                    },
                    SuggestedAction {
                        kind: SuggestedActionKind::AskDifferentQuestion,
                        label: "Ask a narrower question".into(),
                        description:
                            "Grounded answers get stronger when you focus on one concept at a time."
                                .into(),
                    },
                ],
            }
        }
        GraphNodeKind::Concept | GraphNodeKind::Page => {
            let page_count = detail
                .evidence
                .iter()
                .map(|evidence| evidence.page_label.clone())
                .collect::<BTreeSet<_>>()
                .len();
            AnswerResponse {
                status: if detail.evidence.is_empty() {
                    AnswerStatus::LowConfidence
                } else {
                    AnswerStatus::Grounded
                },
                text: Some(format!(
                    "{} currently has {} visible evidence refs across {} page(s).",
                    detail.canonical_name,
                    detail.evidence.len(),
                    page_count
                )),
                explanation:
                    "This answer reflects the current corrected concept node and its visible evidence."
                        .into(),
                citations: detail.evidence.iter().take(3).cloned().collect(),
                related_node_ids,
                suggested_actions: vec![SuggestedAction {
                    kind: SuggestedActionKind::InspectEvidence,
                    label: "Inspect evidence".into(),
                    description:
                        "Use the cited snippets to verify the corrected concept before acting on it."
                            .into(),
                }],
            }
        }
    }
}

pub(crate) fn best_matching_evidence(question: &str, detail: &GraphNodeDetail) -> Vec<EvidenceRef> {
    let question_terms = question_terms(question);
    if question_terms.is_empty() {
        return detail.evidence.iter().take(3).cloned().collect();
    }

    let mut scored = detail
        .evidence
        .iter()
        .map(|evidence| {
            let score = overlap_score(&question_terms, &evidence.snippet);
            (score, evidence.clone())
        })
        .collect::<Vec<_>>();
    scored.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then_with(|| left.1.page_label.cmp(&right.1.page_label))
    });

    let matched = scored
        .iter()
        .filter(|(score, _)| *score > 0)
        .map(|(_, evidence)| evidence.clone())
        .take(3)
        .collect::<Vec<_>>();
    if matched.is_empty() {
        detail.evidence.iter().take(2).cloned().collect()
    } else {
        matched
    }
}

pub(crate) fn answer_text_for_question(
    project: &KnowledgeProject,
    detail: &GraphNodeDetail,
    question: &str,
    status: AnswerStatus,
    citations: &[EvidenceRef],
) -> String {
    let evidence_summary = citations
        .first()
        .map(|citation| citation.snippet.clone())
        .unwrap_or_else(|| "HyprDuck could not find a directly relevant snippet yet.".into());
    let page_count = detail
        .evidence
        .iter()
        .map(|evidence| evidence.page_label.clone())
        .collect::<BTreeSet<_>>()
        .len();

    match detail.node.kind {
        GraphNodeKind::Source | GraphNodeKind::Document => {
            let concept_count = project
                .nodes
                .iter()
                .filter(|node| node.kind == GraphNodeKind::Concept)
                .count();
            match status {
                AnswerStatus::Grounded => format!(
                    "For \"{}\", the strongest grounded reading is that this workspace currently contains {} concept nodes. Best visible support: {}",
                    question, concept_count, evidence_summary
                ),
                AnswerStatus::LowConfidence => format!(
                    "HyprDuck can partially answer \"{}\", but the graph only has weak snippet overlap. Closest visible support: {}",
                    question, evidence_summary
                ),
                AnswerStatus::Blocked | AnswerStatus::Stale => format!(
                    "HyprDuck cannot safely answer \"{}\" from the current workspace yet.",
                    question
                ),
            }
        }
        GraphNodeKind::Concept | GraphNodeKind::Page => match status {
            AnswerStatus::Grounded => format!(
                "For \"{}\", {} is supported by {} visible evidence refs across {} page(s). Best visible support: {}",
                question,
                detail.canonical_name,
                detail.evidence.len(),
                page_count,
                evidence_summary
            ),
            AnswerStatus::LowConfidence => format!(
                "HyprDuck found {} evidence refs for {}, but the question \"{}\" only weakly matches those snippets. Closest visible support: {}",
                detail.evidence.len(),
                detail.canonical_name,
                question,
                evidence_summary
            ),
            AnswerStatus::Blocked | AnswerStatus::Stale => format!(
                "HyprDuck cannot safely answer \"{}\" for {} yet.",
                question, detail.canonical_name
            ),
        },
    }
}

pub(crate) fn answer_explanation_for_question(
    detail: &GraphNodeDetail,
    question: &str,
    status: AnswerStatus,
    citations: &[EvidenceRef],
) -> String {
    match status {
        AnswerStatus::Grounded => format!(
            "HyprDuck answered \"{}\" using {} visible citation(s) attached to {}.",
            question,
            citations.len(),
            detail.canonical_name
        ),
        AnswerStatus::LowConfidence => format!(
            "HyprDuck kept this answer cautious because the question \"{}\" only loosely overlaps with the visible evidence on {}.",
            question, detail.canonical_name
        ),
        AnswerStatus::Blocked => format!(
            "HyprDuck blocked this answer because it could not find enough grounded evidence for \"{}\".",
            question
        ),
        AnswerStatus::Stale => "HyprDuck is still reading from a stale workspace snapshot.".into(),
    }
}

pub(crate) fn answer_suggested_actions(status: AnswerStatus) -> Vec<SuggestedAction> {
    match status {
        AnswerStatus::Grounded => vec![SuggestedAction {
            kind: SuggestedActionKind::InspectEvidence,
            label: "Inspect evidence".into(),
            description: "Review the cited snippets if you want to verify the grounded answer."
                .into(),
        }],
        AnswerStatus::LowConfidence => vec![
            SuggestedAction {
                kind: SuggestedActionKind::InspectEvidence,
                label: "Inspect evidence".into(),
                description:
                    "Check the cited snippets to see where the question stopped matching strongly."
                        .into(),
            },
            SuggestedAction {
                kind: SuggestedActionKind::AskDifferentQuestion,
                label: "Ask a narrower question".into(),
                description:
                    "Use a concept name, relationship, or page label to get a more grounded answer."
                        .into(),
            },
        ],
        AnswerStatus::Blocked | AnswerStatus::Stale => vec![SuggestedAction {
            kind: SuggestedActionKind::AskDifferentQuestion,
            label: "Ask a narrower question".into(),
            description:
                "HyprDuck needs a more concrete, evidence-seeking question before it can answer."
                    .into(),
        }],
    }
}

pub(crate) fn question_terms(question: &str) -> BTreeSet<String> {
    text_terms(question)
}

pub(crate) fn text_terms(value: &str) -> BTreeSet<String> {
    value
        .split(|char: char| !char.is_ascii_alphanumeric())
        .map(|term| term.trim().to_ascii_lowercase())
        .filter(|term| term.len() >= 3)
        .collect()
}

pub(crate) fn overlap_score(question_terms: &BTreeSet<String>, haystack: &str) -> usize {
    let haystack_terms = haystack
        .split(|char: char| !char.is_ascii_alphanumeric())
        .map(|term| term.trim().to_ascii_lowercase())
        .filter(|term| term.len() >= 3)
        .collect::<BTreeSet<_>>();
    question_terms.intersection(&haystack_terms).count()
}

pub(crate) fn edge_explanation(
    edge: &RelationEdgeSummary,
    label_by_node_id: &BTreeMap<String, String>,
    evidence: &[EvidenceRef],
) -> String {
    let source_label = label_by_node_id
        .get(&edge.source_node_id)
        .cloned()
        .unwrap_or_else(|| edge.source_node_id.clone());
    let target_label = label_by_node_id
        .get(&edge.target_node_id)
        .cloned()
        .unwrap_or_else(|| edge.target_node_id.clone());

    match edge.kind {
        RelationKind::SourceDocument => format!(
            "HyprDuck linked the source document to {} because this concept is grounded in cited snippets from the import.",
            target_label
        ),
        RelationKind::RelatedTo if edge.label == "Separated by correction" => format!(
            "HyprDuck keeps {} and {} separate because you explicitly split them during correction review.",
            source_label, target_label
        ),
        RelationKind::RelatedTo => format!(
            "HyprDuck linked {} and {} because they share {} visible evidence ref(s).",
            source_label,
            target_label,
            evidence.len()
        ),
    }
}

pub(crate) fn relation_edge_id(
    kind: RelationKind,
    source_node_id: &str,
    target_node_id: &str,
) -> String {
    match kind {
        RelationKind::SourceDocument => format!("edge-{}-{}", source_node_id, target_node_id),
        RelationKind::RelatedTo => format!("edge-{}-{}", source_node_id, target_node_id),
    }
}

pub(crate) fn normalized_edge_label(kind: RelationKind, label: &str) -> String {
    match kind {
        RelationKind::SourceDocument => "Compiled from source".into(),
        RelationKind::RelatedTo if label == "Separated by correction" => {
            "Separated by correction".into()
        }
        RelationKind::RelatedTo
            if matches!(
                label,
                "Supports" | "Contradicts" | "Supersedes" | "Same as" | "Depends on"
            ) =>
        {
            label.into()
        }
        RelationKind::RelatedTo => "Related in source".into(),
    }
}

pub(crate) fn preferred_edge_label(current: &str, incoming: &str, kind: RelationKind) -> String {
    match kind {
        RelationKind::SourceDocument => "Compiled from source".into(),
        RelationKind::RelatedTo if current == "Separated by correction" => current.into(),
        RelationKind::RelatedTo if incoming == "Separated by correction" => incoming.into(),
        RelationKind::RelatedTo if current != "Related in source" => current.into(),
        RelationKind::RelatedTo
            if matches!(
                incoming,
                "Supports" | "Contradicts" | "Supersedes" | "Same as" | "Depends on"
            ) =>
        {
            incoming.into()
        }
        RelationKind::RelatedTo => "Related in source".into(),
    }
}

pub(crate) fn dedupe_evidence(evidence: Vec<EvidenceRef>) -> Vec<EvidenceRef> {
    let mut seen = BTreeSet::new();
    let mut deduped = Vec::new();
    for item in evidence {
        let key = format!(
            "{}|{}|{}|{}",
            item.id,
            item.page_label,
            item.snippet,
            item.source_path.clone().unwrap_or_default()
        );
        if seen.insert(key) {
            deduped.push(item);
        }
    }
    deduped
}

pub(crate) fn unique_manual_node_id(project: &KnowledgeProject, label: &str) -> String {
    let base = normalize_key(label);
    let base_id = format!("concept-{base}");
    if !project.nodes.iter().any(|node| node.id == base_id) {
        return base_id;
    }

    let mut suffix = 2usize;
    loop {
        let candidate = format!("concept-{base}-manual-{suffix}");
        if !project.nodes.iter().any(|node| node.id == candidate) {
            return candidate;
        }
        suffix += 1;
    }
}

pub(crate) fn manual_split_position(base: &GraphNodePosition, index: usize) -> GraphNodePosition {
    let column = (index % 2) as f32;
    let row = (index / 2) as f32;
    GraphNodePosition {
        x: (base.x + 10.0 + column * 12.0).min(90.0),
        y: (base.y + row * 10.0).min(88.0),
    }
}

pub(crate) fn layout_concept_positions(count: usize) -> Vec<GraphNodePosition> {
    let per_row = if count > 9 { 4 } else { 3 };
    let row_count = ((count as f32) / (per_row as f32)).ceil() as usize;
    let row_spacing = if row_count > 1 {
        48.0 / (row_count.saturating_sub(1) as f32)
    } else {
        0.0
    };
    let mut positions = Vec::with_capacity(count);

    for index in 0..count {
        let row = index / per_row;
        let col = index % per_row;
        let columns_in_row = if row == row_count.saturating_sub(1) {
            let remainder = count % per_row;
            if remainder == 0 {
                per_row
            } else {
                remainder
            }
        } else {
            per_row
        };
        let x = if columns_in_row == 1 {
            50.0
        } else {
            18.0 + (64.0 / (columns_in_row.saturating_sub(1) as f32)) * (col as f32)
        };
        let y = 40.0 + row_spacing * (row as f32);
        positions.push(GraphNodePosition { x, y });
    }

    positions
}
