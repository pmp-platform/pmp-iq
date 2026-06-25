//! Turns a repository checkout into a structured [`AnalysisResult`] using an AI
//! provider, with manifest files gathered as context.

use super::analysis::AnalysisResult;
use crate::ai::{AiProvider, AiRequest};
use crate::fs::FileSystem;
use async_trait::async_trait;
use std::sync::Arc;

const MAX_FILE_BYTES: usize = 8000;
const MAX_RETRIES: u32 = 1;

const SYSTEM_PROMPT: &str = "You are a software platform analyst. Given files from a repository, \
extract structured metadata. Respond with ONLY a JSON object (no prose, no markdown fences) of the \
shape: {\"application\":{\"name\":string,\"app_type\":string,\"description\":string,\
\"primary_language\":string,\"metadata\":object},\"languages\":[{\"name\":string,\"percentage\":number}],\
\"libraries\":[{\"name\":string,\"ecosystem\":string,\"version\":string,\"scope\":string}],\
\"infrastructure\":[{\"name\":string,\"kind\":string,\"version\":string,\"usage\":string}],\
\"dependencies\":[{\"target_name\":string,\"kind\":string,\"description\":string}],\
\"users\":[{\"username\":string,\"email\":string,\"groups\":[string]}],\
\"groups\":[{\"name\":string}],\
\"access\":[{\"principal_type\":\"user\"|\"group\",\"principal_name\":string,\"access_level\":string}]}. \
Use empty arrays when unknown. app_type is one of api, frontend, mobile, cli, library, service.";

/// Files to look for when building analysis context.
const SIGNAL_FILES: &[&str] = &[
    "Cargo.toml",
    "package.json",
    "requirements.txt",
    "pyproject.toml",
    "go.mod",
    "pom.xml",
    "build.gradle",
    "Gemfile",
    "composer.json",
    "docker-compose.yml",
    "docker-compose.yaml",
    "Dockerfile",
    "CODEOWNERS",
    "README.md",
];

/// Input for one analysis (bundled to bound parameters).
pub struct AnalysisInput<'a> {
    pub checkout_path: String,
    pub repo_full_name: String,
    pub provider: &'a dyn AiProvider,
}

/// Errors from analysis.
#[derive(Debug, thiserror::Error)]
pub enum AnalysisError {
    #[error("ai error: {0}")]
    Ai(String),
    #[error("could not produce valid analysis: {0}")]
    Invalid(String),
}

/// Analyses a repository checkout.
#[async_trait]
pub trait RepositoryAnalyzer: Send + Sync {
    async fn analyze(&self, input: AnalysisInput<'_>) -> Result<AnalysisResult, AnalysisError>;
}

/// Filesystem + AI backed analyzer.
pub struct FileAnalyzer {
    fs: Arc<dyn FileSystem>,
}

impl FileAnalyzer {
    pub fn new(fs: Arc<dyn FileSystem>) -> Self {
        Self { fs }
    }

    /// Read known manifest files under the checkout into a context string.
    fn gather_context(&self, checkout_path: &str) -> String {
        let mut context = String::new();
        for file in SIGNAL_FILES {
            let path = format!("{}/{}", checkout_path.trim_end_matches('/'), file);
            if let Ok(Some(content)) = self.fs.read_to_string(&path) {
                let truncated: String = content.chars().take(MAX_FILE_BYTES).collect();
                context.push_str(&format!("\n===== {file} =====\n{truncated}\n"));
            }
        }
        context
    }

    fn build_prompt(repo_full_name: &str, context: &str) -> String {
        if context.trim().is_empty() {
            format!(
                "Repository '{repo_full_name}' has no recognised manifest files. \
                 Infer what you can from the name and return the JSON schema."
            )
        } else {
            format!("Repository '{repo_full_name}'. Files:\n{context}")
        }
    }
}

#[async_trait]
impl RepositoryAnalyzer for FileAnalyzer {
    async fn analyze(&self, input: AnalysisInput<'_>) -> Result<AnalysisResult, AnalysisError> {
        let context = self.gather_context(&input.checkout_path);
        let prompt = Self::build_prompt(&input.repo_full_name, &context);

        let mut last_error = String::new();
        for attempt in 0..=MAX_RETRIES {
            let request = AiRequest::new(prompt.clone()).with_system(SYSTEM_PROMPT);
            let response = input
                .provider
                .complete(request)
                .await
                .map_err(|e| AnalysisError::Ai(e.to_string()))?;
            match AnalysisResult::parse(&response.text) {
                Ok(result) => return Ok(result),
                Err(message) => {
                    last_error = message;
                    tracing::warn!(attempt, error = %last_error, "analysis parse failed");
                }
            }
        }
        Err(AnalysisError::Invalid(last_error))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::provider::MockAiProvider;
    use crate::ai::AiResponse;
    use crate::fs::MockFileSystem;

    fn analyzer_with_manifest(content: &'static str) -> FileAnalyzer {
        let mut fs = MockFileSystem::new();
        fs.expect_read_to_string().returning(move |path: &str| {
            if path.ends_with("Cargo.toml") {
                Ok(Some(content.to_string()))
            } else {
                Ok(None)
            }
        });
        FileAnalyzer::new(Arc::new(fs))
    }

    #[tokio::test]
    async fn analyzes_with_valid_response() {
        let analyzer = analyzer_with_manifest("[package]\nname=\"api\"");
        let mut provider = MockAiProvider::new();
        provider.expect_complete().returning(|_| {
            Ok(AiResponse {
                text: r#"{"application":{"name":"api","app_type":"api"},"languages":[{"name":"Rust"}]}"#.into(),
                input_tokens: None,
                output_tokens: None,
            })
        });
        let input = AnalysisInput {
            checkout_path: "/repo".into(),
            repo_full_name: "org/api".into(),
            provider: &provider,
        };
        let result = analyzer.analyze(input).await.unwrap();
        assert_eq!(result.application.name, "api");
    }

    #[tokio::test]
    async fn retries_then_fails_on_invalid_json() {
        let analyzer = analyzer_with_manifest("x");
        let mut provider = MockAiProvider::new();
        // Two attempts (initial + one retry), both invalid.
        provider
            .expect_complete()
            .times(2)
            .returning(|_| Ok(AiResponse { text: "nonsense".into(), input_tokens: None, output_tokens: None }));
        let input = AnalysisInput {
            checkout_path: "/repo".into(),
            repo_full_name: "org/api".into(),
            provider: &provider,
        };
        assert!(matches!(analyzer.analyze(input).await, Err(AnalysisError::Invalid(_))));
    }
}
