use std::collections::{BTreeMap, BTreeSet};

use crate::*;

#[cfg(test)]
pub(crate) fn validate_provider_workspace_rebuild_snapshot(
    snapshot: &BrainRepoSnapshot,
    baseline: &BrainRepoSnapshot,
) -> Result<()> {
    let source_ids = snapshot
        .sources
        .iter()
        .map(|source| source.source_id.clone())
        .collect::<BTreeSet<_>>();
    let baseline_source_ids = baseline
        .sources
        .iter()
        .map(|source| source.source_id.clone())
        .collect::<BTreeSet<_>>();
    if source_ids != baseline_source_ids {
        bail!("workspace rebuild must preserve exactly the existing source records");
    }
    let evidence_ids = snapshot
        .evidence
        .iter()
        .map(|evidence| evidence.id.clone())
        .collect::<BTreeSet<_>>();
    let node_ids = snapshot
        .nodes
        .iter()
        .map(|node| node.node_id.clone())
        .collect::<BTreeSet<_>>();

    for source_id in &source_ids {
        let source_node_id = format!("source:{source_id}");
        if !node_ids.contains(&source_node_id) {
            bail!("workspace rebuild missing source node {source_node_id}");
        }
    }
    for node in &snapshot.nodes {
        let missing_sources = missing_refs(&node.source_ids, &source_ids);
        let missing_evidence = missing_refs(&node.evidence_ids, &evidence_ids);
        if !missing_sources.is_empty() || !missing_evidence.is_empty() {
            bail!(
                "node {} has missing source/evidence refs: sources={} evidence={}",
                node.node_id,
                join_or_none(&missing_sources),
                join_or_none(&missing_evidence)
            );
        }
    }
    for relation in &snapshot.relations {
        let endpoints = vec![
            relation.source_node_id.clone(),
            relation.target_node_id.clone(),
        ];
        let missing_nodes = missing_refs(&endpoints, &node_ids);
        let missing_evidence = missing_refs(&relation.evidence_ids, &evidence_ids);
        if !missing_nodes.is_empty() || !missing_evidence.is_empty() {
            bail!(
                "relation {} has missing refs: nodes={} evidence={}",
                relation.relation_id,
                join_or_none(&missing_nodes),
                join_or_none(&missing_evidence)
            );
        }
    }
    for claim in &snapshot.claims {
        let missing_nodes = missing_refs(&claim.topic_refs, &node_ids);
        let missing_sources = missing_refs(&claim.source_refs, &source_ids);
        let missing_evidence = missing_refs(&claim.evidence_refs, &evidence_ids);
        if !missing_nodes.is_empty() || !missing_sources.is_empty() || !missing_evidence.is_empty()
        {
            bail!(
                "claim {} has missing refs: nodes={} sources={} evidence={}",
                claim.claim_id,
                join_or_none(&missing_nodes),
                join_or_none(&missing_sources),
                join_or_none(&missing_evidence)
            );
        }
    }
    for memory in &snapshot.memories {
        let missing_sources = missing_refs(&memory.source_refs, &source_ids);
        let missing_evidence = missing_refs(&memory.evidence_refs, &evidence_ids);
        if !missing_sources.is_empty() || !missing_evidence.is_empty() {
            bail!(
                "memory {} has missing refs: sources={} evidence={}",
                memory.memory_id,
                join_or_none(&missing_sources),
                join_or_none(&missing_evidence)
            );
        }
    }
    for page in &snapshot.wiki_pages {
        let missing_nodes = missing_refs(&page.node_refs, &node_ids);
        let missing_sources = missing_refs(&page.source_refs, &source_ids);
        let missing_evidence = missing_refs(&page.evidence_refs, &evidence_ids);
        if !missing_nodes.is_empty() || !missing_sources.is_empty() || !missing_evidence.is_empty()
        {
            bail!(
                "wiki page {} has missing refs: nodes={} sources={} evidence={}",
                page.path,
                join_or_none(&missing_nodes),
                join_or_none(&missing_sources),
                join_or_none(&missing_evidence)
            );
        }
    }
    let report = lint_brain_snapshot(snapshot);
    if report.issue_count > 0 {
        bail!(
            "workspace rebuild snapshot failed lint with {} issue(s): {}",
            report.issue_count,
            report
                .issues
                .iter()
                .take(5)
                .map(|issue| issue.title.clone())
                .collect::<Vec<_>>()
                .join("; ")
        );
    }
    Ok(())
}

