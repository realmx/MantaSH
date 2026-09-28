//! Automatically managed local credential encryption; no master-password or OS keychain prompts.
use crate::{credentials::SecretStore, model::Id};
use aes_gcm::{
    Aes256Gcm, KeyInit,
    aead::{Aead, Payload},
};
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};
use zeroize::Zeroizing;

// The single format's magic, bound into the AAD of every stored value. It is the format's
// only identity — there is no version number — and it never changes.
const CHECK: &[u8] = b"MantaSH local credentials";
const MAX_SECRET: usize = 65536;

/// The current format uses a local key and never requires interactive unlocking.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VaultState {
    Automatic,
}
struct Header {
    salt: [u8; 16],
    nonce: [u8; 12],
    check: Vec<u8>,
}

/// A local key encrypts remembered credentials automatically across sessions and app restarts.
/// The key is stored on this device alongside application data, not in an OS credential store.
pub struct LocalVault {
    path: PathBuf,
    key_path: PathBuf,
}
impl LocalVault {
    pub fn new(directory: &Path) -> Self {
        Self {
            path: directory.join("credentials-local.sqlite3"),
            key_path: directory.join("credentials.key"),
        }
    }
    /// Rendering this status performs no IO and cannot prompt for a password.
    pub fn state(&self) -> VaultState {
        VaultState::Automatic
    }
    /// Remove only the confirmed UUID from the current store. Removing ciphertext needs no key.
    pub fn forget(&self, id: Id) -> Result<()> {
        if self.path.exists() {
            let db = open(&self.path, false)?;
            header(&db)?;
            db.execute("DELETE FROM vault_secrets WHERE id=?1", [id.to_string()])?;
        }
        Ok(())
    }
    /// Resolve the key inside the SQLite writer transaction so simultaneous first saves share it.
    fn prepare(&self, db: &Connection) -> Result<(Header, Zeroizing<[u8; 32]>)> {
        if let Some(h) = header(db)? {
            let key = self.load_key(false)?;
            verify(&h, &key)?;
            return Ok((h, key));
        }
        let key = self.load_key(true)?;
        let salt = random()?;
        let (nonce, check) = encrypt(&key, CHECK, &aad(&salt, CHECK))?;
        db.execute(
            "INSERT INTO vault_meta(id,salt,nonce,check_value) VALUES(1,?1,?2,?3)",
            params![salt.as_slice(), nonce.as_slice(), check],
        )?;
        Ok((Header { salt, nonce, check }, key))
    }
    /// Publish a complete random key without replacing an existing key. A missing/corrupt key for
    /// an initialized database is an error; silently generating another one would lose old data.
    fn load_key(&self, create: bool) -> Result<Zeroizing<[u8; 32]>> {
        if let Some(key) = read_key(&self.key_path)? {
            return Ok(key);
        }
        ensure!(
            create,
            "Local encryption key is missing; restore credentials.key with its database"
        );
        let directory = self
            .key_path
            .parent()
            .context("Credential directory unavailable")?;
        let key = Zeroizing::new(random::<32>()?);
        let mut temporary = tempfile::Builder::new()
            .prefix(".mantash-credentials-")
            .tempfile_in(directory)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            temporary
                .as_file()
                .set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        temporary.write_all(key.as_ref())?;
        temporary.as_file().sync_all()?;
        match temporary.persist_noclobber(&self.key_path) {
            Ok(_) => {
                #[cfg(unix)]
                fs::File::open(directory)?.sync_all()?;
                Ok(key)
            }
            Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
                read_key(&self.key_path)?.context("Local encryption key disappeared")
            }
            Err(error) => Err(error.error.into()),
        }
    }
}
impl SecretStore for LocalVault {
    fn read(&self, id: Id) -> Result<Option<Zeroizing<String>>> {
        if !self.path.exists() {
            return Ok(None);
        }
        let db = open(&self.path, false)?;
        db.execute_batch("BEGIN DEFERRED;")?;
        let Some(h) = header(&db)? else {
            return Ok(None);
        };
        let lengths: Option<(i64, i64)> = db
            .query_row(
                "SELECT length(ciphertext),length(nonce) FROM vault_secrets WHERE id=?1",
                [id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((length, nonce_length)) = lengths else {
            return Ok(None);
        };
        ensure!(
            (16..=(MAX_SECRET + 16) as i64).contains(&length) && nonce_length == 12,
            "Invalid encrypted credential length"
        );
        let (nonce, bytes): (Vec<u8>, Vec<u8>) = db.query_row(
            "SELECT nonce,ciphertext FROM vault_secrets WHERE id=?1",
            [id.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        db.execute_batch("COMMIT;")?;
        let key = self.load_key(false)?;
        verify(&h, &key)?;
        let nonce: [u8; 12] = nonce
            .try_into()
            .map_err(|_| anyhow::anyhow!("Invalid credential nonce"))?;
        let plain = decrypt(&key, &nonce, &bytes, &aad(&h.salt, id.as_bytes()))?;
        Ok(Some(Zeroizing::new(
            std::str::from_utf8(&plain)?.to_owned(),
        )))
    }
    fn write(&self, id: Id, secret: &str) -> Result<()> {
        ensure!(
            !secret.is_empty() && secret.len() <= MAX_SECRET,
            "Credential is empty or too long"
        );
        let mut db = open(&self.path, true)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (h, key) = self.prepare(&tx)?;
        save_record(&tx, &key, &h, id, secret)?;
        tx.commit()?;
        Ok(())
    }
}

/// Read a bounded private key file and reject partial files and symbolic links.
fn read_key(path: &Path) -> Result<Option<Zeroizing<[u8; 32]>>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink() && metadata.len() == 32,
        "Invalid local encryption key file"
    );
    let mut bytes = Zeroizing::new(Vec::new());
    fs::File::open(path)?.take(33).read_to_end(&mut bytes)?;
    let key: [u8; 32] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("Incomplete local encryption key"))?;
    Ok(Some(Zeroizing::new(key)))
}
/// Bind credentials to their format, database identity and stable connection UUID.
fn aad(salt: &[u8; 16], identity: &[u8]) -> Vec<u8> {
    [CHECK, salt.as_slice(), identity].concat()
}
/// Verify the local key before writing or returning any credential from an existing database.
fn verify(h: &Header, key: &[u8; 32]) -> Result<()> {
    let plain = decrypt(key, &h.nonce, &h.check, &aad(&h.salt, CHECK))
        .context("Local credential data or encryption key is damaged")?;
    ensure!(
        plain.as_slice() == CHECK,
        "Invalid local credential database"
    );
    Ok(())
}
/// Validate fixed metadata lengths before loading blobs.
fn header(db: &Connection) -> Result<Option<Header>> {
    let sizes: Option<(i64, i64, i64)> = db
        .query_row(
            "SELECT length(salt),length(nonce),length(check_value) FROM vault_meta WHERE id=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let Some((s, n, c)) = sizes else {
        return Ok(None);
    };
    ensure!(
        s == 16 && n == 12 && c == (CHECK.len() + 16) as i64,
        "Unsupported or damaged local credential database"
    );
    let (s, n, c): (Vec<u8>, Vec<u8>, Vec<u8>) = db.query_row(
        "SELECT salt,nonce,check_value FROM vault_meta WHERE id=1",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    Ok(Some(Header {
        salt: s
            .try_into()
            .map_err(|_| anyhow::anyhow!("Invalid database identity"))?,
        nonce: n.try_into().map_err(|_| anyhow::anyhow!("Invalid nonce"))?,
        check: c,
    }))
}
/// Only authenticated ciphertext, with a fresh random nonce, reaches SQLite and its journals.
fn save_record(db: &Connection, key: &[u8; 32], h: &Header, id: Id, secret: &str) -> Result<()> {
    ensure!(
        !secret.is_empty() && secret.len() <= MAX_SECRET,
        "Credential is empty or too long"
    );
    let (nonce, encrypted) = encrypt(key, secret.as_bytes(), &aad(&h.salt, id.as_bytes()))?;
    db.execute("INSERT INTO vault_secrets(id,nonce,ciphertext) VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET nonce=excluded.nonce,ciphertext=excluded.ciphertext",params![id.to_string(),nonce.as_slice(),encrypted])?;
    Ok(())
}

/// Open only this vault with private creation permissions, bounded locking and durable transactions.
fn open(path: &Path, create: bool) -> Result<Connection> {
    if create {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(path) {
            Ok(_) => (),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(e) => return Err(e.into()),
        }
    }
    let metadata = fs::symlink_metadata(path).context("Cannot open local credential vault")?;
    ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "Vault must be a regular file"
    );
    let canonical = path
        .parent()
        .context("Vault directory missing")?
        .canonicalize()?
        .join(path.file_name().context("Vault filename missing")?);
    let db = Connection::open_with_flags(
        &canonical,
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?;
    db.busy_timeout(Duration::from_secs(3))?;
    db.execute_batch(
        "PRAGMA trusted_schema=OFF; PRAGMA synchronous=FULL; PRAGMA secure_delete=ON;",
    )?;
    if create {
        if metadata.len() > 0 {
            let tables: i64 = db.query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name IN ('vault_meta','vault_secrets')", [], |r| r.get(0))?;
            ensure!(
                tables == 2,
                "Existing file is not a supported password vault"
            );
        }
        db.execute_batch("BEGIN IMMEDIATE; CREATE TABLE IF NOT EXISTS vault_meta(id INTEGER PRIMARY KEY CHECK(id=1),salt BLOB NOT NULL,nonce BLOB NOT NULL,check_value BLOB NOT NULL); CREATE TABLE IF NOT EXISTS vault_secrets(id TEXT PRIMARY KEY,nonce BLOB NOT NULL,ciphertext BLOB NOT NULL); COMMIT;")?;
    }
    Ok(db)
}
/// Obtain fresh salt/nonce bytes directly from the operating system CSPRNG, with errors propagated.
fn random<const N: usize>() -> Result<[u8; N]> {
    let mut bytes = [0; N];
    getrandom::getrandom(&mut bytes)
        .map_err(|_| anyhow::anyhow!("Secure random generator unavailable"))?;
    Ok(bytes)
}
/// Authenticate before exposing plaintext; transient plaintext buffers are cleared when dropped.
fn decrypt(
    key: &[u8; 32],
    nonce: &[u8; 12],
    ciphertext: &[u8],
    aad: &[u8],
) -> Result<Zeroizing<Vec<u8>>> {
    let cipher =
        Aes256Gcm::new_from_slice(key).map_err(|_| anyhow::anyhow!("Invalid encryption key"))?;
    cipher
        .decrypt(
            &(*nonce).into(),
            Payload {
                msg: ciphertext,
                aad,
            },
        )
        .map(Zeroizing::new)
        .map_err(|_| anyhow::anyhow!("Local credential data or encryption key is damaged"))
}
/// Encrypt each write with a fresh 96-bit nonce; no plaintext is sent to SQLite or its journals.
fn encrypt(key: &[u8; 32], plaintext: &[u8], aad: &[u8]) -> Result<([u8; 12], Vec<u8>)> {
    let nonce = random()?;
    let cipher =
        Aes256Gcm::new_from_slice(key).map_err(|_| anyhow::anyhow!("Invalid encryption key"))?;
    let bytes = cipher
        .encrypt(
            &nonce.into(),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| anyhow::anyhow!("Credential encryption failed"))?;
    Ok((nonce, bytes))
}
