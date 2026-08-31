use std::path::{Path, PathBuf};

use chrono::Utc;
use secrecy::ExposeSecret;
use tauri::{AppHandle, State, ipc::Channel};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;
use url::Url;
use uuid::Uuid;

use crate::{
    commands::ensure_active_bearer,
    error::{AppError, CommandError},
    state::AppState,
    yuxi::PendingChatAttachment,
};

use super::{
    CountsPcaRequest, SaveWorkflowModelSettings, WorkflowAgentCompletion, WorkflowAgentEvent,
    WorkflowAgentRequest, WorkflowAgentTurn, WorkflowArtifact, WorkflowEngineStatus, WorkflowEvent,
    WorkflowModelSettings, WorkflowProject, WorkflowRun,
    artifacts::{media_type, register_agent_outputs, sha256_file},
    pca::execute_counts_pca,
    project::{initialize_project, resolve_project_relative},
};

fn command_error(error: AppError) -> CommandError {
    CommandError::from(error)
}

const MODEL_SETTINGS_KEY: &str = "model_settings_v1";

fn validate_model_settings(
    settings: SaveWorkflowModelSettings,
) -> Result<(WorkflowModelSettings, Option<String>), CommandError> {
    let provider = settings.provider.trim().to_ascii_lowercase();
    if !matches!(
        provider.as_str(),
        "openai" | "openai_responses" | "anthropic"
    ) {
        return Err(command_error(AppError::Protocol(
            "工作流模型协议仅支持 openai、openai_responses 或 anthropic".into(),
        )));
    }
    let base_url = settings.base_url.trim().trim_end_matches('/');
    let parsed = Url::parse(base_url)
        .map_err(|_| command_error(AppError::Protocol("工作流模型 Base URL 无效".into())))?;
    let host = parsed
        .host_str()
        .unwrap_or_default()
        .trim_start_matches('[')
        .trim_end_matches(']');
    let local_http = parsed.scheme() == "http" && matches!(host, "127.0.0.1" | "localhost" | "::1");
    if parsed.username() != ""
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || (parsed.scheme() != "https" && !local_http)
    {
        return Err(command_error(AppError::Protocol(
            "远程模型地址必须使用 HTTPS，且不能包含凭据、查询参数或片段".into(),
        )));
    }
    let model = settings.model.trim();
    if model.is_empty() || model.len() > 256 || model.chars().any(char::is_control) {
        return Err(command_error(AppError::Protocol(
            "工作流模型 ID 无效".into(),
        )));
    }
    let api_key = settings.api_key.trim().to_owned();
    if api_key.len() > 4096 || api_key.chars().any(char::is_control) {
        return Err(command_error(AppError::Protocol(
            "工作流模型 API Key 无效".into(),
        )));
    }
    let replacement = (!api_key.is_empty()).then_some(api_key);
    let hint = replacement
        .as_ref()
        .map(|key| format!("{}••••", key.chars().take(6).collect::<String>()));
    Ok((
        WorkflowModelSettings {
            provider,
            base_url: base_url.to_owned(),
            model: model.to_owned(),
            has_api_key: replacement.is_some(),
            api_key_hint: hint,
        },
        replacement,
    ))
}

async fn load_model_settings(state: &AppState) -> Result<WorkflowModelSettings, CommandError> {
    let raw = state
        .workflow
        .store
        .setting(MODEL_SETTINGS_KEY)
        .await
        .map_err(command_error)?
        .ok_or_else(|| command_error(AppError::MissingCredential))?;
    let mut settings = serde_json::from_str::<WorkflowModelSettings>(&raw)
        .map_err(|_| command_error(AppError::Protocol("工作流模型配置已损坏".into())))?;
    settings.has_api_key = state
        .credentials
        .workflow_model_api_key()
        .map_err(command_error)?
        .is_some();
    Ok(settings)
}

