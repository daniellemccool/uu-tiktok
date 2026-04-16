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
