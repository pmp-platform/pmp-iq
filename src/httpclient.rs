//! A minimal HTTP client abstraction so provider integrations can be unit
//! tested with mocked responses.

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

/// A simplified HTTP request.
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    pub headers: HashMap<String, String>,
    pub body: Option<String>,
}

impl HttpRequest {
    pub fn get(url: impl Into<String>) -> Self {
        Self {
            method: "GET".into(),
            url: url.into(),
            headers: HashMap::new(),
            body: None,
        }
    }

    pub fn post(url: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            method: "POST".into(),
            url: url.into(),
            headers: HashMap::new(),
            body: Some(body.into()),
        }
    }

    pub fn header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(key.into(), value.into());
        self
    }
}

/// A simplified HTTP response. Header keys are stored lowercased.
#[derive(Clone, Debug, Default)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
    pub headers: HashMap<String, String>,
}

impl HttpResponse {
    pub fn new(status: u16, body: impl Into<String>) -> Self {
        Self {
            status,
            body: body.into(),
            headers: HashMap::new(),
        }
    }

    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// Look up a response header (case-insensitive).
    pub fn header(&self, key: &str) -> Option<&str> {
        self.headers.get(&key.to_ascii_lowercase()).map(String::as_str)
    }
}

/// Errors from performing an HTTP request.
#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    #[error("transport error: {0}")]
    Transport(String),
}

/// Performs HTTP requests.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait HttpClient: Send + Sync {
    async fn send(&self, request: HttpRequest) -> Result<HttpResponse, HttpError>;
}

/// `reqwest`-backed implementation.
pub struct ReqwestClient {
    inner: reqwest::Client,
}

impl ReqwestClient {
    pub fn new() -> Self {
        Self {
            inner: reqwest::Client::new(),
        }
    }
}

impl Default for ReqwestClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl HttpClient for ReqwestClient {
    async fn send(&self, request: HttpRequest) -> Result<HttpResponse, HttpError> {
        let method = reqwest::Method::from_bytes(request.method.as_bytes())
            .map_err(|e| HttpError::Transport(e.to_string()))?;
        let mut builder = self.inner.request(method, &request.url);
        for (key, value) in request.headers {
            builder = builder.header(key, value);
        }
        if let Some(body) = request.body {
            builder = builder.body(body);
        }
        let resp = builder
            .send()
            .await
            .map_err(|e| HttpError::Transport(e.to_string()))?;
        let status = resp.status().as_u16();
        let mut headers = HashMap::new();
        for (name, value) in resp.headers() {
            if let Ok(v) = value.to_str() {
                headers.insert(name.as_str().to_ascii_lowercase(), v.to_string());
            }
        }
        let body = resp
            .text()
            .await
            .map_err(|e| HttpError::Transport(e.to_string()))?;
        Ok(HttpResponse { status, body, headers })
    }
}

/// Wraps an [`HttpClient`] to enforce a minimum interval between requests,
/// throttling outbound calls (e.g. to GitHub/GitLab) to stay under rate limits.
pub struct ThrottledHttpClient {
    inner: Arc<dyn HttpClient>,
    min_interval: Duration,
    last: Mutex<Option<Instant>>,
}

impl ThrottledHttpClient {
    pub fn new(inner: Arc<dyn HttpClient>, min_interval: Duration) -> Self {
        Self {
            inner,
            min_interval,
            last: Mutex::new(None),
        }
    }

    /// Sleep just long enough to honour the minimum interval since the last send.
    async fn throttle(&self) {
        let wait = {
            let mut last = self.last.lock().await;
            let now = Instant::now();
            let wait = match *last {
                Some(prev) => self.min_interval.checked_sub(now.duration_since(prev)),
                None => None,
            };
            // Reserve this slot immediately so concurrent callers serialise.
            *last = Some(now + wait.unwrap_or_default());
            wait
        };
        if let Some(wait) = wait {
            tokio::time::sleep(wait).await;
        }
    }
}

#[async_trait]
impl HttpClient for ThrottledHttpClient {
    async fn send(&self, request: HttpRequest) -> Result<HttpResponse, HttpError> {
        self.throttle().await;
        self.inner.send(request).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_get_and_post_requests() {
        let g = HttpRequest::get("http://x").header("A", "b");
        assert_eq!(g.method, "GET");
        assert_eq!(g.headers.get("A").unwrap(), "b");

        let p = HttpRequest::post("http://x", "body");
        assert_eq!(p.method, "POST");
        assert_eq!(p.body.as_deref(), Some("body"));
    }

    #[test]
    fn success_range_detected() {
        assert!(HttpResponse::new(200, "").is_success());
        assert!(!HttpResponse::new(404, "").is_success());
    }

    #[test]
    fn header_lookup_is_case_insensitive() {
        let mut resp = HttpResponse::new(200, "");
        resp.headers.insert("retry-after".into(), "30".into());
        assert_eq!(resp.header("Retry-After"), Some("30"));
        assert_eq!(resp.header("missing"), None);
    }

    #[tokio::test]
    async fn throttle_enforces_min_interval() {
        let mut inner = MockHttpClient::new();
        inner.expect_send().times(2).returning(|_| Ok(HttpResponse::new(200, "ok")));
        let client = ThrottledHttpClient::new(std::sync::Arc::new(inner), Duration::from_millis(80));
        let start = Instant::now();
        client.send(HttpRequest::get("http://x")).await.unwrap();
        client.send(HttpRequest::get("http://x")).await.unwrap();
        assert!(start.elapsed() >= Duration::from_millis(70));
    }
}
