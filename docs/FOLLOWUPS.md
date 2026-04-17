# Followups — known issues spotted but not yet acted on

Ad-hoc tracker for things found during code review that don't warrant
immediate action but shouldn't be lost. Each entry should name the task
or context where the finding arose, the disposition (deferred / planned /
accepted), and the trigger that should re-surface it.

When an entry is resolved, remove it from this file (git history retains it).

---

## SHORT_LINK_RE does not handle query parameters on short links

**Found in:** T5 code quality review.
**Disposition:** Deferred to Plan C.
**Trigger to revisit:** Plan C planning session, before short-link resolution lands.

The short-link regex in `src/canonical.rs` ends with `/?$`:

```
^https?://(?:vm\.tiktok\.com|vt\.tiktok\.com|(?:www\.)?tiktok\.com/t)/[A-Za-z0-9]+/?$
```

This means a tracking-parameterized short link such as
`https://vm.tiktok.com/ZMabcdef/?utm_source=share` falls through to
`Canonical::Invalid` rather than `Canonical::NeedsResolution`.

CANONICAL_RE handles `?` correctly via `(?:/|\?|$)`. The asymmetry is real.

**Plan A impact:** small. Plan A only logs short links and skips them; the
miscategorization just shifts a count from `short_links_skipped` to
`invalid_urls_skipped` in `IngestStats`. Both end up not transcribed.

**Plan C impact:** real. Plan C will pick up rows from `pending_resolutions`
for HEAD-redirect resolution. Query-stringed short links would never reach
that table → silent data loss for those URLs.

**Suggested fix (when Plan C lands):** change the SHORT_LINK_RE suffix to
something like `(?:/[A-Za-z0-9]*)?(?:\?.*)?$` (match optional trailing slash,
then optional query string). Add a coverage test for both forms.

If DDP exports turn out to commonly include `?utm_source=…` on shared short
links, consider promoting this to a fixed bug in Plan B's first iteration
rather than waiting for Plan C — depends on what the donation extraction
script actually emits.

---

## `process::run` buffers full stderr/stdout in memory before truncation

**Found in:** T6 code quality review (opus).
**Disposition:** Deferred to Plan B (concurrent fetches make it matter).
**Trigger to revisit:** Plan B's fetch-orchestrator design.

`src/process.rs` reads the entire stdout AND stderr streams into `Vec<u8>` via
`read_to_end` before `ring_buffer_tail` slices the tail down to
`stderr_capture_bytes`. The `cap` only bounds the *retained excerpt* in the
returned `CommandOutcome`; it does not bound peak memory.

For Plan A's curated tools (yt-dlp/ffmpeg/whisper.cpp on a single video) this
never matters in practice. For Plan B's many-concurrent-fetches scenario, a
misbehaving tool that emits 10GB to stderr would allocate 10GB in this process
before truncation.

**Suggested fix (Plan B):** replace `read_to_end` with a streaming reader that
maintains a rolling `VecDeque<u8>` of size `cap`, dropping bytes beyond `cap`
during accumulation. Optionally cap stdout too with a separate
`stdout_capture_bytes` (yt-dlp writes audio to a file so its stdout is small,
but defense-in-depth).

The doc comment on `stderr_capture_bytes` was updated in T6 fixup to honestly
describe the current behavior.

---

## `ring_buffer_tail` is misnamed (it's not a ring buffer)

**Found in:** T6 code quality review (opus).
**Disposition:** Bundle with the bounded-buffering fix above.
**Trigger to revisit:** Plan B's bounded-buffering work.

The function name `ring_buffer_tail` suggests ring-buffer semantics, but the
implementation is a tail-of-slice helper. A clearer name (`tail_excerpt` or
`last_n_bytes_lossy`) would set the right expectations. Defer the rename to
when the bounded-buffering fix lands so we touch this code only once.

---

## `From<RunError> for FetchError` collapses Spawn and Io into NetworkError

**Found in:** T6 code quality review (opus).
**Disposition:** Deferred to Plan B (failure classification work).
**Trigger to revisit:** Plan B introduces `RetryableKind` /
`UnavailableReason` / `ClassifiedFailure`.

The current mapping in `src/process.rs`:

