//! Background text cleanup applied only to the unchanged document revision.
use super::*;
use gpui::EntityInputHandler;

impl Workbench {
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
