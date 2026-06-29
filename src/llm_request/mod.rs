//! The `llm-repository-request` job: clone (or update) a repository, then run an
//! LLM session with a user-supplied input against the checkout, serialised by a
//! per-repository distributed lock.

pub mod job;

pub use job::{
    JOB_TYPE, LlmRepositoryRequestJob, LlmRequestDeps, ensure_job,
};
