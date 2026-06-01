use std::collections::BTreeSet;
use std::path::Path;

use anyhow::{bail, Result};
use hyprduck_engine_types::{
    BrainNodeKind, BrainRepoSnapshot, GraphPatch, GraphPatchNode, GRAPH_PATCH_SCHEMA_VERSION,
};

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
    ensure_unique_nonempty("graphPatch.evidenceRefs", &patch.evidence_refs)?;
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
