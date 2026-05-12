---
adr_id: "0015"
comments:
    - author: Danielle McCool
      comment: "1"
      date: "2026-05-12 13:07:05"
links:
    precedes: []
    succeeds: []
status: decided
tags: []
title: Explicit non-use of whisper-full-parallel
---

## <a name="question"></a> Context and Problem Statement

whisper.cpp's whisper_full_parallel (whisper.cpp:7891) is named as if it parallelizes inference. Should we use it?

## <a name="options"></a> Considered Options

1. <a name="option-1"></a> No — it splits one audio across N states with documented quality loss at chunk boundaries (sharp-edges.md:45); not a parallel-transcription tool
2. <a name="option-2"></a> Yes — use it for short audio where chunk-boundary quality loss is acceptable

## <a name="criteria"></a> Decision Drivers

Research data quality is non-negotiable. Preventing future-reader confusion about the function name.

## <a name="outcome"></a> Decision Outcome
We decided for [Option 1](#option-1) because: Cited verbatim from sharp-edges.md:45 — the transcription quality may be degraded near these boundaries. For research data we cannot accept this quality loss. Documented as a non-decision so future readers don't reach for it under the assumption it's the right tool. For per-video parallelism we use multiple WhisperStates on one context (concurrency.md); for multi-video parallelism we use channel-based orchestration (Epic 2). Rejected alternative: Option 2 (use it for short audio) — TikTok videos are short but research data quality is non-negotiable; trading correctness for a marginal throughput gain on a single-A10 dev grant is the wrong trade. Even if it were the right trade for some future workstream, that decision belongs in its own ADR with its own evidence, not as a permissive default here.

## <a name="comments"></a> Comments
<a name="comment-1"></a>1. (2026-05-12 13:07:05) Danielle McCool: marked decision as decided
