use crate::*;

pub(crate) fn handle_get_brain_health(
    request: GetBrainHealthRequest,
) -> Result<GetBrainHealthResponseData> {
    let root = resolve_brain_workspace_root(&request.scope)?;
    let knowledge_store = KnowledgeStore::open(KnowledgeStore::default_path_for_root(&root))?;
    let knowledge_store_report =
        brain_knowledge_store_report(&knowledge_store, &request.scope.workspace_id)?;
    let repo = BrainArtifactRepository::new(root.clone());
    if !repo.brain_manifest_path().exists() {
        return Ok(GetBrainHealthResponseData {
            status: BrainHealthStatus::Clean,
            attention_count: 0,
            governance: Some(brain_governance_report()),
            knowledge_store: Some(knowledge_store_report),
            source_reports: Vec::new(),
            recent_events: Vec::new(),
        });
    }
    let snapshot = read_materialized_brain_snapshot(&root, &request.scope.workspace_id)?;
    let mut report = lint_brain_snapshot(&snapshot);
    report
        .issues
        .extend(lint_missing_materialized_wiki_refs(&root, &snapshot));
    let mut source_reports = brain_health_source_reports(&repo, &snapshot);
    for report in &mut source_reports {
        if let Some(import_job) = knowledge_store.read_import_job(
            &request.scope.workspace_id,
            None,
            Some(report.source_id.as_str()),
        )? {
            let lifecycle = ImportLifecycleState::from_persisted(
                &import_job.status,
                &import_job.graph_status,
                import_job.citation_ready,
                import_job.graph_ready,
                import_job.graph_retryable,
                import_job.manual_retry_available,
            );
            report.citation_ready = lifecycle.citation_ready;
            report.graph_ready = lifecycle.graph_ready;
            report.graph_status = import_job.graph_status;
            report.manual_retry_available = lifecycle.manual_retry_available;
            if report.citation_ready && !report.graph_ready {
                let warning = match lifecycle.status {
                    ImportLifecycleStatus::CitationReadyGraphSkipped => {
                        "citation_ready_graph_skipped"
                    }
                    ImportLifecycleStatus::GraphRetryWaiting => "graph_retry_waiting",
                    _ => "citation_ready_graph_pending",
                };
                push_health_warning(&mut report.warnings, warning);
            }
        }
    }
    let source_attention_count: usize = source_reports
        .iter()
        .map(|report| report.warnings.len())
        .sum();
    let attention_count = report.issues.len() + source_attention_count;
    let mut recent_events = repo.read_brain_events()?;
    recent_events.sort_by(|left, right| {
        right
            .created_at
            .cmp(&left.created_at)
            .then_with(|| right.event_id.cmp(&left.event_id))
    });
    recent_events.truncate(10);
    Ok(GetBrainHealthResponseData {
        status: if attention_count == 0 {
            BrainHealthStatus::Clean
        } else {
            BrainHealthStatus::AttentionNeeded
        },
        attention_count,
        governance: Some(brain_governance_report()),
        knowledge_store: Some(knowledge_store_report),
        source_reports,
        recent_events,
    })
}

fn brain_governance_report() -> BrainGovernanceReport {
    BrainGovernanceReport {
        storage_locality: "local_workspace".into(),
        interaction_surface: "desktop_mcp".into(),
        evidence_governed: true,
        mutating_tools_require_evidence: true,
        local_path_disclosure_default: "redacted".into(),
    }
}

fn brain_knowledge_store_report(
    knowledge_store: &KnowledgeStore,
    workspace_id: &str,
) -> Result<BrainKnowledgeStoreReport> {
    let health = knowledge_store.health()?;
    let summary = knowledge_store.state_summary(workspace_id)?;
    Ok(BrainKnowledgeStoreReport {
        canonical_storage: "sqlite+graphqlite".into(),
        primary_graph_store: "graphqlite".into(),
        pure_sqlite_relational_graph_rejected: true,
        optional_graphqlite_acceleration_rejected: true,
        graph_store_mode: "required_primary".into(),
        graph_native_query_surface: "graphqlite_cypher".into(),
        migration_mode: "single_db_first_release".into(),
        long_dual_write_transition_rejected: true,
        db_schema_version: health.db_schema_version,
        graph_schema_version: health.graph_schema_version,
        graphqlite_loaded: health.graphqlite_loaded,
        graphqlite_transactional: health.graphqlite_transactional,
        graphqlite_release_gate: if health.graphqlite_loaded && health.graphqlite_transactional {
            "passed".into()
        } else {
            "blocked".into()
        },
        release_blocked_without_graphqlite: true,
        migration_blast_radius: "high".into(),
        broad_verification_required: true,
        json_artifacts_canonical: false,
        json_artifact_role: "migration_export_debug_compat".into(),
        vector_search_enabled: false,
        vector_search_policy: "defer_until_db_graphqlite_read_paths_stabilize".into(),
        checkpoint_rollback_api_enabled: false,
        checkpoint_rollback_policy: "defer_until_checkpoints_reliably_stored".into(),
        graph_algorithms_enabled: false,
        graph_algorithm_policy: "revisit_after_primary_graph_data_stabilizes".into(),
        evidence_item_count: summary.evidence_item_count,
        wiki_page_count: summary.wiki_page_count,
        graph_node_count: summary.graph_node_count,
        graph_relation_count: summary.graph_relation_count,
    })
}

fn brain_health_source_reports(
    repo: &BrainArtifactRepository,
    snapshot: &BrainRepoSnapshot,
) -> Vec<BrainHealthSourceReport> {
    snapshot
        .sources
        .iter()
        .map(|source| brain_health_source_report(repo, source))
        .collect()
}