#[tauri::command]
pub async fn get_workflow_model_settings(
    state: State<'_, AppState>,
) -> Result<Option<WorkflowModelSettings>, CommandError> {
    let Some(raw) = state
        .workflow
        .store
        .setting(MODEL_SETTINGS_KEY)
        .await
        .map_err(command_error)?
    else {
        return Ok(None);
    };
    let mut settings = serde_json::from_str::<WorkflowModelSettings>(&raw)
        .map_err(|_| command_error(AppError::Protocol("工作流模型配置已损坏".into())))?;
    settings.has_api_key = state
        .credentials
        .workflow_model_api_key()
        .map_err(command_error)?
        .is_some();
    Ok(Some(settings))
}

#[tauri::command]
pub async fn save_workflow_model_settings(
    settings: SaveWorkflowModelSettings,
    state: State<'_, AppState>,
) -> Result<WorkflowModelSettings, CommandError> {
    let (mut public, replacement) = validate_model_settings(settings)?;
    let previous = state
        .credentials
        .workflow_model_api_key()
        .map_err(command_error)?;
    if replacement.is_none() && previous.is_none() {
        return Err(command_error(AppError::MissingCredential));
    }
    if let Some(api_key) = replacement.as_deref() {
        state
            .credentials
            .save_workflow_model_api_key(api_key)
            .map_err(command_error)?;
    }
    let effective_key = replacement
        .as_deref()
        .or_else(|| previous.as_ref().map(|secret| secret.expose_secret()))
        .ok_or_else(|| command_error(AppError::MissingCredential))?;
    public.has_api_key = true;
    public.api_key_hint = Some(format!(
        "{}••••",
        effective_key.chars().take(6).collect::<String>()
    ));
    let encoded = serde_json::to_string(&public)
        .map_err(|error| command_error(AppError::Internal(error.to_string())))?;
    if let Err(error) = state
        .workflow
        .store
        .save_setting(MODEL_SETTINGS_KEY, &encoded)
        .await
    {
        if replacement.is_some() {
            let rollback = match previous {
                Some(previous) => state
                    .credentials
                    .save_workflow_model_api_key(previous.expose_secret()),
                None => state.credentials.delete_workflow_model_api_key(),
            };
            if let Err(rollback_error) = rollback {
                crate::diagnostics::log(
                    "ERROR",
                    "workflow_credential_rollback_failed",
                    &rollback_error.to_string(),
                );
            }
        }
        return Err(command_error(error));
    }
    Ok(public)
}

#[tauri::command]
pub async fn delete_workflow_model_settings(
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    let previous = state
        .credentials
        .workflow_model_api_key()
        .map_err(command_error)?;
    state
        .credentials
        .delete_workflow_model_api_key()
        .map_err(command_error)?;
    if let Err(error) = state
        .workflow
        .store
        .delete_setting(MODEL_SETTINGS_KEY)
        .await
    {
        if let Some(previous) = previous
            && let Err(rollback_error) = state
                .credentials
                .save_workflow_model_api_key(previous.expose_secret())
        {
            crate::diagnostics::log(
                "ERROR",
                "workflow_credential_delete_rollback_failed",
                &rollback_error.to_string(),
            );
        }
        return Err(command_error(error));
    }
    Ok(())
}

