use std::time::Duration;

use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{State, ipc::Channel};
use tokio_util::sync::CancellationToken;
use zeroize::Zeroize;

use crate::{
    config::{agent_slug, validate_gateway_url},
    credentials::{api_key_hint, validate_api_key},
    database::{LocalMessage, PublicSettings, ThreadSummary},
    error::{AppError, AppResult, CommandError},
    state::AppState,
    yuxi::{RunResult, progress_text, terminal_status},
};

const TERMINAL_STATUSES: [&str; 4] = ["completed", "failed", "cancelled", "interrupted"];

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
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
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
    state
        .yuxi
        .test_connection(&gateway_url, agent_slug(), &secret)
        .await?;
    state.credentials.save_api_key(secret.expose_secret())?;
    state
        .database
        .save_setting("gateway_url", &gateway_url)
        .await?;
    let hint = api_key_hint(secret.expose_secret());
    state.database.save_setting("api_key_hint", &hint).await?;
    Ok(PublicSettings {
        gateway_url,
        agent_slug: agent_slug().to_owned(),
        has_api_key: true,
        api_key_hint: Some(hint),
    })
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
        .map_err(CommandError::from)
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
                    let value = serde_json::from_str::<Value>(&event.data)
                        .map_err(|error| AppError::Protocol(error.to_string()))?;
                    if let Some(text) = progress_text(&value, &accumulated) {
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
    )
    .await?;
    let final_text = if final_result.output.is_empty() {
        accumulated
    } else {
        final_result.output
    };

    match final_result.status.as_str() {
        "completed" => {
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
                },
            )?;
            Ok(ChatCompletion {
                run_id: created.run_id,
                thread_id: created.thread_id,
                request_id: created.request_id,
                status: "completed".into(),
                text: final_text,
            })
        }
        "cancelled" | "interrupted" => Err(AppError::Cancelled),
        _ => {
            let message = final_result
                .error
                .unwrap_or_else(|| "Agent 运行失败".into());
            state
                .database
                .update_run_progress(
                    &created.run_id,
                    &final_result.status,
                    last_event_id.as_deref(),
                    &final_text,
                    Some("agent_failed"),
                    true,
                )
                .await?;
            Err(AppError::Protocol(message))
        }
    }
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
) -> AppResult<RunResult> {
    for _ in 0..400 {
        let result = tokio::select! {
            _ = cancellation.cancelled() => return cancel_local_run(state, run_id, "").await,
            result = state.yuxi.result(gateway_url, agent_slug(), api_key, run_id) => result?,
        };
        if is_terminal(&result.status) {
            return Ok(result);
        }
        tokio::select! {
            _ = cancellation.cancelled() => return cancel_local_run(state, run_id, "").await,
            _ = tokio::time::sleep(Duration::from_millis(1500)) => {}
        }
    }
    Err(AppError::Network(
        "等待任务结果超时；原任务未被重复创建".into(),
    ))
}

async fn cancel_local_run<T>(state: &AppState, run_id: &str, text: &str) -> AppResult<T> {
    let _ = state
        .database
        .update_run_progress(run_id, "cancelled", None, text, None, true)
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
    matches!(error, AppError::Network(_) | AppError::ServiceUnavailable)
}

#[cfg(test)]
mod tests {
    use super::{SendMessageRequest, validate_send_request};

    #[test]
    fn accepts_gateway_compatible_request_ids() {
        let request = SendMessageRequest {
            thread_id: "thread-1".into(),
            question: "水稻胚乳何时完成细胞化？".into(),
            request_id: "desktop-12345678-1234-1234-1234-123456789012".into(),
        };
        assert!(validate_send_request(&request).is_ok());
    }
}
