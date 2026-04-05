#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use duckdocs_engine_client::{EngineClient, SubprocessEngineClient};
use duckdocs_engine_types::{
    DocumentFormat, EngineConfigPayload, EngineFailure, EngineRequest, EngineSuccess, ParseEvent,
    ParseInput, ParseOptions, ParseOutputTarget, ParseRequest, ParseResponseData,
    ValidateProviderResponseData,
};
use rfd::FileDialog;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

const SNAPSHOT_EVENT: &str = "duckdocs://snapshot";
const MAX_PROGRESS_LOG: usize = 80;

type SharedStore = Arc<Mutex<AppStore>>;

#[derive(Default)]
struct AppStore {
    snapshot: UiSnapshot,
    active_child: Option<Arc<Mutex<Child>>>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct UiSnapshot {
    active_job: Option<ActiveJobSnapshot>,
    progress_log: Vec<ProgressEntry>,
    last_result: Option<CompletedResultSnapshot>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ActiveJobSnapshot {
    job_id: String,
    file_path: String,
    format: String,
    status: String,
    progress_percent: u8,
    last_message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProgressEntry {
    phase: String,
    message: String,
    timestamp: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CompletedResultSnapshot {
    saved_output_path: Option<String>,
    success_count: usize,
    failed_count: usize,
    markdown: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct FileSelection {
    path: String,
    format: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct StartParseRequest {
    path: String,
    format: String,
}

#[derive(Debug, thiserror::Error)]
enum DesktopError {
    #[error("{0}")]
    Message(String),
}

impl serde::Serialize for DesktopError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

fn main() {
    tauri::Builder::default()
        .manage::<SharedStore>(Arc::new(Mutex::new(AppStore::default())))
        .invoke_handler(tauri::generate_handler![
            app_snapshot,
            pick_import_file,
            load_engine_config,
            save_engine_config,
            validate_engine_config,
            start_parse,
            cancel_parse,
            open_saved_output
        ])
        .setup(|app| {
            if let Err(error) = maybe_import_legacy_swift_config(app.handle()) {
                eprintln!("legacy config migration skipped: {error:#}");
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running DuckDocs desktop");
}

#[tauri::command]
fn app_snapshot(store: tauri::State<'_, SharedStore>) -> UiSnapshot {
    store
        .lock()
        .expect("snapshot lock poisoned")
        .snapshot
        .clone()
}

#[tauri::command]
fn pick_import_file() -> Option<FileSelection> {
    let handle = FileDialog::new()
        .add_filter("Documents", &["pdf", "docx", "doc"])
        .pick_file()?;
    let path = handle.display().to_string();
    let format = detect_format(&handle)?.to_string();
    Some(FileSelection { path, format })
}

#[tauri::command]
fn load_engine_config(app: AppHandle) -> Result<EngineConfigPayload, DesktopError> {
    maybe_import_legacy_swift_config(&app)
        .map_err(|error| DesktopError::Message(error.to_string()))?;
    let engine_path =
        resolve_engine_path(&app).map_err(|error| DesktopError::Message(error.to_string()))?;
    let client = SubprocessEngineClient::new(engine_path);
    client
        .load_config()
        .map_err(|error| DesktopError::Message(error.to_string()))
}

#[tauri::command]
fn save_engine_config(
    app: AppHandle,
    payload: EngineConfigPayload,
) -> Result<EngineConfigPayload, DesktopError> {
    let engine_path =
        resolve_engine_path(&app).map_err(|error| DesktopError::Message(error.to_string()))?;
    let client = SubprocessEngineClient::new(engine_path);
    client
        .save_config(payload)
        .map(|response| response.config)
        .map_err(|error| DesktopError::Message(error.to_string()))
}

#[tauri::command]
fn validate_engine_config(
    app: AppHandle,
    payload: Option<EngineConfigPayload>,
) -> Result<ValidateProviderResponseData, DesktopError> {
    maybe_import_legacy_swift_config(&app)
        .map_err(|error| DesktopError::Message(error.to_string()))?;
    let engine_path =
        resolve_engine_path(&app).map_err(|error| DesktopError::Message(error.to_string()))?;
    let client = SubprocessEngineClient::new(engine_path);
    client
        .validate_provider(payload)
        .map_err(|error| DesktopError::Message(error.to_string()))
}

#[tauri::command]
fn start_parse(
    app: AppHandle,
    store: tauri::State<'_, SharedStore>,
    request: StartParseRequest,
) -> Result<(), DesktopError> {
    let format = parse_format(&request.format)?;
    maybe_import_legacy_swift_config(&app)
        .map_err(|error| DesktopError::Message(error.to_string()))?;
    let engine_path =
        resolve_engine_path(&app).map_err(|error| DesktopError::Message(error.to_string()))?;
    let output_name = Path::new(&request.path)
        .file_stem()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| "document".to_string());

    let parse_request = ParseRequest {
        version: "1".into(),
        template: "General".into(),
        input: ParseInput {
            path: request.path.clone(),
            format,
        },
        options: ParseOptions {
            preserve_images: true,
            emit_structured_json: false,
            emit_svg: false,
            language_hints: Vec::new(),
            debug_request_path: None,
            debug_result_path: None,
        },
        output: Some(ParseOutputTarget {
            root_dir: None,
            name: Some(output_name),
        }),
    };

    let command_payload = serde_json::to_vec(&EngineRequest::Parse(parse_request))
        .map_err(|error| DesktopError::Message(error.to_string()))?;

    let mut command = Command::new(&engine_path);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|error| {
        DesktopError::Message(format!("failed to spawn duckdocs-engine: {error}"))
    })?;

    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| DesktopError::Message("missing engine stdin".into()))?;
    stdin.write_all(&command_payload).map_err(|error| {
        DesktopError::Message(format!("failed to send engine request: {error}"))
    })?;
    drop(stdin);

    let child_ref = Arc::new(Mutex::new(child));
    {
        let mut state = store.lock().expect("app store lock poisoned");
        if state.active_child.is_some() {
            return Err(DesktopError::Message("an import is already running".into()));
        }
        state.active_child = Some(child_ref.clone());
        state.snapshot.active_job = Some(ActiveJobSnapshot {
            job_id: next_job_id(),
            file_path: request.path.clone(),
            format: request.format.clone(),
            status: "queued".into(),
            progress_percent: 4,
            last_message: Some("Queued parse request".into()),
        });
        state.snapshot.progress_log.clear();
    }
    publish_snapshot(&app, &store);

    let app_handle = app.clone();
    let store_ref = store.inner().clone();
    thread::spawn(move || {
        if let Err(error) = run_parse_child(&app_handle, &store_ref, child_ref) {
            mark_failed(&app_handle, &store_ref, &error.to_string());
        }
    });

    Ok(())
}

#[tauri::command]
fn cancel_parse(store: tauri::State<'_, SharedStore>) -> Result<(), DesktopError> {
    let child = {
        let state = store.lock().expect("app store lock poisoned");
        state.active_child.clone()
    };

    let Some(child) = child else {
        return Ok(());
    };

    let mut child = child.lock().expect("child lock poisoned");
    child
        .kill()
        .map_err(|error| DesktopError::Message(format!("failed to cancel parse: {error}")))?;
    Ok(())
}

#[tauri::command]
fn open_saved_output(path: String, reveal: bool) -> Result<(), DesktopError> {
    let mut command = Command::new("open");
    if reveal {
        command.arg("-R");
    }
    command.arg(path);
    command
        .status()
        .map_err(|error| DesktopError::Message(format!("failed to launch open: {error}")))?;
    Ok(())
}

fn run_parse_child(
    app: &AppHandle,
    store: &SharedStore,
    child: Arc<Mutex<Child>>,
) -> anyhow::Result<()> {
    let stdout = {
        let mut child = child.lock().expect("child lock poisoned");
        child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("missing engine stdout"))?
    };
    let stderr = {
        let mut child = child.lock().expect("child lock poisoned");
        child
            .stderr
            .take()
            .ok_or_else(|| anyhow::anyhow!("missing engine stderr"))?
    };

    let stderr_app = app.clone();
    let stderr_store = store.clone();
    let stderr_thread = thread::spawn(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines().map_while(Result::ok) {
            if let Ok(event) = serde_json::from_str::<ParseEvent>(&line) {
                apply_progress_event(&stderr_app, &stderr_store, &event);
            }
        }
    });

    let mut stdout_payload = String::new();
    BufReader::new(stdout)
        .read_to_string(&mut stdout_payload)
        .map_err(|error| anyhow::anyhow!("failed to read engine stdout: {error}"))?;

    let status = {
        let mut child = child.lock().expect("child lock poisoned");
        child
            .wait()
            .map_err(|error| anyhow::anyhow!("failed waiting for engine: {error}"))?
    };
    let _ = stderr_thread.join();

    {
        let mut state = store.lock().expect("app store lock poisoned");
        state.active_child = None;
    }

    if status.success() {
        let response: EngineSuccess<ParseResponseData> = serde_json::from_str(&stdout_payload)?;
        let data = response.data;
        {
            let mut state = store.lock().expect("app store lock poisoned");
            state.snapshot.active_job = None;
            state.snapshot.last_result = Some(CompletedResultSnapshot {
                saved_output_path: data.saved_output_path.clone(),
                success_count: data.result.success_count,
                failed_count: data.result.failed_count,
                markdown: data.result.markdown,
            });
            let completion_message = data
                .saved_output_path
                .clone()
                .unwrap_or_else(|| "Parse completed without a saved output path".into());
            push_progress_entry(
                &mut state.snapshot.progress_log,
                "completed",
                &completion_message,
            );
        }
        publish_snapshot(app, store);
        Ok(())
    } else {
        if let Ok(failure) = serde_json::from_str::<EngineFailure>(&stdout_payload) {
            mark_failed(app, store, &failure.error.message);
        } else {
            mark_failed(app, store, "duckdocs-engine exited unsuccessfully");
        }
        Ok(())
    }
}

fn apply_progress_event(app: &AppHandle, store: &SharedStore, event: &ParseEvent) {
    let mut state = store.lock().expect("app store lock poisoned");
    if let Some(job) = &mut state.snapshot.active_job {
        job.status = "running".into();
        match event {
            ParseEvent::Queued => {
                job.progress_percent = 6;
                job.last_message = Some("Queued parse request".into());
                push_progress_entry(
                    &mut state.snapshot.progress_log,
                    "queued",
                    "Queued parse request",
                );
            }
            ParseEvent::DocumentOpened { format } => {
                job.progress_percent = 12;
                job.last_message = Some(format!("Opened {:?}", format));
                push_progress_entry(
                    &mut state.snapshot.progress_log,
                    "opened",
                    &format!("Opened {:?}", format),
                );
            }
            ParseEvent::ConvertingPages { current, total } => {
                job.progress_percent = scaled_progress(*current, *total, 15, 48);
                job.last_message = Some(format!("Preparing page {current} of {total}"));
                push_progress_entry(
                    &mut state.snapshot.progress_log,
                    "converting",
                    &format!("Preparing page {current} of {total}"),
                );
            }
            ParseEvent::Parsing { current, total } => {
                job.progress_percent = scaled_progress(*current, *total, 48, 88);
                job.last_message = Some(format!("Parsing page {current} of {total}"));
                push_progress_entry(
                    &mut state.snapshot.progress_log,
                    "parsing",
                    &format!("Parsing page {current} of {total}"),
                );
            }
            ParseEvent::Packaging => {
                job.progress_percent = 94;
                job.last_message = Some("Saving markdown package".into());
                push_progress_entry(
                    &mut state.snapshot.progress_log,
                    "packaging",
                    "Saving markdown package",
                );
            }
            ParseEvent::Completed => {
                job.progress_percent = 100;
                job.last_message = Some("Parse completed".into());
                push_progress_entry(
                    &mut state.snapshot.progress_log,
                    "completed",
                    "Parse completed",
                );
            }
            ParseEvent::Failed { message } => {
                job.status = "failed".into();
                job.last_message = Some(message.clone());
                push_progress_entry(&mut state.snapshot.progress_log, "failed", message);
            }
        }
    }
    drop(state);
    publish_snapshot(app, store);
}

fn mark_failed(app: &AppHandle, store: &SharedStore, message: &str) {
    {
        let mut state = store.lock().expect("app store lock poisoned");
        state.active_child = None;
        state.snapshot.active_job = None;
        push_progress_entry(&mut state.snapshot.progress_log, "failed", message);
    }
    publish_snapshot(app, store);
}

fn publish_snapshot(app: &AppHandle, store: &SharedStore) {
    let snapshot = store
        .lock()
        .expect("app store lock poisoned")
        .snapshot
        .clone();
    let _ = app.emit(SNAPSHOT_EVENT, snapshot);
}

fn push_progress_entry(log: &mut Vec<ProgressEntry>, phase: &str, message: &str) {
    let mut queue = VecDeque::from(std::mem::take(log));
    queue.push_front(ProgressEntry {
        phase: phase.into(),
        message: message.into(),
        timestamp: format_timestamp(),
    });
    while queue.len() > MAX_PROGRESS_LOG {
        queue.pop_back();
    }
    *log = queue.into();
}

fn scaled_progress(current: u32, total: u32, start: u8, end: u8) -> u8 {
    if total == 0 {
        return start;
    }
    let span = end.saturating_sub(start) as f32;
    let pct = (current as f32 / total as f32).clamp(0.0, 1.0);
    start + (span * pct).round() as u8
}

fn next_job_id() -> String {
    format!(
        "job-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    )
}

fn format_timestamp() -> String {
    format!(
        "{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    )
}

fn parse_format(value: &str) -> Result<DocumentFormat, DesktopError> {
    match value.to_ascii_lowercase().as_str() {
        "pdf" => Ok(DocumentFormat::Pdf),
        "docx" => Ok(DocumentFormat::Docx),
        "doc" => Ok(DocumentFormat::Doc),
        "image" => Ok(DocumentFormat::Image),
        _ => Err(DesktopError::Message(format!(
            "unsupported format: {value}"
        ))),
    }
}

fn detect_format(path: &Path) -> Option<&'static str> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "pdf" => Some("pdf"),
        "docx" => Some("docx"),
        "doc" => Some("doc"),
        "png" | "jpg" | "jpeg" | "webp" => Some("image"),
        _ => None,
    }
}

fn resolve_engine_path(app: &AppHandle) -> anyhow::Result<PathBuf> {
    if let Some(explicit) = std::env::var_os("DUCKDOCS_ENGINE_BIN") {
        return Ok(PathBuf::from(explicit));
    }

    let host_triple = host_triple()?;
    let sidecar_name = format!("duckdocs-engine-{host_triple}");
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let dev_path = manifest_dir.join("binaries").join(&sidecar_name);
    if dev_path.exists() {
        return Ok(dev_path);
    }

    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(macos_dir) = current_exe.parent() {
            let bundled_macos_path = macos_dir.join("duckdocs-engine");
            if bundled_macos_path.exists() {
                return Ok(bundled_macos_path);
            }
        }
    }

    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|error| anyhow::anyhow!("failed to resolve app resource dir: {error}"))?;
    let bundled_path = resource_dir.join(&sidecar_name);
    if bundled_path.exists() {
        return Ok(bundled_path);
    }

    let fallback = PathBuf::from("duckdocs-engine");
    Ok(fallback)
}

fn host_triple() -> anyhow::Result<String> {
    if let Ok(value) = std::env::var("TAURI_ENV_TARGET_TRIPLE") {
        if !value.is_empty() {
            return Ok(value);
        }
    }

    let output = Command::new("rustc").arg("-vV").output()?;
    let stdout = String::from_utf8(output.stdout)?;
    stdout
        .lines()
        .find_map(|line| line.strip_prefix("host: ").map(str::to_string))
        .ok_or_else(|| anyhow::anyhow!("failed to determine rust host triple"))
}

fn maybe_import_legacy_swift_config(app: &AppHandle) -> anyhow::Result<()> {
    if engine_config_path()?.exists() {
        return Ok(());
    }

    let Some(payload) = read_legacy_swift_payload()? else {
        return Ok(());
    };

    let client = SubprocessEngineClient::new(resolve_engine_path(app)?);
    client.save_config(payload)?;
    Ok(())
}

fn engine_config_path() -> anyhow::Result<PathBuf> {
    if let Some(explicit_dir) = std::env::var_os("DUCKDOCS_CONFIG_DIR") {
        return Ok(PathBuf::from(explicit_dir).join("engine-config.json"));
    }

    let home =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("failed to resolve user home directory"))?;
    Ok(home.join(".duckdocs/engine-config.json"))
}

fn read_legacy_swift_payload() -> anyhow::Result<Option<EngineConfigPayload>> {
    for path in legacy_preference_paths() {
        if !path.exists() {
            continue;
        }
        if let Some(payload) = legacy_payload_from_plist(&path)? {
            return Ok(Some(payload));
        }
    }
    Ok(None)
}

fn legacy_preference_paths() -> Vec<PathBuf> {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("~"));
    vec![
        home.join("Library/Preferences/app.DuckDocs.plist"),
        home.join("Library/Preferences/DuckDocs.plist"),
    ]
}

