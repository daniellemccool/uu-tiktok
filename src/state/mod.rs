mod schema;

use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::{params, Connection};

pub use schema::SCHEMA_VERSION;

// Helper for upsert_video / upsert_watch_history. Bin compilation re-includes
// state via `mod state;` (per AD0002) and has no caller until T13 (ingest cmd).
#[allow(dead_code)]
fn unix_now() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Test-only helper for verifying row state. Not part of the public API; gated
/// to test compilation only.
// Cfg-gated to `any(test, feature = "test-helpers")`. When clippy/clippy-style
// tests run with `--features test-helpers`, the bin compilation also gets the
// feature and includes this struct, but never references it — hence dead_code.
#[cfg(any(test, feature = "test-helpers"))]
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct VideoRow {
    pub video_id: String,
    pub status: String,
    pub canonical: bool,
    pub source_url: String,
    pub first_seen_at: i64,
    pub attempt_count: i64,
}

pub struct Store {
    conn: Connection,
}

impl Store {
    // Binary crate wires `mod state;` but doesn't call Store yet — T13 (ingest),
    // T14 (process), and T15 (init) are the first callers.
    #[allow(dead_code)]
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)
            .with_context(|| format!("opening SQLite database at {}", path.display()))?;

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

    // Dead in binary crate until T13/T14/T15 wire Store.
    #[allow(dead_code)]
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

    // Dead in binary crate until T13/T14/T15 wire Store.
    #[allow(dead_code)]
    pub fn pragma_string(&self, name: &str) -> Result<String> {
        let value: String = self
            .conn
            .query_row(&format!("PRAGMA {}", name), [], |row| row.get(0))
            .with_context(|| format!("reading PRAGMA {}", name))?;
        Ok(value)
    }

    /// Borrow the underlying connection for advanced operations. Internal use
    /// for now; the public API will grow as Tasks 9+ add methods.
    // T9 (store-ingest) and T10 (store-claims) are the first consumers.
    #[allow(dead_code)]
    pub(crate) fn conn(&self) -> &Connection {
        &self.conn
    }

    // T9 (store-ingest) and T10 (store-claims) are the first consumers.
    #[allow(dead_code)]
    pub(crate) fn conn_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }

    // T13 (ingest cmd) is the first bin consumer; integration tests at
    // tests/state_ingest.rs already exercise this in the lib compilation.
    #[allow(dead_code)]
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

    // T13 (ingest cmd) is the first bin consumer; integration tests at
    // tests/state_ingest.rs already exercise this in the lib compilation.
    #[allow(dead_code)]
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
}

impl Store {
    // Cfg-gated test helper; same bin-firing dynamic as `VideoRow` above when
    // `--features test-helpers` is enabled at the workspace level.
    #[cfg(any(test, feature = "test-helpers"))]
    #[allow(dead_code)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Real-TDD bug-fix test (per ADR 0003). SQLite's `TEXT PRIMARY KEY` does
    /// NOT imply NOT NULL — only `INTEGER PRIMARY KEY` (rowid alias) does. The
    /// schema must declare NOT NULL explicitly. This test guards against
    /// regressing the schema to the implicit-NULL form.
    #[test]
    fn null_video_id_rejected_by_videos_schema() {
        let tmp = TempDir::new().unwrap();
        let store = Store::open(&tmp.path().join("state.sqlite")).unwrap();
        let result = store.conn().execute(
            "INSERT INTO videos
             (video_id, source_url, canonical, status, first_seen_at, updated_at)
             VALUES (NULL, 'x', 0, 'pending', 0, 0)",
            [],
        );
        assert!(
            result.is_err(),
            "expected NOT NULL constraint to reject NULL video_id, but insert succeeded"
        );
    }

    /// Same SQLite quirk applies to `meta.key`. Guard it too.
    #[test]
    fn null_meta_key_rejected_by_meta_schema() {
        let tmp = TempDir::new().unwrap();
        let store = Store::open(&tmp.path().join("state.sqlite")).unwrap();
        let result = store
            .conn()
            .execute("INSERT INTO meta (key, value) VALUES (NULL, 'x')", []);
        assert!(
            result.is_err(),
            "expected NOT NULL constraint to reject NULL meta.key, but insert succeeded"
        );
    }
}
