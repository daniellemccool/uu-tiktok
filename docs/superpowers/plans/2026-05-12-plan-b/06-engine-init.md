# Task 6 — WhisperEngine::new: model load + GPU verification + init logs

**Goal:** Replace T5's placeholder worker body with real whisper-rs model loading. Verify the backend is GPU when the `cuda` feature is enabled (per AD0013) and log the gpu_device + reported device name. The worker loop still returns Bug for transcribe requests; T7 wires the inference.

**ADRs touched:** AD0009 (whisper-rs), AD0013 (GPU verification), AD0015 (no whisper_full_parallel).

**Files:**
- Modify: `src/transcribe.rs` — replace the T5 placeholder worker body with real model load
- Test: `tests/whisper_engine_init.rs` — new integration test (test-helpers gated)

**Pre-task:** This task requires a whisper model file on disk. The dev machine has `./models/ggml-tiny.en.bin` (from Plan A's `scripts/fetch-tiny-model.sh`). Verify before starting:

```bash
ls -lh ./models/ggml-tiny.en.bin
```

If missing, run `./scripts/fetch-tiny-model.sh` first.

---

- [ ] **Step 1: Write the failing integration test first**

Create `tests/whisper_engine_init.rs`:

```rust
//! WhisperEngine init smoke test.
//!
//! Requires ./models/ggml-tiny.en.bin on disk; gated by test-helpers feature
//! per AD0005 because it depends on a non-trivial fixture.

#![cfg(feature = "test-helpers")]

use std::path::PathBuf;
use std::time::Duration;

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
    // For T6, expect the placeholder Bug error from the worker loop's
    // "not yet implemented" arm. T7 changes this to Ok(...).
    let _ = result;

    engine.shutdown();
}
```

In `Cargo.toml`, register the test:

```toml
[[test]]
name = "whisper_engine_init"
required-features = ["test-helpers"]
```

Run:
```bash
cargo test --features test-helpers --test whisper_engine_init
```

Expected: FAIL with the current T5 worker. The model isn't actually loaded yet because T5's worker doesn't call whisper-rs.

Wait — re-read T5's worker. It currently doesn't load any model; the `_config: &EngineConfig` parameter is ignored. So this test SHOULD currently PASS the model_path check trivially (we don't check it). To get a meaningful failing test, we need the engine to actually attempt to load the model. Let's tighten the test: assert the engine init *would* fail if given a bogus path.

Actually, the cleaner approach: write two tests, one for success and one for failure, and let the failure case drive the model-load implementation.

Add to `tests/whisper_engine_init.rs`:

```rust
#[tokio::test]
async fn engine_rejects_missing_model_path() {
    let config = EngineConfig {
        model_path: PathBuf::from("/nonexistent/model.bin"),
        gpu_device: 0,
        flash_attn: false,
    };
    let result = WhisperEngine::new(&config);
    assert!(result.is_err(), "expected WhisperInitError on missing model");
}
```

Run the test:
```bash
cargo test --features test-helpers --test whisper_engine_init -- engine_rejects_missing_model_path
```

Expected: FAIL — T5's worker doesn't actually load the model, so this returns `Ok(...)`.

- [ ] **Step 2: Implement real model loading in the worker thread**

Modify `src/transcribe.rs`. Replace the T5 placeholder worker body with whisper-rs model loading. The key pattern (per `bindings.md` § "Choosing a binding pattern for our pipeline"):

```rust
use whisper_rs::{WhisperContext, WhisperContextParameters};

// Inside WhisperEngine::new, replacing T5's worker thread spawn:

let model_path = _config.model_path.clone();
let gpu_device = _config.gpu_device;
let flash_attn = _config.flash_attn;

// Use a oneshot channel to surface init errors back to the caller before
// the worker enters its request loop.
let (init_tx, init_rx) = std::sync::mpsc::sync_channel::<Result<(), WhisperInitError>>(0);

let handle = thread::Builder::new()
    .name("uu-tiktok-whisper-worker".to_string())
    .spawn(move || {
        // Build context parameters
        let mut ctx_params = WhisperContextParameters::default();
        ctx_params.use_gpu(true); // whisper-rs picks CUDA if the cuda feature is enabled
        ctx_params.flash_attn(flash_attn);
        ctx_params.gpu_device(gpu_device);

        // Load the model
        let ctx_result = WhisperContext::new_with_params(
            model_path.to_str().unwrap_or(""),
            ctx_params,
        );
        let _ctx = match ctx_result {
            Ok(c) => {
                // AD0013: log the gpu_device index and reported backend.
                // whisper-rs's init emits backend info via whisper_log_set;
                // we capture and log via tracing. For Epic 1's first cut,
                // emit a tracing::info! based on the cuda feature flag — the
                // T13 bake runbook adds the actual backend-assertion log capture.
                tracing::info!(
                    gpu_device = gpu_device,
                    flash_attn = flash_attn,
                    model_path = %model_path.display(),
                    "WhisperEngine: model loaded"
                );
                if !init_tx.send(Ok(())).is_ok() {
                    return; // caller went away
                }
                c
            }
            Err(e) => {
                let _ = init_tx.send(Err(WhisperInitError::ModelLoad {
                    path: model_path.display().to_string(),
                    detail: format!("{e}"),
                }));
                return;
            }
        };

        // Worker request loop
        while let Some(req) = request_rx.blocking_recv() {
            // T7 implements: call _ctx.create_state(), then state.full(...),
            // extract raw signals, reply with TranscribeOutput.
            // T6 placeholder: still Bug error.
            let _ = req.reply.send(Err(TranscribeError::Bug {
                detail: "WhisperEngine::transcribe not yet implemented (T6 init only)".to_string(),
            }));
        }
    })
    .map_err(|e| WhisperInitError::ModelLoad {
        path: _config.model_path.display().to_string(),
        detail: format!("spawn worker thread: {e}"),
    })?;

// Wait for init result
match init_rx.recv() {
    Ok(Ok(())) => {}
    Ok(Err(e)) => {
        // Worker spawned but model load failed; join the handle and return.
        let _ = handle.join();
        return Err(e);
    }
    Err(_) => {
        let _ = handle.join();
        return Err(WhisperInitError::ModelLoad {
            path: _config.model_path.display().to_string(),
            detail: "worker thread died before sending init result".to_string(),
        });
    }
}
```

**Note on GPU verification (AD0013):** whisper.cpp emits a log line like `"using CUDA backend"` at init via the `whisper_log_set` callback. whisper-rs exposes this via `set_log_callback`. The full assertion-then-abort flow lands in T13's bake runbook (which adds the operator-visible verification). For T6 we log the gpu_device + path; T13 adds the backend-mismatch abort.

If the implementer wants to do the assertion now (acceptable), wire `whisper_rs::set_log_callback` before `WhisperContext::new_with_params` and parse for "using <backend> backend" / "selected device:" lines. Return `WhisperInitError::BackendMismatch` on cuda-feature-build with non-CUDA backend.

- [ ] **Step 3: Run the failing test from Step 1**

```bash
cargo test --features test-helpers --test whisper_engine_init -- engine_rejects_missing_model_path
```

Expected: PASS — bogus path returns `WhisperInitError::ModelLoad`.

```bash
cargo test --features test-helpers --test whisper_engine_init -- engine_loads_tiny_en_model_successfully
```

Expected: PASS — tiny.en loads, worker placeholder Bug error returns for transcribe, shutdown clean.

- [ ] **Step 4: Run the full Plan A test suite**

```bash
cargo test --features test-helpers
```

Expected: all existing tests still pass; new whisper_engine_init tests also pass.

- [ ] **Step 5: cargo fmt and clippy**

```bash
cargo fmt --all && cargo clippy --all-targets -- -D warnings
```

Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add src/transcribe.rs tests/whisper_engine_init.rs Cargo.toml
git commit -m "$(cat <<'EOF'
feat(transcribe): WhisperEngine::new loads whisper-rs model + logs init

Replaces T5's worker placeholder with real whisper-rs model loading.
The worker thread loads the model on entry, surfaces init errors back
via a sync oneshot before the request loop starts.

Logs gpu_device index, flash_attn flag, and model path at init
(AD0013). Full backend-mismatch assertion lands in T13's bake runbook;
T6 emits the tracing::info! line for operator inspection.

Worker request loop still replies Bug for transcribe; T7 wires the
actual whisper_full_with_state call + raw signal extraction.

Tier 2 tests cover (a) successful load of tiny.en model and (b)
rejection of missing model path. Test gated by test-helpers feature
per AD0005.

Refs: AD0009, AD0013, AD0015 (no whisper_full_parallel — single-state
inference only)

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Self-check

- [ ] `cargo test --features test-helpers --test whisper_engine_init` passes both tests
- [ ] Full Plan A suite still passes
- [ ] `cargo clippy` clean
- [ ] No `unwrap()` in non-test code (init errors propagate via Result)
- [ ] Worker thread is named for debuggability
- [ ] Model load failure does not leak the thread (handle.join() on the error path)
- [ ] The cuda feature is NOT required to compile this test — the test asserts whisper-rs's CPU build works locally
