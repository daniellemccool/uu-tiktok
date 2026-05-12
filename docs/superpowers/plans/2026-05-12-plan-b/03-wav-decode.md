# Task 3 — WAV decoder helper (hound) + Tier 1 test

**Goal:** Add a small `decode_wav(path) -> Result<Vec<f32>, AudioDecodeError>` helper that decodes a 16 kHz mono WAV file produced by yt-dlp's ffmpeg postprocessor into the float32 PCM samples whisper.cpp expects. Validate the WAV header on every load; reject non-conforming inputs with a typed error.

**ADRs touched:** AD0014 (audio-input invariant).

**Files:**
- Create: `src/audio.rs`
- Modify: `src/lib.rs` (add `pub mod audio;`)
- Modify: `src/main.rs` (add `mod audio;` if main needs to reference it; not required for Epic 1)
- Test: inline `#[cfg(test)] mod tests` in `src/audio.rs`
- Create: `tests/fixtures/audio/silence_16khz_mono.wav` (small 2-second WAV) — used by the integration test

**Note:** This is intentionally a small standalone module so subsequent tasks (T5+) can call it without depending on the whisper-rs internals.

---

- [ ] **Step 1: Write the failing tests first (TDD)**

Create `src/audio.rs` with this content:

```rust
//! WAV → Vec<f32> decoder for whisper.cpp's float32 PCM 16kHz mono audio input.
//!
//! See AD0014 (audio-input invariant) for the contract this module enforces.

use std::path::Path;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AudioDecodeError {
    #[error("opening WAV file {path}: {source}")]
    Open {
        path: String,
        #[source]
        source: hound::Error,
    },

    #[error("invalid WAV format for {path}: expected 16 kHz mono, got sample_rate={sample_rate} channels={channels}")]
    InvalidFormat {
        path: String,
        sample_rate: u32,
        channels: u16,
    },

    #[error("unsupported sample format for {path}: {detail}")]
    UnsupportedSampleFormat { path: String, detail: String },

    #[error("reading samples from {path}: {source}")]
    Read {
        path: String,
        #[source]
        source: hound::Error,
    },
}

/// Decode a 16 kHz mono WAV file into a Vec<f32> of PCM samples in [-1.0, 1.0].
///
/// Whisper.cpp's C API requires this exact format (api-and-pipeline.md:7).
/// Rejects non-conforming inputs with [`AudioDecodeError::InvalidFormat`].
pub fn decode_wav(path: &Path) -> Result<Vec<f32>, AudioDecodeError> {
    todo!("implement in step 3")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_path(name: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/audio")
            .join(name)
    }

    #[test]
    fn decodes_known_16khz_mono_wav_to_nonempty_vec() {
        let path = fixture_path("silence_16khz_mono.wav");
        let samples = decode_wav(&path).expect("fixture should decode");
        assert!(!samples.is_empty(), "expected non-empty samples");
        // 2 seconds at 16 kHz mono = 32000 samples
        assert!(
            samples.len() > 30000 && samples.len() < 35000,
            "expected ~32000 samples for 2-second fixture, got {}",
            samples.len()
        );
    }

    #[test]
    fn rejects_nonexistent_file() {
        let path = fixture_path("does-not-exist.wav");
        let err = decode_wav(&path).expect_err("missing file should error");
        assert!(matches!(err, AudioDecodeError::Open { .. }));
    }

    // Additional tests for InvalidFormat could be added if fixtures exist;
    // for Epic 1 the happy path + open-error coverage is sufficient since
    // yt-dlp's postprocessor always emits 16 kHz mono (AD0014 contract).
}
```

Run:
```bash
cargo test --lib audio::tests::
```

Expected: FAIL — `todo!()` panics in the happy-path test; the missing-file test will panic at `todo!` before it can match the Open variant.

- [ ] **Step 2: Generate the WAV fixture**

Use ffmpeg (already a runtime dep) to generate a 2-second silence WAV at the right format:

```bash
mkdir -p tests/fixtures/audio
ffmpeg -y -f lavfi -i anullsrc=channel_layout=mono:sample_rate=16000 -t 2 \
       -c:a pcm_s16le tests/fixtures/audio/silence_16khz_mono.wav
```

