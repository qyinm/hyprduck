use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::*;

pub(crate) fn parse_provider_workspace_rebuild_snapshot(raw: &str) -> Result<BrainRepoSnapshot> {
    let value = extract_provider_json_value(raw)?;
    if let Ok(payload) = serde_json::from_value::<MaterializedGraphEventPayload>(value.clone()) {
        if let Some(materialized) = payload.materialized_graph {
            return Ok(BrainRepoSnapshot {
                workspace_id: String::new(),
                generated_at: materialized.generated_at.unwrap_or_default(),
                sources: materialized.sources,
                nodes: materialized.nodes,
                relations: materialized.relations,
                evidence: materialized.evidence,
                memories: materialized.memories,
                wiki_pages: materialized.wiki_pages,
                entities: materialized.entities,
                claims: materialized.claims,
                extractions: materialized.extractions,
                events: Vec::new(),
            });
        }
    }
    serde_json::from_value(value).context("failed to decode provider workspace rebuild snapshot")
}

pub(crate) fn normalize_provider_workspace_rebuild_snapshot(
    snapshot: &mut BrainRepoSnapshot,
    workspace_id: &str,
    baseline: &BrainRepoSnapshot,
    generated_at: u64,
) {
    snapshot.workspace_id = workspace_id.to_string();
    snapshot.generated_at = generated_at;
    snapshot.sources = baseline.sources.clone();
    snapshot.evidence = baseline.evidence.clone();
    snapshot.events.clear();
    let source_ids = snapshot
        .sources
        .iter()
        .map(|source| source.source_id.clone())
        .collect::<BTreeSet<_>>();
    let evidence_ids = snapshot
        .evidence
        .iter()
        .map(|evidence| evidence.id.clone())
        .collect::<BTreeSet<_>>();
    for node in &mut snapshot.nodes {
        retain_existing_refs(&mut node.source_ids, &source_ids);
        retain_existing_refs(&mut node.evidence_ids, &evidence_ids);
        if node.updated_at == 0 {
            node.updated_at = generated_at;
        }
    }
    snapshot.nodes.retain(|node| {
        node.node_id.starts_with("source:")
            || !node.source_ids.is_empty()
            || !node.evidence_ids.is_empty()
    });
    ensure_provider_rebuild_source_nodes(snapshot, baseline, generated_at);
    let node_ids = snapshot
        .nodes
        .iter()
        .map(|node| node.node_id.clone())
        .collect::<BTreeSet<_>>();
    for relation in &mut snapshot.relations {
        retain_existing_refs(&mut relation.evidence_ids, &evidence_ids);
        if relation.updated_at == 0 {
            relation.updated_at = generated_at;
        }
    }
    snapshot.relations.retain(|relation| {
        node_ids.contains(&relation.source_node_id)
            && node_ids.contains(&relation.target_node_id)
            && !relation.evidence_ids.is_empty()
    });
    for claim in &mut snapshot.claims {
        claim.workspace_id = workspace_id.to_string();
        retain_existing_refs(&mut claim.topic_refs, &node_ids);
        retain_existing_refs(&mut claim.source_refs, &source_ids);
        retain_existing_refs(&mut claim.evidence_refs, &evidence_ids);
        if claim.updated_at == 0 {
            claim.updated_at = generated_at;
        }
    }
    snapshot.claims.retain(|claim| {
        !claim.topic_refs.is_empty()
            && (!claim.source_refs.is_empty() || !claim.evidence_refs.is_empty())
    });
    for memory in &mut snapshot.memories {
        memory.workspace_id = workspace_id.to_string();
        retain_existing_refs(&mut memory.source_refs, &source_ids);
        retain_existing_refs(&mut memory.evidence_refs, &evidence_ids);
        if memory.created_at == 0 {
            memory.created_at = generated_at;
        }
        if memory.updated_at == 0 {
            memory.updated_at = generated_at;
        }
    }
    snapshot
        .memories
        .retain(|memory| !memory.source_refs.is_empty() || !memory.evidence_refs.is_empty());
    for page in &mut snapshot.wiki_pages {
        page.workspace_id = workspace_id.to_string();
        retain_existing_refs(&mut page.node_refs, &node_ids);
        retain_existing_refs(&mut page.source_refs, &source_ids);
        retain_existing_refs(&mut page.evidence_refs, &evidence_ids);
        if page.updated_at == 0 {
            page.updated_at = generated_at;
        }
    }
    snapshot.wiki_pages.retain(|page| {
        !page.node_refs.is_empty() || !page.source_refs.is_empty() || !page.evidence_refs.is_empty()
    });
}

fn retain_existing_refs(refs: &mut Vec<String>, valid_refs: &BTreeSet<String>) {
    *refs = refs
        .iter()
        .map(|value| value.trim())
        .filter(|value| valid_refs.contains(*value))
        .map(ToString::to_string)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
}

fn ensure_provider_rebuild_source_nodes(
    snapshot: &mut BrainRepoSnapshot,
    baseline: &BrainRepoSnapshot,
    generated_at: u64,
) {
    let mut node_ids = snapshot
        .nodes
        .iter()
        .map(|node| node.node_id.clone())
        .collect::<BTreeSet<_>>();
    let baseline_nodes_by_id = baseline
        .nodes
        .iter()
        .map(|node| (node.node_id.as_str(), node))
        .collect::<BTreeMap<_, _>>();
    for source in &baseline.sources {
        let node_id = format!("source:{}", source.source_id);
        if node_ids.contains(&node_id) {
            continue;
        }
        if let Some(existing) = baseline_nodes_by_id.get(node_id.as_str()) {
            snapshot.nodes.push((*existing).clone());
        } else {
            let evidence_ids = baseline
                .evidence
                .iter()
                .filter(|evidence| evidence.source_id.as_deref() == Some(source.source_id.as_str()))
                .map(|evidence| evidence.id.clone())
                .collect::<Vec<_>>();
            snapshot.nodes.push(BrainNodeRecord {
                node_id: node_id.clone(),
                kind: BrainNodeKind::Source,
                label: source_label_from_record(source),
                scope: BrainScope::Project,
                aliases: Vec::new(),
                evidence_ids,
                source_ids: vec![source.source_id.clone()],
                confidence: Some(1.0),
                updated_at: generated_at,
            });
        }
        node_ids.insert(node_id);
    }
}

fn source_label_from_record(source: &SourceRecord) -> String {
    Path::new(&source.original_path)
        .file_name()
        .or_else(|| Path::new(&source.source_path).file_name())
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| source.source_id.clone())
}

fn extract_provider_json_value(raw: &str) -> Result<Value> {
    let trimmed = raw.trim();
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        return Ok(value);
    }

    let unfenced = trimmed
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    if let Ok(value) = serde_json::from_str::<Value>(unfenced) {
        return Ok(value);
    }

    let starts = raw
        .char_indices()
        .filter_map(|(index, ch)| matches!(ch, '{' | '[').then_some(index))
        .collect::<Vec<_>>();
    let ends = raw
        .char_indices()
        .filter_map(|(index, ch)| matches!(ch, '}' | ']').then_some(index + ch.len_utf8()))
        .rev()
        .collect::<Vec<_>>();
    for start in starts {
        for end in &ends {
            if *end <= start {
                continue;
            }
            if let Ok(value) = serde_json::from_str::<Value>(&raw[start..*end]) {
                return Ok(value);
            }
        }
    }
    bail!("provider graph response did not contain valid JSON")
}
