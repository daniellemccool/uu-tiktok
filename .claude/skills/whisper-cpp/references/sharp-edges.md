# whisper.cpp — Sharp edges and surprises

Read this when debugging unexpected behavior, before relying on a non-obvious assumption, or when an integration produces results that don't match documentation.

Each item is something I found by reading the source that would have bitten me if I hadn't.

## Audio length

- **Audio under 100 ms returns 0 with a warning** (`whisper.cpp:6846-6849`). No error, no segments. If you're seeing empty `result_all` for short clips, check audio length first.

## Language detection

- **`whisper_lang_auto_detect` re-encodes the audio.** Calling it manually before `whisper_full` doubles encoder cost (`whisper.cpp:4040`). If you want auto-detect, set `params.language = "auto"` and let `whisper_full_with_state` do it inline — it uses the existing mel (`whisper.cpp:6815`).
- **`params.detect_language = true`** runs auto-detect and returns 0 immediately without transcribing (`whisper.cpp:6824-6826`). Useful for routing audio to different downstream pipelines by language.
- **`whisper_lang_auto_detect_with_state` clobbers state**. It reuses `state->decoders[0]` and `state->logits`. Don't call it concurrently with a transcription on the same state.

## State lifecycle

- **`whisper_full_with_state` clears `result_all` on entry** (`whisper.cpp:6801`). State reuse across calls is safe; results from the previous call are gone. Drain them before the next call.
- **`whisper_full(ctx, …)` operates on `ctx->state`**. If you opened with `_no_state`, you must use `whisper_full_with_state` — the no-state ctx has no default state. See `concurrency.md`.
- **Each `whisper_init_state` allocates fresh GPU backends.** Per-state cost is 500 MB-1 GB on a large model. Don't allocate states speculatively.

## Prompt history

- **`prompt_past` clears at end-of-audio tail** (within last 5 s, `whisper.cpp:7027-7030`). The model trying to fit too little remaining audio into rolling context hallucinates repeats; whisper.cpp pre-empts this.
- **History is dropped at high fallback temperature** — `WHISPER_HISTORY_CONDITIONING_TEMP_CUTOFF = 0.5f` (`whisper.cpp:145, 7090`). Even if `no_context = false`, once we hit T≥0.5 the rolling prompt is discarded.

## Model quirks

- **First-release distilled models force `no_timestamps = true`** with a warning — detected by `n_text_layer == 2 && n_vocab != 51866` (`whisper.cpp:6967-6974`). You cannot get timestamps out of these distilled checkpoints.
- **Alignment-heads preset must match the model** for DTW timestamps. Wrong preset → garbage token timestamps. Presets at `whisper.cpp:384-410`.

## KV cache and decoder count

- **`whisper_kv_cache` over-allocates by `n_decoders + 2`** when `n_decoders > 1` (`whisper.cpp:7128`) — fragmentation workaround. So beam_size=5 takes ~7× the KV memory of greedy. Memory-bounded? Prefer greedy with low `best_of`.

## CLI / server output asymmetry

- **The CLI's `-ojf` JSON output omits `no_speech_prob`** and `avg_logprob`. Only the server's `verbose_json` includes them (`server.cpp:1102, 1106`). If you want them from CLI you patch the JSON writer (a few lines) or compute via the C API directly.
- **Compression ratio** (Whisper's classic third confidence indicator) is not computed by whisper.cpp anywhere. `entropy_thold` plays a related but different role internally (cumulative entropy threshold over decoded tokens, used for fallback decisions). TODO marker confirms this at `server.cpp:1104`. Compute compression ratio yourself from segment text if you need OpenAI-Whisper parity.
- **Server default params differ from CLI**: `best_of = 2` (vs. CLI's 5), `beam_size = -1` (greedy), `max_len` defaults to 60 if 0 was passed. Don't assume CLI and server produce identical output from "equivalent" inputs.

## Concurrency

- **`whisper_full_parallel` is not parallel transcription.** It splits *one* audio across N states with documented quality loss at chunk boundaries (`whisper.cpp:7891`). For independent files, spawn worker threads with `whisper_full_with_state` instead. See `concurrency.md`.
- **The server is single-threaded per process.** `std::mutex whisper_mutex` (`server.cpp:627, 807`) wraps every inference. For multi-GPU, run multiple server processes.

## VAD

- **VAD context is created lazily inside `whisper_full`** (`whisper.cpp:6644-6652`) the first time `params.vad = true` is seen, and persists on `state->vad_context` across calls. So model load happens at first transcription, not at `whisper_full_params` setup.
- **VAD output is fed back as a re-spliced audio buffer with 0.1s silence between segments.** Timestamps are remapped via `vad_mapping_table`. Don't assume internal whisper segment boundaries align with VAD segment boundaries.

## CUDA-specific

- **CUDA graph capture causes a 2-loop warmup penalty.** See `bench.cpp:92-94` comment. For long-form transcription this amortizes; for very short audio it's a real cost.
- **Flash attention requires Ampere+ (sm_80 minimum).** It's on by default. Disabling it on a supported GPU is leaving ~30% on the table for the encoder.

## Backend selection

- **`gpu_device = N` selects the Nth visible GPU device of `_TYPE_GPU` or `_TYPE_IGPU` type** (`whisper.cpp:1297-1311`), in enumeration order. If your `CUDA_VISIBLE_DEVICES` doesn't include the intended GPU, `gpu_device` will silently pick the wrong one. Verify with the `INFO: device N: <name>` log line at init.
- **CPU backend is always added as a fallback** (`whisper.cpp:1352-1356`). If the GPU backend fails to initialize, you silently get CPU inference at ~100× slower throughput. Check the `using <backend> backend` log line at init.

## Logging

- **`whisper_log_set` sets a process-global callback**, not per-context. The global `g_state` only holds this callback — there's no other shared mutable state. So you can't have different log destinations per context.
- **`params.print_progress = true` by default** (`whisper.cpp:5929`). For embedding, set to false or you'll get progress prints on stderr.
- **`params.print_realtime` prints inside `whisper_full` via `printf` to stdout** (`whisper.cpp:7635`). Never enable this in a server context; use `new_segment_callback` instead.