#[tauri::command]
pub async fn run_workflow_agent(
    request: WorkflowAgentRequest,
    on_event: Channel<WorkflowAgentEvent>,
    state: State<'_, AppState>,
) -> Result<WorkflowAgentCompletion, CommandError> {
    let prompt = request.prompt.trim();
    if prompt.is_empty() || prompt.chars().count() > 100_000 {
        return Err(command_error(AppError::Protocol(
            "工作流指令必须为 1–100000 个字符".into(),
        )));
    }
    let project = state
        .workflow
        .store
        .project(request.project_id.trim())
        .await
        .map_err(command_error)?;
    let settings = load_model_settings(&state).await?;
    let api_key = state
        .credentials
        .workflow_model_api_key()
        .map_err(command_error)?
        .ok_or_else(|| command_error(AppError::MissingCredential))?;
    let run_id = format!("wfr_{}", Uuid::new_v4().simple());
    let turn_id = format!("wft_{}", Uuid::new_v4().simple());
    let stamp = Utc::now().to_rfc3339();
    let run = WorkflowRun {
        id: run_id.clone(),
        project_id: project.id.clone(),
        workflow_kind: "wisp-agent".into(),
        status: "running".into(),
        input_path: None,
        manifest_path: None,
        summary_json: "{}".into(),
        error: None,
        created_at: stamp.clone(),
        started_at: Some(stamp.clone()),
        finished_at: None,
    };
    let turn = WorkflowAgentTurn {
        id: turn_id.clone(),
        run_id: run_id.clone(),
        project_id: project.id.clone(),
        engine_turn_id: None,
        engine_session_id: None,
        provider: settings.provider.clone(),
        model: settings.model.clone(),
        prompt: prompt.to_owned(),
        response: String::new(),
        status: "running".into(),
        error: None,
        input_tokens: 0,
        output_tokens: 0,
        reasoning_tokens: 0,
        created_at: stamp,
        finished_at: None,
    };
    state
        .workflow
        .store
        .begin_agent_run(&run, &turn)
        .await
        .map_err(command_error)?;

    let outcome = state
        .workflow
        .supervisor
        .run_turn(&project, &settings, api_key, prompt, &on_event)
        .await;
    match outcome {
        Ok(completion) => {
            let persisted = register_agent_outputs(
                &project,
                &run_id,
                &settings.provider,
                &settings.model,
                &completion,
            );
            match persisted {
                Ok((manifest_path, artifacts)) => {
                    let summary = serde_json::json!({
                        "artifactCount": artifacts.len(),
                        "inputTokens": completion.input_tokens,
                        "outputTokens": completion.output_tokens,
                        "reasoningTokens": completion.reasoning_tokens,
                        "engineTurnId": completion.turn_id,
                    });
                    if let Err(error) = state
                        .workflow
                        .store
                        .complete_agent_run(
                            &turn_id,
                            &run_id,
                            &completion.turn_id,
                            completion.session_id.as_deref(),
                            &completion.text,
                            token_count(completion.input_tokens),
                            token_count(completion.output_tokens),
                            token_count(completion.reasoning_tokens),
                            &manifest_path,
                            &summary.to_string(),
                            &artifacts,
                        )
                        .await
                    {
                        persist_agent_failure(&state, &turn_id, &run_id, "failed", &error).await;
                        return Err(command_error(error));
                    }
                    Ok(completion)
                }
                Err(error) => {
                    persist_agent_failure(&state, &turn_id, &run_id, "failed", &error).await;
                    Err(command_error(error))
                }
            }
        }
        Err(error) => {
            let status = if matches!(error, AppError::Cancelled) {
                "cancelled"
            } else {
                "failed"
            };
            persist_agent_failure(&state, &turn_id, &run_id, status, &error).await;
            Err(command_error(error))
        }
    }
}

