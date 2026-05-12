# UU TikTok Pipeline — Plan B Design

Date: 2026-05-12
Status: **Draft (mid-brainstorm, revision 3)** — converged after two codex-advisor passes + whisper-cpp skill verification. Awaiting user final review before transition to writing-plans.
Owner: Danielle McCool
Reviewer: codex-advisor (pinned session, two passes); user (final review)

## Revision summary

- **Revision 1**: initial draft from brainstorm conversation (Approach A, Approach 1 epic structure, raw pass-through rule, single-A10 dev target).
- **Revision 2**: applied 13 adjustments from codex-advisor first review + whisper-cpp skill verification. Direction-changers: dedicated transcribe worker thread from Epic 1 (no `spawn_blocking`); Epic 2 absorbs minimum state-machine work; `lang_probs` opt-in via `--compute-lang-probs`; schema_version on the raw_signals object.
- **Revision 3 (this version)**: applied 11 adjustments from codex-advisor second review + user clarification on parallelism. Tightening, not direction change. Per-request cancellation (not engine-level); schema-version handling moves to *first task* of Epic 2; Epic 1 preserves Plan A's fail-fast on TranscribeError; AD0008 stale-recovery decision made explicit (redo, not validate); worker-thread invariants codified; concurrent fetch hardening and claim contention semantics folded into Epic 2; bake plan success criteria sharpened; 1-state vs 2-state measurement added; operational "done" contract drafted; **architecture explicitly future-proofed for multi-state / multi-GPU parallelism** (production-grade upgrade path, single-A10 dev default).

## Context

Plan A — the walking skeleton — is complete on `feat/plan-a-walking-skeleton`. 76 tests passing, 8 ADRs (AD0001–AD0008) capturing process and structural decisions, ~20 deferred entries in `docs/FOLLOWUPS.md`. Plan A's serial loop (`init` → `ingest` → `process`) runs end-to-end with real tools, has been deployed once to SURF Research Cloud on a CPU-only workspace using the `tiny.en` model, and produced real transcripts for fixture data.

Two new contextual changes since Plan A retro:

1. **SURF Small Compute Grant approved** (Workstream 1: ~100 GPU-hours of an expected 1500 GPU-h total; 15K CPU-core-h; 1 TB storage; 12 months). The grant covers *development and evaluation*, not production runs. Production processing of the actual ~1M-video study will run on the relevant researcher's separate grant — this project builds the tool, not the production batches.

2. **whisper.cpp deepdive** (`docs/reference/whisper-cpp-deepdive.md` and the banded form at `.claude/skills/whisper-cpp/`) consolidated mental model of the C API, build flags, CLI vs server vs library trade-offs, VAD subsystem, confidence/uncertainty signals, sampling controls, model variants, and concurrency model. Made it possible to reason precisely about where engineering effort should land.

Two operational facts surfaced this session that shaped framing:

- Plan A's prior SRC deployment burned ~133 CPU-core-hours over 2.5 idle days. Scaling: the dev grant's 15K CPU-core-h budget is over-spent before any real work if a workspace sits 24/7. Conclusion: **"spin down between batches" is operationally mandatory** regardless of which transcribe architecture we pick.
- The original `PLAN-B-KICKOFF-PROMPT.md` (2026-04-17) framed Plan B around failure-classification-first. The deepdive plus grant approval shifted the gravity of the problem: efficiency-first is now the load-bearing priority. The original agenda items are not dropped — they're sequenced after the efficiency thin-slice, *with the minimum state-machine work needed to make Epic 2's pipelined orchestrator recoverable folded back into Epic 2 itself*.

## Framing decisions (this session)

