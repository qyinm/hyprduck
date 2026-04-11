use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine;
use duckdocs_engine_types::{
    AnswerResponse, AnswerStatus, CompileProjectRequest, CompileProjectResponseData,
    CorrectionAction, CorrectionKind, DocumentFormat, EngineCommand, EngineConfigPayload,
    EngineFailure, EngineRequest, EngineSuccess, EvidenceRef, GraphNodeDetail, GraphNodeKind,
    GraphNodePosition, GraphNodeSummary, KnowledgeProject, LoadConfigRequest, LoadProjectRequest,
    LoadProjectResponseData, OutputAsset, ParseEvent, ParseInput, ParseMetadata, ParseOptions,
    ParseRequest, ParseResponseData, ParseResult, ParsedPage, ProjectOverview, ProjectStatus,
    ProviderOption, RelationEdgeDetail, RelationEdgeSummary, RelationKind, SaveConfigRequest,
    SaveConfigResponseData, SuggestedAction, SuggestedActionKind, ValidateProviderRequest,
    ValidateProviderResponseData, ValidationIssue,
};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use tempfile::tempdir;

fn main() {
    if let Err(error) = run() {
        eprintln!("{error:?}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let mut payload = String::new();
    io::stdin()
        .read_to_string(&mut payload)
        .context("failed to read engine request")?;
    let request = decode_request(&payload)?;
    let config_store = EngineConfigStore::default()?;

    match request {
        EngineRequest::Parse(request) => {
            maybe_write_debug(&request.options.debug_request_path, &payload)?;
            let debug_result_path = request.options.debug_result_path.clone();
            let response = handle_parse(request, &config_store)
                .map(|data| {
                    serde_json::to_string_pretty(&EngineSuccess::new(EngineCommand::Parse, data))
                })
                .unwrap_or_else(|error| {
                    let _ = emit_event(&ParseEvent::Failed {
                        message: error.to_string(),
                    });
                    serde_json::to_string_pretty(&engine_failure(EngineCommand::Parse, &error))
                })
                .context("failed to encode parse response")?;
            maybe_write_debug(&debug_result_path, &response)?;
            io::stdout()
                .write_all(response.as_bytes())
                .context("failed to write parse response")?;
        }
        EngineRequest::CompileProject(request) => {
            let payload = EngineSuccess::new(
                EngineCommand::CompileProject,
                handle_compile_project(request)?,
            );
            write_response(&payload)?;
        }
        EngineRequest::LoadProject(request) => {
            let payload =
                EngineSuccess::new(EngineCommand::LoadProject, handle_load_project(request)?);
            write_response(&payload)?;
        }
        EngineRequest::LoadConfig(LoadConfigRequest {}) => {
            let config = config_store.load()?;
            let payload = EngineSuccess::new(EngineCommand::LoadConfig, config.to_payload());
            write_response(&payload)?;
        }
        EngineRequest::SaveConfig(SaveConfigRequest { config }) => {
            let config = EngineConfig::from_payload(config);
            config_store.save(&config)?;
            let payload = EngineSuccess::new(
                EngineCommand::SaveConfig,
                SaveConfigResponseData {
                    config: config.to_payload(),
                    persisted: true,
                },
            );
            write_response(&payload)?;
        }
        EngineRequest::ValidateProvider(ValidateProviderRequest { config }) => {
            let config = config
                .map(EngineConfig::from_payload)
                .unwrap_or(config_store.load()?);
            let payload =
                EngineSuccess::new(EngineCommand::ValidateProvider, validate_provider(&config));
            write_response(&payload)?;
        }
    }
    Ok(())
}

fn decode_request(payload: &str) -> Result<EngineRequest> {
    serde_json::from_str(payload)
        .or_else(|_| serde_json::from_str::<ParseRequest>(payload).map(EngineRequest::Parse))
        .context("failed to decode engine request JSON")
}

fn write_response<T: Serialize>(payload: &T) -> Result<()> {
    let output =
        serde_json::to_string_pretty(payload).context("failed to encode engine response")?;
    io::stdout()
        .write_all(output.as_bytes())
        .context("failed to write engine response")
}

fn maybe_write_debug(path: &Option<String>, contents: &str) -> Result<()> {
    if let Some(path) = path {
        if let Some(parent) = Path::new(path).parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed creating debug directory {}", parent.display()))?;
        }
        fs::write(path, contents)
            .with_context(|| format!("failed writing debug artifact {}", path))?;
    }
    Ok(())
}

fn emit_event(event: &ParseEvent) -> Result<()> {
    let line = serde_json::to_string(event).context("failed to encode parse event")?;
    eprintln!("{line}");
    Ok(())
}

#[derive(Debug)]
struct ParsedDocument {
    pages: Vec<ParsedPage>,
    assets: Vec<OutputAsset>,
    page_count: usize,
    success_count: usize,
    failed_count: usize,
}

fn handle_parse(
    request: ParseRequest,
    config_store: &EngineConfigStore,
) -> Result<ParseResponseData> {
    let started = Instant::now();
    let config = config_store.load()?;

    emit_event(&ParseEvent::Queued)?;
    emit_event(&ParseEvent::DocumentOpened {
        format: request.input.format.clone(),
    })?;

    let parse = parse_document(&request.input, &request.template, &request.options, &config)?;
    let markdown = build_markdown(
        request
            .output
            .as_ref()
            .and_then(|target| target.name.clone())
            .unwrap_or_else(|| {
                Path::new(&request.input.path)
                    .file_stem()
                    .map(|value| value.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "document".to_string())
            }),
        &parse.pages,
    );

    emit_event(&ParseEvent::Packaging)?;
    let result = ParseResult {
        version: request.version.clone(),
        markdown,
        pages: parse.pages,
        assets: parse.assets,
        metadata: ParseMetadata {
            engine_id: format!("{}/{}", config.provider.id_slug(), config.model_id),
            duration_ms: started.elapsed().as_millis() as u64,
            page_count: parse.page_count,
        },
        success_count: parse.success_count,
        failed_count: parse.failed_count,
    };

    let saved_output_path = export_output_package(&request, &result)?;
    emit_event(&ParseEvent::Completed)?;
    Ok(ParseResponseData {
        result,
        saved_output_path,
    })
}

fn handle_compile_project(request: CompileProjectRequest) -> Result<CompileProjectResponseData> {
    let markdown = fs::read_to_string(&request.source_markdown_path).with_context(|| {
        format!(
            "failed reading markdown package {}",
            request.source_markdown_path
        )
    })?;
    let project = compile_knowledge_project(&request, &markdown);
    let store = KnowledgeProjectStore::default()?;
    store.save_project(&project, &request)?;
    Ok(CompileProjectResponseData {
        project_id: project.summary.project_id,
    })
}

fn handle_load_project(request: LoadProjectRequest) -> Result<LoadProjectResponseData> {
    let store = KnowledgeProjectStore::default()?;
    let project = store.load_project(request.project_id.as_deref())?;
    Ok(LoadProjectResponseData { project })
}

#[derive(Debug, Clone)]
struct PageSection {
    page_label: String,
    content: String,
}

#[derive(Debug, Clone)]
struct ConceptAccumulator {
    id: String,
    label: String,
    aliases: BTreeSet<String>,
    evidence: Vec<EvidenceRef>,
    page_labels: BTreeSet<String>,
}

#[derive(Debug, Clone)]
struct PageConceptSet {
    page_label: String,
    concept_ids: Vec<String>,
    snippet: String,
}

#[derive(Debug, Clone)]
struct CollectedConcepts {
    concepts: Vec<ConceptAccumulator>,
    page_concepts: Vec<PageConceptSet>,
}

#[derive(Debug, Clone)]
struct EdgeAccumulator {
    source_node_id: String,
    target_node_id: String,
    evidence: Vec<EvidenceRef>,
    page_labels: BTreeSet<String>,
}

fn compile_knowledge_project(request: &CompileProjectRequest, markdown: &str) -> KnowledgeProject {
    let title = infer_markdown_title(&request.source_markdown_path, markdown);
    let page_sections = extract_page_sections(markdown);
    let source_path = request
        .source_document_path
        .clone()
        .unwrap_or_else(|| request.source_markdown_path.clone());
    let project_id = build_project_id(request);

    let collected = collect_concepts(&page_sections, &source_path);
    let concept_accumulators = collected.concepts;
    let concept_count = concept_accumulators.len();
    let mut document_node = GraphNodeSummary {
        id: "document".into(),
        label: title.clone(),
        kind: GraphNodeKind::Document,
        confidence: Some(if concept_count > 0 { 0.78 } else { 0.42 }),
        related_count: 0,
        evidence_count: page_sections.len(),
        position: GraphNodePosition { x: 50.0, y: 14.0 },
    };

    let concept_positions = layout_concept_positions(concept_count.max(1));
    let mut concept_nodes = concept_accumulators
        .iter()
        .enumerate()
        .map(|(index, concept)| GraphNodeSummary {
            id: concept.id.clone(),
            label: concept.label.clone(),
            kind: GraphNodeKind::Concept,
            confidence: Some((0.62 + (concept.evidence.len().min(3) as f32 * 0.08)).min(0.91)),
            related_count: 0,
            evidence_count: concept.evidence.len(),
            position: concept_positions
                .get(index)
                .cloned()
                .unwrap_or(GraphNodePosition { x: 50.0, y: 54.0 }),
        })
        .collect::<Vec<_>>();

    let concept_by_id = concept_accumulators
        .iter()
        .map(|concept| (concept.id.clone(), concept.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut details_by_node_id = BTreeMap::new();
    let mut edge_details_by_id = BTreeMap::new();
    let mut answer_by_node_id = BTreeMap::new();
    let document_evidence = page_sections
        .iter()
        .take(3)
        .enumerate()
        .map(|(index, section)| EvidenceRef {
            id: format!("ev-document-{}", index + 1),
            page_label: section.page_label.clone(),
            snippet: excerpt(&section.content, 180),
            source_path: Some(source_path.clone()),
        })
        .collect::<Vec<_>>();

    let (edges, built_edge_details_by_id, related_count_by_node_id, connected_node_ids_by_node_id) =
        build_relation_edges(
            &document_node,
            &concept_accumulators,
            &collected.page_concepts,
            &source_path,
        );
    edge_details_by_id.extend(built_edge_details_by_id);
    document_node.related_count = related_count_by_node_id
        .get(document_node.id.as_str())
        .copied()
        .unwrap_or(0);
    for node in &mut concept_nodes {
        node.related_count = related_count_by_node_id
            .get(node.id.as_str())
            .copied()
            .unwrap_or(0);
    }

    details_by_node_id.insert(
        document_node.id.clone(),
        GraphNodeDetail {
            node: document_node.clone(),
            canonical_name: title.clone(),
            aliases: vec!["Imported document".into()],
            description: format!(
                "DuckDocs compiled {} concept nodes from {} visible page sections. Every node below keeps direct evidence back to the imported document.",
                concept_count,
                page_sections.len()
            ),
            evidence: document_evidence.clone(),
            actions: disabled_correction_actions(
                "Merge and rename controls will activate once correction writes land.",
            ),
        },
    );
    answer_by_node_id.insert(
        document_node.id.clone(),
        AnswerResponse {
            status: if concept_count > 0 {
                AnswerStatus::Grounded
            } else {
                AnswerStatus::LowConfidence
            },
            text: Some(format!(
                "DuckDocs found {} concept nodes across {} page sections in this import.",
                concept_count,
                page_sections.len()
            )),
            explanation:
                "This document-level answer is grounded in the concept nodes and visible evidence DuckDocs compiled from the markdown package."
                    .into(),
            citations: document_evidence.clone(),
            related_node_ids: connected_node_ids_by_node_id
                .get(document_node.id.as_str())
                .map(|related| related.iter().cloned().collect())
                .unwrap_or_default(),
            suggested_actions: vec![
                SuggestedAction {
                    kind: SuggestedActionKind::InspectEvidence,
                    label: "Inspect evidence".into(),
                    description:
                        "Review the cited snippets before trusting the document-wide summary."
                            .into(),
                },
                SuggestedAction {
                    kind: SuggestedActionKind::AskDifferentQuestion,
                    label: "Ask a narrower question".into(),
                    description:
                        "Grounded answers get stronger when you focus on one concept at a time."
                            .into(),
                },
            ],
        },
    );

    for node in &concept_nodes {
        let concept = concept_by_id
            .get(&node.id)
            .expect("concept node should have backing accumulator");
        let aliases = concept
            .aliases
            .iter()
            .filter(|alias| alias.as_str() != concept.label)
            .cloned()
            .collect::<Vec<_>>();
        details_by_node_id.insert(
            node.id.clone(),
            GraphNodeDetail {
                node: node.clone(),
                canonical_name: concept.label.clone(),
                aliases,
                description: format!(
                    "Compiled from {} evidence refs across {} page(s). DuckDocs is still conservative and only shows evidence-backed concept nodes.",
                    concept.evidence.len(),
                    concept.page_labels.len()
                ),
                evidence: concept.evidence.clone(),
                actions: disabled_correction_actions(
                    "Correction apply flow lands next. The graph already exposes where a future merge or rename would attach.",
                ),
            },
        );
        answer_by_node_id.insert(
            node.id.clone(),
            AnswerResponse {
                status: AnswerStatus::Grounded,
                text: Some(format!(
                    "{} appears in {} evidence refs across {} page(s).",
                    concept.label,
                    concept.evidence.len(),
                    concept.page_labels.len()
                )),
                explanation:
                    "This answer is grounded in the evidence attached to the selected concept node."
                        .into(),
                citations: concept.evidence.iter().take(3).cloned().collect(),
                related_node_ids: connected_node_ids_by_node_id
                    .get(node.id.as_str())
                    .map(|related| related.iter().cloned().collect())
                    .unwrap_or_else(|| vec!["document".into()]),
                suggested_actions: vec![SuggestedAction {
                    kind: SuggestedActionKind::InspectEvidence,
                    label: "Inspect evidence".into(),
                    description:
                        "Use the cited snippets to verify the concept before acting on it.".into(),
                }],
            },
        );
    }

    let mut nodes = Vec::with_capacity(concept_nodes.len() + 1);
    nodes.push(document_node.clone());
    nodes.extend(concept_nodes.iter().cloned());

    let evidence_count = concept_accumulators
        .iter()
        .map(|concept| concept.evidence.len())
        .sum::<usize>()
        + document_evidence.len()
        + edges.iter().map(|edge| edge.evidence_count).sum::<usize>();

    KnowledgeProject {
        summary: ProjectOverview {
            project_id,
            title,
            status: if concept_count > 0 {
                ProjectStatus::Ready
            } else {
                ProjectStatus::Degraded
            },
            stale: false,
            summary: format!(
                "Compiled {} concept nodes from {} page sections. DuckDocs only shows nodes with visible evidence.",
                concept_count,
                page_sections.len()
            ),
            document_count: 1,
            node_count: nodes.len(),
            relationship_count: edges.len(),
            evidence_count,
        },
        nodes,
        edges,
        details_by_node_id,
        edge_details_by_id,
        answer_by_node_id,
    }
}

fn collect_concepts(page_sections: &[PageSection], source_path: &str) -> CollectedConcepts {
    let mut concepts = BTreeMap::<String, ConceptAccumulator>::new();
    let mut page_concepts = Vec::new();

    for section in page_sections {
        let mut seen_on_page = BTreeSet::new();
        let mut page_concept_ids = Vec::new();
        let candidates = concept_candidates(&section.content);
        for (candidate_index, candidate) in candidates.into_iter().enumerate() {
            let key = normalize_key(&candidate);
            if key.is_empty() || !seen_on_page.insert(key.clone()) {
                continue;
            }
            let concept_id = format!("concept-{key}");
            let concept = concepts
                .entry(key.clone())
                .or_insert_with(|| ConceptAccumulator {
                    id: concept_id.clone(),
                    label: candidate.clone(),
                    aliases: BTreeSet::new(),
                    evidence: Vec::new(),
                    page_labels: BTreeSet::new(),
                });
            if concept.label != candidate {
                concept.aliases.insert(candidate.clone());
            }
            concept.page_labels.insert(section.page_label.clone());
            concept.evidence.push(EvidenceRef {
                id: format!(
                    "ev-{}-{}",
                    key,
                    candidate_index + concept.evidence.len() + 1
                ),
                page_label: section.page_label.clone(),
                snippet: excerpt(&section.content, 180),
                source_path: Some(source_path.to_string()),
            });
            page_concept_ids.push(concept_id);
        }
        if !page_concept_ids.is_empty() {
            page_concepts.push(PageConceptSet {
                page_label: section.page_label.clone(),
                concept_ids: page_concept_ids,
                snippet: excerpt(&section.content, 180),
            });
        }
    }

    if concepts.is_empty() {
        for (index, section) in page_sections.iter().enumerate() {
            let label = fallback_concept_label(&section.content, &section.page_label);
            let key = normalize_key(&label);
            let concept_id = format!("concept-{key}");
            concepts.insert(
                key.clone(),
                ConceptAccumulator {
                    id: concept_id.clone(),
                    label,
                    aliases: BTreeSet::new(),
                    evidence: vec![EvidenceRef {
                        id: format!("ev-fallback-{}", index + 1),
                        page_label: section.page_label.clone(),
                        snippet: excerpt(&section.content, 180),
                        source_path: Some(source_path.to_string()),
                    }],
                    page_labels: [section.page_label.clone()].into_iter().collect(),
                },
            );
            page_concepts.push(PageConceptSet {
                page_label: section.page_label.clone(),
                concept_ids: vec![concept_id],
                snippet: excerpt(&section.content, 180),
            });
        }
    }

    let concepts = concepts.into_values().take(20).collect::<Vec<_>>();
    let allowed_ids = concepts
        .iter()
        .map(|concept| concept.id.clone())
        .collect::<BTreeSet<_>>();
    let page_concepts = page_concepts
        .into_iter()
        .filter_map(|mut page| {
            page.concept_ids.retain(|id| allowed_ids.contains(id));
            page.concept_ids.sort();
            page.concept_ids.dedup();
            if page.concept_ids.is_empty() {
                None
            } else {
                Some(page)
            }
        })
        .collect();

    CollectedConcepts {
        concepts,
        page_concepts,
    }
}

fn build_relation_edges(
    document_node: &GraphNodeSummary,
    concept_accumulators: &[ConceptAccumulator],
    page_concepts: &[PageConceptSet],
    source_path: &str,
) -> (
    Vec<RelationEdgeSummary>,
    BTreeMap<String, RelationEdgeDetail>,
    BTreeMap<String, usize>,
    BTreeMap<String, BTreeSet<String>>,
) {
    let mut edges = Vec::new();
    let mut edge_details_by_id = BTreeMap::new();
    let mut related_count_by_node_id = BTreeMap::<String, usize>::new();
    let mut connected_node_ids_by_node_id = BTreeMap::<String, BTreeSet<String>>::new();
    let concept_by_id = concept_accumulators
        .iter()
        .map(|concept| (concept.id.clone(), concept))
        .collect::<BTreeMap<_, _>>();

    for concept in concept_accumulators {
        let edge = RelationEdgeSummary {
            id: format!("edge-document-{}", concept.id),
            source_node_id: document_node.id.clone(),
            target_node_id: concept.id.clone(),
            kind: RelationKind::SourceDocument,
            label: "Compiled from source".into(),
            confidence: Some(0.94),
            evidence_count: concept.evidence.iter().take(2).count(),
        };
        let evidence = concept.evidence.iter().take(2).cloned().collect::<Vec<_>>();
        edge_details_by_id.insert(
            edge.id.clone(),
            RelationEdgeDetail {
                edge: edge.clone(),
                explanation: format!(
                    "DuckDocs linked the source document to {} because this concept was compiled from cited snippets in the import.",
                    concept.label
                ),
                evidence,
            },
        );
        note_relation(
            &mut related_count_by_node_id,
            &mut connected_node_ids_by_node_id,
            &edge.source_node_id,
            &edge.target_node_id,
        );
        edges.push(edge);
    }

    let mut concept_edge_accumulators = BTreeMap::<(String, String), EdgeAccumulator>::new();
    for page in page_concepts {
        if page.concept_ids.len() < 2 {
            continue;
        }
        for left_index in 0..page.concept_ids.len() {
            for right_index in (left_index + 1)..page.concept_ids.len() {
                let left_id = &page.concept_ids[left_index];
                let right_id = &page.concept_ids[right_index];
                let (source_node_id, target_node_id) = if left_id <= right_id {
                    (left_id.clone(), right_id.clone())
                } else {
                    (right_id.clone(), left_id.clone())
                };
                let accumulator = concept_edge_accumulators
                    .entry((source_node_id.clone(), target_node_id.clone()))
                    .or_insert_with(|| EdgeAccumulator {
                        source_node_id: source_node_id.clone(),
                        target_node_id: target_node_id.clone(),
                        evidence: Vec::new(),
                        page_labels: BTreeSet::new(),
                    });
                accumulator.page_labels.insert(page.page_label.clone());
                accumulator.evidence.push(EvidenceRef {
                    id: format!(
                        "ev-edge-{}-{}-{}",
                        source_node_id,
                        target_node_id,
                        accumulator.evidence.len() + 1
                    ),
                    page_label: page.page_label.clone(),
                    snippet: page.snippet.clone(),
                    source_path: Some(source_path.to_string()),
                });
            }
        }
    }

    let mut concept_edges = concept_edge_accumulators.into_values().collect::<Vec<_>>();
    concept_edges.sort_by(|left, right| {
        right
            .page_labels
            .len()
            .cmp(&left.page_labels.len())
            .then_with(|| left.source_node_id.cmp(&right.source_node_id))
            .then_with(|| left.target_node_id.cmp(&right.target_node_id))
    });

    for accumulator in concept_edges.into_iter().take(16) {
        let source_label = concept_by_id
            .get(&accumulator.source_node_id)
            .map(|concept| concept.label.clone())
            .unwrap_or_else(|| accumulator.source_node_id.clone());
        let target_label = concept_by_id
            .get(&accumulator.target_node_id)
            .map(|concept| concept.label.clone())
            .unwrap_or_else(|| accumulator.target_node_id.clone());
        let edge = RelationEdgeSummary {
            id: format!(
                "edge-{}-{}",
                accumulator.source_node_id, accumulator.target_node_id
            ),
            source_node_id: accumulator.source_node_id.clone(),
            target_node_id: accumulator.target_node_id.clone(),
            kind: RelationKind::RelatedTo,
            label: "Related in source".into(),
            confidence: Some(
                (0.56 + (accumulator.page_labels.len().min(3) as f32 * 0.08)).min(0.84),
            ),
            evidence_count: accumulator.evidence.len(),
        };
        edge_details_by_id.insert(
            edge.id.clone(),
            RelationEdgeDetail {
                edge: edge.clone(),
                explanation: format!(
                    "DuckDocs linked {} and {} because they appeared together in {} page section(s).",
                    source_label,
                    target_label,
                    accumulator.page_labels.len()
                ),
                evidence: accumulator.evidence.clone(),
            },
        );
        note_relation(
            &mut related_count_by_node_id,
            &mut connected_node_ids_by_node_id,
            &edge.source_node_id,
            &edge.target_node_id,
        );
        edges.push(edge);
    }

    (
        edges,
        edge_details_by_id,
        related_count_by_node_id,
        connected_node_ids_by_node_id,
    )
}

fn note_relation(
    related_count_by_node_id: &mut BTreeMap<String, usize>,
    connected_node_ids_by_node_id: &mut BTreeMap<String, BTreeSet<String>>,
    source_node_id: &str,
    target_node_id: &str,
) {
    *related_count_by_node_id
        .entry(source_node_id.to_string())
        .or_default() += 1;
    *related_count_by_node_id
        .entry(target_node_id.to_string())
        .or_default() += 1;
    connected_node_ids_by_node_id
        .entry(source_node_id.to_string())
        .or_default()
        .insert(target_node_id.to_string());
    connected_node_ids_by_node_id
        .entry(target_node_id.to_string())
        .or_default()
        .insert(source_node_id.to_string());
}

fn concept_candidates(content: &str) -> Vec<String> {
    let mut labels: Vec<String> = Vec::new();
    for line in content.lines() {
        let cleaned = clean_candidate_line(line);
        if cleaned.is_empty() {
            continue;
        }
        if let Some(label) = derive_concept_label(&cleaned) {
            if !labels
                .iter()
                .any(|existing| normalize_key(existing) == normalize_key(&label))
            {
                labels.push(label);
            }
        }
        if labels.len() >= 3 {
            break;
        }
    }
    labels
}

fn clean_candidate_line(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed.starts_with("![")
        || trimmed.starts_with("_AI analysis unavailable")
        || trimmed.starts_with("# ")
        || trimmed.starts_with("## Page ")
    {
        return String::new();
    }

    trimmed
        .trim_start_matches('#')
        .trim_start_matches('-')
        .trim_start_matches('*')
        .trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == ')')
        .trim()
        .replace('`', "")
        .replace('*', "")
}

fn derive_concept_label(value: &str) -> Option<String> {
    let first_clause = value
        .split(|char| matches!(char, '.' | ':' | ';' | '(' | ')' | '[' | ']'))
        .next()
        .unwrap_or(value)
        .trim();
    let mut words = first_clause
        .split_whitespace()
        .map(|word| {
            word.trim_matches(|char: char| !char.is_alphanumeric() && char != '-' && char != '/')
        })
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>();

    while matches!(words.first(), Some(word) if is_leading_stopword(word)) {
        words.remove(0);
    }

    if words.len() < 2 {
        return None;
    }

    let label = words.into_iter().take(6).collect::<Vec<_>>().join(" ");
    if label.len() < 10 {
        return None;
    }

    Some(label)
}

fn is_leading_stopword(value: &str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "a" | "an"
            | "and"
            | "as"
            | "for"
            | "from"
            | "in"
            | "into"
            | "of"
            | "on"
            | "or"
            | "the"
            | "this"
            | "that"
            | "to"
            | "with"
    )
}

fn fallback_concept_label(content: &str, page_label: &str) -> String {
    derive_concept_label(content).unwrap_or_else(|| format!("{page_label} summary"))
}

fn normalize_key(value: &str) -> String {
    let mut normalized = String::new();
    let mut last_dash = false;
    for char in value.chars() {
        if char.is_ascii_alphanumeric() {
            normalized.push(char.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            normalized.push('-');
            last_dash = true;
        }
    }
    normalized.trim_matches('-').to_string()
}

fn extract_page_sections(markdown: &str) -> Vec<PageSection> {
    let normalized = markdown.replace("\r\n", "\n");
    let headers = regex_like_page_headers(&normalized);
    if headers.is_empty() {
        return vec![PageSection {
            page_label: "Imported text".into(),
            content: normalized,
        }];
    }

    let mut sections = Vec::with_capacity(headers.len());
    for index in 0..headers.len() {
        let (page_label, _, content_start) = &headers[index];
        let next_start = headers
            .get(index + 1)
            .map(|(_, next_start, _)| *next_start)
            .unwrap_or(normalized.len());
        sections.push(PageSection {
            page_label: page_label.clone(),
            content: normalized[*content_start..next_start].trim().to_string(),
        });
    }
    sections
}

fn regex_like_page_headers(markdown: &str) -> Vec<(String, usize, usize)> {
    let mut headers = Vec::new();
    let mut offset = 0usize;
    for line in markdown.lines() {
        let line_len = line.len();
        if let Some(page_label) = line
            .strip_prefix("## Page ")
            .map(|page| format!("Page {}", page.trim()))
        {
            headers.push((page_label, offset, offset + line_len + 1));
        }
        offset += line_len + 1;
    }
    headers
}

fn infer_markdown_title(markdown_path: &str, markdown: &str) -> String {
    if let Some(heading) = markdown
        .lines()
        .find_map(|line| line.strip_prefix("# ").map(str::trim))
        .filter(|value| !value.is_empty())
    {
        return heading.to_string();
    }

    Path::new(markdown_path)
        .file_stem()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| "DuckDocs import".into())
}

fn excerpt(value: &str, max_length: usize) -> String {
    let compact = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.is_empty() {
        return "No visible evidence snippet is available yet.".into();
    }
    let compact_chars = compact.chars().count();
    if compact_chars <= max_length {
        return compact;
    }
    let truncated = compact
        .chars()
        .take(max_length.saturating_sub(1))
        .collect::<String>();
    format!("{}…", truncated.trim_end())
}

fn disabled_correction_actions(reason: &str) -> Vec<CorrectionAction> {
    vec![
        CorrectionAction {
            kind: CorrectionKind::Merge,
            label: "Merge".into(),
            disabled_reason: Some(reason.into()),
        },
        CorrectionAction {
            kind: CorrectionKind::KeepSeparate,
            label: "Keep Separate".into(),
            disabled_reason: Some(reason.into()),
        },
        CorrectionAction {
            kind: CorrectionKind::Rename,
            label: "Rename".into(),
            disabled_reason: Some(reason.into()),
        },
    ]
}

fn layout_concept_positions(count: usize) -> Vec<GraphNodePosition> {
    let per_row = if count > 9 { 4 } else { 3 };
    let row_count = ((count as f32) / (per_row as f32)).ceil() as usize;
    let row_spacing = if row_count > 1 {
        48.0 / (row_count.saturating_sub(1) as f32)
    } else {
        0.0
    };
    let mut positions = Vec::with_capacity(count);

    for index in 0..count {
        let row = index / per_row;
        let col = index % per_row;
        let columns_in_row = if row == row_count.saturating_sub(1) {
            let remainder = count % per_row;
            if remainder == 0 {
                per_row
            } else {
                remainder
            }
        } else {
            per_row
        };
        let x = if columns_in_row == 1 {
            50.0
        } else {
            18.0 + (64.0 / (columns_in_row.saturating_sub(1) as f32)) * (col as f32)
        };
        let y = 40.0 + row_spacing * (row as f32);
        positions.push(GraphNodePosition { x, y });
    }

    positions
}

fn build_project_id(request: &CompileProjectRequest) -> String {
    let stable_source = request
        .source_document_path
        .as_deref()
        .unwrap_or(&request.source_markdown_path);
    format!("project-{:016x}", fnv1a_hash(stable_source.as_bytes()))
}

fn fnv1a_hash(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn engine_failure(command: EngineCommand, error: &anyhow::Error) -> EngineFailure {
    let code = if format!("{error:?}").contains("decode") {
        "invalid_request"
    } else if format!("{error:?}").contains("config") {
        "config_error"
    } else {
        "runtime_error"
    };
    EngineFailure::new(command, code, error.to_string())
}

fn parse_document(
    input: &ParseInput,
    template: &str,
    _options: &ParseOptions,
    config: &EngineConfig,
) -> Result<ParsedDocument> {
    match input.format {
        DocumentFormat::Pdf | DocumentFormat::Image => {
            parse_visual_document(input, template, config)
        }
        DocumentFormat::Docx | DocumentFormat::Doc => parse_text_document(input, template, config),
    }
}

fn parse_visual_document(
    input: &ParseInput,
    template: &str,
    config: &EngineConfig,
) -> Result<ParsedDocument> {
    let page_images = match input.format {
        DocumentFormat::Image => vec![PathBuf::from(&input.path)],
        DocumentFormat::Pdf => convert_pdf_to_pngs(Path::new(&input.path))?,
        _ => unreachable!(),
    };

    let total = page_images.len() as u32;
    let mut pages = Vec::new();
    let mut assets = Vec::new();
    let mut success_count = 0usize;
    let mut failed_count = 0usize;

    for (idx, image_path) in page_images.iter().enumerate() {
        emit_event(&ParseEvent::ConvertingPages {
            current: (idx + 1) as u32,
            total,
        })?;

        let image_bytes = fs::read(image_path)
            .with_context(|| format!("failed to read rendered image {}", image_path.display()))?;
        let relative_path = format!("images/page_{}.png", idx + 1);
        assets.push(OutputAsset {
            relative_path: relative_path.clone(),
            mime_type: "image/png".into(),
            base64: base64::engine::general_purpose::STANDARD.encode(&image_bytes),
        });

        emit_event(&ParseEvent::Parsing {
            current: (idx + 1) as u32,
            total,
        })?;

        match parse_image_with_provider(config, &image_bytes, template) {
            Ok(markdown) => {
                success_count += 1;
                pages.push(ParsedPage {
                    index: idx,
                    markdown: Some(markdown.clone()),
                    plain_text: Some(markdown),
                    svg: None,
                    image_asset_path: Some(relative_path),
                    error_message: None,
                });
            }
            Err(error) => {
                failed_count += 1;
                pages.push(ParsedPage {
                    index: idx,
                    markdown: None,
                    plain_text: None,
                    svg: None,
                    image_asset_path: Some(relative_path),
                    error_message: Some(error.to_string()),
                });
            }
        }
    }

    Ok(ParsedDocument {
        page_count: pages.len(),
        pages,
        assets,
        success_count,
        failed_count,
    })
}

fn parse_text_document(
    input: &ParseInput,
    template: &str,
    config: &EngineConfig,
) -> Result<ParsedDocument> {
    let text = extract_text_via_textutil(Path::new(&input.path))?;
    emit_event(&ParseEvent::ConvertingPages {
        current: 1,
        total: 1,
    })?;
    emit_event(&ParseEvent::Parsing {
        current: 1,
        total: 1,
    })?;

    let page = match parse_text_with_provider(config, &text, template) {
        Ok(markdown) => ParsedPage {
            index: 0,
            markdown: Some(markdown.clone()),
            plain_text: Some(markdown),
            svg: None,
            image_asset_path: None,
            error_message: None,
        },
        Err(error) => ParsedPage {
            index: 0,
            markdown: None,
            plain_text: Some(text.clone()),
            svg: None,
            image_asset_path: None,
            error_message: Some(error.to_string()),
        },
    };

    let failed_count = usize::from(page.markdown.is_none());
    Ok(ParsedDocument {
        pages: vec![page],
        assets: Vec::new(),
        page_count: 1,
        success_count: 1usize.saturating_sub(failed_count),
        failed_count,
    })
}

fn convert_pdf_to_pngs(path: &Path) -> Result<Vec<PathBuf>> {
    let temp = tempdir().context("failed to create temp directory for pdf conversion")?;
    let prefix = temp.path().join("page");
    let status = Command::new("/opt/homebrew/bin/pdftoppm")
        .arg("-png")
        .arg(path)
        .arg(&prefix)
        .status()
        .context("failed to launch pdftoppm")?;
    if !status.success() {
        bail!("pdftoppm failed for {}", path.display());
    }

    let mut outputs = fs::read_dir(temp.path())
        .with_context(|| {
            format!(
                "failed listing converted PDF pages in {}",
                temp.path().display()
            )
        })?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("png"))
        .collect::<Vec<_>>();
    outputs.sort();

    if outputs.is_empty() {
        bail!("pdf conversion produced no pages for {}", path.display());
    }

    let persisted_root = temp.keep();
    Ok(outputs
        .into_iter()
        .map(|path| persisted_root.join(path.file_name().unwrap()))
        .collect())
}

fn extract_text_via_textutil(path: &Path) -> Result<String> {
    let output = Command::new("/usr/bin/textutil")
        .arg("-convert")
        .arg("txt")
        .arg("-stdout")
        .arg(path)
        .output()
        .context("failed to launch textutil")?;

    if !output.status.success() {
        bail!("textutil failed for {}", path.display());
    }

    let text = String::from_utf8(output.stdout).context("textutil output was not valid UTF-8")?;
    if text.trim().is_empty() {
        bail!(
            "text extraction produced empty output for {}",
            path.display()
        );
    }
    Ok(text)
}

fn build_markdown(title: String, pages: &[ParsedPage]) -> String {
    let mut markdown = format!("# {title}\n\n");
    for (idx, page) in pages.iter().enumerate() {
        markdown.push_str(&format!("## Page {}\n\n", idx + 1));
        if let Some(image_path) = &page.image_asset_path {
            markdown.push_str(&format!("![Page {}]({image_path})\n\n", idx + 1));
        }
        if let Some(body) = page
            .markdown
            .as_ref()
            .or(page.plain_text.as_ref())
            .filter(|value| !value.trim().is_empty())
        {
            markdown.push_str(body);
            markdown.push_str("\n\n");
        } else if let Some(error_message) = &page.error_message {
            markdown.push_str(&format!("_AI analysis unavailable: {error_message}_\n\n"));
        } else {
            markdown.push_str("_AI analysis unavailable._\n\n");
        }
    }
    markdown
}

fn export_output_package(request: &ParseRequest, result: &ParseResult) -> Result<Option<String>> {
    let Some(output) = &request.output else {
        return Ok(None);
    };

    let output_root = match &output.root_dir {
        Some(root) => PathBuf::from(root),
        None => dirs::document_dir()
            .ok_or_else(|| anyhow!("failed to resolve documents directory"))?
            .join("DuckDocs"),
    };
    fs::create_dir_all(&output_root)
        .with_context(|| format!("failed creating output root {}", output_root.display()))?;

    let base_name = output
        .name
        .clone()
        .or_else(|| {
            Path::new(&request.input.path)
                .file_stem()
                .map(|value| value.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "document".to_string());
    let safe_name = sanitize_name(&base_name);
    let timestamp = chrono_like_timestamp();
    let output_dir = output_root.join(format!("{safe_name}_{timestamp}"));
    let images_dir = output_dir.join("images");
    fs::create_dir_all(&images_dir).with_context(|| {
        format!(
            "failed creating image output directory {}",
            images_dir.display()
        )
    })?;

    for asset in &result.assets {
        let target = output_dir.join(&asset.relative_path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed creating asset directory {}", parent.display()))?;
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&asset.base64)
            .with_context(|| format!("failed decoding asset {}", asset.relative_path))?;
        fs::write(&target, bytes)
            .with_context(|| format!("failed writing asset {}", target.display()))?;
    }

    let markdown_path = output_dir.join(format!("{safe_name}.md"));
    fs::write(&markdown_path, &result.markdown)
        .with_context(|| format!("failed writing markdown {}", markdown_path.display()))?;
    Ok(Some(markdown_path.display().to_string()))
}

fn sanitize_name(value: &str) -> String {
    let sanitized = value
        .replace('/', "-")
        .replace('\\', "-")
        .replace(':', "-")
        .replace("..", "-")
        .trim()
        .chars()
        .take(100)
        .collect::<String>();
    if sanitized.is_empty() {
        "output".into()
    } else {
        sanitized
    }
}

fn chrono_like_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    now.to_string()
}

struct KnowledgeProjectStore {
    path: PathBuf,
}

impl KnowledgeProjectStore {
    fn default() -> Result<Self> {
        if let Some(explicit_path) = std::env::var_os("DUCKDOCS_PROJECT_STORE") {
            return Ok(Self {
                path: PathBuf::from(explicit_path),
            });
        }

        let root = dirs::data_local_dir()
            .or_else(dirs::home_dir)
            .ok_or_else(|| anyhow!("failed to resolve local data directory"))?;
        Ok(Self {
            path: root.join("DuckDocs/knowledge.sqlite3"),
        })
    }

    #[cfg(test)]
    fn new(path: PathBuf) -> Self {
        Self { path }
    }

    fn save_project(
        &self,
        project: &KnowledgeProject,
        request: &CompileProjectRequest,
    ) -> Result<()> {
        self.ensure_schema()?;
        let snapshot_json =
            serde_json::to_string(project).context("failed to encode knowledge project")?;
        let snapshot_base64 = base64::engine::general_purpose::STANDARD.encode(snapshot_json);
        let source_document_path = request
            .source_document_path
            .as_ref()
            .map(|path| format!("'{}'", escape_sqlite(path)))
            .unwrap_or_else(|| "NULL".into());
        let status = match project.summary.status {
            ProjectStatus::Preview => "preview",
            ProjectStatus::Ready => "ready",
            ProjectStatus::Degraded => "degraded",
        };
        let sql = format!(
            "INSERT INTO projects (project_id, title, source_markdown_path, source_document_path, status, updated_at, snapshot_base64) \
             VALUES ('{project_id}', '{title}', '{markdown_path}', {source_document_path}, '{status}', {updated_at}, '{snapshot_base64}') \
             ON CONFLICT(project_id) DO UPDATE SET \
               title=excluded.title, \
               source_markdown_path=excluded.source_markdown_path, \
               source_document_path=excluded.source_document_path, \
               status=excluded.status, \
               updated_at=excluded.updated_at, \
               snapshot_base64=excluded.snapshot_base64;",
            project_id = escape_sqlite(&project.summary.project_id),
            title = escape_sqlite(&project.summary.title),
            markdown_path = escape_sqlite(&request.source_markdown_path),
            source_document_path = source_document_path,
            status = status,
            updated_at = unix_timestamp_seconds(),
            snapshot_base64 = snapshot_base64,
        );
        self.run_sql(&sql).map(|_| ())
    }

    fn load_project(&self, project_id: Option<&str>) -> Result<Option<KnowledgeProject>> {
        self.ensure_schema()?;
        let sql = match project_id {
            Some(project_id) => format!(
                "SELECT snapshot_base64 FROM projects WHERE project_id = '{}' LIMIT 1;",
                escape_sqlite(project_id)
            ),
            None => "SELECT snapshot_base64 FROM projects ORDER BY updated_at DESC LIMIT 1;".into(),
        };
        let output = self.run_sql(&sql)?;
        let encoded = output.trim();
        if encoded.is_empty() {
            return Ok(None);
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .context("failed to decode stored project snapshot")?;
        let project = serde_json::from_slice(&bytes).context("failed to decode stored project")?;
        Ok(Some(project))
    }

    fn ensure_schema(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed creating {}", parent.display()))?;
        }
        self.run_sql(
            "CREATE TABLE IF NOT EXISTS projects (
                project_id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                source_markdown_path TEXT NOT NULL,
                source_document_path TEXT,
                status TEXT NOT NULL,
                updated_at INTEGER NOT NULL,
                snapshot_base64 TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_projects_updated_at ON projects(updated_at DESC);",
        )
        .map(|_| ())
    }

    fn run_sql(&self, sql: &str) -> Result<String> {
        let output = Command::new("/usr/bin/sqlite3")
            .arg(&self.path)
            .arg(sql)
            .output()
            .with_context(|| format!("failed to launch sqlite3 for {}", self.path.display()))?;

        if !output.status.success() {
            bail!(
                "sqlite3 failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }

        Ok(String::from_utf8(output.stdout).context("sqlite3 output was not valid UTF-8")?)
    }
}

fn escape_sqlite(value: &str) -> String {
    value.replace('\'', "''")
}

fn unix_timestamp_seconds() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EngineConfig {
    #[serde(deserialize_with = "ProviderKind::deserialize_unknown")]
    provider: ProviderKind,
    model_id: String,
    api_key: String,
    base_url: Option<String>,
    #[serde(default = "default_prompt_template")]
    prompt_template: String,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            provider: ProviderKind::OpenRouter,
            model_id: "openai/gpt-4.1-mini".into(),
            api_key: std::env::var("OPENROUTER_API_KEY").unwrap_or_default(),
            base_url: None,
            prompt_template: default_prompt_template(),
        }
    }
}

fn default_prompt_template() -> String {
    "General".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ProviderKind {
    OpenRouter,
    Ollama,
}

impl ProviderKind {
    /// Deserializes a provider slug, falling back to `OpenRouter` for unknown values.
    /// This handles legacy config files that may contain removed providers like `open_ai` or `anthropic`.
    fn deserialize_unknown<'de, D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let slug = String::deserialize(deserializer)?;
        Ok(Self::from_slug(&slug).unwrap_or(Self::OpenRouter))
    }
}

impl ProviderKind {
    fn id_slug(&self) -> &'static str {
        match self {
            Self::OpenRouter => "open_router",
            Self::Ollama => "ollama",
        }
    }

    fn default_base_url(&self) -> &'static str {
        match self {
            Self::OpenRouter => "https://openrouter.ai/api/v1/chat/completions",
            Self::Ollama => "http://127.0.0.1:11434/v1/chat/completions",
        }
    }

    fn label(&self) -> &'static str {
        match self {
            Self::OpenRouter => "OpenRouter",
            Self::Ollama => "Ollama",
        }
    }

    fn requires_api_key(&self) -> bool {
        !matches!(self, Self::Ollama)
    }

    fn supports_base_url(&self) -> bool {
        true
    }
}

