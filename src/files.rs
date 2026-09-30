//! SFTP operations with fixed paths, bounded editing and guarded replacement.
use crate::{
    encoding::{self, Encoding},
    model::*,
    ssh::Remote,
};
use anyhow::{Context, Result, bail};
use russh_sftp::{client::SftpSession, protocol::OpenFlags};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

pub const EDIT_LIMIT: u64 = 8_000_000;
#[derive(Debug, Clone)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub directory: bool,
    pub symlink: bool,
    pub size: Option<u64>,
    pub modified: Option<u32>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stamp {
    pub hash: [u8; 32],
    pub size: u64,
    pub modified: Option<u32>,
}
#[derive(Debug, Clone)]
pub struct OpenedFile {
    pub path: String,
    pub text: String,
    pub encoding: Encoding,
    pub bom: bool,
    pub stamp: Stamp,
}
#[derive(Debug, Clone)]
pub enum SaveResult {
    Saved(Stamp),
    Conflict,
}

/// Expand only a leading home-directory shorthand in a local user-selected path.
pub fn expand_home(path: &str) -> PathBuf {
    if path == "~" {
        crate::platform::home_directory().into()
    } else if let Some(rest) = path.strip_prefix("~/") {
        Path::new(&crate::platform::home_directory()).join(rest)
    } else {
        path.into()
    }
}
/// Keep server-provided names inside the selected directory, including on Windows.
pub fn valid_name(name: &str) -> Result<()> {
    if name.is_empty() || matches!(name, "." | "..") || name.contains(['/', '\\', '\0', '\r', '\n'])
    {
        bail!("Unsafe or unsupported file name: {name:?}");
    }
    Ok(())
}
/// Apply local filesystem rules only when downloading; valid remote names remain editable.
pub fn valid_download_name(name: &str, windows_rules: bool) -> Result<()> {
    valid_name(name)?;
    if windows_rules {
        let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
        let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || ["COM", "LPT"].iter().any(|prefix| {
                stem.strip_prefix(prefix).is_some_and(|n| {
                    matches!(n, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
                })
            });
        if reserved
            || name.ends_with(['.', ' '])
            || name.contains([':', '*', '?', '"', '<', '>', '|'])
        {
            bail!("This remote filename cannot be downloaded unchanged on Windows: {name:?}");
        }
    }
    Ok(())
}
/// Join POSIX SFTP paths after validating the last component.
pub fn join(directory: &str, name: &str) -> Result<String> {
    valid_name(name)?;
    Ok(format!("{}/{}", directory.trim_end_matches('/'), name))
}
/// Navigate up without platform-dependent local path rules.
pub fn parent(path: &str) -> String {
    let path = path.trim_end_matches('/');
    if path.is_empty() {
        return "/".into();
    }
    path.rsplit_once('/')
        .map(|(p, _)| if p.is_empty() { "/".into() } else { p.into() })
        .unwrap_or_else(|| ".".into())
}

/// Canonicalize the requested directory, preserving hidden files in the returned model.
pub async fn list(remote: &Remote, path: &str) -> Result<(String, Vec<FileEntry>)> {
    let sftp = remote.sftp().await?;
    let directory = sftp.canonicalize(path).await?;
    let mut entries = Vec::new();
    for entry in sftp.read_dir(&directory).await? {
        let name = entry.file_name();
        // One unrepresentable name (e.g. a stray backslash) must not sink the
        // whole listing; such entries cannot join into a usable path anyway,
        // so the listing skips them and keeps the rest readable.
        if valid_name(&name).is_err() {
            continue;
        }
        let metadata = entry.metadata();
        let kind = metadata.file_type();
        entries.push(FileEntry {
            path: join(&directory, &name)?,
            name,
            directory: kind.is_dir(),
            symlink: kind.is_symlink(),
            size: metadata.size,
            modified: metadata.mtime,
        });
    }
    entries.sort_by(|a, b| {
        b.directory
            .cmp(&a.directory)
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok((directory, entries))
}

/// Resolve a link for an explicit UI confirmation; this never opens the target's contents.
pub async fn link_target(remote: &Remote, path: &str) -> Result<Option<(String, bool)>> {
    let sftp = remote.sftp().await?;
    if !sftp.symlink_metadata(path).await?.file_type().is_symlink() {
        return Ok(None);
    }
    let target = sftp.canonicalize(path).await?;
    let kind = sftp.symlink_metadata(&target).await?.file_type();
    if !kind.is_file() && !kind.is_dir() {
        bail!("Symbolic link target is not a regular file or directory");
    }
    Ok(Some((target, kind.is_dir())))
}

async fn read_bounded(sftp: &SftpSession, path: &str) -> Result<(Vec<u8>, Stamp)> {
    let metadata = sftp.symlink_metadata(path).await?;
    let kind = metadata.file_type();
    if kind.is_symlink() {
        bail!("Open the symlink's real target explicitly before editing");
    }
    if !kind.is_file() {
        bail!("Only regular text files can be edited");
    }
    if metadata.size.is_some_and(|n| n > EDIT_LIMIT) {
        bail!("Text editor limit is 8 MB; download this file instead");
    }
    let mut bytes = Vec::new();
    sftp.open(path)
        .await?
        .take(EDIT_LIMIT + 1)
        .read_to_end(&mut bytes)
        .await?;
    if bytes.len() as u64 > EDIT_LIMIT {
        bail!("File grew beyond the 8 MB editor limit");
    }
    let stamp = Stamp {
        hash: Sha256::digest(&bytes).into(),
        size: bytes.len() as u64,
        modified: metadata.mtime,
    };
    Ok((bytes, stamp))
}

/// Read and decode strictly; invalid bytes and binary files never become editable replacement text.
pub async fn open(remote: &Remote, path: &str, requested: Option<Encoding>) -> Result<OpenedFile> {
    let sftp = remote.sftp().await?;
    let (bytes, stamp) = read_bounded(&sftp, path).await?;
    let text = encoding::decode_file(&bytes, requested)?;
    Ok(OpenedFile {
        path: path.into(),
        text: text.text,
        encoding: text.encoding,
        bom: text.bom,
        stamp,
    })
}

/// Replace a file using a prepared sibling. SFTP v3 lacks portable atomic overwrite;
/// the previous bytes remain in a uniquely named recovery sibling until replacement succeeds.
async fn replace(sftp: &SftpSession, temporary: &str, target: &str, overwrite: bool) -> Result<()> {
    if !sftp.try_exists(target).await? {
        sftp.rename(temporary, target).await?;
        return Ok(());
    }
    if !overwrite {
        bail!("Target already exists; confirm overwrite to replace it: {target}");
    }
    let metadata = sftp.symlink_metadata(target).await?;
    if !metadata.file_type().is_file() {
        bail!("Refusing to replace a directory, device, or symbolic link: {target}");
    }
    let prepared = sftp.symlink_metadata(temporary).await?;
    sftp.set_metadata(
        temporary,
        russh_sftp::protocol::FileAttributes {
            permissions: metadata.permissions.map(|mode| mode & 0o7777),
            uid: if prepared.uid != metadata.uid {
                metadata.uid
            } else {
                None
            },
            gid: if prepared.gid != metadata.gid {
                metadata.gid
            } else {
                None
            },
            ..Default::default()
        },
    )
    .await
    .context("Cannot preserve the target file's permissions or ownership")?;
    let backup = format!("{target}.mantash-recovery-{}", Id::new_v4());
    sftp.rename(target, &backup).await?;
    if let Err(error) = sftp.rename(temporary, target).await {
        if let Err(restore) = sftp.rename(&backup, target).await {
            bail!(
                "Replacement failed: {error}; original content remains at {backup}; restore failed: {restore}"
            );
        }
        return Err(error.into());
    }
    sftp.remove_file(&backup)
        .await
        .with_context(|| format!("Content saved, but recovery sibling remains at {backup}"))?;
    Ok(())
}

/// Save a document snapshot with last-write-wins semantics. The editor reads
/// the current target before replacement for validation, then atomically-ish
/// replaces it with the caller's latest content; an external save is therefore
/// overwritten by the later MantaSH save.
pub async fn save(
    remote: &Remote,
    document: &OpenedFile,
    text: &str,
    encoding: Encoding,
    _force: bool,
) -> Result<SaveResult> {
    let bytes = encoding::encode(text, encoding, document.bom)?;
    if bytes.len() as u64 > EDIT_LIMIT {
        bail!("Encoded file exceeds 8 MB");
    }
    let sftp = remote.sftp().await?;
    // Validate that the target is still a readable regular file, but do not
    // reject a newer stamp: the final save is the authoritative version.
    let _ = read_bounded(&sftp, &document.path).await?;
    let temporary = format!("{}.mantash-save-{}", document.path, Id::new_v4());
    let operation = async {
        let mut file = sftp
            .open_with_flags_and_attributes(
                &temporary,
                OpenFlags::CREATE | OpenFlags::EXCLUDE | OpenFlags::WRITE,
                russh_sftp::protocol::FileAttributes {
                    permissions: Some(0o600),
                    ..Default::default()
                },
            )
            .await?;
        file.write_all(&bytes).await?;
        file.flush().await?;
        file.close().await?;
        replace(&sftp, &temporary, &document.path, true).await?;
        let modified = sftp.metadata(&document.path).await?.mtime;
        Ok(SaveResult::Saved(Stamp {
            hash: Sha256::digest(&bytes).into(),
            size: bytes.len() as u64,
            modified,
        }))
    }
    .await;
    if !matches!(&operation, Ok(SaveResult::Saved(_))) {
        let _ = sftp.remove_file(&temporary).await;
    }
    operation
}

/// Validate a deletion root before any remote mutation begins.
pub fn validate_delete_target(path: &str) -> Result<()> {
    if !path.starts_with('/') || path.trim_end_matches('/').is_empty() {
        bail!("Refusing an unsafe deletion target: {path}");
    }
    for component in path.split('/').filter(|component| !component.is_empty()) {
        valid_name(component)?;
    }
    Ok(())
}

/// Collect a complete deletion plan without mutating the remote tree.
async fn preflight_delete(remote: &Remote, root: &str) -> Result<Vec<(String, bool)>> {
    let sftp = remote.sftp().await?;
    let mut pending = vec![root.to_owned()];
    let mut plan = Vec::new();
    while let Some(path) = pending.pop() {
        if remote.cancel.is_cancelled() {
            bail!("Connection closed during deletion preflight");
        }
        let kind = sftp.symlink_metadata(&path).await?.file_type();
        if kind.is_dir() {
            for entry in sftp.read_dir(&path).await? {
                let name = entry.file_name();
                valid_name(&name)?;
                pending.push(join(&path, &name)?);
            }
            plan.push((path, true));
        } else if kind.is_file() || kind.is_symlink() {
            plan.push((path, false));
        } else {
            bail!("Refusing to delete a special file: {path}");
        }
    }
    plan.reverse();
    Ok(plan)
}

/// Recursively delete only after the complete target tree has passed preflight.
pub async fn delete(remote: &Remote, paths: &[String]) -> Result<()> {
    let mut plan = Vec::new();
    for path in paths {
        validate_delete_target(path)?;
        plan.extend(preflight_delete(remote, path).await?);
    }
    if remote.cancel.is_cancelled() {
        bail!("Connection closed before deletion");
    }
    let sftp = remote.sftp().await?;
    for (path, directory) in plan {
        if remote.cancel.is_cancelled() {
            bail!("Connection closed during deletion");
        }
        if directory {
            sftp.remove_dir(&path).await?;
        } else {
            sftp.remove_file(&path).await?;
        }
    }
    Ok(())
}

/// Read a local regular file's size without blocking the UI or following symlinks.
pub async fn upload_file_size(path: &Path) -> Option<u64> {
    tokio::fs::symlink_metadata(path)
        .await
        .ok()
        .and_then(|metadata| metadata.is_file().then_some(metadata.len()))
}

/// Stream a directory tree with cancellation. Symlinks and special files are rejected explicitly.
pub async fn transfer(
    remote: Arc<Remote>,
    record: &mut TransferRecord,
    overwrite: bool,
    cancel: CancellationToken,
    report: impl Fn(&TransferRecord),
) -> Result<()> {
    let sftp = remote.sftp().await?;
    let mut pending = vec![(PathBuf::from(&record.local), record.remote.clone())];
    record.total = if record.upload {
        upload_file_size(Path::new(&record.local)).await
    } else {
        None
    };
    while let Some((local, remote_path)) = pending.pop() {
        if !record.upload {
            valid_download_name(remote_path.rsplit('/').next().unwrap_or(""), cfg!(windows))?;
        }
        if cancel.is_cancelled() || remote.cancel.is_cancelled() {
            bail!("Transfer cancelled; completed items are retained");
        }
        let directory = if record.upload {
            let metadata = tokio::fs::symlink_metadata(&local).await?;
            if metadata.is_symlink() || (!metadata.is_file() && !metadata.is_dir()) {
                bail!(
                    "Transfer does not follow symbolic links or devices: {}",
                    local.display()
                );
            }
            metadata.is_dir()
        } else {
            let kind = sftp.symlink_metadata(&remote_path).await?.file_type();
            if kind.is_symlink() || (!kind.is_file() && !kind.is_dir()) {
                bail!("Transfer does not follow symbolic links or devices: {remote_path}");
            }
            kind.is_dir()
        };
        if directory {
            if record.upload {
                if sftp.try_exists(&remote_path).await? {
                    if !sftp
                        .symlink_metadata(&remote_path)
                        .await?
                        .file_type()
                        .is_dir()
                    {
                        bail!("Target is not a regular directory: {remote_path}");
                    }
                } else {
                    sftp.create_dir(&remote_path).await?;
                }
                let mut entries = tokio::fs::read_dir(&local).await?;
                while let Some(entry) = entries.next_entry().await? {
                    let name = entry
                        .file_name()
                        .into_string()
                        .map_err(|_| anyhow::anyhow!("Filename is not Unicode"))?;
                    pending.push((entry.path(), join(&remote_path, &name)?));
                }
            } else {
                if let Ok(metadata) = tokio::fs::symlink_metadata(&local).await {
                    if metadata.is_symlink() || !metadata.is_dir() {
                        bail!(
                            "Download target is not a regular directory: {}",
                            local.display()
                        );
                    }
                }
                tokio::fs::create_dir_all(&local).await?;
                for entry in sftp.read_dir(&remote_path).await? {
                    let name = entry.file_name();
                    valid_download_name(&name, cfg!(windows))?;
                    pending.push((local.join(&name), join(&remote_path, &name)?));
                }
            }
            continue;
        }
        if record.upload {
            let temporary = format!("{remote_path}.mantash-transfer-{}", record.id);
            let result: Result<()> = async {
                let mut input = tokio::fs::File::open(&local).await?;
                let mut output = sftp.open_with_flags_and_attributes(&temporary, OpenFlags::CREATE | OpenFlags::EXCLUDE | OpenFlags::WRITE, russh_sftp::protocol::FileAttributes { permissions: Some(0o600), ..Default::default() }).await?;
                let mut buffer = vec![0u8; 64 * 1024];
                loop {
                    let count = tokio::select! { _ = cancel.cancelled() => bail!("Transfer cancelled"), _ = remote.cancel.cancelled() => bail!("Connection closed"), r = input.read(&mut buffer) => r? };
                    if count == 0 { break; }
                    tokio::select! { _ = cancel.cancelled() => bail!("Transfer cancelled"), r = output.write_all(&buffer[..count]) => r? }
                    record.bytes += count as u64; report(record);
                }
                output.flush().await?; output.close().await?;
                if cancel.is_cancelled() { bail!("Transfer cancelled"); }
                #[cfg(unix)] {
                    use std::os::unix::fs::PermissionsExt;
                    let mode = tokio::fs::metadata(&local).await?.permissions().mode() & 0o777;
                    sftp.set_metadata(&temporary, russh_sftp::protocol::FileAttributes { permissions: Some(mode), ..Default::default() }).await?;
                }
                replace(&sftp, &temporary, &remote_path, overwrite).await
            }.await;
            if result.is_err() {
                let _ = sftp.remove_file(&temporary).await;
            }
            result?;
        } else {
            if let Ok(metadata) = tokio::fs::symlink_metadata(&local).await {
                if metadata.is_symlink() || !metadata.is_file() || !overwrite {
                    bail!("Download target exists or is unsafe: {}", local.display());
                }
            }
            let temporary = local.with_file_name(format!(
                "{}.mantash-transfer-{}",
                local.file_name().unwrap_or_default().to_string_lossy(),
                record.id
            ));
            let result: Result<()> = async {
                let mut input = sftp.open(&remote_path).await?;
                let mut options = tokio::fs::OpenOptions::new(); options.create_new(true).write(true);
                #[cfg(unix)] options.mode(0o600);
                let mut output = options.open(&temporary).await?;
                let mut buffer = vec![0u8; 64 * 1024];
                loop {
                    let count = tokio::select! { _ = cancel.cancelled() => bail!("Transfer cancelled"), _ = remote.cancel.cancelled() => bail!("Connection closed"), r = input.read(&mut buffer) => r? };
                    if count == 0 { break; }
                    output.write_all(&buffer[..count]).await?;
                    record.bytes += count as u64; report(record);
                }
                output.flush().await?; output.sync_all().await?; drop(output);
                if cancel.is_cancelled() { bail!("Transfer cancelled"); }
                if local.exists() && !overwrite { bail!("Target appeared during download: {}", local.display()); }
                if let Ok(existing) = tokio::fs::symlink_metadata(&local).await {
                    if existing.is_symlink() || !existing.is_file() { bail!("Download target became unsafe: {}", local.display()); }
                    tokio::fs::set_permissions(&temporary, existing.permissions()).await?;
                }
                #[cfg(windows)] if local.exists() {
                    let backup = local.with_extension(format!("mantash-recovery-{}", Id::new_v4()));
                    tokio::fs::rename(&local, &backup).await?;
                    if let Err(error) = tokio::fs::rename(&temporary, &local).await { let _ = tokio::fs::rename(&backup, &local).await; return Err(error.into()); }
                    tokio::fs::remove_file(backup).await?;
                } else { tokio::fs::rename(&temporary, &local).await?; }
                #[cfg(not(windows))] tokio::fs::rename(&temporary, &local).await?;
                Ok(())
            }.await;
            if result.is_err() {
                let _ = tokio::fs::remove_file(&temporary).await;
            }
            result?;
        }
    }
    record.total = Some(record.bytes);
    Ok(())
}
