use std::time::Duration;

use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{State, ipc::Channel};
use tokio_util::sync::CancellationToken;
use zeroize::Zeroize;

use url::Url;

use crate::{
    config::{agent_slug, validate_gateway_url},
    credentials::{api_key_hint, api_key_scope_id, validate_api_key},
    database::{LocalMessage, PublicSettings, ThreadSummary},
    diagnostics,
    error::{AppError, AppResult, CommandError},
    state::AppState,
    yuxi::{
        CliSessionStart, CliTokenPoll, ModelOption, ProgressText, RunResult, ServerRunContext,
        terminal_status,
    },
};

const TERMINAL_STATUSES: [&str; 4] = ["completed", "failed", "cancelled", "interrupted"];
const MAX_EMPTY_COMPLETED_POLLS: i64 = 4;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageRequest {
    pub thread_id: String,
    pub question: String,
    pub request_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatCompletion {
    pub run_id: String,
    pub thread_id: String,
    pub request_id: String,
    pub status: String,
    pub text: String,
    pub context: ServerRunContext,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingRunSync {
    pub recovered: usize,
    pub pending: usize,
    pub failed: usize,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
// rename_all 只作用于 enum 变体名；字段名必须用 rename_all_fields 才会输出
// camelCase，否则前端读到的 runId/eventId 恒为 undefined。
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum RunEvent {
    Started {
        run_id: String,
        thread_id: String,
        request_id: String,
    },
    Status {
        status: String,
        message: String,
    },
    Text {
        text: String,
        event_id: Option<String>,
    },
    Done {
        run_id: String,
        status: String,
        text: String,
        context: Box<ServerRunContext>,
    },
}

#[tauri::command]
pub async fn get_public_settings(
    state: State<'_, AppState>,
) -> Result<PublicSettings, CommandError> {
    let has_api_key = state
        .credentials
        .has_api_key()
        .map_err(CommandError::from)?;
    Ok(PublicSettings {
        gateway_url: state
            .database
            .gateway_url()
            .await
            .map_err(CommandError::from)?,
        agent_slug: agent_slug().to_owned(),
        has_api_key,
        api_key_hint: if has_api_key {
            state
                .database
                .api_key_hint()
                .await
                .map_err(CommandError::from)?
        } else {
            None
        },
    })
}

#[tauri::command]
pub async fn save_connection(
    mut api_key: String,
    gateway_url: String,
    state: State<'_, AppState>,
) -> Result<PublicSettings, CommandError> {
    let result = save_connection_inner(&api_key, &gateway_url, &state).await;
    api_key.zeroize();
    result.map_err(CommandError::from)
}

async fn save_connection_inner(
    api_key: &str,
    gateway_url: &str,
    state: &AppState,
) -> AppResult<PublicSettings> {
    validate_api_key(api_key)?;
    let gateway_url = validate_gateway_url(gateway_url)?;
    let secret = SecretString::from(api_key.to_owned());
    let user_uid = state
        .yuxi
        .test_connection(&gateway_url, agent_slug(), &secret)
        .await?;
    let hint = api_key_hint(secret.expose_secret());
    let principal = user_uid.unwrap_or_else(|| api_key_scope_id(secret.expose_secret()));
    persist_local_connection(
        state,
        &gateway_url,
        &principal,
        &hint,
        Some("手动 API Key"),
        &secret,
    )
    .await?;
    Ok(PublicSettings {
        gateway_url,
        agent_slug: agent_slug().to_owned(),
        has_api_key: true,
        api_key_hint: Some(hint),
    })
}

/// Stronghold 与 SQLite 无法组成同一个物理事务，因此显式保留旧凭证并补偿回滚。
/// SQLite 的 activate_account 自身是事务性的；失败时只需把 Stronghold 恢复到旧值。
async fn persist_local_connection(
    state: &AppState,
    gateway_url: &str,
    principal: &str,
    hint: &str,
    key_name: Option<&str>,
    secret: &SecretString,
) -> AppResult<()> {
    let previous_secret = if state.credentials.has_api_key()? {
        Some(state.credentials.api_key()?)
    } else {
        None
    };

    state.credentials.save_api_key(secret.expose_secret())?;
    if let Err(database_error) = state
        .database
        .activate_account(gateway_url, principal, hint, key_name)
        .await
    {
        let rollback = match previous_secret {
            Some(previous) => state.credentials.save_api_key(previous.expose_secret()),
            None => state.credentials.delete_api_key(),
        };
        if let Err(rollback_error) = rollback {
            diagnostics::log(
                "ERROR",
                "credential_switch_rollback_failed",
                &rollback_error.to_string(),
            );
            return Err(AppError::CredentialStore(
                "本地账号切换失败，且安全凭证回滚失败；请删除凭证后重新登录".into(),
            ));
        }
        return Err(database_error);
    }
    Ok(())
}

#[tauri::command]
pub async fn test_connection(state: State<'_, AppState>) -> Result<(), CommandError> {
    let gateway_url = state
        .database
        .gateway_url()
        .await
        .map_err(CommandError::from)?;
    let api_key = state.credentials.api_key().map_err(CommandError::from)?;
    state
        .yuxi
        .test_connection(&gateway_url, agent_slug(), &api_key)
        .await
        .map_err(CommandError::from)?;
    Ok(())
}

#[tauri::command]
pub async fn delete_api_key(state: State<'_, AppState>) -> Result<(), CommandError> {
    state
        .credentials
        .delete_api_key()
        .map_err(CommandError::from)?;
    state
        .database
        .save_setting("api_key_hint", "")
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn create_thread(state: State<'_, AppState>) -> Result<ThreadSummary, CommandError> {
    state
        .database
        .create_thread()
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn list_threads(state: State<'_, AppState>) -> Result<Vec<ThreadSummary>, CommandError> {
    state
        .database
        .list_threads()
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn load_messages(
    thread_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<LocalMessage>, CommandError> {
    state
        .database
        .load_messages(&thread_id)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn get_thread_run_context(
    thread_id: String,
    state: State<'_, AppState>,
) -> Result<Option<Value>, CommandError> {
    let raw = state
        .database
        .latest_run_context(&thread_id)
        .await
        .map_err(CommandError::from)?;
    Ok(raw.and_then(|value| serde_json::from_str(&value).ok()))
}

#[tauri::command]
pub async fn sync_pending_runs(state: State<'_, AppState>) -> Result<PendingRunSync, CommandError> {
    sync_pending_runs_inner(&state)
        .await
        .map_err(CommandError::from)
}

async fn sync_pending_runs_inner(state: &AppState) -> AppResult<PendingRunSync> {
    let pending_runs = state.database.list_pending_runs().await?;
    if pending_runs.is_empty() {
        return Ok(PendingRunSync {
            recovered: 0,
            pending: 0,
            failed: 0,
            last_error: None,
        });
    }

    let gateway_url = state.database.gateway_url().await?;
    let api_key = state.credentials.api_key()?;
    let mut summary = PendingRunSync {
        recovered: 0,
        pending: 0,
        failed: 0,
        last_error: None,
    };
    for pending_run in pending_runs {
        let result = match state
            .yuxi
            .result(&gateway_url, agent_slug(), &api_key, &pending_run.run_id)
            .await
        {
            Ok(result) => result,
            Err(error) => {
                summary.pending += 1;
                summary.last_error = Some(error.to_string());
                continue;
            }
        };
        persist_run_context(state, &pending_run.run_id, &result.context).await?;
        match result.status.as_str() {
            "completed" if !result.output.is_empty() => {
                state
                    .database
                    .append_message(
                        &format!("assistant-{}", pending_run.run_id),
                        &pending_run.thread_id,
                        "assistant",
                        &result.output,
                    )
                    .await?;
                state
                    .database
                    .update_run_progress(
                        &pending_run.run_id,
                        "completed",
                        None,
                        &result.output,
                        None,
                        true,
                    )
                    .await?;
                summary.recovered += 1;
            }
            "completed" => {
                let poll_count = state
                    .database
                    .record_empty_completed_poll(&pending_run.run_id)
                    .await?;
                if poll_count >= MAX_EMPTY_COMPLETED_POLLS {
                    let message = "Yuxi 运行已完成，但服务端未返回最终回答；任务不会重复提交，请检查服务端 Worker 日志";
                    state
                        .database
                        .update_run_progress(
                            &pending_run.run_id,
                            "failed",
                            None,
                            "",
                            Some("empty_server_output"),
                            true,
                        )
                        .await?;
                    summary.failed += 1;
                    summary.last_error = Some(message.into());
                } else {
                    summary.pending += 1;
                }
            }
            "failed" | "cancelled" | "interrupted" => {
                state
                    .database
                    .update_run_progress(
                        &pending_run.run_id,
                        &result.status,
                        None,
                        &result.output,
                        result.error_code.as_deref(),
                        true,
                    )
                    .await?;
                summary.failed += 1;
                if let Some(message) = result.error {
                    summary.last_error = Some(message);
                }
            }
            _ => {
                state
                    .database
                    .update_run_status(&pending_run.run_id, &result.status)
                    .await?;
                summary.pending += 1;
            }
        }
    }
    Ok(summary)
}

#[tauri::command]
pub async fn rename_thread(
    thread_id: String,
    title: String,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    state
        .database
        .rename_thread(&thread_id, &title)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn delete_thread(
    thread_id: String,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    state
        .database
        .delete_thread(&thread_id)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn send_message(
    request: SendMessageRequest,
    on_event: Channel<RunEvent>,
    state: State<'_, AppState>,
) -> Result<ChatCompletion, CommandError> {
    validate_send_request(&request).map_err(CommandError::from)?;
    let cancellation = state
        .register_request(&request.request_id)
        .map_err(CommandError::from)?;
    let result = send_message_inner(&request, &on_event, &state, cancellation).await;
    state.finish_request(&request.request_id);
    result.map_err(CommandError::from)
}

async fn send_message_inner(
    request: &SendMessageRequest,
    on_event: &Channel<RunEvent>,
    state: &AppState,
    cancellation: CancellationToken,
) -> AppResult<ChatCompletion> {
    state.database.ensure_thread(&request.thread_id).await?;
    let question = request.question.trim();
    state
        .database
        .append_message(
            &format!("user-{}", request.request_id),
            &request.thread_id,
            "user",
            question,
        )
        .await?;

    let gateway_url = state.database.gateway_url().await?;
    let api_key = state.credentials.api_key()?;
    let yuxi_thread_id = state.database.yuxi_thread_id(&request.thread_id).await?;
    let created = tokio::select! {
        _ = cancellation.cancelled() => return Err(AppError::Cancelled),
        result = state.yuxi.create_run(
            &gateway_url,
            agent_slug(),
            &api_key,
            question,
            yuxi_thread_id.as_deref(),
            &request.request_id,
            None,
        ) => result?,
    };
    state.set_request_run_id(&request.request_id, &created.run_id)?;

    state
        .database
        .set_yuxi_thread_id(&request.thread_id, &created.thread_id)
        .await?;
    state
        .database
        .insert_run(
            &created.run_id,
            &request.request_id,
            &request.thread_id,
            &created.status,
            &serde_json::to_string(&created.run_context)
                .map_err(|error| AppError::Internal(error.to_string()))?,
        )
        .await?;
    send_channel(
        on_event,
        RunEvent::Started {
            run_id: created.run_id.clone(),
            thread_id: created.thread_id.clone(),
            request_id: created.request_id.clone(),
        },
    )?;

    let mut accumulated = String::new();
    let mut progress_text = ProgressText::default();
    let mut last_event_id: Option<String> = None;
    let mut terminal_received = false;

    for attempt in 0..4 {
        if attempt > 0 {
            send_channel(
                on_event,
                RunEvent::Status {
                    status: "reconnecting".into(),
                    message: format!("连接中断，正在进行第 {attempt} 次恢复"),
                },
            )?;
            tokio::select! {
                _ = cancellation.cancelled() => return cancel_local_run(state, &created.run_id, &accumulated).await,
                _ = tokio::time::sleep(Duration::from_secs(2_u64.pow(attempt))) => {}
            }
        }

        let response = tokio::select! {
            _ = cancellation.cancelled() => return cancel_local_run(state, &created.run_id, &accumulated).await,
            response = state.yuxi.event_response(
                &gateway_url,
                &api_key,
                &created.run_id,
                last_event_id.as_deref(),
            ) => match response {
                Ok(response) => response,
                Err(error) if error_is_reconnectable(&error) => continue,
                Err(error) => return Err(error),
            },
        };

        let mut stream = response.bytes_stream().eventsource();
        loop {
            let next = tokio::select! {
                _ = cancellation.cancelled() => return cancel_local_run(state, &created.run_id, &accumulated).await,
                next = stream.next() => next,
            };
            match next {
                Some(Ok(event)) => {
                    if !event.id.is_empty() {
                        last_event_id = Some(event.id.clone());
                    }
                    // 心跳或代理注入的非 JSON 帧只跳过该帧，不终止整次对话；
                    // 终态一致性由事件流与结果轮询共同兜底。
                    let value = match serde_json::from_str::<Value>(&event.data) {
                        Ok(value) => value,
                        Err(error) => {
                            diagnostics::log(
                                "WARN",
                                "sse_frame_skipped",
                                &format!("run={}: {error}", created.run_id),
                            );
                            continue;
                        }
                    };
                    let belongs_to_parent_thread = value
                        .get("thread_id")
                        .and_then(Value::as_str)
                        .is_none_or(|thread_id| thread_id == created.thread_id);
                    if belongs_to_parent_thread && let Some(text) = progress_text.apply(&value) {
                        accumulated = text;
                        state
                            .database
                            .update_run_progress(
                                &created.run_id,
                                "running",
                                last_event_id.as_deref(),
                                &accumulated,
                                None,
                                false,
                            )
                            .await?;
                        send_channel(
                            on_event,
                            RunEvent::Text {
                                text: accumulated.clone(),
                                event_id: last_event_id.clone(),
                            },
                        )?;
                    }
                    if event.event == "end" || terminal_status(&value).is_some_and(is_terminal) {
                        terminal_received = true;
                        break;
                    }
                    if event.event == "error" {
                        break;
                    }
                }
                Some(Err(_)) | None => break,
            }
        }
        if terminal_received {
            break;
        }
    }

    if !terminal_received {
        send_channel(
            on_event,
            RunEvent::Status {
                status: "polling".into(),
                message: "流式连接暂不可用，正在安全查询原任务结果".into(),
            },
        )?;
    }

    let final_result = wait_for_result(
        state,
        &gateway_url,
        &api_key,
        &created.run_id,
        &cancellation,
        &accumulated,
    )
    .await?;
    persist_run_context(state, &created.run_id, &final_result.context).await?;
    let context = final_result.context.clone();
    let final_text = final_result.output;

    match final_result.status.as_str() {
        "completed" => {
            if is_reasoning_protocol_failure(&final_text) {
                state
                    .database
                    .update_run_progress(
                        &created.run_id,
                        "failed",
                        last_event_id.as_deref(),
                        "",
                        Some("server_upgrade_required"),
                        true,
                    )
                    .await?;
                return Err(AppError::ServerUpgradeRequired);
            }
            state
                .database
                .append_message(
                    &format!("assistant-{}", created.run_id),
                    &request.thread_id,
                    "assistant",
                    &final_text,
                )
                .await?;
            state
                .database
                .update_run_progress(
                    &created.run_id,
                    "completed",
                    last_event_id.as_deref(),
                    &final_text,
                    None,
                    true,
                )
                .await?;
            send_channel(
                on_event,
                RunEvent::Done {
                    run_id: created.run_id.clone(),
                    status: "completed".into(),
                    text: final_text.clone(),
                    context: Box::new(context.clone()),
                },
            )?;
            Ok(ChatCompletion {
                run_id: created.run_id,
                thread_id: created.thread_id,
                request_id: created.request_id,
                status: "completed".into(),
                text: final_text,
                context,
            })
        }
        "cancelled" => Err(AppError::Cancelled),
        _ => {
            let message = final_result.error.unwrap_or_else(|| {
                if final_result.status == "interrupted" {
                    "服务端已中断本次运行，请在 Yuxi 服务端处理需要人工确认的步骤后重试".into()
                } else {
                    "Agent 运行失败".into()
                }
            });
            let error = if is_reasoning_protocol_failure(&message) {
                AppError::ServerUpgradeRequired
            } else {
                AppError::Protocol(message)
            };
            let persisted_text = if matches!(&error, AppError::ServerUpgradeRequired) {
                ""
            } else {
                &final_text
            };
            state
                .database
                .update_run_progress(
                    &created.run_id,
                    &final_result.status,
                    last_event_id.as_deref(),
                    persisted_text,
                    final_result.error_code.as_deref().or(Some(error.code())),
                    true,
                )
                .await?;
            Err(error)
        }
    }
}

async fn persist_run_context(
    state: &AppState,
    run_id: &str,
    context: &ServerRunContext,
) -> AppResult<()> {
    let serialized =
        serde_json::to_string(context).map_err(|error| AppError::Internal(error.to_string()))?;
    state.database.update_run_context(run_id, &serialized).await
}

#[tauri::command]
pub async fn cancel_run(
    request_id: String,
    run_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    let tracked_run_id = state
        .cancel_request(&request_id)
        .map_err(CommandError::from)?;
    if let Some(run_id) = run_id.filter(|value| !value.is_empty()).or(tracked_run_id) {
        let gateway_url = state
            .database
            .gateway_url()
            .await
            .map_err(CommandError::from)?;
        let api_key = state.credentials.api_key().map_err(CommandError::from)?;
        state
            .yuxi
            .cancel_run(&gateway_url, &api_key, &run_id)
            .await
            .map_err(CommandError::from)?;
    }
    Ok(())
}

async fn wait_for_result(
    state: &AppState,
    gateway_url: &str,
    api_key: &SecretString,
    run_id: &str,
    cancellation: &CancellationToken,
    accumulated_text: &str,
) -> AppResult<RunResult> {
    let mut completed_without_output = 0;
    for _ in 0..400 {
        let result = tokio::select! {
            _ = cancellation.cancelled() => {
                return cancel_local_run(state, run_id, accumulated_text).await
            }
            result = state.yuxi.result(gateway_url, agent_slug(), api_key, run_id) => result?,
        };
        if result.status == "completed" && result.output.is_empty() {
            completed_without_output += 1;
            if completed_without_output >= 4 {
                return Err(AppError::Protocol(
                    "Yuxi 运行已完成，但最终回答尚未生成，请稍后重试".into(),
                ));
            }
        } else if is_terminal(&result.status) {
            return Ok(result);
        }
        tokio::select! {
            _ = cancellation.cancelled() => {
                return cancel_local_run(state, run_id, accumulated_text).await
            }
            _ = tokio::time::sleep(Duration::from_millis(1500)) => {}
        }
    }
    Err(AppError::Network(
        "等待任务结果超时；原任务未被重复创建".into(),
    ))
}

async fn cancel_local_run<T>(state: &AppState, run_id: &str, text: &str) -> AppResult<T> {
    // 本地只写非终态 cancel_requested：服务端可能已经完成（取消晚到），终态
    // 一律交由 sync_pending_runs 对账权威结果。提前写终态 cancelled 会让该
    // 行永久退出对账，服务端已生成的完整回答就此失联。
    let _ = state
        .database
        .update_run_progress(run_id, "cancel_requested", None, text, None, false)
        .await;
    Err(AppError::Cancelled)
}

fn validate_send_request(request: &SendMessageRequest) -> AppResult<()> {
    if request.question.trim().is_empty() || request.question.chars().count() > 20_000 {
        return Err(AppError::Protocol(
            "问题长度必须为 1 至 20000 个字符".into(),
        ));
    }
    if request.request_id.len() < 16
        || request.request_id.len() > 64
        || !request
            .request_id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "._:-".contains(character))
    {
        return Err(AppError::Protocol("request_id 格式无效".into()));
    }
    Ok(())
}

fn send_channel(channel: &Channel<RunEvent>, event: RunEvent) -> AppResult<()> {
    channel
        .send(event)
        .map_err(|error| AppError::Internal(format!("向界面发送运行事件失败：{error}")))
}

fn is_terminal(status: &str) -> bool {
    TERMINAL_STATUSES.contains(&status)
}

fn error_is_reconnectable(error: &AppError) -> bool {
    matches!(
        error,
        AppError::Network(_) | AppError::ServiceUnavailable | AppError::LocalServiceUnavailable
    )
}

fn is_reasoning_protocol_failure(message: &str) -> bool {
    let normalized = message.to_ascii_lowercase();
    normalized.contains("model call failed")
        && normalized.contains("reasoning_content")
        && normalized.contains("must be passed back")
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceLoginStart {
    pub device_code: String,
    pub user_code: String,
    pub verification_url: String,
    pub web_origin: String,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceLoginResult {
    pub approved: bool,
    pub user_name: Option<String>,
    pub user_uid: Option<String>,
}

fn validate_device_login_urls(start: &CliSessionStart) -> AppResult<String> {
    let user_code = start.user_code.as_bytes();
    let valid_user_code = user_code.len() == 9
        && user_code[4] == b'-'
        && user_code.iter().enumerate().all(|(index, byte)| {
            index == 4
                || ((byte.is_ascii_uppercase() || matches!(byte, b'2'..=b'9'))
                    && !matches!(byte, b'I' | b'O'))
        });
    if !valid_user_code {
        return Err(AppError::Protocol("服务端返回的 user_code 格式无效".into()));
    }

    let verification = Url::parse(&start.verification_uri)
        .map_err(|_| AppError::Protocol("服务端返回的网页授权地址无效".into()))?;
    let complete = Url::parse(&start.verification_uri_complete)
        .map_err(|_| AppError::Protocol("服务端返回的完整网页授权地址无效".into()))?;

    for url in [&verification, &complete] {
        let host = url
            .host_str()
            .unwrap_or_default()
            .trim_start_matches('[')
            .trim_end_matches(']');
        let loopback = matches!(host, "localhost" | "127.0.0.1" | "::1");
        if url.host_str().is_none()
            || url.username() != ""
            || url.password().is_some()
            || url.fragment().is_some()
            || (url.scheme() != "https" && !(url.scheme() == "http" && loopback))
        {
            return Err(AppError::Protocol(
                "网页授权地址必须是 HTTPS（本机调试可用 loopback HTTP），且不能包含凭据或片段"
                    .into(),
            ));
        }
    }
    if verification.query().is_some()
        || verification.origin() != complete.origin()
        || verification.path() != complete.path()
    {
        return Err(AppError::Protocol(
            "服务端返回的网页授权地址不属于同一可信页面".into(),
        ));
    }
    let code_matches = complete
        .query_pairs()
        .any(|(key, value)| key == "user_code" && value == start.user_code);
    if !code_matches {
        return Err(AppError::Protocol(
            "完整网页授权地址缺少匹配的 user_code".into(),
        ));
    }
    Ok(verification.origin().ascii_serialization())
}

#[tauri::command]
pub async fn start_device_login(
    gateway_url: String,
    key_name: Option<String>,
    state: State<'_, AppState>,
) -> Result<DeviceLoginStart, CommandError> {
    // 先确认网关可达且地址合法，再发起设备码会话
    let _ = validate_gateway_url(&gateway_url).map_err(CommandError::from)?;
    let start: CliSessionStart = state
        .yuxi
        .create_cli_session(&gateway_url, key_name.as_deref())
        .await
        .map_err(CommandError::from)?;
    let web_origin = validate_device_login_urls(&start).map_err(CommandError::from)?;
    Ok(DeviceLoginStart {
        device_code: start.device_code,
        user_code: start.user_code,
        verification_url: start.verification_uri_complete,
        web_origin,
        expires_in: start.expires_in,
        interval: start.interval,
    })
}

#[tauri::command]
pub async fn poll_device_login(
    gateway_url: String,
    device_code: String,
    state: State<'_, AppState>,
) -> Result<DeviceLoginResult, CommandError> {
    match state
        .yuxi
        .poll_cli_token(&gateway_url, &device_code)
        .await
        .map_err(CommandError::from)?
    {
        CliTokenPoll::Pending => Ok(DeviceLoginResult {
            approved: false,
            user_name: None,
            user_uid: None,
        }),
        CliTokenPoll::Approved {
            mut secret,
            api_key_id,
            account_scope_id,
            key_name,
            user_name,
            user_uid,
        } => {
            let gateway_url = validate_gateway_url(&gateway_url).map_err(CommandError::from)?;
            let hint = api_key_hint(&secret);
            let secure_secret = SecretString::from(secret.clone());
            secret.zeroize();
            let key_display = if key_name.is_empty() {
                "桌面端".to_string()
            } else {
                key_name
            };
            if let Err(error) = persist_local_connection(
                &state,
                &gateway_url,
                &account_scope_id,
                &hint,
                Some(&key_display),
                &secure_secret,
            )
            .await
            {
                // 本机持久化失败时，撤销刚由设备流创建的专用 Key，避免服务端遗留孤儿凭证。
                if let Err(revoke_error) = state
                    .yuxi
                    .delete_api_key(&gateway_url, &secure_secret, api_key_id)
                    .await
                {
                    diagnostics::log(
                        "ERROR",
                        "device_key_compensation_failed",
                        &revoke_error.to_string(),
                    );
                }
                return Err(CommandError::from(error));
            }
            Ok(DeviceLoginResult {
                approved: true,
                user_name: Some(user_name),
                user_uid: Some(user_uid),
            })
        }
    }
}

#[tauri::command]
pub async fn list_chat_models(
    state: State<'_, AppState>,
) -> Result<Vec<ModelOption>, CommandError> {
    let gateway_url = state
        .database
        .gateway_url()
        .await
        .map_err(CommandError::from)?;
    let api_key = state.credentials.api_key().map_err(CommandError::from)?;
    let models = state
        .yuxi
        .list_chat_models(&gateway_url, &api_key)
        .await
        .map_err(CommandError::from)?;
    Ok(models)
}

#[tauri::command]
pub async fn get_chat_model_preference(
    state: State<'_, AppState>,
) -> Result<Option<String>, CommandError> {
    let gateway_url = state
        .database
        .gateway_url()
        .await
        .map_err(CommandError::from)?;
    let api_key = state.credentials.api_key().map_err(CommandError::from)?;
    state
        .yuxi
        .get_chat_model_preference(&gateway_url, &api_key)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn set_chat_model_preference(
    model_spec: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    let gateway_url = state
        .database
        .gateway_url()
        .await
        .map_err(CommandError::from)?;
    let api_key = state.credentials.api_key().map_err(CommandError::from)?;
    let normalized = model_spec
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    state
        .yuxi
        .set_chat_model_preference(&gateway_url, &api_key, normalized)
        .await
        .map_err(CommandError::from)
}

#[cfg(test)]
mod tests {
    use super::{
        RunEvent, SendMessageRequest, is_reasoning_protocol_failure, validate_device_login_urls,
        validate_send_request,
    };
    use crate::yuxi::CliSessionStart;

    #[test]
    fn run_events_serialize_fields_as_camel_case() {
        // 前端 RunEvent 类型按 camelCase 读取（event.runId）；字段名若保持
        // snake_case，前端将拿到 undefined 且恢复轮询永不武装。
        let started = serde_json::to_value(RunEvent::Started {
            run_id: "run-1".into(),
            thread_id: "thread-1".into(),
            request_id: "request-1".into(),
        })
        .expect("serialize started");
        assert_eq!(started["type"], "started");
        assert_eq!(started["runId"], "run-1");
        assert_eq!(started["threadId"], "thread-1");
        assert_eq!(started["requestId"], "request-1");

        let text = serde_json::to_value(RunEvent::Text {
            text: "回答".into(),
            event_id: Some("1-2".into()),
        })
        .expect("serialize text");
        assert_eq!(text["type"], "text");
        assert_eq!(text["eventId"], "1-2");

        let done = serde_json::to_value(RunEvent::Done {
            run_id: "run-1".into(),
            status: "completed".into(),
            text: "回答".into(),
            context: Default::default(),
        })
        .expect("serialize done");
        assert_eq!(done["type"], "done");
        assert_eq!(done["runId"], "run-1");
    }

    #[test]
    fn accepts_gateway_compatible_request_ids() {
        let request = SendMessageRequest {
            thread_id: "thread-1".into(),
            question: "水稻胚乳何时完成细胞化？".into(),
            request_id: "desktop-12345678-1234-1234-1234-123456789012".into(),
        };
        assert!(validate_send_request(&request).is_ok());
    }

    #[test]
    fn detects_reasoning_protocol_failure_from_legacy_server() {
        let message = "Model call failed after 3 attempts with BadRequestError: The `reasoning_content` in the thinking mode must be passed back to the API.";
        assert!(is_reasoning_protocol_failure(message));
    }

    #[test]
    fn does_not_reclassify_normal_model_content() {
        assert!(!is_reasoning_protocol_failure(
            "reasoning_content is an API field described in this answer"
        ));
    }

    #[test]
    fn rejects_cross_origin_or_insecure_device_authorization_urls() {
        let valid = CliSessionStart {
            device_code: "secret".into(),
            user_code: "ABCD-EFGH".into(),
            verification_uri: "https://rice.example.cn/auth/cli/authorize".into(),
            verification_uri_complete:
                "https://rice.example.cn/auth/cli/authorize?user_code=ABCD-EFGH".into(),
            expires_in: 600,
            interval: 2,
        };
        assert_eq!(
            validate_device_login_urls(&valid).unwrap(),
            "https://rice.example.cn"
        );

        let mut cross_origin = valid.clone();
        cross_origin.verification_uri_complete =
            "https://phishing.example/auth/cli/authorize?user_code=ABCD-EFGH".into();
        assert!(validate_device_login_urls(&cross_origin).is_err());

        let mut insecure = valid;
        insecure.verification_uri = "http://rice.example.cn/auth/cli/authorize".into();
        insecure.verification_uri_complete =
            "http://rice.example.cn/auth/cli/authorize?user_code=ABCD-EFGH".into();
        assert!(validate_device_login_urls(&insecure).is_err());

        let mut malformed_code = CliSessionStart {
            device_code: "secret".into(),
            user_code: "../../BAD".into(),
            verification_uri: "https://rice.example.cn/auth/cli/authorize".into(),
            verification_uri_complete:
                "https://rice.example.cn/auth/cli/authorize?user_code=../../BAD".into(),
            expires_in: 600,
            interval: 2,
        };
        assert!(validate_device_login_urls(&malformed_code).is_err());

        malformed_code.user_code = "ABCI-EFGH".into();
        malformed_code.verification_uri_complete =
            "https://rice.example.cn/auth/cli/authorize?user_code=ABCI-EFGH".into();
        assert!(validate_device_login_urls(&malformed_code).is_err());
    }
}
