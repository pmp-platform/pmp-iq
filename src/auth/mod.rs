//! Authentication: pluggable login strategies, password hashing, sessions and
//! middleware.

pub mod github;
pub mod middleware;
pub mod password;
pub mod principal;
pub mod service;
pub mod strategy;

pub use github::{
    GitHubIdentity, GitHubLoginStrategy, GitHubUser, HttpGitHubIdentity, OAuthExchange, authorize,
};
pub use middleware::{SESSION_PRINCIPAL_KEY, require_auth};
pub use password::{
    Argon2Hasher, PasswordHasher, RandomSecretGenerator, SecretGenerator,
};
pub use principal::{AuthError, Credentials, Principal};
pub use service::{AdminSetup, AuthBootstrap, AuthService};
pub use strategy::{LoginStrategy, StaticAdminStrategy};
