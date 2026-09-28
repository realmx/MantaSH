//! Session-owned file, editor, history and Linux-system tools.
use super::*;
use crate::{
    files,
    model::{TransferRecord, TransferState},
};
use gpui_component::IconName;
use gpui_component::menu::ContextMenuExt;

/// File-list context-menu actions. Each carries its owning pane so a menu left
/// open across a tab switch still acts on the pane it was opened from.
macro_rules! file_menu_actions {
    ($($name:ident),* $(,)?) => {
        $(
            #[derive(Clone, PartialEq, serde::Deserialize, gpui::Action)]
            #[action(namespace = mantash, no_json)]
            pub(super) struct $name {
                pub owner: Owner,
            }
        )*
    };
}
file_menu_actions!(
    FileMenuEdit,
    FileMenuDownload,
    FileMenuCopy,
    FileMenuPaste,
    FileMenuRename,
    FileMenuDelete,
    FileMenuMkdir
);

impl FileTool {
    /// Rebuild the flat visible-row index used by the virtualized directory tree.
    /// Only expansion and directory responses call this; scrolling reuses the
    /// snapshot and therefore never walks or lays out the entire tree.
    pub(super) fn rebuild_tree_rows(&mut self, ui_size: f32) {
        fn visit(
            path: &str,
            depth: usize,
            children: &std::collections::HashMap<String, Vec<(String, bool)>>,
            expanded: &std::collections::HashSet<String>,
            rows: &mut Vec<(String, usize)>,
            max_width: &mut f32,
            ui_size: f32,
            show_hidden: bool,
        ) {
            rows.push((path.to_string(), depth));
            let name = path.rsplit('/').next().unwrap_or(path);
            let name = if name.is_empty() { "/" } else { name };
            // Exact row anatomy: indent + chevron slot (ui+2) + 4px gap +
            // folder icon (ui) + 4px gap + label + 4px trailing padding.
            // Combining separate maxima (deepest indent × widest name)
            // overestimated and showed a phantom horizontal scrollbar.
            *max_width = (*max_width).max(
                depth as f32 * 14.
                    + 4.
                    + (ui_size + 2.)
                    + 4.
                    + ui_size
                    + 4.
                    + estimate_text_width(name, ui_size)
                    + 4.,
            );
            if !expanded.contains(path) {
                return;
            }
            let Some(entries) = children.get(path) else {
                return;
            };
            for (name, _) in entries {
                // Hidden directories stay out of the tree unless "show
                // hidden" is on; cached entries just don't render.
                if name.starts_with('.') && !show_hidden {
                    continue;
                }
                let child = if path == "/" {
                    format!("/{name}")
                } else {
                    format!("{path}/{name}")
                };
                visit(
                    &child,
                    depth + 1,
                    children,
                    expanded,
                    rows,
                    max_width,
                    ui_size,
                    show_hidden,
                );
            }
        }

        let mut rows = Vec::new();
        let mut max_width = 0.;
        visit(
            "/",
            0,
            &self.tree_children,
            &self.tree_expanded,
            &mut rows,
            &mut max_width,
            ui_size,
            self.show_hidden,
        );
        self.tree_rows = std::rc::Rc::new(rows);
        self.tree_content_w = max_width;
    }
    /// Range selection and select-all always follow the displayed, filtered order.
    fn visible_paths(&self) -> Vec<String> {
        self.entries
            .iter()
            .filter(|entry| self.show_hidden || !entry.name.starts_with('.'))
            .map(|entry| entry.path.clone())
            .collect()
    }
    /// Batch targets keep display order and cannot include a hidden or removed row.
    fn selected_paths(&self) -> Vec<String> {
        self.visible_paths()
            .into_iter()
            .filter(|path| self.selected.contains(path))
            .collect()
    }
    /// Editing is offered only for files: exactly one selected entry and it
    /// must not be a directory. Resolved fresh at click time like the other
    /// menu handlers so a stale menu cannot edit a changed selection.
    pub(super) fn edit_target(&self) -> Option<String> {
        let paths = self.selected_paths();
        if paths.len() != 1 {
            return None;
        }
        self.entries
            .iter()
            .find(|entry| entry.path == paths[0])
            .filter(|entry| !entry.directory)
            .map(|entry| entry.path.clone())
    }
}

/// Rough glyph-width estimate: CJK ~1em, ASCII ~0.6em, plus a little padding.
pub(super) fn estimate_text_width(text: &str, size: f32) -> f32 {
    text.chars()
        .map(|c| if c.is_ascii() { size * 0.6 } else { size })
        .sum::<f32>()
        + size * 0.5
}

