# Task 8 — PerCallConfig: --compute-lang-probs opt-in + Tier 2 lang_probs test

**Goal:** Wire the `compute_lang_probs` field on `PerCallConfig` to actually trigger the extra-encoder-pass lang-detection call. When opt-in, `TranscribeOutput.lang_probs` contains the per-language probability vector; otherwise it's `None`.

**ADRs touched:** AD0010 (lang_probs opt-in via flag).

**Files:**
- Modify: `src/transcribe.rs` — extend the worker request loop with the optional lang_detect call
- Test: extend `tests/whisper_engine_init.rs` with a lang_probs presence test
- Modify: `src/cli.rs` — add `--compute-lang-probs` global flag (or per-subcommand on `process`)
- Modify: `src/config.rs` — add `compute_lang_probs` field to Config; default false in dev profile

---

- [ ] **Step 1: Write the failing test**

Add to `tests/whisper_engine_init.rs`:

```rust
#[tokio::test]
async fn lang_probs_present_when_opt_in() {
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

    let samples = uu_tiktok::audio::decode_wav(&silence_fixture_path()).expect("decode fixture");

    // Without opt-in: lang_probs should be None
    let output_default = engine
        .transcribe(
            samples.clone(),
            PerCallConfig::default(),
            Duration::from_secs(60),
        )
        .await
        .expect("default transcribe succeeds");
    assert!(
        output_default.lang_probs.is_none(),
        "lang_probs should be None by default"
    );

    // With opt-in: lang_probs should be Some(...) populated
    let mut cfg = PerCallConfig::default();
    cfg.compute_lang_probs = true;
    let output_with_probs = engine
        .transcribe(samples, cfg, Duration::from_secs(60))
        .await
        .expect("opt-in transcribe succeeds");
    assert!(
        output_with_probs.lang_probs.is_some(),
        "lang_probs should be Some when compute_lang_probs is true"
    );
    let probs = output_with_probs.lang_probs.unwrap();
    assert!(!probs.is_empty(), "should have at least one language probability");
    // Probabilities should sum to ~1.0
    let sum: f32 = probs.iter().map(|(_, p)| p).sum();
    assert!((sum - 1.0).abs() < 0.1, "probs should sum to ~1.0, got {}", sum);

    engine.shutdown();
}
```

Run:
```bash
cargo test --features test-helpers --test whisper_engine_init -- lang_probs_present_when_opt_in
```

Expected: FAIL — T7's worker returns `lang_probs: None` unconditionally.

- [ ] **Step 2: Implement the lang_probs opt-in path**

Modify the worker thread in `src/transcribe.rs`. After the successful `state.full(...)` call, BEFORE building the `Ok(TranscribeOutput { ... })` reply:

```rust
// Compute lang_probs only when opt-in. Pays an extra encoder pass per
// sharp-edges.md:13 — whisper_lang_auto_detect re-encodes the audio.
let lang_probs = if req.config.compute_lang_probs {
    // whisper-rs exposes WhisperState::lang_detect which returns Vec<f32>
    // of probabilities indexed by language ID. We pair each probability
    // with its ISO code via WhisperContext::lang_str.
    match state.lang_detect(0, 1) {
        Ok(probs_vec) => {
            // Pair each prob with its language code. lang_max_id() returns
            // the highest valid language ID; whisper-rs's API may vary.
            let max_id = whisper_rs::WhisperContext::lang_max_id() as usize;
            let mut paired = Vec::with_capacity(probs_vec.len().min(max_id + 1));
            for (id, p) in probs_vec.iter().enumerate().take(max_id + 1) {
                if let Some(code) = whisper_rs::WhisperContext::lang_str(id as i32) {
                    paired.push((code.to_string(), *p));
                }
            }
            // Sort descending by probability for operator-readable JSON output
            paired.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            Some(paired)
        }
        Err(e) => {
            tracing::warn!("lang_detect failed: {e}; emitting null lang_probs");
            None
        }
    }
} else {
    None
};

// Then in the reply:
let _ = req.reply.send(Ok(TranscribeOutput {
    text,
    language,
    lang_probs,            // <-- changed from None to the variable
    segments: vec![],
    model_id: /* as before */,
}));
```

**Note on whisper-rs API surface:** the exact signature of `state.lang_detect(offset_ms, n_threads)` and the `WhisperContext::lang_str` / `lang_max_id` static methods depends on the pinned crate version. If your pinned version exposes different names, search the docs.rs page for the equivalent. The semantic shape is constant: "run language detection on the current mel; get a Vec<f32> of probabilities indexed by language ID; pair each with its ISO code via the context's lang_str(id) helper."

If `state.lang_detect` requires the mel to be precomputed via `state.pcm_to_mel(samples, n_threads)` first, add that call before `lang_detect`. The whisper-rs docs.rs page is the source of truth.

- [ ] **Step 3: Add the CLI flag and config field**

Modify `src/cli.rs`. In the global flags or under the `process` subcommand:

```rust
/// Compute per-language probability distribution per video.
/// Costs one extra encoder pass per video; default false.
#[arg(long, env = "UU_TIKTOK_COMPUTE_LANG_PROBS", global = true)]
pub compute_lang_probs: bool,
```

Modify `src/config.rs`. Add to the `Config` struct:

```rust
pub struct Config {
    // ... existing fields ...
    pub compute_lang_probs: bool,
}
```

Wire it through the profile-default → env-var → CLI-flag resolution chain. Default `false` in both dev and prod profiles. Resolution happens in `main.rs`; thread it through to `pipeline::process_one` → `PerCallConfig`.

- [ ] **Step 4: Run the lang_probs test**

```bash
cargo test --features test-helpers --test whisper_engine_init -- lang_probs_present_when_opt_in
```

Expected: PASS.

- [ ] **Step 5: Run full suite**

```bash
cargo test --features test-helpers
```

Expected: all pass.

- [ ] **Step 6: cargo fmt and clippy**

```bash
cargo fmt --all && cargo clippy --all-targets -- -D warnings
```

Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add src/transcribe.rs src/cli.rs src/config.rs tests/whisper_engine_init.rs
git commit -m "$(cat <<'EOF'
feat(transcribe): wire --compute-lang-probs opt-in for lang_probs

Adds the explicit opt-in path for computing lang_probs[]. Per AD0010
and the whisper.cpp deepdive (sharp-edges.md:13), the per-language
probability distribution requires a separate whisper_lang_auto_detect
call that re-encodes the audio. Default behavior emits null lang_probs;
--compute-lang-probs / UU_TIKTOK_COMPUTE_LANG_PROBS=1 enables it at a
cost of one extra encoder pass per video.

Output sorted descending by probability for operator-readable JSON.

Tier 2 test exercises both branches: default => None; opt-in => Some
with probabilities summing to ~1.0.

Refs: AD0010

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Self-check

- [ ] `lang_probs_present_when_opt_in` test passes
- [ ] Default config gives lang_probs=None
- [ ] CLI flag `--compute-lang-probs` works on `process` subcommand
- [ ] Env var `UU_TIKTOK_COMPUTE_LANG_PROBS=1` overrides default
- [ ] No clippy warnings
- [ ] Probabilities sort descending in the output (operator-readable)
- [ ] Lang_detect failure (rare) gracefully degrades to None with a tracing::warn!
