---
name: whisper-cpp
description: Reference lookup for the whisper.cpp codebase as it exists at the version vendored in `~/src/whisper.cpp` — public C API, build flags (CUDA/Metal/Vulkan/etc.), CLI/server example specifics, VAD subsystem, confidence/uncertainty signals, sampling and temperature-fallback behavior, model variants and quantization, bindings landscape, and concurrency model. Use when a question concerns whisper.cpp integration in this project (transcription pipeline architecture, model selection, GPU/CUDA flags, confidence extraction, VAD usage, multi-state concurrency, choosing between CLI/server/library) and a precise answer requires consulting the deepdive in this repo rather than a general explanation, since whisper.cpp evolves quickly and generic Whisper knowledge is often wrong about this specific codebase.
---

# whisper.cpp deepdive

A snapshot of how the whisper.cpp codebase at `~/src/whisper.cpp` actually works, written from reading the source. Generic Whisper knowledge is often stale on this specific implementation — when a question hinges on what whisper.cpp does, exposes, or can be configured to do, consult the references below before answering.

## Repo orientation

What lives where, so the file:line citations in the references make sense:

- `include/whisper.h` (751 lines) — the entire public C API. `extern "C"`, C++11 minimum.
- `src/whisper.cpp` (9003 lines) — one giant translation unit holding all of it: model loader, encoder, decoder, sampling, fallback, VAD integration, DTW timestamps, grammars, mel spectrogram. No internal header beyond `src/whisper-arch.h` (tensor name maps).
- `src/coreml/`, `src/openvino/` — optional alternate encoder backends.
- `ggml/` — bundled vendored math/backend library. `ggml/src/ggml-cuda/` is the CUDA kernels. Other backends: `ggml-metal`, `ggml-vulkan`, `ggml-hip`, `ggml-sycl`, `ggml-cann`, `ggml-musa`, `ggml-opencl`, `ggml-rpc`, `ggml-blas`, `ggml-cpu`.
- `examples/` — `cli`, `server`, `bench`, `vad-speech-segments`, `stream`, `command`, `talk-llama`, `quantize`, plus mobile and WASM examples. `cli` and `server` are the production-quality ones.
- `bindings/` — in-tree `go`, `java`, `javascript` (WASM), `ruby`. Rust is **not** in-tree — `tazz4843/whisper-rs` is the community binding.
- `models/` — Python conversion scripts, download scripts, stub models for CI.

## How to use this skill — banded reference pathway

The intended flow is **question → SKILL.md (this file) → one or two `references/` files → source code via file:line citations**. Don't load everything at once. Pick the file that matches the question; each is 30-100 lines and most questions are answered by one file. Cross-references between files are explicit when a topic genuinely spans two.

Each reference is self-contained and includes file:line citations into `~/src/whisper.cpp` for verification. When a claim doesn't match expected behavior, **follow the citation to the source** before trusting the reference.

## Routing table — which file to read

| If the question is about… | Read |
|---|---|
| The C API surface, `whisper_full` pipeline, callbacks, logit processing, lifecycle | `references/api-and-pipeline.md` |
| CMake flags, CUDA build, FFmpeg/CoreML/OpenVINO toggles, lib-only build | `references/build-flags.md` |
| `whisper-cli` flags, `whisper-server` HTTP API, output JSON shapes, bench | `references/cli-and-server.md` |
| Voice activity detection, Silero VAD params, how `params.vad = true` works | `references/vad.md` |
| Per-token confidence, `no_speech_prob`, `avg_logprob`, temperature fallback, greedy vs. beam | `references/confidence-and-sampling.md` |
| Which model to pick, quantization, ggml format, distilled/fine-tuned models | `references/models.md` |
| Calling whisper.cpp from Rust/Go/Python/etc., binding patterns | `references/bindings.md` |
| Parallel transcription, multi-GPU, multi-state on one context, flash attention | `references/concurrency.md` |
| Debugging weirdness, gotchas, defaults that bite, CLI/server output asymmetry | `references/sharp-edges.md` |

When in doubt about which file is relevant, `sharp-edges.md` is often the right second read after the topic-specific file — it surfaces the easy-to-miss caveats.

## Common question shapes — worked examples

- **"How do I extract per-token confidence?"** → `confidence-and-sampling.md` (the signals table and the per-integration availability table). Cross-check with `cli-and-server.md` if the user is using the CLI/server (the CLI omits `no_speech_prob`).
- **"What CUDA flags should I use for an A10?"** → `build-flags.md` (the CUDA build section). Cross-check `concurrency.md` for the flash-attention default and CUDA-graph warmup behavior.
- **"Should I embed whisper.cpp or run the server?"** → `cli-and-server.md` (server concurrency model) + `concurrency.md` (multi-state in-process pattern) + `bindings.md` (if the embedding side is Rust/Python/etc.).
- **"Why does my transcript have hallucinated repeats?"** → `confidence-and-sampling.md` (temperature fallback, entropy guard) + `sharp-edges.md` (prompt history quirks).
- **"How do I run on two GPUs?"** → `concurrency.md` (rules 1-2, the pool pattern at the bottom).
- **"What model should we use for English+Dutch?"** → `models.md` (the variant table, the picking-a-model section).
- **"VAD seems to be cutting off speech"** → `vad.md` (the tunable parameters table, `speech_pad_ms` and `threshold`).

## What this project is doing with whisper.cpp

This repo (`uu-tiktok`) transcribes donated TikTok video audio at scale. The current MVP shells out to `whisper-cli`, which reloads the model per invocation — the dominant inefficiency. Future work (Plan B and beyond) decides the integration architecture: keep CLI-batching, run a `whisper-server` daemon, embed via `whisper-rs`, or run a long-lived child worker. The references are the source-of-truth for the technical constraints behind that decision (per-GPU concurrency via multi-state, confidence-signal availability per integration path, model picking).

For architecture questions on this pipeline, the typical reading path is `concurrency.md` first (sets the in-process vs. process-isolation framing), then `cli-and-server.md` (concrete CLI/server tradeoffs), then `confidence-and-sampling.md` (which signals each path exposes), then `sharp-edges.md` (the gotchas that bite integrations).

## Freshness

The references were written against the version of whisper.cpp vendored at `~/src/whisper.cpp` as of its `git log` HEAD at the time of writing. If you suspect the upstream has changed in a way that matters (new flags, new API surface, removed behavior), `cd ~/src/whisper.cpp && git log --oneline -20` and verify the relevant claim against current source before relying on it.

There's also a single-file consolidated form of the same content at `docs/reference/whisper-cpp-deepdive.md` (human-readable, no progressive disclosure). The `references/` files here are the canonical version the model should consult.
