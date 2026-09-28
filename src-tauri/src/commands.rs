use std::{path::Path, time::Duration};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{State, ipc::Channel};
use tauri_plugin_opener::OpenerExt;
use tokio_util::sync::CancellationToken;
use zeroize::Zeroize;

use crate::{
    config::{agent_slug, validate_gateway_url},
    credentials::{api_key_hint, api_key_scope_id, validate_api_key},
    database::{
        LocalMessage, LocalMessageAttachment, MessageEvidence, PublicSettings, ThreadSummary,
    },
    diagnostics,
    error::{AppError, AppResult, CommandError},
    state::AppState,
    yuxi::{
        CitationRef, ClarificationQuestion, FigureCardAsset, FigureRef, LocatorCandidate,
        ModelOption, PendingChatAttachment, ProgressText, ProtocolInfo, PublishableTable,
        QuestionAnswer, RunResult, ServerRunContext, build_resume_payload, parse_citation_ready,
        parse_custom_warning, parse_followup_suggestions, parse_graph_snapshot,
        parse_interrupt_questions, parse_locator_candidates, sanitize_visible_model_text,
        terminal_status, validate_authoritative_run_context,
    },
};

const TERMINAL_STATUSES: [&str; 4] = ["completed", "failed", "cancelled", "interrupted"];
const MAX_EMPTY_COMPLETED_POLLS: i64 = 4;
/// SSE 事件名白名单（verbose=false 契约 + 遗留兼容形态）。白名单外的命名事件
/// 记 WARN 并计数——这是发现「服务端发了新事件而桌面端未消费」的唯一信号。
const KNOWN_SSE_EVENTS: [&str; 7] = [
    "messages",
    "custom",
    "interrupt",
    "error",
    "end",
    "trace",
    "message",
];

/// `event: custom` 帧的二级白名单：custom 帧的事件名都是 "custom"，语义名在
/// `payload.name`。一级白名单对 custom 恒命中，若不做二级校验，服务端新增
/// custom 名（如新的证据类事件）会被静默丢弃且无任何告警——这里把未知
/// custom 名记 WARN 并计入会话摘要（sse_custom_unknown）。
const KNOWN_CUSTOM_EVENT_NAMES: [&str; 6] = [
    // 已知且刻意不渲染：进度语义由 messages 帧（status=progress）承载。
    "yuxi.agent_state",
    // 证据平面附件 + 运行提示（1.8 capabilities 对应的 custom 载荷名）。
    "yuxi.warning",
    "yuxi.citation_ready",
    "yuxi.locator_candidates",
    "yuxi.graph_snapshot_ready",
    // 追问建议（1.9 followup_suggestions）：回答终态前的可点击下一问清单。
    "yuxi.followup_suggestions",
];

fn is_known_sse_event(name: &str) -> bool {
    // 未命名事件（默认 data 帧）按已知处理：服务端信封的事件名只在 SSE
    // event: 行上，白名单只针对命名事件。
    name.is_empty() || KNOWN_SSE_EVENTS.contains(&name)
}

