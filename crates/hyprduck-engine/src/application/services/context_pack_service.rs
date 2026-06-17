use crate::policy;
use crate::*;

pub(crate) fn handle_get_context_pack(
    request: GetContextPackRequest,
) -> Result<GetContextPackResponseData> {
    let budget = request.budget.unwrap_or(8000);
    let pack_id = format!("ctx_{}", uuid::Uuid::now_v7().simple());
    let generated_at = current_iso_timestamp_utc();

    // Prefer DB assemble path for primary context pack (v1) per AGENTS.md (DB/GraphQLite authoritative).
    // DB assemble is attempted first for non-selection case; reader is opened only if DB assemble
    // is inapplicable (selected_node) or fails (conditional fallback, matching brain_read_service pattern).
    let db_v1 = if request.selected_node_id.is_none() {
        let root = resolve_brain_workspace_root(&request.scope)?;
        let store = KnowledgeStore::open(KnowledgeStore::default_path_for_root(&root))?;
        store
            .assemble_context_pack_v1_from_db(
                &request.scope.workspace_id,
                &request.query,
                budget,
                pack_id.clone(),
                generated_at.clone(),
            )
            .ok()
    } else {
        None
    };

    let (context_pack, context_pack_v0, context_pack_v1) = if let Some(v1) = db_v1 {
        // DB primary success path: synthesize minimal BrainContextPack for response compatibility
        // (DB assemble populates authoritative agent-facing v1/v0 with sources/evidence; graph
        // fields like nodes/claims are not part of the DB context pack projection).
        let context_pack = hyprduck_engine_types::BrainContextPack {
            workspace_id: request.scope.workspace_id.clone(),
            query: request.query.clone(),
            token_budget: budget,
            summary: "Context assembled from DB evidence using SQLite FTS5 retrieval and GraphQLite graph expansion.".into(),
            wiki_pages: Vec::new(),
            nodes: Vec::new(),
            sources: Vec::new(),
            memories: Vec::new(),
            entities: Vec::new(),
            claims: Vec::new(),
            relations: Vec::new(),
            evidence: Vec::new(),
            recent_events: Vec::new(),
            warnings: Vec::new(),
        };
        let context_pack_v0 = context_pack_v0_from_v1(v1.clone());
        (context_pack, context_pack_v0, v1)
    } else {
        // Reader path (selected_node or DB assemble failure).
        let reader = BrainReader::open(&request.scope)?;
        let context_pack = if request.selected_node_id.is_some() {
            reader.context_pack_with_selection(
                &request.query,
                budget,
                request.selected_node_id.as_deref(),
            )?
        } else {
            reader.context_pack(&request.query, budget)?
        };
        let artifact_metadata =
            build_context_pack_artifact_metadata(reader.root(), &context_pack.sources);
        let context_pack_v0 = hyprduck_engine_types::ContextPackV0::from_brain_context_pack(
            &context_pack,
            pack_id.clone(),
            generated_at.clone(),
            &artifact_metadata,
        );
        let mut context_pack_v1 = hyprduck_engine_types::ContextPackV1::from_brain_context_pack(
            &context_pack,
            pack_id.clone(),
            generated_at.clone(),
            &artifact_metadata,
        );
        if request.selected_node_id.is_none() {
            // DB was attempted (no selected) but failed -> degrade with warning (as before).
            context_pack_v1
                .warnings
                .push(policy::graph_trail_unavailable_warning(
                    "Graph trail projection failed; citation evidence remains available.",
                ));
        }
        (context_pack, context_pack_v0, context_pack_v1)
    };

    let persisted_context_pack_path = if request.persist {
        Some(persist_context_pack_v1(&request.scope, &context_pack_v1)?)
    } else {
        None
    };
    Ok(GetContextPackResponseData {
        context_pack,
        context_pack_v1,
        context_pack_v0,
        persisted_context_pack_path,
    })
}

pub(crate) fn persist_context_pack_v1(
    scope: &BrainReadScope,
    context_pack: &hyprduck_engine_types::ContextPackV1,
) -> Result<String> {
    let workspace_root = resolve_brain_workspace_root(scope)?;
    let history_dir = workspace_root.join("context_packs");
    fs::create_dir_all(&history_dir)
        .with_context(|| format!("failed creating {}", history_dir.display()))?;
    let json =
        serde_json::to_string_pretty(context_pack).context("failed encoding context pack v1")?;
    let history_path = history_dir.join(format!("{}.json", context_pack.pack_id));
    fs::write(&history_path, &json)
        .with_context(|| format!("failed writing {}", history_path.display()))?;
    let latest_path = workspace_root.join("context_pack.json");
    fs::write(&latest_path, json)
        .with_context(|| format!("failed writing {}", latest_path.display()))?;
    Ok(latest_path.display().to_string())
}

