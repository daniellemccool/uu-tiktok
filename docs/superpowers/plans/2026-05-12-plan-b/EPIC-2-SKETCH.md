# Plan B Epic 2 — State-machine + pipelined orchestrator (sketch)

**Status:** Sketch — detailed per-task expansion happens at Epic 2 kickoff after Epic 1 bake numbers exist.

**Goal:** Add the minimum failure state machine needed to make Epic 2's pipelined orchestrator recoverable, then introduce the pipelined orchestrator itself. Three coupled changes shipped together.

**Anticipated approx:** ~1 week, ~8–10 tasks. The heaviest epic in Plan B.

## Sequence (three sub-phases, in order)

### (a) Schema-version handling first

Defensive change — lands before any schema change.

- `Store::open` reads `meta.schema_version`, compares to `SCHEMA_VERSION` constant.
- Policy ADR: likely hard-fail with explicit operator migration tool. Decided in first task of Epic 2.
- Resolves FOLLOWUPS T7.

### (b) Minimum state-machine

- **New schema columns**: `last_retryable_kind`, `last_retryable_message`, `terminal_reason`, `terminal_message`. `SCHEMA_VERSION` increments.
- **Stale-claim sweep at `process` startup**: rows older than `stale_claim_threshold` flip `in_progress` → `pending`. No `attempt_count` bump.
  - **Decided in revision 3**: sweep does NOT validate existing artifacts. AD0008's "in_progress + complete artifacts" state is accepted; we pay redo cost for simplicity. Validate-and-mark optimization is Plan C.
- **`Store::mark_succeeded` gains `WHERE status='in_progress' AND claimed_by = ?` predicate**; returns 0 if row was not in claimed state. Resolves FOLLOWUPS T10.
- **Minimum mutators**: `Store::mark_retryable_failure(kind: &str, message: &str)` and `Store::mark_terminal_failure(reason: &str, message: &str)`. Minimal strings; full typed taxonomy lives in Epic 3.
- **Bug-class supervision shape**: workers run inside `tokio::task::JoinSet`. First task that returns `Err(Bug)` or panics triggers coordinated shutdown via cancellation token. Process exits 1. The Epic 1 `WhisperEngine` worker thread and the new download workers all participate.

### (c) Pipelined orchestrator

- Bounded `tokio::sync::mpsc::channel` from N download workers to 1 transcribe worker (owns the Epic 1 `WhisperEngine`).
- `Acquisition::Successful::AudioFile(path)` routes through the channel; other variants short-circuit.
- `WhisperEngine` already exists from Epic 1 with the worker-thread pattern; Epic 2 generalizes around it.
- **Concurrent fetch hardening**: replace `process::run`'s unbounded stdout/stderr capture with bounded streaming. `VecDeque<u8>` rolling buffer per FOLLOWUPS T6. Load-bearing under N concurrent fetches.
- **Claim contention policy** (FOLLOWUPS T10): specify polling strategy. Plan B uses sleep-and-retry between empty `claim_next` results (bounded backoff, e.g., 100ms–2s). Explicit decision, not inherited from `busy_timeout`.
- **Fix not-actually-racing concurrency test** (FOLLOWUPS T10 entry): rewrite using `std::thread::spawn` + `std::sync::Barrier`.

## Anticipated ADRs

- **AD0018** Schema-version policy (hard-fail / auto-migrate / log+warn)
- **AD0019** Minimum mutator signatures (mark_retryable_failure / mark_terminal_failure shape)
- **AD0020** Stale-claim sweep semantics + redo decision
- **AD0021** Bug-class supervision (JoinSet + coordinated shutdown shape)
- **AD0022** Claim contention polling policy
- **AD0023** Bounded `process::run` capture (replaces FOLLOWUPS T6 entry)

## Anticipated files affected

```
src/state/schema.rs                 # SCHEMA_VERSION bump + new columns + version-check policy
src/state/mod.rs                    # mark_retryable_failure, mark_terminal_failure, mark_succeeded WHERE predicate, stale-claim sweep
src/process.rs                      # bounded stdout/stderr ring buffer
src/pipeline.rs                     # download workers + bounded mpsc + transcribe worker integration; JoinSet supervision
src/cli.rs                          # --download-workers, --channel-capacity, --stale-claim-threshold flags
src/config.rs                       # download_workers, channel_capacity, stale_claim_threshold fields
tests/state_claims.rs               # fix concurrency test; add stale-sweep test; add mark_succeeded WHERE-predicate test
tests/pipeline_fakes.rs             # extend FakeFetcher to script retryable/terminal failures; multi-worker orchestration test
tests/process_bounded_capture.rs    # new test for bounded stderr/stdout
docs/decisions/AD0018-...md         # six new ADRs
...
```

## Key risks to flag at kickoff

- Pre-existing FOLLOWUPS may interact in surprising ways (e.g., the `Store::updated_at` frozen-by-upsert finding — T9 review entry — affects stale detection if `claimed_at` isn't bumped correctly).
- Tokio JoinSet vs Plan A's current `tokio::main` shape may need restructuring of `main.rs`.
- The minimum-mutator design must compose cleanly with Epic 3's typed enums or we'll re-litigate the signatures in Epic 3.

## Inputs from Epic 1 the planner should consult

- The Epic 1 bake numbers in `docs/SRC-BAKE-NOTES.md`. Tells us whether fetch-transcribe overlap actually buys throughput at our scale.
- `docs/FOLLOWUPS.md` updated state — entries marked resolved by Epic 1 are deleted; new ones inform Epic 2 sub-tasks.
- The Epic 1 `WhisperEngine` API surface — Epic 2 must keep the engine's public API stable while wrapping the orchestrator around it.