fn is_known_custom_event(name: &str) -> bool {
    name.is_empty() || KNOWN_CUSTOM_EVENT_NAMES.contains(&name)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageRequest {
    pub thread_id: String,
    pub question: String,
    pub request_id: String,
    #[serde(default)]
    pub attachments: Vec<PendingChatAttachment>,
    /// 中断恢复（人工审批续跑）：被恢复的父 run ID。存在时 `question` 是
    /// 用户对审批的答复，以 resume 载荷发送而非新 query，且不得带附件。
    #[serde(default)]
    pub resume_run_id: Option<String>,
    /// 澄清问题续跑（ask_user_question_required）：随 AskUserQuestion 事件
    /// 原样回传的问题定义与用户作答，Rust 侧组装成 {question_id: 答案} 载荷。
    #[serde(default)]
    pub resume_questions: Vec<ClarificationQuestion>,
    #[serde(default)]
    pub resume_answers: Vec<QuestionAnswer>,
}

#[tauri::command]
pub async fn upload_chat_attachment(
    file_name: String,
    content_type: String,
    data_base64: String,
    state: State<'_, AppState>,
) -> Result<PendingChatAttachment, CommandError> {
    const MAX_ATTACHMENT_BYTES: usize = 5 * 1024 * 1024;
    let safe_name = Path::new(&file_name)
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| CommandError::from(AppError::Protocol("附件文件名无效".into())))?;
    let bytes = BASE64_STANDARD
        .decode(data_base64)
        .map_err(|_| CommandError::from(AppError::Protocol("附件内容编码无效".into())))?;
    if bytes.is_empty() || bytes.len() > MAX_ATTACHMENT_BYTES {
        return Err(CommandError::from(AppError::Protocol(
            "附件必须为非空文件且不能超过 5 MB".into(),
        )));
    }
    let gateway_url = state
        .database
        .gateway_url()
        .await
        .map_err(CommandError::from)?;
    let bearer = ensure_active_bearer(&state)
        .await
        .map_err(CommandError::from)?;
    state
        .yuxi
        .upload_tmp_attachment(&gateway_url, &bearer, safe_name, &content_type, bytes)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn parse_chat_attachment(
    mut attachment: PendingChatAttachment,
    parse_method: String,
    state: State<'_, AppState>,
) -> Result<PendingChatAttachment, CommandError> {
    let gateway_url = state
        .database
        .gateway_url()
        .await
        .map_err(CommandError::from)?;
    let bearer = ensure_active_bearer(&state)
        .await
        .map_err(CommandError::from)?;
    state
        .yuxi
        .parse_tmp_attachment(&gateway_url, &bearer, &mut attachment, parse_method.trim())
        .await
        .map_err(CommandError::from)?;
    Ok(attachment)
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
    Trace {
        run_id: String,
        /// yuxi.run-trace.v1 wire 事件原样透传，前端负责投影与渲染。
        trace: Value,
    },
    /// 澄清问题中断（ask_user_question_required）：questions 为服务端权威
    /// 定义（questionId/question/options/multiSelect/allowOther），UI 渲染
    /// 选项卡片，用户作答后以 resume 载荷续跑本 run。
    AskUserQuestion {
        run_id: String,
        questions: Vec<ClarificationQuestion>,
    },
    /// 证据平面：结构化引用（citation_ready v2 九键）与可选图卡投影。
    /// figures 缺席 ⟺ 本 run 未发布图卡；成员缺取图硬前置键时渲染层
    /// fail-closed 不展示，不猜测补全。
    CitationsReady {
        run_id: String,
        citation: CitationRef,
        figures: Vec<FigureCardAsset>,
        figure_refs: Vec<FigureRef>,
        tables: Vec<PublishableTable>,
    },
    /// 跨文献歧义候选清单（locator_candidates）：只含文档身份三键，展示用；
    /// 「一键重问」依赖服务端 mention.v2 落地后再开。
    LocatorCandidates {
        run_id: String,
        candidates: Vec<LocatorCandidate>,
    },
    /// 规范关系图的版本化消息附件（graph_snapshot_v1）。
    GraphSnapshotReady {
        run_id: String,
        snapshot: Value,
    },
    /// 运行内非阻塞提示（yuxi.warning）：不影响终态，UI 以可关闭的提示条呈现。
    Warning {
        message: String,
    },
    /// 追问建议（yuxi.followup_suggestions）：回答终态前下发的可点击下一问
    /// 清单；点击后由前端作为普通用户消息发送（完整 run 链路）。
    FollowupSuggestions {
        run_id: String,
        suggestions: Vec<String>,
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
    // P5：纯会话账号（激活码开通，无静态 Key）也算已连接
    let has_api_key = state
        .credentials
        .has_api_key()
        .map_err(CommandError::from)?;
    let scope = state
        .database
        .current_account_scope()
        .await
        .unwrap_or_else(|_| "legacy".into());
    let has_session = !state
        .credentials
        .session_blob(&scope)
        .map_err(CommandError::from)?
        .unwrap_or_default()
        .is_empty();
    let connected = has_api_key || has_session;
    Ok(PublicSettings {
        gateway_url: state
            .database
            .gateway_url()
            .await
            .map_err(CommandError::from)?,
        agent_slug: state
            .database
            .server_agent_slug()
            .await
            .map_err(CommandError::from)?,
        has_api_key: connected,
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
    let server_agent_slug = refresh_server_agent_slug(state, &gateway_url, &secret).await?;
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
        agent_slug: server_agent_slug,
        has_api_key: true,
        api_key_hint: Some(hint),
    })
}

/// P5 三字段登录：登录标识 + 密码 + API Key 由服务端原子校验后绑定本机。
/// 服务端同时校验账号状态、Key 归属及租户/部门边界，并返回固化的账号作用域；
/// 客户端只在整组凭据通过后落盘，密钥有效期由服务端签发策略决定。
#[tauri::command]
pub async fn save_connection_with_login(
    mut api_key: String,
    gateway_url: String,
    username: String,
    mut password: String,
    state: State<'_, AppState>,
) -> Result<PublicSettings, CommandError> {
    let result = save_connection_with_login_inner(
        &api_key,
        &gateway_url,
        username.trim(),
        &password,
        &state,
    )
    .await;
    api_key.zeroize();
    password.zeroize();
    result.map_err(CommandError::from)
}

async fn save_connection_with_login_inner(
    api_key: &str,
    gateway_url: &str,
    username: &str,
    password: &str,
    state: &AppState,
) -> AppResult<PublicSettings> {
    validate_api_key(api_key)?;
    let gateway = validate_gateway_url(gateway_url)?;
    let secret = SecretString::from(api_key.to_owned());
    let password_secret = SecretString::from(password.to_owned());

    // 三要素联合校验：密码对 + 密钥属主一致，任一不符即拒绝绑定
    let identity = state
        .yuxi
        .verify_desktop_login(&gateway, username, &password_secret, &secret)
        .await?;
    let server_agent_slug = refresh_server_agent_slug(state, &gateway, &secret).await?;

    let hint = api_key_hint(secret.expose_secret());
    persist_local_connection(
        state,
        &gateway,
        &identity.account_scope_id,
        &hint,
        Some(device_label(&identity.user_uid).as_str()),
        &secret,
    )
    .await?;

    let local_scope = local_account_scope(&gateway, &identity.account_scope_id);
    if let Err(error) = state
        .credentials
        .save_api_key_for_scope(&local_scope, secret.expose_secret())
    {
        diagnostics::log("WARN", "desktop_scoped_key_save_failed", &error.to_string());
    }
    if let Err(error) = state
        .database
        .upsert_account(&local_scope, &identity.user_name, &gateway)
        .await
    {
        diagnostics::log("WARN", "desktop_account_upsert_failed", &error.to_string());
    }
    Ok(PublicSettings {
        gateway_url: gateway.to_string(),
        agent_slug: server_agent_slug,
        has_api_key: true,
        api_key_hint: Some(hint),
    })
}

fn device_label(username: &str) -> String {
    format!("桌面端-{username}")
}

/// 从本地作用域（`{gateway}|{principal}`）解出服务端 principal；
/// 无 `|` 的旧格式（legacy / api-key-sha256）原样返回。
fn principal_of_scope(scope: &str) -> &str {
    scope
        .rsplit_once('|')
        .map_or(scope, |(_, principal)| principal)
}

// ==== P5 企业激活码与设备码开户 ====

/// 企业激活码开户（P0 主路径）：一次性激活码在服务端兑换为设备会话对，
/// **不产生任何静态 Key**。兑换成功即完成会话 blob 的**首次落盘**——这是
/// 会话生命周期的起点，`ensure_active_bearer` 的会话分支自此可达。
#[tauri::command]
pub async fn activate_enterprise_account(
    mut activation_code: String,
    gateway_url: String,
    device_name: Option<String>,
    state: State<'_, AppState>,
) -> Result<PublicSettings, CommandError> {
    let result = activate_enterprise_account_inner(
        &activation_code,
        &gateway_url,
        device_name.as_deref(),
        &state,
    )
    .await;
    activation_code.zeroize();
    result.map_err(CommandError::from)
}

async fn activate_enterprise_account_inner(
    activation_code: &str,
    gateway_url: &str,
    device_name: Option<&str>,
    state: &AppState,
) -> AppResult<PublicSettings> {
    let code = activation_code.trim();
    if !code.starts_with("yxact_") || code.len() < 8 || code.len() > 256 {
        return Err(AppError::InvalidCredential);
    }
    let gateway = validate_gateway_url(gateway_url)?;
    let secret = SecretString::from(code.to_owned());
    let exchange = state
        .yuxi
        .exchange_onboarding_activation(&gateway, &secret, device_name.unwrap_or("桌面端"))
        .await?;
    let label = device_label(&exchange.user_name);
    let server_agent_slug = persist_device_session(
        state,
        &gateway,
        &exchange.account_scope_id,
        &exchange.user_name,
        &exchange.session,
        &label,
    )
    .await?;
    Ok(PublicSettings {
        gateway_url: gateway,
        agent_slug: server_agent_slug,
        has_api_key: true,
        api_key_hint: None,
    })
}

/// 会话对的统一落盘：blob 写入（Stronghold）→ activate_account（SQLite 事务）
/// → upsert_account。Stronghold 与 SQLite 无共同事务，SQLite 失败时补偿清除
/// 刚写入的 blob，避免孤儿会话凭据。返回服务端权威默认智能体。
async fn persist_device_session(
    state: &AppState,
    gateway: &str,
    account_scope_id: &str,
    user_name: &str,
    session: &crate::yuxi::SessionPair,
    key_name: &str,
) -> AppResult<String> {
    use crate::session::StoredSession;

    let local_scope = local_account_scope(gateway, account_scope_id);
    let now = chrono::Utc::now().timestamp();
    let stored = StoredSession {
        access_token: session.access_token.clone(),
        refresh_token: session.refresh_token.clone(),
        family_id: session.session_id.clone(),
        access_expires_at: crate::session::parse_jwt_exp(&session.access_token)
            .unwrap_or(now + session.access_expires_in),
        account_scope_id: account_scope_id.to_owned(),
    };
    let json =
        serde_json::to_string(&stored).map_err(|error| AppError::Internal(error.to_string()))?;
    state.credentials.save_session_blob(&local_scope, &json)?;
    if let Err(database_error) = state
        .database
        .activate_account(gateway, account_scope_id, "", Some(key_name))
        .await
    {
        // 补偿：清除孤儿 blob，保持 Stronghold 与账号目录一致。
        if let Err(cleanup_error) = state.credentials.delete_scope_records(&local_scope) {
            diagnostics::log(
                "ERROR",
                "session_activation_rollback_failed",
                &format!("{database_error}; {cleanup_error}"),
            );
        }
        return Err(database_error);
    }
    if let Err(error) = state
        .database
        .upsert_account(&local_scope, user_name, gateway)
        .await
    {
        diagnostics::log("WARN", "session_account_upsert_failed", &error.to_string());
    }

    let bearer = SecretString::from(session.access_token.clone());
    let server_agent_slug = match refresh_server_agent_slug(state, gateway, &bearer).await {
        Ok(slug) => slug,
        Err(error) => {
            diagnostics::log(
                "WARN",
                "session_agent_slug_refresh_failed",
                &error.to_string(),
            );
            state.database.server_agent_slug().await?
        }
    };
    diagnostics::log(
        "INFO",
        "session_established",
        "device session persisted (scope record and family id redacted)",
    );
    Ok(server_agent_slug)
}

/// 设备码第一步：创建待授权会话。前端用返回的 `verification_uri_complete`
/// 经 `open_authorization_page` 打开浏览器授权页，并提示用户核对 user_code。
#[tauri::command]
pub async fn begin_device_login(
    gateway_url: String,
    state: State<'_, AppState>,
) -> Result<crate::yuxi::DeviceCodeStart, CommandError> {
    let gateway = validate_gateway_url(&gateway_url).map_err(CommandError::from)?;
    state
        .yuxi
        .start_cli_session(&gateway, None)
        .await
        .map_err(CommandError::from)
}

/// 设备码轮询结果：`pending` 表示等待浏览器授权（前端按 interval 继续轮询）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceLoginPoll {
    pub status: String,
    pub settings: Option<PublicSettings>,
}

/// 设备码轮询兑换（P1 自服务会话路径）。兑换成功后：
/// 1. 会话对走 `persist_device_session` 首次落盘；
/// 2. **立即撤销服务端同时签发的 90 天过渡静态 Key**（用新会话令牌调用，
///    404 幂等；失败仅 WARN 不阻断登录）——保证任何账号都不会同时持有
///    「活会话 + 活静态 Key」，让 fail-closed 在结构上闭合；
/// 3. 旧服务端无 session 字段时回退过渡 Key（静态 Key 账号路径）。
#[tauri::command]
pub async fn poll_device_login(
    mut device_code: String,
    gateway_url: String,
    state: State<'_, AppState>,
) -> Result<DeviceLoginPoll, CommandError> {
    let result = poll_device_login_inner(&device_code, &gateway_url, &state).await;
    device_code.zeroize();
    result.map_err(CommandError::from)
}

async fn poll_device_login_inner(
    device_code: &str,
    gateway_url: &str,
    state: &AppState,
) -> AppResult<DeviceLoginPoll> {
    let gateway = validate_gateway_url(gateway_url)?;
    let secret = SecretString::from(device_code.trim().to_owned());
    let Some(exchange) = state.yuxi.poll_cli_session_token(&gateway, &secret).await? else {
        return Ok(DeviceLoginPoll {
            status: "pending".into(),
            settings: None,
        });
    };
    match &exchange.session {
        Some(session) => {
            let label = device_label(&exchange.user_name);
            let server_agent_slug = persist_device_session(
                state,
                &gateway,
                &exchange.account_scope_id,
                &exchange.user_name,
                session,
                &label,
            )
            .await?;
            if let Some(key_id) = exchange.transition_key_id {
                let bearer = SecretString::from(session.access_token.clone());
                if let Err(error) = state
                    .yuxi
                    .delete_api_key_by_id(&gateway, &bearer, key_id)
                    .await
                {
                    diagnostics::log(
                        "WARN",
                        "transition_key_revoke_failed",
                        &format!("api_key_id={key_id}: {error}"),
                    );
                }
            }
            Ok(DeviceLoginPoll {
                status: "done".into(),
                settings: Some(PublicSettings {
                    gateway_url: gateway,
                    agent_slug: server_agent_slug,
                    has_api_key: true,
                    api_key_hint: None,
                }),
            })
        }
        None => {
            // 旧服务端未签发会话对：过渡 Key 即主凭据，按静态 Key 账号落盘。
            let secret = exchange
                .transition_key_secret
                .as_deref()
                .map(SecretString::from)
                .ok_or(AppError::Protocol(
                    "服务端既未签发会话也未返回过渡密钥".into(),
                ))?;
            validate_api_key(secret.expose_secret())?;
            let hint = api_key_hint(secret.expose_secret());
            persist_local_connection(
                state,
                &gateway,
                &exchange.account_scope_id,
                &hint,
                Some(device_label(&exchange.user_name).as_str()),
                &secret,
            )
            .await?;
            let local_scope = local_account_scope(&gateway, &exchange.account_scope_id);
            if let Err(error) = state
                .database
                .upsert_account(&local_scope, &exchange.user_name, &gateway)
                .await
            {
                diagnostics::log("WARN", "desktop_account_upsert_failed", &error.to_string());
            }
            // 旧服务端路径不撤销过渡 Key：没有会话对时它就是该账号的主凭据。
            let server_agent_slug = refresh_server_agent_slug(state, &gateway, &secret).await?;
            Ok(DeviceLoginPoll {
                status: "done".into(),
                settings: Some(PublicSettings {
                    gateway_url: gateway,
                    agent_slug: server_agent_slug,
                    has_api_key: true,
                    api_key_hint: Some(hint),
                }),
            })
        }
    }
}

/// 打开设备码浏览器授权页。不直接放行任意 URL：仅接受 HTTPS（本机调试允许
/// loopback HTTP）、禁止内嵌凭证，把 opener 权限收敛在 Rust 侧校验之后。
#[tauri::command]
pub async fn open_authorization_page(
    url: String,
    app: tauri::AppHandle,
) -> Result<(), CommandError> {
    let parsed = url::Url::parse(url.trim())
        .map_err(|_| CommandError::from(AppError::Protocol("授权页地址格式无效".into())))?;
    let host = parsed
        .host_str()
        .unwrap_or_default()
        .trim_start_matches('[')
        .trim_end_matches(']');
    let loopback = matches!(host, "127.0.0.1" | "localhost" | "::1");
    let allowed = parsed.scheme() == "https" || (parsed.scheme() == "http" && loopback);
    if !allowed
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.fragment().is_some()
    {
        return Err(CommandError::from(AppError::Protocol(
            "授权页地址必须是 HTTPS（本机调试除外）且不携带凭据".into(),
        )));
    }
    app.opener()
        .open_url(parsed.to_string(), None::<&str>)
        .map_err(|error| CommandError::from(AppError::Internal(error.to_string())))
}

// ==== P5 自省：配额 / 用量 / 设备会话管理 ====

#[tauri::command]
pub async fn get_user_quota(
    state: State<'_, AppState>,
) -> Result<crate::yuxi::QuotaSummary, CommandError> {
    let gateway_url = state
        .database
        .gateway_url()
        .await
        .map_err(CommandError::from)?;
    let bearer = ensure_active_bearer(&state)
        .await
        .map_err(CommandError::from)?;
    state
        .yuxi
        .get_user_quota(&gateway_url, &bearer)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn get_user_usage(
    days: Option<u32>,
    state: State<'_, AppState>,
) -> Result<crate::yuxi::UsageSummary, CommandError> {
    let days = days.unwrap_or(14).clamp(1, 90);
    let gateway_url = state
        .database
        .gateway_url()
        .await
        .map_err(CommandError::from)?;
    let bearer = ensure_active_bearer(&state)
        .await
        .map_err(CommandError::from)?;
    state
        .yuxi
        .get_user_usage(&gateway_url, &bearer, days)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn list_auth_sessions(
    state: State<'_, AppState>,
) -> Result<Vec<crate::yuxi::DeviceSessionView>, CommandError> {
    let gateway_url = state
        .database
        .gateway_url()
        .await
        .map_err(CommandError::from)?;
    let bearer = ensure_active_bearer(&state)
        .await
        .map_err(CommandError::from)?;
    state
        .yuxi
        .list_device_sessions(&gateway_url, &bearer)
        .await
        .map_err(CommandError::from)
}

/// 远程下线指定设备会话族；若下线的是本机会话，同步清除本地 blob 并引导重登。
#[tauri::command]
pub async fn revoke_auth_session(
    family_id: String,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    let family_id = family_id.trim().to_string();
    if family_id.is_empty() || family_id.len() > 128 {
        return Err(CommandError::from(AppError::Protocol(
            "会话标识格式无效".into(),
        )));
    }
    let gateway_url = state
        .database
        .gateway_url()
        .await
        .map_err(CommandError::from)?;
    let bearer = ensure_active_bearer(&state)
        .await
        .map_err(CommandError::from)?;
    state
        .yuxi
        .revoke_device_session(&gateway_url, &bearer, &family_id)
        .await
        .map_err(CommandError::from)?;
    let scope = state
        .database
        .current_account_scope()
        .await
        .unwrap_or_default();
    let matches_local_session = state
        .credentials
        .session_blob(&scope)
        .ok()
        .flatten()
        .and_then(|blob| serde_json::from_str::<crate::session::StoredSession>(&blob).ok())
        .is_some_and(|stored| stored.family_id == family_id);
    if matches_local_session && let Err(error) = state.credentials.delete_scope_records(&scope) {
        diagnostics::log("ERROR", "local_session_cleanup_failed", &error.to_string());
    }
    Ok(())
}

/// 多账号机器的旧历史人工认领：把 `legacy` 作用域的会话归入当前账号。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyClaimResult {
    pub claimed_threads: u64,
    pub claimed_messages: u64,
}

#[tauri::command]
pub async fn claim_legacy_history(
    state: State<'_, AppState>,
) -> Result<LegacyClaimResult, CommandError> {
    let scope = state
        .database
        .current_account_scope()
        .await
        .map_err(CommandError::from)?;
    if !principal_of_scope(&scope).starts_with("yxacct_") {
        return Err(CommandError::from(AppError::Protocol(
            "当前账号不是服务端权威账号，无法认领旧历史".into(),
        )));
    }
    state
        .database
        .claim_legacy_scope(&scope)
        .await
        .map(|(threads, messages)| LegacyClaimResult {
            claimed_threads: threads,
            claimed_messages: messages,
        })
        .map_err(CommandError::from)
}

/// Stronghold、账号目录和会话统一使用「规范网关|服务端账号作用域」。
/// 兼容已经采用该格式的记录，避免账号切换时重复拼接网关。
fn local_account_scope(gateway_url: &str, principal: &str) -> String {
    let gateway = gateway_url.trim_end_matches('/');
    let prefix = format!("{gateway}|");
    if principal.starts_with(&prefix) {
        principal.to_owned()
    } else {
        format!("{prefix}{principal}")
    }
}

/// SQLite::activate_account 仍接收服务端原始 principal；切换新格式账号时先解包。
fn remote_principal_for_scope<'a>(gateway_url: &str, account_scope: &'a str) -> &'a str {
    let gateway = gateway_url.trim_end_matches('/');
    account_scope
        .strip_prefix(&format!("{gateway}|"))
        .unwrap_or(account_scope)
}

