# UU TikTok Pipeline — Plan A: Walking Skeleton

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Each task is its own file** in this directory (`01-init-crate.md` … `15-init-cmd.md`). Open only the task you're working on. Do NOT load the full design spec or all task files into a subagent's context — they're large and the per-task files are self-contained.

**Goal:** Stand up the minimal end-to-end pipeline that ingests one DDP-extracted JSON file, fetches one TikTok video's audio with yt-dlp, transcribes it with whisper.cpp (tiny.en, CPU), and writes the transcript artifact to a sharded directory. Dev profile only. Synchronous serial loop. Happy-path only.

**Architecture:** Single Rust binary, single crate. SQLite (file-backed, WAL) as state store. External CLI tools (yt-dlp, ffmpeg, whisper.cpp) do all heavy lifting via a shared subprocess runner. `VideoFetcher` trait introduced now so Plan B can swap implementations without restructuring. No async pipeline, no failure classification, no retry semantics, no short-link resolution — those land in Plans B and C.

**Tech Stack:** Rust 2021, tokio (used only for the subprocess runner; serial main loop), rusqlite (bundled), clap, serde, serde_json, chrono, tracing, async-trait, anyhow, thiserror, tempfile (dev), assert_cmd (dev).

**This is Plan A of three.** Plan B adds production hardening (async pipeline, error classification, retry, multi-instance). Plan C adds the operator surface (short-link resolution, status/requeue/export commands, comments). Each plan is meant to produce working, testable software on its own. Reassess design after Plan A's artifact exists.

**Reference:** Full design in `docs/superpowers/specs/2026-04-16-uu-tiktok-pipeline-design.md`. The plan implements a deliberate subset; the spec is the source of truth for "why." **Subagents implementing tasks should not need to open the spec** — each task file is self-contained.

---

## File Structure (after Plan A)

```
uu-tiktok/
├── Cargo.toml
├── src/
│   ├── main.rs               # CLI entry, profile resolution, dispatch
│   ├── cli.rs                # clap definitions
│   ├── config.rs             # Resolved Config struct + DEV defaults
│   ├── errors.rs             # FetchError, TranscribeError, FailureContext (minimal)
│   ├── canonical.rs          # URL → CanonicalVideoId | NeedsResolution
│   ├── process.rs            # Subprocess runner (spawn, timeout, stderr ring buffer)
│   ├── state/
│   │   ├── mod.rs            # Public Store API (Plan A subset)
│   │   └── schema.rs         # SQL schema + migrations
│   ├── fetcher/
│   │   ├── mod.rs            # VideoFetcher trait + Plan A's minimal Acquisition
│   │   └── ytdlp.rs          # YtDlpFetcher: audio-only happy path
│   ├── transcribe.rs         # whisper.cpp invocation; transcript text only
│   ├── output/
│   │   ├── mod.rs            # shard_path() helper
│   │   └── artifacts.rs      # Atomic write contract for {video_id}.txt + .json
│   ├── ingest.rs             # Walk inbox → parse DDP JSON → upsert via Store
│   └── pipeline.rs           # Serial process loop: claim → fetch → transcribe → write → succeed
└── tests/
    ├── canonical.rs          # Pure unit tests for URL forms
    ├── ingest.rs             # Real fixture → state → verify rows
    ├── pipeline_fakes.rs     # FakeFetcher; serial loop end-to-end without real tools
    ├── cli.rs                # assert_cmd: --help, init, ingest --dry-run
    └── e2e_real_tools.rs     # #[ignore] — real yt-dlp + whisper.cpp on one URL
```

**Files NOT created in Plan A (Plan B / Plan C):**

- `src/state/claims.rs` (claim transaction module — kept inline in `state/mod.rs` until contention warrants a split)
- `src/output/manifest.rs` (Plan C — parquet export)
- `src/state` extensions for short_links / failure persistence columns (Plan B / Plan C)
- Test fixtures `audio/`, `yt_dlp_responses/`, `api_responses/` (Plan B / Plan C as needed)

---

## Dependencies (`Cargo.toml`)

The exact set Plan A introduces:

```toml
[package]
name = "uu-tiktok"
version = "0.1.0"
edition = "2021"

[dependencies]
tokio = { version = "1", features = ["macros", "rt-multi-thread", "process", "time", "fs", "io-util"] }
rusqlite = { version = "0.31", features = ["bundled"] }
clap = { version = "4", features = ["derive", "env"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
chrono = { version = "0.4", default-features = false, features = ["std", "clock", "serde"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }
async-trait = "0.1"
anyhow = "1"
thiserror = "1"
regex = "1"
once_cell = "1"

[dev-dependencies]
tempfile = "3"
assert_cmd = "2"
predicates = "3"
```

