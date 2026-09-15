use std::time::Duration;

use reqwest::{Client, Response, StatusCode, header};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::time::sleep;
use url::Url;

use crate::{
    config::validate_gateway_url,
    error::{AppError, AppResult},
};

const DEFAULT_AGENT_PATH: &str = "/api/agent/default";
const CREATE_THREAD_PATH: &str = "/api/chat/thread";
const CREATE_RUN_PATH: &str = "/api/agent/runs";
const LEGACY_RUN_RESULT_PATH: &str = "/api/agent-invocation/agent-call/runs/result";
const CREDENTIAL_STATUS_PATH: &str = "/api/agent-invocation/credential-status";
const TMP_ATTACHMENT_PATH: &str = "/api/chat/attachments/tmp";
const ONBOARDING_EXCHANGE_PATH: &str = "/api/auth/onboarding/exchange";
const CLI_SESSIONS_PATH: &str = "/api/auth/cli/sessions";
const CLI_SESSIONS_TOKEN_PATH: &str = "/api/auth/cli/sessions/token";
const AUTH_SESSIONS_PATH: &str = "/api/auth/sessions";
const USER_QUOTA_PATH: &str = "/api/user/quota";
const USER_USAGE_PATH: &str = "/api/user/usage";
const GATEWAY_TRACE_HEADER: &str = "X-Gateway-Trace-ID";

#[derive(Clone)]
pub struct YuxiClient {
    client: Client,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreatedRun {
    pub run_id: String,
    pub thread_id: String,
    pub request_id: String,
    pub status: String,
    #[serde(default)]
    pub run_context: ServerRunContext,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerThread {
    pub id: String,
    pub agent_id: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all(serialize = "camelCase", deserialize = "snake_case"))]
pub struct ServerRunContext {
    pub protocol_version: Option<String>,
    pub agent_slug: Option<String>,
    pub thread_id: Option<String>,
    pub request_id: Option<String>,
    pub result_authority: Option<String>,
    pub model_spec: Option<String>,
    #[serde(default)]
    pub knowledge_scope: KnowledgeScopeSummary,
    #[serde(default)]
    pub knowledge_retrievals: Vec<KnowledgeRetrievalSummary>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all(serialize = "camelCase", deserialize = "snake_case"))]
pub struct KnowledgeScopeSummary {
    pub scope_id: Option<String>,
    pub scope_version: Option<i64>,
    pub scope_mode: Option<String>,
    pub knowledge_strategy: Option<String>,
    pub retrieval_mode: Option<String>,
    #[serde(default)]
    pub allow_web: bool,
    #[serde(default)]
    pub kb_count: usize,
    #[serde(default)]
    pub members: Vec<KnowledgeScopeMember>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all(serialize = "camelCase", deserialize = "snake_case"))]
pub struct KnowledgeScopeMember {
    pub kb_id: Option<String>,
    pub kb_name: Option<String>,
    pub kb_type: Option<String>,
    pub priority: Option<i64>,
    #[serde(default)]
    pub document_enabled: bool,
    #[serde(default)]
    pub graph_enabled: bool,
    #[serde(default)]
    pub structured_enabled: bool,
    pub included_via: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all(serialize = "camelCase", deserialize = "snake_case"))]
pub struct KnowledgeRetrievalSummary {
    pub retrieval_id: Option<String>,
    pub status: Option<String>,
    pub intent: Option<String>,
    pub query_mode: Option<String>,
    pub planner_version: Option<String>,
    pub entity_resolver_version: Option<String>,
    pub retrieval_orchestrator_version: Option<String>,
    pub claim_validator_version: Option<String>,
    pub contract_schema_version: Option<String>,
    #[serde(default)]
    pub source_status: Vec<Value>,
    pub returned_relation_count: Option<i64>,
    pub returned_claim_count: Option<i64>,
    pub returned_evidence_count: Option<i64>,
    #[serde(default)]
    pub warnings: Vec<Value>,
    pub error_code: Option<String>,
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RunResult {
    pub status: String,
    pub output: String,
    pub error: Option<String>,
    pub error_code: Option<String>,
    pub context: ServerRunContext,
}

#[derive(Debug, Serialize)]
struct CreateThreadRequest<'a> {
    agent_id: &'a str,
    thread_id: &'a str,
    title: &'a str,
    metadata: Value,
}

#[derive(Debug, Serialize)]
struct CreateRunRequest<'a> {
    /// 普通提问必填；resume run（人工审批续跑）整体省略——网关 schema 为
    /// string，null 会被拒，必须不发送该字段。
    #[serde(skip_serializing_if = "Option::is_none")]
    query: Option<&'a str>,
    agent_slug: &'a str,
    thread_id: &'a str,
    meta: RunRequestMeta<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    model_spec: Option<&'a str>,
    /// 中断恢复：传给 LangGraph 的输入载荷（人工审批场景为用户答复字符串），
    /// 与 created_by_run_id 成对出现；沿用父 run 的冻结模型，不得携带新 model_spec。
    #[serde(skip_serializing_if = "Option::is_none")]
    resume: Option<&'a Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    created_by_run_id: Option<&'a str>,
}

#[derive(Debug, Serialize)]
struct RunRequestMeta<'a> {
    request_id: &'a str,
    client: &'static str,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    attachment_file_ids: Vec<&'a str>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingChatAttachment {
    pub tmp_file_id: String,
    pub file_name: String,
    pub file_type: Option<String>,
    pub file_size: usize,
    pub bucket_name: String,
    pub object_name: String,
    pub parse_supported: bool,
    #[serde(default)]
    pub parse_methods: Vec<String>,
    pub parsed_object_name: Option<String>,
    pub parse_method: Option<String>,
    #[serde(default)]
    pub truncated: bool,
}

#[derive(Debug, Deserialize)]
struct TmpAttachmentResponse {
    tmp_file_id: String,
    file_name: String,
    file_type: Option<String>,
    file_size: usize,
    bucket_name: String,
    object_name: String,
    #[serde(default)]
    parse_supported: bool,
    #[serde(default)]
    parse_methods: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct TmpAttachmentParseResponse {
    parsed_object_name: String,
    parse_method: String,
    #[serde(default)]
    truncated: bool,
}

#[derive(Debug, Serialize)]
struct TmpAttachmentParseRequest<'a> {
    object_name: &'a str,
    file_name: &'a str,
    parse_method: &'a str,
    bucket_name: &'a str,
}

#[derive(Debug, Serialize)]
struct TmpAttachmentConfirmRequest<'a> {
    attachments: Vec<TmpAttachmentConfirmItem<'a>>,
}

#[derive(Debug, Serialize)]
struct TmpAttachmentConfirmItem<'a> {
    file_name: &'a str,
    // 网关 confirm 闭集对可选字段的声明是 type:string（不接受 null）：
    // 未解析附件（parsed_object_name=None）与未知 MIME（file_type=None）
    // 必须整体省略字段；服务端 pydantic 对缺省字段取 None 默认值。
    #[serde(skip_serializing_if = "Option::is_none")]
    file_type: Option<&'a str>,
    bucket_name: &'a str,
    object_name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    parsed_object_name: Option<&'a str>,
    truncated: bool,
}

#[derive(Debug, Deserialize)]
struct TmpAttachmentConfirmResponse {
    attachments: Vec<ConfirmedAttachment>,
}

#[derive(Debug, Deserialize)]
struct ConfirmedAttachment {
    file_id: String,
}

impl YuxiClient {
    pub fn new(app_version: &str) -> AppResult<Self> {
        let mut default_headers = header::HeaderMap::new();
        // 版本可见性：服务端据此统计安装基数，决定 desktop_legacy 静态 Key 的
        // 淘汰时点（先开关后删码，不一步到 410）。
        default_headers.insert(
            "X-Client-Version",
            header::HeaderValue::from_str(app_version)
                .map_err(|error| AppError::Internal(error.to_string()))?,
        );
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(format!("RiceEndospermDesktop/{app_version}"))
            .default_headers(default_headers)
            .build()?;
        Ok(Self { client })
    }

    pub async fn test_connection(
        &self,
        gateway_url: &str,
        agent_slug: &str,
        api_key: &SecretString,
    ) -> AppResult<Option<String>> {
        let base = validate_gateway_url(gateway_url)?;
        let mut last_error = None;
        for delay in [None, Some(Duration::from_millis(800))] {
            if let Some(delay) = delay {
                sleep(delay).await;
            }
            match self.test_connection_once(&base, agent_slug, api_key).await {
                Ok(identity) => return Ok(identity),
                Err(error) if connection_error_is_retryable(&error) => last_error = Some(error),
                Err(error) => return Err(error),
            }
        }

        Err(connection_error_for_gateway(
            &base,
            last_error.unwrap_or(AppError::ServiceUnavailable),
        ))
    }

    async fn test_connection_once(
        &self,
        base: &str,
        agent_slug: &str,
        api_key: &SecretString,
    ) -> AppResult<Option<String>> {
        let status_response = self
            .authorized_get(&format!("{base}{CREDENTIAL_STATUS_PATH}"), api_key)
            .timeout(Duration::from_secs(12))
            .send()
            .await?;
        if status_response.status().is_success() {
            let value = status_response
                .json::<Value>()
                .await
                .map_err(|error| AppError::Protocol(error.to_string()))?;
            return Ok(value
                .get("account_scope_id")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(str::to_owned));
        }
        if status_response.status() != StatusCode::NOT_FOUND {
            return Err(response_error(status_response).await);
        }

        // 兼容尚未部署 credential-status 的 Yuxi：查询一个必然不存在的 run。
        // 该请求不会启动模型，也不会产生计费；401/403 仍能准确验证凭证。
        let probe = self
            .authorized_post(&format!("{base}{LEGACY_RUN_RESULT_PATH}"), api_key)
            .json(&json!({
                "run_id": "desktop-connection-test",
                "agent_slug": agent_slug,
            }))
            .timeout(Duration::from_secs(12))
            .send()
            .await?;
        match probe.status().as_u16() {
            200 | 404 => Ok(None),
            _ => Err(response_error(probe).await),
        }
    }

