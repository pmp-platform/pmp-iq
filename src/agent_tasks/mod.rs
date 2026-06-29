//! AI Agent change tasks: each task is a multi-turn session with an agentic AI
//! over an application's repository. The agent edits files on a dedicated
//! branch, commits, pushes, and opens a pull request (M22).

pub mod job;
pub mod model;
pub mod repository;

pub use job::{AgentTaskDeps, AgentTaskJob, JOB_TYPE, ensure_job};
pub use model::{AgentTask, AgentTaskMessage, NewAgentTask, NewMessage};
pub use repository::{AgentTaskRepository, PgAgentTaskRepository, SqliteAgentTaskRepository};
