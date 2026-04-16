use std::path::{Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;

use crate::errors::FetchError;
use crate::fetcher::{Acquisition, VideoFetcher};
use crate::process::{run, CommandSpec};

// T14 (process serial loop) is the first bin consumer.
#[allow(dead_code)]
pub struct YtDlpFetcher {
    /// Directory under which yt-dlp writes per-video subdirectories. Caller
    /// supplies a writable path under `transcripts_root`.
    pub work_dir: PathBuf,
    pub timeout: Duration,
}

impl YtDlpFetcher {
    // T14 (process serial loop) is the first bin consumer.
    #[allow(dead_code)]
    pub fn new(work_dir: impl AsRef<Path>, timeout: Duration) -> Self {
        Self {
            work_dir: work_dir.as_ref().to_path_buf(),
            timeout,
        }
    }
}

#[async_trait]
impl VideoFetcher for YtDlpFetcher {
    async fn acquire(&self, video_id: &str, source_url: &str) -> Result<Acquisition, FetchError> {
        // Per-video tmp dir keeps yt-dlp's intermediate files contained.
        let video_dir = self.work_dir.join(format!("ytdlp-{}", video_id));
        std::fs::create_dir_all(&video_dir).map_err(|e| {
            FetchError::NetworkError(format!(
                "creating yt-dlp work dir {}: {}",
                video_dir.display(),
                e
            ))
        })?;

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
