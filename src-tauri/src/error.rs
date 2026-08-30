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
}

impl AppError {
    pub fn from_status(status: reqwest::StatusCode, detail: Option<String>) -> Self {
        match status.as_u16() {
            401 => Self::Unauthorized,
            403 => Self::Forbidden(
                detail
                    .filter(|text| !text.trim().is_empty())
                    .unwrap_or_else(|| "当前请求被服务端拒绝，请检查账号状态或模型接入策略".into()),
            ),
            429 => Self::RateLimited(
                detail
                    .filter(|text| !text.trim().is_empty())
                    .unwrap_or_else(|| "请求过于频繁，请稍后重试".into()),
            ),
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
        }
    }

    fn retryable(&self) -> bool {
        matches!(
            self,
            Self::RateLimited(_)
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
            AppError::Forbidden(_) => Some(403),
            AppError::RateLimited(_) => Some(429),
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

#[cfg(test)]
mod tests {
    use reqwest::StatusCode;

    use super::AppError;

    #[test]
    fn forbidden_preserves_server_policy_reason() {
        let error = AppError::from_status(
            StatusCode::FORBIDDEN,
            Some("当前账号未启用自有模型，请联系管理员将模型策略设为 BYOK 可选".into()),
        );
        assert_eq!(
            error.to_string(),
            "当前账号未启用自有模型，请联系管理员将模型策略设为 BYOK 可选"
        );
        assert_eq!(error.code(), "forbidden");
    }

    #[test]
    fn rate_limit_preserves_platform_quota_guidance() {
        let error = AppError::from_status(
            StatusCode::TOO_MANY_REQUESTS,
            Some("平台额度已用完，请配置自己的模型".into()),
        );
        assert_eq!(error.to_string(), "平台额度已用完，请配置自己的模型");
        assert_eq!(error.code(), "rate_limited");
    }
}
