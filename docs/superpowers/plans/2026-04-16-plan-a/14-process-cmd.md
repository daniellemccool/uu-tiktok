# Plan A — Task 14: `process` subcommand (serial loop) + e2e smoke test

> Part of [Plan A: walking skeleton](./00-overview.md). See the overview for file structure, dependency set, conventions, and exit criteria.

**Files:**
- Create: `src/pipeline.rs`
- Modify: `src/main.rs`, `src/lib.rs`
- Test: `tests/pipeline_fakes.rs`, `tests/e2e_real_tools.rs`

- [ ] **Step 1: Write the failing FakeFetcher pipeline test**

Create `tests/pipeline_fakes.rs`:

```rust
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use tempfile::TempDir;
use uu_tiktok::fetcher::FakeFetcher;
use uu_tiktok::pipeline::{run_serial, ProcessOptions};
use uu_tiktok::state::Store;

#[tokio::test]
async fn pipeline_processes_one_video_to_succeeded_with_fake_fetcher() {
    let tmp = TempDir::new().unwrap();
    let mut store = Store::open(&tmp.path().join("state.sqlite")).unwrap();
    store
        .upsert_video("7234567890123456789", "fake://url", true)
        .unwrap();

    // Pre-stage a fake WAV file as the FakeFetcher's canned response.
    let fake_wav = tmp.path().join("fake.wav");
    std::fs::write(&fake_wav, b"RIFF....WAVE....").unwrap();
    let map = HashMap::from([("7234567890123456789".to_string(), fake_wav.clone())]);
    let fetcher = FakeFetcher {
        canned: Mutex::new(map),
    };

    // Inject a fake transcribe function via the test transcriber.
    let opts = ProcessOptions {
        worker_id: "test-worker".into(),
        transcripts_root: tmp.path().join("transcripts"),
        max_videos: Some(1),
        // For Plan A we provide a `fake_transcribe` callback in tests.
        // The real `process` subcommand calls the actual transcribe module.
        transcriber: Box::new(|_path| {
            Ok(uu_tiktok::transcribe::TranscribeResult {
                text: "hello fake world".into(),
                language: Some("en".into()),
                duration_s: None,
            })
        }),
    };

    let stats = run_serial(&mut store, &fetcher, opts).await.expect("pipeline");
    assert_eq!(stats.succeeded, 1);
    assert_eq!(stats.failed, 0);

    let row = store
        .get_video_for_test("7234567890123456789")
        .unwrap()
        .unwrap();
    assert_eq!(row.status, "succeeded");

    // Final artifacts present in the sharded directory.
    let txt = tmp
        .path()
        .join("transcripts/89/7234567890123456789.txt");
    assert!(txt.exists(), "transcript file at {}", txt.display());
    let json = tmp
        .path()
        .join("transcripts/89/7234567890123456789.json");
    assert!(json.exists(), "transcript metadata at {}", json.display());
}
```

- [ ] **Step 2: Implement `pipeline::run_serial`**

Create `src/pipeline.rs`:

```rust
use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::Utc;
use serde::Serialize;

use crate::errors::TranscribeError;
use crate::fetcher::{Acquisition, VideoFetcher};
use crate::output::{artifacts, shard};
use crate::state::{Claim, Store, SuccessArtifacts};
use crate::transcribe::TranscribeResult;

/// Test-injectable transcriber. The real `process` subcommand wires this to
/// `transcribe::transcribe` via the `transcribe` module; tests can supply a
/// closure that returns a fixed result.
pub type Transcriber =
    Box<dyn Fn(&std::path::Path) -> Result<TranscribeResult, TranscribeError> + Send>;

pub struct ProcessOptions {
    pub worker_id: String,
    pub transcripts_root: PathBuf,
    pub max_videos: Option<usize>,
    pub transcriber: Transcriber,
}

#[derive(Debug, Default)]
pub struct ProcessStats {
    pub claimed: usize,
    pub succeeded: usize,
    pub failed: usize,
}

#[derive(Serialize)]
struct TranscriptMetadata<'a> {
    video_id: &'a str,
    source_url: &'a str,
    duration_s: Option<f64>,
    language_detected: Option<&'a str>,
    transcribed_at: String,
    fetcher: &'a str,
    transcript_source: &'a str,
}

pub async fn run_serial(
    store: &mut Store,
    fetcher: &dyn VideoFetcher,
    opts: ProcessOptions,
) -> Result<ProcessStats> {
    let mut stats = ProcessStats::default();
    let max = opts.max_videos.unwrap_or(usize::MAX);
    let processed = 0usize;

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

    let _ = processed;
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

    let wav_path = match acquisition {
        Acquisition::AudioFile(p) => p,
    };
    tracing::info!(video_id = claim.video_id.as_str(), wav = %wav_path.display(), "audio acquired");

    let transcript = (opts.transcriber)(&wav_path)
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
        video_id: &claim.video_id,
        source_url: &claim.source_url,
        duration_s: transcript.duration_s,
        language_detected: transcript.language.as_deref(),
        transcribed_at: Utc::now().to_rfc3339(),
        fetcher: "ytdlp",
        transcript_source: "whisper.cpp",
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
```