**Decided. Plan B = efficiency first, but nothing lost from the original Plan B agenda.** Treated as one plan with multiple epics. Epic 1 produces the efficiency thin-slice (per RETRO's MVP-first guidance); Epic 2 absorbs the minimum failure-state-machine work needed to make pipelining recoverable plus the pipelined orchestrator itself; Epic 3 covers full failure classification taxonomy; subsequent epics absorb time-window filter, multi-fetcher provenance, sync-IO sweep, bin/lib reassessment.

**Decided. Approach A (embed whisper-rs)** chosen over Approach 0 (multi-file CLI batching). Driver: per-video confidence signals are required, and the CLI's `--output-json-full` does not emit `no_speech_prob` (sharp-edges.md:39 confirms). Approach 0 would require maintaining a fork of whisper.cpp — a strictly worse engineering bet than embedding via the Rust binding the project README explicitly points at. Approach 0 is retained mentally as a fallback in case whisper-rs CUDA build fights us during Epic 1 (see Risks).

**Decided. Per-video confidence signals required; per-segment data only if it's truly free.** Plan B's transcript artifact carries the confidence data the C API natively produces (token `p`/`plog`, segment `no_speech_prob`, `language` from `whisper_full_lang_id`).

Verified against the whisper-cpp skill: `lang_probs[]` — the per-language probability distribution — is *not* freely available after `whisper_full`. The buffer is filled internally by `whisper_lang_auto_detect_with_state` but there's no public getter exposing it back to the caller. Acquiring the distribution requires a separate `whisper_lang_auto_detect` call which re-encodes the audio (sharp-edges.md:13). Therefore: by default, `raw_signals` carries only the single detected `language` string. A `--compute-lang-probs` config flag opts into the extra encoder pass when the researcher wants the full distribution.

**Decided. Pass-through, not pre-aggregation.** *Raw pass-through is canonical for research signals; only compute summaries needed for pipeline operation, indexing, or cheap sanity checks.* Extends the system-prompt's YAGNI rule for code to data handling. Allows operational metrics (e.g., a `transcript_empty: bool` for `status` subcommand efficiency) without permitting speculative research aggregation.

**Decided. Architect for parallelism; develop on single A10.** The dev grant target is N=1 transcribe worker on a single A10 (cost-efficient). However, downstream production processing — running on the researcher's separate grant — will use multi-state and/or multi-GPU parallelism (concurrency.md:34-40 notes 2 states per A10 is a common sweet spot with 1.3–1.6× throughput potential). Plan B's `WhisperEngine` API is therefore designed so that the production upgrade is a swap-in change, not a rewrite:

- **Public API stable across single-state and multi-state internals**: `engine.transcribe(samples, cfg).await` returns one result per call regardless of internal parallelism.
- **Internal upgrade path documented**: Epic 1 ships single (context, state, worker thread). Plan C / production-grant work either (a) upgrades Engine internals to (context, Vec<state>, Vec<worker thread>, dispatcher), or (b) introduces `WhisperPool` of N Engines with routing dispatcher.
- **Configuration plumbing anticipates**: Epic 1 ships defaults `gpu_devices=[0]`, `states_per_gpu=1`. Future configuration extends to multi-device, multi-state without changing the per-call API surface.
- **Bake measurement** includes a 1-state vs 2-state experiment on the dev A10 so production planning isn't measurement-blind.

## What is NOT in Plan B (deferred to Plan C unless otherwise noted)

- Short-link resolution (`pending_resolutions`, HEAD redirect follower, `resolve-short-links`)
- `ApiFetcher` implementation (TikTok Research API)
- Comments fetching
- Manifest parquet export
- Multi-instance / multi-GPU coordination *implementation* (architecture is future-proofed)
- Multi-state intra-GPU parallelism *implementation* (architecture is future-proofed; bake measures the throughput delta)
- `SHORT_LINK_RE` query-parameter fix (FOLLOWUPS — Plan C trigger)
- Validate-and-mark-succeeded stale-recovery optimization (Plan B does redo on stale claim; see Epic 2)

## Inherited from Plan A (unchanged)

- **AD0001** per-task file split. Plan B's directory: `docs/superpowers/plans/2026-05-12-plan-b/`
- **AD0002** dead-code suppression strategy; bin/lib restructuring deferred to Epic 5 of Plan B
- **AD0003** test discipline (test-first batch for plan-prescribed; full TDD for deviations)
- **AD0004** transcript output sharding (last two digits of video_id)
- **AD0005** `test-helpers` feature for library items needed by integration tests
- **AD0006** Store mutators return `Result<usize>` row-change count
- **AD0007** stats input-side counters with verb-named fields
- **AD0008** pipeline writes artifacts before `mark_succeeded` for crash-recovery durability. Plan B preserves the invariant unchanged and *adopts the simple-redo recovery branch* (see Epic 2's stale-claim sweep).
- **FOLLOWUPS discipline** — entries resolved by Plan B get deleted; new entries added during Plan B reviews
- **Sonnet for spec compliance, opus for code quality** review tier
- **Single-flight Agent dispatch** via thermal-lock hook
- **Three RETRO meta-process improvements**: capture coherence ADRs during brainstorm (this session — Plan B's first ADRs land before per-task files exist); structure tasks for early-MVP shape (Epic 1 is the thin-slice); curated per-task ADR dispatch (controller pre-selects relevant ADRs per task)

## Epic structure

| # | Epic | Approx | Scope summary |
|---|------|--------|---------------|
| 1 | **Efficiency thin-slice** | ~1 wk | A10 workspace + bake + whisper-rs swap (with dedicated worker thread from the start) + raw-signal pass-through + GPU verification + spin-down + "done"-contract ADRs |
| 2 | **State-machine + pipelined orchestrator** | ~1 wk | Schema-version handling first (defensive), then minimal state-machine (stale-claim sweep, guarded `mark_succeeded`, retryable/terminal minimum, supervision shape), then bounded mpsc + N download workers + 1 transcribe worker + bounded `process::run` stream capture + claim-contention polling semantics |
| 3 | **Full failure classification taxonomy** | ~1 wk | `RetryableKind` / `UnavailableReason` / `ClassifiedFailure` enums; classification rules per spec; Bug-class supervision policy; classification-aware FOLLOWUPS resolution (T6/T11/T12) |
| 4 | **Time-window filter + diagnostics** | ~½ wk | DDP-tz resolution; `recompute-window`; `status` subcommand implementing the "done"-contract ADR from Epic 1 (counts, artifact-existence check, schema-version check) |
| 5 | **Ops hygiene + structural cleanup** | ~½ wk | Multi-fetcher provenance fix; sync-IO sweep; `requeue-retryables`; bin/lib reassessment per AD0002 |

## Epic 1 architecture (detailed — the thin-slice)

### Components changing in source

- **`src/transcribe.rs`** — rewritten. Replace `whisper-cli` subprocess invocation (via `process::run`) with embedded `whisper-rs` calls held by a dedicated worker thread. Most of the current stdout-parsing logic disappears.
- **`src/pipeline.rs`** — adjusted. Construct a `WhisperEngine` once at process startup; communicate with it via channel from `process_one`.
- **WAV decode** — new dependency, likely `hound` (small, focused on PCM WAV; no_std-friendly). whisper.cpp's C API takes float32 PCM at 16 kHz mono (api-and-pipeline.md:7); yt-dlp gives us a 16 kHz mono WAV file. We decode it in-process before handing the samples to whisper-rs.
- **`Cargo.toml`** — add `whisper-rs` (with `cuda` feature gated via cargo feature so local CPU builds still work) and `hound`.

### New structures

```rust
// src/transcribe.rs

pub struct WhisperEngine {
    handle: thread::JoinHandle<()>,
    request_tx: mpsc::Sender<TranscribeRequest>,
    // No engine-level cancellation flag — cancellation is per-request (see TranscribeRequest)
}

struct TranscribeRequest {
    samples: Vec<f32>,                 // 16 kHz mono float32, decoded in-process from WAV
    config: PerCallConfig,             // language pin, compute_lang_probs flag
    cancel: Arc<AtomicBool>,           // per-request, polled by FullParams::abort_callback
    deadline: Instant,                 // worker sets cancel when deadline elapses
    reply: oneshot::Sender<Result<TranscribeOutput, TranscribeError>>,
}

impl WhisperEngine {
    pub fn new(model_path: &Path, gpu_device: i32, flash_attn: bool) -> Result<Self, WhisperInitError>;
    // Spawns the worker thread, loads model + state, asserts GPU backend at init,
    // logs gpu_device index and reported device name (sharp-edges.md:60 silent wrong-GPU).
    // Owns the WhisperContext + WhisperState; nobody else touches them.

    pub async fn transcribe(&self, samples: Vec<f32>, cfg: PerCallConfig, timeout: Duration)
        -> Result<TranscribeOutput, TranscribeError>;
    // Builds TranscribeRequest with a fresh Arc<AtomicBool> cancel + deadline = now() + timeout.
    // Sends request to worker thread via channel; awaits oneshot reply.

    pub fn shutdown(self);  // joins the worker thread cleanly on drop / explicit shutdown
}

pub struct PerCallConfig {
    pub language: Option<String>,       // None means "auto"; Some("en") pins
    pub compute_lang_probs: bool,       // opt-in: extra encoder pass via lang_detect
}

pub struct TranscribeOutput {
    pub text: String,
    pub language: String,                          // from whisper_full_lang_id (single ID, free)
    pub lang_probs: Option<Vec<(String, f32)>>,    // Some only when compute_lang_probs=true
    pub segments: Vec<SegmentRaw>,
    pub model_id: String,                          // already in current metadata
}

pub struct SegmentRaw {
    pub no_speech_prob: f32,
    pub tokens: Vec<TokenRaw>,
}

pub struct TokenRaw {
    pub p: f32,        // whisper_full_get_token_p
    pub plog: f32,     // log-prob
}
```

### Worker-thread invariants (codified)

These rules apply to the Epic 1 implementation and persist through all future internal upgrades (multi-state, multi-Engine pool):

1. **Only owned data crosses the worker boundary.** `Vec<f32>` samples, owned configs, owned output structs (`TranscribeOutput`, `SegmentRaw`, `TokenRaw`). `WhisperContext`, `WhisperState`, and any whisper-rs reference types stay inside the worker thread — they MUST NOT leak through the oneshot reply.
2. **A closed oneshot reply is Bug-class.** If the worker drops the reply sender or sees the request channel closed unexpectedly, it's a defect (orchestrator bug, panic during request build, etc.) — surface as `TranscribeError::Bug` for coordinated shutdown, not as an ordinary transcription failure.
3. **Per-request cancellation only.** The cancel `Arc<AtomicBool>` is built per request and dropped with the request. Never reuse across requests; never store on the Engine. The `FullParams::abort_callback` always polls *this request's* flag (api-and-pipeline.md:41).

### Embedding hygiene defaults (from sharp-edges.md)

- `FullParams::set_print_progress(false)` — default true would pollute stderr (sharp-edges.md:66)
- `FullParams::set_print_realtime(false)` — never enable in any embedded context (sharp-edges.md:67)
- `FullParams::set_abort_callback(...)` — wired to the per-request cancel flag
- At init, **assert the backend log line shows GPU**, not CPU (sharp-edges.md:61: CPU silently engages if GPU init fails, running ~100× slower)
- At init, **log `gpu_device` index + reported device name** (sharp-edges.md:60: silent wrong-GPU if CUDA_VISIBLE_DEVICES misconfigured)

### Data flow per video (serial loop preserved)

```
process startup
  WhisperEngine::new(model_path, gpu_device=0, flash_attn=true)
    spawns worker thread
    worker thread: loads model into GPU VRAM, asserts GPU backend active,
                   logs device name, enters request loop
  ~5–10 s; ~750 MB GPU VRAM pinned for large-v3-turbo-q5_0

per-video loop
  claim_next → YtDlpFetcher.acquire (unchanged) → WAV decode (hound) → samples: Vec<f32>
            → engine.transcribe(samples, cfg, timeout).await
                ├── construct request with fresh cancel flag + deadline
                ├── send via mpsc to worker thread
                ├── worker calls whisper_full_with_state(state, params)
                │   where params.abort_callback polls this request's cancel flag
                ├── worker pulls raw signals via whisper-rs getters
                └── worker replies via oneshot
            → output::artifacts.write_atomic (txt + json with raw_signals)
            → mark_succeeded
```

AD0008 invariant intact: artifacts before `mark_succeeded`. AD0006 mutator signature intact. AD0007 stats convention applies if new counters are introduced.

### JSON artifact schema (additive, versioned)

The existing `{video_id}.json` keeps `video_id`, `source_url`, `duration_s`, `language_detected`, `transcribed_at`, `fetcher`, `transcript_source`, `model`. A new `raw_signals` object is appended:

```json
{
  "...existing fields preserved...": "...",
  "raw_signals": {
    "schema_version": "1",
    "language": "en",
    "lang_probs": null,
    "segments": [
      {
        "no_speech_prob": 0.02,
        "tokens": [{"p": 0.99, "plog": -0.01}, ...]
      }
    ]
  }
}
```

`lang_probs` is `null` by default. Becomes `[["en", 0.93], ["nl", 0.05], ...]` when `--compute-lang-probs` is set on the run, which adds one extra encoder pass per video. Per pass-through rule: no `avg_logprob` (downstream computes it from `plog`s if it wants); no `mean_no_speech_prob`; no segment timestamps. `schema_version` lets future additions extend the object without breaking parsers.

### Error handling, Epic 1 scope (preserves Plan A fail-fast)

Reuse existing `TranscribeError`. Whisper-rs failures map onto the same variants the current CLI wrapper produces; signatures change but `pipeline::process_one`'s observable behavior does not. Model-load failure is Bug-class per AD0008's spirit (configuration broken). Inference failure surfaces as `TranscribeError`.

**On any `TranscribeError` (including `Cancelled` from a deadline-elapsed inference), Epic 1 preserves Plan A's fail-fast behavior**: the error propagates up, `pipeline::process_one` returns it, the process exits non-zero, and the row stays `in_progress` for re-claim. Epic 2's state-machine work introduces the proper retryable/terminal/cancelled persistence so subsequent runs can mark these rows correctly instead of repeating the failure.

**Cooperative cancellation mechanics.** Each call carries a `timeout: Duration`. The worker thread sets the request's `Arc<AtomicBool>` cancel flag when the deadline elapses; `FullParams::abort_callback` polls the flag during graph compute and short-circuits inference. Cancelled inference produces no artifact writes. The cancellation flag is dropped with the request — no risk of leaking to subsequent requests.

**No use of `whisper_full_parallel`** — by explicit decision. It is *not* a parallel-transcription tool; it splits one audio across N states with documented quality loss at chunk boundaries (sharp-edges.md:45). Documented as non-decision in the orchestrator ADR.

Failure classification (`RetryableKind` / `UnavailableReason` / `ClassifiedFailure`) is Epic 3 work and stays Epic 3.

### Testing

| Tier | What | Notes |
|---|---|---|
| 1 | `TranscribeOutput` / `SegmentRaw` / `TokenRaw` serde round-trip | Pure unit; no model file required |
| 1 | `PerCallConfig` construction & defaults | Pure unit |
| 2 | `WhisperEngine::new` model-load happy path with `tiny.en`; assert backend is GPU (or accept CPU when local-dev); assert lang_probs is None unless config opts in | `test-helpers` gated per AD0005; requires `./models/ggml-tiny.en.bin` on disk |
| 2 | Real tiny.en + fixture WAV → assert text non-empty, segments non-empty, every segment carries `no_speech_prob`, `lang_probs` is `Some` iff `compute_lang_probs=true` | Test-helpers; CPU build of whisper-rs is fine locally |
| 2 | Cooperative cancellation: drive abort flag mid-inference, assert `TranscribeError::Cancelled` returns within bounded time | Test-helpers |
| 2 | Closed-oneshot Bug-class behavior: drop the reply receiver, assert worker surfaces a Bug-shaped error rather than silently consuming the request | Test-helpers |
| 3 | `e2e_real_tools` upgraded: real yt-dlp + whisper-rs (was whisper-cli); test loads tiny.en and walks the full pipeline | `#[ignore]`; runs on A10 workspace during bake |

Tier 1 is model-free per Plan A's CI matrix; nothing in Tier 1 depends on whisper-rs being buildable on the host.

### Bake measurements (the second deliverable of Epic 1)

Spin up A10 workspace on the dev grant. Extend `SRC-BAKE-CHECKLIST.md` Phase 7 for A10. Success criteria sharpened (revision 3):

- **Models compared**: `tiny.en`, `small`, `large-v3-turbo-q5_0`, `medium.en` against a small **manually quality-checked** fixture set (~5 videos per language). Don't just trust transcript length / detected language.
- **Per-stage timing**: separate fetch (yt-dlp) / WAV decode / transcribe wallclocks. Capture p50 and p95 per stage.
- **Per-call cost**: raw JSON size with and without `--compute-lang-probs` (measure the opt-in overhead).
- **Warmup characterization**: cold-start (first inference after process start) vs steady-state (post-warmup). CUDA graphs need a 2-loop warmup (sharp-edges.md:55); for TikTok clips often < 30 s the warmup penalty is paid every time. Consider `params.audio_ctx` for short audio.
- **GPU backend verification**: every bake run asserts the `using <backend> backend` log line is GPU and the device name matches; abort the run if CPU fallback silently engaged.
- **1-state vs 2-state measurement** (bake-only; not implemented in Plan B): on a single dev A10, measure throughput with 1 `WhisperState` vs 2 concurrent `WhisperState`s. Record: ms per inference, GPU SM utilization (`nvidia-smi`), VRAM usage, per-clip wallclock at p50/p95. Informs production-grant planning without committing Plan B implementation effort.
- **Output**: `docs/SRC-BAKE-NOTES.md` (Phase 8 already specifies this destination).

Numbers feed Epic 2's pipelined-orchestrator decision: if A10 transcription dominates wallclock at small workloads, fetch-transcribe overlap is academic. If yt-dlp dominates, Epic 2 has clear payoff.

### Operational practice — spin-down (third Epic 1 deliverable)

Workspace must spin down between batches. SRC pause semantics need to be looked up via the `d3i-claude-skills:src-workspace-ops` skill (does pause preserve state? does it bill core-hours?). The ADR captures: when does the operator pause, when do they delete, what's the canonical re-start checklist, where does model storage live such that re-start is fast?

### Operational practice — "done" contract for batch validation (fourth Epic 1 deliverable, ADR only; implementation lands in Epic 4)

The ADR defines what "this batch is done" means. The contract:

- Counts by status: pending / in_progress / succeeded / failed_terminal / failed_retryable (post Epic 2 schema)
- Artifact-existence check: every `succeeded` row has its `.txt` and `.json` on disk
- Raw-signals schema version check: every `.json` has the expected `raw_signals.schema_version`
- Log retention on the GPU box: where are tracing logs written, how long retained, what's the cost
- Transcript / artifact sync or backup to Research Drive (storage policy for the dev workspace)
- "Workspace is safe to spin down" signal: all the above pass + no `in_progress` rows pending recovery

The ADR is drafted in Epic 1. The implementation surface is Epic 4's `status` subcommand (counts + artifact check + schema-version check) plus an explicit operator runbook. The point of writing the ADR now: future tasks know what shape the `status` command must report.

### ADRs to draft during Epic 1 (some land before any code)

1. **Use `whisper-rs` (out-of-tree binding)** for embedding the whisper.cpp library — vs alternatives. Includes version-pinning policy (pin both `whisper-rs` crate AND the whisper.cpp commit it tracks) and explicit fallback decision rule (if CUDA build fails after one debugging cycle, fall back to Approach 0 + a documented whisper-cli JSON patch; otherwise stay the course).
2. **JSON artifact schema for raw-signal pass-through** including schema_version field. Captures the pass-through rule and the specific schema additions. Cross-references AD0008.
3. **Spin-down operational practice** for the dev workspace. Depends on SRC pause lookup.
4. **Cooperative cancellation policy** for embedded inference: per-request `Arc<AtomicBool>` + deadline semantics; per-video timeout default; Epic 1 treats Cancelled as fail-fast (Epic 2 reclassifies).
5. **GPU verification at startup**: assert backend is GPU not CPU at init; log gpu_device index and reported device name; abort process if mismatch.
6. **Audio-input invariant**: float32 PCM 16 kHz mono; document the contract; `hound` (or equivalent) decodes the WAV produced by yt-dlp's postprocessor; reject inputs that don't match.
7. **Explicit non-use of `whisper_full_parallel`**: documented non-decision (it's not for parallel transcription).
8. **Architecture for parallelism**: Engine API stable across single/multi-state internals; documented upgrade path (Plan C / production-grant); configuration plumbing anticipates `gpu_devices: Vec<i32>` and `states_per_gpu: usize`. Epic 1 ships defaults `gpu_devices=[0]`, `states_per_gpu=1`.
9. **Operational "done" contract** for batch validation (described above). ADR landed in Epic 1; implementation lands in Epic 4.
10. **(Conditional)** Codify "pass-through, not pre-aggregation" as a project-wide meta-process ADR landing on `main` (alongside AD0001–3). Likely beneficial; final placement deferred to the per-task brainstorming for Epic 1.

### Risks

- **whisper-rs CUDA build on the A10 workspace.** First-time CMake + CUDA + Rust integration can be painful. Mitigation: bake checklist Phase 1 enumerates tool versions; pin both `whisper-rs` crate version and the whisper.cpp commit; if the build fails after a debugging cycle, fall back to Approach 0 + a documented whisper-cli JSON patch for confidence signals. Extending Epic 1 by 1–2 days, not abandoning Plan B. The ADR (#1 above) records the decision rule.
- **SRC "pause" semantics unknown.** To be resolved via the SRC operations skill before the ops ADR (#3) is written. Not a blocker for the architectural design.
- **GPU memory headroom.** Single `WhisperState` for `large-v3-turbo-q5_0` ~750 MB–1 GB. A10 has 24 GB. Plenty of room. Note for record.
- **CPU silent fallback** (sharp-edges.md:61). Mitigation: GPU-verification ADR (#5) plus init log assertion plus bake-time check.
- **CUDA-graph warmup penalty for short audio** (sharp-edges.md:55). Mitigation: measure during bake; consider `params.audio_ctx` for short audio.

## Epic 2–5 (sketches; to be detailed before each epic begins)

### Epic 2 — State-machine + pipelined orchestrator

Three coupled changes, sequenced:

**(a) Schema-version handling first** (defensive, before any schema change):
- `Store::open` reads `meta.schema_version`, compares to `SCHEMA_VERSION` constant
- Policy ADR (auto-migrate forward / hard-fail mismatch / log warn + continue) — likely hard-fail with explicit operator migration tool
- Resolves FOLLOWUPS T7. Lands before the retryable/terminal columns.

**(b) Minimum state-machine work** (folded in from earlier Epic 3 plan):
- New schema columns: `last_retryable_kind`, `last_retryable_message`, `terminal_reason`, `terminal_message`. Schema-version increment.
- Stale-claim sweep at `process` startup: rows older than `stale_claim_threshold` flip `in_progress` → `pending`. No `attempt_count` bump (already incremented at claim time). **Decision codified (revision 3)**: stale sweep does NOT validate existing artifacts — it knowingly redoes fetch + transcribe. AD0008's "in_progress + complete artifacts after crash" state is accepted; we pay the redo cost in exchange for simplicity. Validate-and-mark-succeeded optimization deferred to Plan C if measured to matter.
- `Store::mark_succeeded` gains `WHERE status='in_progress' AND claimed_by = ?` predicate; returns 0 if row was not in claimed state (caller decides whether that's a Bug). Resolves FOLLOWUPS T10.
- Minimum retryable/terminal pair on `Store`: `mark_retryable_failure(kind: &str, message: &str)` and `mark_terminal_failure(reason: &str, message: &str)` — minimal strings; full taxonomy is Epic 3.
- Bug-class supervision shape: workers run inside a `JoinSet`; first task that returns `Err(Bug)` or panics triggers coordinated shutdown via cancellation token; exit code 1. The worker-thread `WhisperEngine` and the new download workers all participate.

**(c) Pipelined orchestrator** (the throughput-bearing change):
- Bounded `tokio::sync::mpsc::channel` from N download workers to 1 transcribe worker (which owns `WhisperEngine`).
- `Acquisition::Successful::AudioFile(path)` is the only variant routed through the channel; `ReadyTranscript` and `Unavailable` are short-circuit paths (latter is Epic 3 once classification lands).
- `WhisperEngine` already exists from Epic 1 with the worker-thread pattern; Epic 2 generalizes around it. No re-architecture needed.
- **Concurrent fetch hardening**: replace `process::run`'s unbounded stdout/stderr capture with bounded streaming. `VecDeque<u8>` rolling buffer of size `stderr_capture_bytes` per FOLLOWUPS T6 entry — load-bearing under N concurrent fetches because a misbehaving tool could otherwise allocate GB.
- **Claim contention policy** (FOLLOWUPS T10 / Plan B reassessment): specify polling strategy. Plan B uses sleep-and-retry between empty `claim_next` results (bounded backoff, e.g., 100ms–2s); explicit decision, not inherited from `busy_timeout`. Fix the not-actually-racing concurrency test (FOLLOWUPS T10 entry) to genuinely exercise concurrent claims via `std::thread::spawn` + `Barrier`.

### Epic 3 — Full failure classification taxonomy

`RetryableKind`, `UnavailableReason`, `ClassifiedFailure` enums per original spec § "Error handling and failure classification". `classify_fetch_error` and `classify_transcribe_error` functions. The minimum retryable/terminal pair from Epic 2's state-machine work gets enriched with typed kinds. Operator-facing string mapping. Resolves FOLLOWUPS clustered around T6/T11/T12 error mapping.

### Epic 4 — Time-window filter + diagnostics

DDP timestamp timezone resolution: empirically check or document the UTC assumption (FOLLOWUPS T13). `--window-start` / `--window-end` on `ingest`; `recompute-window` subcommand. `status` subcommand that implements the Epic 1 "done"-contract ADR: counts by status, artifact-existence check on succeeded rows, raw-signals schema-version check, respondent/error/retryable views.

### Epic 5 — Ops hygiene + structural cleanup

Multi-fetcher provenance fix (T14). Sync-IO sweep across `ingest`, `transcribe`, `pipeline` (4–5 FOLLOWUPS entries). `requeue-retryables` subcommand. Bin/lib reassessment per AD0002 — decide thin-binary-fat-library vs current dual-`mod` pattern; resolve `Store::conn`/`conn_mut` cleanup; resolve `output::shard_dir` dead helper.

## Open questions

1. **Language pinning default.** Configurable per-run, but should the default `--language` be `auto` or `en` for the first English-only run? Inclination: `auto`-default + `--language en` override flag.
2. **`keep_raw_metadata` default.** Original spec: dev=true, prod=false. Plan A doesn't yet retain raw yt-dlp info.json. Inclination: include in Epic 1 as adjacent to artifact-shape decisions; defer the prod=false flip until Plan C's production scale concerns surface.
3. **DDP timezone (FOLLOWUPS T13).** Resolve empirically in Epic 1 or defer to Epic 4? Inclination: defer to Epic 4 — the gap only matters once time-windowing matters.
4. **Pass-through rule as standalone ADR?** Likely it generalizes beyond Plan B. Inclination: yes, write as a meta-process ADR landing on `main`. (Final placement deferred to Epic 1 per-task brainstorming.)

## Brainstorm session protocol

Per the RETRO meta-process improvements adopted this session:

- This draft is the working document for codex-advisor review (two passes complete) and user final review.
- Coherence-maintaining ADRs (whisper-rs choice + version-pin + fallback, raw-signals schema, spin-down ops, cooperative cancellation, GPU verification, audio-input invariant, whisper_full_parallel non-use, parallelism architecture, "done" contract, optional pass-through meta-ADR) are drafted *during* brainstorm, before the per-task plan files exist.
- Per-task files for Epics 1–5 will live in `docs/superpowers/plans/2026-05-12-plan-b/` and follow Plan A's per-task split convention.
- Each task brief will declare which ADRs are directly relevant (curated dispatch).
- Schema-version handling has been moved from open question to Epic 2 first task.