    /// 中断恢复变体：`resume` 为 LangGraph 输入载荷（人工审批答复），`created_by_run_id`
    /// 指向被恢复的父 run；此时 `question` 传 None。服务端会 recheck BYOK 凭据并
    /// 沿用父 run 冻结模型。普通提问走同一入口（resume=None）。
    #[allow(clippy::too_many_arguments)]
    pub async fn create_run_with_resume(
        &self,
        gateway_url: &str,
        agent_slug: &str,
        api_key: &SecretString,
        question: Option<&str>,
        yuxi_thread_id: &str,
        request_id: &str,
        model_spec: Option<&str>,
        attachment_file_ids: &[String],
        resume: Option<&Value>,
        created_by_run_id: Option<&str>,
    ) -> AppResult<CreatedRun> {
        let base = validate_gateway_url(gateway_url)?;
        let mut last_error = None;
        for attempt in 0..2 {
            let response = self
                .authorized_post(&format!("{base}{CREATE_RUN_PATH}"), api_key)
                .header("X-Client-Request-ID", request_id)
                .json(&CreateRunRequest {
                    query: question,
                    agent_slug,
                    thread_id: yuxi_thread_id,
                    meta: RunRequestMeta {
                        request_id,
                        client: "rice-endosperm-desktop",
                        attachment_file_ids: attachment_file_ids
                            .iter()
                            .map(String::as_str)
                            .collect(),
                    },
                    model_spec,
                    resume,
                    created_by_run_id,
                })
                .timeout(Duration::from_secs(45))
                .send()
                .await;
            match response.map_err(AppError::from) {
                Ok(response) => {
                    let response = ensure_success(response).await?;
                    let created = response
                        .json::<CreatedRun>()
                        .await
                        .map_err(|error| AppError::Protocol(error.to_string()))?;
                    if created.run_id.is_empty() || created.thread_id.is_empty() {
                        return Err(AppError::Protocol(
                            "创建运行响应缺少 run_id 或 thread_id".into(),
                        ));
                    }
                    return Ok(created);
                }
                // 超时/连接类失败时服务端大概率已建 run；借助 request_id 幂等
                // 重试一次，避免远端孤儿 run 永久脱离本地对账。
                Err(error) if attempt == 0 && connection_error_is_retryable(&error) => {
                    last_error = Some(error);
                    sleep(Duration::from_millis(800)).await;
                }
                Err(error) => return Err(error),
            }
        }
        Err(last_error.unwrap_or(AppError::ServiceUnavailable))
    }

    pub async fn upload_tmp_attachment(
        &self,
        gateway_url: &str,
        api_key: &SecretString,
        file_name: &str,
        content_type: &str,
        bytes: Vec<u8>,
    ) -> AppResult<PendingChatAttachment> {
        let base = validate_gateway_url(gateway_url)?;
        let part = reqwest::multipart::Part::bytes(bytes)
            .file_name(file_name.to_owned())
            .mime_str(content_type)
            .map_err(|error| AppError::Protocol(format!("附件 MIME 类型无效: {error}")))?;
        let response = self
            .authorized_post(&format!("{base}{TMP_ATTACHMENT_PATH}"), api_key)
            .multipart(reqwest::multipart::Form::new().part("file", part))
            .timeout(Duration::from_secs(120))
            .send()
            .await?;
        let response = ensure_success(response).await?;
        let uploaded = response
            .json::<TmpAttachmentResponse>()
            .await
            .map_err(|error| AppError::Protocol(format!("附件上传响应无法解析: {error}")))?;
        Ok(PendingChatAttachment {
            tmp_file_id: uploaded.tmp_file_id,
            file_name: uploaded.file_name,
            file_type: uploaded.file_type,
            file_size: uploaded.file_size,
            bucket_name: uploaded.bucket_name,
            object_name: uploaded.object_name,
            parse_supported: uploaded.parse_supported,
            parse_methods: uploaded.parse_methods,
            parsed_object_name: None,
            parse_method: None,
            truncated: false,
        })
    }

    pub async fn parse_tmp_attachment(
        &self,
        gateway_url: &str,
        api_key: &SecretString,
        attachment: &mut PendingChatAttachment,
        parse_method: &str,
    ) -> AppResult<()> {
        if !attachment
            .parse_methods
            .iter()
            .any(|method| method == parse_method)
        {
            return Err(AppError::Protocol(
                "服务端未声明支持所选附件解析引擎".into(),
            ));
        }
        let base = validate_gateway_url(gateway_url)?;
        let response = self
            .authorized_post(&format!("{base}{TMP_ATTACHMENT_PATH}/parse"), api_key)
            .json(&TmpAttachmentParseRequest {
                object_name: &attachment.object_name,
                file_name: &attachment.file_name,
                parse_method,
                bucket_name: &attachment.bucket_name,
            })
            .timeout(Duration::from_secs(15 * 60))
            .send()
            .await?;
        let parsed = ensure_success(response)
            .await?
            .json::<TmpAttachmentParseResponse>()
            .await
            .map_err(|error| AppError::Protocol(format!("附件解析响应无法解析: {error}")))?;
        attachment.parsed_object_name = Some(parsed.parsed_object_name);
        attachment.parse_method = Some(parsed.parse_method);
        attachment.truncated = parsed.truncated;
        Ok(())
    }

    pub async fn confirm_tmp_attachments(
        &self,
        gateway_url: &str,
        api_key: &SecretString,
        thread_id: &str,
        attachments: &[PendingChatAttachment],
    ) -> AppResult<Vec<String>> {
        if attachments.is_empty() {
            return Ok(Vec::new());
        }
        let base = validate_gateway_url(gateway_url)?;
        let response = self
            .authorized_post(
                &format!("{base}/api/chat/thread/{thread_id}/attachments/confirm"),
                api_key,
            )
            .json(&TmpAttachmentConfirmRequest {
                attachments: attachments
                    .iter()
                    .map(|attachment| TmpAttachmentConfirmItem {
                        file_name: &attachment.file_name,
                        file_type: attachment.file_type.as_deref(),
                        bucket_name: &attachment.bucket_name,
                        object_name: &attachment.object_name,
                        parsed_object_name: attachment.parsed_object_name.as_deref(),
                        truncated: attachment.truncated,
                    })
                    .collect(),
            })
            .timeout(Duration::from_secs(120))
            .send()
            .await?;
        let confirmed = ensure_success(response)
            .await?
            .json::<TmpAttachmentConfirmResponse>()
            .await
            .map_err(|error| AppError::Protocol(format!("附件绑定响应无法解析: {error}")))?;
        if confirmed.attachments.len() != attachments.len() {
            return Err(AppError::Protocol("服务端未完整绑定本次附件".into()));
        }
        Ok(confirmed
            .attachments
            .into_iter()
            .map(|item| item.file_id)
            .collect())
    }

    /// 从服务端读取当前用户可访问的权威默认智能体。
    ///
    /// 桌面端不得把编译期 slug 当作运行时真源；服务端返回值会被固化到本地线程，
    /// 后续所有 run 都使用该线程实际绑定的智能体。
    pub async fn default_agent_slug(
        &self,
        gateway_url: &str,
        api_key: &SecretString,
    ) -> AppResult<String> {
        let base = validate_gateway_url(gateway_url)?;
        let response = self
            .authorized_get(&format!("{base}{DEFAULT_AGENT_PATH}"), api_key)
            .timeout(Duration::from_secs(20))
            .send()
            .await?;
        let response = ensure_success(response).await?;
        let value = response
            .json::<Value>()
            .await
            .map_err(|error| AppError::Protocol(error.to_string()))?;
        parse_default_agent_slug(&value)
    }

    /// 创建与 Web 端相同的原生 Yuxi Conversation。
    /// 客户端线程 ID 同时作为幂等键，连接超时后可安全重试而不产生孤儿会话。
    pub async fn create_thread(
        &self,
        gateway_url: &str,
        api_key: &SecretString,
        agent_slug: &str,
        requested_thread_id: &str,
        title: &str,
    ) -> AppResult<ServerThread> {
        let base = validate_gateway_url(gateway_url)?;
        let response = self
            .authorized_post(&format!("{base}{CREATE_THREAD_PATH}"), api_key)
            .json(&CreateThreadRequest {
                agent_id: agent_slug,
                thread_id: requested_thread_id,
                title,
                metadata: json!({"client": "rice-endosperm-desktop"}),
            })
            .timeout(Duration::from_secs(30))
            .send()
            .await?;
        let response = ensure_success(response).await?;
        let thread = response
            .json::<ServerThread>()
            .await
            .map_err(|error| AppError::Protocol(error.to_string()))?;
        if thread.id.is_empty() || thread.agent_id.is_empty() {
            return Err(AppError::Protocol("创建会话响应缺少 id 或 agent_id".into()));
        }
        if thread.id != requested_thread_id {
            return Err(AppError::Protocol(
                "服务端返回的会话 ID 与客户端幂等 ID 不一致".into(),
            ));
        }
        if thread.agent_id != agent_slug {
            return Err(AppError::Protocol(
                "服务端创建的会话绑定了非预期智能体".into(),
            ));
        }
        Ok(thread)
    }

    pub async fn event_response(
        &self,
        gateway_url: &str,
        api_key: &SecretString,
        run_id: &str,
        last_event_id: Option<&str>,
    ) -> AppResult<Response> {
        let base = validate_gateway_url(gateway_url)?;
        let mut request = self
            .authorized_get(
                &format!("{base}/api/agent/runs/{run_id}/events?verbose=false"),
                api_key,
            )
            .header(header::ACCEPT, "text/event-stream")
            .timeout(Duration::from_secs(31 * 60));
        if let Some(event_id) = last_event_id.filter(|value| !value.is_empty()) {
            request = request.header("Last-Event-ID", event_id);
        }
        let response = request.send().await?;
        ensure_success(response).await
    }

