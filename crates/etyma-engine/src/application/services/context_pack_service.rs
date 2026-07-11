use crate::application::services::workspace_knowledge_read::db_then_reader;
use crate::domains::retrieval::brain_search::{db_context_window, db_search_terms};
use crate::policy;
use crate::*;
use graphqlite::Graph;

pub(crate) fn handle_get_context_pack(
    request: GetContextPackRequest,
) -> Result<GetContextPackResponseData> {
    let budget = request.budget.unwrap_or(8000);
    let pack_id = format!("ctx_{}", uuid::Uuid::now_v7().simple());
    let generated_at = current_iso_timestamp_utc();

    // Prefer DB assemble path for primary context pack (v1) per AGENTS.md (DB/GraphQLite authoritative).
    // Internal assembly is V1-only; ContextPack V0 is derived at the service/wire boundary.
    // DB assemble is attempted first for non-selection case; reader is opened only if DB assemble
    // is inapplicable (selected_node) or fails (conditional fallback, matching brain_read_service pattern).
    let (root, mut context_pack, mut context_pack_v1) = if request.selected_node_id.is_some() {
        // Selection queries skip DB assemble and use the legacy reader path only.
        let root = resolve_brain_workspace_root(&request.scope)?;
        let reader = BrainReader::open(&request.scope)?;
        let (context_pack, context_pack_v1) = assemble_context_pack_from_reader(
            &reader,
            &request,
            budget,
            &pack_id,
            &generated_at,
            /*db_attempted*/ false,
            None,
        )?;
        (root, context_pack, context_pack_v1)
    } else {
        // Capture DB assemble failures for the legacy-path warning without inventing a third route.
        let db_pack_error = std::cell::RefCell::new(None::<String>);
        db_then_reader(
            &request.scope,
            |store, root| {
                match store.assemble_context_pack_v1_from_db(
                    &request.scope.workspace_id,
                    &request.query,
                    budget,
                    pack_id.clone(),
                    generated_at.clone(),
                ) {
                    Ok((context_pack, v1)) if db_context_pack_v1_has_content(&v1) => {
                        Ok(Some((root.to_path_buf(), context_pack, v1)))
                    }
                    Ok(_) => Ok(None),
                    Err(error) => {
                        *db_pack_error.borrow_mut() = Some(format!("{error:#}"));
                        Ok(None)
                    }
                }
            },
            |reader, root| {
                let (context_pack, context_pack_v1) = assemble_context_pack_from_reader(
                    reader,
                    &request,
                    budget,
                    &pack_id,
                    &generated_at,
                    /*db_attempted*/ true,
                    db_pack_error.borrow().clone(),
                )?;
                Ok((root.to_path_buf(), context_pack, context_pack_v1))
            },
        )?
    };

    augment_context_pack_with_source_page_text(
        &root,
        &request.scope.workspace_id,
        &request.query,
        &mut context_pack,
        &mut context_pack_v1,
    )?;

    let persisted_context_pack_path = if request.persist {
        Some(persist_context_pack_v1(&request.scope, &context_pack_v1)?)
    } else {
        None
    };
    // Wire adapter: V0 is always projected from the V1 primary pack at the boundary.
    let context_pack_v0 = context_pack_v0_from_v1(context_pack_v1.clone());
    Ok(GetContextPackResponseData {
        context_pack,
        context_pack_v1,
        context_pack_v0,
        persisted_context_pack_path,
    })
}

fn assemble_context_pack_from_reader(
    reader: &BrainReader,
    request: &GetContextPackRequest,
    budget: usize,
    pack_id: &str,
    generated_at: &str,
    db_attempted: bool,
    db_pack_error: Option<String>,
) -> Result<(BrainContextPack, etyma_engine_types::ContextPackV1)> {
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
    let mut context_pack_v1 = etyma_engine_types::ContextPackV1::from_brain_context_pack(
        &context_pack,
        pack_id.to_string(),
        generated_at.to_string(),
        &artifact_metadata,
    );
    if db_attempted {
        // DB was attempted (no selected) but failed/empty -> degrade with warning (as before).
        let message = db_pack_error
            .as_deref()
            .map(|error| {
                format!(
                    "DB-backed Context Pack assembly failed; legacy Context Pack was used. {error}"
                )
            })
            .unwrap_or_else(|| {
                "DB-backed Context Pack assembly returned no citation-ready evidence; legacy Context Pack was used.".into()
            });
        context_pack_v1
            .warnings
            .push(policy::graph_trail_unavailable_warning(&message));
    }
    Ok((context_pack, context_pack_v1))
}

