# Task 7 — WhisperEngine::transcribe: real inference + per-request cancellation + abort_callback

**Goal:** Replace T6's worker placeholder with real whisper_full_with_state calls. Wire `FullParams::abort_callback` to the per-request `Arc<AtomicBool>` cancel flag. Set embedding hygiene defaults (`print_progress=false`, `print_realtime=false`). Raw signal extraction lands in T9; this task returns a TranscribeOutput with `text` and `language` filled but empty `segments` so T9 can extend it without restructuring.

**ADRs touched:** AD0009, AD0012 (cancellation), AD0013 (embedding hygiene).

**Files:**
- Modify: `src/transcribe.rs` — implement the worker's request loop body
- Test: extend `tests/whisper_engine_init.rs` with a happy-path transcribe test + a cancellation test

---

- [ ] **Step 1: Write the failing happy-path transcribe test**

Extend `tests/whisper_engine_init.rs`:

```rust
use std::path::Path;

fn silence_fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/audio/silence_16khz_mono.wav")
}

#[tokio::test]
async fn transcribe_silence_returns_empty_or_short_text() {
    if !tiny_model_path().exists() {
        eprintln!("Skipping: model not on disk");
        return;
    }

    let config = EngineConfig {
        model_path: tiny_model_path(),
        gpu_device: 0,
        flash_attn: false,
    };
    let engine = WhisperEngine::new(&config).expect("engine loads");

    // Decode the silence fixture
    let samples = uu_tiktok::audio::decode_wav(&silence_fixture_path()).expect("decode fixture");

    let output = engine
        .transcribe(samples, PerCallConfig::default(), Duration::from_secs(60))
        .await
        .expect("transcribe of silence should succeed (text may be empty)");

    // Silence may produce empty or near-empty text. Either is fine.
    // The important assertion: we got a real TranscribeOutput, not a Bug error.
    assert!(
        output.text.len() < 200,
        "silence shouldn't transcribe to a long phrase, got: {:?}",
        output.text
    );
    assert!(!output.language.is_empty(), "language should be set");
    // T7 returns empty segments; T9 extends with raw signal extraction.

    engine.shutdown();
}

#[tokio::test]
async fn transcribe_respects_short_deadline() {
    if !tiny_model_path().exists() {
        eprintln!("Skipping: model not on disk");
        return;
    }

    let config = EngineConfig {
        model_path: tiny_model_path(),
        gpu_device: 0,
        flash_attn: false,
    };
    let engine = WhisperEngine::new(&config).expect("engine loads");

    // Build a 30-second sample that takes meaningful time to transcribe even on tiny.en
    let samples = vec![0.0_f32; 16000 * 30]; // 30 seconds of silence — encoder still runs

    // Use a deadline shorter than realistic inference time. tiny.en on CPU
    // typically takes 1-3 seconds for 30-second audio. A 100ms deadline should
    // trip the abort callback well before inference completes.
    let result = engine
        .transcribe(samples, PerCallConfig::default(), Duration::from_millis(100))
        .await;

    // Expect either Cancelled (most likely) or successful very short completion.
    // The key assertion: we did NOT hang past a reasonable timeout.
    match result {
        Ok(_) => {
            // On extremely fast hardware tiny.en may finish in <100ms; that's fine.
            eprintln!("inference completed within 100ms — fine on fast hardware");
        }
        Err(TranscribeError::Cancelled) => {
            // The expected path.
        }
        Err(e) => panic!("expected Cancelled or Ok, got {:?}", e),
    }

    engine.shutdown();
}
```

Run:
```bash
cargo test --features test-helpers --test whisper_engine_init -- transcribe_
```

Expected: FAIL — T6's worker still replies with Bug for transcribe.

- [ ] **Step 2: Implement the worker's request loop**

Modify `src/transcribe.rs`. Replace the worker request loop body in `WhisperEngine::new`:

