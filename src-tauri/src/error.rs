use serde::Serialize;

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
    pub status: Option<u16>,
    /// 服务端结构化错误携带的引导动作（如 contact_admin / configure_byok）。
    pub action: Option<String>,
    /// 网关追踪号（X-Gateway-Trace-ID），用户报障时对齐网关与上游日志。
    pub trace_id: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("尚未配置 Yuxi API Key")]
    MissingCredential,
    #[error("API Key 格式无效")]
    InvalidCredential,
    #[error("服务地址无效：{0}")]
    InvalidGateway(String),
    #[error("认证失败，请检查 API Key 是否有效或已被禁用")]
    Unauthorized,
    /// 403：优先透传服务端的具体原因（如"当前账号未启用自有模型"），
    /// 避免被误导性的通用文案掩盖真实拒绝理由。
    #[error("{0}")]
    Forbidden(String),
    #[error("{0}")]
    RateLimited(String),
    #[error("服务暂时不可用，请稍后重试")]
    ServiceUnavailable,
    #[error(
        "本机 Yuxi 服务未就绪。请确认 Docker Desktop 已启动，并检查 Redis、worker、API 与 APISIX 服务后重试。"
    )]
    LocalServiceUnavailable,
    #[error(
        "Yuxi 服务端版本过旧，无法正确处理思考模型的多轮工具调用。请将 rice-endosperm-agent 更新到最新版并重启服务后重试；无需更换 API Key。"
    )]
    ServerUpgradeRequired,
    #[error("请求已取消")]
    Cancelled,
    #[error("登录会话已失效，请在连接设置中重新登录")]
    SessionRequiresRelogin,
    #[error("找不到本地会话")]
    ThreadNotFound,
    #[error("网络请求失败：{0}")]
    Network(String),
    #[error("服务端返回错误：{0}")]
    Protocol(String),
    #[error("本地安全存储失败：{0}")]
    CredentialStore(String),
    #[error("本地数据库失败：{0}")]
    Database(String),
    #[error("{0}")]
    Internal(String),
    /// 服务端/网关结构化错误：保留 HTTP 状态、服务端错误码（如
    /// daily_run_quota_exceeded / run_busy / authorization_pending）、引导动作
    /// 与网关追踪号，贯通到 CommandError 供前端分支展示。
    #[error("{message}")]
    Server {
        status: u16,
        message: String,
        server_code: Option<String>,
        action: Option<String>,
        trace_id: Option<String>,
    },
}

/// 配额类 429：重试无意义，必须引导用户联系管理员或配置自有模型。
const NON_RETRYABLE_QUOTA_CODES: [&str; 2] =
    ["daily_run_quota_exceeded", "platform_token_quota_exceeded"];

/// 会话族终态失败码：刷新令牌已死，唯一出路是重新登录。
const SESSION_TERMINAL_CODES: [&str; 3] = ["invalid_grant", "session_revoked", "reuse_detected"];

fn default_forbidden_message() -> String {
    "当前请求被服务端拒绝，请检查账号状态或模型接入策略".into()
}

fn default_rate_limited_message() -> String {
    "请求过于频繁，请稍后重试".into()
}

impl AppError {
    /// 由解析后的服务端错误部件构造：
    /// - 401 会话族终态失败（invalid_grant/session_revoked/reuse_detected）→
    ///   `SessionRequiresRelogin`（fail-closed，前端回落连接设置）；
    /// - 裸 403/423/429（无结构化 code，网关限流/登录锁定/默认拒绝体）→ 语义变体；
    /// - 其余保留完整 `Server` 结构化上下文（code/action/trace_id 贯通到 UI）。
    pub fn from_server_parts(
        status: u16,
        message: String,
        server_code: Option<String>,
        action: Option<String>,
        trace_id: Option<String>,
    ) -> Self {
        if status == 401
            && server_code
                .as_deref()
                .is_some_and(|code| SESSION_TERMINAL_CODES.contains(&code))
        {
            return Self::SessionRequiresRelogin;
        }
        if status == 401 && server_code.is_none() {
            return Self::Unauthorized;
        }
        if (500..=599).contains(&status) && server_code.is_none() {
            return Self::ServiceUnavailable;
        }
        if server_code.is_none() {
            match status {
                403 | 422 => {
                    return Self::Forbidden(
                        message
                            .trim()
                            .is_empty()
                            .then(default_forbidden_message)
                            .unwrap_or(message),
                    );
                }
                423 | 429 => {
                    return Self::RateLimited(
                        message
                            .trim()
                            .is_empty()
                            .then(default_rate_limited_message)
                            .unwrap_or(message),
                    );
                }
                _ => {}
            }
        }
        Self::Server {
            status,
            message,
            server_code,
            action,
            trace_id,
        }
    }

