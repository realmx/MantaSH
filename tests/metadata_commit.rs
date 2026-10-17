use mantash::{
    connections,
    credentials::SecretStore,
    events::Event,
    model::*,
    services::{
        Backend,
        metadata::{Applied, Mutation},
    },
    storage::Database,
};
use std::{
    collections::HashMap,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use zeroize::Zeroizing;

fn receive(
    backend: &Backend,
    events: &async_channel::Receiver<Event>,
) -> mantash::services::metadata::Receipt {
    backend.runtime.block_on(async {
        loop {
            if let Event::Metadata(receipt) =
                tokio::time::timeout(Duration::from_secs(5), events.recv())
                    .await
                    .unwrap()
                    .unwrap()
            {
                return receipt;
            }
        }
    })
}
#[test]
fn fifo_repeat_delete_record_and_exit_barrier_do_not_resurrect_ids() {
    let directory = tempfile::tempdir().unwrap();
    let (backend, events, _, warning) = Backend::initialize_in(directory.path().into()).unwrap();
    assert!(warning.is_none());
    backend.mutate_metadata(Mutation::Record(HistoryScope::Ssh, "command".into()));
    let first = receive(&backend, &events);
    let Applied::History(history) = first.result.unwrap() else {
        panic!("history receipt expected")
    };
    let old = history.items[0].id;
    let repeat = backend.mutate_metadata(Mutation::Record(HistoryScope::Ssh, " command ".into()));
    let delete = backend.delete_history(vec![old]);
    let added = backend.mutate_metadata(Mutation::Record(HistoryScope::Ssh, "command".into()));
    let mut prefs = Preferences::default();
    prefs.ui_size = 18.;
    backend
        .runtime
        .block_on(backend.final_workspace(prefs, Workspace::default()))
        .unwrap()
        .unwrap();
    let repeated = receive(&backend, &events);
    assert_eq!(repeated.request, repeat);
    assert_eq!(repeated.revision, first.revision + 1);
    let Applied::History(history) = repeated.result.unwrap() else {
        panic!("history receipt expected")
    };
    assert_eq!(history.items[0].id, old);
    let removed = receive(&backend, &events);
    assert_eq!(removed.request, delete);
    let Applied::History(history) = removed.result.unwrap() else {
        panic!("history receipt expected")
    };
    assert!(history.items.is_empty());
    let new = receive(&backend, &events);
    assert_eq!(new.request, added);
    let Applied::History(history) = new.result.unwrap() else {
        panic!("history receipt expected")
    };
    assert_ne!(history.items[0].id, old);
    let saved = Database::open(directory.path()).unwrap().load().unwrap();
    assert_eq!(saved.history, history);
    assert_eq!(saved.preferences.ui_size, 18.);
}
#[derive(Default)]
struct Store {
    fail: AtomicBool,
    values: Mutex<HashMap<Id, String>>,
}
impl SecretStore for Store {
    fn read(&self, id: Id) -> anyhow::Result<Option<Zeroizing<String>>> {
        Ok(self
            .values
            .lock()
            .unwrap()
            .get(&id)
            .cloned()
            .map(Zeroizing::new))
    }
    fn write(&self, id: Id, secret: &str) -> anyhow::Result<()> {
        anyhow::ensure!(!self.fail.load(Ordering::SeqCst), "fixture unavailable");
        self.values.lock().unwrap().insert(id, secret.into());
        Ok(())
    }
}
#[test]
fn csv_preserves_password_bytes_and_redacts_parser_errors() {
    let text = "\u{feff}password,username,host,port,name\r\n\" \t secret,\nline \t\",user,example.test,22, Fixture \r\n";
    let preview = connections::preview(text, &[]).unwrap();
    assert_eq!(
        preview.rows[0].password.as_deref(),
        Some(" \t secret,\nline \t")
    );
    assert_eq!(preview.rows[0].profile.as_ref().unwrap().name, "Fixture");
    assert!(!format!("{preview:?}").contains("secret,"));
    let malformed = connections::preview(
        "name,host,port,username,password\na,b,SENSITIVE,u,password\n",
        &[],
    )
    .unwrap();
    assert!(!format!("{malformed:?}").contains("SENSITIVE"));
    let header = connections::preview("name,host,port,username,SENSITIVE\n", &[])
        .unwrap_err()
        .to_string();
    assert!(!header.contains("SENSITIVE"));
}
#[test]
fn failed_metadata_import_writes_no_passwords_and_retry_does_not_replay_profiles() {
    let directory = tempfile::tempdir().unwrap();
    let mut db = Database::open(directory.path()).unwrap();
    let raw = rusqlite::Connection::open(directory.path().join("mantash.sqlite3")).unwrap();
    let store = Store::default();
    let preview = connections::preview(
        "name,host,port,username,password\nA,a.test,22,u, password \nB,b.test,22,u,second\n",
        &[],
    )
    .unwrap();
    raw.execute_batch("CREATE TRIGGER fail_profiles BEFORE INSERT ON records WHEN NEW.kind='profile' BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    assert!(connections::import_confirmed(&mut db, &store, &preview, false).is_err());
    assert!(store.values.lock().unwrap().is_empty());
    assert!(db.profiles().unwrap().is_empty());
    raw.execute_batch("DROP TRIGGER fail_profiles").unwrap();
    store.fail.store(true, Ordering::SeqCst);
    let result = connections::import_confirmed(&mut db, &store, &preview, false).unwrap();
    assert_eq!(result.failed.len(), 2);
    let mut changed = result.profiles[0].clone();
    changed.name = "Later edit".into();
    db.save_profiles(&[changed.clone()]).unwrap();
    store.fail.store(false, Ordering::SeqCst);
    let result = connections::retry_passwords(&db, &store, result.failed).unwrap();
    assert!(result.failed.is_empty());
    assert_eq!(result.profiles, vec![changed.clone()]);
    assert_eq!(store.values.lock().unwrap().len(), 1);
    assert_eq!(
        store.read(changed.id).unwrap().unwrap().as_str(),
        " password "
    );
}
#[test]
fn failed_profile_delete_keeps_saved_credentials() {
    let directory = tempfile::tempdir().unwrap();
    let (backend, events, _, _) = Backend::initialize_in(directory.path().into()).unwrap();
    let profile = Profile {
        id: Id::new_v4(),
        name: "fixture".into(),
        host: "example.test".into(),
        port: 22,
        username: "u".into(),
    };
    backend.mutate_metadata(Mutation::SaveProfile {
        profile: profile.clone(),
        existing: false,
    });
    receive(&backend, &events).result.ok().unwrap();
    backend.vault.write(profile.id, "fixture").unwrap();
    let raw = rusqlite::Connection::open(directory.path().join("mantash.sqlite3")).unwrap();
    raw.execute_batch("CREATE TRIGGER fail_delete BEFORE DELETE ON records WHEN OLD.kind='profile' BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    backend.mutate_metadata(Mutation::DeleteProfiles(vec![profile.id]));
    assert!(receive(&backend, &events).result.is_err());
    assert!(backend.vault.read(profile.id).unwrap().is_some());
    assert_eq!(backend.database.lock().profiles().unwrap(), vec![profile]);
}

#[test]
fn sensitive_csv_export_has_bom_restricted_mode_and_never_replaces_on_vault_failure() {
    struct Unavailable;
    impl SecretStore for Unavailable {
        fn read(&self, _: Id) -> anyhow::Result<Option<Zeroizing<String>>> {
            anyhow::bail!("fixture unavailable")
        }
        fn write(&self, _: Id, _: &str) -> anyhow::Result<()> {
            unreachable!()
        }
    }
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("connections.csv");
    let profile = Profile {
        id: Id::new_v4(),
        name: "fixture".into(),
        host: "example.test".into(),
        port: 22,
        username: "u".into(),
    };
    let store = Store::default();
    store.write(profile.id, " \tfixture,\nvalue ").unwrap();
    connections::export_file(&path, &[profile.clone()], &store).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.starts_with(&[0xef, 0xbb, 0xbf]));
    let preview = connections::preview_file(&path, &[]).unwrap();
    assert_eq!(
        preview.rows[0].password.as_deref(),
        Some(" \tfixture,\nvalue ")
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    assert!(connections::export_file(&path, &[profile], &Unavailable).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}
