# UU TikTok Pipeline — Plan A: Walking Skeleton

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stand up the minimal end-to-end pipeline that ingests one DDP-extracted JSON file, fetches one TikTok video's audio with yt-dlp, transcribes it with whisper.cpp (tiny.en, CPU), and writes the transcript artifact to a sharded directory. Dev profile only. Synchronous serial loop. Happy-path only.

**Architecture:** Single Rust binary, single crate. SQLite (file-backed, WAL) as state store. External CLI tools (yt-dlp, ffmpeg, whisper.cpp) do all heavy lifting via a shared subprocess runner. `VideoFetcher` trait introduced now so Plan B can swap implementations without restructuring. No async pipeline, no failure classification, no retry semantics, no short-link resolution — those land in Plans B and C.

**Tech Stack:** Rust 2021, tokio (used only for the subprocess runner; serial main loop), rusqlite (bundled), clap, serde, serde_json, chrono, tracing, async-trait, anyhow, thiserror, tempfile (dev), assert_cmd (dev).

**This is Plan A of three.** Plan B adds production hardening (async pipeline, error classification, retry, multi-instance). Plan C adds the operator surface (short-link resolution, status/requeue/export commands, comments). Each plan is meant to produce working, testable software on its own. Reassess design after Plan A's artifact exists.

**Reference:** Full design in `docs/superpowers/specs/2026-04-16-uu-tiktok-pipeline-design.md`. The plan implements a deliberate subset; the spec is the source of truth for "why."

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

## Tasks

### Task 1: Initialize crate with chosen dependencies

**Files:**
- Create: `Cargo.toml`
- Create: `src/main.rs`

- [ ] **Step 1: Initialize the cargo project**

Run:
```bash
cd /home/dmm/src/uu-tiktok
cargo init --bin --name uu-tiktok
```

Expected: creates `Cargo.toml` and `src/main.rs` with a hello-world default. Working directory must be the repo root (`/home/dmm/src/uu-tiktok`).

- [ ] **Step 2: Replace `Cargo.toml` with the Plan A dependency set**

Overwrite `Cargo.toml` with:

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

- [ ] **Step 3: Replace `src/main.rs` with a hello-world that initializes tracing**

```rust
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tracing::info!("uu-tiktok skeleton up");
    Ok(())
}
```

- [ ] **Step 4: Verify build and test scaffolding work**

Run:
```bash
cargo build 2>&1 | tail -3
cargo test 2>&1 | tail -3
```

Expected: build succeeds (may take a few minutes the first time as rusqlite compiles SQLite). `cargo test` reports `0 passed; 0 failed`.

- [ ] **Step 5: Commit**

```bash
cargo fmt
git add Cargo.toml Cargo.lock src/main.rs
git commit -m "Plan A T1: initialize Rust crate with Plan A dependencies"
```

---

### Task 2: CLI scaffolding (subcommand enum + global flags)

**Files:**
- Create: `src/cli.rs`
- Modify: `src/main.rs`
- Test: `tests/cli.rs`

- [ ] **Step 1: Write the failing CLI smoke test**

Create `tests/cli.rs`:

```rust
use assert_cmd::Command;
use predicates::str::contains;

#[test]
fn help_lists_plan_a_subcommands() {
    let mut cmd = Command::cargo_bin("uu-tiktok").unwrap();
    cmd.arg("--help")
        .assert()
        .success()
        .stdout(contains("init"))
        .stdout(contains("ingest"))
        .stdout(contains("process"));
}

#[test]
fn init_subcommand_help_works() {
    Command::cargo_bin("uu-tiktok")
        .unwrap()
        .args(["init", "--help"])
        .assert()
        .success();
}
```

- [ ] **Step 2: Run test to confirm it fails**

Run: `cargo test --test cli help_lists_plan_a_subcommands -- --nocapture 2>&1 | tail -15`
Expected: FAIL — no subcommands defined yet.

- [ ] **Step 3: Define the CLI in `src/cli.rs`**

```rust
use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser, Debug)]
#[command(name = "uu-tiktok", version, about = "TikTok donation pipeline (Plan A walking skeleton)")]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalArgs,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Parser, Debug, Clone)]
pub struct GlobalArgs {
    #[arg(long, value_enum, default_value_t = Profile::Dev, env = "UU_TIKTOK_PROFILE")]
    pub profile: Profile,

    #[arg(long, default_value = "./state.sqlite", env = "UU_TIKTOK_STATE_DB")]
    pub state_db: PathBuf,

    #[arg(long, default_value = "./inbox", env = "UU_TIKTOK_INBOX")]
    pub inbox: PathBuf,

    #[arg(long, default_value = "./transcripts", env = "UU_TIKTOK_TRANSCRIPTS")]
    pub transcripts: PathBuf,

    #[arg(long, value_enum, default_value_t = LogFormat::Human, env = "UU_TIKTOK_LOG_FORMAT")]
    pub log_format: LogFormat,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Create state.sqlite and apply schema. Idempotent.
    Init,
    /// Walk --inbox, parse DDP JSONs, upsert into videos and watch_history.
    Ingest {
        #[arg(long)]
        dry_run: bool,
    },
    /// Run a batch: claim pending videos, fetch + transcribe, write artifacts.
    Process {
        #[arg(long)]
        max_videos: Option<usize>,
    },
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    Dev,
}

#[derive(ValueEnum, Debug, Clone, Copy)]
pub enum LogFormat {
    Human,
    Json,
}
```

(Plan A only ships `Profile::Dev`. Plan B adds `Profile::Prod`.)

- [ ] **Step 4: Wire `cli.rs` into `main.rs`**

Replace `src/main.rs` with:

```rust
use anyhow::Result;
use clap::Parser;

mod cli;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = cli::Cli::parse();

    init_tracing(cli.global.log_format);

    match cli.command {
        cli::Command::Init => {
            tracing::info!("init: not yet implemented (Task 7+)");
        }
        cli::Command::Ingest { dry_run: _ } => {
            tracing::info!("ingest: not yet implemented (Task 13)");
        }
        cli::Command::Process { max_videos: _ } => {
            tracing::info!("process: not yet implemented (Task 14)");
        }
    }

    Ok(())
}

fn init_tracing(format: cli::LogFormat) {
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    match format {
        cli::LogFormat::Human => {
            tracing_subscriber::fmt().with_env_filter(filter).init();
        }
        cli::LogFormat::Json => {
            tracing_subscriber::fmt().json().with_env_filter(filter).init();
        }
    }
}
```

- [ ] **Step 5: Verify the test now passes**

Run: `cargo test --test cli 2>&1 | tail -10`
Expected: `2 passed; 0 failed`.

- [ ] **Step 6: Commit**

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings 2>&1 | tail -5
git add Cargo.lock src/main.rs src/cli.rs tests/cli.rs
git commit -m "Plan A T2: CLI scaffolding with init/ingest/process subcommands"
```

---

### Task 3: Config struct + profile defaults

**Files:**
- Create: `src/config.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Write tests covering profile resolution**

Append to `src/config.rs` (file does not exist yet — create it for the tests):

```rust
use std::path::PathBuf;
use std::time::Duration;

use crate::cli::{GlobalArgs, Profile};

#[derive(Debug, Clone)]
pub struct Config {
    pub profile: Profile,
    pub state_db: PathBuf,
    pub inbox: PathBuf,
    pub transcripts: PathBuf,

    /// Path to the whisper.cpp model file. Plan A defaults to a tiny.en model
    /// that the operator places at this path before running `process`.
    pub whisper_model_path: PathBuf,
    pub whisper_use_gpu: bool,
    pub whisper_threads: usize,

    pub ytdlp_timeout: Duration,
    pub transcribe_timeout: Duration,
}

impl Config {
    pub fn from_args(args: &GlobalArgs) -> Self {
        match args.profile {
            Profile::Dev => Self {
                profile: Profile::Dev,
                state_db: args.state_db.clone(),
                inbox: args.inbox.clone(),
                transcripts: args.transcripts.clone(),
                whisper_model_path: PathBuf::from("./models/ggml-tiny.en.bin"),
                whisper_use_gpu: false,
                whisper_threads: num_cpus_safe(),
                ytdlp_timeout: Duration::from_secs(300),
                transcribe_timeout: Duration::from_secs(600),
            },
        }
    }
}

fn num_cpus_safe() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev_args() -> GlobalArgs {
        GlobalArgs {
            profile: Profile::Dev,
            state_db: PathBuf::from("/tmp/test.sqlite"),
            inbox: PathBuf::from("/tmp/in"),
            transcripts: PathBuf::from("/tmp/out"),
            log_format: crate::cli::LogFormat::Human,
        }
    }

    #[test]
    fn dev_profile_uses_tiny_en_cpu() {
        let cfg = Config::from_args(&dev_args());
        assert!(cfg.whisper_model_path.to_string_lossy().contains("tiny.en"));
        assert!(!cfg.whisper_use_gpu);
        assert!(cfg.whisper_threads >= 1);
        assert_eq!(cfg.ytdlp_timeout, Duration::from_secs(300));
    }

    #[test]
    fn paths_pass_through_from_args() {
        let cfg = Config::from_args(&dev_args());
        assert_eq!(cfg.inbox, PathBuf::from("/tmp/in"));
        assert_eq!(cfg.transcripts, PathBuf::from("/tmp/out"));
        assert_eq!(cfg.state_db, PathBuf::from("/tmp/test.sqlite"));
    }
}
```

- [ ] **Step 2: Wire `config` into `main.rs` and run tests**

Edit `src/main.rs` to add `mod config;` near the existing `mod cli;` line, then run:
```bash
cargo test config:: 2>&1 | tail -10
```
Expected: `2 passed; 0 failed`.

- [ ] **Step 3: Pass `Config` through to subcommand stubs**

Update `src/main.rs` body of `main`:

```rust
let cli = cli::Cli::parse();
init_tracing(cli.global.log_format);
let cfg = config::Config::from_args(&cli.global);
tracing::info!(profile = ?cfg.profile, state_db = ?cfg.state_db, "config resolved");

match cli.command {
    cli::Command::Init => {
        tracing::info!("init: not yet implemented (Task 7+)");
    }
    cli::Command::Ingest { dry_run: _ } => {
        tracing::info!("ingest: not yet implemented (Task 13)");
    }
    cli::Command::Process { max_videos: _ } => {
        tracing::info!("process: not yet implemented (Task 14)");
    }
}
```

- [ ] **Step 4: Verify nothing broke**

Run: `cargo test 2>&1 | tail -5`
Expected: all tests still pass.

- [ ] **Step 5: Commit**

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings 2>&1 | tail -5
git add Cargo.lock src/config.rs src/main.rs
git commit -m "Plan A T3: Config struct with dev profile defaults"
```

---

### Task 4: Errors module (minimal types)

**Files:**
- Create: `src/errors.rs`
- Modify: `src/main.rs`

Plan A only needs `FetchError` and `TranscribeError` to propagate; classification (RetryableKind / UnavailableReason / ClassifiedFailure) lands in Plan B. Define the bare minimum so trait signatures lock in.

- [ ] **Step 1: Write the failing test**

Create `src/errors.rs`:

```rust
use std::time::Duration;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum FetchError {
    #[error("subprocess `{tool}` timed out after {duration:?}")]
    ToolTimeout {
        tool: &'static str,
        duration: Duration,
    },

    #[error("subprocess `{tool}` exited with status {exit_code}: {stderr_excerpt}")]
    ToolFailed {
        tool: &'static str,
        exit_code: i32,
        stderr_excerpt: String,
    },

    #[error("network error during fetch: {0}")]
    NetworkError(String),

    #[error("failed to parse fetcher output: {0}")]
    ParseError(String),
}

