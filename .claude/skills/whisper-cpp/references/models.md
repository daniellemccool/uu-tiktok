# whisper.cpp — Model variants, quantization, file format

Read this when answering questions about: which Whisper model to use, what quantized variants are available, how to convert/quantize a model, the ggml binary format, distilled models, model RAM/VRAM budgets.

## Pre-built ggml models

Hosted at `huggingface.co/ggerganov/whisper.cpp`. Download via `models/download-ggml-model.sh <variant>`.

| Variant | Disk | RAM (typical) | Notes |
|---|---|---|---|
| `tiny` / `tiny.en` | 75 MB | ~273 MB | English-only `.en` is faster/cleaner for English |
| `base` / `base.en` | 142 MB | ~388 MB | Reasonable for low-stakes English |
| `small` / `small.en` / `small.en-tdrz` | 466 MB | ~852 MB | tdrz = tinydiarize (speaker turn) |
| `medium` / `medium.en` | 1.5 GB | ~2.1 GB | Strong English-only option |
| `large-v1`, `-v2`, `-v3` | 2.9 GB | ~3.9 GB | Multilingual, ~32-layer decoder |
| `large-v2-q5_0`, `large-v3-q5_0` | 1.1 GB | ~1.5 GB | Q5_0 quantized, multilingual |
| `large-v3-turbo` | 1.5 GB | — | Faster decoder (4-layer instead of 32) |
| **`large-v3-turbo-q5_0`** | **547 MB** | ~750 MB | **Sweet spot for multilingual + speed** |

Models are multilingual unless the variant name includes `.en`. `.en` variants train on English-only data and skip the language-id token — faster and slightly more accurate on English at the cost of any other language.

## Picking a model for our pipeline

For a bilingual (EN+NL) pipeline with confidence requirements and an A10-class GPU, `large-v3-turbo-q5_0` is the default-correct choice: ~600 MB on disk, fits with comfortable VRAM headroom for multiple `whisper_state` instances, multilingual (handles Dutch + English in one model), and the "turbo" decoder variant has a 4-layer decoder (vs. 32 in non-turbo large) so decoding is ~4-8× faster while encoder cost is unchanged.

For English-only first studies you *could* use `medium.en` or `small.en`. Practically, sticking with `large-v3-turbo-q5_0` for both phases avoids changing the pipeline mid-study.

## Quantization

Whisper.cpp supports the full ggml quantization spectrum: `Q4_0/1`, `Q5_0/1`, `Q8_0`, plus K-quants. CUDA backend handles them all. CMake option `GGML_CUDA_FORCE_MMQ` chooses MMQ kernels vs. cuBLAS for quantized matmul (default lets the runtime pick).

Quantize from an unquantized binary:
```sh
./build/bin/quantize models/ggml-large-v3-turbo.bin models/ggml-large-v3-turbo-q5_0.bin q5_0
```

Q5_0 is the typical sweet spot — minimal quality loss, ~40% size reduction. Q8_0 is conservative (smaller savings, near-zero quality loss). Q4_0 is aggressive — measurable quality loss especially for ASR (where every token matters more than in chat-style LLMs).

## ggml binary format

`whisper_model_load` (`whisper.cpp:1485`) reads in this order: hparams → mel filters → vocab → weights. Three loading paths:

- `whisper_init_from_file_with_params(path, …)` — read from disk
- `whisper_init_from_buffer_with_params(buffer, size, …)` — read from in-memory buffer
- `whisper_init_with_params(loader, …)` — custom callback (`whisper_model_loader` with `read`/`eof`/`close`)

The custom-loader path is useful for embedded distribution, signed-blob delivery, or pulling from cloud storage without intermediate disk.

## Conversion scripts (`models/`)

- `convert-pt-to-ggml.py` — original OpenAI Whisper `.pt` → ggml
- `convert-h5-to-ggml.py` — Hugging Face Transformers Whisper → ggml. Also works for fine-tuned models.
- `convert-silero-vad-to-ggml.py` — Silero VAD PyTorch → ggml (already done for the shipped models)
- `convert-whisper-to-coreml.py`, `convert-whisper-to-openvino.py` — alternate encoder formats
- `ggml_to_pt.py` — reverse direction (rare)

## Fine-tuned models

Fine-tuned Whisper checkpoints (Hugging Face hosting) convert cleanly with `convert-h5-to-ggml.py`. The README points at this as a documented path. Useful if you ever need domain-specific tuning (TikTok-vernacular, dialectal Dutch, etc.).

## Distilled models

`distil-medium.en`, `distil-large-v2`. Convertible via `convert-h5-to-ggml.py`. **Caveat from `models/README.md`**: chunk-based transcription is not implemented for distilled architectures — sub-optimal quality. Skip these for now; `large-v3-turbo` covers most of the speed gain anyway and is supported properly.

## Stub models for CI

`models/for-tests-*.bin` — zero-weight stubs (~575 KB) used by sanitizers in CI. Don't use for actual inference; they exist only to test that the loader and graph init don't crash on weird inputs.

## Alignment-heads presets (DTW timestamps)

If you turn on `cparams.dtw_token_timestamps`, you must also set `cparams.dtw_aheads_preset` to a model-matching preset (`WHISPER_AHEADS_TINY_EN`, ..., `WHISPER_AHEADS_LARGE_V3_TURBO`). The presets are hard-coded sets of (layer, head) pairs that identify which cross-attention heads correlate with timestamp alignment. Defined at `whisper.cpp:384-410`. Mismatch the preset to the model and DTW timestamps will be garbage.
