---
adr_id: "0012"
comments:
    - author: Danielle McCool
      comment: "1"
      date: "2026-05-12 13:06:20"
    - author: Danielle McCool
      comment: "2"
      date: "2026-05-12 13:18:52"
links:
    precedes: []
    succeeds: []
status: decided
tags: []
title: Cooperative cancellation via per-request Arc AtomicBool and abort callback
---

## <a name="question"></a> Context and Problem Statement

Plan A's whisper-cli subprocess could be killed via SIGTERM/SIGKILL with bounded latency. Embedded whisper-rs runs inside our process and cannot be killed externally. How do we implement per-call timeout and operator-initiated cancellation?

## <a name="options"></a> Considered Options

1. <a name="option-1"></a> Per-request Arc<AtomicBool> flag built fresh per transcribe call, dropped with the request; polled by FullParams::abort_callback. Worker thread sets the flag when the deadline elapses.
2. <a name="option-2"></a> Engine-level cancellation flag (single Arc<AtomicBool> on the WhisperEngine struct, reset per call)
3. <a name="option-3"></a> No cancellation in Epic 1; accept that inference can hang past its budget

## <a name="criteria"></a> Decision Drivers

Must allow per-call timeout enforcement. Must not leak cancellation across requests. Must integrate with whisper-rs's abort_callback mechanism. Must remain compatible with Epic 2's state-machine reclassification.

## <a name="outcome"></a> Decision Outcome
We decided for [Option 1](#option-1) because: Per-request flag eliminates the cross-request leak codex-advisor flagged in second-pass review: a late timeout from request A cannot cancel request B if the flag belongs to A. Mechanics: each TranscribeRequest carries cancel: Arc<AtomicBool> and deadline: Instant. Worker spawns a timer that flips cancel when Instant::now() > deadline. FullParams::abort_callback polls cancel. On flip-to-true, whisper-rs returns from whisper_full_with_state and worker replies Err(TranscribeError::Cancelled). Epic 1 fail-fast: Cancelled propagates up through pipeline::process_one and process exits non-zero (matches Plan A's transcribe-failure behavior). Epic 2's state-machine work reclassifies Cancelled into proper retryable/terminal columns. Rejected alternatives: Option 2 (engine-level flag) has a race condition — a late timer from request A can flip the engine flag while request B is mid-inference, cancelling B with no protection; reset-per-call doesn't close the window because reset and check are not atomic with respect to the timer thread. Option 3 (no cancellation) leaves a hung process if whisper.cpp enters a pathological state (e.g., the lookahead-sampler bug surfaced in upstream issues); operator has no recourse short of SIGKILL on the whole binary, losing in-progress claims on other workers.

## <a name="comments"></a> Comments
<a name="comment-2"></a>2. (2026-05-12 13:18:52) Danielle McCool: Mechanics refinement (from codex code-quality review of T1): prefer checking Instant::now() >= deadline OR cancel.load(Ordering::Relaxed) inside FullParams::abort_callback over spawning a separate timer task — the callback fires frequently during whisper.cpp's encoder/decoder loop, so per-call timeout enforcement is already covered by polling the deadline there. The cancel: Arc<AtomicBool> remains in TranscribeRequest so operator-initiated cancellation can flip it asynchronously. T7 implements this single-callback shape. Operator-initiated path: an external signal flips cancel; abort_callback returns true on next poll; whisper-rs unwinds from whisper_full_with_state; worker replies Err(TranscribeError::Cancelled); pipeline awaits the reply before treating the request as finished — do not drop the oneshot prematurely.
