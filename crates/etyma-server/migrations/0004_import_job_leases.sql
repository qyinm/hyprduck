-- Fence reclaimed import jobs with a per-claim token and align claim indexes
-- with workspace-scoped queued/expired-running predicates.

ALTER TABLE knowledge.import_jobs
  ADD COLUMN lease_token TEXT;

-- Pre-token running jobs cannot be completed safely. Return them to the queue;
-- attempts remain durable and the next claim receives a fresh token.
UPDATE knowledge.import_jobs
SET status = CASE
      WHEN attempts >= max_attempts THEN 'failed'
      ELSE 'queued'
    END,
    last_error = 'lease reset during fencing-token migration',
    lease_owner = NULL,
    lease_expires_at = NULL,
    lease_token = NULL,
    available_at = LEAST(
      available_at,
      EXTRACT(EPOCH FROM clock_timestamp())::BIGINT
    ),
    updated_at = EXTRACT(EPOCH FROM clock_timestamp())::BIGINT
WHERE status = 'running';

ALTER TABLE knowledge.import_jobs
  DROP CONSTRAINT import_jobs_check,
  ADD CONSTRAINT import_jobs_lease_check CHECK (
    (
      status = 'running'
      AND lease_owner IS NOT NULL
      AND lease_expires_at IS NOT NULL
      AND lease_token IS NOT NULL
    )
    OR
    (
      status <> 'running'
      AND lease_owner IS NULL
      AND lease_expires_at IS NULL
      AND lease_token IS NULL
    )
  );

DROP INDEX knowledge.idx_knowledge_import_jobs_claimable;

CREATE INDEX idx_knowledge_import_jobs_queued_claimable
  ON knowledge.import_jobs(workspace_id, available_at, created_at, id)
  WHERE status = 'queued';

CREATE INDEX idx_knowledge_import_jobs_running_expired
  ON knowledge.import_jobs(workspace_id, lease_expires_at, created_at, id)
  WHERE status = 'running';
