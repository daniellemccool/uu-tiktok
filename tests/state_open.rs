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
