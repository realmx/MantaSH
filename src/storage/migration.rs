//! Format 2 migration validates old metadata before backing it up and changing any row.
use super::*;
use anyhow::bail;
use rusqlite::{
    OpenFlags, TransactionBehavior,
    backup::{Backup, StepResult},
};
use std::{
    collections::HashSet,
    fs::OpenOptions,
    time::{Duration, Instant},
};

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyHistory {
    id: Id,
    scope: String,
    command: String,
    timestamp: i64,
}

fn decode<T: DeserializeOwned>(text: &str, kind: &str) -> Result<T> {
    serde_json::from_str(text).map_err(|_| anyhow::anyhow!("Invalid {kind} record"))
}

fn validate_schema(conn: &Connection) -> Result<bool> {
    let tables = conn.prepare("SELECT name FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")?
        .query_map([], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    if tables.is_empty() {
        return Ok(false);
    }
    ensure!(tables == ["records", "trust"], "Unknown database schema");
    for (table, expected) in [
        (
            "records",
            vec![("kind", "TEXT", 1), ("id", "TEXT", 2), ("data", "TEXT", 0)],
        ),
        (
            "trust",
            vec![
                ("host", "TEXT", 1),
                ("port", "INTEGER", 2),
                ("fingerprint", "TEXT", 0),
            ],
        ),
    ] {
        let columns = conn
            .prepare(&format!("PRAGMA table_info({table})"))?
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(5)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        ensure!(
            columns.len() == expected.len()
                && columns
                    .iter()
                    .zip(expected)
                    .all(|(actual, (name, ty, pk))| actual.0 == name
                        && actual.1.eq_ignore_ascii_case(ty)
                        && actual.2 == pk),
            "Unknown database schema"
        );
    }
    Ok(true)
}

fn inspect(conn: &Connection, version: u32) -> Result<HistoryCatalog> {
    let mut legacy = Vec::new();
    let mut catalog = None;
    let mut stmt = conn.prepare("SELECT rowid,kind,id,data FROM records ORDER BY rowid")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
        ))
    })?;
    for row in rows {
        let (rowid, kind, id, data) = row?;
        match kind.as_str() {
            "history" if version < 2 => {
                let item: LegacyHistory = decode(&data, "history")?;
                ensure!(
                    Id::parse_str(&id).ok() == Some(item.id)
                        && HistoryScope::group_of(&item.scope).is_some(),
                    "Invalid legacy history identity or scope"
                );
                legacy.push((rowid, item));
            }
            "history_catalog" if version == 2 && id == "current" => {
                let parsed: HistoryCatalog = decode(&data, "history catalog")?;
                parsed.validate()?;
                catalog = Some(parsed);
            }
            "profile" => {
                let profile: Profile = decode(&data, "profile")?;
                ensure!(
                    Id::parse_str(&id).ok() == Some(profile.id),
                    "Invalid profile identity"
                );
                profile.validate().map_err(anyhow::Error::msg)?;
            }
            "transfer" => {
                let task: TransferRecord = decode(&data, "transfer")?;
                ensure!(
                    Id::parse_str(&id).ok() == Some(task.id),
                    "Invalid transfer identity"
                );
            }
            "preferences" if id == "current" => {
                let _: Preferences = decode(&data, "preferences")?;
            }
            "workspace" if id == "current" => {
                let _: Workspace = decode(&data, "workspace")?;
            }
            _ => bail!("Unknown metadata record"),
        }
    }
    let mut trust = conn.prepare("SELECT host,port,fingerprint FROM trust")?;
    for row in trust.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, String>(2)?,
        ))
    })? {
        let (host, port, fingerprint) = row?;
        ensure!(
            !host.trim().is_empty()
                && (1..=65535).contains(&port)
                && !fingerprint.trim().is_empty(),
            "Invalid trust record"
        );
    }
    if version == 2 {
        return catalog.context("Missing history catalog");
    }
    legacy.sort_by(|(row_a, a), (row_b, b)| b.timestamp.cmp(&a.timestamp).then(row_b.cmp(row_a)));
    let mut catalog = HistoryCatalog::default();
    let mut commands = [HashSet::new(), HashSet::new()];
    for (_, item) in legacy {
        let scope = HistoryScope::group_of(&item.scope).context("Invalid legacy history scope")?;
        let command = item.command.trim();
        if command.is_empty() || !commands[scope.index()].insert(command.to_string()) {
            continue;
        }
        if catalog.items.len() == HISTORY_LIMIT {
            break;
        }
        catalog.items.push(HistoryItem {
            id: item.id,
            command: command.to_string(),
        });
        match scope {
            HistoryScope::Local => &mut catalog.local_ids,
            HistoryScope::Ssh => &mut catalog.ssh_ids,
        }
        .push(item.id);
    }
    catalog.validate()?;
    Ok(catalog)
}