    /// 该错误是否为会话终态失败（刷新令牌已死，应清除本地会话 blob）。
    /// 网络类失败（5xx/超时）不是终态——令牌可能仍然有效，不得清除。
    pub fn is_session_terminal(&self) -> bool {
        matches!(self, Self::SessionRequiresRelogin)
            || matches!(self, Self::Server { status: 401, server_code, .. } if server_code
                .as_deref()
                .is_some_and(|code| SESSION_TERMINAL_CODES.contains(&code)))
    }

    pub(crate) fn code(&self) -> String {
        match self {
            Self::MissingCredential => "missing_credential",
            Self::InvalidCredential => "invalid_credential",
            Self::InvalidGateway(_) => "invalid_gateway",
            Self::Unauthorized => "unauthorized",
            Self::Forbidden(_) => "forbidden",
            Self::RateLimited(_) => "rate_limited",
            Self::ServiceUnavailable => "service_unavailable",
            Self::LocalServiceUnavailable => "local_service_unavailable",
            Self::ServerUpgradeRequired => "server_upgrade_required",
            Self::Cancelled => "cancelled",
            Self::SessionRequiresRelogin => "session_requires_relogin",
            Self::ThreadNotFound => "thread_not_found",
            Self::Network(_) => "network_error",
            Self::Protocol(_) => "protocol_error",
            Self::CredentialStore(_) => "credential_store_error",
            Self::Database(_) => "database_error",
            Self::Internal(_) => "internal_error",
            Self::Server {
                status,
                server_code,
                ..
            } => {
                if let Some(code) = server_code {
                    return code.clone();
                }
                match status {
                    403 | 422 => "forbidden",
                    404 => "not_found",
                    409 => "conflict",
                    429 => "rate_limited",
                    _ => "protocol_error",
                }
            }
        }
        .to_owned()
    }

    fn retryable(&self) -> bool {
        match self {
            Self::Server {
                status,
                server_code,
                ..
            } => match server_code.as_deref() {
                Some(code) if NON_RETRYABLE_QUOTA_CODES.contains(&code) => false,
                Some("platform_token_quota_reservation_busy") => true,
                _ => *status == 429,
            },
            Self::RateLimited(_)
            | Self::ServiceUnavailable
            | Self::LocalServiceUnavailable
            | Self::Network(_) => true,
            _ => false,
        }
    }
}

impl From<AppError> for CommandError {
    fn from(value: AppError) -> Self {
        let status = match &value {
            AppError::Unauthorized => Some(401),
            AppError::Forbidden(_) => Some(403),
            AppError::RateLimited(_) => Some(429),
            AppError::ServiceUnavailable | AppError::LocalServiceUnavailable => Some(503),
            AppError::Server { status, .. } => Some(*status),
            _ => None,
        };
        let (action, trace_id) = match &value {
            AppError::Server {
                action, trace_id, ..
            } => (action.clone(), trace_id.clone()),
            _ => (None, None),
        };
        Self {
            code: value.code(),
            message: value.to_string(),
            retryable: value.retryable(),
            status,
            action,
            trace_id,
        }
    }
}

impl From<sqlx::Error> for AppError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(value.to_string())
    }
}

