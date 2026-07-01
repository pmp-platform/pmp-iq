//! Turns a repository checkout into a structured [`AnalysisResult`] using an AI
//! provider, with manifest files gathered as context.

use super::analysis::{AnalysisConfig, AnalysisResult};
use crate::ai::{AiProvider, AiRequest};
use crate::files::safe_join;
use crate::fs::FileSystem;
use async_trait::async_trait;
use std::sync::Arc;

const MAX_FILE_BYTES: usize = 8000;
const MAX_RETRIES: u32 = 1;

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
    "Makefile",
    ".gitlab-ci.yml",
    ".github/workflows",
    "CODEOWNERS",
    "README.md",
];

/// Input for one analysis (bundled to bound parameters).
pub struct AnalysisInput<'a> {
    pub checkout_path: String,
    pub repo_full_name: String,
    pub provider: &'a dyn AiProvider,
    /// User-configured allowed kinds + properties to inject into the prompt.
    pub config: AnalysisConfig,
    /// User-provided per-entity hints (authoritative corrections) for the
    /// application this repository maps to; empty on a first sync.
    pub hints: Vec<crate::hints::EntityHint>,
    /// When set (M41 incremental), only the listed files changed — re-extract
    /// only the components/use cases that involve them; everything else is left
    /// to the existing model (the writer merges via `write_partial`).
    pub changed_files: Option<Vec<String>>,
}

/// Compose the analyzer system prompt from the configured per-section templates
/// (M34), always injecting the strict kinds/properties vocabulary and schema.
fn build_system_prompt(config: &AnalysisConfig) -> String {
    crate::platform::prompts::compose_system_prompt(&config.prompts, config)
}

/// The incremental focus instruction (M41) appended to the user prompt: re-extract
/// only the components/use cases touching the changed files, leaving the rest of
/// the model untouched (the writer merges the partial result).
fn incremental_focus(changed: &[String]) -> String {
    let list = changed.join(", ");
    format!(
        "\n\nINCREMENTAL UPDATE: only these files changed since the last analysis: [{list}]. \
         Return ONLY the 'application', 'components', 'use_cases' and 'endpoints' that involve these \
         files (with their full sub-fields). Leave every other top-level array empty — they are \
         preserved from the existing model and must not be re-derived here."
    )
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

    /// Drop component/use-case file paths that don't resolve to a real file in
    /// the checkout, so the UI never links to a missing file (the model may emit
    /// repository-relative paths it didn't verify).
    fn retain_existing_files(&self, result: &mut AnalysisResult, checkout_path: &str) {
        for component in &mut result.components {
            component.files.retain(|f| self.file_exists(checkout_path, f));
        }
        for use_case in &mut result.use_cases {
            use_case.files.retain(|f| self.file_exists(checkout_path, f));
        }
        for endpoint in &mut result.endpoints {
            endpoint.files.retain(|f| self.file_exists(checkout_path, f));
        }
    }

    /// Whether a repository-relative path resolves to a real file in the checkout.
    /// Must be a regular file, not a directory: the model sometimes emits module
    /// directories as component "files", and the viewer can only read files.
    fn file_exists(&self, checkout_path: &str, rel: &str) -> bool {
        match safe_join(checkout_path, rel) {
            Ok(path) => self.fs.is_file(&path),
            Err(_) => false,
        }
    }
}

#[async_trait]
impl RepositoryAnalyzer for FileAnalyzer {
    async fn analyze(&self, input: AnalysisInput<'_>) -> Result<AnalysisResult, AnalysisError> {
        let context = self.gather_context(&input.checkout_path);
        let mut prompt = Self::build_prompt(&input.repo_full_name, &context);
        prompt.push_str(&crate::hints::render_hints(&input.hints));
        if let Some(changed) = &input.changed_files {
            prompt.push_str(&incremental_focus(changed));
        }
        let system = build_system_prompt(&input.config);

        let mut last_error = String::new();
        for attempt in 0..=MAX_RETRIES {
            // Run agentic providers (the Claude CLI) inside the checkout so they
            // can read real files and cite real paths; the HTTP API ignores it.
            let request = AiRequest::new(prompt.clone())
                .with_system(system.clone())
                .with_working_dir(input.checkout_path.clone());
            let response = input
                .provider
                .complete(request)
                .await
                .map_err(|e| AnalysisError::Ai(e.to_string()))?;
            match AnalysisResult::parse(&response.text) {
                Ok(mut result) => {
                    self.retain_existing_files(&mut result, &input.checkout_path);
                    return Ok(result);
                }
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

    #[test]
    fn system_prompt_includes_configured_kinds_and_properties() {
        use crate::platform::analysis::{KindDef, PropertyDef};
        let kind = |id: &str| KindDef { kind_id: id.into(), name: id.into(), description: String::new() };
        let mut cfg = AnalysisConfig::default();
        cfg.kinds.insert("services".into(), vec![kind("payments"), kind("other")]);
        cfg.kinds.insert(
            "applications".into(),
            vec![KindDef { kind_id: "api".into(), name: "API".into(), description: "REST service".into() }],
        );
        cfg.properties.insert(
            "applications".into(),
            vec![PropertyDef {
                prop_id: "framework".into(),
                name: "Framework".into(),
                description: String::new(),
                data_type: "string".into(),
            }],
        );
        let prompt = build_system_prompt(&cfg);
        assert!(prompt.contains("services kind: [payments, other]"), "{prompt}");
        assert!(prompt.contains("application app_type: [api (REST service)]"), "{prompt}");
        assert!(prompt.contains("framework (string)"), "{prompt}");
    }

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
            config: AnalysisConfig::default(),
            hints: vec![],
            changed_files: None,
        };
        let result = analyzer.analyze(input).await.unwrap();
        assert_eq!(result.application.name, "api");
    }

    #[tokio::test]
    async fn drops_nonexistent_files_and_runs_in_checkout() {
        let mut fs = MockFileSystem::new();
        // gather_context probes manifests; none present here.
        fs.expect_read_to_string().returning(|_| Ok(None));
        // Only paths ending in "real.rs" resolve to a real file in the checkout.
        fs.expect_is_file().returning(|p: &str| p.ends_with("real.rs"));
        let analyzer = FileAnalyzer::new(Arc::new(fs));

        let mut provider = MockAiProvider::new();
        provider
            .expect_complete()
            // The agentic provider must run inside the checkout.
            .withf(|req| req.working_dir.as_deref() == Some("/repo"))
            .returning(|_| {
                Ok(AiResponse {
                    text: r#"{"application":{"name":"api","app_type":"api"},
                        "components":[{"name":"C","files":["src/real.rs","src/ghost.rs"]}],
                        "use_cases":[{"name":"U","files":["src/real.rs","missing.rs"]}]}"#
                        .into(),
                    input_tokens: None,
                    output_tokens: None,
                })
            });
        let input = AnalysisInput {
            checkout_path: "/repo".into(),
            repo_full_name: "org/api".into(),
            provider: &provider,
            config: AnalysisConfig::default(),
            hints: vec![],
            changed_files: None,
        };
        let result = analyzer.analyze(input).await.unwrap();
        assert_eq!(result.components[0].files, vec!["src/real.rs".to_string()]);
        assert_eq!(result.use_cases[0].files, vec!["src/real.rs".to_string()]);
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
            config: AnalysisConfig::default(),
            hints: vec![],
            changed_files: None,
        };
        assert!(matches!(analyzer.analyze(input).await, Err(AnalysisError::Invalid(_))));
    }
}
