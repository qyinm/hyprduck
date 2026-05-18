use crate::*;
use std::fs::OpenOptions;

pub(crate) struct BrainArtifactRepository {
    root: PathBuf,
}

impl BrainArtifactRepository {
    pub(crate) fn open(root: PathBuf) -> Result<(Self, WorkspaceLock)> {
        fs::create_dir_all(&root).with_context(|| format!("failed creating {}", root.display()))?;
        let lock = WorkspaceLock::acquire(root.join(BRAIN_LOCK_DIRECTORY_NAME))?;
        let repo = Self { root };
        repo.ensure_workspace_dirs()?;
        Ok((repo, lock))
    }

    pub(crate) fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn ensure_workspace_dirs(&self) -> Result<()> {
        for dir in [self.root.join("events"), self.root.join("memory")] {
            fs::create_dir_all(&dir)
                .with_context(|| format!("failed creating {}", dir.display()))?;
        }
        Ok(())
    }

    pub(crate) fn append_event(&self, event: &BrainEvent) -> Result<()> {
        append_brain_event_jsonl(&self.root.join("events/brain_events.jsonl"), event)
    }

    pub(crate) fn read_memory_records(&self) -> Result<Vec<MemoryRecord>> {
        read_memory_records(&self.root)
    }

    #[cfg(test)]
    pub(crate) fn write_memory_records(&self, memories: &[MemoryRecord]) -> Result<()> {
        write_json_pretty(&self.root.join("memory/records.json"), &memories)
    }

    pub(crate) fn brain_manifest_path(&self) -> PathBuf {
        self.root.join("brain-manifest.json")
    }

    pub(crate) fn read_brain_manifest(&self) -> Result<BrainRepoSnapshot> {
        read_json_artifact(&self.brain_manifest_path())
    }

    pub(crate) fn read_brain_events(&self) -> Result<Vec<BrainEvent>> {
        read_brain_events_jsonl(&self.root.join("events/brain_events.jsonl"))
    }
}

pub(crate) struct WorkspaceLock {
    path: PathBuf,
}

impl WorkspaceLock {
    fn acquire(path: PathBuf) -> Result<Self> {
        let started = Instant::now();
        loop {
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    if started.elapsed() > Duration::from_secs(5) {
                        bail!(
                            "timed out waiting for workspace brain lock {}",
                            path.display()
                        );
                    }
                    std::thread::sleep(Duration::from_millis(25));
                }
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("failed acquiring {}", path.display()));
                }
            }
        }
    }
}

impl Drop for WorkspaceLock {
    fn drop(&mut self) {
        let _ = fs::remove_dir(&self.path);
    }
}

pub(crate) fn read_json_artifact<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let json =
        fs::read_to_string(path).with_context(|| format!("failed reading {}", path.display()))?;
    serde_json::from_str(&json).with_context(|| format!("failed decoding {}", path.display()))
}

pub(crate) fn read_optional_json_artifact<T: serde::de::DeserializeOwned + Default>(
    path: &Path,
) -> Result<T> {
    if !path.exists() {
        return Ok(T::default());
    }
    read_json_artifact(path)
}

pub(crate) fn read_memory_records(root: &Path) -> Result<Vec<MemoryRecord>> {
    read_optional_json_artifact(&root.join("memory/records.json"))
}

pub(crate) fn read_brain_events_jsonl(path: &Path) -> Result<Vec<BrainEvent>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let contents =
        fs::read_to_string(path).with_context(|| format!("failed reading {}", path.display()))?;
    contents
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).context("failed decoding brain event JSONL row"))
        .collect()
}

pub(crate) fn write_json_pretty<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let json = serde_json::to_string_pretty(value).context("failed to encode JSON artifact")?;
    write_file_atomic(path, json.as_bytes())
}

pub(crate) fn write_brain_events_jsonl(path: &Path, events: &[BrainEvent]) -> Result<()> {
    let mut lines = String::new();
    for event in events {
        lines.push_str(
            &serde_json::to_string(event).context("failed to encode brain event JSONL row")?,
        );
        lines.push('\n');
    }
    write_file_atomic(path, lines.as_bytes())
}

pub(crate) fn write_file_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed creating {}", parent.display()))?;
    }
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "artifact".into());
    let temp_path = path.with_file_name(format!(".{file_name}.{}.tmp", Uuid::now_v7().as_simple()));
    {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
            .with_context(|| format!("failed opening {}", temp_path.display()))?;
        file.write_all(bytes)
            .with_context(|| format!("failed writing {}", temp_path.display()))?;
        file.sync_all()
            .with_context(|| format!("failed syncing {}", temp_path.display()))?;
    }
    fs::rename(&temp_path, path).with_context(|| {
        format!(
            "failed renaming {} to {}",
            temp_path.display(),
            path.display()
        )
    })
}

pub(crate) fn append_brain_event_jsonl(path: &Path, event: &BrainEvent) -> Result<()> {
    let line = serde_json::to_string(event).context("failed to encode brain event JSONL row")?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed creating {}", parent.display()))?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("failed opening {}", path.display()))?;
    file.write_all(line.as_bytes())
        .with_context(|| format!("failed writing {}", path.display()))?;
    file.write_all(b"\n")
        .with_context(|| format!("failed writing {}", path.display()))?;
    file.sync_all()
        .with_context(|| format!("failed syncing {}", path.display()))
}
