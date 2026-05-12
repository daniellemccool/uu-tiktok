use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::errors::TranscribeError;
use crate::process::{run, CommandSpec};

#[derive(Debug, Clone)]
pub struct TranscribeOptions {
    pub model_path: PathBuf,
    pub use_gpu: bool,
    pub threads: usize,
    pub timeout: Duration,
}

#[derive(Debug, Clone)]
pub struct TranscribeResult {
    pub text: String,
    pub language: Option<String>,
    pub duration_s: Option<f64>,
}

/// Run whisper.cpp on the given WAV. Returns the transcript text plus
/// whatever metadata whisper.cpp reports (language detected, duration).
pub async fn transcribe(
    audio_path: &Path,
    opts: &TranscribeOptions,
) -> Result<TranscribeResult, TranscribeError> {
    let mut args: Vec<String> = vec![
        "-m".into(),
        opts.model_path.to_string_lossy().into_owned(),
        "-f".into(),
        audio_path.to_string_lossy().into_owned(),
        "-otxt".into(),
        "-of".into(),
        // Tell whisper.cpp to write the output text alongside the audio,
        // using the audio's stem as the prefix. We then read the resulting
        // .txt file. Without -of, whisper.cpp's auto-named output has been
        // an inconsistent target across versions.
        audio_path.with_extension("").to_string_lossy().into_owned(),
        "-t".into(),
        opts.threads.to_string(),
        "--language".into(),
        "auto".into(),
        "--print-progress".into(),
    ];
    if !opts.use_gpu {
        args.push("--no-gpu".into());
    }

    let outcome = run(CommandSpec {
        program: "whisper-cli",
        args,
        timeout: opts.timeout,
        stderr_capture_bytes: 8 * 1024,
        redact_arg_indices: &[],
    })
    .await
    .map_err(|e| match e {
        crate::process::RunError::Timeout { duration, .. } => TranscribeError::Timeout { duration },
        other => TranscribeError::Failed {
            exit_code: -1,
            stderr_excerpt: other.to_string(),
        },
    })?;

    if outcome.exit_code != 0 {
        return Err(TranscribeError::Failed {
            exit_code: outcome.exit_code,
            stderr_excerpt: outcome.stderr_excerpt,
        });
    }

    // whisper.cpp wrote {audio_path-stem}.txt
    let txt_path = audio_path.with_extension("txt");
    let text = std::fs::read_to_string(&txt_path)
        .map_err(|e| TranscribeError::Failed {
            exit_code: 0,
            stderr_excerpt: format!("reading {}: {}", txt_path.display(), e),
        })?
        .trim()
        .to_string();

    if text.is_empty() {
        return Err(TranscribeError::EmptyOutput);
    }

    // whisper-cli prints "auto-detected language: en (p = ...)" to stderr.
    // Cheap parse; on failure we just return None.
    let language = parse_language(&outcome.stderr_excerpt);

    Ok(TranscribeResult {
        text,
        language,
        duration_s: None, // Plan A: we don't extract duration; Plan B can add via ffprobe.
    })
}

fn parse_language(stderr: &str) -> Option<String> {
    // Look for "auto-detected language: <code>"
    for line in stderr.lines() {
        if let Some(idx) = line.find("auto-detected language:") {
            let rest = &line[idx + "auto-detected language:".len()..];
            let code = rest
                .split_whitespace()
                .next()
                .unwrap_or("")
                .trim_end_matches(|c: char| !c.is_ascii_alphabetic());
            if !code.is_empty() {
                return Some(code.to_string());
            }
        }
    }
    None
}

// ============================================================================
// Plan B Epic 1: TranscribeOutput types
// ============================================================================
//
// Pass-through raw signals from whisper.cpp's C API via the whisper-rs binding.
// See AD0010 (raw_signals schema), AD0016 (worker-thread invariants).
//
// These types are OWNED data: no references, no whisper-rs handles. They cross
// the worker-thread boundary safely (AD0016 #1: owned data only).

use serde::{Deserialize, Serialize};

