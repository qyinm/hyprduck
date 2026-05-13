use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use hyprduck_engine_types::{
    AnswerProjectRequest, AnswerProjectResponseData, AnswerResponse, ApplyCorrectionRequest,
    ApplyCorrectionResponseData, BrainContextPack, CheckReadinessRequest, CompileProjectRequest,
    CompileProjectResponseData, EngineCommand, EngineConfigPayload, EngineFailure, EngineRequest,
    EngineSuccess, GetBrainHealthRequest, GetBrainHealthResponseData, KnowledgeProject,
    ListBrainReviewItemsRequest, ListBrainReviewItemsResponseData, LoadConfigRequest,
    LoadProjectRequest, LoadProjectResponseData, ParseEvent, ParseProgress, ParseRequest,
    ParseResponseData, ProposeBrainUpdateRequest, ProposeBrainUpdateResponseData,
    ReadGraphHistoryRequest, ReadGraphHistoryResponseData, ReadGraphSnapshotRequest,
    ReadGraphSnapshotResponseData, ReadNodeRequest, ReadNodeResponseData, ReadRecentEventsRequest,
    ReadRecentEventsResponseData, ReadSourceRequest, ReadSourceResponseData, ReadWikiPageRequest,
    ReadWikiPageResponseData, ReconstructBrainRequest, ReconstructBrainResponseData,
    ResolveBrainReviewItemRequest, ResolveBrainReviewItemResponseData,
    RuntimeReadinessResponseData, SaveConfigRequest, SaveConfigResponseData, SearchBrainRequest,
    SearchBrainResponseData, ValidateProviderRequest, ValidateProviderResponseData,
};

pub trait EngineClient {
    fn parse(
        &self,
        request: ParseRequest,
        on_progress: &mut dyn FnMut(ParseProgress),
    ) -> Result<ParseResponseData>;

    fn compile_project(&self, request: CompileProjectRequest)
        -> Result<CompileProjectResponseData>;
    fn load_project(&self, project_id: Option<String>) -> Result<LoadProjectResponseData>;
    fn apply_correction(&self, request: ApplyCorrectionRequest) -> Result<KnowledgeProject>;
    fn answer_project(&self, request: AnswerProjectRequest) -> Result<AnswerResponse>;
    fn search_brain(&self, request: SearchBrainRequest) -> Result<SearchBrainResponseData>;
    fn read_source(&self, request: ReadSourceRequest) -> Result<ReadSourceResponseData>;
    fn read_wiki_page(&self, request: ReadWikiPageRequest) -> Result<ReadWikiPageResponseData>;
    fn read_node(&self, request: ReadNodeRequest) -> Result<ReadNodeResponseData>;
    fn read_recent_events(
        &self,
        request: ReadRecentEventsRequest,
    ) -> Result<ReadRecentEventsResponseData>;
    fn read_graph_history(
        &self,
        request: ReadGraphHistoryRequest,
    ) -> Result<ReadGraphHistoryResponseData>;
    fn read_graph_snapshot(
        &self,
        request: ReadGraphSnapshotRequest,
    ) -> Result<ReadGraphSnapshotResponseData>;
    fn reconstruct_brain(
        &self,
        request: ReconstructBrainRequest,
    ) -> Result<ReconstructBrainResponseData>;
    fn get_context_pack(
        &self,
        request: hyprduck_engine_types::GetContextPackRequest,
    ) -> Result<BrainContextPack>;
    fn propose_brain_update(
        &self,
        request: ProposeBrainUpdateRequest,
    ) -> Result<ProposeBrainUpdateResponseData>;
    fn list_brain_review_items(
        &self,
        request: ListBrainReviewItemsRequest,
    ) -> Result<ListBrainReviewItemsResponseData>;
    fn resolve_brain_review_item(
        &self,
        request: ResolveBrainReviewItemRequest,
    ) -> Result<ResolveBrainReviewItemResponseData>;
    fn get_brain_health(
        &self,
        request: GetBrainHealthRequest,
    ) -> Result<GetBrainHealthResponseData>;
    fn load_config(&self) -> Result<EngineConfigPayload>;
    fn save_config(&self, config: EngineConfigPayload) -> Result<SaveConfigResponseData>;
    fn validate_provider(
        &self,
        config: Option<EngineConfigPayload>,
    ) -> Result<ValidateProviderResponseData>;
    fn check_readiness(&self) -> Result<RuntimeReadinessResponseData>;
}