fn token_count(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

async fn persist_agent_failure(
    state: &AppState,
    turn_id: &str,
    run_id: &str,
    status: &str,
    error: &AppError,
) {
    let message = error.to_string();
    if let Err(persist_error) = state
        .workflow
        .store
        .fail_agent_run(turn_id, run_id, status, &message)
        .await
    {
        crate::diagnostics::log(
            "ERROR",
            "workflow_agent_failure_persist_failed",
            &persist_error.to_string(),
        );
    }
}

#[tauri::command]
pub async fn list_workflow_agent_turns(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<WorkflowAgentTurn>, CommandError> {
    state
        .workflow
        .store
        .list_agent_turns(project_id.trim())
        .await
        .map_err(command_error)
}

#[tauri::command]
pub async fn bridge_workflow_artifact_to_qa(
    artifact_id: String,
    state: State<'_, AppState>,
) -> Result<PendingChatAttachment, CommandError> {
    const MAX_ATTACHMENT_BYTES: u64 = 5 * 1024 * 1024;
    let artifact = state
        .workflow
        .store
        .artifact(artifact_id.trim())
        .await
        .map_err(command_error)?;
    let project = state
        .workflow
        .store
        .project(&artifact.project_id)
        .await
        .map_err(command_error)?;
    let path = resolve_project_relative(Path::new(&project.root), &artifact.relative_path)
        .map_err(command_error)?;
    let (size, digest) = sha256_file(&path).map_err(command_error)?;
    if size == 0 || size > MAX_ATTACHMENT_BYTES {
        return Err(command_error(AppError::Protocol(
            "发送到问答的产物必须非空且不能超过 5 MB".into(),
        )));
    }
    if size != u64::try_from(artifact.size_bytes).unwrap_or(u64::MAX) || digest != artifact.sha256 {
        return Err(command_error(AppError::Protocol(
            "工作流产物已在登记后发生变化；为避免发送未审计内容，本次操作已阻止".into(),
        )));
    }
    let bridge_id = format!("wfb_{}", Uuid::new_v4().simple());
    state
        .workflow
        .store
        .start_bridge_event(&bridge_id, &project.id, &artifact.id)
        .await
        .map_err(command_error)?;

    let outcome = async {
        let bytes = std::fs::read(&path)
            .map_err(|error| AppError::Internal(format!("无法读取工作流产物：{error}")))?;
        let gateway = state.database.gateway_url().await?;
        let bearer = ensure_active_bearer(&state).await?;
        state
            .yuxi
            .upload_tmp_attachment(&gateway, &bearer, &artifact.name, media_type(&path), bytes)
            .await
    }
    .await;
    match outcome {
        Ok(uploaded) => {
            state
                .workflow
                .store
                .finish_bridge_event(&bridge_id, "completed", Some(&uploaded.tmp_file_id), None)
                .await
                .map_err(command_error)?;
            Ok(uploaded)
        }
        Err(error) => {
            let message = error.to_string();
            let _ = state
                .workflow
                .store
                .finish_bridge_event(&bridge_id, "failed", None, Some(&message))
                .await;
            Err(command_error(error))
        }
    }
}

#[tauri::command]
pub async fn respond_workflow_approval(
    project_id: String,
    approval_id: String,
    approved: bool,
    feedback: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    state
        .workflow
        .supervisor
        .respond_approval(
            project_id.trim(),
            approval_id.trim(),
            approved,
            feedback.as_deref(),
        )
        .await
        .map_err(command_error)
}

#[tauri::command]
pub async fn cancel_workflow_agent(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<bool, CommandError> {
    state
        .workflow
        .supervisor
        .cancel_turn(project_id.trim())
        .await
        .map_err(command_error)
}

#[tauri::command]
pub async fn pick_workflow_directory(app: AppHandle) -> Result<Option<String>, CommandError> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.dialog().file().pick_folder(move |path| {
        let _ = sender.send(path);
    });
    let selected = receiver
        .await
        .map_err(|error| command_error(AppError::Internal(error.to_string())))?;
    Ok(selected.map(|path| path.to_string()))
}

#[tauri::command]
pub async fn create_workflow_project(
    root: String,
    name: Option<String>,
    state: State<'_, AppState>,
) -> Result<WorkflowProject, CommandError> {
    let project =
        initialize_project(Path::new(root.trim()), name.as_deref()).map_err(command_error)?;
    state
        .workflow
        .store
        .insert_project(&project)
        .await
        .map_err(command_error)?;
    Ok(project)
}

#[tauri::command]
pub async fn list_workflow_projects(
    state: State<'_, AppState>,
) -> Result<Vec<WorkflowProject>, CommandError> {
    state
        .workflow
        .store
        .list_projects()
        .await
        .map_err(command_error)
}

#[tauri::command]
pub async fn delete_workflow_project(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    state
        .workflow
        .store
        .delete_project(project_id.trim())
        .await
        .map_err(command_error)
}

#[tauri::command]
pub async fn list_workflow_runs(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<WorkflowRun>, CommandError> {
    state
        .workflow
        .store
        .list_runs(project_id.trim())
        .await
        .map_err(command_error)
}

#[tauri::command]
pub async fn list_workflow_artifacts(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<WorkflowArtifact>, CommandError> {
    state
        .workflow
        .store
        .list_artifacts(project_id.trim())
        .await
        .map_err(command_error)
}

#[tauri::command]
pub fn get_workflow_engine_status(state: State<'_, AppState>) -> WorkflowEngineStatus {
    state.workflow.supervisor.status()
}

#[tauri::command]
pub async fn cancel_workflow_run(
    run_id: String,
    state: State<'_, AppState>,
) -> Result<bool, CommandError> {
    state
        .workflow
        .cancel_run(run_id.trim())
        .map_err(command_error)
}

#[tauri::command]
pub async fn open_workflow_artifact(
    artifact_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    let artifact = state
        .workflow
        .store
        .artifact(artifact_id.trim())
        .await
        .map_err(command_error)?;
    let project = state
        .workflow
        .store
        .project(&artifact.project_id)
        .await
        .map_err(command_error)?;
    let real = resolve_project_relative(Path::new(&project.root), &artifact.relative_path)
        .map_err(command_error)?;
    app.opener()
        .open_path(real.to_string_lossy().into_owned(), None::<String>)
        .map_err(|error| command_error(AppError::Internal(format!("无法打开产物：{error}"))))
}

fn emit(channel: &Channel<WorkflowEvent>, event: WorkflowEvent) {
    // UI 切换不会取消后台运行；Channel 已关闭时继续完成并持久化运行。
    let _ = channel.send(event);
}

#[tauri::command]
pub async fn run_counts_pca_workflow(
    request: CountsPcaRequest,
    on_event: Channel<WorkflowEvent>,
    state: State<'_, AppState>,
) -> Result<WorkflowRun, CommandError> {
    let project = state
        .workflow
        .store
        .project(request.project_id.trim())
        .await
        .map_err(command_error)?;
    let input_relative = request.input_relative_path.trim().replace('\\', "/");
    let input_path = super::project::resolve_input_file(Path::new(&project.root), &input_relative)
        .map_err(command_error)?;
    let run_id = format!("wfr_{}", Uuid::new_v4().simple());
    let stamp = Utc::now().to_rfc3339();
    let mut run = WorkflowRun {
        id: run_id.clone(),
        project_id: project.id.clone(),
        workflow_kind: "counts-pca".into(),
        status: "running".into(),
        input_path: Some(input_relative.clone()),
        manifest_path: None,
        summary_json: "{}".into(),
        error: None,
        created_at: stamp.clone(),
        started_at: Some(stamp),
        finished_at: None,
    };
    let cancellation = state
        .workflow
        .register_run(&run_id)
        .map_err(command_error)?;
    if let Err(error) = state.workflow.store.insert_run(&run).await {
        state.workflow.finish_run(&run_id);
        return Err(command_error(error));
    }
    emit(
        &on_event,
        WorkflowEvent::RunStarted {
            run_id: run_id.clone(),
            message: "正在读取表达矩阵并校验样本".into(),
        },
    );
    emit(
        &on_event,
        WorkflowEvent::Progress {
            run_id: run_id.clone(),
            percent: 15,
            message: "执行 log2(count + 1) 与样本 PCA".into(),
        },
    );

    let project_id = project.id.clone();
    let project_root = PathBuf::from(&project.root);
    let execution_run_id = run_id.clone();
    let execution_input_relative = input_relative.clone();
    let execution_cancellation = cancellation.clone();
    let result = tokio::task::spawn_blocking(move || {
        execute_counts_pca(
            &project_id,
            &project_root,
            &input_path,
            &execution_input_relative,
            &execution_run_id,
            &execution_cancellation,
        )
    })
    .await
    .map_err(|error| AppError::Internal(format!("PCA 工作线程异常：{error}")));

    let outcome = match result {
        Ok(outcome) => outcome,
        Err(error) => Err(error),
    };
    match outcome {
        Ok(result) => {
            emit(
                &on_event,
                WorkflowEvent::Progress {
                    run_id: run_id.clone(),
                    percent: 85,
                    message: "正在登记校验和与不可变产物".into(),
                },
            );
            let summary_json = match serde_json::to_string(&result.summary) {
                Ok(summary) => summary,
                Err(error) => {
                    let error = AppError::Internal(error.to_string());
                    let _ = state
                        .workflow
                        .store
                        .update_run(
                            &run_id,
                            "failed",
                            None,
                            "{}",
                            Some(&error.to_string()),
                            true,
                        )
                        .await;
                    state.workflow.finish_run(&run_id);
                    return Err(command_error(error));
                }
            };
            if let Err(error) = state
                .workflow
                .store
                .complete_run(
                    &run_id,
                    &result.manifest_relative_path,
                    &summary_json,
                    &result.artifacts,
                )
                .await
            {
                let _ = state
                    .workflow
                    .store
                    .update_run(
                        &run_id,
                        "failed",
                        None,
                        "{}",
                        Some(&error.to_string()),
                        true,
                    )
                    .await;
                state.workflow.finish_run(&run_id);
                return Err(command_error(error));
            }
            for artifact in &result.artifacts {
                emit(
                    &on_event,
                    WorkflowEvent::ArtifactCreated {
                        run_id: run_id.clone(),
                        artifact: artifact.clone(),
                    },
                );
            }
            run = match state.workflow.store.run(&run_id).await {
                Ok(run) => run,
                Err(error) => {
                    state.workflow.finish_run(&run_id);
                    return Err(command_error(error));
                }
            };
            emit(&on_event, WorkflowEvent::RunCompleted { run: run.clone() });
        }
        Err(error) => {
            let (status, event) =
                if matches!(error, AppError::Cancelled) || cancellation.is_cancelled() {
                    (
                        "cancelled",
                        WorkflowEvent::RunCancelled {
                            run_id: run_id.clone(),
                        },
                    )
                } else {
                    (
                        "failed",
                        WorkflowEvent::RunFailed {
                            run_id: run_id.clone(),
                            message: error.to_string(),
                        },
                    )
                };
            if let Err(persist_error) = state
                .workflow
                .store
                .update_run(&run_id, status, None, "{}", Some(&error.to_string()), true)
                .await
            {
                state.workflow.finish_run(&run_id);
                return Err(command_error(persist_error));
            }
            run = match state.workflow.store.run(&run_id).await {
                Ok(run) => run,
                Err(error) => {
                    state.workflow.finish_run(&run_id);
                    return Err(command_error(error));
                }
            };
            emit(&on_event, event);
        }
    }
    state.workflow.finish_run(&run_id);
    Ok(run)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(base_url: &str, api_key: &str) -> SaveWorkflowModelSettings {
        SaveWorkflowModelSettings {
            provider: "openai".into(),
            base_url: base_url.into(),
            model: "test-model".into(),
            api_key: api_key.into(),
        }
    }

    #[test]
    fn model_settings_can_reuse_an_existing_secret() {
        let (public, replacement) =
            validate_model_settings(settings("https://api.example.com/v1", "")).unwrap();
        assert!(replacement.is_none());
        assert!(!public.has_api_key);
    }

    #[test]
    fn model_settings_reject_plain_http_except_loopback() {
        assert!(validate_model_settings(settings("http://api.example.com/v1", "key")).is_err());
        assert!(validate_model_settings(settings("http://127.0.0.1:11434/v1", "key")).is_ok());
    }
}