struct EngineConfigStore {
    path: PathBuf,
}

impl EngineConfigStore {
    fn default() -> Result<Self> {
        if let Some(explicit_dir) = std::env::var_os("DUCKDOCS_CONFIG_DIR") {
            return Ok(Self {
                path: PathBuf::from(explicit_dir).join("engine-config.json"),
            });
        }

        let home =
            dirs::home_dir().ok_or_else(|| anyhow!("failed to resolve user home directory"))?;
        Ok(Self {
            path: home.join(".duckdocs/engine-config.json"),
        })
    }

    fn load(&self) -> Result<EngineConfig> {
        if !self.path.exists() {
            let config = EngineConfig::default();
            self.save(&config)?;
            return Ok(config);
        }

        let contents = fs::read_to_string(&self.path)
            .with_context(|| format!("failed reading {}", self.path.display()))?;
        serde_json::from_str(&contents)
            .with_context(|| format!("failed decoding {}", self.path.display()))
    }

    fn save(&self, config: &EngineConfig) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed creating config directory {}", parent.display())
            })?;
        }
        let payload =
            serde_json::to_string_pretty(config).context("failed encoding engine config")?;
        fs::write(&self.path, payload)
            .with_context(|| format!("failed writing {}", self.path.display()))
    }
}

impl EngineConfig {
    fn to_payload(&self) -> EngineConfigPayload {
        let provider_options = ProviderKind::all()
            .into_iter()
            .map(|provider| ProviderOption {
                id: provider.id_slug().to_string(),
                label: provider.label().to_string(),
                requires_api_key: provider.requires_api_key(),
                supports_base_url: provider.supports_base_url(),
            })
            .collect();

        EngineConfigPayload {
            provider: self.provider.id_slug().to_string(),
            model_id: self.model_id.clone(),
            api_key: self.api_key.clone(),
            base_url: self.base_url.clone(),
            prompt_template: self.prompt_template.clone(),
            provider_options,
            model_options: model_options_for(&self.provider)
                .into_iter()
                .map(str::to_string)
                .collect(),
            prompt_template_options: prompt_template_options()
                .into_iter()
                .map(str::to_string)
                .collect(),
        }
    }