pub(crate) fn handle_read_context_pack(
    request: ReadContextPackRequest,
) -> Result<ReadContextPackResponseData> {
    let workspace_root = resolve_brain_workspace_root(&request.scope)?;
    let repo = BrainArtifactRepository::new(workspace_root);
    let path = match request.pack_id.as_deref() {
        Some(pack_id) => {
            validate_context_pack_id(pack_id)?;
            format!("context_packs/{pack_id}.json")
        }
        None => "context_pack.json".into(),
    };
    let value: Value = repo
        .read_json_artifact(&path)
        .map_err(|_| anyhow!("persisted context pack could not be read or decoded"))?;
    let schema_version = value
        .get("schemaVersion")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let context_pack = match schema_version {
        hyprduck_engine_types::CONTEXT_PACK_V0_SCHEMA_VERSION => serde_json::from_value(value)
            .map_err(|_| anyhow!("persisted context pack could not be read or decoded"))?,
        hyprduck_engine_types::CONTEXT_PACK_V1_SCHEMA_VERSION => {
            let context_pack_v1: hyprduck_engine_types::ContextPackV1 =
                serde_json::from_value(value)
                    .map_err(|_| anyhow!("persisted context pack could not be read or decoded"))?;
            context_pack_v0_from_v1(context_pack_v1)
        }
        _ => bail!(
            "persisted context pack schemaVersion {} is unsupported",
            schema_version
        ),
    };
    if let Some(pack_id) = request.pack_id.as_deref() {
        if context_pack.pack_id != pack_id {
            bail!(
                "persisted context pack packId {} does not match requested packId {}",
                context_pack.pack_id,
                pack_id
            );
        }
    }
    if context_pack.workspace_id != request.scope.workspace_id {
        bail!(
            "context pack workspace {} does not match requested workspace {}",
            context_pack.workspace_id,
            request.scope.workspace_id
        );
    }
    Ok(ReadContextPackResponseData { context_pack })
}

fn context_pack_v0_from_v1(
    context_pack: hyprduck_engine_types::ContextPackV1,
) -> hyprduck_engine_types::ContextPackV0 {
    hyprduck_engine_types::ContextPackV0 {
        schema_version: hyprduck_engine_types::CONTEXT_PACK_V0_SCHEMA_VERSION.into(),
        pack_id: context_pack.pack_id,
        workspace_id: context_pack.workspace_id,
        query: context_pack.query,
        generated_at: context_pack.generated_at,
        source_set: context_pack.source_set,
        selected_evidence: context_pack
            .selected_evidence
            .into_iter()
            .map(|evidence| hyprduck_engine_types::ContextPackEvidenceV0 {
                evidence_ref: evidence.evidence_ref,
                source_id: evidence.source_id,
                page: evidence.page,
                region: evidence.region,
                span: evidence.span,
                quoted_text: evidence.quoted_text,
                parse_confidence: evidence.parse_confidence,
                selection_reason: evidence.selection_reason,
                content_hash: evidence.content_hash,
            })
            .collect(),
        findings: context_pack.findings,
        warnings: context_pack.warnings,
        retrieval_trace: hyprduck_engine_types::ContextPackRetrievalTraceV0 {
            strategy: context_pack.retrieval_trace.strategy,
            chunks_considered: context_pack.retrieval_trace.chunks_considered,
            chunks_selected: context_pack.retrieval_trace.chunks_selected,
            budget_requested: context_pack.retrieval_trace.budget_requested,
            budget_used: context_pack.retrieval_trace.budget_used,
        },
        suggested_next_reads: context_pack.suggested_next_reads,
    }
}

fn validate_context_pack_id(pack_id: &str) -> Result<()> {
    if pack_id.trim().is_empty()
        || pack_id == "."
        || pack_id == ".."
        || pack_id.contains('/')
        || pack_id.contains('\\')
    {
        bail!("invalid packId: context pack IDs must be single path segments");
    }
    Ok(())
}

fn current_iso_timestamp_utc() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    unix_seconds_to_iso_utc(seconds)
}

fn unix_seconds_to_iso_utc(seconds: i64) -> String {
    let days = seconds.div_euclid(86_400);
    let seconds_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    let year = y + if m <= 2 { 1 } else { 0 };
    (year, m as u32, d as u32)
}