    pub async fn result(
        &self,
        gateway_url: &str,
        api_key: &SecretString,
        run_id: &str,
    ) -> AppResult<RunResult> {
        let base = validate_gateway_url(gateway_url)?;
        let response = self
            .authorized_get(&format!("{base}/api/agent/runs/{run_id}/result"), api_key)
            .timeout(Duration::from_secs(30))
            .send()
            .await?;
        let response = ensure_success(response).await?;
        let value = response
            .json::<Value>()
            .await
            .map_err(|error| AppError::Protocol(error.to_string()))?;
        Ok(parse_run_result(&value))
    }

    /// 获取 run 执行轨迹快照（summary + spans + snapshot_sequence）。
    /// 桌面端在终态后或重启恢复时拉取，作为实时 trace 帧的权威投影。
    pub async fn trace_snapshot(
        &self,
        gateway_url: &str,
        api_key: &SecretString,
        run_id: &str,
    ) -> AppResult<Value> {
        let base = validate_gateway_url(gateway_url)?;
        let response = self
            .authorized_get(&format!("{base}/api/agent/runs/{run_id}/trace"), api_key)
            .timeout(Duration::from_secs(20))
            .send()
            .await?;
        let response = ensure_success(response).await?;
        let value = response
            .json::<Value>()
            .await
            .map_err(|error| AppError::Protocol(error.to_string()))?;
        if value.get("run_id").and_then(Value::as_str) != Some(run_id) {
            return Err(AppError::Protocol("轨迹快照响应缺少 run_id".into()));
        }
        Ok(value)
    }

    pub async fn trace_events(
        &self,
        gateway_url: &str,
        api_key: &SecretString,
        run_id: &str,
        after_sequence: u64,
    ) -> AppResult<Value> {
        let base = validate_gateway_url(gateway_url)?;
        let response = self
            .authorized_get(
                &format!(
                    "{base}/api/agent/runs/{run_id}/trace/events?after_sequence={after_sequence}&limit=500"
                ),
                api_key,
            )
            .timeout(Duration::from_secs(20))
            .send()
            .await?;
        let value = ensure_success(response)
            .await?
            .json::<Value>()
            .await
            .map_err(|error| AppError::Protocol(error.to_string()))?;
        if value.get("run_id").and_then(Value::as_str) != Some(run_id) {
            return Err(AppError::Protocol("轨迹事件响应缺少 run_id".into()));
        }
        Ok(value)
    }

    pub async fn cancel_run(
        &self,
        gateway_url: &str,
        api_key: &SecretString,
        run_id: &str,
    ) -> AppResult<()> {
        let base = validate_gateway_url(gateway_url)?;
        let response = self
            .authorized_post(&format!("{base}/api/agent/runs/{run_id}/cancel"), api_key)
            .timeout(Duration::from_secs(20))
            .send()
            .await?;
        match response.status().as_u16() {
            200 | 202 | 204 | 409 => Ok(()),
            _ => Err(response_error(response).await),
        }
    }

    fn authorized_get(&self, url: &str, key: &SecretString) -> reqwest::RequestBuilder {
        self.client.get(url).bearer_auth(key.expose_secret())
    }

    fn authorized_post(&self, url: &str, key: &SecretString) -> reqwest::RequestBuilder {
        self.client.post(url).bearer_auth(key.expose_secret())
    }

    fn authorized_put(&self, url: &str, key: &SecretString) -> reqwest::RequestBuilder {
        self.client.put(url).bearer_auth(key.expose_secret())
    }

    fn authorized_delete(&self, url: &str, key: &SecretString) -> reqwest::RequestBuilder {
        self.client.delete(url).bearer_auth(key.expose_secret())
    }

