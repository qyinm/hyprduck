-- Knowledge plane product tables (S-PG3).
-- Original source bytes remain in blob storage; these rows hold metadata only.

CREATE TABLE knowledge.sources (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES control.workspaces(id) ON DELETE CASCADE,
  kind TEXT NOT NULL,
  title TEXT NOT NULL,
  blob_key TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  byte_size BIGINT NOT NULL CHECK (byte_size >= 0),
  content_type TEXT NOT NULL,
  external_id TEXT,
  created_at BIGINT NOT NULL,
  updated_at BIGINT NOT NULL,
  UNIQUE (workspace_id, id)
);

CREATE UNIQUE INDEX idx_knowledge_sources_workspace_external_id
  ON knowledge.sources(workspace_id, external_id)
  WHERE external_id IS NOT NULL;

CREATE INDEX idx_knowledge_sources_workspace_created
  ON knowledge.sources(workspace_id, created_at, id);

CREATE TABLE knowledge.evidence (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES control.workspaces(id) ON DELETE CASCADE,
  source_id TEXT NOT NULL,
  source_kind TEXT NOT NULL,
  quote TEXT NOT NULL,
  locator TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  created_at BIGINT NOT NULL,
  updated_at BIGINT NOT NULL,
  UNIQUE (workspace_id, id),
  FOREIGN KEY (workspace_id, source_id)
    REFERENCES knowledge.sources(workspace_id, id) ON DELETE CASCADE
);

CREATE INDEX idx_knowledge_evidence_workspace_created
  ON knowledge.evidence(workspace_id, created_at, id);

CREATE INDEX idx_knowledge_evidence_workspace_source_created
  ON knowledge.evidence(workspace_id, source_id, created_at, id);

CREATE TABLE knowledge.import_jobs (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES control.workspaces(id) ON DELETE CASCADE,
  source_id TEXT,
  kind TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('queued', 'running', 'succeeded', 'failed')),
  attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
  max_attempts INTEGER NOT NULL DEFAULT 3 CHECK (max_attempts > 0),
  last_error TEXT CHECK (char_length(last_error) <= 4096),
  available_at BIGINT NOT NULL,
  lease_owner TEXT,
  lease_expires_at BIGINT,
  created_at BIGINT NOT NULL,
  updated_at BIGINT NOT NULL,
  FOREIGN KEY (workspace_id, source_id)
    REFERENCES knowledge.sources(workspace_id, id) ON DELETE RESTRICT,
  CHECK (
    (status = 'running' AND lease_owner IS NOT NULL AND lease_expires_at IS NOT NULL)
    OR
    (status <> 'running' AND lease_owner IS NULL AND lease_expires_at IS NULL)
  )
);

CREATE INDEX idx_knowledge_import_jobs_workspace_created
  ON knowledge.import_jobs(workspace_id, created_at, id);

CREATE INDEX idx_knowledge_import_jobs_workspace_source
  ON knowledge.import_jobs(workspace_id, source_id)
  WHERE source_id IS NOT NULL;

CREATE INDEX idx_knowledge_import_jobs_claimable
  ON knowledge.import_jobs(available_at, created_at, id)
  WHERE status IN ('queued', 'running');