/// Owned output from a single whisper inference. Crosses the worker-thread
/// boundary (AD0016). T10's artifact writer maps these fields across the
/// artifact JSON: `text` and `model_id` land at the top level (alongside
/// Plan A's existing metadata), while `language`, `lang_probs`, and `segments`
/// are placed inside the `raw_signals` sub-object (AD0010). This struct is
/// the worker-return type, not a 1:1 mirror of `raw_signals`.
// AD0002: fields unused until T9+; suppress dead-code lint until then.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TranscribeOutput {
    /// Concatenated text of all segments.
    pub text: String,
    /// Detected language as a single ISO code, e.g. "en" or "nl".
    /// From whisper_full_lang_id() (free per inference).
    pub language: String,
    /// Per-language probability vector, ONLY when PerCallConfig::compute_lang_probs is true.
    /// Costs one extra encoder pass per video (sharp-edges.md:13).
    pub lang_probs: Option<Vec<(String, f32)>>,
    /// Per-segment raw confidence signals.
    pub segments: Vec<SegmentRaw>,
    /// Model identifier, e.g. "ggml-large-v3-turbo-q5_0.bin".
    /// Already captured by Plan A's metadata.
    pub model_id: String,
}

/// Per-segment raw confidence signals from whisper.cpp.
// AD0002: unused until T9+.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SegmentRaw {
    /// whisper_full_get_segment_no_speech_prob(state, i)
    pub no_speech_prob: f32,
    /// Per-token confidence signals for this segment.
    pub tokens: Vec<TokenRaw>,
}

/// Per-token confidence signals from whisper.cpp.
// AD0002: unused until T9+.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TokenRaw {
    /// whisper_full_get_token_p(state, i, j) — token probability in [0.0, 1.0]
    pub p: f32,
    /// Token log-probability (TokenData::plog from whisper-rs)
    pub plog: f32,
}

#[cfg(test)]
mod plan_b_tests {
    use super::*;

    fn sample_output() -> TranscribeOutput {
        TranscribeOutput {
            text: "Hello world".to_string(),
            language: "en".to_string(),
            lang_probs: None,
            segments: vec![SegmentRaw {
                no_speech_prob: 0.02,
                tokens: vec![
                    TokenRaw {
                        p: 0.99,
                        plog: -0.01,
                    },
                    TokenRaw {
                        p: 0.95,
                        plog: -0.05,
                    },
                ],
            }],
            model_id: "ggml-tiny.en.bin".to_string(),
        }
    }

    #[test]
    fn transcribe_output_round_trip() {
        let before = sample_output();
        let json = serde_json::to_string(&before).expect("serialize");
        let after: TranscribeOutput = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(before, after);
    }

    #[test]
    fn lang_probs_none_serializes_as_null() {
        let output = sample_output();
        let json = serde_json::to_value(&output).expect("serialize");
        assert_eq!(json["lang_probs"], serde_json::Value::Null);
    }

    #[test]
    fn lang_probs_some_serializes_as_array_of_pairs() {
        let mut output = sample_output();
        output.lang_probs = Some(vec![("en".to_string(), 0.93), ("nl".to_string(), 0.05)]);
        let json = serde_json::to_value(&output).expect("serialize");
        let arr = json["lang_probs"].as_array().expect("array");
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0][0], "en");
        assert!((arr[0][1].as_f64().unwrap() - 0.93).abs() < 1e-6);
    }

    #[test]
    fn empty_segments_round_trip() {
        let mut output = sample_output();
        output.segments = vec![];
        let json = serde_json::to_string(&output).expect("serialize");
        let after: TranscribeOutput = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(output, after);
    }
}

// ============================================================================
// Plan B Epic 1: WhisperEngine shell (T5)
// ============================================================================
//
// Worker-thread architecture per AD0016:
// - Only owned data crosses the boundary (samples, configs, output structs)
// - WhisperContext/WhisperState stay inside the worker thread (T6/T7)
// - Closed oneshot reply is Bug-class during normal execution; AD0016 comment-2
//   carves out shutdown (relevant when Epic 2 wires shutdown signaling).
//
// Per-request cancellation per AD0012 (+ comment-2 refinement):
// - Each request carries its own Arc<AtomicBool> for operator-initiated cancel
//   (per-request, never shared across requests — AD0012's no-leak invariant).
// - Each request carries its own `deadline: Instant` for per-call timeout.
// - T7's abort_callback polls BOTH inside whisper.cpp's encoder/decoder loop;
//   no separate timer task is spawned (deviates from the T5 brief's tokio::spawn
//   sketch per AD0012 comment-2; see AD0003 deviation disclosure in commit body).

use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::thread;
use std::time::Instant;

