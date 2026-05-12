---
adr_id: "0014"
comments:
    - author: Danielle McCool
      comment: "1"
      date: "2026-05-12 13:06:53"
links:
    precedes: []
    succeeds: []
status: decided
tags: []
title: Audio-input invariant float32 PCM 16kHz mono via hound
---

## <a name="question"></a> Context and Problem Statement

whisper.cpp's C API takes float32 PCM at 16 kHz mono (api-and-pipeline.md:7). Plan A produces 16 kHz mono WAV via yt-dlp's ffmpeg postprocessor. Embedding requires decoding the WAV in-process. What decoder and what validation?

## <a name="options"></a> Considered Options

1. <a name="option-1"></a> hound crate (small, focused on PCM WAV; no no_std complications) with explicit format validation
2. <a name="option-2"></a> symphonia crate (general audio decoding; supports MP3/FLAC/etc.; heavier)
3. <a name="option-3"></a> Custom WAV parser
4. <a name="option-4"></a> ffmpeg via subprocess

## <a name="criteria"></a> Decision Drivers

Lightweight dependency. Explicit format validation. Produces owned Vec<f32> for cross-thread transport (per AD0016 worker-thread invariants).

## <a name="outcome"></a> Decision Outcome
We decided for [Option 1](#option-1) because: Validate WAV header on every load: sample_rate == 16000, channels == 1, sample format is f32 (or i16 converted to f32). Reject non-conforming inputs with a typed error. Mechanics: small decode_wav(path: &Path) -> Result<Vec<f32>, AudioDecodeError> helper. Reads header, validates format, decodes samples. Returns owned Vec<f32> ready to ship across the worker boundary (per worker-thread invariants from AD0012/AD0016). Rejected alternatives: Option 2 (symphonia) is overkill for a pinned input format — pulling in MP3/FLAC/Vorbis decoders adds dependency weight and compile time for no benefit when our upstream (yt-dlp's ffmpeg postprocessor) emits exactly one format. Option 3 (custom WAV parser) is error-prone; the WAV format has corner cases (RIFX byte order, non-PCM subchunks, extension chunks) that a 1-developer project should not maintain. Option 4 (ffmpeg subprocess) re-introduces the per-invocation subprocess overhead that Plan B is explicitly removing, and brings back the binary-availability failure surface.

## <a name="comments"></a> Comments
<a name="comment-1"></a>1. (2026-05-12 13:06:53) Danielle McCool: marked decision as decided
