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
    async fn acquire(&self, video_id: &str, source_url: &str) -> Result<Acquisition, FetchError>;
}

// Cfg-gated test fixture per AD0005; consumed by T14's pipeline test
// (tests/process_pipeline.rs or similar). Bin compilation also gets the
// feature when --features test-helpers is enabled.
#[cfg(any(test, feature = "test-helpers"))]
#[allow(dead_code)]
pub struct FakeFetcher {
    pub canned: std::sync::Mutex<std::collections::HashMap<String, std::path::PathBuf>>,
}

#[cfg(any(test, feature = "test-helpers"))]
#[async_trait]
impl VideoFetcher for FakeFetcher {
    async fn acquire(&self, video_id: &str, _source_url: &str) -> Result<Acquisition, FetchError> {
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
