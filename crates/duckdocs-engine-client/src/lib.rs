use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use duckdocs_engine_types::{ParseEvent, ParseProgress, ParseRequest, ParseResult};

pub trait EngineClient {
    fn parse(
        &self,
        request: ParseRequest,
        on_progress: &mut dyn FnMut(ParseProgress),
    ) -> Result<ParseResult>;
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
                .unwrap_or_else(|_| EngineLaunchSpec::binary(PathBuf::from("duckdocs-engine"))),
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
}

impl EngineClient for SubprocessEngineClient {
    fn parse(
        &self,
        request: ParseRequest,
        on_progress: &mut dyn FnMut(ParseProgress),
    ) -> Result<ParseResult> {
        let mut command = Command::new(&self.launch_spec.program);
        command
            .args(&self.launch_spec.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if let Some(current_dir) = &self.launch_spec.current_dir {
            command.current_dir(current_dir);
        }

        let mut child = command
            .spawn()
            .with_context(|| format!("failed to spawn engine runtime {}", self.launch_spec.display))?;

        let mut stdin = child.stdin.take().context("missing child stdin")?;
        let stdout = child.stdout.take().context("missing child stdout")?;
        let stderr = child.stderr.take().context("missing child stderr")?;

        let payload = serde_json::to_vec(&request).context("failed to encode parse request")?;
        stdin.write_all(&payload).context("failed to write parse request to engine stdin")?;
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

        loop {
            while let Ok(line) = rx.try_recv() {
                stderr_lines.push(line.clone());
                if let Ok(event) = serde_json::from_str::<ParseEvent>(&line) {
                    on_progress(event.into());
                }
            }

            if let Some(status) = child.try_wait().context("failed waiting on engine process")? {
                stdout_reader
                    .read_to_string(&mut stdout_payload)
                    .context("failed reading engine stdout")?;
                while let Ok(line) = rx.try_recv() {
                    stderr_lines.push(line.clone());
                    if let Ok(event) = serde_json::from_str::<ParseEvent>(&line) {
                        on_progress(event.into());
                    }
                }

                if !status.success() {
                    let last_stderr = stderr_lines
                        .iter()
                        .rev()
                        .find(|line| !line.trim().is_empty())
                        .cloned()
                        .unwrap_or_else(|| "no stderr output".to_string());
                    return Err(anyhow!("engine exited with status {status}: {last_stderr}"));
                }

                let result: ParseResult =
                    serde_json::from_str(&stdout_payload).context("failed decoding engine parse result")?;
                return Ok(result);
            }

            thread::sleep(Duration::from_millis(20));
        }
    }
}

pub fn resolve_engine_bin() -> Result<PathBuf> {
    if let Some(explicit) = std::env::var_os("DUCKDOCS_ENGINE_BIN") {
        return Ok(PathBuf::from(explicit));
    }

    if let Some(explicit) = std::env::var_os("CARGO_BIN_EXE_duckdocs-engine") {
        return Ok(PathBuf::from(explicit));
    }

    let current_exe = std::env::current_exe().context("failed to locate current executable")?;
    for root in candidate_roots(&current_exe, std::env::current_dir().ok(), std::env::var_os("CARGO_MANIFEST_DIR").map(PathBuf::from)) {
        let candidate = root.join(engine_binary_name());
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    Err(anyhow!("unable to resolve duckdocs-engine binary"))
}

fn engine_binary_name() -> OsString {
    if cfg!(target_os = "windows") {
        OsString::from("duckdocs-engine.exe")
    } else {
        OsString::from("duckdocs-engine")
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
    .context("unable to resolve duckdocs-engine runtime")?;

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
            OsString::from("duckdocs-engine"),
            OsString::from("--"),
        ];
        Self {
            program: PathBuf::from("cargo"),
            args,
            current_dir: Some(workspace_root),
            display: "cargo run --quiet -p duckdocs-engine --".to_string(),
        }
    }

    pub fn display(&self) -> &str {
        &self.display
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
    for candidate in start.ancestors() {
        let cargo_toml = candidate.join("Cargo.toml");
        let engine_manifest = candidate.join("crates/duckdocs-engine/Cargo.toml");
        if cargo_toml.exists() && engine_manifest.exists() {
            return Some(candidate.to_path_buf());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{candidate_roots, detect_workspace_root, engine_binary_name};
    use std::path::PathBuf;
    use tempfile::tempdir;

    #[test]
    fn candidate_roots_include_workspace_targets() {
        let workspace = tempdir().unwrap();
        std::fs::create_dir_all(workspace.path().join("crates/duckdocs-engine")).unwrap();
        std::fs::write(workspace.path().join("Cargo.toml"), "[workspace]\n").unwrap();
        std::fs::write(
            workspace.path().join("crates/duckdocs-engine/Cargo.toml"),
            "[package]\nname=\"duckdocs-engine\"\nversion=\"0.1.0\"\n",
        )
        .unwrap();

        let roots = candidate_roots(
            &workspace.path().join("target/debug/duckdocs-cli"),
            Some(workspace.path().join("crates/duckdocs-cli")),
            Some(workspace.path().join("crates/duckdocs-engine-client")),
        );

        assert!(roots.contains(&workspace.path().join("target/debug")));
        assert!(roots.contains(&workspace.path().join("target/release")));
        assert!(roots.iter().any(|root| root.ends_with(PathBuf::from("target/debug"))));
        assert!(!engine_binary_name().is_empty());
    }

    #[test]
    fn detect_workspace_root_walks_up_from_crate_directory() {
        let workspace = tempdir().unwrap();
        std::fs::create_dir_all(workspace.path().join("crates/duckdocs-engine")).unwrap();
        std::fs::create_dir_all(workspace.path().join("crates/duckdocs-cli/src")).unwrap();
        std::fs::write(workspace.path().join("Cargo.toml"), "[workspace]\n").unwrap();
        std::fs::write(
            workspace.path().join("crates/duckdocs-engine/Cargo.toml"),
            "[package]\nname=\"duckdocs-engine\"\nversion=\"0.1.0\"\n",
        )
        .unwrap();

        let resolved = detect_workspace_root(
            Some(workspace.path().join("crates/duckdocs-cli/src")),
            None,
            None,
        );

        assert_eq!(resolved, Some(workspace.path().to_path_buf()));
    }
}
