# Plan A — Task 3: Config struct + profile defaults

> Part of [Plan A: walking skeleton](./00-overview.md). See the overview for file structure, dependency set, conventions, and exit criteria.

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