    /// P2b：旋转设备会话刷新令牌。重放/撤销/过期都以 401 表达，
    /// 由调用方决定是回退过渡 Key 还是提示重新登录。
    pub async fn refresh_cli_session(
        &self,
        gateway_url: &str,
        refresh_token: &SecretString,
    ) -> AppResult<RotatedSession> {
        use secrecy::ExposeSecret as _;

        let base = validate_gateway_url(gateway_url)?;
        let response = self
            .client
            .post(format!("{base}/api/auth/cli/token/refresh"))
            .json(&json!({"refresh_token": refresh_token.expose_secret()}))
            .timeout(Duration::from_secs(20))
            .send()
            .await?;
        let response = ensure_success(response).await?;
        let value = response
            .json::<Value>()
            .await
            .map_err(|error| AppError::Protocol(error.to_string()))?;
        let access_token = value
            .get("access_token")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| AppError::Protocol("刷新响应缺少访问令牌".into()))?
            .to_string();
        let refresh_token = value
            .get("refresh_token")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| AppError::Protocol("刷新响应缺少刷新令牌".into()))?
            .to_string();
        Ok(RotatedSession {
            access_token,
            refresh_token,
        })
    }

    /// 三字段登录由服务端在单个请求中原子校验，避免客户端分别取得 JWT 与
    /// Key 身份后再拼接判断；响应返回服务端固化的账号作用域作为本地隔离真源。
    pub async fn verify_desktop_login(
        &self,
        gateway_url: &str,
        username: &str,
        password: &SecretString,
        api_key: &SecretString,
    ) -> AppResult<DesktopLoginIdentity> {
        use secrecy::ExposeSecret as _;

        let base = validate_gateway_url(gateway_url)?;
        let response = self
            .client
            .post(format!("{base}/api/auth/desktop/login"))
            .json(&json!({
                "login_id": username,
                "password": password.expose_secret(),
                "api_key": api_key.expose_secret(),
            }))
            .timeout(Duration::from_secs(20))
            .send()
            .await?;
        let response = ensure_success(response).await?;
        let value = response
            .json::<Value>()
            .await
            .map_err(|error| AppError::Protocol(error.to_string()))?;
        parse_desktop_login_response(&value)
    }

    /// P5 企业激活码开户：一次性激活码换取设备会话对。
    /// 服务端对兑换**只签发会话、绝不发静态 Key**（响应无 api_key 字段）；
    /// 激活码一次性消费，错误以 detail.{error,message} 表达（404/409/410/403）。
    pub async fn exchange_onboarding_activation(
        &self,
        gateway_url: &str,
        activation_code: &SecretString,
        device_name: &str,
    ) -> AppResult<OnboardingExchange> {
        use secrecy::ExposeSecret as _;

        let base = validate_gateway_url(gateway_url)?;
        let response = self
            .client
            .post(format!("{base}{ONBOARDING_EXCHANGE_PATH}"))
            .json(&json!({
                "activation_code": activation_code.expose_secret(),
                "device_name": device_name,
            }))
            .timeout(Duration::from_secs(20))
            .send()
            .await?;
        let response = ensure_success(response).await?;
        let value = response
            .json::<Value>()
            .await
            .map_err(|error| AppError::Protocol(error.to_string()))?;
        parse_onboarding_exchange(&value)
    }

    /// P2b 设备码第一步：创建待授权会话，返回浏览器授权页与轮询参数。
    pub async fn start_cli_session(
        &self,
        gateway_url: &str,
        key_name: Option<&str>,
    ) -> AppResult<DeviceCodeStart> {
        let base = validate_gateway_url(gateway_url)?;
        let mut payload = json!({});
        if let Some(name) = key_name.filter(|value| !value.trim().is_empty()) {
            payload["key_name"] = json!(name.trim().chars().take(100).collect::<String>());
        }
        let response = self
            .client
            .post(format!("{base}{CLI_SESSIONS_PATH}"))
            .json(&payload)
            .timeout(Duration::from_secs(20))
            .send()
            .await?;
        let response = ensure_success(response).await?;
        let value = response
            .json::<Value>()
            .await
            .map_err(|error| AppError::Protocol(error.to_string()))?;
        parse_device_code_start(&value)
    }

    /// P2b 设备码轮询：返回 `Ok(None)` 表示 `authorization_pending`（继续按
    /// interval 轮询，不是错误）；批准后返回会话对 + 过渡静态 Key。
    pub async fn poll_cli_session_token(
        &self,
        gateway_url: &str,
        device_code: &SecretString,
    ) -> AppResult<Option<DeviceCodeExchange>> {
        use secrecy::ExposeSecret as _;

        let base = validate_gateway_url(gateway_url)?;
        let response = self
            .client
            .post(format!("{base}{CLI_SESSIONS_TOKEN_PATH}"))
            .json(&json!({"device_code": device_code.expose_secret()}))
            .timeout(Duration::from_secs(20))
            .send()
            .await?;
        if response.status().as_u16() == 400 {
            let trace_id = gateway_trace_id(response.headers());
            let value = response.json::<Value>().await.ok();
            let code = value
                .as_ref()
                .and_then(|value| value.pointer("/detail/error"))
                .and_then(Value::as_str)
                .map(str::to_owned);
            if code.as_deref() == Some("authorization_pending") {
                return Ok(None);
            }
            let message = value
                .as_ref()
                .and_then(|value| value.pointer("/detail/message"))
                .and_then(Value::as_str)
                .unwrap_or("设备授权轮询失败")
                .to_owned();
            return Err(AppError::from_server_parts(
                400, message, code, None, trace_id,
            ));
        }
        let response = ensure_success(response).await?;
        let value = response
            .json::<Value>()
            .await
            .map_err(|error| AppError::Protocol(error.to_string()))?;
        parse_device_code_exchange(&value)
    }

    /// 撤销静态 API Key（设备码兑换后清理 90 天过渡 Key，防孤儿）。
    /// 404 视为成功（幂等：Key 已被撤销或过期）。
    pub async fn delete_api_key_by_id(
        &self,
        gateway_url: &str,
        bearer: &SecretString,
        api_key_id: i64,
    ) -> AppResult<()> {
        let base = validate_gateway_url(gateway_url)?;
        let response = self
            .authorized_delete(&format!("{base}/api/user/apikey/{api_key_id}"), bearer)
            .timeout(Duration::from_secs(15))
            .send()
            .await?;
        match response.status().as_u16() {
            200 | 202 | 204 | 404 => Ok(()),
            _ => Err(response_error(response).await),
        }
    }

    /// 列出当前用户的活跃设备会话（P2b 远程下线入口）。
    pub async fn list_device_sessions(
        &self,
        gateway_url: &str,
        bearer: &SecretString,
    ) -> AppResult<Vec<DeviceSessionView>> {
        let base = validate_gateway_url(gateway_url)?;
        let response = self
            .authorized_get(&format!("{base}{AUTH_SESSIONS_PATH}"), bearer)
            .timeout(Duration::from_secs(15))
            .send()
            .await?;
        let value = ensure_success(response)
            .await?
            .json::<Value>()
            .await
            .map_err(|error| AppError::Protocol(error.to_string()))?;
        let items = value
            .get("sessions")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(items
            .iter()
            .map(|item| DeviceSessionView {
                session_id: item
                    .get("session_id")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
                created_at: optional_string(item, "created_at"),
                last_refreshed_at: optional_string(item, "last_refreshed_at"),
            })
            .filter(|session| !session.session_id.is_empty())
            .collect())
    }

    /// 远程下线指定设备会话族。
    pub async fn revoke_device_session(
        &self,
        gateway_url: &str,
        bearer: &SecretString,
        family_id: &str,
    ) -> AppResult<()> {
        let base = validate_gateway_url(gateway_url)?;
        let response = self
            .authorized_delete(&format!("{base}{AUTH_SESSIONS_PATH}/{family_id}"), bearer)
            .timeout(Duration::from_secs(15))
            .send()
            .await?;
        ensure_success(response).await?;
        Ok(())
    }

    /// P5 自省：当前用户权益（策略 + 配额）。
    pub async fn get_user_quota(
        &self,
        gateway_url: &str,
        bearer: &SecretString,
    ) -> AppResult<QuotaSummary> {
        let base = validate_gateway_url(gateway_url)?;
        let response = self
            .authorized_get(&format!("{base}{USER_QUOTA_PATH}"), bearer)
            .timeout(Duration::from_secs(15))
            .send()
            .await?;
        let value = ensure_success(response)
            .await?
            .json::<Value>()
            .await
            .map_err(|error| AppError::Protocol(error.to_string()))?;
        Ok(QuotaSummary {
            daily_run_limit: value.get("daily_run_limit").and_then(Value::as_i64),
            monthly_token_limit: value.get("monthly_token_limit").and_then(Value::as_i64),
            model_access_policy: value
                .get("model_access_policy")
                .and_then(Value::as_str)
                .unwrap_or("byok_optional")
                .to_owned(),
            byok_platform_token_exempt: value
                .get("byok_platform_token_exempt")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            has_active_byok: value
                .get("has_active_byok")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        })
    }

    /// P5 自省：近 N 天用量与本月汇总。
    pub async fn get_user_usage(
        &self,
        gateway_url: &str,
        bearer: &SecretString,
        days: u32,
    ) -> AppResult<UsageSummary> {
        let base = validate_gateway_url(gateway_url)?;
        let response = self
            .authorized_get(&format!("{base}{USER_USAGE_PATH}?days={days}"), bearer)
            .timeout(Duration::from_secs(15))
            .send()
            .await?;
        let value = ensure_success(response)
            .await?
            .json::<Value>()
            .await
            .map_err(|error| AppError::Protocol(error.to_string()))?;
        let days = value
            .get("daily")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(UsageSummary {
            daily: days
                .iter()
                .map(|item| UsageDay {
                    date: item
                        .get("date")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned(),
                    run_count: item.get("run_count").and_then(Value::as_i64).unwrap_or(0),
                    tokens: item.get("tokens").and_then(Value::as_i64).unwrap_or(0),
                })
                .collect(),
            monthly_tokens: value
                .get("monthly_tokens")
                .and_then(Value::as_i64)
                .unwrap_or(0),
            monthly_platform_tokens: value
                .get("monthly_platform_tokens")
                .and_then(Value::as_i64)
                .unwrap_or(0),
            monthly_byok_tokens: value
                .get("monthly_byok_tokens")
                .and_then(Value::as_i64)
                .unwrap_or(0),
        })
    }

    /// P5 BYOK：列出当前用户的自有模型凭据（仅掩码，无明文）。
    pub async fn list_byok_credentials(
        &self,
        gateway_url: &str,
        bearer: &SecretString,
    ) -> AppResult<Vec<ByokCredential>> {
        let base = validate_gateway_url(gateway_url)?;
        let response = self
            .authorized_get(&format!("{base}/api/user/model-credentials"), bearer)
            .timeout(Duration::from_secs(15))
            .send()
            .await?;
        let response = ensure_success(response).await?;
        let value = response
            .json::<Value>()
            .await
            .map_err(|error| AppError::Protocol(error.to_string()))?;
        let items = value
            .get("credentials")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(items
            .iter()
            .map(|item| ByokCredential {
                credential_id: item
                    .get("credential_id")
                    .and_then(Value::as_i64)
                    .unwrap_or(0),
                provider_id: item
                    .get("provider_id")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                label: item
                    .get("label")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                masked_hint: item
                    .get("masked_hint")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                status: item
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                protocol: optional_string(item, "protocol"),
                base_url: optional_string(item, "base_url"),
                model_id: optional_string(item, "model_id"),
                model_spec: optional_string(item, "model_spec"),
            })
            .collect())
    }

    /// P5 BYOK：保存/替换某供应商下的自有密钥（服务端版本化，明文不落盘）。
    pub async fn save_byok_credential(
        &self,
        gateway_url: &str,
        bearer: &SecretString,
        provider_id: &str,
        api_key: &SecretString,
    ) -> AppResult<()> {
        use secrecy::ExposeSecret as _;

        let base = validate_gateway_url(gateway_url)?;
        let response = self
            .authorized_put(&format!("{base}/api/user/model-credentials"), bearer)
            .json(&json!({
                "provider_id": provider_id,
                "api_key": api_key.expose_secret(),
            }))
            .timeout(Duration::from_secs(20))
            .send()
            .await?;
        ensure_success(response).await?;
        Ok(())
    }

    /// 保存用户级 OpenAI/Anthropic 兼容端点；服务端会完成 SSRF 校验、加密和默认模型切换。
    pub async fn save_custom_model_credential(
        &self,
        gateway_url: &str,
        bearer: &SecretString,
        protocol: &str,
        base_url: &str,
        api_key: &SecretString,
        model: &str,
    ) -> AppResult<ModelConfigurationResult> {
        use secrecy::ExposeSecret as _;

        let base = validate_gateway_url(gateway_url)?;
        let response = self
            .authorized_put(&format!("{base}/api/user/model-credentials"), bearer)
            .json(&json!({
                "protocol": protocol,
                "base_url": base_url,
                "api_key": api_key.expose_secret(),
                "model": model,
                "activate_as_default": true,
            }))
            .timeout(Duration::from_secs(20))
            .send()
            .await?;
        parse_model_configuration_response(ensure_success(response).await?).await
    }

    /// 导入 Claude Code 风格 JSON。原文只在本次 HTTPS 请求内存在，不写入本地存储。
    pub async fn import_model_configuration(
        &self,
        gateway_url: &str,
        bearer: &SecretString,
        configuration: &SecretString,
    ) -> AppResult<ModelConfigurationResult> {
        use secrecy::ExposeSecret as _;

        let base = validate_gateway_url(gateway_url)?;
        let response = self
            .authorized_post(&format!("{base}/api/user/model-credentials/import"), bearer)
            .json(&json!({
                "configuration": configuration.expose_secret(),
                "activate_as_default": true,
            }))
            .timeout(Duration::from_secs(20))
            .send()
            .await?;
        parse_model_configuration_response(ensure_success(response).await?).await
    }

    /// P5 BYOK：逻辑撤销自有凭据；进行中任务由服务端 fail-closed 处理。
    pub async fn delete_byok_credential(
        &self,
        gateway_url: &str,
        bearer: &SecretString,
        credential_id: i64,
    ) -> AppResult<()> {
        let base = validate_gateway_url(gateway_url)?;
        let response = self
            .authorized_delete(
                &format!("{base}/api/user/model-credentials/{credential_id}"),
                bearer,
            )
            .timeout(Duration::from_secs(15))
            .send()
            .await?;
        ensure_success(response).await?;
        Ok(())
    }

    /// 拉取当前可用聊天模型列表（用户级模型选择器数据源）。
    pub async fn list_chat_models(
        &self,
        gateway_url: &str,
        api_key: &SecretString,
    ) -> AppResult<Vec<ModelOption>> {
        let base = validate_gateway_url(gateway_url)?;
        let response = self
            .authorized_get(
                &format!("{base}/api/system/model-providers/models/v2?model_type=chat"),
                api_key,
            )
            .timeout(Duration::from_secs(20))
            .send()
            .await?;
        let response = ensure_success(response).await?;
        let value = response
            .json::<Value>()
            .await
            .map_err(|error| AppError::Protocol(error.to_string()))?;
        let mut models: Vec<ModelOption> = Vec::new();
        if let Some(providers) = value.get("data").and_then(Value::as_object) {
            for (provider_id, provider) in providers {
                let provider_name = provider
                    .get("provider_display_name")
                    .and_then(Value::as_str)
                    .unwrap_or(provider_id);
                if let Some(items) = provider.get("models").and_then(Value::as_array) {
                    for item in items {
                        let spec = item.get("spec").and_then(Value::as_str).unwrap_or("");
                        let display = item
                            .get("display_name")
                            .and_then(Value::as_str)
                            .unwrap_or(spec);
                        if spec.is_empty() {
                            continue;
                        }
                        models.push(ModelOption {
                            spec: spec.to_string(),
                            label: format!("{display} · {provider_name}"),
                        });
                    }
                }
            }
        }
        models.sort_by(|a, b| a.label.cmp(&b.label));
        Ok(models)
    }

    pub async fn get_chat_model_preference(
        &self,
        gateway_url: &str,
        api_key: &SecretString,
    ) -> AppResult<Option<String>> {
        let base = validate_gateway_url(gateway_url)?;
        let response = self
            .authorized_get(&format!("{base}/api/user/model-preference"), api_key)
            .timeout(Duration::from_secs(20))
            .send()
            .await?;
        let response = ensure_success(response).await?;
        let value = response
            .json::<Value>()
            .await
            .map_err(|error| AppError::Protocol(error.to_string()))?;
        Ok(value
            .get("chat_model_spec")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned))
    }

    pub async fn set_chat_model_preference(
        &self,
        gateway_url: &str,
        api_key: &SecretString,
        model_spec: Option<&str>,
    ) -> AppResult<()> {
        let base = validate_gateway_url(gateway_url)?;
        let response = self
            .authorized_put(&format!("{base}/api/user/model-preference"), api_key)
            .json(&json!({"chat_model_spec": model_spec}))
            .timeout(Duration::from_secs(20))
            .send()
            .await?;
        ensure_success(response).await?;
        Ok(())
    }
}

