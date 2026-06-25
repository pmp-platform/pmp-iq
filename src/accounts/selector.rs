//! Applies an account's selection mode to a provider's repository list.

use super::model::{RemoteRepo, SelectionMode};
use regex::Regex;

/// Error building/applying a selection.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SelectionError {
    #[error("invalid regex: {0}")]
    Regex(String),
    #[error("invalid explicit list: {0}")]
    List(String),
}

/// Selects a subset of repositories according to the configured mode.
pub struct RepoSelector;

impl RepoSelector {
    /// Filter `repos` by `mode`/`value`. `value` is the regex pattern (for
    /// `Regex`) or a JSON array of full names (for `List`); ignored for `All`.
    pub fn select(
        mode: SelectionMode,
        value: Option<&str>,
        repos: Vec<RemoteRepo>,
    ) -> Result<Vec<RemoteRepo>, SelectionError> {
        match mode {
            SelectionMode::All => Ok(repos),
            SelectionMode::Regex => Self::select_regex(value.unwrap_or(""), repos),
            SelectionMode::List => Self::select_list(value.unwrap_or("[]"), repos),
        }
    }

    fn select_regex(
        pattern: &str,
        repos: Vec<RemoteRepo>,
    ) -> Result<Vec<RemoteRepo>, SelectionError> {
        let re = Regex::new(pattern).map_err(|e| SelectionError::Regex(e.to_string()))?;
        Ok(repos
            .into_iter()
            .filter(|r| re.is_match(&r.full_name) || re.is_match(&r.name))
            .collect())
    }

    fn select_list(json: &str, repos: Vec<RemoteRepo>) -> Result<Vec<RemoteRepo>, SelectionError> {
        let wanted: Vec<String> =
            serde_json::from_str(json).map_err(|e| SelectionError::List(e.to_string()))?;
        Ok(repos
            .into_iter()
            .filter(|r| wanted.iter().any(|w| w == &r.full_name || w == &r.name))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo(full: &str) -> RemoteRepo {
        let name = full.split('/').next_back().unwrap().to_string();
        RemoteRepo {
            name,
            full_name: full.to_string(),
            clone_url: format!("https://example.com/{full}.git"),
            default_branch: Some("main".into()),
            private: false,
        }
    }

    fn sample() -> Vec<RemoteRepo> {
        vec![repo("org/api"), repo("org/web"), repo("org/api-gateway")]
    }

    #[test]
    fn all_returns_everything() {
        let out = RepoSelector::select(SelectionMode::All, None, sample()).unwrap();
        assert_eq!(out.len(), 3);
    }

    #[test]
    fn regex_filters_by_pattern() {
        let out = RepoSelector::select(SelectionMode::Regex, Some("^org/api"), sample()).unwrap();
        let names: Vec<_> = out.iter().map(|r| r.full_name.as_str()).collect();
        assert_eq!(names, vec!["org/api", "org/api-gateway"]);
    }

    #[test]
    fn invalid_regex_errors() {
        let err = RepoSelector::select(SelectionMode::Regex, Some("("), sample()).unwrap_err();
        assert!(matches!(err, SelectionError::Regex(_)));
    }

    #[test]
    fn list_selects_explicit_names() {
        let out = RepoSelector::select(
            SelectionMode::List,
            Some(r#"["org/web"]"#),
            sample(),
        )
        .unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].full_name, "org/web");
    }

    #[test]
    fn invalid_list_json_errors() {
        let err = RepoSelector::select(SelectionMode::List, Some("not json"), sample()).unwrap_err();
        assert!(matches!(err, SelectionError::List(_)));
    }
}
