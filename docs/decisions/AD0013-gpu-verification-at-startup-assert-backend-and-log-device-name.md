---
adr_id: "0013"
comments:
    - author: Danielle McCool
      comment: "1"
      date: "2026-05-12 13:06:38"
links:
    precedes: []
    succeeds: []
status: decided
tags: []
title: GPU verification at startup assert backend and log device name
---

## <a name="question"></a> Context and Problem Statement

Per sharp-edges.md:60-61, whisper.cpp silently falls back to CPU at ~100× slower throughput if GPU backend initialization fails, and gpu_device = N silently picks the wrong GPU if CUDA_VISIBLE_DEVICES is misconfigured. How do we prevent the bake from being meaningless due to silent CPU fallback?

## <a name="options"></a> Considered Options

1. <a name="option-1"></a> Assert at WhisperEngine::new: scan the tracing log emitted by whisper.cpp's init for 'using <backend> backend' and abort if it's not the expected CUDA backend; log gpu_device index and device name
2. <a name="option-2"></a> Defer to bake-time verification only (operator inspects logs manually during bake)
3. <a name="option-3"></a> No verification — trust that build flags worked

## <a name="criteria"></a> Decision Drivers

Must run at startup, not at first transcribe. Must produce a clear log line for operator audit. Must abort hard on mismatch.

## <a name="outcome"></a> Decision Outcome
We decided for [Option 1](#option-1) because: Cost is small (parse one log line at startup); value is large (catches silent CPU fallback that would invalidate every benchmark and waste a workspace session). Mechanics: whisper-rs emits an init log via the C library's whisper_log_set callback. WhisperEngine::new wires a callback that captures the backend identifier and device name, asserts the backend matches expected (CUDA when the cuda feature is enabled), and emits a tracing::info! line with captured values. If mismatch, return WhisperInitError::BackendMismatch and abort. Rejected alternatives: Option 2 (defer to bake-time manual inspection) — relies on a human noticing the absence of a log line, which is exactly the failure mode (silent fallback produces a regular-looking run, just 100× slower); a single missed bake wastes a workspace-day. Option 3 (no verification) — same failure mode, formalized as policy; the build-flag trust assumption fails the moment whisper.cpp's CUDA detection code changes upstream or the workspace's CUDA toolkit drifts.

## <a name="comments"></a> Comments
<a name="comment-1"></a>1. (2026-05-12 13:06:38) Danielle McCool: marked decision as decided