    fn from_payload(payload: EngineConfigPayload) -> Self {
        Self {
            provider: ProviderKind::from_slug(&payload.provider)
                .unwrap_or(ProviderKind::OpenRouter),
            model_id: payload.model_id,
            api_key: payload.api_key,
            base_url: payload.base_url,
            prompt_template: payload.prompt_template,
        }
    }
}

impl ProviderKind {
    fn all() -> [ProviderKind; 2] {
        [
            Self::OpenRouter,
            Self::Ollama,
        ]
    }

    fn from_slug(value: &str) -> Option<Self> {
        match value {
            "open_router" => Some(Self::OpenRouter),
            "ollama" => Some(Self::Ollama),
            _ => None,
        }
    }
}

fn prompt_template_options() -> [&'static str; 6] {
    [
        "General",
        "API Documentation",
        "UI Flow",
        "Tutorial",
        "Code Snippets",
        "Data Tables",
    ]
}

fn model_options_for(provider: &ProviderKind) -> Vec<&'static str> {
    duckdocs_engine_types::model_options_for(provider.id_slug())
}

fn validate_provider(config: &EngineConfig) -> ValidateProviderResponseData {
    let mut issues = Vec::new();
    if config.provider.requires_api_key() && config.api_key.trim().is_empty() {
        issues.push(ValidationIssue {
            code: "missing_api_key".into(),
            message: format!("{} requires an API key.", config.provider.label()),
        });
    }

    if config.model_id.trim().is_empty() {
        issues.push(ValidationIssue {
            code: "missing_model_id".into(),
            message: "A model ID is required.".into(),
        });
    }

    if let Some(base_url) = &config.base_url {
        if !base_url.trim().is_empty()
            && !(base_url.starts_with("http://") || base_url.starts_with("https://"))
        {
            issues.push(ValidationIssue {
                code: "invalid_base_url".into(),
                message: "Base URL must start with http:// or https://".into(),
            });
        }
    }

    ValidateProviderResponseData {
        ready: issues.is_empty(),
        issues,
    }
}