/// 刷新端点返回的轮换结果。
#[derive(Debug, Clone)]
pub struct RotatedSession {
    pub access_token: String,
    pub refresh_token: String,
}

/// 服务端签发的设备会话对（onboarding 与 CLI 两种兑换共用同一形状）。
#[derive(Debug, Clone)]
pub struct SessionPair {
    pub session_id: String,
    pub access_token: String,
    pub refresh_token: String,
    pub access_expires_in: i64,
}

/// 企业激活码兑换结果：只有会话对，无任何静态 Key。
#[derive(Debug, Clone)]
pub struct OnboardingExchange {
    pub session: SessionPair,
    pub user_name: String,
    pub account_scope_id: String,
}

/// 设备码创建结果（浏览器授权页与轮询参数）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceCodeStart {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: String,
    pub expires_in: i64,
    pub interval: i64,
}

/// 设备码兑换结果：会话对为主，过渡静态 Key 仅用于旧服务端兜底与即时撤销。
#[derive(Debug, Clone)]
pub struct DeviceCodeExchange {
    pub session: Option<SessionPair>,
    pub user_name: String,
    pub account_scope_id: String,
    pub transition_key_id: Option<i64>,
    pub transition_key_secret: Option<String>,
}

/// 设备会话摘要（设置页「账号与安全」展示用）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceSessionView {
    pub session_id: String,
    pub created_at: Option<String>,
    pub last_refreshed_at: Option<String>,
}

/// 用户权益摘要（GET /api/user/quota）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaSummary {
    pub daily_run_limit: Option<i64>,
    pub monthly_token_limit: Option<i64>,
    pub model_access_policy: String,
    pub byok_platform_token_exempt: bool,
    pub has_active_byok: bool,
}

/// 用户用量摘要（GET /api/user/usage）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSummary {
    pub daily: Vec<UsageDay>,
    pub monthly_tokens: i64,
    pub monthly_platform_tokens: i64,
    pub monthly_byok_tokens: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageDay {
    pub date: String,
    pub run_count: i64,
    pub tokens: i64,
}

/// 账号、密码和 API Key 联合认证后的服务端权威身份。
#[derive(Debug, Clone)]
pub struct DesktopLoginIdentity {
    pub account_scope_id: String,
    pub user_name: String,
    pub user_uid: String,
}

/// P5 BYOK：用户自有模型凭据（服务端仅返回掩码，无明文）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ByokCredential {
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelConfigurationResult {
    pub credential_id: i64,
    pub model_spec: String,
    pub ignored_fields: Vec<String>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelOption {
    pub spec: String,
    pub label: String,
}

fn optional_string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

async fn parse_model_configuration_response(
    response: reqwest::Response,
) -> AppResult<ModelConfigurationResult> {
    let value = response
        .json::<Value>()
        .await
        .map_err(|error| AppError::Protocol(error.to_string()))?;
    let credential_id = value
        .get("credential_id")
        .and_then(Value::as_i64)
        .ok_or_else(|| AppError::Protocol("模型配置响应缺少 credential_id".into()))?;
    let model_spec = value
        .get("model_spec")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppError::Protocol("模型配置响应缺少 model_spec".into()))?
        .to_owned();
    let ignored_fields = value
        .get("ignored_fields")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    Ok(ModelConfigurationResult {
        credential_id,
        model_spec,
        ignored_fields,
    })
}

fn parse_desktop_login_response(value: &Value) -> AppResult<DesktopLoginIdentity> {
    let account_scope_id = value
        .get("account_scope_id")
        .and_then(Value::as_str)
        .filter(|value| value.starts_with("yxacct_") && value.len() >= 24)
        .ok_or_else(|| AppError::Protocol("登录响应缺少账号作用域标识".into()))?
        .to_string();
    let user_name = value
        .get("username")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppError::Protocol("登录响应缺少用户名".into()))?
        .to_string();
    let user_uid = value
        .get("uid")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppError::Protocol("登录响应缺少用户 UID".into()))?
        .to_string();
    Ok(DesktopLoginIdentity {
        account_scope_id,
        user_name,
        user_uid,
    })
}

/// 解析 onboarding exchange：`{session, user:{uid,username}, account_scope_id}`。
/// 企业开户路径**绝不携带 api_key 字段**——出现即视为契约违约，拒绝绑定。
fn parse_onboarding_exchange(value: &Value) -> AppResult<OnboardingExchange> {
    if value.get("api_key").is_some() || value.get("secret").is_some() {
        return Err(AppError::Protocol(
            "服务端在激活码兑换中签发了静态 Key，违反企业开户契约；已拒绝绑定".into(),
        ));
    }
    let session = parse_session_pair(value.get("session"))?;
    let user_name = value
        .pointer("/user/username")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppError::Protocol("激活码兑换响应缺少用户名".into()))?
        .to_string();
    let account_scope_id = value
        .get("account_scope_id")
        .and_then(Value::as_str)
        .filter(|value| value.starts_with("yxacct_") && value.len() >= 24)
        .ok_or_else(|| AppError::Protocol("激活码兑换响应缺少账号作用域标识".into()))?
        .to_string();
    Ok(OnboardingExchange {
        session,
        user_name,
        account_scope_id,
    })
}

/// 解析设备码创建响应：六字段全量校验，缺一即拒绝（轮询依赖这些参数）。
fn parse_device_code_start(value: &Value) -> AppResult<DeviceCodeStart> {
    let required = |key: &str| {
        value
            .get(key)
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| AppError::Protocol(format!("设备码响应字段缺失：{key}")))
    };
    let expires_in = value
        .get("expires_in")
        .and_then(Value::as_i64)
        .filter(|seconds| *seconds > 0)
        .ok_or_else(|| AppError::Protocol("设备码响应缺少有效期".into()))?;
    let interval = value
        .get("interval")
        .and_then(Value::as_i64)
        .filter(|seconds| *seconds > 0)
        .ok_or_else(|| AppError::Protocol("设备码响应缺少轮询间隔".into()))?;
    Ok(DeviceCodeStart {
        device_code: required("device_code")?,
        user_code: required("user_code")?,
        verification_uri: required("verification_uri")?,
        verification_uri_complete: required("verification_uri_complete")?,
        expires_in,
        interval,
    })
}

/// 解析设备码兑换响应：`{api_key:dict, secret, user, account_scope_id, session|null}`。
/// 与 onboarding 结构不同（多 api_key/secret、session 可空），必须分开解析。
fn parse_device_code_exchange(value: &Value) -> AppResult<Option<DeviceCodeExchange>> {
    let user_name = value
        .pointer("/user/username")
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            value
                .pointer("/user/display_name")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .ok_or_else(|| AppError::Protocol("设备码兑换响应缺少用户名".into()))?;
    let account_scope_id = value
        .get("account_scope_id")
        .and_then(Value::as_str)
        .filter(|text| text.starts_with("yxacct_") && text.len() >= 24)
        .ok_or_else(|| AppError::Protocol("设备码兑换响应缺少账号作用域标识".into()))?
        .to_string();
    let session = match value.get("session").filter(|session| !session.is_null()) {
        Some(session_value) => Some(parse_session_pair(Some(session_value))?),
        None => None,
    };
    let transition_key_id = value
        .pointer("/api_key/id")
        .and_then(Value::as_i64)
        .or_else(|| value.pointer("/api_key/api_key_id").and_then(Value::as_i64));
    let transition_key_secret = value
        .get("secret")
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .map(str::to_owned);
    Ok(Some(DeviceCodeExchange {
        session,
        user_name,
        account_scope_id,
        transition_key_id,
        transition_key_secret,
    }))
}

/// 解析服务端签发的会话对；字段不全或为空即失败关闭。
fn parse_session_pair(value: Option<&Value>) -> AppResult<SessionPair> {
    let value = value.ok_or_else(|| AppError::Protocol("会话响应缺失".into()))?;
    let field = |key: &str| {
        value
            .get(key)
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| AppError::Protocol("会话响应字段缺失".into()))
    };
    Ok(SessionPair {
        session_id: field("session_id")?,
        access_token: field("access_token")?,
        refresh_token: field("refresh_token")?,
        access_expires_in: value
            .get("access_expires_in")
            .and_then(Value::as_i64)
            .unwrap_or(30 * 60),
    })
}

