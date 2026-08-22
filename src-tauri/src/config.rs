use url::Url;

use crate::error::{AppError, AppResult};

pub const LOCAL_GATEWAY: &str = "http://127.0.0.1:9088";
pub const DEFAULT_AGENT_SLUG: &str = "default-chatbot";

pub fn default_gateway_url() -> &'static str {
    option_env!("YUXI_BASE_URL").unwrap_or(LOCAL_GATEWAY)
}

pub fn agent_slug() -> &'static str {
    option_env!("YUXI_AGENT_SLUG").unwrap_or(DEFAULT_AGENT_SLUG)
}

pub fn validate_gateway_url(value: &str) -> AppResult<String> {
    let trimmed = value.trim().trim_end_matches('/');
    let url = Url::parse(trimmed).map_err(|_| AppError::InvalidGateway("URL 格式不正确".into()))?;
    if url.username() != ""
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(AppError::InvalidGateway(
            "不能包含账号、密码、查询参数或片段".into(),
        ));
    }
    if url.path() != "" && url.path() != "/" {
        return Err(AppError::InvalidGateway("只能填写网关根地址".into()));
    }

    // Url::host_str() 对 IPv6 返回带方括号的形式（"[::1]"），去括号后再比较。
    let host = url
        .host_str()
        .unwrap_or_default()
        .trim_start_matches('[')
        .trim_end_matches(']');
    let local_http = url.scheme() == "http" && matches!(host, "127.0.0.1" | "localhost" | "::1");
    if url.scheme() != "https" && !local_http {
        return Err(AppError::InvalidGateway(
            "远程服务必须使用 HTTPS；HTTP 仅允许本机地址".into(),
        ));
    }

    Ok(trimmed.to_owned())
}

#[cfg(test)]
mod tests {
    use super::validate_gateway_url;

    #[test]
    fn accepts_https_and_loopback_http() {
        assert_eq!(
            validate_gateway_url("https://api.example.cn/").unwrap(),
            "https://api.example.cn"
        );
        assert!(validate_gateway_url("http://127.0.0.1:9088").is_ok());
        assert!(validate_gateway_url("http://[::1]:9088").is_ok());
    }

    #[test]
    fn rejects_insecure_remote_and_embedded_credentials() {
        assert!(validate_gateway_url("http://api.example.cn").is_err());
        assert!(validate_gateway_url("https://user:pass@example.cn").is_err());
        assert!(validate_gateway_url("https://example.cn/other").is_err());
    }
}