- `RunError::Spawn` → `FetchError::NetworkError` (binary missing or fork
  failure — environmental/configuration, terminal)
- `RunError::Io` → `FetchError::NetworkError` (pipe read failure — system,
  potentially transient)
- `RunError::Timeout` → `FetchError::ToolTimeout` (correct as-is)

Both Spawn and Io being labeled "NetworkError" will misguide Plan B's
retry/backoff logic: a missing binary should not be retried with network
backoff (the binary will still be missing). Plan B should split these into
dedicated variants (e.g., `FetchError::ToolNotFound`, `FetchError::ConfigError`,
`FetchError::SystemIo`) and classify them appropriately.

A one-line note above the `From` impl in `src/process.rs` points here.

---

## `status.code().unwrap_or(-1)` loses signal information

**Found in:** T6 code quality review (opus).
**Disposition:** Deferred to Plan B (failure classification work).
**Trigger to revisit:** Plan B's classification needs to distinguish OOM-kill
(SIGKILL by oom-killer), user cancel (SIGINT), and crash (SIGSEGV).

When a child is killed by a signal, `status.code()` returns `None`, and the
current code collapses that to the sentinel `-1`. Recovering the signal number
requires `std::os::unix::process::ExitStatusExt::signal()`.

For Plan A this is fine: in-scope timeouts go through the `Timeout` arm before
`code()` is read; out-of-scope kills are rare.

For Plan B's failure classification, distinguishing OOM-kill from
user-cancelled from segfault matters for retry decisions. Plan B should expand
`CommandOutcome` with a `signal: Option<i32>` field (Unix-only via cfg), or
introduce a richer `CompletionStatus` enum.

---

## `Store::open` records `schema_version` but never reads-and-checks it

**Found in:** T7 code quality review (opus).
**Disposition:** Deferred to Plan B (first schema change).
**Trigger to revisit:** any task that changes `state::schema::SCHEMA_SQL`.