fn legacy_payload_from_plist(path: &Path) -> anyhow::Result<Option<EngineConfigPayload>> {
    let provider_blob = plutil_extract_raw(path, "ai_provider_config")?;
    let template_blob = plutil_extract_raw(path, "selected_prompt_template")?;

    let legacy_provider = provider_blob
        .as_deref()
        .map(parse_legacy_provider_blob)
        .transpose()?
        .unwrap_or_default();

    if legacy_provider.provider_type.is_none() && template_blob.is_none() {
        return Ok(None);
    }

    let provider = legacy_provider
        .provider_type
        .as_deref()
        .and_then(engine_provider_slug)
        .unwrap_or("open_router")
        .to_string();
    let prompt_template = template_blob
        .as_deref()
        .map(parse_legacy_template_blob)
        .transpose()?
        .unwrap_or_else(|| "General".to_string());
    let api_key = select_legacy_api_key(path, &provider, legacy_provider.api_key.clone())?;

    Ok(Some(EngineConfigPayload {
        provider: provider.clone(),
        model_id: legacy_provider
            .model_id
            .unwrap_or_else(|| default_model_for_provider(&provider).to_string()),
        api_key,
        base_url: legacy_provider.base_url,
        prompt_template,
        provider_options: Vec::new(),
        model_options: Vec::new(),
        prompt_template_options: Vec::new(),
    }))
}

