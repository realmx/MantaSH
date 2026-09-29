//! Compact forms and fixed-target confirmations.
use super::controls::Button;
use super::transfer_paths::transfer_progress_fraction;
use super::*;
use crate::{
    connections::{self, ImportPreview},
    credentials::SecretStore,
    encoding::Encoding,
    model::{Preferences, TransferRecord},
    services::FileOperation,
};
use gpui_component::{
    IconName,
    checkbox::Checkbox,
    menu::PopupMenuItem,
    plot::shape::{Arc as ProgressArc, ArcData},
};

pub(super) struct ProfileForm {
    id: Id,
    name: Entity<InputState>,
    host: Entity<InputState>,
    port: Entity<InputState>,
    username: Entity<InputState>,
    secret: Entity<InputState>,
    secret_request: Id,
    secret_loading: bool,
    remember: bool,
    reconnect: Option<Owner>,
    error: Option<String>,
    existing: bool,
    cloning: bool,
    connecting: bool,
}
#[cfg(debug_assertions)]
impl ProfileForm {
    /// Expose only form identity and layout state to the isolated driver, never authentication input.
    pub(super) fn qa_metadata(&self, cx: &App) -> serde_json::Value {
        serde_json::json!({"id":self.id,"name":self.name.read(cx).value(),
            "cloning":self.cloning,"existing":self.existing,
            "secret_present":!self.secret.read(cx).value().is_empty(),
            "secret_loading":self.secret_loading})
    }
}
#[derive(Clone)]
pub(super) enum CloseTarget {
    Window,
    /// Install a verified update after the same draft/transfer checks as a window close.
    Update,
    Tab(Id),
    Pane(Owner),
    Document(Owner, Id),
    /// Closing the editor dialog itself: drafts stay with the session, so an
    /// unsaved draft is either saved or reverted, never dropped silently.
    Editor(Owner),
}
pub(super) enum Modal {
    Credentials {
        owner: Owner,
        reason: crate::credentials::PromptReason,
        secret: Entity<InputState>,
        /// Drives the eye toggle: false keeps the input masked, true reveals
        /// the typed secret in place.
        show_secret: bool,
        reply: Option<tokio::sync::oneshot::Sender<crate::credentials::CredentialReply>>,
    },
    Connections,
    LocalHistory,
    DeleteHistory {
        owner: Owner,
        scope: HistoryScope,
        label: String,
        ids: Vec<Id>,
    },
    FollowLink {
        owner: Owner,
        link: String,
        target: String,
        directory: bool,
    },
    Profile(ProfileForm),
    Settings,
    /// Fallback for platforms without the AppKit Window menu.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    WindowControls(window_actions::WindowForm),
    DeleteProfiles {
        profiles: Vec<Profile>,
    },
    Import {
        preview: ImportPreview,
        replace: bool,
    },
    About,
    Update,
    Trust {
        owner: Owner,
        host: String,
        port: u16,
        previous: Option<String>,
        fingerprint: String,
        reply: Option<tokio::sync::oneshot::Sender<bool>>,
    },
    FileName {
        owner: Owner,
        directory: String,
        original: Option<String>,
        input: Entity<InputState>,
    },
    DeleteFiles {
        owner: Owner,
        paths: Vec<String>,
    },
    Close(CloseTarget),
    Conflict {
        owner: Owner,
        document: Id,
    },
    Encoding {
        owner: Option<Owner>,
        document: Option<Id>,
    },
    Transfer {
        owner: Owner,
        batch: Id,
        records: Vec<TransferRecord>,
        overwrite: bool,
        phase: TransferPhase,
        review_origin: Option<(String, Option<Id>)>,
    },
    Font {
        terminal: bool,
        filter: Entity<InputState>,
    },
    ResourceDetails {
        owner: Owner,
        timestamp: i64,
        data: system::ResourceSnapshot,
    },
    ProcessDetails {
        owner: Owner,
        process: crate::monitor::Process,
        request: Id,
        result: Option<Result<crate::processes::Details, String>>,
        refreshing: bool,
        refresh_error: Option<String>,
        /// Isolated debug fixture; its action buttons and signal route are disabled.
        preview: bool,
    },
    ProcessConfirm {
        owner: Owner,
        process: crate::monitor::Process,
        action: crate::processes::Action,
        host: String,
    },
    /// Processes and ports lists opened from the SSH toolbar as dialogs.
    SystemTools {
        owner: Owner,
        page: crate::model::SystemPage,
    },
    /// Remote file editor opened from the file list; the documents live on
    /// the pane and survive closing the dialog.
    Editor {
        owner: Owner,
    },
    /// Transfer history for one session tab, opened from the files toolbar.
    Transfers {
        owner: Owner,
    },
    /// Confirmation for stopping the unfinished entries of one submitted batch.
    CancelTransfers {
        owner: Owner,
        batch: Id,
        records: Vec<TransferRecord>,
        overwrite: bool,
        stop_ids: Vec<Id>,
        scroll: ScrollHandle,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TransferPhase {
    Review,
    Running,
    Result,
}

impl Modal {
    /// Consequential review dialogs keep their established action-specific
    /// close behavior instead of participating in the ordinary parent stack.
    pub(super) fn is_confirmation(&self) -> bool {
        matches!(
            self,
            Modal::DeleteHistory { .. }
                | Modal::DeleteProfiles { .. }
                | Modal::FollowLink { .. }
                | Modal::Trust { .. }
                | Modal::Import { .. }
                | Modal::DeleteFiles { .. }
                | Modal::Close(_)
                | Modal::Update
                | Modal::Conflict { .. }
                | Modal::Transfer { .. }
                | Modal::CancelTransfers { .. }
                | Modal::ProcessConfirm { .. }
        )
    }

    /// Stable, secret-free names used by the debug QA snapshot.
    pub(super) fn qa_key(&self) -> &'static str {
        match self {
            Modal::Credentials { .. } => "credentials",
            Modal::Connections => "connections",
            Modal::LocalHistory => "history",
            Modal::DeleteHistory { .. } => "delete_history",
            Modal::FollowLink { .. } => "follow_link",
            Modal::Profile(_) => "profile",
            Modal::Settings => "settings",
            Modal::WindowControls(_) => "window_controls",
            Modal::DeleteProfiles { .. } => "delete_profiles",
            Modal::Import { .. } => "import",
            Modal::About => "about",
            Modal::Update => "update",
            Modal::Trust { .. } => "trust",
            Modal::FileName { .. } => "file_name",
            Modal::DeleteFiles { .. } => "delete_files",
            Modal::Conflict { .. } => "conflict",
            Modal::Close(_) => "close",
            Modal::Encoding { .. } => "encoding",
            Modal::Transfer { .. } => "transfer",
            Modal::Font { .. } => "font",
            Modal::ResourceDetails { .. } => "resource_details",
            Modal::ProcessDetails { .. } => "process_details",
            Modal::ProcessConfirm { .. } => "process_confirm",
            Modal::SystemTools { .. } => "system_tools",
            Modal::Editor { .. } => "editor",
            Modal::Transfers { .. } => "transfers",
            Modal::CancelTransfers { .. } => "cancel_transfers",
        }
    }
}
pub(super) struct ModalFrame {
    pub(super) modal: Modal,
    pub(super) scroll: ScrollHandle,
    pub(super) focus: FocusHandle,
    /// The concrete input/button focus that was active before the child opened.
    pub(super) focused: Option<FocusHandle>,
    pub(super) font_highlight: Option<String>,
    pub(super) font_scroll: UniformListScrollHandle,
    pub(super) resource_height: Option<f32>,
}

impl Workbench {
    /// Reset all per-instance handles before rendering a newly opened modal.
    fn reset_modal_state(&mut self, cx: &mut Context<Self>) {
        self.modal_focus = cx.focus_handle();
        self.modal_scroll = ScrollHandle::new();
        self.font_highlight = None;
        self.font_scroll = UniformListScrollHandle::new();
        self.resource_modal_height = None;
    }

    /// Move the active modal and every piece of instance state into one frame.
    /// This is what keeps a child layout from changing its parent's scroll.
    fn suspend_modal(
        &mut self,
        modal: Modal,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> ModalFrame {
        ModalFrame {
            modal,
            scroll: std::mem::replace(&mut self.modal_scroll, ScrollHandle::new()),
            focus: std::mem::replace(&mut self.modal_focus, cx.focus_handle()),
            focused: window.focused(cx),
            font_highlight: self.font_highlight.take(),
            font_scroll: std::mem::replace(&mut self.font_scroll, UniformListScrollHandle::new()),
            resource_height: self.resource_modal_height.take(),
        }
    }

    /// Recompute the compatibility return markers used by the header and QA.
    fn refresh_return_markers(&mut self) {
        self.connection_return = self
            .modal_stack
            .last()
            .is_some_and(|frame| matches!(&frame.modal, Modal::Connections));
        self.history_return = self
            .modal_stack
            .last()
            .and_then(|frame| {
                matches!(&frame.modal, Modal::LocalHistory).then(|| self.active_owner())
            })
            .flatten();
    }

    fn clear_return_markers(&mut self) {
        self.connection_return = false;
        self.history_return = None;
        self.editor_return = None;
    }

    /// Clear all in-memory dialog-chain state. Persistence never includes this.
    fn clear_modal_chain(&mut self) {
        self.modal_stack.clear();
        self.modal_confirm_return = None;
        self.clear_return_markers();
    }

    /// Repeated menu commands update the same dialog without creating a self-parenting cycle.
    fn same_modal_instance(current: &Modal, next: &Modal) -> bool {
        matches!((current, next),
            (Modal::Editor { owner: left }, Modal::Editor { owner: right }) if left == right
        ) || matches!(
            (current, next),
            (Modal::Settings, Modal::Settings) | (Modal::Update, Modal::Update)
        )
    }

    pub(super) fn show_modal(&mut self, modal: Modal, window: &mut Window, cx: &mut Context<Self>) {
        if self.update_handoff_active() {
            return;
        }
        if matches!(
            self.modal,
            Some(Modal::Update | Modal::Close(CloseTarget::Update))
        ) && !matches!(modal, Modal::Update | Modal::Close(CloseTarget::Update))
        {
            self.cancel_update();
        }
        self.compact_sidebar_open = false;
        if matches!(&modal, Modal::LocalHistory) {
            self.notice = None;
        }
        if self
            .modal
            .as_ref()
            .is_some_and(|current| Self::same_modal_instance(current, &modal))
        {
            cx.notify();
            return;
        }

        let confirmation = modal.is_confirmation();
        if let Some(current) = self.modal.take() {
            let current_confirmation = current.is_confirmation();
            if !current_confirmation && !confirmation {
                let from_connections = matches!(&current, Modal::Connections);
                let frame = self.suspend_modal(current, window, cx);
                self.modal_stack.push(frame);
                self.modal_confirm_return = None;
                self.connection_return = from_connections;
                self.history_return = None;
                self.editor_return = None;
            } else if confirmation && !current_confirmation {
                let connection_parent = matches!(&current, Modal::Connections)
                    && matches!(&modal, Modal::DeleteProfiles { .. } | Modal::Import { .. });
                let history_parent = matches!(&current, Modal::LocalHistory)
                    && matches!(&modal, Modal::DeleteHistory { .. });
                let editor_parent = match &current {
                    Modal::Editor { .. }
                        if matches!(
                            &modal,
                            Modal::Conflict { .. }
                                | Modal::Close(CloseTarget::Document(..) | CloseTarget::Editor(..))
                        ) =>
                    {
                        true
                    }
                    _ => false,
                };
                let process_parent = matches!((&current, &modal),
                    (Modal::ProcessDetails { owner: left, process, .. }, Modal::ProcessConfirm { owner: right, process: target, .. })
                    if left == right && process.identity == target.identity);
                let update_parent = matches!((&current, &modal), (Modal::Settings, Modal::Update));
                let captures_parent = connection_parent
                    || history_parent
                    || editor_parent
                    || process_parent
                    || update_parent;
                if captures_parent {
                    let owner = match &current {
                        Modal::Editor { owner } => Some(*owner),
                        _ => None,
                    };
                    self.modal_confirm_return = Some(self.suspend_modal(current, window, cx));
                    self.connection_return = connection_parent;
                    self.history_return = history_parent.then(|| self.active_owner()).flatten();
                    self.editor_return = editor_parent.then_some(owner).flatten();
                } else {
                    // Other confirmations retain their existing page-level behavior.
                    self.modal_confirm_return = None;
                    self.clear_return_markers();
                }
            } else {
                // A confirmation completion that opens a fresh ordinary view
                // starts a new chain rather than reviving its old context.
                self.clear_modal_chain();
            }
        } else {
            self.clear_modal_chain();
            self.return_focus = window.focused(cx);
        }

        if matches!(modal, Modal::Transfers { .. }) {
            self.transfer_selected.clear();
            self.transfer_anchor = None;
        }
        self.reset_modal_state(cx);
        self.modal = Some(modal);
        self.modal_focus.focus(window);
        cx.notify();
    }

    /// Focus the first useful control when an old frame has no concrete focus
    /// (for example when its former control was removed by a data update).
    fn focus_restored_modal(&self, window: &mut Window, cx: &mut App) {
        let Some(modal) = &self.modal else {
            self.modal_focus.focus(window);
            return;
        };
        match modal {
            Modal::Connections => self.connection_search.focus_handle(cx).focus(window),
            Modal::Credentials { secret, .. } => secret.focus_handle(cx).focus(window),
            Modal::Profile(form) => {
                let input = if form.reconnect.is_some() {
                    &form.secret
                } else {
                    &form.name
                };
                input.focus_handle(cx).focus(window);
            }
            Modal::LocalHistory => {
                if let Some(pane) = self.active_pane() {
                    self.history_views[pane.spec.history_scope().index()]
                        .search
                        .focus_handle(cx)
                        .focus(window);
                } else {
                    self.modal_focus.focus(window);
                }
            }
            Modal::Font { filter, .. } => filter.focus_handle(cx).focus(window),
            Modal::SystemTools { owner, page } => {
                let filter = self.pane(*owner).map(|pane| match page {
                    crate::model::SystemPage::Processes => pane.process_filter.clone(),
                    crate::model::SystemPage::Ports => pane.port_filter.clone(),
                    crate::model::SystemPage::Overview => pane.process_filter.clone(),
                });
                if let Some(filter) = filter {
                    filter.focus_handle(cx).focus(window);
                } else {
                    self.modal_focus.focus(window);
                }
            }
            Modal::Editor { owner } => {
                if let Some(document) = self.pane(*owner).and_then(|pane| {
                    pane.active_document
                        .and_then(|id| pane.documents.iter().find(|document| document.id == id))
                }) {
                    document.input.focus_handle(cx).focus(window);
                } else {
                    self.modal_focus.focus(window);
                }
            }
            _ => self.modal_focus.focus(window),
        }
    }

    /// Restore one exact parent frame, including its scroll and focus state.
    fn restore_modal(&mut self, frame: ModalFrame, window: &mut Window, cx: &mut Context<Self>) {
        let ModalFrame {
            modal,
            scroll,
            focus,
            focused,
            font_highlight,
            font_scroll,
            resource_height,
        } = frame;
        self.modal = Some(modal);
        self.modal_scroll = scroll;
        self.modal_focus = focus;
        self.font_highlight = font_highlight;
        self.font_scroll = font_scroll;
        self.resource_modal_height = resource_height;
        self.modal_confirm_return = None;
        self.editor_return = None;
        self.refresh_return_markers();
        cx.notify();
        if let Some(focused) = focused {
            focused.focus(window);
        } else {
            self.focus_restored_modal(window, cx);
        }
    }

    pub(super) fn restore_confirmation_parent(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(frame) = self.modal_confirm_return.take() else {
            return false;
        };
        self.restore_modal(frame, window, cx);
        true
    }

    /// Close the active modal and intentionally abandon every parent. Used by
    /// actions that leave the overlay entirely, such as opening a new session.
    fn dismiss_to_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.clear_modal_chain();
        self.dismiss(window, cx);
    }