`Store::open` writes the schema version to `meta` on first run via
`INSERT OR IGNORE`, but no subsequent open verifies the stored version against
the current `SCHEMA_VERSION` constant. A Plan B `Store::open` running against
a Plan A database would silently keep the old schema (CREATE IF NOT EXISTS
doesn't migrate).

The decision the project will eventually need to make is multi-alternative —
worth recording as a proper ADR before Plan B's first schema change:

- (a) Hard-fail `Store::open` on version mismatch
- (b) Auto-migrate forward via numbered migration scripts
- (c) Refuse to open older versions but allow newer (read-only)
- (d) Log warning on mismatch, proceed anyway (current behavior — silent)

Lowest-cost stopgap before Plan B: a one-line `tracing::warn!` in `Store::open`
when stored version differs from `SCHEMA_VERSION`. Converts silent drift into
a loud signal at near-zero cost.

---

## `Store::pragma_string` visibility is `pub`, not `pub(crate)`

**Found in:** T7 code quality review (opus).
**Disposition:** Defer to bin/lib structural reassessment (per ADR 0002).
**Trigger to revisit:** Plan A reassessment point — when bin/lib pattern is decided.

`Store::pragma_string` is currently `pub` (matches the per-task file's
verbatim spec text). It builds `format!("PRAGMA {}", name)` because PRAGMA
names cannot be parameterized in SQLite. Today the only caller is the
`pragma_journal_mode_is_wal` integration test passing the literal
`"journal_mode"`, but `pub` visibility means external library consumers
could pass attacker-controlled or malformed names.

Two reasonable fixes when this is revisited:

- Lower visibility to `pub(crate)` (matches `conn`/`conn_mut`); only the
  integration test would need adjustment, possibly via a `test-helpers`
  feature gate.
- Switch the implementation to `rusqlite::Connection::pragma_query_value`,
  which validates the pragma name internally.

Coupled to AD0002's deferred bin/lib structural decision because the
"is this part of the public library API?" question depends on whether the
project ends up thin-binary, fat-library or stays with the dual-`mod`
pattern.

---

## `Store::read_meta` could use `OptionalExtension::optional()`

**Found in:** T7 code quality review (opus).
**Disposition:** Style improvement; defer indefinitely.
**Trigger to revisit:** any future edit to `Store::read_meta`.

The current implementation uses `map_or_else` to translate
`QueryReturnedNoRows` to `Ok(None)`. Functionally correct but verbose. The
idiomatic rusqlite pattern is `query_row(...).optional()` with the
`OptionalExtension` trait. Pure refactoring — not blocking anything; touch
this code only when there's a real reason to.

---

## `output::shard` slices by bytes; panics on non-ASCII input

**Found in:** T8 code quality review (opus).
**Disposition:** Latent footgun; defer to whenever a `VideoId` newtype is introduced.
**Trigger to revisit:** any task that introduces a typed `VideoId`, or any task that begins accepting video IDs from a source other than the DDP-JSON parser.

`src/output/mod.rs::shard` does `&video_id[len-2..]`, which slices by bytes.
For multi-byte UTF-8 input where `len-2` lands mid-codepoint, this panics.
Real TikTok video IDs are ASCII digits and Plan A's parser only ever produces
those, so this is not exploitable today. The function takes `&str` rather
than a `VideoId` newtype, so the ASCII-only contract is implicit.

The natural fix arrives whenever the project introduces a `VideoId` newtype
(probably Plan B or Plan C, when DB rows and trait boundaries start passing
IDs around as values rather than `&str`). At that point, `shard` should be
a method on `VideoId` and the byte-slice is safe by construction.

Lowest-cost stopgap before then: add a debug assertion or a one-line doc
comment stating the ASCII-only contract.

---

## `output::cleanup_tmp_files` minor cleanups: missing context, overcounted removals

**Found in:** T8 code quality review (opus).
**Disposition:** Cosmetic; bundle with the next real edit to this function.
**Trigger to revisit:** any task that touches `cleanup_tmp_files`, or T15 (init-cmd) when wiring the call site.

Two small inconsistencies in `src/output/artifacts.rs::cleanup_tmp_files`:

1. The inner `std::fs::read_dir(&path)?` and the surrounding `entry?` /
   `shard_entry?` lines bubble up raw `io::Error` without path context. The
   outer `read_dir(transcripts_root)` is contextualized via `with_context`.
   On a permission-denied inside one shard dir, the operator gets a path-less
   error.

2. `let _ = std::fs::remove_file(&p); removed += 1;` increments
   unconditionally. If `remove_file` fails (permission, EBUSY), the returned
   count overstates the cleanup. Best-effort semantics are fine; the count
   just shouldn't claim success it didn't achieve.

Neither is a behavioural bug for Plan A's happy-path single-process loop.
Worth fixing when this function next gets touched.

---

## `output::shard_distributes_uniformly` test rationale is reversed

**Found in:** T8 code quality review (opus).
**Disposition:** Cosmetic; comment is misleading but the assertion still
catches the stated regression.
**Trigger to revisit:** any future edit to the test, or whenever a
`VideoId` newtype absorbs `shard()` and the test moves with it.

`src/output/mod.rs::shard_distributes_uniformly` uses monotonic counter
input (`base + i` for `i in 0..10000`), which produces exactly 100 items per
last-two-digits bucket. The ±50% assertion (`50..=150`) passes with a
margin of 0%, not because the bound is "lenient for synthetic input" as the
comment claims.

The comment says "real Snowflake IDs would be tighter" — that's reversed.
Real Snowflake low bits are pseudorandom; their per-bucket variance over
10k samples is Poisson-like (~10% std dev), so real IDs would be looser,
not tighter, than the artificially perfect counter cycle.

The test still catches the "uses high digits instead of low" regression via
the `counts.len() == 100` assertion (high digits are time-clustered, so a
high-digits implementation would collapse to 1-2 buckets). The bounds check
is decorative for this input; either tighten it (e.g., assert exact equality
to 100) or replace the input with a PRNG-driven sample to exercise the
bound meaningfully.

---

## `videos.updated_at` is frozen at first-seen by `upsert_video`

**Found in:** T9 code quality review (opus).
**Disposition:** Accepted for T9; re-evaluate as T10/T13 land.
**Trigger to revisit:** T10 (`claim_next` / `mark_succeeded`), T13 (ingest cmd),
or any future Store mutator that touches a `videos` row.

`Store::upsert_video` uses `INSERT OR IGNORE` and binds the same `now` value to
both `first_seen_at` and `updated_at`. On a re-upsert, neither column is
written. The brief's idempotence test only asserts `first_seen_at` is
unchanged, but `updated_at` is equally frozen — which contradicts the natural
reading of the column name ("when was this row last touched").

For pure-ingest semantics this is correct: nothing about the row changed. But
T10's `claim_next` / `mark_succeeded` and any later mutators MUST remember to
bump `updated_at` themselves, since `upsert_video` will not update it on
subsequent calls. If they forget, `updated_at` becomes a misnomer.

Two reasonable resolutions when this surfaces:

- Accept the contract: rename to `inserted_at` (or document `updated_at` as
  "last write to mutable columns, not including idempotent re-upsert").
- Switch `upsert_video` to `INSERT ... ON CONFLICT(video_id) DO UPDATE SET
  updated_at = excluded.updated_at` — preserves `first_seen_at` and
  `source_url` invariants while bumping `updated_at` on every observation.
  Add a regression test asserting `updated_at` strictly increases on
  re-upsert and `first_seen_at` does not.

The choice depends on whether `updated_at` is meant as "last-mutation marker"
(useful for stale-claim detection in Plan B) or "last meaningful state
change". Plan B's stale-claim recovery is the most likely first consumer that
will care.

---

## `Store::conn` / `Store::conn_mut` accessor hygiene after T10

**Found in:** T9 code quality review, re-confirmed in T10 review (opus).
**Disposition:** Cleanup commit, or fold into AD0002's bin/lib
reassessment.
**Trigger to revisit:** Plan A reassessment point, or any task that
genuinely needs `&Connection` / `&mut Connection` outside `Store`'s
own `impl`.

`src/state/mod.rs` lines 105 and 111 carry `#[allow(dead_code)]` with
comments naming T9 and T10 as the first consumers. Both tasks have now
landed and both went via direct `self.conn` field access. The comments
are factually wrong.

Current state of consumers:
- `Store::conn` — used only by the `#[cfg(test)]` NULL-rejection
  unit tests at `src/state/mod.rs::tests::null_video_id_rejected_*` and
  `null_meta_key_rejected_*`. So it has one real consumer, gated to
  test compilation.
- `Store::conn_mut` — no consumer at all.

Resolution options:

- Lowest-cost: delete `conn_mut` outright; rewrite the `conn()` comment
  to say "used by cfg(test) schema invariant tests; keep until lib API
  stabilizes."
- Structural: defer to AD0002's reassessment — under Option 4
  (thin-binary fat-library) the `pub(crate)` accessors may go away
  entirely.

Per AD0002's cleanup discipline, the `rg "allow\(dead_code\)" src/`
audit catches this on every pass.

---

## `concurrent_claim_serializes_via_begin_immediate` doesn't actually race

**Found in:** T10 code quality review (opus).
**Disposition:** Test-quality gap; defer until Plan B introduces real
concurrency (multi-instance / async pipeline).
**Trigger to revisit:** Plan B's first multi-worker design, or any change
to the `claim_next` transaction shape.

`tests/state_claims.rs::concurrent_claim_serializes_via_begin_immediate`
creates two `Store` handles to one DB file but invokes `claim_next` on
them sequentially on the main thread. The first call commits before the
second begins, so the second naturally finds no pending row. The
`BEGIN IMMEDIATE` write-lock path, `busy_timeout = 5000`, and the WAL
writer-exclusion contract are never exercised — a regression that
downgraded the transaction to `BEGIN DEFERRED` or removed it entirely
would still pass this test.

**Suggested fix:** rewrite using `std::thread::spawn` + `std::sync::Barrier`
so both threads enter `claim_next` simultaneously, then assert exactly
one returns `Some` and the other returns `Ok(None)` (or, with one row,
that the loser observes the row already `in_progress`). For two-worker
contention with multiple pending rows, assert each worker claims a
distinct `video_id`. Out-of-scope for Plan A's serial loop; Plan B's
multi-worker design will need this anyway.

---

## `mark_succeeded` doesn't require `status = 'in_progress'`

**Found in:** T10 code quality review (opus).
**Disposition:** Defensive-programming gap; defer to Plan B (state
machine + recovery).
**Trigger to revisit:** Plan B's stale-claim recovery / retry design, or
any task that grows additional state-transition mutators.

`Store::mark_succeeded` does an unconditional UPDATE — no `WHERE
status = 'in_progress'` predicate. A caller that invokes it on a
`pending`, already-`succeeded`, or `failed_*` row silently transitions
the row to `succeeded`. For Plan A's strictly-serial loop (claim → fetch
→ transcribe → succeed within one synchronous call) this cannot happen,
so it's accepted for now.