impl Workbench {
    /// Reject clicks from a replaced listing or another pane before changing selection or opening.
    pub(super) fn file_row_action(
        &mut self,
        owner: Owner,
        request: Option<Id>,
        path: &str,
        range: bool,
        additive: bool,
        open: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.active_owner() != Some(owner) {
            return;
        }
        let Some(pane) = self.pane_mut(owner) else {
            return;
        };
        // Accept both the legacy right-panel tool and the bottom files panel;
        // the 2026-09-20 layout keeps files in the bottom panel where
        // `pane.tool` is None, which used to silently drop every selection.
        if (pane.tool != Some(Tool::Files) && !pane.files_open)
            || pane.files.request != request
            || pane.files.loading
            || !pane.files.loaded
        {
            return;
        }
        let visible = pane.files.visible_paths();
        if !pane.files.selected.click(&visible, path, range, additive) {
            return;
        }
        pane.files.focus.focus(window);
        let directory = pane
            .files
            .entries
            .iter()
            .find(|entry| entry.path == path)
            .is_some_and(|entry| entry.directory);
        if open && !range && !additive {
            if directory {
                self.navigate(owner, Some(path.to_owned()), cx);
            } else {
                self.open_document(owner, path.to_owned(), window, cx);
            }
        }
        cx.notify();
    }
    /// Keep the removed checkbox's select-all capability in the menu and keyboard scope.
    pub(super) fn select_all_files(&mut self, owner: Owner, cx: &mut Context<Self>) {
        if let Some(pane) = self.pane_mut(owner).filter(|p| !p.files.loading) {
            let visible = pane.files.visible_paths();
            pane.files.selected.select_all(&visible);
            cx.notify();
        }
    }
    /// Hiding files must also remove their batch targets and any hidden range pivot.
    pub(super) fn toggle_hidden_files(&mut self, owner: Owner, cx: &mut Context<Self>) {
        let ui = self.prefs.ui_size;
        if let Some(pane) = self.pane_mut(owner) {
            pane.files.show_hidden = !pane.files.show_hidden;
            let visible = pane.files.visible_paths();
            pane.files.selected.retain_visible(&visible);
            // The tree rows filter hidden directories by the same flag, so
            // they must be rebuilt for the toggle to take effect immediately.
            pane.files.rebuild_tree_rows(ui);
            cx.notify();
        }
    }
    /// Freeze the current visible selection before showing a destructive-operation confirmation.
    pub(super) fn review_file_delete(
        &mut self,
        owner: Owner,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pane) = self.pane(owner).filter(|p| !p.files.loading) else {
            return;
        };
        let paths = pane.files.selected_paths();
        if !paths.is_empty() {
            self.show_modal(Modal::DeleteFiles { owner, paths }, window, cx);
        }
    }
    pub(super) fn navigate(&mut self, owner: Owner, path: Option<String>, cx: &mut Context<Self>) {
        let Some(pane) = self.pane_mut(owner) else {
            return;
        };
        if !matches!(pane.spec, SessionSpec::Ssh { .. }) {
            return;
        }
        let path = path.unwrap_or_else(|| pane.files.path.clone());
        let request = Id::new_v4();
        pane.files.request = Some(request);
        pane.files.loading = true;
        pane.files.error = None;
        pane.files.link_target = None;
        self.backend.list_files(owner, request, path);
        cx.notify();
    }
    pub(super) fn open_document(
        &mut self,
        owner: Owner,
        path: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Reopening an existing path always starts a fresh remote read. The
        // document entity is reused only after the newest response arrives,
        // so the editor cannot display stale input from an older open.
        let (existing_id, encoding) = {
            let Some(pane) = self.pane(owner) else {
                return;
            };
            let existing_id = pane
                .documents
                .iter()
                .find(|document| document.owner == owner && document.original.path == path)
                .map(|document| document.id);
            let encoding = existing_id
                .and_then(|id| pane.documents.iter().find(|document| document.id == id))
                .map(|document| document.encoding)
                .or(pane.files.encoding);
            (existing_id, encoding)
        };
        let request = Id::new_v4();
        if let Some(pane) = self.pane_mut(owner) {
            pane.opened_requests
                .insert(request, (path.clone(), existing_id));
            pane.open_latest.insert(path.clone(), request);
            if let Some(id) = existing_id {
                if let Some(document) = pane.documents.iter_mut().find(|document| document.id == id)
                {
                    document.open_request = Some(request);
                }
                pane.active_document = Some(id);
            }
        }
        self.backend.open_file(owner, request, path, encoding);
        if existing_id.is_some() {
            self.show_modal(Modal::Editor { owner }, window, cx);
            if let Some(document) = self.pane(owner).and_then(|pane| {
                pane.active_document
                    .and_then(|id| pane.documents.iter().find(|document| document.id == id))
            }) {
                document.input.focus_handle(cx).focus(window);
            }
        }
        cx.notify();
    }
    pub(super) fn add_document(
        &mut self,
        owner: Owner,
        file: OpenedFile,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = Id::new_v4();
        let encoding = file.encoding;
        let input = cx.new(|cx| {
            let mut state = InputState::new(window, cx)
                .code_editor("plain_text")
                .line_number(true)
                .searchable(true)
                .soft_wrap(false);
            state.set_value(file.text.clone(), window, cx);
            state
        });
        self.subscriptions.push(
            cx.subscribe(&input, move |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    if let Some(doc) = this.document_mut(owner, id) {
                        if doc.reverting {
                            // The Change arrives deferred after a close-with-
                            // discard rewrote the buffer; it restores the
                            // original text and must not re-dirty the draft.
                            doc.reverting = false;
                        } else {
                            doc.revision += 1;
                            doc.dirty = true;
                        }
                    }
                    cx.notify();
                }
            }),
        );
        if let Some(pane) = self.pane_mut(owner) {
            pane.documents.push(Document {
                owner,
                id,
                original: file,
                input,
                encoding,
                revision: 0,
                dirty: false,
                open_request: None,
                reverting: false,
                saving: false,
                error: None,
            });
            pane.active_document = Some(id);
        }
        // The opened document presents itself as the editor dialog, even
        // when its pane is not the active one behind the overlay.
        self.show_modal(Modal::Editor { owner }, window, cx);
        if let Some(doc) = self.document(owner, id) {
            doc.input.focus_handle(cx).focus(window);
        }
    }
    /// Replace an existing document with the newest successful remote read.
    /// The request id prevents an older reopen response from restoring stale
    /// text after a newer read has already been started.
    pub(super) fn refresh_document(
        &mut self,
        owner: Owner,
        id: Id,
        request: Id,
        file: OpenedFile,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(document) = self.document_mut(owner, id) else {
            return false;
        };
        if document.open_request != Some(request) {
            return false;
        }
        let input = document.input.clone();
        document.open_request = None;
        document.original = file.clone();
        document.encoding = file.encoding;
        document.revision = document.revision.wrapping_add(1);
        document.dirty = false;
        document.reverting = true;
        document.saving = false;
        document.error = None;
        if let Some(pane) = self.pane_mut(owner) {
            pane.active_document = Some(id);
        }
        input.update(cx, |state, cx| state.set_value(file.text, window, cx));
        if matches!(self.modal, Some(Modal::Editor { owner: modal_owner }) if modal_owner == owner)
        {
            input.focus_handle(cx).focus(window);
        }
        cx.notify();
        true
    }

    /// Read-only view of one document, mirroring [`Self::document_mut`].
    fn document(&self, owner: Owner, id: Id) -> Option<&Document> {
        self.tabs
            .iter()
            .flat_map(|t| &t.panes)
            .find(|p| p.owner == owner)
            .and_then(|p| p.documents.iter().find(|d| d.id == id && d.owner == owner))
    }
    pub(super) fn save_document(
        &mut self,
        owner: Owner,
        id: Id,
        force: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(doc) = self.document_mut(owner, id) else {
            return;
        };
        if doc.saving || (!doc.dirty && !force) {
            return;
        }
        doc.saving = true;
        doc.error = None;
        let original = doc.original.clone();
        let text = doc.input.read(cx).value().to_string();
        let encoding = doc.encoding;
        let revision = doc.revision;
        self.backend
            .save_file(owner, id, revision, original, text, encoding, force);
        cx.notify();
    }
    pub(super) fn reload_document(&mut self, owner: Owner, id: Id, cx: &mut Context<Self>) {
        let Some(doc) = self.document_mut(owner, id) else {
            return;
        };
        let path = doc.original.path.clone();
        let encoding = doc.encoding;
        let request = Id::new_v4();
        if let Some(document) = self.document_mut(owner, id) {
            document.open_request = Some(request);
        }
        if let Some(pane) = self.pane_mut(owner) {
            // Keep the old draft until the replacement read actually succeeds.
            pane.opened_requests
                .insert(request, (path.clone(), Some(id)));
            pane.open_latest.insert(path.clone(), request);
            self.backend.open_file(owner, request, path, Some(encoding));
        }
        cx.notify();
    }
    #[cfg(debug_assertions)]
    fn is_port_qa_owner(&self, owner: Owner) -> bool {
        self.qa
            .as_ref()
            .is_some_and(|qa| qa.ports_fixture_owner == Some(owner))
    }
    pub(super) fn refresh_monitor(&mut self, owner: Owner, cx: &mut Context<Self>) {
        #[cfg(debug_assertions)]
        if self.is_port_qa_owner(owner) {
            return;
        }
        let Some(pane) = self.pane_mut(owner) else {
            return;
        };
        if pane.monitor_request.is_some()
            || pane.state != ConnectionState::Connected
            || !matches!(pane.spec, SessionSpec::Ssh { .. })
        {
            return;
        }
        let request = Id::new_v4();
        pane.monitor_request = Some(request);
        pane.last_sample = Instant::now();
        let previous = pane.monitor.clone();
        self.backend.sample(owner, request, previous);
        cx.notify();
    }
    pub(super) fn file_name_form(
        &mut self,
        owner: Owner,
        rename: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pane) = self.pane(owner) else {
            return;
        };
        let directory = pane.files.path.clone();
        let original = if rename {
            pane.files.selected.iter().next().cloned()
        } else {
            None
        };
        if rename && original.is_none() {
            return;
        }
        let value = original
            .as_ref()
            .and_then(|s| s.rsplit('/').next())
            .unwrap_or("");
        let input = Self::input(value, "", false, window, cx);
        self.show_modal(
            Modal::FileName {
                owner,
                directory,
                original,
                input,
            },
            window,
            cx,
        );
    }
    pub(super) fn choose_transfer(
        &mut self,
        owner: Owner,
        upload: bool,
        directories: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pane) = self
            .pane(owner)
            .filter(|pane| pane.state == ConnectionState::Connected)
        else {
            return;
        };
        let SessionSpec::Ssh { profile, .. } = &pane.spec else {
            return;
        };
        let profile = profile.clone();
        let directory = pane.files.path.clone();
        let selected = pane.files.selected_paths();
        let request = pane.files.request;
        if !upload && selected.is_empty() {
            return;
        }
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: upload && !directories,
            directories: !upload || directories,
            multiple: upload,
            prompt: Some(self.t(if upload { "upload" } else { "download" }).into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = prompt.await {
                let _ = this.update_in(cx, |this, window, cx| {
                    let still_current = this.active_pane().is_some_and(|pane| {
                        pane.owner == owner
                            && pane.state == ConnectionState::Connected
                            && pane.files.path == directory
                            && pane.files.request == request
                            && (upload || pane.files.selected_paths() == selected)
                    });
                    if !still_current {
                        this.notice = Some(this.t("transfer_target_changed").into());
                        cx.notify();
                        return;
                    }
                    let mut records = Vec::new();
                    if upload {
                        for path in paths {
                            let name = path
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .into_owned();
                            match files::join(&directory, &name) {
                                Ok(remote) => records.push(Self::transfer_record(
                                    owner,
                                    profile.clone(),
                                    true,
                                    path.to_string_lossy().into_owned(),
                                    remote,
                                )),
                                Err(error) => this.notice = Some(error.to_string()),
                            }
                        }
                    } else if let Some(local_directory) = paths.first() {
                        for path in &selected {
                            let name = path.rsplit('/').next().unwrap_or("");
                            if let Err(error) = files::valid_download_name(name, cfg!(windows)) {
                                this.notice = Some(error.to_string());
                            } else {
                                records.push(Self::transfer_record(
                                    owner,
                                    profile.clone(),
                                    false,
                                    local_directory.join(name).to_string_lossy().into_owned(),
                                    path.clone(),
                                ));
                            }
                        }
                    }
                    if !records.is_empty() {
                        this.show_transfer_confirmation(owner, records, window, cx);
                    }
                });
            }
        })
        .detach();
    }
    pub(super) fn transfer_record(
        owner: Owner,
        profile: Profile,
        upload: bool,
        local: String,
        remote: String,
    ) -> TransferRecord {
        TransferRecord {
            id: Id::new_v4(),
            profile,
            upload,
            local,
            remote,
            session: Some(owner.session),
            attempt: Some(owner.attempt),
            state: TransferState::Queued,
            bytes: 0,
            total: None,
            error: None,
            timestamp: chrono::Utc::now().timestamp_millis(),
        }
    }
    /// Open immediately, then resolve local upload sizes on the background runtime.
    pub(super) fn show_transfer_confirmation(
        &mut self,
        owner: Owner,
        records: Vec<TransferRecord>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if records.is_empty()
            || !self
                .pane(owner)
                .is_some_and(|pane| pane.state == ConnectionState::Connected)
            || !records.iter().all(|record| record.belongs_to(owner))
        {
            return;
        }
        let batch = Id::new_v4();
        let sources: Vec<(Id, String)> = records
            .iter()
            .filter(|record| record.upload)
            .map(|record| (record.id, record.local.clone()))
            .collect();
        let review_origin = self
            .pane(owner)
            .map(|pane| (pane.files.path.clone(), pane.files.request));
        self.show_modal(
            Modal::Transfer {
                owner,
                batch,
                records,
                overwrite: false,
                review_origin,
                phase: super::dialogs::TransferPhase::Review,
            },
            window,
            cx,
        );
        if sources.is_empty() {
            return;
        }
        let runtime = self.backend.runtime.clone();
        cx.spawn_in(window, async move |this, cx| {
            let sizes = runtime
                .spawn(async move {
                    let mut sizes = Vec::with_capacity(sources.len());
                    for (id, path) in sources {
                        let size = files::upload_file_size(std::path::Path::new(&path)).await;
                        sizes.push((id, size));
                    }
                    sizes
                })
                .await;
            if let Ok(sizes) = sizes {
                let _ = this.update_in(cx, |this, _, cx| {
                    if let Some(Modal::Transfer {
                        owner: current,
                        batch: current_batch,
                        phase: super::dialogs::TransferPhase::Review,
                        records,
                        ..
                    }) = &mut this.modal
                        && *current == owner
                        && *current_batch == batch
                    {
                        for (id, size) in sizes {
                            if let Some(record) = records.iter_mut().find(|record| record.id == id)
                            {
                                record.total = size;
                            }
                        }
                        cx.notify();
                    }
                });
            }
        })
        .detach();
    }
    pub(super) fn retry_transfer(
        &mut self,
        record: TransferRecord,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let owner = record.session.and_then(|session| self.tabs.iter().flat_map(|t| &t.panes)
            .find(|pane| pane.owner.session == session
                && pane.state == ConnectionState::Connected
                && matches!(&pane.spec, SessionSpec::Ssh { profile, .. } if profile.id == record.profile.id))
            .map(|pane| pane.owner));
        if let Some(owner) = owner {
            let record = Self::transfer_record(
                owner,
                record.profile,
                record.upload,
                record.local,
                record.remote,
            );
            self.show_transfer_confirmation(owner, vec![record], window, cx);
        } else {
            self.notice = Some(format!("{}: {}", self.t("reconnect"), record.profile.name));
            cx.notify();
        }
    }
    /// Submit once, then retain exact record UUIDs until all workers settle.
    pub(super) fn confirm_transfers(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(Modal::Transfer {
            owner,
            batch,
            records,
            overwrite,
            phase,
            review_origin,
        }) = &self.modal
        else {
            return;
        };
        if *phase != super::dialogs::TransferPhase::Review || records.is_empty() {
            return;
        }
        let owner = *owner;
        let batch = *batch;
        let overwrite = *overwrite;
        let records = records.clone();
        let Some(pane) = self
            .pane(owner)
            .filter(|pane| pane.state == ConnectionState::Connected)
        else {
            self.notice = Some(self.t("transfer_target_changed").into());
            cx.notify();
            return;
        };
        let file_path = pane.files.path.clone();
        let file_request = pane.files.request;
        if !review_origin
            .as_ref()
            .is_some_and(|(path, request)| *path == file_path && *request == file_request)
        {
            self.notice = Some(self.t("transfer_target_changed").into());
            cx.notify();
            return;
        }
        if !records.iter().all(|record| record.belongs_to(owner))
            || self.transfer_batches.contains_key(&batch)
        {
            return;
        }
        if let Some(Modal::Transfer { phase, .. }) = &mut self.modal {
            *phase = super::dialogs::TransferPhase::Running;
        }
        self.transfer_batches.insert(
            batch,
            super::TransferBatch {
                owner,
                ids: records.iter().map(|record| record.id).collect(),
                file_path,
                file_request,
            },
        );
        for record in records {
            self.transfers.push(record.clone());
            self.backend.transfer(owner, record, overwrite);
        }
        cx.notify();
    }
    /// Ask the worker to stop; its terminal reply, not the request, settles the batch.
    pub(super) fn cancel_transfer(&mut self, id: Id, cx: &mut Context<Self>) {
        if self.backend.cancel_transfer(id) {
            cx.notify();
        }
    }
    pub(super) fn render_tool(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let _ = window;
        let tool = self.active_tool().unwrap_or(Tool::System);
        let body = match tool {
            Tool::Files => self.render_files(cx),
            Tool::History => self.render_history(cx),
            // The editor moved into a dialog; a legacy workspace restoring
            // "editor" normalizes to the system page (see workspace restore).
            Tool::System | Tool::Editor => self.render_monitor(window, cx),
        };
        div()
            .size_full()
            .min_h_0()
            .flex()
            .flex_col()
            .child(body)
            .into_any_element()
    }
    pub(super) fn render_files_toolbar(
        &self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = theme::Palette::new(self.prefs.theme);
        let Some(pane) = self
            .active_pane()
            .filter(|p| matches!(p.spec, SessionSpec::Ssh { .. }))
        else {
            return div().into_any_element();
        };
        let owner = pane.owner;
        let selected = pane.files.selected.len();
        div()
            .flex()
            // The row must claim the full panel width explicitly: without it
            // the flex_1 path host never receives free space and collapses to
            // its content width (the pre-existing broken path display).
            .w(px(f32::from(self.body_bounds.size.width).max(240.)))
            .items_center()
            .gap(px(4.))
            // Toolbar insets: 8px left/right and 4px below; the 4px above
            // the elements lives in the drag strip's surface (the strip is
            // the toolbar's visual top edge), keeping top and bottom visual
            // spacing symmetric. Elements stay vertically centered.
            .px(px(theme::SPACE_CONTROL))
            .pb(px(theme::SPACE_SMALL))
            // The toolbar never yields its height to the list: a short panel
            // must squeeze the rows, not swallow the path input.
            .flex_shrink_0()
            .border_b_1()
            .border_color(p.border)
            .child(
                self.button("parent-dir", "")
                    .disabled(pane.state != ConnectionState::Connected)
                    .icon(IconName::ChevronUp)
                    .ghost()
                    .tooltip(self.t("parent"))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(p) = this.pane(owner) {
                            this.navigate(owner, Some(files::parent(&p.files.path)), cx);
                        }
                    })),
            )
            .child({
                // The path input stays mounted and visible whenever the file
                // panel is open; its value is only ever the current directory
                // (Event::Files syncs both directions).
                let host = div()
                    .id("path-host")
                    .flex_1()
                    .min_w(px(80.))
                    .h(px(self.input_height()))
                    .capture_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                        if event.keystroke.key.as_str() == "enter"
                            && event.keystroke.modifiers == Modifiers::default()
                        {
                            let typed = this.pane(owner).map(|p| {
                                (
                                    p.files.input.read(cx).value().trim().to_string(),
                                    p.files.path.clone(),
                                    p.files.input.clone(),
                                )
                            });
                            if let Some((target, current, input)) = typed {
                                if target.is_empty() {
                                    // An empty entry is not a directory:
                                    // keep the input mirroring the current
                                    // path instead of navigating.
                                    input.update(cx, |s, cx| s.set_value(current, window, cx));
                                } else {
                                    this.navigate(owner, Some(target), cx);
                                }
                            }
                            cx.stop_propagation();
                        }
                    }))
                    .child(self.input_box(&pane.files.input).w_full());
                // QA-only measure hook: reports the input host's exact
                // painted bounds so pixel checks target the real box.
                #[cfg(debug_assertions)]
                let host = if self.qa.is_some() {
                    let measure = cx.entity();
                    host.child(
                        canvas(
                            move |bounds, _, cx| {
                                measure.update(cx, |this, _| {
                                    if let Some(qa) = &mut this.qa {
                                        qa.files_input_bounds = Some(bounds);
                                    }
                                });
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .size_full(),
                    )
                } else {
                    host
                };
                host
            })
            .child({
                // Keep the button in place while loading: only its icon swaps
                // for the spinner, so the toolbar width never shifts.
                let refresh = self
                    .button("refresh-files", "")
                    .svg_icon("icons/refresh-cw.svg")
                    .ghost()
                    .tooltip(self.t("refresh"))
                    .disabled(pane.state != ConnectionState::Connected || pane.files.loading);
                let refresh = if pane.files.loading {
                    refresh.icon_element(self.loading_spinner())
                } else {
                    refresh
                };
                refresh
                    .on_click(cx.listener(move |this, _, _, cx| this.navigate(owner, None, cx)))
                    .into_any_element()
            })
            .child(
                self.button("hidden-files", "")
                    .icon(IconName::Eye)
                    .ghost()
                    .selected(pane.files.show_hidden)
                    .tooltip(self.t("hidden"))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.toggle_hidden_files(owner, cx);
                    })),
            )
            .child(
                self.button("new-directory", "")
                    .disabled(pane.state != ConnectionState::Connected)
                    .svg_icon("icons/folder-plus.svg")
                    .ghost()
                    .tooltip(self.t("mkdir"))
                    .on_click(
                        cx.listener(move |this, _, w, cx| this.file_name_form(owner, false, w, cx)),
                    ),
            )
            .child(
                self.button("upload-file", "")
                    .svg_icon("icons/upload.svg")
                    .ghost()
                    .tooltip(self.t("upload"))
                    .disabled(pane.state != ConnectionState::Connected)
                    .on_click(cx.listener(move |this, _, w, cx| {
                        this.choose_transfer(owner, true, false, w, cx)
                    })),
            )
            .child(
                self.button("transfer-history", "")
                    .svg_icon("icons/arrow-down-up.svg")
                    .ghost()
                    .tooltip(self.t("transfer"))
                    .on_click(cx.listener(move |this, _, w, cx| {
                        this.show_modal(Modal::Transfers { owner }, w, cx)
                    })),
            )
            .when(selected > 0, |row| {
                row.child(
                    self.button("delete-file", "")
                        .svg_icon("icons/trash-2.svg")
                        .ghost()
                        .tooltip(self.t("delete"))
                        .disabled(pane.files.loading || pane.state != ConnectionState::Connected)
                        .on_click(cx.listener(move |this, _, w, cx| {
                            this.review_file_delete(owner, w, cx);
                        })),
                )
            })
            .into_any_element()
    }

    pub(super) fn render_files(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = theme::Palette::new(self.prefs.theme);
        let Some(pane) = self
            .active_pane()
            .filter(|p| matches!(p.spec, SessionSpec::Ssh { .. }))
        else {
            return div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .child(div().p_4().text_color(p.muted).child(self.t("ssh_only")))
                .into_any_element();
        };
        let owner = pane.owner;
        let listing_request = pane.files.request;
        let mut content = div().flex().flex_col().min_h_0().flex_1();
        // Toolbar moved to render_files_toolbar.
        content = content.when_some(pane.files.error.clone(), |d, error| {
            d.child(div().px_3().py_2().text_color(p.error).child(error))
        });
        if let Some((link, target, directory)) = &pane.files.link_target {
            let link = link.clone();
            let target = target.clone();
            let directory = *directory;
            content = content.child(
                div()
                    .p(px(theme::SPACE_PANEL))
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(self.t("symlink_hint"))
                    .child(format!("{link} → {target}"))
                    .child(
                        self.button("review-link", self.t("review_target"))
                            .on_click(cx.listener(move |this, _, w, cx| {
                                this.show_modal(
                                    Modal::FollowLink {
                                        owner,
                                        link: link.clone(),
                                        target: target.clone(),
                                        directory,
                                    },
                                    w,
                                    cx,
                                )
                            })),
                    ),
            );
        }
        // Adaptive columns: every column is at least as wide as its header label
        // and its widest visible cell; a manual drag sets an exact override.
        let name_header = estimate_text_width(self.t("name"), self.prefs.ui_size);
        let size_header = estimate_text_width(self.t("size"), self.prefs.ui_size);
        let date_header = estimate_text_width(self.t("modified"), self.prefs.ui_size);
        let mut adaptive_size = size_header;
        let mut date_w = date_header;
        for entry in pane
            .files
            .entries
            .iter()
            .filter(|e| pane.files.show_hidden || !e.name.starts_with('.'))
        {
            let size_label = if entry.directory {
                "—".to_string()
            } else {
                entry.size.map_or_else(|| "—".into(), crate::monitor::bytes)
            };
            adaptive_size = adaptive_size.max(estimate_text_width(&size_label, self.prefs.ui_size));
            let date_label = entry
                .modified
                .and_then(|m| chrono::DateTime::from_timestamp(m as i64, 0))
                .map(|d| {
                    d.with_timezone(&chrono::Local)
                        .format("%m-%d %H:%M")
                        .to_string()
                })
                .unwrap_or_else(|| "—".into());
            date_w = date_w.max(estimate_text_width(&date_label, self.prefs.ui_size));
        }
        // Every column keeps a 100px floor, whatever its content or drag
        // override says.
        let size_w = (adaptive_size + pane.files.size_extra).max(100.);
        let date_w = date_w.max(100.);
        // The name column has an EXPLICIT width shared verbatim by the header
        // and every row (computed from the rows' own live viewport), so no
        // flex distribution quirk can ever make the two disagree; size and
        // date keep their content minimums and manual-drag overrides.
        let panel = f32::from(pane.files.scroll.bounds().size.width);
        let name_w = pane
            .files
            .name_extra
            .map(|width| width.max(100.))
            .or_else(|| Some(((panel - 50. - size_w - date_w).max(100.)).max(0.)));
        // Single coordinate system: the header is the first row inside the
        // same scroll container as the file rows, so both share one layout
        // and one scroll translation.
        let column_row = |prefix: &'static str,
                          name_cell: AnyElement,
                          second: SharedString,
                          third: SharedString,
                          trailing: bool|
         -> Div {
            div()
                .flex()
                .items_center()
                .gap(px(4.))
                .px(px(theme::SPACE_PANEL))
                .min_w(px(name_w.unwrap_or(name_header) + size_w + date_w + 40.))
                .child(name_cell)
                .child(
                    div()
                        .id((SharedString::from(prefix), 1usize))
                        .w(px(5.))
                        .h(px(22.))
                        .flex_shrink_0()
                        .when(trailing, |edge| {
                            edge.cursor_col_resize()
                                .hover(|d| d.bg(p.accent))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, _, _window, _| {
                                        // The baseline must come from the list's own
                                        // viewport; the old right-panel tool width no
                                        // longer matches the bottom file panel and made
                                        // the first drag jump.
                                        let panel = this
                                            .pane(owner)
                                            .map(|p| f32::from(p.files.scroll.bounds().size.width))
                                            .unwrap_or(name_header + size_w + date_w + 50.);
                                        let current = name_w.unwrap_or_else(|| {
                                            (panel - 50. - size_w - date_w).max(name_header)
                                        });
                                        if let Some(p) = this.pane_mut(owner) {
                                            p.files.name_extra = Some(current);
                                        }
                                        this.resize = Some(Resize::FileName(owner));
                                    }),
                                )
                        }),
                )
                .child(
                    div()
                        .w(px(size_w))
                        .flex_shrink_0()
                        .text_color(p.muted)
                        .child(second),
                )
                .child(
                    div()
                        .id((SharedString::from(prefix), 2usize))
                        .w(px(5.))
                        .h(px(22.))
                        .flex_shrink_0()
                        .when(trailing, |edge| {
                            edge.cursor_col_resize()
                                .hover(|d| d.bg(p.accent))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, _, _, _| {
                                        this.resize = Some(Resize::FileSize(owner));
                                    }),
                                )
                        }),
                )
                .child(
                    div()
                        .w(px(date_w))
                        .flex_shrink_0()
                        .text_color(p.muted)
                        .child(third),
                )
        };
        let header = column_row(
            "file-head",
            div()
                .w(px(name_w.unwrap_or(name_header)))
                .flex_shrink_0()
                .min_w(px(name_header))
                .flex()
                .items_center()
                // The label sits at the column's leading edge, directly
                // above the rows' type icons — the alignment users read as
                // "columns line up". No leading slot, icon or padding: every
                // offset variant placed it right of the row content.
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .child(self.t("name")),
                )
                .into_any_element(),
            self.t("size").into(),
            self.t("modified").into(),
            true,
        )
        // Stateful like the row elements: rows carry .id(...) and measurably
        // lay their leading icon out differently from an id-less container.
        .id("file-header")
        .py_1()
        .border_b_1()
        .border_color(p.border);
        content = content.child(
            // Non-scrolling shell for the columns stage and its horizontal
            // indicator; the bar overlays the stage bottom edge instead of
            // living in a second flex child (which halved the list height).
            div()
                .relative()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .child(
                    // Horizontal stage: the fixed header and the rows share
                    // this X-scroll so the columns stay aligned while only
                    // the rows scroll vertically beneath the header.
                    div()
                        .id("files-columns")
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_h_0()
                            .overflow_x_scroll()
                            .track_scroll(&pane.files.scroll_x)
                            .child(header)
                            .child(
                                div().relative().flex().flex_1().min_h_0().child(
                                    div()
                                        .id("file-list")
                                        // The explicit full height keeps the
                                        // viewport definite inside the wrapper
                                        // so vertical overflow scrolls.
                                        .h_full()
                                        .track_focus(&pane.files.focus.clone().tab_stop(true))
                                        .key_context("MantaSHFiles")
                                        .on_action(cx.listener(
                                            move |this,
                                                  _: &gpui_component::input::SelectAll,
                                                  _,
                                                  cx| {
                                                this.select_all_files(owner, cx);
                                                cx.stop_propagation();
                                            },
                                        ))
                                        .on_key_down(cx.listener(
                                            move |this, event: &KeyDownEvent, window, cx| {
                                                match event.keystroke.key.as_str() {
                                                    "up" | "down" | "home" | "end" => {
                                                        let Some(pane) = this.pane(owner) else {
                                                            return;
                                                        };
                                                        let visible = pane.files.visible_paths();
                                                        if visible.is_empty() {
                                                            return;
                                                        }
                                                        let current =
                                                            pane.files.selected.lead().and_then(
                                                                |path| {
                                                                    visible
                                                                        .iter()
                                                                        .position(|p| *path == *p)
                                                                },
                                                            );
                                                        let last = visible.len() - 1;
                                                        let index =
                                                            match event.keystroke.key.as_str() {
                                                                "home" => 0,
                                                                "end" => last,
                                                                "down" => current.map_or(0, |i| {
                                                                    (i + 1).min(last)
                                                                }),
                                                                _ => current.map_or(last, |i| {
                                                                    i.saturating_sub(1)
                                                                }),
                                                            };
                                                        this.file_row_action(
                                                            owner,
                                                            listing_request,
                                                            &visible[index],
                                                            event.keystroke.modifiers.shift,
                                                            event.keystroke.modifiers.control
                                                                || event
                                                                    .keystroke
                                                                    .modifiers
                                                                    .platform,
                                                            false,
                                                            window,
                                                            cx,
                                                        );
                                                        if let Some(pane) = this.pane(owner) {
                                                            let scroll = &pane.files.scroll;
                                                            if let Some(bounds) =
                                                                scroll.bounds_for_item(index)
                                                            {
                                                                let mut offset = scroll.offset();
                                                                if bounds.top() + offset.y
                                                                    < scroll.bounds().top()
                                                                {
                                                                    offset.y =
                                                                        scroll.bounds().top()
                                                                            - bounds.top();
                                                                } else if bounds.bottom() + offset.y
                                                                    > scroll.bounds().bottom()
                                                                {
                                                                    offset.y =
                                                                        scroll.bounds().bottom()
                                                                            - bounds.bottom();
                                                                }
                                                                scroll.set_offset(offset);
                                                            }
                                                        }
                                                        cx.stop_propagation();
                                                    }
                                                    "escape" => {
                                                        if let Some(pane) = this.pane_mut(owner) {
                                                            pane.files.selected.clear();
                                                        }
                                                        cx.notify();
                                                        cx.stop_propagation();
                                                    }
                                                    "enter" => {
                                                        let path = this
                                                            .pane(owner)
                                                            .filter(|p| p.files.selected.len() == 1)
                                                            .and_then(|p| {
                                                                p.files
                                                                    .selected
                                                                    .iter()
                                                                    .next()
                                                                    .cloned()
                                                            });
                                                        if let Some(path) = path {
                                                            this.file_row_action(
                                                                owner,
                                                                listing_request,
                                                                &path,
                                                                false,
                                                                false,
                                                                true,
                                                                window,
                                                                cx,
                                                            );
                                                        }
                                                        cx.stop_propagation();
                                                    }
                                                    _ => {}
                                                }
                                            },
                                        ))
                                        .relative()
                                        .flex_1()
                                        .min_h_0()
                                        .overflow_y_scroll()
                                        .track_scroll(&pane.files.scroll)
                                        .on_mouse_down(MouseButton::Left, {
                                            let list_focus = pane.files.focus.clone();
                                            cx.listener(move |this, _, window, cx| {
                                                // Only blank space reaches here; rows stop
                                                // propagation after handling their own click.
                                                if let Some(pane) = this.pane_mut(owner) {
                                                    pane.files.selected.clear();
                                                }
                                                list_focus.focus(window);
                                                cx.notify();
                                            })
                                        })
                                        .children(
                                            pane.files
                                                .entries
                                                .iter()
                                                .filter(|e| {
                                                    pane.files.show_hidden
                                                        || !e.name.starts_with('.')
                                                })
                                                .enumerate()
                                                .map(|(visible_index, entry)| {
                                                    let path = entry.path.clone();
                                                    let directory = entry.directory;
                                                    let selected =
                                                        pane.files.selected.contains(&path);
                                                    let size_label: SharedString = if directory {
                                                        "—".into()
                                                    } else {
                                                        entry
                                                            .size
                                                            .map_or_else(
                                                                || "—".into(),
                                                                crate::monitor::bytes,
                                                            )
                                                            .into()
                                                    };
                                                    let date_label: SharedString = entry
                                                        .modified
                                                        .and_then(|m| {
                                                            chrono::DateTime::from_timestamp(
                                                                m as i64, 0,
                                                            )
                                                        })
                                                        .map(|d| {
                                                            d.with_timezone(&chrono::Local)
                                                                .format("%m-%d %H:%M")
                                                                .to_string()
                                                        })
                                                        .unwrap_or_else(|| "—".into())
                                                        .into();
                                                    let click_path = path.clone();
                                                    let rc_path = path.clone();
                                                    column_row(
                                            "file-body",
                                            div()
                                                .w(px(name_w.unwrap_or(name_header)))
                                                .flex_shrink_0()
                                                .min_w(px(name_header))
                                                .flex()
                                                .items_center()
                                                // Same explicit width as the header's name
                                                // cell: both come from one name_w value.
                                                .child(
                                                    div()
                                                        .flex_1()
                                                        .min_w_0()
                                                        .overflow_hidden()
                                                        .child(format!(
                                                            "{}{}",
                                                            entry.name,
                                                            if entry.symlink { " ↗" } else { "" }
                                                        )),
                                                )
                                                .into_any_element(),
                                            size_label,
                                            date_label,
                                            false,
                                        )
                                        .id(SharedString::from(path.clone()))
                                        .py(px(4.))
                                        // Zebra striping replaces row hairlines;
                                        // selected rows keep the selection color.
                                        .bg(if selected {
                                            p.selected
                                        } else if visible_index % 2 == 1 {
                                            p.background
                                        } else {
                                            p.surface
                                        })
                                        .when(!selected, |row| row.hover(|d| d.bg(p.background)))
                                        .cursor_pointer()
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(
                                                move |this, event: &MouseDownEvent, window, cx| {
                                                    window.prevent_default();
                                                    cx.stop_propagation();
                                                    this.file_row_action(
                                                        owner,
                                                        listing_request,
                                                        &path,
                                                        event.modifiers.shift,
                                                        event.modifiers.control
                                                            || event.modifiers.platform,
                                                        false,
                                                        window,
                                                        cx,
                                                    );
                                                },
                                            ),
                                        )
                                        .on_click(
                                            cx.listener(
                                                move |this, event: &ClickEvent, window, cx| {
                                                    if this.tab_strip.suppress_click.is_some()
                                                        && matches!(event, ClickEvent::Mouse(_))
                                                    {
                                                        return;
                                                    }
                                                    // Selection already happened in mouse_down with
                                                    // modifier support; clicks only open (double-click
                                                    // by mouse, single activation by keyboard).
                                                    let open = event.click_count() >= 2
                                                        || matches!(event, ClickEvent::Keyboard(_));
                                                    if open {
                                                        this.file_row_action(
                                                            owner,
                                                            listing_request,
                                                            &click_path,
                                                            false,
                                                            false,
                                                            true,
                                                            window,
                                                            cx,
                                                        );
                                                    }
                                                },
                                            ),
                                        )
                                        .on_mouse_down(
                                            MouseButton::Right,
                                            cx.listener(
                                                move |this, _: &MouseDownEvent, window, cx| {
                                                    // Merge like native file managers: a
                                                    // right-click outside the selection makes
                                                    // the row the sole selection; inside it,
                                                    // the whole selection stays for the menu.
                                                    if let Some(pane) = this.pane_mut(owner) {
                                                        let visible =
                                                            pane.files.visible_paths();
                                                        if !pane.files.selected.contains(&rc_path)
                                                        {
                                                            pane.files.selected.click(
                                                                &visible,
                                                                &rc_path,
                                                                false,
                                                                false,
                                                            );
                                                        }
                                                        pane.files.focus.focus(window);
                                                    }
                                                    cx.notify();
                                                },
                                            ),
                                        )
                                                })
                                        ),
                                )
                                // Same persistent position indicator as the
                                // connection library list: always visible, drag or
                                // click the track to jump. A child of the scroll
                                // container itself scrolls away with the rows, so
                                // it must hang on this non-scrolling wrapper.
                                .child(self.overlay_scrollbar(
                                    "file-list-scrollbar",
                                    pane.files.scroll.clone(),
                                    Resize::FileScroll(owner),
                                    cx,
                                ))
                                // One menu for the whole list: right-clicking a
                                // row merges it into the selection first (the row
                                // handler runs before the deferred menu build), and
                                // right-clicking blank space keeps the current
                                // selection, so the menu always reflects live
                                // state read through the workbench entity.
                                .context_menu({
                                    let entity = cx.entity();
                                    move |menu: gpui_component::menu::PopupMenu,
                                          _: &mut Window,
                                          menu_cx: &mut Context<
                                        gpui_component::menu::PopupMenu,
                                    >| {
                                        let this = entity.read(menu_cx);
                                        let (busy, count, can_paste, editable) =
                                            this.pane(owner).map_or((true, 0, false, false), |pane| {
                                                (
                                                    pane.files.loading
                                                        || pane.state
                                                            != ConnectionState::Connected,
                                                    pane.files.selected_paths().len(),
                                                    pane.files.clipboard.is_some(),
                                                    pane.files.edit_target().is_some(),
                                                )
                                            });
                                        let no_targets = count == 0;
                                        let multi = count != 1;
                                        menu
                                            .menu_with_disabled(
                                                this.t("edit").to_string(),
                                                Box::new(FileMenuEdit { owner }),
                                                !editable || busy,
                                            )
                                            .menu_with_disabled(
                                                this.t("download").to_string(),
                                                Box::new(FileMenuDownload { owner }),
                                                no_targets || busy,
                                            )
                                            .menu_with_disabled(
                                                this.t("copy").to_string(),
                                                Box::new(FileMenuCopy { owner }),
                                                no_targets || busy,
                                            )
                                            .menu_with_disabled(
                                                this.t("paste").to_string(),
                                                Box::new(FileMenuPaste { owner }),
                                                !can_paste || busy,
                                            )
                                            // Destructive actions follow the safe file
                                            // operations; refresh remains in the toolbar.
                                            .menu_with_disabled(
                                                this.t("mkdir").to_string(),
                                                Box::new(FileMenuMkdir { owner }),
                                                busy,
                                            )
                                            .separator()
                                            .menu_with_disabled(
                                                this.t("rename").to_string(),
                                                Box::new(FileMenuRename { owner }),
                                                multi || no_targets || busy,
                                            )
                                            .menu_element_with_disabled(
                                                Box::new(FileMenuDelete { owner }),
                                                no_targets || busy,
                                                {
                                                    let label = this.t("delete").to_string();
                                                    let color = theme::Palette::new(this.prefs.theme);
                                                    move |_, _| {
                                                        div()
                                                            .text_color(if no_targets || busy {
                                                                color.muted
                                                            } else {
                                                                color.error
                                                            })
                                                            .child(label.clone())
                                                    }
                                                },
                                            )
                                    }
                                }),
                            ),
                    )
            // The horizontal indicator overlays the stage's bottom edge on
            // the non-scrolling shell — same arrangement as the tree pane.
            .child(self.overlay_scrollbar_x(
                "files-columns-scrollbar-x",
                pane.files.scroll_x.clone(),
                Resize::FileScrollX(owner),
                cx,
            )),
        );
        if pane.files.loaded
            && pane.files.entries.is_empty()
            && !pane.files.loading
            && pane.files.error.is_none()
        {
            content = content.child(
                div()
                    .p_3()
                    .text_color(p.muted)
                    .child(self.t("empty_directory")),
            );
        }
        content.into_any_element()
    }
    /// Capture the current selection as this session's copy buffer.
    pub(super) fn copy_file_selection(&mut self, owner: Owner, cx: &mut Context<Self>) {
        if let Some(pane) = self.pane_mut(owner) {
            if pane.state == ConnectionState::Connected && !pane.files.loading {
                let paths = pane.files.selected_paths();
                if !paths.is_empty() {
                    pane.files.clipboard = Some(paths);
                }
            }
        }
        cx.notify();
    }
    /// Paste the copy buffer into the directory on screen: name collisions
    /// are rejected up front so the remote copy never nests or overwrites.
    pub(super) fn paste_file_clipboard(&mut self, owner: Owner, cx: &mut Context<Self>) {
        let exists_label = self.t("paste_exists").to_string();
        let Some(pane) = self.pane_mut(owner) else {
            return;
        };
        if pane.state != ConnectionState::Connected || pane.files.loading {
            return;
        }
        let Some(sources) = pane.files.clipboard.clone() else {
            return;
        };
        let target_dir = pane.files.path.clone();
        for source in &sources {
            let name = source.rsplit('/').next().unwrap_or(source);
            let destination = if target_dir == "/" {
                format!("/{}", name)
            } else {
                format!("{}/{}", target_dir, name)
            };
            if pane.files.entries.iter().any(|e| e.path == destination) {
                pane.files.error = Some(format!("{}: {}", name, exists_label));
                cx.notify();
                return;
            }
        }
        let request = Id::new_v4();
        self.backend.file_operation(
            owner,
            request,
            crate::services::FileOperation::Copy(sources, target_dir),
        );
        cx.notify();
    }
    /// Expand a tree directory (always fetching the freshest listing) or
    /// collapse it (releasing the cached subtree so memory and row state
    /// stay bounded no matter how much was browsed).
    pub(super) fn tree_toggle(&mut self, owner: Owner, path: &str, cx: &mut Context<Self>) {
        let ui = self.prefs.ui_size;
        let reveal_snapshot = self.tree_reveal.borrow().clone();
        let Some(pane) = self.pane_mut(owner) else {
            return;
        };
        if !matches!(pane.spec, SessionSpec::Ssh { .. }) || pane.state != ConnectionState::Connected
        {
            return;
        }
        if pane.files.tree_expanded.contains(path) {
            // Collapse: drop this node's children and every cached descendant
            // beneath it — nested caches, expanded marks and reveal anchors
            // under a collapsed node are unreachable until re-expanded (which
            // refetches anyway), so keeping them only bloats scrolling.
            let prefix = format!("{}/", path.trim_end_matches('/'));
            pane.files
                .tree_children
                .retain(|key, _| key != path && !key.starts_with(&prefix));
            pane.files
                .tree_expanded
                .retain(|key| key != path && !key.starts_with(&prefix));
            pane.files.tree_expanded.remove(path);
            // Drop a pending reveal whose target just left the rows: the
            // consumer re-queues unreachable targets every frame.
            let stale_reveal = match reveal_snapshot {
                Some((reveal_owner, ref reveal_path))
                    if reveal_owner == owner
                        && !pane
                            .files
                            .tree_rows
                            .iter()
                            .any(|(candidate, _)| candidate == reveal_path) =>
                {
                    true
                }
                _ => false,
            };
            pane.files.rebuild_tree_rows(ui);
            // Re-anchor the virtual list like a directory refresh does
            // (scroll_to_item normalizes the list state); a stale deep offset
            // left the scrolled state heavy until the next listing.
            pane.files
                .tree_scroll
                .scroll_to_item(0, gpui::ScrollStrategy::Top);
            let _ = pane;
            if stale_reveal {
                self.tree_reveal.borrow_mut().take();
            }
            cx.notify();
            return;
        }
        let _ = pane;
        self.tree_expand(owner, path, cx);
    }
    /// Expand one tree node and fetch its children. `tree_toggle` uses this
    /// for its expand path, and the first-listing root auto-load calls it
    /// directly so an already-expanded root still fetches instead of taking
    /// the collapse branch.
    pub(super) fn tree_expand(&mut self, owner: Owner, path: &str, cx: &mut Context<Self>) {
        let ui = self.prefs.ui_size;
        let Some(pane) = self.pane_mut(owner) else {
            return;
        };
        if !matches!(pane.spec, SessionSpec::Ssh { .. }) || pane.state != ConnectionState::Connected
        {
            return;
        }
        // Expand: even a cached node refetches so the listing is current;
        // the fetch keeps showing the cached children until it lands.
        let request = Id::new_v4();
        pane.files.tree_request = Some((request, path.to_string()));
        pane.files.tree_expanded.insert(path.to_string());
        pane.files.rebuild_tree_rows(ui);
        let path = path.to_string();
        self.backend.list_files(owner, request, path);
        cx.notify();
    }
    /// Render one flattened tree row. The uniform list only calls this for
    /// visible indexes, so mouse handlers resolve live state through the
    /// workbench entity without rebuilding off-screen descendants.
    pub(super) fn render_tree_row(
        &self,
        owner: Owner,
        path: &str,
        depth: usize,
        content_width: f32,
        entity: Entity<Workbench>,
    ) -> Stateful<Div> {
        let p = theme::Palette::new(self.prefs.theme);
        let pane = self.pane(owner);
        let expanded = pane.is_some_and(|pane| pane.files.tree_expanded.contains(path));
        let has_children = pane.is_some_and(|pane| pane.files.tree_children.contains_key(path));
        let tree_loading = pane.is_some_and(|pane| {
            pane.files
                .tree_request
                .as_ref()
                .is_some_and(|(_, pending)| pending == path)
        });
        let node_loading = pane.is_some_and(|pane| {
            pane.files.loading
                && pane
                    .files
                    .tree_loading_path
                    .as_ref()
                    .is_some_and(|pending| pending == path)
        });
        let is_current = pane.is_some_and(|pane| pane.files.path == path);
        let name = path.rsplit('/').next().unwrap_or(path);
        let display: SharedString = if name.is_empty() {
            "/".into()
        } else {
            name.to_string().into()
        };
        let row_height = self.prefs.ui_size + 6.;
        let indent = depth as f32 * 14. + 4.;
        let row_path = path.to_string();
        let toggle_path = row_path.clone();
        let toggle_entity = entity.clone();
        let click_path = row_path.clone();
        let click_entity = entity;
        let mut row = div()
            .id(SharedString::from(format!("tree:{}", path)))
            .flex()
            .flex_shrink_0()
            .w(px(content_width))
            .h(px(row_height))
            .items_center()
            .gap(px(theme::SPACE_SMALL))
            .pl(px(indent))
            .pr(px(theme::SPACE_PANEL))
            .rounded(px(3.))
            .bg(if is_current {
                p.selected
            } else {
                gpui::transparent_black()
            })
            .when(!is_current, |row| row.hover(|style| style.bg(p.background)))
            .when(depth > 0, |row| {
                row.border_l_1().border_color(p.border.opacity(0.5))
            })
            .cursor_pointer()
            .on_click(move |_, _, cx| {
                click_entity.update(cx, |this, cx| {
                    if let Some(pane) = this.pane_mut(owner) {
                        pane.files.tree_loading_path = Some(click_path.clone());
                    }
                    this.navigate(owner, Some(click_path.clone()), cx);
                    let already_expanded = this
                        .pane(owner)
                        .is_some_and(|pane| pane.files.tree_expanded.contains(&click_path));
                    if !already_expanded {
                        this.tree_toggle(owner, &click_path, cx);
                    }
                });
            });
        row = row.child(
            div()
                .id(SharedString::from(format!("tree-chev:{}", path)))
                .flex()
                .items_center()
                .justify_center()
                .size(px(self.prefs.ui_size + 2.))
                .flex_shrink_0()
                .cursor_pointer()
                .when(!tree_loading, |chevron| {
                    chevron.on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        toggle_entity.update(cx, |this, cx| {
                            this.tree_toggle(owner, &toggle_path, cx);
                        });
                        cx.stop_propagation();
                    })
                })
                .child(if tree_loading {
                    div().text_color(p.muted).child("…").into_any_element()
                } else if has_children || expanded {
                    gpui::svg()
                        .path(if expanded {
                            "icons/chevron-down.svg"
                        } else {
                            "icons/chevron-right.svg"
                        })
                        .size(px(self.prefs.ui_size - 2.))
                        .text_color(p.muted)
                        .into_any_element()
                } else {
                    div().into_any_element()
                }),
        );
        // The folder icon enters its loading state for BOTH fetch paths:
        // a row click driving the list (node_loading) and a chevron expand
        // fetching this node's children (tree_loading).
        row.child(if node_loading || tree_loading {
            div()
                .flex()
                .items_center()
                .justify_center()
                .size(px(self.prefs.ui_size))
                .flex_none()
                .child(gpui_component::spinner::Spinner::new().color(p.accent))
                .into_any_element()
        } else {
            gpui::svg()
                .path(if expanded {
                    "icons/folder-open.svg"
                } else {
                    "icons/folder-closed.svg"
                })
                .flex_none()
                .size(px(self.prefs.ui_size))
                .text_color(if expanded {
                    p.accent
                } else if is_current {
                    p.text
                } else {
                    p.muted
                })
                .into_any_element()
        })
        .child(
            div()
                .text_color(if is_current { p.text } else { p.muted })
                .whitespace_nowrap()
                .child(display),
        )
    }

    /// Transfer history as the transfers dialog body: one row per record
    /// (status | path | size | actions), separated by hairlines, scoped to
    /// the opening tab's session. Removal lives in the dialog footer;
    /// the modal body handles scrolling.
    pub(super) fn render_transfers(&self, owner: Owner, cx: &mut Context<Self>) -> AnyElement {
        let p = theme::Palette::new(self.prefs.theme);
        let records: Vec<&TransferRecord> = self
            .transfers
            .iter()
            .rev()
            .filter(|task| task.session == Some(owner.session))
            .collect();
        if records.is_empty() {
            return div()
                .p(px(theme::SPACE_SECTION))
                .text_color(p.muted)
                .child(self.t("transfer_empty"))
                .into_any_element();
        }
        div()
            .id("transfers")
            .flex()
            .flex_col()
            .children(records.iter().enumerate().map(|(index, task)| {
                let task = *task;
                let id = task.id;
                // Status: a percentage while transferring, a directional icon
                // once finished; other states keep their text.
                let direction_key = if task.upload {
                    "uploading"
                } else {
                    "downloading"
                };
                let status: AnyElement = match (task.upload, &task.state) {
                    (_, TransferState::Running) => div()
                        .text_color(p.accent)
                        .child(match task.total {
                            Some(total) if total > 0 => format!(
                                "{} {}%",
                                self.t(direction_key),
                                (task.bytes as f32 / total as f32 * 100.).min(100.) as u32
                            ),
                            _ => self.t(direction_key).to_string(),
                        })
                        .into_any_element(),
                    (upload, TransferState::Completed) => gpui::svg()
                        .path(if upload {
                            "icons/upload.svg"
                        } else {
                            "icons/download.svg"
                        })
                        .size(px(14.))
                        .flex_shrink_0()
                        .text_color(p.meter_green)
                        .into_any_element(),
                    (_, state) => {
                        let key = match state {
                            TransferState::Queued => "queued",
                            TransferState::Failed => "failed",
                            TransferState::Cancelled => "cancelled",
                            TransferState::Interrupted => "interrupted",
                            _ => "completed",
                        };
                        let color = match state {
                            TransferState::Queued => p.muted,
                            _ => p.error,
                        };
                        div()
                            .text_color(color)
                            .child(self.t(key))
                            .into_any_element()
                    }
                };
                // Active tasks can be cancelled; failed tasks can be retried,
                // and completed downloads can be revealed locally.
                let mut actions = div().flex().items_center().gap(px(8.)).flex_shrink_0();
                if task.state.active() {
                    actions = actions.child(
                        self.button(("cancel-transfer", id.as_u128() as u64), self.t("cancel"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.cancel_transfer(id, cx);
                            })),
                    );
                } else {
                    if matches!(
                        task.state,
                        TransferState::Failed
                            | TransferState::Interrupted
                            | TransferState::Cancelled
                    ) {
                        let record = task.clone();
                        actions = actions.child(
                            self.button(("retry-transfer", id.as_u128() as u64), self.t("retry"))
                                .on_click(cx.listener(move |this, _, w, cx| {
                                    this.retry_transfer(record.clone(), w, cx)
                                })),
                        );
                    }
                    if !task.upload && task.state == TransferState::Completed {
                        let local = task.local.clone();
                        actions = actions.child(
                            self.button(("reveal-transfer", id.as_u128() as u64), "")
                                .svg_icon("icons/folder-search.svg")
                                .ghost()
                                .tooltip(self.t("reveal"))
                                .on_click(move |_, _, cx| {
                                    cx.reveal_path(std::path::Path::new(&local))
                                }),
                        );
                    }
                }
                let row_selected = self.transfer_selected.contains(&task.id);
                let select_id = task.id;
                div()
                    .w_full()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(8.))
                    .py(px(8.))
                    .when(row_selected, |row| row.bg(p.selected))
                    .when(index + 1 < records.len(), |row| {
                        row.border_b_1().border_color(p.border)
                    })
                    // Row selection follows the library-list rules: plain
                    // click selects one, Shift extends from the anchor,
                    // Cmd/Ctrl toggles. Action buttons sit behind their own
                    // mouse-down guard below so clicks there never select.
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                            let visible: Vec<Id> = this
                                .transfers
                                .iter()
                                .rev()
                                .filter(|task| task.session == Some(owner.session))
                                .map(|task| task.id)
                                .collect();
                            let Some(index) = visible.iter().position(|row| *row == select_id)
                            else {
                                return;
                            };
                            let additive = event.modifiers.control || event.modifiers.platform;
                            if event.modifiers.shift {
                                let anchor = this
                                    .transfer_anchor
                                    .or_else(|| this.transfer_selected.iter().next().copied())
                                    .filter(|anchor| visible.contains(anchor))
                                    .unwrap_or(select_id);
                                if let Some(start) = visible.iter().position(|row| *row == anchor) {
                                    if !additive {
                                        this.transfer_selected.clear();
                                    }
                                    for row in &visible[start.min(index)..=start.max(index)] {
                                        this.transfer_selected.insert(*row);
                                    }
                                }
                            } else if additive {
                                if this.transfer_selected.contains(&select_id) {
                                    this.transfer_selected.remove(&select_id);
                                } else {
                                    this.transfer_selected.insert(select_id);
                                }
                            } else {
                                this.transfer_selected.clear();
                                this.transfer_selected.insert(select_id);
                            }
                            if !event.modifiers.shift {
                                this.transfer_anchor = Some(select_id);
                            }
                            cx.notify();
                        }),
                    )
                    // Status column: content-sized, leftmost.
                    .child(div().flex().items_center().flex_shrink_0().child(status))
                    // Wrap the remote path within its column, including long filenames.
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .whitespace_normal()
                            .child(task.remote.clone()),
                    )
                    // Size column
                    .child(
                        div()
                            .w(px(90.))
                            .flex()
                            .justify_end()
                            .flex_shrink_0()
                            .whitespace_nowrap()
                            .text_color(p.muted)
                            .child(crate::monitor::bytes(task.total.unwrap_or(task.bytes))),
                    )
                    .child(
                        // Guard the action buttons from row selection, the
                        // same interception the connection rows use.
                        div()
                            .flex()
                            .items_center()
                            .flex_shrink_0()
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .child(actions),
                    )
                    .when_some(task.error.clone(), |row, error| {
                        // w_full forces the error onto its own line under the row.
                        row.child(div().w_full().min_w_0().text_color(p.error).child(error))
                    })
            }))
            .into_any_element()
    }
}