Verify:
```bash
file tests/fixtures/audio/silence_16khz_mono.wav
# expect: RIFF (little-endian) data, WAVE audio, Microsoft PCM, 16 bit, mono 16000 Hz
```

- [ ] **Step 3: Implement decode_wav**

Replace the `todo!()` body of `decode_wav` in `src/audio.rs`:

```rust
pub fn decode_wav(path: &Path) -> Result<Vec<f32>, AudioDecodeError> {
    let path_str = path.display().to_string();
    let mut reader = hound::WavReader::open(path).map_err(|e| AudioDecodeError::Open {
        path: path_str.clone(),
        source: e,
    })?;

    let spec = reader.spec();
    if spec.sample_rate != 16000 || spec.channels != 1 {
        return Err(AudioDecodeError::InvalidFormat {
            path: path_str,
            sample_rate: spec.sample_rate,
            channels: spec.channels,
        });
    }

    // Whisper.cpp expects float32 PCM in [-1.0, 1.0]. hound exposes either
    // integer (PCM_S16LE, the yt-dlp postprocessor default) or float samples.
    // Convert integer samples to f32 by dividing by i16::MAX.
    let samples: Result<Vec<f32>, _> = match spec.sample_format {
        hound::SampleFormat::Int => {
            if spec.bits_per_sample != 16 {
                return Err(AudioDecodeError::UnsupportedSampleFormat {
                    path: path_str,
                    detail: format!("int {} bits per sample (expected 16)", spec.bits_per_sample),
                });
            }
            reader
                .samples::<i16>()
                .map(|r| r.map(|s| s as f32 / i16::MAX as f32))
                .collect()
        }
        hound::SampleFormat::Float => reader.samples::<f32>().collect(),
    };

    samples.map_err(|e| AudioDecodeError::Read {
        path: path_str,
        source: e,
    })
}
```

- [ ] **Step 4: Wire the module into the library**

Modify `src/lib.rs`. Add the new module to the existing `pub mod` declarations:

```rust
pub mod audio;
```

Verify the dual-mod pattern is preserved per AD0002. If `src/main.rs` ever needs to call `audio::decode_wav` directly (Epic 1 only needs it via the engine, so this is unlikely), add a `mod audio;` there too with `#[allow(dead_code)]` if needed.

- [ ] **Step 5: Run tests to verify they pass**

Run:
```bash
cargo test --lib audio::tests::
```

Expected: PASS — both `decodes_known_16khz_mono_wav_to_nonempty_vec` and `rejects_nonexistent_file`.

- [ ] **Step 6: Run the full Plan A test suite to verify no regressions**

Run:
```bash
cargo test --features test-helpers
```

Expected: all existing tests still pass.

- [ ] **Step 7: cargo fmt and clippy**

Run:
```bash
cargo fmt --all && cargo clippy --all-targets -- -D warnings
```

Expected: clean.

- [ ] **Step 8: Commit**

```bash
git add src/audio.rs src/lib.rs tests/fixtures/audio/silence_16khz_mono.wav
git commit -m "$(cat <<'EOF'
feat(audio): add hound-based WAV decoder for float32 PCM samples

Adds src/audio.rs with decode_wav() that converts a 16 kHz mono WAV
file (yt-dlp's ffmpeg postprocessor output format) to Vec<f32> PCM
samples in [-1.0, 1.0] — the exact format whisper.cpp's C API expects
(api-and-pipeline.md:7).

The decoder validates the WAV header on every load. Non-conforming
inputs (wrong sample rate, wrong channel count, unsupported bit depth)
are rejected with a typed AudioDecodeError. Supports both PCM_S16LE
(the yt-dlp postprocessor default) and PCM_F32 sample formats.

Adds a 2-second silence fixture to tests/fixtures/audio/ for the
Tier 1 happy-path test.

Refs: AD0014

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Self-check

- [ ] `cargo test --lib audio::` passes both tests
- [ ] Full suite still passes
- [ ] `cargo clippy` clean
- [ ] AudioDecodeError variants name the path in their Display impl (operator-readable)
- [ ] decode_wav never panics on valid input (no `.unwrap()` in non-test code)
- [ ] Fixture WAV is checked into the repo and < 100 KB
