# Plan A — Task 9: `Store` ingest methods (upsert)

> Part of [Plan A: walking skeleton](./00-overview.md). See the overview for file structure, dependency set, conventions, and exit criteria.

**Files:**
- Modify: `src/state/mod.rs`
- Test: `tests/state_ingest.rs`

- [ ] **Step 1: Write the failing tests**

Create `tests/state_ingest.rs`:

```rust
use tempfile::TempDir;
use uu_tiktok::state::Store;

fn fresh_store() -> (TempDir, Store) {
    let tmp = TempDir::new().unwrap();
    let store = Store::open(&tmp.path().join("state.sqlite")).expect("open");
    (tmp, store)
}

#[test]
fn upsert_video_inserts_new_row_with_pending_status() {
    let (_tmp, mut store) = fresh_store();
    store
        .upsert_video(
            "7234567890123456789",
            "https://www.tiktokv.com/share/video/7234567890123456789/",
            true,
        )
        .expect("upsert");

    let row = store.get_video_for_test("7234567890123456789").unwrap().expect("row exists");
    assert_eq!(row.status, "pending");
    assert_eq!(row.canonical, true);
    assert!(row.first_seen_at > 0);
    assert_eq!(row.attempt_count, 0);
}

#[test]
fn upsert_video_is_idempotent_first_seen_at_unchanged() {
    let (_tmp, mut store) = fresh_store();
    store
        .upsert_video("7234567890123456789", "url-A", true)
        .unwrap();
    let first = store
        .get_video_for_test("7234567890123456789")
        .unwrap()
        .unwrap();

    std::thread::sleep(std::time::Duration::from_millis(1100));

    store
        .upsert_video("7234567890123456789", "url-B", true)
        .unwrap();
    let second = store
        .get_video_for_test("7234567890123456789")
        .unwrap()
        .unwrap();

    assert_eq!(
        first.first_seen_at, second.first_seen_at,
        "first_seen_at must NOT change on re-upsert"
    );
    assert_eq!(
        first.source_url, second.source_url,
        "source_url must NOT change on re-upsert (we keep the first one)"
    );
}

#[test]
fn upsert_watch_history_inserts() {
    let (_tmp, mut store) = fresh_store();
    store
        .upsert_video("7234567890123456789", "https://example/video/7234567890123456789", true)
        .unwrap();
    let inserted = store
        .upsert_watch_history("respondent-1", "7234567890123456789", 1_700_000_000, true)
        .unwrap();
    assert_eq!(inserted, 1);
}

#[test]
fn upsert_watch_history_is_idempotent_on_duplicate_pk() {
    let (_tmp, mut store) = fresh_store();
    store
        .upsert_video("7234567890123456789", "url", true)
        .unwrap();
    let first = store
        .upsert_watch_history("r1", "7234567890123456789", 1_700_000_000, true)
        .unwrap();
    let second = store
        .upsert_watch_history("r1", "7234567890123456789", 1_700_000_000, false)
        .unwrap();
    assert_eq!(first, 1);
    assert_eq!(second, 0, "duplicate PK insert returns 0 rows changed");
}

#[test]
fn upsert_watch_history_fk_violation_when_video_missing() {
    let (_tmp, mut store) = fresh_store();
    let result = store.upsert_watch_history("r1", "7234567890123456789", 1_700_000_000, true);
    assert!(result.is_err(), "FK should fail when video row absent");
}
```

- [ ] **Step 2: Implement the methods on `Store`**

Append to `src/state/mod.rs` (inside `impl Store`):

```rust
    pub fn upsert_video(
        &mut self,
        video_id: &str,
        source_url: &str,
        canonical: bool,
    ) -> Result<()> {
        let now = unix_now();
        self.conn
            .execute(
                "INSERT OR IGNORE INTO videos
                 (video_id, source_url, canonical, status,
                  first_seen_at, updated_at)
                 VALUES (?1, ?2, ?3, 'pending', ?4, ?4)",
                params![video_id, source_url, canonical as i64, now],
            )
            .with_context(|| format!("upserting video {}", video_id))?;
        Ok(())
    }

    pub fn upsert_watch_history(
        &mut self,
        respondent_id: &str,
        video_id: &str,
        watched_at: i64,
        in_window: bool,
    ) -> Result<usize> {
        let changed = self
            .conn
            .execute(
                "INSERT OR IGNORE INTO watch_history
                 (respondent_id, video_id, watched_at, in_window)
                 VALUES (?1, ?2, ?3, ?4)",
                params![respondent_id, video_id, watched_at, in_window as i64],
            )
            .with_context(|| {
                format!(
                    "upserting watch_history (respondent={}, video={}, watched_at={})",
                    respondent_id, video_id, watched_at
                )
            })?;
        Ok(changed)
    }
```

Add this helper near the top of the file (after the imports):

```rust
fn unix_now() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Test-only helper for verifying row state. Not part of the public API; gated
/// to test compilation only.
#[cfg(any(test, feature = "test-helpers"))]
#[derive(Debug, Clone)]
pub struct VideoRow {
    pub video_id: String,
    pub status: String,
    pub canonical: bool,
    pub source_url: String,
    pub first_seen_at: i64,
    pub attempt_count: i64,
}

impl Store {
    #[cfg(any(test, feature = "test-helpers"))]
    pub fn get_video_for_test(&self, video_id: &str) -> Result<Option<VideoRow>> {
        use rusqlite::OptionalExtension;
        let row = self
            .conn
            .query_row(
                "SELECT video_id, status, canonical, source_url, first_seen_at, attempt_count
                 FROM videos WHERE video_id = ?1",
                params![video_id],
                |r| {
                    Ok(VideoRow {
                        video_id: r.get(0)?,
                        status: r.get(1)?,
                        canonical: r.get::<_, i64>(2)? != 0,
                        source_url: r.get(3)?,
                        first_seen_at: r.get(4)?,
                        attempt_count: r.get(5)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }
}
```

The `#[cfg(any(test, feature = "test-helpers"))]` makes `get_video_for_test` available for both unit tests in this crate AND integration tests in `tests/`. Add the feature gate to `Cargo.toml` so integration tests can opt in:

```toml
[features]
test-helpers = []

[[test]]
name = "state_ingest"
required-features = ["test-helpers"]
```

(Append the `[features]` block at the end of `Cargo.toml`. Add the `[[test]]` block too.)

- [ ] **Step 3: Run tests to confirm pass**

Run: `cargo test --test state_ingest --features test-helpers 2>&1 | tail -15`
Expected: 5 passed; 0 failed.

- [ ] **Step 4: Commit**

```bash
cargo fmt
cargo clippy --all-targets --features test-helpers -- -D warnings 2>&1 | tail -5
git add src/state/mod.rs Cargo.toml Cargo.lock tests/state_ingest.rs
git commit -m "Plan A T9: Store::upsert_video and upsert_watch_history (INSERT OR IGNORE)"
```
