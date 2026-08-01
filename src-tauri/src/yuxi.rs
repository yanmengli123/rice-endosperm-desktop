use std::time::Duration;

use reqwest::{Client, Response, StatusCode, header};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    config::validate_gateway_url,
    error::{AppError, AppResult},
};

const CREATE_RUN_PATH: &str = "/api/agent-invocation/agent-call/runs";
const RUN_RESULT_PATH: &str = "/api/agent-invocation/agent-call/runs/result";
const CREDENTIAL_STATUS_PATH: &str = "/api/agent-invocation/credential-status";

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
}

#[derive(Debug, Clone)]
pub struct RunResult {
    pub status: String,
    pub output: String,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
struct CreateRunRequest<'a> {
    agent_slug: &'a str,
    messages: [InputMessage<'a>; 1],
    #[serde(skip_serializing_if = "Option::is_none")]
    thread_id: Option<&'a str>,
    request_id: &'a str,
    async_mode: bool,
}

#[derive(Debug, Serialize)]
struct InputMessage<'a> {
    role: &'static str,
    content: &'a str,
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
    ) -> AppResult<()> {
        let base = validate_gateway_url(gateway_url)?;
        let status_response = self
            .authorized_get(&format!("{base}{CREDENTIAL_STATUS_PATH}"), api_key)
            .timeout(Duration::from_secs(20))
            .send()
            .await?;
        if status_response.status().is_success() {
            return Ok(());
        }
        if status_response.status() != StatusCode::NOT_FOUND {
            return Err(response_error(status_response).await);
        }

        // 兼容尚未部署 credential-status 的 Yuxi：查询一个必然不存在的 run。
        // 该请求不会启动模型，也不会产生计费；401/403 仍能准确验证凭证。
        let probe = self
            .authorized_post(&format!("{base}{RUN_RESULT_PATH}"), api_key)
            .json(&json!({
                "run_id": "desktop-connection-test",
                "agent_slug": agent_slug,
            }))
            .timeout(Duration::from_secs(20))
            .send()
            .await?;
        match probe.status().as_u16() {
            200 | 404 => Ok(()),
            _ => Err(response_error(probe).await),
        }
    }

    pub async fn create_run(
        &self,
        gateway_url: &str,
        agent_slug: &str,
        api_key: &SecretString,
        question: &str,
        yuxi_thread_id: Option<&str>,
        request_id: &str,
    ) -> AppResult<CreatedRun> {
        let base = validate_gateway_url(gateway_url)?;
        let response = self
            .authorized_post(&format!("{base}{CREATE_RUN_PATH}"), api_key)
            .header("X-Client-Request-ID", request_id)
            .json(&CreateRunRequest {
                agent_slug,
                messages: [InputMessage {
                    role: "user",
                    content: question,
                }],
                thread_id: yuxi_thread_id,
                request_id,
                async_mode: true,
            })
            .timeout(Duration::from_secs(45))
            .send()
            .await?;
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
        Ok(created)
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
        agent_slug: &str,
        api_key: &SecretString,
        run_id: &str,
    ) -> AppResult<RunResult> {
        let base = validate_gateway_url(gateway_url)?;
        let response = self
            .authorized_post(&format!("{base}{RUN_RESULT_PATH}"), api_key)
            .json(&json!({ "run_id": run_id, "agent_slug": agent_slug }))
            .timeout(Duration::from_secs(30))
            .send()
            .await?;
        let response = ensure_success(response).await?;
        let value = response
            .json::<Value>()
            .await
            .map_err(|error| AppError::Protocol(error.to_string()))?;
        Ok(RunResult {
            status: value
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_owned(),
            output: final_output(&value),
            error: value
                .get("error")
                .and_then(Value::as_str)
                .map(str::to_owned),
        })
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
}

pub fn progress_text(value: &Value, current: &str) -> Option<String> {
    let payload = value.get("payload")?;
    if let Some(response) = payload
        .get("chunk")
        .and_then(|chunk| chunk.get("response"))
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
    {
        return Some(response.to_owned());
    }

    let items = payload.get("items")?.as_array()?;
    for item in items.iter().rev() {
        if let Some(response) = item
            .get("response")
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
        {
            return Some(response.to_owned());
        }
        let Some(stream_event) = item.get("stream_event") else {
            continue;
        };
        if stream_event.get("type").and_then(Value::as_str) == Some("message_delta")
            && let Some(delta) = stream_event.get("content").and_then(Value::as_str)
        {
            return Some(format!("{current}{delta}"));
        }
    }
    None
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
        .unwrap_or_default()
        .to_owned()
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

    use super::{final_output, progress_text, terminal_status};

    #[test]
    fn extracts_cumulative_and_delta_text() {
        let cumulative = json!({"payload": {"items": [{"response": "水稻胚乳"}]}});
        assert_eq!(progress_text(&cumulative, "").as_deref(), Some("水稻胚乳"));

        let delta = json!({"payload": {"items": [{"stream_event": {"type": "message_delta", "content": "形成"}}]}});
        assert_eq!(
            progress_text(&delta, "水稻胚乳").as_deref(),
            Some("水稻胚乳形成")
        );
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
    }
}
