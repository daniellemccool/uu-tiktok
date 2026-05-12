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

    // Verify the worker is alive by sending a transcribe and getting any reply
    // (T6 worker still returns Bug; T7 will return a real transcript).
    let samples = vec![0.0_f32; 16000]; // 1 second of silence
    let result = engine
        .transcribe(samples, PerCallConfig::default(), Duration::from_secs(30))
        .await;
    // T6 placeholder: worker returns a specific Bug message. T7 replaces this
    // assertion with the real transcript expectation.
    match result {
        Err(TranscribeError::Bug { ref detail }) if detail.contains("T6 init only") => {}
        other => panic!("expected T6 placeholder Bug error, got: {other:?}"),
    }

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