/// A separate read connection sees committed WAL pages while the migration reserves the writer.
fn backup(directory: &Path) -> Result<()> {
    let path = directory.join(format!("mantash-before-v2-{}.sqlite3", Id::new_v4()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(&path)?;
    let source = Connection::open_with_flags(
        directory.join("mantash.sqlite3"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    let mut destination = Connection::open(&path)?;
    {
        let backup = Backup::new(&source, &mut destination)?;
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            ensure!(Instant::now() < deadline, "Database backup timed out");
            match backup.step(256)? {
                StepResult::Done => break,
                StepResult::More => {}
                StepResult::Busy | StepResult::Locked => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                _ => bail!("Database backup did not finish"),
            }
        }
    }
    let integrity: String = destination.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
    ensure!(integrity == "ok", "Database backup verification failed");
    destination.execute_batch("PRAGMA journal_mode=DELETE")?;
    destination.close().map_err(|(_, e)| e)?;
    file.sync_all()?;
    #[cfg(unix)]
    {
        std::fs::File::open(directory)?.sync_all()?;
    }
    Ok(())
}

pub(super) fn read(conn: &Connection) -> Result<HistoryCatalog> {
    let data: String = conn.query_row(
        "SELECT data FROM records WHERE kind='history_catalog' AND id='current'",
        [],
        |r| r.get(0),
    )?;
    let catalog: HistoryCatalog = decode(&data, "history catalog")?;
    catalog.validate()?;
    Ok(catalog)
}
pub(super) fn write(conn: &Connection, catalog: &HistoryCatalog) -> Result<()> {
    catalog.validate()?;
    conn.execute("INSERT INTO records(kind,id,data) VALUES('history_catalog','current',?1) ON CONFLICT(kind,id) DO UPDATE SET data=excluded.data", [serde_json::to_string(catalog)?])?;
    Ok(())
}
pub(super) fn initialize(conn: &mut Connection, directory: Option<&Path>) -> Result<()> {
    let transaction = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let version: u32 = transaction.pragma_query_value(None, "user_version", |r| r.get(0))?;
    ensure!(
        version <= 2,
        "Database format version {version} is not supported"
    );
    let exists = validate_schema(&transaction)?;
    if version == 2 {
        inspect(&transaction, version)?;
        transaction.commit()?;
        return Ok(());
    }
    let catalog = if exists {
        let catalog = inspect(&transaction, version)?;
        backup(directory.context("Migration requires a persistent data directory")?)
            .context("Migration backup failed; original database retained")?;
        catalog
    } else {
        ensure!(version == 0, "Missing database schema");
        transaction.execute_batch("CREATE TABLE records(kind TEXT NOT NULL,id TEXT NOT NULL,data TEXT NOT NULL,PRIMARY KEY(kind,id)); CREATE TABLE trust(host TEXT NOT NULL,port INTEGER NOT NULL,fingerprint TEXT NOT NULL,PRIMARY KEY(host,port));")?;
        HistoryCatalog::default()
    };
    write(&transaction, &catalog)?;
    transaction.execute("DELETE FROM records WHERE kind='history'", [])?;
    transaction.pragma_update(None, "user_version", 2)?;
    transaction.commit()?;
    ensure!(
        read(conn)? == catalog,
        "History verification failed; retain migration backup"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_backup_never_starts_migration() {
        let directory = tempfile::tempdir().unwrap();
        let mut conn = Connection::open(directory.path().join("mantash.sqlite3")).unwrap();
        conn.execute_batch("CREATE TABLE records(kind TEXT,id TEXT,data TEXT,PRIMARY KEY(kind,id)); CREATE TABLE trust(host TEXT,port INTEGER,fingerprint TEXT,PRIMARY KEY(host,port)); PRAGMA user_version=1;").unwrap();
        let id = Id::new_v4().to_string();
        let data = serde_json::json!({"id":id,"scope":"local","command":"fixture","timestamp":1})
            .to_string();
        conn.execute(
            "INSERT INTO records VALUES('history',?1,?2)",
            params![id, data],
        )
        .unwrap();
        // A regular file cannot contain a backup, independent of privileges or chmod.
        let blocked = directory.path().join("not-a-directory");
        std::fs::write(&blocked, b"fixture").unwrap();
        let error = initialize(&mut conn, Some(&blocked)).unwrap_err();
        assert!(error.to_string().contains("Migration backup failed"));
        assert_eq!(
            conn.pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))
                .unwrap(),
            1
        );
        let retained: (String, String) = conn
            .query_row(
                "SELECT id,data FROM records WHERE kind='history'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(retained, (id, data));
        assert_eq!(
            conn.query_row::<i64, _, _>(
                "SELECT count(*) FROM records WHERE kind='history_catalog'",
                [],
                |r| r.get(0)
            )
            .unwrap(),
            0
        );
    }
}