fn select_legacy_api_key(
    plist_path: &Path,
    provider_slug: &str,
    embedded_api_key: Option<String>,
) -> anyhow::Result<String> {
    if let Some(value) = embedded_api_key.filter(|value| !value.trim().is_empty()) {
        return Ok(value);
    }

    if let Some(value) = legacy_api_key_from_defaults(plist_path, provider_slug)? {
        if !value.trim().is_empty() {
            return Ok(value);
        }
    }

    if let Some(value) = legacy_api_key_from_keychain(provider_slug)? {
        if !value.trim().is_empty() {
            return Ok(value);
        }
    }

    Ok(String::new())
}

fn legacy_api_key_from_defaults(
    plist_path: &Path,
    provider_slug: &str,
) -> anyhow::Result<Option<String>> {
    let key = match provider_slug {
        "open_router" => "openrouter_api_key",
        "open_ai" => "openai_api_key",
        "anthropic" => "anthropic_api_key",
        "ollama" => "ollama_api_key",
        _ => return Ok(None),
    };
    plutil_extract_raw(plist_path, key)
}

fn legacy_api_key_from_keychain(provider_slug: &str) -> anyhow::Result<Option<String>> {
    let service = match provider_slug {
        "open_router" => "com.duckdocs.openrouter",
        "open_ai" => "com.duckdocs.openai",
        "anthropic" => "com.duckdocs.anthropic",
        "ollama" => "com.duckdocs.ollama",
        _ => return Ok(None),
    };

    let output = Command::new("/usr/bin/security")
        .arg("find-generic-password")
        .arg("-s")
        .arg(service)
        .arg("-a")
        .arg("apikey")
        .arg("-w")
        .output()?;

    if !output.status.success() {
        return Ok(None);
    }

    let value = String::from_utf8(output.stdout)?.trim().to_string();
    if value.is_empty() {
        Ok(None)
    } else {
        Ok(Some(value))
    }
}

