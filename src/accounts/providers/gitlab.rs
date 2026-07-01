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
    /// Optional group to scope project listing to; `None` lists all projects
    /// the token is a member of.
    organization: Option<String>,
}

impl GitLabProvider {
    pub fn new(
        http: Arc<dyn HttpClient>,
        token: Option<String>,
        base_url: Option<String>,
        organization: Option<String>,
    ) -> Self {
        Self {
            http,
            token,
            base_url: crate::strings::blank_to_none(base_url)
                .unwrap_or_else(|| DEFAULT_API.to_string()),
            organization: crate::strings::blank_to_none(organization),
        }
    }

    fn request(&self, url: &str) -> HttpRequest {
        let mut req = HttpRequest::get(url).header("User-Agent", "pmp-iq");
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
        // A configured group scopes the token's visible projects to that
        // namespace (subgroups included via the `group/…` path prefix).
        Ok(super::scope_to_namespace(out, &self.organization))
    }

    async fn get_repository(&self, full_name: &str) -> Result<Option<RemoteRepo>, ProviderError> {
        // The GitLab project API takes the URL-encoded `namespace/project` path.
        let enc = crate::strings::percent_encode(full_name);
        let url = format!("{}/api/v4/projects/{enc}", self.base_url);
        let resp = self
            .http
            .send(self.request(&url))
            .await
            .map_err(|e| ProviderError::Request(e.to_string()))?;
        if resp.status == 404 {
            return Ok(None);
        }
        Self::check_status(&resp)?;
        let p: GlProject =
            serde_json::from_str(&resp.body).map_err(|e| ProviderError::Parse(e.to_string()))?;
        Ok(Some(RemoteRepo {
            name: p.path,
            full_name: p.path_with_namespace,
            clone_url: p.http_url_to_repo,
            default_branch: p.default_branch,
            private: p.visibility.as_deref() != Some("public"),
        }))
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
        let provider = GitLabProvider::new(Arc::new(http), Some("t".into()), None, None);
        let repos = provider.list_repositories().await.unwrap();
        assert_eq!(repos.len(), 1);
        assert_eq!(repos[0].full_name, "grp/api");
        assert!(repos[0].private);
    }

    // Projects across a group and a nested subgroup, plus an unrelated one.
    const GL_MIXED: &str = r#"[
        {"path":"api","path_with_namespace":"acme/api","http_url_to_repo":"u1","default_branch":"main","visibility":"private"},
        {"path":"svc","path_with_namespace":"acme/team/svc","http_url_to_repo":"u2","default_branch":"main","visibility":"private"},
        {"path":"lib","path_with_namespace":"other/lib","http_url_to_repo":"u3","default_branch":"main","visibility":"public"}
    ]"#;

    fn gl_pages(body: &'static str) -> MockHttpClient {
        let mut http = MockHttpClient::new();
        let mut call = 0;
        http.expect_send().returning(move |_| {
            call += 1;
            Ok(HttpResponse::new(200, if call == 1 { body } else { "[]" }))
        });
        http
    }

    #[tokio::test]
    async fn group_filters_listing_to_namespace_including_subgroups() {
        let provider =
            GitLabProvider::new(Arc::new(gl_pages(GL_MIXED)), Some("t".into()), None, Some("acme".into()));
        let repos = provider.list_repositories().await.unwrap();
        assert_eq!(repos.len(), 2);
        assert!(repos.iter().all(|r| r.full_name.starts_with("acme/")));
    }

    #[tokio::test]
    async fn subgroup_matches_only_nested_path() {
        let provider = GitLabProvider::new(
            Arc::new(gl_pages(GL_MIXED)),
            Some("t".into()),
            None,
            Some("acme/team".into()),
        );
        let repos = provider.list_repositories().await.unwrap();
        assert_eq!(repos.len(), 1);
        assert_eq!(repos[0].full_name, "acme/team/svc");
    }

    #[tokio::test]
    async fn get_repository_uses_encoded_path_and_maps_404() {
        let mut http = MockHttpClient::new();
        http.expect_send()
            .withf(|req| req.url.ends_with("/api/v4/projects/acme%2Fteam%2Fsvc"))
            .returning(|_| {
                Ok(HttpResponse::new(
                    200,
                    r#"{"path":"svc","path_with_namespace":"acme/team/svc","http_url_to_repo":"u","default_branch":"main","visibility":"private"}"#,
                ))
            });
        http.expect_send()
            .withf(|req| req.url.ends_with("/api/v4/projects/acme%2Fgone"))
            .returning(|_| Ok(HttpResponse::new(404, "")));
        let provider = GitLabProvider::new(Arc::new(http), Some("t".into()), None, None);
        let found = provider.get_repository("acme/team/svc").await.unwrap();
        assert_eq!(found.unwrap().full_name, "acme/team/svc");
        assert!(provider.get_repository("acme/gone").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn validate_succeeds_on_200() {
        let mut http = MockHttpClient::new();
        http.expect_send().returning(|_| Ok(HttpResponse::new(200, "{}")));
        let provider = GitLabProvider::new(Arc::new(http), Some("t".into()), None, None);
        assert!(provider.validate().await.is_ok());
    }

    #[tokio::test]
    async fn unauthorized_maps_to_auth_and_blank_base_url_falls_back() {
        let mut http = MockHttpClient::new();
        http.expect_send()
            .withf(|req| req.url.starts_with("https://gitlab.com/"))
            .returning(|_| Ok(HttpResponse::new(401, "")));
        // A blank base URL falls back to the default API host.
        let provider = GitLabProvider::new(Arc::new(http), None, Some("  ".into()), None);
        assert!(matches!(provider.validate().await, Err(ProviderError::Auth)));
    }

    #[tokio::test]
    async fn rate_limited_and_server_errors_map() {
        let mut http = MockHttpClient::new();
        http.expect_send().returning(|_| Ok(HttpResponse::new(429, "")));
        let provider = GitLabProvider::new(Arc::new(http), Some("t".into()), None, None);
        assert!(matches!(provider.validate().await, Err(ProviderError::RateLimited { .. })));

        let mut http2 = MockHttpClient::new();
        http2.expect_send().returning(|_| Ok(HttpResponse::new(500, "")));
        let provider2 = GitLabProvider::new(Arc::new(http2), Some("t".into()), None, None);
        assert!(matches!(provider2.validate().await, Err(ProviderError::Request(_))));
    }
}
