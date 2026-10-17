//! Modal-local commit state keeps global data updates separate from navigation and secrets.
use super::*;
use crate::{
    connections::PasswordRetry,
    services::metadata::{Applied, Mutation, Receipt},
};
use zeroize::Zeroizing;

pub(super) enum AfterCommit {
    Stay,
    Return,
    Profile {
        profile: Profile,
        connect: bool,
        reconnect: Option<Owner>,
        secret: Zeroizing<String>,
        remember: bool,
    },
}
#[derive(Default)]
pub(super) struct CommitState {
    pub request: Option<Id>,
    pub error: Option<String>,
    pub committed: bool,
    pub failed: PasswordRetry,
    pub(super) after: Option<AfterCommit>,
    pub(super) owner: Option<Owner>,
}
impl CommitState {
    pub fn busy(&self) -> bool {
        self.request.is_some()
    }
    pub fn suspend(&mut self) {
        self.after = None;
    }
}
impl Workbench {
    pub(super) fn invalidate_commit_navigation(&mut self) {
        self.modal_commit.suspend();
        self.import_request = None;
        for frame in &mut self.modal_stack {
            frame.commit.suspend();
        }
        if let Some(frame) = &mut self.modal_confirm_return {
            frame.commit.suspend();
        }
    }
    pub(super) fn submit_metadata(
        &mut self,
        mutation: Mutation,
        after: AfterCommit,
        cx: &mut Context<Self>,
    ) {
        if self.modal_commit.busy() {
            return;
        }
        self.modal_commit.error = None;
        self.modal_commit.owner = self.active_owner();
        self.modal_commit.after = Some(after);
        self.modal_commit.request = Some(self.backend.mutate_metadata(mutation));
        cx.notify();
    }
    pub(super) fn submit_import(&mut self, cx: &mut Context<Self>) {
        if self.modal_commit.busy() {
            return;
        }
        let mutation = if !self.modal_commit.failed.is_empty() {
            Mutation::RetryPasswords(self.modal_commit.failed.clone())
        } else {
            if self.modal_commit.committed {
                return;
            }
            let Some(Modal::Import { preview, replace }) = &self.modal else {
                return;
            };
            if !preview.rows.iter().any(|row| row.profile.is_some()) {
                return;
            }
            Mutation::Import {
                preview: preview.clone(),
                replace: *replace,
            }
        };
        self.submit_metadata(mutation, AfterCommit::Return, cx);
    }
    pub(super) fn metadata_receipt(
        &mut self,
        receipt: Receipt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut failed = PasswordRetry::default();
        let mut error = None;
        let succeeded = receipt.result.is_ok();
        if receipt.revision > self.metadata_revision {
            self.metadata_revision = receipt.revision;
            match receipt.result {
                Ok(Applied::History(history)) => {
                    self.history = history;
                    for scope in [HistoryScope::Local, HistoryScope::Ssh] {
                        let ids: HashSet<_> = self
                            .visible_history(scope, cx)
                            .iter()
                            .map(|item| item.id)
                            .collect();
                        let view = &mut self.history_views[scope.index()];
                        view.selected.retain(|id| ids.contains(id));
                        view.anchor = view.anchor.filter(|id| ids.contains(id));
                    }
                }
                Ok(Applied::Profiles {
                    profiles,
                    forget_failed,
                }) => {
                    self.profiles = profiles;
                    if forget_failed > 0 {
                        self.notice = Some(format!(
                            "{} ({forget_failed})",
                            self.t("credential_remove_failed")
                        ));
                    }
                }
                Ok(Applied::Import(result)) => {
                    self.profiles = result.profiles;
                    failed = result.failed;
                    if !failed.is_empty() {
                        error = Some(format!("{} ({})", self.t("passwords_failed"), failed.len()));
                    }
                }
                Ok(Applied::Workspace) => {}
                Err(message) => error = Some(format!("{}: {message}", self.t("metadata_failed"))),
            }
            let ids: HashSet<_> = self.profiles.iter().map(|profile| profile.id).collect();
            self.connection_multi.retain(|id| ids.contains(id));
            self.connection_selected = self.connection_selected.filter(|id| ids.contains(id));
            self.connection_anchor = self.connection_anchor.filter(|id| ids.contains(id));
        } else {
            return;
        }
        let current = self.modal_commit.request == Some(receipt.request);
        let active_owner = self.active_owner();
        let (state, modal) = if current {
            (&mut self.modal_commit, self.modal.as_mut())
        } else if let Some(frame) = self
            .modal_stack
            .iter_mut()
            .chain(self.modal_confirm_return.iter_mut())
            .find(|frame| frame.commit.request == Some(receipt.request))
        {
            (&mut frame.commit, Some(&mut frame.modal))
        } else {
            if let Some(error) = error {
                self.notice = Some(error);
            }
            cx.notify();
            return;
        };
        state.request = None;
        state.error = error;
        if succeeded {
            state.failed = failed;
        }
        state.committed |= succeeded;
        if succeeded && let Some(Modal::Import { preview, .. }) = modal {
            // The submitted copy is dropped by the worker. Keep only the failed retry secrets.
            use zeroize::Zeroize;
            for row in &mut preview.rows {
                if let Some(mut password) = row.password.take() {
                    password.zeroize();
                }
            }
        }
        let after = state.after.take();
        let can_navigate =
            current && state.owner == active_owner && succeeded && state.failed.is_empty();
        if can_navigate && let Some(after) = after {
            match after {
                AfterCommit::Stay => {}
                AfterCommit::Return => self.cancel_modal(window, cx),
                AfterCommit::Profile {
                    profile,
                    connect,
                    reconnect,
                    secret,
                    remember,
                } => {
                    self.connection_selected = Some(profile.id);
                    if !crate::connections::matches_query(
                        &profile,
                        &self.connection_search.read(cx).value(),
                    ) {
                        self.connection_search
                            .update(cx, |input, cx| input.set_value("", window, cx));
                    }
                    if connect {
                        if let Some(owner) = reconnect {
                            self.cancel_credential_prompt(owner);
                        }
                        self.dismiss_to_page(window, cx);
                        if let Some(owner) = reconnect {
                            if let Some(pane) = self.pane_mut(owner) {
                                let encoding = pane.spec.encoding();
                                pane.spec = SessionSpec::Ssh { profile, encoding };
                                self.reconnect(owner, secret.to_string(), remember, window, cx);
                            }
                        } else {
                            self.new_ssh(profile, secret.to_string(), remember, true, window, cx);
                        }
                    } else {
                        self.dismiss(window, cx);
                    }
                }
            }
        }
        cx.notify();
    }
}