fn plutil_extract_raw(plist_path: &Path, key: &str) -> anyhow::Result<Option<String>> {
    let output = Command::new("/usr/bin/plutil")
        .arg("-extract")
        .arg(key)
        .arg("raw")
        .arg("-o")
        .arg("-")
        .arg(plist_path)
        .output()?;

    if !output.status.success() {
        return Ok(None);
    }

    let value = String::from_utf8(output.stdout)?.trim().to_string();
    if value.is_empty() {
        Ok(None)
    } else {
        Ok(Some(value))
    }
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyAiProviderConfig {
    provider_type: Option<String>,
    model_id: Option<String>,
    api_key: Option<String>,
    base_url: Option<String>,
}

fn parse_legacy_provider_blob(blob: &str) -> anyhow::Result<LegacyAiProviderConfig> {
    let decoded = base64::engine::general_purpose::STANDARD.decode(blob)?;
    Ok(serde_json::from_slice::<LegacyAiProviderConfig>(&decoded)?)
}

fn parse_legacy_template_blob(blob: &str) -> anyhow::Result<String> {
    let decoded = base64::engine::general_purpose::STANDARD.decode(blob)?;
    Ok(serde_json::from_slice::<String>(&decoded)?)
}

fn engine_provider_slug(value: &str) -> Option<&'static str> {
    match value {
        "OpenRouter" => Some("open_router"),
        "OpenAI" => Some("open_ai"),
        "Anthropic" => Some("anthropic"),
        "Ollama" => Some("ollama"),
        _ => None,
    }
}

