# Task 11 — pipeline.rs: replace whisper-cli subprocess with WhisperEngine

**Goal:** Wire the WhisperEngine from T5–T9 into `src/pipeline.rs::process_one`, replacing the legacy whisper-cli subprocess call. Plan A's serial loop is preserved unchanged. Engine is constructed once at process startup and threaded into `process_one`. AD0008 invariant (artifacts before mark_succeeded) preserved.

**ADRs touched:** AD0008 (artifact-before-mark_succeeded), AD0009 (whisper-rs path), AD0012 (cancellation timeout), AD0014 (audio decode).

**Files:**
- Modify: `src/pipeline.rs` — replace transcribe-cli call with WhisperEngine
- Modify: `src/transcribe.rs` — delete legacy `transcribe()` function + `TranscribeResult` struct
- Modify: `src/main.rs` (or wherever `process` is dispatched) — construct WhisperEngine once at process startup, pass into the loop
- Modify: `tests/pipeline_fakes.rs` — update to use a `WhisperEngine` test double OR feature-gate the path that requires whisper-rs
- Test: extend `tests/pipeline_fakes.rs` with an end-to-end fake test asserting raw_signals lands in the JSON artifact

---

- [ ] **Step 1: Decide on the test-double strategy**

`pipeline::process_one` is called from a serial loop in Plan A's `process` subcommand. Plan A's tests use a `FakeFetcher`; transcribe was a free function we could mock by overriding the path.

Plan B's WhisperEngine wraps a worker thread and whisper-rs context — non-trivial to mock. Two options:

- **Option A (recommended):** Define a `Transcriber` trait that both `WhisperEngine` and a `FakeTranscriber` implement; `process_one` takes `&dyn Transcriber`. Production wires `WhisperEngine`; tests wire `FakeTranscriber`. Promotes T7's owned API to a trait.
- **Option B:** Conditional compilation — use `WhisperEngine` in production, skip the transcribe step entirely in tests when fakes are wired.

Option A is cleaner. Define a minimal `Transcriber` trait in `src/transcribe.rs`:

```rust
#[async_trait::async_trait]
pub trait Transcriber: Send + Sync {
    async fn transcribe(
        &self,
        samples: Vec<f32>,
        config: PerCallConfig,
        timeout: Duration,
    ) -> Result<TranscribeOutput, TranscribeError>;

    fn name(&self) -> &'static str;
}

#[async_trait::async_trait]
impl Transcriber for WhisperEngine {
    async fn transcribe(
        &self,
        samples: Vec<f32>,
        config: PerCallConfig,
        timeout: Duration,
    ) -> Result<TranscribeOutput, TranscribeError> {
        // Delegates to the inherent method
        self.transcribe(samples, config, timeout).await
    }

    fn name(&self) -> &'static str {
        "whisper-rs"
    }
}
```

The `name()` method also resolves FOLLOWUPS T14 (multi-fetcher provenance) for the transcribe side. Plan B records the actual transcriber name in artifact metadata (no more hardcoded "whisper.cpp").

- [ ] **Step 2: Write the failing test in tests/pipeline_fakes.rs**

Add a fake transcriber and extend the pipeline test:

```rust
use std::sync::Arc;
use std::time::Duration;

use uu_tiktok::transcribe::{PerCallConfig, TranscribeOutput, Transcriber, TranscribeError};
use uu_tiktok::transcribe::{SegmentRaw, TokenRaw};

struct FakeTranscriber {
    scripted: TranscribeOutput,
}

#[async_trait::async_trait]
impl Transcriber for FakeTranscriber {
    async fn transcribe(
        &self,
        _samples: Vec<f32>,
        _config: PerCallConfig,
        _timeout: Duration,
    ) -> Result<TranscribeOutput, TranscribeError> {
        Ok(self.scripted.clone())
    }
    fn name(&self) -> &'static str {
        "fake-transcriber"
    }
}

#[tokio::test]
async fn pipeline_writes_raw_signals_to_json_artifact() {
    // Build a fake fetcher that returns a known WAV path
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let wav_path = temp_dir.path().join("fake.wav");
    // Generate or copy in the silence fixture
    std::fs::copy(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/audio/silence_16khz_mono.wav"),
        &wav_path,
    )
    .expect("copy fixture");

    let fake_fetcher = /* construct a FakeFetcher that returns wav_path */;

    let scripted_output = TranscribeOutput {
        text: "hello world".to_string(),
        language: "en".to_string(),
        lang_probs: None,
        segments: vec![SegmentRaw {
            no_speech_prob: 0.02,
            tokens: vec![TokenRaw { p: 0.99, plog: -0.01 }],
        }],
        model_id: "fake-model.bin".to_string(),
    };
    let fake_transcriber = FakeTranscriber { scripted: scripted_output };

    // Run process_one (or the relevant pipeline entry) with both fakes
    /* ... call into pipeline ... */

    // Find the written .json artifact
    let video_id = "7234567890123456789";
    let shard = &video_id[video_id.len() - 2..];
    let json_path = temp_dir.path().join("transcripts").join(shard).join(format!("{video_id}.json"));
    let json_text = std::fs::read_to_string(&json_path).expect("read artifact");
    let parsed: serde_json::Value = serde_json::from_str(&json_text).expect("parse json");

    // Assert raw_signals shape
    let rs = &parsed["raw_signals"];
    assert_eq!(rs["schema_version"], "1");
    assert_eq!(rs["language"], "en");
    assert_eq!(rs["lang_probs"], serde_json::Value::Null);
    let segments = rs["segments"].as_array().expect("segments");
    assert_eq!(segments.len(), 1);
    assert!((segments[0]["no_speech_prob"].as_f64().unwrap() - 0.02).abs() < 1e-6);
    let tokens = segments[0]["tokens"].as_array().expect("tokens");
    assert_eq!(tokens.len(), 1);
    assert!((tokens[0]["p"].as_f64().unwrap() - 0.99).abs() < 1e-6);

    // Assert transcript_source is "fake-transcriber" (no more hardcoded "whisper.cpp")
    assert_eq!(parsed["transcript_source"], "fake-transcriber");
}
```

Run:
```bash
cargo test --features test-helpers --test pipeline_fakes -- pipeline_writes_raw_signals
```

Expected: FAIL — compile errors (Transcriber trait not yet defined) and missing wiring.

- [ ] **Step 3: Wire the pipeline to use Transcriber**

Modify `src/pipeline.rs::process_one`. The current signature looks roughly like:

```rust
pub async fn process_one(claim: Claim, opts: &ProcessOptions, fetcher: &dyn VideoFetcher) -> ...
```

Add a `transcriber: &dyn Transcriber` parameter. Replace the call site that previously invoked `transcribe::transcribe(...)` (the legacy function) with:

```rust
// Decode WAV to PCM samples (T3's helper)
let samples = crate::audio::decode_wav(&wav_path)
    .map_err(|e| /* map to FetchError or TranscribeError appropriately */)?;

let per_call = PerCallConfig {
    language: opts.language.clone(),
    compute_lang_probs: opts.compute_lang_probs,
};

let transcribe_output = transcriber
    .transcribe(samples, per_call, opts.transcribe_timeout)
    .await
    .map_err(|e| /* propagate */)?;

// Build artifact metadata with raw_signals
let metadata = TranscriptMetadata {
    video_id: claim.video_id.clone(),
    source_url: /* claim.source_url */,
    duration_s: /* computed elsewhere */,
    language_detected: Some(transcribe_output.language.clone()),
    transcribed_at: chrono::Utc::now().to_rfc3339(),
    fetcher: fetcher.name().to_string(),
    transcript_source: transcriber.name().to_string(),
    model: transcribe_output.model_id.clone(),
    raw_signals: Some(transcribe_output.to_raw_signals()),
};

// AD0008: write artifacts first, then mark_succeeded
output::artifacts::write_atomic(&opts.transcripts_root, &claim.video_id, &transcribe_output.text, &metadata)?;

store.mark_succeeded(&claim.video_id, /* success artifacts ... */)?;
```

Note: this requires `VideoFetcher` to gain a `name()` method too. Add it for symmetry with `Transcriber::name()` — also resolves FOLLOWUPS T14 fetcher side.

- [ ] **Step 4: Delete the legacy transcribe()**

