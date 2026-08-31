CREATE TABLE IF NOT EXISTS workflow_agent_turns (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL UNIQUE,
    project_id TEXT NOT NULL,
    engine_turn_id TEXT,
    engine_session_id TEXT,
    provider TEXT NOT NULL,
    model TEXT NOT NULL,
    prompt TEXT NOT NULL,
    response TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL CHECK (status IN ('running', 'completed', 'failed', 'cancelled', 'interrupted')),
    error TEXT,
    input_tokens INTEGER NOT NULL DEFAULT 0,
    output_tokens INTEGER NOT NULL DEFAULT 0,
    reasoning_tokens INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    finished_at TEXT,
    FOREIGN KEY (run_id) REFERENCES workflow_runs(id) ON DELETE CASCADE,
    FOREIGN KEY (project_id) REFERENCES workflow_projects(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_workflow_agent_turns_project_created
ON workflow_agent_turns(project_id, created_at DESC);

CREATE TABLE IF NOT EXISTS workflow_bridge_events (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    artifact_id TEXT NOT NULL,
    direction TEXT NOT NULL CHECK (direction IN ('workflow_to_qa')),
    status TEXT NOT NULL CHECK (status IN ('initiated', 'completed', 'failed')),
    remote_ref TEXT,
    error TEXT,
    created_at TEXT NOT NULL,
    finished_at TEXT,
    FOREIGN KEY (project_id) REFERENCES workflow_projects(id) ON DELETE CASCADE,
    FOREIGN KEY (artifact_id) REFERENCES workflow_artifacts(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_workflow_bridge_events_project_created
ON workflow_bridge_events(project_id, created_at DESC);
