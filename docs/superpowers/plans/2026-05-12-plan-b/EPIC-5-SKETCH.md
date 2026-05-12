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

## Notes from the brainstorm session (codex-advisor + whisper-cpp skill)

- **Bin/lib reassessment criteria** (AD0030 supersedes AD0002): the decision hinges on whether downstream library consumers exist. Plan B is a single-binary tool with no external library consumers. Option 4 (thin-binary-fat-library) is structurally cleaner but adds no value if no one imports `uu_tiktok` as a library. Conversely, the current dual-`mod` pattern works fine and `cargo clippy` doesn't complain. **Default recommendation: keep the dual-mod pattern**; revisit only if a real library consumer emerges (e.g., a separate `uu-tiktok-tools` crate that wants to reuse `Store`).
- **`Store::conn_mut` is dead.** FOLLOWUPS confirms zero consumers. Delete it in Epic 5. `Store::conn` has one cfg(test) consumer; keep with comment "used by cfg(test) schema invariant tests."
- **`output::shard_dir` is dead.** FOLLOWUPS confirms zero consumers. Delete in Epic 5.
- **`videos.updated_at` semantics**: FOLLOWUPS T9 entry. Epic 5 decides between (a) renaming to `inserted_at`, or (b) switching `upsert_video` to `ON CONFLICT DO UPDATE SET updated_at = excluded.updated_at`. Option (b) is the right move if Epic 2's stale-claim sweep ends up consuming `updated_at` for anything; Option (a) is the right move if it doesn't. Defer the choice until Epic 2 is shipped and we know what consumed `updated_at`.
- **`Store::pragma_string` visibility**: FOLLOWUPS entry recommends `pub(crate)`. Lower it unconditionally; the only caller is the cfg(test) integration test which can opt in via the `test-helpers` feature per AD0005.
- **`ring_buffer_tail` rename**: bundle with Epic 2's bounded-buffer work (already noted in Epic 2 sketch). Epic 5 doesn't need to repeat.
- **`From<RunError> for FetchError` mapping cleanup**: deferred from Epic 3's typed-error work into Epic 5 if Epic 3 didn't fully clean it. Verify after Epic 3 ships.
- **The `--whisper-model` global flag fix** (FOLLOWUPS entry at end of file): one-line `global = true` on the clap argument. Trivial; do in Epic 5 alongside any clap surface touch.

## FOLLOWUPS resolution map (Plan B-wide)

Tracking which Plan B epic resolves which FOLLOWUPS entry. Plan B is "done" when every Plan-B-scope FOLLOWUPS entry is either deleted (resolved) or moved to "Plan C" with explicit rationale.

| FOLLOWUPS entry | Plan B epic | Resolution |
|---|---|---|
| `process::run` unbounded stdout/stderr (T6) | Epic 2 | Bounded streaming capture |
| `ring_buffer_tail` misnamed (T6) | Epic 2 | Rename alongside bounded-buffer work |
| `From<RunError> for FetchError` collapses Spawn/Io (T6) | Epic 3 | Typed variants |
| `status.code().unwrap_or(-1)` loses signal info (T6) | Epic 3 | Add `signal` field |
| `Store::open` schema-version not read (T7) | Epic 2 first task | Read-and-check policy |
| `Store::pragma_string` pub vs pub(crate) (T7) | Epic 5 | Lower to pub(crate) |
| `Store::read_meta` OptionalExtension (T7) | Epic 5 | Refactor when touched |
| `output::shard` ASCII-only byte slice (T8) | Plan C | When VideoId newtype lands |
| `output::cleanup_tmp_files` polish (T8) | Epic 5 | Bundle with sync-IO sweep |
| `output::shard_distributes_uniformly` rationale (T8) | Epic 5 | Refactor comment when touched |
| `videos.updated_at` frozen by upsert_video (T9) | Epic 5 | Decision after Epic 2 ships |
| `Store::conn`/`conn_mut` accessor hygiene (T9/T10) | Epic 5 | Delete conn_mut; refresh comment |
| `concurrent_claim_serializes_via_begin_immediate` doesn't race (T10) | Epic 2 | Rewrite with Barrier |
| `mark_succeeded` doesn't require status='in_progress' (T10) | Epic 2 | WHERE predicate |
| `claim_next`/`mark_succeeded` lack `with_context` (T10) | Epic 3 | Bundle with error restructure |
| `claim_next` polling semantics (T10) | Epic 2 | Explicit sleep/backoff policy |
| Missing round-trip test: succeeded not re-claimable (T10) | Epic 2 | Add to state_claims tests |
| `YtDlpFetcher::acquire` error mapping (T11) | Epic 3 | Classifier covers it |
| `transcribe::transcribe` error mapping (T12) | Epic 1 (T11 deletes the function) | Resolved by Plan B Epic 1 |
| `parse_watched_at` UTC assumption (T13) | Epic 4 | AD0027 resolution path |
| `ingest::walk_recursive` polish (T13) | Epic 5 | Bundle with sync-IO sweep |
| Pipeline hardcodes fetcher/transcript_source (T14) | Epic 1 (T11) | Resolved by Plan B Epic 1 |
| `pipeline_fakes` test doesn't verify .json (T14) | Epic 1 (T11) | Resolved by Plan B Epic 1 |
| `output::shard_dir` unused (T15) | Epic 5 | Delete |
| `--whisper-model` global flag rejected after subcommand | Epic 5 | One-line `global = true` |
| SHORT_LINK_RE query parameters | Plan C | Short-link resolution is Plan C |