    pub(super) fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.update_handoff_active() {
            return;
        }
        if matches!(
            self.modal,
            Some(Modal::Update | Modal::Close(CloseTarget::Update))
        ) {
            self.cancel_update();
        }
        let Some(current) = self.modal.take() else {
            return;
        };
        let current_is_confirmation = current.is_confirmation();
        let cancelled_owner = match &current {
            Modal::Trust {
                owner,
                reply: Some(_),
                ..
            }
            | Modal::Credentials {
                owner,
                reply: Some(_),
                ..
            } => Some(*owner),
            _ => None,
        };
        if let Some(owner) = cancelled_owner {
            self.backend.close(owner);
            self.queued_credentials.retain(
                |event| !matches!(event, Event::Credentials { owner: pending, .. } if *pending == owner),
            );
            if let Some(pane) = self.pane_mut(owner) {
                pane.state = ConnectionState::Cancelled;
            }
        }

        // Conflicts return to the live editor. A document close returns only
        // while the editor still has documents; closing the editor itself has
        // explicitly cleared `editor_return` before it reaches this method.
        let restore_editor = match &current {
            Modal::Conflict { .. } => true,
            Modal::Close(CloseTarget::Document(owner, _)) => self
                .pane(*owner)
                .is_some_and(|pane| !pane.documents.is_empty()),
            Modal::Close(CloseTarget::Editor(_)) => self.editor_return.is_some(),
            Modal::ProcessConfirm { .. } | Modal::Update => true,
            _ => false,
        };
        if restore_editor && self.restore_confirmation_parent(window, cx) {
            return;
        }

        if !current_is_confirmation {
            if let Some(frame) = self.modal_stack.pop() {
                self.restore_modal(frame, window, cx);
                return;
            }
        }

        // Consequential confirmations and explicit page transitions finish
        // the current overlay chain rather than reviving an older parent.
        self.clear_modal_chain();
        if let Some(focus) = self.return_focus.take() {
            focus.focus(window);
        } else {
            self.focus_active(window, cx);
        }
        cx.notify();
    }

    /// Return from connection management without discarding the search or list position.
    pub(super) fn back_to_connections(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let confirm_parent = self
            .modal_confirm_return
            .as_ref()
            .is_some_and(|frame| matches!(&frame.modal, Modal::Connections));
        if confirm_parent && self.restore_confirmation_parent(window, cx) {
            self.connection_search.focus_handle(cx).focus(window);
            return;
        }
        let ordinary_parent = self
            .modal_stack
            .last()
            .is_some_and(|frame| matches!(&frame.modal, Modal::Connections));
        if ordinary_parent {
            if let Some(frame) = self.modal_stack.pop() {
                self.restore_modal(frame, window, cx);
                self.connection_search.focus_handle(cx).focus(window);
                return;
            }
        }
        self.modal = None;
        self.clear_modal_chain();
        self.show_modal(Modal::Connections, window, cx);
        self.connection_search.focus_handle(cx).focus(window);
    }

