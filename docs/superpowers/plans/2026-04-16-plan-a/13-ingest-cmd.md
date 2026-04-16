# Plan A — Task 13: `ingest` subcommand

> Part of [Plan A: walking skeleton](./00-overview.md). See the overview for file structure, dependency set, conventions, and exit criteria.

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
