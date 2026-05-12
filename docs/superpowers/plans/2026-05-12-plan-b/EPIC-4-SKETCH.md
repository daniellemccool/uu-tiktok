# Plan B Epic 4 — Time-window filter + diagnostics (sketch)

**Status:** Sketch — detailed expansion at Epic 4 kickoff after Epic 3 ships.

**Goal:** Make the pipeline operational at study scale. Add the time-window filter on watch_history, resolve the DDP timezone assumption, and implement the `status` subcommand that fulfills Epic 1's "done"-contract ADR (AD0017).

**Anticipated approx:** ~½ week, ~4–5 tasks.

## Scope

### Time-window filter

- **DDP timestamp timezone resolution** (FOLLOWUPS T13). Two paths:
  - Empirical: pick a DDP export from a donor whose true watch time is known and compare parsed UTC against expected wall-clock. If skewed by exactly the donor's UTC offset, they're local times.
  - Documentary: search for a fresh TikTok DDP docs scrape that clarifies the timezone convention.
  - Record findings in AD0027.
- **`ingest --window-start` / `--window-end` flags**: absolute dates, both optional. Computed at ingest time; stored on every `watch_history` row.
- **`recompute-window` subcommand**: one-shot update of `in_window` flags across `watch_history`. Refuses to run without flags or `--clear` (silently wiping the entire study's filter would be too easy a mistake).

### `status` subcommand (implements Epic 1's "done"-contract ADR AD0017)

- **Counts by status**: pending, in_progress, succeeded, failed_terminal, failed_retryable.
- **Artifact-existence check**: walk succeeded rows, verify `.txt` and `.json` files exist at their sharded paths.
- **Raw-signals schema-version check**: parse each succeeded row's `.json` and verify `raw_signals.schema_version` matches the expected value.
- **`--video-id <ID>`**: full event history for one video (consumes the future `video_events` table or current `videos.updated_at` + last-error fields).
- **`--respondent-id <ID>`**: per-respondent summary fields per spec.
- **`--errors` / `--retryable`**: list failed videos with their respective columns.
- **`--json`**: output as JSON for tooling.

### DDP timezone treatment

Depending on AD0027's resolution path:

- **If UTC confirmed**: add a one-line doc-comment on `parse_watched_at` citing the evidence. No code change.
- **If local time confirmed**: store the original string alongside the i64 (add column), or add `respondent_timezone` captured at donation time, or document the i64 as "naive timestamp reinterpreted as UTC" and force every consumer to treat the offset as unknown.

## Anticipated ADRs

- **AD0027** DDP timestamp timezone treatment (UTC assumption resolution)
- **AD0028** Window flag semantics (computed at ingest; updated only via explicit `recompute-window`)
- **AD0029** Status subcommand output schema (counts shape; respondent-id summary fields)

## Anticipated files affected

```
src/cli.rs                                  # status subcommand + --window-start, --window-end on ingest, recompute-window subcommand
src/ingest.rs                               # window flag computation at ingest
src/state/mod.rs                            # add in_window column write; recompute_window method
src/state/schema.rs                         # SCHEMA_VERSION bump; in_window column on watch_history
src/status.rs                               # NEW — implements the status subcommand
tests/status.rs                             # status output assertions per AD0029
tests/recompute_window.rs                   # window recompute test; refuse-without-flags test
```

## Key risks to flag at kickoff

- If AD0027 resolves to "local time," several downstream consumers (Epic 5 export, future analytics) silently produce wrong-window results. Treat as a data-correctness blocker for any time-window assertion in tests or status output.
- The `status` subcommand reads files; at production scale (1M videos × ~5 files each) walking the disk takes time. Plan B operates at dev scale (~300 videos) so this isn't blocking but worth measuring.

## Inputs the planner should consult

- AD0017 (Epic 1's "done"-contract ADR) — defines what `status` must report.
- `docs/FOLLOWUPS.md` T13 entry — full context on the timezone question.
- The status subcommand's spec in `docs/superpowers/specs/2026-04-16-uu-tiktok-pipeline-design.md` § "CLI surface > status".
