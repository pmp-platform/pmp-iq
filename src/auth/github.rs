//! GitHub authentication: an identity client over `HttpClient`, an allowlist
//! check, and a [`LoginStrategy`] for the personal-token form path. The OAuth
//! web flow (routes) reuses [`GitHubIdentity`] + [`authorize`] directly.

use super::principal::{AuthError, Credentials, Principal};
use super::strategy::LoginStrategy;
use crate::config::{GitHubAuthConfig, GitHubAuthMode};
use crate::httpclient::{HttpClient, HttpRequest};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

/// A GitHub account identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHubUser {
    pub login: String,
    pub id: u64,
}

/// Parameters for exchanging an OAuth authorization `code` for a token.
#[derive(Debug, Clone)]
pub struct OAuthExchange {
    pub client_id: String,
    pub client_secret: String,
    pub code: String,
    pub redirect_url: String,
}

/// The GitHub API surface the auth layer needs. Mocked in tests.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait GitHubIdentity: Send + Sync {
    /// `GET /user` with a bearer token → the authenticated user.
    async fn current_user(&self, token: &str) -> Result<GitHubUser, AuthError>;

    /// `GET /user/orgs` → org logins the user belongs to (for the allowlist).
    async fn user_orgs(&self, token: &str) -> Result<Vec<String>, AuthError>;

    /// Exchange an OAuth `code` for an access token (oauth_app mode).
    async fn exchange_code(&self, exchange: OAuthExchange) -> Result<String, AuthError>;
}

/// Decide whether a verified GitHub user may sign in. Safe-by-default: with both
/// allowlists empty, no one is authorised.
pub fn authorize(user: &GitHubUser, orgs: &[String], cfg: &GitHubAuthConfig) -> bool {
    let login_ok = cfg
        .allowed_logins
        .iter()
        .any(|l| l.eq_ignore_ascii_case(&user.login));
    let org_ok = orgs
        .iter()
        .any(|o| cfg.allowed_orgs.iter().any(|a| a.eq_ignore_ascii_case(o)));
    login_ok || org_ok
}

#[derive(Deserialize)]
struct GhUser {
    login: String,
    id: u64,
}

#[derive(Deserialize)]
struct GhOrg {
    login: String,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
}

fn provider(e: impl std::fmt::Display) -> AuthError {
    AuthError::Provider(e.to_string())
}

/// `GitHubIdentity` over an [`HttpClient`].
pub struct HttpGitHubIdentity {
    http: Arc<dyn HttpClient>,
    api_base_url: String,
    web_base_url: String,
}

impl HttpGitHubIdentity {
    pub fn new(http: Arc<dyn HttpClient>, cfg: &GitHubAuthConfig) -> Self {
        Self {
            http,
            api_base_url: cfg.api_base_url.clone(),
            web_base_url: cfg.web_base_url.clone(),
        }
    }

    fn get(url: &str, token: &str) -> HttpRequest {
        HttpRequest::get(url)
            .header("User-Agent", "platform-inspector")
            .header("Accept", "application/vnd.github+json")
            .header("Authorization", format!("Bearer {token}"))
    }
}

#[async_trait]
impl GitHubIdentity for HttpGitHubIdentity {
    async fn current_user(&self, token: &str) -> Result<GitHubUser, AuthError> {
        let url = format!("{}/user", self.api_base_url);
        let resp = self.http.send(Self::get(&url, token)).await.map_err(provider)?;
        if resp.status == 401 || resp.status == 403 {
            return Err(AuthError::InvalidCredentials);
        }
        if !resp.is_success() {
            return Err(AuthError::Provider(format!("GET /user → {}", resp.status)));
        }
        let user: GhUser = serde_json::from_str(&resp.body).map_err(provider)?;
        Ok(GitHubUser { login: user.login, id: user.id })
    }

    async fn user_orgs(&self, token: &str) -> Result<Vec<String>, AuthError> {
        let url = format!("{}/user/orgs?per_page=100", self.api_base_url);
        let resp = self.http.send(Self::get(&url, token)).await.map_err(provider)?;
        if !resp.is_success() {
            // A missing read:org scope must not block login by login-allowlist.
            return Ok(Vec::new());
        }
        let orgs: Vec<GhOrg> = serde_json::from_str(&resp.body).map_err(provider)?;
        Ok(orgs.into_iter().map(|o| o.login).collect())
    }

