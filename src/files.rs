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
    /// POSIX type and permission bits returned by this listing, if supplied by the server.
    pub permissions: Option<u32>,
}
/// Placeholder shown in the file list when the server did not report a modification time.
pub const UNKNOWN_MODIFIED: &str = "—";

/// Format an SFTP `mtime` (seconds since the Unix epoch) for the file list in local time.
///
/// SFTP v3 attributes carry `mtime` as an unsigned 32-bit second count; a missing value
/// (the server omitted `ACMODTIME`) renders as [`UNKNOWN_MODIFIED`] instead of a guessed date.
pub fn format_modified(mtime: Option<u32>) -> String {
    format_modified_in(mtime, &chrono::Local)
}

/// [`format_modified`] for an explicit time zone, as `YYYY-MM-DD HH:mm:ss` (seconds shown, not rounded).
pub fn format_modified_in<Tz>(mtime: Option<u32>, zone: &Tz) -> String
where
    Tz: chrono::TimeZone,
    Tz::Offset: std::fmt::Display,
{
    mtime
        .and_then(|seconds| chrono::DateTime::from_timestamp(i64::from(seconds), 0))
        .map(|date| {
            date.with_timezone(zone)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        })
        .unwrap_or_else(|| UNKNOWN_MODIFIED.into())
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

/// Render the server's mode, including file type and setuid/setgid/sticky bits.
pub fn permission_text(mode: Option<u32>) -> String {
    let Some(mode) = mode else {
        return "—".into();
    };
    let mut text = String::with_capacity(10);
    text.push(match mode & 0o170000 {
        0o040000 => 'd',
        0o120000 => 'l',
        0o020000 => 'c',
        0o060000 => 'b',
        0o010000 => 'p',
        0o140000 => 's',
        _ => '-',
    });
    for (shift, special, lower, upper) in [
        (6, 0o4000, 's', 'S'),
        (3, 0o2000, 's', 'S'),
        (0, 0o1000, 't', 'T'),
    ] {
        let bits = (mode >> shift) & 7;
        text.push(if bits & 4 != 0 { 'r' } else { '-' });
        text.push(if bits & 2 != 0 { 'w' } else { '-' });
        text.push(if mode & special != 0 {
            if bits & 1 != 0 { lower } else { upper }
        } else if bits & 1 != 0 {
            'x'
        } else {
            '-'
        });
    }
    text
}

/// A copy source freezes its path and type; the backend rechecks both before copying.
#[derive(Clone, Debug)]
pub struct CopySource {
    pub path: String,
    pub directory: bool,
    pub symlink: bool,
}

/// A lexical containment test for canonical POSIX paths, preserving component boundaries.
pub fn contains_path(parent: &str, child: &str) -> bool {
    parent == child
        || child
            .strip_prefix(parent.trim_end_matches('/'))
            .is_some_and(|rest| rest.starts_with('/'))
}

/// Validate the entire copy batch before executing the verified session's quoted POSIX cp.
pub async fn copy(remote: &Remote, sources: &[CopySource], target: &str) -> Result<()> {
    if sources.is_empty() {
        bail!("No copy sources");
    }
    let sftp = remote.sftp().await?;
    if !sftp.symlink_metadata(target).await?.file_type().is_dir() {
        bail!("Copy target is not a regular directory");
    }
    let target = sftp.canonicalize(target).await?;
    let mut destinations = std::collections::HashSet::new();
    for source in sources {
        validate_delete_target(&source.path)?;
        let kind = sftp.symlink_metadata(&source.path).await?.file_type();
        if source.symlink
            || kind.is_symlink()
            || (!kind.is_file() && !kind.is_dir())
            || kind.is_dir() != source.directory
        {
            bail!(
                "Copy source type changed or is unsupported: {}",
                source.path
            );
        }
        let canonical = sftp.canonicalize(&source.path).await?;
        if source.directory && contains_path(&canonical, &target) {
            bail!("A directory cannot be copied into itself or its descendants");
        }
        let destination = join(&target, source.path.rsplit('/').next().unwrap_or(""))?;
        if !destinations.insert(destination.clone()) || sftp.try_exists(&destination).await? {
            bail!("Copy target already exists: {destination}");
        }
        let mut pending = vec![source.path.clone()];
        while let Some(path) = pending.pop() {
            if remote.cancel.is_cancelled() {
                bail!("Connection closed during copy preflight");
            }
            let kind = sftp.symlink_metadata(&path).await?.file_type();
            if kind.is_symlink() || (!kind.is_file() && !kind.is_dir()) {
                bail!("Copy does not follow symbolic links or special files: {path}");
            }
            if kind.is_dir() {
                for entry in sftp.read_dir(&path).await? {
                    pending.push(join(&path, &entry.file_name())?);
                }
            }
        }
    }
    if remote.cancel.is_cancelled() {
        bail!("Connection closed before copy");
    }
    let mut command = String::from("cp -R --");
    for source in sources {
        command.push(' ');
        command.push_str(&crate::services::shell_quote(&source.path));
    }
    command.push(' ');
    command.push_str(&crate::services::shell_quote(&target));
    remote.run(&command).await
}

/// Validate source type, same-directory naming and destination collision in the worker.
pub async fn rename(remote: &Remote, from: &str, to: &str) -> Result<()> {
    validate_delete_target(from)?;
    validate_delete_target(to)?;
    if parent(from) != parent(to) {
        bail!("Rename must stay in the same directory");
    }
    let sftp = remote.sftp().await?;
    let kind = sftp.symlink_metadata(from).await?.file_type();
    if !kind.is_file() && !kind.is_dir() && !kind.is_symlink() {
        bail!("Cannot rename a special file");
    }
    if sftp.try_exists(to).await? {
        bail!("Rename target already exists");
    }
    if remote.cancel.is_cancelled() {
        bail!("Connection closed before rename");
    }
    sftp.rename(from, to).await?;
    Ok(())
}

/// Why a move batch is refused before any remote entry changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MoveRejection {
    /// Nothing was marked for moving.
    Empty,
    /// A source or the target is not an absolute path of valid components.
    InvalidPath(String),
    /// The target is the directory the sources already live in.
    SameDirectory,
    /// A directory would move into itself or one of its descendants.
    IntoItself(String),
    /// Two sources share a name, so both would land on one destination.
    DuplicateName(String),
    /// The destination already holds an entry with this name; it is never overwritten.
    Exists(String),
    /// The source vanished, changed type, or is a special file since it was marked.
    SourceChanged(String),
    /// The target is not a directory, or not the same directory once resolved.
    TargetChanged,
}
impl std::fmt::Display for MoveRejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "No move sources"),
            Self::InvalidPath(path) => write!(f, "Unsafe or unsupported move path: {path:?}"),
            Self::SameDirectory => write!(f, "Move target is the source directory"),
            Self::IntoItself(path) => write!(
                f,
                "A directory cannot be moved into itself or its descendants: {path}"
            ),
            Self::DuplicateName(name) => write!(f, "Two move sources share the name {name:?}"),
            Self::Exists(path) => write!(f, "Move target already exists: {path}"),
            Self::SourceChanged(path) => {
                write!(f, "Move source type changed or is unsupported: {path}")
            }
            Self::TargetChanged => write!(f, "Move target is not a regular directory"),
        }
    }
}
impl std::error::Error for MoveRejection {}

