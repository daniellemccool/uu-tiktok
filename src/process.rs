use std::time::Duration;

use thiserror::Error;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::time::timeout;

use crate::errors::FetchError;

// Dead code allowed: CommandSpec, CommandOutcome, RunError, run, and the From impl
// are consumed by T11 (YtDlpFetcher) and T12 (transcribe step).
#[allow(dead_code)]
#[derive(Debug)]
pub struct CommandSpec<'a> {
    pub program: &'static str,
    pub args: Vec<String>,
    pub timeout: Duration,
    /// Last-N bytes of stderr to retain. Avoids OOM on chatty tools.
    pub stderr_capture_bytes: usize,
    /// Argument indices to redact in the structured log (e.g., cookie file paths).
    pub redact_arg_indices: &'a [usize],
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct CommandOutcome {
    pub exit_code: i32,
    pub stdout: Vec<u8>,
    pub stderr_excerpt: String,
    pub elapsed: Duration,
}

#[allow(dead_code)]
#[derive(Debug, Error)]
pub enum RunError {
    #[error("failed to spawn `{tool}`: {source}")]
    Spawn {
        tool: &'static str,
        #[source]
        source: std::io::Error,
    },

    #[error("subprocess `{tool}` timed out after {duration:?}")]
    Timeout {
        tool: &'static str,
        duration: Duration,
    },

    #[error("io error reading subprocess output for `{tool}`: {source}")]
    Io {
        tool: &'static str,
        #[source]
        source: std::io::Error,
    },
}

// Dead code allowed: consumed by T11 (YtDlpFetcher) and T12 (transcribe step).
#[allow(dead_code)]
impl From<RunError> for FetchError {
    fn from(err: RunError) -> Self {
        match err {
            RunError::Timeout { tool, duration } => FetchError::ToolTimeout { tool, duration },
            RunError::Spawn { tool, source } => {
                FetchError::NetworkError(format!("failed to spawn {}: {}", tool, source))
            }
            RunError::Io { tool, source } => {
                FetchError::NetworkError(format!("io error reading {} output: {}", tool, source))
            }
        }
    }
}

// Dead code allowed: `run` and `ring_buffer_tail` are consumed by T11 (YtDlpFetcher)
// and T12 (transcribe step).
#[allow(dead_code)]
#[tracing::instrument(level = "debug", skip(spec), fields(tool = spec.program))]
pub async fn run(spec: CommandSpec<'_>) -> Result<CommandOutcome, RunError> {
    let started = std::time::Instant::now();

    let logged_args: Vec<String> = spec
        .args
        .iter()
        .enumerate()
        .map(|(i, a)| {
            if spec.redact_arg_indices.contains(&i) {
                "<redacted>".into()
            } else {
                a.clone()
            }
        })
        .collect();
    tracing::debug!(args = ?logged_args, "spawning subprocess");

    let mut child = Command::new(spec.program)
        .args(&spec.args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|source| RunError::Spawn {
            tool: spec.program,
            source,
        })?;

    let mut stdout = child.stdout.take().expect("piped stdout");
    let mut stderr = child.stderr.take().expect("piped stderr");

    let read_outputs = async {
        let mut stdout_buf = Vec::new();
        let mut stderr_buf = Vec::new();
        tokio::try_join!(
            stdout.read_to_end(&mut stdout_buf),
            stderr.read_to_end(&mut stderr_buf),
        )?;
        Ok::<_, std::io::Error>((stdout_buf, stderr_buf))
    };

    let result = timeout(spec.timeout, async {
        let (stdout_buf, stderr_buf) = read_outputs.await.map_err(|source| RunError::Io {
            tool: spec.program,
            source,
        })?;
        let status = child.wait().await.map_err(|source| RunError::Io {
            tool: spec.program,
            source,
        })?;
        Ok::<_, RunError>((stdout_buf, stderr_buf, status))
    })
    .await;

    match result {
        Ok(Ok((stdout_buf, stderr_buf, status))) => {
            let exit_code = status.code().unwrap_or(-1);
            let stderr_excerpt = ring_buffer_tail(&stderr_buf, spec.stderr_capture_bytes);
            let elapsed = started.elapsed();
            Ok(CommandOutcome {
                exit_code,
                stdout: stdout_buf,
                stderr_excerpt,
                elapsed,
            })
        }
        Ok(Err(e)) => Err(e),
        Err(_elapsed) => {
            // Timed out: kill_on_drop will SIGKILL the child when `child` is dropped.
            // Drop here happens at the end of this async block.
            let _ = child.start_kill();
            Err(RunError::Timeout {
                tool: spec.program,
                duration: spec.timeout,
            })
        }
    }
}

#[allow(dead_code)]
fn ring_buffer_tail(buf: &[u8], cap: usize) -> String {
    if cap == 0 || buf.is_empty() {
        return String::new();
    }
    let start = buf.len().saturating_sub(cap);
    String::from_utf8_lossy(&buf[start..]).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn echo_succeeds_with_stdout() {
        let spec = CommandSpec {
            program: "echo",
            args: vec!["hello".into(), "world".into()],
            timeout: Duration::from_secs(5),
            stderr_capture_bytes: 1024,
            redact_arg_indices: &[],
        };
        let outcome = run(spec).await.expect("echo runs");
        assert_eq!(outcome.exit_code, 0);
        assert_eq!(
            String::from_utf8_lossy(&outcome.stdout).trim(),
            "hello world"
        );
    }

    #[tokio::test]
    async fn false_returns_nonzero_exit() {
        let spec = CommandSpec {
            program: "false",
            args: vec![],
            timeout: Duration::from_secs(5),
            stderr_capture_bytes: 1024,
            redact_arg_indices: &[],
        };
        let outcome = run(spec).await.expect("false runs");
        assert_ne!(outcome.exit_code, 0);
    }

    #[tokio::test]
    async fn timeout_kills_long_running_subprocess() {
        let spec = CommandSpec {
            program: "sleep",
            args: vec!["10".into()],
            timeout: Duration::from_millis(200),
            stderr_capture_bytes: 1024,
            redact_arg_indices: &[],
        };
        let result = run(spec).await;
        match result {
            Err(RunError::Timeout { tool, .. }) => assert_eq!(tool, "sleep"),
            other => panic!("expected timeout, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn missing_program_returns_spawn_error() {
        let spec = CommandSpec {
            program: "this-program-does-not-exist-1234567",
            args: vec![],
            timeout: Duration::from_secs(5),
            stderr_capture_bytes: 1024,
            redact_arg_indices: &[],
        };
        let result = run(spec).await;
        match result {
            Err(RunError::Spawn { .. }) => {}
            other => panic!("expected Spawn error, got {:?}", other),
        }
    }
}