    async fn exchange_code(&self, exchange: OAuthExchange) -> Result<String, AuthError> {
        let url = format!("{}/login/oauth/access_token", self.web_base_url);
        let body = json!({
            "client_id": exchange.client_id,
            "client_secret": exchange.client_secret,
            "code": exchange.code,
            "redirect_uri": exchange.redirect_url,
        });
        let req = HttpRequest::post(&url, body.to_string())
            .header("User-Agent", "platform-inspector")
            .header("Accept", "application/json")
            .header("Content-Type", "application/json");
        let resp = self.http.send(req).await.map_err(provider)?;
        if !resp.is_success() {
            return Err(AuthError::Provider(format!("token exchange → {}", resp.status)));
        }
        let parsed: TokenResponse = serde_json::from_str(&resp.body).map_err(provider)?;
        // A `null` access_token means GitHub rejected the code.
        parsed.access_token.ok_or(AuthError::InvalidCredentials)
    }
}

/// Authenticates a GitHub personal access token submitted at the login form, and
/// (via [`authenticate_token`](Self::authenticate_token)) backs the OAuth flow.
pub struct GitHubLoginStrategy {
    identity: Arc<dyn GitHubIdentity>,
    config: GitHubAuthConfig,
}

impl GitHubLoginStrategy {
    pub fn new(identity: Arc<dyn GitHubIdentity>, config: GitHubAuthConfig) -> Self {
        Self { identity, config }
    }

    /// Verify an access token and authorise the user against the allowlist.
    /// Shared by the personal-token form path and the OAuth callback.
    pub async fn authenticate_token(&self, token: &str) -> Result<Principal, AuthError> {
        let user = self.identity.current_user(token).await?;
        let orgs = self.identity.user_orgs(token).await?;
        if authorize(&user, &orgs, &self.config) {
            Ok(Principal::github(&user.login))
        } else {
            Err(AuthError::InvalidCredentials)
        }
    }
}

#[async_trait]
impl LoginStrategy for GitHubLoginStrategy {
    fn name(&self) -> &str {
        "github"
    }

