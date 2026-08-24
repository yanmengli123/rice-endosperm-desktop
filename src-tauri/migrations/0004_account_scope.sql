ALTER TABLE threads ADD COLUMN account_scope TEXT NOT NULL DEFAULT 'legacy';

CREATE INDEX IF NOT EXISTS idx_threads_account_updated
ON threads(account_scope, updated_at DESC);
