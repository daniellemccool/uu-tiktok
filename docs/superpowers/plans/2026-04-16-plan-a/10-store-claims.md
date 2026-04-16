# Plan A — Task 10: `Store::claim_next` + `mark_succeeded` (transactional)

> Part of [Plan A: walking skeleton](./00-overview.md). See the overview for file structure, dependency set, conventions, and exit criteria.

**Files:**
- Modify: `src/state/mod.rs`
- Test: `tests/state_claims.rs`

- [ ] **Step 1: Write the failing tests**

Create `tests/state_claims.rs`:

```rust
use tempfile::TempDir;
use uu_tiktok::state::{Claim, Store, SuccessArtifacts};

fn fresh_store_with(videos: &[(&str, &str)]) -> (TempDir, Store) {
    let tmp = TempDir::new().unwrap();
    let mut store = Store::open(&tmp.path().join("state.sqlite")).unwrap();
    for (id, url) in videos {
        store.upsert_video(id, url, true).unwrap();
    }
    (tmp, store)
}

#[test]
fn claim_next_returns_none_on_empty_db() {
    let (_tmp, mut store) = fresh_store_with(&[]);
    let claim = store.claim_next("worker-1").unwrap();
    assert!(claim.is_none());
}

#[test]
fn claim_next_returns_pending_video_and_marks_in_progress() {
    let (_tmp, mut store) = fresh_store_with(&[("7234567890123456789", "url")]);

    let claim = store.claim_next("worker-1").unwrap().expect("claim");
    assert_eq!(claim.video_id, "7234567890123456789");
    assert_eq!(claim.source_url, "url");

    let row = store.get_video_for_test("7234567890123456789").unwrap().unwrap();
    assert_eq!(row.status, "in_progress");
    assert_eq!(row.attempt_count, 1, "attempt_count incremented on claim");
}

#[test]
fn claim_next_orders_by_first_seen_at() {
    let (_tmp, mut store) = fresh_store_with(&[]);
    store.upsert_video("7234567890123456789", "first", true).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    store.upsert_video("7234567890123456788", "second", true).unwrap();

    let first_claim = store.claim_next("w").unwrap().unwrap();
    assert_eq!(first_claim.video_id, "7234567890123456789");
}

#[test]
fn mark_succeeded_writes_status_and_event_in_one_transaction() {
    let (_tmp, mut store) = fresh_store_with(&[("7234567890123456789", "url")]);
    let claim = store.claim_next("w").unwrap().unwrap();
    assert_eq!(claim.video_id, "7234567890123456789");

    let artifacts = SuccessArtifacts {
        duration_s: Some(23.4),
        language_detected: Some("en".into()),
        fetcher: "ytdlp",
        transcript_source: "whisper.cpp",
    };
    store.mark_succeeded(&claim.video_id, artifacts).unwrap();

    let row = store.get_video_for_test(&claim.video_id).unwrap().unwrap();
    assert_eq!(row.status, "succeeded");
    let events = store.get_events_for_test(&claim.video_id).unwrap();
    let kinds: Vec<&str> = events.iter().map(|e| e.event_type.as_str()).collect();
    assert!(kinds.contains(&"claimed"), "claimed event recorded");
    assert!(kinds.contains(&"succeeded"), "succeeded event recorded");
}

#[test]
fn concurrent_claim_serializes_via_begin_immediate() {
    // Two distinct connections to the same DB. claim_next must atomically
    // pick a row so only one connection succeeds for a given pending row.
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("state.sqlite");

    let mut store_a = Store::open(&path).unwrap();
    let mut store_b = Store::open(&path).unwrap();

    store_a
        .upsert_video("7234567890123456789", "url", true)
        .unwrap();

    let a = store_a.claim_next("worker-a").unwrap();
    let b = store_b.claim_next("worker-b").unwrap();

    let claimed: Vec<&Claim> = [a.as_ref(), b.as_ref()].into_iter().flatten().collect();
    assert_eq!(claimed.len(), 1, "exactly one connection wins the row");
}
```

- [ ] **Step 2: Implement `claim_next` and `mark_succeeded`**

Append to `src/state/mod.rs`:

