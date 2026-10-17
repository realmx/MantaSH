//! Shared history browsing with actions bound to one modal instance and terminal attempt.
use super::*;
use gpui_component::IconName;

/// Browsing scope and execution owner are frozen when this modal instance opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(debug_assertions, derive(serde::Deserialize, serde::Serialize))]
pub struct HistoryTarget {
    pub(super) instance: Id,
    pub(super) owner: Owner,
    pub(super) scope: HistoryScope,
}

/// Browsing and selection state belongs to a history scope, shared by SSH tabs.
pub(super) struct HistoryView {
    pub search: Entity<InputState>,
    pub scroll: ScrollHandle,
    pub selected: HashSet<Id>,
    pub anchor: Option<Id>,
}

impl HistoryView {
    pub(super) fn clear_selection(&mut self) {
        self.selected.clear();
        self.anchor = None;
    }

    /// Use the visible row order for Shift ranges; hidden records cannot become targets.
    pub(super) fn select(&mut self, visible: &[Id], id: Id, shift: bool, additive: bool) {
        update_visible_selection(
            visible,
            &mut self.selected,
            &mut self.anchor,
            id,
            shift,
            additive,
        );
    }
}

/// Frozen history deletion context used to keep stale button events inert.
#[derive(Clone)]
pub(super) struct HistoryDeleteSnapshot {
    pub(super) target: HistoryTarget,
    pub(super) query: String,
    pub(super) visible_ids: Vec<Id>,
    pub(super) target_ids: Vec<Id>,
}

impl Workbench {
    pub(super) fn history_target(&self) -> Option<HistoryTarget> {
        match self.modal {
            Some(Modal::LocalHistory(target)) => Some(target),
            _ => None,
        }
    }

