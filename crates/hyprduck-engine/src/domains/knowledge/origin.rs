use super::*;

pub(super) const MATERIALIZED_RECORD_ORIGINS_PATH: &str = "state/materialized-record-origins.json";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MaterializedRecordOrigins {
    #[serde(default)]
    pub(super) schema_version: u32,
    #[serde(default)]
    pub(super) relations: BTreeMap<String, MaterializedRecordOrigin>,
    #[serde(default)]
    pub(super) claims: BTreeMap<String, MaterializedRecordOrigin>,
    #[serde(default)]
    pub(super) memories: BTreeMap<String, MaterializedRecordOrigin>,
    #[serde(default)]
    pub(super) wiki_pages_by_id: BTreeMap<String, MaterializedRecordOrigin>,
    #[serde(default)]
    pub(super) wiki_pages_by_path: BTreeMap<String, MaterializedRecordOrigin>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MaterializedRecordOrigin {
    event_id: String,
    operation_type: String,
}

impl MaterializedRecordOrigin {
    pub(super) fn from_event(event: &BrainEvent) -> Self {
        Self {
            event_id: event.event_id.clone(),
            operation_type: event
                .operation_type
                .clone()
                .unwrap_or_else(|| "graph_materialized".into()),
        }
    }

    pub(super) fn is_workspace_linking(&self) -> bool {
        self.operation_type == "workspace_linking"
    }
}

pub(super) fn read_materialized_record_origins(root: &Path) -> Result<MaterializedRecordOrigins> {
    let path = root.join(MATERIALIZED_RECORD_ORIGINS_PATH);
    if !path.exists() {
        return Ok(MaterializedRecordOrigins {
            schema_version: BRAIN_EVENT_SCHEMA_VERSION,
            ..Default::default()
        });
    }
    let mut origins: MaterializedRecordOrigins =
        read_json_artifact(&path).with_context(|| format!("failed reading {}", path.display()))?;
    origins.schema_version = BRAIN_EVENT_SCHEMA_VERSION;
    Ok(origins)
}

pub(super) fn merge_bootstrapped_materialized_record_origins(
    mut origins: MaterializedRecordOrigins,
    snapshot: &BrainRepoSnapshot,
    generated_snapshot: &BrainRepoSnapshot,
) -> MaterializedRecordOrigins {
    let bootstrapped = bootstrap_legacy_workspace_linking_origins(snapshot);
    let generated_valid_source_ids = generated_snapshot
        .sources
        .iter()
        .map(|source| source.source_id.clone())
        .collect::<BTreeSet<_>>();
    let generated_valid_node_ids = generated_snapshot
        .nodes
        .iter()
        .map(|node| node.node_id.clone())
        .collect::<BTreeSet<_>>();
    let generated_valid_evidence_ids = generated_snapshot
        .evidence
        .iter()
        .map(|evidence| evidence.id.clone())
        .collect::<BTreeSet<_>>();
    let generated_relation_ids = generated_snapshot
        .relations
        .iter()
        .filter(|relation| {
            relation_refs_are_current(
                relation,
                &generated_valid_node_ids,
                &generated_valid_evidence_ids,
            )
        })
        .map(|relation| relation.relation_id.as_str())
        .collect::<BTreeSet<_>>();
    let generated_claim_ids = generated_snapshot
        .claims
        .iter()
        .filter(|claim| {
            claim_refs_are_current(
                claim,
                &generated_valid_source_ids,
                &generated_valid_evidence_ids,
                &generated_valid_node_ids,
            )
        })
        .map(|claim| claim.claim_id.as_str())
        .collect::<BTreeSet<_>>();
    let generated_memory_ids = generated_snapshot
        .memories
        .iter()
        .filter(|memory| {
            source_and_evidence_refs_are_current(
                &memory.source_refs,
                &memory.evidence_refs,
                &generated_valid_source_ids,
                &generated_valid_evidence_ids,
            )
        })
        .map(|memory| memory.memory_id.as_str())
        .collect::<BTreeSet<_>>();
    let generated_wiki_page_ids = generated_snapshot
        .wiki_pages
        .iter()
        .filter(|page| {
            wiki_page_refs_are_current(
                page,
                &generated_valid_source_ids,
                &generated_valid_evidence_ids,
                &generated_valid_node_ids,
            )
        })
        .map(|page| page.page_id.as_str())
        .collect::<BTreeSet<_>>();
    let generated_wiki_paths = generated_snapshot
        .wiki_pages
        .iter()
        .filter(|page| {
            wiki_page_refs_are_current(
                page,
                &generated_valid_source_ids,
                &generated_valid_evidence_ids,
                &generated_valid_node_ids,
            )
        })
        .map(|page| page.path.as_str())
        .collect::<BTreeSet<_>>();
    for (id, origin) in bootstrapped.relations {
        if !generated_relation_ids.contains(id.as_str()) {
            origins.relations.entry(id).or_insert(origin);
        }
    }
    for (id, origin) in bootstrapped.claims {
        if !generated_claim_ids.contains(id.as_str()) {
            origins.claims.entry(id).or_insert(origin);
        }
    }
    for (id, origin) in bootstrapped.memories {
        if !generated_memory_ids.contains(id.as_str()) {
            origins.memories.entry(id).or_insert(origin);
        }
    }
    for (id, origin) in bootstrapped.wiki_pages_by_id {
        if !generated_wiki_page_ids.contains(id.as_str()) {
            origins.wiki_pages_by_id.entry(id).or_insert(origin);
        }
    }
    for (path, origin) in bootstrapped.wiki_pages_by_path {
        if !generated_wiki_paths.contains(path.as_str()) {
            origins.wiki_pages_by_path.entry(path).or_insert(origin);
        }
    }
    origins
}

pub(super) fn write_materialized_record_origins(
    root: &Path,
    origins: &MaterializedRecordOrigins,
) -> Result<()> {
    let mut origins = origins.clone();
    origins.schema_version = BRAIN_EVENT_SCHEMA_VERSION;
    write_json_pretty(&root.join(MATERIALIZED_RECORD_ORIGINS_PATH), &origins)
}

fn bootstrap_legacy_workspace_linking_origins(
    snapshot: &BrainRepoSnapshot,
) -> MaterializedRecordOrigins {
    let valid_source_ids = snapshot
        .sources
        .iter()
        .map(|source| source.source_id.clone())
        .collect::<BTreeSet<_>>();
    let valid_node_ids = snapshot
        .nodes
        .iter()
        .map(|node| node.node_id.clone())
        .collect::<BTreeSet<_>>();
    let valid_evidence_ids = snapshot
        .evidence
        .iter()
        .map(|evidence| evidence.id.clone())
        .collect::<BTreeSet<_>>();
    let mut origins = MaterializedRecordOrigins {
        schema_version: BRAIN_EVENT_SCHEMA_VERSION,
        ..Default::default()
    };
    for event in snapshot.events.iter().filter(|event| {
        event.event_type == BrainEventKind::GraphMaterialized
            && event.operation_type.as_deref() == Some("workspace_linking")
    }) {
        let Ok(payload) =
            serde_json::from_str::<MaterializedGraphEventPayload>(&event.payload_json)
        else {
            continue;
        };
        let Some(materialized_graph) = payload.materialized_graph else {
            continue;
        };
        let origin = MaterializedRecordOrigin::from_event(event);
        for relation in materialized_graph.relations {
            let Some(current) = snapshot
                .relations
                .iter()
                .find(|current| current.relation_id == relation.relation_id)
            else {
                continue;
            };
            if relation_is_legacy_workspace_linking_stale(
                current,
                &valid_node_ids,
                &valid_evidence_ids,
            ) {
                origins
                    .relations
                    .insert(current.relation_id.clone(), origin.clone());
            }
        }
        for claim in materialized_graph.claims {
            let Some(current) = snapshot
                .claims
                .iter()
                .find(|current| current.claim_id == claim.claim_id)
            else {
                continue;
            };
            if claim_is_legacy_workspace_linking_stale(
                current,
                &valid_source_ids,
                &valid_evidence_ids,
                &valid_node_ids,
            ) {
                origins
                    .claims
                    .insert(current.claim_id.clone(), origin.clone());
            }
        }
        for memory in materialized_graph.memories {
            let Some(current) = snapshot
                .memories
                .iter()
                .find(|current| current.memory_id == memory.memory_id)
            else {
                continue;
            };
            if memory_is_legacy_workspace_linking_stale(
                current,
                &valid_source_ids,
                &valid_evidence_ids,
            ) {
                origins
                    .memories
                    .insert(current.memory_id.clone(), origin.clone());
            }
        }
        for page in materialized_graph.wiki_pages {
            let current = snapshot
                .wiki_pages
                .iter()
                .find(|current| current.page_id == page.page_id || current.path == page.path);
            let Some(current) = current else {
                continue;
            };
            if wiki_page_is_legacy_workspace_linking_stale(
                current,
                &valid_source_ids,
                &valid_evidence_ids,
                &valid_node_ids,
            ) {
                origins
                    .wiki_pages_by_id
                    .insert(current.page_id.clone(), origin.clone());
                origins
                    .wiki_pages_by_path
                    .insert(current.path.clone(), origin.clone());
            }
        }
    }
    origins
}

fn relation_is_legacy_workspace_linking_stale(
    relation: &BrainRelationRecord,
    valid_node_ids: &BTreeSet<String>,
    valid_evidence_ids: &BTreeSet<String>,
) -> bool {
    !valid_node_ids.contains(&relation.source_node_id)
        || !valid_node_ids.contains(&relation.target_node_id)
        || relation
            .evidence_ids
            .iter()
            .any(|evidence_id| !valid_evidence_ids.contains(evidence_id))
}

pub(super) fn relation_refs_are_current(
    relation: &BrainRelationRecord,
    valid_node_ids: &BTreeSet<String>,
    valid_evidence_ids: &BTreeSet<String>,
) -> bool {
    valid_node_ids.contains(&relation.source_node_id)
        && valid_node_ids.contains(&relation.target_node_id)
        && relation
            .evidence_ids
            .iter()
            .all(|evidence_id| valid_evidence_ids.contains(evidence_id))
}

fn claim_is_legacy_workspace_linking_stale(
    claim: &ClaimRecord,
    valid_source_ids: &BTreeSet<String>,
    valid_evidence_ids: &BTreeSet<String>,
    valid_node_ids: &BTreeSet<String>,
) -> bool {
    claim
        .source_refs
        .iter()
        .any(|source_id| !valid_source_ids.contains(source_id))
        || claim
            .evidence_refs
            .iter()
            .any(|evidence_id| !valid_evidence_ids.contains(evidence_id))
        || claim
            .topic_refs
            .iter()
            .any(|node_id| !valid_node_ids.contains(node_id))
}

pub(super) fn claim_refs_are_current(
    claim: &ClaimRecord,
    valid_source_ids: &BTreeSet<String>,
    valid_evidence_ids: &BTreeSet<String>,
    valid_node_ids: &BTreeSet<String>,
) -> bool {
    source_and_evidence_refs_are_current(
        &claim.source_refs,
        &claim.evidence_refs,
        valid_source_ids,
        valid_evidence_ids,
    ) && claim
        .topic_refs
        .iter()
        .all(|node_id| valid_node_ids.contains(node_id))
}

fn memory_is_legacy_workspace_linking_stale(
    memory: &MemoryRecord,
    valid_source_ids: &BTreeSet<String>,
    valid_evidence_ids: &BTreeSet<String>,
) -> bool {
    memory
        .source_refs
        .iter()
        .any(|source_id| !valid_source_ids.contains(source_id))
        || memory
            .evidence_refs
            .iter()
            .any(|evidence_id| !valid_evidence_ids.contains(evidence_id))
}

pub(super) fn source_and_evidence_refs_are_current(
    source_refs: &[String],
    evidence_refs: &[String],
    valid_source_ids: &BTreeSet<String>,
    valid_evidence_ids: &BTreeSet<String>,
) -> bool {
    source_refs
        .iter()
        .all(|source_id| valid_source_ids.contains(source_id))
        && evidence_refs
            .iter()
            .all(|evidence_id| valid_evidence_ids.contains(evidence_id))
}

fn wiki_page_is_legacy_workspace_linking_stale(
    page: &WikiPage,
    valid_source_ids: &BTreeSet<String>,
    valid_evidence_ids: &BTreeSet<String>,
    valid_node_ids: &BTreeSet<String>,
) -> bool {
    page.source_refs
        .iter()
        .any(|source_id| !valid_source_ids.contains(source_id))
        || page
            .evidence_refs
            .iter()
            .any(|evidence_id| !valid_evidence_ids.contains(evidence_id))
        || page
            .node_refs
            .iter()
            .any(|node_id| !valid_node_ids.contains(node_id))
}

pub(super) fn wiki_page_refs_are_current(
    page: &WikiPage,
    valid_source_ids: &BTreeSet<String>,
    valid_evidence_ids: &BTreeSet<String>,
    valid_node_ids: &BTreeSet<String>,
) -> bool {
    source_and_evidence_refs_are_current(
        &page.source_refs,
        &page.evidence_refs,
        valid_source_ids,
        valid_evidence_ids,
    ) && page
        .node_refs
        .iter()
        .all(|node_id| valid_node_ids.contains(node_id))
}

pub(super) fn record_has_workspace_linking_origin(
    origins: &BTreeMap<String, MaterializedRecordOrigin>,
    record_id: &str,
) -> bool {
    origins
        .get(record_id)
        .is_some_and(MaterializedRecordOrigin::is_workspace_linking)
}