/// Accept an absolute POSIX directory path, including `/`, made of valid components.
fn valid_directory(path: &str) -> bool {
    path.starts_with('/')
        && path
            .split('/')
            .filter(|component| !component.is_empty())
            .all(|component| valid_name(component).is_ok())
}

/// Plan a move batch lexically: one `(from, to)` pair per source, in batch order.
///
/// This pure check runs both in the UI (before submitting) and in the worker (before any
/// rename). It rejects empty batches, invalid paths, a target equal to the sources' own
/// directory, a directory moving into itself or a descendant, and duplicate destination
/// names. Existing-name conflicts need the destination listing; see [`move_conflicts`].
pub fn plan_move(
    sources: &[CopySource],
    target: &str,
) -> std::result::Result<Vec<(String, String)>, MoveRejection> {
    if sources.is_empty() {
        return Err(MoveRejection::Empty);
    }
    if !valid_directory(target) {
        return Err(MoveRejection::InvalidPath(target.into()));
    }
    let target_key = if target.trim_end_matches('/').is_empty() {
        "/"
    } else {
        target.trim_end_matches('/')
    };
    let mut plan: Vec<(String, String)> = Vec::with_capacity(sources.len());
    for source in sources {
        if validate_delete_target(&source.path).is_err() {
            return Err(MoveRejection::InvalidPath(source.path.clone()));
        }
        let from = source.path.trim_end_matches('/');
        if parent(from) == target_key {
            return Err(MoveRejection::SameDirectory);
        }
        if contains_path(from, target_key) {
            return Err(MoveRejection::IntoItself(from.into()));
        }
        let name = from.rsplit('/').next().unwrap_or("");
        let to = join(target_key, name).map_err(|_| MoveRejection::InvalidPath(from.into()))?;
        if plan.iter().any(|(_, existing)| existing == &to) {
            return Err(MoveRejection::DuplicateName(name.into()));
        }
        plan.push((from.into(), to));
    }
    Ok(plan)
}

