use std::collections::BTreeSet;

use crate::*;

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
