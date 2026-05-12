# UU TikTok Pipeline — Plan B: Efficiency-first Refactor

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Each task is its own file** in this directory (`01-adr-drafts.md` … `13-bake-runbook.md`). Open only the task you're working on. Do NOT load the full design spec or all task files into a subagent's context — they're large and the per-task files are self-contained.

**Goal:** Move Plan A's walking skeleton from "works on CPU at small scale via `whisper-cli` subprocess" to "works on a single A10 GPU via embedded `whisper-rs`, captures per-video raw confidence signals via pass-through serialization, and operates within tight grant-budget constraints by spinning down between batches." Architecture is future-proofed for multi-state and multi-GPU production parallelism while defaulting to single-state on a single A10 for dev grant cost.

**Architecture:** Embedded whisper.cpp via the `whisper-rs` Rust binding, held by a dedicated worker thread owning `WhisperContext` + `WhisperState`. Communication via tokio mpsc channel + oneshot reply per request. Per-request cancellation via `Arc<AtomicBool>` polled by `FullParams::abort_callback`. Plan A's serial loop preserved through Epic 1; pipelined orchestrator (bounded mpsc + N download workers) lands in Epic 2.

**Tech Stack:** Rust 2021, tokio (existing), rusqlite (existing), `whisper-rs` (new — `cuda` feature gated), `hound` (new — WAV decode to float32 PCM), all existing Plan A deps preserved.

**Reference:** Full design in `docs/superpowers/specs/2026-05-12-uu-tiktok-pipeline-plan-b-design.md`. The plan implements Epic 1 verbatim; the spec is the source of truth for "why." **Subagents implementing tasks should not need to open the spec** — each task file is self-contained.

**This is Plan B Epic 1 of 5 epics**. Each subsequent epic gets its own per-task expansion when it begins. Epic 2–5 sketches are in `EPIC-2-SKETCH.md` through `EPIC-5-SKETCH.md`. Reassess design after Epic 1's artifact exists and bake numbers are in.

---

## File Structure (after Epic 1)

```
uu-tiktok/
├── Cargo.toml                # +whisper-rs (cuda feature), +hound
├── src/
│   ├── main.rs               # unchanged
│   ├── cli.rs                # unchanged in Epic 1
│   ├── config.rs             # +whisper_engine config (gpu_devices, states_per_gpu, compute_lang_probs default)
│   ├── errors.rs             # +TranscribeError::Cancelled, +TranscribeError::Bug refinement
│   ├── canonical.rs          # unchanged
│   ├── process.rs            # unchanged in Epic 1 (Epic 2 hardens bounded capture)
│   ├── state/                # unchanged in Epic 1 (Epic 2 adds schema-version handling)
│   ├── fetcher/              # unchanged
│   ├── transcribe.rs         # REWRITTEN: WhisperEngine, worker thread, per-request cancellation, raw signal extraction
│   ├── output/
│   │   ├── mod.rs            # unchanged
│   │   └── artifacts.rs      # updated: TranscriptMetadata gains raw_signals field
│   ├── ingest.rs             # unchanged
│   └── pipeline.rs           # updated: process_one constructs/uses WhisperEngine instead of whisper-cli
└── tests/
    ├── canonical.rs          # unchanged
    ├── ingest.rs             # unchanged
    ├── pipeline_fakes.rs     # updated: FakeWhisperEngine or feature-gate around it
    ├── cli.rs                # unchanged
    └── e2e_real_tools.rs     # upgraded: uses whisper-rs path (was whisper-cli)
```

**Files NOT changed in Epic 1 (Epic 2 or later):**

- `src/state/*` — Epic 2 adds schema-version handling + retryable/terminal columns
- `src/process.rs` — Epic 2 hardens stderr/stdout capture to bounded streaming
- Failure classification types — Epic 3
- `cli.rs` time-window flags — Epic 4

---

## Dependency changes (`Cargo.toml`)

Epic 1 adds:

```toml
[dependencies]
whisper-rs = { version = "0.X", default-features = false }   # exact version pinned in T2
hound = "3"

[features]
default = []
cuda = ["whisper-rs/cuda"]
```

The `cuda` feature is opt-in so local CPU builds still work. CI builds non-cuda by default; SRC A10 workspace builds with `--features cuda`. Exact `whisper-rs` version + the whisper.cpp commit it tracks get pinned in T2 (Cargo deps) and recorded in the ADR (T1).

---

## Task Conventions (inherited from Plan A unchanged)