#[derive(Debug, Error)]
pub enum TranscribeError {
    #[error("whisper.cpp timed out after {duration:?}")]
    Timeout { duration: Duration },

    #[error("whisper.cpp exited with status {exit_code}: {stderr_excerpt}")]
    Failed {
        exit_code: i32,
        stderr_excerpt: String,
    },

    #[error("whisper.cpp produced no transcript")]
    EmptyOutput,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fetch_error_displays_with_context() {
        let err = FetchError::ToolTimeout {
            tool: "yt-dlp",
            duration: Duration::from_secs(300),
        };
        let msg = format!("{}", err);
        assert!(msg.contains("yt-dlp"));
        assert!(msg.contains("300"));
    }

    #[test]
    fn transcribe_error_failed_carries_exit_code() {
        let err = TranscribeError::Failed {
            exit_code: 1,
            stderr_excerpt: "out of memory".into(),
        };
        let msg = format!("{}", err);
        assert!(msg.contains("status 1"));
        assert!(msg.contains("out of memory"));
    }
}
```

- [ ] **Step 2: Run tests to confirm pass**

Add `mod errors;` to `src/main.rs` (next to other `mod` declarations).

Run: `cargo test errors:: 2>&1 | tail -10`
Expected: `2 passed; 0 failed`.

- [ ] **Step 3: Commit**

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings 2>&1 | tail -5
git add src/errors.rs src/main.rs
git commit -m "Plan A T4: minimal FetchError and TranscribeError types"
```

---

### Task 5: URL canonicalization (forms 1 and 2)

**Files:**
- Create: `src/canonical.rs`
- Modify: `src/main.rs`
- Test: `tests/canonical.rs`

- [ ] **Step 1: Write the failing table test**

Create `tests/canonical.rs`:

```rust
use uu_tiktok::canonical::{canonicalize_url, Canonical};

#[test]
fn canonicalizes_form_1_tiktokv_share_video() {
    let result = canonicalize_url("https://www.tiktokv.com/share/video/7234567890123456789/");
    match result {
        Canonical::VideoId(id) => assert_eq!(id, "7234567890123456789"),
        other => panic!("expected VideoId, got {:?}", other),
    }
}

#[test]
fn canonicalizes_form_2_tiktok_user_video() {
    let result = canonicalize_url("https://www.tiktok.com/@coolcreator/video/7234567890123456789");
    match result {
        Canonical::VideoId(id) => assert_eq!(id, "7234567890123456789"),
        other => panic!("expected VideoId, got {:?}", other),
    }
}

#[test]
fn canonicalizes_form_1_with_query_string() {
    let result = canonicalize_url(
        "https://www.tiktokv.com/share/video/7234567890123456789/?utm_source=share",
    );
    match result {
        Canonical::VideoId(id) => assert_eq!(id, "7234567890123456789"),
        other => panic!("expected VideoId, got {:?}", other),
    }
}

#[test]
fn marks_short_link_form_3_as_needs_resolution() {
    let result = canonicalize_url("https://vm.tiktok.com/ZMabcdef/");
    match result {
        Canonical::NeedsResolution(url) => {
            assert_eq!(url, "https://vm.tiktok.com/ZMabcdef/");
        }
        other => panic!("expected NeedsResolution, got {:?}", other),
    }
}

#[test]
fn marks_short_link_form_4_as_needs_resolution() {
    let result = canonicalize_url("https://www.tiktok.com/t/ZTabcdef/");
    match result {
        Canonical::NeedsResolution(url) => {
            assert_eq!(url, "https://www.tiktok.com/t/ZTabcdef/");
        }
        other => panic!("expected NeedsResolution, got {:?}", other),
    }
}

#[test]
fn rejects_non_tiktok_url() {
    match canonicalize_url("https://example.com/video/123") {
        Canonical::Invalid(_) => {}
        other => panic!("expected Invalid, got {:?}", other),
    }
}

#[test]
fn rejects_malformed_url() {
    match canonicalize_url("not a url at all") {
        Canonical::Invalid(_) => {}
        other => panic!("expected Invalid, got {:?}", other),
    }
}
```

The integration test references the crate as `uu_tiktok` — the binary crate auto-exposes a library named after the package only if we have a `lib.rs`. Create `src/lib.rs` for this purpose:

Create `src/lib.rs`:
```rust
pub mod canonical;
```

(The library is purely for re-exporting modules to integration tests; the binary stays in `main.rs`.)

- [ ] **Step 2: Run the test to confirm it fails**

Run: `cargo test --test canonical 2>&1 | tail -15`
Expected: FAIL — `canonical` module does not exist.

- [ ] **Step 3: Implement `canonical.rs`**

Create `src/canonical.rs`:

```rust
use once_cell::sync::Lazy;
use regex::Regex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Canonical {
    /// URL parsed cleanly to a 19-digit numeric video_id (forms 1 and 2).
    VideoId(String),

    /// Short link (forms 3 and 4): `vm.tiktok.com/...` or `tiktok.com/t/...`.
    /// Cannot extract video_id without following the redirect. Plan C resolves
    /// these; Plan A logs and skips them.
    NeedsResolution(String),

    /// Not a TikTok URL or unparseable.
    Invalid(String),
}

// Form 1: https://www.tiktokv.com/share/video/{19-digit-id}/[?...]
// Form 2: https://www.tiktok.com/@username/video/{19-digit-id}[?...]
static CANONICAL_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"^https?://(?:www\.)?(?:tiktokv|tiktok)\.com/(?:share/video|@[^/]+/video)/(\d{19})(?:/|\?|$)",
    )
    .expect("canonical regex compiles")
});

// Forms 3 and 4: short links that 302 to a canonical form.
static SHORT_LINK_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^https?://(?:vm\.tiktok\.com|vt\.tiktok\.com|(?:www\.)?tiktok\.com/t)/[A-Za-z0-9]+/?$")
        .expect("short-link regex compiles")
});

pub fn canonicalize_url(url: &str) -> Canonical {
    if let Some(captures) = CANONICAL_RE.captures(url) {
        let id = captures.get(1).expect("group 1 captured").as_str();
        return Canonical::VideoId(id.to_string());
    }
    if SHORT_LINK_RE.is_match(url) {
        return Canonical::NeedsResolution(url.to_string());
    }
    Canonical::Invalid(url.to_string())
}
```

- [ ] **Step 4: Run integration test to confirm pass**

Run: `cargo test --test canonical 2>&1 | tail -10`
Expected: `7 passed; 0 failed`.

- [ ] **Step 5: Wire `canonical` into the binary too**

Add `mod canonical;` to `src/main.rs` so the binary can use it later (the integration test goes through `lib.rs`; the binary needs the same module reachable internally — `main.rs` and `lib.rs` are separate compilation units).

Run: `cargo build 2>&1 | tail -3`
Expected: clean build.

- [ ] **Step 6: Commit**

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings 2>&1 | tail -5
git add Cargo.lock src/canonical.rs src/lib.rs src/main.rs tests/canonical.rs
git commit -m "Plan A T5: canonical URL parsing for forms 1 and 2; short links flagged"
```

---

### Task 6: Subprocess runner (`process::run`)

**Files:**
- Create: `src/process.rs`
- Modify: `src/lib.rs`, `src/main.rs`

- [ ] **Step 1: Write the failing tests**

Create `src/process.rs`:

```rust
use std::time::Duration;

use thiserror::Error;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::time::timeout;

use crate::errors::FetchError;

#[derive(Debug)]
pub struct CommandSpec<'a> {
    pub program: &'static str,
    pub args: Vec<String>,
    pub timeout: Duration,
    /// Last-N bytes of stderr to retain. Avoids OOM on chatty tools.
    pub stderr_capture_bytes: usize,
    /// Argument indices to redact in the structured log (e.g., cookie file paths).
    pub redact_arg_indices: &'a [usize],
}

#[derive(Debug, Clone)]
pub struct CommandOutcome {
    pub exit_code: i32,
    pub stdout: Vec<u8>,
    pub stderr_excerpt: String,
    pub elapsed: Duration,
}

#[derive(Debug, Error)]
pub enum RunError {
    #[error("failed to spawn `{tool}`: {source}")]
    Spawn {
        tool: &'static str,
        #[source]
        source: std::io::Error,
    },

    #[error("subprocess `{tool}` timed out after {duration:?}")]
    Timeout {
        tool: &'static str,
        duration: Duration,
    },

    #[error("io error reading subprocess output for `{tool}`: {source}")]
    Io {
        tool: &'static str,
        #[source]
        source: std::io::Error,
    },
}

impl From<RunError> for FetchError {
    fn from(err: RunError) -> Self {
        match err {
            RunError::Timeout { tool, duration } => FetchError::ToolTimeout { tool, duration },
            RunError::Spawn { tool, source } => FetchError::NetworkError(format!(
                "failed to spawn {}: {}",
                tool, source
            )),
            RunError::Io { tool, source } => FetchError::NetworkError(format!(
                "io error reading {} output: {}",
                tool, source
            )),
        }
    }
}

#[tracing::instrument(level = "debug", skip(spec), fields(tool = spec.program))]
pub async fn run(spec: CommandSpec<'_>) -> Result<CommandOutcome, RunError> {
    let started = std::time::Instant::now();

    let logged_args: Vec<String> = spec
        .args
        .iter()
        .enumerate()
        .map(|(i, a)| {
            if spec.redact_arg_indices.contains(&i) {
                "<redacted>".into()
            } else {
                a.clone()
            }
        })
        .collect();
    tracing::debug!(args = ?logged_args, "spawning subprocess");

    let mut child = Command::new(spec.program)
        .args(&spec.args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|source| RunError::Spawn {
            tool: spec.program,
            source,
        })?;

    let mut stdout = child.stdout.take().expect("piped stdout");
    let mut stderr = child.stderr.take().expect("piped stderr");

    let read_outputs = async {
        let mut stdout_buf = Vec::new();
        let mut stderr_buf = Vec::new();
        tokio::try_join!(
            stdout.read_to_end(&mut stdout_buf),
            stderr.read_to_end(&mut stderr_buf),
        )?;
        Ok::<_, std::io::Error>((stdout_buf, stderr_buf))
    };

    let result = timeout(spec.timeout, async {
        let (stdout_buf, stderr_buf) = read_outputs.await.map_err(|source| RunError::Io {
            tool: spec.program,
            source,
        })?;
        let status = child.wait().await.map_err(|source| RunError::Io {
            tool: spec.program,
            source,
        })?;
        Ok::<_, RunError>((stdout_buf, stderr_buf, status))
    })
    .await;

    match result {
        Ok(Ok((stdout_buf, stderr_buf, status))) => {
            let exit_code = status.code().unwrap_or(-1);
            let stderr_excerpt = ring_buffer_tail(&stderr_buf, spec.stderr_capture_bytes);
            let elapsed = started.elapsed();
            Ok(CommandOutcome {
                exit_code,
                stdout: stdout_buf,
                stderr_excerpt,
                elapsed,
            })
        }
        Ok(Err(e)) => Err(e),
        Err(_elapsed) => {
            // Timed out: kill_on_drop will SIGKILL the child when `child` is dropped.
            // Drop here happens at the end of this async block.
            let _ = child.start_kill();
            Err(RunError::Timeout {
                tool: spec.program,
                duration: spec.timeout,
            })
        }
    }
}

