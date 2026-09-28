//! SQLite persistence. Call from startup/background workers, never from GPUI render.
use crate::model::*;
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Serialize, de::DeserializeOwned};
use std::collections::HashMap;
use std::path::Path;

pub struct Database {
    conn: Connection,
}
#[derive(Default)]
pub struct Snapshot {
    pub profiles: Vec<Profile>,
    pub preferences: Preferences,
    pub workspace: Workspace,
    pub history: Vec<HistoryEntry>,
    pub transfers: Vec<TransferRecord>,
}

impl Database {
    /// Open an independent database without deleting or resetting damaged data.
    pub fn open(directory: &Path) -> Result<Self> {
        std::fs::create_dir_all(directory)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))?;
        }
        let conn = Connection::open(directory.join("mantash.sqlite3"))?;
        conn.busy_timeout(std::time::Duration::from_secs(3))?;
        Self::initialize(conn)
    }
    /// Isolated storage for tests and explicitly disclosed session-only recovery mode.
    pub fn memory() -> Result<Self> {
        Self::initialize(Connection::open_in_memory()?)
    }
    fn initialize(conn: Connection) -> Result<Self> {
        // One format, stamped with a generation number: files without a stamp adopt 1,
        // any other number is a format this build does not understand and is rejected
        // without modification. Additive-only evolution keeps the stamp at 1 forever.
        let version: u32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        ensure!(
            version == 0 || version == 1,
            "Database format version {version} is not supported"
        );
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;
          CREATE TABLE IF NOT EXISTS records(kind TEXT NOT NULL, id TEXT NOT NULL, data TEXT NOT NULL, PRIMARY KEY(kind,id));
          CREATE TABLE IF NOT EXISTS trust(host TEXT NOT NULL, port INTEGER NOT NULL, fingerprint TEXT NOT NULL, PRIMARY KEY(host,port));")?;
        if version == 0 {
            conn.pragma_update(None, "user_version", 1)?;
        }
        Ok(Self { conn })
    }
    fn list<T: DeserializeOwned>(&self, kind: &str) -> Result<Vec<T>> {
        let mut stmt = self
            .conn
            .prepare("SELECT data FROM records WHERE kind=?1 ORDER BY rowid")?;
        let records = stmt
            .query_map([kind], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        records
            .iter()
            .map(|s| serde_json::from_str(s).with_context(|| format!("Cannot read {kind} record")))
            .collect()
    }
    fn one<T: DeserializeOwned + Default>(&self, kind: &str) -> Result<T> {
        let data: Option<String> = self
            .conn
            .query_row(
                "SELECT data FROM records WHERE kind=?1 AND id='current'",
                [kind],
                |r| r.get(0),
            )
            .optional()?;
        data.map_or_else(|| Ok(T::default()), |s| Ok(serde_json::from_str(&s)?))
    }
    /// Read all application metadata and repair only transient restoration state.
    pub fn load(&self) -> Result<Snapshot> {
        let mut preferences: Preferences = self.one("preferences")?;
        preferences.normalize();
        // Work restoration is unconditional: tabs and layout always come back.
        let mut workspace: Workspace = self.one("workspace")?;
        workspace.normalize();
        let mut history: Vec<HistoryEntry> = self.list("history")?;
        history.sort_by_key(|h| std::cmp::Reverse(h.timestamp));
        let mut transfers: Vec<TransferRecord> = self.list("transfer")?;
        for task in &mut transfers {
            if task.state.active() {
                task.state = TransferState::Interrupted;
                task.error = Some("Application exited before the transfer finished".into());
            }
        }
        Ok(Snapshot {
            profiles: self.list("profile")?,
            preferences,
            workspace,
            history,
            transfers,
        })
    }
    fn put<T: Serialize>(&self, kind: &str, id: &str, value: &T) -> Result<()> {
        self.conn.execute("INSERT INTO records(kind,id,data) VALUES(?1,?2,?3) ON CONFLICT(kind,id) DO UPDATE SET data=excluded.data", params![kind, id, serde_json::to_string(value)?])?;
        Ok(())
    }
    /// Save an entire connection merge atomically, never clearing before validation.
    pub fn save_profiles(&mut self, profiles: &[Profile]) -> Result<()> {
        for p in profiles {
            p.validate().map_err(anyhow::Error::msg)?;
        }
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM records WHERE kind='profile'", [])?;
        for p in profiles {
            tx.execute(
                "INSERT INTO records(kind,id,data) VALUES('profile',?1,?2)",
                params![p.id.to_string(), serde_json::to_string(p)?],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
    /// Persist preferences and workspace in one transaction.
    pub fn save_workspace(&mut self, prefs: &Preferences, workspace: &Workspace) -> Result<()> {
        let tx = self.conn.transaction()?;
        for (kind, data) in [
            ("preferences", serde_json::to_string(prefs)?),
            ("workspace", serde_json::to_string(workspace)?),
        ] {
            tx.execute("INSERT INTO records(kind,id,data) VALUES(?1,'current',?2) ON CONFLICT(kind,id) DO UPDATE SET data=excluded.data", params![kind, data])?;
        }
        tx.commit()?;
        Ok(())
    }
    /// Persist one reliably reported command, replacing older copies of the same
    /// command in its logical list, and keep the newest 5,000 history entries.
    pub fn touch_history(&self, entry: &HistoryEntry) -> Result<()> {
        if let Some(group) = HistoryScope::group_of(&entry.scope) {
            for old in self
                .list::<HistoryEntry>("history")?
                .into_iter()
                .filter(|old| {
                    old.command == entry.command
                        && HistoryScope::group_of(&old.scope) == Some(group)
                })
            {
                self.conn.execute(
                    "DELETE FROM records WHERE kind='history' AND id=?1",
                    params![old.id.to_string()],
                )?;
            }
        }
        self.put("history", &entry.id.to_string(), entry)?;
        self.conn.execute("DELETE FROM records WHERE kind='history' AND rowid NOT IN (SELECT rowid FROM records WHERE kind='history' ORDER BY rowid DESC LIMIT 5000)", [])?;
        Ok(())
    }
    /// Collapse stored duplicates once at startup: per logical list each command
    /// keeps only its newest entry, so pre-dedup data cannot reappear after deletes.
    pub fn compact_history(&mut self) -> Result<()> {
        let entries: Vec<HistoryEntry> = self.list("history")?;
        let mut newest: HashMap<(bool, &str), (i64, Id)> = HashMap::new();
        let mut stale: Vec<Id> = Vec::new();
        for entry in &entries {
            let Some(group) = HistoryScope::group_of(&entry.scope) else {
                continue;
            };
            let key = (matches!(group, HistoryScope::Ssh), entry.command.as_str());
            match newest.get(&key) {
                Some((timestamp, _)) if *timestamp >= entry.timestamp => stale.push(entry.id),
                Some((_, kept)) => {
                    stale.push(*kept);
                    newest.insert(key, (entry.timestamp, entry.id));
                }
                None => {
                    newest.insert(key, (entry.timestamp, entry.id));
                }
            }
        }
        if !stale.is_empty() {
            self.delete_records("history", &stale)?;
        }
        Ok(())
    }
    /// Delete only explicitly selected history identifiers.
    pub fn delete_history(&mut self, ids: &[Id]) -> Result<()> {
        self.delete_records("history", ids)
    }
    fn delete_records(&mut self, kind: &str, ids: &[Id]) -> Result<()> {
        let tx = self.conn.transaction()?;
        for id in ids {
            tx.execute(
                "DELETE FROM records WHERE kind=?1 AND id=?2",
                params![kind, id.to_string()],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
    /// Store transfers independently from the terminal that started them.
    pub fn save_transfer(&self, transfer: &TransferRecord) -> Result<()> {
        self.put("transfer", &transfer.id.to_string(), transfer)
    }
    /// Remove finished transfer records; the caller must reject active task removal.
    pub fn delete_transfers(&mut self, ids: &[Id]) -> Result<()> {
        self.delete_records("transfer", ids)
    }
    /// Find this application's trusted host fingerprint, never reading the old application's trust store.
    pub fn fingerprint(&self, host: &str, port: u16) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT fingerprint FROM trust WHERE host=?1 AND port=?2",
                params![host.to_lowercase(), port],
                |r| r.get(0),
            )
            .optional()?)
    }
    /// Record explicit user approval before the SSH authentication phase begins.
    pub fn trust(&self, host: &str, port: u16, fingerprint: &str) -> Result<()> {
        self.conn.execute("INSERT INTO trust(host,port,fingerprint) VALUES(?1,?2,?3) ON CONFLICT(host,port) DO UPDATE SET fingerprint=excluded.fingerprint", params![host.to_lowercase(), port, fingerprint])?;
        Ok(())
    }
}