- [ ] **Step 3: Wire `pipeline` and the `process` subcommand**

Add `pub mod pipeline;` to `src/lib.rs`. Add `mod pipeline;` to `src/main.rs`.

In `src/main.rs`, replace the `Command::Process` arm:

```rust
        cli::Command::Process { max_videos } => {
            let mut store = state::Store::open(&cfg.state_db)
                .context("opening state DB")?;
            std::fs::create_dir_all(&cfg.transcripts).context("creating transcripts dir")?;
            // Tmp cleanup at startup
            let removed = output::artifacts::cleanup_tmp_files(&cfg.transcripts)?;
            if removed > 0 {
                tracing::info!(removed, "cleaned up leftover .tmp files");
            }

            let work_dir = cfg.transcripts.join(".work");
            std::fs::create_dir_all(&work_dir).context("creating work dir")?;

            let fetcher = fetcher::ytdlp::YtDlpFetcher::new(&work_dir, cfg.ytdlp_timeout);
            let model_path = cfg.whisper_model_path.clone();
            let use_gpu = cfg.whisper_use_gpu;
            let threads = cfg.whisper_threads;
            let timeout = cfg.transcribe_timeout;

            let opts = pipeline::ProcessOptions {
                worker_id: format!(
                    "{}-{}",
                    hostname_or_default(),
                    std::process::id()
                ),
                transcripts_root: cfg.transcripts.clone(),
                max_videos,
                transcriber: Box::new(move |path| {
                    let opts = transcribe::TranscribeOptions {
                        model_path: model_path.clone(),
                        use_gpu,
                        threads,
                        timeout,
                    };
                    // Block on the async transcribe — pipeline is serial in Plan A.
                    tokio::runtime::Handle::current()
                        .block_on(transcribe::transcribe(path, &opts))
                }),
            };

            let stats = pipeline::run_serial(&mut store, &fetcher, opts).await?;
            tracing::info!(
                claimed = stats.claimed,
                succeeded = stats.succeeded,
                failed = stats.failed,
                "process complete"
            );
            if stats.claimed == 0 {
                std::process::exit(3);
            }
        }
```

Add the `hostname_or_default` helper at the bottom of `main.rs`:

```rust
fn hostname_or_default() -> String {
    std::env::var("HOSTNAME").unwrap_or_else(|_| "host".to_string())
}
```

Register `pipeline_fakes` in `Cargo.toml`:

```toml
[[test]]
name = "pipeline_fakes"
required-features = ["test-helpers"]
```

- [ ] **Step 4: Run the FakeFetcher integration test**

Run: `cargo test --features test-helpers --test pipeline_fakes 2>&1 | tail -15`
Expected: 1 passed; 0 failed.

- [ ] **Step 5: Add the real-tools e2e smoke test (`#[ignore]` by default)**

Create `tests/e2e_real_tools.rs`:

```rust
use std::path::PathBuf;
use std::process::Command;

/// Real-network test: requires yt-dlp, ffmpeg, and whisper-cli on PATH, plus
/// the tiny.en model at ./models/ggml-tiny.en.bin. Run manually:
///   cargo test --features test-helpers --test e2e_real_tools -- --ignored --nocapture
#[test]
#[ignore]
fn end_to_end_one_known_url() {
    let tmp = tempfile::TempDir::new().unwrap();
    let inbox = tmp.path().join("inbox");
    std::fs::create_dir_all(&inbox).unwrap();

    // Construct a single-row DDP-extracted JSON file pointing at a known
    // long-lived TikTok URL. Chosen URL must be public and stable; replace if
    // it ever 404s. (Operator-curated; documented in README.)
    let respondent_file = inbox.join(
        "assignment=1_task=1_participant=test_source=tiktok_key=1-tiktok.json",
    );
    let payload = r#"[
        {"tiktok_watch_history": [
            {"Date": "2024-01-01 00:00:00",
             "Link": "https://www.tiktokv.com/share/video/7234567890123456789/"}
        ], "deleted row count": "0"}
    ]"#;
    std::fs::write(&respondent_file, payload).unwrap();

    let bin = PathBuf::from(env!("CARGO_BIN_EXE_uu-tiktok"));

    // Init
    Command::new(&bin)
        .args(["--state-db", tmp.path().join("state.sqlite").to_str().unwrap()])
        .arg("init")
        .status()
        .unwrap();
    // Ingest (note: init isn't implemented yet in T15; this test is wired in T15 and re-run there.)

    // The full e2e validation lands once T15 wires `init`.
}
```

(This test will be revisited in Task 15 once `init` is wired. For now it's a stub that compiles and is `#[ignore]`d.)

- [ ] **Step 6: Run the full test suite to make sure nothing's broken**

Run: `cargo test --features test-helpers 2>&1 | tail -15`
Expected: all non-ignored tests pass.

- [ ] **Step 7: Commit**

```bash
cargo fmt
cargo clippy --all-targets --features test-helpers -- -D warnings 2>&1 | tail -5
git add src/pipeline.rs src/main.rs src/lib.rs Cargo.toml tests/pipeline_fakes.rs tests/e2e_real_tools.rs
git commit -m "Plan A T14: process subcommand serial loop + FakeFetcher pipeline test"
```