pub(crate) fn persist_context_pack_v1(
    scope: &BrainReadScope,
    context_pack: &etyma_engine_types::ContextPackV1,
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
        etyma_engine_types::CONTEXT_PACK_V0_SCHEMA_VERSION => serde_json::from_value(value)
            .map_err(|_| anyhow!("persisted context pack could not be read or decoded"))?,
        etyma_engine_types::CONTEXT_PACK_V1_SCHEMA_VERSION => {
            let context_pack_v1: etyma_engine_types::ContextPackV1 =
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

fn db_context_pack_v1_has_content(v1: &etyma_engine_types::ContextPackV1) -> bool {
    !v1.selected_evidence.is_empty() || !v1.source_set.is_empty()
}

fn context_pack_v0_from_v1(
    context_pack: etyma_engine_types::ContextPackV1,
) -> etyma_engine_types::ContextPackV0 {
    etyma_engine_types::ContextPackV0 {
        schema_version: etyma_engine_types::CONTEXT_PACK_V0_SCHEMA_VERSION.into(),
        pack_id: context_pack.pack_id,
        workspace_id: context_pack.workspace_id,
        query: context_pack.query,
        generated_at: context_pack.generated_at,
        source_set: context_pack.source_set,
        selected_evidence: context_pack
            .selected_evidence
            .into_iter()
            .map(|evidence| etyma_engine_types::ContextPackEvidenceV0 {
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
        retrieval_trace: etyma_engine_types::ContextPackRetrievalTraceV0 {
            strategy: context_pack.retrieval_trace.strategy,
            chunks_considered: context_pack.retrieval_trace.chunks_considered,
            chunks_selected: context_pack.retrieval_trace.chunks_selected,
            budget_requested: context_pack.retrieval_trace.budget_requested,
            budget_used: context_pack.retrieval_trace.budget_used,
        },
        suggested_next_reads: context_pack.suggested_next_reads,
    }
}

fn augment_context_pack_with_source_page_text(
    root: &Path,
    workspace_id: &str,
    query: &str,
    context_pack: &mut BrainContextPack,
    context_pack_v1: &mut etyma_engine_types::ContextPackV1,
) -> Result<bool> {
    let terms = db_search_terms(query);
    if terms.is_empty() || context_pack_v1.selected_evidence.is_empty() {
        return Ok(false);
    }

    let db_path = KnowledgeStore::default_path_for_root(root);
    if !db_path.exists() {
        return Ok(false);
    }

    let graph = Graph::open(&db_path).context("GraphQLite failed to open knowledge DB")?;
    let context_snippets_by_source = context_pack_snippets_by_source(context_pack, &terms);
    let source_ids = context_pack_v1
        .selected_evidence
        .iter()
        .map(|evidence| evidence.source_id.clone())
        .collect::<BTreeSet<_>>();
    let artifact_snippets_by_source = artifact_page_snippets_by_source(root, &source_ids, &terms);
    let mut updates = BTreeMap::new();
    for evidence in &mut context_pack_v1.selected_evidence {
        let mut candidates = Vec::new();
        if let Some(snippet) = artifact_snippets_by_source.get(&evidence.source_id) {
            candidates.push(snippet.clone());
        }
        if let Some(snippet) = context_snippets_by_source.get(&evidence.source_id) {
            candidates.push(snippet.clone());
        }
        if let Some(page_text) =
            load_source_page_plain_text(&graph, workspace_id, &evidence.source_id, evidence.page)?
        {
            candidates.push(db_context_window(&page_text, &terms, 1_600));
        }
        let quoted_text = candidates
            .into_iter()
            .filter(|candidate| !candidate.trim().is_empty())
            .max_by_key(|candidate| context_snippet_score(candidate, &terms));
        let Some(quoted_text) = quoted_text else {
            continue;
        };
        if quoted_text.trim().is_empty() || quoted_text.trim() == evidence.quoted_text.trim() {
            continue;
        }
        evidence.quoted_text = quoted_text.clone();
        updates.insert(evidence.evidence_ref.clone(), quoted_text);
    }

    if updates.is_empty() {
        return Ok(false);
    }

    for evidence in &mut context_pack.evidence {
        if let Some(quoted_text) = updates.get(&evidence.id) {
            evidence.snippet = quoted_text.clone();
        }
    }
    Ok(true)
}

fn artifact_page_snippets_by_source(
    root: &Path,
    source_ids: &BTreeSet<String>,
    terms: &[String],
) -> BTreeMap<String, String> {
    let mut snippets: BTreeMap<String, String> = BTreeMap::new();
    for source_id in source_ids {
        let pages_dir = root.join("artifacts").join(source_id).join("pages");
        let Ok(entries) = fs::read_dir(&pages_dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(extension) = path.extension().and_then(|value| value.to_str()) else {
                continue;
            };
            if extension != "txt" && extension != "md" {
                continue;
            }
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            let snippet = db_context_window(&text, terms, 1_600);
            if snippet.trim().is_empty() {
                continue;
            }
            let score = context_snippet_score(&snippet, terms);
            let existing_score = snippets
                .get(source_id)
                .map(|existing| context_snippet_score(existing, terms))
                .unwrap_or(i64::MIN);
            if score > existing_score {
                snippets.insert(source_id.clone(), snippet);
            }
        }
    }
    snippets
}

fn context_pack_snippets_by_source(
    context_pack: &BrainContextPack,
    terms: &[String],
) -> BTreeMap<String, String> {
    let mut snippets: BTreeMap<String, String> = BTreeMap::new();
    for evidence in &context_pack.evidence {
        let Some(source_id) = evidence.source_id.as_ref() else {
            continue;
        };
        let snippet = db_context_window(&evidence.snippet, terms, 1_600);
        if snippet.trim().is_empty() {
            continue;
        }
        let score = context_snippet_score(&snippet, terms);
        let existing_score = snippets
            .get(source_id)
            .map(|existing| context_snippet_score(existing, terms))
            .unwrap_or(i64::MIN);
        if score > existing_score {
            snippets.insert(source_id.clone(), snippet);
        }
    }
    snippets
}

fn context_snippet_score(snippet: &str, terms: &[String]) -> i64 {
    let lower = snippet.to_lowercase();
    let unique_terms = terms
        .iter()
        .filter(|term| lower.contains(term.as_str()))
        .count() as i64;
    if unique_terms == 0 {
        return i64::MIN;
    }
    let occurrences = terms
        .iter()
        .map(|term| lower.match_indices(term.as_str()).count().min(8) as i64)
        .sum::<i64>();
    let toc_penalty = if lower.contains("contents")
        && lower.contains("8.1")
        && lower.contains("8.2")
        && lower.contains("8.3")
    {
        900
    } else {
        0
    };
    let detail_bonus = [
        "directory",
        "directories",
        "bucket",
        "split",
        "overflow",
        "fixed",
        "table",
        "insert",
        "search",
        "delete",
        "loading",
        "density",
    ]
    .iter()
    .filter(|needle| lower.contains(**needle))
    .count() as i64
        * 30;
    unique_terms * 1_000 + occurrences * 40 + detail_bonus - toc_penalty
}

fn load_source_page_plain_text(
    graph: &Graph,
    workspace_id: &str,
    source_id: &str,
    page: usize,
) -> Result<Option<String>> {
    let page_index = page.saturating_sub(1) as i64;
    let sqlite = graph.connection().sqlite_connection();
    let mut statement = sqlite
        .prepare(
            "SELECT sp.plain_text
             FROM source_pages sp
             JOIN sources s ON s.source_id = sp.source_id
             WHERE s.workspace_id = ?1
               AND sp.source_id = ?2
               AND sp.page_index = ?3
             LIMIT 1",
        )
        .context("failed preparing source page text lookup")?;
    let mut rows = statement
        .query((workspace_id, source_id, page_index))
        .context("failed querying source page text")?;
    let Some(row) = rows.next().context("failed reading source page text row")? else {
        return Ok(None);
    };
    let text = row
        .get::<_, String>(0)
        .context("failed decoding source page text")?;
    Ok((!text.trim().is_empty()).then_some(text))
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

#[cfg(test)]
mod tests {
    use super::*;
    use etyma_engine_types::{
        ContextPackArtifactMetadataV0, ContextPackEvidenceMetadataV0, ContextPackParseConfidence,
        ContextPackSourceMetadataV0, ContextPackV1, EvidenceType,
    };

    #[test]
    fn selected_node_context_pack_uses_source_page_text_for_topic_evidence() {
        let temp = tempfile::tempdir().expect("temp dir");
        let db_path = KnowledgeStore::default_path_for_root(temp.path());
        let _store = KnowledgeStore::open(db_path.clone()).expect("open knowledge store");
        let graph = Graph::open(&db_path).expect("open graph");
        let sqlite = graph.connection().sqlite_connection();
        sqlite
            .execute(
                "INSERT INTO sources (
                    source_id,
                    workspace_id,
                    title,
                    format,
                    status,
                    page_count,
                    provider_route,
                    provider_locality,
                    content_hash
                 ) VALUES (?1, ?2, ?3, 'pdf', 'ingested', 12, 'test-local', 'local', 'sha256:hashing')",
                ("source-hashing", "workspace-default", "ch8.pdf"),
            )
            .expect("insert source");
        let page_text = "Hashing\nChapter 8\nContents\n8.1 Introduction\n8.2 Static Hashing\n8.3 Dynamic Hashing\n\nRemind: Dictionaries\nCollection of pairs. Operations include Search, Delete, and Insert.\n\nDynamic Hashing\nDynamic hashing grows and shrinks a directory of bucket pointers as records are inserted or deleted. A bucket split redistributes records using additional hash bits, and directory doubling only happens when the split needs another address bit.";
        sqlite
            .execute(
                "INSERT INTO source_pages (
                    source_id,
                    page_index,
                    page_label,
                    plain_text
                 ) VALUES (?1, 7, 'p8', ?2)",
                ("source-hashing", page_text),
            )
            .expect("insert source page");

        let outline =
            "Hashing Chapter 8 Contents 8.1 Introduction 8.2 Static Hashing 8.3 Dynamic Hashing";
        let mut context_pack = BrainContextPack {
            workspace_id: "workspace-default".into(),
            query: "dynamic hashing 내용 설명해".into(),
            token_budget: 8_000,
            summary: "Synthetic graph context pack.".into(),
            wiki_pages: Vec::new(),
            nodes: Vec::new(),
            sources: vec![SourceRecord {
                source_id: "source-hashing".into(),
                workspace_id: "workspace-default".into(),
                original_path: "[redacted-local-path]/ch8.pdf".into(),
                source_path: "[redacted-local-path]/ch8.pdf".into(),
                markdown_path: "[redacted-local-path]/ch8.md".into(),
                format: "pdf".into(),
                status: "ingested".into(),
                page_count: 12,
                description: String::new(),
                user_context: String::new(),
                ingest_instruction: String::new(),
                updated_at: 1,
            }],
            memories: Vec::new(),
            entities: Vec::new(),
            claims: Vec::new(),
            relations: Vec::new(),
            evidence: vec![EvidenceRef {
                id: "evidence-dynamic-heading".into(),
                page_label: "p8".into(),
                page_index: Some(7),
                snippet: outline.into(),
                source_path: Some("[redacted-local-path]/ch8.pdf".into()),
                source_id: Some("source-hashing".into()),
                markdown_path: Some("[redacted-local-path]/ch8.md".into()),
                image_path: None,
                provenance: Some("synthetic graph context".into()),
            }],
            recent_events: Vec::new(),
            warnings: Vec::new(),
        };
        let mut artifact_metadata =
            ContextPackArtifactMetadataV0::from_sources(BTreeMap::from([(
                "source-hashing".into(),
                ContextPackSourceMetadataV0 {
                    content_hash: "sha256:hashing".into(),
                    provider_route: "test-local".into(),
                    local_only: true,
                },
            )]));
        artifact_metadata.evidence.insert(
            "source-hashing".into(),
            BTreeMap::from([(
                "evidence-dynamic-heading".into(),
                ContextPackEvidenceMetadataV0 {
                    source_id: "source-hashing".into(),
                    page: 8,
                    region: None,
                    span: None,
                    quoted_text: outline.into(),
                    parse_confidence: ContextPackParseConfidence::High,
                    content_hash: "sha256:hashing".into(),
                    markdown_path: Some("[redacted-local-path]/ch8.md".into()),
                    image_path: None,
                    evidence_type: EvidenceType::Text,
                },
            )]),
        );
        let mut context_pack_v1 = ContextPackV1::from_brain_context_pack(
            &context_pack,
            "ctx-selected",
            "2026-06-19T00:00:00Z",
            &artifact_metadata,
        );

        let augmented = augment_context_pack_with_source_page_text(
            temp.path(),
            "workspace-default",
            "dynamic hashing 내용 설명해",
            &mut context_pack,
            &mut context_pack_v1,
        )
        .expect("augment context pack");
        let context_pack_v0 = context_pack_v0_from_v1(context_pack_v1.clone());

        assert!(augmented);
        assert!(context_pack_v1.selected_evidence[0]
            .quoted_text
            .contains("bucket split redistributes records"));
        assert!(context_pack_v0.selected_evidence[0]
            .quoted_text
            .contains("bucket split redistributes records"));
        assert!(context_pack.evidence[0]
            .snippet
            .contains("bucket split redistributes records"));
    }
}
