ALTER TABLE runs ADD COLUMN server_context TEXT;

CREATE INDEX IF NOT EXISTS idx_runs_status_updated
ON runs(status, updated_at);
