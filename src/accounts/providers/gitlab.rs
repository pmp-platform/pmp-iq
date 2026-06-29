//! GitLab repository provider.

use super::{ProviderError, RepositoryProvider, retry_at_from_headers};
use crate::accounts::model::RemoteRepo;
use crate::httpclient::{HttpClient, HttpRequest, HttpResponse};
use async_trait::async_trait;
use serde::Deserialize;
use std::sync::Arc;

const DEFAULT_API: &str = "https://gitlab.com";
const MAX_PAGES: u32 = 10;

#[derive(Deserialize)]
struct GlProject {
    path: String,
    path_with_namespace: String,
    http_url_to_repo: String,
    default_branch: Option<String>,
    visibility: Option<String>,
}

/// Lists projects visible to a GitLab token.
pub struct GitLabProvider {
    http: Arc<dyn HttpClient>,
    token: Option<String>,
    base_url: String,
}

impl GitLabProvider {
    pub fn new(http: Arc<dyn HttpClient>, token: Option<String>, base_url: Option<String>) -> Self {
        Self {
            http,
            token,
            base_url: crate::strings::blank_to_none(base_url)
                .unwrap_or_else(|| DEFAULT_API.to_string()),
        }
    }

    fn request(&self, url: &str) -> HttpRequest {
        let mut req = HttpRequest::get(url).header("User-Agent", "platform-inspector");
        if let Some(token) = &self.token {
            req = req.header("PRIVATE-TOKEN", token.clone());
        }
        req
    }

    fn check_status(resp: &HttpResponse) -> Result<(), ProviderError> {
        match resp.status {
            s if (200..300).contains(&s) => Ok(()),
            429 => Err(ProviderError::RateLimited {
                retry_at: retry_at_from_headers(resp),
            }),
            401 | 403 => Err(ProviderError::Auth),
            s => Err(ProviderError::Request(format!("status {s}"))),
        }
    }
}

#[async_trait]
impl RepositoryProvider for GitLabProvider {
    async fn validate(&self) -> Result<(), ProviderError> {
        let url = format!("{}/api/v4/user", self.base_url);
        let resp = self
            .http
            .send(self.request(&url))
            .await
            .map_err(|e| ProviderError::Request(e.to_string()))?;
        Self::check_status(&resp)
    }

    async fn list_repositories(&self) -> Result<Vec<RemoteRepo>, ProviderError> {
        let mut out = Vec::new();
        for page in 1..=MAX_PAGES {
            let url = format!(
                "{}/api/v4/projects?membership=true&per_page=100&page={page}",
                self.base_url
            );
            let resp = self
                .http
                .send(self.request(&url))
                .await
                .map_err(|e| ProviderError::Request(e.to_string()))?;
            Self::check_status(&resp)?;
            let projects: Vec<GlProject> = serde_json::from_str(&resp.body)
                .map_err(|e| ProviderError::Parse(e.to_string()))?;
            if projects.is_empty() {
                break;
            }
            out.extend(projects.into_iter().map(|p| RemoteRepo {
                name: p.path,
                full_name: p.path_with_namespace,
                clone_url: p.http_url_to_repo,
                default_branch: p.default_branch,
                private: p.visibility.as_deref() != Some("public"),
            }));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::httpclient::MockHttpClient;

    #[tokio::test]
    async fn lists_and_maps_projects() {
        let mut http = MockHttpClient::new();
        let body = r#"[{"path":"api","path_with_namespace":"grp/api","http_url_to_repo":"https://gl/api.git","default_branch":"main","visibility":"private"}]"#;
        let mut call = 0;
        http.expect_send().returning(move |_| {
            call += 1;
            Ok(HttpResponse::new(200, if call == 1 { body } else { "[]" }))
        });
        let provider = GitLabProvider::new(Arc::new(http), Some("t".into()), None);
        let repos = provider.list_repositories().await.unwrap();
        assert_eq!(repos.len(), 1);
        assert_eq!(repos[0].full_name, "grp/api");
        assert!(repos[0].private);
    }

    #[tokio::test]
    async fn validate_succeeds_on_200() {
        let mut http = MockHttpClient::new();
        http.expect_send().returning(|_| Ok(HttpResponse::new(200, "{}")));
        let provider = GitLabProvider::new(Arc::new(http), Some("t".into()), None);
        assert!(provider.validate().await.is_ok());
    }

    #[tokio::test]
    async fn unauthorized_maps_to_auth_and_blank_base_url_falls_back() {
        let mut http = MockHttpClient::new();
        http.expect_send()
            .withf(|req| req.url.starts_with("https://gitlab.com/"))
            .returning(|_| Ok(HttpResponse::new(401, "")));
        // A blank base URL falls back to the default API host.
        let provider = GitLabProvider::new(Arc::new(http), None, Some("  ".into()));
        assert!(matches!(provider.validate().await, Err(ProviderError::Auth)));
    }

    #[tokio::test]
    async fn rate_limited_and_server_errors_map() {
        let mut http = MockHttpClient::new();
        http.expect_send().returning(|_| Ok(HttpResponse::new(429, "")));
        let provider = GitLabProvider::new(Arc::new(http), Some("t".into()), None);
        assert!(matches!(provider.validate().await, Err(ProviderError::RateLimited { .. })));

        let mut http2 = MockHttpClient::new();
        http2.expect_send().returning(|_| Ok(HttpResponse::new(500, "")));
        let provider2 = GitLabProvider::new(Arc::new(http2), Some("t".into()), None);
        assert!(matches!(provider2.validate().await, Err(ProviderError::Request(_))));
    }
}
