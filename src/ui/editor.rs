//! Background text cleanup applied only to the unchanged document revision,
//! plus the reconnect rules for drafts opened by an earlier attempt.
use super::*;
use gpui::EntityInputHandler;

/// SSH endpoint a document was read from. A draft may only move to a newer
/// attempt of its session when the current spec still names this endpoint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct DocumentEndpoint {
    host: String,
    port: u16,
    username: String,
}
impl DocumentEndpoint {
    /// Endpoint of an SSH spec; local sessions have no remote documents.
    pub(super) fn of(spec: &SessionSpec) -> Option<Self> {
        match spec {
            SessionSpec::Ssh { profile, .. } => Some(Self {
                host: profile.host.clone(),
                port: profile.port,
                username: profile.username.clone(),
            }),
            SessionSpec::Local { .. } => None,
        }
    }
}

/// How a pane treats one of its documents relative to the pane's current attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DocumentBinding {
    /// Read by the current attempt: open, edit and save normally.
    Current,
    /// Clean leftover of an earlier attempt: discard it and read fresh
    /// content through the current attempt instead of reusing it.
    StaleClean,
    /// Unsaved draft of an earlier attempt. Saving stays disabled until the
    /// user explicitly continues on the current connection, which is offered
    /// only when `can_continue` holds.
    StaleDraft { can_continue: bool },
}

/// Decide whether a document belongs to the pane's live attempt, is a
/// disposable clean leftover, or is a draft that may be rebound. A rebind
/// requires the same session, a connected pane, no save in flight and an
/// unchanged host/port/username; otherwise saving the draft stays disabled.
pub(super) fn document_binding(
    document_owner: Owner,
    document_endpoint: Option<&DocumentEndpoint>,
    dirty: bool,
    saving: bool,
    pane_owner: Owner,
    pane_connected: bool,
    pane_endpoint: Option<&DocumentEndpoint>,
) -> DocumentBinding {
    if document_owner == pane_owner {
        return DocumentBinding::Current;
    }
    let same_session = document_owner.session == pane_owner.session;
    if same_session && !dirty && !saving {
        return DocumentBinding::StaleClean;
    }
    DocumentBinding::StaleDraft {
        can_continue: same_session
            && !saving
            && pane_connected
            && document_endpoint.is_some()
            && document_endpoint == pane_endpoint,
    }
}

