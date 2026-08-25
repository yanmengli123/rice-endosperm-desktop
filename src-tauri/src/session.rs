//! P2b 设备会话本地持久化：与 Yuxi 服务端旋转刷新令牌协议配套。

use serde::{Deserialize, Serialize};

/// 按账号作用域存放在 Stronghold 中的会话凭据。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredSession {
    pub access_token: String,
    pub refresh_token: String,
    pub family_id: String,
    /// 访问令牌过期时间（epoch 秒，UTC）。
    pub access_expires_at: i64,
}

/// 从 JWT 第三段前的 payload 解析 exp 声明（不做签名校验——校验由服务端负责，
/// 本地仅用于判断是否需要提前刷新）。解析失败返回 None，调用方按"需要刷新"处理。
pub fn parse_jwt_exp(token: &str) -> Option<i64> {
    use base64::Engine as _;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;

    let payload_segment = token.split('.').nth(1)?;
    let payload = URL_SAFE_NO_PAD.decode(payload_segment).ok()?;
    serde_json::from_slice::<serde_json::Value>(&payload)
        .ok()?
        .get("exp")?
        .as_i64()
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine as _;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;

    #[test]
    fn parses_exp_from_real_shape_jwt() {
        let payload = URL_SAFE_NO_PAD.encode(br#"{"exp":1700000000,"sub":"7"}"#);
        let token = format!("header.{payload}.signature");
        assert_eq!(parse_jwt_exp(&token), Some(1_700_000_000));
    }

    #[test]
    fn malformed_tokens_return_none() {
        assert_eq!(parse_jwt_exp("not-a-jwt"), None);
        assert_eq!(parse_jwt_exp("a.b"), None);
        assert_eq!(parse_jwt_exp("h.c2Fi.bw"), None); // payload 非 JSON
    }

    #[test]
    fn stored_session_roundtrips_through_camel_case_json() {
        let original = StoredSession {
            access_token: "eyJ".into(),
            refresh_token: "yxrt_abc".into(),
            family_id: "fam".into(),
            access_expires_at: 123,
        };
        let json = serde_json::to_string(&original).unwrap();
        let parsed: StoredSession = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.refresh_token, "yxrt_abc");
    }
}
