use std::collections::BTreeSet;
use std::path::Path;

use anyhow::{bail, Result};
use hyprduck_engine_types::{
    BrainNodeKind, BrainRepoSnapshot, GraphPatch, GraphPatchNode, GRAPH_PATCH_SCHEMA_VERSION,
};

#[derive(Debug)]
pub(crate) struct ValidatedGraphPatchScope {
    pub(crate) source_ids: Vec<String>,
    pub(crate) evidence_refs: Vec<String>,
    pub(crate) node_ids: Vec<String>,
    pub(crate) relation_ids: Vec<String>,
    pub(crate) claim_ids: Vec<String>,
    pub(crate) wiki_page_ids: Vec<String>,
}

impl ValidatedGraphPatchScope {
    pub(crate) fn validate_contract(patch: &GraphPatch) -> Result<Self> {
        validate_graph_patch_contract(patch)?;
        Ok(Self {
            source_ids: graph_patch_source_ids(patch),
            evidence_refs: graph_patch_evidence_refs(patch),
            node_ids: patch
                .nodes
                .iter()
                .map(|node| node.node_id.clone())
                .collect(),
            relation_ids: patch
                .relations
                .iter()
                .map(|relation| relation.relation_id.clone())
                .collect(),
            claim_ids: patch
                .claims
                .iter()
                .map(|claim| claim.claim_id.clone())
                .collect(),
            wiki_page_ids: patch
                .wiki_pages
                .iter()
                .map(|page| page.page_id.clone())
                .collect(),
        })
    }

    pub(crate) fn validate_records(
        &self,
        patch: &GraphPatch,
        snapshot: &BrainRepoSnapshot,
    ) -> Result<()> {
        validate_graph_patch_record_refs(patch, &self.source_ids, &self.evidence_refs, snapshot)
    }
}

fn validate_graph_patch_contract(patch: &GraphPatch) -> Result<()> {
    if patch.schema_version != GRAPH_PATCH_SCHEMA_VERSION {
        bail!(
            "unsupported graphPatch.schemaVersion {}; expected {}",
            patch.schema_version,
            GRAPH_PATCH_SCHEMA_VERSION
        );
    }
    if patch.nodes.is_empty()
        && patch.relations.is_empty()
        && patch.claims.is_empty()
        && patch.wiki_pages.is_empty()
    {
        bail!("graphPatch must include at least one node, relation, claim, or wiki page");
    }
    ensure_unique_nonempty("graphPatch.sourceIds", &patch.source_ids)?;
    validate_non_empty_refs("graphPatch.sourceIds", &patch.source_ids)?;
    ensure_unique_nonempty("graphPatch.evidenceRefs", &patch.evidence_refs)?;
    validate_non_empty_refs("graphPatch.evidenceRefs", &patch.evidence_refs)?;
    ensure_unique_nonempty(
        "graphPatch.nodes.nodeId",
        &patch
            .nodes
            .iter()
            .map(|node| node.node_id.clone())
            .collect::<Vec<_>>(),
    )?;
    ensure_unique_nonempty(
        "graphPatch.relations.relationId",
        &patch
            .relations
            .iter()
            .map(|relation| relation.relation_id.clone())
            .collect::<Vec<_>>(),
    )?;
    ensure_unique_nonempty(
        "graphPatch.claims.claimId",
        &patch
            .claims
            .iter()
            .map(|claim| claim.claim_id.clone())
            .collect::<Vec<_>>(),
    )?;
    ensure_unique_nonempty(
        "graphPatch.wikiPages.pageId",
        &patch
            .wiki_pages
            .iter()
            .map(|page| page.page_id.clone())
            .collect::<Vec<_>>(),
    )?;
    for node in &patch.nodes {
        if node.label.trim().is_empty() {
            bail!("graphPatch node {} label cannot be empty", node.node_id);
        }
    }
    for claim in &patch.claims {
        if claim.statement.trim().is_empty() {
            bail!(
                "graphPatch claim {} statement cannot be empty",
                claim.claim_id
            );
        }
        if claim.status.trim().is_empty() {
            bail!("graphPatch claim {} status cannot be empty", claim.claim_id);
        }
    }
    for page in &patch.wiki_pages {
        if page.path.trim().is_empty()
            || Path::new(&page.path).is_absolute()
            || page.path.contains("..")
        {
            bail!(
                "graphPatch wiki page {} path must be workspace-relative",
                page.page_id
            );
        }
        if page.title.trim().is_empty() || page.body.trim().is_empty() {
            bail!(
                "graphPatch wiki page {} title/body cannot be empty",
                page.page_id
            );
        }
    }
    Ok(())
}

