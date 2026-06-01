use crate::*;

pub(crate) fn handle_search_brain(request: SearchBrainRequest) -> Result<SearchBrainResponseData> {
    let root = resolve_brain_workspace_root(&request.scope)?;
    let store = KnowledgeStore::open(KnowledgeStore::default_path_for_root(&root))?;
    let db_results = store.search_brain_from_db(
        &request.scope.workspace_id,
        &request.query,
        request.limit.unwrap_or(10),
    )?;
    if !db_results.is_empty() {
        return Ok(SearchBrainResponseData {
            results: db_results,
        });
    }

    let reader = BrainReader::open(&request.scope)?;
    Ok(SearchBrainResponseData {
        results: reader.search(&request.query, request.limit.unwrap_or(10)),
    })
}

pub(crate) fn handle_read_source(request: ReadSourceRequest) -> Result<ReadSourceResponseData> {
    let root = resolve_brain_workspace_root(&request.scope)?;
    let store = KnowledgeStore::open(KnowledgeStore::default_path_for_root(&root))?;
    if let Some(mut response) = store.read_source_from_db(
        &request.scope.workspace_id,
        &request.source_id,
        request.include_local_paths,
    )? {
        if request.include_local_paths {
            if let Ok(reader) = BrainReader::open(&request.scope) {
                enrich_read_source_with_local_paths(&mut response, &reader, &request.source_id);
            }
            expand_read_source_local_paths(&mut response, &root);
        }
        return Ok(response);
    }
    let reader = BrainReader::open(&request.scope)?;
    if let Some(mut response) = store.read_source_from_db(
        &request.scope.workspace_id,
        &request.source_id,
        request.include_local_paths,
    )? {
        if request.include_local_paths {
            enrich_read_source_with_local_paths(&mut response, &reader, &request.source_id);
            expand_read_source_local_paths(&mut response, &root);
        }
        return Ok(response);
    }
    let source = reader
        .snapshot
        .sources
        .iter()
        .find(|source| source.source_id == request.source_id)
        .cloned()
        .ok_or_else(|| anyhow!("source {} was not found", request.source_id))?;
    let wiki_page = reader
        .snapshot
        .wiki_pages
        .iter()
        .find(|page| {
            page.source_refs
                .iter()
                .any(|source_ref| source_ref == &source.source_id)
        })
        .cloned()
        .map(|page| reader.read_wiki_page_body(page))
        .transpose()?;
    let evidence = reader
        .snapshot
        .evidence
        .iter()
        .filter(|evidence| evidence.source_id.as_deref() == Some(source.source_id.as_str()))
        .cloned()
        .collect();
    let mut response = ReadSourceResponseData {
        source,
        wiki_page,
        evidence,
    };
    if !request.include_local_paths {
        redact_read_source_agent_paths(&mut response);
    }
    Ok(response)
}

pub(crate) fn handle_read_page_evidence(
    request: ReadPageEvidenceRequest,
) -> Result<ReadPageEvidenceResponseData> {
    if request.page == Some(0) {
        bail!("argument page must be a positive 1-based integer");
    }

    let root = resolve_brain_workspace_root(&request.scope)?;
    let store = KnowledgeStore::open(KnowledgeStore::default_path_for_root(&root))?;
    if let Some(mut response) = store.read_page_evidence_from_db(
        &request.scope.workspace_id,
        &request.source_id,
        request.page,
        request.include_local_paths,
    )? {
        if request.include_local_paths {
            if let Ok(reader) = BrainReader::open(&request.scope) {
                enrich_page_evidence_with_local_paths(&mut response, &reader, &request.source_id);
            }
            expand_page_evidence_local_paths(&mut response, &root);
        }
        return Ok(response);
    }

    let reader = BrainReader::open(&request.scope)?;
    let source = reader
        .snapshot
        .sources
        .iter()
        .find(|source| source.source_id == request.source_id)
        .cloned()
        .ok_or_else(|| anyhow!("source {} was not found", request.source_id))?;

    let artifact_metadata =
        build_context_pack_artifact_metadata(reader.root(), std::slice::from_ref(&source));
    let mut evidence = artifact_metadata
        .evidence
        .get(&source.source_id)
        .into_iter()
        .flat_map(|source_evidence| source_evidence.iter())
        .filter(|(_, metadata)| request.page.map_or(true, |page| metadata.page == page))
        .map(|(evidence_ref, metadata)| PageEvidenceV0 {
            evidence_ref: evidence_ref.clone(),
            source_id: metadata.source_id.clone(),
            page: metadata.page,
            region: metadata
                .region
                .clone()
                .unwrap_or_else(|| format!("page:{}", metadata.page)),
            span: metadata.span.clone(),
            quoted_text: metadata.quoted_text.clone(),
            parse_confidence: metadata.parse_confidence.clone(),
            content_hash: metadata.content_hash.clone(),
            markdown_path: metadata.markdown_path.clone(),
            image_path: metadata.image_path.clone(),
        })
        .collect::<Vec<_>>();
    evidence.sort_by(|left, right| {
        left.page
            .cmp(&right.page)
            .then_with(|| left.evidence_ref.cmp(&right.evidence_ref))
    });

    let mut response = ReadPageEvidenceResponseData {
        source,
        evidence,
        warnings: artifact_metadata.warnings,
    };
    if !request.include_local_paths {
        redact_page_evidence_agent_paths(&mut response);
    }
    Ok(response)
}

