# Task 10 — JSON artifact writer with raw_signals (schema_version=1)

**Goal:** Extend the existing `{video_id}.json` transcript metadata artifact with a `raw_signals` object carrying `schema_version`, `language`, `lang_probs`, and `segments` per AD0010. Additive — Plan A's existing fields preserved.

**ADRs touched:** AD0010 (raw_signals schema, schema_version), AD0008 (artifact-before-mark_succeeded invariant unchanged).

**Files:**
- Modify: `src/output/artifacts.rs` — extend `TranscriptMetadata` struct and the JSON writer
- Modify: `src/transcribe.rs` — add a helper that converts `TranscribeOutput` → `RawSignals` for the artifact
- Test: inline `#[cfg(test)] mod tests` extending the existing artifact tests

---

- [ ] **Step 1: Write the failing test**

In `src/output/artifacts.rs`, add (or extend) a test module:

```rust
#[cfg(test)]
mod plan_b_tests {
    use super::*;
    use serde_json::Value;

    fn sample_metadata_with_raw_signals() -> TranscriptMetadata {
        TranscriptMetadata {
            video_id: "7234567890123456789".to_string(),
            source_url: "https://www.tiktokv.com/share/video/7234567890123456789/".to_string(),
            duration_s: 23.4,
            language_detected: Some("en".to_string()),
            transcribed_at: "2026-05-12T13:45:22Z".to_string(),
            fetcher: "ytdlp".to_string(),
            transcript_source: "whisper-rs".to_string(),
            model: "ggml-tiny.en.bin".to_string(),
            raw_signals: Some(RawSignals {
                schema_version: "1".to_string(),
                language: "en".to_string(),
                lang_probs: None,
                segments: vec![RawSegment {
                    no_speech_prob: 0.02,
                    tokens: vec![RawToken { p: 0.99, plog: -0.01 }],
                }],
            }),
        }
    }

    #[test]
    fn metadata_serializes_with_raw_signals_object() {
        let meta = sample_metadata_with_raw_signals();
        let json: Value = serde_json::to_value(&meta).expect("serialize");
        let rs = &json["raw_signals"];
        assert_eq!(rs["schema_version"], "1");
        assert_eq!(rs["language"], "en");
        assert_eq!(rs["lang_probs"], Value::Null);
        let segments = rs["segments"].as_array().expect("segments array");
        assert_eq!(segments.len(), 1);
        assert!((segments[0]["no_speech_prob"].as_f64().unwrap() - 0.02).abs() < 1e-6);
    }

    #[test]
    fn metadata_without_raw_signals_serializes_with_null_or_absent_field() {
        let mut meta = sample_metadata_with_raw_signals();
        meta.raw_signals = None;
        let json: Value = serde_json::to_value(&meta).expect("serialize");
        // The field is either absent or null; we accept either via serde attrs.
        let rs = &json["raw_signals"];
        assert!(rs.is_null() || rs == &Value::Null);
    }
}
```

Run:
```bash
cargo test --lib output::artifacts::plan_b_tests::
```

Expected: FAIL with compile errors — `RawSignals`, `RawSegment`, `RawToken`, `TranscriptMetadata::raw_signals` not yet defined.

- [ ] **Step 2: Extend TranscriptMetadata and add the raw_signals types**

