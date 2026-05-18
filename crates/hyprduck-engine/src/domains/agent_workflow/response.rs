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

#[cfg(test)]
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
        normalize_provider_node_presentation(node, baseline, generated_at);
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

pub(crate) fn normalize_provider_source_local_graph_snapshot(
    snapshot: &mut BrainRepoSnapshot,
    workspace_id: &str,
    baseline: &BrainRepoSnapshot,
    source_id: &str,
    generated_at: u64,
) {
    snapshot.workspace_id = workspace_id.to_string();
    snapshot.generated_at = generated_at;
    snapshot.sources = baseline
        .sources
        .iter()
        .filter(|source| source.source_id == source_id)
        .cloned()
        .collect();
    snapshot.evidence = baseline
        .evidence
        .iter()
        .filter(|evidence| evidence.source_id.as_deref() == Some(source_id))
        .cloned()
        .collect();
    snapshot.events.clear();

    let source_ids = [source_id.to_string()].into_iter().collect::<BTreeSet<_>>();
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
        node.node_id == format!("source:{source_id}")
            || (!node.node_id.starts_with("source:")
                && (!node.source_ids.is_empty() || !node.evidence_ids.is_empty()))
    });
    ensure_source_local_source_node(snapshot, baseline, source_id, generated_at);

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
    ensure_source_local_node_edges(snapshot, source_id, generated_at);

    normalize_supported_records(
        snapshot,
        workspace_id,
        &source_ids,
        &evidence_ids,
        generated_at,
    );
}

