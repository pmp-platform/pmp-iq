//! Anthropic Messages API provider.

use super::model::{AiRequest, AiResponse};
use super::provider::{AiError, AiProvider};
use crate::httpclient::{HttpClient, HttpRequest, HttpResponse};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

const DEFAULT_BASE: &str = "https://api.anthropic.com";
const DEFAULT_MODEL: &str = "claude-opus-4-8";
const API_VERSION: &str = "2023-06-01";

/// Typed configuration for the Anthropic provider.
#[derive(Debug, Clone, Deserialize)]
pub struct AnthropicConfig {
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    #[serde(default)]
    pub effort: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
}

fn default_model() -> String {
    DEFAULT_MODEL.to_string()
}

fn default_max_tokens() -> u32 {
    4096
}

/// Calls the Anthropic Messages API over the injected HTTP client.
pub struct AnthropicProvider {
    http: Arc<dyn HttpClient>,
    api_key: String,
    config: AnthropicConfig,
}

impl AnthropicProvider {
    pub fn new(http: Arc<dyn HttpClient>, api_key: String, config: AnthropicConfig) -> Self {
        Self { http, api_key, config }
    }

    fn base(&self) -> &str {
        self.config.base_url.as_deref().unwrap_or(DEFAULT_BASE)
    }

    fn build_body(&self, request: &AiRequest) -> Value {
        let mut body = json!({
            "model": self.config.model,
            "max_tokens": self.config.max_tokens,
            "messages": [{ "role": "user", "content": request.prompt }],
        });
        if let Some(system) = &request.system {
            body["system"] = json!(system);
        }
        if let Some(effort) = &self.config.effort {
            body["output_config"] = json!({ "effort": effort });
        }
        body
    }

    fn request(&self, body: Value) -> HttpRequest {
        HttpRequest::post(format!("{}/v1/messages", self.base()), body.to_string())
            .header("content-type", "application/json")
            .header("x-api-key", self.api_key.clone())
            .header("anthropic-version", API_VERSION)
    }

    fn check_status(resp: &HttpResponse) -> Result<(), AiError> {
        match resp.status {
            s if (200..300).contains(&s) => Ok(()),
            401 | 403 => Err(AiError::Auth),
            s => Err(AiError::Request(format!("status {s}: {}", resp.body))),
        }
    }

    fn parse_response(body: &str) -> Result<AiResponse, AiError> {
        let value: Value = serde_json::from_str(body).map_err(|e| AiError::Parse(e.to_string()))?;
        let text = value["content"]
            .as_array()
            .and_then(|blocks| blocks.iter().find(|b| b["type"] == "text"))
            .and_then(|b| b["text"].as_str())
            .unwrap_or("")
            .to_string();
        Ok(AiResponse {
            text,
            input_tokens: value["usage"]["input_tokens"].as_u64().map(|v| v as u32),
            output_tokens: value["usage"]["output_tokens"].as_u64().map(|v| v as u32),
        })
    }
}

#[async_trait]
impl AiProvider for AnthropicProvider {
    async fn complete(&self, request: AiRequest) -> Result<AiResponse, AiError> {
        let body = self.build_body(&request);
        let resp = self
            .http
            .send(self.request(body))
            .await
            .map_err(|e| AiError::Request(e.to_string()))?;
        Self::check_status(&resp)?;
        Self::parse_response(&resp.body)
    }

    async fn validate(&self) -> Result<(), AiError> {
        let request = AiRequest::new("ping");
        let mut body = self.build_body(&request);
        body["max_tokens"] = json!(16);
        let resp = self
            .http
            .send(self.request(body))
            .await
            .map_err(|e| AiError::Request(e.to_string()))?;
        Self::check_status(&resp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::httpclient::MockHttpClient;

    fn config() -> AnthropicConfig {
        AnthropicConfig {
            model: DEFAULT_MODEL.into(),
            max_tokens: 1024,
            effort: Some("high".into()),
            base_url: None,
        }
    }

    #[tokio::test]
    async fn parses_text_and_usage() {
        let mut http = MockHttpClient::new();
        http.expect_send().returning(|req| {
            // Auth header present.
            assert_eq!(req.headers.get("x-api-key").map(String::as_str), Some("key"));
            Ok(HttpResponse::new(
                200,
                r#"{"content":[{"type":"text","text":"hi"}],"usage":{"input_tokens":3,"output_tokens":1}}"#,
            ))
        });
        let provider = AnthropicProvider::new(Arc::new(http), "key".into(), config());
        let out = provider.complete(AiRequest::new("hello")).await.unwrap();
        assert_eq!(out.text, "hi");
        assert_eq!(out.input_tokens, Some(3));
        assert_eq!(out.output_tokens, Some(1));
    }

    #[tokio::test]
    async fn unauthorized_maps_to_auth() {
        let mut http = MockHttpClient::new();
        http.expect_send()
            .returning(|_| Ok(HttpResponse::new(401, "{}")));
        let provider = AnthropicProvider::new(Arc::new(http), "bad".into(), config());
        assert!(matches!(provider.validate().await, Err(AiError::Auth)));
    }

    #[test]
    fn body_includes_system_and_effort() {
        let provider = AnthropicProvider::new(Arc::new(MockHttpClient::new()), "k".into(), config());
        let body = provider.build_body(&AiRequest::new("p").with_system("sys"));
        assert_eq!(body["system"], "sys");
        assert_eq!(body["output_config"]["effort"], "high");
    }
}
