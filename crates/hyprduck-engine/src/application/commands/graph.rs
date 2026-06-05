use std::collections::{BTreeMap, BTreeSet};

use anyhow::{anyhow, bail, Context, Result};
use hyprduck_engine_types::{
    ApplyGraphPatchRequest, ApplyGraphPatchResponseData, BrainActor, BrainActorType, BrainEvent,
    BrainEventCausality, BrainEventKind, BrainNodeRecord, BrainRelationRecord, BrainScope,
    ClaimRecord, PolicyResult, WikiPage, BRAIN_EVENT_SCHEMA_VERSION, GRAPH_PATCH_SCHEMA_VERSION,
};
use serde_json::json;
use uuid::Uuid;

use crate::brain_repo::{read_materialized_brain_snapshot, BrainWorkspaceWriter};
use crate::graph_commit::commit_graph_materialization;
use crate::graph_patch_policy::ValidatedGraphPatchScope;
use crate::{resolve_brain_workspace_root, unix_timestamp_seconds, KnowledgeStore};

pub(crate) fn handle_apply_graph_patch(
    request: ApplyGraphPatchRequest,
) -> Result<ApplyGraphPatchResponseData> {
    let root = resolve_brain_workspace_root(&request.scope)?;
    let writer = BrainWorkspaceWriter::open(root.clone())?;
    let mut snapshot = read_materialized_brain_snapshot(&root, &request.scope.workspace_id)?;
    let store = KnowledgeStore::open(KnowledgeStore::default_path_for_root(&root))?;

    let validated_patch = ValidatedGraphPatchScope::validate_contract(&request.graph_patch)?;
    if validated_patch.source_ids.is_empty() {
        bail!("graphPatch.sourceIds must contain at least one source ID");
    }

    let mut source_records = Vec::new();
    let mut evidence_by_id = BTreeMap::new();
    for source_id in &validated_patch.source_ids {
        let response = store
            .read_source_from_db(&request.scope.workspace_id, source_id, false)?
            .ok_or_else(|| anyhow!("graphPatch references unknown sourceId {source_id}"))?;
        source_records.push(response.source);
        for evidence in response.evidence {
            evidence_by_id.insert(evidence.id.clone(), evidence);
        }
    }

    if validated_patch.evidence_refs.is_empty() {
        bail!("graphPatch must reference at least one evidence ref");
    }
    for evidence_ref in &validated_patch.evidence_refs {
        if !evidence_by_id.contains_key(evidence_ref) {
            bail!("graphPatch references unknown or out-of-scope evidence ref {evidence_ref}");
        }
    }

    validated_patch.validate_records(&request.graph_patch, &snapshot)?;

    let now = unix_timestamp_seconds();
    let node_ids = validated_patch.node_ids.clone();
    let relation_ids = validated_patch.relation_ids.clone();
    let claim_ids = validated_patch.claim_ids.clone();
    let wiki_page_ids = validated_patch.wiki_page_ids.clone();

    upsert_by_id(&mut snapshot.sources, source_records, |source| {
        source.source_id.clone()
    });
    upsert_by_id(
        &mut snapshot.evidence,
        evidence_by_id.into_values().collect(),
        |evidence| evidence.id.clone(),
    );
    let existing_nodes = snapshot
        .nodes
        .iter()
        .map(|node| (node.node_id.clone(), node.clone()))
        .collect::<BTreeMap<_, _>>();
    let existing_relations = snapshot
        .relations
        .iter()
        .map(|relation| (relation.relation_id.clone(), relation.clone()))
        .collect::<BTreeMap<_, _>>();
    let existing_claims = snapshot
        .claims
        .iter()
        .map(|claim| (claim.claim_id.clone(), claim.clone()))
        .collect::<BTreeMap<_, _>>();
    let existing_wiki_pages = snapshot
        .wiki_pages
        .iter()
        .map(|page| (page.page_id.clone(), page.clone()))
        .collect::<BTreeMap<_, _>>();

    upsert_by_id(
        &mut snapshot.nodes,
        request
            .graph_patch
            .nodes
            .iter()
            .map(|node| {
                let existing = existing_nodes.get(&node.node_id);
                BrainNodeRecord {
                    node_id: node.node_id.clone(),
                    kind: node.kind,
                    label: node.label.clone(),
                    scope: node
                        .scope
                        .or_else(|| existing.map(|record| record.scope))
                        .unwrap_or(BrainScope::Project),
                    aliases: merge_strings(existing.map(|record| &record.aliases), &node.aliases),
                    evidence_ids: merge_strings(
                        existing.map(|record| &record.evidence_ids),
                        &node.evidence_ids,
                    ),
                    source_ids: merge_strings(
                        existing.map(|record| &record.source_ids),
                        &node.source_ids,
                    ),
                    confidence: existing.and_then(|record| record.confidence),
                    updated_at: now,
                    valid_from: existing.map(|record| record.valid_from).unwrap_or(0),
                    valid_to: existing.and_then(|record| record.valid_to),
                    superseded_by: existing.and_then(|record| record.superseded_by.clone()),
                }
            })
            .collect(),
        |node| node.node_id.clone(),
    );
    upsert_by_id(
        &mut snapshot.relations,
        request
            .graph_patch
            .relations
            .iter()
            .map(|relation| {
                let existing = existing_relations.get(&relation.relation_id);
                BrainRelationRecord {
                    relation_id: relation.relation_id.clone(),
                    kind: relation.kind,
                    source_node_id: relation.source_node_id.clone(),
                    target_node_id: relation.target_node_id.clone(),
                    label: relation.label.clone(),
                    evidence_ids: merge_strings(
                        existing.map(|record| &record.evidence_ids),
                        &relation.evidence_ids,
                    ),
                    confidence: existing.and_then(|record| record.confidence),
                    updated_at: now,
                    valid_from: existing.map(|record| record.valid_from).unwrap_or(0),
                    valid_to: existing.and_then(|record| record.valid_to),
                    superseded_by: existing.and_then(|record| record.superseded_by.clone()),
                }
            })
            .collect(),
        |relation| relation.relation_id.clone(),
    );
    upsert_by_id(
        &mut snapshot.claims,
        request
            .graph_patch
            .claims
            .iter()
            .map(|claim| {
                let existing = existing_claims.get(&claim.claim_id);
                ClaimRecord {
                    claim_id: claim.claim_id.clone(),
                    workspace_id: request.scope.workspace_id.clone(),
                    statement: claim.statement.clone(),
                    topic_refs: merge_strings(
                        existing.map(|record| &record.topic_refs),
                        &claim.topic_refs,
                    ),
                    source_refs: merge_strings(
                        existing.map(|record| &record.source_refs),
                        &claim.source_refs,
                    ),
                    evidence_refs: merge_strings(
                        existing.map(|record| &record.evidence_refs),
                        &claim.evidence_refs,
                    ),
                    status: claim.status.clone(),
                    updated_at: now,
                }
            })
            .collect(),
        |claim| claim.claim_id.clone(),
    );
    upsert_by_id(
        &mut snapshot.wiki_pages,
        request
            .graph_patch
            .wiki_pages
            .iter()
            .map(|page| {
                let existing = existing_wiki_pages.get(&page.page_id);
                WikiPage {
                    page_id: page.page_id.clone(),
                    workspace_id: request.scope.workspace_id.clone(),
                    path: page.path.clone(),
                    title: page.title.clone(),
                    body: page.body.clone(),
                    node_refs: merge_strings(
                        existing.map(|record| &record.node_refs),
                        &page.node_refs,
                    ),
                    source_refs: merge_strings(
                        existing.map(|record| &record.source_refs),
                        &page.source_refs,
                    ),
                    evidence_refs: merge_strings(
                        existing.map(|record| &record.evidence_refs),
                        &page.evidence_refs,
                    ),
                    updated_at: now,
                }
            })
            .collect(),
        |page| page.page_id.clone(),
    );

    let event_id = format!("evt-graph-patch-{}", Uuid::now_v7().as_simple());
    let event = BrainEvent {
        event_id: event_id.clone(),
        schema_version: BRAIN_EVENT_SCHEMA_VERSION,
        workspace_id: request.scope.workspace_id.clone(),
        scope: BrainScope::Project,
        event_type: BrainEventKind::GraphMaterialized,
        operation_type: Some("agent_graph_patch_apply".into()),
        actor: BrainActor {
            actor_type: BrainActorType::Agent,
            actor_id: request
                .agent_id
                .filter(|agent_id| !agent_id.trim().is_empty())
                .unwrap_or_else(|| "hyprduck-agent-graph-patch".into()),
        },
        source_refs: validated_patch.source_ids.clone(),
        source_markdown_refs: Vec::new(),
        node_refs: node_ids.clone(),
        relation_refs: relation_ids.clone(),
        claim_refs: claim_ids.clone(),
        memory_refs: Vec::new(),
        target_node_ids: node_ids.clone(),
        target_edge_ids: relation_ids.clone(),
        target_claim_ids: claim_ids.clone(),
        target_memory_ids: Vec::new(),
        evidence_refs: validated_patch.evidence_refs.clone(),
        payload_json: serde_json::to_string(&json!({
            "schemaVersion": GRAPH_PATCH_SCHEMA_VERSION,
            "agentMetadata": request.graph_patch.agent_metadata,
            "nodeCount": node_ids.len(),
            "relationCount": relation_ids.len(),
            "claimCount": claim_ids.len(),
            "wikiPageCount": wiki_page_ids.len()
        }))
        .context("failed encoding graph patch event payload")?,
        causality: BrainEventCausality {
            caused_by_source_ids: validated_patch.source_ids.clone(),
            snapshot_id: Some(format!("snapshot-{}-{now}", request.scope.workspace_id)),
            materialized_version: Some(now),
            ..Default::default()
        },
        confidence: None,
        policy_result: PolicyResult::accepted(),
        created_at: now,
    };
    snapshot.generated_at = now;
    snapshot.events.push(event.clone());

    let report = commit_graph_materialization(writer.root(), &store, &snapshot)?;

    Ok(ApplyGraphPatchResponseData {
        event_id,
        status: "applied".into(),
        graph_ready: true,
        graph_status: "ready".into(),
        applied_at: now,
        source_ids: validated_patch.source_ids,
        evidence_refs: validated_patch.evidence_refs,
        changed_node_ids: node_ids,
        changed_relation_ids: relation_ids,
        changed_claim_ids: claim_ids,
        changed_wiki_page_ids: wiki_page_ids,
        warnings: if report.node_count == 0 {
            vec!["graph patch applied without materialized graph nodes".into()]
        } else {
            Vec::new()
        },
    })
}

fn merge_strings(existing: Option<&Vec<String>>, incoming: &[String]) -> Vec<String> {
    existing
        .into_iter()
        .flat_map(|values| values.iter())
        .chain(incoming.iter())
        .filter(|value| !value.trim().is_empty())
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn upsert_by_id<T, F>(target: &mut Vec<T>, values: Vec<T>, id: F)
where
    F: Fn(&T) -> String,
{
    let mut by_id = target
        .drain(..)
        .map(|value| (id(&value), value))
        .collect::<BTreeMap<_, _>>();
    for value in values {
        by_id.insert(id(&value), value);
    }
    target.extend(by_id.into_values());
}