impl From<reqwest::Error> for AppError {
    fn from(value: reqwest::Error) -> Self {
        Self::Network(value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::AppError;

    #[test]
    fn forbidden_preserves_server_policy_reason() {
        let error = AppError::from_server_parts(
            403,
            "当前账号未启用自有模型，请联系管理员将模型策略设为 BYOK 可选".into(),
            None,
            None,
            None,
        );
        assert!(matches!(error, AppError::Forbidden(_)));
        assert_eq!(
            error.to_string(),
            "当前账号未启用自有模型，请联系管理员将模型策略设为 BYOK 可选"
        );
        assert_eq!(error.code(), "forbidden");
    }

    #[test]
    fn rate_limit_preserves_platform_quota_guidance() {
        let error = AppError::from_server_parts(
            429,
            "平台额度已用完，请配置自己的模型".into(),
            None,
            None,
            None,
        );
        assert!(matches!(error, AppError::RateLimited(_)));
        assert_eq!(error.to_string(), "平台额度已用完，请配置自己的模型");
        assert_eq!(error.code(), "rate_limited");
    }

    #[test]
    fn bare_status_without_message_uses_friendly_defaults() {
        let forbidden = AppError::from_server_parts(403, String::new(), None, None, None);
        assert!(forbidden.to_string().contains("服务端拒绝"));
        let limited = AppError::from_server_parts(429, String::new(), None, None, None);
        assert!(limited.to_string().contains("频繁"));
        let locked = AppError::from_server_parts(423, "剩余 300 秒".into(), None, None, None);
        assert_eq!(locked.to_string(), "剩余 300 秒");
    }

    #[test]
    fn structured_server_error_carries_code_action_and_trace() {
        let error = AppError::from_server_parts(
            429,
            "今日问答次数已达上限，请联系管理员".into(),
            Some("daily_run_quota_exceeded".into()),
            Some("contact_admin".into()),
            Some("8f14e45f-ea0b-4a1d".into()),
        );
        let command = super::CommandError::from(error);
        assert_eq!(command.code, "daily_run_quota_exceeded");
        assert_eq!(command.action.as_deref(), Some("contact_admin"));
        assert_eq!(command.trace_id.as_deref(), Some("8f14e45f-ea0b-4a1d"));
        assert!(!command.retryable, "配额类 429 重试无意义");
        assert_eq!(command.status, Some(429));
    }

    #[test]
    fn reservation_busy_quota_stays_retryable() {
        let error = AppError::from_server_parts(
            429,
            "已有任务正在使用平台额度".into(),
            Some("platform_token_quota_reservation_busy".into()),
            None,
            None,
        );
        assert!(super::CommandError::from(error).retryable);
    }

    #[test]
    fn gateway_rate_limit_without_code_is_retryable() {
        let error = AppError::from_server_parts(429, "请求过于频繁".into(), None, None, None);
        assert!(super::CommandError::from(error).retryable);
    }

    #[test]
    fn session_terminal_codes_collapse_to_relogin_and_flag_terminal() {
        for code in ["invalid_grant", "session_revoked", "reuse_detected"] {
            let error = AppError::from_server_parts(
                401,
                format!("会话错误 {code}"),
                Some(code.into()),
                None,
                None,
            );
            assert!(matches!(error, AppError::SessionRequiresRelogin), "{code}");
            assert!(error.is_session_terminal());
        }
        // 网络类 5xx 不是会话终态：令牌可能仍有效，不得触发清 blob
        let transient = AppError::from_server_parts(503, "upstream down".into(), None, None, None);
        assert!(matches!(transient, AppError::ServiceUnavailable));
        assert!(!transient.is_session_terminal());
        // 非 401 的结构化错误保留 Server 形态（如 run_busy）
        let busy = AppError::from_server_parts(
            409,
            "该会话已有任务在运行".into(),
            Some("run_busy".into()),
            None,
            None,
        );
        assert_eq!(busy.code(), "run_busy");
        assert!(!busy.is_session_terminal());
    }

    #[test]
    fn plain_unauthorized_without_code_keeps_legacy_mapping() {
        let error = AppError::from_server_parts(401, "Not authenticated".into(), None, None, None);
        assert!(matches!(error, AppError::Unauthorized));
    }
}