fn brain_health_source_report(
    repo: &BrainArtifactRepository,
    source: &SourceRecord,
) -> BrainHealthSourceReport {
    let mut warnings = Vec::new();
    let mut provider_route = "unknown".to_string();
    let mut local_only = None;
    let mut content_hash = None;
    let mut content_hash_status = "unknown".to_string();
    let mut failed_page_count = 0usize;

    let mut source_pack_read_failed = false;
    let source_pack = match read_source_pack_v0(repo, &source.source_id) {
        Ok(source_pack) => source_pack,
        Err(_error) => {
            source_pack_read_failed = true;
            push_health_warning(&mut warnings, "source_pack_unreadable");
            None
        }
    };
    let valid_source_pack = source_pack.as_ref().filter(|pack| {
        pack.schema_version == etyma_engine_types::SOURCE_PACK_V0_SCHEMA_VERSION
            && pack.source_id == source.source_id
            && pack.workspace_id == source.workspace_id
    });
    match source_pack.as_ref() {
        Some(pack)
            if pack.schema_version != etyma_engine_types::SOURCE_PACK_V0_SCHEMA_VERSION =>
        {
            push_health_warning(&mut warnings, "source_pack_schema_mismatch");
        }
        Some(pack) if pack.source_id != source.source_id => {
            push_health_warning(&mut warnings, "source_pack_source_mismatch");
        }
        Some(pack) if pack.workspace_id != source.workspace_id => {
            push_health_warning(&mut warnings, "source_pack_workspace_mismatch");
        }
        None if !source_pack_read_failed => {
            push_health_warning(&mut warnings, "source_pack_missing");
        }
        _ => {}
    }

    if let Some(pack) = valid_source_pack {
        provider_route = pack.provider_route.clone();
        local_only = Some(pack.local_only);
        content_hash = Some(pack.content_hash.clone());
        content_hash_status = "source_pack_only".into();
        failed_page_count = pack
            .pages
            .iter()
            .filter(|page| page.error_message.is_some())
            .count();
        for warning in &pack.warnings {
            push_health_warning(&mut warnings, source_pack_health_warning_summary(warning));
        }
    }

    let mut evidence_index_read_failed = false;
    let evidence_index = match read_evidence_index_artifact(repo, &source.source_id) {
        Ok(evidence_index) => evidence_index,
        Err(_error) => {
            evidence_index_read_failed = true;
            push_health_warning(&mut warnings, "evidence_index_unreadable");
            None
        }
    };
    let valid_evidence_index = evidence_index.as_ref().filter(|index| {
        (index.schema_version() == etyma_engine_types::EVIDENCE_INDEX_V0_SCHEMA_VERSION
            || index.schema_version() == etyma_engine_types::EVIDENCE_INDEX_V1_SCHEMA_VERSION)
            && index.source_id() == Some(source.source_id.as_str())
            && index.workspace_id() == Some(source.workspace_id.as_str())
    });
    match evidence_index.as_ref() {
        Some(index)
            if index.schema_version()
                != etyma_engine_types::EVIDENCE_INDEX_V0_SCHEMA_VERSION
                && index.schema_version()
                    != etyma_engine_types::EVIDENCE_INDEX_V1_SCHEMA_VERSION =>
        {
            push_health_warning(&mut warnings, "evidence_index_schema_mismatch");
        }
        Some(index) if index.source_id() != Some(source.source_id.as_str()) => {
            push_health_warning(&mut warnings, "evidence_index_source_mismatch");
        }
        Some(index) if index.workspace_id() != Some(source.workspace_id.as_str()) => {
            push_health_warning(&mut warnings, "evidence_index_workspace_mismatch");
        }
        None if !evidence_index_read_failed => {
            push_health_warning(&mut warnings, "evidence_index_missing");
        }
        _ => {}
    }

    if let Some(index) = valid_evidence_index {
        if provider_route == "unknown" {
            if let Some(route) = index.provider_route() {
                provider_route = route.to_string();
            }
            local_only = index.local_only();
        }
        if let Some(pack_hash) = content_hash.as_deref() {
            if Some(pack_hash) == index.content_hash() {
                content_hash_status = "current".into();
            } else {
                content_hash_status = "mismatch".into();
                push_health_warning(&mut warnings, "content_hash_mismatch");
            }
        } else if let Some(index_hash) = index.content_hash() {
            content_hash = Some(index_hash.to_string());
            content_hash_status = "evidence_index_only".into();
        }
    }

    if source.status == SourceStatus::partial() {
        push_health_warning(&mut warnings, "partial_import");
    } else if source.status == SourceStatus::failed() {
        push_health_warning(&mut warnings, "import_failed");
    } else if source.status == SourceStatus::stale() {
        content_hash_status = "stale".into();
        push_health_warning(&mut warnings, "stale_source");
    }
    if failed_page_count > 0 {
        push_health_warning(
            &mut warnings,
            format!("{failed_page_count} page(s) failed during import"),
        );
    }

    BrainHealthSourceReport {
        source_id: source.source_id.clone(),
        status: source.status.clone(),
        page_count: source.page_count,
        failed_page_count,
        provider_route,
        local_only,
        content_hash,
        content_hash_status,
        citation_ready: success_count_for_health_source(source),
        graph_ready: false,
        graph_status: String::new(),
        manual_retry_available: false,
        warnings,
    }
}

fn success_count_for_health_source(source: &SourceRecord) -> bool {
    source.status != SourceStatus::failed() && source.page_count > 0
}

fn source_pack_health_warning_summary(
    warning: &etyma_engine_types::SourcePackWarningV0,
) -> String {
    match warning.page {
        Some(page) => format!(
            "source_pack_warning:{}:severity:{}:page:{page}",
            warning.warning_type,
            warning_severity_slug(&warning.severity)
        ),
        None => format!(
            "source_pack_warning:{}:severity:{}",
            warning.warning_type,
            warning_severity_slug(&warning.severity)
        ),
    }
}

fn warning_severity_slug(
    severity: &etyma_engine_types::ContextPackWarningSeverity,
) -> &'static str {
    match severity {
        etyma_engine_types::ContextPackWarningSeverity::Low => "low",
        etyma_engine_types::ContextPackWarningSeverity::Medium => "medium",
        etyma_engine_types::ContextPackWarningSeverity::High => "high",
    }
}