fn ring_buffer_tail(buf: &[u8], cap: usize) -> String {
    if cap == 0 || buf.is_empty() {
        return String::new();
    }
    let start = buf.len().saturating_sub(cap);
    String::from_utf8_lossy(&buf[start..]).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn echo_succeeds_with_stdout() {
        let spec = CommandSpec {
            program: "echo",
            args: vec!["hello".into(), "world".into()],
            timeout: Duration::from_secs(5),
            stderr_capture_bytes: 1024,
            redact_arg_indices: &[],
        };
        let outcome = run(spec).await.expect("echo runs");
        assert_eq!(outcome.exit_code, 0);
        assert_eq!(String::from_utf8_lossy(&outcome.stdout).trim(), "hello world");
    }

    #[tokio::test]
    async fn false_returns_nonzero_exit() {
        let spec = CommandSpec {
            program: "false",
            args: vec![],
            timeout: Duration::from_secs(5),
            stderr_capture_bytes: 1024,
            redact_arg_indices: &[],
        };
        let outcome = run(spec).await.expect("false runs");
        assert_ne!(outcome.exit_code, 0);
    }

    #[tokio::test]
    async fn timeout_kills_long_running_subprocess() {
        let spec = CommandSpec {
            program: "sleep",
            args: vec!["10".into()],
            timeout: Duration::from_millis(200),
            stderr_capture_bytes: 1024,
            redact_arg_indices: &[],
        };
        let result = run(spec).await;
        match result {
            Err(RunError::Timeout { tool, .. }) => assert_eq!(tool, "sleep"),
            other => panic!("expected timeout, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn missing_program_returns_spawn_error() {
        let spec = CommandSpec {
            program: "this-program-does-not-exist-1234567",
            args: vec![],
            timeout: Duration::from_secs(5),
            stderr_capture_bytes: 1024,
            redact_arg_indices: &[],
        };
        let result = run(spec).await;
        match result {
            Err(RunError::Spawn { .. }) => {}
            other => panic!("expected Spawn error, got {:?}", other),
        }
    }
}
```

- [ ] **Step 2: Wire `process` into the binary and library**

Add `mod process;` to both `src/main.rs` and `src/lib.rs` (after the existing `mod` lines). Also add `mod errors;` to `src/lib.rs` (the runner depends on it).

- [ ] **Step 3: Run tests to confirm pass**

Run: `cargo test process:: 2>&1 | tail -15`
Expected: `4 passed; 0 failed`. The timeout test should complete in well under a second.

- [ ] **Step 4: Commit**

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings 2>&1 | tail -5
git add src/process.rs src/main.rs src/lib.rs
git commit -m "Plan A T6: subprocess runner with timeout and stderr ring buffer"
```

---

### Task 7: SQLite schema + `Store::open` + migrations

**Files:**
- Create: `src/state/mod.rs`
- Create: `src/state/schema.rs`
- Modify: `src/lib.rs`, `src/main.rs`
- Test: `tests/state_open.rs`

Plan A schema is a deliberate subset of the spec's full schema. Plan B adds failure persistence columns; Plan C adds `pending_resolutions`.

- [ ] **Step 1: Write the failing schema test**

Create `tests/state_open.rs`:

```rust
use tempfile::TempDir;
use uu_tiktok::state::Store;

#[test]
fn open_creates_schema_on_fresh_db() {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("state.sqlite");

    let _store = Store::open(&db_path).expect("open succeeds");

    assert!(db_path.exists());
}

#[test]
fn open_is_idempotent() {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("state.sqlite");

    let _first = Store::open(&db_path).expect("first open");
    drop(_first);
    let _second = Store::open(&db_path).expect("second open does not fail");
}

#[test]
fn schema_version_is_recorded() {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("state.sqlite");

    let store = Store::open(&db_path).expect("open succeeds");
    let version: String = store
        .read_meta("schema_version")
        .expect("read_meta succeeds")
        .expect("schema_version present");
    assert_eq!(version, "1");
}

#[test]
fn pragma_journal_mode_is_wal() {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("state.sqlite");

    let store = Store::open(&db_path).expect("open succeeds");
    let mode = store.pragma_string("journal_mode").expect("read pragma");
    assert_eq!(mode.to_lowercase(), "wal");
}
```

- [ ] **Step 2: Implement schema and `Store::open`**

Create `src/state/schema.rs`:

```rust
pub const SCHEMA_VERSION: &str = "1";

pub const SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS videos (
    video_id            TEXT PRIMARY KEY,
    source_url          TEXT NOT NULL,
    canonical           INTEGER NOT NULL,
    status              TEXT NOT NULL CHECK (status IN
                          ('pending','in_progress','succeeded','failed_terminal','failed_retryable')),
    claimed_by          TEXT,
    claimed_at          INTEGER,
    attempt_count       INTEGER NOT NULL DEFAULT 0,
    succeeded_at        INTEGER,
    duration_s          REAL,
    language_detected   TEXT,
    fetcher             TEXT,
    transcript_source   TEXT,
    first_seen_at       INTEGER NOT NULL,
    updated_at          INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_videos_pending
    ON videos (status, first_seen_at, video_id)
    WHERE status = 'pending';

CREATE TABLE IF NOT EXISTS watch_history (
    respondent_id  TEXT NOT NULL,
    video_id       TEXT NOT NULL,
    watched_at     INTEGER NOT NULL,
    in_window      INTEGER NOT NULL,
    PRIMARY KEY (respondent_id, video_id, watched_at),
    FOREIGN KEY (video_id) REFERENCES videos(video_id)
);
CREATE INDEX IF NOT EXISTS idx_watch_history_video ON watch_history (video_id);

CREATE TABLE IF NOT EXISTS video_events (
    id           INTEGER PRIMARY KEY,
    video_id     TEXT NOT NULL,
    at           INTEGER NOT NULL,
    event_type   TEXT NOT NULL,
    worker_id    TEXT,
    detail_json  TEXT,
    FOREIGN KEY (video_id) REFERENCES videos(video_id)
);
CREATE INDEX IF NOT EXISTS idx_video_events_video ON video_events (video_id, at);

CREATE TABLE IF NOT EXISTS meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
"#;
```

Create `src/state/mod.rs`:

```rust
mod schema;

use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::{params, Connection};

pub use schema::SCHEMA_VERSION;

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path).with_context(|| {
            format!("opening SQLite database at {}", path.display())
        })?;

        // Pragmas applied at every open.
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;
             PRAGMA busy_timeout = 5000;",
        )
        .context("setting connection pragmas")?;

        // Schema (idempotent — uses CREATE IF NOT EXISTS).
        conn.execute_batch(schema::SCHEMA_SQL)
            .context("applying schema")?;

        // Record schema version (only on first run).
        conn.execute(
            "INSERT OR IGNORE INTO meta (key, value) VALUES ('schema_version', ?1)",
            params![SCHEMA_VERSION],
        )
        .context("recording schema_version")?;

        Ok(Self { conn })
    }

    pub fn read_meta(&self, key: &str) -> Result<Option<String>> {
        let result = self
            .conn
            .query_row(
                "SELECT value FROM meta WHERE key = ?1",
                params![key],
                |row| row.get::<_, String>(0),
            )
            .map_or_else(
                |e| match e {
                    rusqlite::Error::QueryReturnedNoRows => Ok(None),
                    other => Err(other),
                },
                |v| Ok(Some(v)),
            )?;
        Ok(result)
    }

    pub fn pragma_string(&self, name: &str) -> Result<String> {
        let value: String = self
            .conn
            .query_row(&format!("PRAGMA {}", name), [], |row| row.get(0))
            .with_context(|| format!("reading PRAGMA {}", name))?;
        Ok(value)
    }

    /// Borrow the underlying connection for advanced operations. Internal use
    /// for now; the public API will grow as Tasks 9+ add methods.
    pub(crate) fn conn(&self) -> &Connection {
        &self.conn
    }

    pub(crate) fn conn_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }
}
```

- [ ] **Step 3: Wire into `lib.rs` and `main.rs`**

Add `pub mod state;` to `src/lib.rs`. Add `mod state;` to `src/main.rs`.

- [ ] **Step 4: Run tests to confirm pass**

Run: `cargo test --test state_open 2>&1 | tail -10`
Expected: `4 passed; 0 failed`.

- [ ] **Step 5: Commit**

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings 2>&1 | tail -5
git add src/state/ src/main.rs src/lib.rs tests/state_open.rs
git commit -m "Plan A T7: SQLite schema and Store::open with WAL/foreign-keys pragmas"
```

---

### Task 8: `output::shard_path` + atomic write helper

**Files:**
- Create: `src/output/mod.rs`
- Create: `src/output/artifacts.rs`
- Modify: `src/lib.rs`, `src/main.rs`

- [ ] **Step 1: Write the failing tests**

Create `src/output/mod.rs`:

```rust
pub mod artifacts;

use std::path::{Path, PathBuf};

/// Returns the shard segment for a video_id: the last two characters.
/// Snowflake low digits are essentially random, giving uniform 100-bucket
/// distribution. The single source of truth for path layout — no other
/// module hard-codes a path scheme.
pub fn shard(video_id: &str) -> &str {
    let len = video_id.len();
    if len < 2 {
        return video_id;
    }
    &video_id[len - 2..]
}

/// Returns `{transcripts_root}/{shard}/` (does NOT create the directory).
pub fn shard_dir(transcripts_root: &Path, video_id: &str) -> PathBuf {
    transcripts_root.join(shard(video_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn shard_uses_last_two_chars() {
        assert_eq!(shard("7234567890123456789"), "89");
        assert_eq!(shard("0000000000000000001"), "01");
    }

    #[test]
    fn shard_handles_short_ids() {
        assert_eq!(shard("7"), "7");
        assert_eq!(shard("12"), "12");
    }

    #[test]
    fn shard_dir_joins_correctly() {
        let root = PathBuf::from("/data/transcripts");
        assert_eq!(
            shard_dir(&root, "7234567890123456789"),
            PathBuf::from("/data/transcripts/89")
        );
    }

    /// Distribution test — synthesise IDs and verify no shard is wildly under
    /// or over-represented. Catches a regression where someone uses the high
    /// digits (which are time-clustered) instead of the low digits.
    #[test]
    fn shard_distributes_uniformly() {
        use std::collections::HashMap;

        let mut counts: HashMap<String, usize> = HashMap::new();
        // Synthetic IDs: monotonically increasing 19-digit numbers.
        let base: u64 = 7_000_000_000_000_000_000;
        for i in 0..10_000u64 {
            let id = format!("{}", base + i);
            *counts.entry(shard(&id).to_string()).or_default() += 1;
        }

        // 10000 / 100 buckets = 100 mean. Each bucket should be within ±50% of mean
        // (i.e., 50..=150). Lenient bound for synthetic counter input; real Snowflake
        // IDs would be tighter.
        for (bucket, n) in &counts {
            assert!(
                (50..=150).contains(n),
                "bucket {} has {} items, outside 50..=150",
                bucket,
                n
            );
        }
        assert_eq!(counts.len(), 100, "expected 100 distinct buckets");
    }
}
```

Create `src/output/artifacts.rs`:

```rust
use std::fs::File;
use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};

/// Atomic write for one file: write to `{path}.tmp`, fsync, rename to `{path}`,
/// fsync the parent directory. Caller is responsible for parent existence.
pub fn atomic_write(path: &Path, contents: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .with_context(|| format!("path {} has no parent", path.display()))?;

    let mut tmp_path = path.to_path_buf();
    let tmp_name = format!(
        "{}.tmp",
        path.file_name()
            .and_then(|s| s.to_str())
            .with_context(|| format!("path {} has no filename", path.display()))?
    );
    tmp_path.set_file_name(tmp_name);

    {
        let mut f = File::create(&tmp_path).with_context(|| {
            format!("creating tmp file {}", tmp_path.display())
        })?;
        f.write_all(contents)
            .with_context(|| format!("writing tmp file {}", tmp_path.display()))?;
        f.sync_all()
            .with_context(|| format!("fsyncing tmp file {}", tmp_path.display()))?;
    }

    std::fs::rename(&tmp_path, path).with_context(|| {
        format!(
            "renaming {} to {}",
            tmp_path.display(),
            path.display()
        )
    })?;

    let dir = File::open(parent).with_context(|| {
        format!("opening parent dir {} for fsync", parent.display())
    })?;
    dir.sync_all().with_context(|| {
        format!("fsyncing parent dir {}", parent.display())
    })?;

    Ok(())
}

/// Sweep all `*.tmp` files under the transcripts root. Called at process
/// startup so leftover tmp files from crashed runs don't accumulate.
pub fn cleanup_tmp_files(transcripts_root: &Path) -> Result<usize> {
    if !transcripts_root.exists() {
        return Ok(0);
    }
    let mut removed = 0;
    for entry in std::fs::read_dir(transcripts_root).with_context(|| {
        format!("reading transcripts root {}", transcripts_root.display())
    })? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            for shard_entry in std::fs::read_dir(&path)? {
                let shard_entry = shard_entry?;
                let p = shard_entry.path();
                if p.extension().and_then(|s| s.to_str()) == Some("tmp") {
                    let _ = std::fs::remove_file(&p);
                    removed += 1;
                }
            }
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn atomic_write_creates_file_and_no_tmp_remains() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("hello.txt");
        atomic_write(&target, b"world").expect("write succeeds");

        assert_eq!(std::fs::read_to_string(&target).unwrap(), "world");
        let tmp_path = tmp.path().join("hello.txt.tmp");
        assert!(!tmp_path.exists(), "tmp file should be renamed away");
    }

    #[test]
    fn atomic_write_overwrites_existing_file() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("hello.txt");
        atomic_write(&target, b"first").unwrap();
        atomic_write(&target, b"second").unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "second");
    }

    #[test]
    fn cleanup_tmp_removes_tmp_files_in_shard_dirs() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        // Set up shard 89 with one tmp file and one real file.
        let shard_dir = root.join("89");
        std::fs::create_dir_all(&shard_dir).unwrap();
        std::fs::write(shard_dir.join("video.txt.tmp"), b"junk").unwrap();
        std::fs::write(shard_dir.join("video.txt"), b"real").unwrap();

        let removed = cleanup_tmp_files(root).unwrap();
        assert_eq!(removed, 1);
        assert!(!shard_dir.join("video.txt.tmp").exists());
        assert!(shard_dir.join("video.txt").exists());
    }
}
```

- [ ] **Step 2: Wire `output` into the binary and library**

Add `pub mod output;` to `src/lib.rs`. Add `mod output;` to `src/main.rs`.

- [ ] **Step 3: Run tests to confirm pass**

Run: `cargo test output 2>&1 | tail -15`
Expected: 6 passed; 0 failed.

- [ ] **Step 4: Commit**

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings 2>&1 | tail -5
git add src/output/ src/main.rs src/lib.rs
git commit -m "Plan A T8: output::shard helper + atomic write + tmp cleanup"
```

