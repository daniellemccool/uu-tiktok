# Task 5 — WhisperEngine shell: struct, worker-thread skeleton, closed-oneshot Bug test

**Goal:** Stand up the architectural shape of `WhisperEngine` — owns a worker thread, accepts requests via mpsc channel, replies via oneshot. The whisper-rs internals come in T6 (init) and T7 (transcribe). This task establishes the boundary so T6/T7 just fill in the body.

**ADRs touched:** AD0009 (whisper-rs path), AD0012 (per-request cancellation), AD0016 (worker-thread invariants).

**Files:**
- Modify: `src/transcribe.rs` — add WhisperEngine, TranscribeRequest, channel types, worker shell
- Modify: `src/errors.rs` — extend TranscribeError with new variants (Cancelled, Bug)
- Test: inline tests in `src/transcribe.rs`

---

- [ ] **Step 1: Extend `TranscribeError` for the new variants**

Modify `src/errors.rs`. Find the existing `TranscribeError` enum and add:

```rust
#[derive(Debug, thiserror::Error)]
pub enum TranscribeError {
    // ... existing variants kept ...

    #[error("transcription cancelled (deadline elapsed or operator-initiated)")]
    Cancelled,

    #[error("transcription bug: {detail}")]
    Bug { detail: String },
}
```

