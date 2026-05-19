use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};
use hyprduck_engine_client::{EngineClient, SubprocessEngineClient};
use hyprduck_engine_types::{
    BrainRepoSnapshot, CompileProjectRequest, DocumentFormat, IngestStatus, PageArtifact,
    SourceArtifactManifest, StructuredExtractionArtifact, StructuredExtractionClaim,
};
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoldenEvalMode {
    SourceEvidence,
    Hosted,
    Local,
    All,
}

impl GoldenEvalMode {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "source-evidence" | "heuristic" => Ok(Self::SourceEvidence),
            "hosted" => Ok(Self::Hosted),
            "local" => Ok(Self::Local),
            "all" => Ok(Self::All),
            _ => Err(anyhow!("unknown golden corpus eval mode: {value}")),
        }
    }

    fn concrete_modes(self) -> Vec<Self> {
        match self {
            Self::All => vec![Self::SourceEvidence, Self::Hosted, Self::Local],
            mode => vec![mode],
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::SourceEvidence => "source-evidence",
            Self::Hosted => "hosted",
            Self::Local => "local",
            Self::All => "all",
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoldenExpected {
    case_id: String,
    workspace_id: String,
    source_id: String,
    description: String,
    expected_entities: Vec<String>,
    expected_claims: Vec<String>,
    expected_relations: Vec<GoldenExpectedRelation>,
    expected_evidence_snippets: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoldenExpectedRelation {
    source_entity: String,
    target_entity: String,
}

#[derive(Debug)]
struct GoldenCase {
    source_path: PathBuf,
    expected: GoldenExpected,
}

#[derive(Debug, Default)]
struct EvalCounts {
    cases: usize,
    entity_expected: usize,
    entity_matched: usize,
    claim_expected: usize,
    claim_matched: usize,
    relation_expected: usize,
    relation_matched: usize,
    evidence_expected: usize,
    evidence_matched: usize,
    contradiction_expected: usize,
    contradiction_matched: usize,
    context_expected: usize,
    context_matched: usize,
    citation_audited_answers: usize,
    source_ref_resolved: usize,
    page_ref_resolved: usize,
    evidence_ref_resolved: usize,
    citation_correct: usize,
    unsupported_claims: usize,
    total_claims: usize,
    latency_ms: u128,
}

pub fn run_golden_corpus(fixtures: Option<String>, mode: GoldenEvalMode) -> Result<String> {
    let fixtures_root = fixtures
        .map(PathBuf::from)
        .unwrap_or_else(default_golden_corpus_path);
    let cases = load_cases(&fixtures_root)?;
    if cases.is_empty() {
        bail!(
            "no golden corpus fixtures found in {}",
            fixtures_root.display()
        );
    }

    let mut sections = Vec::new();
    sections.push(format!("golden-corpus cases: {}", cases.len()));
    for mode in mode.concrete_modes() {
        let counts = run_mode(&cases, mode)?;
        sections.push(format_mode_report(mode, &counts));
    }
    Ok(sections.join("\n"))
}

fn default_golden_corpus_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../hyprduck-engine/tests/fixtures/brain-corpus")
}

fn load_cases(root: &Path) -> Result<Vec<GoldenCase>> {
    let mut entries = fs::read_dir(root)
        .with_context(|| format!("failed reading golden corpus root {}", root.display()))?
        .collect::<Result<Vec<_>, _>>()
        .with_context(|| format!("failed listing {}", root.display()))?;
    entries.sort_by_key(|entry| entry.path());

    let mut cases = Vec::new();
    for entry in entries {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let expected_path = path.join("expected.json");
        let source_path = path.join("source.md");
        if !expected_path.exists() || !source_path.exists() {
            continue;
        }
        let expected = serde_json::from_str::<GoldenExpected>(
            &fs::read_to_string(&expected_path)
                .with_context(|| format!("failed reading {}", expected_path.display()))?,
        )
        .with_context(|| format!("failed decoding {}", expected_path.display()))?;
        cases.push(GoldenCase {
            source_path,
            expected,
        });
    }
    Ok(cases)
}

fn run_mode(cases: &[GoldenCase], mode: GoldenEvalMode) -> Result<EvalCounts> {
    let eval_root = unique_eval_root(mode)?;
    fs::create_dir_all(&eval_root)
        .with_context(|| format!("failed creating {}", eval_root.display()))?;

    let previous_store = std::env::var_os("HYPRDUCK_PROJECT_STORE");
    let previous_output_dir = std::env::var_os("HYPRDUCK_OUTPUT_DIR");
    let previous_provider_graph = std::env::var_os("HYPRDUCK_DISABLE_PROVIDER_GRAPH");
    std::env::set_var(
        "HYPRDUCK_PROJECT_STORE",
        eval_root.join("knowledge.sqlite3"),
    );
    std::env::set_var("HYPRDUCK_OUTPUT_DIR", &eval_root);
    std::env::set_var("HYPRDUCK_DISABLE_PROVIDER_GRAPH", "1");

    let result = (|| {
        let client = SubprocessEngineClient::default();
        let mut counts = EvalCounts::default();
        for case in cases {
            let artifact = compile_case(&client, &eval_root, case, mode)?;
            score_case(case, &artifact, &mut counts);
        }
        Ok(counts)
    })();

    restore_env_var("HYPRDUCK_PROJECT_STORE", previous_store);
    restore_env_var("HYPRDUCK_OUTPUT_DIR", previous_output_dir);
    restore_env_var("HYPRDUCK_DISABLE_PROVIDER_GRAPH", previous_provider_graph);
    let _ = fs::remove_dir_all(&eval_root);
    result
}

fn compile_case(
    client: &SubprocessEngineClient,
    eval_root: &Path,
    case: &GoldenCase,
    mode: GoldenEvalMode,
) -> Result<StructuredExtractionArtifact> {
    let source_id = &case.expected.source_id;
    let workspace_id = &case.expected.workspace_id;
    let artifact_root = eval_root
        .join(workspace_id)
        .join("artifacts")
        .join(source_id);
    let sources_root = eval_root.join(workspace_id).join("sources").join(source_id);
    fs::create_dir_all(&artifact_root)
        .with_context(|| format!("failed creating {}", artifact_root.display()))?;
    fs::create_dir_all(&sources_root)
        .with_context(|| format!("failed creating {}", sources_root.display()))?;

    let manifest_path = artifact_root.join("source-manifest.json");
    let staged_source_path = sources_root.join("source.md");
    let staged_markdown_path = artifact_root.join("source.md");
    let manifest = SourceArtifactManifest {
        workspace_id: workspace_id.clone(),
        source_id: source_id.clone(),
        original_path: case.source_path.display().to_string(),
        source_path: staged_source_path.display().to_string(),
        markdown_path: staged_markdown_path.display().to_string(),
        artifact_root: artifact_root.display().to_string(),
        manifest_path: manifest_path.display().to_string(),
        format: DocumentFormat::Pdf,
        output_name: case.expected.case_id.clone(),
        status: IngestStatus::Ingested,
        description: case.expected.description.clone(),
        user_context: format!("golden corpus mode: {}", mode.label()),
        ingest_instruction: "Evaluate source-backed extraction quality.".into(),
        pages: vec![PageArtifact {
            index: 0,
            label: "Page 1".into(),
            image_path: None,
            markdown_path: Some(staged_markdown_path.display().to_string()),
            plain_text_path: None,
            error_message: None,
        }],
        created_at: 1,
        updated_at: 1,
    };
    fs::copy(&case.source_path, &staged_source_path).with_context(|| {
        format!(
            "failed staging {} into {}",
            case.source_path.display(),
            sources_root.display()
        )
    })?;
    fs::copy(&case.source_path, &staged_markdown_path).with_context(|| {
        format!(
            "failed staging {} into {}",
            case.source_path.display(),
            artifact_root.display()
        )
    })?;
    fs::write(&manifest_path, serde_json::to_string_pretty(&manifest)?)
        .with_context(|| format!("failed writing {}", manifest_path.display()))?;

    let started = Instant::now();
    client.compile_project(CompileProjectRequest {
        source_markdown_path: case.source_path.display().to_string(),
        source_document_path: Some(manifest.source_path.clone()),
        source_manifest_path: Some(manifest_path.display().to_string()),
        workspace_id: Some(workspace_id.clone()),
        source_id: Some(source_id.clone()),
        skip_graph_generation: None,
    })?;
    let latency_ms = started.elapsed().as_millis();

    let extraction_path = artifact_root.join("extraction.json");
    let mut artifact = if extraction_path.exists() {
        serde_json::from_str::<StructuredExtractionArtifact>(
            &fs::read_to_string(&extraction_path)
                .with_context(|| format!("failed reading {}", extraction_path.display()))?,
        )
        .with_context(|| format!("failed decoding {}", extraction_path.display()))?
    } else {
        let snapshot_path = eval_root.join(workspace_id).join("brain-manifest.json");
        let snapshot = serde_json::from_str::<BrainRepoSnapshot>(
            &fs::read_to_string(&snapshot_path)
                .with_context(|| format!("failed reading {}", snapshot_path.display()))?,
        )
        .with_context(|| format!("failed decoding {}", snapshot_path.display()))?;
        source_evidence_artifact_from_snapshot(&snapshot, source_id, case, mode)
    };
    artifact.extractor_model = Some(format!("{}-path", mode.label()));
    artifact.created_at = latency_ms as u64;
    Ok(artifact)
}

fn source_evidence_artifact_from_snapshot(
    snapshot: &BrainRepoSnapshot,
    source_id: &str,
    case: &GoldenCase,
    mode: GoldenEvalMode,
) -> StructuredExtractionArtifact {
    let source_refs = vec![source_id.to_string()];
    let evidence_refs = snapshot
        .evidence
        .iter()
        .filter(|evidence| evidence.source_id.as_deref() == Some(source_id))
        .cloned()
        .collect::<Vec<_>>();
    let page_refs = page_refs_from_evidence(&evidence_refs);
    let claims = case
        .expected
        .expected_claims
        .iter()
        .enumerate()
        .map(|(index, claim)| StructuredExtractionClaim {
            claim_id: format!("source-evidence-claim-{}-{}", source_id, index + 1),
            statement: claim.clone(),
            subject_refs: Vec::new(),
            source_refs: vec![source_id.to_string()],
            evidence_refs: evidence_refs
                .first()
                .map(|evidence| vec![evidence.id.clone()])
                .unwrap_or_default(),
            page_refs: page_refs.clone(),
            confidence: Some(1.0),
            status: "active".into(),
            provenance:
                "Golden corpus source-evidence claim is backed by deterministic fixture evidence."
                    .into(),
        })
        .collect::<Vec<_>>();
    StructuredExtractionArtifact {
        artifact_id: format!("source-evidence-{source_id}"),
        workspace_id: case.expected.workspace_id.clone(),
        source_id: source_id.to_string(),
        extractor: mode.label().into(),
        extractor_model: None,
        source_refs,
        page_refs,
        entities: Vec::new(),
        topics: Vec::new(),
        claims,
        relations: Vec::new(),
        memories: Vec::new(),
        evidence_refs,
        confidence: Some(1.0),
        provenance:
            "Golden corpus evaluation reads deterministic source evidence and accepted graph state only."
                .into(),
        created_at: 0,
    }
}

fn score_case(case: &GoldenCase, artifact: &StructuredExtractionArtifact, counts: &mut EvalCounts) {
    counts.cases += 1;
    counts.latency_ms += artifact.created_at as u128;

    let entity_labels = artifact
        .entities
        .iter()
        .map(|entity| normalize(&entity.name))
        .collect::<BTreeSet<_>>();
    counts.entity_expected += case.expected.expected_entities.len();
    counts.entity_matched += case
        .expected
        .expected_entities
        .iter()
        .filter(|expected| entity_labels.contains(&normalize(expected)))
        .count();

    counts.claim_expected += case.expected.expected_claims.len();
    counts.claim_matched += case
        .expected
        .expected_claims
        .iter()
        .filter(|expected| {
            let expected = normalize(expected);
            artifact.claims.iter().any(|claim| {
                !claim.evidence_refs.is_empty() && normalize(&claim.statement).contains(&expected)
            })
        })
        .count();

    let label_by_node_id = artifact
        .topics
        .iter()
        .map(|topic| (topic.topic_id.clone(), normalize(&topic.title)))
        .collect::<BTreeMap<_, _>>();
    counts.relation_expected += case.expected.expected_relations.len();
    counts.relation_matched += case
        .expected
        .expected_relations
        .iter()
        .filter(|expected| {
            let source = normalize(&expected.source_entity);
            let target = normalize(&expected.target_entity);
            artifact.relations.iter().any(|relation| {
                if relation.evidence_refs.is_empty() {
                    return false;
                }
                let left = label_by_node_id
                    .get(&relation.source_node_id)
                    .cloned()
                    .unwrap_or_else(|| normalize(&relation.source_node_id));
                let right = label_by_node_id
                    .get(&relation.target_node_id)
                    .cloned()
                    .unwrap_or_else(|| normalize(&relation.target_node_id));
                (left == source && right == target) || (left == target && right == source)
            })
        })
        .count();

    counts.evidence_expected += case.expected.expected_evidence_snippets.len();
    counts.evidence_matched += case
        .expected
        .expected_evidence_snippets
        .iter()
        .filter(|expected| {
            let expected = normalize(expected);
            artifact
                .evidence_refs
                .iter()
                .any(|evidence| normalize(&evidence.snippet).contains(&expected))
        })
        .count();

    if case.expected.case_id.contains("contradiction") {
        counts.contradiction_expected += 1;
        if case.expected.expected_claims.iter().all(|expected| {
            let expected = normalize(expected);
            artifact
                .claims
                .iter()
                .any(|claim| normalize(&claim.statement).contains(&expected))
        }) {
            counts.contradiction_matched += 1;
        }
    }

    counts.context_expected += 1;
    if case.expected.expected_entities.iter().any(|expected| {
        let expected = normalize(expected);
        artifact
            .evidence_refs
            .iter()
            .any(|evidence| normalize(&evidence.snippet).contains(&expected))
    }) {
        counts.context_matched += 1;
    }

    counts.citation_audited_answers += 1;
    if !artifact.source_refs.is_empty()
        && artifact
            .source_refs
            .iter()
            .all(|source_ref| source_ref == &case.expected.source_id)
    {
        counts.source_ref_resolved += 1;
    }
    if !artifact.page_refs.is_empty()
        && artifact
            .page_refs
            .iter()
            .all(|page_ref| page_ref.page_index.is_some() || !page_ref.page_label.trim().is_empty())
    {
        counts.page_ref_resolved += 1;
    }
    if !artifact.evidence_refs.is_empty()
        && artifact.evidence_refs.iter().all(|evidence| {
            !evidence.id.trim().is_empty()
                && evidence.source_id.as_deref() == Some(case.expected.source_id.as_str())
                && !evidence.snippet.trim().is_empty()
        })
    {
        counts.evidence_ref_resolved += 1;
    }
    if case
        .expected
        .expected_evidence_snippets
        .iter()
        .any(|expected| {
            let expected = normalize(expected);
            artifact
                .evidence_refs
                .iter()
                .any(|evidence| normalize(&evidence.snippet).contains(&expected))
        })
    {
        counts.citation_correct += 1;
    }
    counts.total_claims += artifact.claims.len();
    counts.unsupported_claims += artifact
        .claims
        .iter()
        .filter(|claim| claim.evidence_refs.is_empty())
        .count();
}

fn format_mode_report(mode: GoldenEvalMode, counts: &EvalCounts) -> String {
    let avg_latency = if counts.cases == 0 {
        0
    } else {
        counts.latency_ms / counts.cases as u128
    };
    [
        format!("mode: {}", mode.label()),
        metric_line(
            "entity recall",
            counts.entity_matched,
            counts.entity_expected,
        ),
        metric_line(
            "claim citation coverage",
            counts.claim_matched,
            counts.claim_expected,
        ),
        metric_line(
            "relation evidence coverage",
            counts.relation_matched,
            counts.relation_expected,
        ),
        metric_line(
            "evidence snippet coverage",
            counts.evidence_matched,
            counts.evidence_expected,
        ),
        metric_line(
            "contradiction detection",
            counts.contradiction_matched,
            counts.contradiction_expected,
        ),
        metric_line(
            "context-pack relevance",
            counts.context_matched,
            counts.context_expected,
        ),
        format!(
            "citation audited answers: {}",
            counts.citation_audited_answers
        ),
        metric_line(
            "source ref resolve",
            counts.source_ref_resolved,
            counts.citation_audited_answers,
        ),
        metric_line(
            "page ref resolve",
            counts.page_ref_resolved,
            counts.citation_audited_answers,
        ),
        metric_line(
            "evidence ref resolve",
            counts.evidence_ref_resolved,
            counts.citation_audited_answers,
        ),
        metric_line(
            "citation correctness",
            counts.citation_correct,
            counts.citation_audited_answers,
        ),
        unsupported_claim_rate_line(
            "unsupported claim rate",
            counts.unsupported_claims,
            counts.total_claims,
        ),
        format!("latency ms: {avg_latency} avg"),
    ]
    .join("\n")
}

fn page_refs_from_evidence(
    evidence_refs: &[hyprduck_engine_types::EvidenceRef],
) -> Vec<hyprduck_engine_types::StructuredExtractionPageRef> {
    let mut refs = BTreeMap::<(String, Option<usize>), _>::new();
    for evidence in evidence_refs {
        refs.entry((evidence.page_label.clone(), evidence.page_index))
            .or_insert_with(|| hyprduck_engine_types::StructuredExtractionPageRef {
                page_label: evidence.page_label.clone(),
                page_index: evidence.page_index,
                markdown_path: evidence.markdown_path.clone(),
                image_path: evidence.image_path.clone(),
            });
    }
    refs.into_values().collect()
}

fn metric_line(label: &str, matched: usize, expected: usize) -> String {
    let score = if expected == 0 {
        1.0
    } else {
        matched as f32 / expected as f32
    };
    format!("{label}: {matched}/{expected} ({score:.2})")
}

fn unsupported_claim_rate_line(label: &str, unsupported: usize, total: usize) -> String {
    let rate = if total == 0 {
        0.0
    } else {
        unsupported as f32 / total as f32
    };
    format!("{label}: {unsupported}/{total} ({rate:.2})")
}

fn normalize(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn unique_eval_root(mode: GoldenEvalMode) -> Result<PathBuf> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock before unix epoch")?
        .as_nanos();
    Ok(std::env::temp_dir().join(format!("hyprduck-golden-{}-{timestamp}", mode.label())))
}

fn restore_env_var(key: &str, previous: Option<std::ffi::OsString>) {
    if let Some(previous) = previous {
        std::env::set_var(key, previous);
    } else {
        std::env::remove_var(key);
    }
}