---

### Task 9: `Store` ingest methods (upsert)

**Files:**
- Modify: `src/state/mod.rs`
- Test: `tests/state_ingest.rs`

- [ ] **Step 1: Write the failing tests**

Create `tests/state_ingest.rs`:

```rust
use tempfile::TempDir;
use uu_tiktok::state::Store;

fn fresh_store() -> (TempDir, Store) {
    let tmp = TempDir::new().unwrap();
    let store = Store::open(&tmp.path().join("state.sqlite")).expect("open");
    (tmp, store)
}

#[test]
fn upsert_video_inserts_new_row_with_pending_status() {
    let (_tmp, mut store) = fresh_store();
    store
        .upsert_video(
            "7234567890123456789",
            "https://www.tiktokv.com/share/video/7234567890123456789/",
            true,
        )
        .expect("upsert");

    let row = store.get_video_for_test("7234567890123456789").unwrap().expect("row exists");
    assert_eq!(row.status, "pending");
    assert_eq!(row.canonical, true);
    assert!(row.first_seen_at > 0);
    assert_eq!(row.attempt_count, 0);
}

#[test]
fn upsert_video_is_idempotent_first_seen_at_unchanged() {
    let (_tmp, mut store) = fresh_store();
    store
        .upsert_video("7234567890123456789", "url-A", true)
        .unwrap();
    let first = store
        .get_video_for_test("7234567890123456789")
        .unwrap()
        .unwrap();

    std::thread::sleep(std::time::Duration::from_millis(1100));

    store
        .upsert_video("7234567890123456789", "url-B", true)
        .unwrap();
    let second = store
        .get_video_for_test("7234567890123456789")
        .unwrap()
        .unwrap();

    assert_eq!(
        first.first_seen_at, second.first_seen_at,
        "first_seen_at must NOT change on re-upsert"
    );
    assert_eq!(
        first.source_url, second.source_url,
        "source_url must NOT change on re-upsert (we keep the first one)"
    );
}

#[test]
fn upsert_watch_history_inserts() {
    let (_tmp, mut store) = fresh_store();
    store
        .upsert_video("7234567890123456789", "https://example/video/7234567890123456789", true)
        .unwrap();
    let inserted = store
        .upsert_watch_history("respondent-1", "7234567890123456789", 1_700_000_000, true)
        .unwrap();
    assert_eq!(inserted, 1);
}

#[test]
fn upsert_watch_history_is_idempotent_on_duplicate_pk() {
    let (_tmp, mut store) = fresh_store();
    store
        .upsert_video("7234567890123456789", "url", true)
        .unwrap();
    let first = store
        .upsert_watch_history("r1", "7234567890123456789", 1_700_000_000, true)
        .unwrap();
    let second = store
        .upsert_watch_history("r1", "7234567890123456789", 1_700_000_000, false)
        .unwrap();
    assert_eq!(first, 1);
    assert_eq!(second, 0, "duplicate PK insert returns 0 rows changed");
}

#[test]
fn upsert_watch_history_fk_violation_when_video_missing() {
    let (_tmp, mut store) = fresh_store();
    let result = store.upsert_watch_history("r1", "7234567890123456789", 1_700_000_000, true);
    assert!(result.is_err(), "FK should fail when video row absent");
}
```

- [ ] **Step 2: Implement the methods on `Store`**

Append to `src/state/mod.rs` (inside `impl Store`):

```rust
    pub fn upsert_video(
        &mut self,
        video_id: &str,
        source_url: &str,
        canonical: bool,
    ) -> Result<()> {
        let now = unix_now();
        self.conn
            .execute(
                "INSERT OR IGNORE INTO videos
                 (video_id, source_url, canonical, status,
                  first_seen_at, updated_at)
                 VALUES (?1, ?2, ?3, 'pending', ?4, ?4)",
                params![video_id, source_url, canonical as i64, now],
            )
            .with_context(|| format!("upserting video {}", video_id))?;
        Ok(())
    }

    pub fn upsert_watch_history(
        &mut self,
        respondent_id: &str,
        video_id: &str,
        watched_at: i64,
        in_window: bool,
    ) -> Result<usize> {
        let changed = self
            .conn
            .execute(
                "INSERT OR IGNORE INTO watch_history
                 (respondent_id, video_id, watched_at, in_window)
                 VALUES (?1, ?2, ?3, ?4)",
                params![respondent_id, video_id, watched_at, in_window as i64],
            )
            .with_context(|| {
                format!(
                    "upserting watch_history (respondent={}, video={}, watched_at={})",
                    respondent_id, video_id, watched_at
                )
            })?;
        Ok(changed)
    }
```

Add this helper near the top of the file (after the imports):

```rust
fn unix_now() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Test-only helper for verifying row state. Not part of the public API; gated
/// to test compilation only.
#[cfg(any(test, feature = "test-helpers"))]
#[derive(Debug, Clone)]
pub struct VideoRow {
    pub video_id: String,
    pub status: String,
    pub canonical: bool,
    pub source_url: String,
    pub first_seen_at: i64,
    pub attempt_count: i64,
}

impl Store {
    #[cfg(any(test, feature = "test-helpers"))]
    pub fn get_video_for_test(&self, video_id: &str) -> Result<Option<VideoRow>> {
        use rusqlite::OptionalExtension;
        let row = self
            .conn
            .query_row(
                "SELECT video_id, status, canonical, source_url, first_seen_at, attempt_count
                 FROM videos WHERE video_id = ?1",
                params![video_id],
                |r| {
                    Ok(VideoRow {
                        video_id: r.get(0)?,
                        status: r.get(1)?,
                        canonical: r.get::<_, i64>(2)? != 0,
                        source_url: r.get(3)?,
                        first_seen_at: r.get(4)?,
                        attempt_count: r.get(5)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }
}
```

The `#[cfg(any(test, feature = "test-helpers"))]` makes `get_video_for_test` available for both unit tests in this crate AND integration tests in `tests/`. Add the feature gate to `Cargo.toml` so integration tests can opt in:

```toml
[features]
test-helpers = []

[[test]]
name = "state_ingest"
required-features = ["test-helpers"]
```

(Append the `[features]` block at the end of `Cargo.toml`. Add the `[[test]]` block too.)

- [ ] **Step 3: Run tests to confirm pass**

Run: `cargo test --test state_ingest --features test-helpers 2>&1 | tail -15`
Expected: 5 passed; 0 failed.

- [ ] **Step 4: Commit**

```bash
cargo fmt
cargo clippy --all-targets --features test-helpers -- -D warnings 2>&1 | tail -5
git add src/state/mod.rs Cargo.toml Cargo.lock tests/state_ingest.rs
git commit -m "Plan A T9: Store::upsert_video and upsert_watch_history (INSERT OR IGNORE)"
```

---

### Task 10: `Store::claim_next` + `mark_succeeded` (transactional)

**Files:**
- Modify: `src/state/mod.rs`
- Test: `tests/state_claims.rs`

- [ ] **Step 1: Write the failing tests**

Create `tests/state_claims.rs`:

```rust
use tempfile::TempDir;
use uu_tiktok::state::{Claim, Store, SuccessArtifacts};

fn fresh_store_with(videos: &[(&str, &str)]) -> (TempDir, Store) {
    let tmp = TempDir::new().unwrap();
    let mut store = Store::open(&tmp.path().join("state.sqlite")).unwrap();
    for (id, url) in videos {
        store.upsert_video(id, url, true).unwrap();
    }
    (tmp, store)
}

#[test]
fn claim_next_returns_none_on_empty_db() {
    let (_tmp, mut store) = fresh_store_with(&[]);
    let claim = store.claim_next("worker-1").unwrap();
    assert!(claim.is_none());
}

#[test]
fn claim_next_returns_pending_video_and_marks_in_progress() {
    let (_tmp, mut store) = fresh_store_with(&[("7234567890123456789", "url")]);

    let claim = store.claim_next("worker-1").unwrap().expect("claim");
    assert_eq!(claim.video_id, "7234567890123456789");
    assert_eq!(claim.source_url, "url");

    let row = store.get_video_for_test("7234567890123456789").unwrap().unwrap();
    assert_eq!(row.status, "in_progress");
    assert_eq!(row.attempt_count, 1, "attempt_count incremented on claim");
}

#[test]
fn claim_next_orders_by_first_seen_at() {
    let (_tmp, mut store) = fresh_store_with(&[]);
    store.upsert_video("7234567890123456789", "first", true).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    store.upsert_video("7234567890123456788", "second", true).unwrap();

    let first_claim = store.claim_next("w").unwrap().unwrap();
    assert_eq!(first_claim.video_id, "7234567890123456789");
}

#[test]
fn mark_succeeded_writes_status_and_event_in_one_transaction() {
    let (_tmp, mut store) = fresh_store_with(&[("7234567890123456789", "url")]);
    let claim = store.claim_next("w").unwrap().unwrap();
    assert_eq!(claim.video_id, "7234567890123456789");

    let artifacts = SuccessArtifacts {
        duration_s: Some(23.4),
        language_detected: Some("en".into()),
        fetcher: "ytdlp",
        transcript_source: "whisper.cpp",
    };
    store.mark_succeeded(&claim.video_id, artifacts).unwrap();

    let row = store.get_video_for_test(&claim.video_id).unwrap().unwrap();
    assert_eq!(row.status, "succeeded");
    let events = store.get_events_for_test(&claim.video_id).unwrap();
    let kinds: Vec<&str> = events.iter().map(|e| e.event_type.as_str()).collect();
    assert!(kinds.contains(&"claimed"), "claimed event recorded");
    assert!(kinds.contains(&"succeeded"), "succeeded event recorded");
}

#[test]
fn concurrent_claim_serializes_via_begin_immediate() {
    // Two distinct connections to the same DB. claim_next must atomically
    // pick a row so only one connection succeeds for a given pending row.
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("state.sqlite");

    let mut store_a = Store::open(&path).unwrap();
    let mut store_b = Store::open(&path).unwrap();

    store_a
        .upsert_video("7234567890123456789", "url", true)
        .unwrap();

    let a = store_a.claim_next("worker-a").unwrap();
    let b = store_b.claim_next("worker-b").unwrap();

    let claimed: Vec<&Claim> = [a.as_ref(), b.as_ref()].into_iter().flatten().collect();
    assert_eq!(claimed.len(), 1, "exactly one connection wins the row");
}
```

- [ ] **Step 2: Implement `claim_next` and `mark_succeeded`**

Append to `src/state/mod.rs`:

```rust
#[derive(Debug, Clone)]
pub struct Claim {
    pub video_id: String,
    pub source_url: String,
    pub attempt_count: i64,
}

#[derive(Debug, Clone)]
pub struct SuccessArtifacts {
    pub duration_s: Option<f64>,
    pub language_detected: Option<String>,
    pub fetcher: &'static str,
    pub transcript_source: &'static str,
}

impl Store {
    pub fn claim_next(&mut self, worker_id: &str) -> Result<Option<Claim>> {
        let now = unix_now();
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .context("begin immediate for claim_next")?;

        let candidate: Option<(String, String, i64)> = tx
            .query_row(
                "SELECT video_id, source_url, attempt_count
                 FROM videos
                 WHERE status = 'pending'
                 ORDER BY first_seen_at ASC, video_id ASC
                 LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;

        let Some((video_id, source_url, prev_attempts)) = candidate else {
            tx.commit()?;
            return Ok(None);
        };

        let new_attempts = prev_attempts + 1;
        tx.execute(
            "UPDATE videos
             SET status = 'in_progress',
                 claimed_by = ?2,
                 claimed_at = ?3,
                 attempt_count = ?4,
                 updated_at = ?3
             WHERE video_id = ?1",
            params![video_id, worker_id, now, new_attempts],
        )?;

        tx.execute(
            "INSERT INTO video_events (video_id, at, event_type, worker_id, detail_json)
             VALUES (?1, ?2, 'claimed', ?3, NULL)",
            params![video_id, now, worker_id],
        )?;

        tx.commit().context("commit claim transaction")?;

        Ok(Some(Claim {
            video_id,
            source_url,
            attempt_count: new_attempts,
        }))
    }

    pub fn mark_succeeded(
        &mut self,
        video_id: &str,
        artifacts: SuccessArtifacts,
    ) -> Result<()> {
        let now = unix_now();
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .context("begin immediate for mark_succeeded")?;

        tx.execute(
            "UPDATE videos
             SET status = 'succeeded',
                 succeeded_at = ?2,
                 duration_s = ?3,
                 language_detected = ?4,
                 fetcher = ?5,
                 transcript_source = ?6,
                 updated_at = ?2
             WHERE video_id = ?1",
            params![
                video_id,
                now,
                artifacts.duration_s,
                artifacts.language_detected,
                artifacts.fetcher,
                artifacts.transcript_source,
            ],
        )
        .with_context(|| format!("update videos for succeeded {}", video_id))?;

        tx.execute(
            "INSERT INTO video_events (video_id, at, event_type, worker_id, detail_json)
             VALUES (?1, ?2, 'succeeded', NULL, NULL)",
            params![video_id, now],
        )?;

        tx.commit().context("commit mark_succeeded")?;
        Ok(())
    }
}

#[cfg(any(test, feature = "test-helpers"))]
#[derive(Debug, Clone)]
pub struct EventRow {
    pub event_type: String,
    pub worker_id: Option<String>,
}

#[cfg(any(test, feature = "test-helpers"))]
impl Store {
    pub fn get_events_for_test(&self, video_id: &str) -> Result<Vec<EventRow>> {
        let mut stmt = self
            .conn
            .prepare("SELECT event_type, worker_id FROM video_events WHERE video_id = ?1 ORDER BY id")?;
        let rows: Vec<EventRow> = stmt
            .query_map(params![video_id], |r| {
                Ok(EventRow {
                    event_type: r.get(0)?,
                    worker_id: r.get(1)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        Ok(rows)
    }
}
```

Register the new test file in `Cargo.toml`:

```toml
[[test]]
name = "state_claims"
required-features = ["test-helpers"]
```

- [ ] **Step 3: Run tests to confirm pass**

Run: `cargo test --features test-helpers --test state_claims 2>&1 | tail -15`
Expected: 5 passed; 0 failed.

- [ ] **Step 4: Commit**

```bash
cargo fmt
cargo clippy --all-targets --features test-helpers -- -D warnings 2>&1 | tail -5
git add src/state/mod.rs Cargo.toml tests/state_claims.rs
git commit -m "Plan A T10: Store::claim_next (BEGIN IMMEDIATE) + mark_succeeded (transactional)"
```

---

### Task 11: `VideoFetcher` trait + `YtDlpFetcher`

**Files:**
- Create: `src/fetcher/mod.rs`
- Create: `src/fetcher/ytdlp.rs`
- Modify: `src/lib.rs`, `src/main.rs`

Plan A's `Acquisition` is intentionally simpler than the spec's full enum: only `AudioFile` and `Failed`. No `Unavailable` (Plan B), no `ReadyTranscript` (Plan C with API), no metadata bundle (Plan B for normalized; Plan C for raw + comments).

- [ ] **Step 1: Write the trait + fake fetcher tests**

Create `src/fetcher/mod.rs`:

```rust
pub mod ytdlp;

use std::path::PathBuf;

use async_trait::async_trait;

use crate::errors::FetchError;

#[derive(Debug)]
pub enum Acquisition {
    /// Audio file written to disk; pipeline will hand to whisper.cpp next.
    AudioFile(PathBuf),
}

#[async_trait]
pub trait VideoFetcher: Send + Sync {
    async fn acquire(
        &self,
        video_id: &str,
        source_url: &str,
    ) -> Result<Acquisition, FetchError>;
}

#[cfg(any(test, feature = "test-helpers"))]
pub struct FakeFetcher {
    pub canned: std::sync::Mutex<std::collections::HashMap<String, std::path::PathBuf>>,
}

#[cfg(any(test, feature = "test-helpers"))]
#[async_trait]
impl VideoFetcher for FakeFetcher {
    async fn acquire(
        &self,
        video_id: &str,
        _source_url: &str,
    ) -> Result<Acquisition, FetchError> {
        let map = self.canned.lock().expect("canned mutex");
        match map.get(video_id) {
            Some(path) => Ok(Acquisition::AudioFile(path.clone())),
            None => Err(FetchError::ParseError(format!(
                "FakeFetcher has no canned response for {}",
                video_id
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;

    #[tokio::test]
    async fn fake_fetcher_returns_canned_audio_file() {
        let map = HashMap::from([(
            "7234567890123456789".to_string(),
            PathBuf::from("/tmp/fake.wav"),
        )]);
        let fake = FakeFetcher {
            canned: std::sync::Mutex::new(map),
        };
        let result = fake.acquire("7234567890123456789", "url").await.unwrap();
        match result {
            Acquisition::AudioFile(p) => assert_eq!(p, PathBuf::from("/tmp/fake.wav")),
        }
    }
}
```

- [ ] **Step 2: Implement `YtDlpFetcher`**

Create `src/fetcher/ytdlp.rs`:

```rust
use std::path::{Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;

use crate::errors::FetchError;
use crate::fetcher::{Acquisition, VideoFetcher};
use crate::process::{run, CommandSpec};

pub struct YtDlpFetcher {
    /// Directory under which yt-dlp writes per-video subdirectories. Caller
    /// supplies a writable path under `transcripts_root`.
    pub work_dir: PathBuf,
    pub timeout: Duration,
}

impl YtDlpFetcher {
    pub fn new(work_dir: impl AsRef<Path>, timeout: Duration) -> Self {
        Self {
            work_dir: work_dir.as_ref().to_path_buf(),
            timeout,
        }
    }
}

#[async_trait]
impl VideoFetcher for YtDlpFetcher {
    async fn acquire(
        &self,
        video_id: &str,
        source_url: &str,
    ) -> Result<Acquisition, FetchError> {
        // Per-video tmp dir keeps yt-dlp's intermediate files contained.
        let video_dir = self.work_dir.join(format!("ytdlp-{}", video_id));
        std::fs::create_dir_all(&video_dir)
            .map_err(|e| FetchError::NetworkError(format!(
                "creating yt-dlp work dir {}: {}",
                video_dir.display(),
                e
            )))?;

        // Output template: write to {video_dir}/{video_id}.%(ext)s
        let output_template = format!("{}/{}.%(ext)s", video_dir.display(), video_id);

        let args = vec![
            "--no-playlist".into(),
            "--no-warnings".into(),
            "--quiet".into(),
            "-x".into(),
            "--audio-format".into(),
            "wav".into(),
            "--postprocessor-args".into(),
            "ffmpeg:-ar 16000 -ac 1".into(),
            "-o".into(),
            output_template,
            source_url.to_string(),
        ];

        let outcome = run(CommandSpec {
            program: "yt-dlp",
            args,
            timeout: self.timeout,
            stderr_capture_bytes: 8 * 1024,
            redact_arg_indices: &[],
        })
        .await?;

        if outcome.exit_code != 0 {
            return Err(FetchError::ToolFailed {
                tool: "yt-dlp",
                exit_code: outcome.exit_code,
                stderr_excerpt: outcome.stderr_excerpt,
            });
        }

        // Expected output: {video_dir}/{video_id}.wav
        let wav_path = video_dir.join(format!("{}.wav", video_id));
        if !wav_path.exists() {
            return Err(FetchError::ParseError(format!(
                "yt-dlp succeeded but expected file {} not found",
                wav_path.display()
            )));
        }

        Ok(Acquisition::AudioFile(wav_path))
    }
}
```

- [ ] **Step 3: Wire `fetcher` into the binary and library**