For Plan B this becomes a real concern: stale-claim recovery, retry
flows, and any out-of-order mutator could land here. Either:
- Add a `WHERE status = 'in_progress' AND claimed_by = ?` predicate and
  return an error (or `bool`) when 0 rows update; or
- Introduce a typed state-machine layer above `Store` that gates
  transitions before SQL emission.

The same observation applies to the future `mark_failed_terminal` /
`mark_failed_retryable` mutators that Plan B will add — bake the gate
into the convention before they're written.

---

## `claim_next` / `mark_succeeded` inner statements lack `with_context`

**Found in:** T10 code quality review (opus).
**Disposition:** Cosmetic; bundle with the next real edit to these
functions.
**Trigger to revisit:** Plan B (failure classification will likely
restructure error mapping anyway), or whenever a real bug surfaces
without enough context to diagnose.

`Store::claim_next` wraps the `transaction_with_behavior` and `commit`
with `.context(...)` but its inner `tx.execute(...)` calls (UPDATE
videos and INSERT video_events) bare-`?` raw `rusqlite::Error`. Same in
`Store::mark_succeeded` for the INSERT video_events statement (the
videos UPDATE is correctly contextualized via `with_context`).

A FK violation or other constraint failure on those statements surfaces
without `worker_id` / `video_id` context. Operationally fine for Plan
A's single-row happy path; worth tightening when failure classification
lands in Plan B.