- **TDD throughout.** Each task: write the failing test, run it to confirm the failure, write minimum implementation, run to confirm pass, commit.
- **Commit per task** with a focused message. The plan supplies the message.
- **`cargo test` runs cleanly at the end of every task.** If a step adds a test that depends on later code, mark the test `#[ignore]` until the supporting code lands.
- **No `unwrap()` in non-test code** unless justified by an invariant the type system enforces.
- **Run `cargo fmt` and `cargo clippy --all-targets -- -D warnings` before each commit.** If clippy fires, fix the lint or `#[allow]` it with a one-line justification comment.
- **AD0003 deviation honesty.** Every brief deviation (clippy-driven cosmetic fixes, structural choices that diverge from verbatim brief) gets prominent disclosure in commit message bodies.

## Review cycle (Plan B refinement — 3 tiers instead of Plan A's 2)

Plan B adds codex-advisor as a third reviewer tier. Per the brainstorm session that produced this plan, codex-advisor's input on code-quality dimensions complements opus's intra-Anthropic reviews with a different model family. The three tiers, per dispatch:

| Tier | Role | What it checks |
|------|------|----------------|
| **Opus implementer** | Writes the code per the task brief | TDD discipline; brief-verbatim implementation; ADR compliance; AD0003 deviation honesty in commits |
| **Sonnet spec-compliance reviewer** | Mechanical "does this match the brief" check | Brief steps were followed verbatim modulo documented deviations; ADRs declared in the task brief are honored; AD0002 dead-code cleanup applied; clippy/fmt clean |
| **codex-advisor code-quality reviewer** | Qualitative correctness review | Subtle correctness issues; cross-file consistency; race conditions; lifetime/Send/Sync hazards; perf footguns; testing gaps |

**codex-advisor session continuity:** the pinned session from the brainstorm (UUID `019e1b70-1ea0-75b3-83ba-9a68f63d0545` as of plan write) maintains all the Plan B context already. Reuse it for code-quality reviews via `codex-advisor ask <prompt>` per task. If the session is lost or reset, re-init with the priming prompt at the top of the spec and `orient` on the Plan B design spec file.

**Dispatch protocol per task:**

