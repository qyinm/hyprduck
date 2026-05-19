use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;

const SCHEMA_VERSION: &str = "hyprduck.benchmark.v1";
const BASELINES: &[BenchmarkBaseline] = &[
    BenchmarkBaseline::RawTextDump,
    BenchmarkBaseline::DirectUploadChat,
    BenchmarkBaseline::ContextPack,
    BenchmarkBaseline::ContextPackPageEvidence,
];

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BenchmarkReport {
    schema_version: String,
    generated_at: String,
    documents: Vec<BenchmarkDocument>,
    runs: Vec<BenchmarkRun>,
    comparisons: Vec<BenchmarkComparison>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BenchmarkDocument {
    document_id: String,
    document_type: BenchmarkDocumentType,
    task_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
enum BenchmarkDocumentType {
    Pdf,
    Docx,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
enum BenchmarkBaseline {
    RawTextDump,
    DirectUploadChat,
    ContextPack,
    ContextPackPageEvidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ProviderClass {
    Hosted,
    Local,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Judgment {
    Pass,
    Partial,
    Fail,
    NotApplicable,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BenchmarkRun {
    run_id: String,
    document_id: String,
    task_id: String,
    baseline: BenchmarkBaseline,
    provider_route: String,
    provider_class: ProviderClass,
    task_correctness: Judgment,
    citation_correctness: Judgment,
    unsupported_claim: bool,
    useful_answer_ms: u32,
    visual_table_handling: Judgment,
    repeated_use_success: bool,
    user_confidence: u8,
    failure_taxonomy: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BenchmarkComparison {
    metric: String,
    outcome: ComparisonOutcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ComparisonOutcome {
    Win,
    Tie,
    Loss,
}

pub fn run_benchmark_report(input: String) -> Result<String> {
    let body = fs::read_to_string(&input).with_context(|| format!("failed reading {input}"))?;
    let report = serde_json::from_str::<BenchmarkReport>(&body)
        .with_context(|| format!("failed decoding {input}"))?;
    validate_report(&report)?;
    Ok(format_report(&report))
}

fn validate_report(report: &BenchmarkReport) -> Result<()> {
    if report.schema_version != SCHEMA_VERSION {
        bail!(
            "unsupported benchmark schemaVersion {}; expected {SCHEMA_VERSION}",
            report.schema_version
        );
    }
    reject_sensitive("generatedAt", &report.generated_at)?;
    validate_documents(report)?;
    validate_runs(report)?;
    validate_baseline_matrix(report)?;
    validate_provider_variance(report)?;
    validate_context_pack_comparison(report)?;
    validate_comparisons(report)?;
    Ok(())
}

fn validate_documents(report: &BenchmarkReport) -> Result<()> {
    if report.documents.len() < 2 {
        bail!("benchmark report needs at least 2 documents");
    }
    if !report
        .documents
        .iter()
        .any(|document| document.document_type == BenchmarkDocumentType::Pdf)
    {
        bail!("benchmark report needs at least one PDF document");
    }
    if !report
        .documents
        .iter()
        .any(|document| document.document_type == BenchmarkDocumentType::Docx)
    {
        bail!("benchmark report needs at least one DOCX document");
    }
    for document in &report.documents {
        reject_sensitive("documentId", &document.document_id)?;
        if document.task_ids.len() < 3 {
            bail!(
                "document {} needs at least 3 benchmark tasks",
                document.document_id
            );
        }
        for task_id in &document.task_ids {
            reject_sensitive("taskId", task_id)?;
        }
    }
    Ok(())
}

fn validate_runs(report: &BenchmarkReport) -> Result<()> {
    if report.runs.len() < 24 {
        bail!("benchmark report needs at least 24 runs");
    }
    let documents = report
        .documents
        .iter()
        .map(|document| document.document_id.as_str())
        .collect::<BTreeSet<_>>();
    let tasks = report
        .documents
        .iter()
        .flat_map(|document| {
            document
                .task_ids
                .iter()
                .map(|task_id| (document.document_id.as_str(), task_id.as_str()))
        })
        .collect::<BTreeSet<_>>();
    for run in &report.runs {
        reject_sensitive("runId", &run.run_id)?;
        reject_sensitive("documentId", &run.document_id)?;
        reject_sensitive("taskId", &run.task_id)?;
        reject_sensitive("providerRoute", &run.provider_route)?;
        for failure in &run.failure_taxonomy {
            reject_sensitive("failureTaxonomy", failure)?;
        }
        if !documents.contains(run.document_id.as_str()) {
            bail!("run {} references unknown document", run.run_id);
        }
        if !tasks.contains(&(run.document_id.as_str(), run.task_id.as_str())) {
            bail!("run {} references unknown task", run.run_id);
        }
        if run.useful_answer_ms == 0 {
            bail!("run {} needs useful answer timing", run.run_id);
        }
        if !(1..=5).contains(&run.user_confidence) {
            bail!("run {} user confidence must be 1..5", run.run_id);
        }
        let _ = run.task_correctness;
        let _ = run.visual_table_handling;
        let _ = run.repeated_use_success;
    }
    Ok(())
}

fn validate_baseline_matrix(report: &BenchmarkReport) -> Result<()> {
    let mut by_task = BTreeMap::<(&str, &str), BTreeSet<BenchmarkBaseline>>::new();
    for run in &report.runs {
        by_task
            .entry((run.document_id.as_str(), run.task_id.as_str()))
            .or_default()
            .insert(run.baseline);
    }
    for document in &report.documents {
        for task_id in &document.task_ids {
            let baselines = by_task
                .get(&(document.document_id.as_str(), task_id.as_str()))
                .cloned()
                .unwrap_or_default();
            for baseline in BASELINES {
                if !baselines.contains(baseline) {
                    bail!(
                        "document {} task {} is missing baseline {}",
                        document.document_id,
                        task_id,
                        baseline_label(*baseline)
                    );
                }
            }
        }
    }
    Ok(())
}

fn validate_provider_variance(report: &BenchmarkReport) -> Result<()> {
    let classes = report
        .runs
        .iter()
        .map(|run| run.provider_class)
        .collect::<BTreeSet<_>>();
    if !classes.contains(&ProviderClass::Hosted) || !classes.contains(&ProviderClass::Local) {
        bail!("benchmark report must separate hosted and local provider results");
    }
    Ok(())
}

fn validate_context_pack_comparison(report: &BenchmarkReport) -> Result<()> {
    let direct = aggregate_baseline(report, BenchmarkBaseline::DirectUploadChat);
    let context_page = aggregate_baseline(report, BenchmarkBaseline::ContextPackPageEvidence);
    if context_page.citation_score < direct.citation_score {
        bail!("Context Pack + page evidence citation correctness must be >= direct upload/chat");
    }
    if context_page.unsupported_rate > direct.unsupported_rate {
        bail!("Context Pack + page evidence unsupported claim rate must be <= direct upload/chat");
    }
    Ok(())
}

fn validate_comparisons(report: &BenchmarkReport) -> Result<()> {
    if report.comparisons.is_empty() {
        bail!("benchmark report needs comparison outcomes");
    }
    let has_win = report
        .comparisons
        .iter()
        .any(|comparison| comparison.outcome == ComparisonOutcome::Win);
    let has_loss_or_tie = report.comparisons.iter().any(|comparison| {
        matches!(
            comparison.outcome,
            ComparisonOutcome::Loss | ComparisonOutcome::Tie
        )
    });
    if !has_win || !has_loss_or_tie {
        bail!("benchmark report must include at least one win and one loss or tie");
    }
    for comparison in &report.comparisons {
        reject_sensitive("comparisonMetric", &comparison.metric)?;
        let computed = computed_comparison_outcome(report, &comparison.metric)?;
        if comparison.outcome != computed {
            bail!(
                "comparison {} claimed {} but computed {}",
                comparison.metric,
                comparison_outcome_label(comparison.outcome),
                comparison_outcome_label(computed)
            );
        }
    }
    Ok(())
}

#[derive(Debug)]
struct BaselineAggregate {
    citation_score: f32,
    unsupported_rate: f32,
}

fn aggregate_baseline(report: &BenchmarkReport, baseline: BenchmarkBaseline) -> BaselineAggregate {
    let runs = report
        .runs
        .iter()
        .filter(|run| run.baseline == baseline)
        .collect::<Vec<_>>();
    let citation_points = runs
        .iter()
        .map(|run| judgment_points(run.citation_correctness))
        .sum::<f32>();
    let unsupported = runs.iter().filter(|run| run.unsupported_claim).count();
    let total = runs.len().max(1) as f32;
    BaselineAggregate {
        citation_score: citation_points / total,
        unsupported_rate: unsupported as f32 / total,
    }
}

fn computed_comparison_outcome(
    report: &BenchmarkReport,
    metric: &str,
) -> Result<ComparisonOutcome> {
    let direct = report
        .runs
        .iter()
        .filter(|run| run.baseline == BenchmarkBaseline::DirectUploadChat)
        .collect::<Vec<_>>();
    let context_page = report
        .runs
        .iter()
        .filter(|run| run.baseline == BenchmarkBaseline::ContextPackPageEvidence)
        .collect::<Vec<_>>();
    let direct_score = metric_score(&direct, metric)?;
    let context_score = metric_score(&context_page, metric)?;
    Ok(compare_scores(metric, context_score, direct_score))
}

fn metric_score(runs: &[&BenchmarkRun], metric: &str) -> Result<f32> {
    let total = runs.len().max(1) as f32;
    match metric {
        "task correctness" => Ok(runs
            .iter()
            .map(|run| judgment_points(run.task_correctness))
            .sum::<f32>()
            / total),
        "citation correctness" => Ok(runs
            .iter()
            .map(|run| judgment_points(run.citation_correctness))
            .sum::<f32>()
            / total),
        "unsupported claim rate" => {
            Ok(runs.iter().filter(|run| run.unsupported_claim).count() as f32 / total)
        }
        "useful answer time" => Ok(runs
            .iter()
            .map(|run| run.useful_answer_ms as f32)
            .sum::<f32>()
            / total),
        "visual table handling" => Ok(runs
            .iter()
            .map(|run| judgment_points(run.visual_table_handling))
            .sum::<f32>()
            / total),
        "repeated use success" => {
            Ok(runs.iter().filter(|run| run.repeated_use_success).count() as f32 / total)
        }
        "user confidence" => Ok(runs
            .iter()
            .map(|run| run.user_confidence as f32)
            .sum::<f32>()
            / total),
        _ => bail!("unknown benchmark comparison metric: {metric}"),
    }
}

fn compare_scores(metric: &str, context_page: f32, direct: f32) -> ComparisonOutcome {
    let lower_is_better = matches!(metric, "unsupported claim rate" | "useful answer time");
    let tolerance = if metric == "useful answer time" {
        (direct * 0.05).max(1.0)
    } else {
        0.0001
    };
    let delta = context_page - direct;
    if delta.abs() <= tolerance {
        ComparisonOutcome::Tie
    } else if lower_is_better {
        if context_page < direct {
            ComparisonOutcome::Win
        } else {
            ComparisonOutcome::Loss
        }
    } else if context_page > direct {
        ComparisonOutcome::Win
    } else {
        ComparisonOutcome::Loss
    }
}

fn format_report(report: &BenchmarkReport) -> String {
    let direct = aggregate_baseline(report, BenchmarkBaseline::DirectUploadChat);
    let context_page = aggregate_baseline(report, BenchmarkBaseline::ContextPackPageEvidence);
    let classes = report
        .runs
        .iter()
        .map(|run| provider_class_label(run.provider_class))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join(", ");
    let wins = report
        .comparisons
        .iter()
        .filter(|comparison| comparison.outcome == ComparisonOutcome::Win)
        .count();
    let ties = report
        .comparisons
        .iter()
        .filter(|comparison| comparison.outcome == ComparisonOutcome::Tie)
        .count();
    let losses = report
        .comparisons
        .iter()
        .filter(|comparison| comparison.outcome == ComparisonOutcome::Loss)
        .count();

    [
        format!("benchmark documents: {}", report.documents.len()),
        format!("benchmark runs: {}", report.runs.len()),
        "baselines: raw_text_dump, direct_upload_chat, context_pack, context_pack_page_evidence"
            .to_string(),
        format!("provider classes: {classes}"),
        format!(
            "direct upload/chat citation score: {:.2}",
            direct.citation_score
        ),
        format!(
            "context pack + page evidence citation score: {:.2}",
            context_page.citation_score
        ),
        format!(
            "direct upload/chat unsupported claim rate: {:.2}",
            direct.unsupported_rate
        ),
        format!(
            "context pack + page evidence unsupported claim rate: {:.2}",
            context_page.unsupported_rate
        ),
        format!("comparison outcomes: {wins} win, {ties} tie, {losses} loss"),
        "benchmark export: valid".to_string(),
    ]
    .join("\n")
}

fn judgment_points(judgment: Judgment) -> f32 {
    match judgment {
        Judgment::Pass => 1.0,
        Judgment::Partial => 0.5,
        Judgment::Fail | Judgment::NotApplicable => 0.0,
    }
}

fn reject_sensitive(field: &str, value: &str) -> Result<()> {
    let lower = value.to_ascii_lowercase();
    if lower.contains("/users/")
        || lower.contains("/private/")
        || lower.contains("docs/private")
        || lower.contains("file://")
        || lower.contains("\\users\\")
        || lower.contains("snippet")
        || lower.contains("fulltext")
    {
        return Err(anyhow!(
            "benchmark field {field} appears to contain sensitive path or document content"
        ));
    }
    Ok(())
}

fn baseline_label(baseline: BenchmarkBaseline) -> &'static str {
    match baseline {
        BenchmarkBaseline::RawTextDump => "raw_text_dump",
        BenchmarkBaseline::DirectUploadChat => "direct_upload_chat",
        BenchmarkBaseline::ContextPack => "context_pack",
        BenchmarkBaseline::ContextPackPageEvidence => "context_pack_page_evidence",
    }
}

fn provider_class_label(provider_class: ProviderClass) -> &'static str {
    match provider_class {
        ProviderClass::Hosted => "hosted",
        ProviderClass::Local => "local",
    }
}

fn comparison_outcome_label(outcome: ComparisonOutcome) -> &'static str {
    match outcome {
        ComparisonOutcome::Win => "win",
        ComparisonOutcome::Tie => "tie",
        ComparisonOutcome::Loss => "loss",
    }
}