pub(crate) fn validate_provider_source_local_graph_snapshot(
    snapshot: &BrainRepoSnapshot,
    source_id: &str,
) -> Result<()> {
    let source_ids = snapshot
        .sources
        .iter()
        .map(|source| source.source_id.clone())
        .collect::<BTreeSet<_>>();
    if source_ids != [source_id.to_string()].into_iter().collect::<BTreeSet<_>>() {
        bail!("source graph must contain only imported source {source_id}");
    }
    let evidence_ids = snapshot
        .evidence
        .iter()
        .map(|evidence| evidence.id.clone())
        .collect::<BTreeSet<_>>();
    let node_ids = snapshot
        .nodes
        .iter()
        .map(|node| node.node_id.clone())
        .collect::<BTreeSet<_>>();
    let source_node_id = format!("source:{source_id}");
    if !node_ids.contains(&source_node_id) {
        bail!("source graph missing source node {source_node_id}");
    }
    for node in &snapshot.nodes {
        let missing_sources = missing_refs(&node.source_ids, &source_ids);
        let missing_evidence = missing_refs(&node.evidence_ids, &evidence_ids);
        if !missing_sources.is_empty() || !missing_evidence.is_empty() {
            bail!(
                "node {} has missing source/evidence refs: sources={} evidence={}",
                node.node_id,
                join_or_none(&missing_sources),
                join_or_none(&missing_evidence)
            );
        }
        if node.node_id != source_node_id
            && node.source_ids.is_empty()
            && node.evidence_ids.is_empty()
        {
            bail!("source graph node {} is not grounded", node.node_id);
        }
    }
    for node in snapshot
        .nodes
        .iter()
        .filter(|node| node.node_id != source_node_id)
    {
        let connected_to_source = snapshot.relations.iter().any(|relation| {
            relation.source_node_id == source_node_id && relation.target_node_id == node.node_id
        });
        if !connected_to_source {
            bail!(
                "source graph node {} is not linked from source",
                node.node_id
            );
        }
    }
    let evidence_source_by_id = evidence_source_map(snapshot);
    validate_common_refs(
        snapshot,
        &source_ids,
        &evidence_ids,
        &evidence_source_by_id,
    )?;
    let report = lint_brain_snapshot(snapshot);
    if report.issue_count > 0 {
        bail!(
            "source graph snapshot failed lint with {} issue(s): {}",
            report.issue_count,
            report
                .issues
                .iter()
                .take(5)
                .map(|issue| issue.title.clone())
                .collect::<Vec<_>>()
                .join("; ")
        );
    }
    Ok(())
}

pub(crate) fn validate_provider_workspace_linking_snapshot(
    snapshot: &BrainRepoSnapshot,
    baseline: &BrainRepoSnapshot,
    source_id: &str,
) -> Result<()> {
    let source_ids = snapshot
        .sources
        .iter()
        .map(|source| source.source_id.clone())
        .collect::<BTreeSet<_>>();
    let baseline_source_ids = baseline
        .sources
        .iter()
        .map(|source| source.source_id.clone())
        .collect::<BTreeSet<_>>();
    if source_ids != baseline_source_ids {
        bail!("workspace linking must preserve exactly the existing source records");
    }
    let evidence_ids = snapshot
        .evidence
        .iter()
        .map(|evidence| evidence.id.clone())
        .collect::<BTreeSet<_>>();
    let evidence_source_by_id = evidence_source_map(snapshot);
    validate_common_refs(
        snapshot,
        &source_ids,
        &evidence_ids,
        &evidence_source_by_id,
    )?;

    let node_sources = snapshot
        .nodes
        .iter()
        .map(|node| (node.node_id.as_str(), node.source_ids.as_slice()))
        .collect::<Vec<_>>();
    for relation in &snapshot.relations {
        let left = node_sources
            .iter()
            .find(|(node_id, _)| *node_id == relation.source_node_id)
            .map(|(_, sources)| *sources)
            .unwrap_or(&[]);
        let right = node_sources
            .iter()
            .find(|(node_id, _)| *node_id == relation.target_node_id)
            .map(|(_, sources)| *sources)
            .unwrap_or(&[]);
        let left_has_import = left.iter().any(|candidate| candidate == source_id);
        let right_has_import = right.iter().any(|candidate| candidate == source_id);
        let left_has_other = left.iter().any(|candidate| candidate != source_id);
        let right_has_other = right.iter().any(|candidate| candidate != source_id);
        if !((left_has_import && right_has_other) || (right_has_import && left_has_other)) {
            bail!(
                "workspace linking relation {} is not cross-source",
                relation.relation_id
            );
        }
        if !evidence_covers_any_source(&relation.evidence_ids, left, &evidence_source_by_id)
            || !evidence_covers_any_source(&relation.evidence_ids, right, &evidence_source_by_id)
        {
            bail!(
                "workspace linking relation {} evidence does not cover both endpoint source sides",
                relation.relation_id
            );
        }
    }
    let report = lint_brain_snapshot(snapshot);
    if report.issue_count > 0 {
        bail!(
            "workspace linking snapshot failed lint with {} issue(s): {}",
            report.issue_count,
            report
                .issues
                .iter()
                .take(5)
                .map(|issue| issue.title.clone())
                .collect::<Vec<_>>()
                .join("; ")
        );
    }
    Ok(())
}