async fn refresh_server_agent_slug(
    state: &AppState,
    gateway_url: &str,
    bearer: &SecretString,
) -> AppResult<String> {
    let slug = state.yuxi.default_agent_slug(gateway_url, bearer).await?;
    state.database.save_server_agent_slug(&slug).await?;
    Ok(slug)
}

/// P2b：返回当前应使用的 Bearer 凭证。
///
/// 会话访问令牌仍有效时优先使用；临近过期（<120s）经**按作用域单飞锁**旋转一次
/// ——服务端对已消费的刷新令牌判重放并撤销整个会话族，并发刷新会自己把自己
/// 踢下线；等待者拿到锁后**双重检查**（重读 blob、重解析 exp），只复用新令牌。
///
/// P5 fail-closed：会话型账号（存在会话 blob）在旋转失败时**禁止回退静态 API Key**
/// ——管理员撤销设备后静态 Key 不能成为旁路。刷新令牌终态死亡（401 族）时清除
/// 该作用域的会话 blob 并返回 SessionRequiresRelogin 引导重新登录；网络类瞬态
/// 失败（超时/5xx）原样返回可重试错误，不清 blob。仅"无任何会话记录"的传统
/// 手动 Key 账号继续走 api_key() 路径。
pub(crate) async fn ensure_active_bearer(state: &AppState) -> AppResult<SecretString> {
    use crate::session::{StoredSession, parse_jwt_exp};

    let gateway = state.database.gateway_url().await?;
    let scope = state.database.current_account_scope().await?;
    if state.credentials.session_blob(&scope)?.is_none() {
        return state.credentials.api_key();
    }
    if session_access_still_valid(state, &scope) {
        return session_access_token(state, &scope);
    }

    // 单飞：同作用域并发刷新只有一个任务真正发起 HTTP 轮换。
    let lock = state.session_refresh_lock(&scope)?;
    let _guard = lock.lock().await;
    // 双重检查：拿到锁时其他任务可能已完成轮换，直接复用新令牌。
    if session_access_still_valid(state, &scope) {
        return session_access_token(state, &scope);
    }

    let (stored, refresh_secret) = match load_stored_session(state, &scope) {
        Ok(stored) => (
            stored.clone(),
            SecretString::from(stored.refresh_token.clone()),
        ),
        Err(error) => return Err(error),
    };
    let rotated = match state
        .yuxi
        .refresh_cli_session(&gateway, &refresh_secret)
        .await
    {
        Ok(rotated) => rotated,
        Err(error) => {
            diagnostics::log("WARN", "session_refresh_failed", &error.to_string());
            // 刷新端点的 401 一律终态（invalid_grant/revoked/reuse/过期共用，
            // detail 为纯字符串没有结构化 code）：清除死 blob，让 has_session
            // 归 false、前端自然回落连接设置；网络类瞬态失败原样透传。
            let terminal = error.is_session_terminal() || matches!(error, AppError::Unauthorized);
            if terminal {
                if let Err(cleanup_error) = state.credentials.delete_scope_records(&scope) {
                    diagnostics::log(
                        "ERROR",
                        "session_blob_cleanup_failed",
                        &cleanup_error.to_string(),
                    );
                }
                return Err(AppError::SessionRequiresRelogin);
            }
            return Err(error);
        }
    };

    let now = chrono::Utc::now().timestamp();
    let new_expires = parse_jwt_exp(&rotated.access_token).unwrap_or(now + 30 * 60);
    let updated = StoredSession {
        access_token: rotated.access_token.clone(),
        refresh_token: rotated.refresh_token,
        family_id: stored.family_id,
        access_expires_at: new_expires,
        account_scope_id: stored.account_scope_id,
    };
    match serde_json::to_string(&updated) {
        Ok(json) => {
            if let Err(save_error) = state.credentials.save_session_blob(&scope, &json) {
                // 轮换已发生但本地无法固化新刷新令牌：保留旧 blob 会在下次用已
                // 消费的令牌重放，必须清除并要求重新登录。
                diagnostics::log(
                    "ERROR",
                    "session_blob_write_failed",
                    &save_error.to_string(),
                );
                let _ = state.credentials.delete_scope_records(&scope);
                return Err(AppError::SessionRequiresRelogin);
            }
        }
        Err(error) => {
            diagnostics::log("ERROR", "session_blob_serialize_failed", &error.to_string());
            let _ = state.credentials.delete_scope_records(&scope);
            return Err(AppError::SessionRequiresRelogin);
        }
    }
    Ok(SecretString::from(rotated.access_token))
}