use tokio::sync::{mpsc, oneshot};

// AD0002: shell types are unused until T6/T7 wire them in.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub model_path: PathBuf,
    pub gpu_device: i32,
    pub flash_attn: bool,
}

// AD0002: shell types are unused until T6/T7 wire them in.
#[allow(dead_code)]
#[derive(Debug, Clone, Default)]
pub struct PerCallConfig {
    /// Some("en") to pin; None for auto-detect.
    pub language: Option<String>,
    /// If true, an extra encoder pass populates TranscribeOutput::lang_probs.
    /// See sharp-edges.md:13 — calling lang_detect re-encodes the audio.
    pub compute_lang_probs: bool,
}

// AD0002: shell type — fields read inside the worker once T7 lands.
#[allow(dead_code)]
#[derive(Debug)]
pub(crate) struct TranscribeRequest {
    pub samples: Vec<f32>,
    pub config: PerCallConfig,
    /// Per-request cancel flag (AD0012). Operator-initiated cancellation flips
    /// this; T7's abort_callback polls it. Never shared across requests.
    pub cancel: Arc<AtomicBool>,
    /// Per-call deadline (AD0012 comment-2). T7's abort_callback polls
    /// `Instant::now() >= deadline` directly — no separate timer task.
    pub deadline: Instant,
    pub reply: oneshot::Sender<Result<TranscribeOutput, TranscribeError>>,
}

// AD0002: variants unused until T6 calls them.
#[allow(dead_code)]
#[derive(Debug, thiserror::Error)]
pub enum WhisperInitError {
    #[error("loading whisper model from {path}: {detail}")]
    ModelLoad { path: String, detail: String },

    #[error(
        "backend mismatch: expected GPU but whisper.cpp engaged CPU fallback (sharp-edges.md:61)"
    )]
    BackendMismatch,

    #[error("spawning whisper worker thread: {detail}")]
    WorkerSpawn { detail: String },
}

/// Drop guard that flips the per-request cancel flag when the caller's
/// `transcribe()` future is dropped before the worker replies. Without this,
/// a caller cancelling the future would leave the worker chewing on an
/// orphaned request whose result no one will read. Per AD0012 comment-2,
/// the cancel flag is the operator-initiated cancellation channel; future-drop
/// is a special case of operator-initiated.
struct CancelOnDrop(Arc<AtomicBool>);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Worker-thread-owning engine handle. See AD0016 for the parallelism contract
/// (engine API stays stable across single- and multi-state worker pools).
///
/// Both fields are `Option` so `shutdown()` and `Drop::drop` can share the same
/// teardown sequence: drop the sender FIRST (closes the channel, lets the
/// worker's `blocking_recv` return `None`), THEN join. If the sender were
/// dropped after the join attempt, the worker would park forever in
/// `blocking_recv` and the join would hang. (Brief code had this hazard;
/// AD0003 deviation — see commit body.)
// AD0002: unused until T6/T7 wires the engine into the pipeline.
#[allow(dead_code)]
pub struct WhisperEngine {
    request_tx: Option<mpsc::Sender<TranscribeRequest>>,
    handle: Option<thread::JoinHandle<()>>,
}

#[allow(dead_code)]
impl WhisperEngine {
    pub fn new(_config: &EngineConfig) -> Result<Self, WhisperInitError> {
        // T6 inserts model load + GPU verification here. For T5 this is just
        // the worker-channel + thread skeleton; whisper-rs is not imported yet.
        //
        // Channel capacity 1: each TranscribeRequest carries a Vec<f32> of decoded
        // audio (~MB scale for a single-minute video). Epic 1's serial pipeline
        // never needs more than one request in flight. Epic 2's pipelined
        // orchestrator decides its own outer queue depth.
        let (request_tx, mut request_rx) = mpsc::channel::<TranscribeRequest>(1);

        let handle = thread::Builder::new()
            .name("uu-tiktok-whisper-worker".to_string())
            .spawn(move || {
                while let Some(req) = request_rx.blocking_recv() {
                    // T7 inserts whisper_full_with_state + raw-signal extraction here.
                    // T5 placeholder: every request replies with a Bug error so the
                    // channel shape can be exercised end-to-end before T6/T7 land.
                    let _ = req.reply.send(Err(TranscribeError::Bug {
                        detail: "WhisperEngine not yet implemented (T5 shell)".to_string(),
                    }));
                }
                // Sender dropped → channel closed → orderly exit. Per AD0016
                // comment-2, this is the shutdown-carve-out path (not Bug).
            })
            .map_err(|e| WhisperInitError::WorkerSpawn {
                detail: format!("spawn whisper worker thread: {e}"),
            })?;

        Ok(Self {
            request_tx: Some(request_tx),
            handle: Some(handle),
        })
    }

