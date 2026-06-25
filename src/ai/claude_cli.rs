//! Claude CLI binary provider (invokes the local `claude` executable).

use super::model::{AiRequest, AiResponse};
use super::provider::{AiError, AiProvider};
use crate::process::{CommandRunner, CommandSpec};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;

fn default_binary() -> String {
    "claude".to_string()
}

/// Typed configuration for the Claude CLI provider.
#[derive(Debug, Clone, Deserialize)]
pub struct ClaudeCliConfig {
    #[serde(default = "default_binary")]
    pub binary_path: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
    #[serde(default)]
    pub extra_args: Vec<String>,
}

/// Runs the `claude` binary through the injected command runner.
pub struct ClaudeCliProvider {
    runner: Arc<dyn CommandRunner>,
    config: ClaudeCliConfig,
}

impl ClaudeCliProvider {
    pub fn new(runner: Arc<dyn CommandRunner>, config: ClaudeCliConfig) -> Self {
        Self { runner, config }
    }

    fn build_args(&self, request: &AiRequest) -> Vec<String> {
        let mut args = vec!["-p".to_string(), "--output-format".to_string(), "json".to_string()];
        if let Some(model) = &self.config.model {
            args.push("--model".to_string());
            args.push(model.clone());
        }
        if let Some(system) = &request.system {
            args.push("--append-system-prompt".to_string());
            args.push(system.clone());
        }
        args.extend(self.config.extra_args.clone());
        args.push(request.prompt.clone());
        args
    }

    /// Parse the CLI output: prefer the JSON `result` field, fall back to raw.
    fn parse_output(stdout: &str) -> AiResponse {
        if let Ok(value) = serde_json::from_str::<Value>(stdout) {
            if let Some(result) = value["result"].as_str() {
                return AiResponse {
                    text: result.to_string(),
                    input_tokens: value["usage"]["input_tokens"].as_u64().map(|v| v as u32),
                    output_tokens: value["usage"]["output_tokens"].as_u64().map(|v| v as u32),
                };
            }
        }
        AiResponse {
            text: stdout.trim().to_string(),
            input_tokens: None,
            output_tokens: None,
        }
    }
}

#[async_trait]
impl AiProvider for ClaudeCliProvider {
    async fn complete(&self, request: AiRequest) -> Result<AiResponse, AiError> {
        let spec = CommandSpec {
            program: self.config.binary_path.clone(),
            args: self.build_args(&request),
            stdin: None,
        };
        let output = self
            .runner
            .run(spec)
            .await
            .map_err(|e| AiError::Request(e.to_string()))?;
        if !output.success() {
            return Err(AiError::Request(format!(
                "claude exited with {}: {}",
                output.status, output.stderr
            )));
        }
        Ok(Self::parse_output(&output.stdout))
    }

    async fn validate(&self) -> Result<(), AiError> {
        let spec = CommandSpec {
            program: self.config.binary_path.clone(),
            args: vec!["--version".to_string()],
            stdin: None,
        };
        let output = self
            .runner
            .run(spec)
            .await
            .map_err(|e| AiError::Config(e.to_string()))?;
        if output.success() {
            Ok(())
        } else {
            Err(AiError::Config(format!("claude --version failed: {}", output.stderr)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::{CommandOutput, MockCommandRunner};

    fn config() -> ClaudeCliConfig {
        ClaudeCliConfig {
            binary_path: "claude".into(),
            model: Some("claude-opus-4-8".into()),
            effort: None,
            extra_args: vec![],
        }
    }

    #[tokio::test]
    async fn parses_json_result() {
        let mut runner = MockCommandRunner::new();
        runner.expect_run().returning(|spec| {
            assert_eq!(spec.program, "claude");
            assert!(spec.args.contains(&"--output-format".to_string()));
            Ok(CommandOutput {
                status: 0,
                stdout: r#"{"result":"done","usage":{"input_tokens":5,"output_tokens":2}}"#.into(),
                stderr: String::new(),
            })
        });
        let provider = ClaudeCliProvider::new(Arc::new(runner), config());
        let out = provider.complete(AiRequest::new("hi")).await.unwrap();
        assert_eq!(out.text, "done");
        assert_eq!(out.input_tokens, Some(5));
    }

    #[tokio::test]
    async fn nonzero_exit_is_request_error() {
        let mut runner = MockCommandRunner::new();
        runner.expect_run().returning(|_| {
            Ok(CommandOutput { status: 1, stdout: String::new(), stderr: "boom".into() })
        });
        let provider = ClaudeCliProvider::new(Arc::new(runner), config());
        assert!(matches!(provider.complete(AiRequest::new("x")).await, Err(AiError::Request(_))));
    }

    #[test]
    fn falls_back_to_raw_text() {
        let out = ClaudeCliProvider::parse_output("plain text\n");
        assert_eq!(out.text, "plain text");
        assert!(out.input_tokens.is_none());
    }
}
