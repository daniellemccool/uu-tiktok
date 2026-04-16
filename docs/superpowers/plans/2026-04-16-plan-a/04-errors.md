# Plan A — Task 4: Errors module (minimal types)

> Part of [Plan A: walking skeleton](./00-overview.md). See the overview for file structure, dependency set, conventions, and exit criteria.

**Files:**
- Create: `src/errors.rs`
- Modify: `src/main.rs`

Plan A only needs `FetchError` and `TranscribeError` to propagate; classification (RetryableKind / UnavailableReason / ClassifiedFailure) lands in Plan B. Define the bare minimum so trait signatures lock in.

- [ ] **Step 1: Write the failing test**

Create `src/errors.rs`:

```rust
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
```

- [ ] **Step 2: Run tests to confirm pass**

Add `mod errors;` to `src/main.rs` (next to other `mod` declarations).

Run: `cargo test errors:: 2>&1 | tail -10`
Expected: `2 passed; 0 failed`.

- [ ] **Step 3: Commit**

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings 2>&1 | tail -5
git add src/errors.rs src/main.rs
git commit -m "Plan A T4: minimal FetchError and TranscribeError types"
```