---

## Plan B reassessment: `claim_next` polling semantics

**Found in:** T10 code quality review (opus).
**Disposition:** Defer to Plan B's process-loop / multi-instance design.
**Trigger to revisit:** Plan B planning session.

Two related concerns about how `Store::claim_next` will behave under
Plan B's concurrent / multi-instance workloads, neither relevant to
Plan A's serial single-process loop:

1. **Empty-DB path commits an empty IMMEDIATE transaction.** When no
   pending row exists, `claim_next` calls `tx.commit()?` before
   returning `Ok(None)`. Functionally correct — committing an empty
   transaction releases the RESERVED lock the same as rollback would —
   but a hot polling loop that finds nothing on every tick churns the
   write lock. `drop(tx)` would be marginally cheaper and clearer
   about "we did nothing." Plan B should decide whether the polling
   loop short-polls (then the change matters) or sleeps between polls
   (then it doesn't).

2. **`BEGIN IMMEDIATE` + `busy_timeout = 5000` blocking semantics.**
   A worker that finds another worker mid-claim will block up to 5
   seconds inside `transaction_with_behavior` waiting for the lock.
   For Plan A (one worker) this never fires. For Plan B's
   multi-worker design, the choice between "block up to N seconds"
   and "fail fast and back off" is a design decision that should be
   explicit, not inherited from the per-connection PRAGMA.

Both concerns out of scope for T10 — flag for the Plan A → Plan B
reassessment point.

---

## Missing round-trip test: succeeded videos must not be re-claimable

**Found in:** T10 code quality review (opus).
**Disposition:** Coverage gap; defer until next edit to state_claims.rs
or T14 (process serial loop) lands a higher-level e2e fake-fetcher test.
**Trigger to revisit:** T14 implementation, or any change to
`claim_next`'s status filter.

`tests/state_claims.rs` exercises each transition independently
(`claim_next` of a pending row, `mark_succeeded` of an in_progress row)
but never composes `claim_next` → `mark_succeeded` → `claim_next` and
asserts the second claim returns `Ok(None)`. A regression that, say,
changed the SELECT predicate to `WHERE status IN ('pending',
'succeeded')` would not be caught by the current suite. T14's
end-to-end fake-fetcher tests will likely cover this incidentally;
if they don't, add a one-liner here.

---

## `YtDlpFetcher::acquire` error mapping and yt-dlp output-filename coupling

**Found in:** T11 code quality review (opus).
**Disposition:** Deferred. Findings 1–2 fold into Plan B's failure-classification
work; finding 3 is hardening; finding 4 is Plan C scope.
**Trigger to revisit:** Plan B's `RetryableKind` / `UnavailableReason` design
(findings 1, 2); Plan B's fetch-orchestrator hardening (finding 3); Plan C's
short-link resolution (finding 4).

Four concerns in `src/fetcher/ytdlp.rs::acquire`, none blocking for Plan A's
serial happy path:

1. **`create_dir_all` failure → `FetchError::NetworkError`.** Filesystem
   ENOSPC / EACCES is not a network condition. Will misclassify into Plan B's
   network-backoff path. Extends the existing T6 follow-up on
   `From<RunError>`'s coarse mappings — same root cause (`FetchError`
   variants too coarse), additional symptom (the mismapping now happens inside
   `acquire` itself, not just at the `From` boundary).

