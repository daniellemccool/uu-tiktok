# Task 9 — Raw signals extraction: whisper-rs getters → SegmentRaw / TokenRaw

**Goal:** Replace T7's `segments: vec![]` with proper extraction of per-segment `no_speech_prob` and per-token `p` / `plog` via whisper-rs's getter API. Pure pass-through per AD0010 — no aggregation, no segment timestamps, no derived metrics.

**ADRs touched:** AD0010 (raw_signals schema, pass-through).

**Files:**
- Modify: `src/transcribe.rs` — extend the worker's Ok branch with raw signal extraction
- Test: extend `tests/whisper_engine_init.rs` with a structural test on segments + tokens

---

- [ ] **Step 1: Write the failing test**

Add to `tests/whisper_engine_init.rs`:

```rust
#[tokio::test]
async fn transcribe_populates_raw_signals_segments_and_tokens() {
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

    // Use a non-silent fixture so segments are likely non-empty.
    // For Epic 1's bake we'll add a real spoken-English fixture; for now,
    // silence may still produce one empty segment (whisper.cpp behavior).
    // The test asserts structural shape, not content.
    let samples = uu_tiktok::audio::decode_wav(&silence_fixture_path()).expect("decode fixture");

    let output = engine
        .transcribe(samples, PerCallConfig::default(), Duration::from_secs(60))
        .await
        .expect("transcribe succeeds");

    // Every segment must have a no_speech_prob in [0.0, 1.0]
    for (i, seg) in output.segments.iter().enumerate() {
        assert!(
            seg.no_speech_prob >= 0.0 && seg.no_speech_prob <= 1.0,
            "segment {i} no_speech_prob out of range: {}",
            seg.no_speech_prob
        );
        // Every token must have p in [0.0, 1.0] and plog non-positive
        for (j, tok) in seg.tokens.iter().enumerate() {
            assert!(
                tok.p >= 0.0 && tok.p <= 1.0,
                "segment {i} token {j} p out of range: {}",
                tok.p
            );
            assert!(
                tok.plog <= 0.0001,
                "segment {i} token {j} plog should be non-positive log-prob, got {}",
                tok.plog
            );
        }
    }

    engine.shutdown();
}
```

Run:
```bash
cargo test --features test-helpers --test whisper_engine_init -- transcribe_populates_raw_signals
```

Expected: PASS trivially if `output.segments` is empty (vacuous truth on the for loop). Need to either (a) use a fixture that produces non-empty segments, OR (b) add an explicit assertion that segments is non-empty for a non-silent fixture.

For T9 the silence fixture may legitimately produce 0 segments. Strengthen the assertion: if `output.text` is non-empty, then `output.segments` must also be non-empty.

```rust
if !output.text.trim().is_empty() {
    assert!(
        !output.segments.is_empty(),
        "non-empty text should produce non-empty segments"
    );
}
```

The structural assertions on no_speech_prob ranges fire when the loop has iterations.

- [ ] **Step 2: Implement raw signal extraction**

Modify the worker's Ok branch in `src/transcribe.rs`. Replace `segments: vec![]` with:

```rust
let mut segments_raw = Vec::with_capacity(n_segments as usize);
for i in 0..n_segments {
    // Per-segment no_speech_prob
    let no_speech_prob = match state.full_get_segment_no_speech_prob(i) {
        Ok(p) => p,
        Err(_) => 0.0,  // fall back to neutral if API errors (rare)
    };

    // Per-token p and plog within this segment
    let n_tokens = state.full_n_tokens(i).unwrap_or(0);
    let mut tokens_raw = Vec::with_capacity(n_tokens as usize);
    for j in 0..n_tokens {
        // whisper-rs's API for getting per-token data is via TokenData struct
        let token_data = match state.full_get_token_data(i, j) {
            Ok(td) => td,
            Err(_) => continue, // skip if the getter errors; rare
        };
        tokens_raw.push(TokenRaw {
            p: token_data.p,
            plog: token_data.plog,
        });
    }

    segments_raw.push(SegmentRaw {
        no_speech_prob,
        tokens: tokens_raw,
    });
}

// Then in the reply:
let _ = req.reply.send(Ok(TranscribeOutput {
    text,
    language,
    lang_probs,
    segments: segments_raw,
    model_id: /* as before */,
}));
```

**Note on whisper-rs's TokenData fields:** the exact field names in `TokenData` (e.g., `p`, `plog`, `pt`, `ptsum`) may differ slightly by version. Check the docs.rs page for the pinned version. The C API exposes `whisper_token_data.p`, `.plog`, `.pt`, `.ptsum` (confidence-and-sampling.md), and whisper-rs typically mirrors these 1:1.

**Note on token boundary characters:** whisper.cpp tokens include special markers like `[BEG]`, `[END]`, language tokens, etc. For Plan B's raw_signals capture we **keep them** — pass-through rule. Downstream consumers decide whether to filter. The `text` field already has whisper's own filtering applied.

- [ ] **Step 3: Run the test**

```bash
cargo test --features test-helpers --test whisper_engine_init -- transcribe_populates_raw_signals
```

Expected: PASS.

- [ ] **Step 4: Run full suite**

```bash
cargo test --features test-helpers
```

Expected: all pass.

- [ ] **Step 5: cargo fmt and clippy**

```bash
cargo fmt --all && cargo clippy --all-targets -- -D warnings
```

Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add src/transcribe.rs tests/whisper_engine_init.rs
git commit -m "$(cat <<'EOF'
feat(transcribe): extract per-segment no_speech_prob + per-token p/plog

Replaces T7's empty segments vec with real raw signal extraction via
whisper-rs's getter API. Per AD0010 pass-through rule:
- Per-segment: no_speech_prob from full_get_segment_no_speech_prob
- Per-token: p and plog from full_get_token_data

No aggregation, no segment timestamps, no derived metrics. The raw
signals carry whisper.cpp's native confidence shape unchanged into
the TranscribeOutput.

Tier 2 structural test asserts probability ranges on emitted data;
trivially passes on the silence fixture (zero segments) and gains
real coverage during T13's bake against speech fixtures.

Refs: AD0010

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Self-check

- [ ] `transcribe_populates_raw_signals_segments_and_tokens` passes
- [ ] All previous tests still pass
- [ ] No clippy warnings
- [ ] Getter errors do not crash the worker — they degrade to neutral values
- [ ] Special tokens (language IDs, [BEG], [END]) are retained per pass-through
- [ ] No aggregation code paths exist; downstream consumers compute what they need