    pub(super) fn open_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pane) = self.active_pane() else {
            return;
        };
        let target = HistoryTarget {
            instance: Id::new_v4(),
            owner: pane.owner,
            scope: pane.spec.history_scope(),
        };
        self.show_modal(Modal::LocalHistory(target), window, cx);
    }

    /// The catalogue owns MRU order; scope is container membership, not command metadata.
    pub(super) fn visible_history(&self, scope: HistoryScope, cx: &App) -> Vec<&HistoryItem> {
        let query = self.history_views[scope.index()]
            .search
            .read(cx)
            .value()
            .trim()
            .to_lowercase();
        self.history
            .list(scope)
            .filter(|entry| entry.command.to_lowercase().contains(&query))
            .collect()
    }
    /// Snapshot the visible rows and selection before opening a fixed-target confirmation.
    pub(super) fn history_delete_snapshot(
        &self,
        target: HistoryTarget,
        cx: &App,
    ) -> HistoryDeleteSnapshot {
        let scope = target.scope;
        let query = self.history_views[scope.index()]
            .search
            .read(cx)
            .value()
            .to_string();
        let visible_ids = self
            .visible_history(scope, cx)
            .into_iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>();
        let selected = self.history_views[scope.index()].selected.clone();
        let target_ids = history_delete_targets(&visible_ids, &selected);
        HistoryDeleteSnapshot {
            target,
            query,
            visible_ids,
            target_ids,
        }
    }
    /// Reuse only this modal's active, uncancelled terminal attempt at a reliable shell prompt.
    pub(super) fn can_insert_history(&self, target: HistoryTarget, cx: &App) -> bool {
        self.history_target() == Some(target)
            && self.active_owner() == Some(target.owner)
            && self
                .pane(target.owner)
                .filter(|pane| {
                    pane.state == ConnectionState::Connected
                        && pane.spec.history_scope() == target.scope
                })
                .and_then(|pane| pane.terminal.as_ref())
                .is_some_and(|terminal| {
                    let view = terminal.read(cx);
                    view.session.owner == target.owner
                        && !view.session.cancel.is_cancelled()
                        && view.connected
                        && view
                            .session
                            .terminal
                            .try_lock()
                            .is_some_and(|buffer| buffer.command_cursor.editing())
                })
    }
    /// Insert without executing, and return focus to the terminal captured by this action.
    pub(super) fn insert_history(
        &mut self,
        target: HistoryTarget,
        command: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.can_insert_history(target, cx) {
            self.notice = Some(self.t("history_prompt_required").into());
            cx.notify();
            return;
        }
        if let Some(terminal) = self
            .pane(target.owner)
            .and_then(|pane| pane.terminal.clone())
        {
            terminal.update(cx, |terminal, cx| terminal.paste_text(command, cx));
            self.notice = None;
            self.dismiss(window, cx);
            terminal.focus_handle(cx).focus(window);
        }
    }
    /// Paste a history command and submit it immediately from the current shell prompt.
    pub(super) fn execute_history(
        &mut self,
        target: HistoryTarget,
        command: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.can_insert_history(target, cx) {
            self.notice = Some(self.t("history_prompt_required").into());
            cx.notify();
            return;
        }
        if let Some(terminal) = self
            .pane(target.owner)
            .and_then(|pane| pane.terminal.clone())
        {
            terminal.update(cx, |terminal, cx| {
                terminal.paste_text(command, cx);
                terminal.type_text("\r", cx);
            });
            self.notice = None;
            self.dismiss(window, cx);
            terminal.focus_handle(cx).focus(window);
        }
    }
    /// Validate the rendered context before opening a fixed-UUID confirmation.
    pub(super) fn review_history_delete(
        &mut self,
        snapshot: HistoryDeleteSnapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = snapshot.target;
        let current_query = self.history_views[target.scope.index()]
            .search
            .read(cx)
            .value()
            .to_string();
        let current_visible_ids = self
            .visible_history(target.scope, cx)
            .into_iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>();
        let current_selected = self.history_views[target.scope.index()].selected.clone();
        let current_targets = history_delete_targets(&current_visible_ids, &current_selected);
        let valid = self.history_target() == Some(target)
            && self.active_owner() == Some(target.owner)
            && current_query == snapshot.query
            && current_visible_ids == snapshot.visible_ids
            && current_targets == snapshot.target_ids
            && !snapshot.target_ids.is_empty();
        if !valid {
            self.notice = Some(self.t("history_context_expired").into());
            cx.notify();
            return;
        }
        self.show_modal(
            Modal::DeleteHistory {
                owner: target.owner,
                scope: target.scope,
                label: self.t(target.scope.label_key()).into(),
                ids: snapshot.target_ids,
            },
            window,
            cx,
        );
    }
    /// Keep the search toolbar stationary, with full command text and direct actions.
    pub(super) fn render_history(
        &self,
        target: HistoryTarget,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let scope = target.scope;
        let view = &self.history_views[scope.index()];
        let p = theme::Palette::new(self.prefs.effective_theme());
        let notice_is_info = self.notice.as_deref() == Some(self.t("copied"));
        let rows = self.visible_history(scope, cx);
        let can_insert = self.can_insert_history(target, cx);
        self.history_content_marker.set(None);
        let marker = self.history_content_marker.clone();
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            // Keep 12px beneath the scroll viewport even when the footer is hidden.
            .gap(px(theme::SPACE_CONTROL))
            .pt(px(theme::SPACE_PANEL))
            .pb(px(theme::SPACE_PANEL))
            .child(
                div()
                    .px(px(theme::SPACE_PANEL))
                    .flex()
                    .items_center()
                    .gap(px(theme::SPACE_CONTROL))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .on_action(cx.listener({
                                let search = view.search.clone();
                                move |this, _: &gpui_component::input::Enter, window, cx| {
                                    if search.update(cx, |input, cx| {
                                        EntityInputHandler::marked_text_range(input, window, cx)
                                            .is_some()
                                    }) {
                                        return;
                                    }
                                    let command = this
                                        .visible_history(scope, cx)
                                        .first()
                                        .map(|entry| entry.command.clone());
                                    if let Some(command) = command {
                                        this.insert_history(target, &command, window, cx);
                                    }
                                }
                            }))
                            .child(self.input_box(&view.search)),
                    ),
            )
            .when_some(self.notice.clone(), |content, notice| {
                content.child(
                    div()
                        .px(px(theme::SPACE_PANEL))
                        .text_color(if notice_is_info { p.muted } else { p.error })
                        .child(notice),
                )
            })
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child(
                        div()
                            .id(("history-list", scope.index()))
                            .track_scroll(&view.scroll)
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .pl(px(theme::SPACE_PANEL))
                            .pr(px(theme::SPACE_SECTION))
                            .when(rows.is_empty(), |list| {
                                list.child(
                                    div()
                                        .py(px(theme::SPACE_SECTION))
                                        .text_color(p.muted)
                                        .child(self.t(
                                            if view.search.read(cx).value().is_empty() {
                                                "empty_history"
                                            } else {
                                                "history_no_match"
                                            },
                                        )),
                                )
                            })
                            .children(rows.into_iter().map(|entry| {
                                let id = entry.id;
                                let copy = entry.command.clone();
                                let run = entry.command.clone();
                                let insert = entry.command.clone();
                                let selected = view.selected.contains(&id);
                                div()
                                    .id(("history-row", id.as_u128() as u64))
                                    .w_full()
                                    .flex()
                                    .flex_shrink_0()
                                    .items_center()
                                    .gap(px(theme::SPACE_SMALL))
                                    .py(px(theme::SPACE_SMALL))
                                    .border_b_1()
                                    .border_color(p.border)
                                    .bg(if selected { p.selected } else { p.surface })
                                    .hover(move |style| {
                                        style.bg(if selected { p.selected } else { p.tab_hover })
                                    })
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                                            if this.history_target() != Some(target) {
                                                return;
                                            }
                                            let visible = this
                                                .visible_history(scope, cx)
                                                .iter()
                                                .map(|entry| entry.id)
                                                .collect::<Vec<_>>();
                                            this.history_views[scope.index()].select(
                                                &visible,
                                                id,
                                                event.modifiers.shift,
                                                event.modifiers.control || event.modifiers.platform,
                                            );
                                            cx.notify();
                                            cx.stop_propagation();
                                        }),
                                    )
                                    // Command text wraps; copy/run stay in a fixed trailing column.
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .font_family(self.prefs.terminal_font.clone())
                                            .text_size(px(self.prefs.ui_size))
                                            .line_height(relative(1.3))
                                            .whitespace_normal()
                                            .child(entry.command.clone()),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .flex_shrink_0()
                                            .items_center()
                                            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                                cx.stop_propagation()
                                            })
                                            .child(
                                                self.icon_button(
                                                    ("copy-history", id.as_u128() as u64),
                                                    "copy",
                                                    IconName::Copy,
                                                )
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    cx.stop_propagation();
                                                    if this.history_target() != Some(target) {
                                                        return;
                                                    }
                                                    cx.write_to_clipboard(
                                                        ClipboardItem::new_string(copy.clone()),
                                                    );
                                                    this.notice = Some(this.t("copied").into());
                                                    cx.notify();
                                                })),
                                            )
                                            .child(
                                                self.icon_button(
                                                    ("insert-history", id.as_u128() as u64),
                                                    "insert",
                                                    IconName::ArrowDown,
                                                )
                                                .disabled(!can_insert)
                                                .on_click(cx.listener(
                                                    move |this, _, window, cx| {
                                                        cx.stop_propagation();
                                                        this.insert_history(
                                                            target, &insert, window, cx,
                                                        );
                                                    },
                                                )),
                                            )
                                            .child(
                                                self.button(
                                                    ("run-history", id.as_u128() as u64),
                                                    "",
                                                )
                                                .svg_icon("mantash-play.svg")
                                                .ghost()
                                                .primary()
                                                .tooltip(self.t("execute"))
                                                .disabled(!can_insert)
                                                .on_click(cx.listener(
                                                    move |this, _, window, cx| {
                                                        cx.stop_propagation();
                                                        this.execute_history(
                                                            target, &run, window, cx,
                                                        );
                                                    },
                                                )),
                                            ),
                                    )
                            }))
                            .child(
                                canvas(
                                    move |bounds, _, _| marker.set(Some(bounds)),
                                    |_, _, _, _| {},
                                )
                                .h(px(1.))
                                .w_full(),
                            ),
                    )
                    .child(self.overlay_scrollbar(
                        "history-scrollbar",
                        view.scroll.clone(),
                        Resize::ModalScroll,
                        cx,
                    )),
            )
            .into_any_element()
    }
}
