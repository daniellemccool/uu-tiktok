# Task 4 — TranscribeOutput / SegmentRaw / TokenRaw types + serde + Tier 1 tests

**Goal:** Define the owned-data types that cross the worker-thread boundary (per AD0016 worker-thread invariants) and that get serialized into the `raw_signals` JSON object (per AD0010). Tier 1 unit tests on serde round-tripping.

**ADRs touched:** AD0010 (raw_signals schema), AD0016 (worker-thread invariants — owned data only).

**Files:**
- Create: `src/transcribe.rs` — replaces the existing file but the new content starts with just the types
- Modify: `src/lib.rs` (already declares `pub mod transcribe;`; no change)
- Test: inline `#[cfg(test)] mod tests` in `src/transcribe.rs`

**Important:** This task **replaces** the existing `src/transcribe.rs` (which holds the Plan A whisper-cli subprocess wrapper). The new file will be progressively built up in T4 → T5 → T6 → T7 → T9. At the end of T4 it contains only the types; existing pipeline.rs callers of the old `transcribe::transcribe` function will break, so this task ALSO updates `src/pipeline.rs` to stub the call temporarily (or marks the test that exercises it `#[ignore]` until T11 lands).

**Pre-task: handle the file replacement carefully.**

Before deleting Plan A's `transcribe.rs` content, capture what currently exists so we know what callers depend on. Read it first:

```bash
cat src/transcribe.rs
```

The existing public API is: `pub async fn transcribe(...) -> Result<TranscribeResult, TranscribeError>` and the `TranscribeResult` struct. These are called from `src/pipeline.rs::process_one`. We need to keep the old API working until T11, OR `#[ignore]` the tests that depend on it.

**Approach (chosen):** keep Plan A's `transcribe::transcribe` function and `TranscribeResult` struct **temporarily renamed** (e.g., `legacy_transcribe`) under `#[allow(dead_code)]` so they continue to compile. The new types from this task live alongside. T11 (pipeline integration) deletes the legacy entries.

---

- [ ] **Step 1: Read the existing transcribe.rs and identify the API to preserve temporarily**

Run:
```bash
cat src/transcribe.rs
```

Identify:
- `TranscribeResult` struct (fields: `text`, `language`, `duration_s`, etc.)
- `transcribe(...)` async function
- `TranscribeError` enum (kept and extended; do not rename)

- [ ] **Step 2: Write the failing tests first (TDD)**

Add to `src/transcribe.rs` (alongside the existing content, before any deletions):

```rust
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SegmentRaw {
    /// whisper_full_get_segment_no_speech_prob(state, i)
    pub no_speech_prob: f32,
    /// Per-token confidence signals for this segment.
    pub tokens: Vec<TokenRaw>,
}

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
                    TokenRaw { p: 0.99, plog: -0.01 },
                    TokenRaw { p: 0.95, plog: -0.05 },
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
```

Run:
```bash
cargo test --lib transcribe::plan_b_tests::
```

Expected: FAIL with compile errors — the test file references `TranscribeOutput`, `SegmentRaw`, `TokenRaw` not yet in scope at the top of the file because we haven't actually added them yet. Then add the types verbatim above the test module.

Actually, the types ARE shown above in the test scaffolding step. So after writing the file content the test should compile.

Re-run:
```bash
cargo test --lib transcribe::plan_b_tests::
```

Expected: PASS for all four tests.

- [ ] **Step 3: Verify the rest of the codebase still compiles**

Run:
```bash
cargo build
```

Expected: clean build. The old `transcribe::transcribe` function and `TranscribeResult` struct still exist; we haven't deleted anything yet. The new types coexist alongside.

- [ ] **Step 4: Run the full Plan A test suite to verify no regressions**

Run:
```bash
cargo test --features test-helpers
```

Expected: all existing tests pass; the 4 new tests also pass.

- [ ] **Step 5: cargo fmt and clippy**

Run:
```bash
cargo fmt --all && cargo clippy --all-targets -- -D warnings
```

Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add src/transcribe.rs
git commit -m "$(cat <<'EOF'
feat(transcribe): add TranscribeOutput / SegmentRaw / TokenRaw types

Adds the owned-data types Plan B Epic 1 uses to carry whisper.cpp's
raw confidence signals out of the worker thread and into the JSON
artifact. Owned (no references, no whisper-rs handles) so they
satisfy the worker-thread boundary invariants from AD0016.

TranscribeOutput.lang_probs is Option<Vec<(String, f32)>> serializing
as null when None — the default since computing lang_probs requires
an extra encoder pass per video (sharp-edges.md:13).

Tier 1 tests cover serde round-trip and lang_probs null vs Some
representations. Plan A's existing transcribe function and
TranscribeResult struct remain in place; they'll be removed in
T11 (pipeline integration).

Refs: AD0010, AD0016

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Self-check

- [ ] `cargo test --lib transcribe::plan_b_tests::` passes all 4 tests
- [ ] Full Plan A suite still passes (76+ tests)
- [ ] No new clippy warnings
- [ ] The new types are `Debug`, `Clone`, `Serialize`, `Deserialize`, `PartialEq`
- [ ] `lang_probs` is `Option<Vec<(String, f32)>>` (tuple, not struct) so it serializes as nested arrays per AD0010
- [ ] The legacy `transcribe::transcribe` function and `TranscribeResult` are still in place untouched