Modify `src/output/artifacts.rs`. Add the types alongside the existing `TranscriptMetadata`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RawSignals {
    pub schema_version: String,
    pub language: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang_probs: Option<Vec<(String, f32)>>,
    pub segments: Vec<RawSegment>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RawSegment {
    pub no_speech_prob: f32,
    pub tokens: Vec<RawToken>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RawToken {
    pub p: f32,
    pub plog: f32,
}

// Extend the existing TranscriptMetadata struct:
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TranscriptMetadata {
    // ... existing fields preserved ...
    pub video_id: String,
    pub source_url: String,
    pub duration_s: f64,
    pub language_detected: Option<String>,
    pub transcribed_at: String,
    pub fetcher: String,
    pub transcript_source: String,
    pub model: String,

    /// Plan B Epic 1 addition. None for legacy rows; Some for new ones.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_signals: Option<RawSignals>,
}
```

The `#[serde(skip_serializing_if = "Option::is_none")]` on `raw_signals` keeps backward compatibility: a row written by Plan A code (with `raw_signals: None`) serializes without the field. A row written by Plan B serializes with the field populated.

`lang_probs` uses the same skip-if-none so a default raw_signals doesn't emit `"lang_probs": null` (the AD0010 schema requires null be present — let me reconsider).

Actually, per AD0010, `lang_probs` should be present as `null` when not opted in. Remove the `skip_serializing_if` from `RawSignals::lang_probs` so it serializes as `null`. Re-check the tests after.

- [ ] **Step 3: Add the conversion helper**

In `src/transcribe.rs`, add a `TranscribeOutput::into_raw_signals(&self) -> RawSignals` method:

```rust
impl TranscribeOutput {
    pub fn to_raw_signals(&self) -> crate::output::artifacts::RawSignals {
        use crate::output::artifacts::{RawSegment, RawSignals, RawToken};
        RawSignals {
            schema_version: "1".to_string(),
            language: self.language.clone(),
            lang_probs: self.lang_probs.clone(),
            segments: self.segments.iter().map(|s| RawSegment {
                no_speech_prob: s.no_speech_prob,
                tokens: s.tokens.iter().map(|t| RawToken { p: t.p, plog: t.plog }).collect(),
            }).collect(),
        }
    }
}
```

- [ ] **Step 4: Run the tests**

```bash
cargo test --lib output::artifacts::plan_b_tests::
```

Expected: PASS.

- [ ] **Step 5: Update the existing TranscriptMetadata writer site to set raw_signals**

The actual write site is in `src/pipeline.rs::process_one`. T11 wires Plan B's TranscribeOutput into TranscriptMetadata; for T10 we just ensure the struct supports the field and serializes correctly. The default-None case means existing tests that construct TranscriptMetadata without `raw_signals` still pass (the field has a `#[serde(default)]`).

Run the full Plan A test suite:

```bash
cargo test --features test-helpers
```

Expected: all pass. The existing artifact-writing tests in Plan A continue to work because `raw_signals: None` is the implicit default and gets skipped on serialization.

- [ ] **Step 6: cargo fmt and clippy**

```bash
cargo fmt --all && cargo clippy --all-targets -- -D warnings
```

Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add src/output/artifacts.rs src/transcribe.rs
git commit -m "$(cat <<'EOF'
feat(artifacts): extend TranscriptMetadata with raw_signals (schema v1)

Adds RawSignals, RawSegment, RawToken types and embeds an optional
raw_signals object on TranscriptMetadata. Per AD0010:

- schema_version: "1" from day one (additive evolution path)
- language: single ISO code (free from whisper_full_lang_id)
- lang_probs: null by default; populated when --compute-lang-probs
- segments[]: per-segment no_speech_prob + tokens[] with p / plog

Plan A's existing TranscriptMetadata callers continue to work because
raw_signals is Option<...> with serde(default) and
skip_serializing_if=Option::is_none — a legacy None row serializes
without the field.

TranscribeOutput::to_raw_signals() bridges from the transcribe-side
types (T4) to the artifact-side types (T10) without coupling the two
modules' dependency graphs.

Tier 1 tests confirm serialization shape: schema_version present,
lang_probs null when None, segments[] structure as documented.

Refs: AD0010 (raw_signals schema)

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Self-check

- [ ] `output::artifacts::plan_b_tests::` passes
- [ ] Existing artifact tests still pass (no regressions to Plan A's metadata writer)
- [ ] Field serialization matches AD0010 exactly: lang_probs serializes as null when None (NOT omitted); raw_signals serializes as the object when Some
- [ ] `TranscribeOutput::to_raw_signals` is the only bridge between the two type families — no circular module imports
- [ ] Schema version is "1" (string, not integer — AD0010 specifies string for evolution flexibility)