fn default_model_for_provider(provider_slug: &str) -> &'static str {
    match provider_slug {
        "open_router" => "openai/gpt-4.1-mini",
        "open_ai" => "gpt-4o-mini",
        "anthropic" => "claude-3-5-sonnet-latest",
        "ollama" => "llama3.2",
        _ => "openai/gpt-4.1-mini",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        default_model_for_provider, engine_provider_slug, legacy_payload_from_plist,
        parse_legacy_provider_blob, parse_legacy_template_blob,
    };
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_temp_path(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        std::env::temp_dir().join(format!("duckdocs-{name}-{nonce}.plist"))
    }

    #[test]
    fn legacy_provider_blob_decodes_into_engine_shape() {
        let decoded = parse_legacy_provider_blob(
            "eyJwcm92aWRlclR5cGUiOiJBbnRocm9waWMiLCJhcGlLZXkiOiIiLCJtb2RlbElkIjoiY2xhdWRlLXNvbm5ldC00LTIwMjUwNTE0In0=",
        )
        .expect("legacy provider blob");
        assert_eq!(decoded.provider_type.as_deref(), Some("Anthropic"));
        assert_eq!(
            decoded.model_id.as_deref(),
            Some("claude-sonnet-4-20250514")
        );
        assert_eq!(
            engine_provider_slug(decoded.provider_type.as_deref().unwrap()),
            Some("anthropic")
        );
    }

    #[test]
    fn legacy_template_blob_decodes() {
        let decoded = parse_legacy_template_blob("IkdlbmVyYWwi").expect("template blob");
        assert_eq!(decoded, "General");
    }

    #[test]
    fn provider_defaults_cover_all_migrated_providers() {
        assert_eq!(
            default_model_for_provider("open_router"),
            "openai/gpt-4.1-mini"
        );
        assert_eq!(default_model_for_provider("open_ai"), "gpt-4o-mini");
        assert_eq!(
            default_model_for_provider("anthropic"),
            "claude-3-5-sonnet-latest"
        );
        assert_eq!(default_model_for_provider("ollama"), "llama3.2");
    }

    #[test]
    fn legacy_plist_payload_migrates_into_engine_config() {
        let plist_path = unique_temp_path("legacy-config");
        let plist = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>ai_provider_config</key>
    <string>eyJwcm92aWRlclR5cGUiOiJPcGVuUm91dGVyIiwiYXBpS2V5IjoiIiwibW9kZWxJZCI6Im9wZW5haS9ncHQtNC4xLW1pbmkiLCJiYXNlVXJsIjoiaHR0cHM6Ly9vcGVucm91dGVyLmFpL2FwaS92MSJ9</string>
    <key>selected_prompt_template</key>
    <string>IkdlbmVyYWwi</string>
    <key>openrouter_api_key</key>
    <string>legacy-router-key</string>
</dict>
</plist>
"#;
        fs::write(&plist_path, plist).expect("write plist");

        let payload = legacy_payload_from_plist(&plist_path)
            .expect("parse legacy plist")
            .expect("payload");

        fs::remove_file(&plist_path).ok();

        assert_eq!(payload.provider, "open_router");
        assert_eq!(payload.model_id, "openai/gpt-4.1-mini");
        assert_eq!(payload.prompt_template, "General");
        assert_eq!(payload.api_key, "legacy-router-key");
        assert_eq!(
            payload.base_url.as_deref(),
            Some("https://openrouter.ai/api/v1")
        );
    }
}
