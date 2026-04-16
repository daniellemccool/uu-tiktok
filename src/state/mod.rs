mod schema;

use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::{params, Connection};

pub use schema::SCHEMA_VERSION;

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
