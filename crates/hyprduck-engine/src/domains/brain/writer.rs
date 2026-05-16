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

    pub(crate) fn write_proposal(&self, proposal: &BrainUpdateProposal) -> Result<PathBuf> {
        self.repo.write_proposal(proposal)
    }

    pub(crate) fn proposal_path(&self, proposal_id: &str) -> PathBuf {
        self.repo.proposal_path(proposal_id)
    }

    pub(crate) fn append_event(&self, event: &BrainEvent) -> Result<()> {
        self.repo.append_event(event)
    }

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

    pub(crate) fn apply_source_note_metadata(
        &self,
        proposal: &BrainUpdateProposal,
        request: &ProposeBrainUpdateRequest,
    ) -> Result<()> {
        let source_id = proposal
            .target_source_id
            .as_ref()
            .or_else(|| proposal.source_refs.first())
            .context("source note proposal needs a source id")?;
        let manifest_path = self
            .repo
            .root()
            .join("artifacts")
            .join(source_id)
            .join("source-manifest.json");
        let mut manifest: SourceArtifactManifest = read_json_artifact(&manifest_path)?;
        merge_source_metadata(
            &mut manifest.description,
            request
                .source_description
                .as_deref()
                .unwrap_or(&proposal.body),
        );
        merge_source_metadata(
            &mut manifest.user_context,
            request.source_user_context.as_deref().unwrap_or(""),
        );
        merge_source_metadata(
            &mut manifest.ingest_instruction,
            request.source_ingest_instruction.as_deref().unwrap_or(""),
        );
        manifest.updated_at = unix_timestamp_seconds();
        write_source_manifest(&manifest)?;
        if let Some(output_root) = self.repo.output_root() {
            let store = KnowledgeProjectStore::new(output_root.join("knowledge.sqlite3"));
            store.update_source_manifest_snapshot(&manifest)?;
        }
        self.update_brain_manifest_source(&manifest)?;
        Ok(())
    }

    pub(crate) fn apply_accepted_proposal(&self, proposal: &BrainUpdateProposal) -> Result<()> {
        if proposal.status != BrainProposalStatus::Accepted {
            return Ok(());
        }
        let manifest_path = self.repo.brain_manifest_path();
        if !manifest_path.exists() {
            return Ok(());
        }
        if matches!(
            proposal.kind,
            BrainProposalKind::Memory
                | BrainProposalKind::Observation
                | BrainProposalKind::SourceNote
        ) {
            return Ok(());
        }
        let mut snapshot =
            read_materialized_brain_snapshot(self.repo.root(), &proposal.workspace_id)
                .or_else(|_| self.repo.read_brain_manifest())?;
        reduce_accepted_proposals_into_snapshot(
            self.repo.root(),
            &mut snapshot,
            vec![proposal.clone()],
        )?;
        refresh_materialized_wiki_pages(&mut snapshot);
        refresh_current_materialized_events(&mut snapshot)?;
        persist_effective_brain_snapshot(self.repo.root(), &snapshot)?;
        Ok(())
    }

    fn update_brain_manifest_source(&self, manifest: &SourceArtifactManifest) -> Result<()> {
        let path = self.repo.brain_manifest_path();
        if !path.exists() {
            return Ok(());
        }
        let mut snapshot = self.repo.read_brain_manifest()?;
        if let Some(source) = snapshot
            .sources
            .iter_mut()
            .find(|source| source.source_id == manifest.source_id)
        {
            source.description = manifest.description.clone();
            source.user_context = manifest.user_context.clone();
            source.ingest_instruction = manifest.ingest_instruction.clone();
            source.updated_at = manifest.updated_at;
            self.repo.write_brain_manifest(&snapshot)?;
        }
        Ok(())
    }
}

fn merge_source_metadata(target: &mut String, value: &str) {
    let value = value.trim();
    if !value.is_empty() {
        *target = value.to_string();
    }
}
