//! Pluggable login strategies.

use super::password::PasswordHasher;
use super::principal::{AuthError, Credentials, Principal};
use async_trait::async_trait;
use std::sync::Arc;

/// A way of authenticating credentials into a [`Principal`].
#[async_trait]
pub trait LoginStrategy: Send + Sync {
    /// Stable identifier for the strategy (e.g. `"static-admin"`).
    fn name(&self) -> &str;

    /// Attempt to authenticate. Returns [`AuthError::InvalidCredentials`] when
    /// the credentials are not accepted by this strategy.
    async fn authenticate(&self, creds: &Credentials) -> Result<Principal, AuthError>;
}

/// Authenticates a single configured admin account against a stored hash.
pub struct StaticAdminStrategy {
    username: String,
    password_hash: String,
    hasher: Arc<dyn PasswordHasher>,
}

impl StaticAdminStrategy {
    pub fn new(username: String, password_hash: String, hasher: Arc<dyn PasswordHasher>) -> Self {
        Self {
            username,
            password_hash,
            hasher,
        }
    }
}

#[async_trait]
impl LoginStrategy for StaticAdminStrategy {
    fn name(&self) -> &str {
        "static-admin"
    }

    async fn authenticate(&self, creds: &Credentials) -> Result<Principal, AuthError> {
        if creds.username != self.username {
            return Err(AuthError::InvalidCredentials);
        }
        if self.hasher.verify(&creds.password, &self.password_hash)? {
            Ok(Principal::admin(&self.username))
        } else {
            Err(AuthError::InvalidCredentials)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::password::MockPasswordHasher;

    fn strategy_with(verify_result: bool) -> StaticAdminStrategy {
        let mut hasher = MockPasswordHasher::new();
        hasher
            .expect_verify()
            .returning(move |_, _| Ok(verify_result));
        StaticAdminStrategy::new("admin".into(), "hash".into(), Arc::new(hasher))
    }

    #[tokio::test]
    async fn accepts_correct_credentials() {
        let s = strategy_with(true);
        let creds = Credentials {
            username: "admin".into(),
            password: "pw".into(),
        };
        assert_eq!(s.authenticate(&creds).await.unwrap().username, "admin");
    }

    #[tokio::test]
    async fn rejects_wrong_password() {
        let s = strategy_with(false);
        let creds = Credentials {
            username: "admin".into(),
            password: "bad".into(),
        };
        assert_eq!(
            s.authenticate(&creds).await.unwrap_err(),
            AuthError::InvalidCredentials
        );
    }

    #[tokio::test]
    async fn rejects_unknown_username() {
        let s = strategy_with(true);
        let creds = Credentials {
            username: "someone".into(),
            password: "pw".into(),
        };
        assert_eq!(
            s.authenticate(&creds).await.unwrap_err(),
            AuthError::InvalidCredentials
        );
    }
}
