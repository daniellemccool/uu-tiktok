# Plan B Epic 5 — Ops hygiene + structural cleanup (sketch)

**Status:** Sketch — detailed expansion at Epic 5 kickoff after Epic 4 ships.

**Goal:** Sweep remaining FOLLOWUPS items that don't fit into Epics 1–4. Resolve AD0002's deferred bin/lib reassessment. Add the `requeue-retryables` subcommand. Clean up the multi-fetcher provenance lie. Final polish before Plan B is "done."

**Anticipated approx:** ~½ week, ~4–5 tasks.

## Scope

### Multi-fetcher provenance fix (FOLLOWUPS T14)

The current `pipeline.rs` hard-codes `fetcher: "ytdlp"` and `transcript_source: "whisper.cpp"` regardless of what actually ran. Plan B has only one fetcher and one transcriber so this is symbolic — but Plan C will add an `ApiFetcher`, and the test fakes already lie about provenance. Fix shape: add a `fn name(&self) -> &'static str` to `VideoFetcher`; promote `Transcriber` (currently an opaque `Fn`) to a small trait with `name()` and an async `transcribe()`. Thread the name through `process_one`.

### Sync-IO sweep

FOLLOWUPS clusters 4–5 entries here: synchronous `std::fs` calls inside async functions in `ingest.rs`, `transcribe.rs` (post Epic 1), `pipeline.rs`, `output/artifacts.rs`. Sweep: replace with `tokio::fs` or guard with `spawn_blocking`. Audit `cargo clippy --fix` doesn't catch these; they're judgment calls.

### `requeue-retryables` subcommand

Operator command per spec § "requeue-retryables". Flags:
- `--older-than <DURATION>` — only requeue if `videos.updated_at` older than this
- `--error-kinds <KIND,KIND,...>` — only requeue rows whose `last_retryable_kind` matches
- `--max-attempts <N>` — skip rows whose `attempt_count` is >= N
- `--max <N>` — cap the number requeued in this call
- `--dry-run`

`failed_retryable` → `pending`. Retains `last_retryable_*` for history.

### Bin/lib reassessment per AD0002

Decide between the current dual-`mod` pattern (main.rs and lib.rs both declare `mod foo`) and the thin-binary-fat-library pattern (lib.rs holds all `mod`s, main.rs imports via `use uu_tiktok::...`). AD0002 deferred this decision; Epic 5 makes it.

If thin-binary-fat-library chosen:
- Consume `Store::conn` / `Store::conn_mut` cleanup (FOLLOWUPS entry; orphaned accessors)
- Consume `output::shard_dir` dead helper (FOLLOWUPS entry)
- Several `#[allow(dead_code)]` annotations naturally disappear
- Per-task brief enumerates the cleanup

### `reset-stale-claims` subcommand (operator escape hatch)

Per spec § "reset-stale-claims". Distinct from Epic 2's startup stale-sweep — this is a manual operator command for one-off recovery. `--max-age <DURATION>` REQUIRED (no default; forces a deliberate choice). `--dry-run`.

### Other FOLLOWUPS still pending after Epics 1-4 (sweep)

- `output::cleanup_tmp_files` minor cleanups (missing path context, overcounted removals)
- `output::shard_distributes_uniformly` test rationale reversed
- `ingest::walk_recursive` silent missing-inbox; missing inner context
- `Store::pragma_string` visibility (pub vs pub(crate)) — bundle with bin/lib decision
- `Store::read_meta` could use OptionalExtension — defer or bundle
- `videos.updated_at` frozen at first-seen by upsert_video — Epic 5 chooses semantics

These are mostly cosmetic but the right time to clean them up is when the file gets touched, and Epic 5 is the planned touch.

## Anticipated ADRs

- **AD0030** Bin/lib structure decision (thin-binary-fat-library vs current dual-mod) — supersedes AD0002's deferral
- **AD0031** Multi-fetcher provenance: name() method on traits
- **AD0032** Sync-IO policy in async fns (when to use tokio::fs vs spawn_blocking)
- **AD0033** requeue-retryables / reset-stale-claims semantics (default-deny, operator-driven)

## Anticipated files affected

```
src/lib.rs                              # potentially restructured per AD0030
src/main.rs                             # potentially restructured per AD0030
src/fetcher/mod.rs                      # add name() to VideoFetcher trait
src/fetcher/ytdlp.rs                    # impl name() -> "ytdlp"
src/transcribe.rs                       # Transcriber becomes trait with name(); impl WhisperEngineTranscriber
src/pipeline.rs                         # thread fetcher/transcriber names through; sync-IO sweep
src/ingest.rs                           # sync-IO sweep; walk_recursive missing-inbox bail
src/output/artifacts.rs                 # sync-IO sweep; cleanup_tmp_files polish
src/output/mod.rs                       # delete shard_dir if dead; or wire to pipeline
src/state/mod.rs                        # videos.updated_at semantics decision; pragma_string visibility
src/cli.rs                              # add requeue-retryables, reset-stale-claims subcommands
tests/...                               # update for renamed/restructured items per AD0030
```

## Key risks to flag at kickoff

- AD0030 (bin/lib decision) is the heaviest item in this epic. Option 4 from AD0002 (thin-binary-fat-library) is the most structural; expect 1–2 days of mechanical refactoring if chosen.
- Sync-IO sweep risks introducing subtle race conditions if a function was relying on synchronous semantics. Treat each conversion as TDD-worthy.
- Tests that touch the changed surfaces (canonical, state, output) may need updates if the bin/lib restructure changes module paths.

## Inputs the planner should consult

- AD0002 (full text) — captures the rejected alternatives and the deferred decision.
- `docs/FOLLOWUPS.md` — every entry not resolved by Epics 1–4 is in Epic 5 scope.
- Epic 1's pipeline integration code — Epic 5's `Transcriber` trait promotion may require touch-ups.

## Plan B exit

After Epic 5 ships, Plan B is done. Outputs:
- Production-shaped pipeline on a single A10
- Full failure classification + recovery
- Operator commands for re-running failed videos and resetting stuck claims
- `status` subcommand for batch-done validation
- Time-window filter properly applied
- Architecture future-proofed for multi-state/multi-GPU
- Clean bin/lib structure
- FOLLOWUPS.md drained of Plan B–scope entries

The next milestone (Plan C) is the production-grade work: API fetcher, comments, multi-GPU implementation, manifest export, short-link resolution.
