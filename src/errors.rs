use std::time::Duration;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum FetchError {
    #[error("subprocess `{tool}` timed out after {duration:?}")]
    ToolTimeout {
        tool: &'static str,
        duration: Duration,
    },

    #[error("subprocess `{tool}` exited with status {exit_code}: {stderr_excerpt}")]
    ToolFailed {
        tool: &'static str,
        exit_code: i32,
        stderr_excerpt: String,
    },

    #[error("network error during fetch: {0}")]
    NetworkError(String),

    #[error("failed to parse fetcher output: {0}")]
    ParseError(String),
}

#[derive(Debug, Error)]
pub enum TranscribeError {
    #[error("whisper.cpp timed out after {duration:?}")]
    Timeout { duration: Duration },

    #[error("whisper.cpp exited with status {exit_code}: {stderr_excerpt}")]
    Failed {
        exit_code: i32,
        stderr_excerpt: String,
    },

    #[error("whisper.cpp produced no transcript")]
    EmptyOutput,

    // AD0002: Cancelled is constructed by T7's abort_callback path; suppress
    // dead_code until then.
    #[allow(dead_code)]
    #[error("transcription cancelled (deadline elapsed or operator-initiated)")]
    Cancelled,

    #[error("transcription bug: {detail}")]
    Bug { detail: String },
}

impl From<crate::audio::AudioDecodeError> for TranscribeError {
    fn from(e: crate::audio::AudioDecodeError) -> Self {
        TranscribeError::Bug {
            detail: format!("audio decode failure (should be classified, not Bug, in Epic 3): {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fetch_error_displays_with_context() {
        let err = FetchError::ToolTimeout {
            tool: "yt-dlp",
            duration: Duration::from_secs(300),
        };
        let msg = format!("{}", err);
        assert!(msg.contains("yt-dlp"));
        assert!(msg.contains("300"));
    }

    #[test]
    fn transcribe_error_failed_carries_exit_code() {
        let err = TranscribeError::Failed {
            exit_code: 1,
            stderr_excerpt: "out of memory".into(),
        };
        let msg = format!("{}", err);
        assert!(msg.contains("status 1"));
        assert!(msg.contains("out of memory"));
    }
}
