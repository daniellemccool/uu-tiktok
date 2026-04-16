# Plan A — Task 8: `output::shard_path` + atomic write helper

> Part of [Plan A: walking skeleton](./00-overview.md). See the overview for file structure, dependency set, conventions, and exit criteria.

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