```rust
#[derive(Debug, Clone)]
pub struct Claim {
    pub video_id: String,
    pub source_url: String,
    pub attempt_count: i64,
}

#[derive(Debug, Clone)]
pub struct SuccessArtifacts {
    pub duration_s: Option<f64>,
    pub language_detected: Option<String>,
    pub fetcher: &'static str,
    pub transcript_source: &'static str,
}

impl Store {
    pub fn claim_next(&mut self, worker_id: &str) -> Result<Option<Claim>> {
        let now = unix_now();
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .context("begin immediate for claim_next")?;

        let candidate: Option<(String, String, i64)> = tx
            .query_row(
                "SELECT video_id, source_url, attempt_count
                 FROM videos
                 WHERE status = 'pending'
                 ORDER BY first_seen_at ASC, video_id ASC
                 LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;

        let Some((video_id, source_url, prev_attempts)) = candidate else {
            tx.commit()?;
            return Ok(None);
        };

        let new_attempts = prev_attempts + 1;
        tx.execute(
            "UPDATE videos
             SET status = 'in_progress',
                 claimed_by = ?2,
                 claimed_at = ?3,
                 attempt_count = ?4,
                 updated_at = ?3
             WHERE video_id = ?1",
            params![video_id, worker_id, now, new_attempts],
        )?;

        tx.execute(
            "INSERT INTO video_events (video_id, at, event_type, worker_id, detail_json)
             VALUES (?1, ?2, 'claimed', ?3, NULL)",
            params![video_id, now, worker_id],
        )?;

        tx.commit().context("commit claim transaction")?;

        Ok(Some(Claim {
            video_id,
            source_url,
            attempt_count: new_attempts,
        }))
    }

    pub fn mark_succeeded(
        &mut self,
        video_id: &str,
        artifacts: SuccessArtifacts,
    ) -> Result<()> {
        let now = unix_now();
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .context("begin immediate for mark_succeeded")?;

        tx.execute(
            "UPDATE videos
             SET status = 'succeeded',
                 succeeded_at = ?2,
                 duration_s = ?3,
                 language_detected = ?4,
                 fetcher = ?5,
                 transcript_source = ?6,
                 updated_at = ?2
             WHERE video_id = ?1",
            params![
                video_id,
                now,
                artifacts.duration_s,
                artifacts.language_detected,
                artifacts.fetcher,
                artifacts.transcript_source,
            ],
        )
        .with_context(|| format!("update videos for succeeded {}", video_id))?;

        tx.execute(
            "INSERT INTO video_events (video_id, at, event_type, worker_id, detail_json)
             VALUES (?1, ?2, 'succeeded', NULL, NULL)",
            params![video_id, now],
        )?;

        tx.commit().context("commit mark_succeeded")?;
        Ok(())
    }
}

#[cfg(any(test, feature = "test-helpers"))]
#[derive(Debug, Clone)]
pub struct EventRow {
    pub event_type: String,
    pub worker_id: Option<String>,
}

#[cfg(any(test, feature = "test-helpers"))]
impl Store {
    pub fn get_events_for_test(&self, video_id: &str) -> Result<Vec<EventRow>> {
        let mut stmt = self
            .conn
            .prepare("SELECT event_type, worker_id FROM video_events WHERE video_id = ?1 ORDER BY id")?;
        let rows: Vec<EventRow> = stmt
            .query_map(params![video_id], |r| {
                Ok(EventRow {
                    event_type: r.get(0)?,
                    worker_id: r.get(1)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        Ok(rows)
    }
}
```

Register the new test file in `Cargo.toml`:

```toml
[[test]]
name = "state_claims"
required-features = ["test-helpers"]
```

- [ ] **Step 3: Run tests to confirm pass**

Run: `cargo test --features test-helpers --test state_claims 2>&1 | tail -15`
Expected: 5 passed; 0 failed.

- [ ] **Step 4: Commit**

```bash
cargo fmt
cargo clippy --all-targets --features test-helpers -- -D warnings 2>&1 | tail -5
git add src/state/mod.rs Cargo.toml tests/state_claims.rs
git commit -m "Plan A T10: Store::claim_next (BEGIN IMMEDIATE) + mark_succeeded (transactional)"
```
