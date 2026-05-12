# Task 12 — e2e_real_tools test: switch to whisper-rs path

**Goal:** Upgrade the existing `tests/e2e_real_tools.rs` integration test to exercise the whisper-rs pipeline instead of whisper-cli. Test remains `#[ignore]` per Plan A; runs manually during the A10 bake (Task 13).

**ADRs touched:** AD0009 (whisper-rs), AD0005 (test-helpers gating).

**Files:**
- Modify: `tests/e2e_real_tools.rs`

---

- [ ] **Step 1: Read the current e2e test**

```bash
cat tests/e2e_real_tools.rs
```

Identify the existing test structure. It likely:
- Sets up a tempdir
- Writes a DDP fixture JSON
- Runs `init`, `ingest`, `process`
- Asserts a transcript appears
- Was using `whisper-cli` as the transcribe path

- [ ] **Step 2: Update the test to use the whisper-rs path**

The test now goes through `WhisperEngine`. The orchestrator is the same `process` subcommand; from the test's perspective, the binary entry point is unchanged. The change is purely that the binary internally uses whisper-rs.

So the test SHOULD work end-to-end with the new code IF:
- The model file is present at `./models/ggml-tiny.en.bin`
- whisper-rs builds (which it does, per T2)

What's new: assert the JSON artifact contains the `raw_signals` field. Add:

```rust
#[test]
#[ignore]
fn e2e_real_tools_with_whisper_rs_writes_raw_signals() {
    // ... existing setup (tempdir, fixture, init, ingest, process) ...

    // After process completes, find the transcript JSON
    let transcripts_dir = workdir.path().join("transcripts");
    let json_files: Vec<_> = walkdir::WalkDir::new(&transcripts_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |ext| ext == "json") && !e.path().to_string_lossy().contains(".raw."))
        .collect();
    assert!(!json_files.is_empty(), "expected at least one transcript .json");

    let json_text = std::fs::read_to_string(json_files[0].path()).expect("read json");
    let parsed: serde_json::Value = serde_json::from_str(&json_text).expect("parse");

    // Raw signals must be present per AD0010
    let rs = &parsed["raw_signals"];
    assert!(!rs.is_null(), "raw_signals must be populated");
    assert_eq!(rs["schema_version"], "1");
    assert!(!rs["language"].as_str().unwrap_or("").is_empty(), "language must be set");

    // Segments must be non-empty for real audio
    let segments = rs["segments"].as_array().expect("segments");
    assert!(!segments.is_empty(), "real transcript should have segments");

    // Each segment has no_speech_prob and a tokens array
    for seg in segments {
        let no_speech = seg["no_speech_prob"].as_f64().expect("no_speech_prob");
        assert!(no_speech >= 0.0 && no_speech <= 1.0, "no_speech_prob out of range");
        let tokens = seg["tokens"].as_array().expect("tokens");
        for tok in tokens {
            let p = tok["p"].as_f64().expect("p");
            assert!(p >= 0.0 && p <= 1.0, "p out of range");
        }
    }

    // Transcript_source should be "whisper-rs", not "whisper.cpp"
    assert_eq!(parsed["transcript_source"], "whisper-rs");
}
```

If the existing test is structurally similar, fold the new assertions into it rather than duplicating the setup.

- [ ] **Step 3: Update the e2e test's whisper model assertion**

The existing test likely expected output from `whisper-cli`'s text format. Now the text comes from whisper-rs's getter. Format should be equivalent (concatenated segment text); update the assertion to be loose (text non-empty, no specific phrase match — TikTok content drifts).

- [ ] **Step 4: Verify the test compiles and the existing test still runs (ignored)**

```bash
cargo test --features test-helpers --test e2e_real_tools
```

Expected: tests are listed but `0 passed, 0 failed, 1 ignored`.

- [ ] **Step 5: Document how to run the test during the A10 bake**

Add a comment header to the test file:

```rust
//! End-to-end real-tools tests. Runs against real yt-dlp + the embedded
//! whisper-rs pipeline. Requires:
//! - yt-dlp on PATH
//! - ffmpeg on PATH (yt-dlp's postprocessor)
//! - ./models/ggml-tiny.en.bin on disk
//! - network egress to TikTok CDN
//!
//! Run manually during the SRC A10 bake (Task 13):
//!   cargo test --release --features test-helpers,cuda --test e2e_real_tools -- --ignored --nocapture
//!
//! Without --features cuda, falls back to whisper-rs's CPU build (slow but functional).
```

- [ ] **Step 6: cargo fmt and clippy**

```bash
cargo fmt --all && cargo clippy --all-targets -- -D warnings
```

Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add tests/e2e_real_tools.rs
git commit -m "$(cat <<'EOF'
test(e2e): upgrade e2e_real_tools to assert raw_signals + whisper-rs path

Adapts the existing end-to-end real-tools test to the Plan B Epic 1
pipeline:

- Asserts raw_signals object is present with schema_version="1"
- Asserts segments[] is non-empty for real audio with structural
  range checks on no_speech_prob and per-token p
- Asserts transcript_source is "whisper-rs" (no longer hardcoded
  "whisper.cpp")

Test remains #[ignore] per Plan A pattern. Runs manually during the
A10 bake via:

    cargo test --release --features test-helpers,cuda \
      --test e2e_real_tools -- --ignored --nocapture

Falls back to whisper-rs CPU build without --features cuda.

Refs: AD0009, AD0010

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Self-check

- [ ] Test compiles with `cargo test --features test-helpers --test e2e_real_tools` (listed, not run)
- [ ] Assertions check raw_signals structure per AD0010
- [ ] Assertions check transcript_source == "whisper-rs" (FOLLOWUPS T14 fix verified)
- [ ] No assertion on specific transcript content (resilient to TikTok content drift)
- [ ] Header comment documents the bake-time invocation
- [ ] No clippy warnings on the test file