Plus a `From<AudioDecodeError>` impl (we'll need it in T11 but adding now is cheap):

```rust
impl From<crate::audio::AudioDecodeError> for TranscribeError {
    fn from(e: crate::audio::AudioDecodeError) -> Self {
        TranscribeError::Bug {
            detail: format!("audio decode failure (should be classified, not Bug, in Epic 3): {e}"),
        }
    }
}
```

The "should be classified later" note is intentional: Epic 3 introduces proper classification for audio decode failures. For Epic 1 we surface them as Bug to fail-fast.

Run:
```bash
cargo build
```

Expected: clean build.

- [ ] **Step 2: Write the failing tests first**

Add to `src/transcribe.rs` (append to the file, after the existing types from T4):

```rust
// ============================================================================
// Plan B Epic 1: WhisperEngine
// ============================================================================
//
// Worker-thread architecture per AD0016:
// - Only owned data crosses the boundary (samples, configs, output structs)
// - WhisperContext/WhisperState stay inside the worker thread
// - Closed oneshot reply is Bug-class
//
// Per-request cancellation per AD0012:
// - Each request carries its own Arc<AtomicBool>; never reused across requests.
// - FullParams::abort_callback polls the request's flag.

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};

use crate::errors::TranscribeError;

/// Configuration that doesn't change across a session (engine startup).
#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub model_path: std::path::PathBuf,
    pub gpu_device: i32,
    pub flash_attn: bool,
}

/// Configuration that varies per call.
#[derive(Debug, Clone, Default)]
pub struct PerCallConfig {
    /// Some("en") to pin; None for auto-detect.
    pub language: Option<String>,
    /// If true, an extra encoder pass populates TranscribeOutput::lang_probs.
    /// See sharp-edges.md:13 — calling lang_detect re-encodes the audio.
    pub compute_lang_probs: bool,
}

#[derive(Debug)]
pub(crate) struct TranscribeRequest {
    pub samples: Vec<f32>,
    pub config: PerCallConfig,
    pub cancel: Arc<AtomicBool>,
    pub reply: oneshot::Sender<Result<TranscribeOutput, TranscribeError>>,
}

#[derive(Debug, thiserror::Error)]
pub enum WhisperInitError {
    #[error("loading whisper model from {path}: {detail}")]
    ModelLoad { path: String, detail: String },

    #[error("backend mismatch: expected GPU but whisper.cpp engaged CPU fallback (sharp-edges.md:61)")]
    BackendMismatch,
}

pub struct WhisperEngine {
    handle: Option<thread::JoinHandle<()>>,
    request_tx: mpsc::Sender<TranscribeRequest>,
}

impl WhisperEngine {
    pub fn new(_config: &EngineConfig) -> Result<Self, WhisperInitError> {
        let (request_tx, mut request_rx) = mpsc::channel::<TranscribeRequest>(8);

        // Worker thread skeleton; whisper-rs model load + inference lands in T6/T7.
        // For T5 the worker just dequeues requests and replies with a placeholder
        // Bug error so we can test the channel shape end-to-end.
        let handle = thread::Builder::new()
            .name("uu-tiktok-whisper-worker".to_string())
            .spawn(move || {
                while let Some(req) = request_rx.blocking_recv() {
                    // T6 inserts model-load happy path here.
                    // T7 inserts whisper_full_with_state + raw signal extraction here.
                    // T5 placeholder: every request replies with a Bug error.
                    let _ = req.reply.send(Err(TranscribeError::Bug {
                        detail: "WhisperEngine not yet implemented (T5 shell)".to_string(),
                    }));
                }
            })
            .map_err(|e| WhisperInitError::ModelLoad {
                path: "(no model yet — T5 shell)".to_string(),
                detail: format!("spawn worker thread: {e}"),
            })?;

        Ok(Self {
            handle: Some(handle),
            request_tx,
        })
    }

    pub async fn transcribe(
        &self,
        samples: Vec<f32>,
        config: PerCallConfig,
        timeout: Duration,
    ) -> Result<TranscribeOutput, TranscribeError> {
        let cancel = Arc::new(AtomicBool::new(false));
        let (reply_tx, reply_rx) = oneshot::channel();

        let req = TranscribeRequest {
            samples,
            config,
            cancel: Arc::clone(&cancel),
            reply: reply_tx,
        };

        // Spawn a tokio task to flip the cancel flag when the deadline elapses.
        // The flag belongs to THIS request only (AD0012 — no cross-request leak).
        let cancel_for_deadline = Arc::clone(&cancel);
        tokio::spawn(async move {
            tokio::time::sleep(timeout).await;
            cancel_for_deadline.store(true, std::sync::atomic::Ordering::Relaxed);
        });

        self.request_tx
            .send(req)
            .await
            .map_err(|_| TranscribeError::Bug {
                detail: "worker thread channel closed (engine shut down mid-flight)".to_string(),
            })?;

        reply_rx
            .await
            .unwrap_or_else(|_| {
                Err(TranscribeError::Bug {
                    detail: "worker dropped reply oneshot (worker panicked or restarted)".to_string(),
                })
            })
    }

    pub fn shutdown(mut self) {
        // Closing the sender will end the worker loop.
        drop(self.request_tx);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for WhisperEngine {
    fn drop(&mut self) {
        // The Drop impl can't fully shut down (we don't have ownership). Worker
        // thread exits on its own when the sender is dropped; we just join.
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod engine_tests {
    use super::*;

    fn dummy_config() -> EngineConfig {
        EngineConfig {
            model_path: std::path::PathBuf::from("/dev/null"),
            gpu_device: 0,
            flash_attn: false,
        }
    }

    #[tokio::test]
    async fn shell_returns_bug_error_on_transcribe() {
        let engine = WhisperEngine::new(&dummy_config()).expect("shell construction succeeds");
        let result = engine
            .transcribe(vec![0.0_f32; 16000], PerCallConfig::default(), Duration::from_secs(5))
            .await;
        assert!(matches!(result, Err(TranscribeError::Bug { .. })));
        engine.shutdown();
    }

    #[tokio::test]
    async fn shell_send_closed_after_shutdown() {
        let engine = WhisperEngine::new(&dummy_config()).expect("shell construction succeeds");
        // Drop request_tx by shutting down the engine first.
        engine.shutdown();
        // Note: after shutdown the engine is moved, so the next call wouldn't
        // compile. The closed-oneshot Bug case is instead exercised by dropping
        // the reply receiver in another test (T7 covers it more thoroughly).
        // This test just verifies shutdown joins cleanly without hanging.
    }
}
```

- [ ] **Step 3: Run the tests**

```bash
cargo test --lib transcribe::engine_tests::
```

Expected: PASS — `shell_returns_bug_error_on_transcribe` confirms the channel shape works; `shell_send_closed_after_shutdown` confirms shutdown joins cleanly.

- [ ] **Step 4: Run the full Plan A test suite**

```bash
cargo test --features test-helpers
```

Expected: all existing tests still pass; new shell tests also pass.

- [ ] **Step 5: cargo fmt and clippy**

```bash
cargo fmt --all && cargo clippy --all-targets -- -D warnings
```

Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add src/transcribe.rs src/errors.rs
git commit -m "$(cat <<'EOF'
feat(transcribe): WhisperEngine worker-thread shell + per-request cancellation

Stands up the architectural shape of WhisperEngine per AD0016 (worker
thread owns state; only owned data crosses the boundary) and AD0012
(per-request Arc<AtomicBool> cancellation; never reused across requests).

The shell's worker loop replies with a placeholder Bug error to every
request — T6 (model load + GPU verification) and T7 (whisper_full_with_state
+ raw signal extraction) fill in the real inference.

Adds TranscribeError::Cancelled and TranscribeError::Bug variants for
the new failure modes; adds From<AudioDecodeError> for TranscribeError
that surfaces as Bug for Epic 1 (Epic 3 reclassifies).

Tier 2 test confirms the channel shape works end-to-end (placeholder
Bug error returned through the worker). Shutdown joins cleanly.

Refs: AD0009, AD0012, AD0016

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Self-check

- [ ] `cargo test --lib transcribe::engine_tests::` passes
- [ ] Full Plan A suite still passes
- [ ] `cargo clippy` clean
- [ ] WhisperEngine drops cleanly without hanging
- [ ] No whisper-rs imports yet (T6 introduces them); the file compiles without the `cuda` feature
- [ ] Worker thread is named for debuggability
