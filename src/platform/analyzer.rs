//! Turns a repository checkout into a structured [`AnalysisResult`] using an AI
//! provider, with manifest files gathered as context.

use super::analysis::{AnalysisConfig, AnalysisResult};
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
\"libraries\":[{\"name\":string,\"ecosystem\":string,\"version\":string,\"scope\":string,\"metadata\":object}],\
\"infrastructure\":[{\"name\":string,\"kind\":string,\"version\":string,\"usage\":string,\"metadata\":object}],\
\"tools\":[{\"name\":string,\"kind\":string,\"version\":string,\"usage\":string,\"metadata\":object}],\
\"cloud_providers\":[{\"name\":string,\"kind\":string,\"version\":string,\"usage\":string,\"metadata\":object}],\
\"services\":[{\"name\":string,\"kind\":string,\"version\":string,\"usage\":string,\"metadata\":object}],\
\"platforms\":[{\"name\":string,\"kind\":string,\"version\":string,\"usage\":string,\"metadata\":object}],\
\"external\":[{\"name\":string,\"kind\":string,\"version\":string,\"usage\":string,\"metadata\":object}],\
\"dependencies\":[{\"target_name\":string,\"kind\":string,\"description\":string,\"metadata\":object}],\
\"users\":[{\"username\":string,\"email\":string,\"groups\":[string],\"metadata\":object}],\
\"groups\":[{\"name\":string,\"metadata\":object}],\
\"access\":[{\"principal_type\":\"user\"|\"group\",\"principal_name\":string,\"access_level\":string}],\
\"components\":[{\"name\":string,\"kind\":string,\"description\":string,\"metadata\":object,\
\"observability_signals\":[{\"name\":string,\"kind\":string,\"description\":string,\"metadata\":object}]}],\
\"use_cases\":[{\"name\":string,\"description\":string,\"metadata\":object,\"components\":[string],\
\"diagrams\":[{\"name\":string,\"kind\":string,\"description\":string,\"content\":string,\"metadata\":object}]}]}. \
Use empty arrays when unknown. \
Classify each discovered dependency into exactly one array: \
'infrastructure' = self-hosted runtime backing services (database, cache, queue, storage, message broker); \
'tools' = build/orchestration/dev tooling, not runtime (docker compose, gradle, maven, make, npm, terraform, CI like github actions); \
'cloud_providers' = cloud platforms (AWS, GCP, Azure, Cloudflare); \
'services' = third-party or internal network APIs the app calls (Stripe, Twilio, an internal auth-service); \
'platforms' = SaaS for observability/identity/CI/error tracking (Datadog, Auth0, Sentry); \
'external' = any other external dependency that fits none of the above; \
'dependencies' = ONLY other applications in this same codebase that this app depends on, keyed by their repository/app name. \
Never place the same thing in more than one array. \
Populate 'users', 'groups' and 'access' ONLY from a CODEOWNERS file (its code owners and owning teams); \
leave all three as empty arrays when there is no CODEOWNERS file. Repository membership is collected \
separately from the provider, so do not infer members from commits, READMEs or other files. \
'components' are the internal building blocks of THIS application (e.g. controllers, models, services); \
give each a thorough 'description' and list the observability signals (metrics, traces, logs) it emits. \
'use_cases' are the capabilities the application fulfils; give each a thorough 'description', reference \
the involved components by their exact 'name', and include one or more 'diagrams'. Each diagram's \
'content' MUST be valid mermaid source that renders standalone (e.g. starts with 'flowchart TD', \
'sequenceDiagram', 'classDiagram', etc.), and its 'kind' names the mermaid diagram type. Do not wrap \
diagram content in markdown fences.";

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
}

/// The prompt field that carries the kind for an entity type.
fn kind_field_label(entity: &str) -> String {
    match entity {
        "applications" => "application app_type".to_string(),
        "libraries" => "library ecosystem".to_string(),
        other => format!("{other} kind"),
    }
}

/// Render one allowed kind as `id` (or `id (description)` when described).
fn describe_kind(k: &crate::platform::KindDef) -> String {
    if k.description.trim().is_empty() {
        k.kind_id.clone()
    } else {
        format!("{} ({})", k.kind_id, k.description.trim())
    }
}

/// Render one property as `id (type)` (or `id (type, description)`).
fn describe_property(p: &crate::platform::PropertyDef) -> String {
    if p.description.trim().is_empty() {
        format!("{} ({})", p.prop_id, p.data_type)
    } else {
        format!("{} ({}, {})", p.prop_id, p.data_type, p.description.trim())
    }
}

/// Append the configured allowed-kinds and extraction-properties guidance to the
/// base system prompt. Both are strict: the model must use only the listed
/// values/keys.
fn build_system_prompt(config: &AnalysisConfig) -> String {
    let mut prompt = String::from(SYSTEM_PROMPT);
    let kind_sections: Vec<String> = config
        .kinds
        .iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(entity, kinds)| {
            let list = kinds.iter().map(describe_kind).collect::<Vec<_>>().join(", ");
            format!("{}: [{}]", kind_field_label(entity), list)
        })
        .collect();
    if !kind_sections.is_empty() {
        prompt.push_str(
            "\nAllowed kind values per type — output EXACTLY one listed id (the value before any \
             parenthesis) for each item's kind; never invent a value. An item whose kind is not \
             listed will be discarded, so prefer the closest listed id. ",
        );
        prompt.push_str(&kind_sections.join("; "));
        prompt.push('.');
    }
    let prop_sections: Vec<String> = config
        .properties
        .iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(entity, props)| {
            let list = props.iter().map(describe_property).collect::<Vec<_>>().join(", ");
            format!("{entity} — {list}")
        })
        .collect();
    if !prop_sections.is_empty() {
        prompt.push_str(
            "\nPopulate each entity's metadata object ONLY with these keys (when known); do not add \
             any other keys (unlisted keys are discarded): ",
        );
        prompt.push_str(&prop_sections.join("; "));
        prompt.push('.');
    }
    prompt
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
        let system = build_system_prompt(&input.config);

        let mut last_error = String::new();
        for attempt in 0..=MAX_RETRIES {
            let request = AiRequest::new(prompt.clone()).with_system(system.clone());
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
            config: AnalysisConfig::default(),
        };
        assert!(matches!(analyzer.analyze(input).await, Err(AnalysisError::Invalid(_))));
    }
}
