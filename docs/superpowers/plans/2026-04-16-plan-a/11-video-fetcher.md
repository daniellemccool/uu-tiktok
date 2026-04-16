# Plan A — Task 11: `VideoFetcher` trait + `YtDlpFetcher`

> Part of [Plan A: walking skeleton](./00-overview.md). See the overview for file structure, dependency set, conventions, and exit criteria.

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
