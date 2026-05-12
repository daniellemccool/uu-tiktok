//! WhisperEngine init smoke test.
//!
//! Requires ./models/ggml-tiny.en.bin on disk; gated by test-helpers feature
//! per AD0005 because it depends on a non-trivial fixture.

#![cfg(feature = "test-helpers")]

use std::path::PathBuf;
use std::time::{Duration, Instant};

use uu_tiktok::errors::TranscribeError;
use uu_tiktok::transcribe::{EngineConfig, PerCallConfig, WhisperEngine};

fn tiny_model_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("models/ggml-tiny.en.bin")
}

#[tokio::test]
async fn engine_loads_tiny_en_model_successfully() {
    if !tiny_model_path().exists() {
        eprintln!("Skipping: ./models/ggml-tiny.en.bin not found. Run scripts/fetch-tiny-model.sh");
        return;
    }

    let config = EngineConfig {
        model_path: tiny_model_path(),
        gpu_device: 0,
        // flash_attn forced false locally because we don't always build with cuda
        flash_attn: false,
    };

    let engine = WhisperEngine::new(&config).expect("engine should load tiny.en");

    // Verify the worker is alive by sending a transcribe and getting a real
    // reply (T7 wires inference). 1s of silence on tiny.en should succeed
    // with empty/short text and a populated language code.
    let samples = vec![0.0_f32; 16000]; // 1 second of silence
    let output = engine
        .transcribe(samples, PerCallConfig::default(), Duration::from_secs(30))
        .await
        .expect("transcribe of 1s silence should succeed");
    assert!(
        output.text.len() < 200,
        "silence shouldn't transcribe to a long phrase, got: {:?}",
        output.text
    );
    assert!(!output.language.is_empty(), "language should be set");

    // Regression guard for the Drop-ordering deadlock (see WhisperEngine::teardown).
    // If a future change inverts "drop sender → join handle" to "join handle → drop
    // sender", the worker will park forever in blocking_recv and shutdown() will
    // hang. Fail fast at 5s wallclock instead of letting the test harness time out
    // at 60s+ with no diagnostic context.
    let start = Instant::now();
    engine.shutdown();
    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_secs(5),
        "shutdown took {elapsed:?} — possible Drop-ordering deadlock in WhisperEngine"
    );
}

#[tokio::test]
async fn engine_rejects_missing_model_path() {
    let config = EngineConfig {
        model_path: PathBuf::from("/nonexistent/model.bin"),
        gpu_device: 0,
        flash_attn: false,
    };
    let result = WhisperEngine::new(&config);
    assert!(
        result.is_err(),
        "expected WhisperInitError on missing model"
    );
}

fn silence_fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/audio/silence_16khz_mono.wav")
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

    let samples = uu_tiktok::audio::decode_wav(&silence_fixture_path()).expect("decode fixture");

    let output = engine
        .transcribe(samples, PerCallConfig::default(), Duration::from_secs(60))
        .await
        .expect("transcribe of silence should succeed (text may be empty)");

    // Silence may produce empty or near-empty text. Either is fine.
    assert!(
        output.text.len() < 200,
        "silence shouldn't transcribe to a long phrase, got: {:?}",
        output.text
    );
    assert!(!output.language.is_empty(), "language should be set");
    // T7 returns empty segments; T9 extends with raw signal extraction.
    assert!(
        output.segments.is_empty(),
        "T7 returns empty segments; T9 fills them"
    );
    // model_id pinned to the file_name() of the configured model path.
    assert_eq!(output.model_id, "ggml-tiny.en.bin");

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

    // 30 seconds of silence — encoder still runs over the full window.
    let samples = vec![0.0_f32; 16000 * 30];

    // tiny.en on CPU typically takes 1-3 seconds for 30s audio. A 100ms
    // deadline should trip the abort callback well before completion.
    //
    // Wallclock guard: if a regression breaks cancellation, the test should
    // fail fast (within a few seconds) rather than hang to the harness timeout.
    // The abort callback fires at ggml-level frequency so cancellation latency
    // is sub-second; allow up to 10s for slow CI machines.
    let start = Instant::now();
    let result = engine
        .transcribe(
            samples,
            PerCallConfig::default(),
            Duration::from_millis(100),
        )
        .await;
    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_secs(10),
        "transcribe took {elapsed:?} — possible cancellation regression"
    );

    // Expect either Cancelled (most likely) or successful very short completion
    // on extremely fast hardware. Key invariant: we did NOT hang past a
    // reasonable wallclock.
    match result {
        Ok(_) => {
            eprintln!("inference completed within 100ms — fine on very fast hardware");
        }
        Err(TranscribeError::Cancelled) => {
            // The expected path.
        }
        Err(e) => panic!("expected Cancelled or Ok, got {e:?}"),
    }

    engine.shutdown();
}
