# whisper.cpp — Voice Activity Detection (Silero VAD)

Read this when answering questions about: enabling VAD, tuning VAD thresholds, what `params.vad = true` actually does to the audio, downloading the VAD model, time-mapping for VAD-trimmed timestamps.

## What VAD buys you

Silero VAD is a separate small model (`ggml-silero-v6.2.0.bin`, ~865 KB). Enabling it skips silence before whisper sees the audio. On social-media audio (intros, gaps, pauses) reductions of 30-50% of input duration are typical. The reduction logs as: `Reduced audio from N to M samples (X% reduction)` (`whisper.cpp:6784`).

Get the model:
```sh
./models/download-vad-model.sh silero-v6.2.0
# downloads to models/ggml-silero-v6.2.0.bin
```

## API surface (`whisper.h:678-729`)

Standalone VAD context (separate from `whisper_context`):

- `whisper_vad_init_from_file_with_params(path, ctx_params)` → `whisper_vad_context *`
- `whisper_vad_segments_from_samples(vctx, params, samples, n)` → `whisper_vad_segments *` (start/end pairs in centiseconds)
- Streaming variants: `whisper_vad_detect_speech` / `_no_reset` / `whisper_vad_reset_state`
- Free: `whisper_vad_free_segments`, `whisper_vad_free`

`whisper_vad_context_params` (`whisper.h:682-688`): `n_threads`, `use_gpu`, `gpu_device`. **VAD can run on GPU separately from whisper** (or on CPU while whisper is on GPU — it's small enough that CPU is fine).

## Tunable parameters — `whisper_vad_params` (`whisper.h:192-199`)

| Field | Default | What it does |
|---|---|---|
| `threshold` | 0.5 | Probability above which a frame is speech |
| `min_speech_duration_ms` | 250 | Drop speech segments shorter than this (filters brief noise) |
| `min_silence_duration_ms` | 100 | Silence shorter than this doesn't end a segment |
| `max_speech_duration_s` | FLT_MAX | Force-split speech longer than this at silence points >98ms |
| `speech_pad_ms` | 30 | Padding before/after each detected segment (avoid clipping speech edges) |
| `samples_overlap` | 0.1 (s) | Overlap when copying samples between adjacent segments (avoids hard cuts in the concatenated buffer) |

For TikTok-style social audio, the defaults are reasonable. Two parameters worth tuning if you see hallucinations or dropped speech:

- Bump `speech_pad_ms` to 50-100 if early/late words get clipped
- Lower `threshold` to 0.3-0.4 if quiet/whispered speech is being dropped
- Set `max_speech_duration_s` to ~30 to align segment boundaries with whisper's 30s window (avoids whisper having to internally re-split)

## Inline integration when `params.vad = true`

Implementation: `whisper_vad` at `whisper.cpp:6630-6790`. The flow when `whisper_full` is called:

1. Lazily creates a VAD context the first time, reuses across `whisper_full` calls (lives on `state->vad_context`)
2. Calls `whisper_vad_segments_from_samples` to detect speech ranges
3. Builds a NEW audio buffer that **concatenates speech segments separated by 0.1s of zero-silence padding** (the silence is real silence, not the original audio between segments)
4. Builds `state->vad_mapping_table` of `(processed_time, original_time)` pairs at every segment boundary
5. Whisper runs over the trimmed audio
6. Output timestamps are remapped via `map_processed_to_original_time` (`whisper.cpp:7912`) — so segment timestamps you read out are in original-audio coordinates, even though whisper internally saw the trimmed buffer

The mapping table is sorted and de-duplicated for binary search (`whisper.cpp:6766-6779`).

## Two integration paths

### Path 1: inline (`params.vad = true`)

Set `params.vad = true`, `params.vad_model_path = "models/ggml-silero-v6.2.0.bin"`, optionally tune `params.vad_params`. `whisper_full` does everything. **This is the right path for most use cases.**

### Path 2: external VAD, fed to whisper manually

Run VAD yourself (`whisper_vad_segments_from_samples`), iterate the segments, call `whisper_full_with_state` per segment with that segment's audio. Useful when you want to do per-segment decisions (e.g., skip segments under a duration threshold, route to different models, parallelize across states).

The standalone tool `examples/vad-speech-segments/` is the reference implementation for path 2 — it just outputs the speech segments without transcribing them.

## Order of operations in `whisper_full`

VAD runs **first**, before mel computation, before language detection. The whole pipeline downstream operates on the trimmed buffer. Language auto-detect therefore sees only speech-bearing audio (good — less ambiguity from silent intros).