fn redact_read_source_agent_paths(response: &mut ReadSourceResponseData) {
    redact_source_record_agent_paths(&mut response.source);
    for evidence in &mut response.evidence {
        redact_optional_agent_path(&mut evidence.source_path);
        redact_optional_agent_path(&mut evidence.markdown_path);
        redact_optional_agent_path(&mut evidence.image_path);
    }
}

fn redact_page_evidence_agent_paths(response: &mut ReadPageEvidenceResponseData) {
    redact_source_record_agent_paths(&mut response.source);
    for evidence in &mut response.evidence {
        redact_optional_agent_path(&mut evidence.markdown_path);
        redact_optional_agent_path(&mut evidence.image_path);
    }
}

fn redact_source_record_agent_paths(source: &mut SourceRecord) {
    source.original_path = redact_agent_path(&source.original_path);
    source.source_path = redact_agent_path(&source.source_path);
    source.markdown_path = redact_agent_path(&source.markdown_path);
}

fn redact_optional_agent_path(value: &mut Option<String>) {
    if let Some(path) = value {
        *path = redact_agent_path(path);
    }
}

fn redact_agent_path(value: &str) -> String {
    if value.is_empty() {
        return String::new();
    }
    Path::new(value)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "<redacted>".into())
}

fn enrich_read_source_with_local_paths(
    response: &mut ReadSourceResponseData,
    reader: &BrainReader,
    source_id: &str,
) {
    if let Some(source) = reader
        .snapshot
        .sources
        .iter()
        .find(|source| source.source_id == source_id)
    {
        response.source.original_path = source.original_path.clone();
        response.source.source_path = source.source_path.clone();
        response.source.markdown_path = source.markdown_path.clone();
    }

    let evidence_by_id = reader
        .snapshot
        .evidence
        .iter()
        .map(|evidence| (evidence.id.as_str(), evidence))
        .collect::<BTreeMap<_, _>>();
    for evidence in &mut response.evidence {
        if let Some(raw) = evidence_by_id.get(evidence.id.as_str()) {
            evidence.source_path = raw.source_path.clone();
            evidence.markdown_path = raw.markdown_path.clone();
            evidence.image_path = raw.image_path.clone();
        }
    }
}

fn expand_read_source_local_paths(response: &mut ReadSourceResponseData, workspace_root: &Path) {
    expand_source_record_local_paths(&mut response.source, workspace_root);
    for evidence in &mut response.evidence {
        let source_id = evidence
            .source_id
            .as_deref()
            .unwrap_or(response.source.source_id.as_str());
        expand_optional_path(
            &mut evidence.source_path,
            workspace_root,
            &["sources", source_id],
        );
        expand_optional_path(
            &mut evidence.markdown_path,
            workspace_root,
            &["artifacts", source_id, "pages"],
        );
        expand_optional_path(
            &mut evidence.image_path,
            workspace_root,
            &["artifacts", source_id, "images"],
        );
    }
}

