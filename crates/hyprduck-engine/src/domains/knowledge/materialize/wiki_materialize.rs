use super::*;

pub(crate) fn build_materialized_wiki_pages(
    workspace_id: &str,
    sources: &[SourceRecord],
    nodes: &[BrainNodeRecord],
    generated_at: u64,
) -> Vec<WikiPage> {
    let mut pages = vec![
        WikiPage {
            page_id: "wiki-overview".into(),
            workspace_id: workspace_id.into(),
            path: "wiki/overview.md".into(),
            title: "Workspace Overview".into(),
            body: format!(
                "# Workspace Overview\n\n- Workspace: `{workspace_id}`\n- Sources: {}\n- Nodes: {}\n",
                sources.len(),
                nodes.len()
            ),
            node_refs: nodes.iter().map(|node| node.node_id.clone()).collect(),
            source_refs: sources
                .iter()
                .map(|source| source.source_id.clone())
                .collect(),
            evidence_refs: nodes
                .iter()
                .flat_map(|node| node.evidence_ids.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
            updated_at: generated_at,
        },
        WikiPage {
            page_id: "wiki-index".into(),
            workspace_id: workspace_id.into(),
            path: "wiki/index.md".into(),
            title: "Brain Index".into(),
            body: String::new(),
            node_refs: nodes.iter().map(|node| node.node_id.clone()).collect(),
            source_refs: sources
                .iter()
                .map(|source| source.source_id.clone())
                .collect(),
            evidence_refs: Vec::new(),
            updated_at: generated_at,
        },
        WikiPage {
            page_id: "wiki-log".into(),
            workspace_id: workspace_id.into(),
            path: "wiki/log.md".into(),
            title: "Brain Log".into(),
            body: String::new(),
            node_refs: Vec::new(),
            source_refs: Vec::new(),
            evidence_refs: Vec::new(),
            updated_at: generated_at,
        },
    ];
    pages.extend(sources.iter().map(|source| WikiPage {
        page_id: format!("wiki-source-{}", source.source_id),
        workspace_id: workspace_id.into(),
        path: format!("wiki/sources/{}.md", sanitize_name(&source.source_id)),
        title: source.source_id.clone(),
        body: format!(
            "# {}\n\n- Original: `{}`\n- Source: `{}`\n- Markdown: `{}`\n- Status: `{}`\n",
            source.source_id,
            source.original_path,
            source.source_path,
            source.markdown_path,
            source.status
        ),
        node_refs: Vec::new(),
        source_refs: vec![source.source_id.clone()],
        evidence_refs: Vec::new(),
        updated_at: generated_at,
    }));
    pages.extend(
        nodes
            .iter()
            .filter(|node| node.kind == BrainNodeKind::Concept)
            .map(|node| WikiPage {
                page_id: format!("wiki-topic-{}", node.node_id),
                workspace_id: workspace_id.into(),
                path: format!("wiki/topics/{}.md", sanitize_name(&node.node_id)),
                title: node.label.clone(),
                body: format!(
                    "# {}\n\n- Node: `{}`\n- Sources: {}\n- Evidence refs: {}\n",
                    node.label,
                    node.node_id,
                    node.source_ids.join(", "),
                    node.evidence_ids.len()
                ),
                node_refs: vec![node.node_id.clone()],
                source_refs: node.source_ids.clone(),
                evidence_refs: node.evidence_ids.clone(),
                updated_at: generated_at,
            }),
    );
    pages
}

pub(crate) fn materialized_wiki_page_body(page: &WikiPage, snapshot: &BrainRepoSnapshot) -> String {
    if page.path == "wiki/index.md" {
        let source_links = snapshot
            .sources
            .iter()
            .map(|source| {
                format!(
                    "- [{}](sources/{}.md)",
                    source.source_id,
                    sanitize_name(&source.source_id)
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let topic_links = snapshot
            .nodes
            .iter()
            .filter(|node| node.kind == BrainNodeKind::Concept)
            .map(|node| {
                format!(
                    "- [{}](topics/{}.md)",
                    node.label,
                    sanitize_name(&node.node_id)
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        return format!(
            "# Brain Index\n\n## Sources\n\n{}\n\n## Topics\n\n{}\n",
            source_links, topic_links
        );
    }
    if page.path == "wiki/log.md" {
        return snapshot
            .events
            .iter()
            .map(|event| {
                format!(
                    "- {} `{}` by `{}`",
                    event.created_at, event.event_id, event.actor.actor_id
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    if page.path.starts_with("wiki/topics/") {
        let page_node_ids = page.node_refs.iter().collect::<BTreeSet<_>>();
        let evidence_by_id = snapshot
            .evidence
            .iter()
            .map(|evidence| (evidence.id.as_str(), evidence))
            .collect::<BTreeMap<_, _>>();
        let node_descriptions = snapshot
            .nodes
            .iter()
            .filter(|node| page_node_ids.contains(&node.node_id))
            .filter_map(|node| {
                let evidence = node
                    .evidence_ids
                    .iter()
                    .filter_map(|evidence_id| evidence_by_id.get(evidence_id.as_str()).copied())
                    .find(|evidence| !evidence.snippet.trim().is_empty())?;
                Some(format!(
                    "- `{}`: {} _(source: {}; evidence: `{}`)_",
                    node.node_id,
                    evidence.snippet.trim(),
                    evidence
                        .source_id
                        .as_deref()
                        .or(evidence.source_path.as_deref())
                        .unwrap_or("unknown"),
                    evidence.id
                ))
            })
            .collect::<Vec<_>>();
        let attached_claims = snapshot
            .claims
            .iter()
            .filter(|claim| {
                claim
                    .topic_refs
                    .iter()
                    .any(|node_id| page_node_ids.contains(node_id))
            })
            .map(|claim| {
                format!(
                    "- `{}` {} _(sources: {}; evidence: {})_",
                    claim.status,
                    claim.statement,
                    join_or_none(&claim.source_refs),
                    join_or_none(&claim.evidence_refs)
                )
            })
            .collect::<Vec<_>>();
        let node_labels = snapshot
            .nodes
            .iter()
            .map(|node| (node.node_id.as_str(), node.label.as_str()))
            .collect::<BTreeMap<_, _>>();
        let topic_page_paths = snapshot
            .wiki_pages
            .iter()
            .filter(|page| page.path.starts_with("wiki/topics/"))
            .flat_map(|page| {
                page.node_refs
                    .iter()
                    .map(move |node_id| (node_id.as_str(), page.path.as_str()))
            })
            .collect::<BTreeMap<_, _>>();
        let attached_relations = snapshot
            .relations
            .iter()
            .filter(|relation| {
                page_node_ids.contains(&relation.source_node_id)
                    || page_node_ids.contains(&relation.target_node_id)
            })
            .map(|relation| {
                let source_label = node_labels
                    .get(relation.source_node_id.as_str())
                    .copied()
                    .unwrap_or(relation.source_node_id.as_str());
                let target_label = node_labels
                    .get(relation.target_node_id.as_str())
                    .copied()
                    .unwrap_or(relation.target_node_id.as_str());
                let source_link = topic_node_wiki_link(
                    source_label,
                    &relation.source_node_id,
                    &page.path,
                    &topic_page_paths,
                );
                let target_link = topic_node_wiki_link(
                    target_label,
                    &relation.target_node_id,
                    &page.path,
                    &topic_page_paths,
                );
                let relation_source_refs =
                    source_refs_for_evidence_ids(&relation.evidence_ids, &evidence_by_id);
                format!(
                    "- `{}` {} -> {} _(relation: {}; sources: {}; evidence: {})_",
                    relation.relation_id,
                    source_link,
                    target_link,
                    relation.label,
                    join_or_none(&relation_source_refs),
                    join_or_none(&relation.evidence_ids)
                )
            })
            .collect::<Vec<_>>();
        let source_references = topic_source_references_markdown(page, snapshot, &evidence_by_id);
        if !node_descriptions.is_empty()
            || !attached_claims.is_empty()
            || !attached_relations.is_empty()
            || !source_references.is_empty()
        {
            let mut body = page.body.trim_end().to_string();
            if !source_references.is_empty() {
                body.push_str("\n\n## Source References\n\n");
                body.push_str(&source_references.join("\n"));
                body.push('\n');
            }
            if !node_descriptions.is_empty() {
                body.push_str("\n\n## Node Description\n\n");
                body.push_str(&node_descriptions.join("\n"));
                body.push('\n');
            }
            if !attached_claims.is_empty() {
                body.push_str(
                    "\n\n## Claims\n\n_Source-backed claims linked to materialized evidence._\n\n",
                );
                body.push_str(&attached_claims.join("\n"));
                body.push('\n');
            }
            if !attached_relations.is_empty() {
                body.push_str("\n\n## Relations\n\n");
                body.push_str(&attached_relations.join("\n"));
                body.push('\n');
            }
            return body;
        }
    }
    page.body.clone()
}

pub(crate) fn topic_source_references_markdown(
    page: &WikiPage,
    snapshot: &BrainRepoSnapshot,
    evidence_by_id: &BTreeMap<&str, &EvidenceRef>,
) -> Vec<String> {
    let mut source_refs = page.source_refs.iter().cloned().collect::<BTreeSet<_>>();
    for evidence_id in &page.evidence_refs {
        if let Some(evidence) = evidence_by_id.get(evidence_id.as_str()) {
            if let Some(source_id) = evidence.source_id.as_deref() {
                if !source_id.trim().is_empty() {
                    source_refs.insert(source_id.to_string());
                }
            }
        }
    }
    let sources_by_id = snapshot
        .sources
        .iter()
        .map(|source| (source.source_id.as_str(), source))
        .collect::<BTreeMap<_, _>>();
    source_refs
        .into_iter()
        .map(|source_ref| {
            if let Some(source) = sources_by_id.get(source_ref.as_str()) {
                format!(
                    "- [{}](../sources/{}.md) _(source: `{}`; markdown: `{}`)_",
                    source.source_id,
                    sanitize_name(&source.source_id),
                    source.source_path,
                    source.markdown_path
                )
            } else {
                format!("- `{source_ref}`")
            }
        })
        .collect()
}

pub(crate) fn source_refs_for_evidence_ids(
    evidence_ids: &[String],
    evidence_by_id: &BTreeMap<&str, &EvidenceRef>,
) -> Vec<String> {
    evidence_ids
        .iter()
        .filter_map(|evidence_id| evidence_by_id.get(evidence_id.as_str()).copied())
        .filter_map(|evidence| {
            evidence
                .source_id
                .as_deref()
                .or(evidence.source_path.as_deref())
        })
        .filter(|source_ref| !source_ref.trim().is_empty())
        .map(ToString::to_string)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub(crate) fn topic_node_wiki_link(
    label: &str,
    node_id: &str,
    current_page_path: &str,
    topic_page_paths: &BTreeMap<&str, &str>,
) -> String {
    let Some(target_path) = topic_page_paths.get(node_id).copied() else {
        return label.to_string();
    };
    if target_path == current_page_path {
        return label.to_string();
    }
    let relative_path = target_path
        .strip_prefix("wiki/topics/")
        .unwrap_or(target_path);
    format!("[{}]({})", label, relative_path)
}