    async fn authenticate(&self, creds: &Credentials) -> Result<Principal, AuthError> {
        // Only personal-token mode authenticates via the form; oauth_app uses
        // the dedicated routes.
        if self.config.mode != GitHubAuthMode::PersonalToken {
            return Err(AuthError::InvalidCredentials);
        }
        let token = creds.password.trim();
        if token.is_empty() {
            return Err(AuthError::InvalidCredentials);
        }
        self.authenticate_token(token).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::GitHubAuthMode;
    use crate::httpclient::{HttpResponse, MockHttpClient};

    fn cfg(mode: GitHubAuthMode, orgs: &[&str], logins: &[&str]) -> GitHubAuthConfig {
        GitHubAuthConfig {
            mode,
            client_id: Some("cid".into()),
            client_secret: Some("secret".into()),
            redirect_url: Some("http://localhost/cb".into()),
            api_base_url: "https://api.github.com".into(),
            web_base_url: "https://github.com".into(),
            allowed_orgs: orgs.iter().map(|s| s.to_string()).collect(),
            allowed_logins: logins.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn user(login: &str) -> GitHubUser {
        GitHubUser { login: login.into(), id: 1 }
    }

    #[test]
    fn authorize_allows_listed_login() {
        let c = cfg(GitHubAuthMode::PersonalToken, &[], &["alice"]);
        assert!(authorize(&user("Alice"), &[], &c)); // case-insensitive
        assert!(!authorize(&user("mallory"), &[], &c));
    }

    #[test]
    fn authorize_allows_org_member() {
        let c = cfg(GitHubAuthMode::PersonalToken, &["acme"], &[]);
        assert!(authorize(&user("bob"), &["ACME".into()], &c));
        assert!(!authorize(&user("bob"), &["other".into()], &c));
    }

    #[test]
    fn authorize_denies_when_allowlists_empty() {
        let c = cfg(GitHubAuthMode::PersonalToken, &[], &[]);
        assert!(!authorize(&user("anyone"), &["any".into()], &c));
    }

    fn identity_for(login: &'static str, orgs: Vec<String>) -> MockGitHubIdentity {
        let mut id = MockGitHubIdentity::new();
        id.expect_current_user().returning(move |_| Ok(user(login)));
        id.expect_user_orgs().returning(move |_| Ok(orgs.clone()));
        id
    }

    #[tokio::test]
    async fn personal_token_authenticates_allowlisted_user() {
        let strategy = GitHubLoginStrategy::new(
            Arc::new(identity_for("alice", vec![])),
            cfg(GitHubAuthMode::PersonalToken, &[], &["alice"]),
        );
        let creds = Credentials { username: "alice".into(), password: "ghp_token".into() };
        let principal = strategy.authenticate(&creds).await.unwrap();
        assert_eq!(principal.username, "alice");
        assert!(principal.has_role("admin"));
    }

    #[tokio::test]
    async fn personal_token_denies_non_allowlisted_user() {
        let strategy = GitHubLoginStrategy::new(
            Arc::new(identity_for("mallory", vec![])),
            cfg(GitHubAuthMode::PersonalToken, &["acme"], &["alice"]),
        );
        let creds = Credentials { username: "mallory".into(), password: "ghp".into() };
        assert_eq!(
            strategy.authenticate(&creds).await.unwrap_err(),
            AuthError::InvalidCredentials
        );
    }

    #[tokio::test]
    async fn oauth_app_mode_rejects_form_login() {
        let mut id = MockGitHubIdentity::new();
        id.expect_current_user().never();
        let strategy = GitHubLoginStrategy::new(
            Arc::new(id),
            cfg(GitHubAuthMode::OauthApp, &[], &["alice"]),
        );
        let creds = Credentials { username: "alice".into(), password: "x".into() };
        assert_eq!(
            strategy.authenticate(&creds).await.unwrap_err(),
            AuthError::InvalidCredentials
        );
    }

    #[tokio::test]
    async fn http_identity_parses_current_user() {
        let mut http = MockHttpClient::new();
        http.expect_send()
            .withf(|r| r.url.ends_with("/user"))
            .returning(|_| Ok(HttpResponse::new(200, r#"{"login":"alice","id":42}"#)));
        let identity = HttpGitHubIdentity::new(Arc::new(http), &cfg(GitHubAuthMode::OauthApp, &[], &[]));
        let user = identity.current_user("tok").await.unwrap();
        assert_eq!(user, GitHubUser { login: "alice".into(), id: 42 });
    }

    #[tokio::test]
    async fn http_identity_bad_token_is_invalid_credentials() {
        let mut http = MockHttpClient::new();
        http.expect_send().returning(|_| Ok(HttpResponse::new(401, "")));
        let identity = HttpGitHubIdentity::new(Arc::new(http), &cfg(GitHubAuthMode::OauthApp, &[], &[]));
        assert_eq!(
            identity.current_user("bad").await.unwrap_err(),
            AuthError::InvalidCredentials
        );
    }

    #[tokio::test]
    async fn http_identity_exchanges_code_for_token() {
        let mut http = MockHttpClient::new();
        http.expect_send()
            .withf(|r| r.method == "POST" && r.url.ends_with("/login/oauth/access_token"))
            .returning(|_| Ok(HttpResponse::new(200, r#"{"access_token":"gho_abc"}"#)));
        let identity = HttpGitHubIdentity::new(Arc::new(http), &cfg(GitHubAuthMode::OauthApp, &[], &[]));
        let token = identity
            .exchange_code(OAuthExchange {
                client_id: "cid".into(),
                client_secret: "secret".into(),
                code: "code123".into(),
                redirect_url: "http://localhost/cb".into(),
            })
            .await
            .unwrap();
        assert_eq!(token, "gho_abc");
    }

    #[tokio::test]
    async fn http_identity_rejects_bad_code() {
        let mut http = MockHttpClient::new();
        http.expect_send()
            .returning(|_| Ok(HttpResponse::new(200, r#"{"error":"bad_verification_code"}"#)));
        let identity = HttpGitHubIdentity::new(Arc::new(http), &cfg(GitHubAuthMode::OauthApp, &[], &[]));
        let err = identity
            .exchange_code(OAuthExchange {
                client_id: "cid".into(),
                client_secret: "secret".into(),
                code: "bad".into(),
                redirect_url: "http://localhost/cb".into(),
            })
            .await
            .unwrap_err();
        assert_eq!(err, AuthError::InvalidCredentials);
    }
}