Plan B will add: `tokio-util` (CancellationToken if we end up wanting graceful shutdown), `parquet` + `arrow` (Plan C). Plan C will add: `reqwest` (rustls-tls) for HEAD redirect resolution.

---

## Task Conventions

- **TDD throughout.** Each task: write the failing test, run it to confirm the failure, write minimum implementation, run to confirm pass, commit.
- **Commit per task** with a focused message. The plan supplies the message.
- **`cargo test` runs cleanly at the end of every task.** If a step adds a test that depends on later code, mark the test `#[ignore]` until the supporting code lands.
- **No `unwrap()` in non-test code** unless the unwrap is justified by an invariant the type system enforces (e.g., `String::from_utf8(known_valid).unwrap()`). Use `?` and `anyhow::Context` everywhere else.
- **Run `cargo fmt` and `cargo clippy --all-targets -- -D warnings` before each commit.** If clippy fires, fix the lint or `#[allow]` it with a one-line justification comment.

---

## Architectural Decision Records (ADRs)

ADRs live in `docs/decisions/` and are managed via the [adg](https://github.com/adr/ad-guidance-tool) tool. The format is MADR.

**When to write an ADR (mandatory triggers):**

- Structural patterns (file/module organization, abstraction boundaries)
- Tooling/process choices (linting policy, suppression conventions, hook designs)
- Intentional deviations from the plan's verbatim instructions
- Cross-task patterns where the decision will affect multiple future tasks

**Skip ADR for:**

- Implementation detail that follows obviously from the plan
- Bug fix (unless the fix establishes a pattern)
- Trivial cleanup

**Authorship:** The controller writes the ADR. Subagents that encounter a multi-alternative decision should pause and report back as `BLOCKED` or `DONE_WITH_CONCERNS` rather than choosing silently — they lack the project context to record reasoning effectively.

**Mechanics:**

```bash
adg add --model docs/decisions --title "<slug-safe title>"
# Avoid /, ;, [ in titles — adg's slugger interprets / as a path separator.
adg edit --model docs/decisions --id 000N \
  --question "..." \
  --option "..." [--option "..."]* \
  --criteria "..."
adg decide --model docs/decisions --id 000N \
  --option "..." --rationale "..."
```

**Reviewer obligation:** Every spec compliance reviewer and every code quality reviewer MUST scan `docs/decisions/` for decided ADRs (`ls docs/decisions/AD*.md` or `adg list --model docs/decisions`) and verify the work-under-review does not violate any decided ADR. Flag violations as blocking issues. New ADRs added since the previous task may also be relevant — don't assume the list is stable across tasks.

**Cleanup discipline (per ADR 0002):** When a task consumes a previously-dead type — one carrying `#[allow(dead_code)]` whose justification comment names this task as the future reader — remove the now-stale `#[allow(dead_code)]` as part of your work. Periodic backstop: `rg "allow\(dead_code\)" src/` to spot stale suppressions.

---

## Task Index

| # | File | Subject |
|---|------|---------|
| 1 | [01-init-crate.md](./01-init-crate.md) | Initialize crate with chosen dependencies |
| 2 | [02-cli-scaffold.md](./02-cli-scaffold.md) | CLI scaffolding (subcommand enum + global flags) |
| 3 | [03-config.md](./03-config.md) | Config struct + profile defaults |
| 4 | [04-errors.md](./04-errors.md) | Errors module (minimal types) |
| 5 | [05-canonical-urls.md](./05-canonical-urls.md) | URL canonicalization (forms 1 and 2) |
| 6 | [06-process-runner.md](./06-process-runner.md) | Subprocess runner (`process::run`) |
| 7 | [07-state-store.md](./07-state-store.md) | SQLite schema + `Store::open` + migrations |
| 8 | [08-output-helpers.md](./08-output-helpers.md) | `output::shard_path` + atomic write helper |
| 9 | [09-store-ingest.md](./09-store-ingest.md) | `Store` ingest methods (upsert) |
| 10 | [10-store-claims.md](./10-store-claims.md) | `Store::claim_next` + `mark_succeeded` (transactional) |
| 11 | [11-video-fetcher.md](./11-video-fetcher.md) | `VideoFetcher` trait + `YtDlpFetcher` |
| 12 | [12-transcribe.md](./12-transcribe.md) | `transcribe` module |
| 13 | [13-ingest-cmd.md](./13-ingest-cmd.md) | `ingest` subcommand |
| 14 | [14-process-cmd.md](./14-process-cmd.md) | `process` subcommand (serial loop) + e2e smoke test |
| 15 | [15-init-cmd.md](./15-init-cmd.md) | `init` subcommand and end-to-end smoke |

---

## Plan A Exit Criteria

After Task 15 is committed, the following commands work end-to-end on the dev profile:

```bash
cargo build --release
./scripts/fetch-tiny-model.sh
mkdir test-run && cd test-run
cp -r ../tests/fixtures/ddp ./inbox

../target/release/uu-tiktok init
../target/release/uu-tiktok ingest
../target/release/uu-tiktok process --max-videos 3

# Inspect:
ls transcripts/*/[0-9]*.txt  # see real transcripts
sqlite3 state.sqlite "SELECT video_id, status FROM videos WHERE status = 'succeeded';"
```

`cargo test --features test-helpers` passes with all non-ignored tests green.

`cargo test --features test-helpers --test e2e_real_tools -- --ignored --nocapture` also passes (slow; requires real network + tiny.en model).

**The walking skeleton is alive. Reassess from this point before writing Plan B.**

---

## What Plan A Deliberately Omits

These are the things Plan B and Plan C will add. Listed so the engineer doesn't accidentally implement them now:

- Async/pipelined orchestrator (Plan A is strictly serial)
- GPU semaphore / multi-GPU coordination
- Multi-instance lifecycle (single instance only)
- Failure classification (`RetryableKind`, `UnavailableReason`, `ClassifiedFailure`)
- Failure persistence columns (`last_retryable_*`, `terminal_*`, `next_retry_at`-not-applicable)
- `Acquisition::Unavailable` and `ReadyTranscript` variants — Plan A has only `AudioFile`
- Bug-class supervision via JoinSet (we just propagate errors)
- Stale-claim recovery (Plan B)
- `requeue-retryables`, `reset-stale-claims`, `recompute-window`, `status` subcommands
- Short-link resolution (`pending_resolutions`, HEAD redirect follower, `resolve-short-links`)
- `prod` profile (`large-v3`, GPU)
- Comments fetching (`fetch_comments`, `comments.json`)
- Raw metadata persistence (`metadata.raw.json`)
- Normalized video metadata (`metadata.json` with the union schema)
- Manifest parquet export
- Tier 3 against curated public URLs (we have one stub URL only)
- Stress test (1000 fake videos)
- `process.rs` doesn't redact arguments yet beyond the API surface
- Tracing-subscriber JSON format hasn't been verified end-to-end
- `--dry-run` flag on `ingest` does not actually short-circuit yet

---

## Self-Review Checklist (run by author after writing)

**Spec coverage:** Plan A maps to spec sections "High-level architecture" (subset: serial), "Components and module boundaries" (creates the boundary files), "Data flow and state machine" (subset: claim → fetch → transcribe → succeed), "Schemas: SQLite + transcript + transcript metadata", "CLI surface" (init, ingest, process only), and "Atomic write contract." Sections explicitly out of scope: failure classification, retries, multi-instance, short-link resolution, manifest, comments — all flagged in "Plan A Deliberately Omits" above.

**Placeholder scan:** None of the no-placeholder anti-patterns ("TBD", "TODO", "implement later", "add appropriate error handling") appear in task steps. Each TDD step has actual code. The `e2e_real_tools` test is `#[ignore]` and the operator-curated URL is documented as needing replacement.

**Type consistency:** `Acquisition` only has `AudioFile` here (matches what we use). `Store` methods (`upsert_video`, `upsert_watch_history`, `claim_next`, `mark_succeeded`) used consistently across tasks. `SuccessArtifacts` field names (`duration_s`, `language_detected`, `fetcher`, `transcript_source`) match the SQL columns and the `videos` table. `TranscribeResult` field names (`text`, `language`, `duration_s`) match what the pipeline reads. `cli::Profile` is `Dev` only — `Prod` is for Plan B.

**Scope:** ~15 tasks, each producing a meaningful increment with TDD + commit. Final state is a runnable binary that ingests the real test fixture and produces real transcripts with real tools (manual e2e). This is a single coherent plan; further increments belong in Plans B and C.

**Ambiguity:** Each step shows exact code, exact commands, and expected output. Module wiring (`mod` declarations in `main.rs` and `lib.rs`) is called out per task. Cargo feature gating for test helpers is documented inline.
