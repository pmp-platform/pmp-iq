# Milestone 21 — GitHub login (GitHub App / personal token)

## Goal

Let an operator switch the app's login from the built-in single admin account to
**GitHub authentication**. The default stays exactly as today (the
`StaticAdminStrategy` from M03). When `auth.provider: github` is configured (M18),
users authenticate with GitHub instead — either through a **GitHub App / OAuth web
flow** ("Sign in with GitHub") or by presenting a **personal access token**. In
both modes the authenticated GitHub identity is authorised against an allowlist
(orgs and/or logins).

## Scope

- A new `GitHubLoginStrategy` implementing the existing `LoginStrategy` trait,
  selected by config; static-admin remains the default.
- Two GitHub modes behind one strategy: **OAuth App** (authorization-code web
  flow) and **personal token** (user presents a PAT).
- A small, mockable GitHub identity client (`GET /user`, `GET /user/orgs`) over
  the existing `HttpClient` trait — no live GitHub in unit tests.
- An allowlist (orgs / logins) deciding who may sign in; clear deny messaging.
- Login-page + routes adapting to the configured provider.

## Deliverables

### Config (M18 `auth` section)

```yaml
auth:
  provider: github            # admin (default) | github
  github:
    mode: oauth_app           # oauth_app | personal_token
    client_id: "${GITHUB_CLIENT_ID}"
    client_secret: "${GITHUB_CLIENT_SECRET}"   # oauth_app mode
    redirect_url: "https://host/auth/github/callback"
    api_base_url: "https://api.github.com"     # override for GHES
    allowed_orgs: ["my-org"]                    # allowlist (any-of)
    allowed_logins: ["alice", "bob"]            # allowlist (any-of)
```

Parse into a typed `GitHubAuthConfig` on `AuthConfig`. When `provider: admin`
(default), none of this is required and the app behaves as today.

### GitHub identity client (mockable)

A focused trait so the strategy is unit-testable without GitHub:

```rust
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait GitHubIdentity: Send + Sync {
    /// GET /user with a bearer token → the authenticated login + id.
    async fn current_user(&self, token: &str) -> Result<GitHubUser, AuthError>;
    /// GET /user/orgs → org logins the user belongs to (for the allowlist).
    async fn user_orgs(&self, token: &str) -> Result<Vec<String>, AuthError>;
}
```

`HttpGitHubIdentity` implements it over the existing `HttpClient` trait (reuse the
client already used by `GitHubProvider`, honouring `api_base_url` for GHES). A
shared `authorize(user, orgs, &GitHubAuthConfig)` helper applies the allowlist
(member of any `allowed_orgs` **or** in `allowed_logins`); empty allowlists mean
"deny all" unless a future explicit "allow any authenticated user" flag is set —
document the chosen default and keep it safe-by-default.

### `GitHubLoginStrategy`

Implements `LoginStrategy` (`name() -> "github"`), constructed with the
`GitHubAuthConfig` + an `Arc<dyn GitHubIdentity>`. It maps a verified, authorised
GitHub user to a `Principal` (carrying the GitHub login; the GitHub token may be
stashed for later write operations — see M22). It serves the two modes:

- **`personal_token`**: the login form accepts a GitHub **token** (no password);
  the strategy calls `current_user` + `user_orgs`, applies the allowlist, and
  authenticates on success. Implemented entirely behind `HttpClient` →
  unit-testable with a mocked `GitHubIdentity`.
- **`oauth_app`**: the web flow does not fit `authenticate(creds)` directly, so
  add OAuth routes (below) that exchange the code for a token, then reuse the same
  `current_user`/`authorize` path to mint the session principal.

### Routes & login page

- `auth.provider` is exposed to the login template so it renders the right UI:
  the existing username/password form (admin), a single **"Sign in with GitHub"**
  button (oauth_app), or a token field (personal_token).
- OAuth web flow (oauth_app mode), public routes:
  - `GET /auth/github/login` → redirect to GitHub's authorize URL with
    `client_id`, `redirect_url`, scopes (`read:user`, `read:org`, plus `repo` when
    M22 PR creation is enabled), and a CSRF `state` stored in the session.
  - `GET /auth/github/callback?code&state` → verify `state`, exchange `code` for a
    token (token endpoint via `HttpClient`), `current_user` + `authorize`, then
    establish the session (reuse the existing session/login plumbing from M03).
- Authorisation failures (not in allowlist, bad token, bad `state`) render a clear
  "access denied" message; nothing leaks token contents.
- `AuthService::from_config` selects the strategy list from `auth.provider`
  (static-admin by default, GitHub strategy when configured). The
  ordered-strategy design (M03) means admin can optionally remain as a fallback.

## Tasks

- [ ] `GitHubAuthConfig` on `AuthConfig`; `AuthProvider` selection in
      `AuthService::from_config`.
- [ ] `GitHubIdentity` trait + `HttpGitHubIdentity` over `HttpClient`; `authorize`
      allowlist helper.
- [ ] `GitHubLoginStrategy` (personal_token via `authenticate`; oauth_app via
      routes) → `Principal`.
- [ ] `/auth/github/login` + `/auth/github/callback` with CSRF `state`; token
      exchange over `HttpClient`.
- [ ] Login template branches on `auth.provider` (password / GitHub button /
      token field).
- [ ] Unit tests (mocked `GitHubIdentity`/`HttpClient`): allowlisted user
      authenticates; non-member and unknown login are denied; bad/invalid token
      rejected; callback rejects a mismatched `state`. No live GitHub.

## Acceptance criteria

- With no config (or `provider: admin`) login is unchanged — the static admin
  account works exactly as today.
- With `provider: github` + `oauth_app`, "Sign in with GitHub" completes the OAuth
  flow and signs in an allowlisted user; non-allowlisted users are denied.
- With `provider: github` + `personal_token`, a valid PAT for an allowlisted user
  authenticates; an invalid or non-allowlisted token is rejected.
- The strategy and routes are unit-tested with a mocked GitHub identity client —
  no test calls GitHub.

## Dependencies

Milestones 03 (login strategies, sessions, `AuthService`), 18 (`auth` config),
and `src/httpclient.rs` (`HttpClient`). The GitHub token captured here is reused
by M22 for pushes / PR creation.

## Out of scope

GitLab SSO, SAML/OIDC generally, GitHub App **installation** management and
fine-grained per-repo permission sync, and team-level (vs org-level) allowlists —
org + login allowlists only.