fn validate_graph_patch_record_refs(
    patch: &GraphPatch,
    source_ids: &[String],
    evidence_refs: &[String],
    snapshot: &BrainRepoSnapshot,
) -> Result<()> {
    let source_set = source_ids.iter().cloned().collect::<BTreeSet<_>>();
    let evidence_set = evidence_refs.iter().cloned().collect::<BTreeSet<_>>();
    let patch_node_set = patch
        .nodes
        .iter()
        .map(|node| node.node_id.clone())
        .collect::<BTreeSet<_>>();
    let source_node_set = source_ids
        .iter()
        .map(|source_id| format!("source:{source_id}"))
        .collect::<BTreeSet<_>>();
    let existing_in_scope_node_set = snapshot
        .nodes
        .iter()
        .filter(|node| {
            refs_intersect(&node.source_ids, &source_set)
                || refs_intersect(&node.evidence_ids, &evidence_set)
        })
        .map(|node| node.node_id.clone())
        .chain(
            snapshot
                .wiki_pages
                .iter()
                .filter(|page| {
                    refs_intersect(&page.source_refs, &source_set)
                        || refs_intersect(&page.evidence_refs, &evidence_set)
                })
                .map(|page| page.page_id.clone()),
        )
        .collect::<BTreeSet<_>>();
    let node_set = patch_node_set
        .iter()
        .cloned()
        .chain(source_node_set.iter().cloned())
        .chain(existing_in_scope_node_set.iter().cloned())
        .collect::<BTreeSet<_>>();

    reject_out_of_scope_collisions(patch, snapshot, &source_set, &evidence_set)?;

    for node in &patch.nodes {
        if !is_cited_source_node(node, &source_set) {
            validate_non_empty_refs(
                &format!("graphPatch node {} sourceIds", node.node_id),
                &node.source_ids,
            )?;
            validate_non_empty_refs(
                &format!("graphPatch node {} evidenceIds", node.node_id),
                &node.evidence_ids,
            )?;
        }
        validate_ref_subset(
            &format!("graphPatch node {} sourceIds", node.node_id),
            &node.source_ids,
            &source_set,
        )?;
        validate_ref_subset(
            &format!("graphPatch node {} evidenceIds", node.node_id),
            &node.evidence_ids,
            &evidence_set,
        )?;
    }
    for relation in &patch.relations {
        if !node_set.contains(relation.source_node_id.as_str()) {
            bail!(
                "graphPatch relation {} references unknown sourceNodeId {}",
                relation.relation_id,
                relation.source_node_id
            );
        }
        if !node_set.contains(relation.target_node_id.as_str()) {
            bail!(
                "graphPatch relation {} references unknown targetNodeId {}",
                relation.relation_id,
                relation.target_node_id
            );
        }
        validate_non_empty_refs(
            &format!("graphPatch relation {} evidenceIds", relation.relation_id),
            &relation.evidence_ids,
        )?;
        validate_ref_subset(
            &format!("graphPatch relation {} evidenceIds", relation.relation_id),
            &relation.evidence_ids,
            &evidence_set,
        )?;
    }
    for claim in &patch.claims {
        validate_non_empty_refs(
            &format!("graphPatch claim {} sourceRefs", claim.claim_id),
            &claim.source_refs,
        )?;
        validate_non_empty_refs(
            &format!("graphPatch claim {} evidenceRefs", claim.claim_id),
            &claim.evidence_refs,
        )?;
        validate_ref_subset(
            &format!("graphPatch claim {} sourceRefs", claim.claim_id),
            &claim.source_refs,
            &source_set,
        )?;
        validate_ref_subset(
            &format!("graphPatch claim {} evidenceRefs", claim.claim_id),
            &claim.evidence_refs,
            &evidence_set,
        )?;
        for topic_ref in &claim.topic_refs {
            if !node_set.contains(topic_ref.as_str()) {
                bail!(
                    "graphPatch claim {} references unknown topicRef {}",
                    claim.claim_id,
                    topic_ref
                );
            }
        }
    }
    for page in &patch.wiki_pages {
        validate_non_empty_refs(
            &format!("graphPatch wiki page {} sourceRefs", page.page_id),
            &page.source_refs,
        )?;
        validate_non_empty_refs(
            &format!("graphPatch wiki page {} evidenceRefs", page.page_id),
            &page.evidence_refs,
        )?;
        validate_ref_subset(
            &format!("graphPatch wiki page {} sourceRefs", page.page_id),
            &page.source_refs,
            &source_set,
        )?;
        validate_ref_subset(
            &format!("graphPatch wiki page {} evidenceRefs", page.page_id),
            &page.evidence_refs,
            &evidence_set,
        )?;
        for node_ref in &page.node_refs {
            if !node_set.contains(node_ref.as_str()) {
                bail!(
                    "graphPatch wiki page {} references unknown nodeRef {}",
                    page.page_id,
                    node_ref
                );
            }
        }
    }
    Ok(())
}

