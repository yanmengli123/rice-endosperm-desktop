use serde::Serialize;

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug, Serialize)]
pub struct CommandError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
    pub status: Option<u16>,
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
    #[error("当前 API Key 没有调用该智能体的权限")]
    Forbidden,
    #[error("请求过于频繁，请稍后重试")]
    RateLimited,
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
    #[error("Yuxi 返回了无法识别的数据：{0}")]
    Protocol(String),
    #[error("本地安全存储失败：{0}")]
    CredentialStore(String),
    #[error("本地数据库失败：{0}")]
    Database(String),
    #[error("{0}")]
    Internal(String),
}

impl AppError {
    pub fn from_status(status: reqwest::StatusCode, detail: Option<String>) -> Self {
        match status.as_u16() {
            401 => Self::Unauthorized,
            403 => Self::Forbidden,
            429 => Self::RateLimited,
            500..=599 => Self::ServiceUnavailable,
            _ => Self::Protocol(detail.unwrap_or_else(|| format!("HTTP {}", status.as_u16()))),
        }
    }

    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::MissingCredential => "missing_credential",
            Self::InvalidCredential => "invalid_credential",
            Self::InvalidGateway(_) => "invalid_gateway",
            Self::Unauthorized => "unauthorized",
            Self::Forbidden => "forbidden",
            Self::RateLimited => "rate_limited",
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
        }
    }

    fn retryable(&self) -> bool {
        matches!(
            self,
            Self::RateLimited
                | Self::ServiceUnavailable
                | Self::LocalServiceUnavailable
                | Self::Network(_)
        )
    }
}

impl From<AppError> for CommandError {
    fn from(value: AppError) -> Self {
        let status = match value {
            AppError::Unauthorized => Some(401),
            AppError::Forbidden => Some(403),
            AppError::RateLimited => Some(429),
            AppError::ServiceUnavailable => Some(503),
            AppError::LocalServiceUnavailable => Some(503),
            _ => None,
        };
        Self {
            code: value.code().to_owned(),
            message: value.to_string(),
            retryable: value.retryable(),
            status,
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
