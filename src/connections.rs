//! Five-column connection CSV with bounded previews and explicit credential import.
use crate::{
    credentials::SecretStore,
    model::{Id, Profile},
    storage::Database,
};
use anyhow::{Result, bail, ensure};
use std::{collections::HashMap, io::Read, path::Path};
use zeroize::{Zeroize, Zeroizing};

pub fn matches_query(profile: &Profile, query: &str) -> bool {
    let text = format!("{} {}", profile.name, profile.endpoint()).to_lowercase();
    query
        .split_whitespace()
        .all(|term| text.contains(&term.to_lowercase()))
}
pub fn clone_profile(profile: &Profile, name: String) -> Profile {
    Profile {
        id: Id::new_v4(),
        name,
        ..profile.clone()
    }
}

#[derive(Clone)]
pub struct ImportRow {
    pub row: usize,
    pub profile: Option<Profile>,
    pub password: Option<String>,
    pub duplicate: Option<Id>,
    pub error: Option<String>,
}
impl std::fmt::Debug for ImportRow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImportRow")
            .field("row", &self.row)
            .field("profile", &self.profile)
            .field("has_password", &self.password.is_some())
            .field("duplicate", &self.duplicate)
            .field("error", &self.error)
            .finish()
    }
}
impl Drop for ImportRow {
    fn drop(&mut self) {
        if let Some(password) = &mut self.password {
            password.zeroize();
        }
    }
}
#[derive(Debug, Clone)]
pub struct ImportPreview {
    pub rows: Vec<ImportRow>,
}

/// Only regular UTF-8 files are accepted; the read stays bounded if a file grows after stat.
pub fn preview_file(path: &Path, existing: &[Profile]) -> Result<ImportPreview> {
    let file = std::fs::File::open(path)?;
    ensure!(
        file.metadata()?.is_file(),
        "Connection import requires a regular file"
    );
    let mut bytes = Zeroizing::new(Vec::new());
    file.take(8_000_001).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= 8_000_000,
        "Connection import exceeds the 8 MB limit"
    );
    let text =
        std::str::from_utf8(&bytes).map_err(|_| anyhow::anyhow!("Connection CSV must be UTF-8"))?;
    preview(text, existing)
}

/// Diagnostics never include a header, cell value or parser error that may contain a password.
pub fn preview(text: &str, existing: &[Profile]) -> Result<ImportPreview> {
    ensure!(
        text.len() <= 8_000_000,
        "Connection import exceeds the 8 MB limit"
    );
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .from_reader(text.strip_prefix('\u{feff}').unwrap_or(text).as_bytes());
    let header = reader
        .headers()
        .map_err(|_| anyhow::anyhow!("Invalid connection CSV header"))?
        .clone();
    let expected = ["name", "host", "port", "username", "password"];
    ensure!(
        header.len() == 5,
        "Connection CSV requires exactly name, host, port, username, password"
    );
    let mut indices = [usize::MAX; 5];
    for (position, column) in header.iter().enumerate() {
        let Some(index) = expected
            .iter()
            .position(|name| name.eq_ignore_ascii_case(column.trim()))
        else {
            bail!("Unsupported connection CSV column");
        };
        ensure!(
            indices[index] == usize::MAX,
            "Duplicate connection CSV column"
        );
        indices[index] = position;
    }
    let mut seen = existing.to_vec();
    let mut rows = Vec::new();
    for (index, record) in reader.records().enumerate() {
        let mut row = ImportRow {
            row: index + 1,
            profile: None,
            password: None,
            duplicate: None,
            error: None,
        };
        let record = match record {
            Ok(record) if record.len() == 5 => record,
            _ => {
                row.error = Some("Invalid CSV record; expected five fields".into());
                rows.push(row);
                continue;
            }
        };
        let field = |index: usize| record.get(indices[index]).unwrap_or("");
        let port = match field(2).trim().parse::<u16>() {
            Ok(port) if port != 0 => port,
            _ => {
                row.error = Some("Port must be between 1 and 65535".into());
                rows.push(row);
                continue;
            }
        };
        let profile = Profile {
            id: Id::new_v4(),
            name: field(0).trim().into(),
            host: field(1).trim().into(),
            port,
            username: field(3).trim().into(),
        };
        if profile.validate().is_err() {
            row.error = Some("Invalid connection name, host or username".into());
        } else {
            row.duplicate = seen.iter().find(|p| p.duplicates(&profile)).map(|p| p.id);
            if row.duplicate.is_none() {
                seen.push(profile.clone());
            }
            row.password = (!field(4).is_empty()).then(|| field(4).to_string());
            row.profile = Some(profile);
        }
        rows.push(row);
    }
    Ok(ImportPreview { rows })
}

