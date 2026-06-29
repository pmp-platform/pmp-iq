//! Natural-language query over the whole platform catalog (M26). Serialises the
//! platform connection graph (applications, libraries, infrastructure, services
//! and their dependencies/usage edges) as grounding data and answers the user's
//! question with the LLM — citing entities, and refusing to invent data not in
//! the catalog.

use crate::ai::{AiProfileService, AiRequest};
use crate::error::AppError;
use crate::platform::graph::{GraphQuery, GraphScope};
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;

/// Cap on the grounding-data size so the prompt stays bounded.
const MAX_DATA_CHARS: usize = 60_000;
/// Cap on graph nodes pulled for the snapshot.
const GRAPH_LIMIT: i64 = 800;

const SYSTEM: &str = "You answer questions about a software platform using ONLY the provided \
    catalog graph. Cite the relevant node names (applications, libraries, infrastructure, …). If \
    the catalog does not contain the answer, say so explicitly — never invent applications, \
    dependencies, or infrastructure.";

/// Dependencies for the catalog query service.
#[derive(Clone)]
pub struct CatalogQueryDeps {
    pub graph: Arc<dyn GraphQuery>,
    pub ai: AiProfileService,
}

/// Answers natural-language questions grounded in the platform catalog.
pub struct CatalogQuery {
    deps: CatalogQueryDeps,
}

impl CatalogQuery {
    pub fn new(deps: CatalogQueryDeps) -> Self {
        Self { deps }
    }

    async fn default_profile(&self) -> Option<Uuid> {
        let profiles = self.deps.ai.list().await.ok()?;
        profiles.iter().find(|p| p.enabled).or_else(|| profiles.first()).map(|p| p.id)
    }

    /// Answer `question` grounded in the catalog graph. Returns `{ "answer": … }`.
    pub async fn answer(&self, question: &str) -> Result<Value, AppError> {
        let graph = self.deps.graph.build(&GraphScope::new(None, Some(GRAPH_LIMIT))).await?;
        let profile_id = self
            .default_profile()
            .await
            .ok_or_else(|| AppError::BadRequest("no AI agent profile configured".into()))?;
        let profile = self.deps.ai.get(profile_id).await?;

        let data = truncate(&serde_json::to_string(&graph).unwrap_or_default(), MAX_DATA_CHARS);
        let prompt = format!(
            "Platform catalog as a connection graph (JSON). Nodes are applications, libraries, \
             infrastructure, services, etc.; edges are dependencies/usage:\n\n{data}\n\n\
             Question: {question}"
        );
        let response = self
            .deps
            .ai
            .complete(&profile, AiRequest::new(prompt).with_system(SYSTEM.to_string()))
            .await?;
        Ok(json!({ "answer": response.text }))
    }
}

/// Truncate `s` to at most `max` chars, marking truncation.
fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n…(catalog truncated)", &s[..end])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::AiProviderDeps;
    use crate::ai::model::{AiProfile, AiProviderType};
    use crate::ai::repository::MockAiProfileRepository;
    use crate::crypto::MockEncryptor;
    use crate::db::RepoResult;
    use crate::httpclient::MockHttpClient;
    use crate::process::{CommandOutput, MockCommandRunner};
    use async_trait::async_trait;

    struct FakeGraph(Value);

    #[async_trait]
    impl GraphQuery for FakeGraph {
        async fn build(&self, _scope: &GraphScope) -> RepoResult<Value> {
            Ok(self.0.clone())
        }
    }

    fn cli_ai(stdout: &'static str) -> AiProfileService {
        let mut repo = MockAiProfileRepository::new();
        repo.expect_list().returning(|| {
            Ok(vec![AiProfile {
                id: Uuid::new_v4(),
                name: "cli".into(),
                provider_type: AiProviderType::ClaudeCli,
                config: json!({ "binary_path": "claude" }),
                secrets_enc: None,
                enabled: true,
            }])
        });
        repo.expect_get().returning(|id| {
            Ok(AiProfile {
                id,
                name: "cli".into(),
                provider_type: AiProviderType::ClaudeCli,
                config: json!({ "binary_path": "claude" }),
                secrets_enc: None,
                enabled: true,
            })
        });
        let mut runner = MockCommandRunner::new();
        runner.expect_run().returning(move |_| {
            Ok(CommandOutput { status: 0, stdout: stdout.into(), stderr: String::new() })
        });
        AiProfileService::new(
            Arc::new(repo),
            AiProviderDeps {
                http: Arc::new(MockHttpClient::new()),
                runner: Arc::new(runner),
                encryptor: Arc::new(MockEncryptor::new()),
            },
        )
    }

    #[test]
    fn truncate_marks_oversize() {
        assert_eq!(truncate("hello", 10), "hello");
        assert!(truncate(&"x".repeat(100), 10).contains("truncated"));
    }

    #[tokio::test]
    async fn answers_grounded_in_the_graph() {
        let graph = json!({ "nodes": [{ "id": "app:1", "data": { "label": "api", "kind": "application" } }], "edges": [] });
        let cq = CatalogQuery::new(CatalogQueryDeps {
            graph: Arc::new(FakeGraph(graph)),
            ai: cli_ai(r#"{"result":"The platform has one application: api."}"#),
        });
        let out = cq.answer("which applications exist?").await.unwrap();
        assert_eq!(out["answer"], "The platform has one application: api.");
    }

    #[tokio::test]
    async fn errors_when_no_profile() {
        let mut repo = MockAiProfileRepository::new();
        repo.expect_list().returning(|| Ok(vec![]));
        let ai = AiProfileService::new(
            Arc::new(repo),
            AiProviderDeps {
                http: Arc::new(MockHttpClient::new()),
                runner: Arc::new(MockCommandRunner::new()),
                encryptor: Arc::new(MockEncryptor::new()),
            },
        );
        let cq = CatalogQuery::new(CatalogQueryDeps {
            graph: Arc::new(FakeGraph(json!({ "nodes": [], "edges": [] }))),
            ai,
        });
        assert!(cq.answer("anything?").await.is_err());
    }
}