2. **Post-success `wav_path.exists() == false` → `FetchError::ParseError`.**
   `ParseError` means "couldn't parse tool output." This case is "tool
   succeeded but artifact convention was violated" — closer to a tool-contract
   postcondition error. Same Plan B classification work catches this. (The
   `FakeFetcher` missing-fixture error reuses `ParseError` similarly; that one
   is test-only and cosmetic.)

3. **Tight coupling to yt-dlp's `{video_id}.wav` output filename.** The
   `wav_path.exists()` check assumes yt-dlp's `--audio-format wav` +
   `%(ext)s` template always produces exactly `{video_id}.wav`. If yt-dlp
   emits a sanitized variant, intermediate partial files, or a suffix for
   collisions, the check fails despite a successful exit. A robustness
   improvement: scan `video_dir` for any `.wav` after success, or glob
   `{video_id}.*.wav`. Defer to Plan B's fetch-orchestrator hardening.

4. **`source_url` is bound as the last positional arg with no `--` separator.**
   Today this is safe because `source_url` always comes from `Canonical::Valid`
   whose regex anchors `^https?://`. Plan C will introduce short-link
   resolution that produces resolved URLs from external sources; an attacker-
   controlled or malformed URL beginning with `-` could be reinterpreted as a
   yt-dlp flag. One-line defense: insert `"--".into()` immediately before
   `source_url.to_string()` in the `args` vector. Land this when Plan C wires
   resolved URLs into the fetcher pipeline.

---

## `transcribe::transcribe` error mapping is inconsistent and lossy

**Found in:** T12 code quality review (opus).
**Disposition:** Deferred. Folds into Plan B's failure-classification work
alongside the existing T6 / T11 entries.
**Trigger to revisit:** Plan B's `RetryableKind` / `UnavailableReason` /
`ClassifiedFailure` design.

Three concerns in `src/transcribe.rs::transcribe`, none blocking for Plan A's
serial happy path:

1. **Inline `.map_err(|e| match e {...})` instead of `From<RunError> for TranscribeError`.**
   T6 chose the `From` idiom for `FetchError` so fetcher code can use `?`
   directly; T12 chose the inline match. Brief's intentional choice (no
   `From<RunError> for TranscribeError` impl in `errors.rs`), but Plan B's
   failure-classification work should harmonize on one idiom across both
   error types.

2. **`exit_code: -1` sentinel collapses non-Timeout RunError variants.**
   `RunError::Spawn`, `RunError::Io`, and any Plan B additions all collapse
   to `TranscribeError::Failed { exit_code: -1, stderr_excerpt: other.to_string() }`.
   Same loss-of-signal already flagged for T6's `From<RunError> for FetchError`
   and `status.code().unwrap_or(-1)`. Whisper-cli OOM (signal kill) and
   missing whisper-cli binary become indistinguishable to a downstream
   classifier.

3. **`exit_code: 0` for post-success artifact-read failure is misleading.**
   When `std::fs::read_to_string(&txt_path)` fails after a 0-exit
   whisper-cli run, the error is built as
   `TranscribeError::Failed { exit_code: 0, stderr_excerpt: "reading {path}: {io_err}" }`.
   A downstream consumer reading `exit_code: 0` would conclude the tool
   succeeded; the failure was actually in the artifact-reading step.
   Parallel to T11's `wav_path.exists() == false → FetchError::ParseError`
   mismatch. Plan B should introduce a dedicated variant
   (e.g., `TranscribeError::ArtifactMissing` /
   `TranscribeError::ArtifactUnreadable`).