pub(crate) fn normalize_provider_workspace_linking_snapshot(
    snapshot: &mut BrainRepoSnapshot,
    workspace_id: &str,
    baseline: &BrainRepoSnapshot,
    source_id: &str,
    generated_at: u64,
) {
    snapshot.workspace_id = workspace_id.to_string();
    snapshot.generated_at = generated_at;
    snapshot.sources = baseline.sources.clone();
    snapshot.evidence = baseline.evidence.clone();
    snapshot.nodes = baseline.nodes.clone();
    for node in &mut snapshot.nodes {
        normalize_provider_node_presentation(node, baseline, generated_at);
    }
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
    let evidence_source_by_id = snapshot
        .evidence
        .iter()
        .filter_map(|evidence| {
            evidence
                .source_id
                .as_ref()
                .map(|source_id| (evidence.id.clone(), source_id.clone()))
        })
        .collect::<BTreeMap<_, _>>();
    let node_source_ids = snapshot
        .nodes
        .iter()
        .map(|node| (node.node_id.clone(), node.source_ids.clone()))
        .collect::<BTreeMap<_, _>>();
    let node_ids = node_source_ids.keys().cloned().collect::<BTreeSet<_>>();
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
            && is_cross_source_relation(relation, &node_source_ids, source_id)
            && relation_evidence_covers_endpoint_sources(
                relation,
                &node_source_ids,
                &evidence_source_by_id,
            )
    });
    normalize_supported_records(
        snapshot,
        workspace_id,
        &source_ids,
        &evidence_ids,
        generated_at,
    );
    let node_ids = snapshot
        .nodes
        .iter()
        .map(|node| node.node_id.clone())
        .collect::<BTreeSet<_>>();
    snapshot.claims.retain(|claim| {
        !claim.topic_refs.is_empty()
            && claim
                .topic_refs
                .iter()
                .all(|node_id| node_ids.contains(node_id))
            && claim
                .source_refs
                .iter()
                .any(|candidate| candidate == source_id)
            && claim
                .source_refs
                .iter()
                .any(|candidate| candidate != source_id)
    });
    snapshot.memories.retain(|memory| {
        memory
            .source_refs
            .iter()
            .any(|candidate| candidate == source_id)
            && memory
                .source_refs
                .iter()
                .any(|candidate| candidate != source_id)
    });
    snapshot.wiki_pages.retain(|page| {
        page.source_refs
            .iter()
            .any(|candidate| candidate == source_id)
            && page
                .source_refs
                .iter()
                .any(|candidate| candidate != source_id)
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

fn ensure_source_local_source_node(
    snapshot: &mut BrainRepoSnapshot,
    baseline: &BrainRepoSnapshot,
    source_id: &str,
    generated_at: u64,
) {
    let node_id = format!("source:{source_id}");
    if snapshot.nodes.iter().any(|node| node.node_id == node_id) {
        return;
    }
    if let Some(existing) = baseline.nodes.iter().find(|node| node.node_id == node_id) {
        snapshot.nodes.push(existing.clone());
        return;
    }
    if let Some(source) = baseline
        .sources
        .iter()
        .find(|source| source.source_id == source_id)
    {
        let evidence_ids = baseline
            .evidence
            .iter()
            .filter(|evidence| evidence.source_id.as_deref() == Some(source_id))
            .map(|evidence| evidence.id.clone())
            .collect::<Vec<_>>();
        snapshot.nodes.push(BrainNodeRecord {
            node_id,
            kind: BrainNodeKind::Source,
            label: source_label_from_record(source),
            scope: BrainScope::Project,
            aliases: Vec::new(),
            evidence_ids,
            source_ids: vec![source_id.to_string()],
            confidence: Some(1.0),
            updated_at: generated_at,
        });
    }
}

fn normalize_provider_node_presentation(
    node: &mut BrainNodeRecord,
    baseline: &BrainRepoSnapshot,
    generated_at: u64,
) {
    node.scope = BrainScope::Project;
    if node.kind == BrainNodeKind::Source {
        let source_id = node
            .node_id
            .strip_prefix("source:")
            .map(str::to_string)
            .or_else(|| node.source_ids.first().cloned());
        if let Some(source_id) = source_id {
            if let Some(source) = baseline
                .sources
                .iter()
                .find(|source| source.source_id == source_id)
            {
                node.label = source_label_from_record(source);
                node.source_ids = vec![source.source_id.clone()];
                node.evidence_ids = baseline
                    .evidence
                    .iter()
                    .filter(|evidence| evidence.source_id.as_deref() == Some(source_id.as_str()))
                    .map(|evidence| evidence.id.clone())
                    .collect();
                if node.aliases.is_empty() {
                    node.aliases.push("Workspace source".into());
                }
                if node.updated_at == 0 {
                    node.updated_at = generated_at;
                }
            }
        }
        return;
    }

    if is_machine_label(&node.label, &node.node_id) {
        if let Some(alias) = node
            .aliases
            .iter()
            .map(|alias| alias.trim())
            .find(|alias| !alias.is_empty() && !is_machine_label(alias, &node.node_id))
        {
            node.label = alias.to_string();
        } else {
            node.label = readable_label_from_node_id(&node.node_id);
        }
    }
}

fn is_machine_label(label: &str, node_id: &str) -> bool {
    let label = label.trim();
    label.is_empty()
        || label == node_id
        || label.starts_with("source-")
        || label.starts_with("concept:")
        || label.starts_with("source:")
}

fn readable_label_from_node_id(node_id: &str) -> String {
    let suffix = node_id
        .rsplit_once(':')
        .map(|(_, suffix)| suffix)
        .unwrap_or(node_id);
    suffix
        .split(['-', '_'])
        .filter(|part| !part.trim().is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn ensure_source_local_node_edges(
    snapshot: &mut BrainRepoSnapshot,
    source_id: &str,
    generated_at: u64,
) {
    let source_node_id = format!("source:{source_id}");
    let mut existing_targets = snapshot
        .relations
        .iter()
        .filter(|relation| relation.source_node_id == source_node_id)
        .map(|relation| relation.target_node_id.clone())
        .collect::<BTreeSet<_>>();
    let fallback_evidence_ids = snapshot
        .evidence
        .iter()
        .map(|evidence| evidence.id.clone())
        .collect::<Vec<_>>();
    let mut relations = Vec::new();
    for node in &snapshot.nodes {
        if node.node_id == source_node_id || existing_targets.contains(&node.node_id) {
            continue;
        }
        let evidence_ids = if node.evidence_ids.is_empty() {
            fallback_evidence_ids.clone()
        } else {
            node.evidence_ids.clone()
        };
        if evidence_ids.is_empty() {
            continue;
        }
        let relation_id = format!(
            "rel-source_of-{}-{}",
            sanitize_name(source_id),
            sanitize_name(&node.node_id)
        );
        relations.push(BrainRelationRecord {
            relation_id,
            kind: BrainRelationKind::SourceOf,
            source_node_id: source_node_id.clone(),
            target_node_id: node.node_id.clone(),
            label: "source_of".into(),
            evidence_ids,
            confidence: Some(1.0),
            updated_at: generated_at,
        });
        existing_targets.insert(node.node_id.clone());
    }
    snapshot.relations.extend(relations);
}

fn normalize_supported_records(
    snapshot: &mut BrainRepoSnapshot,
    workspace_id: &str,
    source_ids: &BTreeSet<String>,
    evidence_ids: &BTreeSet<String>,
    generated_at: u64,
) {
    let node_ids = snapshot
        .nodes
        .iter()
        .map(|node| node.node_id.clone())
        .collect::<BTreeSet<_>>();
    for claim in &mut snapshot.claims {
        claim.workspace_id = workspace_id.to_string();
        retain_existing_refs(&mut claim.topic_refs, &node_ids);
        retain_existing_refs(&mut claim.source_refs, source_ids);
        retain_existing_refs(&mut claim.evidence_refs, evidence_ids);
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
        retain_existing_refs(&mut memory.source_refs, source_ids);
        retain_existing_refs(&mut memory.evidence_refs, evidence_ids);
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
        retain_existing_refs(&mut page.source_refs, source_ids);
        retain_existing_refs(&mut page.evidence_refs, evidence_ids);
        if page.updated_at == 0 {
            page.updated_at = generated_at;
        }
    }
    snapshot.wiki_pages.retain(|page| {
        !page.node_refs.is_empty() || !page.source_refs.is_empty() || !page.evidence_refs.is_empty()
    });
}

fn is_cross_source_relation(
    relation: &BrainRelationRecord,
    node_source_ids: &BTreeMap<String, Vec<String>>,
    imported_source_id: &str,
) -> bool {
    let left = node_source_ids
        .get(&relation.source_node_id)
        .cloned()
        .unwrap_or_default();
    let right = node_source_ids
        .get(&relation.target_node_id)
        .cloned()
        .unwrap_or_default();
    let left_has_import = left.iter().any(|source_id| source_id == imported_source_id);
    let right_has_import = right
        .iter()
        .any(|source_id| source_id == imported_source_id);
    let left_has_other = left.iter().any(|source_id| source_id != imported_source_id);
    let right_has_other = right
        .iter()
        .any(|source_id| source_id != imported_source_id);
    let left_import_only = left_has_import && !left_has_other;
    let right_import_only = right_has_import && !right_has_other;
    let left_other_only = left_has_other && !left_has_import;
    let right_other_only = right_has_other && !right_has_import;
    (left_import_only && right_other_only) || (right_import_only && left_other_only)
}

fn relation_evidence_covers_endpoint_sources(
    relation: &BrainRelationRecord,
    node_source_ids: &BTreeMap<String, Vec<String>>,
    evidence_source_by_id: &BTreeMap<String, String>,
) -> bool {
    let left = node_source_ids
        .get(&relation.source_node_id)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let right = node_source_ids
        .get(&relation.target_node_id)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    relation_evidence_covers_any_source(&relation.evidence_ids, left, evidence_source_by_id)
        && relation_evidence_covers_any_source(&relation.evidence_ids, right, evidence_source_by_id)
}

fn relation_evidence_covers_any_source(
    evidence_ids: &[String],
    source_ids: &[String],
    evidence_source_by_id: &BTreeMap<String, String>,
) -> bool {
    evidence_ids.iter().any(|evidence_id| {
        evidence_source_by_id
            .get(evidence_id)
            .is_some_and(|evidence_source_id| {
                source_ids
                    .iter()
                    .any(|source_id| source_id == evidence_source_id)
            })
    })
}

#[cfg(test)]
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
