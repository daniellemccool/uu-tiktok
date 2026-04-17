use std::path::PathBuf;
use std::process::Command;

/// Real-network test: requires yt-dlp, ffmpeg, and whisper-cli on PATH, plus
/// the tiny.en model at ./models/ggml-tiny.en.bin. Run manually:
///   cargo test --features test-helpers --test e2e_real_tools -- --ignored --nocapture
#[test]
#[ignore]
fn end_to_end_one_known_url() {
    let tmp = tempfile::TempDir::new().unwrap();
    let inbox = tmp.path().join("inbox");
    std::fs::create_dir_all(&inbox).unwrap();

    // Construct a single-row DDP-extracted JSON file pointing at a known
    // long-lived TikTok URL. Chosen URL must be public and stable; replace if
    // it ever 404s. (Operator-curated; documented in README.)
    let respondent_file =
        inbox.join("assignment=1_task=1_participant=test_source=tiktok_key=1-tiktok.json");
    let payload = r#"[
        {"tiktok_watch_history": [
            {"Date": "2024-01-01 00:00:00",
             "Link": "https://www.tiktokv.com/share/video/7234567890123456789/"}
        ], "deleted row count": "0"}
    ]"#;
    std::fs::write(&respondent_file, payload).unwrap();

    let bin = PathBuf::from(env!("CARGO_BIN_EXE_uu-tiktok"));

    // Init
    Command::new(&bin)
        .args([
            "--state-db",
            tmp.path().join("state.sqlite").to_str().unwrap(),
        ])
        .arg("init")
        .status()
        .unwrap();
    // Ingest (note: init isn't implemented yet in T15; this test is wired in T15 and re-run there.)

    // The full e2e validation lands once T15 wires `init`.
}