fn reject_out_of_scope_collisions(
    patch: &GraphPatch,
    snapshot: &BrainRepoSnapshot,
    source_set: &BTreeSet<String>,
    evidence_set: &BTreeSet<String>,
) -> Result<()> {
    for node in &patch.nodes {
        if let Some(existing) = snapshot
            .nodes
            .iter()
            .find(|existing| existing.node_id == node.node_id)
        {
            if !refs_intersect(&existing.source_ids, source_set)
                && !refs_intersect(&existing.evidence_ids, evidence_set)
            {
                bail!(
                    "graphPatch node {} collides with an out-of-scope existing node",
                    node.node_id
                );
            }
        }
    }
    for relation in &patch.relations {
        if let Some(existing) = snapshot
            .relations
            .iter()
            .find(|existing| existing.relation_id == relation.relation_id)
        {
            if !refs_intersect(&existing.evidence_ids, evidence_set) {
                bail!(
                    "graphPatch relation {} collides with an out-of-scope existing relation",
                    relation.relation_id
                );
            }
        }
    }
    for claim in &patch.claims {
        if let Some(existing) = snapshot
            .claims
            .iter()
            .find(|existing| existing.claim_id == claim.claim_id)
        {
            if !refs_intersect(&existing.source_refs, source_set)
                && !refs_intersect(&existing.evidence_refs, evidence_set)
            {
                bail!(
                    "graphPatch claim {} collides with an out-of-scope existing claim",
                    claim.claim_id
                );
            }
        }
    }
    for page in &patch.wiki_pages {
        if let Some(existing) = snapshot
            .wiki_pages
            .iter()
            .find(|existing| existing.page_id == page.page_id || existing.path == page.path)
        {
            if !refs_intersect(&existing.source_refs, source_set)
                && !refs_intersect(&existing.evidence_refs, evidence_set)
            {
                bail!(
                    "graphPatch wiki page {} collides with an out-of-scope existing wiki page",
                    page.page_id
                );
            }
        }
    }
    Ok(())
}

fn is_cited_source_node(node: &GraphPatchNode, source_set: &BTreeSet<String>) -> bool {
    node.kind == BrainNodeKind::Source
        && node
            .node_id
            .strip_prefix("source:")
            .is_some_and(|source_id| source_set.contains(source_id))
}

