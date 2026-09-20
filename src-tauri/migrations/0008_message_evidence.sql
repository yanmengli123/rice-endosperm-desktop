-- Persist the authoritative evidence metadata (citation chips / figure cards /
-- locator candidates / run warnings) observed on the SSE stream alongside the
-- final assistant message, so reopening a thread restores the evidence plane
-- from local truth instead of replaying or guessing from message text.
ALTER TABLE messages ADD COLUMN evidence_json TEXT NOT NULL DEFAULT '[]';