/// 读取并解析当前作用域的会话 blob；损坏视同需要重新登录。
fn load_stored_session(state: &AppState, scope: &str) -> AppResult<crate::session::StoredSession> {
    let blob = state
        .credentials
        .session_blob(scope)?
        .ok_or(AppError::SessionRequiresRelogin)?;
    serde_json::from_str(&blob).map_err(|error| {
        diagnostics::log("ERROR", "session_blob_corrupted", &error.to_string());
        AppError::SessionRequiresRelogin
    })
}

fn session_access_still_valid(state: &AppState, scope: &str) -> bool {
    let Ok(stored) = load_stored_session(state, scope) else {
        return false;
    };
    let now = chrono::Utc::now().timestamp();
    let expires_at =
        crate::session::parse_jwt_exp(&stored.access_token).unwrap_or(stored.access_expires_at);
    expires_at.saturating_sub(now) > 120
}

fn session_access_token(state: &AppState, scope: &str) -> AppResult<SecretString> {
    Ok(SecretString::from(
        load_stored_session(state, scope)?.access_token,
    ))
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
    // 会话感知：纯会话账号（无静态 Key）也必须能通过连接测试。
    let bearer = ensure_active_bearer(&state)
        .await
        .map_err(CommandError::from)?;
    state
        .yuxi
        .test_connection(&gateway_url, agent_slug(), &bearer)
        .await
        .map_err(CommandError::from)?;
    refresh_server_agent_slug(&state, &gateway_url, &bearer)
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
    // 登出时同时清掉当前作用域的 Key/会话记录，防止残留凭据串到下次登录。
    let scope = state
        .database
        .current_account_scope()
        .await
        .map_err(CommandError::from)?;
    if !scope.is_empty()
        && scope != "legacy"
        && let Err(error) = state.credentials.delete_scope_records(&scope)
    {
        diagnostics::log("WARN", "scope_records_cleanup_failed", &error.to_string());
    }
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
    let mut threads = state
        .database
        .list_threads()
        .await
        .map_err(CommandError::from)?;
    // Legacy rows persisted before reasoning redaction may still contain
    // chain-of-thought inside messages.content, which feeds this preview.  The
    // sidebar preview must never surface it.
    for thread in &mut threads {
        thread.preview = sanitize_visible_model_text(&thread.preview);
    }
    Ok(threads)
}

