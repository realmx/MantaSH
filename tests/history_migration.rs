//! Isolated v0/v1 databases exercise backup, atomic migration and failure preservation.
use mantash::{model::*, storage::Database};
use rusqlite::{Connection, params};
use serde_json::json;
use std::path::{Path, PathBuf};

fn legacy(path: &Path, version: u32) -> Connection {
    let db = Connection::open(path.join("mantash.sqlite3")).unwrap();
    db.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE records(kind TEXT,id TEXT,data TEXT,PRIMARY KEY(kind,id)); CREATE TABLE trust(host TEXT,port INTEGER,fingerprint TEXT,PRIMARY KEY(host,port));").unwrap();
    db.pragma_update(None, "user_version", version).unwrap();
    db
}
fn put(db: &Connection, id: Id, scope: &str, command: &str, timestamp: i64) {
    db.execute(
        "INSERT INTO records VALUES('history',?1,?2)",
        params![
            id.to_string(),
            json!({"id":id,"scope":scope,"command":command,"timestamp":timestamp}).to_string()
        ],
    )
    .unwrap();
}
fn backups(path: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(path)
        .unwrap()
        .map(|p| p.unwrap().path())
        .filter(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("mantash-before-v2-")
        })
        .collect()
}
fn version(db: &Connection) -> u32 {
    db.pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap()
}

#[test]
fn wal_backup_preserves_metadata_and_migration_is_idempotent() {
    let directory = tempfile::tempdir().unwrap();
    let old = legacy(directory.path(), 1);
    let profile = Profile {
        id: Id::new_v4(),
        name: "fixture".into(),
        host: "example.test".into(),
        port: 22,
        username: "tester".into(),
    };
    let prefs = json!({"ui_size":18.0,"terminal_size":16.0});
    old.execute(
        "INSERT INTO records VALUES('profile',?1,?2)",
        params![
            profile.id.to_string(),
            serde_json::to_string(&profile).unwrap()
        ],
    )
    .unwrap();
    old.execute(
        "INSERT INTO records VALUES('preferences','current',?1)",
        [prefs.to_string()],
    )
    .unwrap();
    old.execute(
        "INSERT INTO trust VALUES('example.test',22,'SHA256:fixture')",
        [],
    )
    .unwrap();
    let first = Id::new_v4();
    let newest = Id::new_v4();
    let local = Id::new_v4();
    let distinct = Id::new_v4();
    put(
        &old,
        first,
        &format!("ssh:{}", Id::new_v4()),
        " echo  中文 ",
        10,
    );
    put(&old, local, "local", "echo  中文", 11);
    put(&old, distinct, "ssh", "echo 中文", 12);
    put(
        &old,
        newest,
        &format!("ssh:{}", Id::new_v4()),
        "echo  中文",
        10,
    );
    put(&old, Id::new_v4(), "local", " \n", 99);
    // Keep this WAL connection open through backup; copying only the main file loses rows.
    let mut db = Database::open(directory.path()).unwrap();
    let snapshot = db.load().unwrap();
    assert_eq!(
        snapshot
            .history
            .items
            .iter()
            .map(|item| item.id)
            .collect::<Vec<_>>(),
        vec![distinct, local, newest]
    );
    assert_eq!(snapshot.profiles[0].id, profile.id);
    assert_eq!(snapshot.preferences.ui_size, 18.);
    assert_eq!(
        db.fingerprint("example.test", 22).unwrap().as_deref(),
        Some("SHA256:fixture")
    );
    let backup_paths = backups(directory.path());
    assert_eq!(backup_paths.len(), 1);
    let backup = Connection::open(&backup_paths[0]).unwrap();
    assert_eq!(version(&backup), 1);
    assert_eq!(
        backup
            .query_row::<i64, _, _>(
                "SELECT count(*) FROM records WHERE kind='history'",
                [],
                |r| r.get(0)
            )
            .unwrap(),
        5
    );
    assert_eq!(version(&old), 2);
    let text: String = old
        .query_row(
            "SELECT data FROM records WHERE kind='history_catalog'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(!text.contains("timestamp"));
    assert!(!text.contains("scope"));
    db.delete_history(&[newest]).unwrap();
    drop(db);
    let db = Database::open(directory.path()).unwrap();
    assert!(
        !db.load()
            .unwrap()
            .history
            .items
            .iter()
            .any(|item| item.id == newest)
    );
    assert_eq!(backups(directory.path()).len(), 1);
}
#[test]
fn empty_formats_and_global_capacity() {
    for stamp in [0, 1] {
        let directory = tempfile::tempdir().unwrap();
        let old = legacy(directory.path(), stamp);
        let db = Database::open(directory.path()).unwrap();
        assert!(db.load().unwrap().history.items.is_empty());
        assert_eq!(version(&old), 2);
        assert_eq!(backups(directory.path()).len(), 1);
    }
    let directory = tempfile::tempdir().unwrap();
    let old = legacy(directory.path(), 1);
    old.execute_batch("BEGIN").unwrap();
    for i in 0..5005 {
        put(
            &old,
            Id::new_v4(),
            if i % 2 == 0 { "local" } else { "ssh" },
            &format!("cmd {i}"),
            i,
        );
    }
    old.execute_batch("COMMIT").unwrap();
    let db = Database::open(directory.path()).unwrap();
    let history = db.load().unwrap().history;
    assert_eq!(history.items.len(), 5000);
    assert_eq!(history.items.first().unwrap().command, "cmd 5004");
    assert_eq!(history.items.last().unwrap().command, "cmd 5");
    history.validate().unwrap();
}
#[test]
fn invalid_records_do_not_change_original_or_make_a_backup() {
    for raw in [
        "{broken".into(),
        json!({"id":"not-a-uuid","scope":"ssh","command":"fixture","timestamp":1}).to_string(),
        json!({"id":Id::new_v4(),"scope":"ssh:invalid","command":"fixture","timestamp":1})
            .to_string(),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let old = legacy(directory.path(), 1);
        old.execute(
            "INSERT INTO records VALUES('history',?1,?2)",
            params![Id::new_v4().to_string(), raw],
        )
        .unwrap();
        assert!(Database::open(directory.path()).is_err());
        assert_eq!(version(&old), 1);
        assert_eq!(
            old.query_row::<String, _, _>("SELECT data FROM records", [], |r| r.get(0))
                .unwrap(),
            raw
        );
        assert!(backups(directory.path()).is_empty());
    }
}
#[test]
fn failed_transaction_keeps_old_rows_and_backup() {
    let directory = tempfile::tempdir().unwrap();
    let old = legacy(directory.path(), 1);
    let id = Id::new_v4();
    put(&old, id, "local", "fixture", 1);
    old.execute_batch("CREATE TRIGGER fail_migration BEFORE INSERT ON records WHEN NEW.kind='history_catalog' BEGIN SELECT RAISE(ABORT,'fixture failure'); END;").unwrap();
    assert!(Database::open(directory.path()).is_err());
    assert_eq!(version(&old), 1);
    assert_eq!(
        old.query_row::<String, _, _>("SELECT id FROM records", [], |r| r.get(0))
            .unwrap(),
        id.to_string()
    );
    assert_eq!(backups(directory.path()).len(), 1);
}
