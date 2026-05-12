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

// ============================================================================
// Plan B Epic 1: TranscribeOutput types
// ============================================================================
//
// Pass-through raw signals from whisper.cpp's C API via the whisper-rs binding.
// See AD0010 (raw_signals schema), AD0016 (worker-thread invariants).
//
// These types are OWNED data: no references, no whisper-rs handles. They cross
// the worker-thread boundary safely (AD0016 #1: owned data only).

use serde::{Deserialize, Serialize};

/// Owned output from a single whisper inference. Crosses the worker-thread
/// boundary (AD0016). T10's artifact writer maps these fields across the
/// artifact JSON: `text` and `model_id` land at the top level (alongside
/// Plan A's existing metadata), while `language`, `lang_probs`, and `segments`
/// are placed inside the `raw_signals` sub-object (AD0010). This struct is
/// the worker-return type, not a 1:1 mirror of `raw_signals`.
// AD0002: fields unused until T9+; suppress dead-code lint until then.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TranscribeOutput {
    /// Concatenated text of all segments.
    pub text: String,
    /// Detected language as a single ISO code, e.g. "en" or "nl".
    /// From whisper_full_lang_id() (free per inference).
    pub language: String,
    /// Per-language probability vector, ONLY when PerCallConfig::compute_lang_probs is true.
    /// Costs one extra encoder pass per video (sharp-edges.md:13).
    pub lang_probs: Option<Vec<(String, f32)>>,
    /// Per-segment raw confidence signals.
    pub segments: Vec<SegmentRaw>,
    /// Model identifier, e.g. "ggml-large-v3-turbo-q5_0.bin".
    /// Already captured by Plan A's metadata.
    pub model_id: String,
}

/// Per-segment raw confidence signals from whisper.cpp.
// AD0002: unused until T9+.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SegmentRaw {
    /// whisper_full_get_segment_no_speech_prob(state, i)
    pub no_speech_prob: f32,
    /// Per-token confidence signals for this segment.
    pub tokens: Vec<TokenRaw>,
}

/// Per-token confidence signals from whisper.cpp.
// AD0002: unused until T9+.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TokenRaw {
    /// whisper_full_get_token_p(state, i, j) — token probability in [0.0, 1.0]
    pub p: f32,
    /// Token log-probability (TokenData::plog from whisper-rs)
    pub plog: f32,
}

#[cfg(test)]
mod plan_b_tests {
    use super::*;

    fn sample_output() -> TranscribeOutput {
        TranscribeOutput {
            text: "Hello world".to_string(),
            language: "en".to_string(),
            lang_probs: None,
            segments: vec![SegmentRaw {
                no_speech_prob: 0.02,
                tokens: vec![
                    TokenRaw {
                        p: 0.99,
                        plog: -0.01,
                    },
                    TokenRaw {
                        p: 0.95,
                        plog: -0.05,
                    },
                ],
            }],
            model_id: "ggml-tiny.en.bin".to_string(),
        }
    }

    #[test]
    fn transcribe_output_round_trip() {
        let before = sample_output();
        let json = serde_json::to_string(&before).expect("serialize");
        let after: TranscribeOutput = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(before, after);
    }

    #[test]
    fn lang_probs_none_serializes_as_null() {
        let output = sample_output();
        let json = serde_json::to_value(&output).expect("serialize");
        assert_eq!(json["lang_probs"], serde_json::Value::Null);
    }

    #[test]
    fn lang_probs_some_serializes_as_array_of_pairs() {
        let mut output = sample_output();
        output.lang_probs = Some(vec![("en".to_string(), 0.93), ("nl".to_string(), 0.05)]);
        let json = serde_json::to_value(&output).expect("serialize");
        let arr = json["lang_probs"].as_array().expect("array");
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0][0], "en");
        assert!((arr[0][1].as_f64().unwrap() - 0.93).abs() < 1e-6);
    }

    #[test]
    fn empty_segments_round_trip() {
        let mut output = sample_output();
        output.segments = vec![];
        let json = serde_json::to_string(&output).expect("serialize");
        let after: TranscribeOutput = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(output, after);
    }
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
