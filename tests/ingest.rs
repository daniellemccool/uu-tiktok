use std::path::PathBuf;

use tempfile::TempDir;
use uu_tiktok::ingest::{ingest, IngestStats};
use uu_tiktok::state::Store;

fn fixture_inbox() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ddp")
}

#[test]
fn ingest_real_fixture_writes_videos_and_watch_history() {
    let tmp = TempDir::new().unwrap();
    let mut store = Store::open(&tmp.path().join("state.sqlite")).unwrap();

    let stats: IngestStats = ingest(&fixture_inbox(), &mut store).expect("ingest succeeds");

    // The fixture has ~200 watch_history rows but many duplicates; expect
    // a smaller number of unique videos plus all the watch_history rows.
    assert!(
        stats.unique_videos_seen > 50,
        "expected >50 unique videos, got {}",
        stats.unique_videos_seen
    );
    assert!(
        stats.watch_history_rows_processed > 100,
        "expected >100 watch_history rows, got {}",
        stats.watch_history_rows_processed
    );
    assert_eq!(stats.short_links_skipped, 0, "fixture has no short links");
    assert_eq!(stats.invalid_urls_skipped, 0, "fixture has no invalid URLs");

    // Spot-check: a known video_id from the fixture file.
    let row = store
        .get_video_for_test("7583050189527682336")
        .unwrap()
        .expect("known video present");
    assert_eq!(row.status, "pending");
    assert!(row.canonical);
}

#[test]
fn ingest_is_idempotent() {
    let tmp = TempDir::new().unwrap();
    let mut store = Store::open(&tmp.path().join("state.sqlite")).unwrap();

    let first = ingest(&fixture_inbox(), &mut store).unwrap();
    let second = ingest(&fixture_inbox(), &mut store).unwrap();

    // Input-side counts: both runs see the same unique videos and the same
    // total processed rows. On the second run, EVERY processed row is a
    // duplicate of an existing watch_history entry.
    assert_eq!(first.unique_videos_seen, second.unique_videos_seen);
    assert_eq!(
        first.watch_history_rows_processed,
        second.watch_history_rows_processed
    );
    assert_eq!(
        second.watch_history_duplicates,
        second.watch_history_rows_processed
    );
}
