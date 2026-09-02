use serde::{Deserialize, Serialize};

pub const RICE_WORKFLOW_PROTOCOL: &str = "rice.workflow.v1";

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowProject {
    pub id: String,
    pub name: String,
    pub root: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowRun {
    pub id: String,
    pub project_id: String,
    pub workflow_kind: String,
    pub status: String,
    pub input_path: Option<String>,
    pub manifest_path: Option<String>,
    pub summary_json: String,
    pub error: Option<String>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowArtifact {
    pub id: String,
    pub run_id: String,
    pub project_id: String,
    pub name: String,
    pub relative_path: String,
    pub media_type: String,
    pub size_bytes: i64,
    pub sha256: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowAgentTurn {
    pub id: String,
    pub run_id: String,
    pub project_id: String,
    pub engine_turn_id: Option<String>,
    pub engine_session_id: Option<String>,
    pub provider: String,
    pub model: String,
    pub prompt: String,
    pub response: String,
    pub status: String,
    pub error: Option<String>,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_tokens: i64,
    pub created_at: String,
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowEngineStatus {
    pub protocol: String,
    pub available: bool,
    pub running_projects: usize,
    pub worker_path: Option<String>,
    pub worker_version: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowModelSettings {
    pub provider: String,
    pub base_url: String,
    pub model: String,
    #[serde(default)]
    pub has_api_key: bool,
    #[serde(default)]
    pub api_key_hint: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveWorkflowModelSettings {
    pub provider: String,
    pub base_url: String,
    pub model: String,
    pub api_key: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkflowAgentEvent {
    EngineReady {
        protocol: String,
        model: String,
        root: String,
    },
    TurnStarted {
        turn_id: String,
    },
    Progress {
        phase: String,
        message: String,
        elapsed_ms: u64,
    },
    TextDelta {
        delta: String,
    },
    ReasoningActive,
    ToolStarted {
        call_id: Option<String>,
        name: String,
        preview: String,
    },
    ToolFinished {
        call_id: Option<String>,
        name: String,
        ok: bool,
        content: String,
        duration_ms: u64,
    },
    ApprovalRequired {
        approval_id: String,
        message: String,
    },
    FileChanged {
        path: String,
    },
    Usage {
        input_tokens: u64,
        output_tokens: u64,
        reasoning_tokens: u64,
    },
    TurnCompleted {
        ok: bool,
        error: Option<String>,
    },
    EngineError {
        message: String,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowAgentRequest {
    pub project_id: String,
    pub prompt: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowAgentCompletion {
    pub turn_id: String,
    pub text: String,
    pub session_id: Option<String>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_tokens: u64,
    pub changed_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkflowEvent {
    RunStarted {
        run_id: String,
        message: String,
    },
    Progress {
        run_id: String,
        percent: u8,
        message: String,
    },
    ArtifactCreated {
        run_id: String,
        artifact: WorkflowArtifact,
    },
    RunCompleted {
        run: WorkflowRun,
    },
    RunFailed {
        run_id: String,
        message: String,
    },
    RunCancelled {
        run_id: String,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CountsPcaRequest {
    pub project_id: String,
    pub input_relative_path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PcaSummary {
    pub sample_count: usize,
    pub feature_count: usize,
    pub variable_feature_count: usize,
    pub pc1_explained_percent: f64,
    pub pc2_explained_percent: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WispRpcEnvelope {
    pub schema: String,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub command_id: Option<String>,
    #[serde(rename = "type")]
    pub event_type: String,
    #[serde(default)]
    pub sequence: Option<u64>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(flatten)]
    pub payload: serde_json::Map<String, serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::WorkflowAgentEvent;

    #[test]
    fn workflow_progress_event_matches_the_frontend_wire_contract() {
        let value = serde_json::to_value(WorkflowAgentEvent::Progress {
            phase: "waiting_model".into(),
            message: "正在等待模型返回首个结果…".into(),
            elapsed_ms: 5_000,
        })
        .expect("serialize progress event");

        assert_eq!(value["type"], "progress");
        assert_eq!(value["phase"], "waiting_model");
        assert_eq!(value["message"], "正在等待模型返回首个结果…");
        assert_eq!(value["elapsed_ms"], 5_000);
    }
}