/// Re-evaluate duplicate identities against the latest committed list, not the preview snapshot.
fn merge_plan(
    existing: &[Profile],
    preview: &ImportPreview,
    replace: bool,
) -> (Vec<Profile>, PasswordRetry) {
    let mut output = existing.to_vec();
    let mut secrets = HashMap::new();
    for row in &preview.rows {
        let Some(profile) = row.profile.as_ref().filter(|_| row.error.is_none()) else {
            continue;
        };
        let id = if let Some(index) = output.iter().position(|p| p.duplicates(profile)) {
            if !replace {
                continue;
            }
            let id = output[index].id;
            output[index] = Profile {
                id,
                ..profile.clone()
            };
            id
        } else {
            output.push(profile.clone());
            profile.id
        };
        if let Some(password) = &row.password {
            secrets.insert(id, Zeroizing::new(password.clone()));
        }
    }
    (output, PasswordRetry(secrets.into_iter().collect()))
}
pub fn merge(existing: &[Profile], preview: &ImportPreview, replace: bool) -> Vec<Profile> {
    merge_plan(existing, preview, replace).0
}

/// Retains only failed explicit password writes, with no Debug or serialization surface.
#[derive(Default, Clone)]
pub struct PasswordRetry(Vec<(Id, Zeroizing<String>)>);
impl PasswordRetry {
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}
pub struct ImportResult {
    pub profiles: Vec<Profile>,
    pub failed: PasswordRetry,
}

/// Commit public metadata first. No credential operation runs if the transaction fails.
pub fn import_confirmed(
    db: &mut Database,
    store: &dyn SecretStore,
    preview: &ImportPreview,
    replace: bool,
) -> Result<ImportResult> {
    let (profiles, pending) = merge_plan(&db.profiles()?, preview, replace);
    db.save_profiles(&profiles)?;
    retry_passwords(db, store, pending)
}
/// Retrying credentials cannot replay an old profile snapshot or recreate deleted identities.
pub fn retry_passwords(
    db: &Database,
    store: &dyn SecretStore,
    pending: PasswordRetry,
) -> Result<ImportResult> {
    let profiles = db.profiles()?;
    let mut failed = Vec::new();
    for (id, secret) in pending.0 {
        if profiles.iter().any(|profile| profile.id == id) && store.write(id, &secret).is_err() {
            failed.push((id, secret));
        }
    }
    Ok(ImportResult {
        profiles,
        failed: PasswordRetry(failed),
    })
}

/// Caller writes the result as a sensitive UTF-8 BOM file after explicit export selection.
pub fn export(profiles: &[Profile], secrets: &HashMap<Id, String>) -> Result<String> {
    let mut writer = csv::Writer::from_writer(Vec::new());
    writer.write_record(["name", "host", "port", "username", "password"])?;
    for p in profiles {
        writer.write_record([
            p.name.as_str(),
            p.host.as_str(),
            &p.port.to_string(),
            p.username.as_str(),
            secrets.get(&p.id).map(String::as_str).unwrap_or(""),
        ])?;
    }
    Ok(String::from_utf8(writer.into_inner()?)?)
}

/// Export with BOM and private file permissions; any unavailable credential aborts the export.
pub fn export_file(path: &Path, profiles: &[Profile], store: &dyn SecretStore) -> Result<()> {
    use std::io::Write;
    struct Secrets(HashMap<Id, String>);
    impl Drop for Secrets {
        fn drop(&mut self) {
            for secret in self.0.values_mut() {
                secret.zeroize();
            }
        }
    }
    let mut secrets = Secrets(HashMap::new());
    for profile in profiles {
        if let Some(secret) = store.read(profile.id)? {
            secrets.0.insert(profile.id, secret.to_string());
        }
    }
    let text = Zeroizing::new(export(profiles, &secrets.0)?);
    if let Ok(meta) = std::fs::symlink_metadata(path) {
        ensure!(
            meta.is_file() && !meta.file_type().is_symlink(),
            "Export target must be a regular file"
        );
    }
    let directory = path.parent().unwrap_or_else(|| Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temporary
            .as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    temporary.write_all(b"\xef\xbb\xbf")?;
    temporary.write_all(text.as_bytes())?;
    temporary.as_file().sync_all()?;
    temporary.persist(path)?;
    Ok(())
}