    /// Cancel a management step back to its library; authentication keeps its own cancellation.
    pub(super) fn cancel_modal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.update_handoff_active() {
            return;
        }
        if matches!(
            self.modal,
            Some(Modal::Update | Modal::Close(CloseTarget::Update))
        ) {
            // Cancel the request before restoring Settings, so late results cannot reopen it.
            self.cancel_update();
        }
        // Closing the editor with unsaved drafts asks save-or-revert first;
        // clean documents close without interruption.
        if let Some(Modal::Editor { owner }) = &self.modal {
            let owner = *owner;
            if self
                .pane(owner)
                .is_some_and(|p| p.documents.iter().any(|d| d.dirty))
            {
                self.show_modal(Modal::Close(CloseTarget::Editor(owner)), window, cx);
                return;
            }
        }
        if let Some(Modal::CancelTransfers {
            owner,
            batch,
            records,
            overwrite,
            scroll,
            ..
        }) = &self.modal
        {
            let scroll = scroll.clone();
            let restored = Modal::Transfer {
                owner: *owner,
                batch: *batch,
                records: records.clone(),
                overwrite: *overwrite,
                phase: if self.transfer_batches.contains_key(batch) {
                    TransferPhase::Running
                } else {
                    TransferPhase::Result
                },
                review_origin: None,
            };
            self.modal = Some(restored);
            self.modal_scroll = scroll;
            self.modal_focus.focus(window);
            cx.notify();
            return;
        }
        // These confirmations have an explicit parent instance. Restoring it
        // here keeps cancellation distinct from a page-level completion.
        if self.modal.as_ref().is_some_and(Modal::is_confirmation)
            && self.restore_confirmation_parent(window, cx)
        {
            return;
        }
        // Compatibility fallback for state created before the frame return
        // context was available.
        if self.history_return.is_some() {
            self.history_return = None;
            self.show_modal(Modal::LocalHistory, window, cx);
        } else if self.connection_return {
            self.back_to_connections(window, cx);
        } else {
            self.dismiss(window, cx);
        }
    }

    /// Fix the host and unfinished task ids before requesting a stop.
    pub(super) fn review_stop_transfers(
        &mut self,
        owner: Owner,
        batch: Id,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Modal::Transfer {
            owner: current,
            batch: current_batch,
            records,
            overwrite,
            phase: TransferPhase::Running,
            ..
        }) = &self.modal
        else {
            return;
        };
        if *current != owner
            || *current_batch != batch
            || !self
                .transfer_batches
                .get(&batch)
                .is_some_and(|b| b.owner == owner)
        {
            return;
        }
        let stop_ids = records
            .iter()
            .filter(|record| {
                self.transfers.iter().any(|task| {
                    task.id == record.id && task.belongs_to(owner) && task.state.active()
                })
            })
            .map(|record| record.id)
            .collect::<Vec<_>>();
        if stop_ids.is_empty() {
            return;
        }
        let modal = Modal::CancelTransfers {
            owner,
            batch,
            records: records.clone(),
            overwrite: *overwrite,
            stop_ids,
            scroll: self.modal_scroll.clone(),
        };
        self.show_modal(modal, window, cx);
    }

    /// Stop only the fixed unfinished ids still belonging to this exact batch.
    pub(super) fn stop_transfer_batch(
        &mut self,
        owner: Owner,
        batch: Id,
        records: Vec<TransferRecord>,
        overwrite: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(Modal::CancelTransfers {
            owner: current,
            batch: current_batch,
            stop_ids,
            scroll,
            ..
        }) = &self.modal
        else {
            return;
        };
        if *current != owner || *current_batch != batch {
            return;
        }
        let Some(batch_state) = self.transfer_batches.get(&batch) else {
            return;
        };
        if batch_state.owner != owner {
            return;
        }
        let ids = stop_ids
            .iter()
            .copied()
            .filter(|id| {
                batch_state.ids.contains(id)
                    && self
                        .transfers
                        .iter()
                        .any(|task| task.id == *id && task.belongs_to(owner) && task.state.active())
            })
            .collect::<Vec<_>>();
        let scroll = scroll.clone();
        for id in ids {
            self.cancel_transfer(id, cx);
        }
        self.modal = Some(Modal::Transfer {
            owner,
            batch,
            records,
            overwrite,
            phase: TransferPhase::Running,
            review_origin: None,
        });
        self.modal_scroll = scroll;
        cx.notify();
    }

    /// Remove only finished records currently visible in this transfer dialog.
    pub(super) fn remove_transfer_records(&mut self, owner: Owner, cx: &mut Context<Self>) {
        if !self.modal.as_ref().is_some_and(
            |modal| matches!(modal, Modal::Transfers { owner: current } if *current == owner),
        ) {
            return;
        }
        let ids: Vec<Id> = self
            .transfers
            .iter()
            .filter(|task| {
                task.session == Some(owner.session)
                    && !task.state.active()
                    && (self.transfer_selected.is_empty()
                        || self.transfer_selected.contains(&task.id))
            })
            .map(|task| task.id)
            .collect();
        if ids.is_empty() {
            return;
        }
        let targets: HashSet<Id> = ids.iter().copied().collect();
        self.transfers.retain(|task| !targets.contains(&task.id));
        self.transfer_selected.clear();
        self.transfer_anchor = None;
        let db = self.backend.database.clone();
        self.backend
            .runtime
            .spawn_blocking(move || db.lock().delete_transfers(&ids));
        cx.notify();
    }

    /// Drop a pending password reply when editing a credential prompt replaces
    /// the old connection attempt with a new attempt.
    pub(super) fn cancel_credential_prompt(&mut self, owner: Owner) {
        fn drop_reply(modal: &mut Modal, owner: Owner) {
            match modal {
                Modal::Credentials {
                    owner: target,
                    reply,
                    ..
                } if *target == owner => {
                    let _ = reply.take();
                }
                _ => {}
            }
        }
        if let Some(modal) = &mut self.modal {
            drop_reply(modal, owner);
        }
        for frame in &mut self.modal_stack {
            drop_reply(&mut frame.modal, owner);
        }
        if let Some(frame) = &mut self.modal_confirm_return {
            drop_reply(&mut frame.modal, owner);
        }
        self.queued_credentials.retain(
            |event| !matches!(event, Event::Credentials { owner: pending, .. } if *pending == owner),
        );
        self.backend.close(owner);
        if let Some(pane) = self.pane_mut(owner) {
            if pane.state == ConnectionState::CredentialsRequired {
                pane.state = ConnectionState::Cancelled;
            }
        }
    }
    /// Send one response to the verified attempt; the secret is never copied to profile data.
    pub(super) fn submit_credentials(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Modal::Credentials {
            owner,
            secret,
            reply,
            ..
        }) = &mut self.modal
        else {
            return;
        };
        let owner = *owner;
        let value = crate::credentials::CredentialReply {
            secret: zeroize::Zeroizing::new(secret.read(cx).value().to_string()),
            remember: true,
        };
        if value.secret.is_empty() {
            return;
        }
        let sender = reply.take();
        secret.update(cx, |input, cx| input.set_value("", window, cx));
        if self
            .pane(owner)
            .is_some_and(|p| p.state == ConnectionState::CredentialsRequired)
        {
            if let Some(sender) = sender {
                let _ = sender.send(value);
            }
        }
        self.dismiss(window, cx);
    }
    pub(super) fn profile_form(
        &mut self,
        profile: Option<Profile>,
        reconnect: Option<Owner>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_profile_form(profile, reconnect, true, window, cx);
    }
    /// Open a connection form without copying credentials into a newly cloned profile.
    fn profile_form_without_secret(
        &mut self,
        profile: Option<Profile>,
        reconnect: Option<Owner>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_profile_form(profile, reconnect, false, window, cx);
    }
    /// Build an edit form and load its saved credential off the UI thread when requested.
    fn open_profile_form(
        &mut self,
        profile: Option<Profile>,
        reconnect: Option<Owner>,
        load_secret: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let existing = profile.is_some();
        let profile = profile.unwrap_or(Profile {
            id: Id::new_v4(),
            name: String::new(),
            host: String::new(),
            port: 22,
            username: String::new(),
        });
        let secret_request = Id::new_v4();
        let secret_loading = existing && load_secret;
        let secret = Self::input("", "", false, window, cx);
        let secret_entity = secret.clone();
        let profile_id = profile.id;
        let form = ProfileForm {
            id: profile.id,
            name: Self::input(&profile.name, "Production", false, window, cx),
            host: Self::input(&profile.host, "192.168.1.10", false, window, cx),
            port: Self::input(&profile.port.to_string(), "22", false, window, cx),
            username: Self::input(&profile.username, "root", false, window, cx),
            secret,
            secret_request,
            secret_loading,
            remember: true,
            reconnect,
            error: None,
            existing,
            cloning: false,
            connecting: false,
        };
        let focus = if reconnect.is_some() {
            form.secret.focus_handle(cx)
        } else {
            form.name.focus_handle(cx)
        };
        self.show_modal(Modal::Profile(form), window, cx);
        focus.focus(window);
        if secret_loading {
            self.load_profile_secret(profile_id, secret_request, secret_entity, window, cx);
        }
    }
    fn profile_form_for_request(&mut self, request: Id) -> Option<&mut ProfileForm> {
        if let Some(Modal::Profile(form)) = self.modal.as_mut()
            && form.secret_request == request
        {
            return Some(form);
        }
        for frame in self.modal_stack.iter_mut().rev() {
            if let Modal::Profile(form) = &mut frame.modal
                && form.secret_request == request
            {
                return Some(form);
            }
        }
        if let Some(frame) = self.modal_confirm_return.as_mut()
            && let Modal::Profile(form) = &mut frame.modal
            && form.secret_request == request
        {
            return Some(form);
        }
        None
    }

    /// Load a saved password without blocking layout or replacing user input.
    fn load_profile_secret(
        &mut self,
        profile_id: Id,
        request: Id,
        secret: Entity<InputState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let vault = self.backend.vault.clone();
        let task = self
            .backend
            .runtime
            .spawn_blocking(move || vault.read(profile_id));
        cx.spawn_in(window, async move |view, cx| {
            let result = task.await;
            let _ = view.update_in(cx, |this, window, cx| {
                let Some(form) = this.profile_form_for_request(request) else {
                    return;
                };
                form.secret_loading = false;
                let fill = secret.read(cx).value().is_empty();
                if fill && let Ok(Ok(Some(value))) = &result {
                    secret.update(cx, |input, cx| {
                        input.set_value(value.to_string(), window, cx)
                    });
                }
                cx.notify();
            });
        })
        .detach();
    }
    pub(super) fn submit_profile(
        &mut self,
        connect: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Modal::Profile(form)) = &mut self.modal else {
            return;
        };
        let port = form.port.read(cx).value().parse::<u16>();
        if port.is_err() {
            form.error = Some("Port must be between 1 and 65535".into());
            cx.notify();
            return;
        }
        let mut profile = Profile {
            id: form.id,
            name: form.name.read(cx).value().to_string().trim().into(),
            host: form.host.read(cx).value().to_string().trim().into(),
            port: port.unwrap_or(22),
            username: form.username.read(cx).value().to_string().trim().into(),
        };
        if profile.name.trim().is_empty() {
            profile.name = profile.address();
        }
        if let Err(error) = profile.validate() {
            form.error = Some(error);
            cx.notify();
            return;
        }
        let secret = form.secret.read(cx).value().to_string();
        let remember = form.remember;
        let reconnect = form.reconnect;
        if let Some(existing) = self.profiles.iter_mut().find(|p| p.id == profile.id) {
            *existing = profile.clone();
        } else {
            self.profiles.push(profile.clone());
        }
        self.backend.save_profiles(self.profiles.clone());
        self.connection_selected = Some(profile.id);
        if !connections::matches_query(&profile, &self.connection_search.read(cx).value()) {
            self.connection_search
                .update(cx, |input, cx| input.set_value("", window, cx));
        }
        if connect {
            // A credential prompt is the parent of this form when reconnecting.
            // Cancel its sender before clearing the modal chain so the old
            // attempt cannot consume the replacement connection's response.
            if let Some(owner) = reconnect {
                self.cancel_credential_prompt(owner);
            }
            self.dismiss_to_page(window, cx);
        } else {
            // Save-only is an ordinary form completion: return to whichever
            // modal opened the form, preserving that modal instance.
            self.dismiss(window, cx);
        }
        if connect {
            if let Some(owner) = reconnect {
                if let Some(pane) = self.pane_mut(owner) {
                    // Reconnect keeps the live session's terminal encoding.
                    let encoding = pane.spec.encoding();
                    pane.spec = SessionSpec::Ssh { profile, encoding };
                }
                self.reconnect(owner, secret, remember, window, cx);
            } else {
                self.new_ssh(profile, secret, remember, true, window, cx);
            }
        }
    }
    pub(super) fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.local_shells = None;
        self.local_shells_loading = true;
        let task = self.backend.runtime.spawn_blocking(crate::platform::shells);
        cx.spawn_in(window, async move |view, cx| {
            let shells = task.await;
            let _ = view.update_in(cx, |this, _window, cx| {
                this.local_shells_loading = false;
                this.local_shells = shells.ok();
                cx.notify();
            });
        })
        .detach();
        self.show_modal(Modal::Settings, window, cx);
    }
    pub(super) fn open_github(&mut self, cx: &mut Context<Self>) {
        let task = self.backend.runtime.spawn_blocking(|| {
            #[cfg(target_os = "macos")]
            return std::process::Command::new("open")
                .arg("https://github.com/realmx/MantaSH")
                .status();
            #[cfg(target_os = "windows")]
            return std::process::Command::new("cmd")
                .args(["/C", "start", "", "https://github.com/realmx/MantaSH"])
                .status();
            #[cfg(all(unix, not(target_os = "macos")))]
            return std::process::Command::new("xdg-open")
                .arg("https://github.com/realmx/MantaSH")
                .status();
        });
        cx.spawn(async move |view, cx| {
            let failed = task.await.map_or(true, |result| {
                result.map_or(true, |status| !status.success())
            });
            if failed {
                let _ = view.update(cx, |this, cx| {
                    this.notice = Some("无法打开 GitHub 页面".into());
                    cx.notify();
                });
            }
        })
        .detach();
    }
    pub(super) fn open_font_picker(
        &mut self,
        terminal: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let filter = Self::input("", self.t("font_search"), false, window, cx);
        self.show_modal(Modal::Font { terminal, filter }, window, cx);
        if let Some(Modal::Font { filter, .. }) = &self.modal {
            filter.focus_handle(cx).focus(window);
        }
    }

    /// Apply one family and return to the exact Settings instance underneath.
    pub(super) fn select_font(
        &mut self,
        family: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Modal::Font { terminal, .. }) = &self.modal else {
            return;
        };
        let terminal = *terminal;
        self.font_highlight = Some(family.clone());
        if terminal {
            self.prefs.terminal_font = family;
        } else {
            self.prefs.ui_font = family;
        }
        self.apply_preferences(window, cx);
        self.dismiss(window, cx);
    }

    /// Open connection editing from a password prompt without dropping the
    /// prompt's sender, secret input, or parent focus state.
    pub(super) fn edit_credential_connection(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Modal::Credentials { owner, .. }) = &self.modal else {
            return;
        };
        let owner = *owner;
        let profile = self.pane(owner).and_then(|pane| match &pane.spec {
            SessionSpec::Ssh { profile, .. } => Some(profile.clone()),
            _ => None,
        });
        if let Some(profile) = profile {
            self.profile_form(Some(profile), Some(owner), window, cx);
        }
    }

    /// Open a clone as an unsaved form with a fresh credential identity and empty secret.
    pub(super) fn clone_connection(
        &mut self,
        profile: &Profile,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let profile = connections::clone_profile(
            profile,
            format!("{} {}", profile.name, self.t("copy_suffix")),
        );
        self.profile_form_without_secret(Some(profile), None, window, cx);
        if let Some(Modal::Profile(form)) = &mut self.modal {
            form.existing = false;
            form.cloning = true;
        }
    }
    pub(super) fn open_saved(
        &mut self,
        profile: Profile,
        new_instance: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dismiss_to_page(window, cx);
        self.new_ssh(profile, String::new(), false, new_instance, window, cx);
    }
    pub(super) fn request_close(
        &mut self,
        target: CloseTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.update_handoff_active() {
            return;
        }
        let owners = self.close_owners(&target);
        let dirty = self
            .tabs
            .iter()
            .flat_map(|t| &t.panes)
            .filter(|p| owners.contains(&p.owner))
            .flat_map(|p| &p.documents)
            .any(|d| {
                d.dirty
                    && match target {
                        CloseTarget::Document(_, id) => d.id == id,
                        _ => true,
                    }
            });
        // The editor dialog only guards its own drafts; live transfers belong
        // to the session and keep running behind it.
        let active_transfer =
            !matches!(target, CloseTarget::Document(_, _) | CloseTarget::Editor(_))
                && self
                    .transfers
                    .iter()
                    .any(|t| t.state.active() && owners.iter().any(|owner| t.belongs_to(*owner)));
        if dirty || active_transfer {
            self.show_modal(Modal::Close(target), window, cx);
        } else {
            self.finish_close(target, window, cx);
        }
    }
    pub(super) fn close_owners(&self, target: &CloseTarget) -> Vec<Owner> {
        match target {
            CloseTarget::Window | CloseTarget::Update => self
                .tabs
                .iter()
                .flat_map(|t| &t.panes)
                .map(|p| p.owner)
                .collect(),
            CloseTarget::Tab(id) => self
                .tabs
                .iter()
                .find(|t| t.id == *id)
                .into_iter()
                .flat_map(|t| &t.panes)
                .map(|p| p.owner)
                .collect(),
            CloseTarget::Pane(owner) => vec![*owner],
            CloseTarget::Editor(owner) => vec![*owner],
            CloseTarget::Document(owner, id) => self
                .tabs
                .iter()
                .flat_map(|t| &t.panes)
                .find(|p| p.documents.iter().any(|d| d.owner == *owner && d.id == *id))
                .map(|p| vec![p.owner])
                .unwrap_or_default(),
        }
    }
    pub(super) fn finish_close(
        &mut self,
        target: CloseTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.update_handoff_active() {
            return;
        }
        if matches!(target, CloseTarget::Update) {
            self.finish_update(window, cx);
            return;
        }
        if matches!(target, CloseTarget::Window) {
            self.cancel_update();
            self.preferences_dirty = None;
            let task = self
                .backend
                .final_workspace(self.prefs.clone(), self.workspace());
            let backend = self.backend.clone();
            self.allow_close = true;
            cx.spawn(async move |this, cx| {
                let result = task.await;
                match result {
                    Ok(Ok(())) => {
                        #[cfg(target_os = "macos")]
                        let _ = this.update(cx, |this, _| {
                            if let Some(routing) = &this.native_titlebar {
                                routing.stop();
                            }
                        });
                        backend.shutdown();
                        let _ = cx.update(|cx| cx.quit());
                    }
                    error => {
                        let _ = this.update(cx, |this, cx| {
                            this.allow_close = false;
                            this.notice =
                                Some(format!("Could not save layout before closing: {error:?}"));
                            cx.notify();
                        });
                    }
                }
            })
            .detach();
            return;
        }
        if let CloseTarget::Document(owner, id) = target {
            for p in self.tabs.iter_mut().flat_map(|t| &mut t.panes) {
                if p.documents.iter().any(|d| d.owner == owner && d.id == id) {
                    p.documents.retain(|d| d.id != id);
                    // Fall back to the most recently opened remaining document
                    // instead of leaving the editor dialog on its empty state.
                    p.active_document = p.documents.last().map(|d| d.id);
                }
            }
        } else if let CloseTarget::Editor(owner) = target {
            // Closing the editor never touches the session, its transfers or
            // the drafts themselves; any draft still dirty at this point (the
            // "don't save" path) is reverted to its original content. The
            // return-to-editor hook must go too, or the shared dismiss below
            // would reopen the dialog this close is meant to end.
            self.editor_return = None;
            let dirty: Vec<(Id, Entity<InputState>, String)> = self
                .pane(owner)
                .map(|pane| {
                    pane.documents
                        .iter()
                        .filter(|d| d.dirty)
                        .map(|d| (d.id, d.input.clone(), d.original.text.clone()))
                        .collect()
                })
                .unwrap_or_default();
            for (id, input, text) in dirty {
                // Arm the guard first: set_value emits InputEvent::Change into
                // gpui's deferred effect queue, and that late Change must not
                // re-mark the reverted draft as dirty.
                if let Some(doc) = self.document_mut(owner, id) {
                    doc.reverting = true;
                }
                input.update(cx, |state, cx| state.set_value(text, window, cx));
                if let Some(doc) = self.document_mut(owner, id) {
                    doc.dirty = false;
                    doc.error = None;
                }
            }
        } else {
            let selected_tab = self.tabs.get(self.active).map(|t| t.id);
            let owners = self.close_owners(&target);
            for owner in &owners {
                self.backend.close(*owner);
            }
            for tab in &mut self.tabs {
                let active = tab.panes.get(tab.active).map(|p| p.owner);
                for owner in &owners {
                    if let Some(layout) = tab.layout.clone().without(owner.session) {
                        tab.layout = layout;
                    }
                }
                tab.panes.retain(|p| !owners.contains(&p.owner));
                tab.active = active
                    .and_then(|o| tab.panes.iter().position(|p| p.owner == o))
                    .unwrap_or(tab.active.min(tab.panes.len().saturating_sub(1)));
            }
            self.tabs.retain(|t| !t.panes.is_empty());
            self.active = selected_tab
                .and_then(|id| self.tabs.iter().position(|t| t.id == id))
                .unwrap_or(self.active.min(self.tabs.len().saturating_sub(1)));
        }
        self.dismiss(window, cx);
        self.focus_active(window, cx);
        self.changed(cx);
    }
    fn save_before_close(
        &mut self,
        target: CloseTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let owners = self.close_owners(&target);
        let docs: Vec<_> = self
            .tabs
            .iter()
            .flat_map(|t| &t.panes)
            .filter(|p| owners.contains(&p.owner))
            .flat_map(|p| {
                p.documents
                    .iter()
                    .filter(|d| {
                        d.dirty
                            && match target {
                                CloseTarget::Document(_, id) => d.id == id,
                                _ => true,
                            }
                    })
                    .map(|d| (d.owner, d.id))
            })
            .collect();
        for (owner, id) in docs {
            self.save_document(owner, id, false, cx);
        }
        self.close_after_save = Some(target.clone());
        // The editor-close flow is heading out, so its return-to-editor hook
        // must not reopen the dialog this dismiss would otherwise restore.
        if matches!(target, CloseTarget::Editor(_)) {
            self.editor_return = None;
        }
        if matches!(target, CloseTarget::Update) {
            // Keep update consent while document saves are in flight. Dismiss means cancel.
            self.show_modal(Modal::Update, window, cx);
        } else {
            self.dismiss(window, cx);
        }
    }
    pub(super) fn import_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(self.t("import").into()),
        });
        let profiles = self.profiles.clone();
        let runtime = self.backend.runtime.clone();
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = paths.await {
                if let Some(path) = paths.first().cloned() {
                    let task = runtime.spawn_blocking(move || {
                        if std::fs::metadata(&path)?.len() > 8 * 1024 * 1024 {
                            anyhow::bail!("Connection import exceeds the 8 MiB limit");
                        }
                        let text = std::fs::read_to_string(&path)?;
                        let text = text
                            .strip_prefix('\u{feff}')
                            .map(str::to_string)
                            .unwrap_or(text);
                        connections::preview(&text, &profiles)
                    });
                    let result = task.await;
                    let _ = this.update_in(cx, |this, window, cx| match result {
                        Ok(Ok(preview)) => this.show_modal(
                            Modal::Import {
                                preview,
                                replace: false,
                            },
                            window,
                            cx,
                        ),
                        other => {
                            this.notice = Some(format!("{other:?}"));
                            cx.notify();
                        }
                    });
                }
            }
        })
        .detach();
    }
    pub(super) fn export_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Default to Downloads until the user picks another folder, then keep it.
        let start = self
            .prefs
            .export_directory
            .clone()
            .filter(|directory| !directory.is_empty())
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::Path::new(&crate::platform::home_directory()).join("Downloads")
            });
        let path = cx.prompt_for_new_path(&start, Some("mantash-connections.csv"));
        let profiles = self.profiles.clone();
        let runtime = self.backend.runtime.clone();
        let vault = self.backend.vault.clone();
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(path))) = path.await {
                let result = runtime
                    .spawn_blocking(move || {
                        // Decrypt stored passwords off the UI thread for the export file.
                        let mut secrets = std::collections::HashMap::new();
                        for profile in &profiles {
                            if let Ok(Some(secret)) = vault.read(profile.id) {
                                secrets.insert(profile.id, secret.to_string());
                            }
                        }
                        let text = connections::export(&profiles, &secrets)?;
                        // UTF-8 BOM so spreadsheet apps decode Chinese text correctly.
                        let bytes =
                            crate::encoding::encode(&text, crate::encoding::Encoding::Utf8, true)?;
                        std::fs::write(&path, bytes)?;
                        Ok::<_, anyhow::Error>(path)
                    })
                    .await;
                let _ = this.update_in(cx, |this, _, cx| {
                    this.notice = Some(match result {
                        Ok(Ok(path)) => {
                            if let Some(parent) = path.parent() {
                                this.prefs.export_directory = Some(parent.display().to_string());
                                this.changed(cx);
                            }
                            format!("{}: {}", this.t("export"), path.display())
                        }
                        other => format!("{other:?}"),
                    });
                    cx.notify();
                });
            }
        })
        .detach();
    }
    pub(super) fn field(&self, label: &str, input: &Entity<InputState>) -> Div {
        div()
            .flex()
            .flex_col()
            .gap(px(theme::SPACE_SMALL))
            .w_full()
            .child(div().child(label.to_string()))
            .child(self.input_box(input).text_size(px(self.prefs.ui_size)))
    }
    pub(super) fn button(
        &self,
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
    ) -> Button {
        Button::new(
            id,
            label,
            self.prefs.ui_size,
            theme::Palette::new(self.prefs.theme),
        )
    }
    /// Paint a determinate transfer ring inside the fixed 24px status slot.
    fn transfer_progress_ring(&self, fraction: f32) -> AnyElement {
        let palette = theme::Palette::new(self.prefs.theme);
        canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                let arc = ProgressArc::new().inner_radius(5.).outer_radius(8.);
                let data = ();
                let segment = |end_angle| ArcData {
                    data: &data,
                    index: 0,
                    value: 1.,
                    start_angle: 0.,
                    end_angle,
                    pad_angle: 0.,
                };
                arc.paint(
                    &segment(std::f32::consts::TAU),
                    palette.border,
                    None,
                    None,
                    &bounds,
                    window,
                );
                if fraction > 0. {
                    arc.paint(
                        &segment(std::f32::consts::TAU * fraction.clamp(0., 1.)),
                        palette.accent,
                        None,
                        None,
                        &bounds,
                        window,
                    );
                }
            },
        )
        .w(px(24.))
        .h(px(24.))
        .into_any_element()
    }
    /// Record the laid-out frame only in an explicitly isolated debug QA window.
    fn measure_dialog_frame(&self, _cx: &mut Context<Self>) -> AnyElement {
        #[cfg(debug_assertions)]
        let view = _cx.entity();
        canvas(
            move |_bounds, _, _cx| {
                #[cfg(debug_assertions)]
                view.update(_cx, |this, _| {
                    if let Some(qa) = &mut this.qa {
                        qa.dialog_frame_bounds = Some(_bounds);
                    }
                });
            },
            |_, _, _, _| {},
        )
        .absolute()
        .inset_0()
        .into_any_element()
    }
    /// Measure a transfer row or column only in an explicitly isolated debug window.
    fn measure_transfer_cell(&self, name: &'static str, _cx: &mut Context<Self>) -> AnyElement {
        #[cfg(debug_assertions)]
        let view = _cx.entity();
        canvas(
            move |_bounds, _, _cx| {
                #[cfg(debug_assertions)]
                view.update(_cx, |this, _| {
                    if let Some(qa) = &mut this.qa {
                        qa.transfer_geometry.insert(name, _bounds);
                    }
                });
            },
            |_, _, _, _| {},
        )
        .absolute()
        .inset_0()
        .into_any_element()
    }

    pub(super) fn render_modal(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(modal) = &self.modal else {
            return div().into_any_element();
        };
        let p = theme::Palette::new(self.prefs.theme);
        let mut title = self.t("settings").to_string();
        let resource_dialog = matches!(
            modal,
            Modal::ResourceDetails { .. } | Modal::WindowControls(_)
        );
        let compact_dialog =
            resource_dialog || matches!(modal, Modal::Connections | Modal::Profile(_));
        let history_modal = matches!(modal, Modal::LocalHistory);
        #[cfg(debug_assertions)]
        let measure_frame = self.qa.is_some();
        #[cfg(not(debug_assertions))]
        let measure_frame = false;
        let width_cap = if matches!(modal, Modal::Editor { .. }) {
            960.
        } else if modal.is_confirmation() {
            480.
        } else {
            640.
        };
        // These dialogs act directly and always offer the header close button.
        let closable_dialog = matches!(
            modal,
            Modal::About
                | Modal::Encoding { .. }
                | Modal::Transfer { .. }
                | Modal::LocalHistory
                | Modal::SystemTools { .. }
                | Modal::ProcessDetails { .. }
                | Modal::Editor { .. }
                | Modal::Transfers { .. }
                | Modal::Settings
                | Modal::Font { .. }
        );
        let mut body = div()
            .flex()
            .flex_col()
            .min_w_0()
            .when(matches!(modal, Modal::Transfer { .. }), |body| {
                body.w_full()
            })
            // A capped history dialog must pass its remaining height to the
            // list instead of retaining the list's full min-content height.
            .when(history_modal, |body| body.min_h_0())
            .gap(px(theme::SPACE_PANEL));
        let mut footer = div()
            .flex()
            .flex_wrap()
            .justify_end()
            .gap(px(theme::SPACE_CONTROL));
        // Encoding pickers act directly on chips and do not need a footer strip.
        let mut footer_hidden = false;
        match modal {
            Modal::Update => {
                title = self.update_dialog_title().into();
                body = body.child(self.render_update_body());
                footer = footer.child(self.render_update_footer(cx));
            }
            Modal::About => {
                footer_hidden = true;
                title = self.t("about_mantash").into();
                body = body
                    .items_center()
                    .child(
                        gpui::svg()
                            .path("mantash-mark.svg")
                            .size(px(72.))
                            .text_color(p.accent),
                    )
                    .child(div().text_size(px(18.)).child("MantaSH"))
                    .child(div().text_color(p.muted).child(format!(
                                "{} · {}",
                                self.t("settings_version").replace("{version}", crate::APP_VERSION),
                                self.t("about_author")
                            )))
                    .child(
                        self.button("open-github", self.t("about_github"))
                            .ghost()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.open_github(cx);
                            })),
                    );
            }
            Modal::WindowControls(form) => {
                title = self.t("window_controls").into();
                body = body.child(self.render_window_controls(form, window, cx));
                footer = self.window_footer(window, cx);
            }
            Modal::Credentials {
                owner: _,
                secret,
                show_secret,
                ..
            } => {
                title = self.t("enter_password").into();
                // The prompt stays minimal: the field with its eye toggle —
                // no label, no reason prose. The title already says what the
                // input is for; QA still reads `reason` from the modal state
                // to tell prompt causes apart.
                let showing = *show_secret;
                body =
                    body.child(
                        div().flex().flex_col().w_full().child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(theme::SPACE_SMALL))
                                .child(div().flex_1().min_w_0().child(
                                    self.input_box(secret).text_size(px(self.prefs.ui_size)),
                                ))
                                .child(
                                    self.button("toggle-secret", "")
                                        .svg_icon(if showing {
                                            "icons/eye-off.svg"
                                        } else {
                                            "icons/eye.svg"
                                        })
                                        .ghost()
                                        .tooltip(self.t(if showing {
                                            "hide_password"
                                        } else {
                                            "show_password"
                                        }))
                                        .on_click(cx.listener(move |this, _, w, cx| {
                                            let reveal = !showing;
                                            if let Some(Modal::Credentials {
                                                show_secret,
                                                secret,
                                                ..
                                            }) = &mut this.modal
                                            {
                                                *show_secret = reveal;
                                                secret.update(cx, |state, cx| {
                                                    state.set_masked(!reveal, w, cx)
                                                });
                                            }
                                            cx.notify();
                                        })),
                                ),
                        ),
                    );
                // No remember toggle: submitted passwords are always stored
                // (submit_credentials replies with remember=true unconditionally).
                footer = footer.child(
                    self.button("edit-credential-connection", self.t("edit"))
                        .on_click(
                            cx.listener(|this, _, w, cx| this.edit_credential_connection(w, cx)),
                        ),
                );
                footer = footer.child(
                    self.button("submit-credentials", self.t("connect"))
                        .primary()
                        .disabled(secret.read(cx).value().is_empty())
                        .on_click(cx.listener(|this, _, w, cx| this.submit_credentials(w, cx))),
                );
            }
            Modal::Connections => {
                title = self.t("connection_library").into();
                body = body.child(self.render_connection_library(window, cx));
            }
            Modal::LocalHistory => {
                title = self.t("history").into();
                if let Some(pane) = self.active_pane() {
                    let owner = pane.owner;
                    let scope = pane.spec.history_scope();
                    let snapshot = self.history_delete_snapshot(owner, scope, cx);
                    if !snapshot.target_ids.is_empty() {
                        let delete_snapshot = snapshot.clone();
                        footer = footer
                            .justify_start()
                            .child(
                                self.button(
                                    "delete-history",
                                    format!("{} ({})", self.t("delete"), snapshot.target_ids.len()),
                                )
                                .svg_icon("icons/trash-2.svg")
                                .danger()
                                .on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        this.review_history_delete(
                                            delete_snapshot.clone(),
                                            window,
                                            cx,
                                        )
                                    },
                                )),
                            )
                            .child(
                                self.button("clear-history-selection", self.t("clear_selection"))
                                    .icon(IconName::Undo)
                                    .ghost()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.history_views[scope.index()].clear_selection();
                                        cx.notify();
                                    })),
                            );
                    } else {
                        footer_hidden = true;
                    }
                } else {
                    footer_hidden = true;
                }
                body = body.child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .flex_col()
                        .overflow_hidden()
                        .child(self.render_history(cx)),
                );
            }
            Modal::Profile(form) => {
                title = self
                    .t(if form.reconnect.is_some() {
                        "reconnect"
                    } else if form.connecting {
                        "connect"
                    } else if form.existing {
                        "edit"
                    } else if form.cloning {
                        "clone_connection"
                    } else {
                        "new_ssh"
                    })
                    .into();
                body = body
                    // Name stands alone; host/port and username/password share rows.
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(self.field(self.t("optional_name"), &form.name)),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(theme::SPACE_PANEL))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(self.field(self.t("host"), &form.host)),
                            )
                            .child(
                                div()
                                    .w(px(80.))
                                    .child(self.field(self.t("port"), &form.port)),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(theme::SPACE_PANEL))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(self.field(self.t("username"), &form.username)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(self.field(self.t("password"), &form.secret)),
                            ),
                    )
                    .when_some(form.error.clone(), |body, error| {
                        body.child(div().text_color(p.error).child(error))
                    });
                footer = footer
                    .child(
                        self.button("save-only", self.t("save")).on_click(
                            cx.listener(|this, _, w, cx| this.submit_profile(false, w, cx)),
                        ),
                    )
                    .child(
                        self.button(
                            "save-connect",
                            self.t(if form.connecting {
                                "connect"
                            } else {
                                "save_connect"
                            }),
                        )
                        .primary()
                        .on_click(cx.listener(|this, _, w, cx| this.submit_profile(true, w, cx))),
                    )
                    // Cancel/close always sits at the far right of a footer.
                    .child(
                        self.button("dismiss-modal", self.t("cancel"))
                            .ghost()
                            .on_click(cx.listener(|this, _, w, cx| this.cancel_modal(w, cx))),
                    );
            }
            Modal::Settings => {
                // The footer carries only the reset-defaults action; closing
                // stays on the header X.
                // Section label for the language switcher; the old vault
                // heading was a leftover from the removed password section.
                let language = div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.))
                    .child(self.t("language"));
                body = body
                    .child(language)
                    .child(div().flex().gap(px(8.)).children(
                        [(Language::Zh, "中文"), (Language::En, "English")].map(
                            |(language, label)| {
                                self.button(label, label)
                                    .selected(self.prefs.language == language)
                                    .on_click(cx.listener(move |this, _, w, cx| {
                                        this.prefs.language = language;
                                        this.apply_preferences(w, cx);
                                    }))
                            },
                        ),
                    ));
                let shell_choices = self.local_shells.clone().unwrap_or_default();
                let shell_picker = if self.local_shells_loading {
                    div()
                        .text_color(p.muted)
                        .child(self.t("loading"))
                        .into_any_element()
                } else if shell_choices.is_empty() {
                    div()
                        .text_color(p.error)
                        .child(self.t("local_shell_unavailable"))
                        .into_any_element()
                } else {
                    let current = self.prefs.local_shell.clone();
                    let label = current
                        .rsplit(['/', '\\'])
                        .next()
                        .unwrap_or(&current)
                        .to_string();
                    let view = cx.entity().downgrade();
                    self.button("local-shell-selector", label)
                        .tooltip(current.clone())
                        .dropdown_menu(move |mut menu, _, _| {
                            for shell in &shell_choices {
                                // Windows accepts both separators, including paths saved by older versions.
                                let selected =
                                    std::path::Path::new(shell) == std::path::Path::new(&current);
                                let shell = shell.clone();
                                let view = view.clone();
                                menu = menu.item(
                                    PopupMenuItem::new(shell.clone())
                                        .checked(selected)
                                        .on_click(move |_, window, cx| {
                                            let _ = view.update(cx, |this, cx| {
                                                if matches!(this.modal, Some(Modal::Settings))
                                                    && this.local_shells.as_ref().is_some_and(
                                                        |choices| choices.contains(&shell),
                                                    )
                                                {
                                                    this.prefs.local_shell = shell.clone();
                                                    this.apply_preferences(window, cx);
                                                }
                                            });
                                        }),
                                );
                            }
                            menu
                        })
                        .into_any_element()
                };
                body = body.child(
                    div()
                        .flex()
                        .flex_col()
                        .items_start()
                        .gap(px(theme::SPACE_SMALL))
                        .child(self.t("local_shell"))
                        .child(shell_picker),
                );
                for terminal in [false, true] {
                    let family = if terminal {
                        &self.prefs.terminal_font
                    } else {
                        &self.prefs.ui_font
                    };
                    let size = if terminal {
                        self.prefs.terminal_size
                    } else {
                        self.prefs.ui_size
                    };
                    body = body.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(self.t(if terminal { "terminal_font" } else { "ui_font" }))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(
                                        self.button(
                                            if terminal {
                                                "terminal-family"
                                            } else {
                                                "ui-family"
                                            },
                                            if family.starts_with('.') {
                                                self.t("system_font").to_string()
                                            } else {
                                                family.clone()
                                            },
                                        )
                                        .on_click(
                                            cx.listener(move |this, _, w, cx| {
                                                let filter = Self::input(
                                                    "",
                                                    this.t("font_search"),
                                                    false,
                                                    w,
                                                    cx,
                                                );
                                                this.show_modal(
                                                    Modal::Font { terminal, filter },
                                                    w,
                                                    cx,
                                                );
                                            }),
                                        ),
                                    )
                                    .child(
                                        self.button(
                                            if terminal {
                                                "terminal-minus"
                                            } else {
                                                "ui-minus"
                                            },
                                            "−",
                                        )
                                        .on_click(
                                            cx.listener(move |this, _, w, cx| {
                                                if terminal {
                                                    this.prefs.terminal_size -= 1.;
                                                } else {
                                                    this.prefs.ui_size -= 1.;
                                                }
                                                this.apply_preferences(w, cx);
                                            }),
                                        ),
                                    )
                                    // Same height as the stepper buttons so the
                                    // value sits level with them, not on its own
                                    // padded line.
                                    .child(
                                        div()
                                            .h(px(24.))
                                            .w(px(52.))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(format!("{size:.0} px")),
                                    )
                                    .child(
                                        self.button(
                                            if terminal { "terminal-plus" } else { "ui-plus" },
                                            "+",
                                        )
                                        .on_click(
                                            cx.listener(move |this, _, w, cx| {
                                                if terminal {
                                                    this.prefs.terminal_size += 1.;
                                                } else {
                                                    this.prefs.ui_size += 1.;
                                                }
                                                this.apply_preferences(w, cx);
                                            }),
                                        ),
                                    ),
                            ),
                    );
                }
                footer = footer.items_center().justify_between().child(
                    self.button("reset-settings", self.t("reset_defaults"))
                        .on_click(cx.listener(|this, _, w, cx| {
                            // Reset exactly the items this dialog shows:
                            // language and the two font/size pairs. Theme
                            // lives on the top bar and hidden preferences
                            // (window geometry, tool widths) keep their values.
                            let defaults = Preferences::default();
                            this.prefs.language = defaults.language;
                            this.prefs.ui_font = defaults.ui_font;
                            this.prefs.ui_size = defaults.ui_size;
                            this.prefs.terminal_font = defaults.terminal_font;
                            this.prefs.terminal_size = defaults.terminal_size;
                            this.apply_preferences(w, cx);
                        })),
                );
                footer = footer.child(self.check_update_button(cx));
            }
            Modal::Font { terminal, filter } => {
                title = self
                    .t(if *terminal {
                        "terminal_font"
                    } else {
                        "ui_font"
                    })
                    .into();
                let terminal = *terminal;
                let query = filter.read(cx).value().to_lowercase();
                // Virtualized family list (same uniform_list path as the
                // connection library): only visible rows build, the current
                // family is marked, and a highlight row carries the keyboard
                // selection. Apply commits the highlight; Esc or the header X
                // returns to settings with nothing changed.
                let families: Vec<String> = window
                    .text_system()
                    .all_font_names()
                    .into_iter()
                    .filter(|name| name.to_lowercase().contains(&query))
                    .collect();
                let current = if terminal {
                    self.prefs.terminal_font.clone()
                } else {
                    self.prefs.ui_font.clone()
                };
                let highlight = self
                    .font_highlight
                    .clone()
                    .filter(|name| families.contains(name))
                    .or_else(|| {
                        families
                            .iter()
                            .find(|name| name.as_str() == current.as_str())
                            .cloned()
                    });
                let entity = cx.entity();
                let rows = families.len();
                let highlight_for_rows = highlight.clone();
                let current_for_rows = current.clone();
                body = body
                    .child(self.input_box(filter))
                    .when(rows == 0, |b| {
                        b.child(
                            div()
                                .py(px(theme::SPACE_SECTION))
                                .text_color(p.muted)
                                .child(self.t("history_no_match")),
                        )
                    })
                    .child(if rows == 0 {
                        div().into_any_element()
                    } else {
                        uniform_list("font-families", rows, move |range, _window, _cx| {
                            let theme = p;
                            range
                                .map(|index| {
                                    let family = families[index].clone();
                                    let selected =
                                        highlight_for_rows.as_deref() == Some(family.as_str());
                                    let active = family == current_for_rows;
                                    let down_family = family.clone();
                                    let click_family = family.clone();
                                    let label_family = family.clone();
                                    let down_entity = entity.clone();
                                    let click_entity = entity.clone();
                                    div()
                                        .id(("font-family", index))
                                        .flex()
                                        .items_center()
                                        .w_full()
                                        .h(px(28.))
                                        .px(px(8.))
                                        .when(selected, |row| row.bg(theme.selected))
                                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                            down_entity.update(cx, |this, cx| {
                                                this.font_highlight = Some(down_family.clone());
                                                cx.notify();
                                            });
                                        })
                                        .on_click(move |_, w, cx| {
                                            click_entity.update(cx, |this, cx| {
                                                this.select_font(click_family.clone(), w, cx);
                                            });
                                        })
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .overflow_hidden()
                                                .text_ellipsis()
                                                .whitespace_nowrap()
                                                .font_family(family)
                                                .text_color(if active {
                                                    theme.text
                                                } else {
                                                    theme.muted
                                                })
                                                .child(label_family),
                                        )
                                })
                                .collect::<Vec<_>>()
                        })
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w_0()
                        .min_h_0()
                        .h(px(320.))
                        .track_scroll(self.font_scroll.clone())
                        .into_any_element()
                    });
                // No footer: selecting a row applies the font and returns;
                // the header X (or Esc) discards and returns.
                footer_hidden = true;
            }
            Modal::DeleteProfiles { profiles } => {
                title = self.t("delete_confirm").into();
                body = body
                    .child(format!("{} {}", profiles.len(), self.t("selected")))
                    .children(profiles.iter().map(|profile| {
                        div().child(format!("{} ({})", profile.name.clone(), profile.endpoint()))
                    }))
                    .child(self.t("delete_profile_hint"));
                let ids = profiles
                    .iter()
                    .map(|profile| profile.id)
                    .collect::<Vec<_>>();
                footer = footer.child(
                    self.button("confirm-delete-profiles", self.t("delete"))
                        .danger()
                        .on_click(cx.listener(move |this, _, w, cx| {
                            // Remove the confirmed UUIDs, persist the new list, and drop
                            // their stored credentials in the background.
                            this.profiles.retain(|profile| !ids.contains(&profile.id));
                            this.backend.save_profiles(this.profiles.clone());
                            let events = this.backend.events.clone();
                            let vault = this.backend.vault.clone();
                            let forgotten = ids.clone();
                            this.backend.runtime.spawn_blocking(move || {
                                for id in &forgotten {
                                    if let Err(error) = vault.forget(*id) {
                                        let _ = events.try_send(Event::Error(error.to_string()));
                                    }
                                }
                            });
                            this.connection_multi.clear();
                            this.connection_selected = None;
                            this.connection_anchor = None;
                            if this.connection_return {
                                this.back_to_connections(w, cx);
                            } else {
                                this.dismiss(w, cx);
                            }
                        })),
                );
            }
            Modal::Trust {
                owner,
                host,
                port,
                previous,
                fingerprint,
                ..
            } => {
                title = self
                    .t(if previous.is_some() {
                        "changed_host"
                    } else {
                        "first_host"
                    })
                    .into();
                body = body
                    .child(format!("{host}:{port}"))
                    .child(self.t("trust_hint"));
                if let Some(previous) = previous {
                    body = body.child(
                        div()
                            .text_color(p.error)
                            .child(format!("{}: {previous}", self.t("previous"))),
                    );
                }
                body = body.child(
                    div()
                        .text_color(p.accent)
                        .child(format!("{}: {fingerprint}", self.t("fingerprint"))),
                );
                let owner = *owner;
                footer = footer
                    .child(
                        self.button("reject-host", self.t("reject"))
                            .on_click(cx.listener(move |this, _, w, cx| {
                                this.backend.close(owner);
                                this.dismiss(w, cx);
                            })),
                    )
                    .child(
                        self.button("trust-host", self.t("trust"))
                            .primary()
                            .on_click(cx.listener(|this, _, w, cx| {
                                if let Some(Modal::Trust { reply, .. }) = &mut this.modal {
                                    if let Some(reply) = reply.take() {
                                        let _ = reply.send(true);
                                    }
                                }
                                this.dismiss(w, cx);
                            })),
                    );
            }
            Modal::Import { preview, replace } => {
                title = self.t("import_preview").into();
                body = body.child(
                    div()
                        .id("import-rows")
                        .relative()
                        .max_h(px(280.))
                        .overflow_y_scroll()
                        .track_scroll(&self.modal_scroll)
                        .children(preview.rows.iter().map(|row| {
                            let row_number = row.row;
                            let text = row.error.clone().unwrap_or_else(|| {
                                row.profile
                                    .as_ref()
                                    .map(|p| format!("{} — {}", p.name, p.endpoint()))
                                    .unwrap_or_default()
                            });
                            div()
                                .flex()
                                .gap(px(8.))
                                .min_w_0()
                                .py(px(4.))
                                .border_b_1()
                                .border_color(p.border)
                                .text_color(if row.error.is_some() { p.error } else { p.text })
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .child(format!("{} · {}", row_number, text)),
                                )
                                .child(
                                    self.button(("remove-import-row", row_number), "")
                                        .icon(IconName::Close)
                                        .ghost()
                                        .h(px(20.))
                                        .tooltip(self.t("remove_import_row"))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            if let Some(Modal::Import { preview, .. }) =
                                                &mut this.modal
                                            {
                                                preview.rows.retain(|row| row.row != row_number);
                                            }
                                            cx.notify();
                                        })),
                                )
                        }))
                        .vertical_scrollbar(&self.modal_scroll),
                );
                footer = footer
                    .child(
                        Checkbox::new("replace-duplicates")
                            .label(self.t("replace_duplicates"))
                            .checked(*replace)
                            .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                if let Some(Modal::Import { replace, .. }) = &mut this.modal {
                                    *replace = *checked;
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        self.button("confirm-import", self.t("import"))
                            .primary()
                            .on_click(cx.listener(|this, _, w, cx| {
                                if let Some(Modal::Import { preview, replace }) = &this.modal {
                                    this.profiles =
                                        connections::merge(&this.profiles, preview, *replace);
                                    this.backend.save_profiles(this.profiles.clone());
                                    // Passwords from the file go into the vault for rows that
                                    // actually landed; skipped duplicates keep their saved value.
                                    let imported: Vec<(Id, String)> = preview
                                        .rows
                                        .iter()
                                        .filter(|row| row.duplicate.is_none() || *replace)
                                        .filter_map(|row| {
                                            let password = row.password.clone()?;
                                            let profile = row.profile.as_ref()?;
                                            let id = this
                                                .profiles
                                                .iter()
                                                .find(|saved| saved.duplicates(profile))?
                                                .id;
                                            Some((id, password))
                                        })
                                        .collect();
                                    if !imported.is_empty() {
                                        let vault = this.backend.vault.clone();
                                        this.backend.runtime.spawn_blocking(move || {
                                            for (id, secret) in &imported {
                                                let _ = crate::credentials::SecretStore::write(
                                                    &*vault, *id, secret,
                                                );
                                            }
                                        });
                                    }
                                }
                                this.dismiss(w, cx);
                            })),
                    );
            }
            Modal::FileName {
                owner,
                directory,
                original,
                input,
            } => {
                title = self
                    .t(if original.is_some() {
                        "rename"
                    } else {
                        "mkdir"
                    })
                    .into();
                body = body.child(directory.clone()).child(self.input_box(input));
                let owner = *owner;
                footer = footer.child(
                    self.button("confirm-name", self.t("apply"))
                        .primary()
                        .on_click(cx.listener(move |this, _, w, cx| {
                            if let Some(Modal::FileName {
                                directory,
                                original,
                                input,
                                ..
                            }) = &this.modal
                            {
                                match crate::files::join(directory, &input.read(cx).value()) {
                                    Ok(path) => {
                                        let operation = original.as_ref().map_or_else(
                                            || FileOperation::Mkdir(path.clone()),
                                            |old| FileOperation::Rename(old.clone(), path.clone()),
                                        );
                                        this.backend.file_operation(owner, Id::new_v4(), operation);
                                        this.dismiss(w, cx);
                                    }
                                    Err(error) => {
                                        this.notice = Some(error.to_string());
                                        cx.notify();
                                    }
                                }
                            }
                        })),
                );
            }
            Modal::DeleteFiles { owner, paths } => {
                title = self.t("delete_confirm").into();
                body = body.children(paths.iter().cloned().map(|path| div().child(path)));
                let owner = *owner;
                let paths = paths.clone();
                footer = footer.child(
                    self.button("confirm-delete-files", self.t("delete"))
                        .danger()
                        .on_click(cx.listener(move |this, _, w, cx| {
                            this.backend.file_operation(
                                owner,
                                Id::new_v4(),
                                FileOperation::Delete(paths.clone()),
                            );
                            this.dismiss(w, cx);
                        })),
                );
            }
            Modal::DeleteHistory {
                owner,
                scope,
                label,
                ids,
            } => {
                title = self.t("delete_history_confirm").into();
                body = body.child(label.clone()).child(format!(
                    "{} {}",
                    ids.len(),
                    self.t("selected")
                ));
                let owner = *owner;
                let scope = *scope;
                let ids = ids.clone();
                footer = footer.child(
                    self.button("confirm-history-delete", self.t("delete"))
                        .danger()
                        .on_click(cx.listener(move |this, _, w, cx| {
                            let context_valid = this.modal.as_ref().is_some_and(|modal| {
                                matches!(
                                    modal,
                                    Modal::DeleteHistory {
                                        owner: current_owner,
                                        scope: current_scope,
                                        ids: current_ids,
                                        ..
                                    } if *current_owner == owner
                                        && *current_scope == scope
                                        && current_ids.as_slice() == ids.as_slice()
                                )
                            }) && this.active_owner() == Some(owner)
                                && this.active_pane().is_some_and(|pane| {
                                    pane.owner == owner && pane.spec.history_scope() == scope
                                });
                            if !context_valid {
                                this.notice = Some(this.t("history_context_expired").into());
                                cx.notify();
                                return;
                            }
                            // `ids` is the confirmation's frozen UUID list;
                            // records added after it was created are excluded.
                            let ids = this
                                .history
                                .iter()
                                .filter(|entry| {
                                    scope.includes(&entry.scope) && ids.contains(&entry.id)
                                })
                                .map(|entry| entry.id)
                                .collect::<Vec<_>>();
                            if ids.is_empty() {
                                this.cancel_modal(w, cx);
                                return;
                            }
                            this.backend.delete_history(ids.clone());
                            this.history.retain(|entry| {
                                !(scope.includes(&entry.scope) && ids.contains(&entry.id))
                            });
                            this.history_views[scope.index()].clear_selection();
                            this.cancel_modal(w, cx);
                        })),
                );
            }
            Modal::Close(target) => {
                // Document-only and editor-close keep the body to the
                // essentials — the title already says "unsaved", so each row
                // is just the draft's path. The multi-target variants keep
                // pane labels and transfer lines because closing a session
                // affects more than drafts.
                let document_only =
                    matches!(target, CloseTarget::Document(_, _) | CloseTarget::Editor(_));
                title = self
                    .t(if document_only {
                        "unsaved"
                    } else {
                        "confirm_close"
                    })
                    .into();
                let owners = self.close_owners(target);
                if document_only {
                    for pane in self
                        .tabs
                        .iter()
                        .flat_map(|t| &t.panes)
                        .filter(|p| owners.contains(&p.owner))
                    {
                        for document in &pane.documents {
                            if document.dirty
                                && match target {
                                    CloseTarget::Document(_, id) => document.id == *id,
                                    _ => true,
                                }
                            {
                                body = body.child(document.original.path.clone());
                            }
                        }
                    }
                } else {
                    body = body.child(self.t("close_impact"));
                    for pane in self
                        .tabs
                        .iter()
                        .flat_map(|t| &t.panes)
                        .filter(|p| owners.contains(&p.owner))
                    {
                        for document in &pane.documents {
                            if document.dirty {
                                body = body.child(format!(
                                    "{} · {}",
                                    self.t("unsaved"),
                                    document.original.path
                                ));
                            }
                        }
                        for task in &self.transfers {
                            if task.state.active() && task.belongs_to(pane.owner) {
                                body = body.child(format!("{} → {}", task.local, task.remote));
                            }
                        }
                    }
                }
                let target_save = target.clone();
                let target_discard = target.clone();
                footer = footer
                    .child(
                        self.button("save-close", self.t("save_all_close"))
                            .on_click(cx.listener(move |this, _, w, cx| {
                                this.save_before_close(target_save.clone(), w, cx)
                            })),
                    )
                    .child(
                        self.button(
                            "discard-close",
                            self.t(if document_only {
                                "discard"
                            } else {
                                "discard_close"
                            }),
                        )
                        .danger()
                        .on_click(cx.listener(move |this, _, w, cx| {
                            this.finish_close(target_discard.clone(), w, cx)
                        })),
                    );
            }
            Modal::Conflict { owner, document } => {
                title = self.t("conflict").into();
                body = body.child(self.t("conflict_hint"));
                let owner = *owner;
                let id = *document;
                footer = footer
                    .child(
                        self.button("reload-document", self.t("reload"))
                            .on_click(cx.listener(move |this, _, w, cx| {
                                this.reload_document(owner, id, cx);
                                this.dismiss(w, cx);
                            })),
                    )
                    .child(
                        self.button("overwrite-document", self.t("overwrite"))
                            .danger()
                            .on_click(cx.listener(move |this, _, w, cx| {
                                this.save_document(owner, id, true, cx);
                                this.dismiss(w, cx);
                            })),
                    );
            }
            Modal::Encoding { owner, document } => {
                title = self.t("encoding").into();
                let encodings = if document.is_some() {
                    Encoding::FILE.to_vec()
                } else {
                    Encoding::TERMINAL.to_vec()
                };
                let owner = *owner;
                let document = *document;
                body = body.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(theme::SPACE_CONTROL))
                        .children(encodings.into_iter().map(|encoding| {
                            self.button(encoding.label(), encoding.label())
                                .on_click(cx.listener(move |this, _, w, cx| {
                                    if let Some(owner) = owner {
                                        if let Some(id) = document {
                                            if let Some(doc) = this.document_mut(owner, id) {
                                                doc.encoding = encoding;
                                                doc.revision += 1;
                                                doc.dirty = true;
                                            }
                                        } else if let Some(pane) = this.pane_mut(owner) {
                                            pane.encoding_warning = false;
                                            if let Some(terminal) = &pane.terminal {
                                                terminal
                                                    .read(cx)
                                                    .session
                                                    .terminal
                                                    .lock()
                                                    .set_encoding(encoding);
                                            }
                                            match &mut pane.spec {
                                                SessionSpec::Local { encoding: e, .. } => {
                                                    *e = encoding
                                                }
                                                SessionSpec::Ssh { encoding: e, .. } => {
                                                    *e = encoding
                                                }
                                            }
                                        }
                                    } else {
                                        this.prefs.local_encoding = encoding;
                                    }
                                    this.dismiss(w, cx);
                                    this.changed(cx);
                                }))
                        })),
                );
                footer_hidden = true;
            }
            Modal::Transfer {
                owner,
                batch,
                records,
                overwrite,
                phase,
                ..
            } => {
                let all_uploads = records.iter().all(|record| record.upload);
                let all_downloads = records.iter().all(|record| !record.upload);
                let action_key = if all_uploads {
                    "start_upload"
                } else if all_downloads {
                    "start_download"
                } else {
                    "start_transfer"
                };
                let title_key = match phase {
                    TransferPhase::Review if all_uploads => "confirm_upload",
                    TransferPhase::Review if all_downloads => "confirm_download",
                    TransferPhase::Review => "confirm_transfer",
                    TransferPhase::Running => "transfer_running",
                    TransferPhase::Result => "transfer_result",
                };
                title = format!("{} ({})", self.t(title_key), records.len());
                body = body.child(div().w_full().min_w_0().flex().flex_col().children(
                    records.iter().enumerate().map(|(index, record)| {
                        let id = record.id;
                        let live = if *phase == TransferPhase::Review {
                            None
                        } else {
                            self.transfers.iter().find(|task| task.id == id)
                        };
                        let shown = live.unwrap_or(record);
                        let (source, destination) = super::transfer_paths::endpoints(record);
                        let leading: AnyElement = match live.map(|task| task.state) {
                            None => gpui::svg()
                                .path(if record.upload {
                                    "icons/upload.svg"
                                } else {
                                    "icons/download.svg"
                                })
                                .size(px(16.))
                                .text_color(p.accent)
                                .into_any_element(),
                            Some(TransferState::Queued) => gpui::svg()
                                .path("icons/hourglass.svg")
                                .size(px(16.))
                                .text_color(p.muted)
                                .into_any_element(),
                            Some(TransferState::Running) => {
                                match transfer_progress_fraction(shown.bytes, shown.total) {
                                    Some(fraction) => self.transfer_progress_ring(fraction),
                                    None => gpui_component::spinner::Spinner::new()
                                        .with_size(px(14.))
                                        .color(p.accent)
                                        .into_any_element(),
                                }
                            }
                            Some(TransferState::Completed) => gpui::svg()
                                .path("icons/check-check.svg")
                                .size(px(16.))
                                .text_color(p.meter_green)
                                .into_any_element(),
                            Some(state) => gpui::svg()
                                .path(if state == TransferState::Cancelled {
                                    "icons/ban.svg"
                                } else {
                                    "icons/circle-alert.svg"
                                })
                                .size(px(16.))
                                .text_color(p.error)
                                .into_any_element(),
                        };
                        let status_key = match shown.state {
                            TransferState::Queued => "queued",
                            TransferState::Running => "running",
                            TransferState::Completed => "completed",
                            TransferState::Failed => "failed",
                            TransferState::Cancelled => "cancelled",
                            TransferState::Interrupted => "interrupted",
                        };
                        let size = shown.total.or(record.total);
                        div()
                            .w_full()
                            .flex()
                            .items_start()
                            .gap(px(theme::SPACE_CONTROL))
                            .py(px(theme::SPACE_CONTROL))
                            .when(index == 0 && measure_frame, |row| {
                                row.relative().child(self.measure_transfer_cell("row", cx))
                            })
                            .when(index + 1 < records.len(), |row| {
                                row.border_b_1().border_color(p.border)
                            })
                            .child(
                                div()
                                    .w(px(24.))
                                    .h(px(24.))
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(leading),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(theme::SPACE_SMALL))
                                    .when(index == 0 && measure_frame, |column| {
                                        column
                                            .relative()
                                            .child(self.measure_transfer_cell("column", cx))
                                    })
                                    .child(
                                        div()
                                            .min_w_0()
                                            .whitespace_normal()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(
                                                super::transfer_paths::display_name(record)
                                                    .to_string(),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .min_w_0()
                                            .when(index == 0 && measure_frame, |source| {
                                                source
                                                    .relative()
                                                    .child(self.measure_transfer_cell("source", cx))
                                            })
                                            .gap(px(theme::SPACE_CONTROL))
                                            .text_color(p.muted)
                                            .child(div().flex_none().child(self.t("source_path")))
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w_0()
                                                    .when(index == 0 && measure_frame, |path| {
                                                        path.relative().child(
                                                            self.measure_transfer_cell(
                                                                "source_path",
                                                                cx,
                                                            ),
                                                        )
                                                    })
                                                    .whitespace_normal()
                                                    .child(source.to_string()),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .min_w_0()
                                            .gap(px(theme::SPACE_CONTROL))
                                            .text_color(p.muted)
                                            .child(div().flex_none().child(self.t("target_path")))
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w_0()
                                                    .whitespace_normal()
                                                    .child(destination.to_string()),
                                            ),
                                    )
                                    .when(*phase != TransferPhase::Review, |column| {
                                        column.child(
                                            div()
                                                .min_w_0()
                                                .whitespace_normal()
                                                .text_color(
                                                    if shown.state == TransferState::Completed {
                                                        p.meter_green
                                                    } else if matches!(
                                                        shown.state,
                                                        TransferState::Failed
                                                            | TransferState::Cancelled
                                                            | TransferState::Interrupted
                                                    ) {
                                                        p.error
                                                    } else {
                                                        p.muted
                                                    },
                                                )
                                                .child(
                                                    if shown.state == TransferState::Running
                                                        && shown.bytes > 0
                                                    {
                                                        format!(
                                                            "{} · {}",
                                                            self.t(status_key),
                                                            crate::monitor::bytes(shown.bytes)
                                                        )
                                                    } else {
                                                        self.t(status_key).to_string()
                                                    },
                                                ),
                                        )
                                    })
                                    .when_some(shown.error.clone(), |column, error| {
                                        column.child(
                                            div()
                                                .min_w_0()
                                                .whitespace_normal()
                                                .text_color(p.error)
                                                .child(error),
                                        )
                                    })
                                    .when(
                                        *phase == TransferPhase::Result
                                            && matches!(
                                                shown.state,
                                                TransferState::Failed
                                                    | TransferState::Cancelled
                                                    | TransferState::Interrupted
                                            ),
                                        |column| {
                                            let retry_record = shown.clone();
                                            column.child(
                                                self.button(
                                                    ("retry-batch-item", id.as_u128() as u64),
                                                    self.t("retry"),
                                                )
                                                .ghost()
                                                .on_click(cx.listener(move |this, _, w, cx| {
                                                    this.retry_transfer(retry_record.clone(), w, cx)
                                                })),
                                            )
                                        },
                                    ),
                            )
                            .child(
                                div()
                                    .w(px(100.))
                                    .flex_none()
                                    .flex()
                                    .flex_col()
                                    .items_end()
                                    .gap(px(theme::SPACE_SMALL))
                                    .child(
                                        div().whitespace_nowrap().text_color(p.muted).child(
                                            size.map(crate::monitor::bytes)
                                                .unwrap_or_else(|| "—".into()),
                                        ),
                                    )
                                    .when(*phase == TransferPhase::Review, |column| {
                                        column.child(
                                            self.button(
                                                ("remove-transfer-target", id.as_u128() as u64),
                                                "",
                                            )
                                            .icon(IconName::Close)
                                            .ghost()
                                            .w(px(20.))
                                            .h(px(20.))
                                            .p_0()
                                            .tooltip(self.t("remove_record"))
                                            .on_click(
                                                cx.listener(move |this, _, _, cx| {
                                                    if let Some(Modal::Transfer {
                                                        phase: TransferPhase::Review,
                                                        records,
                                                        ..
                                                    }) = &mut this.modal
                                                    {
                                                        records.retain(|task| task.id != id);
                                                        cx.notify();
                                                    }
                                                }),
                                            ),
                                        )
                                    }),
                            )
                    }),
                ));
                let current_batch = *batch;
                let current_owner = *owner;
                match phase {
                    TransferPhase::Review => {
                        footer = footer
                            .child(
                                Checkbox::new("overwrite-transfers")
                                    .label(self.t("overwrite_files"))
                                    .checked(*overwrite)
                                    .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                        if let Some(Modal::Transfer {
                                            phase: TransferPhase::Review,
                                            overwrite,
                                            ..
                                        }) = &mut this.modal
                                        {
                                            *overwrite = *checked;
                                            cx.notify();
                                        }
                                    })),
                            )
                            .child(div().flex_1())
                            .child(
                                self.button("cancel-transfer-review", self.t("cancel"))
                                    .on_click(
                                        cx.listener(|this, _, w, cx| this.cancel_modal(w, cx)),
                                    ),
                            )
                            .child(
                                self.button("start-transfers", self.t(action_key))
                                    .primary()
                                    .disabled(records.is_empty())
                                    .on_click(
                                        cx.listener(|this, _, w, cx| this.confirm_transfers(w, cx)),
                                    ),
                            );
                    }
                    TransferPhase::Running => {
                        footer = footer
                            .child(div().flex_1())
                            .child(
                                self.button("transfer-background", self.t("background_continue"))
                                    .on_click(cx.listener(|this, _, w, cx| this.dismiss(w, cx))),
                            )
                            .child(
                                self.button("transfer-stop", self.t("stop_pending"))
                                    .danger()
                                    .on_click(cx.listener(move |this, _, w, cx| {
                                        this.review_stop_transfers(
                                            current_owner,
                                            current_batch,
                                            w,
                                            cx,
                                        );
                                    })),
                            );
                    }
                    TransferPhase::Result => {
                        footer = footer.child(div().flex_1()).child(
                            self.button("transfer-close-result", self.t("close"))
                                .on_click(cx.listener(|this, _, w, cx| this.dismiss(w, cx))),
                        );
                    }
                }
            }
            Modal::CancelTransfers {
                owner,
                batch,
                records,
                overwrite,
                stop_ids,
                ..
            } => {
                title = self.t("stop_transfer").into();
                let host = records
                    .first()
                    .map(|record| format!("{}:{}", record.profile.host, record.profile.port))
                    .unwrap_or_default();
                let pending = records
                    .iter()
                    .filter(|record| stop_ids.contains(&record.id))
                    .collect::<Vec<_>>();
                body = body
                    .child(format!("{}: {host}", self.t("host")))
                    .child(format!("{}: {}", self.t("stop_pending"), pending.len()))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(theme::SPACE_SMALL))
                            .children(pending.iter().map(|record| {
                                div()
                                    .min_w_0()
                                    .whitespace_normal()
                                    .text_color(p.muted)
                                    .child(super::transfer_paths::display_name(record).to_string())
                            })),
                    )
                    .child(self.t("stop_transfer_hint"));
                let owner = *owner;
                let batch = *batch;
                let overwrite = *overwrite;
                let records = records.clone();
                footer = footer
                    .child(div().flex_1())
                    .child(
                        self.button("keep-transfers", self.t("keep_uploading"))
                            .on_click(cx.listener(|this, _, w, cx| this.cancel_modal(w, cx))),
                    )
                    .child(
                        self.button("stop-transfers", self.t("stop_pending"))
                            .danger()
                            .disabled(!stop_ids.iter().any(|id| {
                                self.transfers
                                    .iter()
                                    .any(|task| task.id == *id && task.state.active())
                            }))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.stop_transfer_batch(
                                    owner,
                                    batch,
                                    records.clone(),
                                    overwrite,
                                    cx,
                                );
                            })),
                    );
            }
            Modal::FollowLink {
                owner,
                link,
                target,
                directory,
            } => {
                title = self.t("review_target").into();
                body = body
                    .child(link.clone())
                    .child(format!("→ {target}"))
                    .child(self.t("symlink_hint"));
                let owner = *owner;
                let target = target.clone();
                let directory = *directory;
                footer = footer.child(
                    self.button("follow-link", self.t("open"))
                        .primary()
                        .disabled(
                            !self
                                .pane(owner)
                                .is_some_and(|p| p.state == ConnectionState::Connected),
                        )
                        .on_click(cx.listener(move |this, _, w, cx| {
                            if let Some(p) = this.pane_mut(owner) {
                                p.files.link_target = None;
                            }
                            this.dismiss(w, cx);
                            if directory {
                                this.navigate(owner, Some(target.clone()), cx);
                            } else {
                                this.open_document(owner, target.clone(), w, cx);
                            }
                        })),
                );
            }
            Modal::ResourceDetails {
                owner,
                timestamp: _,
                data,
            } => {
                title = self.t(data.kind().title_key()).into();
                // The header close button already covers dismissal; no footer strip.
                footer_hidden = true;
                // Render from the live sample so the dialog keeps refreshing;
                // the frozen snapshot only covers a missing or cleared monitor.
                let live = self
                    .pane(*owner)
                    .and_then(|pane| pane.monitor.as_ref())
                    .map(|sample| match data {
                        system::ResourceSnapshot::Cpu(_) => {
                            system::ResourceSnapshot::Cpu(sample.cpu.clone())
                        }
                        system::ResourceSnapshot::Disk(_) => {
                            system::ResourceSnapshot::Disk(sample.disks.clone())
                        }
                    });
                let data = live.as_ref().unwrap_or(data);
                body = body.child(self.render_resource_details(
                    data,
                    480_f32.min(f32::from(window.viewport_size().width) - 32.)
                        - 2. * theme::SPACE_PANEL,
                ));
            }
            Modal::ProcessDetails {
                owner,
                process,
                result,
                refresh_error,
                ..
            } => {
                title = self.t("process_details").into();
                let current = self.process_target(*owner, process);
                let unavailable = current.as_ref().err().copied();
                let current = current.ok();
                let attempt = process.identity.as_ref().and_then(|identity| {
                    self.pane(*owner).and_then(|pane| {
                        pane.process_attempts
                            .iter()
                            .rev()
                            .find(|attempt| &attempt.identity == identity)
                    })
                });
                let unavailable_key = self.process_action_reason(*owner, process);
                let pid = process.pid;
                // Labels and values share a row; values may wrap without squeezing labels.
                let field = |label: &str, value: String, prominent: bool| {
                    div()
                        .min_w_0()
                        .flex()
                        .items_start()
                        .gap(px(theme::SPACE_CONTROL))
                        .child(
                            div()
                                .flex_none()
                                .text_color(p.muted)
                                .child(label.to_string()),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .whitespace_normal()
                                .when(prominent, |value| value.font_weight(FontWeight::SEMIBOLD))
                                .child(value),
                        )
                };
                body = body.gap(px(theme::SPACE_PANEL)).child(
                    div()
                        .relative()
                        .w_full()
                        .min_w_0()
                        .grid()
                        .grid_cols(2)
                        .gap(px(theme::SPACE_SECTION))
                        .child(self.measure_process_region("detail_identity", cx))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(theme::SPACE_CONTROL))
                                .child(div().text_color(p.muted).child("PID"))
                                .child(
                                    div()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(pid.to_string()),
                                )
                                .child(
                                    self.button("copy-process-pid", "")
                                        .icon(IconName::Copy)
                                        .ghost()
                                        .w(px(20.))
                                        .h(px(20.))
                                        .p_0()
                                        .tooltip(self.t("copy_pid"))
                                        .on_click(move |_, _, cx| {
                                            cx.write_to_clipboard(ClipboardItem::new_string(
                                                pid.to_string(),
                                            ));
                                        }),
                                ),
                        )
                        .child(field(
                            self.t("process_state"),
                            current
                                .map_or("—", |process| process.state.as_str())
                                .to_string(),
                            false,
                        )),
                );
                // Keep operation outcomes and data availability together, above the metrics.
                if attempt.is_some()
                    || unavailable.is_some()
                    || refresh_error.is_some()
                    || matches!(result, Some(Err(_)))
                {
                    let bad_attempt = attempt.is_some_and(|a| {
                        matches!(
                            a.phase,
                            system::ProcessPhase::Denied
                                | system::ProcessPhase::Changed
                                | system::ProcessPhase::Unknown
                                | system::ProcessPhase::Unsupported
                        )
                    });
                    let confirmed_exit = unavailable == Some("process_gone")
                        && attempt.is_some_and(|a| a.phase == system::ProcessPhase::Gone);
                    let status_color = if bad_attempt
                        || (unavailable.is_some() && !confirmed_exit)
                        || refresh_error.is_some()
                        || matches!(result, Some(Err(_)))
                    {
                        p.error
                    } else if confirmed_exit {
                        p.meter_green
                    } else {
                        p.accent
                    };
                    let mut status = div()
                        .w_full()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(theme::SPACE_SMALL))
                        .pl(px(theme::SPACE_CONTROL))
                        .border_l_1()
                        .border_color(status_color)
                        .text_color(status_color);
                    if let Some(attempt) = attempt {
                        status = status.child(self.t(attempt.phase.key()));
                        if let Some(error) = &attempt.error {
                            status = status.child(div().whitespace_normal().child(error.clone()));
                        }
                    }
                    if let Some(key) = unavailable {
                        if !(key == "process_gone"
                            && attempt.is_some_and(|a| a.phase == system::ProcessPhase::Gone))
                        {
                            status = status.child(div().whitespace_normal().child(self.t(key)));
                        }
                    }
                    if let Some(error) = refresh_error {
                        status = status.child(div().whitespace_normal().child(error.clone()));
                    }
                    if let Some(Err(error)) = result {
                        if unavailable.map(|key| self.t(key)) != Some(error.as_str()) {
                            status = status.child(div().whitespace_normal().child(error.clone()));
                        }
                    }
                    body = body.child(status);
                }
                let started = current
                    .and_then(|p| p.started_at)
                    .and_then(|s| chrono::DateTime::from_timestamp(s, 0))
                    .map(|date| {
                        date.with_timezone(&chrono::Local)
                            .format("%Y-%m-%d %H:%M:%S")
                            .to_string()
                    })
                    .unwrap_or_else(|| "—".into());
                let fields = [
                    (
                        "CPU",
                        current
                            .filter(|p| p.cpu.is_finite())
                            .map(|p| format!("{:.1}%", p.cpu))
                            .unwrap_or_else(|| "—".into()),
                        true,
                    ),
                    (
                        self.t("process_resident_memory"),
                        current
                            .map(|p| crate::monitor::bytes(p.rss))
                            .unwrap_or_else(|| "—".into()),
                        true,
                    ),
                    (
                        self.t("username"),
                        current
                            .map(|p| p.user.clone())
                            .unwrap_or_else(|| "—".into()),
                        false,
                    ),
                    (
                        self.t("process_parent_pid"),
                        current
                            .map(|p| p.parent.to_string())
                            .unwrap_or_else(|| "—".into()),
                        false,
                    ),
                ];
                body = body.child(
                    div()
                        .relative()
                        .w_full()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(theme::SPACE_CONTROL))
                        .child(self.measure_process_region("detail_metrics", cx))
                        .child(
                            div()
                                .grid()
                                .grid_cols(2)
                                .gap_x(px(theme::SPACE_SECTION))
                                .gap_y(px(theme::SPACE_CONTROL))
                                .children(fields.into_iter().map(|(label, value, prominent)| {
                                    field(label, value, prominent)
                                })),
                        )
                        .child(field(self.t("started"), started, false)),
                );
                let details = result.as_ref().and_then(|r| r.as_ref().ok());
                let command = details.map(|d| d.command.clone());
                body = body.child(
                    div()
                        .relative()
                        .w_full()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(theme::SPACE_CONTROL))
                        .pt(px(theme::SPACE_CONTROL))
                        .border_t_1()
                        .border_color(p.border)
                        .child(self.measure_process_region("detail_command", cx))
                        .child(
                            div()
                                .w_full()
                                .min_w_0()
                                .flex()
                                .items_center()
                                .gap(px(theme::SPACE_CONTROL))
                                .child(
                                    div()
                                        .flex_1()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(self.t("process_full_command")),
                                )
                                .child(
                                    self.button("copy-process-command", "")
                                        .icon(IconName::Copy)
                                        .ghost()
                                        .w(px(20.))
                                        .h(px(20.))
                                        .p_0()
                                        .tooltip(self.t("process_copy_command"))
                                        .disabled(command.as_ref().is_none_or(String::is_empty))
                                        .on_click(move |_, _, cx| {
                                            if let Some(command) = &command {
                                                cx.write_to_clipboard(ClipboardItem::new_string(
                                                    command.clone(),
                                                ));
                                            }
                                        }),
                                ),
                        )
                        .child(
                            div()
                                .w_full()
                                .min_w_0()
                                .rounded(px(theme::SPACE_SMALL))
                                .bg(p.terminal)
                                .p(px(theme::SPACE_CONTROL))
                                .font_family(self.prefs.terminal_font.clone())
                                .whitespace_normal()
                                .child(
                                    details
                                        .map_or("—", |d| {
                                            if d.command.is_empty() {
                                                "—"
                                            } else {
                                                &d.command
                                            }
                                        })
                                        .to_string(),
                                ),
                        ),
                );
                if let Some(details) = details {
                    let status = details.supplementary_status();
                    if !status.is_empty() {
                        body = body.child(
                            div()
                                .w_full()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap(px(theme::SPACE_CONTROL))
                                .pt(px(theme::SPACE_CONTROL))
                                .border_t_1()
                                .border_color(p.border)
                                .child(
                                    div()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(self.t("process_additional_status")),
                                )
                                .child(
                                    div()
                                        .w_full()
                                        .min_w_0()
                                        .bg(p.terminal)
                                        .p(px(theme::SPACE_CONTROL))
                                        .whitespace_normal()
                                        .font_family(self.prefs.terminal_font.clone())
                                        .child(status),
                                ),
                        );
                    }
                }
                let owner = *owner;
                let process = process.clone();
                let disabled = unavailable_key.is_some() || !self.can_signal(owner, &process);
                let tooltip = self.t(unavailable_key.unwrap_or("end_process"));
                footer = footer
                    .child(div().flex_1())
                    .child(
                        self.button("end-process", self.t("end_process"))
                            .disabled(disabled)
                            .tooltip(tooltip)
                            .on_click(cx.listener({
                                let process = process.clone();
                                move |this, _, w, cx| {
                                    this.review_process_action(
                                        owner,
                                        process.clone(),
                                        crate::processes::Action::Terminate,
                                        w,
                                        cx,
                                    )
                                }
                            })),
                    )
                    .child(
                        self.button("force-process", self.t("force_end"))
                            .danger()
                            .disabled(disabled)
                            .tooltip(self.t(unavailable_key.unwrap_or("force_end")))
                            .on_click(cx.listener(move |this, _, w, cx| {
                                this.review_process_action(
                                    owner,
                                    process.clone(),
                                    crate::processes::Action::Force,
                                    w,
                                    cx,
                                )
                            })),
                    );
            }
            Modal::ProcessConfirm {
                owner,
                process,
                action,
                host,
            } => {
                let force = *action == crate::processes::Action::Force;
                title = self
                    .t(if force { "force_end" } else { "end_process" })
                    .into();
                body = body
                    .child(
                        div()
                            .min_w_0()
                            .whitespace_normal()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(process.command.clone()),
                    )
                    .child(format!("{}: {host}", self.t("host")))
                    .child(format!("PID: {}", process.pid))
                    .child(format!(
                        "{}: {} ({})",
                        self.t("process_action"),
                        self.t(if force { "force_end" } else { "end_process" }),
                        if force { "SIGKILL" } else { "SIGTERM" }
                    ))
                    .child(
                        div()
                            .min_w_0()
                            .whitespace_normal()
                            .text_color(if force { p.error } else { p.muted })
                            .child(self.t(if force {
                                "force_hint"
                            } else {
                                "terminate_hint"
                            })),
                    );
                let owner = *owner;
                let process = process.clone();
                let action = *action;
                let reason = self.process_action_reason(owner, &process);
                footer = footer.child(
                    self.button(
                        "confirm-process",
                        self.t(if force { "force_end" } else { "end_process" }),
                    )
                    .when(force, |button| button.danger())
                    .when(!force, |button| button.primary())
                    .disabled(reason.is_some() || !self.can_signal(owner, &process))
                    .tooltip(self.t(reason.unwrap_or(if force {
                        "force_end"
                    } else {
                        "end_process"
                    })))
                    .on_click(cx.listener(move |this, _, w, cx| {
                        this.submit_process(owner, process.clone(), action, w, cx)
                    })),
                );
            }
            Modal::SystemTools { owner, page } => {
                title = self
                    .t(if matches!(page, crate::model::SystemPage::Ports) {
                        "ports"
                    } else {
                        "processes"
                    })
                    .into();
                footer_hidden = true;
                body = body.child(
                    div()
                        .when(
                            matches!(
                                page,
                                crate::model::SystemPage::Ports
                                    | crate::model::SystemPage::Processes
                            ),
                            |d| d.flex_1().min_h_0().w_full(),
                        )
                        .when(*page == crate::model::SystemPage::Overview, |d| {
                            d.h(px(360.).min(window.viewport_size().height - px(160.)))
                        })
                        .flex()
                        .flex_col()
                        .overflow_hidden()
                        .child(self.render_system_page(*owner, *page, cx)),
                );
            }
            Modal::Editor { owner } => {
                title = self.t("editor").into();
                // The action row lives in the footer (file name left, actions
                // right); leaving a document stays on the header close button,
                // so the footer itself carries no close action.
                let has_document = self.pane(*owner).is_some_and(|pane| {
                    pane.active_document
                        .is_some_and(|id| pane.documents.iter().any(|d| d.id == id))
                });
                footer_hidden = !has_document;
                if has_document {
                    footer = self.editor_footer(*owner, cx);
                }
                body = body.child(self.render_editor(*owner, cx));
            }
            Modal::Transfers { owner } => {
                title = self.t("transfer").into();
                // Row actions remain state-specific; the footer removes selected
                // finished records, or all finished records without a selection.
                let records: Vec<&crate::model::TransferRecord> = self
                    .transfers
                    .iter()
                    .rev()
                    .filter(|task| task.session == Some(owner.session))
                    .collect();
                let selected_count = self.transfer_selected.len();
                let removal_ids: Vec<Id> = records
                    .iter()
                    .filter(|task| {
                        !task.state.active()
                            && (selected_count == 0 || self.transfer_selected.contains(&task.id))
                    })
                    .map(|task| task.id)
                    .collect();
                let batch_label = if selected_count > 0 {
                    format!("{}({})", self.t("remove"), removal_ids.len())
                } else {
                    self.t("remove_all_records").to_string()
                };
                let remove_owner = *owner;
                footer_hidden = records.is_empty();
                footer = footer.child(
                    self.button("transfers-batch-remove", batch_label)
                        .svg_icon("icons/trash-2.svg")
                        .danger()
                        .disabled(removal_ids.is_empty())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.remove_transfer_records(remove_owner, cx);
                        })),
                );
                body = body.child(self.render_transfers(*owner, cx));
            }
        }
        // The editor's header carries the active document's full path beside
        // the title; other dialogs show their plain title only.
        let editor_path = match modal {
            Modal::Editor { owner } => self
                .pane(*owner)
                .and_then(|pane| {
                    pane.active_document
                        .and_then(|id| pane.documents.iter().find(|d| d.id == id))
                })
                .map(|doc| doc.original.path.clone()),
            _ => None,
        };
        let close_label = self.t(
            if matches!(
                modal,
                Modal::Settings
                    | Modal::Font { .. }
                    | Modal::Connections
                    | Modal::ResourceDetails { .. }
                    | Modal::WindowControls(_)
                    | Modal::Transfers { .. }
            ) {
                "close"
            } else {
                "cancel"
            },
        );
        if matches!(modal, Modal::Connections) {
            footer = self.connection_library_footer(cx);
        } else if !matches!(
            modal,
            Modal::Profile(_)
                | Modal::Editor { .. }
                | Modal::Settings
                | Modal::LocalHistory
                | Modal::Transfers { .. }
                | Modal::Transfer { .. }
                | Modal::CancelTransfers { .. }
                | Modal::Update
                | Modal::About
        ) {
            footer = footer.child(
                self.button("dismiss-modal", close_label)
                    .on_click(cx.listener(|this, _, w, cx| this.cancel_modal(w, cx))),
            );
        }
        // GPUI can expand an auto-height flex dialog to its max-height. Measure the natural
        // footer and scroll content instead, so short resource dialogs hug their content.
        let footer_bounds = std::rc::Rc::new(std::cell::Cell::new(None::<Bounds<Pixels>>));
        let compact_footer_bounds = footer_bounds.clone();
        let footer = footer.when(compact_dialog, |footer| {
            let bounds = compact_footer_bounds.clone();
            footer.relative().child(
                canvas(move |rect, _, _| bounds.set(Some(rect)), |_, _, _, _| {})
                    .absolute()
                    .inset_0(),
            )
        });
        let footer = footer.when(
            matches!(modal, Modal::Transfer { .. }) && measure_frame,
            |footer| {
                #[cfg(debug_assertions)]
                {
                    let view = cx.entity();
                    footer.relative().child(
                        canvas(
                            move |bounds, _, cx| {
                                view.update(cx, |this, _| {
                                    if let Some(qa) = &mut this.qa {
                                        qa.transfer_footer_bounds = Some(bounds);
                                    }
                                })
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .inset_0(),
                    )
                }
                #[cfg(not(debug_assertions))]
                {
                    footer
                }
            },
        );
        let footer = footer.when(
            matches!(modal, Modal::ProcessDetails { .. }) && measure_frame,
            |footer| {
                #[cfg(debug_assertions)]
                {
                    let view = cx.entity();
                    footer.relative().child(
                        canvas(
                            move |bounds, _, cx| {
                                view.update(cx, |this, _| {
                                    if let Some(qa) = &mut this.qa {
                                        qa.process_footer_bounds = Some(bounds);
                                    }
                                });
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .inset_0(),
                    )
                }
                #[cfg(not(debug_assertions))]
                {
                    footer
                }
            },
        );
        let resource_view = cx.entity();
        let history_measure = resource_view.clone();
        // Per-frame card bounds keep history chrome measurement independent of
        // font size, footer visibility and the scroll position.
        // The connections library fills the available dialog height: its list
        // takes the leftover row and scrolls internally instead of the body.
        // History owns its fixed toolbar/list viewport and overlay scrollbar;
        // it must not inherit the generic modal-body scroller.
        let connections_modal = matches!(modal, Modal::Connections);
        let history_height = self.resource_modal_height;
        // These dialogs fill the available body height and scroll internally.
        let editor_modal = matches!(modal, Modal::Editor { .. });
        let transfer_modal = matches!(modal, Modal::Transfer { .. });
        let ports_modal = matches!(
            modal,
            Modal::SystemTools {
                page: crate::model::SystemPage::Ports,
                ..
            }
        );
        let process_modal = matches!(modal, Modal::ProcessDetails { .. });
        let process_list_modal = matches!(
            modal,
            Modal::SystemTools {
                page: crate::model::SystemPage::Processes,
                ..
            }
        );
        let full_height_modal = connections_modal
            || editor_modal
            || history_modal
            || transfer_modal
            || ports_modal
            || process_list_modal
            || process_modal;
        div()
            .absolute()
            .inset_0()
            .bg(gpui::black().opacity(if resource_dialog { 0.2 } else { 0.28 }))
            .flex()
            .items_center()
            .justify_center()
            .occlude()
            .child(
                div()
                    .id("mantash-modal")
                    .track_focus(&self.modal_focus)
                    .key_context("MantaSHModal")
                    .on_action(
                        cx.listener(|this, _: &gpui_component::input::Enter, w, cx| {
                            // Single-line inputs propagate Enter. Consume it before platform text
                            // insertion can alter a password; submission reads the unchanged value.
                            if matches!(this.modal, Some(Modal::Credentials { .. })) {
                                this.submit_credentials(w, cx);
                            } else if matches!(this.modal, Some(Modal::WindowControls(_))) {
                                this.apply_custom_window(w, cx);
                            } else if matches!(this.modal, Some(Modal::Connections)) {
                                let input = this.connection_search.clone();
                                let composing = input.update(cx, |input, cx| {
                                    EntityInputHandler::marked_text_range(input, w, cx).is_some()
                                });
                                if !composing {
                                    this.open_selected_connection(w, cx);
                                }
                            } else {
                                cx.propagate();
                            }
                        }),
                    )
                    .w(px(if matches!(modal, Modal::Connections) {
                        640.
                    } else if matches!(modal, Modal::Editor { .. }) {
                        960.
                    } else if matches!(modal, Modal::LocalHistory) {
                        // The history list uses the same 640px content width as the
                        // connection library while keeping its footer controls compact.
                        640.
                    } else if matches!(
                        modal,
                        Modal::SystemTools { .. }
                            | Modal::ProcessDetails { .. }
                            | Modal::Transfers { .. }
                    ) {
                        640.
                    } else if matches!(modal, Modal::Profile(_)) {
                        // The connection form needs room for host/port and
                        // username/password pairs at the normal form width.
                        560.
                    } else if matches!(modal, Modal::FileName { .. }) {
                        // The rename/new-directory prompt is one path line and
                        // a single input; 320px fits without waste.
                        320.
                    } else if matches!(modal, Modal::Credentials { .. }) {
                        // The password prompt is one input, a checkbox and two
                        // buttons; 360px is all it needs.
                        360.
                    } else if matches!(
                        modal,
                        Modal::Close(CloseTarget::Document(_, _) | CloseTarget::Editor(_))
                    ) {
                        // The unsaved-draft confirmation is one path per row and
                        // three footer buttons; the generic 560 bucket wastes
                        // width on it.
                        440.
                    } else if resource_dialog {
                        // CPU/disk detail dialogs: meter rows and mount names
                        // fit comfortably at 480px.
                        480.
                    } else if transfer_modal {
                        480.
                    } else if closable_dialog {
                        320.
                    } else {
                        560.
                    })
                    .min(window.viewport_size().width - px(32.))
                    .min(px(width_cap)))
                    .max_h(window.viewport_size().height - px(48.))
                    .when(history_modal, |dialog| {
                        dialog.max_h(px(600.).min(window.viewport_size().height - px(48.)))
                    })
                    .when(history_modal, |dialog| {
                        let available = window.viewport_size().height - px(48.);
                        let height = history_height.unwrap_or(320.).min(f32::from(available));
                        dialog.h(px(height)).min_h(px(320.).min(available))
                    })
                    .when(connections_modal, |dialog| {
                        // The library uses the available viewport up to 600px.
                        dialog.h((window.viewport_size().height - px(48.)).min(px(600.)))
                    })
                    .when(ports_modal, |dialog| {
                        let (count, invalid, expanded) = match modal {
                            Modal::SystemTools { owner, .. } => {
                                self.pane(*owner).map_or((0, false, 0), |pane| {
                                    let invalid = pane.monitor.as_ref().is_some_and(|sample| {
                                        sample.errors.contains_key("ports")
                                            || sample.errors.contains_key("system")
                                    });
                                    let count = if pane.state == ConnectionState::Connected
                                        && !invalid
                                    {
                                        pane.monitor.as_ref().map_or(0, |sample| sample.ports.len())
                                    } else {
                                        0
                                    };
                                    let expanded = if count > 0 {
                                        pane.port_expanded.len().min(2)
                                    } else {
                                        0
                                    };
                                    (count, invalid, expanded)
                                })
                            }
                            _ => (0, false, 0),
                        };
                        let height = (count.min(12) as f32 * (self.prefs.ui_size + 16.)
                            + if invalid { 248. } else { 204. }
                            + expanded as f32 * 128.)
                            .clamp(220., 560.);
                        dialog.h(px(height).min(window.viewport_size().height - px(48.)))
                    })
                    .when(transfer_modal, |dialog| {
                        let count = match modal {
                            Modal::Transfer { records, .. } => records.len(),
                            _ => 1,
                        };
                        let height = (count as f32 * 96. + 136.).clamp(280., 560.);
                        dialog.h((window.viewport_size().height - px(48.)).min(px(height)))
                    })
                    .when(process_list_modal, |dialog| {
                        let (count, invalid_sample) = match modal {
                            Modal::SystemTools { owner, .. } => self
                                .pane(*owner)
                                .and_then(|pane| pane.monitor.as_ref())
                                .map_or((0, false), |sample| {
                                    let invalid = sample.errors.contains_key("processes");
                                    (if invalid { 0 } else { sample.processes.len() }, invalid)
                                }),
                            _ => (0, false),
                        };
                        let height = (count.min(12) as f32 * (self.prefs.ui_size + 16.)
                            + if invalid_sample { 248. } else { 192. })
                        .clamp(220., 560.);
                        dialog.h(px(height).min(window.viewport_size().height - px(48.)))
                    })
                    .when(process_modal, |dialog| {
                        dialog.h((window.viewport_size().height - px(48.)).min(px(560.)))
                    })
                    .when(editor_modal, |dialog| {
                        dialog.h((window.viewport_size().height - px(48.)) * 0.85)
                    })
                    .flex()
                    .flex_col()
                    .rounded(px(10.))
                    // Clip body backgrounds to the rounded frame; otherwise the
                    // viewport background pokes out as square corners.
                    .overflow_hidden()
                    .border_1()
                    .border_color(p.border)
                    .bg(p.surface)
                    .shadow_lg()
                    .when(measure_frame, |dialog| {
                        dialog.relative().child(self.measure_dialog_frame(cx))
                    })
                    .child(
                        div()
                            .flex_shrink_0()
                            .min_w_0()
                            .gap(px(theme::SPACE_SMALL))
                            .px(px(12.))
                            .py(px(8.))
                            .border_b_1()
                            .border_color(p.border)
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(theme::SPACE_CONTROL))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .flex()
                                            .items_center()
                                            .gap(px(theme::SPACE_CONTROL))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .when(resource_dialog, |title| {
                                                title.text_size(px(self.prefs.ui_size + 2.))
                                            })
                                            .child(title)
                                            .when_some(editor_path, |row, path| {
                                                row.child(
                                                    div()
                                                        .min_w_0()
                                                        .flex_1()
                                                        .overflow_hidden()
                                                        .text_ellipsis()
                                                        .whitespace_nowrap()
                                                        .text_color(p.muted)
                                                        .font_weight(FontWeight::NORMAL)
                                                        .child(path),
                                                )
                                            }),
                                    )
                                    .when(
                                        self.connection_return
                                            && !matches!(modal, Modal::Profile(_)),
                                        |header| {
                                            // Library management dialogs expose a back arrow
                                            // to the connection list; Profile is handled by the
                                            // close button below so its form remains a child.
                                            header.child(
                                                self.button("library-back", "")
                                                    .icon(IconName::ArrowLeft)
                                                    .ghost()
                                                    .h(px((self.prefs.ui_size + 6.).min(32.)))
                                                    .w(px((self.prefs.ui_size + 6.).min(32.)))
                                                    .p_0()
                                                    .tooltip(self.t("back"))
                                                    .on_click(cx.listener(|this, _, w, cx| {
                                                        this.back_to_connections(w, cx);
                                                    })),
                                            )
                                        },
                                    )
                                    .when(process_modal, |header| {
                                        let refreshing = matches!(
                                            modal,
                                            Modal::ProcessDetails {
                                                refreshing: true,
                                                ..
                                            }
                                        );
                                        let refresh = self
                                            .button("refresh-process-details", "")
                                            .svg_icon("icons/refresh-cw.svg")
                                            .ghost()
                                            .w(px(24.))
                                            .h(px(24.))
                                            .p_0()
                                            .tooltip(self.t("refresh"))
                                            .disabled(refreshing);
                                        let refresh = if refreshing {
                                            refresh.icon_element(self.loading_spinner())
                                        } else {
                                            refresh
                                        };
                                        header.child(refresh.on_click(cx.listener(
                                            |this, _, _, cx| this.refresh_process_details(cx),
                                        )))
                                    })
                                    .when(
                                        (compact_dialog || closable_dialog)
                                            && (!self.connection_return
                                                || matches!(modal, Modal::Profile(_))),
                                        |header| {
                                            header.child(
                                                self.button("resource-close", "")
                                                    .icon(gpui_component::IconName::Close)
                                                    .ghost()
                                                    .h(px(
                                                        if matches!(
                                                            modal,
                                                            Modal::Transfer { .. }
                                                                | Modal::ProcessDetails { .. }
                                                                | Modal::SystemTools { .. }
                                                        ) {
                                                            24.
                                                        } else {
                                                            (self.prefs.ui_size + 6.).min(32.)
                                                        },
                                                    ))
                                                    .w(px(
                                                        if matches!(
                                                            modal,
                                                            Modal::Transfer { .. }
                                                                | Modal::ProcessDetails { .. }
                                                                | Modal::SystemTools { .. }
                                                        ) {
                                                            24.
                                                        } else {
                                                            (self.prefs.ui_size + 6.).min(32.)
                                                        },
                                                    ))
                                                    .p_0()
                                                    .on_click(cx.listener(|this, _, w, cx| {
                                                        this.cancel_modal(w, cx)
                                                    })),
                                            )
                                        },
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .relative()
                            .flex()
                            .flex_col()
                            .min_w_0()
                            .min_h_0()
                            .when(full_height_modal, |body| body.flex_1())
                            .overflow_hidden()
                            .child(
                                div()
                                    .id("modal-body")
                                    .flex()
                                    .flex_col()
                                    .min_h_0()
                                    .min_w_0()
                                    .when(transfer_modal || process_modal, |body| body.w_full())
                                    .when(full_height_modal, |body| body.flex_1())
                                    .when(
                                        !full_height_modal || transfer_modal || process_modal,
                                        |body| {
                                            body.overflow_y_scroll()
                                                .track_scroll(&self.modal_scroll)
                                                .p(px(theme::SPACE_PANEL))
                                        },
                                    )
                                    .child(if transfer_modal || process_modal {
                                        body.flex_shrink_0()
                                    } else if full_height_modal {
                                        body.flex_1().min_h_0()
                                    } else {
                                        body.flex_shrink_0()
                                    }),
                            )
                            .when(transfer_modal || process_modal, |body| {
                                body.child(self.overlay_scrollbar(
                                    "modal-scrollbar",
                                    self.modal_scroll.clone(),
                                    Resize::ModalScroll,
                                    cx,
                                ))
                            })
                            .when(!full_height_modal, |body| {
                                if matches!(modal, Modal::Settings) {
                                    body.child(self.overlay_scrollbar(
                                        "modal-scrollbar",
                                        self.modal_scroll.clone(),
                                        Resize::ModalScroll,
                                        cx,
                                    ))
                                } else {
                                    body.vertical_scrollbar(&self.modal_scroll)
                                }
                            }),
                    )
                    .when(!footer_hidden, |dialog| {
                        dialog.child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_shrink_0()
                                .px(px(theme::SPACE_PANEL))
                                .py(px(theme::SPACE_CONTROL))
                                .border_t_1()
                                .border_color(p.border)
                                .child(footer),
                        )
                    })
                    .when(compact_dialog, |dialog| {
                        dialog.relative().child(
                            canvas(
                                move |bounds, window, cx| {
                                    let Some(footer) = compact_footer_bounds.get() else {
                                        return;
                                    };
                                    resource_view.update(cx, |this, cx| {
                                        if !matches!(
                                            this.modal,
                                            Some(
                                                Modal::ResourceDetails { .. }
                                                    | Modal::WindowControls(_)
                                                    | Modal::Connections
                                                    | Modal::Profile(_)
                                            )
                                        ) {
                                            return;
                                        }
                                        let Some(content) = this.modal_scroll.bounds_for_item(0)
                                        else {
                                            return;
                                        };
                                        let viewport = this.modal_scroll.bounds();
                                        let natural = f32::from(footer.bottom() - bounds.origin.y)
                                            + theme::SPACE_PANEL
                                            + 2.
                                            + f32::from(content.size.height - viewport.size.height)
                                            + 2. * theme::SPACE_SECTION;
                                        let height = natural
                                            .ceil()
                                            .min(f32::from(window.viewport_size().height) - 48.);
                                        if this
                                            .resource_modal_height
                                            .is_none_or(|old| (old - height).abs() > 0.5)
                                        {
                                            this.resource_modal_height = Some(height);
                                            cx.notify();
                                        }
                                    });
                                },
                                |_, _, _, _| {},
                            )
                            .absolute()
                            .inset_0(),
                        )
                    }),
            )
            .when(history_modal, |dialog| {
                let measure = history_measure.clone();
                // Keep the backdrop absolute; measurement must never join page layout.
                dialog.child(
                    canvas(
                        move |bounds, window, cx| {
                            measure.update(cx, |this, cx| {
                                if let Some(qa) = &mut this.qa {
                                    qa.modal_bounds = Some(bounds);
                                }
                                let content_height = this
                                    .active_pane()
                                    .map(|pane| {
                                        let scroll = &this.history_views
                                            [pane.spec.history_scope().index()]
                                        .scroll;
                                        let viewport = scroll.bounds();
                                        this.history_content_marker
                                            .get()
                                            .map(|marker| {
                                                f32::from(
                                                    marker.bottom()
                                                        - viewport.origin.y
                                                        - scroll.offset().y,
                                                )
                                            })
                                            .unwrap_or_else(|| {
                                                f32::from(viewport.size.height)
                                                    + f32::from(scroll.max_offset().height)
                                            })
                                    })
                                    .unwrap_or(0.);
                                let available = f32::from(window.viewport_size().height) - 48.;
                                let min_height = 320f32.min(available);
                                let max_height = 600f32.min(available);
                                let viewport_height = this
                                    .active_pane()
                                    .map(|pane| {
                                        f32::from(
                                            this.history_views[pane.spec.history_scope().index()]
                                                .scroll
                                                .bounds()
                                                .size
                                                .height,
                                        )
                                    })
                                    .unwrap_or(0.);
                                // The active dialog height minus the measured list viewport
                                // is its actual header, toolbar, padding, border and footer.
                                // Only the height cap should leave the list overflowing.
                                let chrome = this
                                    .resource_modal_height
                                    .unwrap_or(min_height)
                                    .min(available)
                                    - viewport_height;
                                let height = (content_height + chrome)
                                    .ceil()
                                    .clamp(min_height, max_height);
                                if this
                                    .resource_modal_height
                                    .is_none_or(|old| (old - height).abs() > 0.5)
                                {
                                    this.resource_modal_height = Some(height);
                                    cx.notify();
                                }
                            });
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .inset_0(),
                )
            })
            .into_any_element()
    }
}