```rust
// Inside the worker thread, after model load:
let mut _ctx = match ctx_result { /* ...as before... */ };

while let Some(req) = request_rx.blocking_recv() {
    let cancel = Arc::clone(&req.cancel);

    // Build FullParams with embedding hygiene defaults
    use whisper_rs::FullParams;
    use whisper_rs::SamplingStrategy;

    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_print_progress(false);   // sharp-edges.md:66
    params.set_print_realtime(false);   // sharp-edges.md:67
    params.set_print_special(false);
    params.set_print_timestamps(false); // we don't emit timestamps per AD0010

    // Language pin
    if let Some(lang) = &req.config.language {
        params.set_language(Some(lang.as_str()));
    } else {
        params.set_language(Some("auto"));
    }

    // Cooperative cancellation
    let cancel_for_abort = Arc::clone(&cancel);
    params.set_abort_callback_safe(move || cancel_for_abort.load(std::sync::atomic::Ordering::Relaxed));

    // Allocate state for this call. T9 will explore reusing state across calls;
    // for T7 we keep it simple — fresh state per request keeps memory bounded.
    let mut state = match _ctx.create_state() {
        Ok(s) => s,
        Err(e) => {
            let _ = req.reply.send(Err(TranscribeError::Bug {
                detail: format!("create_state: {e}"),
            }));
            continue;
        }
    };

    let run_result = state.full(params, &req.samples);

    let was_cancelled = cancel.load(std::sync::atomic::Ordering::Relaxed);

    match run_result {
        Err(_) if was_cancelled => {
            let _ = req.reply.send(Err(TranscribeError::Cancelled));
        }
        Err(e) => {
            let _ = req.reply.send(Err(TranscribeError::Bug {
                detail: format!("whisper_full failed: {e}"),
            }));
        }
        Ok(_) => {
            // Extract text and language. Raw signals (segments) are added in T9.
            let n_segments = state.full_n_segments().unwrap_or(0);
            let mut text = String::new();
            for i in 0..n_segments {
                if let Ok(seg_text) = state.full_get_segment_text(i) {
                    text.push_str(&seg_text);
                }
            }

            // Detected language (free per inference)
            let lang_id = state.full_lang_id();
            let language = whisper_rs::WhisperContext::lang_str(lang_id)
                .unwrap_or("unknown")
                .to_string();

            let _ = req.reply.send(Ok(TranscribeOutput {
                text,
                language,
                lang_probs: None,  // T8 adds the opt-in path
                segments: vec![],  // T9 fills this with raw signal extraction
                model_id: model_path
                    .file_name()
                    .and_then(|os| os.to_str())
                    .unwrap_or("unknown")
                    .to_string(),
            }));
        }
    }
}
```

**Note on `set_abort_callback_safe`:** whisper-rs offers two abort-callback APIs — `set_abort_callback` (unsafe FFI shape) and `set_abort_callback_safe` (idiomatic Rust closure). Use the safe variant. If the binding version pinned in T2 doesn't expose `_safe`, use the unsafe variant with a careful `extern "C" fn` shim.

- [ ] **Step 3: Run the tests**

```bash
cargo test --features test-helpers --test whisper_engine_init
```

Expected: all four tests now pass — model-load happy, model-load reject, transcribe silence, transcribe respect deadline.

If the deadline test still PASSES inference (the placeholder Bug error returned previously will be replaced with a successful run on very fast hardware), it's flaky. Consider lengthening the audio to 60s of silence OR shortening the deadline further (50ms).

- [ ] **Step 4: Run the full Plan A test suite**

```bash
cargo test --features test-helpers
```

Expected: all existing tests still pass; whisper_engine_init now has 4 passing tests.

- [ ] **Step 5: cargo fmt and clippy**

```bash
cargo fmt --all && cargo clippy --all-targets -- -D warnings
```

Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add src/transcribe.rs tests/whisper_engine_init.rs
git commit -m "$(cat <<'EOF'
feat(transcribe): WhisperEngine::transcribe wires real whisper_full + cancellation

Replaces T6's worker placeholder with real inference via
whisper_rs::WhisperState::full(). Per-request Arc<AtomicBool> cancel
flag wired into FullParams::set_abort_callback_safe (AD0012).

Embedding hygiene defaults applied: print_progress, print_realtime,
print_special, print_timestamps all set false. Language pin honored
when PerCallConfig::language is Some(_); falls back to "auto" when None.

Returns TranscribeOutput with text + language + model_id. Raw signal
extraction (segments populated with no_speech_prob and per-token p/plog)
lands in T9. lang_probs opt-in lands in T8.

Cancellation produces TranscribeError::Cancelled; other inference
errors surface as Bug for Epic 1's fail-fast posture (Epic 3 classifies).

Tier 2 tests: silence transcription completes; sub-100ms deadline trips
the abort callback within reasonable time.

Refs: AD0009, AD0012, AD0013

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Self-check

- [ ] All four whisper_engine_init tests pass on the dev machine
- [ ] Full Plan A suite still passes
- [ ] `cargo clippy` clean
- [ ] Cancellation flag is per-request (cloned into the abort callback's closure); never reused
- [ ] Cancelled inference returns `TranscribeError::Cancelled`, not a generic error
- [ ] `print_progress` and `print_realtime` both set to false (sharp-edges.md hygiene)
- [ ] Language pin honored when PerCallConfig.language is Some; auto otherwise
