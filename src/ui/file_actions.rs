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
}
#[derive(Clone)]
pub(super) struct FileClipboard {
    pub owner: Owner,
    pub sources: Vec<files::CopySource>,
}
#[derive(Clone, Copy)]
pub(super) enum FileAction {
    Rename,
    Copy,
    Download,
    Delete,
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
    /// A rename owns both path prefixes until its real receipt. Reads/saves cannot race it.
    pub(super) fn file_path_busy(&self, owner: Owner, path: &str) -> bool {
        self.pane(owner).is_some_and(|pane| {
            pane.files.pending.values().any(|pending| {
                matches!(&pending.operation, FileOperation::Rename(from, to)
                if files::contains_path(from, path) || files::contains_path(to, path))
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
            FileAction::Rename => self.file_name_target(origin, Some(path), window, cx),
            FileAction::Copy => self.copy_file_targets(owner, vec![path], cx),
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
    pub(super) fn copy_file_targets(
        &mut self,
        owner: Owner,
        paths: Vec<String>,
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
        pane.files.clipboard = Some(FileClipboard { owner, sources });
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
            self.notice = Some(self.t("file_target_changed").into());
            cx.notify();
            return;
        }
        let request = Id::new_v4();
        let owner = origin.owner;
        if let Some(pane) = self.pane_mut(owner) {
            if let FileOperation::Rename(from, _) = &operation {
                if pane
                    .documents
                    .iter()
                    .any(|doc| doc.saving && files::contains_path(from, &doc.original.path))
                {
                    self.notice = Some(self.t("saving").into());
                    cx.notify();
                    return;
                }
                pane.opened_requests
                    .retain(|_, (path, _)| !files::contains_path(from, path));
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
        if result.is_ok()
            && let Some(pane) = self.pane_mut(owner)
        {
            if let FileOperation::Rename(from, to) = &pending.operation {
                for document in &mut pane.documents {
                    if files::contains_path(from, &document.original.path) {
                        document.original.path =
                            format!("{to}{}", &document.original.path[from.len()..]);
                        document.open_request = None;
                    }
                }
                pane.opened_requests
                    .retain(|_, (path, _)| !files::contains_path(from, path));
                pane.open_latest
                    .retain(|path, _| !files::contains_path(from, path));
                pane.files.clipboard = None;
            }
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
            if result.is_ok() {
                self.navigate(owner, None, cx);
            } else if let Some(pane) = self.pane_mut(owner) {
                pane.files.error = result.err();
            }
        }
        if close {
            self.cancel_modal(window, cx);
        }
        cx.notify();
    }
}
