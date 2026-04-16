# Plan A — Task 2: CLI scaffolding (subcommand enum + global flags)

> Part of [Plan A: walking skeleton](./00-overview.md). See the overview for file structure, dependency set, conventions, and exit criteria.

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
