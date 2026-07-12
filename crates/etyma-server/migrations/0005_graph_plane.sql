-- Graph plane product tables (S-PG4).
-- Cloud relational projection of nodes / relations / claims.
-- Physical rows are versioned; live projection = valid_to IS NULL.
-- GraphQLite is local/engine only — not used by etyma-server.

CREATE TABLE graph.nodes (
  version_id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES control.workspaces(id) ON DELETE CASCADE,
  logical_id TEXT NOT NULL,
  kind TEXT NOT NULL,
  label TEXT NOT NULL,
  scope TEXT NOT NULL DEFAULT 'workspace',
  aliases TEXT[] NOT NULL DEFAULT '{}',
  evidence_ids TEXT[] NOT NULL DEFAULT '{}',
  source_ids TEXT[] NOT NULL DEFAULT '{}',
  confidence DOUBLE PRECISION,
  created_by_event_id TEXT,
  valid_from BIGINT NOT NULL,
  valid_to BIGINT,
  superseded_by TEXT,
  updated_at BIGINT NOT NULL,
  CHECK (valid_to IS NULL OR valid_to >= valid_from),
  CHECK (
    (valid_to IS NULL AND superseded_by IS NULL)
    OR (valid_to IS NOT NULL AND superseded_by IS NOT NULL)
  )
);

-- One live version per logical node per workspace.
CREATE UNIQUE INDEX idx_graph_nodes_live
  ON graph.nodes(workspace_id, logical_id)
  WHERE valid_to IS NULL;

CREATE INDEX idx_graph_nodes_workspace_live
  ON graph.nodes(workspace_id, logical_id)
  WHERE valid_to IS NULL;

CREATE INDEX idx_graph_nodes_workspace_evidence
  ON graph.nodes USING GIN (evidence_ids)
  WHERE valid_to IS NULL;

CREATE TABLE graph.relations (
  version_id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES control.workspaces(id) ON DELETE CASCADE,
  logical_id TEXT NOT NULL,
  kind TEXT NOT NULL,
  source_logical_id TEXT NOT NULL,
  target_logical_id TEXT NOT NULL,
  label TEXT NOT NULL DEFAULT '',
  evidence_ids TEXT[] NOT NULL DEFAULT '{}',
  confidence DOUBLE PRECISION,
  created_by_event_id TEXT,
  valid_from BIGINT NOT NULL,
  valid_to BIGINT,
  superseded_by TEXT,
  updated_at BIGINT NOT NULL,
  CHECK (valid_to IS NULL OR valid_to >= valid_from),
  CHECK (
    (valid_to IS NULL AND superseded_by IS NULL)
    OR (valid_to IS NOT NULL AND superseded_by IS NOT NULL)
  )
);

CREATE UNIQUE INDEX idx_graph_relations_live
  ON graph.relations(workspace_id, logical_id)
  WHERE valid_to IS NULL;

CREATE INDEX idx_graph_relations_workspace_live
  ON graph.relations(workspace_id, logical_id)
  WHERE valid_to IS NULL;

CREATE INDEX idx_graph_relations_workspace_endpoints
  ON graph.relations(workspace_id, source_logical_id, target_logical_id)
  WHERE valid_to IS NULL;

CREATE TABLE graph.claims (
  version_id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES control.workspaces(id) ON DELETE CASCADE,
  logical_id TEXT NOT NULL,
  statement TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'open',
  topic_refs TEXT[] NOT NULL DEFAULT '{}',
  source_refs TEXT[] NOT NULL DEFAULT '{}',
  evidence_refs TEXT[] NOT NULL DEFAULT '{}',
  created_by_event_id TEXT,
  valid_from BIGINT NOT NULL,
  valid_to BIGINT,
  superseded_by TEXT,
  updated_at BIGINT NOT NULL,
  CHECK (valid_to IS NULL OR valid_to >= valid_from),
  CHECK (
    (valid_to IS NULL AND superseded_by IS NULL)
    OR (valid_to IS NOT NULL AND superseded_by IS NOT NULL)
  )
);

CREATE UNIQUE INDEX idx_graph_claims_live
  ON graph.claims(workspace_id, logical_id)
  WHERE valid_to IS NULL;

CREATE INDEX idx_graph_claims_workspace_live
  ON graph.claims(workspace_id, logical_id)
  WHERE valid_to IS NULL;
