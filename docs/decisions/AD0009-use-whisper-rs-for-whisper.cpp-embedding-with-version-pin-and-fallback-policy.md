---
adr_id: "0009"
comments:
    - author: Danielle McCool
      comment: "1"
      date: "2026-05-12 13:05:36"
    - author: Danielle McCool
      comment: "2"
      date: "2026-05-12 13:46:33"
links:
    precedes: []
    succeeds: []
status: decided
tags: []
title: Use whisper-rs for whisper.cpp embedding with version-pin and fallback policy
---

## <a name="question"></a> Context and Problem Statement

How do we embed whisper.cpp in the Rust binary, capture per-video confidence signals not emitted by whisper-cli's JSON output, and avoid the per-invocation model load that dominates Plan A's CPU runtime?

## <a name="options"></a> Considered Options

1. <a name="option-1"></a> whisper-rs (out-of-tree Rust binding) with cuda feature, version-pinned to crate version + whisper.cpp commit
2. <a name="option-2"></a> Custom CGO/FFI binding written in this repo
3. <a name="option-3"></a> Other Rust bindings
4. <a name="option-4"></a> Patch whisper-cli's JSON writer for no_speech_prob and stay with subprocess pattern (fallback only)
5. <a name="option-5"></a> Run whisper-server locally and call over HTTP

## <a name="criteria"></a> Decision Drivers

Per-video confidence signals must be captured (token p/plog, no_speech_prob). Model load amortized across a batch. Maintenance cost manageable on a 1-developer project. Fallback path identified if CUDA build fights us. Architecture future-proofs multi-state and multi-GPU per AD0016.

## <a name="outcome"></a> Decision Outcome
We decided for [Option 1](#option-1) because: The C API exposes everything we need (token p/plog, no_speech_prob, language); whisper-rs wraps it 1:1; the README points at it; it actively tracks upstream. Pin the version (both crate and whisper.cpp commit) to keep behavior reproducible across SRC workspace re-provisions. If CUDA build fails after one day of investigation, fall back to Option 4 (patch whisper-cli) as documented in a superseding ADR — do not fall back to custom FFI or HTTP. Rejected alternatives: Option 2 (custom FFI) — maintenance cost on a 1-developer project is prohibitive; whisper.cpp's API surface evolves and tracking it ourselves is wasted effort when whisper-rs already does this. Option 3 (other Rust bindings) — whisper-rs is the upstream-recommended binding (README link) and actively tracked; no other binding has comparable maturity. Option 4 is the documented fallback, not the primary, because subprocess pattern keeps the per-invocation model-load cost Plan A suffers. Option 5 (HTTP server) adds an out-of-process hop, serialization cost, and an extra failure surface; HTTP server is appropriate for cross-machine fan-out (Plan C territory), not single-process single-GPU.

## <a name="comments"></a> Comments
<a name="comment-1"></a>1. (2026-05-12 13:05:36) Danielle McCool: marked decision as decided
<a name="comment-2"></a>2. (2026-05-12 13:46:33) Danielle McCool: Pinned versions (recorded in T2 cargo-deps commit, 2026-05-12): whisper-rs crate version = =0.16.0; whisper.cpp tracked = v1.8.3 (commit 2eeeba56e9edd762b4b38467bab96c2517163158). Upgrade discipline: bump these together (both lines) when whisper-rs releases a new version; re-run the bake measurements and verify they still match prior numbers before merging the bump. Build prerequisite: whisper-rs-sys runs bindgen at build time which requires libclang (Arch: `clang` package); install before first build.
