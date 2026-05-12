use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use anyhow::{Context, Result};
use chrono::Utc;

use crate::errors::TranscribeError;
use crate::fetcher::{Acquisition, VideoFetcher};
use crate::output::artifacts::TranscriptMetadata;
use crate::output::{artifacts, shard};
use crate::state::{Claim, Store, SuccessArtifacts};
use crate::transcribe::TranscribeResult;

/// Test-injectable transcriber. Returns a boxed Future so the pipeline can
/// `.await` it from within the async runtime — calling a blocking
/// `tokio::runtime::Handle::block_on` from within an async context panics with
/// "Cannot start a runtime from within a runtime." Tests pass a closure that
/// returns `Box::pin(async { ... })`; production wires this to
/// `transcribe::transcribe(...)` in `src/main.rs`.
// Nested `Box<dyn Fn(...) -> Pin<Box<dyn Future<...> + Send>> + Send + Sync>`
// trips clippy::type_complexity. The shape is the standard Rust idiom for an
// injectable async callback, so suppress here rather than restructuring.
#[allow(clippy::type_complexity)]
pub type Transcriber = Box<
    dyn Fn(
            &std::path::Path,
        )
            -> Pin<Box<dyn Future<Output = Result<TranscribeResult, TranscribeError>> + Send>>
        + Send
        + Sync,
>;

pub struct ProcessOptions {
    pub worker_id: String,
    pub transcripts_root: PathBuf,
    pub max_videos: Option<usize>,
    /// Identifier of the whisper.cpp model in use (e.g., the model file's
    /// basename like "ggml-small.bin"). Threaded into each transcript's
    /// metadata sidecar for provenance. Computed once at process startup
    /// from the configured model path; no per-video cost.
    pub transcript_model: String,
    pub transcriber: Transcriber,
}

#[derive(Debug, Default)]
pub struct ProcessStats {
    pub claimed: usize,
    pub succeeded: usize,
    pub failed: usize,
}

// T10 lifted the (formerly private, borrowed-string) `TranscriptMetadata`
// struct to `src/output/artifacts.rs` with owned `String` fields and the new
// optional `raw_signals` field per AD0010. The construction site below
// clones strings to satisfy the owned shape — T11 will rewrite this entire
// block when wiring the Plan B engine, so the extra allocations are
// transient and not worth optimizing here.

// `stats.failed += 1` is followed immediately by `return Err(e)` in Plan A's
// fail-fast behavior, so the increment is dead under -D warnings. Plan B will
// drop the early return (persist failure and continue), at which point the
// increment becomes load-bearing. Keeping the bookkeeping in place now means
// Plan B's diff is a one-line removal rather than a re-introduction.
#[allow(unused_assignments)]
pub async fn run_serial(
    store: &mut Store,
    fetcher: &dyn VideoFetcher,
    opts: ProcessOptions,
) -> Result<ProcessStats> {
    let mut stats = ProcessStats::default();
    let max = opts.max_videos.unwrap_or(usize::MAX);

    while stats.claimed + stats.failed < max {
        let claim = match store.claim_next(&opts.worker_id)? {
            Some(c) => c,
            None => break,
        };
        stats.claimed += 1;

        match process_one(store, fetcher, &claim, &opts).await {
            Ok(()) => stats.succeeded += 1,
            Err(e) => {
                stats.failed += 1;
                tracing::error!(
                    video_id = claim.video_id.as_str(),
                    error = %e,
                    "video failed (Plan A: aborting; Plan B will classify and persist)"
                );
                // Plan A behavior: leave the row in_progress; operator inspects.
                // Plan B will persist failure and continue.
                return Err(e);
            }
        }
    }

    Ok(stats)
}

async fn process_one(
    store: &mut Store,
    fetcher: &dyn VideoFetcher,
    claim: &Claim,
    opts: &ProcessOptions,
) -> Result<()> {
    tracing::info!(
        video_id = claim.video_id.as_str(),
        attempt = claim.attempt_count,
        "claimed"
    );

    let acquisition = fetcher
        .acquire(&claim.video_id, &claim.source_url)
        .await
        .with_context(|| format!("fetching {}", claim.video_id))?;

    // Plan A's `Acquisition` has only one variant; Plan B will add `Unavailable`
    // and `ReadyTranscript`, at which point the `match` becomes load-bearing.
    // Keeping it now means Plan B's diff is additive arms, not a syntax flip.
    #[allow(clippy::infallible_destructuring_match)]
    let wav_path = match acquisition {
        Acquisition::AudioFile(p) => p,
    };
    tracing::info!(video_id = claim.video_id.as_str(), wav = %wav_path.display(), "audio acquired");

    let transcript = (opts.transcriber)(&wav_path)
        .await
        .with_context(|| format!("transcribing {}", claim.video_id))?;
    tracing::info!(
        video_id = claim.video_id.as_str(),
        chars = transcript.text.len(),
        language = transcript.language.as_deref().unwrap_or("?"),
        "transcribed"
    );

    let shard_dir = opts.transcripts_root.join(shard(&claim.video_id));
    std::fs::create_dir_all(&shard_dir)
        .with_context(|| format!("creating shard dir {}", shard_dir.display()))?;

    let txt_path = shard_dir.join(format!("{}.txt", claim.video_id));
    artifacts::atomic_write(&txt_path, transcript.text.as_bytes())
        .with_context(|| format!("writing transcript {}", txt_path.display()))?;

    let metadata = TranscriptMetadata {
        video_id: claim.video_id.clone(),
        source_url: claim.source_url.clone(),
        duration_s: transcript.duration_s,
        language_detected: transcript.language.clone(),
        transcribed_at: Utc::now().to_rfc3339(),
        fetcher: "ytdlp".to_string(),
        transcript_source: "whisper.cpp".to_string(),
        model: opts.transcript_model.clone(),
        // T11 will populate this from the Plan B engine's TranscribeOutput
        // via `RawSignals::from_transcribe_output`. Plan A's TranscribeResult
        // (whisper-cli) does not carry raw signals — None here serializes to
        // an absent field on the wire (`skip_serializing_if`).
        raw_signals: None,
    };
    let json_bytes =
        serde_json::to_vec_pretty(&metadata).context("serializing transcript metadata")?;
    let json_path = shard_dir.join(format!("{}.json", claim.video_id));
    artifacts::atomic_write(&json_path, &json_bytes)?;

    // Cleanup the wav file once durably committed.
    if let Err(e) = std::fs::remove_file(&wav_path) {
        tracing::warn!(path = %wav_path.display(), error = %e, "could not remove wav after success");
    }

    store.mark_succeeded(
        &claim.video_id,
        SuccessArtifacts {
            duration_s: transcript.duration_s,
            language_detected: transcript.language,
            fetcher: "ytdlp",
            transcript_source: "whisper.cpp",
        },
    )?;

    tracing::info!(video_id = claim.video_id.as_str(), "succeeded");
    Ok(())
}
