//! GitHub repository provider.

use super::{ProviderError, RepoMember, RepositoryProvider, retry_at_from_headers};
use crate::accounts::model::RemoteRepo;
use crate::httpclient::{HttpClient, HttpRequest, HttpResponse};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

const DEFAULT_API: &str = "https://api.github.com";
const MAX_PAGES: u32 = 10;

#[derive(Deserialize)]
struct GhRepo {
    name: String,
    full_name: String,
    clone_url: String,
    default_branch: Option<String>,
    private: bool,
}

#[derive(Deserialize)]
struct GhCollaborator {
    login: String,
    #[serde(default)]
    role_name: Option<String>,
    #[serde(default)]
    permissions: Option<Value>,
}

/// Lists repositories visible to a GitHub token.
pub struct GitHubProvider {
    http: Arc<dyn HttpClient>,
    token: Option<String>,
    base_url: String,
}

impl GitHubProvider {
    pub fn new(http: Arc<dyn HttpClient>, token: Option<String>, base_url: Option<String>) -> Self {
        Self {
            http,
            token,
            base_url: crate::strings::blank_to_none(base_url)
                .unwrap_or_else(|| DEFAULT_API.to_string()),
        }
    }

    fn request(&self, url: &str) -> HttpRequest {
        let mut req = HttpRequest::get(url)
            .header("User-Agent", "platform-inspector")
            .header("Accept", "application/vnd.github+json");
        if let Some(token) = &self.token {
            req = req.header("Authorization", format!("Bearer {token}"));
        }
        req
    }

    fn check_status(resp: &HttpResponse) -> Result<(), ProviderError> {
        let rate_limited = resp.status == 429
            || (resp.status == 403 && resp.header("x-ratelimit-remaining") == Some("0"));
        match resp.status {
            s if (200..300).contains(&s) => Ok(()),
            _ if rate_limited => Err(ProviderError::RateLimited {
                retry_at: retry_at_from_headers(resp),
            }),
            401 | 403 => Err(ProviderError::Auth),
            s => Err(ProviderError::Request(format!("status {s}"))),
        }
    }
}

