//! An [`AiProvider`] decorator that records every LLM exchange to a job
//! execution: the full request and response go to the execution output, and
//! token usage is accumulated into the execution metadata.

use super::log_sink::LogSink;
use super::repository::JobExecutionRepository;
use crate::ai::{AiError, AiProvider, AiRequest, AiResponse};
use crate::cost::{LlmUsageInput, LlmUsageRepository};
use async_trait::async_trait;
use serde_json::json;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

/// Running token totals across the calls made during one job execution.
#[derive(Default, Clone, Copy)]
struct Usage {
    calls: u64,
    input_tokens: u64,
    output_tokens: u64,
}

/// Attributes a recorded call to an application/profile/model so its cost (M39)
/// can be rolled up. Set via [`RecordingAiProvider::with_usage`].
#[derive(Debug, Clone)]
pub struct UsageAttribution {
    pub application_id: Option<Uuid>,
    pub ai_profile_id: Option<Uuid>,
    pub model: String,
}

/// Optional per-call usage sink: where priced usage rows are appended.
struct UsageSink {
    repo: Arc<dyn LlmUsageRepository>,
    attribution: UsageAttribution,
}

/// Wraps an inner provider, mirroring its traffic to a job execution.
pub struct RecordingAiProvider {
    inner: Box<dyn AiProvider>,
    execution_id: Uuid,
    log: Arc<dyn LogSink>,
    executions: Arc<dyn JobExecutionRepository>,
    usage: Mutex<Usage>,
    sink: Option<UsageSink>,
}

impl RecordingAiProvider {
    pub fn new(
        inner: Box<dyn AiProvider>,
        execution_id: Uuid,
        log: Arc<dyn LogSink>,
        executions: Arc<dyn JobExecutionRepository>,
    ) -> Self {
        Self {
            inner,
            execution_id,
            log,
            executions,
            usage: Mutex::new(Usage::default()),
            sink: None,
        }
    }

    /// Also append a priced `llm_usage` row per call, attributed for cost (M39).
    pub fn with_usage(
        mut self,
        repo: Arc<dyn LlmUsageRepository>,
        attribution: UsageAttribution,
    ) -> Self {
        self.sink = Some(UsageSink { repo, attribution });
        self
    }

    /// Append this call's usage row to the cost table, if a sink is configured.
    async fn record_usage(&self, response: &AiResponse) {
        let Some(sink) = &self.sink else { return };
        let row = LlmUsageInput {
            job_execution_id: self.execution_id,
            application_id: sink.attribution.application_id,
            ai_profile_id: sink.attribution.ai_profile_id,
            model: sink.attribution.model.clone(),
            input_tokens: response.input_tokens.unwrap_or(0) as i64,
            output_tokens: response.output_tokens.unwrap_or(0) as i64,
        };
        let _ = sink.repo.record(&row).await;
    }

    fn render_request(request: &AiRequest) -> String {
        let mut block = String::from("\n----- LLM REQUEST -----\n");
        if let Some(system) = &request.system {
            block.push_str(&format!("[system]\n{system}\n"));
        }
        block.push_str(&format!("[prompt]\n{}", request.prompt));
        block
    }

    /// Accumulate this call's tokens and return the cumulative totals.
    fn accumulate(&self, response: &AiResponse) -> Usage {
        let mut usage = self.usage.lock().expect("usage poisoned");
        usage.calls += 1;
        usage.input_tokens += response.input_tokens.unwrap_or(0) as u64;
        usage.output_tokens += response.output_tokens.unwrap_or(0) as u64;
        *usage
    }
}

#[async_trait]
impl AiProvider for RecordingAiProvider {
    async fn complete(&self, request: AiRequest) -> Result<AiResponse, AiError> {
        let _ = self.log.append(self.execution_id, &Self::render_request(&request)).await;
        let response = self.inner.complete(request).await?;
        let _ = self
            .log
            .append(self.execution_id, &format!("----- LLM RESPONSE -----\n{}", response.text))
            .await;

        let totals = self.accumulate(&response);
        let patch = json!({ "llm": {
            "calls": totals.calls,
            "input_tokens": totals.input_tokens,
            "output_tokens": totals.output_tokens,
        }});
        let _ = self.executions.merge_metadata(self.execution_id, &patch).await;
        self.record_usage(&response).await;
        Ok(response)
    }

    async fn validate(&self) -> Result<(), AiError> {
        self.inner.validate().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::provider::MockAiProvider;
    use crate::jobs::log_sink::MockLogSink;
    use crate::jobs::repository::MockJobExecutionRepository;

    #[tokio::test]
    async fn records_io_and_accumulates_tokens() {
        let mut inner = MockAiProvider::new();
        inner.expect_complete().times(2).returning(|_| {
            Ok(AiResponse { text: "answer".into(), input_tokens: Some(3), output_tokens: Some(4) })
        });

        let mut log = MockLogSink::new();
        // Two calls × (request + response) = 4 appends.
        log.expect_append().times(4).returning(|_, _| Ok(()));

        let mut execs = MockJobExecutionRepository::new();
        // The cumulative totals are merged after the second call.
        execs
            .expect_merge_metadata()
            .withf(|_, patch| patch["llm"]["calls"] == 2 && patch["llm"]["input_tokens"] == 6)
            .times(1)
            .returning(|_, _| Ok(()));
        execs
            .expect_merge_metadata()
            .times(1)
            .returning(|_, _| Ok(()));

        let recorder = RecordingAiProvider::new(
            Box::new(inner),
            Uuid::new_v4(),
            Arc::new(log),
            Arc::new(execs),
        );
        recorder.complete(AiRequest::new("q1")).await.unwrap();
        let out = recorder.complete(AiRequest::new("q2")).await.unwrap();
        assert_eq!(out.text, "answer");
    }

    #[tokio::test]
    async fn appends_priced_usage_row_per_call_when_sink_set() {
        let mut inner = MockAiProvider::new();
        inner.expect_complete().times(1).returning(|_| {
            Ok(AiResponse { text: "ok".into(), input_tokens: Some(7), output_tokens: Some(11) })
        });
        let mut log = MockLogSink::new();
        log.expect_append().returning(|_, _| Ok(()));
        let mut execs = MockJobExecutionRepository::new();
        execs.expect_merge_metadata().returning(|_, _| Ok(()));

        let app = Uuid::new_v4();
        let mut usage = crate::cost::repository::MockLlmUsageRepository::new();
        usage
            .expect_record()
            .withf(move |u| {
                u.model == "claude-opus-4"
                    && u.input_tokens == 7
                    && u.output_tokens == 11
                    && u.application_id == Some(app)
            })
            .times(1)
            .returning(|_| Ok(()));

        let recorder = RecordingAiProvider::new(
            Box::new(inner),
            Uuid::new_v4(),
            Arc::new(log),
            Arc::new(execs),
        )
        .with_usage(
            Arc::new(usage),
            UsageAttribution {
                application_id: Some(app),
                ai_profile_id: None,
                model: "claude-opus-4".into(),
            },
        );
        recorder.complete(AiRequest::new("q")).await.unwrap();
    }
}
