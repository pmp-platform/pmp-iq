//! GitHub repository provider.

use super::{
    PrCheck, PrComment, PrStatus, ProviderError, PullRequest, PullRequestSpec, RepoMember,
    RepositoryProvider, retry_at_from_headers,
};
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

#[derive(Deserialize)]
struct GhPull {
    number: u64,
    html_url: String,
    state: String,
}

impl From<GhPull> for PullRequest {
    fn from(p: GhPull) -> Self {
        PullRequest {
            number: p.number,
            url: p.html_url,
            state: p.state,
        }
    }
}

#[derive(Deserialize)]
struct GhPullDetail {
    state: String,
    #[serde(default)]
    merged: bool,
    #[serde(default)]
    mergeable: Option<bool>,
    head: GhHead,
}

#[derive(Deserialize)]
struct GhHead {
    sha: String,
}

#[derive(Deserialize)]
struct GhIssueComment {
    id: u64,
    user: GhUser,
    #[serde(default)]
    body: String,
}

#[derive(Deserialize)]
struct GhUser {
    login: String,
}

#[derive(Deserialize)]
struct GhCheckRuns {
    #[serde(default)]
    check_runs: Vec<GhCheckRun>,
}

#[derive(Deserialize)]
struct GhCheckRun {
    name: String,
    status: String,
    #[serde(default)]
    conclusion: Option<String>,
}

/// Lists repositories visible to a GitHub token.
pub struct GitHubProvider {
    http: Arc<dyn HttpClient>,
    token: Option<String>,
    base_url: String,
    /// Optional organization to scope repository listing to; `None` lists the
    /// token's own repositories.
    organization: Option<String>,
}

