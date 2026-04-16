# UU TikTok Pipeline — Design

Date: 2026-04-16
Status: Draft (under review)
Owner: Danielle McCool

## Background

A UU researcher is running a data donation study on Prolific (US sample) examining exposure to crime and crime-related news through TikTok. Respondents donate their TikTok data export (DDP). The donation platform extracts watch history (URLs of videos watched) per respondent and stores it. This pipeline runs **after** donation, on the analysis side: it consumes the per-respondent watch-history JSONs, fetches each video's audio, transcribes it to plain text, and stores the transcripts for downstream analysis (PII redaction by a separate LLM pass, then crime exposure classification by the researcher).

The same researcher has a future U18 study on hate-speech exposure that will likely receive only TikTok's Virtual Compute Environment (VCE), not the Research API. The pipeline designed here is for the **crime study**, which has been granted Research API access. The VCE study will use a different (in-environment) approach and is out of scope.

### Operating model

- **Local development** on the engineer's machine (no GPU; `tiny.en` Whisper model on CPU, small fixture data).
- **Production** on SURF Research Cloud workspace with GPU (A10/A100). Per-batch invocation as donations arrive, not a long-running daemon. Operator manually moves DDP JSONs into an inbox folder, runs `process`, inspects, and runs subsequent waves as new donations arrive.

### Scale (estimated)

- ~1000 respondents
- 3-month watch-history window per respondent
- Thousands of videos per respondent → likely millions of `(respondent, video)` rows pre-deduplication
- Heavy video-level deduplication expected (viral content shared across respondents) — unit of work is **canonicalized `video_id`**, not `(respondent, url)` pair

### In scope

- Reading donation-side watch-history JSONs from a local inbox folder
- URL canonicalization to `video_id`; deduplication
- Per-video acquisition (download audio via yt-dlp + ffmpeg)
- Per-video transcription (whisper.cpp)
- On-disk artifacts: transcript, transcript metadata, video metadata
- SQLite as source of truth for pipeline state
- Operator CLI for ingest, process, inspect, requeue, export
- Boundary that allows swapping yt-dlp for the TikTok Research API later

### Out of scope

