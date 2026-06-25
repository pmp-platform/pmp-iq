//! Domain model for repository accounts.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Parse-or-fail helper shared by the small string enums below.
fn parse_enum<T>(value: &str, variants: &[(&str, T)]) -> Result<T, ModelError>
where
    T: Copy,
{
    variants
        .iter()
        .find(|(name, _)| *name == value)
        .map(|(_, v)| *v)
        .ok_or_else(|| ModelError(value.to_string()))
}

/// Error parsing a model enum from its stored representation.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("invalid value: {0}")]
pub struct ModelError(pub String);

/// Kind of source-control provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderType {
    Github,
    Gitlab,
    Local,
}

impl ProviderType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProviderType::Github => "github",
            ProviderType::Gitlab => "gitlab",
            ProviderType::Local => "local",
        }
    }

    pub fn parse(value: &str) -> Result<Self, ModelError> {
        parse_enum(
            value,
            &[
                ("github", ProviderType::Github),
                ("gitlab", ProviderType::Gitlab),
                ("local", ProviderType::Local),
            ],
        )
    }
}

/// How credentials are supplied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthType {
    Token,
    App,
    None,
}

impl AuthType {
    pub fn as_str(&self) -> &'static str {
        match self {
            AuthType::Token => "token",
            AuthType::App => "app",
            AuthType::None => "none",
        }
    }

    pub fn parse(value: &str) -> Result<Self, ModelError> {
        parse_enum(
            value,
            &[
                ("token", AuthType::Token),
                ("app", AuthType::App),
                ("none", AuthType::None),
            ],
        )
    }
}

/// How repositories are selected from an account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SelectionMode {
    All,
    Regex,
    List,
}

impl SelectionMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            SelectionMode::All => "all",
            SelectionMode::Regex => "regex",
            SelectionMode::List => "list",
        }
    }

    pub fn parse(value: &str) -> Result<Self, ModelError> {
        parse_enum(
            value,
            &[
                ("all", SelectionMode::All),
                ("regex", SelectionMode::Regex),
                ("list", SelectionMode::List),
            ],
        )
    }
}

/// A configured repository account (credentials stay encrypted on the row).
#[derive(Debug, Clone)]
pub struct RepositoryAccount {
    pub id: Uuid,
    pub name: String,
    pub provider_type: ProviderType,
    pub auth_type: AuthType,
    pub base_url: Option<String>,
    pub credentials_enc: Option<Vec<u8>>,
    pub selection_mode: SelectionMode,
    pub selection_value: Option<String>,
    pub enabled: bool,
}

/// Fields needed to create or update an account (pre-encrypted credentials).
#[derive(Debug, Clone)]
pub struct AccountInput {
    pub name: String,
    pub provider_type: ProviderType,
    pub auth_type: AuthType,
    pub base_url: Option<String>,
    pub credentials_enc: Option<Vec<u8>>,
    pub selection_mode: SelectionMode,
    pub selection_value: Option<String>,
    pub enabled: bool,
}

/// A repository discovered from a provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RemoteRepo {
    pub name: String,
    pub full_name: String,
    pub clone_url: String,
    pub default_branch: Option<String>,
    pub private: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_type_round_trips() {
        for v in ["github", "gitlab", "local"] {
            assert_eq!(ProviderType::parse(v).unwrap().as_str(), v);
        }
        assert!(ProviderType::parse("svn").is_err());
    }

    #[test]
    fn selection_mode_round_trips() {
        for v in ["all", "regex", "list"] {
            assert_eq!(SelectionMode::parse(v).unwrap().as_str(), v);
        }
    }

    #[test]
    fn auth_type_round_trips() {
        for v in ["token", "app", "none"] {
            assert_eq!(AuthType::parse(v).unwrap().as_str(), v);
        }
    }
}
