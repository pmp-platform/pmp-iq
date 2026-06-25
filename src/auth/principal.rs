//! Identity of an authenticated operator.

use serde::{Deserialize, Serialize};

/// An authenticated operator (the person logging into Platform Inspector).
///
/// Distinct from the discovered platform `users`/`groups` model populated by
/// the review job.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Principal {
    pub username: String,
    pub display_name: String,
    pub roles: Vec<String>,
}

impl Principal {
    pub fn admin(username: impl Into<String>) -> Self {
        let username = username.into();
        Self {
            display_name: username.clone(),
            username,
            roles: vec!["admin".to_string()],
        }
    }

    pub fn has_role(&self, role: &str) -> bool {
        self.roles.iter().any(|r| r == role)
    }
}

/// Credentials submitted at login.
#[derive(Clone, Debug, Deserialize)]
pub struct Credentials {
    pub username: String,
    pub password: String,
}

/// Errors raised during authentication.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AuthError {
    #[error("invalid username or password")]
    InvalidCredentials,

    #[error("password hashing error: {0}")]
    Hashing(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admin_principal_has_admin_role() {
        let p = Principal::admin("root");
        assert_eq!(p.username, "root");
        assert!(p.has_role("admin"));
        assert!(!p.has_role("viewer"));
    }
}
