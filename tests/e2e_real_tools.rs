use std::path::PathBuf;
use std::process::Command;

use tempfile::TempDir;

/// Real-network test: requires yt-dlp, ffmpeg, and whisper-cli on PATH, plus
/// the tiny.en model at ./models/ggml-tiny.en.bin (relative to the project
/// root). The model path is currently a Plan A dev-profile constant; override
/// support is FOLLOWUPS-tracked.
///
/// Manual run from the project root:
///   ./scripts/fetch-tiny-model.sh   # one-time: download the tiny.en model
///   cargo build --release
///   cargo test --features test-helpers --test e2e_real_tools -- --ignored --nocapture
#[test]
#[ignore]
fn end_to_end_one_known_url() {
    let tmp = TempDir::new().unwrap();
    let inbox = tmp.path().join("inbox");
    let transcripts = tmp.path().join("transcripts");
    std::fs::create_dir_all(&inbox).unwrap();

    // Construct a single-row donation-extractor JSON file. Replace the URL below
    // with a known long-lived public TikTok video; document the choice in README.
    let respondent_file =
        inbox.join("assignment=1_task=1_participant=test_source=tiktok_key=1-tiktok.json");
    let url = std::env::var("UU_TIKTOK_E2E_URL")
        .unwrap_or_else(|_| "https://www.tiktokv.com/share/video/7234567890123456789/".into());
    let payload = format!(
        r#"[
            {{"tiktok_watch_history": [
                {{"Date": "2024-01-01 00:00:00", "Link": "{}"}}
            ], "deleted row count": "0"}}
        ]"#,
        url
    );
    std::fs::write(&respondent_file, payload).unwrap();

    let bin = PathBuf::from(env!("CARGO_BIN_EXE_uu-tiktok"));
    let db = tmp.path().join("state.sqlite");

    let run = |args: &[&str]| {
        let status = Command::new(&bin)
            .args(["--state-db", db.to_str().unwrap()])
            .args(["--inbox", inbox.to_str().unwrap()])
            .args(["--transcripts", transcripts.to_str().unwrap()])
            .args(args)
            .status()
            .expect("uu-tiktok runs");
        assert!(status.success(), "uu-tiktok {:?} failed", args);
    };

    run(&["init"]);
    run(&["ingest"]);
    run(&["process", "--max-videos", "1"]);

    // At least one .txt file present somewhere under transcripts/
    let mut found_any = false;
    for entry in walkdir(&transcripts) {
        if entry.extension().and_then(|s| s.to_str()) == Some("txt") {
            let body = std::fs::read_to_string(&entry).unwrap();
            assert!(
                !body.trim().is_empty(),
                "transcript at {} is empty",
                entry.display()
            );
            found_any = true;
        }
    }
    assert!(found_any, "no .txt transcript produced");
}

fn walkdir(root: &std::path::Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if !root.exists() {
        return out;
    }
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.push(p);
            }
        }
    }
    out
}
