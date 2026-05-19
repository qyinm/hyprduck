use std::collections::BTreeSet;
use std::fs;

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;

const SCHEMA_VERSION: &str = "hyprduck.dry-run-metrics.v1";
const REQUIRED_FAILURE_CAUSES: &[FailureCause] = &[
    FailureCause::ProviderConfig,
    FailureCause::Parsing,
    FailureCause::McpRegistration,
    FailureCause::Path,
    FailureCause::Citation,
];

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DryRunMetricsLog {
    schema_version: String,
    generated_at: String,
    dry_runs: Vec<DryRunRecord>,
    repeated_use_events: Vec<RepeatedUseEvent>,
    stop_condition_reviews: Vec<StopConditionReview>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DryRunRecord {
    run_id: String,
    document_type: String,
    provider_route: String,
    mcp_client: String,
    status: DryRunStatus,
    milestones: Vec<String>,
    primary_failure_causes: Vec<FailureCause>,
    mcp_setup_time_to_success_minutes: Option<u32>,
    first_cited_answer_step_count: Option<u32>,
    citation_correctness: DryRunJudgment,
    unsupported_claim_rate: f32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DryRunStatus {
    Success,
    Failure,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DryRunJudgment {
    Pass,
    Fail,
    NotApplicable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
enum FailureCause {
    ProviderConfig,
    Parsing,
    McpRegistration,
    Path,
    Citation,
    OllamaUnavailable,
    UnsupportedFormat,
    Unknown,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RepeatedUseEvent {
    event_id: String,
    source_set_hash: String,
    same_source_set_second_agent_task: bool,
    day_offset: u8,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StopConditionReview {
    review_id: String,
    dry_run_count: usize,
    decision: StopConditionDecision,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum StopConditionDecision {
    Continue,
    NarrowIcp,
    SimplifySetup,
    NarrowDocumentType,
    ReworkArtifactContract,
}

pub fn run_dry_run_log(input: String) -> Result<String> {
    let body = fs::read_to_string(&input).with_context(|| format!("failed reading {input}"))?;
    let log = serde_json::from_str::<DryRunMetricsLog>(&body)
        .with_context(|| format!("failed decoding {input}"))?;
    validate_log(&log)?;
    Ok(format_report(&log))
}

fn validate_log(log: &DryRunMetricsLog) -> Result<()> {
    if log.schema_version != SCHEMA_VERSION {
        bail!(
            "unsupported dry-run metrics schemaVersion {}; expected {SCHEMA_VERSION}",
            log.schema_version
        );
    }
    reject_sensitive("generatedAt", &log.generated_at)?;

    if log.dry_runs.len() < 10 {
        bail!("dry-run log needs at least 10 records");
    }

    let mut failure_causes = BTreeSet::new();
    for run in &log.dry_runs {
        validate_run(run)?;
        for cause in &run.primary_failure_causes {
            failure_causes.insert(*cause);
        }
    }

    for required in REQUIRED_FAILURE_CAUSES {
        if !failure_causes.contains(required) {
            bail!(
                "failure taxonomy missing required class: {}",
                failure_cause_label(*required)
            );
        }
    }

    let first_cited_answer = log
        .dry_runs
        .iter()
        .filter(|run| reached_first_cited_answer(run))
        .count();
    if first_cited_answer < 5 {
        bail!("at least 5 dry runs must reach first cited answer");
    }

    let same_source_second_query = log
        .dry_runs
        .iter()
        .filter(|run| has_milestone(run, "same_source_second_query"))
        .count();
    if same_source_second_query < 3 {
        bail!("at least 3 dry runs must record same_source_second_query");
    }

    let repeated_use_events = valid_repeated_use_events(log)?;
    if repeated_use_events < 10 {
        bail!("at least 10 repeated-use events are required");
    }

    if !log
        .dry_runs
        .iter()
        .any(|run| has_milestone(run, "second_private_document_imported"))
    {
        bail!("dry-run log must record second_private_document_imported at least once");
    }

    if !log
        .repeated_use_events
        .iter()
        .any(|event| event.same_source_set_second_agent_task && event.day_offset == 1)
    {
        bail!("dry-run log must record D1 reuse");
    }
    if !log
        .repeated_use_events
        .iter()
        .any(|event| event.same_source_set_second_agent_task && event.day_offset == 7)
    {
        bail!("dry-run log must record D7 reuse");
    }

    if !log
        .stop_condition_reviews
        .iter()
        .any(|review| review.dry_run_count >= 10)
    {
        bail!("stop condition review must run after at least 10 dry runs");
    }
    for review in &log.stop_condition_reviews {
        reject_sensitive("reviewId", &review.review_id)?;
        let _ = &review.decision;
    }

    Ok(())
}

fn validate_run(run: &DryRunRecord) -> Result<()> {
    reject_sensitive("runId", &run.run_id)?;
    reject_sensitive("documentType", &run.document_type)?;
    reject_sensitive("providerRoute", &run.provider_route)?;
    reject_sensitive("mcpClient", &run.mcp_client)?;
    for milestone in &run.milestones {
        reject_sensitive("milestone", milestone)?;
    }
    match run.status {
        DryRunStatus::Success => {
            if run.mcp_setup_time_to_success_minutes.is_none() {
                bail!(
                    "successful dry run {} is missing MCP setup time",
                    run.run_id
                );
            }
            if run.first_cited_answer_step_count.is_none() {
                bail!(
                    "successful dry run {} is missing first cited answer step count",
                    run.run_id
                );
            }
        }
        DryRunStatus::Failure => {
            if run.primary_failure_causes.is_empty() {
                bail!(
                    "failed dry run {} needs a primary failure cause",
                    run.run_id
                );
            }
        }
    }
    if !(0.0..=1.0).contains(&run.unsupported_claim_rate) {
        bail!(
            "dry run {} unsupported claim rate must be between 0 and 1",
            run.run_id
        );
    }
    let _ = &run.citation_correctness;
    Ok(())
}

fn valid_repeated_use_events(log: &DryRunMetricsLog) -> Result<usize> {
    let mut count = 0usize;
    for event in &log.repeated_use_events {
        reject_sensitive("eventId", &event.event_id)?;
        reject_sensitive("sourceSetHash", &event.source_set_hash)?;
        if event.same_source_set_second_agent_task {
            count += 1;
        }
    }
    Ok(count)
}

fn reached_first_cited_answer(run: &DryRunRecord) -> bool {
    has_milestone(run, "first_cited_answer_with_source_refs")
        && has_milestone(run, "first_cited_answer_with_page_refs")
        && has_milestone(run, "first_cited_answer_with_evidence_refs")
}

fn has_milestone(run: &DryRunRecord, milestone: &str) -> bool {
    run.milestones.iter().any(|value| value == milestone)
}

fn reject_sensitive(field: &str, value: &str) -> Result<()> {
    let lower = value.to_ascii_lowercase();
    if lower.contains("/users/")
        || lower.contains("/private/")
        || lower.contains("docs/private")
        || lower.contains("file://")
        || lower.contains("\\users\\")
        || lower.contains("rawcontent")
        || lower.contains("snippet")
        || lower.contains("fulltext")
    {
        return Err(anyhow!(
            "dry-run metrics field {field} appears to contain sensitive path or document content"
        ));
    }
    Ok(())
}

fn format_report(log: &DryRunMetricsLog) -> String {
    let first_cited_answer = log
        .dry_runs
        .iter()
        .filter(|run| reached_first_cited_answer(run))
        .count();
    let same_source_second_query = log
        .dry_runs
        .iter()
        .filter(|run| has_milestone(run, "same_source_second_query"))
        .count();
    let repeated_use_events = log
        .repeated_use_events
        .iter()
        .filter(|event| event.same_source_set_second_agent_task)
        .count();
    let setup_time_success = log
        .dry_runs
        .iter()
        .filter(|run| matches!(run.status, DryRunStatus::Success))
        .filter(|run| run.mcp_setup_time_to_success_minutes.is_some())
        .count();
    let success_count = log
        .dry_runs
        .iter()
        .filter(|run| matches!(run.status, DryRunStatus::Success))
        .count();
    let failure_causes = log
        .dry_runs
        .iter()
        .flat_map(|run| run.primary_failure_causes.iter().copied())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(failure_cause_label)
        .collect::<Vec<_>>()
        .join(", ");
    let stop_decision = log
        .stop_condition_reviews
        .iter()
        .find(|review| review.dry_run_count >= 10)
        .map(|review| stop_decision_label(&review.decision))
        .unwrap_or("missing");

    [
        format!("dry-run records: {}", log.dry_runs.len()),
        format!(
            "first cited answer: {}/{}",
            first_cited_answer,
            log.dry_runs.len()
        ),
        format!(
            "same-source second query: {}/{}",
            same_source_second_query,
            log.dry_runs.len()
        ),
        format!("repeated-use events: {repeated_use_events}"),
        format!("MCP setup time recorded: {setup_time_success}/{success_count}"),
        format!("failure taxonomy: {failure_causes}"),
        format!("stop condition decision: {stop_decision}"),
        "sensitive metrics scan: passed".to_string(),
    ]
    .join("\n")
}

fn failure_cause_label(cause: FailureCause) -> &'static str {
    match cause {
        FailureCause::ProviderConfig => "provider_config",
        FailureCause::Parsing => "parsing",
        FailureCause::McpRegistration => "mcp_registration",
        FailureCause::Path => "path",
        FailureCause::Citation => "citation",
        FailureCause::OllamaUnavailable => "ollama_unavailable",
        FailureCause::UnsupportedFormat => "unsupported_format",
        FailureCause::Unknown => "unknown",
    }
}

fn stop_decision_label(decision: &StopConditionDecision) -> &'static str {
    match decision {
        StopConditionDecision::Continue => "continue",
        StopConditionDecision::NarrowIcp => "narrow ICP",
        StopConditionDecision::SimplifySetup => "simplify setup",
        StopConditionDecision::NarrowDocumentType => "narrow document type",
        StopConditionDecision::ReworkArtifactContract => "rework artifact contract",
    }
}