impl GitHubProvider {
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
        self.authed(HttpRequest::get(url))
    }

    /// Apply the standard GitHub headers + bearer auth to any request.
    fn authed(&self, req: HttpRequest) -> HttpRequest {
        let mut req = req
            .header("User-Agent", "pmp-iq")
            .header("Accept", "application/vnd.github+json");
        if let Some(token) = &self.token {
            req = req.header("Authorization", format!("Bearer {token}"));
        }
        req
    }

    /// Find the open PR for `spec`'s head branch, if one already exists.
    async fn find_open_pr(&self, spec: &PullRequestSpec) -> Result<Option<PullRequest>, ProviderError> {
        let owner = spec.repo_full_name.split('/').next().unwrap_or_default();
        let url = format!(
            "{}/repos/{}/pulls?state=open&head={}:{}",
            self.base_url, spec.repo_full_name, owner, spec.head_branch
        );
        let resp = self
            .http
            .send(self.authed(HttpRequest::get(&url)))
            .await
            .map_err(|e| ProviderError::Request(e.to_string()))?;
        Self::check_status(&resp)?;
        let pulls: Vec<GhPull> =
            serde_json::from_str(&resp.body).map_err(|e| ProviderError::Parse(e.to_string()))?;
        Ok(pulls.into_iter().next().map(PullRequest::from))
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
        // An account-configured organization scopes the token's visible repos
        // to that namespace (keeps outside-collaborator repos the org-only
        // listing endpoint would omit).
        Ok(super::scope_to_namespace(out, &self.organization))
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

    async fn open_pull_request(&self, spec: PullRequestSpec) -> Result<PullRequest, ProviderError> {
        let url = format!("{}/repos/{}/pulls", self.base_url, spec.repo_full_name);
        let body = json!({
            "title": spec.title,
            "head": spec.head_branch,
            "base": spec.base_branch,
            "body": spec.body,
        });
        let resp = self
            .http
            .send(self.authed(HttpRequest::post(&url, body.to_string())))
            .await
            .map_err(|e| ProviderError::Request(e.to_string()))?;
        // 422 typically means a PR already exists for this head branch.
        if resp.status == 422 {
            if let Some(existing) = self.find_open_pr(&spec).await? {
                return Ok(existing);
            }
        }
        Self::check_status(&resp)?;
        let pr: GhPull =
            serde_json::from_str(&resp.body).map_err(|e| ProviderError::Parse(e.to_string()))?;
        Ok(pr.into())
    }

    async fn get_pull_request(
        &self,
        repo_full_name: &str,
        number: u64,
    ) -> Result<PullRequest, ProviderError> {
        let url = format!("{}/repos/{}/pulls/{number}", self.base_url, repo_full_name);
        let resp = self
            .http
            .send(self.request(&url))
            .await
            .map_err(|e| ProviderError::Request(e.to_string()))?;
        Self::check_status(&resp)?;
        let pr: GhPull =
            serde_json::from_str(&resp.body).map_err(|e| ProviderError::Parse(e.to_string()))?;
        Ok(pr.into())
    }

    async fn pull_request_status(
        &self,
        repo_full_name: &str,
        number: u64,
    ) -> Result<PrStatus, ProviderError> {
        let url = format!("{}/repos/{}/pulls/{number}", self.base_url, repo_full_name);
        let resp = self
            .http
            .send(self.request(&url))
            .await
            .map_err(|e| ProviderError::Request(e.to_string()))?;
        Self::check_status(&resp)?;
        let pr: GhPullDetail =
            serde_json::from_str(&resp.body).map_err(|e| ProviderError::Parse(e.to_string()))?;
        let state = if pr.merged { "merged" } else { pr.state.as_str() }.to_string();
        Ok(PrStatus { state, mergeable: pr.mergeable, head_sha: pr.head.sha })
    }

    async fn pull_request_comments(
        &self,
        repo_full_name: &str,
        number: u64,
    ) -> Result<Vec<PrComment>, ProviderError> {
        let url =
            format!("{}/repos/{}/issues/{number}/comments?per_page=100", self.base_url, repo_full_name);
        let resp = self
            .http
            .send(self.request(&url))
            .await
            .map_err(|e| ProviderError::Request(e.to_string()))?;
        Self::check_status(&resp)?;
        let comments: Vec<GhIssueComment> =
            serde_json::from_str(&resp.body).map_err(|e| ProviderError::Parse(e.to_string()))?;
        Ok(comments
            .into_iter()
            .map(|c| PrComment { id: c.id, author: c.user.login, body: c.body })
            .collect())
    }

    async fn pull_request_checks(
        &self,
        repo_full_name: &str,
        head_sha: &str,
    ) -> Result<Vec<PrCheck>, ProviderError> {
        let url = format!(
            "{}/repos/{}/commits/{head_sha}/check-runs?per_page=100",
            self.base_url, repo_full_name
        );
        let resp = self
            .http
            .send(self.request(&url))
            .await
            .map_err(|e| ProviderError::Request(e.to_string()))?;
        Self::check_status(&resp)?;
        let runs: GhCheckRuns =
            serde_json::from_str(&resp.body).map_err(|e| ProviderError::Parse(e.to_string()))?;
        Ok(runs
            .check_runs
            .into_iter()
            .map(|c| PrCheck { name: c.name, status: c.status, conclusion: c.conclusion })
            .collect())
    }

    async fn post_pull_request_comment(
        &self,
        repo_full_name: &str,
        number: u64,
        body: &str,
    ) -> Result<(), ProviderError> {
        let url = format!("{}/repos/{}/issues/{number}/comments", self.base_url, repo_full_name);
        let payload = json!({ "body": body });
        let resp = self
            .http
            .send(self.authed(HttpRequest::post(&url, payload.to_string())))
            .await
            .map_err(|e| ProviderError::Request(e.to_string()))?;
        Self::check_status(&resp)?;
        Ok(())
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
        let provider = GitHubProvider::new(Arc::new(http), Some("t".into()), None, None);
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
        let provider = GitHubProvider::new(Arc::new(http), Some("t".into()), None, None);
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
        let provider = GitHubProvider::new(Arc::new(http), Some("t".into()), None, None);
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
        let provider = GitHubProvider::new(Arc::new(http), Some("t".into()), Some("  ".into()), None);
        assert!(provider.list_repositories().await.is_ok());
    }

    /// Two accessible repos across a page; only those under the configured org
    /// namespace survive (still listed via the token-scoped `/user/repos`, so
    /// outside-collaborator repos are included).
    const MIXED_REPOS: &str = r#"[
        {"name":"api","full_name":"acme/api","clone_url":"u1","default_branch":"main","private":true},
        {"name":"lib","full_name":"other/lib","clone_url":"u2","default_branch":"main","private":false}
    ]"#;

    #[tokio::test]
    async fn organization_filters_listing_to_namespace() {
        let mut http = MockHttpClient::new();
        let mut call = 0;
        http.expect_send()
            .withf(|req| req.url.contains("/user/repos"))
            .returning(move |_| {
                call += 1;
                Ok(if call == 1 { ok(MIXED_REPOS) } else { ok("[]") })
            });
        let provider =
            GitHubProvider::new(Arc::new(http), Some("t".into()), None, Some("acme".into()));
        let repos = provider.list_repositories().await.unwrap();
        assert_eq!(repos.len(), 1);
        assert_eq!(repos[0].full_name, "acme/api");
    }

    #[tokio::test]
    async fn blank_organization_lists_all_token_repos() {
        let mut http = MockHttpClient::new();
        let mut call = 0;
        http.expect_send()
            .withf(|req| req.url.contains("/user/repos"))
            .returning(move |_| {
                call += 1;
                Ok(if call == 1 { ok(MIXED_REPOS) } else { ok("[]") })
            });
        // A blank organization is treated as unset: no namespace filtering.
        let provider =
            GitHubProvider::new(Arc::new(http), Some("t".into()), None, Some("  ".into()));
        assert_eq!(provider.list_repositories().await.unwrap().len(), 2);
    }

    fn pr_spec() -> PullRequestSpec {
        PullRequestSpec {
            repo_full_name: "org/api".into(),
            head_branch: "agent/x".into(),
            base_branch: "main".into(),
            title: "Add endpoint".into(),
            body: "body".into(),
        }
    }

    #[tokio::test]
    async fn opens_pull_request() {
        let mut http = MockHttpClient::new();
        http.expect_send()
            .withf(|r| r.method == "POST" && r.url.ends_with("/repos/org/api/pulls"))
            .returning(|_| {
                Ok(ok(
                    r#"{"number":7,"html_url":"https://github.com/org/api/pull/7","state":"open"}"#,
                ))
            });
        let provider = GitHubProvider::new(Arc::new(http), Some("t".into()), None, None);
        let pr = provider.open_pull_request(pr_spec()).await.unwrap();
        assert_eq!(pr.number, 7);
        assert_eq!(pr.url, "https://github.com/org/api/pull/7");
    }

    #[tokio::test]
    async fn open_pr_reuses_existing_on_conflict() {
        let mut http = MockHttpClient::new();
        http.expect_send().returning(|req| {
            if req.method == "POST" {
                Ok(HttpResponse::new(422, r#"{"message":"already exists"}"#))
            } else {
                Ok(ok(
                    r#"[{"number":3,"html_url":"https://github.com/org/api/pull/3","state":"open"}]"#,
                ))
            }
        });
        let provider = GitHubProvider::new(Arc::new(http), Some("t".into()), None, None);
        let pr = provider.open_pull_request(pr_spec()).await.unwrap();
        assert_eq!(pr.number, 3);
    }

    #[tokio::test]
    async fn pull_request_status_detects_merged_and_head() {
        let mut http = MockHttpClient::new();
        http.expect_send().returning(|_| {
            Ok(ok(r#"{"state":"closed","merged":true,"mergeable":null,"head":{"sha":"abc123"}}"#))
        });
        let p = GitHubProvider::new(Arc::new(http), Some("t".into()), None, None);
        let s = p.pull_request_status("org/api", 7).await.unwrap();
        assert_eq!(s.state, "merged");
        assert_eq!(s.head_sha, "abc123");
    }

    #[tokio::test]
    async fn pull_request_checks_report_failures() {
        let mut http = MockHttpClient::new();
        http.expect_send()
            .withf(|r| r.url.contains("/commits/abc/check-runs"))
            .returning(|_| {
                Ok(ok(r#"{"check_runs":[{"name":"build","status":"completed","conclusion":"failure"}]}"#))
            });
        let p = GitHubProvider::new(Arc::new(http), Some("t".into()), None, None);
        let checks = p.pull_request_checks("org/api", "abc").await.unwrap();
        assert_eq!(checks.len(), 1);
        assert_eq!(checks[0].name, "build");
        assert_eq!(checks[0].conclusion.as_deref(), Some("failure"));
    }

    #[tokio::test]
    async fn comments_listed_and_posted() {
        let mut http = MockHttpClient::new();
        http.expect_send().returning(|r| {
            if r.method == "POST" {
                Ok(ok(r#"{"id":1}"#))
            } else {
                Ok(ok(r#"[{"id":5,"user":{"login":"alice"},"body":"please fix"}]"#))
            }
        });
        let p = GitHubProvider::new(Arc::new(http), Some("t".into()), None, None);
        let comments = p.pull_request_comments("org/api", 7).await.unwrap();
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].author, "alice");
        assert!(p.post_pull_request_comment("org/api", 7, "🤖 on it").await.is_ok());
    }

    #[tokio::test]
    async fn unauthorized_maps_to_auth_error() {
        let mut http = MockHttpClient::new();
        http.expect_send()
            .returning(|_| Ok(HttpResponse::new(401, "")));
        let provider = GitHubProvider::new(Arc::new(http), Some("bad".into()), None, None);
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
        let provider = GitHubProvider::new(Arc::new(http), Some("t".into()), None, None);
        match provider.validate().await {
            Err(ProviderError::RateLimited { retry_at }) => assert!(retry_at.is_some()),
            other => panic!("expected RateLimited, got {other:?}"),
        }
    }
}