#[async_trait]
impl RepositoryProvider for GitHubProvider {
    async fn validate(&self) -> Result<(), ProviderError> {
        let url = format!("{}/user", self.base_url);
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
            let url = format!("{}/user/repos?per_page=100&page={page}", self.base_url);
            let resp = self
                .http
                .send(self.request(&url))
                .await
                .map_err(|e| ProviderError::Request(e.to_string()))?;
            Self::check_status(&resp)?;
            let repos: Vec<GhRepo> = serde_json::from_str(&resp.body)
                .map_err(|e| ProviderError::Parse(e.to_string()))?;
            if repos.is_empty() {
                break;
            }
            out.extend(repos.into_iter().map(|r| RemoteRepo {
                name: r.name,
                full_name: r.full_name,
                clone_url: r.clone_url,
                default_branch: r.default_branch,
                private: r.private,
            }));
        }
        Ok(out)
    }

    async fn list_members(&self, repo_full_name: &str) -> Result<Vec<RepoMember>, ProviderError> {
        let mut out = Vec::new();
        for page in 1..=MAX_PAGES {
            let url = format!(
                "{}/repos/{repo_full_name}/collaborators?affiliation=all&per_page=100&page={page}",
                self.base_url
            );
            let resp = self
                .http
                .send(self.request(&url))
                .await
                .map_err(|e| ProviderError::Request(e.to_string()))?;
            Self::check_status(&resp)?;
            let collaborators: Vec<GhCollaborator> = serde_json::from_str(&resp.body)
                .map_err(|e| ProviderError::Parse(e.to_string()))?;
            if collaborators.is_empty() {
                break;
            }
            out.extend(collaborators.into_iter().map(|c| RepoMember {
                username: c.login,
                email: None,
                role: c.role_name,
                permissions: c.permissions.unwrap_or_else(|| json!({})),
            }));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::httpclient::MockHttpClient;

    fn ok(body: &str) -> HttpResponse {
        HttpResponse::new(200, body)
    }

    #[tokio::test]
    async fn lists_and_maps_repositories() {
        let mut http = MockHttpClient::new();
        let page1 = r#"[{"name":"api","full_name":"org/api","clone_url":"https://x/api.git","default_branch":"main","private":true}]"#;
        let mut call = 0;
        http.expect_send().returning(move |_| {
            call += 1;
            Ok(if call == 1 { ok(page1) } else { ok("[]") })
        });
        let provider = GitHubProvider::new(Arc::new(http), Some("t".into()), None);
        let repos = provider.list_repositories().await.unwrap();
        assert_eq!(repos.len(), 1);
        assert_eq!(repos[0].full_name, "org/api");
        assert!(repos[0].private);
    }

    #[tokio::test]
    async fn lists_and_maps_members() {
        let mut http = MockHttpClient::new();
        let page1 = r#"[{"login":"alice","role_name":"admin","permissions":{"admin":true,"push":true,"pull":true}},
            {"login":"bob","role_name":"write","permissions":{"admin":false,"push":true,"pull":true}}]"#;
        let mut call = 0;
        http.expect_send()
            .withf(|req| req.url.contains("/repos/org/api/collaborators"))
            .returning(move |_| {
                call += 1;
                Ok(if call == 1 { ok(page1) } else { ok("[]") })
            });
        let provider = GitHubProvider::new(Arc::new(http), Some("t".into()), None);
        let members = provider.list_members("org/api").await.unwrap();
        assert_eq!(members.len(), 2);
        assert_eq!(members[0].username, "alice");
        assert_eq!(members[0].role.as_deref(), Some("admin"));
        assert_eq!(members[0].permissions["admin"], serde_json::json!(true));
        assert_eq!(members[1].username, "bob");
        assert_eq!(members[1].role.as_deref(), Some("write"));
    }

    #[tokio::test]
    async fn members_default_empty_permissions_when_absent() {
        let mut http = MockHttpClient::new();
        let mut call = 0;
        http.expect_send().returning(move |_| {
            call += 1;
            Ok(if call == 1 { ok(r#"[{"login":"carol"}]"#) } else { ok("[]") })
        });
        let provider = GitHubProvider::new(Arc::new(http), Some("t".into()), None);
        let members = provider.list_members("org/api").await.unwrap();
        assert_eq!(members.len(), 1);
        assert_eq!(members[0].username, "carol");
        assert!(members[0].role.is_none());
        assert_eq!(members[0].permissions, serde_json::json!({}));
    }

    #[tokio::test]
    async fn blank_base_url_falls_back_to_default() {
        let mut http = MockHttpClient::new();
        http.expect_send()
            .withf(|req| req.url.starts_with("https://api.github.com/"))
            .returning(|_| Ok(ok("[]")));
        let provider = GitHubProvider::new(Arc::new(http), Some("t".into()), Some("  ".into()));
        assert!(provider.list_repositories().await.is_ok());
    }

    #[tokio::test]
    async fn unauthorized_maps_to_auth_error() {
        let mut http = MockHttpClient::new();
        http.expect_send()
            .returning(|_| Ok(HttpResponse::new(401, "")));
        let provider = GitHubProvider::new(Arc::new(http), Some("bad".into()), None);
        assert!(matches!(provider.validate().await, Err(ProviderError::Auth)));
    }

    #[tokio::test]
    async fn rate_limited_403_maps_to_rate_limited() {
        let mut http = MockHttpClient::new();
        http.expect_send().returning(|_| {
            let mut resp = HttpResponse::new(403, "");
            resp.headers.insert("x-ratelimit-remaining".into(), "0".into());
            resp.headers.insert("x-ratelimit-reset".into(), "1893456000".into());
            Ok(resp)
        });
        let provider = GitHubProvider::new(Arc::new(http), Some("t".into()), None);
        match provider.validate().await {
            Err(ProviderError::RateLimited { retry_at }) => assert!(retry_at.is_some()),
            other => panic!("expected RateLimited, got {other:?}"),
        }
    }
}