In `src/transcribe.rs`, remove:
- The legacy `pub async fn transcribe(...)` function
- The legacy `TranscribeResult` struct
- Any imports they require that aren't otherwise used (e.g., `crate::process::CommandSpec` if no other call site uses it)

Per AD0002 cleanup-on-consumption discipline, also scan for `#[allow(dead_code)]` annotations that are now consumed by the new pipeline path and remove them. Use `rg "allow\(dead_code\)" src/` to find candidates.

- [ ] **Step 5: Wire WhisperEngine construction in main.rs / process subcommand dispatcher**

The `process` subcommand currently calls `pipeline::process_one` in a loop. Add engine construction at the top:

```rust
let engine_config = EngineConfig {
    model_path: config.whisper_model_path.clone(),
    gpu_device: 0,
    flash_attn: cfg!(feature = "cuda"),  // flash_attn on with cuda, off without
};
let engine = WhisperEngine::new(&engine_config).context("constructing WhisperEngine")?;

// Existing loop:
loop {
    let claim = match store.claim_next(...)? { ... };
    let result = pipeline::process_one(claim, &opts, &*fetcher, &engine).await;
    // ... existing handling ...
}

engine.shutdown();
```

- [ ] **Step 6: Run the pipeline_fakes test**

```bash
cargo test --features test-helpers --test pipeline_fakes -- pipeline_writes_raw_signals
```

Expected: PASS.

- [ ] **Step 7: Run the full Plan A test suite**

```bash
cargo test --features test-helpers
```

Expected: all existing tests pass; the new integration test also passes; the legacy `transcribe()` tests (if any) are removed alongside the function.

- [ ] **Step 8: cargo fmt and clippy**

```bash
cargo fmt --all && cargo clippy --all-targets -- -D warnings
```

Expected: clean.

- [ ] **Step 9: Commit**

```bash
git add src/pipeline.rs src/transcribe.rs src/main.rs src/fetcher/mod.rs src/fetcher/ytdlp.rs tests/pipeline_fakes.rs
git commit -m "$(cat <<'EOF'
feat(pipeline): replace whisper-cli subprocess with WhisperEngine

Wires the Plan B Epic 1 WhisperEngine into pipeline::process_one,
replacing the legacy whisper-cli subprocess call. Plan A's serial
loop is preserved; engine is constructed once at process startup and
shut down at the end.

- New Transcriber trait in transcribe.rs; both WhisperEngine and
  test FakeTranscriber implement it. process_one takes &dyn Transcriber.
- Transcriber::name() returns "whisper-rs"; FakeTranscriber returns
  "fake-transcriber"; the value lands in TranscriptMetadata.transcript_source
  (no more hardcoded "whisper.cpp" — partial fix for FOLLOWUPS T14).
- VideoFetcher gains a name() method symmetrically; YtDlpFetcher returns
  "ytdlp"; lands in TranscriptMetadata.fetcher (no more hardcoded).
- Legacy transcribe::transcribe() and TranscribeResult deleted.
- WAV decode (audio::decode_wav) called from process_one before the
  transcribe call; passes owned Vec<f32> samples per AD0016.
- AD0008 invariant preserved: artifacts written before mark_succeeded.

Tier 2 integration test asserts raw_signals lands in the JSON artifact
with schema_version="1" and the expected segment/token structure;
asserts transcript_source matches the actual transcriber name.

Resolves FOLLOWUPS T14 (multi-fetcher provenance) for both fetcher
and transcriber sides.

Refs: AD0008, AD0009, AD0012, AD0014, AD0016

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Self-check

- [ ] All previous tests pass + the new pipeline_writes_raw_signals test passes
- [ ] Legacy `transcribe::transcribe` function and `TranscribeResult` struct are gone
- [ ] `transcript_source` in the JSON artifact reflects the actual transcriber name (not hardcoded)
- [ ] `fetcher` in the JSON artifact reflects the actual fetcher name
- [ ] AD0008 invariant: artifacts written before mark_succeeded
- [ ] WhisperEngine constructed once per `process` invocation, shut down at end
- [ ] `cargo clippy` clean; `#[allow(dead_code)]` annotations on consumed items removed
- [ ] FOLLOWUPS.md updated: T14 entry removed (or annotated as resolved by Epic 1)