impl Workbench {
    /// Binding of one document in the pane that currently holds it.
    pub(super) fn binding_of(pane: &Pane, doc: &Document) -> DocumentBinding {
        document_binding(
            doc.owner,
            doc.endpoint.as_ref(),
            doc.dirty,
            doc.saving,
            pane.owner,
            pane.state == ConnectionState::Connected,
            DocumentEndpoint::of(&pane.spec).as_ref(),
        )
    }
    /// Move an earlier attempt's draft onto the pane's live attempt so the
    /// existing protective save path can write it. Only drafts that
    /// [`document_binding`] marks as `can_continue` are rebound; old
    /// open/save/format requests were already cleared by `reconnect`.
    pub(super) fn continue_document_on_current(
        &mut self,
        pane_owner: Owner,
        id: Id,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pane) = self.pane_mut(pane_owner) else {
            return;
        };
        let Some(index) = pane.documents.iter().position(|doc| doc.id == id) else {
            return;
        };
        if Self::binding_of(pane, &pane.documents[index])
            != (DocumentBinding::StaleDraft { can_continue: true })
        {
            return;
        }
        let endpoint = DocumentEndpoint::of(&pane.spec);
        let doc = &mut pane.documents[index];
        doc.owner = pane_owner;
        doc.endpoint = endpoint;
        doc.open_request = None;
        doc.saving = false;
        doc.save_snapshot = None;
        doc.formatting = None;
        doc.error = None;
        doc.preview = false;
        let input = doc.input.clone();
        pane.active_document = Some(id);
        input.focus_handle(cx).focus(window);
        cx.notify();
    }
    /// Switch a safe (non-lossy) preview into the editable editor without re-fetching.
    pub(super) fn promote_preview_to_edit(
        &mut self,
        owner: Owner,
        id: Id,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(doc) = self.document_mut(owner, id) else {
            return;
        };
        if !doc.preview || doc.original.lossy {
            return;
        }
        doc.preview = false;
        doc.original.read_only = false;
        let input = doc.input.clone();
        input.focus_handle(cx).focus(window);
        cx.notify();
    }
    pub(super) fn format_document(
        &mut self,
        owner: Owner,
        id: Id,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(doc) = self.document_mut(owner, id) else {
            return;
        };
        if doc.preview || doc.original.lossy || doc.formatting.is_some() || doc.saving {
            return;
        }
        let input = doc.input.clone();
        if input.update(cx, |state, cx| {
            state.marked_text_range(window, cx).is_some()
        }) {
            return;
        }
        let revision = doc.revision;
        let text = input.read(cx).value();
        let before = input.read(cx).edit_snapshot();
        let request = Id::new_v4();
        doc.formatting = Some(request);
        doc.error = None;
        let original = text.clone();
        let selection = before.selection.clone();
        let task = self
            .backend
            .runtime
            .spawn_blocking(move || crate::editor_format::format_selection(&text, selection));
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                let stale_message = this.t("format_stale").to_owned();
                let failure_message = this.t("format_failed").to_owned();
                let Some(doc) = this.document_mut(owner, id) else {
                    return;
                };
                if doc.formatting != Some(request) {
                    return;
                }
                doc.formatting = None;
                if doc.revision != revision
                    || doc.input.read(cx).value() != original
                    || doc.saving
                    || doc.input.read(cx).edit_snapshot() != before
                    || doc.input.update(cx, |state, cx| {
                        state.marked_text_range(window, cx).is_some()
                    })
                {
                    doc.error = Some(stale_message);
                    cx.notify();
                    return;
                }
                match result {
                    Ok(formatted) if formatted.text != original.as_ref() => {
                        let input = doc.input.clone();
                        let mut after = before;
                        after.selection = formatted.selection;
                        if !input.update(cx, |state, cx| {
                            state.apply_edit_transaction(&formatted.text, after, window, cx)
                        }) {
                            doc.error = Some(stale_message);
                        }
                    }
                    Ok(_) => {}
                    Err(_) => doc.error = Some(failure_message),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    pub(super) fn editor_toolbar(&self, owner: Owner, cx: &mut Context<Self>) -> Div {
        // The editor dialog body has no generic padding (the editor fills it
        // edge to edge), so the toolbar insets itself to line up with the
        // header and footer.
        let mut toolbar = div()
            .flex()
            .items_center()
            .gap(px(theme::SPACE_CONTROL))
            .px(px(theme::SPACE_PANEL))
            .pt(px(theme::SPACE_CONTROL));
        let Some(doc) = self.pane(owner).and_then(|pane| {
            pane.documents
                .iter()
                .find(|doc| Some(doc.id) == pane.active_document)
        }) else {
            return toolbar;
        };
        let id = doc.id;
        let doc_owner = doc.owner;
        let preview = doc.preview;
        let can_edit = preview && !doc.original.lossy;
        toolbar = toolbar
            .child(div().flex_1().min_w_0().child(doc.original.path.clone()))
            .child(
                self.button("editor-encoding", doc.encoding.label())
                    .disabled(preview)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.show_modal(
                            Modal::Encoding {
                                owner: Some(doc_owner),
                                document: Some(id),
                            },
                            window,
                            cx,
                        )
                    })),
            )
            .when(preview, |bar| {
                bar.child(
                    self.button("edit-from-preview", self.t("edit"))
                        .tooltip(self.t("edit_from_preview_hint"))
                        .disabled(!can_edit || doc_owner != owner)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.promote_preview_to_edit(owner, id, window, cx)
                        })),
                )
            })
            .when(!preview, |bar| {
                bar.child(
                    self.button("format-document", self.t("format_text"))
                        .tooltip(self.t("format_text_hint"))
                        .disabled(doc.formatting.is_some() || doc.saving || doc_owner != owner)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.format_document(owner, id, window, cx)
                        })),
                )
            });
        toolbar
    }
}