#[derive(Debug, Clone)]
pub struct SubprocessEngineClient {
    launch_spec: EngineLaunchSpec,
}

#[derive(Debug, Clone)]
pub struct EngineLaunchSpec {
    program: PathBuf,
    args: Vec<OsString>,
    current_dir: Option<PathBuf>,
    display: String,
}

impl Default for SubprocessEngineClient {
    fn default() -> Self {
        Self {
            launch_spec: resolve_engine_launch()
                .unwrap_or_else(|_| EngineLaunchSpec::binary(PathBuf::from("hyprduck-engine"))),
        }
    }
}

impl SubprocessEngineClient {
    pub fn new(engine_bin: PathBuf) -> Self {
        Self {
            launch_spec: EngineLaunchSpec::binary(engine_bin),
        }
    }

    pub fn launch_display(&self) -> &str {
        &self.launch_spec.display
    }

    fn run_command<T, R>(
        &self,
        request: EngineRequest,
        command_kind: EngineCommand,
        on_progress: Option<&mut dyn FnMut(ParseProgress)>,
    ) -> Result<R>
    where
        T: serde::de::DeserializeOwned,
        R: From<T>,
    {
        let mut command = self.launch_spec.command();
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = command.spawn().with_context(|| {
            format!(
                "failed to spawn engine runtime {}",
                self.launch_spec.display
            )
        })?;

        let mut stdin = child.stdin.take().context("missing child stdin")?;
        let stdout = child.stdout.take().context("missing child stdout")?;
        let stderr = child.stderr.take().context("missing child stderr")?;

        let payload = serde_json::to_vec(&request).context("failed to encode engine request")?;
        stdin
            .write_all(&payload)
            .context("failed to write engine request to stdin")?;
        drop(stdin);

        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines() {
                match line {
                    Ok(line) => {
                        if tx.send(line).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        let mut stdout_reader = BufReader::new(stdout);
        let mut stdout_payload = String::new();
        let mut stderr_lines = Vec::new();
        let mut on_progress = on_progress;

        loop {
            while let Ok(line) = rx.try_recv() {
                stderr_lines.push(line.clone());
                if command_kind == EngineCommand::Parse {
                    if let Some(ref mut callback) = on_progress {
                        if let Ok(event) = serde_json::from_str::<ParseEvent>(&line) {
                            callback(event.into());
                        }
                    }
                }
            }

            if let Some(status) = child
                .try_wait()
                .context("failed waiting on engine process")?
            {
                stdout_reader
                    .read_to_string(&mut stdout_payload)
                    .context("failed reading engine stdout")?;
                while let Ok(line) = rx.try_recv() {
                    stderr_lines.push(line.clone());
                    if command_kind == EngineCommand::Parse {
                        if let Some(ref mut callback) = on_progress {
                            if let Ok(event) = serde_json::from_str::<ParseEvent>(&line) {
                                callback(event.into());
                            }
                        }
                    }
                }

                if !status.success() {
                    if let Ok(failure) = serde_json::from_str::<EngineFailure>(&stdout_payload) {
                        return Err(anyhow!("{}: {}", failure.error.code, failure.error.message));
                    }
                    let last_stderr = stderr_lines
                        .iter()
                        .rev()
                        .find(|line| !line.trim().is_empty())
                        .cloned()
                        .unwrap_or_else(|| "no stderr output".to_string());
                    return Err(anyhow!("engine exited with status {status}: {last_stderr}"));
                }

                let response: EngineSuccess<T> = serde_json::from_str(&stdout_payload)
                    .context("failed decoding engine response")?;
                if response.command != command_kind {
                    return Err(anyhow!(
                        "engine response command mismatch: expected {:?}, got {:?}",
                        command_kind,
                        response.command
                    ));
                }
                return Ok(response.data.into());
            }

            thread::sleep(Duration::from_millis(20));
        }
    }
}

impl EngineClient for SubprocessEngineClient {
    fn parse(
        &self,
        request: ParseRequest,
        on_progress: &mut dyn FnMut(ParseProgress),
    ) -> Result<ParseResponseData> {
        self.run_command::<ParseResponseData, ParseResponseData>(
            EngineRequest::Parse(request),
            EngineCommand::Parse,
            Some(on_progress),
        )
    }

    fn compile_project(
        &self,
        request: CompileProjectRequest,
    ) -> Result<CompileProjectResponseData> {
        self.run_command::<CompileProjectResponseData, CompileProjectResponseData>(
            EngineRequest::CompileProject(request),
            EngineCommand::CompileProject,
            None,
        )
    }

    fn load_project(&self, project_id: Option<String>) -> Result<LoadProjectResponseData> {
        self.run_command::<LoadProjectResponseData, LoadProjectResponseData>(
            EngineRequest::LoadProject(LoadProjectRequest {
                project_id,
                workspace_id: None,
            }),
            EngineCommand::LoadProject,
            None,
        )
    }

    fn apply_correction(&self, request: ApplyCorrectionRequest) -> Result<KnowledgeProject> {
        let response = self
            .run_command::<ApplyCorrectionResponseData, ApplyCorrectionResponseData>(
                EngineRequest::ApplyCorrection(request),
                EngineCommand::ApplyCorrection,
                None,
            )?;
        Ok(response.project)
    }

    fn answer_project(&self, request: AnswerProjectRequest) -> Result<AnswerResponse> {
        let response = self.run_command::<AnswerProjectResponseData, AnswerProjectResponseData>(
            EngineRequest::AnswerProject(request),
            EngineCommand::AnswerProject,
            None,
        )?;
        Ok(response.answer)
    }

    fn search_brain(&self, request: SearchBrainRequest) -> Result<SearchBrainResponseData> {
        self.run_command::<SearchBrainResponseData, SearchBrainResponseData>(
            EngineRequest::SearchBrain(request),
            EngineCommand::SearchBrain,
            None,
        )
    }

    fn read_source(&self, request: ReadSourceRequest) -> Result<ReadSourceResponseData> {
        self.run_command::<ReadSourceResponseData, ReadSourceResponseData>(
            EngineRequest::ReadSource(request),
            EngineCommand::ReadSource,
            None,
        )
    }

    fn read_wiki_page(&self, request: ReadWikiPageRequest) -> Result<ReadWikiPageResponseData> {
        self.run_command::<ReadWikiPageResponseData, ReadWikiPageResponseData>(
            EngineRequest::ReadWikiPage(request),
            EngineCommand::ReadWikiPage,
            None,
        )
    }

    fn read_node(&self, request: ReadNodeRequest) -> Result<ReadNodeResponseData> {
        self.run_command::<ReadNodeResponseData, ReadNodeResponseData>(
            EngineRequest::ReadNode(request),
            EngineCommand::ReadNode,
            None,
        )
    }

    fn read_recent_events(
        &self,
        request: ReadRecentEventsRequest,
    ) -> Result<ReadRecentEventsResponseData> {
        self.run_command::<ReadRecentEventsResponseData, ReadRecentEventsResponseData>(
            EngineRequest::ReadRecentEvents(request),
            EngineCommand::ReadRecentEvents,
            None,
        )
    }

    fn read_graph_history(
        &self,
        request: ReadGraphHistoryRequest,
    ) -> Result<ReadGraphHistoryResponseData> {
        self.run_command::<ReadGraphHistoryResponseData, ReadGraphHistoryResponseData>(
            EngineRequest::ReadGraphHistory(request),
            EngineCommand::ReadGraphHistory,
            None,
        )
    }

    fn read_graph_snapshot(
        &self,
        request: ReadGraphSnapshotRequest,
    ) -> Result<ReadGraphSnapshotResponseData> {
        self.run_command::<ReadGraphSnapshotResponseData, ReadGraphSnapshotResponseData>(
            EngineRequest::ReadGraphSnapshot(request),
            EngineCommand::ReadGraphSnapshot,
            None,
        )
    }

    fn reconstruct_brain(
        &self,
        request: ReconstructBrainRequest,
    ) -> Result<ReconstructBrainResponseData> {
        self.run_command::<ReconstructBrainResponseData, ReconstructBrainResponseData>(
            EngineRequest::ReconstructBrain(request),
            EngineCommand::ReconstructBrain,
            None,
        )
    }

    fn get_context_pack(
        &self,
        request: hyprduck_engine_types::GetContextPackRequest,
    ) -> Result<BrainContextPack> {
        let response = self.run_command::<
            hyprduck_engine_types::GetContextPackResponseData,
            hyprduck_engine_types::GetContextPackResponseData,
        >(
            EngineRequest::GetContextPack(request),
            EngineCommand::GetContextPack,
            None,
        )?;
        Ok(response.context_pack)
    }

    fn propose_brain_update(
        &self,
        request: ProposeBrainUpdateRequest,
    ) -> Result<ProposeBrainUpdateResponseData> {
        self.run_command::<ProposeBrainUpdateResponseData, ProposeBrainUpdateResponseData>(
            EngineRequest::ProposeBrainUpdate(request),
            EngineCommand::ProposeBrainUpdate,
            None,
        )
    }

    fn list_brain_review_items(
        &self,
        request: ListBrainReviewItemsRequest,
    ) -> Result<ListBrainReviewItemsResponseData> {
        self.run_command::<ListBrainReviewItemsResponseData, ListBrainReviewItemsResponseData>(
            EngineRequest::ListBrainReviewItems(request),
            EngineCommand::ListBrainReviewItems,
            None,
        )
    }

    fn resolve_brain_review_item(
        &self,
        request: ResolveBrainReviewItemRequest,
    ) -> Result<ResolveBrainReviewItemResponseData> {
        self.run_command::<ResolveBrainReviewItemResponseData, ResolveBrainReviewItemResponseData>(
            EngineRequest::ResolveBrainReviewItem(request),
            EngineCommand::ResolveBrainReviewItem,
            None,
        )
    }

    fn get_brain_health(
        &self,
        request: GetBrainHealthRequest,
    ) -> Result<GetBrainHealthResponseData> {
        self.run_command::<GetBrainHealthResponseData, GetBrainHealthResponseData>(
            EngineRequest::GetBrainHealth(request),
            EngineCommand::GetBrainHealth,
            None,
        )
    }

    fn load_config(&self) -> Result<EngineConfigPayload> {
        self.run_command::<EngineConfigPayload, EngineConfigPayload>(
            EngineRequest::LoadConfig(LoadConfigRequest {}),
            EngineCommand::LoadConfig,
            None,
        )
    }

    fn save_config(&self, config: EngineConfigPayload) -> Result<SaveConfigResponseData> {
        self.run_command::<SaveConfigResponseData, SaveConfigResponseData>(
            EngineRequest::SaveConfig(SaveConfigRequest { config }),
            EngineCommand::SaveConfig,
            None,
        )
    }

    fn validate_provider(
        &self,
        config: Option<EngineConfigPayload>,
    ) -> Result<ValidateProviderResponseData> {
        self.run_command::<ValidateProviderResponseData, ValidateProviderResponseData>(
            EngineRequest::ValidateProvider(ValidateProviderRequest { config }),
            EngineCommand::ValidateProvider,
            None,
        )
    }

    fn check_readiness(&self) -> Result<RuntimeReadinessResponseData> {
        self.run_command::<RuntimeReadinessResponseData, RuntimeReadinessResponseData>(
            EngineRequest::CheckReadiness(CheckReadinessRequest {}),
            EngineCommand::CheckReadiness,
            None,
        )
    }
}

pub fn resolve_engine_bin() -> Result<PathBuf> {
    if let Some(explicit) = std::env::var_os("HYPRDUCK_ENGINE_BIN") {
        return Ok(PathBuf::from(explicit));
    }

    if let Some(explicit) = std::env::var_os("CARGO_BIN_EXE_hyprduck-engine") {
        return Ok(PathBuf::from(explicit));
    }

    let current_exe = std::env::current_exe().context("failed to locate current executable")?;
    for root in candidate_roots(
        &current_exe,
        std::env::current_dir().ok(),
        std::env::var_os("CARGO_MANIFEST_DIR").map(PathBuf::from),
    ) {
        let candidate = root.join(engine_binary_name());
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    Err(anyhow!("unable to resolve hyprduck-engine binary"))
}

fn engine_binary_name() -> OsString {
    if cfg!(target_os = "windows") {
        OsString::from("hyprduck-engine.exe")
    } else {
        OsString::from("hyprduck-engine")
    }
}

pub fn resolve_engine_launch() -> Result<EngineLaunchSpec> {
    if let Ok(engine_bin) = resolve_engine_bin() {
        return Ok(EngineLaunchSpec::binary(engine_bin));
    }

    let current_exe = std::env::current_exe().context("failed to locate current executable")?;
    let workspace_root = detect_workspace_root(
        std::env::current_dir().ok(),
        std::env::var_os("CARGO_MANIFEST_DIR").map(PathBuf::from),
        current_exe.parent().map(Path::to_path_buf),
    )
    .context("unable to resolve hyprduck-engine runtime")?;

    Ok(EngineLaunchSpec::cargo_run(workspace_root))
}

impl EngineLaunchSpec {
    fn binary(program: PathBuf) -> Self {
        let display = program.display().to_string();
        Self {
            program,
            args: Vec::new(),
            current_dir: None,
            display,
        }
    }

    fn cargo_run(workspace_root: PathBuf) -> Self {
        let args = vec![
            OsString::from("run"),
            OsString::from("--quiet"),
            OsString::from("-p"),
            OsString::from("hyprduck-engine"),
            OsString::from("--"),
        ];
        Self {
            program: PathBuf::from("cargo"),
            args,
            current_dir: Some(workspace_root),
            display: "cargo run --quiet -p hyprduck-engine --".to_string(),
        }
    }

    pub fn display(&self) -> &str {
        &self.display
    }

    pub fn command(&self) -> Command {
        let mut command = Command::new(&self.program);
        command.args(&self.args);
        if let Some(current_dir) = &self.current_dir {
            command.current_dir(current_dir);
        }
        command
    }
}

fn candidate_roots(
    current_exe: &Path,
    current_dir: Option<PathBuf>,
    manifest_dir: Option<PathBuf>,
) -> Vec<PathBuf> {
    let mut roots = Vec::new();

    if let Some(parent) = current_exe.parent() {
        roots.push(parent.to_path_buf());
        if let Some(grandparent) = parent.parent() {
            roots.push(grandparent.to_path_buf());
        }
    }

    if let Some(current_dir) = current_dir {
        roots.push(current_dir.join("target/debug"));
        roots.push(current_dir.join("target/release"));
        if let Some(workspace_root) = find_workspace_root(&current_dir) {
            roots.push(workspace_root.join("target/debug"));
            roots.push(workspace_root.join("target/release"));
        }
    }

    if let Some(manifest_dir) = manifest_dir {
        roots.push(manifest_dir.join("../../target/debug"));
        roots.push(manifest_dir.join("../../target/release"));
        if let Some(workspace_root) = find_workspace_root(&manifest_dir) {
            roots.push(workspace_root.join("target/debug"));
            roots.push(workspace_root.join("target/release"));
        }
    }

    roots
}

fn detect_workspace_root(
    current_dir: Option<PathBuf>,
    manifest_dir: Option<PathBuf>,
    current_exe_dir: Option<PathBuf>,
) -> Option<PathBuf> {
    current_dir
        .and_then(|dir| find_workspace_root(&dir))
        .or_else(|| manifest_dir.and_then(|dir| find_workspace_root(&dir)))
        .or_else(|| current_exe_dir.and_then(|dir| find_workspace_root(&dir)))
}

fn find_workspace_root(start: &Path) -> Option<PathBuf> {
    let mut current = Some(start);
    while let Some(path) = current {
        if path.join("Cargo.toml").exists() {
            return Some(path.to_path_buf());
        }
        current = path.parent();
    }
    None
}
