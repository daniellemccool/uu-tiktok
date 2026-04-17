use std::path::{Path, PathBuf};
use std::time::Duration;

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
        crate::process::RunError::Timeout { duration, .. } => TranscribeError::Timeout { duration },
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
