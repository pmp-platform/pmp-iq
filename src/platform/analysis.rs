//! The typed schema the AI returns for a repository, plus validation.

use serde::Deserialize;
use serde_json::Value;

/// Top-level analysis result for one repository.
#[derive(Debug, Clone, Deserialize)]
pub struct AnalysisResult {
    pub application: AppInfo,
    #[serde(default)]
    pub languages: Vec<LanguageInfo>,
    #[serde(default)]
    pub libraries: Vec<LibraryInfo>,
    #[serde(default)]
    pub infrastructure: Vec<InfraInfo>,
    #[serde(default)]
    pub dependencies: Vec<DependencyInfo>,
    #[serde(default)]
    pub users: Vec<UserInfo>,
    #[serde(default)]
    pub groups: Vec<GroupInfo>,
    #[serde(default)]
    pub access: Vec<AccessInfo>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AppInfo {
    pub name: String,
    #[serde(default)]
    pub app_type: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub primary_language: Option<String>,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LanguageInfo {
    pub name: String,
    #[serde(default)]
    pub percentage: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LibraryInfo {
    pub name: String,
    pub ecosystem: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct InfraInfo {
    pub name: String,
    pub kind: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub usage: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DependencyInfo {
    pub target_name: String,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UserInfo {
    pub username: String,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub groups: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GroupInfo {
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AccessInfo {
    pub principal_type: String,
    pub principal_name: String,
    pub access_level: String,
}

impl AnalysisResult {
    /// Parse from a (possibly fenced) model response, then validate.
    pub fn parse(text: &str) -> Result<Self, String> {
        let json = extract_json(text);
        let result: AnalysisResult =
            serde_json::from_str(&json).map_err(|e| format!("invalid analysis JSON: {e}"))?;
        result.validate()?;
        Ok(result)
    }

    fn validate(&self) -> Result<(), String> {
        if self.application.name.trim().is_empty() {
            return Err("application.name is required".into());
        }
        for access in &self.access {
            if access.principal_type != "user" && access.principal_type != "group" {
                return Err(format!("invalid principal_type '{}'", access.principal_type));
            }
        }
        Ok(())
    }
}

/// Strip Markdown code fences and isolate the first JSON object.
fn extract_json(text: &str) -> String {
    let trimmed = text.trim();
    let without_fence = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .map(|s| s.trim_start())
        .unwrap_or(trimmed);
    let body = without_fence.strip_suffix("```").unwrap_or(without_fence);
    match (body.find('{'), body.rfind('}')) {
        (Some(start), Some(end)) if end > start => body[start..=end].to_string(),
        _ => body.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fenced_json() {
        let text = "```json\n{\"application\":{\"name\":\"api\"},\"languages\":[{\"name\":\"Rust\"}]}\n```";
        let result = AnalysisResult::parse(text).unwrap();
        assert_eq!(result.application.name, "api");
        assert_eq!(result.languages.len(), 1);
    }

    #[test]
    fn parses_with_surrounding_prose() {
        let text = "Here is the analysis:\n{\"application\":{\"name\":\"web\"}}\nThanks!";
        let result = AnalysisResult::parse(text).unwrap();
        assert_eq!(result.application.name, "web");
    }

    #[test]
    fn rejects_empty_name() {
        let text = "{\"application\":{\"name\":\"\"}}";
        assert!(AnalysisResult::parse(text).is_err());
    }

    #[test]
    fn rejects_invalid_principal_type() {
        let text = r#"{"application":{"name":"a"},"access":[{"principal_type":"robot","principal_name":"x","access_level":"read"}]}"#;
        assert!(AnalysisResult::parse(text).is_err());
    }

    #[test]
    fn invalid_json_errors() {
        assert!(AnalysisResult::parse("not json").is_err());
    }
}