- Donation-side extraction script modifications (will live in a future fork of `~/src/d3i-infra/data-donation-task`)
- PII redaction (separate downstream tool with local LLM)
- Crime/news classification (researcher's own analysis on plain-text transcripts)
- VCE-based pipelines (different study, different architecture)
- Comments-by-default in v1 scrape mode (yt-dlp comment extraction is slow and flaky); comments are first-class once API access is wired in
- Cookie/login-based fetching, IP rotation, proxies (deferrable; design accepts the flags later without restructuring)
- Graceful shutdown (SIGTERM handling, in-flight drain) — recovery handled by stale-claim sweep
- HTTP status server / dashboard
- Property/fuzz testing

## High-level architecture

A single Rust binary, single crate. Pipelined async orchestrator: N download workers feed a bounded mpsc channel; one transcribe worker consumes from the channel and holds the GPU. SQLite (WAL mode, file-backed) is the operational source of truth. External CLI tools (yt-dlp, ffmpeg, whisper.cpp) do all heavy lifting; the orchestrator supervises and tracks state.

```
inbox/                         state.sqlite              transcripts/
{respondent_id}.json   ──┐
{respondent_id}.json   ──┤
                         ▼
                    [ingest cmd]
                         │
                  parse + canonicalize
                         │
                  upsert video rows  ──────►  videos
                                              (new = pending)
                    [process cmd]
                         │
            reset_stale_claims(threshold)
                         │
                ┌────────┴────────┐
                ▼                 ▼
        download worker    download worker  ◄── claim_next() loop
                │                 │
                │  Acquisition    │
                ▼                 ▼
              [bounded mpsc channel, capacity 4]
                         │
                         ▼
                  transcribe worker (1, holds GPU)
                         │
              ┌──────────┼──────────┐
              ▼          ▼          ▼
         write .txt + .json + .metadata.json (atomic)
                         │
                         ▼
                  Store::mark_succeeded
                  (UPDATE videos + INSERT video_events
                   in one SQLite transaction)
```

### Profiles

A single binary with two profile defaults:

- `--profile dev` — `tiny.en` model, CPU only, 1 download worker, channel capacity 2, stale-claim threshold 30s. For laptop iteration on a handful of test videos.
- `--profile prod` — `large-v3` model, CUDA, 3 download workers, channel capacity 4, stale-claim threshold 1h. For SURF Research Cloud.

### Multi-GPU on a 2× A10 box

Two independent binary instances. Each instance gets `CUDA_VISIBLE_DEVICES=0` (or `=1`) and a unique `--worker-id`. Both share the same `state.sqlite`. SQLite WAL mode + `BEGIN IMMEDIATE` on the claim transaction serializes work-acquisition atomically across the two instances. Aggregate throughput ≈ 2× single-instance; orchestration code unchanged.

```bash
CUDA_VISIBLE_DEVICES=0 uu-tiktok process --profile prod --worker-id gpu0 &
CUDA_VISIBLE_DEVICES=1 uu-tiktok process --profile prod --worker-id gpu1 &
```

### API-swap boundary

A `VideoFetcher` trait defines acquisition. The yt-dlp implementation always returns `Audio`. A future TikTok Research API implementation can return `ReadyTranscript` directly when the `voice_to_text` field is populated, falling back to `Audio` otherwise, or `Unavailable` in either case. The downstream pipeline branches on the `Acquisition` enum and is otherwise unchanged.

The API also surfaces rich metadata (`POST /v2/research/video/query/`, fields including `voice_to_text`, `video_description`, `view_count`, `like_count`, `comment_count`, `share_count`, `hashtag_names`, `effect_ids`, `playlist_id`, `video_duration`, `region_code`, `music_id`, `favorites_count`) and comments (`POST /v2/research/video/comment/list/`) cleanly. The `metadata.json` artifact uses a normalized union schema across both fetchers; comments are written to a dedicated artifact when the `--fetch-comments` flag is on (default off in scrape mode, default on once API is in use).

API surface details verified against `docs/reference/tiktok-for-developers/markdown/doc_research-api-codebook.md` and `doc_research-api-specs-query-videos.md` (corpus snapshot 2026-04-16). Re-verify before implementing the API fetcher — TikTok ships breaking changes.

## Components and module boundaries

Single Rust crate. Modules organized so the boundaries that matter are explicit.

```
uu-tiktok/
├── Cargo.toml
├── src/
│   ├── main.rs               # CLI entry, profile resolution, top-level orchestration
│   ├── cli.rs                # clap definitions
│   ├── config.rs             # Resolved Config struct (profile + paths + tunables)
│   ├── ingest.rs             # Walk DDP folder → parse watch history → upsert into state
│   ├── canonical.rs          # URL → CanonicalVideoId | NeedsResolution
│   ├── state/
│   │   ├── mod.rs            # Public Store API
│   │   ├── schema.rs         # Schema + migrations
│   │   └── claims.rs         # Atomic claim transactions (BEGIN IMMEDIATE)
│   ├── fetcher/
│   │   ├── mod.rs            # VideoFetcher trait, Acquisition enum, FetchError
│   │   └── ytdlp.rs          # YtDlpFetcher impl
│   ├── transcribe.rs         # whisper.cpp invocation, language detection, output writing
│   ├── pipeline.rs           # The pipelined orchestrator (download workers, channel, transcribe worker)
│   ├── output/
│   │   ├── artifacts.rs      # Per-video atomic writes
│   │   └── manifest.rs       # One-shot parquet export
│   ├── process.rs            # Shared subprocess runner (spawn, timeout, stderr ring buffer)
│   └── errors.rs             # RetryableKind, UnavailableReason, ClassifiedFailure, FailureContext
└── tests/
    ├── canonical.rs
    ├── state_claims.rs
    ├── pipeline_fakes.rs
    └── ingest.rs
```

### Five boundaries called out explicitly

1. **`fetcher::VideoFetcher` trait** — the boundary that survives the API swap.

   ```rust
   #[async_trait]
   trait VideoFetcher: Send + Sync {
       async fn acquire(
           &self,
           video_id: &VideoId,
           source_url: &Url,
       ) -> Result<Acquisition, FetchError>;
   }

   enum Acquisition {
       AudioFile(PathBuf),
       ReadyTranscript(TranscriptPayload),
       Unavailable(UnavailableReason),
   }

   struct TranscriptPayload {
       text: String,
       language: Option<String>,
       source_attribution: &'static str,  // "api_voice_to_text" etc.
   }
   ```

   Only `YtDlpFetcher` exists at v1. `ApiFetcher` lands when API access is granted; no stub in v1 (would only rot).

2. **`state::Store`** — abstracts SQLite. Operational, not SQL-shaped:

   ```rust
   impl Store {
       fn open(path: &Path) -> Result<Self>;
       fn claim_next(&self, worker_id: &str) -> Result<Option<Claim>>;
       fn mark_succeeded(&self, video_id: &VideoId, artifacts: SuccessArtifacts) -> Result<()>;
       fn mark_retryable_failure(&self, video_id: &VideoId, kind: RetryableKind, ctx: FailureContext) -> Result<()>;
       fn mark_terminal_failure(&self, video_id: &VideoId, reason: UnavailableReason, ctx: Option<FailureContext>) -> Result<()>;
       fn requeue_retryables(&self, filter: RequeueFilter) -> Result<usize>;
       fn reset_stale_claims(&self, older_than: Duration) -> Result<usize>;
       fn upsert_video(&self, ...) -> Result<()>;
       fn upsert_watch_history(&self, ...) -> Result<()>;
   }
   ```

   Every state-mutating method commits the videos UPDATE + the video_events INSERT in a single SQLite transaction. There is no scenario where the row updates but the event log is missing.

3. **`canonical::canonicalize_url`** — pure function, no I/O. Returns `CanonicalVideoId(String)` for forms 1 and 2 (regex extracts the 19-digit ID), `NeedsResolution(Url)` for short links (forms 3 and 4). The dedup primitive; gets the heaviest test coverage.

4. **`output` module** — every artifact write is atomic (write to `.tmp`, fsync, rename, fsync parent dir). Crash-safety guarantee: a transcript file on disk is always complete; a partially-written file would be the `.tmp` and the SQLite row would still be `in_progress`, so the next run re-does that video.

5. **`config::Config`** — single resolved struct passed everywhere. CLI + env + profile defaults are merged exactly once, in `main`. No module reads env vars or argv directly.

### External CLI tool invocation pattern

Every tool call goes through `process::run`:

```rust
pub struct CommandSpec<'a> {
    pub program: &'static str,
    pub args: Vec<String>,
    pub timeout: Duration,
    pub stderr_capture_bytes: usize,   // ring buffer; truncate, don't OOM
    pub redact_args: &'a [&'a str],    // for logging; cookies, tokens, etc.
}

pub struct CommandOutcome {
    pub exit_code: i32,
    pub stdout: Vec<u8>,
    pub stderr_excerpt: String,
    pub elapsed: Duration,
}

pub async fn run(spec: CommandSpec<'_>) -> Result<CommandOutcome, FetchError>;
```

Handles: spawn, timeout-then-SIGTERM-then-SIGKILL, stderr ring buffer, structured `tracing` logging with redacted args. Tool-specific argument construction and output parsing stay inside `fetcher/ytdlp.rs` and `transcribe.rs`.

### One simplification

`yt-dlp -x --audio-format wav --postprocessor-args "ffmpeg:-ar 16000 -ac 1"` produces 16 kHz mono WAV in one invocation (yt-dlp shells out to ffmpeg internally). One subprocess per video for the fetch stage, not two.

## Data flow and state machine

The system is **operator-driven, not scheduler-driven**. Retries happen only when the operator runs `requeue-retryables` and then `process` again. There is no internal retry timer.

### Per-video state machine

```
            ingest (writes pending)
                │
                ▼
          ┌─[pending]◄────────────────────────┐
          │   │                               │
          │   claim_next() (pending only)     │
          │   │                               │
          │   ▼                               │
          │ [in_progress]                     │
          │   │                               │
          │ acquire():                        │
          │ ┌─┼────────┬────────┬──────────┐  │
          │ ▼ ▼        ▼        ▼          ▼  │
          │AudioFile Ready    Unavail    Err  │
          │ │   │      │        │         │   │
          │ ▼   │      │        │     classify│
          │trans│      │        │         │   │
          │ │   │      │        │   ┌─────┼──────┐
          │ ▼   ▼      ▼        ▼   ▼     ▼      ▼
          │[succeeded][succeeded][terminal][retryable] [Bug]
          │                                 │       coordinated
          │                                 │       shutdown
          │ reset_stale_claims              │
          │ (in_progress→pending if stale)  │
          │                                 │
          │ requeue_retryables (operator)   │
          │ (failed_retryable→pending)      │
          └─────────────────────────────────┘
```

### Eligibility and ordering

`claim_next` selects only pending rows:

```sql
SELECT video_id FROM videos
WHERE status = 'pending'
ORDER BY rowid ASC
LIMIT 1;
```

Inside a `BEGIN IMMEDIATE` transaction so concurrent workers cannot grab the same row. Status flipped to `in_progress`, `claimed_by` and `claimed_at` set, `attempt_count` incremented, `claimed` event recorded — all in the same transaction.

### Where each transition happens

| Trigger | Module | Effect |
|---|---|---|
| New video seen in DDP | `ingest` | `INSERT OR IGNORE` → `pending` |
| Worker claims | `pipeline` (via `claim_next`) | `pending` → `in_progress`; `attempt_count++` |
| `Acquisition::AudioFile` → transcribe OK | `pipeline` (transcribe worker) | `in_progress` → `succeeded` |
| `Acquisition::ReadyTranscript` | `pipeline` (download worker, short-circuit) | `in_progress` → `succeeded` |
| `Acquisition::Unavailable(reason)` | `pipeline` (download worker, short-circuit) | `in_progress` → `failed_terminal` |
| `Err(FetchError)` → `Retryable` | `pipeline` | `in_progress` → `failed_retryable` |
| `Err(FetchError)` → `Bug` | `pipeline` | panic; coordinated shutdown |
| Stale `in_progress` row | `pipeline` (startup) | `in_progress` → `pending` (no `attempt_count` change) |
| Operator: `requeue-retryables` | `cli` | `failed_retryable` → `pending` |

### Short-circuit asymmetry

Only the `AudioFile` path traverses the channel and the GPU. `ReadyTranscript` and `Unavailable` outcomes complete inside the download worker (write artifacts + `mark_succeeded`, or `mark_terminal_failure`) without touching the GPU. Keeps the transcribe worker focused on its one job.

### Bug supervision

Any `ClassifiedFailure::Bug` from any worker triggers coordinated shutdown of the whole `process` invocation. Workers run inside a `tokio::task::JoinSet`; on the first task that returns `Err(Bug)` or panics, the main loop drops the JoinSet (cancelling remaining tasks at their next await point) and exits with status 1. Partial work that finished durably stays durable; in-flight work is left as `in_progress` for the next batch's stale-claim sweep.

### Lifecycle

```bash
# One-time
uu-tiktok init                                          # create state.sqlite

# Each batch (operator workflow)
uu-tiktok ingest --inbox ./inbox                        # idempotent
uu-tiktok process --profile prod --worker-id gpu0
uu-tiktok status

# Between batches: when a new wave of donations arrives
uu-tiktok ingest --inbox ./inbox                        # picks up new files
uu-tiktok requeue-retryables --older-than 12h           # optional
uu-tiktok process --profile prod --worker-id gpu0

# After all expected batches processed and a week has passed
uu-tiktok requeue-retryables                            # final wave
uu-tiktok process --profile prod --worker-id gpu0

# Final export
uu-tiktok export-manifest --out ./manifest.parquet
```

## Schemas

### SQLite (`state.sqlite`)

```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;

CREATE TABLE videos (
    video_id            TEXT PRIMARY KEY,        -- 19-digit numeric ID as TEXT (preserve precision)
    source_url          TEXT NOT NULL,            -- representative URL fed to yt-dlp; back-filled for short-link fetches
    canonical           INTEGER NOT NULL,         -- 1 if URL parsed inline, 0 if resolved via fetcher

    status              TEXT NOT NULL CHECK (status IN
                          ('pending','in_progress','succeeded','failed_terminal','failed_retryable')),
    claimed_by          TEXT,
    claimed_at          INTEGER,                  -- unix seconds
    attempt_count       INTEGER NOT NULL DEFAULT 0,
    last_error_kind     TEXT,
    last_error_message  TEXT,
    succeeded_at        INTEGER,

    duration_s          REAL,
    language_detected   TEXT,
    fetcher             TEXT,                     -- 'ytdlp' | 'api'
    transcript_source   TEXT,                     -- 'whisper.cpp' | 'api_voice_to_text'

    last_batch_label    TEXT,

    first_seen_at       INTEGER NOT NULL,
    updated_at          INTEGER NOT NULL
);

CREATE INDEX idx_videos_pending ON videos (rowid) WHERE status = 'pending';

CREATE TABLE watch_history (
    respondent_id  TEXT NOT NULL,
    video_id       TEXT NOT NULL,
    watched_at     INTEGER NOT NULL,
    in_window      INTEGER NOT NULL,
    PRIMARY KEY (respondent_id, video_id, watched_at),
    FOREIGN KEY (video_id) REFERENCES videos(video_id)
);
CREATE INDEX idx_watch_history_video ON watch_history (video_id);

CREATE TABLE video_events (
    id           INTEGER PRIMARY KEY,
    video_id     TEXT NOT NULL,
    at           INTEGER NOT NULL,
    event_type   TEXT NOT NULL,    -- claimed | succeeded | failed_retryable | failed_terminal | released_stale | requeued
    worker_id    TEXT,
    batch_label  TEXT,
    detail_json  TEXT,
    FOREIGN KEY (video_id) REFERENCES videos(video_id)
);
CREATE INDEX idx_video_events_video ON video_events (video_id, at);

CREATE TABLE meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
INSERT INTO meta (key, value) VALUES ('schema_version', '1');
```

`attempt_count` semantics: incremented inside the `claim_next` transaction (i.e., once per transition into `in_progress`). Counts attempts, not failures. Stale-claim recovery does **not** bump `attempt_count` (the count was already incremented when the row was originally claimed).

### Per-video transcript: `transcripts/{video_id}.txt`

Plain UTF-8, segments concatenated with newlines as whisper.cpp emits with `-of txt`. Trailing newline.

### Per-video transcript metadata: `transcripts/{video_id}.json`

```json
{
  "video_id": "7234567890123456789",
  "source_url": "https://www.tiktokv.com/share/video/7234567890123456789/",
  "duration_s": 23.4,
  "language_detected": "en",
  "transcribed_at": "2026-04-16T13:45:22Z",
  "fetcher": "ytdlp",
  "transcript_source": "whisper.cpp",
  "model": "large-v3"
}
```

### Per-video video metadata: `transcripts/{video_id}.metadata.json`

Union schema across yt-dlp's `--write-info-json` and the API's `/research/video/query/` response, normalized to lowercase snake_case. Fields a source doesn't provide are `null`.

```json
{
  "video_id": "7234567890123456789",
  "source": "ytdlp",
  "fetched_at": "2026-04-16T13:45:22Z",

  "uploader": "username",
  "uploader_id": "...",
  "title": "...",
  "description": "...",
  "duration_s": 23.4,
  "create_time": "2026-04-15T10:30:00Z",
  "region_code": "US",

  "view_count": 12345,
  "like_count": 678,
  "comment_count": 45,
  "share_count": 12,

  "hashtags": ["crime", "news"],
  "music_id": "7234...",

  "ytdlp_extractor": "tiktok",
  "ytdlp_format_id": null,

  "api_voice_to_text": null,
  "effect_ids": null,
  "playlist_id": null,

  "raw_source_payload_path": "transcripts/7234567890123456789.metadata.raw.json"
}
```

The full original payload (yt-dlp info JSON or API response) goes into the sibling `.metadata.raw.json` rather than embedded. Behind a `--keep-raw-metadata` flag, default on.

### Per-video comments: `transcripts/{video_id}.comments.json` (only when `--fetch-comments`)

```json
{
  "video_id": "7234567890123456789",
  "source": "api",
  "fetched_at": "2026-04-16T13:45:22Z",
  "fetched_count": 45,
  "reported_total": 45,
  "is_complete": true,
  "comments": [
    {
      "comment_id": "...",
      "text": "...",
      "create_time": "2026-04-15T11:02:00Z",
      "like_count": 3,
      "reply_count": 0,
      "parent_comment_id": null
    }
  ]
}
```

### Manifest Parquet (`manifest.parquet`, derived export, one-shot rewrite)

| Column            | Type            | Source                              | Nullable |
|-------------------|-----------------|-------------------------------------|----------|
| respondent_id     | string          | watch_history.respondent_id         | no       |
| video_id          | string          | watch_history.video_id              | no       |
| watched_at        | timestamp[ms]   | watch_history.watched_at            | no       |
| in_window         | bool            | watch_history.in_window             | no       |
| status            | string          | videos.status                       | no       |
| duration_s        | double          | videos.duration_s                   | yes      |
| language_detected | string          | videos.language_detected            | yes      |
| fetcher           | string          | videos.fetcher                      | yes      |
| transcript_source | string          | videos.transcript_source            | yes      |
| has_transcript    | bool            | derived: status='succeeded'         | no       |
| has_metadata      | bool            | derived: file exists                | no       |
| has_comments      | bool            | derived: file exists                | no       |
| last_error_kind   | string          | videos.last_error_kind              | yes      |
| attempt_count     | int32           | videos.attempt_count                | no       |

Generated by reading SQLite + checking artifact file presence. Read-only with respect to live state; safe to run while `process` is running (WAL).

### `Config` struct

```rust
struct Config {
    profile: Profile,                            // Dev | Prod
    paths: Paths,                                // inbox, transcripts, state_db, raw_metadata

    download_workers: usize,                     // dev=1, prod=3
    channel_capacity: usize,                     // dev=2, prod=4

    whisper_model_path: PathBuf,                 // dev=tiny.en, prod=large-v3
    whisper_threads: usize,                      // dev=cpu_count, prod=1
    whisper_use_gpu: bool,                       // dev=false, prod=true

    ytdlp_timeout: Duration,                     // 5min default
    transcribe_timeout: Duration,                // 10min default

    stale_claim_threshold: Duration,             // dev=30s, prod=1h

    keep_raw_metadata: bool,                     // default true
    fetch_comments: bool,                        // default false in scrape mode

    worker_id: String,
    batch_label: Option<String>,                 // optional provenance label

    analysis_window: Option<Duration>,           // e.g. 90 days; None = no filter
}
```

Resolved from: profile defaults → env vars (`UU_TIKTOK_*`) → CLI flags. Single struct passed everywhere.

`video_id` is stored and passed as `String` (TEXT in SQLite) throughout. TikTok IDs are 19-digit Snowflake IDs that fit in i64. yt-dlp serializes them as strings in its info JSON, but the Research API documentation specifies the field as `int64` (and example responses show it unquoted). 19-digit values exceed the JavaScript safe-integer range (2^53 ≈ 16 digits) and many JSON parsers silently lose precision on them. We parse the API's int64 into a Rust i64 and immediately format-as-string for storage; we never round-trip through a JS-style parser. There is also a docs inconsistency between the spec field name (`id`) and the example response field (`video_id`); resolve at integration time against the live API.

## CLI surface

```
uu-tiktok [GLOBAL FLAGS] <SUBCOMMAND> [SUBCOMMAND FLAGS]
```

### Global flags

```
--profile <dev|prod>            (default: dev)
--state-db <PATH>               (default: ./state.sqlite)
--inbox <PATH>                  (default: ./inbox)
--transcripts <PATH>            (default: ./transcripts)
--log-format <human|json>       (default: human)
--log-level <debug|info|warn|error>   (default: info)
```

Configuration precedence: CLI flag → env var (`UU_TIKTOK_*`) → profile default → built-in. Resolved once in `main`. No config file in v1.

### `init`

Create `state.sqlite`, apply schema. Idempotent. Refuses to overwrite if `schema_version` exists.

### `migrate`

Apply pending schema migrations.

### `ingest`

Walk `--inbox`, parse DDP JSONs, canonicalize URLs, upsert into `videos` and `watch_history`. Idempotent.

```
--analysis-window <DURATION>    e.g. 90d; sets in_window flag (default: none = all true)
--batch-label <NAME>            Optional provenance label
--dry-run                       Report counts without writing
```

### `process`

Run a batch. Claims `pending` rows only; no scheduler eligibility logic. Exits cleanly when no pending rows remain.

```
--worker-id <NAME>              Required for multi-instance (default: hostname)
--max-videos <N>                Stop after N (succeeded + failed)
--time-budget <DURATION>        Stop accepting new claims after this elapsed
--no-stale-sweep                Skip startup reset_stale_claims (debugging only)
--batch-label <NAME>            Optional provenance label
```

Emits a progress line every 30s; per-video INFO log lines for stage transitions.

### `status`

```
(no args)                       Counts by status
--video-id <ID>                 Full event history for one video
--respondent-id <ID>            Per-respondent summary
--errors                        List failed_terminal videos with last_error_kind/message
--retryable                     List failed_retryable videos
--json                          Output as JSON
```

### `requeue-retryables`

Operator command; flips selected `failed_retryable` rows back to `pending`.

```
--older-than <DURATION>         Only requeue if videos.updated_at older than this
--error-kinds <KIND,KIND,...>   Only requeue rows whose last_error_kind matches
--max <N>                       Cap the number requeued
--dry-run
```

### `reset-stale-claims`

Operator escape hatch. Resets `in_progress` rows back to `pending`.

```
--max-age <DURATION>            REQUIRED — no default; force a deliberate choice
--dry-run
```

### `export-manifest`

Read-only derivation of Parquet manifest from SQLite + transcripts directory.

```
--out <PATH>                    REQUIRED
--include-failed                Include failed_terminal rows (default: true)
--respondent-filter <PATH>      Optional CSV of respondent_ids to include
```

### Exit codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Pipeline error (Bug, panic, unrecoverable) |
| 2 | Usage error (missing file, bad flag, schema version mismatch) |
| 3 | Nothing to do (`process` found no pending rows at startup) |

## Error handling and failure classification

### Type structure

```rust
enum RetryableKind {
    NoMediaProduced,
    RateLimited,
    TransientNetwork,
    BadAudio,
    EmptyTranscript,
    OOM,
    ToolTimeout,
    ToolCrashedUnknown,
    YtDlpUnknown,
    WhisperUnknown,
    TransientStorage,
}

enum UnavailableReason {
    Deleted,
    Private,
    LoginRequired,
    RegionBlocked,
    AgeRestricted,
    NoMediaInResponse,
    Other(String),
}

struct FailureContext {
    tool: Option<&'static str>,
    exit_code: Option<i32>,
    stderr_excerpt: Option<String>,
    timeout: Option<Duration>,
    classification_reason: &'static str,
}

enum ClassifiedFailure {
    Retryable { kind: RetryableKind, ctx: FailureContext },
    Bug { ctx: FailureContext },
}

fn classify_fetch_error(err: &FetchError) -> ClassifiedFailure;
fn classify_transcribe_error(err: &TranscribeError) -> ClassifiedFailure;
```

`UnavailableReason` describes content/access verdicts (terminal business outcomes). `RetryableKind` describes recoverable execution failures. The two enums never cross. Terminal failures arrive only via `Acquisition::Unavailable(reason)`; classifier output is only `Retryable` or `Bug`.

### Dispatch

```rust
match acquire(video_id, source_url).await {
    Ok(Acquisition::AudioFile(path))           => /* hand off to transcribe stage */,
    Ok(Acquisition::ReadyTranscript(payload))  => commit_transcript_and_succeed(payload),
    Ok(Acquisition::Unavailable(reason))       => store.mark_terminal_failure(video_id, reason, None),
    Err(fetch_err) => match classify_fetch_error(&fetch_err) {
        ClassifiedFailure::Retryable { kind, ctx } => store.mark_retryable_failure(video_id, kind, ctx),
        ClassifiedFailure::Bug { ctx }              => abort_with_bug(ctx),
    },
}
```

### Classification rules

Default-cautious posture: unrecognized stderr or unknown exit codes → `Retryable`, never `Bug`. `Bug` is reserved for our defects.

**yt-dlp:**

| Pattern | Result |
|---------|--------|
| Exit 0, no media file at expected path (tmp dir intact, writable) | `Retryable(NoMediaProduced)` |
| Exit 0, no media file *and* tmp dir invariants broken | `Bug` |
| Exit 1 + stderr `Video unavailable` / `Video has been removed` / `not available` | `Unavailable(Deleted)` (returned as `Acquisition::Unavailable`, not via classifier) |
| Exit 1 + stderr `Private video` / `private` | `Unavailable(Private)` |
| Exit 1 + stderr `Login required` / `requires login` | `Unavailable(LoginRequired)` |
| Exit 1 + stderr `not available in your country` / `geo` | `Unavailable(RegionBlocked)` |
| Exit 1 + stderr `age restricted` / `age-restricted` | `Unavailable(AgeRestricted)` |
| Exit 1 + stderr `HTTP Error 429` / `rate limit` | `Retryable(RateLimited)` |
| Exit 1 + stderr `HTTP Error 5\d\d` / `temporarily unavailable` / `connection reset` / `timed out` | `Retryable(TransientNetwork)` |
| Exit 1, any other stderr | `Retryable(YtDlpUnknown)` |
| `ToolTimeout` (our wrapper killed it) | `Retryable(ToolTimeout)` |
| Unknown exit code (e.g., 137) | `Retryable(ToolCrashedUnknown)` |

**ffmpeg (invoked via yt-dlp postprocessor):**

| Pattern | Result |
|---------|--------|
| stderr `Invalid data found` / `moov atom not found` / `truncated` | `Retryable(BadAudio)` |
| stderr `No such file or directory` for a path *we just wrote* | `Bug` |
| stderr `No such file or directory` for plausibly tmp-cleanup race | `Retryable(BadAudio)` |

**whisper.cpp:**

| Pattern | Result |
|---------|--------|
| Exit 0, empty transcript output | `Retryable(EmptyTranscript)` |
| Exit non-zero, stderr `out of memory` / `cudaMalloc` | `Retryable(OOM)` |
| Exit non-zero, stderr `failed to load model` | `Bug` (config issue) |
| `ToolTimeout` | `Retryable(ToolTimeout)` |
| Other non-zero | `Retryable(WhisperUnknown)` |

Operator note: persistent `OOM` for the same video across waves typically indicates a deterministic memory issue (unusually long audio, model size mismatch). Operator's recourse: inspect via `status --video-id`, decide to skip manually (SQL UPDATE to `failed_terminal`), use a smaller model for that video, or accept loss.

### Bug class — what triggers coordinated shutdown

- Subprocess returns an exit code we have no rule for **and** it doesn't match any retryable pattern (rare; default is `Retryable(ToolCrashedUnknown)`)
- `failed to load model` from whisper.cpp (configuration broken)
- Path-bookkeeping invariant broken (we lost a temp file we just wrote)
- SQLite constraint violation on tables we own (different from busy timeout)
- Internal invariant violation (e.g., `claim_next` returning a `succeeded` row)
- Panic in our code

Behavior: log at ERROR with full context, set abort flag, worker exits with `Err(Bug)`, main loop drops the JoinSet, process exits 1.

### SQLite contention

- `PRAGMA busy_timeout = 5000` applied at connection open.
- If exhausted during a Store operation: returns `StoreError::Busy` → worker treats as `Retryable(TransientStorage)` for that video. Video stays in `in_progress` (its current state) and gets swept on next batch by `reset_stale_claims`. Worker continues with next claim.
- Repeated `Busy` across many videos in one batch suggests a real concurrency problem and warrants operator attention but is not Bug-class.

### Atomic write contract

Per-video commit sequence — invariant: **never mark `succeeded` before durable artifact visibility.**

1. Write `{video_id}.txt.tmp`, `{video_id}.json.tmp`, `{video_id}.metadata.json.tmp` (and `.metadata.raw.json.tmp`, `.comments.json.tmp` when applicable)
2. `fsync` each temp file
3. `rename` each to its final name
4. `fsync` the parent directory
5. `Store::mark_succeeded` (videos UPDATE + video_events INSERT in one transaction)

Failure modes:
- Artifact files exist but row is `in_progress` → next run treats as stale claim, re-fetches, idempotent overwrite.
- Row says `succeeded` but file missing → cannot happen by construction.

### Stale-claim recovery

- Operates only on `in_progress` rows whose `claimed_at < now - stale_claim_threshold`.
- Resets to `pending`. Records `released_stale` event.
- **Does not change `attempt_count`** (the count was already bumped when the row was originally claimed; the attempt happened, it just didn't complete).
- Never touches `failed_retryable` rows.

### Deferred to v2

- Cookie / login-based fetching (`--ytdlp-cookies PATH`)
- IP rotation / proxies (`--ytdlp-proxy URL`)
- Tool version pinning / startup version check
- Graceful shutdown (SIGTERM handling, in-flight drain)
- `ApiFetcher` implementation (lands when API access is granted)

## Testing strategy

Three tiers, with explicit boundaries.

### Tier 1 — Pure unit tests (run on every `cargo test`, fast, no I/O)

| What | Where | Notes |
|------|-------|-------|
| URL canonicalization | `src/canonical.rs` | Table-driven; one row per URL form, malformed inputs, edge cases |
| Error classification | `src/errors.rs` | Table-driven; one row per pattern from the classification tables |
| Config resolution precedence | `src/config.rs` | A few cases per source |
| DDP JSON parsing | `src/ingest.rs` | Sample fixture JSONs |
| Classification boundary consistency | `src/errors.rs` | Verify: classifier output is only `Retryable` or `Bug`, never `Terminal`; unknown tool exit never becomes `Bug` |

### Tier 2 — Integration tests (run on every `cargo test`, ~1s each)

Use a **real on-disk SQLite** in `tempfile::TempDir` (with WAL mode) for any test exercising claim semantics, multi-connection behavior, or stale-recovery. `:memory:` only for fast single-connection store tests.

| What | Notes |
|------|-------|
| Schema migration: fresh DB | Verify clean schema after `init` |
| Schema migration: old-fixture-→-latest | Shaped for future migrations even though v1 has only initial schema |
| Atomic claim semantics | Two workers, two `Store` handles, two connections, one DB file; concurrent `claim_next` must never return the same row |
| State transitions write event log | After `mark_succeeded`, verify `video_events` row exists with right `event_type` and `worker_id` |
| Transactional contract | Both `videos.status` update and `video_events` insert commit together; if one fails, neither commits |
| Pipeline orchestration | `FakeFetcher` returns scripted `Acquisition` outcomes; verify final state of all videos |
| Retryable → operator requeue → claim works | One pending + one failed_retryable; first run claims only pending; after `requeue_retryables`, second run claims the formerly retryable |
| Stale claim sweep | Manually insert `in_progress` row with old `claimed_at`; sweep; verify `pending` and `attempt_count` **unchanged** |
| Bug shutdown — orchestration level | `FakeFetcher` scripted Bug; verify abort flag set, no further claims/commits, JoinSet unwinds |
| Bug shutdown — exit code | CLI smoke test verifying `process` exits with code 1 when a bug is triggered |
| Artifact-write contract | Single video; verify `.txt`, `.json`, `.metadata.json` all exist at final paths, no `.tmp` left, then `mark_succeeded` row matches |
| `process` claims only pending | Confirms manual-batch eligibility rule explicitly |

`FakeFetcher` shape:

```rust
struct FakeFetcher {
    script: HashMap<VideoId, VecDeque<FakeOutcome>>,
    delay: Duration,
}

enum FakeOutcome {
    AudioFile(PathBuf),
    ReadyTranscript(TranscriptPayload),
    Unavailable(UnavailableReason),
    Error(FetchError),
}
```

Each test owns its temp dir of pre-staged WAV fixtures (when needed for AudioFile outcomes).

### Tier 3 — Smoke tests against real tools (`#[ignore]`, run via `cargo test -- --ignored`, slow)

| What | Notes |
|------|-------|
| Real yt-dlp downloads from a known-stable TikTok URL | Pinned curated set of 2–3 long-lived public videos. Loose assertions: download succeeded, file exists, output contains expected video ID. No assertions on titles, durations, view counts (drift). |
| Real ffmpeg postprocessor produces 16 kHz mono WAV | Verifies the full invocation |
| Real whisper.cpp transcribes test WAV with `tiny.en` | One short fixture audio; expected text known |
| End-to-end `--profile dev` on 1 known URL with real tools | Integration test of last resort |

Anything that requires GPU or `large-v3` is tested manually on SURF before deployment, not in CI.

### Stress test (separate, `#[ignore]`, manual or pre-deploy)

1000 fake videos through the pipeline with `FakeFetcher`. Verifies orchestration scales to realistic batch sizes without resource leaks or starvation. Not run on every `cargo test`.

### Test fixtures

```
tests/fixtures/
├── ddp/                          # Sample DDP JSONs: minimal, realistic, edge cases (short links, malformed)
├── audio/                        # Short public-domain WAV for whisper.cpp smoke tests
├── yt_dlp_responses/             # Captured stderr from real failures (deleted, private, geo, 429, etc.)
└── api_responses/                # Placeholder; populated when API access lands
```

### Explicitly NOT tested

- Behavior under real TikTok rate-limiting at scale (cannot reproduce reliably)
- Multi-instance + multi-GPU coordination on real hardware (manual SURF verification; in-memory atomic-claim test is the proxy)
- Long-running batch behavior over hours (1000-fake-video stress test approximates)
- yt-dlp output format stability (we pin a version; Tier 3 catches drift)

### CI matrix

| Job | Toolchain | Tier 1 | Tier 2 | Tier 3 | Stress |
|-----|-----------|--------|--------|--------|--------|
| Linux stable | rust stable | ✓ | ✓ | ✗ | ✗ |
| Manual (pre-deploy) | — | ✓ | ✓ | ✓ | ✓ |

Nightly Rust added later if a real reason emerges.

## Decisions log

| Decision | Why |
|----------|-----|
| Rust over Python | Operator preferred; orchestration-friendly (memory, single binary, deploy). Transcription speed is C++ either way (faster-whisper is C++-wrapped Python; whisper.cpp is direct C++). |
| Single binary, single crate | Workspace split would be premature for ~3 kloc; modules give sufficient structure. |
| SQLite as source of truth | File-per-video state is fragile; Postgres is overkill; SQLite WAL handles multi-writer + multi-reader for our scale. |
| Approach 2 (pipelined) over Approach 1 (serial) or Approach 3 (generic stages) | Saturates GPU + network in parallel; well-trodden tokio pattern; not over-engineered. |
| Same binary for dev + prod, profile-driven | Avoids dev/prod codebase drift. |
| Manual-batch model (no scheduler, no `next_retry_at`) | Operator runs waves; retries are operator-initiated. Modeling an internal scheduler would lie about the workflow. |
| Status-flip on requeue (`failed_retryable → pending`) | Matches operator mental model; cleaner observability than hidden-eligibility model. |
| `attempt_count` increments on claim, not on failure | Counts attempts directly; auditable. |
| Event log + status update in one SQLite transaction | Eliminates "log out of sync with state" failure mode by construction. |
| `Acquisition::Unavailable` separate from `FetchError` | Terminal content verdicts are not execution failures; types should not conflate them. |
| `ClassifiedFailure` two-variant (`Retryable`/`Bug`), no `Terminal` | Classifier only operates on execution errors; terminal verdicts arrive through `Acquisition::Unavailable`. |
| Default-cautious classification: unknown → `Retryable` | Cost of wrong-Retryable is one wasted attempt; cost of wrong-Terminal is permanent loss. |
| `manifest.parquet` derived, not transactional | Avoids parquet append/concurrent-write concerns in hot path. |
| No graceful shutdown in v1 | Stale-claim sweep recovers from any abrupt exit; production mode rarely interrupts mid-batch. |
| No `ApiFetcher` stub in v1 | Stub would rot; trait + enum are sufficient to land the implementation later without restructuring. |
| `video_id` as TEXT throughout | Avoids JSON precision-loss bugs at boundaries; yt-dlp and API both serialize as strings. |
| Multi-GPU = two binary instances, shared SQLite | Simpler than per-binary GPU pool; SQLite atomic claims serialize work-acquisition correctly. |
