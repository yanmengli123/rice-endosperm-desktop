CREATE TABLE IF NOT EXISTS accounts (
    account_scope TEXT PRIMARY KEY,
    display_name TEXT NOT NULL DEFAULT '',
    gateway_url TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_accounts_created ON accounts(created_at);
