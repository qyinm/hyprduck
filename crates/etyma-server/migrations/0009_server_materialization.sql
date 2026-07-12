-- Server-side materialization metadata for PON-11.
-- Existing source bytes remain blob-backed; evidence rows carry citation and retrieval metadata.

ALTER TABLE knowledge.evidence
  ADD COLUMN evidence_type TEXT NOT NULL DEFAULT 'text_evidence',
  ADD COLUMN page INTEGER,
  ADD COLUMN region TEXT,
  ADD COLUMN span_start BIGINT,
  ADD COLUMN span_end BIGINT,
  ADD COLUMN parse_warnings TEXT[] NOT NULL DEFAULT '{}',
  ADD COLUMN retrieval_text TEXT;

CREATE INDEX idx_knowledge_evidence_workspace_retrieval
  ON knowledge.evidence USING GIN (to_tsvector('simple', coalesce(retrieval_text, quote)));