---

## `parse_watched_at` assumes DDP `Date` strings are UTC; TikTok docs are silent

**Found in:** T13 code quality review (opus).
**Disposition:** Real semantic risk; defer until evidence is available about
TikTok's DDP timestamp convention.
**Trigger to revisit:** any task that begins comparing `watch_history.watched_at`
against an externally-meaningful time (Plan B's time-window filter, Plan C's
status/export commands, or any operator inspecting a single donor's timeline);
also any DDP-docs refresh that adds a timezone annotation to the
"Browsing History" data type.

`src/ingest.rs::parse_watched_at` parses TikTok DDP's `Date` field with
`NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")` and then converts via
`Utc.from_utc_datetime(&naive)`, baking a UTC assumption into every
`watch_history.watched_at` i64. The TikTok Data Portability API documentation
in this repo (`docs/reference/tiktok-for-developers/markdown/doc_data-portability-data-types.md`)
lists the Browsing History `Date` field with no timezone annotation. The only
"UTC" mentions in the DDP corpus apply to API request/response timestamps
(`docs/...check-status-of-data-request.md` lines 1955 / 1963), not to data
inside the export. If DDP `Date` is actually the user's local wall-clock —
plausible since DDP renders into the user's locale — every `watched_at` is
off by the user's UTC offset (1–2h for NL donors), silently miscategorizing
any time-window filter built on top.

**Plan A impact:** none. Plan A only persists the i64 and never compares it.

**Plan B impact:** real if a time-window filter or stale-claim recovery uses
`watched_at` as input. Stale-claim recovery uses `claimed_at` (server-side
clock, not affected); the time-window filter is the load-bearing case.

**Plan C impact:** real for status/export. A donor inspecting their own
timeline will see times shifted by their own UTC offset.

**Suggested resolution paths (when this surfaces):**

1. Empirically check a known donation: pick a DDP export from a donor whose
   true watch time is known (e.g., the test fixture's owner) and compare
   parsed UTC against expected wall-clock. If skewed by exactly the donor's
   UTC offset, they're local times.
2. Find authoritative TikTok statement (developer-relations contact, source
   inspection of the DDP renderer, or a fresh docs scrape post-2026-04-16).
3. If local: store the original string alongside the i64 (add column, or
   defer parsing to display time), or add a `respondent_timezone` column
   captured at donation time, or document the i64 as "naive timestamp
   reinterpreted as UTC" and force every consumer to treat the offset as
   unknown.
4. If UTC: add a one-line doc-comment on `parse_watched_at` citing the
   evidence so the next reader doesn't re-litigate.

The verbatim T13 brief made this assumption silently. Recording the gap so
the project can answer it deliberately rather than discover it via a
data-quality bug.

---

## `ingest::walk_recursive` minor polish: silent missing-inbox + missing inner context

**Found in:** T13 code quality review (opus).
**Disposition:** Cosmetic; bundle with the next real edit to `ingest::*`.
**Trigger to revisit:** any task that touches `walk_recursive` or `ingest`
error-handling.

Two small inconsistencies in `src/ingest.rs`:

1. `walk_recursive` returns `Ok(())` if the root inbox doesn't exist, so an
   operator who passes a typo to `--inbox` gets a successful run with
   `files=0` and no error. Cheap defense: `bail!` at the top-level `ingest()`
   call when the root doesn't exist. Deeper subdirectories disappearing
   mid-walk is a different story (race; acceptable to ignore).

2. The outer `read_dir(transcripts_root)` is contextualized via
   `with_context`; the inner `entry?` and recursive `walk_recursive(&path,
   out)?` calls bubble up raw `io::Error` without path context. Same minor
   pattern as `output::cleanup_tmp_files` already in FOLLOWUPS. On a
   permission-denied inside one shard subdirectory, the operator gets a
   path-less error.

Both fine for Plan A's happy-path single-process loop; worth fixing when
this code next gets touched.

