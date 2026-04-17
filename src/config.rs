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