fn push_health_warning(warnings: &mut Vec<String>, warning: impl Into<String>) {
    let warning = warning.into();
    if !warnings.contains(&warning) {
        warnings.push(warning);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BrainLintIssue {
    pub(crate) issue_id: String,
    pub(crate) kind: String,
    pub(crate) severity: String,
    pub(crate) title: String,
    pub(crate) body: String,
    #[serde(default)]
    pub(crate) source_refs: Vec<String>,
    #[serde(default)]
    pub(crate) node_refs: Vec<String>,
    #[serde(default)]
    pub(crate) relation_refs: Vec<String>,
    #[serde(default)]
    pub(crate) evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BrainMaintenanceReport {
    pub(crate) workspace_id: String,
    pub(crate) generated_at: u64,
    pub(crate) issue_count: usize,
    pub(crate) repair_count: usize,
    pub(crate) new_markdown_source_count: usize,
    pub(crate) enqueued_markdown_source_count: usize,
    pub(crate) ingest_worker_started: bool,
    pub(crate) ingested_markdown_source_count: usize,
    pub(crate) failed_markdown_source_count: usize,
    #[serde(default)]
    pub(crate) repairs: Vec<String>,
    #[serde(default)]
    pub(crate) new_markdown_sources: Vec<String>,
    #[serde(default)]
    pub(crate) enqueued_markdown_sources: Vec<String>,
    #[serde(default)]
    pub(crate) ingested_markdown_sources: Vec<String>,
    #[serde(default)]
    pub(crate) failed_markdown_sources: Vec<String>,
    #[serde(default)]
    pub(crate) issues: Vec<BrainLintIssue>,
}

#[cfg(test)]
pub(crate) fn run_brain_maintenance(scope: &BrainReadScope) -> Result<BrainMaintenanceReport> {
    let root = resolve_brain_workspace_root(scope)?;
    let ingest_paths = resolve_markdown_ingest_paths(scope)?;
    let initial_snapshot = read_materialized_brain_snapshot(&root, &scope.workspace_id)?;
    let source_state = read_markdown_source_state(&ingest_paths)?;
    let ingest_queue = read_markdown_ingest_queue(&ingest_paths)?;
    let markdown_scan = scan_new_markdown_sources(
        &ingest_paths,
        &initial_snapshot,
        &source_state,
        &ingest_queue,
    )?;
    let enqueue_result = {
        let writer = BrainWorkspaceWriter::open(root.clone())?;
        enqueue_markdown_sources(&writer, &ingest_paths, &ingest_queue, &markdown_scan)?
    };
    write_markdown_source_state(&ingest_paths, &markdown_scan.current_state)?;
    let queued_after_enqueue = read_markdown_ingest_queue(&ingest_paths)?;
    let store = KnowledgeProjectStore::default()?;
    let worker_result = run_markdown_ingest_worker(&ingest_paths, &queued_after_enqueue, &store)?;
    let mut snapshot = if worker_result.processed > 0 || worker_result.failed > 0 {
        read_materialized_brain_snapshot(&root, &scope.workspace_id)?
    } else {
        initial_snapshot
    };
    let mut report = lint_brain_snapshot(&snapshot);
    report.new_markdown_source_count = markdown_scan.new_sources.len();
    report.new_markdown_sources = markdown_scan
        .new_sources
        .iter()
        .map(|source| source.relative_path.display().to_string())
        .collect();
    report.enqueued_markdown_source_count = enqueue_result.enqueued.len();
    report.enqueued_markdown_sources = enqueue_result
        .enqueued
        .iter()
        .map(|source| source.relative_path.clone())
        .collect();
    report.ingest_worker_started = worker_result.started;
    report.ingested_markdown_source_count = worker_result.processed;
    report.failed_markdown_source_count = worker_result.failed;
    report.ingested_markdown_sources = worker_result.processed_sources;
    report.failed_markdown_sources = worker_result.failed_sources;
    report.repair_count +=
        repair_missing_materialized_wiki_stubs(&root, &mut snapshot, &mut report.repairs)?;
    report
        .issues
        .extend(lint_missing_materialized_wiki_refs(&root, &snapshot));
    report.repair_count += repair_generated_brain_artifacts(&root, &snapshot, &mut report.repairs)?;
    let writer = BrainWorkspaceWriter::open(root.clone())?;
    report.issue_count = report.issues.len();
    write_json_pretty(&root.join("state/maintenance-latest.json"), &report)?;
    if report.repair_count > 0
        || report.new_markdown_source_count > 0
        || report.enqueued_markdown_source_count > 0
        || report.ingest_worker_started
    {
        writer.append_event(&brain_maintenance_event(&snapshot, &report)?)?;
    }
    Ok(report)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn materialized_graph_event_payload_json(
    generated_at: u64,
    sources: &[SourceRecord],
    nodes: &[BrainNodeRecord],
    relations: &[BrainRelationRecord],
    evidence: &[EvidenceRef],
    memories: &[MemoryRecord],
    wiki_pages: &[WikiPage],
    entities: &[EntityRecord],
    claims: &[ClaimRecord],
    extractions: &[StructuredExtractionArtifact],
) -> Result<String> {
    serde_json::to_string(&json!({
        "nodeCount": nodes.len(),
        "relationCount": relations.len(),
        "sourceCount": sources.len(),
        "materializedGraph": {
            "generatedAt": generated_at,
            "sources": sources,
            "nodes": nodes,
            "edges": relations,
            "evidence": evidence,
            "memories": memories,
            "wikiPages": wiki_pages,
            "entities": entities,
            "claims": claims,
            "extractions": extractions,
        }
    }))
    .context("failed to encode materialized graph event payload")
}

pub(crate) fn lint_brain_snapshot(snapshot: &BrainRepoSnapshot) -> BrainMaintenanceReport {
    let mut issues = Vec::new();
    let generated_at = unix_timestamp_seconds();
    let evidence_ids = snapshot
        .evidence
        .iter()
        .map(|evidence| evidence.id.clone())
        .collect::<BTreeSet<_>>();
    let source_ids = snapshot
        .sources
        .iter()
        .map(|source| source.source_id.clone())
        .collect::<BTreeSet<_>>();
    let node_ids = snapshot
        .nodes
        .iter()
        .map(|node| node.node_id.clone())
        .collect::<BTreeSet<_>>();
    let connected_node_ids = snapshot
        .relations
        .iter()
        .flat_map(|relation| {
            [
                relation.source_node_id.clone(),
                relation.target_node_id.clone(),
            ]
        })
        .collect::<BTreeSet<_>>();

    for claim in &snapshot.claims {
        let missing_evidence = missing_refs(&claim.evidence_refs, &evidence_ids);
        if claim.evidence_refs.is_empty() || !missing_evidence.is_empty() {
            issues.push(BrainLintIssue {
                issue_id: stable_lint_issue_id(
                    "missing-evidence",
                    &claim.claim_id,
                    &missing_evidence,
                ),
                kind: "missing_evidence".into(),
                severity: "risky".into(),
                title: format!("Claim needs evidence: {}", claim.statement),
                body: if claim.evidence_refs.is_empty() {
                    "This claim has no evidence refs. Review it before agents use it as durable brain context.".into()
                } else {
                    format!(
                        "This claim references missing evidence ids: {}.",
                        missing_evidence.join(", ")
                    )
                },
                source_refs: claim.source_refs.clone(),
                node_refs: claim.topic_refs.clone(),
                relation_refs: Vec::new(),
                evidence_refs: claim.evidence_refs.clone(),
            });
        }
    }

    for relation in &snapshot.relations {
        let mut missing_nodes = missing_refs(
            &[
                relation.source_node_id.clone(),
                relation.target_node_id.clone(),
            ],
            &node_ids,
        );
        let missing_evidence = missing_refs(&relation.evidence_ids, &evidence_ids);
        if !missing_nodes.is_empty() || !missing_evidence.is_empty() {
            missing_nodes.extend(missing_evidence);
            issues.push(BrainLintIssue {
                issue_id: stable_lint_issue_id("orphan-relation", &relation.relation_id, &missing_nodes),
                kind: "orphan".into(),
                severity: "risky".into(),
                title: format!("Typed relation needs review: {}", relation.label),
                body: "This relation points at a missing node or evidence ref. Review it before keeping it in the durable graph.".into(),
                source_refs: Vec::new(),
                node_refs: vec![relation.source_node_id.clone(), relation.target_node_id.clone()],
                relation_refs: vec![relation.relation_id.clone()],
                evidence_refs: relation.evidence_ids.clone(),
            });
        }
    }

    for node in &snapshot.nodes {
        if matches!(node.kind, BrainNodeKind::Concept | BrainNodeKind::Topic)
            && node.evidence_ids.is_empty()
            && node.source_ids.is_empty()
            && !connected_node_ids.contains(&node.node_id)
        {
            issues.push(BrainLintIssue {
                issue_id: stable_lint_issue_id("orphan-node", &node.node_id, &[]),
                kind: "orphan".into(),
                severity: "risky".into(),
                title: format!("Orphan node needs review: {}", node.label),
                body: "This node is not connected to a source, evidence ref, or typed relation."
                    .into(),
                source_refs: Vec::new(),
                node_refs: vec![node.node_id.clone()],
                relation_refs: Vec::new(),
                evidence_refs: Vec::new(),
            });
        }
        let missing_sources = missing_refs(&node.source_ids, &source_ids);
        if !missing_sources.is_empty() {
            issues.push(BrainLintIssue {
                issue_id: stable_lint_issue_id("missing-source", &node.node_id, &missing_sources),
                kind: "missing_evidence".into(),
                severity: "risky".into(),
                title: format!("Node references missing source: {}", node.label),
                body: format!("Missing source refs: {}.", missing_sources.join(", ")),
                source_refs: node.source_ids.clone(),
                node_refs: vec![node.node_id.clone()],
                relation_refs: Vec::new(),
                evidence_refs: node.evidence_ids.clone(),
            });
        }
    }

    for source in &snapshot.sources {
        if source.status == "stale" || source.updated_at > snapshot.generated_at {
            issues.push(BrainLintIssue {
                issue_id: stable_lint_issue_id("stale-source", &source.source_id, &[]),
                kind: "stale".into(),
                severity: "risky".into(),
                title: format!("Source may need recompilation: {}", source.source_id),
                body: "This source is stale or newer than the materialized brain snapshot.".into(),
                source_refs: vec![source.source_id.clone()],
                node_refs: Vec::new(),
                relation_refs: Vec::new(),
                evidence_refs: Vec::new(),
            });
        }
    }

    for (left_index, left) in snapshot.claims.iter().enumerate() {
        for right in snapshot.claims.iter().skip(left_index + 1) {
            if claims_may_conflict(left, right) {
                issues.push(BrainLintIssue {
                    issue_id: stable_lint_issue_id(
                        "conflict",
                        &left.claim_id,
                        std::slice::from_ref(&right.claim_id),
                    ),
                    kind: "conflict".into(),
                    severity: "risky".into(),
                    title: "Claims may conflict".into(),
                    body: format!(
                        "Review potentially conflicting claims: `{}` vs `{}`.",
                        left.statement, right.statement
                    ),
                    source_refs: left
                        .source_refs
                        .iter()
                        .chain(right.source_refs.iter())
                        .cloned()
                        .collect::<BTreeSet<_>>()
                        .into_iter()
                        .collect(),
                    node_refs: left
                        .topic_refs
                        .iter()
                        .chain(right.topic_refs.iter())
                        .cloned()
                        .collect::<BTreeSet<_>>()
                        .into_iter()
                        .collect(),
                    relation_refs: Vec::new(),
                    evidence_refs: left
                        .evidence_refs
                        .iter()
                        .chain(right.evidence_refs.iter())
                        .cloned()
                        .collect::<BTreeSet<_>>()
                        .into_iter()
                        .collect(),
                });
            }
        }
    }

    BrainMaintenanceReport {
        workspace_id: snapshot.workspace_id.clone(),
        generated_at,
        issue_count: issues.len(),
        repair_count: 0,
        new_markdown_source_count: 0,
        enqueued_markdown_source_count: 0,
        ingest_worker_started: false,
        ingested_markdown_source_count: 0,
        failed_markdown_source_count: 0,
        repairs: Vec::new(),
        new_markdown_sources: Vec::new(),
        enqueued_markdown_sources: Vec::new(),
        ingested_markdown_sources: Vec::new(),
        failed_markdown_sources: Vec::new(),
        issues,
    }
}

pub(crate) fn missing_refs(refs: &[String], existing: &BTreeSet<String>) -> Vec<String> {
    refs.iter()
        .filter(|value| !existing.contains(*value))
        .cloned()
        .collect()
}

pub(crate) fn lint_missing_materialized_wiki_refs(
    root: &Path,
    snapshot: &BrainRepoSnapshot,
) -> Vec<BrainLintIssue> {
    let wiki_paths = snapshot
        .wiki_pages
        .iter()
        .map(|page| page.path.clone())
        .collect::<BTreeSet<_>>();
    let mut missing = BTreeMap::<String, BrainLintIssue>::new();

    for page in &snapshot.wiki_pages {
        if !root.join(&page.path).exists() {
            upsert_missing_wiki_issue(
                &mut missing,
                &page.path,
                &format!("wiki-page:{}", page.page_id),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            );
        }
    }

    for node in &snapshot.nodes {
        let missing_refs = missing_wiki_refs(root, &wiki_paths, &node.source_ids);
        for path in missing_refs {
            upsert_missing_wiki_issue(
                &mut missing,
                &path,
                &format!("node:{}", node.node_id),
                vec![node.node_id.clone()],
                node.source_ids.clone(),
                node.evidence_ids.clone(),
            );
        }
    }

    for claim in &snapshot.claims {
        let missing_refs = missing_wiki_refs(root, &wiki_paths, &claim.source_refs);
        for path in missing_refs {
            upsert_missing_wiki_issue(
                &mut missing,
                &path,
                &format!("claim:{}", claim.claim_id),
                claim.topic_refs.clone(),
                claim.source_refs.clone(),
                claim.evidence_refs.clone(),
            );
        }
    }

    for memory in &snapshot.memories {
        let missing_refs = missing_wiki_refs(root, &wiki_paths, &memory.source_refs);
        for path in missing_refs {
            upsert_missing_wiki_issue(
                &mut missing,
                &path,
                &format!("memory:{}", memory.memory_id),
                Vec::new(),
                memory.source_refs.clone(),
                memory.evidence_refs.clone(),
            );
        }
    }

    for event in &snapshot.events {
        if !is_graph_or_memory_change_event(event.event_type) {
            continue;
        }
        let event_refs = event
            .source_refs
            .iter()
            .chain(event.source_markdown_refs.iter())
            .cloned()
            .collect::<Vec<_>>();
        for path in missing_wiki_refs(root, &wiki_paths, &event_refs) {
            upsert_missing_wiki_issue(
                &mut missing,
                &path,
                &format!("event:{}", event.event_id),
                event.node_refs.clone(),
                event_refs.clone(),
                event.evidence_refs.clone(),
            );
        }
    }

    missing.into_values().collect()
}

#[derive(Debug, Clone, Default)]
#[cfg(test)]
struct MissingWikiPageStub {
    path: String,
    title: String,
    contexts: Vec<String>,
    node_refs: Vec<String>,
    source_refs: Vec<String>,
    evidence_refs: Vec<String>,
}

#[cfg(test)]
fn repair_missing_materialized_wiki_stubs(
    root: &Path,
    snapshot: &mut BrainRepoSnapshot,
    repairs: &mut Vec<String>,
) -> Result<usize> {
    let wiki_paths = snapshot
        .wiki_pages
        .iter()
        .map(|page| page.path.clone())
        .collect::<BTreeSet<_>>();
    let mut stubs = BTreeMap::<String, MissingWikiPageStub>::new();
    let node_labels = snapshot
        .nodes
        .iter()
        .map(|node| (node.node_id.clone(), node.label.clone()))
        .collect::<BTreeMap<_, _>>();

    for page in &snapshot.wiki_pages {
        if !root.join(&page.path).exists() {
            let path_node_ref = page
                .path
                .strip_prefix("wiki/topics/")
                .and_then(|path| path.strip_suffix(".md"))
                .map(ToString::to_string);
            let page_node_refs = merge_string_refs(
                &page.node_refs,
                &path_node_ref.clone().into_iter().collect::<Vec<_>>(),
            );
            let page_context = if page_node_refs.is_empty() {
                format!(
                    "Existing materialized page record `{}` was missing on disk.",
                    page.page_id
                )
            } else {
                let labels = page
                    .node_refs
                    .iter()
                    .chain(path_node_ref.iter())
                    .map(|node_id| {
                        node_labels
                            .get(node_id)
                            .map(String::as_str)
                            .unwrap_or(node_id.as_str())
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "Markdown-derived node page record `{}` was missing on disk for: {}.",
                    page.page_id, labels
                )
            };
            upsert_missing_wiki_stub(
                &mut stubs,
                &page.path,
                &page.title,
                page_context,
                page_node_refs,
                page.source_refs.clone(),
                page.evidence_refs.clone(),
            );
        }
    }

    for node in &snapshot.nodes {
        for path in missing_wiki_refs(root, &wiki_paths, &node.source_ids) {
            upsert_missing_wiki_stub(
                &mut stubs,
                &path,
                &node.label,
                format!(
                    "Markdown-derived node `{}` (`{}`) originated from this missing wiki page.",
                    node.node_id, node.label
                ),
                vec![node.node_id.clone()],
                node.source_ids.clone(),
                node.evidence_ids.clone(),
            );
        }
    }

    for claim in &snapshot.claims {
        for path in missing_wiki_refs(root, &wiki_paths, &claim.source_refs) {
            upsert_missing_wiki_stub(
                &mut stubs,
                &path,
                "Recovered Claim Context",
                format!(
                    "Markdown-derived claim `{}`: {}",
                    claim.claim_id, claim.statement
                ),
                claim.topic_refs.clone(),
                claim.source_refs.clone(),
                claim.evidence_refs.clone(),
            );
        }
    }

    for memory in &snapshot.memories {
        for path in missing_wiki_refs(root, &wiki_paths, &memory.source_refs) {
            upsert_missing_wiki_stub(
                &mut stubs,
                &path,
                &memory.title,
                format!(
                    "Markdown-derived memory `{}`: {}",
                    memory.memory_id, memory.body
                ),
                Vec::new(),
                memory.source_refs.clone(),
                memory.evidence_refs.clone(),
            );
        }
    }

    for event in &snapshot.events {
        if !is_graph_or_memory_change_event(event.event_type) {
            continue;
        }
        let event_refs = event
            .source_refs
            .iter()
            .chain(event.source_markdown_refs.iter())
            .cloned()
            .collect::<Vec<_>>();
        for path in missing_wiki_refs(root, &wiki_paths, &event_refs) {
            let mut context = format!(
                "Event `{}` applied `{}` from markdown-derived graph context.",
                event.event_id,
                event.operation_type.as_deref().unwrap_or("graph_change")
            );
            if !event.relation_refs.is_empty() {
                let relation_contexts = event
                    .relation_refs
                    .iter()
                    .filter_map(|relation_id| {
                        snapshot
                            .relations
                            .iter()
                            .find(|relation| &relation.relation_id == relation_id)
                    })
                    .map(|relation| {
                        let source = node_labels
                            .get(&relation.source_node_id)
                            .map(String::as_str)
                            .unwrap_or(&relation.source_node_id);
                        let target = node_labels
                            .get(&relation.target_node_id)
                            .map(String::as_str)
                            .unwrap_or(&relation.target_node_id);
                        format!(
                            "edge `{}` connects `{}` to `{}` as `{}`",
                            relation.relation_id, source, target, relation.label
                        )
                    })
                    .collect::<Vec<_>>();
                if !relation_contexts.is_empty() {
                    context.push_str(" Related edge context: ");
                    context.push_str(&relation_contexts.join("; "));
                    context.push('.');
                }
            }
            upsert_missing_wiki_stub(
                &mut stubs,
                &path,
                "Recovered Graph Context",
                context,
                event.node_refs.clone(),
                event_refs.clone(),
                event.evidence_refs.clone(),
            );
        }
    }

    if stubs.is_empty() {
        return Ok(0);
    }

    let existing_paths = snapshot
        .wiki_pages
        .iter()
        .map(|page| page.path.clone())
        .collect::<BTreeSet<_>>();
    let updated_at = unix_timestamp_seconds();
    for stub in stubs.values() {
        if !existing_paths.contains(&stub.path) {
            let existing_body = fs::read_to_string(root.join(&stub.path)).ok();
            snapshot.wiki_pages.push(WikiPage {
                page_id: format!("wiki-stub-{}", sanitize_name(&stub.path)),
                workspace_id: snapshot.workspace_id.clone(),
                path: stub.path.clone(),
                title: stub.title.clone(),
                body: existing_body.unwrap_or_else(|| missing_wiki_stub_body(stub)),
                node_refs: merge_string_refs(&stub.node_refs, &[]),
                source_refs: merge_string_refs(&stub.source_refs, std::slice::from_ref(&stub.path)),
                evidence_refs: merge_string_refs(&stub.evidence_refs, &[]),
                updated_at,
            });
        } else if let Some(page) = snapshot
            .wiki_pages
            .iter_mut()
            .find(|page| page.path == stub.path)
        {
            page.body = missing_wiki_stub_body(stub);
            page.node_refs = merge_string_refs(&page.node_refs, &stub.node_refs);
            page.source_refs = merge_string_refs(&page.source_refs, &stub.source_refs);
            page.evidence_refs = merge_string_refs(&page.evidence_refs, &stub.evidence_refs);
            page.updated_at = page.updated_at.max(updated_at);
        }
        merge_unique_string(repairs, &stub.path);
    }
    snapshot.wiki_pages = dedupe_wiki_pages(std::mem::take(&mut snapshot.wiki_pages));
    persist_materialized_graph_and_wiki_state(root, snapshot)?;
    Ok(stubs.len())
}

#[cfg(test)]
fn upsert_missing_wiki_stub(
    stubs: &mut BTreeMap<String, MissingWikiPageStub>,
    path: &str,
    title: &str,
    context: String,
    node_refs: Vec<String>,
    source_refs: Vec<String>,
    evidence_refs: Vec<String>,
) {
    if !is_wiki_markdown_ref(path) {
        return;
    }
    let stub = stubs
        .entry(path.to_string())
        .or_insert_with(|| MissingWikiPageStub {
            path: path.to_string(),
            title: if title.trim().is_empty() {
                title_from_wiki_path(path)
            } else {
                title.trim().to_string()
            },
            contexts: Vec::new(),
            node_refs: Vec::new(),
            source_refs: vec![path.to_string()],
            evidence_refs: Vec::new(),
        });
    merge_unique_string(&mut stub.contexts, &context);
    for node_ref in node_refs {
        merge_unique_string(&mut stub.node_refs, &node_ref);
    }
    for source_ref in source_refs {
        merge_unique_string(&mut stub.source_refs, &source_ref);
    }
    for evidence_ref in evidence_refs {
        merge_unique_string(&mut stub.evidence_refs, &evidence_ref);
    }
}

#[cfg(test)]
fn missing_wiki_stub_body(stub: &MissingWikiPageStub) -> String {
    let contexts = if stub.contexts.is_empty() {
        "- Recovered from a missing materialized wiki reference.".into()
    } else {
        stub.contexts
            .iter()
            .map(|context| format!("- {context}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        "# {}\n\nThis page was automatically regenerated as a reviewable stub from markdown-derived graph context.\n\n## Origin Context\n\n{}\n\n## Refs\n\n- Nodes: {}\n- Sources: {}\n- Evidence: {}\n",
        stub.title,
        contexts,
        join_or_none(&stub.node_refs),
        join_or_none(&stub.source_refs),
        join_or_none(&stub.evidence_refs)
    )
}

#[cfg(test)]
fn title_from_wiki_path(path: &str) -> String {
    path.trim_start_matches("wiki/")
        .trim_end_matches(".md")
        .rsplit('/')
        .next()
        .unwrap_or("Recovered Wiki Page")
        .split(['-', '_'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn missing_wiki_refs(root: &Path, wiki_paths: &BTreeSet<String>, refs: &[String]) -> Vec<String> {
    refs.iter()
        .filter(|value| is_wiki_markdown_ref(value))
        .filter(|value| !wiki_paths.contains(*value) || !root.join(value).exists())
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub(crate) fn is_wiki_markdown_ref(value: &str) -> bool {
    value.starts_with("wiki/") && value.ends_with(".md") && !value.contains("..")
}

fn is_graph_or_memory_change_event(event_type: BrainEventKind) -> bool {
    matches!(
        event_type,
        BrainEventKind::GraphMaterialized
            | BrainEventKind::WikiMaterialized
            | BrainEventKind::MemoryAccepted
            | BrainEventKind::CorrectionApplied
            | BrainEventKind::BrainMaintenanceRun
    )
}

fn upsert_missing_wiki_issue(
    issues: &mut BTreeMap<String, BrainLintIssue>,
    path: &str,
    origin: &str,
    node_refs: Vec<String>,
    source_refs: Vec<String>,
    evidence_refs: Vec<String>,
) {
    let issue = issues.entry(path.to_string()).or_insert_with(|| BrainLintIssue {
        issue_id: stable_lint_issue_id("missing-wiki-page", path, &[]),
        kind: "missing_wiki_page".into(),
        severity: "risky".into(),
        title: format!("Wiki page is not materialized: {path}"),
        body: "A graph or memory change references a wiki page that is absent from the materialized wiki. Re-run replay/materialization before agents use this path.".into(),
        source_refs: vec![path.to_string()],
        node_refs: Vec::new(),
        relation_refs: Vec::new(),
        evidence_refs: Vec::new(),
    });
    merge_unique_string(&mut issue.source_refs, path);
    merge_unique_string(&mut issue.source_refs, origin);
    for source_ref in source_refs {
        merge_unique_string(&mut issue.source_refs, &source_ref);
    }
    for node_ref in node_refs {
        merge_unique_string(&mut issue.node_refs, &node_ref);
    }
    for evidence_ref in evidence_refs {
        merge_unique_string(&mut issue.evidence_refs, &evidence_ref);
    }
}

fn stable_lint_issue_id(kind: &str, primary: &str, rest: &[String]) -> String {
    let mut parts = vec![kind.to_string(), primary.to_string()];
    parts.extend(rest.iter().cloned());
    format!("lint-{}", sanitize_name(&parts.join("-")))
}

fn claims_may_conflict(left: &ClaimRecord, right: &ClaimRecord) -> bool {
    if left.topic_refs.is_empty()
        || right.topic_refs.is_empty()
        || left
            .topic_refs
            .iter()
            .all(|topic| !right.topic_refs.contains(topic))
    {
        return false;
    }
    let left_negative = contains_negative_claim_marker(&left.statement);
    let right_negative = contains_negative_claim_marker(&right.statement);
    left_negative != right_negative && shared_claim_terms(&left.statement, &right.statement) >= 3
}

fn contains_negative_claim_marker(value: &str) -> bool {
    value
        .split(|char: char| !char.is_ascii_alphanumeric())
        .any(|word| {
            matches!(
                word.to_ascii_lowercase().as_str(),
                "no" | "not" | "never" | "without"
            )
        })
}

fn shared_claim_terms(left: &str, right: &str) -> usize {
    let left_terms = claim_terms(left);
    let right_terms = claim_terms(right);
    left_terms.intersection(&right_terms).count()
}

fn claim_terms(value: &str) -> BTreeSet<String> {
    value
        .split(|char: char| !char.is_ascii_alphanumeric())
        .map(|word| word.to_ascii_lowercase())
        .filter(|word| word.len() >= 4)
        .filter(|word| {
            !matches!(
                word.as_str(),
                "this" | "that" | "with" | "from" | "into" | "evidence" | "backed"
            )
        })
        .collect()
}

#[cfg(test)]
fn repair_generated_brain_artifacts(
    root: &Path,
    snapshot: &BrainRepoSnapshot,
    repairs: &mut Vec<String>,
) -> Result<usize> {
    let mut count = 0;
    for page in snapshot
        .wiki_pages
        .iter()
        .filter(|page| page.path == "wiki/index.md" || page.path == "wiki/log.md")
    {
        let path = root.join(&page.path);
        let next = materialized_wiki_page_body(page, snapshot);
        if fs::read_to_string(&path).unwrap_or_default() != next {
            write_file_atomic(&path, next.as_bytes())?;
            repairs.push(page.path.clone());
            count += 1;
        }
    }
    count += repair_json_artifact(
        &root.join("graph/nodes.json"),
        &snapshot.nodes,
        "graph/nodes.json",
        repairs,
    )?;
    count += repair_json_artifact(
        &root.join("graph/edges.json"),
        &snapshot.relations,
        "graph/edges.json",
        repairs,
    )?;
    count += repair_json_artifact(
        &root.join("graph/evidence.json"),
        &snapshot.evidence,
        "graph/evidence.json",
        repairs,
    )?;
    count += repair_json_artifact(
        &root.join("graph/entities.json"),
        &snapshot.entities,
        "graph/entities.json",
        repairs,
    )?;
    count += repair_json_artifact(
        &root.join("graph/claims.json"),
        &snapshot.claims,
        "graph/claims.json",
        repairs,
    )?;
    Ok(count)
}

#[cfg(test)]
fn repair_json_artifact<T: Serialize>(
    path: &Path,
    value: &T,
    label: &str,
    repairs: &mut Vec<String>,
) -> Result<usize> {
    let next = serde_json::to_string_pretty(value).context("failed to encode repair artifact")?;
    if fs::read_to_string(path).unwrap_or_default() == next {
        return Ok(0);
    }
    write_file_atomic(path, next.as_bytes())?;
    repairs.push(label.into());
    Ok(1)
}

#[cfg(test)]
fn brain_maintenance_event(
    snapshot: &BrainRepoSnapshot,
    report: &BrainMaintenanceReport,
) -> Result<BrainEvent> {
    Ok(BrainEvent {
        event_id: format!("evt-{}", Uuid::now_v7()),
        schema_version: BRAIN_EVENT_SCHEMA_VERSION,
        workspace_id: snapshot.workspace_id.clone(),
        scope: BrainScope::Project,
        event_type: BrainEventKind::BrainMaintenanceRun,
        operation_type: Some("brain_maintenance_run".into()),
        actor: BrainActor {
            actor_type: BrainActorType::System,
            actor_id: "etyma-maintenance".into(),
        },
        source_refs: Vec::new(),
        source_markdown_refs: Vec::new(),
        node_refs: Vec::new(),
        relation_refs: Vec::new(),
        claim_refs: Vec::new(),
        memory_refs: Vec::new(),
        target_node_ids: Vec::new(),
        target_edge_ids: Vec::new(),
        target_claim_ids: Vec::new(),
        target_memory_ids: Vec::new(),
        evidence_refs: Vec::new(),
        payload_json: serde_json::to_string(report)
            .context("failed to encode maintenance event payload")?,
        causality: BrainEventCausality {
            snapshot_id: Some(format!(
                "snapshot-{}-{}",
                snapshot.workspace_id, snapshot.generated_at
            )),
            materialized_version: Some(snapshot.generated_at),
            ..Default::default()
        },
        confidence: None,
        policy_result: if report.issue_count == 0 {
            "auto_repaired".into()
        } else {
            "attention_needed".into()
        },
        created_at: report.generated_at,
    })
}
pub(crate) fn brain_node_record_content_matches(
    left: &BrainNodeRecord,
    right: &BrainNodeRecord,
) -> bool {
    left.node_id == right.node_id
        && left.kind == right.kind
        && left.label == right.label
        && left.scope == right.scope
        && left.aliases == right.aliases
        && left.evidence_ids == right.evidence_ids
        && left.source_ids == right.source_ids
        && left.confidence == right.confidence
}

pub(crate) fn brain_relation_record_content_matches(
    left: &BrainRelationRecord,
    right: &BrainRelationRecord,
) -> bool {
    left.relation_id == right.relation_id
        && left.kind == right.kind
        && left.source_node_id == right.source_node_id
        && left.target_node_id == right.target_node_id
        && left.label == right.label
        && left.evidence_ids == right.evidence_ids
        && left.confidence == right.confidence
}

#[cfg(test)]
fn dedupe_wiki_pages(pages: Vec<WikiPage>) -> Vec<WikiPage> {
    let mut merged = BTreeMap::<String, WikiPage>::new();
    for mut page in pages {
        page.node_refs = merge_string_refs(&page.node_refs, &[]);
        page.source_refs = merge_string_refs(&page.source_refs, &[]);
        page.evidence_refs = merge_string_refs(&page.evidence_refs, &[]);
        match merged.get_mut(&page.path) {
            Some(existing) => merge_wiki_page_record(existing, page),
            None => {
                merged.insert(page.path.clone(), page);
            }
        }
    }
    let mut pages = merged.into_values().collect::<Vec<_>>();
    pages.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.page_id.cmp(&right.page_id))
    });
    pages
}

#[cfg(test)]
fn merge_wiki_page_record(existing: &mut WikiPage, incoming: WikiPage) {
    existing.node_refs = merge_string_refs(&existing.node_refs, &incoming.node_refs);
    existing.source_refs = merge_string_refs(&existing.source_refs, &incoming.source_refs);
    existing.evidence_refs = merge_string_refs(&existing.evidence_refs, &incoming.evidence_refs);
    if incoming.updated_at >= existing.updated_at {
        existing.page_id = incoming.page_id;
        existing.workspace_id = incoming.workspace_id;
        existing.title = incoming.title;
        existing.body = incoming.body;
        existing.updated_at = incoming.updated_at;
    }
}

fn merge_unique_string(values: &mut Vec<String>, value: &str) {
    let value = value.trim();
    if !value.is_empty() && !values.iter().any(|existing| existing == value) {
        values.push(value.to_string());
    }
}

pub(crate) fn refresh_materialized_wiki_pages(snapshot: &mut BrainRepoSnapshot) {
    let generated = build_materialized_wiki_pages(
        &snapshot.workspace_id,
        &snapshot.sources,
        &snapshot.nodes,
        unix_timestamp_seconds(),
    );
    let generated_paths = generated
        .iter()
        .map(|page| page.path.clone())
        .collect::<BTreeSet<_>>();
    snapshot
        .wiki_pages
        .retain(|page| !generated_paths.contains(&page.path));
    snapshot.wiki_pages.extend(generated);
    snapshot.wiki_pages.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.page_id.cmp(&right.page_id))
    });
}
