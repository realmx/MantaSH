//! Frozen file targets and operation receipts, independent from directory-list requests.
use super::commits::AfterCommit;
use super::*;
use crate::{files, services::FileOperation};

#[derive(Clone)]
pub(super) struct FileSnapshot {
    pub owner: Owner,
    pub directory: String,
    pub listing: Option<Id>,
    hidden: bool,
    pub paths: Vec<String>,
}
impl FileSnapshot {
    pub fn capture(pane: &Pane, paths: Vec<String>) -> Self {
        Self {
            owner: pane.owner,
            directory: pane.files.path.clone(),
            listing: pane.files.request,
            hidden: pane.files.show_hidden,
            paths,
        }
    }
    pub fn matches(&self, pane: &Pane) -> bool {
        pane.owner == self.owner
            && pane.state == ConnectionState::Connected
            && !pane.files.loading
            && pane.files.loaded
            && pane.files.path == self.directory
            && pane.files.request == self.listing
            && pane.files.show_hidden == self.hidden
            && self.paths.iter().all(|path| {
                pane.files.entries.iter().any(|entry| {
                    &entry.path == path && (self.hidden || !entry.name.starts_with('.'))
                })
            })
    }
}
pub(super) struct PendingFileOperation {
    pub origin: FileSnapshot,
    pub operation: FileOperation,
    /// The marked buffer this move submitted; later marks have a different ID.
    pub clipboard_id: Option<Id>,
}
impl PendingFileOperation {
    /// A successful move consumes only its own buffer; rename retains its existing
    /// invalidation behavior. A fully refused operation leaves the buffer available.
    fn update_clipboard(&self, clipboard: &mut Option<FileClipboard>, moved: bool) {
        if moved
            && match &self.operation {
                FileOperation::Move(..) => clipboard
                    .as_ref()
                    .is_some_and(|current| Some(current.id) == self.clipboard_id),
                FileOperation::Rename(..) => true,
                _ => false,
            }
        {
            *clipboard = None;
        }
    }
}
/// What a paste does with the marked entries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ClipboardMode {
    /// Duplicate with the remote POSIX `cp -R`.
    Copy,
    /// Relocate with SFTP RENAME; the buffer is consumed once anything moved.
    Move,
}
/// One session's marked entries; a single buffer, so marking a move replaces a copy.
#[derive(Clone)]
pub(super) struct FileClipboard {
    pub id: Id,
    pub owner: Owner,
    pub mode: ClipboardMode,
    pub sources: Vec<files::CopySource>,
}
/// Applied only to the listing `request` a finished move started: select what landed
/// there and keep a per-entry failure report visible after the refresh.
pub(super) struct ListingFollowUp {
    pub request: Id,
    pub select: Vec<String>,
    pub error: Option<String>,
}
#[derive(Clone, Copy)]
pub(super) enum FileAction {
    Preview,
    Rename,
    Copy,
    Move,
    Download,
    Delete,
}

/// `(from, to)` prefixes an operation relocates. Reads, saves and draft paths under
/// either side wait for its receipt, exactly as a rename does.
pub(super) fn relocations(operation: &FileOperation) -> Vec<(String, String)> {
    match operation {
        FileOperation::Rename(from, to) => vec![(from.clone(), to.clone())],
        FileOperation::Move(sources, target) => {
            files::plan_move(sources, target).unwrap_or_default()
        }
        _ => Vec::new(),
    }
}