#[tauri::command]
pub async fn load_messages(
    thread_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<LocalMessage>, CommandError> {
    let mut messages = state
        .database
        .load_messages(&thread_id)
        .await
        .map_err(CommandError::from)?;
    for message in &mut messages {
        if message.role == "assistant" {
            message.content = sanitize_visible_model_text(&message.content);
        }
    }
    Ok(messages)
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

/// 获取指定 run 的执行轨迹快照；供 UI 在终态后或重启恢复时渲染权威投影。
#[tauri::command]
pub async fn get_run_trace(
    run_id: String,
    state: State<'_, AppState>,
) -> Result<Value, CommandError> {
    let run_id = run_id.trim().to_string();
    if run_id.is_empty() {
        return Err(CommandError::from(AppError::Protocol(
            "run_id 不能为空".into(),
        )));
    }
    let gateway_url = state
        .database
        .gateway_url()
        .await
        .map_err(CommandError::from)?;
    let bearer = ensure_active_bearer(&state)
        .await
        .map_err(CommandError::from)?;
    state
        .yuxi
        .trace_snapshot(&gateway_url, &bearer, &run_id)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn get_run_trace_events(
    run_id: String,
    after_sequence: u64,
    state: State<'_, AppState>,
) -> Result<Value, CommandError> {
    let run_id = run_id.trim().to_string();
    if run_id.is_empty() {
        return Err(CommandError::from(AppError::Protocol(
            "run_id 不能为空".into(),
        )));
    }
    let gateway_url = state
        .database
        .gateway_url()
        .await
        .map_err(CommandError::from)?;
    let bearer = ensure_active_bearer(&state)
        .await
        .map_err(CommandError::from)?;
    state
        .yuxi
        .trace_events(&gateway_url, &bearer, &run_id, after_sequence)
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
    let api_key = ensure_active_bearer(state).await?;
    let mut summary = PendingRunSync {
        recovered: 0,
        pending: 0,
        failed: 0,
        last_error: None,
    };
    for pending_run in pending_runs {
        let result = match state
            .yuxi
            .result(&gateway_url, &api_key, &pending_run.run_id)
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
                if !result.output.trim().is_empty() {
                    state
                        .database
                        .append_message(
                            &format!("assistant-{}", pending_run.run_id),
                            &pending_run.thread_id,
                            "assistant",
                            &result.output,
                        )
                        .await?;
                }
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
    let local_attachments = request
        .attachments
        .iter()
        .map(|attachment| LocalMessageAttachment {
            id: attachment.tmp_file_id.clone(),
            name: attachment.file_name.clone(),
            content_type: attachment.file_type.clone(),
            file_size: attachment.file_size,
        })
        .collect::<Vec<_>>();
    let local_attachments_json = serde_json::to_string(&local_attachments)
        .map_err(|error| AppError::Internal(format!("附件元数据无法序列化: {error}")))?;
    state
        .database
        .append_message_with_attachments(
            &format!("user-{}", request.request_id),
            &request.thread_id,
            "user",
            question,
            &local_attachments_json,
        )
        .await?;

    let gateway_url = state.database.gateway_url().await?;
    let api_key = ensure_active_bearer(state).await?;
    let existing_yuxi_thread_id = state.database.yuxi_thread_id(&request.thread_id).await?;
    let (yuxi_thread_id, run_agent_slug) = if let Some(yuxi_thread_id) = existing_yuxi_thread_id {
        let bound_agent_slug = state.database.thread_agent_slug(&request.thread_id).await?;
        (yuxi_thread_id, bound_agent_slug)
    } else {
        let server_agent_slug = refresh_server_agent_slug(state, &gateway_url, &api_key).await?;
        let title = question.chars().take(80).collect::<String>();
        let server_thread = state
            .yuxi
            .create_thread(
                &gateway_url,
                &api_key,
                &server_agent_slug,
                &request.thread_id,
                &title,
            )
            .await?;
        state
            .database
            .bind_server_thread(
                &request.thread_id,
                &server_thread.id,
                &server_thread.agent_id,
            )
            .await?;
        (server_thread.id, server_thread.agent_id)
    };
    let attachment_file_ids = state
        .yuxi
        .confirm_tmp_attachments(
            &gateway_url,
            &api_key,
            &yuxi_thread_id,
            &request.attachments,
        )
        .await?;
    // 续跑（人工审批/澄清问题）：query 省略，用户答复以 resume 载荷发送；
    // 沿用父 run 冻结模型，不携带请求级 model_spec。澄清问题的答复组装为
    // {question_id: 答案}（与 Web 端同构），人工审批仍是纯字符串。
    let resume_payload = match request.resume_run_id.as_deref() {
        Some(parent_run_id) if !parent_run_id.trim().is_empty() => {
            let answer = if request.resume_answers.is_empty() {
                Value::String(question.to_owned())
            } else {
                build_resume_payload(&request.resume_questions, &request.resume_answers)?
            };
            Some((answer, parent_run_id.trim().to_owned()))
        }
        _ => None,
    };
    let created = tokio::select! {
        _ = cancellation.cancelled() => return Err(AppError::Cancelled),
        result = state.yuxi.create_run_with_resume(
            &gateway_url,
            &run_agent_slug,
            &api_key,
            if resume_payload.is_some() { None } else { Some(question) },
            &yuxi_thread_id,
            &request.request_id,
            None,
            &attachment_file_ids,
            resume_payload.as_ref().map(|(answer, _)| answer),
            resume_payload.as_ref().map(|(_, parent)| parent.as_str()),
        ) => result?,
    };
    state.set_request_run_id(&request.request_id, &created.run_id)?;

    if created.thread_id != yuxi_thread_id {
        return Err(AppError::Protocol(
            "服务端 run 返回的线程与已绑定会话不一致".into(),
        ));
    }
    if created.request_id != request.request_id {
        return Err(AppError::Protocol(
            "服务端 run 返回的 request_id 与桌面请求不一致".into(),
        ));
    }
    validate_authoritative_run_context(
        &created.run_context,
        &run_agent_slug,
        &yuxi_thread_id,
        &request.request_id,
    )?;
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
    let mut error_frame_received = false;
    // 帧级观测：未知命名事件、未知 custom 名与非 JSON 帧的计数进入会话摘要
    // 日志，用于发现服务端新增而桌面端尚未消费的事件（静默忽略 = 不可诊断）。
    let mut frames_total: u64 = 0;
    let mut frames_unknown_event: u64 = 0;
    let mut frames_unknown_custom: u64 = 0;
    let mut frames_skipped_non_json: u64 = 0;
    let mut attempts_used: u32 = 0;
    // 证据平面收集器：本 run 实际观测到的权威投影，终态时随助手消息落库。
    let mut run_evidence = MessageEvidence::default();

    for attempt in 0..4 {
        attempts_used = attempt + 1;
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
                next = tokio::time::timeout(Duration::from_secs(45), stream.next()) => match next {
                    Ok(next) => next,
                    Err(_) => {
                        send_channel(
                            on_event,
                            RunEvent::Status {
                                status: "polling".into(),
                                message: "事件流暂时没有新数据，正在核对服务端任务状态…".into(),
                            },
                        )?;
                        break;
                    }
                },
            };
            match next {
                Some(Ok(event)) => {
                    frames_total += 1;
                    if !event.id.is_empty() {
                        last_event_id = Some(event.id.clone());
                    }
                    // 心跳或代理注入的非 JSON 帧只跳过该帧，不终止整次对话；
                    // 终态一致性由事件流与结果轮询共同兜底。
                    let value = match serde_json::from_str::<Value>(&event.data) {
                        Ok(value) => value,
                        Err(error) => {
                            frames_skipped_non_json += 1;
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
                    // 白名单外的命名事件：不中断，但必须可见——这是服务端
                    // 新增事件未被消费时的唯一诊断信号。
                    if !event.event.is_empty() && !is_known_sse_event(&event.event) {
                        frames_unknown_event += 1;
                        diagnostics::log(
                            "WARN",
                            "sse_event_unknown",
                            &format!(
                                "run={}: event={} keys={}",
                                created.run_id,
                                event.event,
                                value
                                    .as_object()
                                    .map(|object| object
                                        .keys()
                                        .cloned()
                                        .collect::<Vec<_>>()
                                        .join(","))
                                    .unwrap_or_default()
                            ),
                        );
                    }
                    // custom 帧二级白名单：语义名在 payload.name，一级白名单
                    // 恒命中 custom，未知 custom 名单独计数（否则新增证据类
                    // 事件会被静默丢弃且无告警）。
                    let custom_event_name = if event.event == "custom" {
                        value
                            .pointer("/payload/name")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned()
                    } else {
                        String::new()
                    };
                    if event.event == "custom" && !is_known_custom_event(&custom_event_name) {
                        frames_unknown_custom += 1;
                        diagnostics::log(
                            "WARN",
                            "sse_custom_unknown",
                            &format!(
                                "run={}: custom={} keys={}",
                                created.run_id,
                                custom_event_name,
                                value
                                    .pointer("/payload/chunk")
                                    .and_then(Value::as_object)
                                    .map(|object| object
                                        .keys()
                                        .cloned()
                                        .collect::<Vec<_>>()
                                        .join(","))
                                    .unwrap_or_default()
                            ),
                        );
                    }
                    // 执行轨迹帧（SSE event: trace）：协议 ≤1.3 的遗留兼容路径
                    //（1.4 起服务端已从消息流剔除，改走 /trace 端点轮询）。
                    // 整帧透传给前端投影，不参与文本累积与终态判定。
                    if event.event == "trace" {
                        if let Some(trace) = extract_run_trace_event(&value, &created.run_id) {
                            send_channel(
                                on_event,
                                RunEvent::Trace {
                                    run_id: created.run_id.clone(),
                                    trace,
                                },
                            )?;
                        }
                        continue;
                    }
                    // 澄清问题中断（ask_user_question_required）：透传问题给
                    // UI 渲染选项卡片；run 稍后以 end{status:interrupted} 收尾，
                    // 用户答复经 SendMessageRequest.resume_* 续跑。
                    if belongs_to_parent_thread
                        && let Some(questions) = parse_interrupt_questions(&value)
                    {
                        send_channel(
                            on_event,
                            RunEvent::AskUserQuestion {
                                run_id: created.run_id.clone(),
                                questions,
                            },
                        )?;
                    }
                    // 证据平面（custom 帧分发）：citation/图卡/候选清单/运行提示。
                    // 仅消费服务端确定性投影，不参与文本累积与终态判定。
                    if belongs_to_parent_thread && event.event == "custom" {
                        match custom_event_name.as_str() {
                            "yuxi.citation_ready" => {
                                if let Some((citation, figures, figure_refs, tables)) =
                                    parse_citation_ready(&value)
                                {
                                    // 取图硬前置（Rust 侧 fail-closed 观测）：缺键图卡不阻断
                                    // 事件下发，前端按题注降级渲染；这里只保证方法在生产路径
                                    // 被调用，不猜测补全。
                                    for figure in &figures {
                                        if !figure.is_fetchable() {
                                            diagnostics::log(
                                                "DEBUG",
                                                "figure_not_fetchable",
                                                &format!(
                                                    "run={}: asset={}",
                                                    created.run_id,
                                                    figure
                                                        .asset_name
                                                        .as_deref()
                                                        .unwrap_or_default()
                                                ),
                                            );
                                        }
                                    }
                                    send_channel(
                                        on_event,
                                        RunEvent::CitationsReady {
                                            run_id: created.run_id.clone(),
                                            citation: citation.clone(),
                                            figures: figures.clone(),
                                            figure_refs: figure_refs.clone(),
                                            tables: tables.clone(),
                                        },
                                    )?;
                                    run_evidence.citations.push(citation);
                                    run_evidence.figures.extend(figures);
                                    run_evidence.figure_refs.extend(figure_refs);
                                    run_evidence.tables.extend(tables);
                                }
                            }
                            "yuxi.locator_candidates" => {
                                if let Some(candidates) = parse_locator_candidates(&value) {
                                    send_channel(
                                        on_event,
                                        RunEvent::LocatorCandidates {
                                            run_id: created.run_id.clone(),
                                            candidates: candidates.clone(),
                                        },
                                    )?;
                                    run_evidence.locator_candidates.extend(candidates);
                                }
                            }
                            "yuxi.graph_snapshot_ready" => {
                                if let Some(snapshot) = parse_graph_snapshot(&value) {
                                    send_channel(
                                        on_event,
                                        RunEvent::GraphSnapshotReady {
                                            run_id: created.run_id.clone(),
                                            snapshot: snapshot.clone(),
                                        },
                                    )?;
                                    run_evidence.graph_snapshots.push(snapshot);
                                }
                            }
                            "yuxi.warning" => {
                                if let Some(warning) = parse_custom_warning(&value) {
                                    send_channel(
                                        on_event,
                                        RunEvent::Warning {
                                            message: warning.clone(),
                                        },
                                    )?;
                                    run_evidence.warnings.push(warning);
                                }
                            }
                            "yuxi.followup_suggestions" => {
                                if let Some(suggestions) = parse_followup_suggestions(&value) {
                                    send_channel(
                                        on_event,
                                        RunEvent::FollowupSuggestions {
                                            run_id: created.run_id.clone(),
                                            suggestions: suggestions.clone(),
                                        },
                                    )?;
                                    run_evidence.followup_suggestions = suggestions;
                                }
                            }
                            _ => {}
                        }
                    }
                    if belongs_to_parent_thread && let Some(message) = run_progress_message(&value)
                    {
                        send_channel(
                            on_event,
                            RunEvent::Status {
                                status: "running".into(),
                                message: message.into(),
                            },
                        )?;
                    }
                    if belongs_to_parent_thread
                        && let Some(message) = compression_status_message(&value)
                    {
                        send_channel(
                            on_event,
                            RunEvent::Status {
                                status: "running".into(),
                                message: message.into(),
                            },
                        )?;
                    }
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
                        // error 帧不再盲目重连：转 wait_for_result 以结果端点
                        // （权威）裁决终态，避免对已失败任务叠加 4 次重连。
                        error_frame_received = true;
                        break;
                    }
                }
                Some(Err(_)) | None => break,
            }
        }
        if terminal_received || error_frame_received {
            break;
        }
    }

    // 帧级观测摘要：未知事件/未知 custom 名计数 >0 即说明服务端契约超前于
    // 桌面端消费面。
    diagnostics::log(
        "INFO",
        "sse_stream_summary",
        &format!(
            "run={}: frames={} attempts={} unknown_events={} unknown_custom={} skipped_non_json={} terminal={} error_frame={} evidence=(citations:{} figures:{} candidates:{} warnings:{})",
            created.run_id,
            frames_total,
            attempts_used,
            frames_unknown_event,
            frames_unknown_custom,
            frames_skipped_non_json,
            terminal_received,
            error_frame_received,
            run_evidence.citations.len(),
            run_evidence.figures.len(),
            run_evidence.locator_candidates.len(),
            run_evidence.warnings.len()
        ),
    );

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
    validate_authoritative_run_context(
        &final_result.context,
        &run_agent_slug,
        &yuxi_thread_id,
        &request.request_id,
    )?;
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
                .append_message_with_evidence(
                    &format!("assistant-{}", created.run_id),
                    &request.thread_id,
                    "assistant",
                    &final_text,
                    &run_evidence,
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
            if final_result.status == "interrupted" {
                // 可续跑态（人工审批等待）：不是错误。已流式产出的内容先落库，
                // 向 UI 发 Done(status=interrupted)，前端呈现「继续运行」入口，
                // 用户答复经 SendMessageRequest.resume_run_id 以 resume 续跑。
                let persisted_text = if final_text.trim().is_empty() {
                    accumulated.clone()
                } else {
                    final_text.clone()
                };
                if !persisted_text.trim().is_empty() {
                    state
                        .database
                        .append_message_with_evidence(
                            &format!("assistant-{}", created.run_id),
                            &request.thread_id,
                            "assistant",
                            &persisted_text,
                            &run_evidence,
                        )
                        .await?;
                }
                state
                    .database
                    .update_run_progress(
                        &created.run_id,
                        "interrupted",
                        last_event_id.as_deref(),
                        &persisted_text,
                        Some("awaiting_approval"),
                        true,
                    )
                    .await?;
                send_channel(
                    on_event,
                    RunEvent::Done {
                        run_id: created.run_id.clone(),
                        status: "interrupted".into(),
                        text: persisted_text.clone(),
                        context: Box::new(context.clone()),
                    },
                )?;
                return Ok(ChatCompletion {
                    run_id: created.run_id,
                    thread_id: created.thread_id,
                    request_id: created.request_id,
                    status: "interrupted".into(),
                    text: persisted_text,
                    context,
                });
            }
            let message = final_result
                .error
                .unwrap_or_else(|| "Agent 运行失败".into());
            let error = if is_reasoning_protocol_failure(&message) {
                AppError::ServerUpgradeRequired
            } else {
                AppError::Protocol(message)
            };
            let persisted_text = if matches!(&error, AppError::ServerUpgradeRequired) {
                ""
            } else if final_text.trim().is_empty() {
                &accumulated
            } else {
                &final_text
            };
            // 服务端把 run 标记为 failed/interrupted 时，答案可能已经流式输出完毕
            //（例如服务端收尾清理抛异常污染了终态）。只要实际产生过非空回答，
            // 先按幂等 id 落库为助手消息再返回错误——保证"任何已完成回答都先
            // 持久化再切换"。append_message 对同 id 是 upsert，后续
            // sync_pending_runs 对账不会产生重复消息。
            if !persisted_text.trim().is_empty() {
                state
                    .database
                    .append_message_with_evidence(
                        &format!("assistant-{}", created.run_id),
                        &request.thread_id,
                        "assistant",
                        persisted_text,
                        &run_evidence,
                    )
                    .await?;
            }
            state
                .database
                .update_run_progress(
                    &created.run_id,
                    &final_result.status,
                    last_event_id.as_deref(),
                    persisted_text,
                    final_result.error_code.as_deref().or(Some(&error.code())),
                    true,
                )
                .await?;
            if !persisted_text.trim().is_empty() {
                send_channel(
                    on_event,
                    RunEvent::Done {
                        run_id: created.run_id.clone(),
                        status: final_result.status.clone(),
                        text: persisted_text.to_owned(),
                        context: Box::new(context),
                    },
                )?;
            }
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
        let api_key = ensure_active_bearer(&state)
            .await
            .map_err(CommandError::from)?;
        state
            .yuxi
            .cancel_run(&gateway_url, &api_key, &run_id)
            .await
            .map_err(CommandError::from)?;
    }
    Ok(())
}

/// 当前网关的服务端协议能力快照（渲染分支的能力位真源）。按网关地址缓存，
/// 未协商过则现场探测一次；旧服务端（无该端点）返回 None，前端按能力缺席
/// 降级——不渲染图卡等增强面，基础问答不受影响。
#[tauri::command]
pub async fn get_protocol_capabilities(
    state: State<'_, AppState>,
) -> Result<Option<ProtocolInfo>, CommandError> {
    let gateway_url = state
        .database
        .gateway_url()
        .await
        .map_err(CommandError::from)?;
    if let Some(cached) = state.cached_protocol_info(&gateway_url) {
        return Ok(Some(cached));
    }
    let bearer = ensure_active_bearer(&state)
        .await
        .map_err(CommandError::from)?;
    let info = state
        .yuxi
        .fetch_protocol_info(&gateway_url, &bearer)
        .await
        .map_err(CommandError::from)?;
    if let Some(snapshot) = &info {
        state.set_protocol_info(&gateway_url, snapshot.clone());
    }
    Ok(info)
}

/// 图卡资产字节（base64 通道）：路径段经校验编码，MIME 走服务端同源白名单，
/// 前端拼 data URL 渲染；资产字节不落盘，随消息生命周期驻留内存。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FigureAssetPayload {
    pub media_type: String,
    pub data_base64: String,
}

#[tauri::command]
pub async fn get_figure_asset(
    kb_id: String,
    file_id: String,
    revision_id: String,
    asset_name: String,
    state: State<'_, AppState>,
) -> Result<FigureAssetPayload, CommandError> {
    let gateway_url = state
        .database
        .gateway_url()
        .await
        .map_err(CommandError::from)?;
    let bearer = ensure_active_bearer(&state)
        .await
        .map_err(CommandError::from)?;
    let (media_type, bytes) = state
        .yuxi
        .fetch_knowledge_asset(
            &gateway_url,
            &bearer,
            &kb_id,
            &file_id,
            &revision_id,
            &asset_name,
        )
        .await
        .map_err(CommandError::from)?;
    Ok(FigureAssetPayload {
        media_type,
        data_base64: BASE64_STANDARD.encode(bytes),
    })
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
            result = state.yuxi.result(gateway_url, api_key, run_id) => result?,
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
    let is_resume = request
        .resume_run_id
        .as_deref()
        .is_some_and(|run_id| !run_id.trim().is_empty());
    if request.question.trim().is_empty() || request.question.chars().count() > 20_000 {
        return Err(AppError::Protocol(
            "问题长度必须为 1 至 20000 个字符".into(),
        ));
    }
    if request.attachments.len() > 6 {
        return Err(AppError::Protocol("每次最多添加 6 个附件".into()));
    }
    if is_resume && !request.attachments.is_empty() {
        return Err(AppError::Protocol("续跑请求不能携带新附件".into()));
    }
    if !request.resume_answers.is_empty() {
        if !is_resume {
            return Err(AppError::Protocol("澄清问题答复只能用于续跑请求".into()));
        }
        if request.resume_questions.is_empty() {
            return Err(AppError::Protocol("澄清问题答复缺少对应的问题定义".into()));
        }
        if request
            .resume_answers
            .iter()
            .any(|answer| answer.question_id.trim().is_empty())
        {
            return Err(AppError::Protocol("澄清问题答复缺少问题标识".into()));
        }
    }
    if is_resume {
        let parent_run_id = request.resume_run_id.as_deref().unwrap_or_default();
        let valid_shape = (1..=128).contains(&parent_run_id.len())
            && parent_run_id
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || "._:-".contains(character));
        if !valid_shape {
            return Err(AppError::Protocol("续跑的父任务标识格式无效".into()));
        }
    }
    if request.attachments.iter().any(|attachment| {
        attachment.tmp_file_id.is_empty()
            || attachment.file_name.is_empty()
            || attachment.bucket_name.is_empty()
            || attachment.object_name.is_empty()
    }) {
        return Err(AppError::Protocol("附件元数据不完整，请重新上传".into()));
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

fn run_progress_message(value: &Value) -> Option<&str> {
    let chunk = value.pointer("/payload/chunk")?;
    (chunk.get("status").and_then(Value::as_str) == Some("progress"))
        .then(|| chunk.get("message").and_then(Value::as_str))
        .flatten()
        .filter(|message| !message.trim().is_empty())
}

/// messages 帧内的上下文压缩状态（chunk.status == "context_compression"）：
/// 压缩期间给用户一个可解释的进度提示；结束语义由后续进度文本自然覆盖
/// （与 Web 端 threadState.contextCompressing 同一事件源）。
fn compression_status_message(value: &Value) -> Option<&'static str> {
    let chunk = value.pointer("/payload/chunk")?;
    if chunk.get("status").and_then(Value::as_str) != Some("context_compression") {
        return None;
    }
    match chunk.pointer("/compression/status").and_then(Value::as_str) {
        Some("started") => Some("正在压缩上下文…"),
        _ => None,
    }
}

/// 从 SSE `event: trace` 帧中提取属于当前 run 的 wire 事件；
/// 形状不符或 run_id 不匹配（防御未来跨 run 混流）时返回 None。
fn extract_run_trace_event(value: &Value, run_id: &str) -> Option<Value> {
    let trace = value.pointer("/payload/trace")?;
    if trace.get("run_id").and_then(Value::as_str) != Some(run_id) {
        return None;
    }
    trace.get("sequence").and_then(Value::as_i64)?;
    Some(trace.clone())
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

#[tauri::command]
pub async fn list_accounts(
    state: State<'_, AppState>,
) -> Result<Vec<crate::database::AccountSummary>, CommandError> {
    let accounts = state
        .database
        .list_accounts()
        .await
        .map_err(CommandError::from)?;
    let active = state
        .database
        .current_account_scope()
        .await
        .unwrap_or_default();
    Ok(accounts
        .into_iter()
        .map(|mut account| {
            account.is_active =
                local_account_scope(&account.gateway_url, &account.account_scope) == active;
            account
        })
        .collect())
}

#[tauri::command]
pub async fn switch_account(
    account_scope: String,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    // 1) 目标账号必须存在于目录中（同时取回网关地址与显示名）
    let accounts = state
        .database
        .list_accounts()
        .await
        .map_err(CommandError::from)?;
    let target = accounts
        .iter()
        .find(|account| account.account_scope == account_scope)
        .cloned()
        .ok_or_else(|| CommandError::from(AppError::MissingCredential))?;

    // 2) 从 Stronghold 取该作用域的凭据：静态 Key 拷贝为 ACTIVE 记录；
    //    纯会话账号没有 Key——清除 ACTIVE 缓存（blob 本身按作用域直读，
    //    ensure_active_bearer 不经过 ACTIVE）。两者都缺才是坏账号。
    let scoped_key = state.credentials.api_key_for_scope(&account_scope)?;
    let has_session = state
        .credentials
        .session_blob(&account_scope)
        .map_err(CommandError::from)?
        .is_some();
    if scoped_key.is_none() && !has_session {
        return Err(CommandError::from(AppError::MissingCredential));
    }
    let hint = scoped_key
        .as_ref()
        .map(|key| api_key_hint(key.expose_secret()))
        .unwrap_or_default();

    let previous_active = if state.credentials.has_api_key()? {
        Some(state.credentials.api_key()?)
    } else {
        None
    };
    match &scoped_key {
        Some(key) => state.credentials.save_api_key(key.expose_secret())?,
        // 切到纯会话账号时清掉残留的旧 ACTIVE Key，避免 has_api_key 误报连接。
        None => state.credentials.delete_api_key()?,
    }
    let remote_principal = remote_principal_for_scope(&target.gateway_url, &account_scope);
    if let Err(database_error) = state
        .database
        .activate_account(
            &target.gateway_url,
            remote_principal,
            &hint,
            None, // 切换不改 Key 名称
        )
        .await
    {
        let rollback = match (previous_active, &scoped_key) {
            (Some(previous), _) => state.credentials.save_api_key(previous.expose_secret()),
            (None, Some(_)) => {
                // 原本无 ACTIVE Key（纯会话账号）却被我们写入了 Key：删除恢复。
                state.credentials.delete_api_key()
            }
            (None, None) => Ok(()),
        };
        if let Err(rollback_error) = rollback {
            diagnostics::log(
                "ERROR",
                "credential_switch_rollback_failed",
                &format!("{database_error}; {rollback_error}"),
            );
        }
        return Err(CommandError::from(database_error));
    }
    Ok(())
}

#[tauri::command]
pub async fn remove_account(
    account_scope: String,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    let active = state
        .database
        .current_account_scope()
        .await
        .unwrap_or_default();
    let selected_scope = state
        .database
        .list_accounts()
        .await
        .map_err(CommandError::from)?
        .into_iter()
        .find(|account| account.account_scope == account_scope)
        .map(|account| local_account_scope(&account.gateway_url, &account.account_scope))
        .unwrap_or_else(|| account_scope.clone());
    if selected_scope == active {
        return Err(CommandError::from(AppError::CredentialStore(
            "不能移除当前登录中的账号，请先切换到其他账号".into(),
        )));
    }
    state.credentials.delete_scope_records(&account_scope)?;
    state.database.delete_account(&account_scope).await?;
    Ok(())
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
    let api_key = ensure_active_bearer(&state)
        .await
        .map_err(CommandError::from)?;
    let models = state
        .yuxi
        .list_chat_models(&gateway_url, &api_key)
        .await
        .map_err(CommandError::from)?;
    Ok(models)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ByokCredentialView {
    pub credential_id: i64,
    pub provider_id: String,
    pub label: String,
    pub masked_hint: String,
    pub status: String,
    pub protocol: Option<String>,
    pub base_url: Option<String>,
    pub model_id: Option<String>,
    pub model_spec: Option<String>,
}

#[tauri::command]
pub async fn list_byok_credentials(
    state: State<'_, AppState>,
) -> Result<Vec<ByokCredentialView>, CommandError> {
    let gateway_url = state
        .database
        .gateway_url()
        .await
        .map_err(CommandError::from)?;
    let bearer = ensure_active_bearer(&state)
        .await
        .map_err(CommandError::from)?;
    let items = state
        .yuxi
        .list_byok_credentials(&gateway_url, &bearer)
        .await
        .map_err(CommandError::from)?;
    Ok(items
        .into_iter()
        .map(|item| ByokCredentialView {
            credential_id: item.credential_id,
            provider_id: item.provider_id,
            label: item.label,
            masked_hint: item.masked_hint,
            status: item.status,
            protocol: item.protocol,
            base_url: item.base_url,
            model_id: item.model_id,
            model_spec: item.model_spec,
        })
        .collect())
}

#[tauri::command]
pub async fn save_byok_credential(
    provider_id: String,
    api_key: String,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    let gateway_url = state
        .database
        .gateway_url()
        .await
        .map_err(CommandError::from)?;
    let bearer = ensure_active_bearer(&state)
        .await
        .map_err(CommandError::from)?;
    state
        .yuxi
        .save_byok_credential(
            &gateway_url,
            &bearer,
            provider_id.trim(),
            &SecretString::from(api_key),
        )
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn save_custom_model_credential(
    protocol: String,
    base_url: String,
    api_key: String,
    model: String,
    state: State<'_, AppState>,
) -> Result<crate::yuxi::ModelConfigurationResult, CommandError> {
    let gateway_url = state
        .database
        .gateway_url()
        .await
        .map_err(CommandError::from)?;
    let bearer = ensure_active_bearer(&state)
        .await
        .map_err(CommandError::from)?;
    state
        .yuxi
        .save_custom_model_credential(
            &gateway_url,
            &bearer,
            protocol.trim(),
            base_url.trim(),
            &SecretString::from(api_key),
            model.trim(),
        )
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn import_model_configuration(
    configuration: String,
    state: State<'_, AppState>,
) -> Result<crate::yuxi::ModelConfigurationResult, CommandError> {
    let gateway_url = state
        .database
        .gateway_url()
        .await
        .map_err(CommandError::from)?;
    let bearer = ensure_active_bearer(&state)
        .await
        .map_err(CommandError::from)?;
    state
        .yuxi
        .import_model_configuration(&gateway_url, &bearer, &SecretString::from(configuration))
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn remove_byok_credential(
    credential_id: i64,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    let gateway_url = state
        .database
        .gateway_url()
        .await
        .map_err(CommandError::from)?;
    let bearer = ensure_active_bearer(&state)
        .await
        .map_err(CommandError::from)?;
    state
        .yuxi
        .delete_byok_credential(&gateway_url, &bearer, credential_id)
        .await
        .map_err(CommandError::from)
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
    let api_key = ensure_active_bearer(&state)
        .await
        .map_err(CommandError::from)?;
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
    let api_key = ensure_active_bearer(&state)
        .await
        .map_err(CommandError::from)?;
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
        AppState, CitationRef, ClarificationQuestion, LocatorCandidate, QuestionAnswer, RunEvent,
        SendMessageRequest, compression_status_message, ensure_active_bearer,
        extract_run_trace_event, is_known_custom_event, is_known_sse_event,
        is_reasoning_protocol_failure, local_account_scope, principal_of_scope,
        remote_principal_for_scope, run_progress_message, validate_send_request,
    };
    use crate::yuxi::{PendingChatAttachment, QuestionOption};
    use secrecy::ExposeSecret as _;
    use serde_json::json;
    use std::sync::Arc;

    #[test]
    fn trace_event_serializes_with_camel_case_wrapper_and_raw_wire_payload() {
        // RunEvent::Trace 外壳字段是 camelCase；trace 载荷是服务端 wire
        // 原样透传（snake_case），前端投影按 wire 字段解析。
        let value = serde_json::to_value(RunEvent::Trace {
            run_id: "run-1".into(),
            trace: json!({
                "schema_version": "yuxi.run-trace.v1",
                "sequence": 7,
                "run_id": "run-1",
                "category": "TOOL",
                "event_type": "tool.execution.completed",
                "span_id": "tool-1",
                "duration_ms": 812
            }),
        })
        .expect("serialize trace");
        assert_eq!(value["type"], "trace");
        assert_eq!(value["runId"], "run-1");
        assert_eq!(value["trace"]["event_type"], "tool.execution.completed");
        assert_eq!(value["trace"]["sequence"], 7);
    }

    #[test]
    fn extract_run_trace_event_filters_by_run_id_and_shape() {
        let frame = json!({
            "run_id": "run-1",
            "event": "trace",
            "payload": {
                "trace": {
                    "sequence": 3,
                    "run_id": "run-1",
                    "category": "MODEL",
                    "event_type": "model.generation.started"
                }
            }
        });
        let extracted = extract_run_trace_event(&frame, "run-1").expect("match run");
        assert_eq!(extracted["sequence"], 3);
        // 其他 run 的 trace 帧（未来跨 run 混流防御）与缺 sequence 的畸形帧都拒绝
        assert!(extract_run_trace_event(&frame, "run-other").is_none());
        let malformed = json!({"payload": {"trace": {"run_id": "run-1"}}});
        assert!(extract_run_trace_event(&malformed, "run-1").is_none());
        let missing = json!({"payload": {}});
        assert!(extract_run_trace_event(&missing, "run-1").is_none());
    }

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

        let question = serde_json::to_value(RunEvent::AskUserQuestion {
            run_id: "run-1".into(),
            questions: vec![ClarificationQuestion {
                question_id: "q-stage".into(),
                question: "重点分析哪个发育阶段？".into(),
                options: vec![QuestionOption {
                    label: "灌浆期".into(),
                    value: "灌浆期".into(),
                }],
                multi_select: false,
                allow_other: true,
            }],
        })
        .expect("serialize ask_user_question");
        assert_eq!(question["type"], "ask_user_question");
        assert_eq!(question["runId"], "run-1");
        assert_eq!(question["questions"][0]["questionId"], "q-stage");
        assert_eq!(question["questions"][0]["multiSelect"], false);
        assert_eq!(question["questions"][0]["allowOther"], true);
        assert_eq!(question["questions"][0]["options"][0]["label"], "灌浆期");

        let citations = serde_json::to_value(RunEvent::CitationsReady {
            run_id: "run-1".into(),
            citation: CitationRef {
                kb_id: Some("kb-0001".into()),
                revision_id: Some("rev-0012".into()),
                page: Some(3),
                ..Default::default()
            },
            figures: vec![],
            figure_refs: vec![],
            tables: vec![],
        })
        .expect("serialize citations_ready");
        assert_eq!(citations["type"], "citations_ready");
        assert_eq!(citations["citation"]["kbId"], "kb-0001");
        assert_eq!(citations["citation"]["revisionId"], "rev-0012");
        assert_eq!(citations["citation"]["page"], 3);

        let locator = serde_json::to_value(RunEvent::LocatorCandidates {
            run_id: "run-1".into(),
            candidates: vec![LocatorCandidate {
                file_id: "file-0001".into(),
                kb_id: "kb-0001".into(),
                filename: "rice.pdf".into(),
            }],
        })
        .expect("serialize locator_candidates");
        assert_eq!(locator["type"], "locator_candidates");
        assert_eq!(locator["candidates"][0]["fileId"], "file-0001");

        let warning = serde_json::to_value(RunEvent::Warning {
            message: "部分检索来源超时".into(),
        })
        .expect("serialize warning");
        assert_eq!(warning["type"], "warning");
        assert_eq!(warning["message"], "部分检索来源超时");
    }

    #[test]
    fn known_sse_event_whitelist_matches_verbose_false_contract() {
        for event in ["messages", "custom", "interrupt", "error", "end"] {
            assert!(is_known_sse_event(event), "{event} 必须在白名单内");
        }
        // 遗留兼容形态（≤1.3 服务端 / Redis 默认名）不算未知。
        assert!(is_known_sse_event("trace"));
        assert!(is_known_sse_event("message"));
        // 未命名事件（默认 data 帧）不算未知；新增未消费事件必须可被观测到。
        assert!(is_known_sse_event(""));
        assert!(!is_known_sse_event("messages_v2"));
        assert!(!is_known_sse_event("citations"));
    }

    #[test]
    fn known_custom_event_names_cover_evidence_plane() {
        // 1.8 契约的 custom 语义名：证据附件 + 运行提示 + 刻意不渲染的
        // agent_state（进度语义由 messages 帧承载）。
        for name in [
            "yuxi.agent_state",
            "yuxi.warning",
            "yuxi.citation_ready",
            "yuxi.locator_candidates",
            "yuxi.graph_snapshot_ready",
            "yuxi.followup_suggestions",
        ] {
            assert!(is_known_custom_event(name), "{name} 必须在 custom 白名单内");
        }
        // 未声明的 custom 名必须可被观测（sse_custom_unknown 计数）。
        assert!(!is_known_custom_event("yuxi.new_evidence_kind"));
        assert!(!is_known_custom_event("citation_ready"));
    }

    #[test]
    fn sse_fixture_event_names_are_covered_by_whitelists() {
        // 契约语料联动：sse_frames.jsonl 中出现过的每一个 SSE 事件名与 custom
        // 语义名都必须落在桌面端白名单内。服务端新增事件并同步语料后，若
        // 桌面端未跟进消费面，本测试先红（而不是线上静默丢弃）。
        let fixture_path = crate::yuxi::contract_fixture_dir().join("sse_frames.jsonl");
        let corpus = std::fs::read_to_string(&fixture_path)
            .unwrap_or_else(|error| panic!("读取契约语料失败（先跑同步脚本）：{error}"));
        let mut unknown_events = Vec::new();
        let mut unknown_customs = Vec::new();
        for line in corpus.lines().filter(|line| !line.trim().is_empty()) {
            let frame: serde_json::Value =
                serde_json::from_str(line).expect("SSE 语料行必须是合法 JSON");
            let event = frame["event"].as_str().unwrap_or_default();
            if !is_known_sse_event(event) {
                unknown_events.push(event.to_owned());
            }
            if event == "custom"
                && let Some(name) = frame["raw_payload"]["name"].as_str()
                && !is_known_custom_event(name)
            {
                unknown_customs.push(name.to_owned());
            }
        }
        assert!(
            unknown_events.is_empty(),
            "语料事件名超出白名单（服务端契约超前，需扩展桌面消费面）：{unknown_events:?}"
        );
        assert!(
            unknown_customs.is_empty(),
            "语料 custom 名超出白名单（服务端契约超前，需扩展桌面消费面）：{unknown_customs:?}"
        );
    }

    #[test]
    fn reads_authoritative_server_progress_events() {
        let value = serde_json::json!({
            "payload": {
                "chunk": {
                    "status": "progress",
                    "message": "服务端正在检索知识并生成回复…"
                }
            }
        });
        assert_eq!(
            run_progress_message(&value),
            Some("服务端正在检索知识并生成回复…")
        );
        assert_eq!(
            run_progress_message(&serde_json::json!({"payload": {"status": "running"}})),
            None
        );
    }

    #[test]
    fn reads_context_compression_status() {
        let started = serde_json::json!({
            "payload": {
                "chunk": {
                    "status": "context_compression",
                    "compression": { "type": "yuxi.context_compression", "status": "started" }
                }
            }
        });
        assert_eq!(
            compression_status_message(&started),
            Some("正在压缩上下文…")
        );
        let completed = serde_json::json!({
            "payload": {
                "chunk": {
                    "status": "context_compression",
                    "compression": { "type": "yuxi.context_compression", "status": "completed" }
                }
            }
        });
        assert_eq!(compression_status_message(&completed), None);
        assert_eq!(
            compression_status_message(&serde_json::json!({
                "payload": { "chunk": { "status": "progress", "message": "检索中" } }
            })),
            None
        );
    }

    #[test]
    fn accepts_gateway_compatible_request_ids() {
        let request = SendMessageRequest {
            thread_id: "thread-1".into(),
            question: "水稻胚乳何时完成细胞化？".into(),
            request_id: "desktop-12345678-1234-1234-1234-123456789012".into(),
            attachments: vec![],
            resume_run_id: None,
            resume_questions: vec![],
            resume_answers: vec![],
        };
        assert!(validate_send_request(&request).is_ok());
    }

    #[test]
    fn resume_requests_reject_attachments_and_bad_parent_ids() {
        let base = SendMessageRequest {
            thread_id: "thread-1".into(),
            question: "确认继续".into(),
            request_id: "desktop-12345678-1234-1234-1234-123456789012".into(),
            attachments: vec![],
            resume_run_id: Some("run-parent-1".into()),
            resume_questions: vec![],
            resume_answers: vec![],
        };
        assert!(validate_send_request(&base).is_ok());

        // 澄清问题答复没有对应问题定义 / 不用于续跑 → 拒绝
        let orphan_answers = SendMessageRequest {
            resume_run_id: None,
            resume_questions: vec![],
            resume_answers: vec![QuestionAnswer {
                question_id: "q-1".into(),
                selected: vec!["A".into()],
                other_text: None,
            }],
            ..base.clone()
        };
        assert!(validate_send_request(&orphan_answers).is_err());
        let answers_without_questions = SendMessageRequest {
            resume_questions: vec![],
            resume_answers: vec![QuestionAnswer {
                question_id: "q-1".into(),
                selected: vec!["A".into()],
                other_text: None,
            }],
            ..base.clone()
        };
        assert!(validate_send_request(&answers_without_questions).is_err());

        let with_attachment = SendMessageRequest {
            attachments: vec![PendingChatAttachment {
                tmp_file_id: "tmp-1".into(),
                file_name: "a.pdf".into(),
                file_type: Some("application/pdf".into()),
                file_size: 128,
                bucket_name: "b".into(),
                object_name: "o".into(),
                parse_supported: true,
                parse_methods: vec![],
                parsed_object_name: None,
                parse_method: None,
                truncated: false,
            }],
            ..base.clone()
        };
        assert!(validate_send_request(&with_attachment).is_err());

        let bad_parent = SendMessageRequest {
            resume_run_id: Some("bad parent id!".into()),
            ..base
        };
        assert!(validate_send_request(&bad_parent).is_err());
    }

    #[test]
    fn normalizes_local_account_scope_without_double_gateway_prefix() {
        let gateway = "https://api.example.cn/";
        let remote = "yxacct_0123456789abcdef0123456789abcdef";
        let local = local_account_scope(gateway, remote);

        assert_eq!(local, format!("https://api.example.cn|{remote}"));
        assert_eq!(local_account_scope(gateway, &local), local);
        assert_eq!(remote_principal_for_scope(gateway, &local), remote);
        assert_eq!(remote_principal_for_scope(gateway, remote), remote);
    }

    #[test]
    fn extracts_principal_from_local_scope_for_all_generations() {
        // 新格式：网关|yxacct_
        assert_eq!(
            principal_of_scope("https://api.example.cn|yxacct_0123456789abcdef"),
            "yxacct_0123456789abcdef"
        );
        // 旧格式：legacy / api-key-sha256 摘要（无竖线，原样返回）
        assert_eq!(principal_of_scope("legacy"), "legacy");
        assert_eq!(
            principal_of_scope("api-key-sha256:0123abcd"),
            "api-key-sha256:0123abcd"
        );
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

    // ---- 单飞锁并发测试：8 个并发 ensure_active_bearer 只允许一次 HTTP 轮换 ----
    //
    // 这是 P0 安全验收（并发刷新不得触发服务端 reuse_detected 整族撤销）的
    // 机器判据。刷新端点用测试内手写的极简 HTTP 计数服务模拟（std TcpListener
    // + 原子计数，不引入 mock 依赖），每个请求一条连接、Connection: close。

    /// 生成形如 header.payload.signature 的 JWT；exp 可指定（epoch 秒）。
    fn fake_jwt(exp: i64) -> String {
        use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"HS256"}"#);
        let payload = URL_SAFE_NO_PAD.encode(format!(r#"{{"exp":{exp},"sub":"7"}}"#));
        format!("{header}.{payload}.c2ln")
    }

    /// 在 127.0.0.1 随机端口起一个「刷新端点」：返回新会话对并计数收到的请求。
    fn spawn_refresh_counter_server() -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
        use std::io::{Read as _, Write as _};
        use std::net::TcpListener;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind counter server");
        let addr = listener.local_addr().expect("local addr");
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_for_thread = counter.clone();
        std::thread::spawn(move || {
            let now = chrono::Utc::now().timestamp();
            for stream in listener.incoming() {
                let mut stream = match stream {
                    Ok(stream) => stream,
                    Err(_) => break,
                };
                counter_for_thread.fetch_add(1, Ordering::SeqCst);
                let mut buffer = [0u8; 4096];
                let _ = stream.read(&mut buffer);
                let body = format!(
                    r#"{{"access_token":"{}","refresh_token":"yxrt_rotated_new"}}"#,
                    fake_jwt(now + 30 * 60)
                );
                let response = format!(
                    "HTTP/1.1 200 OK
Content-Type: application/json
Content-Length: {}
Connection: close

{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });
        (format!("http://{addr}"), counter)
    }

    #[tokio::test]
    async fn concurrent_bearer_refresh_rotates_exactly_once() {
        use std::sync::{Arc, atomic::Ordering};

        let root =
            std::env::temp_dir().join(format!("daoxin-single-flight-{}", uuid::Uuid::new_v4()));
        // Arc 包装以便 8 个 'static 任务共享同一 AppState（字段均已内部同步）。
        let state = Arc::new(AppState::open(&root, "test").await.expect("open app state"));

        let (gateway_url, counter) = spawn_refresh_counter_server();
        let scope = "http://127.0.0.1:9088|yxacct_concurrent0123456789";
        state
            .database
            .save_setting("gateway_url", &gateway_url)
            .await
            .expect("save gateway");
        state
            .database
            .save_setting("current_account_scope", scope)
            .await
            .expect("save scope");
        // 过期访问令牌 + 有效刷新令牌：所有并发任务都会进入刷新路径
        let now = chrono::Utc::now().timestamp();
        let stored = crate::session::StoredSession {
            access_token: fake_jwt(now - 3600),
            refresh_token: "yxrt_original".into(),
            family_id: "fam-concurrent".into(),
            access_expires_at: now - 3600,
            account_scope_id: "yxacct_concurrent0123456789".into(),
        };
        state
            .credentials
            .save_session_blob(scope, &serde_json::to_string(&stored).expect("serialize"))
            .expect("save session blob");

        let mut handles = Vec::new();
        for _ in 0..8 {
            let state_for_task = state.clone();
            handles.push(tokio::spawn(async move {
                ensure_active_bearer(&state_for_task).await
            }));
        }
        let mut tokens = Vec::new();
        for handle in handles {
            let token = handle.await.expect("join").expect("bearer ok");
            tokens.push(token.expose_secret().to_owned());
        }

        // 全部拿到同一个新访问令牌，且刷新端点只被调用一次
        assert!(tokens.windows(2).all(|pair| pair[0] == pair[1]));
        assert_eq!(counter.load(Ordering::SeqCst), 1, "并发刷新必须单飞");
        assert!(tokens[0] != stored.access_token);

        // 回写的新 blob 已替换旧刷新令牌
        let updated = state
            .credentials
            .session_blob(scope)
            .expect("read blob")
            .expect("blob exists");
        assert!(updated.contains("yxrt_rotated_new"));

        drop(state);
        remove_state_test_directory(&root).await;
    }

    async fn remove_state_test_directory(path: &std::path::Path) {
        let mut last_error = None;
        for _ in 0..20 {
            match std::fs::remove_dir_all(path) {
                Ok(()) => return,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
                Err(error) => last_error = Some(error),
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        panic!("remove state test directory: {:?}", last_error.unwrap());
    }
}
