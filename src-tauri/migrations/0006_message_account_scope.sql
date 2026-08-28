-- v0.3.5 历史丢失 bug 的根治迁移：给 messages 加 account_scope 列并按账号
-- 隔离，把任何历史（无该列、值固定为当前账号）的 rows 反推自对应 thread。

ALTER TABLE messages ADD COLUMN account_scope TEXT NOT NULL DEFAULT 'legacy';

CREATE INDEX IF NOT EXISTS idx_messages_account_thread
ON messages(account_scope, thread_id, position);

UPDATE messages
SET account_scope = (
    SELECT t.account_scope
    FROM threads t
    WHERE t.id = messages.thread_id
)
WHERE EXISTS (
    SELECT 1 FROM threads t WHERE t.id = messages.thread_id
);
