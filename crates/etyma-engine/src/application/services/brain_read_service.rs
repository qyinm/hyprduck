use crate::*;

use crate::application::services::workspace_knowledge_read::db_then_reader;
use crate::policy;

const READ_NODE_FALLBACK_EVIDENCE_LIMIT: usize = 32;
const READ_NODE_FALLBACK_RELATION_LIMIT: usize = 64;

pub(crate) fn handle_search_brain(request: SearchBrainRequest) -> Result<SearchBrainResponseData> {
    let limit = request.limit.unwrap_or(10);
    db_then_reader(
        &request.scope,
        |store, _root| {
            let db_results =
                store.search_brain_from_db(&request.scope.workspace_id, &request.query, limit)?;
            if db_results.is_empty() {
                return Ok(None);
            }
            Ok(Some(SearchBrainResponseData {
                results: db_results,
            }))
        },
        |reader, _root| {
            Ok(SearchBrainResponseData {
                results: reader.search(&request.query, limit),
            })
        },
    )
}

pub(crate) fn handle_read_source(request: ReadSourceRequest) -> Result<ReadSourceResponseData> {
    db_then_reader(
        &request.scope,
        |store, root| {
            // Prefer the DB projection (authoritative per AGENTS.md).
            // Only fall back to the legacy BrainReader artifact path when the DB has no data for this source.
            let Some(mut response) = store.read_source_from_db(
                &request.scope.workspace_id,
                &request.source_id,
                request.include_local_paths,
            )?
            else {
                return Ok(None);
            };
            if request.include_local_paths {
                // Enrichment with real local paths still requires the artifact snapshot for now.
                if let Ok(reader) = BrainReader::open(&request.scope) {
                    policy::enrich_read_source_with_local_paths(
                        &mut response,
                        &reader.snapshot,
                        &request.source_id,
                    );
                }
                policy::expand_read_source_local_paths(&mut response, root);
            }
            Ok(Some(response))
        },
        |reader, _root| {
            // Legacy artifact-only path (kept for migration / artifact-only workspaces).
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
                policy::redact_read_source_agent_paths(&mut response);
            }
            Ok(response)
        },
    )
}

pub(crate) fn handle_read_page_evidence(
    request: ReadPageEvidenceRequest,
) -> Result<ReadPageEvidenceResponseData> {
    if request.page == Some(0) {
        bail!("argument page must be a positive 1-based integer");
    }

    db_then_reader(
        &request.scope,
        |store, root| {
            // Prefer DB projection.
            let Some(mut response) = store.read_page_evidence_from_db(
                &request.scope.workspace_id,
                &request.source_id,
                request.page,
                request.include_local_paths,
            )?
            else {
                return Ok(None);
            };
            if request.include_local_paths {
                if let Ok(reader) = BrainReader::open(&request.scope) {
                    policy::enrich_page_evidence_with_local_paths(
                        &mut response,
                        &reader.snapshot,
                        &request.source_id,
                    );
                }
                policy::expand_page_evidence_local_paths(&mut response, root);
            }
            Ok(Some(response))
        },
        |reader, _root| {
            // Legacy artifact path.
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
                policy::redact_page_evidence_agent_paths(&mut response);
            }
            Ok(response)
        },
    )
}

// Redaction and local-path enrichment/expand helpers now live in policy.rs
// (centralized "agent-safe read" contract, reusable with snapshot data or BrainReader.snapshot).
// See policy.rs for the implementation (enrich/expand take &BrainRepoSnapshot, not &BrainReader).

pub(crate) fn handle_read_wiki_page(
    request: ReadWikiPageRequest,
) -> Result<ReadWikiPageResponseData> {
    db_then_reader(
        &request.scope,
        |store, _root| {
            Ok(
                store
                    .read_wiki_page_from_db(&request.scope.workspace_id, &request.path)?
                    .map(|page| ReadWikiPageResponseData { page }),
            )
        },
        |reader, _root| {
            let page = reader.read_wiki_page(&request.path)?;
            Ok(ReadWikiPageResponseData { page })
        },
    )
}

pub(crate) fn handle_read_node(request: ReadNodeRequest) -> Result<ReadNodeResponseData> {
    db_then_reader(
        &request.scope,
        |store, _root| {
            Ok(store.read_node_from_db(&request.scope.workspace_id, &request.node_id)?)
        },
        |reader, _root| {
            let node = reader
                .snapshot
                .nodes
                .iter()
                .find(|node| node.node_id == request.node_id && node.valid_to.is_none())
                .cloned()
                .ok_or_else(|| anyhow!("node {} was not found", request.node_id))?;
            let node = sanitize_read_node_fallback_node(node)
                .ok_or_else(|| anyhow!("node {} was not found", request.node_id))?;
            let evidence_ids = node.evidence_ids.iter().collect::<BTreeSet<_>>();
            let mut evidence = reader
                .snapshot
                .evidence
                .iter()
                .filter(|evidence| evidence_ids.contains(&evidence.id))
                .take(READ_NODE_FALLBACK_EVIDENCE_LIMIT)
                .cloned()
                .collect::<Vec<_>>();
            for evidence in &mut evidence {
                policy::redact_optional_agent_path(&mut evidence.source_path);
                policy::redact_optional_agent_path(&mut evidence.markdown_path);
                policy::redact_optional_agent_path(&mut evidence.image_path);
            }
            let relations = reader
                .snapshot
                .relations
                .iter()
                .filter(|relation| {
                    relation.valid_to.is_none()
                        && (relation.source_node_id == node.node_id
                            || relation.target_node_id == node.node_id)
                })
                .filter(|relation| read_node_fallback_relation_is_agent_safe(relation))
                .take(READ_NODE_FALLBACK_RELATION_LIMIT)
                .cloned()
                .collect();
            Ok(ReadNodeResponseData {
                node,
                evidence,
                relations,
            })
        },
    )
}

fn read_node_fallback_relation_is_agent_safe(relation: &BrainRelationRecord) -> bool {
    policy::is_agent_text_safe(&relation.relation_id)
        && policy::is_agent_text_safe(&relation.label)
        && policy::is_agent_text_safe(&relation.source_node_id)
        && policy::is_agent_text_safe(&relation.target_node_id)
}

fn sanitize_read_node_fallback_node(mut node: BrainNodeRecord) -> Option<BrainNodeRecord> {
    if !policy::is_agent_text_safe(&node.node_id) || !policy::is_agent_text_safe(&node.label) {
        return None;
    }
    node.aliases
        .retain(|alias| policy::is_agent_text_safe(alias));
    Some(node)
}

pub(crate) fn handle_read_recent_events(
    request: ReadRecentEventsRequest,
) -> Result<ReadRecentEventsResponseData> {
    let reader = BrainReader::open(&request.scope)?;
    Ok(ReadRecentEventsResponseData {
        events: reader.recent_events(&request),
    })
}