    pub async fn transcribe(
        &self,
        samples: Vec<f32>,
        config: PerCallConfig,
        timeout: Duration,
    ) -> Result<TranscribeOutput, TranscribeError> {
        let cancel = Arc::new(AtomicBool::new(false));
        let deadline = Instant::now() + timeout;
        let (reply_tx, reply_rx) = oneshot::channel();

        // CancelOnDrop fires `cancel = true` if this future is dropped before
        // the worker replies (caller-initiated future cancellation). The worker
        // owns its own Arc clone via the request and polls it in T7's
        // abort_callback. Post-reply firing is a no-op (worker has already moved on).
        let _cancel_guard = CancelOnDrop(Arc::clone(&cancel));

        let req = TranscribeRequest {
            samples,
            config,
            cancel,
            deadline,
            reply: reply_tx,
        };

        // No tokio::spawn timer here: T7's abort_callback polls deadline + cancel
        // directly inside whisper.cpp's encoder/decoder loop. AD0012 comment-2.

        let tx = self
            .request_tx
            .as_ref()
            .ok_or_else(|| TranscribeError::Bug {
                detail: "engine already shut down (request_tx taken)".to_string(),
            })?;

        tx.send(req).await.map_err(|_| TranscribeError::Bug {
            detail: "worker thread channel closed (engine shut down mid-flight)".to_string(),
        })?;

        reply_rx.await.unwrap_or_else(|_| {
            Err(TranscribeError::Bug {
                detail: "worker dropped reply oneshot (worker panicked or restarted)".to_string(),
            })
        })
    }

    /// Drop the sender (closing the channel and letting the worker exit), then
    /// join the worker thread. Idempotent with `Drop::drop`.
    pub fn shutdown(mut self) {
        self.teardown();
    }

    fn teardown(&mut self) {
        // Order matters: closing the channel must happen BEFORE the join, or
        // the worker stays parked in blocking_recv and the join hangs forever.
        drop(self.request_tx.take());
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for WhisperEngine {
    fn drop(&mut self) {
        self.teardown();
    }
}

#[cfg(test)]
mod engine_tests {
    use super::*;

    fn dummy_config() -> EngineConfig {
        EngineConfig {
            model_path: PathBuf::from("/dev/null"),
            gpu_device: 0,
            flash_attn: false,
        }
    }

    #[tokio::test]
    async fn shell_returns_bug_error_on_transcribe() {
        let engine = WhisperEngine::new(&dummy_config()).expect("shell construction succeeds");
        let result = engine
            .transcribe(
                vec![0.0_f32; 16000],
                PerCallConfig::default(),
                Duration::from_secs(5),
            )
            .await;
        assert!(matches!(result, Err(TranscribeError::Bug { .. })));
        engine.shutdown();
    }

    // Renamed from the brief's `shell_send_closed_after_shutdown`: the body
    // checks clean shutdown timing, not closed-send semantics. T7 will cover
    // the closed-oneshot Bug path more thoroughly once real inference lands.
    #[tokio::test]
    async fn shutdown_joins_cleanly() {
        let engine = WhisperEngine::new(&dummy_config()).expect("shell construction succeeds");
        let start = Instant::now();
        engine.shutdown();
        // If teardown ordering were wrong (join before sender-drop) this would
        // hang until the test harness timeout. Guard with a generous bound so
        // a regression surfaces as a clear assertion rather than a 60s timeout.
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_secs(2),
            "shutdown took {elapsed:?}; expected sub-second teardown — possible deadlock"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_language_extracts_code_from_whisper_stderr() {
        let stderr = "
whisper_init_from_file_with_params_no_state: loading model from './models/ggml-tiny.en.bin'
auto-detected language: en (p = 0.99)
done
";
        assert_eq!(parse_language(stderr), Some("en".to_string()));
    }

    #[test]
    fn parse_language_returns_none_when_absent() {
        let stderr = "no language line here\n";
        assert_eq!(parse_language(stderr), None);
    }
}
