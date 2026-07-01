//! Roles, teams and ownership types (M37).

use serde::Serialize;
use uuid::Uuid;

/// Operator role, ordered least → most privileged (so `>=` compares power).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Viewer,
    Maintainer,
    Admin,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::Viewer => "viewer",
            Role::Maintainer => "maintainer",
            Role::Admin => "admin",
        }
    }

    /// Parse a role string, defaulting unknown values to the least-privileged
    /// `viewer` (fail closed).
    pub fn parse(raw: &str) -> Role {
        match raw.trim().to_ascii_lowercase().as_str() {
            "admin" => Role::Admin,
            "maintainer" => Role::Maintainer,
            _ => Role::Viewer,
        }
    }

    /// The highest role named in a principal's role list (default viewer).
    pub fn highest(roles: &[String]) -> Role {
        roles.iter().map(|r| Role::parse(r)).max().unwrap_or(Role::Viewer)
    }
}

/// A team that can own applications.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Team {
    pub id: Uuid,
    pub name: String,
    pub tenant_id: Option<String>,
}

/// Fields to create a team.
#[derive(Debug, Clone)]
pub struct TeamInput {
    pub name: String,
    pub tenant_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_ordering_and_parse() {
        assert!(Role::Admin > Role::Maintainer);
        assert!(Role::Maintainer > Role::Viewer);
        assert_eq!(Role::parse("ADMIN"), Role::Admin);
        assert_eq!(Role::parse("nonsense"), Role::Viewer); // fail closed
        assert_eq!(Role::parse("maintainer").as_str(), "maintainer");
    }

    #[test]
    fn highest_picks_strongest() {
        assert_eq!(Role::highest(&["viewer".into(), "admin".into()]), Role::Admin);
        assert_eq!(Role::highest(&[]), Role::Viewer);
    }
}
