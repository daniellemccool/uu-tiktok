use std::fs::File;
use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};

/// Atomic write for one file: write to `{path}.tmp`, fsync, rename to `{path}`,
/// fsync the parent directory. Caller is responsible for parent existence.
// consumed by T11 (video-fetcher) and T14 (process-cmd)
#[allow(dead_code)]
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
        let mut f = File::create(&tmp_path)
            .with_context(|| format!("creating tmp file {}", tmp_path.display()))?;
        f.write_all(contents)
            .with_context(|| format!("writing tmp file {}", tmp_path.display()))?;
        f.sync_all()
            .with_context(|| format!("fsyncing tmp file {}", tmp_path.display()))?;
    }

    std::fs::rename(&tmp_path, path)
        .with_context(|| format!("renaming {} to {}", tmp_path.display(), path.display()))?;

    let dir = File::open(parent)
        .with_context(|| format!("opening parent dir {} for fsync", parent.display()))?;
    dir.sync_all()
        .with_context(|| format!("fsyncing parent dir {}", parent.display()))?;

    Ok(())
}

/// Sweep all `*.tmp` files under the transcripts root. Called at process
/// startup so leftover tmp files from crashed runs don't accumulate.
// consumed by T14/T15 (process-cmd, init-cmd)
#[allow(dead_code)]
pub fn cleanup_tmp_files(transcripts_root: &Path) -> Result<usize> {
    if !transcripts_root.exists() {
        return Ok(0);
    }
    let mut removed = 0;
    for entry in std::fs::read_dir(transcripts_root)
        .with_context(|| format!("reading transcripts root {}", transcripts_root.display()))?
    {
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
