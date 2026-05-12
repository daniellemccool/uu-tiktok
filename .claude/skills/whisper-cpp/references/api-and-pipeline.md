# whisper.cpp — Public C API and the `whisper_full` pipeline

Read this when answering questions about: the C API surface, context vs. state handles, lifecycle (init/free), the encoder/decoder pipeline, callbacks, prompt history, logits processing.

## Constants

16 kHz mono float32 audio. `WHISPER_SAMPLE_RATE=16000`, `WHISPER_N_FFT=400`, `WHISPER_HOP_LENGTH=160`, `WHISPER_CHUNK_SIZE=30`. The encoder always processes 30 s windows.

## Two-level handle model

- `whisper_context` = model weights + vocab + a default `whisper_state`. Read-only after load.
- `whisper_state` = per-inference scratch: KV self/cross/pad caches, mel buffer, batch, **N decoders** (`WHISPER_MAX_DECODERS`), 4 graph schedulers (conv/encode/cross/decode), encoder output cache, prompt history, no-speech prob, VAD state, DTW alignment buffers, and **its own `std::vector<ggml_backend_t>`**. Defined `whisper.cpp:834-935`.

The library is thread-safe iff you don't share a `whisper_state` across threads (API doc at `whisper.h:45-47`). Concurrency rules in `concurrency.md`.

## Lifecycle functions

`whisper_init_from_file_with_params`, `…_from_buffer_with_params`, `…_with_params` (custom loader callback). Each has a `_no_state` variant. Plus `whisper_init_state`, `whisper_free`, `whisper_free_state`, `whisper_free_params`, `whisper_free_context_params`.

## The pipeline — `whisper_full`

`whisper_full(ctx, params, samples, n_samples)` (`whisper.cpp:7743`). Internally:

1. If `params.vad`, run `whisper_vad()` first (shrinks samples, builds time-mapping table — see `vad.md`)
2. Compute mel via `whisper_pcm_to_mel_with_state` (CPU, parallel)
3. If language unset/`"auto"`, call `whisper_lang_auto_detect_with_state` → fills `lang_probs[]`, sets `state->lang_id`
4. Outer loop in 30 s windows (`whisper_full_with_state`, `whisper.cpp:6792-7741`):
   - `whisper_encode_internal`
   - First decode pass with the prompt → captures `no_speech_prob` from the SOT logits *before* any logit filtering (`whisper.cpp:7158-7161`)
   - Inner sampling loop, up to `n_text_ctx/2 - 4` iterations, with `n_decoders_cur` parallel decoders (`whisper.cpp:7184`). Sampling and logit-processing are parallelized across decoders via `std::thread`
   - Score sequences, reject low-entropy ones (`entropy_thold`, repetition guard at `whisper.cpp:7527`)
   - **Temperature fallback** (`whisper.cpp:7548-7571`): if `failed || (avg_logprobs < logprob_thold && no_speech_prob < no_speech_thold)`, retry at next `temperature` from the schedule `[t0, t0+inc, t0+2·inc, … < 1.0]`. See `confidence-and-sampling.md`.
   - On success, walk tokens, split on timestamp tokens, emit `whisper_segment` records into `result_all`, fire `new_segment_callback`
5. Output via `whisper_full_get_segment_*` and `whisper_full_get_token_*` getters

## Callbacks (set on `whisper_full_params`)

- `new_segment_callback(ctx, state, n_new, ud)` — fires per emitted segment (NOT in DTW mode where it fires per chunk)
- `progress_callback(ctx, state, %, ud)` — at start of each 30 s window
- `encoder_begin_callback(ctx, state, ud) → bool` — return false to abort before encoder runs
- `abort_callback(ud) → bool` (ggml-level) — checked frequently during graph compute
- `logits_filter_callback(ctx, state, tokens, n, logits, ud)` — modify logits after temperature scaling, before final logit suppression rules

## Logits processing

`whisper.cpp:6164-6432`, `whisper_process_logits` applies, in order: temperature scaling → suppress_blank (initial only) → suppress `<|notimestamps|>` and lang/task tokens → user `logits_filter_callback` → `suppress_regex` → `suppress_nst` → timestamp pairing constraints → `max_initial_ts` → monotonic timestamps → softmax → timestamp-vs-text decision (logsumexp comparison) → grammar suppression. Mirrors OpenAI `whisper/decoding.py`.

## Defaults that matter

`whisper_full_default_params` (`whisper.cpp:5915-6021`):

- `language = "en"` — change to `"auto"` for multilingual
- `n_threads = min(4, hw_concurrency)`
- `no_context = true` — clears prompt history at start of each `whisper_full` call
- `temperature = 0`, `temperature_inc = 0.2`, `entropy_thold = 2.4`, `logprob_thold = -1.0`, `no_speech_thold = 0.6` — same as OpenAI Whisper
- `greedy.best_of = 5`, `beam_search.beam_size = 5` (when chosen)
- `print_progress = true` — set false when embedding
- `suppress_blank = true`, `suppress_nst = false`
- `vad = false`, `vad_params = whisper_vad_default_params()`

`whisper_context_default_params` (`whisper.cpp:3606-3622`): `use_gpu = true`, `flash_attn = true`, `gpu_device = 0`. So GPU+FA out of the box.