/// Destinations of a planned move that already exist, as reported by `exists`.
///
/// The UI passes the destination listing (hidden entries included); the worker repeats the
/// check against the server immediately before renaming, so nothing is ever overwritten.
pub fn move_conflicts<'a>(
    plan: &'a [(String, String)],
    exists: impl Fn(&str) -> bool,
) -> Vec<&'a str> {
    plan.iter()
        .map(|(_, to)| to.as_str())
        .filter(|to| exists(to))
        .collect()
}

/// Per-entry result of an executed move batch.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MoveReport {
    /// `(from, to)` pairs that the server renamed.
    pub moved: Vec<(String, String)>,
    /// `(from, error)` for entries that were not moved after the batch passed preflight.
    pub failed: Vec<(String, String)>,
}

/// `lstat`-based existence, so a dangling symlink still counts as an occupied name.
async fn entry_exists(sftp: &SftpSession, path: &str) -> Result<bool> {
    match sftp.symlink_metadata(path).await {
        Ok(_) => Ok(true),
        Err(russh_sftp::client::error::Error::Status(status))
            if status.status_code == russh_sftp::protocol::StatusCode::NoSuchFile =>
        {
            Ok(false)
        }
        Err(error) => Err(error.into()),
    }
}

/// Move marked entries into `target` with SFTP RENAME, never overwriting.
///
/// The whole batch is preflighted first (lexical plan, frozen types, resolved containment,
/// destination collisions); any rejection returns `Err` with nothing changed. Each entry is
/// then renamed independently and re-checked for collisions just before its rename, so a
/// later failure (permissions, cross-filesystem, race) is reported per entry in the
/// [`MoveReport`] instead of hiding earlier successes. Preflight and rename are not atomic.
pub async fn move_entries(
    remote: &Remote,
    sources: &[CopySource],
    target: &str,
) -> Result<MoveReport> {
    let plan = plan_move(sources, target)?;
    let sftp = remote.sftp().await?;
    if !sftp.symlink_metadata(target).await?.file_type().is_dir() {
        return Err(MoveRejection::TargetChanged.into());
    }
    let resolved_target = sftp.canonicalize(target).await?;
    for (source, (from, to)) in sources.iter().zip(&plan) {
        if remote.cancel.is_cancelled() {
            bail!("Connection closed during move preflight");
        }
        let kind = match sftp.symlink_metadata(from.as_str()).await {
            Ok(metadata) => metadata.file_type(),
            Err(_) => return Err(MoveRejection::SourceChanged(from.clone()).into()),
        };
        if (!kind.is_file() && !kind.is_dir() && !kind.is_symlink())
            || kind.is_symlink() != source.symlink
            || kind.is_dir() != source.directory
        {
            return Err(MoveRejection::SourceChanged(from.clone()).into());
        }
        // Resolve the parent (never the entry itself, which may be a symlink) so a
        // differently spelled path to the same directory is still a no-op move.
        if sftp.canonicalize(parent(from)).await? == resolved_target {
            return Err(MoveRejection::SameDirectory.into());
        }
        if kind.is_dir()
            && contains_path(&sftp.canonicalize(from.as_str()).await?, &resolved_target)
        {
            return Err(MoveRejection::IntoItself(from.clone()).into());
        }
        if entry_exists(&sftp, to).await? {
            return Err(MoveRejection::Exists(to.clone()).into());
        }
    }
    let mut report = MoveReport::default();
    for (from, to) in plan {
        if remote.cancel.is_cancelled() {
            report
                .failed
                .push((from, "Connection closed before move".into()));
            continue;
        }
        let result = async {
            if entry_exists(&sftp, &to).await? {
                return Err(anyhow::Error::from(MoveRejection::Exists(to.clone())));
            }
            sftp.rename(from.as_str(), to.as_str()).await?;
            Ok(())
        }
        .await;
        match result {
            Ok(()) => report.moved.push((from, to)),
            Err(error) => report.failed.push((from, format!("{error:#}"))),
        }
    }
    Ok(report)
}

