use std::collections::HashMap;
use std::sync::Mutex;

use tempfile::TempDir;
use uu_tiktok::fetcher::FakeFetcher;
use uu_tiktok::pipeline::{run_serial, ProcessOptions};
use uu_tiktok::state::Store;

#[tokio::test]
async fn pipeline_processes_one_video_to_succeeded_with_fake_fetcher() {
    let tmp = TempDir::new().unwrap();
    let mut store = Store::open(&tmp.path().join("state.sqlite")).unwrap();
    store
        .upsert_video("7234567890123456789", "fake://url", true)
        .unwrap();

    // Pre-stage a fake WAV file as the FakeFetcher's canned response.
    let fake_wav = tmp.path().join("fake.wav");
    std::fs::write(&fake_wav, b"RIFF....WAVE....").unwrap();
    let map = HashMap::from([("7234567890123456789".to_string(), fake_wav.clone())]);
    let fetcher = FakeFetcher {
        canned: Mutex::new(map),
    };

    // Inject a fake transcribe function via the test transcriber.
    let opts = ProcessOptions {
        worker_id: "test-worker".into(),
        transcripts_root: tmp.path().join("transcripts"),
        max_videos: Some(1),
        transcript_model: "ggml-test.bin".into(),
        // For Plan A we provide a `fake_transcribe` callback in tests.
        // The real `process` subcommand calls the actual transcribe module.
        transcriber: Box::new(|_path| {
            Box::pin(async {
                Ok(uu_tiktok::transcribe::TranscribeResult {
                    text: "hello fake world".into(),
                    language: Some("en".into()),
                    duration_s: None,
                })
            })
        }),
    };

    let stats = run_serial(&mut store, &fetcher, opts)
        .await
        .expect("pipeline");
    assert_eq!(stats.succeeded, 1);
    assert_eq!(stats.failed, 0);

    let row = store
        .get_video_for_test("7234567890123456789")
        .unwrap()
        .unwrap();
    assert_eq!(row.status, "succeeded");

    // Final artifacts present in the sharded directory.
    let txt = tmp.path().join("transcripts/89/7234567890123456789.txt");
    assert!(txt.exists(), "transcript file at {}", txt.display());
    let json = tmp.path().join("transcripts/89/7234567890123456789.json");
    assert!(json.exists(), "transcript metadata at {}", json.display());
    let json_value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&json).unwrap()).unwrap();
    assert_eq!(
        json_value["model"], "ggml-test.bin",
        "model field should reflect the configured model (T10 rename: \
         transcript_model → model on the lifted TranscriptMetadata struct)"
    );
}