fn enrich_page_evidence_with_local_paths(
    response: &mut ReadPageEvidenceResponseData,
    reader: &BrainReader,
    source_id: &str,
) {
    if let Some(source) = reader
        .snapshot
        .sources
        .iter()
        .find(|source| source.source_id == source_id)
    {
        response.source.original_path = source.original_path.clone();
        response.source.source_path = source.source_path.clone();
        response.source.markdown_path = source.markdown_path.clone();
    }

    let evidence_by_id = reader
        .snapshot
        .evidence
        .iter()
        .map(|evidence| (evidence.id.as_str(), evidence))
        .collect::<BTreeMap<_, _>>();
    for evidence in &mut response.evidence {
        if let Some(raw) = evidence_by_id.get(evidence.evidence_ref.as_str()) {
            evidence.markdown_path = raw.markdown_path.clone();
            evidence.image_path = raw.image_path.clone();
        }
    }
}

fn expand_page_evidence_local_paths(
    response: &mut ReadPageEvidenceResponseData,
    workspace_root: &Path,
) {
    expand_source_record_local_paths(&mut response.source, workspace_root);
    let source_id = response.source.source_id.as_str();
    for evidence in &mut response.evidence {
        expand_optional_path(
            &mut evidence.markdown_path,
            workspace_root,
            &["artifacts", source_id, "pages"],
        );
        expand_optional_path(
            &mut evidence.image_path,
            workspace_root,
            &["artifacts", source_id, "images"],
        );
    }
}

fn expand_source_record_local_paths(source: &mut SourceRecord, workspace_root: &Path) {
    expand_string_path(&mut source.original_path, workspace_root, &[]);
    expand_string_path(
        &mut source.source_path,
        workspace_root,
        &["sources", source.source_id.as_str()],
    );
    expand_string_path(
        &mut source.markdown_path,
        workspace_root,
        &["artifacts", source.source_id.as_str()],
    );
}

fn expand_optional_path(value: &mut Option<String>, workspace_root: &Path, segments: &[&str]) {
    if let Some(path) = value {
        expand_string_path(path, workspace_root, segments);
    }
}

fn expand_string_path(value: &mut String, workspace_root: &Path, segments: &[&str]) {
    if value.is_empty() || value == "[redacted-local-path]" || Path::new(value).is_absolute() {
        return;
    }
    let mut path = workspace_root.to_path_buf();
    for segment in segments {
        path.push(segment);
    }
    path.push(value.as_str());
    *value = path.to_string_lossy().into_owned();
}

pub(crate) fn handle_read_wiki_page(
    request: ReadWikiPageRequest,
) -> Result<ReadWikiPageResponseData> {
    let root = resolve_brain_workspace_root(&request.scope)?;
    let store = KnowledgeStore::open(KnowledgeStore::default_path_for_root(&root))?;
    if let Some(page) = store.read_wiki_page_from_db(&request.scope.workspace_id, &request.path)? {
        return Ok(ReadWikiPageResponseData { page });
    }
    let reader = BrainReader::open(&request.scope)?;
    let page = reader.read_wiki_page(&request.path)?;
    Ok(ReadWikiPageResponseData { page })
}

pub(crate) fn handle_read_node(request: ReadNodeRequest) -> Result<ReadNodeResponseData> {
    let root = resolve_brain_workspace_root(&request.scope)?;
    let store = KnowledgeStore::open(KnowledgeStore::default_path_for_root(&root))?;
    if let Some(response) =
        store.read_node_from_db(&request.scope.workspace_id, &request.node_id)?
    {
        return Ok(response);
    }
    let reader = BrainReader::open(&request.scope)?;
    let node = reader
        .snapshot
        .nodes
        .iter()
        .find(|node| node.node_id == request.node_id)
        .cloned()
        .ok_or_else(|| anyhow!("node {} was not found", request.node_id))?;
    let evidence_ids = node.evidence_ids.iter().collect::<BTreeSet<_>>();
    let evidence = reader
        .snapshot
        .evidence
        .iter()
        .filter(|evidence| evidence_ids.contains(&evidence.id))
        .cloned()
        .collect();
    let relations = reader
        .snapshot
        .relations
        .iter()
        .filter(|relation| {
            relation.source_node_id == node.node_id || relation.target_node_id == node.node_id
        })
        .cloned()
        .collect();
    Ok(ReadNodeResponseData {
        node,
        evidence,
        relations,
    })
}

pub(crate) fn handle_read_recent_events(
    request: ReadRecentEventsRequest,
) -> Result<ReadRecentEventsResponseData> {
    let reader = BrainReader::open(&request.scope)?;
    Ok(ReadRecentEventsResponseData {
        events: reader.recent_events(&request),
    })
}