Add `pub mod fetcher;` to `src/lib.rs`. Add `mod fetcher;` to `src/main.rs`.

- [ ] **Step 4: Verify build and run unit tests**

Run:
```bash
cargo build 2>&1 | tail -3
cargo test --features test-helpers fetcher:: 2>&1 | tail -10
```
Expected: build clean; `1 passed; 0 failed`. (No real-network test of `YtDlpFetcher` here — that goes in Task 14's e2e test marked `#[ignore]`.)

- [ ] **Step 5: Commit**

```bash
cargo fmt
cargo clippy --all-targets --features test-helpers -- -D warnings 2>&1 | tail -5
git add src/fetcher/ src/main.rs src/lib.rs
git commit -m "Plan A T11: VideoFetcher trait + YtDlpFetcher (audio-only happy path)"
```

---

### Task 12: `transcribe` module

**Files:**
- Create: `src/transcribe.rs`
- Modify: `src/lib.rs`, `src/main.rs`

- [ ] **Step 1: Write the transcribe API skeleton**

Create `src/transcribe.rs`:

```rust
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Context;

use crate::errors::TranscribeError;
use crate::process::{run, CommandSpec};

#[derive(Debug, Clone)]
pub struct TranscribeOptions {
    pub model_path: PathBuf,
    pub use_gpu: bool,
    pub threads: usize,
    pub timeout: Duration,
}

#[derive(Debug, Clone)]
pub struct TranscribeResult {
    pub text: String,
    pub language: Option<String>,
    pub duration_s: Option<f64>,
}

/// Run whisper.cpp on the given WAV. Returns the transcript text plus
/// whatever metadata whisper.cpp reports (language detected, duration).
pub async fn transcribe(
    audio_path: &Path,
    opts: &TranscribeOptions,
) -> Result<TranscribeResult, TranscribeError> {
    let mut args: Vec<String> = vec![
        "-m".into(),
        opts.model_path.to_string_lossy().into_owned(),
        "-f".into(),
        audio_path.to_string_lossy().into_owned(),
        "-otxt".into(),
        "-of".into(),
        // Tell whisper.cpp to write the output text alongside the audio,
        // using the audio's stem as the prefix. We then read the resulting
        // .txt file. Without -of, whisper.cpp's auto-named output has been
        // an inconsistent target across versions.
        audio_path.with_extension("").to_string_lossy().into_owned(),
        "-t".into(),
        opts.threads.to_string(),
        "--language".into(),
        "auto".into(),
        "--print-progress".into(),
    ];
    if !opts.use_gpu {
        args.push("--no-gpu".into());
    }

    let outcome = run(CommandSpec {
        program: "whisper-cli",
        args,
        timeout: opts.timeout,
        stderr_capture_bytes: 8 * 1024,
        redact_arg_indices: &[],
    })
    .await
    .map_err(|e| match e {
        crate::process::RunError::Timeout { duration, .. } => {
            TranscribeError::Timeout { duration }
        }
        other => TranscribeError::Failed {
            exit_code: -1,
            stderr_excerpt: other.to_string(),
        },
    })?;

    if outcome.exit_code != 0 {
        return Err(TranscribeError::Failed {
            exit_code: outcome.exit_code,
            stderr_excerpt: outcome.stderr_excerpt,
        });
    }

    // whisper.cpp wrote {audio_path-stem}.txt
    let txt_path = audio_path.with_extension("txt");
    let text = std::fs::read_to_string(&txt_path)
        .map_err(|e| TranscribeError::Failed {
            exit_code: 0,
            stderr_excerpt: format!("reading {}: {}", txt_path.display(), e),
        })?
        .trim()
        .to_string();

    if text.is_empty() {
        return Err(TranscribeError::EmptyOutput);
    }

    // whisper-cli prints "auto-detected language: en (p = ...)" to stderr.
    // Cheap parse; on failure we just return None.
    let language = parse_language(&outcome.stderr_excerpt);

    Ok(TranscribeResult {
        text,
        language,
        duration_s: None, // Plan A: we don't extract duration; Plan B can add via ffprobe.
    })
}

fn parse_language(stderr: &str) -> Option<String> {
    // Look for "auto-detected language: <code>"
    for line in stderr.lines() {
        if let Some(idx) = line.find("auto-detected language:") {
            let rest = &line[idx + "auto-detected language:".len()..];
            let code = rest
                .trim()
                .split_whitespace()
                .next()
                .unwrap_or("")
                .trim_end_matches(|c: char| !c.is_ascii_alphabetic());
            if !code.is_empty() {
                return Some(code.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_language_extracts_code_from_whisper_stderr() {
        let stderr = "
whisper_init_from_file_with_params_no_state: loading model from './models/ggml-tiny.en.bin'
auto-detected language: en (p = 0.99)
done
";
        assert_eq!(parse_language(stderr), Some("en".to_string()));
    }

    #[test]
    fn parse_language_returns_none_when_absent() {
        let stderr = "no language line here\n";
        assert_eq!(parse_language(stderr), None);
    }
}
```

- [ ] **Step 2: Wire `transcribe` into `lib.rs` and `main.rs`**

Add `pub mod transcribe;` to `src/lib.rs`. Add `mod transcribe;` to `src/main.rs`.

- [ ] **Step 3: Run unit tests**

Run: `cargo test transcribe::tests 2>&1 | tail -10`
Expected: 2 passed; 0 failed.

- [ ] **Step 4: Commit**

```bash
cargo fmt
cargo clippy --all-targets --features test-helpers -- -D warnings 2>&1 | tail -5
git add src/transcribe.rs src/main.rs src/lib.rs
git commit -m "Plan A T12: transcribe module wrapping whisper.cpp"
```

---

### Task 13: `ingest` subcommand

**Files:**
- Create: `src/ingest.rs`
- Modify: `src/main.rs`, `src/lib.rs`
- Test: `tests/ingest.rs`

- [ ] **Step 1: Write the failing integration test**

Create `tests/ingest.rs`:

```rust
use std::path::PathBuf;

use tempfile::TempDir;
use uu_tiktok::ingest::{ingest, IngestStats};
use uu_tiktok::state::Store;

fn fixture_inbox() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ddp")
}

#[test]
fn ingest_real_fixture_writes_videos_and_watch_history() {
    let tmp = TempDir::new().unwrap();
    let mut store = Store::open(&tmp.path().join("state.sqlite")).unwrap();

    let stats = ingest(&fixture_inbox(), &mut store).expect("ingest succeeds");

    // The fixture has ~200 watch_history rows but many duplicates; expect
    // a smaller number of unique videos plus all the watch_history rows.
    assert!(
        stats.unique_videos_seen > 50,
        "expected >50 unique videos, got {}",
        stats.unique_videos_seen
    );
    assert!(
        stats.watch_history_rows_inserted > 100,
        "expected >100 watch_history rows, got {}",
        stats.watch_history_rows_inserted
    );
    assert_eq!(stats.short_links_skipped, 0, "fixture has no short links");
    assert_eq!(stats.invalid_urls_skipped, 0, "fixture has no invalid URLs");

    // Spot-check: a known video_id from the fixture file.
    let row = store
        .get_video_for_test("7583050189527682336")
        .unwrap()
        .expect("known video present");
    assert_eq!(row.status, "pending");
    assert!(row.canonical);
}

#[test]
fn ingest_is_idempotent() {
    let tmp = TempDir::new().unwrap();
    let mut store = Store::open(&tmp.path().join("state.sqlite")).unwrap();

    let first = ingest(&fixture_inbox(), &mut store).unwrap();
    let second = ingest(&fixture_inbox(), &mut store).unwrap();

    // Second run sees the same files but inserts no new watch_history rows.
    assert_eq!(first.unique_videos_seen, second.unique_videos_seen);
    assert_eq!(first.watch_history_rows_inserted, second.watch_history_rows_inserted);
    assert_eq!(second.watch_history_duplicates, second.watch_history_rows_inserted);
}
```

- [ ] **Step 2: Implement the ingest module**

Create `src/ingest.rs`:

```rust
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{NaiveDateTime, TimeZone, Utc};
use serde::Deserialize;

use crate::canonical::{canonicalize_url, Canonical};
use crate::state::Store;

#[derive(Debug, Default, Clone)]
pub struct IngestStats {
    pub files_processed: usize,
    pub unique_videos_seen: usize,
    pub watch_history_rows_inserted: usize,
    pub watch_history_duplicates: usize,
    pub short_links_skipped: usize,
    pub invalid_urls_skipped: usize,
    pub date_parse_failures: usize,
}

/// Walk `inbox`, parse each `*.json` file, and upsert resolvable rows into the
/// store. Plan A skips short links with a WARN log; Plan C writes them to a
/// pending_resolutions table.
pub fn ingest(inbox: &Path, store: &mut Store) -> Result<IngestStats> {
    let mut stats = IngestStats::default();

    for path in walk_json_files(inbox)? {
        let respondent_id = parse_respondent_id_from_filename(&path)
            .with_context(|| format!("parsing respondent_id from {}", path.display()))?;

        let raw = std::fs::read(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let sections: Vec<Section> = serde_json::from_slice(&raw)
            .with_context(|| format!("parsing JSON from {}", path.display()))?;

        for section in sections {
            if let Some(rows) = section.tiktok_watch_history {
                for entry in rows {
                    process_watch_entry(store, &respondent_id, &entry, &mut stats)?;
                }
            }
        }

        stats.files_processed += 1;
    }

    Ok(stats)
}

fn process_watch_entry(
    store: &mut Store,
    respondent_id: &str,
    entry: &WatchEntry,
    stats: &mut IngestStats,
) -> Result<()> {
    let canon = canonicalize_url(&entry.link);
    let video_id = match canon {
        Canonical::VideoId(id) => id,
        Canonical::NeedsResolution(_) => {
            tracing::warn!(
                respondent = respondent_id,
                url = entry.link.as_str(),
                "short link skipped (Plan C will resolve)"
            );
            stats.short_links_skipped += 1;
            return Ok(());
        }
        Canonical::Invalid(_) => {
            tracing::warn!(
                respondent = respondent_id,
                url = entry.link.as_str(),
                "invalid URL skipped"
            );
            stats.invalid_urls_skipped += 1;
            return Ok(());
        }
    };

    let watched_at = match parse_watched_at(&entry.date) {
        Some(t) => t,
        None => {
            tracing::warn!(
                respondent = respondent_id,
                date = entry.date.as_str(),
                "could not parse Date; skipping row"
            );
            stats.date_parse_failures += 1;
            return Ok(());
        }
    };

    let was_new = store
        .get_video_for_test(&video_id)
        .ok()
        .flatten()
        .is_none();
    store.upsert_video(&video_id, &entry.link, true)?;
    if was_new {
        stats.unique_videos_seen += 1;
    }

    let inserted = store.upsert_watch_history(respondent_id, &video_id, watched_at, true)?;
    if inserted > 0 {
        stats.watch_history_rows_inserted += 1;
    } else {
        stats.watch_history_duplicates += 1;
    }
    Ok(())
}

fn walk_json_files(inbox: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    walk_recursive(inbox, &mut out)?;
    Ok(out)
}

fn walk_recursive(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)
        .with_context(|| format!("reading directory {}", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            walk_recursive(&path, out)?;
        } else if path.extension().and_then(|s| s.to_str()) == Some("json") {
            out.push(path);
        }
    }
    Ok(())
}

/// Filename convention: `assignment={N}_task={N}_participant={ID}_source=tiktok_key={N}-tiktok.json`
/// Returns the value of `participant=`.
pub fn parse_respondent_id_from_filename(path: &Path) -> Result<String> {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .with_context(|| format!("path {} has no filename", path.display()))?;

    for segment in stem.split('_') {
        if let Some(rest) = segment.strip_prefix("participant=") {
            return Ok(rest.to_string());
        }
    }

    anyhow::bail!(
        "filename {} does not contain a `participant=` segment",
        stem
    )
}

#[derive(Debug, Deserialize)]
struct Section {
    #[serde(default)]
    tiktok_watch_history: Option<Vec<WatchEntry>>,
}

#[derive(Debug, Deserialize)]
struct WatchEntry {
    #[serde(rename = "Date")]
    date: String,
    #[serde(rename = "Link")]
    link: String,
}

fn parse_watched_at(s: &str) -> Option<i64> {
    let naive = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").ok()?;
    Some(Utc.from_utc_datetime(&naive).timestamp())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn parse_respondent_from_realistic_filename() {
        let path = PathBuf::from(
            "/x/assignment=500_task=1221_participant=preview_source=tiktok_key=1776350251592-tiktok.json",
        );
        let id = parse_respondent_id_from_filename(&path).unwrap();
        assert_eq!(id, "preview");
    }

    #[test]
    fn parse_respondent_errors_when_segment_missing() {
        let path = PathBuf::from("/x/no-segments.json");
        assert!(parse_respondent_id_from_filename(&path).is_err());
    }

    #[test]
    fn parse_watched_at_handles_standard_format() {
        assert!(parse_watched_at("2026-02-03 13:20:15").is_some());
    }

    #[test]
    fn parse_watched_at_returns_none_on_garbage() {
        assert!(parse_watched_at("not a date").is_none());
    }
}
```

Register the integration test in `Cargo.toml`:

```toml
[[test]]
name = "ingest"
required-features = ["test-helpers"]
```

- [ ] **Step 3: Wire `ingest` into `lib.rs` and `main.rs`**

Add `pub mod ingest;` to `src/lib.rs`. Add `mod ingest;` to `src/main.rs`.

In `src/main.rs`, replace the `Command::Ingest` arm with:

```rust
        cli::Command::Ingest { dry_run } => {
            let mut store = state::Store::open(&cfg.state_db)
                .context("opening state DB")?;
            if dry_run {
                tracing::info!("dry-run: not yet implemented; running real ingest");
            }
            let stats = ingest::ingest(&cfg.inbox, &mut store)
                .context("ingest failed")?;
            tracing::info!(
                files = stats.files_processed,
                videos = stats.unique_videos_seen,
                history = stats.watch_history_rows_inserted,
                duplicates = stats.watch_history_duplicates,
                short_links_skipped = stats.short_links_skipped,
                invalid_urls_skipped = stats.invalid_urls_skipped,
                "ingest complete"
            );
        }
```

Add `use anyhow::Context;` at the top of `main.rs`.

- [ ] **Step 4: Run all tests**

Run:
```bash
cargo test --features test-helpers --test ingest 2>&1 | tail -10
cargo test --features test-helpers ingest:: 2>&1 | tail -10
```
Expected: integration tests pass (2); unit tests pass (4).

- [ ] **Step 5: Smoke-test from the CLI**

```bash
mkdir -p /tmp/uu-tiktok-test && cd /tmp/uu-tiktok-test
/home/dmm/src/uu-tiktok/target/debug/uu-tiktok \
    --state-db ./state.sqlite \
    --inbox /home/dmm/src/uu-tiktok/tests/fixtures/ddp \
    --transcripts ./transcripts \
    ingest
```

Expected: a tracing line like `ingest complete files=1 videos=N history=M duplicates=K`. `state.sqlite` exists.

- [ ] **Step 6: Commit**

```bash
cd /home/dmm/src/uu-tiktok
cargo fmt
cargo clippy --all-targets --features test-helpers -- -D warnings 2>&1 | tail -5
git add src/ingest.rs src/main.rs src/lib.rs Cargo.toml tests/ingest.rs
git commit -m "Plan A T13: ingest subcommand parses DDP-extracted JSON into state"
```

---

### Task 14: `process` subcommand (serial loop) + e2e smoke test

**Files:**
- Create: `src/pipeline.rs`
- Modify: `src/main.rs`, `src/lib.rs`
- Test: `tests/pipeline_fakes.rs`, `tests/e2e_real_tools.rs`

- [ ] **Step 1: Write the failing FakeFetcher pipeline test**

Create `tests/pipeline_fakes.rs`:

```rust
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use tempfile::TempDir;
use uu_tiktok::fetcher::FakeFetcher;
use uu_tiktok::pipeline::{run_serial, ProcessOptions};
use uu_tiktok::state::Store;

#[tokio::test]
async fn pipeline_processes_one_video_to_succeeded_with_fake_fetcher() {
    let tmp = TempDir::new().unwrap();
    let mut store = Store::open(&tmp.path().join("state.sqlite")).unwrap();
    store
        .upsert_video("7234567890123456789", "fake://url", true)
        .unwrap();

    // Pre-stage a fake WAV file as the FakeFetcher's canned response.
    let fake_wav = tmp.path().join("fake.wav");
    std::fs::write(&fake_wav, b"RIFF....WAVE....").unwrap();
    let map = HashMap::from([("7234567890123456789".to_string(), fake_wav.clone())]);
    let fetcher = FakeFetcher {
        canned: Mutex::new(map),
    };

    // Inject a fake transcribe function via the test transcriber.
    let opts = ProcessOptions {
        worker_id: "test-worker".into(),
        transcripts_root: tmp.path().join("transcripts"),
        max_videos: Some(1),
        // For Plan A we provide a `fake_transcribe` callback in tests.
        // The real `process` subcommand calls the actual transcribe module.
        transcriber: Box::new(|_path| {
            Ok(uu_tiktok::transcribe::TranscribeResult {
                text: "hello fake world".into(),
                language: Some("en".into()),
                duration_s: None,
            })
        }),
    };

    let stats = run_serial(&mut store, &fetcher, opts).await.expect("pipeline");
    assert_eq!(stats.succeeded, 1);
    assert_eq!(stats.failed, 0);

    let row = store
        .get_video_for_test("7234567890123456789")
        .unwrap()
        .unwrap();
    assert_eq!(row.status, "succeeded");

    // Final artifacts present in the sharded directory.
    let txt = tmp
        .path()
        .join("transcripts/89/7234567890123456789.txt");
    assert!(txt.exists(), "transcript file at {}", txt.display());
    let json = tmp
        .path()
        .join("transcripts/89/7234567890123456789.json");
    assert!(json.exists(), "transcript metadata at {}", json.display());
}
```

- [ ] **Step 2: Implement `pipeline::run_serial`**

Create `src/pipeline.rs`:

```rust
use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::Utc;
use serde::Serialize;

use crate::errors::TranscribeError;
use crate::fetcher::{Acquisition, VideoFetcher};
use crate::output::{artifacts, shard};
use crate::state::{Claim, Store, SuccessArtifacts};
use crate::transcribe::TranscribeResult;

/// Test-injectable transcriber. The real `process` subcommand wires this to
/// `transcribe::transcribe` via the `transcribe` module; tests can supply a
/// closure that returns a fixed result.
pub type Transcriber =
    Box<dyn Fn(&std::path::Path) -> Result<TranscribeResult, TranscribeError> + Send>;

pub struct ProcessOptions {
    pub worker_id: String,
    pub transcripts_root: PathBuf,
    pub max_videos: Option<usize>,
    pub transcriber: Transcriber,
}

#[derive(Debug, Default)]
pub struct ProcessStats {
    pub claimed: usize,
    pub succeeded: usize,
    pub failed: usize,
}

#[derive(Serialize)]
struct TranscriptMetadata<'a> {
    video_id: &'a str,
    source_url: &'a str,
    duration_s: Option<f64>,
    language_detected: Option<&'a str>,
    transcribed_at: String,
    fetcher: &'a str,
    transcript_source: &'a str,
}

pub async fn run_serial(
    store: &mut Store,
    fetcher: &dyn VideoFetcher,
    opts: ProcessOptions,
) -> Result<ProcessStats> {
    let mut stats = ProcessStats::default();
    let max = opts.max_videos.unwrap_or(usize::MAX);
    let processed = 0usize;

    while stats.claimed + stats.failed < max {
        let claim = match store.claim_next(&opts.worker_id)? {
            Some(c) => c,
            None => break,
        };
        stats.claimed += 1;

        match process_one(store, fetcher, &claim, &opts).await {
            Ok(()) => stats.succeeded += 1,
            Err(e) => {
                stats.failed += 1;
                tracing::error!(
                    video_id = claim.video_id.as_str(),
                    error = %e,
                    "video failed (Plan A: aborting; Plan B will classify and persist)"
                );
                // Plan A behavior: leave the row in_progress; operator inspects.
                // Plan B will persist failure and continue.
                return Err(e);
            }
        }
    }

    let _ = processed;
    Ok(stats)
}

async fn process_one(
    store: &mut Store,
    fetcher: &dyn VideoFetcher,
    claim: &Claim,
    opts: &ProcessOptions,
) -> Result<()> {
    tracing::info!(
        video_id = claim.video_id.as_str(),
        attempt = claim.attempt_count,
        "claimed"
    );

    let acquisition = fetcher
        .acquire(&claim.video_id, &claim.source_url)
        .await
        .with_context(|| format!("fetching {}", claim.video_id))?;

    let wav_path = match acquisition {
        Acquisition::AudioFile(p) => p,
    };
    tracing::info!(video_id = claim.video_id.as_str(), wav = %wav_path.display(), "audio acquired");

    let transcript = (opts.transcriber)(&wav_path)
        .with_context(|| format!("transcribing {}", claim.video_id))?;
    tracing::info!(
        video_id = claim.video_id.as_str(),
        chars = transcript.text.len(),
        language = transcript.language.as_deref().unwrap_or("?"),
        "transcribed"
    );

    let shard_dir = opts.transcripts_root.join(shard(&claim.video_id));
    std::fs::create_dir_all(&shard_dir)
        .with_context(|| format!("creating shard dir {}", shard_dir.display()))?;

    let txt_path = shard_dir.join(format!("{}.txt", claim.video_id));
    artifacts::atomic_write(&txt_path, transcript.text.as_bytes())
        .with_context(|| format!("writing transcript {}", txt_path.display()))?;

    let metadata = TranscriptMetadata {
        video_id: &claim.video_id,
        source_url: &claim.source_url,
        duration_s: transcript.duration_s,
        language_detected: transcript.language.as_deref(),
        transcribed_at: Utc::now().to_rfc3339(),
        fetcher: "ytdlp",
        transcript_source: "whisper.cpp",
    };
    let json_bytes =
        serde_json::to_vec_pretty(&metadata).context("serializing transcript metadata")?;
    let json_path = shard_dir.join(format!("{}.json", claim.video_id));
    artifacts::atomic_write(&json_path, &json_bytes)?;

    // Cleanup the wav file once durably committed.
    if let Err(e) = std::fs::remove_file(&wav_path) {
        tracing::warn!(path = %wav_path.display(), error = %e, "could not remove wav after success");
    }

    store.mark_succeeded(
        &claim.video_id,
        SuccessArtifacts {
            duration_s: transcript.duration_s,
            language_detected: transcript.language,
            fetcher: "ytdlp",
            transcript_source: "whisper.cpp",
        },
    )?;

    tracing::info!(video_id = claim.video_id.as_str(), "succeeded");
    Ok(())
}
```

- [ ] **Step 3: Wire `pipeline` and the `process` subcommand**

Add `pub mod pipeline;` to `src/lib.rs`. Add `mod pipeline;` to `src/main.rs`.

In `src/main.rs`, replace the `Command::Process` arm:

```rust
        cli::Command::Process { max_videos } => {
            let mut store = state::Store::open(&cfg.state_db)
                .context("opening state DB")?;
            std::fs::create_dir_all(&cfg.transcripts).context("creating transcripts dir")?;
            // Tmp cleanup at startup
            let removed = output::artifacts::cleanup_tmp_files(&cfg.transcripts)?;
            if removed > 0 {
                tracing::info!(removed, "cleaned up leftover .tmp files");
            }

            let work_dir = cfg.transcripts.join(".work");
            std::fs::create_dir_all(&work_dir).context("creating work dir")?;

            let fetcher = fetcher::ytdlp::YtDlpFetcher::new(&work_dir, cfg.ytdlp_timeout);
            let model_path = cfg.whisper_model_path.clone();
            let use_gpu = cfg.whisper_use_gpu;
            let threads = cfg.whisper_threads;
            let timeout = cfg.transcribe_timeout;

            let opts = pipeline::ProcessOptions {
                worker_id: format!(
                    "{}-{}",
                    hostname_or_default(),
                    std::process::id()
                ),
                transcripts_root: cfg.transcripts.clone(),
                max_videos,
                transcriber: Box::new(move |path| {
                    let opts = transcribe::TranscribeOptions {
                        model_path: model_path.clone(),
                        use_gpu,
                        threads,
                        timeout,
                    };
                    // Block on the async transcribe — pipeline is serial in Plan A.
                    tokio::runtime::Handle::current()
                        .block_on(transcribe::transcribe(path, &opts))
                }),
            };

            let stats = pipeline::run_serial(&mut store, &fetcher, opts).await?;
            tracing::info!(
                claimed = stats.claimed,
                succeeded = stats.succeeded,
                failed = stats.failed,
                "process complete"
            );
            if stats.claimed == 0 {
                std::process::exit(3);
            }
        }
```

Add the `hostname_or_default` helper at the bottom of `main.rs`:

```rust
fn hostname_or_default() -> String {
    std::env::var("HOSTNAME").unwrap_or_else(|_| "host".to_string())
}
```

Register `pipeline_fakes` in `Cargo.toml`:

```toml
[[test]]
name = "pipeline_fakes"
required-features = ["test-helpers"]
```

- [ ] **Step 4: Run the FakeFetcher integration test**

Run: `cargo test --features test-helpers --test pipeline_fakes 2>&1 | tail -15`
Expected: 1 passed; 0 failed.

- [ ] **Step 5: Add the real-tools e2e smoke test (`#[ignore]` by default)**

Create `tests/e2e_real_tools.rs`:

```rust
use std::path::PathBuf;
use std::process::Command;

/// Real-network test: requires yt-dlp, ffmpeg, and whisper-cli on PATH, plus
/// the tiny.en model at ./models/ggml-tiny.en.bin. Run manually:
///   cargo test --features test-helpers --test e2e_real_tools -- --ignored --nocapture
#[test]
#[ignore]
fn end_to_end_one_known_url() {
    let tmp = tempfile::TempDir::new().unwrap();
    let inbox = tmp.path().join("inbox");
    std::fs::create_dir_all(&inbox).unwrap();

    // Construct a single-row DDP-extracted JSON file pointing at a known
    // long-lived TikTok URL. Chosen URL must be public and stable; replace if
    // it ever 404s. (Operator-curated; documented in README.)
    let respondent_file = inbox.join(
        "assignment=1_task=1_participant=test_source=tiktok_key=1-tiktok.json",
    );
    let payload = r#"[
        {"tiktok_watch_history": [
            {"Date": "2024-01-01 00:00:00",
             "Link": "https://www.tiktokv.com/share/video/7234567890123456789/"}
        ], "deleted row count": "0"}
    ]"#;
    std::fs::write(&respondent_file, payload).unwrap();

    let bin = PathBuf::from(env!("CARGO_BIN_EXE_uu-tiktok"));

    // Init
    Command::new(&bin)
        .args(["--state-db", tmp.path().join("state.sqlite").to_str().unwrap()])
        .arg("init")
        .status()
        .unwrap();
    // Ingest (note: init isn't implemented yet in T15; this test is wired in T15 and re-run there.)

    // The full e2e validation lands once T15 wires `init`.
}
```

(This test will be revisited in Task 15 once `init` is wired. For now it's a stub that compiles and is `#[ignore]`d.)

- [ ] **Step 6: Run the full test suite to make sure nothing's broken**

Run: `cargo test --features test-helpers 2>&1 | tail -15`
Expected: all non-ignored tests pass.

- [ ] **Step 7: Commit**

```bash
cargo fmt
cargo clippy --all-targets --features test-helpers -- -D warnings 2>&1 | tail -5
git add src/pipeline.rs src/main.rs src/lib.rs Cargo.toml tests/pipeline_fakes.rs tests/e2e_real_tools.rs
git commit -m "Plan A T14: process subcommand serial loop + FakeFetcher pipeline test"
```

---

### Task 15: `init` subcommand and end-to-end smoke

**Files:**
- Modify: `src/main.rs`
- Modify: `tests/e2e_real_tools.rs`

- [ ] **Step 1: Wire the `init` subcommand**

In `src/main.rs`, replace the `Command::Init` arm:

```rust
        cli::Command::Init => {
            let path = &cfg.state_db;
            if path.exists() {
                let store = state::Store::open(path)?;
                if let Some(version) = store.read_meta("schema_version")? {
                    tracing::info!(
                        path = %path.display(),
                        version = version.as_str(),
                        "state.sqlite already initialized; nothing to do"
                    );
                    return Ok(());
                }
            }
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .context("creating state.sqlite parent dir")?;
            }
            let _store = state::Store::open(path)?;
            tracing::info!(path = %path.display(), "state.sqlite initialized");
        }
```

- [ ] **Step 2: Write the CLI smoke test for init**

Append to `tests/cli.rs`:

```rust
#[test]
fn init_creates_state_sqlite() {
    let tmp = tempfile::TempDir::new().unwrap();
    let db = tmp.path().join("state.sqlite");

    Command::cargo_bin("uu-tiktok")
        .unwrap()
        .args(["--state-db", db.to_str().unwrap(), "init"])
        .assert()
        .success();

    assert!(db.exists());
}

#[test]
fn init_is_idempotent() {
    let tmp = tempfile::TempDir::new().unwrap();
    let db = tmp.path().join("state.sqlite");

    for _ in 0..2 {
        Command::cargo_bin("uu-tiktok")
            .unwrap()
            .args(["--state-db", db.to_str().unwrap(), "init"])
            .assert()
            .success();
    }
}
```

- [ ] **Step 3: Run the test**

Run: `cargo test --test cli 2>&1 | tail -10`
Expected: 4 passed total.

- [ ] **Step 4: Complete the e2e real-tools test**

Replace `tests/e2e_real_tools.rs`:

```rust
use std::path::PathBuf;
use std::process::Command;

use tempfile::TempDir;

/// Real-network test: requires yt-dlp, ffmpeg, and whisper-cli on PATH, plus
/// the tiny.en model at ./models/ggml-tiny.en.bin (relative to the project root
/// or override via UU_TIKTOK_MODEL).
///
/// Manual run from the project root:
///   ./scripts/fetch-tiny-model.sh   # one-time: download the tiny.en model
///   cargo build --release
///   cargo test --features test-helpers --test e2e_real_tools -- --ignored --nocapture
#[test]
#[ignore]
fn end_to_end_one_known_url() {
    let tmp = TempDir::new().unwrap();
    let inbox = tmp.path().join("inbox");
    let transcripts = tmp.path().join("transcripts");
    std::fs::create_dir_all(&inbox).unwrap();

    // Construct a single-row donation-extractor JSON file. Replace the URL below
    // with a known long-lived public TikTok video; document the choice in README.
    let respondent_file = inbox.join(
        "assignment=1_task=1_participant=test_source=tiktok_key=1-tiktok.json",
    );
    let url = std::env::var("UU_TIKTOK_E2E_URL").unwrap_or_else(|_| {
        "https://www.tiktokv.com/share/video/7234567890123456789/".into()
    });
    let payload = format!(
        r#"[
            {{"tiktok_watch_history": [
                {{"Date": "2024-01-01 00:00:00", "Link": "{}"}}
            ], "deleted row count": "0"}}
        ]"#,
        url
    );
    std::fs::write(&respondent_file, payload).unwrap();

    let bin = PathBuf::from(env!("CARGO_BIN_EXE_uu-tiktok"));
    let db = tmp.path().join("state.sqlite");

    let run = |args: &[&str]| {
        let status = Command::new(&bin)
            .args(["--state-db", db.to_str().unwrap()])
            .args(["--inbox", inbox.to_str().unwrap()])
            .args(["--transcripts", transcripts.to_str().unwrap()])
            .args(args)
            .status()
            .expect("uu-tiktok runs");
        assert!(status.success(), "uu-tiktok {:?} failed", args);
    };

    run(&["init"]);
    run(&["ingest"]);
    run(&["process", "--max-videos", "1"]);

    // At least one .txt file present somewhere under transcripts/
    let mut found_any = false;
    for entry in walkdir(&transcripts) {
        if entry.extension().and_then(|s| s.to_str()) == Some("txt") {
            let body = std::fs::read_to_string(&entry).unwrap();
            assert!(!body.trim().is_empty(), "transcript at {} is empty", entry.display());
            found_any = true;
        }
    }
    assert!(found_any, "no .txt transcript produced");
}

fn walkdir(root: &std::path::Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if !root.exists() {
        return out;
    }
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.push(p);
            }
        }
    }
    out
}
```

- [ ] **Step 5: Verify the test compiles (do not run yet — requires real tools and model)**

Run: `cargo test --features test-helpers --test e2e_real_tools --no-run 2>&1 | tail -5`
Expected: builds.

- [ ] **Step 6: Document the manual e2e run in `scripts/fetch-tiny-model.sh`**

Create `scripts/fetch-tiny-model.sh`:

```bash
#!/usr/bin/env bash
# Download the whisper.cpp tiny.en model used for dev-profile transcription.
set -euo pipefail

MODEL_DIR="${MODEL_DIR:-./models}"
MODEL_NAME="ggml-tiny.en.bin"
URL="https://huggingface.co/ggerganov/whisper.cpp/resolve/main/${MODEL_NAME}"

mkdir -p "$MODEL_DIR"
if [ -f "$MODEL_DIR/$MODEL_NAME" ]; then
    echo "$MODEL_NAME already present at $MODEL_DIR — skipping"
    exit 0
fi

echo "Downloading $MODEL_NAME (~75MB) to $MODEL_DIR..."
curl -L -o "$MODEL_DIR/$MODEL_NAME" "$URL"
echo "Done. Path to use in UU_TIKTOK_WHISPER_MODEL_PATH or default config: $MODEL_DIR/$MODEL_NAME"
```

Then:
```bash
chmod +x scripts/fetch-tiny-model.sh
```

- [ ] **Step 7: Commit**

```bash
cargo fmt
cargo clippy --all-targets --features test-helpers -- -D warnings 2>&1 | tail -5
git add src/main.rs tests/cli.rs tests/e2e_real_tools.rs scripts/fetch-tiny-model.sh
git commit -m "Plan A T15: init subcommand wired + e2e smoke test scaffolded (ignored by default)"
```

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
