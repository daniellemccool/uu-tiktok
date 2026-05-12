# Plan B Epic 3 — Full failure classification taxonomy (sketch)

**Status:** Sketch — detailed expansion at Epic 3 kickoff after Epic 2 ships.

**Goal:** Replace Epic 2's minimum string-based retryable/terminal mutators with the full typed failure taxonomy from the original spec. Wire classification into the orchestrator so different failure modes trigger different recovery behavior.

**Anticipated approx:** ~1 week, ~5–7 tasks.

## Scope

- **`RetryableKind` enum** per spec § "Error handling and failure classification": `NoMediaProduced`, `RateLimited`, `TransientNetwork`, `BadAudio`, `EmptyTranscript`, `OOM`, `ToolTimeout`, `ToolCrashedUnknown`, `YtDlpUnknown`, `WhisperUnknown`, `TransientStorage`.
- **`UnavailableReason` enum**: `Deleted`, `Private`, `LoginRequired`, `RegionBlocked`, `AgeRestricted`, `NoMediaInResponse`, `Other(String)`.
- **`ClassifiedFailure` enum**: `Retryable { kind, ctx }` | `Bug { ctx }`.
- **`FailureContext` struct**: tool, exit_code, stderr_excerpt, timeout, classification_reason.
- **Classifier functions**: `classify_fetch_error(&FetchError) -> ClassifiedFailure`, `classify_transcribe_error(&TranscribeError) -> ClassifiedFailure`.
- **Classification rules** per spec's classification tables (yt-dlp / ffmpeg / whisper.cpp patterns).
- **Default-cautious posture**: unrecognized stderr → `Retryable`, never `Bug`. `Bug` reserved for our defects.
- **Update Epic 2's minimum mutators** to accept typed kinds (e.g., `mark_retryable_failure(kind: RetryableKind, ctx: FailureContext)`).
- **Resolves FOLLOWUPS clustered at T6/T11/T12** (error mapping in `process.rs`, `YtDlpFetcher`, `transcribe.rs`) plus T10 `mark_succeeded` predicate (already done in Epic 2; this epic just verifies it composes).

## Anticipated ADRs

- **AD0024** Full failure taxonomy (RetryableKind / UnavailableReason / ClassifiedFailure enums + classification rules)
- **AD0025** Default-cautious posture (unknown → Retryable, not Bug)
- **AD0026** Bug class semantics (what triggers it, what doesn't)

## Anticipated files affected

```
src/errors.rs                       # RetryableKind, UnavailableReason, ClassifiedFailure, FailureContext, classifier fns
src/state/mod.rs                    # mark_retryable_failure signature: kind: RetryableKind, ctx: FailureContext
src/fetcher/ytdlp.rs                # classify on error paths; emit Acquisition::Unavailable for terminal verdicts
src/transcribe.rs                   # classify on error paths; Cancelled, OOM, EmptyTranscript distinctions
src/pipeline.rs                     # dispatch on ClassifiedFailure; Acquisition::Unavailable path
tests/errors.rs                     # table-driven classification tests per spec's classification tables
tests/pipeline_fakes.rs             # extend FakeOutcome for typed failures
```

## Key risks to flag at kickoff

- Pattern matching on stderr strings is brittle to tool updates. ADR should record the version of yt-dlp + whisper.cpp the patterns were validated against.
- ffmpeg patterns are observed second-hand (yt-dlp wraps it). The "No such file or directory for a path we just wrote → Bug" rule needs careful path-bookkeeping invariants.
- The `Bug` class must not become a catch-all; default to `Retryable` and require explicit pattern matches for `Bug` classification.

## Inputs the planner should consult

- The classification tables in `docs/superpowers/specs/2026-04-16-uu-tiktok-pipeline-design.md` § "Classification rules" (yt-dlp / ffmpeg / whisper).
- Captured stderr fixtures: Plan B should capture real stderr from a handful of known-failure URLs during Epic 1's bake (deleted videos, private videos, geo-blocked) and store them in `tests/fixtures/yt_dlp_responses/` for table-driven tests.
- Epic 2's minimum mutator signatures — Epic 3 enriches them rather than replacing.