/// Build one listing row from the attributes the same READDIR reply carried.
///
/// Size, modification time and permission bits are copied as reported; a field the server
/// omitted stays `None` so the list shows a placeholder rather than an invented value.
pub fn listing_entry(
    directory: &str,
    name: String,
    metadata: &russh_sftp::protocol::FileAttributes,
) -> Result<FileEntry> {
    let kind = metadata.file_type();
    Ok(FileEntry {
        path: join(directory, &name)?,
        name,
        directory: kind.is_dir(),
        symlink: kind.is_symlink(),
        size: metadata.size,
        modified: metadata.mtime,
        permissions: metadata.permissions,
    })
}

/// A file-list column whose header can change the display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortColumn {
    Size,
    Modified,
}

/// Display order of one file list. Folders always stay grouped before files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FileSort {
    /// Folders first, then case-insensitive name: the order a listing arrives in.
    #[default]
    Default,
    /// Sort by `column`; `descending` puts the largest or newest first.
    By {
        column: SortColumn,
        descending: bool,
    },
}

impl FileSort {
    /// Header click cycle: descending, then ascending, then back to the default order.
    pub fn toggled(self, column: SortColumn) -> Self {
        match self {
            Self::By {
                column: current,
                descending: true,
            } if current == column => Self::By {
                column,
                descending: false,
            },
            Self::By {
                column: current,
                descending: false,
            } if current == column => Self::Default,
            _ => Self::By {
                column,
                descending: true,
            },
        }
    }
    /// `Some(descending)` when `column` drives the current order, for the header arrow.
    pub fn direction(self, column: SortColumn) -> Option<bool> {
        match self {
            Self::By {
                column: current,
                descending,
            } if current == column => Some(descending),
            _ => None,
        }
    }
}

/// Unknown values sort after known ones in either direction; only known values reverse.
fn known_first<T: Ord>(a: Option<T>, b: Option<T>, descending: bool) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a, b) {
        (Some(a), Some(b)) if descending => b.cmp(&a),
        (Some(a), Some(b)) => a.cmp(&b),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

/// Order listing rows in place for display.
///
/// Folders precede files under every order. Within each group the chosen column decides,
/// with unknown sizes or times last in both directions; folders carry no size, so a size
/// order leaves them by name. Remaining ties fall back to the default name order
/// (case-insensitive, then exact), which keeps the result independent of the prior order.
pub fn sort_entries(entries: &mut [FileEntry], sort: FileSort) {
    entries.sort_by(|a, b| {
        b.directory
            .cmp(&a.directory)
            .then_with(|| match sort {
                FileSort::Default => std::cmp::Ordering::Equal,
                FileSort::By {
                    column: SortColumn::Size,
                    ..
                } if a.directory => std::cmp::Ordering::Equal,
                FileSort::By {
                    column: SortColumn::Size,
                    descending,
                } => known_first(a.size, b.size, descending),
                FileSort::By {
                    column: SortColumn::Modified,
                    descending,
                } => known_first(a.modified, b.modified, descending),
            })
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.name.cmp(&b.name))
    });
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
        entries.push(listing_entry(&directory, name, &entry.metadata())?);
    }
    sort_entries(&mut entries, FileSort::Default);
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