1. Controller dispatches opus implementer with the task brief + curated ADRs (per RETRO meta-improvement #3)
2. Implementer reports `DONE` (or `DONE_WITH_CONCERNS` / `BLOCKED`)
3. Controller dispatches sonnet spec-compliance reviewer with: the brief, the diff, and the curated ADRs
4. If sonnet flags issues: controller decides fix path (inline edit, re-dispatch implementer, or accept with AD0003 deviation note)
5. Controller asks codex-advisor for code-quality review via `codex-advisor ask` with the diff and the task brief
6. If codex-advisor flags genuine issues: controller decides fix path (same options as step 4)
7. After all reviews resolve: commit (controller does the commit), update FOLLOWUPS if needed, move to next task

**Cost-quality calibration** (from Plan A's RETRO refined for Plan B):

- Opus for implementation when the task has multi-subtle interactions (Plan B's T6 engine init, T7 transcribe, T11 pipeline integration); sonnet for mechanically tractable tasks (T2, T3, T8, T12).
- Sonnet is sufficient for spec-compliance review across the board — that work is mechanical.
- codex-advisor is consulted per-task. For trivial tasks (T2 cargo deps) the review may be a one-line "looks fine"; for substantial tasks (T7, T11) expect specific findings.

The single-flight Agent dispatch (thermal lock from Plan A) still applies.

---

## Architectural Decision Records (ADRs) — Epic 1

ADRs live in `docs/decisions/` and are managed via the `adg` tool. The format is MADR.

Plan B inherits Plan A's ADRs (AD0001–AD0008). Epic 1's first task (T1) drafts and decides nine new ADRs:

| Proposed ADR # | Title | Branch | Drafted in |
|---|---|---|---|
| AD0009 | Use whisper-rs (out-of-tree binding) for embedding + version-pin + fallback policy | feat | T1 |
| AD0010 | JSON artifact schema for raw-signal pass-through (schema_version) | feat | T1 |
| AD0011 | Spin-down operational practice for dev workspace | feat | T1 |
| AD0012 | Cooperative cancellation policy (per-request Arc<AtomicBool>, abort_callback) | feat | T1 |
| AD0013 | GPU verification at startup (assert backend = GPU; log device name) | feat | T1 |
| AD0014 | Audio-input invariant: float32 PCM 16 kHz mono via hound | feat | T1 |
| AD0015 | Explicit non-use of whisper_full_parallel | feat | T1 |
| AD0016 | Architecture for parallelism (Engine API stable across single/multi-state; production upgrade path) | feat | T1 |
| AD0017 | Operational "done" contract for batch validation (drafted Epic 1; implemented Epic 4) | feat | T1 |

A tenth ADR — codifying the **"pass-through, not pre-aggregation"** rule as project-wide meta-process — is conditional. T1 decides whether to write it; if yes, it lands on `main` (alongside AD0001–3 meta/process ADRs), not on feat.

**Authorship convention** (from Plan A): the controller writes ADRs. Subagents that encounter a multi-alternative decision should pause and report back as `BLOCKED` or `DONE_WITH_CONCERNS` rather than choosing silently — they lack the project context to record reasoning effectively.

**Curated dispatch** (RETRO meta-process improvement #3): each per-task brief in this plan declares which ADRs are directly relevant to that task. Subagents read those plus the overview, not all ADRs.

**Cleanup discipline** (per AD0002): when a task consumes a previously-dead type, remove the now-stale `#[allow(dead_code)]` as part of the work. Periodic backstop: `rg "allow\(dead_code\)" src/`.

---

## Task Index — Epic 1

| # | File | Subject | ADRs touched |
|---|------|---------|--------------|
| 1 | [01-adr-drafts.md](./01-adr-drafts.md) | Draft + decide all 9 Epic 1 ADRs via adg | all (writes them) |
| 2 | [02-cargo-deps.md](./02-cargo-deps.md) | Add whisper-rs (cuda feature-gated) + hound to Cargo.toml | AD0009, AD0014 |
| 3 | [03-wav-decode.md](./03-wav-decode.md) | WAV → Vec<f32> decoder (hound) + Tier 1 test | AD0014 |
| 4 | [04-transcribe-types.md](./04-transcribe-types.md) | TranscribeOutput / SegmentRaw / TokenRaw + serde + Tier 1 tests | AD0010 |
| 5 | [05-whisper-engine-shell.md](./05-whisper-engine-shell.md) | WhisperEngine struct + worker thread shell + closed-oneshot Bug test | AD0009, AD0012, AD0016 |
| 6 | [06-engine-init.md](./06-engine-init.md) | WhisperEngine::new: model load + GPU verification + init logs + Tier 2 test | AD0009, AD0013, AD0015 |
| 7 | [07-engine-transcribe.md](./07-engine-transcribe.md) | transcribe(): request/response, per-request Arc<AtomicBool>, abort_callback, deadline + Tier 2 cancellation test | AD0012 |
| 8 | [08-per-call-config.md](./08-per-call-config.md) | PerCallConfig: language pin, --compute-lang-probs + Tier 2 lang_probs test | AD0010 |
| 9 | [09-raw-signals.md](./09-raw-signals.md) | Whisper-rs getters → SegmentRaw/TokenRaw extraction + Tier 2 structural test | AD0010 |
| 10 | [10-artifact-schema.md](./10-artifact-schema.md) | Artifact writer with raw_signals (schema_version=1) + Tier 1 serde test | AD0010 |
| 11 | [11-pipeline-integration.md](./11-pipeline-integration.md) | pipeline.rs: replace whisper-cli subprocess with WhisperEngine + Tier 2 integration test | AD0009, AD0012 |
| 12 | [12-e2e-upgrade.md](./12-e2e-upgrade.md) | e2e_real_tools: switch to whisper-rs path + Tier 3 #[ignore] test | AD0009 |
| 13 | [13-bake-runbook.md](./13-bake-runbook.md) | A10 bake operator runbook + writes SRC-BAKE-NOTES.md | AD0011, AD0013, AD0017 |

---

## Epic 1 Exit Criteria

After Task 13 is committed, the following hold:

1. `cargo build --release --features cuda` on the A10 workspace produces a working binary.
2. `cargo test` (no features) passes Tier 1 + Tier 2 tests (Tier 2 requires `--features test-helpers` and `./models/ggml-tiny.en.bin`).
3. `cargo test --features test-helpers,cuda --test e2e_real_tools -- --ignored --nocapture` passes on the A10 workspace.
4. End-to-end run on the A10:
   ```bash
   cargo run --release --features cuda -- init
   cargo run --release --features cuda -- ingest
   cargo run --release --features cuda -- process --max-videos 5
   ```
   produces 5 transcripts whose `{video_id}.json` files each contain a `raw_signals` object with `schema_version: "1"`, per-segment `no_speech_prob`, per-token `p` / `plog`, and a single-string `language` field.
5. `docs/SRC-BAKE-NOTES.md` exists with: per-clip wallclock for tiny.en / small / large-v3-turbo-q5_0 / medium.en; 1-state vs 2-state measurement; GPU verification log; --compute-lang-probs overhead measurement.
6. Nine ADRs (AD0009–AD0017) exist as `decided` in `docs/decisions/`.

**Epic 1 is done when the above six items pass and `docs/FOLLOWUPS.md` has been updated** (resolved Plan B items deleted; new items added for things deferred to Epic 2+).

---

## What Epic 1 Deliberately Omits

These are deferred to Epic 2–5. Listed so the engineer doesn't accidentally implement them now:

- Pipelined orchestrator (bounded mpsc, N download workers) — Epic 2
- Minimum state-machine work (stale-claim sweep, guarded `mark_succeeded`, retryable/terminal columns + mutators) — Epic 2
- Schema-version handling on `Store::open` — Epic 2 (first task)
- Bounded `process::run` stderr/stdout capture — Epic 2
- Claim contention polling semantics — Epic 2
- Failure classification (`RetryableKind`, `UnavailableReason`, `ClassifiedFailure`) — Epic 3
- Time-window filter on watch_history.watched_at — Epic 4
- DDP timestamp timezone resolution — Epic 4
- `recompute-window` subcommand — Epic 4
- `status` subcommand (implements Epic 1's "done"-contract ADR) — Epic 4
- Multi-fetcher provenance fix (T14 from FOLLOWUPS) — Epic 5
- Sync-IO sweep — Epic 5
- `requeue-retryables` subcommand — Epic 5
- Bin/lib reassessment per AD0002 — Epic 5
- Short-link resolution — Plan C
- API fetcher / comments / manifest parquet — Plan C
- Multi-instance / multi-GPU implementation (architecture is future-proofed, implementation deferred) — Plan C / production grant
- Multi-state intra-GPU parallelism implementation (architecture is future-proofed, bake measures the delta) — Plan C / production grant
- Validate-and-mark-succeeded stale-recovery optimization — Plan C if measured to matter

---

## Self-Review Checklist (run by author after writing)

**Spec coverage:** Epic 1 maps to spec sections "Framing decisions" (Approach A, per-video raw confidence, pass-through, architect for parallelism), "Epic 1 architecture (detailed)" (verbatim — components, structures, data flow, JSON schema, error handling, embedding hygiene, testing, bake), "ADRs to draft during Epic 1" (T1 drafts all 9), and "Risks" (whisper-rs CUDA build fallback documented in AD0009). Sections explicitly out of scope: Epic 2–5 sketches stay sketches; all those items are flagged in "What Epic 1 Deliberately Omits."

**Placeholder scan:** None of the no-placeholder anti-patterns ("TBD", "TODO", "implement later", "add appropriate error handling") appear in task steps. Each TDD step has actual code. The `e2e_real_tools` test upgrade is `#[ignore]` and runs on the A10 workspace during bake (Task 13).

**Type consistency:** `WhisperEngine`, `TranscribeRequest`, `TranscribeOutput`, `SegmentRaw`, `TokenRaw`, `PerCallConfig` used consistently across T4–T11. JSON field names (`raw_signals`, `schema_version`, `language`, `lang_probs`, `segments`, `no_speech_prob`, `tokens`, `p`, `plog`) match across T4, T10, and T13's expected output.

**Scope:** 13 tasks, each producing a meaningful increment with TDD + commit. Epic 1 produces a binary that transcribes real TikTok audio on a single A10 with raw confidence signals captured and bake numbers documented. Final state of Epic 1 is "Plan B's efficiency thin-slice is alive on a real A10." Further increments belong in Epic 2–5.

**Ambiguity:** Each step shows exact code, exact commands, and expected output. Module wiring (`mod` declarations in `lib.rs` and `main.rs`) is called out per task. Cargo feature gating (`cuda`, `test-helpers`) is documented inline.

---

## Plan B Epic Sketches

The other four epics have sketches but not detailed per-task expansions. Each sketch documents scope, key files affected, anticipated ADRs, and rough task count. Detailed expansions happen at the start of each epic, per the "reassess between epics" pattern from Plan A.

- [EPIC-2-SKETCH.md](./EPIC-2-SKETCH.md) — State-machine + pipelined orchestrator
- [EPIC-3-SKETCH.md](./EPIC-3-SKETCH.md) — Full failure classification taxonomy
- [EPIC-4-SKETCH.md](./EPIC-4-SKETCH.md) — Time-window filter + diagnostics
- [EPIC-5-SKETCH.md](./EPIC-5-SKETCH.md) — Ops hygiene + structural cleanup