fn refs_intersect(values: &[String], allowed: &BTreeSet<String>) -> bool {
    values.iter().any(|value| allowed.contains(value))
}

fn validate_non_empty_refs(label: &str, values: &[String]) -> Result<()> {
    if values.is_empty() {
        bail!("{label} must include at least one evidence-scoped ref");
    }
    Ok(())
}

fn graph_patch_source_ids(patch: &GraphPatch) -> Vec<String> {
    unique_strings(
        patch
            .source_ids
            .iter()
            .cloned()
            .chain(patch.nodes.iter().flat_map(|node| node.source_ids.clone()))
            .chain(
                patch
                    .claims
                    .iter()
                    .flat_map(|claim| claim.source_refs.clone()),
            )
            .chain(
                patch
                    .wiki_pages
                    .iter()
                    .flat_map(|page| page.source_refs.clone()),
            )
            .collect(),
    )
}

fn graph_patch_evidence_refs(patch: &GraphPatch) -> Vec<String> {
    unique_strings(
        patch
            .evidence_refs
            .iter()
            .cloned()
            .chain(
                patch
                    .nodes
                    .iter()
                    .flat_map(|node| node.evidence_ids.clone()),
            )
            .chain(
                patch
                    .relations
                    .iter()
                    .flat_map(|relation| relation.evidence_ids.clone()),
            )
            .chain(
                patch
                    .claims
                    .iter()
                    .flat_map(|claim| claim.evidence_refs.clone()),
            )
            .chain(
                patch
                    .wiki_pages
                    .iter()
                    .flat_map(|page| page.evidence_refs.clone()),
            )
            .collect(),
    )
}

fn ensure_unique_nonempty(label: &str, values: &[String]) -> Result<()> {
    let mut seen = BTreeSet::new();
    for value in values {
        if value.trim().is_empty() {
            bail!("{label} contains an empty value");
        }
        if !seen.insert(value.as_str()) {
            bail!("{label} contains duplicate value {value}");
        }
    }
    Ok(())
}

fn validate_ref_subset(label: &str, values: &[String], allowed: &BTreeSet<String>) -> Result<()> {
    for value in values {
        if !allowed.contains(value) {
            bail!("{label} references out-of-scope value {value}");
        }
    }
    Ok(())
}

