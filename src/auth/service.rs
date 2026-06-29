//! The authentication service: resolves the admin account from configuration
//! and authenticates credentials against the registered strategies.

use super::github::{GitHubIdentity, GitHubLoginStrategy};
use super::password::{PasswordHasher, SecretGenerator};
use super::principal::{AuthError, Credentials, Principal};
use super::strategy::{LoginStrategy, StaticAdminStrategy};
use crate::config::{AuthConfig, AuthProvider};
use std::sync::Arc;

/// Describes how the admin account was resolved at boot.
pub struct AdminSetup {
    pub username: String,
    /// Present only when the password was auto-generated.
    pub generated_password: Option<String>,
}

/// Result of building the auth service: the service plus admin setup info.
pub struct AuthBootstrap {
    pub service: AuthService,
    pub admin: AdminSetup,
}

/// Authenticates credentials against an ordered list of login strategies.
pub struct AuthService {
    strategies: Vec<Box<dyn LoginStrategy>>,
}

impl AuthService {
    /// Build the service from configuration, hashing the admin password (from
    /// env, or generated when absent). When `provider` is `github` and a GitHub
    /// identity client is supplied, the GitHub strategy is appended after the
    /// admin strategy so admin login remains a fallback.
    pub fn from_config(
        auth: &AuthConfig,
        hasher: Arc<dyn PasswordHasher>,
        generator: &dyn SecretGenerator,
        github_identity: Option<Arc<dyn GitHubIdentity>>,
    ) -> Result<AuthBootstrap, AuthError> {
        let username = auth.admin_user.clone().unwrap_or_else(|| "admin".to_string());
        let (password, generated) = match &auth.admin_pass {
            Some(p) => (p.clone(), None),
            None => {
                let p = generator.generate(24);
                (p.clone(), Some(p))
            }
        };
        let hash = hasher.hash(&password)?;
        let mut strategies: Vec<Box<dyn LoginStrategy>> =
            vec![Box::new(StaticAdminStrategy::new(username.clone(), hash, hasher))];
        if auth.provider == AuthProvider::Github {
            if let (Some(identity), Some(github)) = (github_identity, auth.github.clone()) {
                strategies.push(Box::new(GitHubLoginStrategy::new(identity, github)));
            }
        }
        Ok(AuthBootstrap {
            service: Self { strategies },
            admin: AdminSetup {
                username,
                generated_password: generated,
            },
        })
    }

    /// Build directly from strategies (used in tests).
    pub fn from_strategies(strategies: Vec<Box<dyn LoginStrategy>>) -> Self {
        Self { strategies }
    }

    /// Try each strategy in turn; the first acceptance wins.
    pub async fn authenticate(&self, creds: &Credentials) -> Result<Principal, AuthError> {
        for strategy in &self.strategies {
            match strategy.authenticate(creds).await {
                Ok(principal) => return Ok(principal),
                Err(AuthError::InvalidCredentials) => continue,
                Err(other) => return Err(other),
            }
        }
        Err(AuthError::InvalidCredentials)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::password::{MockPasswordHasher, MockSecretGenerator};

    fn config(user: Option<&str>, pass: Option<&str>) -> AuthConfig {
        AuthConfig {
            provider: crate::config::AuthProvider::Admin,
            admin_user: user.map(String::from),
            admin_pass: pass.map(String::from),
            session_secret: "s".into(),
            encryption_key: "k".into(),
            github: None,
        }
    }

    #[test]
    fn uses_env_credentials_when_present() {
        let mut hasher = MockPasswordHasher::new();
        hasher.expect_hash().returning(|_| Ok("hash".into()));
        let mut generator = MockSecretGenerator::new();
        generator.expect_generate().never();

        let boot = AuthService::from_config(
            &config(Some("root"), Some("pw")),
            Arc::new(hasher),
            &generator,
            None,
        )
        .unwrap();
        assert_eq!(boot.admin.username, "root");
        assert!(boot.admin.generated_password.is_none());
    }

    #[test]
    fn generates_password_when_absent() {
        let mut hasher = MockPasswordHasher::new();
        hasher.expect_hash().returning(|_| Ok("hash".into()));
        let mut generator = MockSecretGenerator::new();
        generator.expect_generate().returning(|_| "generated-pw".into());

        let boot = AuthService::from_config(&config(None, None), Arc::new(hasher), &generator, None)
            .unwrap();
        assert_eq!(boot.admin.username, "admin");
        assert_eq!(boot.admin.generated_password.as_deref(), Some("generated-pw"));
    }

    #[tokio::test]
    async fn authenticate_round_trip_with_real_hasher() {
        use crate::auth::password::{Argon2Hasher, RandomSecretGenerator};
        let boot = AuthService::from_config(
            &config(Some("admin"), Some("hunter2")),
            Arc::new(Argon2Hasher),
            &RandomSecretGenerator,
            None,
        )
        .unwrap();
        let ok = boot
            .service
            .authenticate(&Credentials {
                username: "admin".into(),
                password: "hunter2".into(),
            })
            .await;
        assert!(ok.is_ok());
        let bad = boot
            .service
            .authenticate(&Credentials {
                username: "admin".into(),
                password: "nope".into(),
            })
            .await;
        assert_eq!(bad.unwrap_err(), AuthError::InvalidCredentials);
    }
}
