//! Process execution abstraction so command-line integrations can be mocked.

use async_trait::async_trait;

/// Outcome of running a command.
#[derive(Debug, Clone)]
pub struct CommandOutput {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

impl CommandOutput {
    pub fn success(&self) -> bool {
        self.status == 0
    }
}

/// A command to execute (kept as one struct to bound parameter count).
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
    pub stdin: Option<String>,
    /// Extra environment variables for the child process (name, value).
    pub env: Vec<(String, String)>,
    /// Working directory for the child process (defaults to the parent's).
    pub cwd: Option<String>,
}

/// Errors from running a command.
#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error("failed to spawn process: {0}")]
    Spawn(String),
    #[error("process io error: {0}")]
    Io(String),
}

/// Runs external commands.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait CommandRunner: Send + Sync {
    async fn run(&self, spec: CommandSpec) -> Result<CommandOutput, CommandError>;
}

/// Real implementation backed by `tokio::process`.
pub struct TokioCommandRunner;

#[async_trait]
impl CommandRunner for TokioCommandRunner {
    async fn run(&self, spec: CommandSpec) -> Result<CommandOutput, CommandError> {
        use tokio::io::AsyncWriteExt;
        use tokio::process::Command;

        let mut command = Command::new(&spec.program);
        command.args(&spec.args);
        command.envs(spec.env.clone());
        if let Some(cwd) = &spec.cwd {
            command.current_dir(cwd);
        }
        command.stdout(std::process::Stdio::piped());
        command.stderr(std::process::Stdio::piped());
        if spec.stdin.is_some() {
            command.stdin(std::process::Stdio::piped());
        }

        let mut child = command.spawn().map_err(|e| CommandError::Spawn(e.to_string()))?;
        if let Some(input) = spec.stdin {
            if let Some(mut handle) = child.stdin.take() {
                handle
                    .write_all(input.as_bytes())
                    .await
                    .map_err(|e| CommandError::Io(e.to_string()))?;
            }
        }
        let output = child
            .wait_with_output()
            .await
            .map_err(|e| CommandError::Io(e.to_string()))?;
        Ok(CommandOutput {
            status: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_success_checks_status() {
        let ok = CommandOutput { status: 0, stdout: String::new(), stderr: String::new() };
        assert!(ok.success());
        let bad = CommandOutput { status: 1, stdout: String::new(), stderr: String::new() };
        assert!(!bad.success());
    }
}