fn parse_image_with_provider(
    config: &EngineConfig,
    image_bytes: &[u8],
    template: &str,
) -> Result<String> {
    if provider_unavailable(config) {
        return Ok(format!(
            "_DuckDocs fallback parse._\n\nProvider `{}` is not configured or reachable, so this page was packaged as an image-only placeholder.\n\n- Template: {}\n- Image bytes: {}\n",
            config.provider.id_slug(),
            template,
            image_bytes.len()
        ));
    }

    let image_base64 = base64::engine::general_purpose::STANDARD.encode(image_bytes);
    let prompt = format!(
        "Convert this document page into clean markdown. Template: {template}. Preserve headings, lists, tables, and code blocks where possible."
    );
    match config.provider {
        ProviderKind::OpenRouter | ProviderKind::Ollama => {
            parse_openai_compatible(config, &prompt, Some(image_base64))
        }
    }
}

fn parse_text_with_provider(config: &EngineConfig, text: &str, template: &str) -> Result<String> {
    if provider_unavailable(config) {
        return Ok(format!(
            "_DuckDocs fallback parse._\n\nProvider `{}` is not configured or reachable, so this document was returned from extracted text.\n\n- Template: {}\n\n{}",
            config.provider.id_slug(),
            template,
            text
        ));
    }

    let prompt = format!(
        "Convert the following extracted document text into clean markdown. Template: {template}.\n\n{text}"
    );
    match config.provider {
        ProviderKind::OpenRouter | ProviderKind::Ollama => parse_openai_compatible(config, &prompt, None),
    }
}

