//! End-to-end real-tools test for Plan B Epic 1. Exercises the embedded
//! whisper-rs pipeline against a real TikTok URL via yt-dlp.
//!
//! Requires (per AD0009, AD0011):
//! - yt-dlp on PATH
//! - ffmpeg on PATH (yt-dlp's postprocessor — 16 kHz mono WAV per AD0014)
//! - ./models/ggml-tiny.en.bin on disk (or UU_TIKTOK_WHISPER_MODEL=PATH)
//! - clang/libclang installed (whisper-rs's whisper-rs-sys binds via bindgen)
//! - network egress to the TikTok CDN
//!
//! Run manually during the SRC A10 bake (Task 13):
//!   cargo test --release --features test-helpers,cuda \
//!     --test e2e_real_tools -- --ignored --nocapture
//!
//! Without --features cuda, falls back to whisper-rs's CPU build
//! (functional but slow; not suitable for the bake's throughput numbers).

use std::path::PathBuf;
use std::process::Command;

use tempfile::TempDir;
use uu_tiktok::output::artifacts::EXPECTED_RAW_SIGNALS_SCHEMA_VERSION;

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

    // .txt sanity check (kept from Plan A): some transcript text exists.
    let mut found_txt = false;
    for entry in walkdir(&transcripts) {
        if entry.extension().and_then(|s| s.to_str()) == Some("txt") {
            let body = std::fs::read_to_string(&entry).unwrap();
            assert!(
                !body.trim().is_empty(),
                "transcript at {} is empty",
                entry.display()
            );
            found_txt = true;
        }
    }
    assert!(found_txt, "no .txt transcript produced");

    // Plan B Epic 1 (T12): .json artifact shape per AD0010.
    //
    // Real audio content is non-deterministic and TikTok content drifts. Assert
    // structural shape per AD0010 (schema_version, ranges, types) rather than
    // specific token text / probabilities / segment counts.
    let json_paths: Vec<_> = walkdir(&transcripts)
        .into_iter()
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("json"))
        .collect();
    assert!(
        !json_paths.is_empty(),
        "no .json transcript artifact produced"
    );

    let json_text = std::fs::read_to_string(&json_paths[0]).expect("read json artifact");
    let parsed: serde_json::Value = serde_json::from_str(&json_text).expect("parse json");

    // Provenance: trait name() values per T11 (no more hardcoded strings).
    assert_eq!(parsed["fetcher"], "ytdlp");
    assert_eq!(parsed["transcript_source"], "whisper-rs");

    // raw_signals: AD0010 contract.
    let rs = &parsed["raw_signals"];
    assert!(!rs.is_null(), "raw_signals must be present");
    assert_eq!(rs["schema_version"], EXPECTED_RAW_SIGNALS_SCHEMA_VERSION);
    assert!(
        !rs["language"].as_str().unwrap_or("").is_empty(),
        "language must be a non-empty ISO code"
    );

    let segments = rs["segments"].as_array().expect("segments array");
    assert!(
        !segments.is_empty(),
        "real audio should produce at least one segment"
    );
    for (i, seg) in segments.iter().enumerate() {
        let no_speech = seg["no_speech_prob"]
            .as_f64()
            .expect("no_speech_prob is number");
        assert!(
            (0.0..=1.0).contains(&no_speech),
            "segment {i} no_speech_prob out of range: {no_speech}"
        );
        let tokens = seg["tokens"].as_array().expect("tokens array");
        for (j, tok) in tokens.iter().enumerate() {
            let id = tok["id"].as_i64().expect("token id is integer");
            assert!(
                id >= 0,
                "segment {i} token {j} id should be non-negative, got {id}"
            );
            assert!(
                tok["text"].is_string(),
                "segment {i} token {j} text must be a string"
            );
            let p = tok["p"].as_f64().expect("token p is number");
            assert!(
                (0.0..=1.0).contains(&p),
                "segment {i} token {j} p out of range: {p}"
            );
            let plog = tok["plog"].as_f64().expect("token plog is number");
            assert!(
                plog <= 0.0001,
                "segment {i} token {j} plog should be non-positive log-prob, got {plog}"
            );
        }
    }
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