fn gateway_trace_id(headers: &reqwest::header::HeaderMap) -> Option<String> {
    headers
        .get(GATEWAY_TRACE_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

fn connection_error_is_retryable(error: &AppError) -> bool {
    matches!(error, AppError::Network(_) | AppError::ServiceUnavailable)
}

fn connection_error_for_gateway(gateway_url: &str, error: AppError) -> AppError {
    // Url::host_str() 对 IPv6 返回带方括号的形式（"[::1]"），必须去括号后再比较。
    let is_loopback = Url::parse(gateway_url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
        .is_some_and(|host| {
            let normalized = host.trim_start_matches('[').trim_end_matches(']');
            matches!(normalized, "127.0.0.1" | "localhost" | "::1")
        });
    if is_loopback && connection_error_is_retryable(&error) {
        AppError::LocalServiceUnavailable
    } else {
        error
    }
}

#[derive(Default)]
pub struct ProgressText {
    message_id: Option<String>,
    raw_text: String,
    text: String,
}

fn ascii_tag_at(text: &str, start: usize) -> Option<(usize, bool)> {
    let remaining = text.get(start..)?;
    let bytes = remaining.as_bytes();
    let mut pos = bytes.iter().take_while(|&&byte| byte == b'\\').count();

    let left = ["<", "&lt;", "&#60;", "&#x3c;"].into_iter().find(|token| {
        remaining
            .get(pos..pos + token.len())
            .is_some_and(|value| value.eq_ignore_ascii_case(token))
    })?;
    pos += left.len();
    while bytes.get(pos).is_some_and(u8::is_ascii_whitespace) {
        pos += 1;
    }
    let is_open = bytes.get(pos) != Some(&b'/');
    if !is_open {
        pos += 1;
        while bytes.get(pos).is_some_and(u8::is_ascii_whitespace) {
            pos += 1;
        }
    }
    const THINK: &str = "think";
    if !remaining
        .get(pos..pos + THINK.len())
        .is_some_and(|value| value.eq_ignore_ascii_case(THINK))
    {
        return None;
    }
    pos += THINK.len();
    while bytes.get(pos).is_some_and(u8::is_ascii_whitespace) {
        pos += 1;
    }
    let right = [">", "&gt;", "&#62;", "&#x3e;"].into_iter().find(|token| {
        remaining
            .get(pos..pos + token.len())
            .is_some_and(|value| value.eq_ignore_ascii_case(token))
    })?;
    pos += right.len();
    Some((pos, is_open))
}

fn hold_partial_opening_tag(text: &str) -> &str {
    let pending = pending_tag_prefix_len(text);
    &text[..text.len() - pending]
}

/// Length in bytes of the trailing span of `text` that could still become a
/// reasoning tag once more characters arrive: a bracket token ("<", "&lt",
/// "&#60", "&#x3c"), one run of backslashes, or the whole of "<", "< t",
/// "< / t", "&lt; think " etc.  Streaming emitters hold this suffix back so a
/// provider tag split across many deltas never flashes on screen; a complete
/// message reports 0.
fn pending_tag_prefix_len(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }
    // A trailing backslash run may still open an escaped tag.
    let backslash_run = text.bytes().rev().take_while(|&b| b == b'\\').count();
    if backslash_run > 0 {
        return backslash_run;
    }
    let indices: Vec<usize> = text.char_indices().map(|(index, _)| index).collect();
    for &start in indices.iter().rev() {
        let tail = &text[start..];
        if let Some(token_len) = bracket_token_len(tail)
            && is_tag_progress(&tail[token_len..])
        {
            return tail.len();
        }
    }
    0
}

fn is_tag_progress(s: &str) -> bool {
    let bytes = s.as_bytes();
    let mut pos = 0;
    let skip_whitespace = |pos: &mut usize| {
        while *pos < bytes.len() && bytes[*pos].is_ascii_whitespace() {
            *pos += 1;
        }
    };
    skip_whitespace(&mut pos);
    if pos < bytes.len() && bytes[pos] == b'/' {
        pos += 1;
        skip_whitespace(&mut pos);
    }
    const THINK: [u8; 5] = *b"think";
    let mut letter_index = 0;
    while pos < bytes.len()
        && letter_index < THINK.len()
        && bytes[pos].to_ascii_lowercase() == THINK[letter_index]
    {
        pos += 1;
        letter_index += 1;
    }
    skip_whitespace(&mut pos);
    pos == bytes.len()
}

/// Length of a reasoning open/close bracket token at the start of `s`, if any.
/// Mirrors the bracket alternation of the Python/TS `TAG_PATTERN`.
fn bracket_token_len(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let backslashes = bytes.iter().take_while(|&&b| b == b'\\').count();
    let rest = &s[backslashes..];
    for token in ["<", "&lt;", "&#60;", "&#x3c;"] {
        let comparable_len = rest.len().min(token.len());
        if rest
            .get(..comparable_len)
            .zip(token.get(..comparable_len))
            .is_some_and(|(value, prefix)| value.eq_ignore_ascii_case(prefix))
            && (rest.len() <= token.len()
                || rest
                    .get(..token.len())
                    .is_some_and(|value| value.eq_ignore_ascii_case(token)))
        {
            return Some(backslashes + comparable_len);
        }
    }
    None
}

/// Return only user-facing answer text. Unclosed reasoning fails closed.
pub fn sanitize_visible_model_text(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }

    let mut visible = String::new();
    let mut cursor = 0;
    let mut depth = 0_u32;
    let mut position = 0;
    while position < text.len() {
        if let Some((length, is_open)) = ascii_tag_at(text, position) {
            if depth == 0 {
                visible.push_str(&text[cursor..position]);
            }
            if is_open {
                depth = depth.saturating_add(1);
            } else {
                depth = depth.saturating_sub(1);
            }
            position += length;
            cursor = position;
            continue;
        }
        position += text[position..].chars().next().map_or(1, char::len_utf8);
    }
    if depth == 0 {
        visible.push_str(&text[cursor..]);
    }
    visible
}

impl ProgressText {
    pub fn apply(&mut self, value: &Value) -> Option<String> {
        let payload = value.get("payload")?;
        let chunks: Vec<&Value> =
            if let Some(items) = payload.get("items").and_then(Value::as_array) {
                items.iter().collect()
            } else {
                payload.get("chunk").into_iter().collect()
            };

        let mut changed = false;
        for chunk in chunks {
            let stream_event = chunk.get("stream_event");
            let semantic_delta = stream_event
                .filter(|event| event.get("type").and_then(Value::as_str) == Some("message_delta"))
                .and_then(|event| event.get("content").and_then(Value::as_str))
                .filter(|content| !content.is_empty());

            if let Some(delta) = semantic_delta {
                let incoming_message_id = stream_event
                    .and_then(|event| event.get("message_id"))
                    .and_then(Value::as_str)
                    .filter(|id| !id.is_empty());
                if incoming_message_id != self.message_id.as_deref() {
                    self.message_id = incoming_message_id.map(str::to_owned);
                    self.raw_text.clear();
                    self.text.clear();
                }
                self.raw_text.push_str(delta);
                let visible = sanitize_visible_model_text(&self.raw_text);
                let emit = hold_partial_opening_tag(&visible);
                if emit != self.text {
                    self.text = emit.to_owned();
                    changed = true;
                }
                continue;
            }

            if stream_event.is_none()
                && let Some(delta) = chunk
                    .get("response")
                    .and_then(Value::as_str)
                    .filter(|content| !content.is_empty())
            {
                self.raw_text.push_str(delta);
                let visible = sanitize_visible_model_text(&self.raw_text);
                let emit = hold_partial_opening_tag(&visible);
                if emit != self.text {
                    self.text = emit.to_owned();
                    changed = true;
                }
            }
        }

        changed.then(|| self.text.clone())
    }
}

pub fn terminal_status(value: &Value) -> Option<&str> {
    value
        .pointer("/payload/status")
        .and_then(Value::as_str)
        .or_else(|| {
            value
                .pointer("/payload/chunk/status")
                .and_then(Value::as_str)
        })
}

fn final_output(value: &Value) -> String {
    value
        .get("output")
        .and_then(Value::as_str)
        .or_else(|| {
            value
                .pointer("/choices/0/messages/0/content")
                .and_then(Value::as_str)
        })
        .map(sanitize_visible_model_text)
        .unwrap_or_default()
}

fn parse_default_agent_slug(value: &Value) -> AppResult<String> {
    value
        .pointer("/agent/slug")
        .or_else(|| value.pointer("/agent/agent_id"))
        .and_then(Value::as_str)
        .filter(|slug| !slug.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| AppError::Protocol("默认智能体响应缺少 slug".into()))
}

pub fn validate_authoritative_run_context(
    context: &ServerRunContext,
    expected_agent_slug: &str,
    expected_thread_id: &str,
    expected_request_id: &str,
) -> AppResult<()> {
    let compatible_protocol = context
        .protocol_version
        .as_deref()
        .and_then(|version| version.split_once('.'))
        .and_then(|(major, minor)| Some((major.parse::<u64>().ok()?, minor.parse::<u64>().ok()?)))
        .is_some_and(|(major, minor)| major > 1 || (major == 1 && minor >= 2));
    if !compatible_protocol
        || context.result_authority.as_deref() != Some("yuxi_server")
        || context.agent_slug.as_deref() != Some(expected_agent_slug)
        || context.thread_id.as_deref() != Some(expected_thread_id)
        || context.request_id.as_deref() != Some(expected_request_id)
    {
        return Err(AppError::Protocol(
            "服务端 AgentRun 权威上下文与桌面请求不一致；请更新并重启 rice-endosperm-agent 与 APISIX"
                .into(),
        ));
    }
    Ok(())
}

fn parse_run_result(value: &Value) -> RunResult {
    RunResult {
        status: value
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_owned(),
        output: final_output(value),
        error: value
            .get("error")
            .and_then(|error| {
                error
                    .as_str()
                    .or_else(|| error.get("message").and_then(Value::as_str))
            })
            .map(str::to_owned),
        error_code: value
            .pointer("/error/type")
            .and_then(Value::as_str)
            .or_else(|| value.pointer("/error/code").and_then(Value::as_str))
            .map(str::to_owned),
        context: value
            .get("run_context")
            .cloned()
            .and_then(|context| serde_json::from_value(context).ok())
            .unwrap_or_default(),
    }
}

async fn ensure_success(response: Response) -> AppResult<Response> {
    if response.status().is_success() {
        Ok(response)
    } else {
        Err(response_error(response).await)
    }
}

/// 错误体解析结果：兼容服务端三种形态——
///
/// 1. `{"detail": {"code", "message", "action"}}`（AgentRun/配额类业务错误）
/// 2. `{"detail": {"error", "message"}}`（onboarding / cli-sessions 授权错误）
/// 3. `{"detail": "纯字符串"}`（refresh 端点与部分 FastAPI 默认错误）
///
/// 以及网关自身的 `{"error_msg": ...}` 与 text/plain（request-validation 拒绝体）。
#[derive(Default)]
struct ParsedErrorBody {
    code: Option<String>,
    action: Option<String>,
    message: Option<String>,
}

fn parse_error_body(body: &str) -> ParsedErrorBody {
    let Ok(value) = serde_json::from_str::<Value>(body) else {
        let trimmed = body.trim();
        return ParsedErrorBody {
            message: (!trimmed.is_empty()).then(|| trimmed.to_owned()),
            ..Default::default()
        };
    };
    let mut parsed = ParsedErrorBody::default();
    if let Some(detail) = value.get("detail").filter(|detail| !detail.is_null()) {
        if let Some(text) = detail.as_str() {
            parsed.message = (!text.trim().is_empty()).then(|| text.to_owned());
        } else {
            parsed.code = detail
                .get("code")
                .and_then(Value::as_str)
                .or_else(|| detail.get("error").and_then(Value::as_str))
                .map(str::to_owned);
            parsed.action = detail
                .get("action")
                .and_then(Value::as_str)
                .map(str::to_owned);
            parsed.message = detail
                .get("message")
                .and_then(Value::as_str)
                .map(str::to_owned);
        }
    }
    if parsed.message.is_none() {
        parsed.message = value
            .get("message")
            .and_then(Value::as_str)
            .map(str::to_owned);
    }
    if parsed.message.is_none() {
        parsed.message = value
            .get("error_msg")
            .and_then(Value::as_str)
            .map(|text| format!("网关错误：{text}"))
            .or_else(|| {
                value
                    .get("message")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            });
    }
    parsed
}

fn default_status_message(status: u16) -> String {
    match status {
        400 => "请求被拒绝（HTTP 400）：请求字段与服务端契约不匹配，请升级桌面端".into(),
        401 => "认证失败，请检查凭证是否有效或已被禁用".into(),
        403 => "当前请求被服务端拒绝，请检查账号状态或模型接入策略".into(),
        404 => {
            "请求的接口不存在（HTTP 404）：网关可能未放行该路由，请联系管理员更新网关配置".into()
        }
        429 => "请求过于频繁，请稍后重试".into(),
        _ => format!("HTTP {status}"),
    }
}

async fn response_error(response: Response) -> AppError {
    let status = response.status().as_u16();
    // 必须在消费 body 前读取响应头（reqwest 消费 body 不影响 headers，但显式先取）。
    let trace_id = gateway_trace_id(response.headers());
    let lock_remaining = response
        .headers()
        .get("X-Lock-Remaining")
        .and_then(|value| value.to_str().ok())
        .and_then(|text| text.trim().parse::<u64>().ok());
    let body = response.text().await.unwrap_or_default();
    let parsed = parse_error_body(&body);
    let message = parsed
        .message
        .filter(|text| !text.trim().is_empty())
        .unwrap_or_else(|| default_status_message(status));
    let message = apply_lock_remaining(status, message, lock_remaining);
    AppError::from_server_parts(status, message, parsed.code, parsed.action, trace_id)
}

/// 423 登录锁定：把服务端 `X-Lock-Remaining` 头换算成可行动的等待提示。
fn apply_lock_remaining(status: u16, message: String, remaining: Option<u64>) -> String {
    if status == 423 && remaining.is_some_and(|seconds| seconds > 0) {
        format!("{message}（请 {} 秒后重试）", remaining.unwrap_or_default())
    } else {
        message
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::error::AppError;

    use super::{
        CreateRunRequest, ProgressText, RunRequestMeta, TmpAttachmentConfirmItem,
        connection_error_for_gateway, default_status_message, final_output,
        parse_default_agent_slug, parse_desktop_login_response, parse_device_code_exchange,
        parse_device_code_start, parse_error_body, parse_onboarding_exchange, parse_run_result,
        sanitize_visible_model_text, terminal_status, validate_authoritative_run_context,
    };

    #[test]
    fn serializes_the_same_native_agent_run_contract_as_web() {
        let value = serde_json::to_value(CreateRunRequest {
            query: Some("水稻胚乳发育的关键调控基因有哪些？"),
            agent_slug: "default-chatbot",
            thread_id: "thread-1",
            meta: RunRequestMeta {
                request_id: "desktop-request-123456",
                client: "rice-endosperm-desktop",
                attachment_file_ids: vec![],
            },
            model_spec: None,
            resume: None,
            created_by_run_id: None,
        })
        .expect("serialize native AgentRun request");

        assert_eq!(value["agent_slug"], "default-chatbot");
        assert_eq!(value["thread_id"], "thread-1");
        assert_eq!(value["meta"]["request_id"], "desktop-request-123456");
        assert_eq!(value["meta"]["client"], "rice-endosperm-desktop");
        assert!(value.get("messages").is_none());
        assert!(value.get("async_mode").is_none());
        assert!(value.get("model_spec").is_none());
        assert!(value.get("resume").is_none());
    }

    #[test]
    fn serializes_resume_payload_only_when_present() {
        let resume = json!({"values": {"step1": "确认继续"}});
        let value = serde_json::to_value(CreateRunRequest {
            query: None,
            agent_slug: "default-chatbot",
            thread_id: "thread-1",
            meta: RunRequestMeta {
                request_id: "desktop-request-resume-1",
                client: "rice-endosperm-desktop",
                attachment_file_ids: vec![],
            },
            model_spec: None,
            resume: Some(&resume),
            created_by_run_id: Some("run-parent-1"),
        })
        .expect("serialize resume run request");

        assert_eq!(value["created_by_run_id"], "run-parent-1");
        assert_eq!(value["resume"]["values"]["step1"], "确认继续");
        // query 必须整体省略：网关 schema 的 string 类型不接受 null
        assert!(value.get("query").is_none());
    }

    #[test]
    fn serializes_confirmed_attachment_ids_inside_run_meta() {
        let value = serde_json::to_value(CreateRunRequest {
            query: Some("总结附件"),
            agent_slug: "default-chatbot",
            thread_id: "thread-1",
            meta: RunRequestMeta {
                request_id: "desktop-request-with-file",
                client: "rice-endosperm-desktop",
                attachment_file_ids: vec!["file-1", "file-2"],
            },
            model_spec: None,
            resume: None,
            created_by_run_id: None,
        })
        .expect("serialize attachment run request");

        assert_eq!(
            value["meta"]["attachment_file_ids"],
            json!(["file-1", "file-2"])
        );
    }

    #[test]
    fn confirm_item_omits_null_optional_fields_for_gateway_closed_set() {
        // 网关 confirm schema 的可选字段是 type:string，null 会被 400 拒绝；
        // 未解析附件与未知 MIME 是合法场景，字段必须整体省略。
        let unparsed = TmpAttachmentConfirmItem {
            file_name: "evidence.pdf",
            file_type: None,
            bucket_name: "chat-tmp",
            object_name: "tmp/o-1",
            parsed_object_name: None,
            truncated: false,
        };
        let value = serde_json::to_value(&unparsed).expect("serialize unparsed confirm item");
        assert!(
            value.get("file_type").is_none(),
            "file_type=null 会被网关 400"
        );
        assert!(
            value.get("parsed_object_name").is_none(),
            "parsed_object_name=null 会被网关 400"
        );
        assert_eq!(value["truncated"], false);

        let parsed = TmpAttachmentConfirmItem {
            file_name: "evidence.pdf",
            file_type: Some("application/pdf"),
            bucket_name: "chat-tmp",
            object_name: "tmp/o-1",
            parsed_object_name: Some("tmp/p-1"),
            truncated: true,
        };
        let value = serde_json::to_value(&parsed).expect("serialize parsed confirm item");
        assert_eq!(value["file_type"], "application/pdf");
        assert_eq!(value["parsed_object_name"], "tmp/p-1");
    }

    #[test]
    fn parses_authoritative_default_agent_from_server() {
        assert_eq!(
            parse_default_agent_slug(&json!({"agent": {"slug": "default-chatbot"}}))
                .expect("parse default agent"),
            "default-chatbot"
        );
        assert!(parse_default_agent_slug(&json!({"agent": {}})).is_err());
    }

    #[test]
    fn validates_server_owned_agent_run_context() {
        let context = serde_json::from_value(json!({
            "protocol_version": "1.2",
            "agent_slug": "default-chatbot",
            "thread_id": "thread-1",
            "request_id": "request-1",
            "result_authority": "yuxi_server"
        }))
        .expect("decode authoritative context");
        assert!(
            validate_authoritative_run_context(
                &context,
                "default-chatbot",
                "thread-1",
                "request-1"
            )
            .is_ok()
        );
        assert!(
            validate_authoritative_run_context(&context, "another-agent", "thread-1", "request-1")
                .is_err()
        );
    }

    #[test]
    fn decodes_atomic_desktop_login_contract() {
        let identity = parse_desktop_login_response(&json!({
            "account_scope_id": "yxacct_0123456789abcdef0123456789abcdef",
            "username": "Rice Researcher",
            "uid": "rice_researcher",
            "api_key_id": 42,
            "key_prefix": "yxkey_123456",
            "expires_at": "2026-11-24T00:00:00"
        }))
        .expect("decode desktop login response");

        assert_eq!(
            identity.account_scope_id,
            "yxacct_0123456789abcdef0123456789abcdef"
        );
        assert_eq!(identity.user_name, "Rice Researcher");
        assert_eq!(identity.user_uid, "rice_researcher");
    }

    #[test]
    fn decodes_onboarding_exchange_without_any_static_key() {
        let exchange = parse_onboarding_exchange(&json!({
            "session": {
                "session_id": "fam-1",
                "access_token": "eyJhbGciOi.eyJzdWIiOjdcInw.c2ln",
                "refresh_token": "yxrt_0123456789abcdef",
                "access_expires_in": 1800
            },
            "user": {"uid": "rice_researcher", "username": "Rice Researcher"},
            "account_scope_id": "yxacct_0123456789abcdef0123456789abcdef"
        }))
        .expect("decode onboarding exchange");

        assert_eq!(exchange.session.session_id, "fam-1");
        assert_eq!(exchange.session.access_expires_in, 1800);
        assert_eq!(exchange.user_name, "Rice Researcher");
        assert_eq!(
            exchange.account_scope_id,
            "yxacct_0123456789abcdef0123456789abcdef"
        );
    }

    #[test]
    fn rejects_onboarding_exchange_that_issuances_static_key() {
        let mut payload = json!({
            "session": {
                "session_id": "fam-1",
                "access_token": "eyJhbGciOi.eyJzdWIiOjdcInw.c2ln",
                "refresh_token": "yxrt_0123456789abcdef",
                "access_expires_in": 1800
            },
            "user": {"uid": "u", "username": "n"},
            "account_scope_id": "yxacct_0123456789abcdef0123456789abcdef"
        });
        payload["secret"] = json!("yxkey_should_not_exist");
        assert!(parse_onboarding_exchange(&payload).is_err());
    }

    #[test]
    fn decodes_device_code_start_and_exchange() {
        let start = parse_device_code_start(&json!({
            "device_code": "dc-1",
            "user_code": "ABCD-1234",
            "verification_uri": "https://web.example.cn/auth/cli/authorize",
            "verification_uri_complete": "https://web.example.cn/auth/cli/authorize?user_code=ABCD-1234",
            "expires_in": 600,
            "interval": 2
        }))
        .expect("decode device code start");
        assert_eq!(start.user_code, "ABCD-1234");
        assert_eq!(start.interval, 2);

        let exchange = parse_device_code_exchange(&json!({
            "api_key": {"id": 77, "key_prefix": "yxkey_99"},
            "secret": "yxkey_transition_0123456789",
            "user": {"uid": "u", "username": "Rice"},
            "account_scope_id": "yxacct_0123456789abcdef0123456789abcdef",
            "session": {
                "session_id": "fam-2",
                "access_token": "a.b.c2lnbmF0dXJl",
                "refresh_token": "yxrt_fedcba9876543210",
                "access_expires_in": 1800
            }
        }))
        .expect("decode device exchange")
        .expect("session present");
        assert_eq!(
            exchange.session.as_ref().expect("session").session_id,
            "fam-2"
        );
        assert_eq!(exchange.transition_key_id, Some(77));
        assert!(
            exchange
                .transition_key_secret
                .as_deref()
                .is_some_and(|secret| secret.starts_with("yxkey_"))
        );

        // 旧服务端无 session 字段：静态 Key 兜底路径必须仍可解析
        let legacy = parse_device_code_exchange(&json!({
            "api_key": {"id": 78},
            "secret": "yxkey_legacy_0123456789",
            "user": {"uid": "u", "username": "Rice"},
            "account_scope_id": "yxacct_0123456789abcdef0123456789abcdef"
        }))
        .expect("decode legacy exchange")
        .expect("legacy exchange present");
        assert!(legacy.session.is_none());
    }

    #[test]
    fn parses_all_server_error_body_dialects() {
        // 形态 1：detail.{code,message,action}（配额类业务错误）
        let quota = parse_error_body(
            r#"{"detail": {"code": "daily_run_quota_exceeded", "message": "今日问答次数已达上限", "action": "contact_admin"}}"#,
        );
        assert_eq!(quota.code.as_deref(), Some("daily_run_quota_exceeded"));
        assert_eq!(quota.action.as_deref(), Some("contact_admin"));
        assert_eq!(quota.message.as_deref(), Some("今日问答次数已达上限"));

        // 形态 2：detail.{error,message}（onboarding / cli 授权错误）
        let auth =
            parse_error_body(r#"{"detail": {"error": "expired", "message": "激活码已过期"}}"#);
        assert_eq!(auth.code.as_deref(), Some("expired"));
        assert_eq!(auth.message.as_deref(), Some("激活码已过期"));

        // 形态 3：detail 纯字符串（refresh 端点）
        let refresh = parse_error_body(r#"{"detail": "检测到刷新令牌重放，会话已撤销"}"#);
        assert_eq!(
            refresh.message.as_deref(),
            Some("检测到刷新令牌重放，会话已撤销")
        );
        assert!(refresh.code.is_none());

        // 网关 404 与 text/plain 校验拒绝体
        let gateway = parse_error_body(r#"{"error_msg": "404 Route Not Found"}"#);
        assert_eq!(
            gateway.message.as_deref(),
            Some("网关错误：404 Route Not Found")
        );
        let plain = parse_error_body("invalid request");
        assert_eq!(plain.message.as_deref(), Some("invalid request"));

        assert!(default_status_message(404).contains("网关"));
    }

    #[test]
    fn login_lockdown_appends_actionable_wait_seconds() {
        use super::apply_lock_remaining;
        assert_eq!(
            apply_lock_remaining(423, "登录失败次数过多".into(), Some(300)),
            "登录失败次数过多（请 300 秒后重试）"
        );
        // 非锁定状态或剩余 0 秒不加尾巴
        assert_eq!(
            apply_lock_remaining(429, "请求过于频繁".into(), Some(300)),
            "请求过于频繁"
        );
        assert_eq!(
            apply_lock_remaining(423, "登录失败次数过多".into(), Some(0)),
            "登录失败次数过多"
        );
    }

    #[test]
    fn groups_deltas_by_message_and_ignores_duplicate_legacy_response() {
        let mut progress = ProgressText::default();
        let first = json!({"payload": {"items": [{
            "response": "水稻",
            "stream_event": {"type": "message_delta", "message_id": "message-1", "content": "水稻"}
        }]}});
        assert_eq!(progress.apply(&first).as_deref(), Some("水稻"));

        let second = json!({"payload": {"chunk": {
            "response": "胚乳",
            "stream_event": {"type": "message_delta", "message_id": "message-1", "content": "胚乳"}
        }}});
        assert_eq!(progress.apply(&second).as_deref(), Some("水稻胚乳"));

        let next_message = json!({"payload": {"chunk": {
            "stream_event": {"type": "message_delta", "message_id": "message-2", "content": "最终回答"}
        }}});
        assert_eq!(progress.apply(&next_message).as_deref(), Some("最终回答"));
    }

    #[test]
    fn excludes_reasoning_and_tool_events_from_visible_progress() {
        let mut progress = ProgressText::default();
        let reasoning = json!({"payload": {"items": [
            {"stream_event": {"type": "message_delta", "message_id": "message-1", "reasoning_content": "内部思考"}},
            {"stream_event": {"type": "tool_call", "message_id": "message-1", "name": "query_knowledge_scope"}}
        ]}});
        assert_eq!(progress.apply(&reasoning), None);
    }

    #[test]
    fn redacts_tagged_reasoning_across_stream_boundaries() {
        let mut progress = ProgressText::default();
        let first = json!({"payload": {"chunk": {
            "stream_event": {"type": "message_delta", "message_id": "message-1", "content": "<th"}
        }}});
        assert_eq!(progress.apply(&first), None);

        let second = json!({"payload": {"chunk": {
            "stream_event": {"type": "message_delta", "message_id": "message-1", "content": "ink>private chain"}
        }}});
        assert_eq!(progress.apply(&second), None);

        let third = json!({"payload": {"chunk": {
            "stream_event": {"type": "message_delta", "message_id": "message-1", "content": "</think>你好！"}
        }}});
        assert_eq!(progress.apply(&third).as_deref(), Some("你好！"));
    }

    #[test]
    fn sanitizes_escaped_entity_and_unclosed_reasoning() {
        assert_eq!(
            sanitize_visible_model_text(r"\\\<think>private</think>公开答案"),
            "公开答案"
        );
        assert_eq!(
            sanitize_visible_model_text("&lt;think&gt;private&lt;/think&gt;公开答案"),
            "公开答案"
        );
        assert_eq!(
            sanitize_visible_model_text("<  THINK  >private< / think >公开答案"),
            "公开答案"
        );
        assert_eq!(sanitize_visible_model_text("<think>private"), "");
    }

    #[test]
    fn holds_partial_entity_tag_during_streaming() {
        let mut progress = ProgressText::default();
        for delta in ["&", "lt", ";thi", "nk&gt;private"] {
            let value = json!({"payload": {"chunk": {
                "stream_event": {"type": "message_delta", "message_id": "message-1", "content": delta}
            }}});
            assert_eq!(progress.apply(&value), None);
        }
        let answer = json!({"payload": {"chunk": {
            "stream_event": {"type": "message_delta", "message_id": "message-1", "content": "&lt;/think&gt;公开答案"}
        }}});
        assert_eq!(progress.apply(&answer).as_deref(), Some("公开答案"));
    }

    #[test]
    fn extracts_terminal_and_authoritative_output() {
        let value = json!({
            "status": "completed",
            "output": "最终回答",
            "payload": {"status": "completed"}
        });
        assert_eq!(terminal_status(&value), Some("completed"));
        assert_eq!(final_output(&value), "最终回答");

        let legacy = json!({
            "status": "completed",
            "output": "<think>private chain</think>最终回答"
        });
        assert_eq!(final_output(&legacy), "最终回答");
    }

    #[test]
    fn parses_structured_server_error_and_authoritative_run_context() {
        let result = parse_run_result(&json!({
            "status": "failed",
            "error": {"type": "model_error", "message": "服务端模型调用失败"},
            "run_context": {
                "protocol_version": "1.1",
                "model_spec": "minimax-cn:MiniMax-M3",
                "knowledge_scope": {
                    "scope_version": 11,
                    "kb_count": 3,
                    "members": [{"kb_id": "kb-1", "kb_name": "水稻胚乳发育neo4j"}]
                },
                "knowledge_retrievals": [{
                    "status": "completed",
                    "returned_claim_count": 11,
                    "returned_evidence_count": 11
                }]
            }
        }));

        assert_eq!(result.error.as_deref(), Some("服务端模型调用失败"));
        assert_eq!(result.error_code.as_deref(), Some("model_error"));
        assert_eq!(result.context.protocol_version.as_deref(), Some("1.1"));
        assert_eq!(
            result.context.model_spec.as_deref(),
            Some("minimax-cn:MiniMax-M3")
        );
        assert_eq!(result.context.knowledge_scope.kb_count, 3);
        assert_eq!(
            result.context.knowledge_retrievals[0].returned_claim_count,
            Some(11)
        );
    }

    #[test]
    fn gives_actionable_errors_only_for_local_gateways() {
        assert!(matches!(
            connection_error_for_gateway("http://127.0.0.1:9088", AppError::ServiceUnavailable),
            AppError::LocalServiceUnavailable
        ));
        assert!(matches!(
            connection_error_for_gateway("https://api.example.cn", AppError::ServiceUnavailable),
            AppError::ServiceUnavailable
        ));
    }
}
