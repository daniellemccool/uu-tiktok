# whisper.cpp — Concurrency model and the actionable rules

Read this when answering questions about: running parallel transcriptions, using multiple GPUs, the `whisper_state` vs `whisper_context` distinction, flash attention, CUDA graphs, `whisper_full_parallel` (and why it's not what you think).

## The two-handle model — recap

- `whisper_context` = model weights + vocab + (optionally) a default `whisper_state`. Read-only after load. The model lives here.
- `whisper_state` = per-inference scratch. Owns its own backends (`std::vector<ggml_backend_t>`), KV self/cross/pad caches, mel buffer, decoders array, 4 graph schedulers, prompt history, no-speech prob, VAD state. **All the working memory.** Defined at `whisper.cpp:834-935`.

Each call to `whisper_init_state(ctx)` (`whisper.cpp:3374-3545`) allocates a fresh set of backends and KV caches. So multiple states from one context = multiple independent inference lanes that share the model weights.

## The actionable concurrency rules

1. **One context, many states** is the canonical pattern for in-process concurrency. Open with `whisper_init_from_file_with_params_no_state(path, cparams)`, then `whisper_init_state(ctx)` per worker thread. Each state owns its own backends and KV caches. Run `whisper_full_with_state(ctx, states[i], …)` from N threads concurrently — safe because each thread touches its own state.

2. **GPU device selection is per-context**, via `cparams.gpu_device` (an index into the visible GPU devices, `whisper.cpp:1304-1311`). To use 2 GPUs concurrently: open 2 contexts, one with `gpu_device=0` and one with `gpu_device=1`. Same model file, separately loaded into each device's memory. Each context can then have its own states for intra-GPU parallelism.

3. **`whisper_full_parallel` is not a true concurrency tool** — it splits *one audio* across N states with documented quality loss at chunk boundaries (`whisper.cpp:7891`: "the transcription quality may be degraded near these boundaries"). Useful only when the audio is much longer than 30s AND per-file latency matters more than quality. Don't use it for parallel transcription of independent files — spawn proper worker threads with `whisper_full_with_state` instead.

4. **`flash_attn = true`** is the default in `whisper_context_default_params` (`whisper.cpp:3609`) and is supported by the CUDA backend on Ampere+ (`GGML_CUDA_FA = ON` by default in `ggml-cuda/CMakeLists.txt`). Keep it on for A10/A100. Disabling it would cost ~30% on the encoder.

5. **CUDA graphs require a 2-loop warmup** (see `examples/bench/bench.cpp:92-94` comment). The first 30s window of any inference is slower than steady-state because of graph capture. For long-form transcription this amortizes away. For very short (<30s) audio you eat the warmup every time — consider `params.audio_ctx` for short audio (see `confidence-and-sampling.md`).

6. **Inner threading** (`n_threads`) drives mel computation (`whisper.cpp:3212`), parallel sampling across decoders (`whisper.cpp:7242, 7494`), and CPU-backend ops via `ggml_backend_set_n_threads`. On a CUDA build it's mostly irrelevant for throughput because GPU does the heavy lifting — leave at the default unless profiling shows a CPU-bound mel step.

## Memory cost of additional states

Per state, on top of the model weights (which live in the context):
- KV self cache: `n_text_state × n_text_layer × n_text_ctx (256-padded) × itype` (FP16 typically). For large-v3-turbo: ~40 MB.
- KV cross cache: same dims but with `n_audio_ctx` (1500): ~125 MB.
- KV pad cache (flash-attn padded): `n_audio_state × 1 × n_audio_ctx`: ~50 MB.
- Compute buffers (conv/encode/cross/decode allocators): logged per-state at INFO level. Typically a few hundred MB combined.

Total per extra state: roughly 500 MB-1 GB depending on model. On an A10 (24 GB) with `large-v3-turbo-q5_0` (model ~750 MB on GPU), you can comfortably allocate 4-6 states; quality-of-life ceiling is more like 2-3 states before diminishing returns (GPU saturates).

## What "concurrent" actually means on one GPU

Multiple states on the same GPU share the device. The CUDA backend runs them on **independent streams**, so kernels from different states can interleave on the GPU's SMs. You get speedup when individual inferences underutilize the GPU (which Whisper's small batch sizes generally do). Expect 1.3-1.6× throughput from 2 states, 1.5-1.8× from 3 states, then diminishing.

A common sweet spot: 2 states per A10. Beyond that, prefer scaling out to a second GPU.

## Pattern: pool of (context, state) pairs

For a job-queue architecture:

```
contexts = [open_context(gpu_device=i) for i in [0, 1]]
states_per_ctx = 2
workers = []
for ctx in contexts:
    for _ in range(states_per_ctx):
        workers.append(spawn_worker(ctx, init_state(ctx)))
```

4 workers total on 2 A10s. Each worker pulls one audio at a time from the queue, calls `whisper_full_with_state`, writes the transcript, repeats. Model loaded twice (once per GPU), but the per-state overhead is amortized over many files.

## Lifecycle gotcha — `whisper_full` vs `whisper_full_with_state`

`whisper_full(ctx, …)` operates on `ctx->state`. If you opened with `_no_state`, there is no `ctx->state` — you must use `whisper_full_with_state(ctx, state, …)`. Calling `whisper_full` on a no-state context will null-deref or fail at the first state access.

## Process-level isolation (alternative pattern)

If you don't want in-process concurrency complexity, run N copies of `whisper-server` (or a worker binary) on different ports, each with its own `gpu_device`. The client load-balances. Costs: model loaded once per process (N× the memory), HTTP framing per request. Wins: complete isolation, easier crash recovery, language-agnostic clients.

Trade-off vs. in-process pool: process isolation is simpler operationally but more memory-hungry and slower per-request. In-process is denser but you eat any whisper.cpp segfault.

## Inner parallelism — what whisper.cpp does for you

Within a single `whisper_full_with_state` call, the library already uses `std::thread` for:
- Mel computation (parallelized over frames, `whisper.cpp:3212`)
- Sampling across decoders (when `n_decoders > 1`, `whisper.cpp:7242, 7494`)

This is not the concurrency you tune — it's automatic. The concurrency *you* control is "how many states are running `whisper_full_with_state` simultaneously."
