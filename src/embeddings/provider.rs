//! The embedding provider strategy (M40): turns texts into vectors. The HTTP
//! implementation targets an OpenAI/Voyage-compatible `/embeddings` endpoint
//! (`{ "model", "input": [..] } → { "data": [{ "embedding": [..] }] }`), so it
//! works with any such service; it is mocked in unit tests.

use crate::httpclient::{HttpClient, HttpRequest};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Arc;

/// Errors raised by embedding providers.
#[derive(Debug, thiserror::Error)]
pub enum EmbeddingError {
    #[error("embedding request failed: {0}")]
    Request(String),
    #[error("could not parse embedding response: {0}")]
    Parse(String),
}

/// Produces embedding vectors for a batch of texts.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbeddingError>;
    /// The model id, recorded with each stored embedding.
    fn model(&self) -> String;
}

/// Configuration for the HTTP embedding provider.
#[derive(Debug, Clone)]
pub struct EmbeddingConfig {
    pub endpoint: String,
    pub model: String,
    pub api_key: Option<String>,
}

/// An [`EmbeddingProvider`] over the shared [`HttpClient`].
pub struct HttpEmbeddingProvider {
    http: Arc<dyn HttpClient>,
    config: EmbeddingConfig,
}

impl HttpEmbeddingProvider {
    pub fn new(http: Arc<dyn HttpClient>, config: EmbeddingConfig) -> Self {
        Self { http, config }
    }

    fn parse(body: &str) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        let v: Value = serde_json::from_str(body).map_err(|e| EmbeddingError::Parse(e.to_string()))?;
        let data = v.get("data").and_then(Value::as_array).ok_or_else(|| {
            EmbeddingError::Parse("response has no 'data' array".into())
        })?;
        data.iter()
            .map(|row| {
                let arr = row
                    .get("embedding")
                    .and_then(Value::as_array)
                    .ok_or_else(|| EmbeddingError::Parse("row has no 'embedding' array".into()))?;
                Ok(arr.iter().map(|n| n.as_f64().unwrap_or(0.0) as f32).collect())
            })
            .collect()
    }
}

#[async_trait]
impl EmbeddingProvider for HttpEmbeddingProvider {
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        if texts.is_empty() {
            return Ok(vec![]);
        }
        let body = json!({ "model": self.config.model, "input": texts }).to_string();
        let mut req = HttpRequest::post(&self.config.endpoint, body)
            .header("content-type", "application/json");
        if let Some(key) = &self.config.api_key {
            req = req.header("authorization", format!("Bearer {key}"));
        }
        let resp = self.http.send(req).await.map_err(|e| EmbeddingError::Request(e.to_string()))?;
        if !resp.is_success() {
            return Err(EmbeddingError::Request(format!("status {}", resp.status)));
        }
        Self::parse(&resp.body)
    }

    fn model(&self) -> String {
        self.config.model.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::httpclient::{HttpResponse, MockHttpClient};

    #[tokio::test]
    async fn posts_and_parses_embeddings() {
        let mut http = MockHttpClient::new();
        http.expect_send()
            .withf(|r| r.method == "POST" && r.body.as_deref().unwrap().contains("\"input\""))
            .returning(|_| {
                Ok(HttpResponse::new(200, r#"{"data":[{"embedding":[0.1,0.2]},{"embedding":[0.3,0.4]}]}"#))
            });
        let provider = HttpEmbeddingProvider::new(
            Arc::new(http),
            EmbeddingConfig { endpoint: "http://x/embeddings".into(), model: "m".into(), api_key: Some("k".into()) },
        );
        let out = provider.embed(&["a".into(), "b".into()]).await.unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0], vec![0.1, 0.2]);
        assert_eq!(provider.model(), "m");
    }

    #[tokio::test]
    async fn empty_input_skips_request() {
        let provider = HttpEmbeddingProvider::new(
            Arc::new(MockHttpClient::new()),
            EmbeddingConfig { endpoint: "http://x".into(), model: "m".into(), api_key: None },
        );
        assert!(provider.embed(&[]).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn non_success_is_error() {
        let mut http = MockHttpClient::new();
        http.expect_send().returning(|_| Ok(HttpResponse::new(500, "boom")));
        let provider = HttpEmbeddingProvider::new(
            Arc::new(http),
            EmbeddingConfig { endpoint: "http://x".into(), model: "m".into(), api_key: None },
        );
        assert!(provider.embed(&["a".into()]).await.is_err());
    }
}
