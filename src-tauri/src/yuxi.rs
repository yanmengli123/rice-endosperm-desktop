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
    query: &'a str,
    agent_slug: &'a str,
    thread_id: &'a str,
    meta: RunRequestMeta<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    model_spec: Option<&'a str>,
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
    file_type: Option<&'a str>,
    bucket_name: &'a str,
    object_name: &'a str,
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
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(format!("RiceEndospermDesktop/{app_version}"))
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

    #[allow(clippy::too_many_arguments)] // 网关契约字段逐一显式传参，避免引入参数对象
    pub async fn create_run(
        &self,
        gateway_url: &str,
        agent_slug: &str,
        api_key: &SecretString,
        question: &str,
        yuxi_thread_id: &str,
        request_id: &str,
        model_spec: Option<&str>,
        attachment_file_ids: &[String],
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

async fn response_error(response: Response) -> AppError {
    let status = response.status();
    let detail = response.json::<Value>().await.ok().and_then(|value| {
        value
            .get("detail")
            .and_then(Value::as_str)
            .or_else(|| value.pointer("/detail/message").and_then(Value::as_str))
            .or_else(|| value.get("message").and_then(Value::as_str))
            .map(str::to_owned)
    });
    AppError::from_status(status, detail)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::error::AppError;

    use super::{
        CreateRunRequest, ProgressText, RunRequestMeta, connection_error_for_gateway,
        final_output, parse_default_agent_slug, parse_desktop_login_response, parse_run_result,
        sanitize_visible_model_text, terminal_status, validate_authoritative_run_context,
    };

    #[test]
    fn serializes_the_same_native_agent_run_contract_as_web() {
        let value = serde_json::to_value(CreateRunRequest {
            query: "水稻胚乳发育的关键调控基因有哪些？",
            agent_slug: "default-chatbot",
            thread_id: "thread-1",
            meta: RunRequestMeta {
                request_id: "desktop-request-123456",
                client: "rice-endosperm-desktop",
                attachment_file_ids: vec![],
            },
            model_spec: None,
        })
        .expect("serialize native AgentRun request");

        assert_eq!(value["agent_slug"], "default-chatbot");
        assert_eq!(value["thread_id"], "thread-1");
        assert_eq!(value["meta"]["request_id"], "desktop-request-123456");
        assert_eq!(value["meta"]["client"], "rice-endosperm-desktop");
        assert!(value.get("messages").is_none());
        assert!(value.get("async_mode").is_none());
        assert!(value.get("model_spec").is_none());
    }

    #[test]
    fn serializes_confirmed_attachment_ids_inside_run_meta() {
        let value = serde_json::to_value(CreateRunRequest {
            query: "总结附件",
            agent_slug: "default-chatbot",
            thread_id: "thread-1",
            meta: RunRequestMeta {
                request_id: "desktop-request-with-file",
                client: "rice-endosperm-desktop",
                attachment_file_ids: vec!["file-1", "file-2"],
            },
            model_spec: None,
        })
        .expect("serialize attachment run request");

        assert_eq!(
            value["meta"]["attachment_file_ids"],
            json!(["file-1", "file-2"])
        );
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