fn validate_common_refs(
    snapshot: &BrainRepoSnapshot,
    source_ids: &BTreeSet<String>,
    evidence_ids: &BTreeSet<String>,
    evidence_source_by_id: &BTreeMap<String, String>,
) -> Result<()> {
    let node_ids = snapshot
        .nodes
        .iter()
        .map(|node| node.node_id.clone())
        .collect::<BTreeSet<_>>();
    for relation in &snapshot.relations {
        let endpoints = vec![
            relation.source_node_id.clone(),
            relation.target_node_id.clone(),
        ];
        let missing_nodes = missing_refs(&endpoints, &node_ids);
        let missing_evidence = missing_refs(&relation.evidence_ids, evidence_ids);
        if !missing_nodes.is_empty() || !missing_evidence.is_empty() {
            bail!(
                "relation {} has missing refs: nodes={} evidence={}",
                relation.relation_id,
                join_or_none(&missing_nodes),
                join_or_none(&missing_evidence)
            );
        }
    }
    for claim in &snapshot.claims {
        let missing_nodes = missing_refs(&claim.topic_refs, &node_ids);
        let missing_sources = missing_refs(&claim.source_refs, source_ids);
        let missing_evidence = missing_refs(&claim.evidence_refs, evidence_ids);
        if !missing_nodes.is_empty() || !missing_sources.is_empty() || !missing_evidence.is_empty()
        {
            bail!(
                "claim {} has missing refs: nodes={} sources={} evidence={}",
                claim.claim_id,
                join_or_none(&missing_nodes),
                join_or_none(&missing_sources),
                join_or_none(&missing_evidence)
            );
        }
        if !evidence_covers_all_sources(
            &claim.evidence_refs,
            &claim.source_refs,
            evidence_source_by_id,
        ) {
            bail!(
                "claim {} evidence does not cover all source refs",
                claim.claim_id
            );
        }
    }
    for memory in &snapshot.memories {
        let missing_sources = missing_refs(&memory.source_refs, source_ids);
        let missing_evidence = missing_refs(&memory.evidence_refs, evidence_ids);
        if !missing_sources.is_empty() || !missing_evidence.is_empty() {
            bail!(
                "memory {} has missing refs: sources={} evidence={}",
                memory.memory_id,
                join_or_none(&missing_sources),
                join_or_none(&missing_evidence)
            );
        }
        if !evidence_covers_all_sources(
            &memory.evidence_refs,
            &memory.source_refs,
            evidence_source_by_id,
        ) {
            bail!(
                "memory {} evidence does not cover all source refs",
                memory.memory_id
            );
        }
    }
    for page in &snapshot.wiki_pages {
        let missing_nodes = missing_refs(&page.node_refs, &node_ids);
        let missing_sources = missing_refs(&page.source_refs, source_ids);
        let missing_evidence = missing_refs(&page.evidence_refs, evidence_ids);
        if !missing_nodes.is_empty() || !missing_sources.is_empty() || !missing_evidence.is_empty()
        {
            bail!(
                "wiki page {} has missing refs: nodes={} sources={} evidence={}",
                page.path,
                join_or_none(&missing_nodes),
                join_or_none(&missing_sources),
                join_or_none(&missing_evidence)
            );
        }
        if !evidence_covers_all_sources(
            &page.evidence_refs,
            &page.source_refs,
            evidence_source_by_id,
        ) {
            bail!(
                "wiki page {} evidence does not cover all source refs",
                page.path
            );
        }
    }
    Ok(())
}

fn evidence_source_map(snapshot: &BrainRepoSnapshot) -> BTreeMap<String, String> {
    snapshot
        .evidence
        .iter()
        .filter_map(|evidence| {
            evidence
                .source_id
                .as_ref()
                .map(|source_id| (evidence.id.clone(), source_id.clone()))
        })
        .collect()
}

fn evidence_covers_any_source(
    evidence_refs: &[String],
    source_refs: &[String],
    evidence_source_by_id: &BTreeMap<String, String>,
) -> bool {
    source_refs.iter().any(|source_id| {
        evidence_refs
            .iter()
            .any(|evidence_id| evidence_source_by_id.get(evidence_id) == Some(source_id))
    })
}

fn evidence_covers_all_sources(
    evidence_refs: &[String],
    source_refs: &[String],
    evidence_source_by_id: &BTreeMap<String, String>,
) -> bool {
    source_refs.iter().all(|source_id| {
        evidence_covers_any_source(
            evidence_refs,
            std::slice::from_ref(source_id),
            evidence_source_by_id,
        )
    })
}