#[cfg(test)]
mod tests {
    use super::{DocumentBinding, DocumentEndpoint, document_binding};
    use crate::model::{Id, Owner};

    fn endpoint(host: &str, port: u16, username: &str) -> DocumentEndpoint {
        DocumentEndpoint {
            host: host.into(),
            port,
            username: username.into(),
        }
    }

    fn owners() -> (Owner, Owner) {
        let session = Id::new_v4();
        (
            Owner {
                session,
                attempt: Id::new_v4(),
            },
            Owner {
                session,
                attempt: Id::new_v4(),
            },
        )
    }

    #[test]
    fn current_attempt_documents_are_used_directly() {
        let (owner, _) = owners();
        let here = endpoint("h", 22, "root");
        for (dirty, saving) in [(false, false), (true, false), (true, true)] {
            assert_eq!(
                document_binding(owner, Some(&here), dirty, saving, owner, false, Some(&here)),
                DocumentBinding::Current
            );
        }
    }

    #[test]
    fn clean_stale_documents_are_reloaded_not_reused() {
        let (old, new) = owners();
        let here = endpoint("h", 22, "root");
        let elsewhere = endpoint("other", 22, "root");
        // Clean leftovers are dropped whatever the connection state or
        // endpoint: they hold no user changes and the fresh read decides.
        for connected in [true, false] {
            for current in [&here, &elsewhere] {
                assert_eq!(
                    document_binding(
                        old,
                        Some(&here),
                        false,
                        false,
                        new,
                        connected,
                        Some(current)
                    ),
                    DocumentBinding::StaleClean
                );
            }
        }
    }

    #[test]
    fn dirty_stale_draft_can_continue_on_same_endpoint() {
        let (old, new) = owners();
        let here = endpoint("h", 2222, "deploy");
        assert_eq!(
            document_binding(
                old,
                Some(&here),
                true,
                false,
                new,
                true,
                Some(&here.clone())
            ),
            DocumentBinding::StaleDraft { can_continue: true }
        );
    }

    #[test]
    fn dirty_stale_draft_cannot_continue_when_unsafe() {
        let (old, new) = owners();
        let here = endpoint("h", 22, "root");
        let cases = [
            // Reconnect still in progress or failed.
            (Some(here.clone()), false, Some(here.clone())),
            // Host, port or username edited in the profile before reconnecting.
            (Some(here.clone()), true, Some(endpoint("h2", 22, "root"))),
            (Some(here.clone()), true, Some(endpoint("h", 2222, "root"))),
            (Some(here.clone()), true, Some(endpoint("h", 22, "admin"))),
            // Unknown endpoint on either side never matches.
            (None, true, Some(here.clone())),
            (Some(here.clone()), true, None),
            (None, true, None),
        ];
        for (document, connected, pane) in cases {
            assert_eq!(
                document_binding(
                    old,
                    document.as_ref(),
                    true,
                    false,
                    new,
                    connected,
                    pane.as_ref()
                ),
                DocumentBinding::StaleDraft {
                    can_continue: false
                }
            );
        }
    }

    #[test]
    fn foreign_session_or_saving_draft_never_rebinds() {
        let (old, new) = owners();
        let here = endpoint("h", 22, "root");
        let foreign = Owner {
            session: Id::new_v4(),
            attempt: new.attempt,
        };
        assert_eq!(
            document_binding(old, Some(&here), false, false, foreign, true, Some(&here)),
            DocumentBinding::StaleDraft {
                can_continue: false
            }
        );
        assert_eq!(
            document_binding(old, Some(&here), true, true, new, true, Some(&here)),
            DocumentBinding::StaleDraft {
                can_continue: false
            }
        );
    }
}