fn provider_unavailable(config: &EngineConfig) -> bool {
    match config.provider {
        ProviderKind::OpenRouter => config.api_key.trim().is_empty(),
        ProviderKind::Ollama => false,
    }
}

fn parse_openai_compatible(
    config: &EngineConfig,
    prompt: &str,
    image_base64: Option<String>,
) -> Result<String> {
    let client = Client::new();
    let mut content = vec![serde_json::json!({ "type": "text", "text": prompt })];
    if let Some(image_base64) = image_base64 {
        content.push(serde_json::json!({
            "type": "image_url",
            "image_url": { "url": format!("data:image/png;base64,{image_base64}") }
        }));
    }

    let body = serde_json::json!({
        "model": config.model_id,
        "messages": [{ "role": "user", "content": content }],
    });
    let response = client
        .post(
            config
                .base_url
                .clone()
                .unwrap_or_else(|| config.provider.default_base_url().to_string()),
        )
        .bearer_auth(config.api_key.clone())
        .json(&body)
        .send()
        .context("failed to send provider request")?;
    let response = response
        .error_for_status()
        .context("provider returned error status")?;
    let json: serde_json::Value = response
        .json()
        .context("failed to decode provider response")?;
    json["choices"][0]["message"]["content"]
        .as_str()
        .map(|value| value.to_string())
        .ok_or_else(|| anyhow!("provider response did not include markdown text"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compile_and_store_project_round_trip() {
        let temp = tempfile::tempdir().expect("temp dir");
        let markdown_path = temp.path().join("sample.md");
        fs::write(
            &markdown_path,
            "# Sample import\n\n## Page 1\n\nDuckDocs compile path keeps evidence visible for every concept.\nExplainable graph view grounds answers in visible snippets.\n\n## Page 2\n\nEvidence inspector helps people trust the graph.\n",
        )
        .expect("write markdown");
        let request = CompileProjectRequest {
            source_markdown_path: markdown_path.display().to_string(),
            source_document_path: Some("/tmp/source.pdf".into()),
        };

        let markdown = fs::read_to_string(&markdown_path).expect("read markdown");
        let project = compile_knowledge_project(&request, &markdown);
        assert_eq!(project.summary.status, ProjectStatus::Ready);
        assert!(project
            .nodes
            .iter()
            .any(|node| node.kind == GraphNodeKind::Concept));
        assert!(!project.edges.is_empty());
        assert!(project
            .edges
            .iter()
            .any(|edge| edge.kind == RelationKind::RelatedTo));

        let store_path = temp.path().join("knowledge.sqlite3");
        let store = KnowledgeProjectStore::new(store_path);
        store
            .save_project(&project, &request)
            .expect("save project to sqlite");

        let loaded = store
            .load_project(Some(&project.summary.project_id))
            .expect("load project")
            .expect("stored project");
        assert_eq!(loaded.summary.project_id, project.summary.project_id);
        assert_eq!(loaded.summary.title, "Sample import");
        assert_eq!(loaded.nodes.len(), project.nodes.len());
        assert_eq!(loaded.edges.len(), project.edges.len());
        assert!(loaded.details_by_node_id.contains_key("document"));
        assert!(!loaded.edge_details_by_id.is_empty());
    }
}
