# Plan A — Task 15: `init` subcommand and end-to-end smoke

> Part of [Plan A: walking skeleton](./00-overview.md). See the overview for file structure, dependency set, conventions, and exit criteria.

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