fn unique_strings(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .filter(|value| !value.trim().is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyprduck_engine_types::{
        BrainNodeRecord, BrainRelationKind, BrainRelationRecord, BrainScope, ClaimRecord,
        EvidenceRef, GraphPatchRelation, GraphPatchWikiPage, WikiPage,
    };

    #[test]
    fn rejects_empty_top_level_scope_even_with_record_refs() {
        let patch = GraphPatch {
            schema_version: GRAPH_PATCH_SCHEMA_VERSION.into(),
            source_ids: Vec::new(),
            evidence_refs: Vec::new(),
            nodes: vec![GraphPatchNode {
                node_id: "node-a".into(),
                kind: BrainNodeKind::Concept,
                label: "Alpha".into(),
                scope: Some(BrainScope::Project),
                aliases: Vec::new(),
                source_ids: vec!["source-a".into()],
                evidence_ids: vec!["ev-a".into()],
            }],
            relations: Vec::new(),
            claims: Vec::new(),
            wiki_pages: Vec::new(),
            agent_metadata: Default::default(),
        };

        let error = ValidatedGraphPatchScope::validate_contract(&patch)
            .expect_err("empty top-level source/evidence scope should fail");

        assert!(error.to_string().contains("graphPatch.sourceIds"));
    }

    #[test]
    fn rejects_out_of_scope_existing_id_collisions() {
        let patch = GraphPatch {
            schema_version: GRAPH_PATCH_SCHEMA_VERSION.into(),
            source_ids: vec!["source-a".into()],
            evidence_refs: vec!["ev-a".into()],
            nodes: vec![GraphPatchNode {
                node_id: "node-b".into(),
                kind: BrainNodeKind::Concept,
                label: "Overwritten".into(),
                scope: Some(BrainScope::Project),
                aliases: Vec::new(),
                source_ids: vec!["source-a".into()],
                evidence_ids: vec!["ev-a".into()],
            }],
            relations: vec![GraphPatchRelation {
                relation_id: "rel-b".into(),
                kind: BrainRelationKind::Mentions,
                source_node_id: "node-b".into(),
                target_node_id: "source:source-a".into(),
                label: String::new(),
                evidence_ids: vec!["ev-a".into()],
            }],
            claims: vec![hyprduck_engine_types::GraphPatchClaim {
                claim_id: "claim-b".into(),
                statement: "Overwritten claim".into(),
                topic_refs: vec!["node-b".into()],
                source_refs: vec!["source-a".into()],
                evidence_refs: vec!["ev-a".into()],
                status: "agent_generated".into(),
            }],
            wiki_pages: vec![GraphPatchWikiPage {
                page_id: "wiki-b".into(),
                path: "wiki/b.md".into(),
                title: "Overwritten wiki".into(),
                body: "Body".into(),
                node_refs: vec!["node-b".into()],
                source_refs: vec!["source-a".into()],
                evidence_refs: vec!["ev-a".into()],
            }],
            agent_metadata: Default::default(),
        };
        let scope = ValidatedGraphPatchScope::validate_contract(&patch).expect("valid contract");
        let snapshot = snapshot_with_out_of_scope_records();

        let error = scope
            .validate_records(&patch, &snapshot)
            .expect_err("out-of-scope ID collisions should fail before upsert");

        assert!(error.to_string().contains("out-of-scope existing"));
    }

    fn snapshot_with_out_of_scope_records() -> BrainRepoSnapshot {
        BrainRepoSnapshot {
            workspace_id: "workspace-default".into(),
            generated_at: 1,
            sources: Vec::new(),
            nodes: vec![BrainNodeRecord {
                node_id: "node-b".into(),
                kind: BrainNodeKind::Concept,
                label: "Beta".into(),
                scope: BrainScope::Project,
                aliases: Vec::new(),
                evidence_ids: vec!["ev-b".into()],
                source_ids: vec!["source-b".into()],
                confidence: None,
                updated_at: 1,
            }],
            relations: vec![BrainRelationRecord {
                relation_id: "rel-b".into(),
                kind: BrainRelationKind::Mentions,
                source_node_id: "node-b".into(),
                target_node_id: "source:source-b".into(),
                label: String::new(),
                evidence_ids: vec!["ev-b".into()],
                confidence: None,
                updated_at: 1,
            }],
            evidence: vec![EvidenceRef {
                id: "ev-b".into(),
                page_label: "1".into(),
                page_index: Some(0),
                snippet: "Beta".into(),
                source_path: None,
                source_id: Some("source-b".into()),
                markdown_path: None,
                image_path: None,
                provenance: None,
            }],
            memories: Vec::new(),
            wiki_pages: vec![WikiPage {
                page_id: "wiki-b".into(),
                workspace_id: "workspace-default".into(),
                path: "wiki/b.md".into(),
                title: "Beta wiki".into(),
                body: "Body".into(),
                node_refs: vec!["node-b".into()],
                source_refs: vec!["source-b".into()],
                evidence_refs: vec!["ev-b".into()],
                updated_at: 1,
            }],
            entities: Vec::new(),
            claims: vec![ClaimRecord {
                claim_id: "claim-b".into(),
                workspace_id: "workspace-default".into(),
                statement: "Beta claim".into(),
                topic_refs: vec!["node-b".into()],
                source_refs: vec!["source-b".into()],
                evidence_refs: vec!["ev-b".into()],
                status: "provider_generated".into(),
                updated_at: 1,
            }],
            extractions: Vec::new(),
            events: Vec::new(),
        }
    }
}
