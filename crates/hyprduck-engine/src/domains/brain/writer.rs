use crate::*;

pub(crate) struct BrainWorkspaceWriter {
    repo: BrainArtifactRepository,
    _lock: WorkspaceLock,
}

impl BrainWorkspaceWriter {
    pub(crate) fn open(root: PathBuf) -> Result<Self> {
        let (repo, lock) = BrainArtifactRepository::open(root)?;
        Ok(Self { repo, _lock: lock })
    }

    pub(crate) fn root(&self) -> &Path {
        self.repo.root()
    }

    pub(crate) fn append_event(&self, event: &BrainEvent) -> Result<()> {
        self.repo.append_event(event)
    }

    #[cfg(test)]
    pub(crate) fn upsert_memory_record(&self, memory: MemoryRecord) -> Result<()> {
        let mut memories = self.repo.read_memory_records()?;
        if let Some(existing) = memories
            .iter_mut()
            .find(|record| record.memory_id == memory.memory_id)
        {
            *existing = memory;
        } else {
            memories.push(memory);
        }
        memories.sort_by(|left, right| {
            right
                .updated_at
                .cmp(&left.updated_at)
                .then_with(|| left.memory_id.cmp(&right.memory_id))
        });
        self.repo.write_memory_records(&memories)
    }
}