impl Workbench {
    /// Submit the visible file confirmation; the frozen origin is revalidated below.
    pub(super) fn confirm_file_operation(&mut self, cx: &mut Context<Self>) {
        let (origin, operation) = match &self.modal {
            Some(Modal::FileName {
                directory,
                original,
                input,
                origin,
                ..
            }) => {
                let path = match files::join(directory, &input.read(cx).value()) {
                    Ok(path) => path,
                    Err(error) => {
                        self.notice = Some(error.to_string());
                        cx.notify();
                        return;
                    }
                };
                let operation = original.as_ref().map_or_else(
                    || FileOperation::Mkdir(path.clone()),
                    |old| FileOperation::Rename(old.clone(), path.clone()),
                );
                (origin.clone(), operation)
            }
            Some(Modal::DeleteFiles { origin, paths, .. }) => {
                (origin.clone(), FileOperation::Delete(paths.clone()))
            }
            _ => return,
        };
        self.submit_file_operation(origin, operation, true, cx);
    }
    /// A rename or move owns both path prefixes until its real receipt. Reads/saves cannot race it.
    pub(super) fn file_path_busy(&self, owner: Owner, path: &str) -> bool {
        self.pane(owner).is_some_and(|pane| {
            pane.files.pending.values().any(|pending| {
                relocations(&pending.operation).iter().any(|(from, to)| {
                    files::contains_path(from, path) || files::contains_path(to, path)
                })
            })
        })
    }
    pub(super) fn file_direct_action(
        &mut self,
        owner: Owner,
        listing: Option<Id>,
        path: String,
        action: FileAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pane) = self
            .pane(owner)
            .filter(|pane| self.active_owner() == Some(owner) && pane.files.request == listing)
        else {
            return;
        };
        let origin = FileSnapshot::capture(pane, vec![path.clone()]);
        if !origin.matches(pane) {
            return;
        }
        match action {
            FileAction::Preview => {
                self.open_document(owner, path, crate::files::OpenIntent::Preview, window, cx)
            }
            FileAction::Rename => self.file_name_target(origin, Some(path), window, cx),
            FileAction::Copy => self.mark_file_targets(owner, vec![path], ClipboardMode::Copy, cx),
            FileAction::Move => self.mark_file_targets(owner, vec![path], ClipboardMode::Move, cx),
            FileAction::Download => {
                self.choose_transfer_targets(owner, false, false, Some(vec![path]), window, cx)
            }
            FileAction::Delete => self.show_modal(
                Modal::DeleteFiles {
                    owner,
                    paths: vec![path],
                    origin,
                },
                window,
                cx,
            ),
        }
    }
    pub(super) fn file_name_target(
        &mut self,
        origin: FileSnapshot,
        original: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let value = original
            .as_deref()
            .and_then(|path| path.rsplit('/').next())
            .unwrap_or("");
        let input = Self::input(value, "", false, window, cx);
        self.show_modal(
            Modal::FileName {
                owner: origin.owner,
                directory: origin.directory.clone(),
                original,
                input,
                origin,
            },
            window,
            cx,
        );
    }
    /// Mark visible entries of the current listing for a later copy or move paste.
    /// Hidden or stale paths make the whole capture inert, like other batch targets.
    pub(super) fn mark_file_targets(
        &mut self,
        owner: Owner,
        paths: Vec<String>,
        mode: ClipboardMode,
        cx: &mut Context<Self>,
    ) {
        if self.active_owner() != Some(owner) {
            return;
        }
        let Some(pane) = self.pane_mut(owner) else {
            return;
        };
        let origin = FileSnapshot::capture(pane, paths.clone());
        if paths.is_empty() || !origin.matches(pane) {
            return;
        }
        let sources = pane
            .files
            .entries
            .iter()
            .filter(|entry| paths.contains(&entry.path))
            .map(|entry| files::CopySource {
                path: entry.path.clone(),
                directory: entry.directory,
                symlink: entry.symlink,
            })
            .collect();
        pane.files.clipboard = Some(FileClipboard {
            id: Id::new_v4(),
            owner,
            mode,
            sources,
        });
        cx.notify();
    }
    /// Drop this session's pending copy or move without touching any remote entry.
    pub(super) fn cancel_file_clipboard(&mut self, owner: Owner, cx: &mut Context<Self>) {
        if let Some(pane) = self.pane_mut(owner) {
            pane.files.clipboard = None;
        }
        cx.notify();
    }
    /// Report a refused paste where the file panel shows it, without navigating.
    pub(super) fn file_paste_refused(
        &mut self,
        owner: Owner,
        message: String,
        cx: &mut Context<Self>,
    ) {
        if let Some(pane) = self.pane_mut(owner) {
            pane.files.error = Some(message.clone());
        }
        self.notice = Some(message);
        cx.notify();
    }
    pub(super) fn submit_file_operation(
        &mut self,
        origin: FileSnapshot,
        operation: FileOperation,
        modal: bool,
        cx: &mut Context<Self>,
    ) {
        if modal && self.modal_commit.busy() {
            return;
        }
        if self.active_owner() != Some(origin.owner)
            || !self
                .pane(origin.owner)
                .is_some_and(|pane| origin.matches(pane))
        {
            let message = self.t("file_target_changed").to_string();
            if modal {
                self.notice = Some(message);
                cx.notify();
            } else {
                // Pastes have no confirmation dialog; show the refusal in the panel.
                self.file_paste_refused(origin.owner, message, cx);
            }
            return;
        }
        let request = Id::new_v4();
        let owner = origin.owner;
        let saving = self.t("saving");
        if let Some(pane) = self.pane_mut(owner) {
            let relocated = relocations(&operation);
            if relocated.iter().any(|(from, _)| {
                pane.documents
                    .iter()
                    .any(|doc| doc.saving && files::contains_path(from, &doc.original.path))
            }) {
                if !modal {
                    pane.files.error = Some(saving.into());
                }
                self.notice = Some(saving.into());
                cx.notify();
                return;
            }
            for (from, _) in &relocated {
                pane.opened_requests
                    .retain(|_, (path, _, _)| !files::contains_path(from, path));
                pane.open_latest
                    .retain(|path, _| !files::contains_path(from, path));
                for doc in &mut pane.documents {
                    if files::contains_path(from, &doc.original.path) {
                        doc.open_request = None;
                    }
                }
            }
            pane.files.pending.insert(
                request,
                PendingFileOperation {
                    origin,
                    operation: operation.clone(),
                    clipboard_id: match &operation {
                        FileOperation::Move(..) => {
                            pane.files.clipboard.as_ref().map(|clipboard| clipboard.id)
                        }
                        _ => None,
                    },
                },
            );
        }
        if modal {
            self.modal_commit.request = Some(request);
            self.modal_commit.owner = Some(owner);
            self.modal_commit.after = Some(AfterCommit::Return);
            self.modal_commit.error = None;
        }
        self.backend.file_operation(owner, request, operation);
        cx.notify();
    }
    pub(super) fn file_operation_receipt(
        &mut self,
        owner: Owner,
        request: Id,
        result: Result<(), String>,
        moved: Vec<(String, String)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ui_size = self.prefs.ui_size;
        let Some(pending) = self
            .pane_mut(owner)
            .and_then(|pane| pane.files.pending.remove(&request))
        else {
            return;
        };
        let refresh = self
            .pane(owner)
            .is_some_and(|pane| pending.origin.matches(pane));
        // A move batch reports what it really relocated even when another entry failed.
        let move_total = match &pending.operation {
            FileOperation::Move(sources, _) => Some(sources.len()),
            _ => None,
        };
        let result = match (move_total, result) {
            (Some(total), Err(error)) if !moved.is_empty() => Err(format!(
                "{}: {}/{total}\n{error}",
                self.t("move_partial"),
                total - moved.len()
            )),
            (Some(_), Err(error)) => Err(format!("{}: {error}", self.t("move_failed"))),
            (_, result) => result,
        };
        if (result.is_ok() || !moved.is_empty())
            && let Some(pane) = self.pane_mut(owner)
        {
            // Drafts follow every entry the server relocated (rename or move).
            for (from, to) in &moved {
                for document in &mut pane.documents {
                    if files::contains_path(from, &document.original.path) {
                        document.original.path =
                            format!("{to}{}", &document.original.path[from.len()..]);
                        document.open_request = None;
                    }
                }
                pane.opened_requests
                    .retain(|_, (path, _, _)| !files::contains_path(from, path));
                pane.open_latest
                    .retain(|path, _| !files::contains_path(from, path));
            }
            pending.update_clipboard(&mut pane.files.clipboard, !moved.is_empty());
            pane.files.tree_children.clear();
            pane.files.tree_request = None;
            pane.files.tree_loading_path = None;
            pane.files.rebuild_tree_rows(ui_size);
        }
        let current = self.modal_commit.request == Some(request);
        let active_owner = self.active_owner();
        let state = if current {
            Some(&mut self.modal_commit)
        } else {
            self.modal_stack
                .iter_mut()
                .chain(self.modal_confirm_return.iter_mut())
                .find(|frame| frame.commit.request == Some(request))
                .map(|frame| &mut frame.commit)
        };
        let mut close = false;
        if let Some(state) = state {
            state.request = None;
            state.error = result.as_ref().err().cloned();
            state.committed = result.is_ok();
            close = current
                && state.after.take().is_some()
                && active_owner == Some(owner)
                && result.is_ok();
        } else if let Err(error) = &result {
            self.notice = Some(error.clone());
        }
        if refresh {
            if result.is_ok() || !moved.is_empty() {
                self.navigate(owner, None, cx);
                if move_total.is_some()
                    && let Some(pane) = self.pane_mut(owner)
                    && let Some(listing) = pane.files.request
                {
                    pane.files.after_listing = Some(ListingFollowUp {
                        request: listing,
                        select: moved.iter().map(|(_, to)| to.clone()).collect(),
                        error: result.as_ref().err().cloned(),
                    });
                }
            } else if let Some(pane) = self.pane_mut(owner) {
                pane.files.error = result.err();
            }
        } else if move_total.is_some()
            && let Err(error) = &result
            && let Some(pane) = self.pane_mut(owner)
        {
            // The destination is no longer on screen; still surface the move report.
            pane.files.error = Some(error.clone());
        }
        if close {
            self.cancel_modal(window, cx);
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::{ClipboardMode, FileClipboard, FileSnapshot, PendingFileOperation};
    use crate::{
        files,
        model::{Id, Owner},
        services::FileOperation,
    };

    fn clipboard(owner: Owner, mode: ClipboardMode) -> FileClipboard {
        FileClipboard {
            id: Id::new_v4(),
            owner,
            mode,
            sources: vec![files::CopySource {
                path: "/source/file.txt".into(),
                directory: false,
                symlink: false,
            }],
        }
    }

    fn pending_move(clipboard: &FileClipboard) -> PendingFileOperation {
        PendingFileOperation {
            origin: FileSnapshot {
                owner: clipboard.owner,
                directory: "/destination".into(),
                listing: Some(Id::new_v4()),
                hidden: false,
                paths: Vec::new(),
            },
            operation: FileOperation::Move(clipboard.sources.clone(), "/destination".into()),
            clipboard_id: Some(clipboard.id),
        }
    }

    #[test]
    fn move_receipt_preserves_new_copy_or_move_marks() {
        let owner = Owner::new();
        let original = clipboard(owner, ClipboardMode::Move);
        let pending = pending_move(&original);
        for mode in [ClipboardMode::Copy, ClipboardMode::Move] {
            // Even marking the same paths again creates a new user intent.
            let replacement = clipboard(owner, mode);
            let replacement_id = replacement.id;
            let mut current = Some(replacement);
            pending.update_clipboard(&mut current, true);
            let current = current.expect("an old receipt must preserve a later mark");
            assert_eq!(current.id, replacement_id);
            assert_eq!(current.mode, mode);
        }
    }

    #[test]
    fn move_receipt_consumes_its_buffer_only_after_something_moved() {
        let original = clipboard(Owner::new(), ClipboardMode::Move);
        let original_id = original.id;
        let pending = pending_move(&original);
        let mut current = Some(original);
        pending.update_clipboard(&mut current, false);
        assert_eq!(current.as_ref().unwrap().id, original_id);
        pending.update_clipboard(&mut current, true);
        assert!(current.is_none());
        pending.update_clipboard(&mut current, true);
        assert!(current.is_none());
    }

    #[test]
    fn rename_receipt_keeps_existing_clipboard_invalidation() {
        let original = clipboard(Owner::new(), ClipboardMode::Copy);
        let mut pending = pending_move(&original);
        pending.operation =
            FileOperation::Rename("/source/file.txt".into(), "/source/new.txt".into());
        pending.clipboard_id = None;
        let mut current = Some(original);
        pending.update_clipboard(&mut current, false);
        assert!(current.is_some());
        pending.update_clipboard(&mut current, true);
        assert!(current.is_none());
    }
}
