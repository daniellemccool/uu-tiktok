# whisper.cpp — Confidence signals and sampling/fallback behavior

Read this when answering questions about: how to extract per-token / per-segment confidence, what `no_speech_prob` means, how the temperature fallback ladder works, greedy vs. beam, computing `avg_logprob` or compression ratio.

## Confidence / uncertainty signals — what's available and where

| Signal | Source | Granularity |
|---|---|---|
| `whisper_token_data.p` | `whisper_full_get_token_data` / `_get_token_p` | Per token |
| `whisper_token_data.plog` | same | Per token (log-prob) |
| `whisper_token_data.pt`, `.ptsum` | same | Per timestamp token |
| `no_speech_prob` | `whisper_full_get_segment_no_speech_prob` (`whisper.h:745`) | Per segment |
| Detected language | `whisper_full_lang_id(ctx)` | Per inference |
| Language probabilities | `whisper_lang_auto_detect`'s `lang_probs[]` buffer (size = `whisper_lang_max_id()+1`) | Per inference (re-encodes!) |
| `avg_logprob` | sum of `token.plog` / `n_tokens` over a segment | Per segment (you compute) |
| Compression ratio | not exposed; compute from segment text | Per segment (you compute) |
| Fallback failure counters | `state` fields `n_fail_p`, `n_fail_h` (`whisper.cpp:847-848`); shown in `whisper_print_timings` | Per inference (cumulative) |
| Per-decoder score | `decoder.sequence.score`, `.entropy`, `.avg_logprobs`; only logged at debug | Internal; surface via `-ls` CLI |

## Per-integration availability — important asymmetry

| Path | Per-token `p` | `no_speech_prob` | `avg_logprob` | Lang probs |
|---|---|---|---|---|
| C API | ✓ | ✓ | compute it | ✓ |
| `whisper-cli` `-ojf` | ✓ | ✗ (missing!) | ✗ (missing!) | ✗ |
| `whisper-server` `verbose_json` | ✓ | ✓ | ✓ | ✓ (set `no_language_probabilities=false`) |

**If you need `no_speech_prob` per segment**, you must use the C API or the server's `verbose_json`. The CLI's `--output-json-full` omits it (TODO marker at `server.cpp:1104` confirms compression_ratio also isn't computed anywhere — only the server exposes `no_speech_prob`).

## Per-video aggregate confidence — practical recipe

For "how sure are we about this transcript" as one or two scalars per file:

1. **Mean token log-probability**: average `token.plog` across all tokens (excluding special tokens with `id >= whisper_token_eot(ctx)`). Equivalent to `avg_logprob` from OpenAI Whisper's segment output. Higher (closer to 0) = more confident.
2. **Mean `no_speech_prob`** weighted by segment duration, or fraction of segments with `no_speech_prob > 0.6`. Tells you what fraction of the audio is silence/noise.
3. **Language detection probability**: read `lang_probs[detected_lang_id]` after auto-detect. Below ~0.7 suggests mixed/ambiguous audio.
4. **Fallback counter ratio**: `n_fail_p / n_segments` and `n_fail_h / n_segments` from `whisper_get_timings`. High values mean many windows needed temperature fallback — strong signal of poor audio quality.

Cheap quality flag: any of (`avg_logprob < -1.0`, mean `no_speech_prob > 0.4`, lang_prob < 0.7) → mark transcript as low-confidence.

## Sampling strategies (`whisper.h:455-458`)

- `WHISPER_SAMPLING_GREEDY` — uses `params.greedy.best_of` (default 5). At T=0, single greedy decode; at T>0 (fallback), `best_of` independent decodes.
- `WHISPER_SAMPLING_BEAM_SEARCH` — uses `params.beam_search.beam_size` (default 5) at T=0, switches to `best_of` at T>0. Patience param exists in struct but TODO-not-implemented.

`n_decoders = max(1, max(best_of, beam_size))`, capped at `WHISPER_MAX_DECODERS` (`whisper.cpp:6862-6881`).

**Memory cost grows with decoder count.** The KV self-cache is over-allocated by `n_decoders + 2` factor (`whisper.cpp:7128`) to work around fragmentation. So beam_size=5 takes ~7× the KV memory of pure greedy. For high-throughput pipelines on bounded VRAM, prefer greedy with low `best_of` (1 or 2).

## Temperature fallback ladder

Schedule: `[temperature, temperature+inc, …, < 1.0]`. With defaults that's `[0.0, 0.2, 0.4, 0.6, 0.8]`.

Triggered when, for the best decoder in the current window (`whisper.cpp:7548-7571`):
- decoder marked failed (entropy guard tripped — repetition loop), OR
- `avg_logprobs < logprob_thold` AND `no_speech_prob < no_speech_thold`

The conjunction matters: if `no_speech_prob` is already high (probably silence), we don't fall back — we accept the failure and let the segment be suppressed downstream.

Set `temperature_inc = 0` (CLI `-nf` / `--no-fallback`) to disable the ladder entirely. **Don't do this in production** — fallback is the main repetition-hallucination escape hatch.

## Repetition / entropy guard

`whisper.cpp:7527`: if `result_len > 32 && entropy < entropy_thold` (default 2.4), the decoder is marked failed. This catches the classic "repeating the same phrase forever" Whisper failure mode. Counter is `state->n_fail_h`.

## Quality cliff: prompt history

`params.no_context = true` (default) keeps each 30s window independent. Setting it to false enables prompt-history conditioning across windows (better long-form continuity) but risks cascading hallucinations (a hallucination in window N becomes the prompt for window N+1).

The internal `WHISPER_HISTORY_CONDITIONING_TEMP_CUTOFF = 0.5f` (`whisper.cpp:145, 7090`) drops prompt history at high fallback temperatures — even if you allow context, once we hit T≥0.5 the model is signaling "this is hard," so we stop poisoning the next window with the troubled context.

## Speed-up knobs that cost quality

- `params.audio_ctx` (`-ac`): shrinks the encoder's audio context dimension. For audio shorter than 30s this skips wasted encoder work. Speed-up but accuracy hit. Don't use for arbitrary-length audio.
- Disabling fallback (`-nf`): faster but no escape from repetition loops.
- Reducing `best_of` / `beam_size`: linear speed and memory win. `best_of=1` is the floor.
- Smaller model: see `models.md`.
