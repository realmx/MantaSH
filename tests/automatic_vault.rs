//! Current credential storage works without any interactive unlock.
use mantash::{credentials::SecretStore, model::Id, vault::LocalVault};
use std::fs;
use tempfile::tempdir;

#[test]
fn first_save_and_restart_need_no_master_password() {
    let dir = tempdir().unwrap();
    let id = Id::new_v4();
    let vault = LocalVault::new(dir.path());
    assert!(vault.read(id).unwrap().is_none());
    assert!(!dir.path().join("credentials.key").exists());
    vault.write(id, "ssh-秘密-example").unwrap();
    let bytes = fs::read(dir.path().join("credentials-local.sqlite3")).unwrap();
    assert!(
        !bytes
            .windows("ssh-秘密-example".len())
            .any(|b| b == "ssh-秘密-example".as_bytes())
    );
    let restarted = LocalVault::new(dir.path());
    assert_eq!(
        restarted.read(id).unwrap().unwrap().as_str(),
        "ssh-秘密-example"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for path in ["credentials.key", "credentials-local.sqlite3"] {
            assert_eq!(
                fs::metadata(dir.path().join(path))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }
}

#[test]
fn simultaneous_first_saves_use_one_key_and_preserve_every_record() {
    let dir = tempdir().unwrap();
    let ids: Vec<_> = (0..8).map(|_| Id::new_v4()).collect();
    std::thread::scope(|s| {
        for (i, id) in ids.iter().copied().enumerate() {
            let store = LocalVault::new(dir.path());
            s.spawn(move || store.write(id, &format!("secret-{i}")).unwrap());
        }
    });
    let store = LocalVault::new(dir.path());
    for (i, id) in ids.into_iter().enumerate() {
        assert_eq!(
            store.read(id).unwrap().unwrap().as_str(),
            format!("secret-{i}")
        );
    }
    assert_eq!(
        fs::read(dir.path().join("credentials.key")).unwrap().len(),
        32
    );
}

#[test]
fn missing_or_damaged_key_does_not_replace_old_data() {
    let dir = tempdir().unwrap();
    let store = LocalVault::new(dir.path());
    let id = Id::new_v4();
    store.write(id, "keep-this-password").unwrap();
    let path = dir.path().join("credentials-local.sqlite3");
    let before = fs::read(&path).unwrap();
    // Keep the original key for the assertion; no user files are touched or discarded.
    fs::rename(
        dir.path().join("credentials.key"),
        dir.path().join("original.key"),
    )
    .unwrap();
    assert!(store.read(id).is_err());
    assert!(store.write(id, "replacement").is_err());
    assert!(!dir.path().join("credentials.key").exists());
    assert_eq!(before, fs::read(&path).unwrap());
    fs::write(dir.path().join("credentials.key"), [0_u8; 32]).unwrap();
    assert!(store.read(id).is_err());
    assert!(store.write(id, "replacement").is_err());
    assert_eq!(before, fs::read(&path).unwrap());
}

#[test]
fn ciphertext_is_bound_to_its_uuid_and_each_write_uses_a_new_nonce() {
    let dir = tempdir().unwrap();
    let store = LocalVault::new(dir.path());
    let a = Id::new_v4();
    let b = Id::new_v4();
    store.write(a, "one").unwrap();
    store.write(b, "two").unwrap();
    let db = rusqlite::Connection::open(dir.path().join("credentials-local.sqlite3")).unwrap();
    let nonce: Vec<u8> = db
        .query_row(
            "SELECT nonce FROM vault_secrets WHERE id=?1",
            [a.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    store.write(a, "one").unwrap();
    let next: Vec<u8> = db
        .query_row(
            "SELECT nonce FROM vault_secrets WHERE id=?1",
            [a.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_ne!(nonce, next);
    db.execute("UPDATE vault_secrets SET nonce=(SELECT nonce FROM vault_secrets WHERE id=?1),ciphertext=(SELECT ciphertext FROM vault_secrets WHERE id=?1) WHERE id=?2",[a.to_string(),b.to_string()]).unwrap();
    assert!(store.read(b).is_err());
    assert_eq!(store.read(a).unwrap().unwrap().as_str(), "one");
    store.forget(a).unwrap();
    assert!(store.read(a).unwrap().is_none());
}
