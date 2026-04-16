# Plan A — Task 7: SQLite schema + `Store::open` + migrations

> Part of [Plan A: walking skeleton](./00-overview.md). See the overview for file structure, dependency set, conventions, and exit criteria.

**Files:**
- Create: `src/state/mod.rs`
- Create: `src/state/schema.rs`
- Modify: `src/lib.rs`, `src/main.rs`
- Test: `tests/state_open.rs`

Plan A schema is a deliberate subset of the spec's full schema. Plan B adds failure persistence columns; Plan C adds `pending_resolutions`.

- [ ] **Step 1: Write the failing schema test**

Create `tests/state_open.rs`:

```rust
use tempfile::TempDir;
use uu_tiktok::state::Store;

#[test]
fn open_creates_schema_on_fresh_db() {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("state.sqlite");

    let _store = Store::open(&db_path).expect("open succeeds");

    assert!(db_path.exists());
}

#[test]
fn open_is_idempotent() {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("state.sqlite");

    let _first = Store::open(&db_path).expect("first open");
    drop(_first);
    let _second = Store::open(&db_path).expect("second open does not fail");
}

#[test]
fn schema_version_is_recorded() {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("state.sqlite");

    let store = Store::open(&db_path).expect("open succeeds");
    let version: String = store
        .read_meta("schema_version")
        .expect("read_meta succeeds")
        .expect("schema_version present");
    assert_eq!(version, "1");
}

#[test]
fn pragma_journal_mode_is_wal() {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("state.sqlite");

    let store = Store::open(&db_path).expect("open succeeds");
    let mode = store.pragma_string("journal_mode").expect("read pragma");
    assert_eq!(mode.to_lowercase(), "wal");
}
```

- [ ] **Step 2: Implement schema and `Store::open`**

Create `src/state/schema.rs`:

```rust
pub const SCHEMA_VERSION: &str = "1";

pub const SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS videos (
    video_id            TEXT PRIMARY KEY,
    source_url          TEXT NOT NULL,
    canonical           INTEGER NOT NULL,
    status              TEXT NOT NULL CHECK (status IN
                          ('pending','in_progress','succeeded','failed_terminal','failed_retryable')),
    claimed_by          TEXT,
    claimed_at          INTEGER,
    attempt_count       INTEGER NOT NULL DEFAULT 0,
    succeeded_at        INTEGER,
    duration_s          REAL,
    language_detected   TEXT,
    fetcher             TEXT,
    transcript_source   TEXT,
    first_seen_at       INTEGER NOT NULL,
    updated_at          INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_videos_pending
    ON videos (status, first_seen_at, video_id)
    WHERE status = 'pending';

CREATE TABLE IF NOT EXISTS watch_history (
    respondent_id  TEXT NOT NULL,
    video_id       TEXT NOT NULL,
    watched_at     INTEGER NOT NULL,
    in_window      INTEGER NOT NULL,
    PRIMARY KEY (respondent_id, video_id, watched_at),
    FOREIGN KEY (video_id) REFERENCES videos(video_id)
);
CREATE INDEX IF NOT EXISTS idx_watch_history_video ON watch_history (video_id);

CREATE TABLE IF NOT EXISTS video_events (
    id           INTEGER PRIMARY KEY,
    video_id     TEXT NOT NULL,
    at           INTEGER NOT NULL,
    event_type   TEXT NOT NULL,
    worker_id    TEXT,
    detail_json  TEXT,
    FOREIGN KEY (video_id) REFERENCES videos(video_id)
);
CREATE INDEX IF NOT EXISTS idx_video_events_video ON video_events (video_id, at);

CREATE TABLE IF NOT EXISTS meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
"#;
```

Create `src/state/mod.rs`:

```rust
mod schema;

use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::{params, Connection};

pub use schema::SCHEMA_VERSION;

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path).with_context(|| {
            format!("opening SQLite database at {}", path.display())
        })?;

        // Pragmas applied at every open.
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;
             PRAGMA busy_timeout = 5000;",
        )
        .context("setting connection pragmas")?;

        // Schema (idempotent — uses CREATE IF NOT EXISTS).
        conn.execute_batch(schema::SCHEMA_SQL)
            .context("applying schema")?;

        // Record schema version (only on first run).
        conn.execute(
            "INSERT OR IGNORE INTO meta (key, value) VALUES ('schema_version', ?1)",
            params![SCHEMA_VERSION],
        )
        .context("recording schema_version")?;

        Ok(Self { conn })
    }

    pub fn read_meta(&self, key: &str) -> Result<Option<String>> {
        let result = self
            .conn
            .query_row(
                "SELECT value FROM meta WHERE key = ?1",
                params![key],
                |row| row.get::<_, String>(0),
            )
            .map_or_else(
                |e| match e {
                    rusqlite::Error::QueryReturnedNoRows => Ok(None),
                    other => Err(other),
                },
                |v| Ok(Some(v)),
            )?;
        Ok(result)
    }

    pub fn pragma_string(&self, name: &str) -> Result<String> {
        let value: String = self
            .conn
            .query_row(&format!("PRAGMA {}", name), [], |row| row.get(0))
            .with_context(|| format!("reading PRAGMA {}", name))?;
        Ok(value)
    }

    /// Borrow the underlying connection for advanced operations. Internal use
    /// for now; the public API will grow as Tasks 9+ add methods.
    pub(crate) fn conn(&self) -> &Connection {
        &self.conn
    }

    pub(crate) fn conn_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }
}
```

- [ ] **Step 3: Wire into `lib.rs` and `main.rs`**

Add `pub mod state;` to `src/lib.rs`. Add `mod state;` to `src/main.rs`.

- [ ] **Step 4: Run tests to confirm pass**

Run: `cargo test --test state_open 2>&1 | tail -10`
Expected: `4 passed; 0 failed`.

- [ ] **Step 5: Commit**

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings 2>&1 | tail -5
git add src/state/ src/main.rs src/lib.rs tests/state_open.rs
git commit -m "Plan A T7: SQLite schema and Store::open with WAL/foreign-keys pragmas"
```
