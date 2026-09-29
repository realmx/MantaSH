//! Selected paired workbench: native terminals with a contextual SSH tool page.
use super::*;
use dialogs::CloseTarget;
use gpui_component::{IconName, InteractiveElementExt, Sizable, TitleBar, input::Input};
use std::rc::Rc;
use tools::{
    FileMenuCopy, FileMenuDelete, FileMenuDownload, FileMenuEdit, FileMenuMkdir, FileMenuPaste,
    FileMenuRename,
};

/// Render snapshot for one connection library row; `uniform_list` builds any
/// visible range from these owned values without borrowing the workbench.
/// Selection flags are read live at paint time, so they are not cached here.
struct ConnectionRow {
    id: Id,
    name: String,
    endpoint: String,
}

/// Filtered rows cached between renders, keyed by the query and a fingerprint
/// of every profile field the rows and filter depend on. Scroll frames change
/// neither, so they reuse the snapshot without re-filtering or reallocating.
pub(super) struct ConnectionRowsCache {
    rows: Rc<Vec<ConnectionRow>>,
    query: String,
    fingerprint: u64,
}

impl Default for ConnectionRowsCache {
    fn default() -> Self {
        Self {
            rows: Rc::new(Vec::new()),
            query: String::new(),
            fingerprint: 0,
        }
    }
}

/// Hash every profile field that feeds row identity, row text, or the search
/// filter, so any mutation forces a snapshot rebuild and stale rows cannot
/// survive profile edits, imports, or deletions.
fn connection_fingerprint(profiles: &[Profile]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    profiles.len().hash(&mut hasher);
    for profile in profiles {
        profile.id.as_u128().hash(&mut hasher);
        profile.name.hash(&mut hasher);
        profile.host.hash(&mut hasher);
        profile.port.hash(&mut hasher);
        profile.username.hash(&mut hasher);
    }
    hasher.finish()
}

/// Build the filtered row snapshot for the current query.
fn connection_rows_snapshot(profiles: &[Profile], query: &str) -> Rc<Vec<ConnectionRow>> {
    Rc::new(
        profiles
            .iter()
            .filter(|profile| crate::connections::matches_query(profile, query))
            .map(|profile| ConnectionRow {
                id: profile.id,
                name: profile.name.clone(),
                endpoint: profile.endpoint(),
            })
            .collect(),
    )
}

/// Build one library row from a snapshot row; selection flags come from the
/// live workbench state. Click handlers re-resolve live state through the
/// entity, so a click on a stale row is ignored instead of acting on
/// outdated connection data.
fn connection_library_row(
    row: &ConnectionRow,
    ix: usize,
    drag_slot: Option<usize>,
    dragging: bool,
    visible_len: usize,
    workbench: &Workbench,
    click_workbench: Entity<Workbench>,
    p: theme::Palette,
) -> Stateful<Div> {
    let id = row.id;
    // Manual sort feedback: a 2px accent line at this row's slot (or below
    // the last row for a tail drop); the dragged row dims slightly.
    let line_above = drag_slot == Some(ix);
    let line_below = drag_slot == Some(visible_len) && ix + 1 == visible_len;
    let selected =
        workbench.connection_multi.contains(&id) || workbench.connection_selected == Some(id);
    let down_workbench = click_workbench.clone();
    // Each row action re-resolves live state through its own entity handle.
    let edit_workbench = click_workbench.clone();
    let clone_workbench = click_workbench.clone();
    let connect_workbench = click_workbench.clone();
    div()
        .id(("connection-row", id.as_u128() as u64))
        .flex()
        .flex_shrink_0()
        .relative()
        .when(dragging, |row| row.opacity(0.6))
        // uniform_list lays each item out as a fit-content root, so without an
        // explicit width every row stops at its own content and the selection
        // background falls short of the dialog edge.
        .w_full()
        .items_center()
        .gap(px(4.))
        .pl(px(8.))
        // Keep the right-edge action buttons clear of the 10px scroll strip,
        // which overlays the last 11px of every row and would swallow clicks.
        .pr(px(12.))
        .py(px(4.))
        .rounded(px(4.))
        .bg(if selected { p.selected } else { p.surface })
        .hover(move |style| style.bg(if selected { p.selected } else { p.tab_hover }))
        .cursor_pointer()
        // QA-only: record this row's painted bounds so drivers can assert the
        // row spans the full list width (uniform_list items are fit-content
        // roots without the explicit `.w_full()` above).
        .when(line_above, |row| {
            row.child(
                div()
                    .absolute()
                    .top(px(-1.))
                    .left(px(2.))
                    .right(px(2.))
                    .h(px(2.))
                    .rounded(px(1.))
                    .bg(p.accent),
            )
        })
        .when(line_below, |row| {
            row.child(
                div()
                    .absolute()
                    .bottom(px(-1.))
                    .left(px(2.))
                    .right(px(2.))
                    .h(px(2.))
                    .rounded(px(1.))
                    .bg(p.accent),
            )
        })
        .when(cfg!(debug_assertions) && workbench.qa.is_some(), |row| {
            let record = click_workbench.clone();
            let row_id = id.as_u128();
            row.child(
                canvas(
                    move |bounds, _, cx| {
                        record.update(cx, |this, _| {
                            if let Some(qa) = &mut this.qa {
                                qa.connection_rows.insert(row_id, bounds);
                            }
                        });
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
        })
        .on_click(move |event: &ClickEvent, window, cx| {
            click_workbench.update(cx, |this, cx| {
                if !matches!(this.modal, Some(Modal::Connections)) {
                    return;
                }
                if this.connection_suppress_click == Some(id) {
                    this.connection_suppress_click = None;
                    return;
                }
                this.connection_selected = this.connection_multi.contains(&id).then_some(id);
                if event.click_count() == 2 {
                    if let Some(profile) = this
                        .profiles
                        .iter()
                        .find(|profile| profile.id == id)
                        .cloned()
                    {
                        this.open_saved(profile, true, window, cx);
                    }
                }
                cx.notify();
            });
        })
        .on_mouse_down(MouseButton::Left, move |event: &MouseDownEvent, _, cx| {
            down_workbench.update(cx, |this, cx| {
                if !matches!(this.modal, Some(Modal::Connections)) {
                    return;
                }
                if this.connection_suppress_click == Some(id) {
                    this.connection_suppress_click = None;
                }
                // Plain click selects one row, Cmd/Ctrl toggles membership,
                // Shift extends a range over the visible order.
                let visible: Vec<Id> = this
                    .visible_connections(cx)
                    .iter()
                    .map(|profile| profile.id)
                    .collect();
                update_visible_selection(
                    &visible,
                    &mut this.connection_multi,
                    &mut this.connection_anchor,
                    id,
                    event.modifiers.shift,
                    event.modifiers.control || event.modifiers.platform,
                );
                if !event.modifiers.shift {
                    this.connection_anchor = Some(id);
                }
                this.connection_selected = this.connection_multi.contains(&id).then_some(id);
                // A plain (non-shift, non-additive) press may become a
                // manual-sort drag; the 4px threshold keeps clicks clean.
                if !event.modifiers.shift && !event.modifiers.control && !event.modifiers.platform {
                    this.begin_connection_drag(id, event);
                    this.spawn_connection_drag_ticker(cx);
                }
                cx.notify();
            });
            cx.stop_propagation();
        })
        .child(div().flex_1().min_w_0().truncate().child(row.name.clone()))
        .child(
            div()
                .flex_shrink_0()
                .min_w_0()
                .truncate()
                .text_color(p.muted)
                .child(row.endpoint.clone()),
        )
        // Per-row actions mirror the footer buttons but act directly on this
        // row's connection. They use the 20px chromeless icon tier so the
        // virtualized rows stay cheap to rebuild while scrolling; the
        // container stops mouse-down so the row's selection logic never
        // treats an action click as a selection change or double-click open.
        .child(
            div()
                .flex()
                .flex_shrink_0()
                .items_center()
                .gap(px(4.))
                .ml(px(4.))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    workbench
                        .button(("connection-row-edit", id.as_u128() as u64), "")
                        .svg_icon("icons/pencil.svg")
                        .ghost()
                        .chromeless()
                        .w(px(20.))
                        .h(px(20.))
                        .tooltip(workbench.t("edit"))
                        .on_click(move |_, window, cx| {
                            cx.stop_propagation();
                            edit_workbench.update(cx, |this, cx| {
                                if !matches!(this.modal, Some(Modal::Connections)) {
                                    return;
                                }
                                if let Some(profile) = this
                                    .profiles
                                    .iter()
                                    .find(|profile| profile.id == id)
                                    .cloned()
                                {
                                    this.profile_form(Some(profile), None, window, cx);
                                }
                            });
                        }),
                )
                .child(
                    workbench
                        .button(("connection-row-clone", id.as_u128() as u64), "")
                        .icon(IconName::Copy)
                        .ghost()
                        .chromeless()
                        .w(px(20.))
                        .h(px(20.))
                        .tooltip(workbench.t("clone_connection"))
                        .on_click(move |_, window, cx| {
                            cx.stop_propagation();
                            clone_workbench.update(cx, |this, cx| {
                                if !matches!(this.modal, Some(Modal::Connections)) {
                                    return;
                                }
                                if let Some(profile) = this
                                    .profiles
                                    .iter()
                                    .find(|profile| profile.id == id)
                                    .cloned()
                                {
                                    this.clone_connection(&profile, window, cx);
                                }
                            });
                        }),
                )
                .child(
                    workbench
                        .button(("connection-row-connect", id.as_u128() as u64), "")
                        .svg_icon("icons/link-2.svg")
                        .ghost()
                        .chromeless()
                        .w(px(20.))
                        .h(px(20.))
                        // Library connects always open a fresh tab; duplicate
                        // sessions of one profile are allowed.
                        .tooltip(workbench.t("connect"))
                        .on_click(move |_, window, cx| {
                            cx.stop_propagation();
                            connect_workbench.update(cx, |this, cx| {
                                if !matches!(this.modal, Some(Modal::Connections)) {
                                    return;
                                }
                                if let Some(profile) = this
                                    .profiles
                                    .iter()
                                    .find(|profile| profile.id == id)
                                    .cloned()
                                {
                                    this.open_saved(profile, true, window, cx);
                                }
                            });
                        }),
                ),
        )
}

impl Workbench {
    fn modal_tab(&self, backwards: bool, window: &mut Window, cx: &mut App) {
        if backwards {
            window.focus_prev();
        } else {
            window.focus_next();
        }
        let _ = cx;
    }
    pub(super) fn toolbar_height(&self) -> f32 {
        (self.prefs.ui_size * 1.45 + 16.).max(38.).min(44.)
    }
    pub(super) fn controls_height(&self) -> f32 {
        (self.prefs.ui_size * 1.45 + 8.).max(30.).min(32.)
    }
    /// Uniform single-line input height: the same 24px tier as standard
    /// buttons, scaling with the font up to the 32px cap.
    pub(super) fn input_height(&self) -> f32 {
        (self.prefs.ui_size * 1.45 + 2.).clamp(24., 32.)
    }
    /// Shared styling for every input box: the component's Small tier keeps the
    /// 2px vertical padding that lets a 14px line fit the 24px height, then the
    /// height follows the same 24px tier as buttons. The SFTP document editor
    /// is the only Input exempt because it fills its pane as an editing
    /// surface.
    pub(super) fn input_box(&self, input: &Entity<InputState>) -> Input {
        Input::new(input).small().h(px(self.input_height()))
    }
    /// Keep header actions outside the scroll clip; QA observes their actual native hit areas.
    pub(super) fn header_control(
        &self,
        _name: &'static str,
        control: impl IntoElement,
        _cx: &mut Context<Self>,
    ) -> AnyElement {
        let control = div()
            .flex()
            .flex_shrink_0()
            .items_center()
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(control);
        #[cfg(target_os = "macos")]
        let control = {
            let bounds = self.tab_strip.control_bounds.clone();
            control.relative().child(
                canvas(
                    move |rect, _, _| {
                        bounds.borrow_mut().insert(_name, rect);
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
        };
        #[cfg(debug_assertions)]
        let control = control.when(self.qa.is_some(), |d| {
            let measure = _cx.entity();
            d.relative().child(
                canvas(
                    move |bounds, _, cx| {
                        measure.update(cx, |this, _| {
                            if let Some(qa) = &mut this.qa {
                                qa.header_controls.insert(_name, bounds);
                            }
                        });
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
        });
        control.into_any_element()
    }
    pub(super) fn pane_label(&self, pane: &Pane) -> String {
        match &pane.spec {
            SessionSpec::Ssh { profile, .. } => {
                if profile.name.trim().is_empty() {
                    profile.address()
                } else {
                    profile.name.clone()
                }
            }
            SessionSpec::Local { shell, .. } => {
                crate::titles::local_label(&pane.directory, pane.program.as_deref(), shell)
            }
        }
    }
    fn pane_tooltip(&self, pane: &Pane) -> String {
        let identity = match &pane.spec {
            SessionSpec::Local { .. } => self.t("local").to_string(),
            SessionSpec::Ssh { profile, .. } => {
                format!("{} · {}", profile.name, profile.endpoint())
            }
        };
        format!(
            "{}\n{}\n{}",
            identity,
            crate::titles::clean(&pane.directory, 4096),
            crate::titles::clean(&pane.title, 256)
        )
    }
    /// Reveal a selected tab after navigation or a layout change, never on output-only renders.
    fn render_tabs(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let palette = theme::Palette::new(self.prefs.theme);
        let active = self.tabs.get(self.active).map(|tab| (tab.id, self.active));
        let label = self
            .active_pane()
            .map(|pane| self.pane_label(pane))
            .unwrap_or_default();
        let strip = &mut self.tab_strip;
        let active_was_visible = strip
            .scroll
            .bounds_for_item(self.active)
            .is_some_and(|bounds| {
                let offset = strip.scroll.offset().x;
                bounds.left() + offset >= strip.scroll.bounds().left() - px(1.)
                    && bounds.right() + offset <= strip.scroll.bounds().right() + px(1.)
            });
        let reveal = strip.active != active
            || (strip.label != label && active_was_visible)
            || strip.viewport_width != window.viewport_size().width
            || strip.fullscreen != window.is_fullscreen()
            || strip.font != self.prefs.ui_font
            || strip.font_size != self.prefs.ui_size
            || strip.language != self.prefs.language;
        if reveal {
            if active.is_some() {
                strip.scroll.scroll_to_item(self.active);
            }
            strip.active = active;
            strip.viewport_width = window.viewport_size().width;
            strip.fullscreen = window.is_fullscreen();
            strip.font.clone_from(&self.prefs.ui_font);
            strip.font_size = self.prefs.ui_size;
            strip.language = self.prefs.language;
        }
        strip.label = label;
        let scroll = self.tab_strip.scroll.clone();
        let view = cx.entity_id();
        div()
            .on_children_prepainted(move |_, _, cx| {
                if reveal && let Some((_, index)) = active {
                    // GPUI 0.2.2 refreshes scroll bounds after its reveal pass. Repeat
                    // once against the measured bounds for first paint, resize and fonts.
                    scroll.scroll_to_item(index);
                    cx.notify(view);
                }
            })
            .id("work-tabs")
            .flex()
            .flex_1()
            .min_w_0()
            .overflow_x_scroll()
            .track_scroll(&self.tab_strip.scroll)
            .items_center()
            .gap(px(4.))
            .children(self.tabs.iter().enumerate().map(|(index, tab)| {
                let id = tab.id;
                let selected = index == self.active;
                let pane = tab.panes.get(tab.active).unwrap_or(&tab.panes[0]);
                let label = self.pane_label(pane);
                let tooltip = self.pane_tooltip(pane);
                let ssh = matches!(pane.spec, SessionSpec::Ssh { .. });
                // One fixed-size dot per SSH tab: green connected, blue connecting,
                // red disconnected/failed. The slot never changes tab width.
                let state_color = if tab
                    .panes
                    .iter()
                    .any(|p| matches!(p.state, ConnectionState::Connected))
                {
                    palette.meter_green
                } else if tab.panes.iter().any(|p| {
                    matches!(
                        p.state,
                        ConnectionState::Connecting
                            | ConnectionState::Authenticating
                            | ConnectionState::HostVerification
                            | ConnectionState::CredentialsRequired
                    )
                }) {
                    palette.meter_blue
                } else {
                    palette.meter_red
                };
                let dragging = self
                    .tab_strip
                    .drag
                    .as_ref()
                    .is_some_and(|drag| drag.tab == id && drag.moved);
                let marker = self.tab_strip.drag.as_ref().filter(|drag| drag.moved);
                let before = marker.is_some_and(|drag| drag.before == Some(id));
                let after = marker
                    .is_some_and(|drag| drag.before.is_none() && index + 1 == self.tabs.len());
                div()
                    .id(("tab", id.as_u128() as u64))
                    .group("work-tab")
                    .relative()
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .h(px(self.controls_height()))
                    .pr(px(4.))
                    .max_w(px(self.prefs.ui_size * 20.))
                    .rounded(px(4.))
                    .bg(if selected {
                        palette.tab_selected
                    } else {
                        transparent_black()
                    })
                    .when(!selected, |d| d.hover(|style| style.bg(palette.tab_hover)))
                    .occlude()
                    .when(dragging, |tab| tab.opacity(0.65))
                    .on_scroll_wheel(
                        cx.listener(|this, event, _, cx| this.scroll_tabs_from_pointer(event, cx)),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event, window, cx| {
                            this.begin_tab_drag(id, event, window, cx)
                        }),
                    )
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .when(before || after, |tab| {
                        tab.child(
                            div()
                                .absolute()
                                .top(px(3.))
                                .bottom(px(3.))
                                .w(px(2.))
                                .when(before, |line| line.left_0())
                                .when(after, |line| line.right_0())
                                .bg(palette.accent),
                        )
                    })
                    .when(ssh, |tab| {
                        tab.child(
                            div()
                                .flex_none()
                                .ml(px(5.))
                                .size(px((self.prefs.ui_size * 0.5).max(6.)))
                                .rounded_full()
                                .bg(state_color),
                        )
                    })
                    .child(
                        self.button(("tab-switch", id.as_u128() as u64), label)
                            .tab_label(selected)
                            .h(px(self.controls_height()))
                            .min_w_0()
                            .tooltip(tooltip)
                            .on_click(cx.listener(move |this, event, w, cx| {
                                cx.stop_propagation();
                                if this.tab_strip.suppress_click == Some(id)
                                    && matches!(event, ClickEvent::Mouse(_))
                                {
                                    return;
                                }
                                if let Some(index) = this.tabs.iter().position(|t| t.id == id) {
                                    this.active = index;
                                }
                                this.focus_active(w, cx);
                                this.changed(cx);
                            })),
                    )
                    .child(
                        div()
                            .occlude()
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .child(
                                self.button(("tab-close", id.as_u128() as u64), "")
                                    .icon(IconName::Close)
                                    .tab_close()
                                    .rounded(px(3.))
                                    .p_0()
                                    // One tier below the old
                                    // (controls_height-6).max(24) size: the
                                    // 20px small-icon tier of the size ladder.
                                    .h(px(20.))
                                    .w(px(20.))
                                    .tooltip(self.t("close"))
                                    .on_click(cx.listener(move |this, _, w, cx| {
                                        cx.stop_propagation();
                                        this.request_close(CloseTarget::Tab(id), w, cx)
                                    })),
                            ),
                    )
                    .when(selected, |d| {
                        d.child(
                            div()
                                .absolute()
                                .bottom_0()
                                .left(px(10.))
                                .right(px(10.))
                                .h(px(2.))
                                .rounded(px(1.))
                                // SSH tabs mirror their status dot (green/blue/
                                // red); local tabs keep the accent underline.
                                .bg(if ssh { state_color } else { palette.accent }),
                        )
                    })
            }))
            .into_any_element()
    }
    /// Derive keyboard targets only from the current query, never a previously filtered row.
    pub(super) fn visible_connections(&self, cx: &App) -> Vec<&Profile> {
        let query = self.connection_search.read(cx).value();
        self.profiles
            .iter()
            .filter(|profile| crate::connections::matches_query(profile, &query))
            .collect()
    }
    /// Keep the highlighted connection stable by UUID, with the first visible result as fallback.
    pub(super) fn selected_connection(&self, cx: &App) -> Option<&Profile> {
        let visible = self.visible_connections(cx);
        visible
            .iter()
            .find(|profile| Some(profile.id) == self.connection_selected)
            .copied()
            .or_else(|| visible.first().copied())
    }
    /// Open the current visible selection through the normal host-verification and credential flow.
    /// Open the highlighted row; library opens always start a fresh tab.
    pub(super) fn open_selected_connection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.modal, Some(Modal::Connections)) {
            return;
        }
        if let Some(profile) = self.selected_connection(cx).cloned() {
            self.open_saved(profile, true, window, cx);
        }
    }
    /// Move within visible results and reveal the new row without moving the search field.
    fn step_connection(&mut self, backwards: bool, cx: &mut Context<Self>) {
        let visible = self.visible_connections(cx);
        if visible.is_empty() {
            return;
        }
        let current = visible
            .iter()
            .position(|profile| Some(profile.id) == self.connection_selected)
            .unwrap_or(0);
        let next = if backwards {
            current.saturating_sub(1)
        } else {
            (current + 1).min(visible.len() - 1)
        };
        let next_id = visible[next].id;
        self.connection_selected = Some(next_id);
        self.connection_anchor = Some(next_id);
        self.connection_scroll
            .scroll_to_item(next, ScrollStrategy::Top);
        cx.notify();
    }
    /// Keep saved targets in a full-width list and act on the current selection below it.
    pub(super) fn render_connection_library(
        &self,
        _window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = theme::Palette::new(self.prefs.theme);
        // Scrolling re-renders every frame; rebuild the filtered snapshot only
        // when the query or the saved profiles actually changed.
        let rows = {
            let query = self.connection_search.read(cx).value();
            let fingerprint = connection_fingerprint(&self.profiles);
            let mut cache = self.connection_rows_cache.borrow_mut();
            if cache.query != query.as_ref() || cache.fingerprint != fingerprint {
                cache.rows = connection_rows_snapshot(&self.profiles, &query);
                cache.query = query.to_string();
                cache.fingerprint = fingerprint;
            }
            cache.rows.clone()
        };
        let drag_entity = cx.entity();
        div()
            .flex()
            .flex_col()
            .flex_1()
            .h_full()
            .min_w_0()
            .min_h_0()
            .gap(px(theme::SPACE_PANEL))
            .child(
                // Window-level drag continuation: subtree listeners inside
                // modals consume moves before the workbench root, so the
                // manual-sort drag follows the scrollbar listener pattern.
                canvas(
                    |_, _, _| (),
                    move |_, _, window, _| {
                        let moving = drag_entity.clone();
                        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                            if phase != DispatchPhase::Bubble {
                                return;
                            }
                            let moving = moving.clone();
                            let _ = moving.update(cx, |this, cx| {
                                if this.connection_drag.is_none() {
                                    return;
                                }
                                if event.pressed_button == Some(MouseButton::Left) {
                                    this.move_connection_drag(event.position, cx);
                                } else {
                                    this.cancel_connection_drag(cx);
                                }
                            });
                        });
                        let releasing = drag_entity.clone();
                        window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                            if phase != DispatchPhase::Bubble || event.button != MouseButton::Left {
                                return;
                            }
                            let releasing = releasing.clone();
                            let _ = releasing.update(cx, |this, cx| {
                                this.finish_connection_drag(event.position, cx);
                            });
                        });
                    },
                )
                .absolute()
                .size_0(),
            )
            // Toolbar: search only; adding a connection lives at the footer's far right.
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(theme::SPACE_CONTROL))
                    // Toolbar and list share the same 12px side insets.
                    .px(px(theme::SPACE_PANEL))
                    .pt(px(theme::SPACE_PANEL))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .capture_key_down(cx.listener(
                                |this, event: &KeyDownEvent, window, cx| {
                                    if !matches!(event.keystroke.key.as_str(), "up" | "down")
                                        || event.keystroke.modifiers != Modifiers::default()
                                    {
                                        return;
                                    }
                                    let input = this.connection_search.clone();
                                    if input.update(cx, |input, cx| {
                                        EntityInputHandler::marked_text_range(input, window, cx)
                                            .is_some()
                                    }) {
                                        return;
                                    }
                                    this.step_connection(event.keystroke.key == "up", cx);
                                    cx.stop_propagation();
                                },
                            ))
                            .child(self.input_box(&self.connection_search)),
                    ),
            )
            // Result list with a persistent scrollbar overlay at its own right edge.
            // The uniform list paints only visible rows; overflow scrolls here.
            // The list keeps its 12px side inset, while the scrollbar fills the
            // wrapper's right edge without an extra one-pixel gap.
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .mx(px(theme::SPACE_PANEL))
                    .mb(px(theme::SPACE_PANEL))
                    .child(if rows.is_empty() {
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .child(div().p(px(theme::SPACE_PANEL)).child(self.t(
                                if self.profiles.is_empty() {
                                    "connection_empty"
                                } else {
                                    "connection_no_match"
                                },
                            )))
                            .into_any_element()
                    } else {
                        let entity = cx.entity();
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .mr(px(theme::SPACE_PANEL))
                            .child(
                                uniform_list(
                                    "connection-results",
                                    rows.len(),
                                    move |range, _window, cx| {
                                        let workbench = entity.read(cx);
                                        range
                                            .map(|ix| {
                                                let drag = workbench
                                                    .connection_drag
                                                    .as_ref()
                                                    .filter(|drag| drag.moved);
                                                connection_library_row(
                                                    &rows[ix],
                                                    ix,
                                                    drag.map(|drag| drag.slot),
                                                    drag.is_some_and(|drag| {
                                                        drag.row == rows[ix].id
                                                    }),
                                                    rows.len(),
                                                    workbench,
                                                    entity.clone(),
                                                    p,
                                                )
                                            })
                                            .collect::<Vec<_>>()
                                    },
                                )
                                .flex()
                                .flex_col()
                                .flex_1()
                                .min_w_0()
                                .min_h_0()
                                .track_scroll(self.connection_scroll.clone()),
                            )
                            .into_any_element()
                    })
                    .child(self.connection_scrollbar(cx)),
            )
            .into_any_element()
    }
    /// Point a scroll viewport at the offset under a window-space track
    /// position, keeping the thumb centered on the pointer while dragging.
    /// GPUI scroll offsets are negative when scrolled down (prepaint clamps
    /// them to [-max, 0]), so the drag target must be set negative; a positive
    /// value would be clamped back to the top every frame.
    pub(super) fn drag_scroll_handle(handle: &gpui::ScrollHandle, position: gpui::Point<Pixels>) {
        let bounds = handle.bounds();
        let height = f32::from(bounds.size.height);
        let max = f32::from(handle.max_offset().height);
        if max <= 0. || height <= 0. {
            return;
        }
        // Mirror the scrollbar geometry (2px track insets) so the thumb stays
        // under the pointer instead of jumping between two track lengths.
        let track = (height - 4.).max(1.);
        let thumb = (height / (height + max) * track).clamp(12f32.min(track), track);
        let local = f32::from(position.y - bounds.origin.y) - 2.;
        let ratio = ((local - thumb / 2.) / (track - thumb).max(1.)).clamp(0., 1.);
        // Preserve the horizontal position: viewports with both axes (the
        // file tree) must not snap back to x=0 on a vertical drag.
        handle.set_offset(point(handle.offset().x, px(-ratio * max)));
    }
    /// Horizontal counterpart of [`Self::drag_scroll_handle`]: map a window-space
    /// pointer onto the x offset, preserving the vertical position.
    pub(super) fn drag_scroll_handle_x(handle: &gpui::ScrollHandle, position: gpui::Point<Pixels>) {
        let bounds = handle.bounds();
        let width = f32::from(bounds.size.width);
        let max = f32::from(handle.max_offset().width);
        if max <= 0. || width <= 0. {
            return;
        }
        let track = (width - 4.).max(1.);
        let thumb = (width / (width + max) * track).clamp(12f32.min(track), track);
        let local = f32::from(position.x - bounds.origin.x) - 2.;
        let ratio = ((local - thumb / 2.) / (track - thumb).max(1.)).clamp(0., 1.);
        handle.set_offset(point(px(-ratio * max), handle.offset().y));
    }
    /// Always-visible position indicator for a vertical scroll viewport; drag
    /// the thumb or click the track to jump (shared by connection, file,
    /// history, process, port and modal lists).
    pub(super) fn overlay_scrollbar(
        &self,
        id: &'static str,
        handle: gpui::ScrollHandle,
        drag: Resize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = theme::Palette::new(self.prefs.theme);
        let viewport = f32::from(handle.bounds().size.height);
        let max = f32::from(handle.max_offset().height);
        // Sub-pixel/padding phantoms (a stray 1–2px range) hide below this
        // threshold; a real overflow is always far larger.
        if max <= 2. || viewport <= 0. {
            return div().into_any_element();
        }
        let track = (viewport - 4.).max(0.);
        // A tiny viewport can shrink the track below the 12px minimum; clamp
        // the lower bound so f32::clamp never sees min > max (it panics).
        let thumb = (viewport / (viewport + max) * track).clamp(12f32.min(track), track);
        // offset().y is negative while scrolled down; flip it into the 0..max
        // progress range before mapping onto the track.
        let position =
            (-f32::from(handle.offset().y) / max * (track - thumb)).clamp(0., track - thumb);
        // Some subtree listeners (e.g. inside modals) consume move events
        // before they can bubble to the workbench root, so the thumb drag
        // follows the window-level listener pattern (same as the
        // gpui-component scrollbar): an invisible canvas registers it during
        // paint for the current frame. The mouse-down still starts on the
        // strip itself with a center-jump onto the pointer.
        let entity = cx.entity();
        let canvas_drag = drag.clone();
        let canvas_handle = handle.clone();
        let measure_entity = entity.clone();
        let measure_id = id;
        let thumb_measure = entity.clone();
        div()
            .id(id)
            .absolute()
            .top(px(2.))
            .bottom(px(2.))
            .right(px(0.))
            .w(px(10.))
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    this.resize = Some(drag.clone());
                    Self::drag_scroll_handle(&handle, event.position);
                    cx.notify();
                }),
            )
            .child(
                canvas(
                    move |bounds, _, cx| match measure_id {
                        "connection-scrollbar" => measure_entity.update(cx, |this, _| {
                            if let Some(qa) = &mut this.qa {
                                qa.connection_scrollbar_bounds = Some(bounds);
                            }
                        }),
                        "process-list-scrollbar" => measure_entity.update(cx, |this, _| {
                            if let Some(qa) = &mut this.qa {
                                qa.process_geometry.insert("scrollbar", bounds);
                            }
                        }),
                        "port-list-scrollbar" => measure_entity.update(cx, |this, _| {
                            if let Some(qa) = &mut this.qa {
                                qa.port_geometry.insert("scrollbar", bounds);
                                qa.port_revision += 1;
                            }
                        }),
                        _ => {}
                    },
                    move |_, _, window, _| {
                        let canvas_drag = canvas_drag.clone();
                        let canvas_handle = canvas_handle.clone();
                        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                            if phase != DispatchPhase::Bubble {
                                return;
                            }
                            let entity = entity.clone();
                            let dragging = entity
                                .update(cx, |this, _| this.resize.as_ref() == Some(&canvas_drag));
                            if !dragging {
                                return;
                            }
                            if event.pressed_button != Some(MouseButton::Left) {
                                entity.update(cx, |this, cx| {
                                    this.resize = None;
                                    cx.notify();
                                });
                                return;
                            }
                            entity.update(cx, |_, cx| {
                                Self::drag_scroll_handle(&canvas_handle, event.position);
                                cx.notify();
                            });
                        });
                    },
                )
                .absolute()
                .size_full(),
            )
            .child(
                div()
                    .absolute()
                    .top(px(2. + position))
                    .left(px(2.))
                    .w(px(6.))
                    .h(px(thumb))
                    .rounded(px(3.))
                    .bg(p.muted.opacity(0.55))
                    .hover(|style| style.bg(p.muted))
                    .child(
                        canvas(
                            move |bounds, _, cx| {
                                if measure_id == "connection-scrollbar" {
                                    thumb_measure.update(cx, |this, _| {
                                        if let Some(qa) = &mut this.qa {
                                            qa.connection_thumb_bounds = Some(bounds);
                                        }
                                    });
                                }
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .size_full(),
                    ),
            )
            .into_any_element()
    }
    /// Always-visible position indicator for the connection list; drag to scroll.
    fn connection_scrollbar(&self, cx: &mut Context<Self>) -> AnyElement {
        let base = self.connection_scroll.0.borrow().base_handle.clone();
        self.overlay_scrollbar("connection-scrollbar", base, Resize::ConnectionScroll, cx)
    }
    /// Horizontal counterpart of [`Self::overlay_scrollbar`]: an always-visible
    /// bottom strip for viewports that overflow sideways (the file tree),
    /// with the same thumb drag and track-jump interactions.
    pub(super) fn overlay_scrollbar_x(
        &self,
        id: &'static str,
        handle: gpui::ScrollHandle,
        drag: Resize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = theme::Palette::new(self.prefs.theme);
        let viewport = f32::from(handle.bounds().size.width);
        let max = f32::from(handle.max_offset().width);
        // Sub-pixel/padding phantoms (a stray 1–2px range) hide below this
        // threshold; a real overflow is always far larger.
        if max <= 2. || viewport <= 0. {
            return div().into_any_element();
        }
        let track = (viewport - 4.).max(0.);
        // A tiny viewport can shrink the track below the 12px minimum; clamp
        // the lower bound so f32::clamp never sees min > max (it panics).
        let thumb = (viewport / (viewport + max) * track).clamp(12f32.min(track), track);
        // offset().x is negative while scrolled right; flip into 0..max first.
        let position =
            (-f32::from(handle.offset().x) / max * (track - thumb)).clamp(0., track - thumb);
        let entity = cx.entity();
        let canvas_drag = drag.clone();
        let canvas_handle = handle.clone();
        div()
            .id(id)
            .absolute()
            .bottom(px(1.))
            .left(px(2.))
            .right(px(12.))
            .h(px(10.))
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    this.resize = Some(drag.clone());
                    Self::drag_scroll_handle_x(&handle, event.position);
                    cx.notify();
                }),
            )
            .child(
                canvas(
                    |_, _, _| (),
                    move |_, _, window, _| {
                        let canvas_drag = canvas_drag.clone();
                        let canvas_handle = canvas_handle.clone();
                        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                            if phase != DispatchPhase::Bubble {
                                return;
                            }
                            let entity = entity.clone();
                            let dragging = entity
                                .update(cx, |this, _| this.resize.as_ref() == Some(&canvas_drag));
                            if !dragging {
                                return;
                            }
                            if event.pressed_button != Some(MouseButton::Left) {
                                entity.update(cx, |this, cx| {
                                    this.resize = None;
                                    cx.notify();
                                });
                                return;
                            }
                            entity.update(cx, |_, cx| {
                                Self::drag_scroll_handle_x(&canvas_handle, event.position);
                                cx.notify();
                            });
                        });
                    },
                )
                .absolute()
                .size_0(),
            )
            .child(
                div()
                    .absolute()
                    .left(px(2. + position))
                    .top(px(2.))
                    .h(px(6.))
                    .w(px(thumb))
                    .rounded(px(3.))
                    .bg(p.muted.opacity(0.55))
                    .hover(|style| style.bg(p.muted)),
            )
            .into_any_element()
    }
    /// Keep library-wide actions in the fixed dialog footer, outside the result scrollbar.
    pub(super) fn connection_library_footer(&self, cx: &mut Context<Self>) -> Div {
        let multi_count = self.connection_multi.len();
        let add_measure = cx.entity();
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(theme::SPACE_CONTROL))
            // Batch delete only exists while rows are selected; it confirms exact targets.
            .when(multi_count > 0, |bar| {
                bar.child(
                    self.button(
                        "library-batch-delete",
                        format!("{} ({})", self.t("delete"), multi_count),
                    )
                    .svg_icon("icons/trash-2.svg")
                    .danger()
                    .on_click(cx.listener(|this, _, w, cx| {
                        let profiles: Vec<Profile> = this
                            .profiles
                            .iter()
                            .filter(|profile| this.connection_multi.contains(&profile.id))
                            .cloned()
                            .collect();
                        if profiles.is_empty() {
                            return;
                        }
                        this.show_modal(Modal::DeleteProfiles { profiles }, w, cx);
                    })),
                )
            })
            // Clear every selection state: batch set, range anchor, and highlighted row.
            .when(multi_count > 0, |bar| {
                bar.child(
                    self.button("library-clear-selection", self.t("clear_selection"))
                        .icon(IconName::Undo)
                        .ghost()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.connection_multi.clear();
                            this.connection_anchor = None;
                            this.connection_selected = None;
                            cx.notify();
                        })),
                )
            })
            // Row actions live on each row's own edit/clone/connect buttons; the
            // footer keeps only library-wide actions.
            .child(div().flex_1())
            .child(
                self.button("library-import", self.t("import"))
                    .svg_icon("icons/download.svg")
                    .ghost()
                    .on_click(cx.listener(|this, _, w, cx| this.import_file(w, cx))),
            )
            .child(
                self.button("library-export", self.t("export"))
                    .svg_icon("icons/upload.svg")
                    .ghost()
                    .on_click(cx.listener(|this, _, w, cx| this.export_file(w, cx))),
            )
            .child(
                div()
                    .relative()
                    .child(
                        self.button("library-new", self.t("add"))
                            .icon(IconName::Plus)
                            .on_click(
                                cx.listener(|this, _, w, cx| this.profile_form(None, None, w, cx)),
                            ),
                    )
                    .when(cfg!(debug_assertions) && self.qa.is_some(), move |button| {
                        let measure = add_measure.clone();
                        button.child(
                            canvas(
                                move |bounds, _, cx| {
                                    measure.update(cx, |this, _| {
                                        if let Some(qa) = &mut this.qa {
                                            qa.connection_add_bounds = Some(bounds);
                                        }
                                    });
                                },
                                |_, _, _, _| {},
                            )
                            .absolute()
                            .size_full(),
                        )
                    }),
            )
    }
    fn render_terminal_toolbar(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = theme::Palette::new(self.prefs.theme);
        let mut header = div()
            .flex()
            .items_center()
            .flex_wrap()
            .gap(px(8.))
            .px(px(8.))
            .py(px(4.))
            .min_h(px((self.prefs.ui_size * 1.45 + 14.).max(36.)))
            .border_b_1()
            .border_color(p.border)
            .bg(p.surface);
        if let Some(pane) = self.active_pane() {
            let owner = pane.owner;
            let local = matches!(pane.spec, SessionSpec::Local { .. });
            let identity = if let SessionSpec::Ssh { profile, .. } = &pane.spec {
                profile.address()
            } else {
                "localhost".into()
            };
            // The session state control replaces the old status dot at the far
            // left. One link-2 glyph for all three states, colored by state:
            // green connected, blue connecting, red disconnected/failed.
            let state_control = (!local).then(|| match pane.state {
                ConnectionState::Connected => self
                    .button(("disconnect-session", owner.session.as_u128() as u64), "")
                    .svg_icon("icons/link-2.svg")
                    .ghost()
                    .icon_color(p.meter_green)
                    .tooltip(self.t("disconnect"))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.backend.close(owner);
                        if let Some(pane) = this.pane_mut(owner) {
                            pane.state = ConnectionState::Disconnected;
                            pane.files.loading = false;
                            pane.files.request = None;
                            pane.monitor_request = None;
                            pane.pending_terminal = None;
                            if let Some(terminal) = &pane.terminal {
                                terminal.update(cx, |view, cx| {
                                    view.connected = false;
                                    cx.notify();
                                });
                            }
                        }
                        cx.notify();
                    }))
                    .into_any_element(),
                ConnectionState::Connecting
                | ConnectionState::Authenticating
                | ConnectionState::HostVerification
                | ConnectionState::CredentialsRequired => self
                    .button(("session-cancel", owner.session.as_u128() as u64), "")
                    .svg_icon("icons/link-2.svg")
                    .ghost()
                    .icon_color(p.accent)
                    .tooltip(self.t("cancel"))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.backend.close(owner);
                        if let Some(pane) = this.pane_mut(owner) {
                            pane.state = ConnectionState::Cancelled;
                            pane.pending_terminal = None;
                        }
                        cx.notify();
                    }))
                    .into_any_element(),
                _ => self
                    .button(("session-reconnect", owner.session.as_u128() as u64), "")
                    .svg_icon("icons/link-2.svg")
                    .ghost()
                    .icon_color(p.error)
                    .tooltip(self.t("reconnect"))
                    .on_click(cx.listener(move |this, _, w, cx| {
                        this.reconnect(owner, String::new(), false, w, cx)
                    }))
                    .into_any_element(),
            });
            header = header
                .when_some(state_control, |h, control| h.child(control))
                // The spacer keeps the controls right-aligned; only SSH
                // panes carry the address label, the local toolbar starts
                // directly with its buttons.
                .child(div().min_w_0().flex_1().when(!local, |d| d.child(identity)));
            header = header
                // History is SSH-only; local pages carry no history entry.
                // SSH sessions carry their own terminal encoding, set per session.
                .when(!local, |h| {
                    h.child(
                        // The button label is the live encoding value, not an icon.
                        self.button("ssh-encoding", pane.spec.encoding().label())
                            .ghost()
                            .tooltip(self.t("encoding"))
                            .on_click(cx.listener(move |this, _, w, cx| {
                                this.show_modal(
                                    Modal::Encoding {
                                        owner: Some(owner),
                                        document: None,
                                    },
                                    w,
                                    cx,
                                )
                            })),
                    )
                })
                // Session editing stays on the right of the toolbar.
                .when(!local, |h| {
                    if let SessionSpec::Ssh { profile, .. } = &pane.spec {
                        let profile = profile.clone();
                        h.child(
                            self.icon_button(
                                ("session-edit", owner.session.as_u128() as u64),
                                "edit",
                                IconName::Settings2,
                            )
                            .on_click(cx.listener(
                                move |this, _, w, cx| {
                                    this.profile_form(Some(profile.clone()), None, w, cx)
                                },
                            )),
                        )
                    } else {
                        h
                    }
                })
                .when(!local, |h| {
                    h.child(
                        self.button("ssh-history", "")
                            .svg_icon("icons/history.svg")
                            .ghost()
                            .tooltip(self.t("history"))
                            .on_click(cx.listener(|this, _, w, cx| {
                                this.show_modal(Modal::LocalHistory, w, cx);
                            })),
                    )
                    .child(
                        self.button("ssh-files", "")
                            .svg_icon("icons/folder-tree.svg")
                            .ghost()
                            .selected(pane.files_open)
                            .tooltip(self.t("files"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.toggle_files_panel(cx);
                            })),
                    )
                    .child(
                        self.button("ssh-system", "")
                            .svg_icon("icons/hard-drive.svg")
                            .ghost()
                            .selected(pane.tool.is_some())
                            .tooltip(self.t("system"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                if this.active_tool().is_some() {
                                    this.set_active_tool(None, cx);
                                } else {
                                    this.set_active_tool(Some(Tool::System), cx);
                                }
                                this.changed(cx);
                            })),
                    )
                });
        }
        header.into_any_element()
    }
    fn render_pane(&self, pane: &Pane, active: bool, cx: &mut Context<Self>) -> AnyElement {
        let p = theme::Palette::new(self.prefs.theme);
        let owner = pane.owner;
        let multiple = self
            .tabs
            .get(self.active)
            .is_some_and(|t| t.panes.len() > 1);
        let mut center = div().flex().flex_col().flex_1().min_h_0().min_w_0();
        if pane.encoding_warning {
            center = center.child(
                div()
                    .px_2()
                    .py_1()
                    .text_color(p.error)
                    .child(self.t("encoding_warning")),
            );
        }
        if let Some(terminal) = &pane.terminal {
            // The scrollbar hangs on the padded wrapper (absolute children
            // span the padding box), so it hugs the pane edge like every
            // other overlay bar instead of floating 8px in with the text.
            center = center.child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .p(px(8.))
                    .child(terminal.clone())
                    .when_some(
                        terminal.read(cx).scroll_metrics(),
                        |host, (position, thumb, track)| {
                            let _ = track;
                            host.child(
                                div()
                                    .id(("terminal-scrollbar", owner.session.as_u128() as u64))
                                    .absolute()
                                    .top(px(2.))
                                    .bottom(px(2.))
                                    .right(px(1.))
                                    .w(px(10.))
                                    .flex_shrink_0()
                                    .occlude()
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, event: &MouseDownEvent, w, cx| {
                                            this.jump_terminal_scroll(
                                                owner,
                                                event.position.y,
                                                w,
                                                cx,
                                            );
                                            cx.notify();
                                        }),
                                    )
                                    .on_mouse_move(cx.listener(
                                        move |this, event: &MouseMoveEvent, w, cx| {
                                            if event.pressed_button == Some(MouseButton::Left) {
                                                this.jump_terminal_scroll(
                                                    owner,
                                                    event.position.y,
                                                    w,
                                                    cx,
                                                );
                                                cx.notify();
                                            }
                                        },
                                    ))
                                    .child(
                                        div()
                                            .absolute()
                                            .top(px(2. + position))
                                            .left(px(2.))
                                            .w(px(6.))
                                            .h(px(thumb))
                                            .rounded(px(3.))
                                            .bg(p.muted.opacity(0.55))
                                            .hover(|style| style.bg(p.muted)),
                                    ),
                            )
                        },
                    ),
            );
        } else {
            center = center.child(div().flex_1().bg(p.terminal));
        }
        let label = self.pane_label(pane);
        div()
            .size_full()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(p.terminal)
            .when(multiple, |d| {
                d.border_1()
                    .border_color(if active { p.accent } else { p.border })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            // Pane strip: 2px vertical and 4px horizontal
                            // insets around a 20px close button (one tier
                            // below the standard 24px controls).
                            .py(px(2.))
                            .px(px(4.))
                            .bg(if active { p.selected } else { p.surface })
                            // The pane title is plain text on the left; only the
                            // close button on the right is interactive.
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .h(px(20.))
                                    .flex()
                                    .items_center()
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .whitespace_nowrap()
                                    .child(label),
                            )
                            .child(
                                self.button(("close-pane", owner.session.as_u128() as u64), "")
                                    .icon(IconName::Close)
                                    .ghost()
                                    .h(px(20.))
                                    .w(px(20.))
                                    .on_click(cx.listener(move |this, _, w, cx| {
                                        this.request_close(CloseTarget::Pane(owner), w, cx)
                                    })),
                            ),
                    )
            })
            .child(center)
            .into_any_element()
    }
    /// Forward a scrollbar drag/click to the pane's terminal view.
    fn jump_terminal_scroll(
        &mut self,
        owner: Owner,
        y: gpui::Pixels,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pane) = self.pane(owner) else {
            return;
        };
        let Some(terminal) = pane.terminal.clone() else {
            return;
        };
        terminal.update(cx, |view, cx| view.scrollbar_jump(y, cx));
        let _ = window;
    }
    /// Editor dialog body for one session's documents; keyed by owner so the
    /// dialog keeps its pane even when the active tab changes behind it.
    /// Editor dialog body: only the editing surface itself. Document
    /// metadata (name, unsaved state, errors, disconnects) lives in the
    /// footer; switching documents happens by reopening from the file list.
    pub(super) fn render_editor(&self, owner: Owner, _cx: &mut Context<Self>) -> AnyElement {
        let Some(pane) = self.pane(owner) else {
            return div().into_any_element();
        };
        let Some(doc) = pane
            .active_document
            .and_then(|id| pane.documents.iter().find(|d| d.id == id))
        else {
            let p = theme::Palette::new(self.prefs.theme);
            return div()
                .p_3()
                .text_color(p.muted)
                .child(self.t("open"))
                .into_any_element();
        };
        let id = doc.id;
        div()
            .id(("editor", id.as_u128() as u64))
            .key_context("MantaSHEditor")
            .flex_1()
            .min_h_0()
            .font_family(self.prefs.terminal_font.clone())
            .text_size(px(self.prefs.terminal_size))
            .child(
                Input::new(&doc.input)
                    .h_full()
                    .appearance(false)
                    .bordered(false),
            )
            .into_any_element()
    }
    /// Editor dialog footer: the unsaved marker and any save error or
    /// disconnect notice on the left (the full path lives in the header),
    /// then encoding and the primary save right-aligned. Closing stays on
    /// the header's dedicated button, so the footer carries no close action.
    pub(super) fn editor_footer(&self, owner: Owner, cx: &mut Context<Self>) -> Div {
        let p = theme::Palette::new(self.prefs.theme);
        let mut footer = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(theme::SPACE_CONTROL));
        let Some(pane) = self.pane(owner) else {
            return footer;
        };
        let Some(doc) = pane
            .active_document
            .and_then(|id| pane.documents.iter().find(|d| d.id == id))
        else {
            return footer;
        };
        let id = doc.id;
        let doc_owner = doc.owner;
        // The header carries the full path, so the footer omits the file
        // name; the dirty marker and save/disconnect feedback stay visible.
        let status = doc.error.clone().or_else(|| {
            if doc.owner != pane.owner || pane.state != ConnectionState::Connected {
                Some(self.t("editor_disconnected").to_string())
            } else {
                None
            }
        });
        let has_status = status.is_some();
        footer = footer
            .when(doc.dirty, |row| {
                row.child(
                    div()
                        .whitespace_nowrap()
                        .text_color(p.muted)
                        .child(self.t("unsaved")),
                )
            })
            .when_some(status, |row, status| {
                row.child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .text_color(p.error)
                        .child(status),
                )
            })
            .when(!has_status, |row| row.child(div().flex_1()))
            .child(
                self.button(("doc-encoding", id.as_u128() as u64), doc.encoding.label())
                    .on_click(cx.listener(move |this, _, w, cx| {
                        this.show_modal(
                            Modal::Encoding {
                                owner: Some(doc_owner),
                                document: Some(id),
                            },
                            w,
                            cx,
                        )
                    })),
            )
            .child(
                self.button(
                    ("save-document", id.as_u128() as u64),
                    self.t(if doc.saving { "saving" } else { "save" }),
                )
                .primary()
                .disabled(
                    doc.saving
                        || doc.owner != pane.owner
                        || pane.state != ConnectionState::Connected,
                )
                .on_click(
                    cx.listener(move |this, _, _, cx| this.save_document(doc_owner, id, false, cx)),
                ),
            );
        footer
    }
    fn layout_minimum(&self, node: &PaneLayout) -> Size<f32> {
        match node {
            PaneLayout::Pane { .. } => size(
                self.prefs.terminal_size * 20. + 40.,
                self.prefs.terminal_size * 1.75 * 3. + self.prefs.ui_size * 1.45 + 40.,
            ),
            PaneLayout::Split {
                axis,
                first,
                second,
                ..
            } => {
                let a = self.layout_minimum(first);
                let b = self.layout_minimum(second);
                if *axis == Split::Horizontal {
                    size(a.width + b.width + 6., a.height.max(b.height))
                } else {
                    size(a.width.max(b.width), a.height + b.height + 6.)
                }
            }
        }
    }
    fn render_layout(
        &self,
        tab: &Tab,
        node: &PaneLayout,
        area: Size<f32>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match node {
            PaneLayout::Pane { pane } => {
                let content = tab
                    .panes
                    .iter()
                    .find(|p| p.owner.session == *pane)
                    .map(|p| self.render_pane(p, tab.panes[tab.active].owner == p.owner, cx));
                div()
                    .w(px(area.width))
                    .h(px(area.height))
                    .flex_shrink_0()
                    .when_some(content, |d, e| d.child(e))
                    .into_any_element()
            }
            PaneLayout::Split {
                id,
                axis,
                ratio,
                first,
                second,
            } => {
                let vertical = *axis == Split::Vertical;
                let a = self.layout_minimum(first);
                let b = self.layout_minimum(second);
                let extent = if vertical { area.height } else { area.width };
                let first_min = if vertical { a.height } else { a.width };
                let second_min = if vertical { b.height } else { b.width };
                let first_size = ((extent - 6.) * ratio)
                    .clamp(first_min, (extent - 6. - second_min).max(first_min));
                let second_size = (extent - 6. - first_size).max(second_min);
                let first_area = if vertical {
                    size(area.width, first_size)
                } else {
                    size(first_size, area.height)
                };
                let second_area = if vertical {
                    size(area.width, second_size)
                } else {
                    size(second_size, area.height)
                };
                let node_id = *id;
                let tab_id = tab.id;
                let axis = *axis;
                let measure = cx.entity();
                let focus = self.split_focus.get(id).cloned();
                // Keep a 6px drag target; the visible line stays 1px so the seam
                // between panes reads thin at every font scale.
                let divider = div()
                    .id(("split-divider", id.as_u128() as u64))
                    .flex_shrink_0()
                    .when(vertical, |d| {
                        d.h(px(6.))
                            .w_full()
                            .cursor_row_resize()
                            .flex()
                            .items_center()
                    })
                    .when(!vertical, |d| {
                        d.w(px(6.))
                            .h_full()
                            .cursor_col_resize()
                            .flex()
                            .justify_center()
                    })
                    .child(if vertical {
                        div()
                            .h(px(1.))
                            .w_full()
                            .bg(theme::Palette::new(self.prefs.theme).border)
                    } else {
                        div()
                            .w(px(1.))
                            .h_full()
                            .bg(theme::Palette::new(self.prefs.theme).border)
                    })
                    .when_some(focus.clone(), |d, f| d.track_focus(&f))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, w, _| {
                            if let Some(f) = &focus {
                                f.focus(w);
                            }
                            this.resize = Some(Resize::Split(tab_id, node_id, axis));
                        }),
                    )
                    .on_key_down(cx.listener(move |this, e: &KeyDownEvent, _, cx| {
                        let delta = match e.keystroke.key.as_str() {
                            "left" | "up" => -0.05,
                            "right" | "down" => 0.05,
                            "home" => -1.,
                            "end" => 1.,
                            _ => return,
                        };
                        if let Some(tab) = this.tabs.iter_mut().find(|t| t.id == tab_id) {
                            let value = layout_ratio(&tab.layout, node_id).unwrap_or(0.5) + delta;
                            tab.layout.set_ratio(node_id, value);
                        }
                        this.changed(cx);
                    }));
                div()
                    .w(px(area.width))
                    .h(px(area.height))
                    .flex()
                    .when(vertical, |d| d.flex_col())
                    .relative()
                    .child(
                        canvas(
                            move |bounds, _, cx| {
                                measure.update(cx, |this, _| {
                                    this.split_bounds.insert(node_id, bounds);
                                });
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .size_full(),
                    )
                    .child(self.render_layout(tab, first, first_area, cx))
                    .child(divider)
                    .child(self.render_layout(tab, second, second_area, cx))
                    .into_any_element()
            }
        }
    }
    /// Bottom files panel for SSH panes; wraps render_files with a close affordance.
    fn render_files_panel(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let p = theme::Palette::new(self.prefs.theme);
        let tree_owner = self.active_owner();
        let tree_scroll = self
            .active_pane()
            .map(|pd| pd.files.tree_scroll.clone())
            .unwrap_or_default();
        let tree_rows = self
            .active_pane()
            .map(|pane| pane.files.tree_rows.clone())
            .filter(|rows| !rows.is_empty())
            .unwrap_or_else(|| Rc::new(vec![(String::from("/"), 0)]));
        let tree_width = self.tree_width(window);
        let tree_content_width = self
            .active_pane()
            .map(|pane| pane.files.tree_content_w.max(tree_width))
            .unwrap_or(tree_width);
        let tree_scroll_base = tree_scroll.0.borrow().base_handle.clone();
        let _ = window;
        div()
            .size_full()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(p.surface)
            .child(self.render_files_toolbar(window, cx))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    // Left: directory tree. The right border lives on a
                    // non-scrolling shell: gpui derives the scroll range from
                    // the child bounding box minus the container's border box,
                    // so a border (or padding, see the py note below) on the
                    // scrolling element itself leaks into the computed range
                    // and shows a phantom 1px horizontal scrollbar even when
                    // nothing overflows.
                    .child(
                        div()
                            .relative()
                            .flex()
                            .flex_col()
                            .w(px(tree_width))
                            .flex_shrink_0()
                            .min_h_0()
                            .border_r_1()
                            .border_color(p.border)
                            .child({
                                let entity = cx.entity();
                                let rows = tree_rows.clone();
                                uniform_list("files-tree", rows.len(), move |range, _, app| {
                                    let Some(owner) = tree_owner else {
                                        return Vec::new();
                                    };
                                    let workbench = entity.read(app);
                                    range
                                        .map(|index| {
                                            let (path, depth) = &rows[index];
                                            workbench.render_tree_row(
                                                owner,
                                                path,
                                                *depth,
                                                tree_content_width,
                                                entity.clone(),
                                            )
                                        })
                                        .collect::<Vec<_>>()
                                })
                                .flex()
                                .flex_col()
                                .flex_1()
                                .min_w_0()
                                .min_h_0()
                                .with_horizontal_sizing_behavior(
                                    gpui::ListHorizontalSizingBehavior::Unconstrained,
                                )
                                .track_scroll(tree_scroll.clone())
                            })
                            // The persistent horizontal indicator hangs on the
                            // shell like the vertical one; the built-in
                            // auto-hiding strip was neither visible nor
                            // draggable enough for deep trees.
                            .when_some(tree_owner, |shell, owner| {
                                shell.child(self.overlay_scrollbar_x(
                                    "files-tree-scrollbar-x",
                                    tree_scroll_base.clone(),
                                    Resize::TreeScrollX(owner),
                                    cx,
                                ))
                            })
                            // The persistent vertical indicator hangs on the
                            // non-scrolling shell: a child of the scroll
                            // container scrolls away with the tree.
                            .when_some(tree_owner, |shell, owner| {
                                shell.child(self.overlay_scrollbar(
                                    "files-tree-scrollbar",
                                    tree_scroll_base.clone(),
                                    Resize::TreeScroll(owner),
                                    cx,
                                ))
                            })
                            // Reveal the current directory through the
                            // virtualized list's indexed scroll API. Unlike
                            // paint-time row coordinates, this also works when
                            // the target row is currently off-screen.
                            .when_some(tree_owner, |shell, owner| {
                                let entity = cx.entity();
                                let scroll = tree_scroll.clone();
                                let rows = tree_rows.clone();
                                shell.child(
                                    canvas(
                                        |_, _, _| {},
                                        move |_, _, _, cx| {
                                            entity.update(cx, |this, cx| {
                                                let target = this.tree_reveal.borrow_mut().take();
                                                let Some((target_owner, path)) = target else {
                                                    return;
                                                };
                                                if target_owner != owner {
                                                    // Not this tree; keep it queued.
                                                    this.tree_reveal
                                                        .borrow_mut()
                                                        .replace((target_owner, path));
                                                    return;
                                                }
                                                let Some(index) = rows
                                                    .iter()
                                                    .position(|(candidate, _)| candidate == &path)
                                                else {
                                                    this.tree_reveal
                                                        .borrow_mut()
                                                        .replace((target_owner, path));
                                                    return;
                                                };
                                                scroll
                                                    .scroll_to_item(index, ScrollStrategy::Center);
                                                cx.notify();
                                            });
                                        },
                                    )
                                    .absolute()
                                    .size_0(),
                                )
                            }),
                    )
                    // Tree width resize handle
                    .child(
                        div()
                            .id("tree-width-resize")
                            .w(px(4.))
                            .flex_shrink_0()
                            .h_full()
                            .cursor_col_resize()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _, _, _cx| {
                                    this.resize = Some(Resize::TreeWidth);
                                }),
                            ),
                    )
                    // Right: the file list (existing content). The column
                    // direction keeps the vertical chain on the main axis so
                    // the list viewport receives a definite height (nested
                    // cross-stretch left it content-sized and unscrollable).
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .overflow_hidden()
                            .child(self.render_files(cx)),
                    ),
            )
    }
    fn render_center(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(tab) = self.tabs.get(self.active) else {
            return div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .gap(px(8.))
                .child(
                    self.button("start-local", self.t("new_local"))
                        .primary()
                        .on_click(cx.listener(|this, _, w, cx| this.new_local(w, cx))),
                )
                .child(
                    self.button("start-ssh", self.t("connections"))
                        .on_click(cx.listener(|this, _, w, cx| this.toggle_sidebar(w, cx))),
                )
                .into_any_element();
        };
        // Local pages have no terminal toolbar — their split controls live in
        // the title bar — so the layout keeps the full height.
        let local_tab = self
            .active_pane()
            .is_some_and(|p| matches!(p.spec, SessionSpec::Local { .. }));
        let toolbar_height = if local_tab {
            0.
        } else {
            (self.prefs.ui_size * 1.45 + 14.).max(36.)
        };
        let min = self.layout_minimum(&tab.layout);
        let area = size(
            f32::from(self.body_bounds.size.width).max(min.width),
            (f32::from(self.body_bounds.size.height) - toolbar_height).max(min.height),
        );
        div()
            .size_full()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .when(!local_tab, |d| d.child(self.render_terminal_toolbar(cx)))
            .child(
                div()
                    .id(("terminal-viewport", tab.id.as_u128() as u64))
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .overflow_scroll()
                    .track_scroll(&tab.scroll)
                    .child(self.render_layout(tab, &tab.layout, area, cx)),
            )
            .into_any_element()
    }
}
fn layout_ratio(node: &PaneLayout, id: Id) -> Option<f32> {
    match node {
        PaneLayout::Pane { .. } => None,
        PaneLayout::Split {
            id: node_id,
            ratio,
            first,
            second,
            ..
        } => {
            if *node_id == id {
                Some(*ratio)
            } else {
                layout_ratio(first, id).or_else(|| layout_ratio(second, id))
            }
        }
    }
}

impl Render for Workbench {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(target_os = "macos")]
        self.tab_strip.control_bounds.borrow_mut().clear();
        window.set_rem_size(px(self.prefs.ui_size / 0.875));
        let p = theme::Palette::new(self.prefs.theme);
        let measure = cx.entity();
        let remote = self
            .active_pane()
            .is_some_and(|pane| matches!(pane.spec, SessionSpec::Ssh { .. }));
        let files_open = remote && self.active_pane().is_some_and(|pane| pane.files_open);
        let mut body = div().flex().flex_1().min_w_0().min_h_0().child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .min_h_0()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .min_h_0()
                        .relative()
                        .child(
                            canvas(
                                move |bounds, _, cx| {
                                    measure.update(cx, |this, cx| {
                                        if this.body_bounds != bounds {
                                            this.body_bounds = bounds;
                                            cx.notify();
                                        }
                                    });
                                },
                                |_, _, _, _| {},
                            )
                            .absolute()
                            .size_full(),
                        )
                        .child(self.render_center(cx)),
                )
                .when(files_open, |col| {
                    col.child(
                        div()
                            .h(px(self.files_height(window)))
                            .flex_shrink_0()
                            .min_h_0()
                            .flex()
                            .flex_col()
                            .child(
                                // Drag zone doubling as the divider: its top
                                // border is the 1px seam line, flush with the
                                // terminal above, and its surface background
                                // continues seamlessly into the toolbar below
                                // (the old centred line inside a 6px handle
                                // left visible gaps on both sides). Keeping the
                                // border on the strip makes presses on the line
                                // itself start the drag.
                                div()
                                    .id("files-height-resize")
                                    // 1px seam border + 4px surface: the surface
                                    // doubles as the toolbar's visual top inset,
                                    // so the toolbar itself needs no extra pt.
                                    .h(px(5.))
                                    .flex_shrink_0()
                                    .border_t_1()
                                    .border_color(p.border)
                                    .bg(p.surface)
                                    .cursor_row_resize()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, _, _, _| {
                                            this.resize = Some(Resize::FilesHeight);
                                        }),
                                    ),
                            )
                            .child(self.render_files_panel(window, cx)),
                    )
                }),
        );
        if self.active_tool().is_some() {
            let focus = self.tool_resize_focus.clone();
            let tool_width = self.tool_width(window);
            body = body
                .relative()
                .child(
                    div()
                        .w(px(tool_width))
                        .flex_shrink_0()
                        .min_h_0()
                        // The panel's own left border is the divider: the 1px
                        // line sits exactly on the seam, flush with the terminal
                        // on the left and the panel background on the right
                        // (the old centred line inside a 6px handle left visible
                        // gaps on both sides).
                        .border_l_1()
                        .border_color(p.border)
                        // One uniform background for the whole tool panel content.
                        .bg(p.surface)
                        .child(self.render_tool(window, cx)),
                )
                .child(
                    // Transparent drag zone centred on the divider; it paints
                    // nothing itself, so no gap shows on either side of the
                    // border line. It keeps the drag, the double-click reset
                    // and the keyboard resize of the old handle.
                    div()
                        .id("tool-resize")
                        .absolute()
                        .right(px((tool_width - 3.).max(0.)))
                        .w(px(6.))
                        .h_full()
                        .cursor_col_resize()
                        .track_focus(&self.tool_resize_focus)
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, e: &MouseDownEvent, w, cx| {
                                focus.focus(w);
                                if e.click_count == 2 {
                                    this.prefs.tool_preferred_width = None;
                                    this.changed(cx);
                                } else {
                                    this.resize = Some(Resize::Tools);
                                }
                            }),
                        )
                        .on_key_down(cx.listener(|this, e: &KeyDownEvent, w, cx| {
                            let current = this.tool_width(w);
                            let max = f32::from(w.viewport_size().width) / 2.;
                            let step = if e.keystroke.modifiers.shift { 32. } else { 8. };
                            let value = match e.keystroke.key.as_str() {
                                "left" => current + step,
                                "right" => current - step,
                                // Home returns to the same minimum-width default
                                // used when no sidebar preference is persisted.
                                "home" => crate::layout::tool_width(
                                    None,
                                    w.viewport_size().width.into(),
                                    w.viewport_size().width.into(),
                                ),
                                "end" => max,
                                _ => return,
                            };
                            this.prefs.tool_preferred_width = Some(crate::layout::tool_width(
                                Some(value),
                                w.viewport_size().width.into(),
                                w.viewport_size().width.into(),
                            ));
                            this.changed(cx);
                        })),
                );
        }
        // Keep tab widths out of the toolbar's intrinsic minimum size.
        // Windows keeps a client-drawn caption with explicitly queued native operations.
        let logo = div().flex().items_center().flex_shrink_0().px_2().child(
            gpui::svg()
                .path("mantash-mark.svg")
                .size(px(self.prefs.ui_size + 8.))
                .text_color(p.accent),
        );
        let logo = self.header_control("app-mark", logo, cx);
        let header = div()
            .flex()
            .absolute()
            .inset_0()
            .min_w_0()
            .items_center()
            .child(logo)
            .child(self.render_tabs(window, cx))
            .id("workbench-titlebar-header")
            .when(cfg!(target_os = "windows"), |header| {
                header
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, _, _| {
                            this.tab_strip.caption_drag = if this.modal.is_none() {
                                Some(event.position)
                            } else {
                                None
                            };
                        }),
                    )
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _, _, _| this.tab_strip.caption_drag = None),
                    )
                    .on_mouse_up_out(
                        MouseButton::Left,
                        cx.listener(|this, _, _, _| this.tab_strip.caption_drag = None),
                    )
                    .on_mouse_down_out(
                        cx.listener(|this, _, _, _| this.tab_strip.caption_drag = None),
                    )
                    .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, window, cx| {
                        if event.pressed_button != Some(MouseButton::Left)
                            || !window.is_window_active()
                            || this.modal.is_some()
                        {
                            this.tab_strip.caption_drag = None;
                            return;
                        }
                        if this.tab_strip.caption_drag.is_some_and(|start| {
                            (event.position.x - start.x).abs() >= px(4.)
                                || (event.position.y - start.y).abs() >= px(4.)
                        }) {
                            this.tab_strip.caption_drag = None;
                            this.caption_command(window, windows_caption::CaptionCommand::Move, cx);
                        }
                    }))
                    .on_double_click(cx.listener(|this, _, window, cx| {
                        this.tab_strip.caption_drag = None;
                        this.caption_command(
                            window,
                            windows_caption::CaptionCommand::ToggleMaximize,
                            cx,
                        );
                    }))
            })
            .on_click(|event, _, cx| {
                if event.click_count() == 2 {
                    // AppKit's title bar owns the configured Zoom action.
                    // Consume the duplicate GPUI propagation without invoking
                    // a second window zoom from the application layer.
                    cx.stop_propagation();
                }
            })
            .child(
                div()
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap(px(4.))
                    .pl(px(8.))
                    .pr(px(theme::HEADER_END_PADDING))
                    // Local tabs carry their split controls here, left of the
                    // new-terminal button; the local page itself has no
                    // terminal toolbar anymore.
                    .when(
                        self.active_pane()
                            .is_some_and(|p| matches!(p.spec, SessionSpec::Local { .. })),
                        |controls| {
                            let count = self.tabs[self.active].panes.len();
                            controls.children(
                                [
                                    (Split::Horizontal, IconName::PanelRight, "split-right"),
                                    (Split::Vertical, IconName::PanelBottom, "split-down"),
                                ]
                                .map(|(axis, icon, key)| {
                                    self.header_control(
                                        key,
                                        self.button(key, "")
                                            .icon(icon)
                                            .ghost()
                                            .h(px(self.controls_height()))
                                            .disabled(count >= MAX_LOCAL_PANES)
                                            .tooltip(self.t(match axis {
                                                Split::Horizontal => "split_right",
                                                Split::Vertical => "split_down",
                                            }))
                                            .on_click(cx.listener(move |this, _, w, cx| {
                                                this.split(axis, None, w, cx)
                                            })),
                                        cx,
                                    )
                                }),
                            )
                        },
                    )
                    .child(
                        self.header_control(
                            "new-local",
                            self.button("new-local", "")
                                .icon(IconName::Plus)
                                .ghost()
                                .h(px(self.controls_height()))
                                .tooltip(self.t("new_local"))
                                .on_click(cx.listener(|this, _, w, cx| this.new_local(w, cx))),
                            cx,
                        ),
                    )
                    .child(
                        self.header_control(
                            "connections",
                            self.button("connections", "")
                                .svg_icon("icons/server.svg")
                                .ghost()
                                .h(px(self.controls_height()))
                                .on_click(cx.listener(|this, _, w, cx| this.toggle_sidebar(w, cx))),
                            cx,
                        ),
                    )
                    .child(
                        self.header_control(
                            "theme-toggle",
                            self.button("theme-toggle", "")
                                .svg_icon(if self.prefs.theme == Theme::Night {
                                    "icons/sun.svg"
                                } else {
                                    "icons/moon.svg"
                                })
                                .ghost()
                                .h(px(self.controls_height()))
                                .tooltip(self.t(if self.prefs.theme == Theme::Night {
                                    "day"
                                } else {
                                    "night"
                                }))
                                .on_click(cx.listener(|this, _, w, cx| {
                                    this.prefs.theme = if this.prefs.theme == Theme::Night {
                                        Theme::Day
                                    } else {
                                        Theme::Night
                                    };
                                    this.apply_preferences(w, cx);
                                })),
                            cx,
                        ),
                    )
                    .child(
                        self.header_control(
                            "settings",
                            self.button("settings", "")
                                .icon(IconName::Settings)
                                .ghost()
                                .h(px(self.controls_height()))
                                .tooltip(self.t("settings"))
                                .on_click(cx.listener(|this, _, w, cx| this.open_settings(w, cx))),
                            cx,
                        ),
                    )
                    .when(cfg!(target_os = "windows"), |toolbar| {
                        toolbar.child(
                            self.header_control(
                                "about",
                                self.button("about", "")
                                    .icon(IconName::Info)
                                    .ghost()
                                    .h(px(self.controls_height()))
                                    .tooltip(self.t("about_mantash"))
                                    .on_click(cx.listener(|this, _, w, cx| {
                                        this.show_modal(Modal::About, w, cx);
                                    })),
                                cx,
                            ),
                        )
                    }),
            );
        let toolbar = if cfg!(target_os = "windows") {
            div()
                .flex()
                .items_center()
                .flex_shrink_0()
                .h(px(self.toolbar_height()))
                .bg(p.surface)
                .border_b_1()
                .border_color(p.border)
                .child(div().relative().flex_1().min_w_0().h_full().child(header))
                .child(self.windows_caption_buttons(window, cx))
                .into_any_element()
        } else {
            TitleBar::new()
                .h(px(self.toolbar_height()))
                .bg(p.surface)
                .border_b_1()
                .border_color(p.border)
                .child(div().relative().flex_1().min_w_0().h_full().child(header))
                .into_any_element()
        };
        let entity = cx.entity();
        let root = div()
            .id("mantash-workbench")
            .size_full()
            .min_w_0()
            .min_h_0()
            .relative()
            .flex()
            .flex_col()
            .track_focus(&self.root_focus)
            .key_context("MantaSH")
            .font_family(self.prefs.ui_font.clone())
            .text_size(px(self.prefs.ui_size))
            .text_color(p.text)
            .bg(p.background)
            .on_action(cx.listener(|this, _: &NewLocal, w, cx| {
                if this.modal.is_none() {
                    this.new_local(w, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &NewSsh, w, cx| {
                if this.modal.is_none() {
                    this.profile_form(None, None, w, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &OpenConnections, w, cx| {
                if this.modal.is_none() || matches!(this.modal, Some(Modal::Connections)) {
                    this.toggle_sidebar(w, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &CloseActive, w, cx| {
                let editor = match &this.modal {
                    Some(Modal::Editor { owner }) => Some(*owner),
                    _ => None,
                };
                if let Some(owner) = editor {
                    // Closing inside the editor dialog targets its active
                    // document, not the tab behind the overlay.
                    if let Some(id) = this.pane(owner).and_then(|p| p.active_document) {
                        this.request_close(CloseTarget::Document(owner, id), w, cx);
                    }
                } else if this.modal.is_none() {
                    if let Some(pane) = this.active_pane() {
                        let _ = pane;
                        let target = CloseTarget::Tab(this.tabs[this.active].id);
                        this.request_close(target, w, cx);
                    }
                }
            }))
            .on_action(cx.listener(|this, _: &SplitRight, w, cx| {
                if this.modal.is_none() {
                    this.split(Split::Horizontal, None, w, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &SplitDown, w, cx| {
                if this.modal.is_none() {
                    this.split(Split::Vertical, None, w, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &OpenSettings, w, cx| {
                if this.modal.is_none() {
                    this.open_settings(w, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ShowAboutModal, w, cx| {
                this.show_modal(Modal::About, w, cx);
            }))
            .on_action(cx.listener(|this, _: &SaveFile, _, cx| {
                let editor = match &this.modal {
                    Some(Modal::Editor { owner }) => Some(*owner),
                    _ => None,
                };
                if let Some(owner) = editor {
                    if let Some(id) = this.pane(owner).and_then(|p| p.active_document) {
                        this.save_document(owner, id, false, cx);
                    }
                }
            }))
            .on_action(
                cx.listener(|this, _: &Quit, w, cx| this.request_close(CloseTarget::Window, w, cx)),
            )
            .on_action(cx.listener(|this, _: &Escape, w, cx| {
                if this.connection_drag.is_some() {
                    // Escape belongs to the active manual-sort drag: it cancels
                    // the drag, not the modal underneath.
                    this.cancel_connection_drag(cx);
                    return;
                }
                if matches!(this.modal, Some(Modal::Connections))
                    && (!this.connection_multi.is_empty()
                        || this.connection_anchor.is_some()
                        || this.connection_selected.is_some())
                {
                    this.connection_multi.clear();
                    this.connection_anchor = None;
                    this.connection_selected = None;
                    cx.notify();
                } else if matches!(this.modal, Some(Modal::LocalHistory))
                    && this.active_pane().is_some_and(|pane| {
                        !this.history_views[pane.spec.history_scope().index()]
                            .selected
                            .is_empty()
                    })
                {
                    if let Some(scope) = this.active_pane().map(|pane| pane.spec.history_scope()) {
                        this.history_views[scope.index()].clear_selection();
                        cx.notify();
                    }
                } else if matches!(this.modal, Some(Modal::Transfers { .. }))
                    && !this.transfer_selected.is_empty()
                {
                    this.transfer_selected.clear();
                    this.transfer_anchor = None;
                    cx.notify();
                } else if this.modal.is_some() {
                    this.cancel_modal(w, cx);
                }
            }))
            .on_action(
                cx.listener(|this, _: &OpenWindowControls, w, cx| this.open_window_controls(w, cx)),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    // A stale suppress flag from a finished tab drag would swallow
                    // every later mouse click (file rows stop opening on double
                    // click); any new press ends that suppression immediately.
                    if this.tab_strip.drag.is_none() && this.tab_strip.suppress_click.is_some() {
                        this.tab_strip.suppress_click = None;
                        cx.notify();
                    }
                }),
            )
            .on_action(
                cx.listener(|this, action: &window_actions::ArrangeWindow, w, cx| {
                    this.arrange_window(action.command, w, cx)
                }),
            )
            .on_action(cx.listener(|_, _: &MinimizeWindow, w, _| w.minimize_window()))
            .on_action(cx.listener(|_, _: &FullscreenWindow, w, _| w.toggle_fullscreen()))
            .on_action(cx.listener(|this, _: &ToggleFiles, _, cx| {
                if this.modal.is_none() {
                    this.toggle_files_panel(cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ToggleHistory, w, cx| {
                // History exists only on SSH pages; the local pane ignores the
                // shortcut and the menu entry alike.
                if this.modal.is_none()
                    && this
                        .active_pane()
                        .is_some_and(|p| !matches!(p.spec, SessionSpec::Local { .. }))
                {
                    this.show_modal(Modal::LocalHistory, w, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ToggleSystem, _, cx| {
                if this.modal.is_none() {
                    this.set_tool(Tool::System, cx);
                }
            }))
            // File-list context menu: actions carry their owning pane so a
            // menu left open across a tab switch cannot hit the wrong pane.
            .on_action(cx.listener(|this, action: &FileMenuEdit, w, cx| {
                // Re-resolve the single-file target at execution time; a menu
                // left open across selection or directory changes stays inert.
                if let Some(path) = this
                    .pane(action.owner)
                    .and_then(|pane| pane.files.edit_target())
                {
                    this.open_document(action.owner, path, w, cx);
                }
            }))
            .on_action(cx.listener(|this, action: &FileMenuDownload, w, cx| {
                this.choose_transfer(action.owner, false, true, w, cx);
            }))
            .on_action(cx.listener(|this, action: &FileMenuCopy, _, cx| {
                this.copy_file_selection(action.owner, cx);
            }))
            .on_action(cx.listener(|this, action: &FileMenuPaste, _, cx| {
                this.paste_file_clipboard(action.owner, cx);
            }))
            .on_action(cx.listener(|this, action: &FileMenuRename, w, cx| {
                this.file_name_form(action.owner, true, w, cx);
            }))
            .on_action(cx.listener(|this, action: &FileMenuDelete, w, cx| {
                this.review_file_delete(action.owner, w, cx);
            }))
            .on_action(cx.listener(|this, action: &FileMenuMkdir, w, cx| {
                this.file_name_form(action.owner, false, w, cx);
            }))
            .on_action(cx.listener(|this, _: &NextPane, w, cx| {
                if this.modal.is_none() {
                    if let Some(tab) = this.tabs.get_mut(this.active) {
                        tab.active = (tab.active + 1) % tab.panes.len();
                    }
                    this.focus_active(w, cx);
                    this.changed(cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ModalNext, w, cx| this.modal_tab(false, w, cx)))
            .on_action(cx.listener(|this, _: &ModalPrevious, w, cx| this.modal_tab(true, w, cx)))
            .child(
                canvas(
                    |_, _, _| (),
                    move |_, _, window, _| {
                        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                            if phase != DispatchPhase::Bubble {
                                return;
                            }
                            let entity = entity.clone();
                            entity.update(cx, |this, cx| {
                                drag_resize_step(this, event, window, cx);
                            });
                        });
                    },
                )
                .absolute()
                .size_0(),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.resize = None;
                    this.resize_last_x = None;
                    this.resize_last_y = None;
                    this.changed(cx);
                }),
            )
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if this.tab_strip.drag.is_some() && event.keystroke.key == "escape" {
                    this.cancel_tab_drag(cx);
                    window.prevent_default();
                    cx.stop_propagation();
                }
            }))
            .child(self.tab_drag_capture(cx))
            .child(toolbar)
            .map(|root| {
                #[cfg(target_os = "macos")]
                let root = root.child(self.native_titlebar_geometry());
                root
            })
            .when_some(self.storage_warning.clone(), |d, warning| {
                d.child(div().px_3().py_2().text_color(p.error).child(warning))
            })
            .child(body);
        root.when(self.modal.is_some(), |d| {
            d.child(self.render_modal(window, cx))
        })
    }
}

/// Window-level resize drag continuation. Element-level mouse moves
/// never arrive once the pointer crosses the path input (the input
/// swallows button-down moves for its own text selection), so every
/// resize drag is driven from a window listener instead.
fn drag_resize_step(
    this: &mut Workbench,
    e: &MouseMoveEvent,
    w: &mut Window,
    cx: &mut Context<Workbench>,
) {
    if e.pressed_button != Some(MouseButton::Left) {
        this.resize = None;
        return;
    }
    match this.resize {
        // ConnectionScroll, FileScroll, TreeScroll and ModalScroll
        // drags run on the window-level listener registered by the
        // scrollbar itself (see overlay_scrollbar).
        Some(Resize::ConnectionScroll)
        | Some(Resize::FileScroll(_))
        | Some(Resize::TreeScroll(_))
        | Some(Resize::TreeScrollX(_))
        | Some(Resize::FileScrollX(_))
        | Some(Resize::ModalScroll) => {}
        Some(Resize::TreeWidth) => {
            let last = *this.resize_last_x.get_or_insert(f32::from(e.position.x));
            let delta = f32::from(e.position.x) - last;
            this.resize_last_x = Some(f32::from(e.position.x));
            let viewport = f32::from(w.viewport_size().width);
            let max = viewport * 0.5;
            if let Some(pane) = this.active_pane_mut() {
                pane.files.tree_width = (pane.files.tree_width + delta).clamp(120., max);
            }
        }
        Some(Resize::FilesHeight) => {
            let last = *this.resize_last_y.get_or_insert(f32::from(e.position.y));
            let delta = f32::from(e.position.y) - last;
            this.resize_last_y = Some(f32::from(e.position.y));
            let current = this.prefs.files_preferred_height.unwrap_or(240.);
            this.prefs.files_preferred_height = Some((current - delta).clamp(120., 600.));
        }
        Some(Resize::Tools) => {
            this.prefs.tool_preferred_width = Some(crate::layout::tool_width(
                Some(f32::from(w.viewport_size().width - e.position.x)),
                w.viewport_size().width.into(),
                w.viewport_size().width.into(),
            ))
        }
        Some(Resize::Split(tab_id, node_id, axis)) => {
            if let Some(bounds) = this.split_bounds.get(&node_id) {
                let ratio = if axis == Split::Horizontal {
                    (e.position.x - bounds.left()) / bounds.size.width
                } else {
                    (e.position.y - bounds.top()) / bounds.size.height
                };
                if let Some(tab) = this.tabs.iter_mut().find(|t| t.id == tab_id) {
                    tab.layout.set_ratio(node_id, ratio.into());
                }
            }
        }
        // Classic table gesture: pulling an edge right widens the
        // column to its left; pulling left narrows to the minimum.
        Some(Resize::FileName(owner)) => {
            let last = *this.resize_last_x.get_or_insert(f32::from(e.position.x));
            let delta = f32::from(e.position.x) - last;
            this.resize_last_x = Some(f32::from(e.position.x));
            let minimum = 100.;
            if let Some(p) = this.pane_mut(owner) {
                // No upper cap: the name column may widen as far
                // as the user drags (horizontal scroll takes over
                // beyond the viewport); the 100px column floor is
                // the only bound.
                let width = (p.files.name_extra.unwrap_or(minimum) + delta).max(minimum);
                p.files.name_extra = Some(width);
            }
        }
        Some(Resize::FileSize(owner)) => {
            let last = *this.resize_last_x.get_or_insert(f32::from(e.position.x));
            let delta = f32::from(e.position.x) - last;
            this.resize_last_x = Some(f32::from(e.position.x));
            if let Some(p) = this.pane_mut(owner) {
                p.files.size_extra = (p.files.size_extra + delta).max(0.);
            }
        }
        None => {}
    }
    this.changed(cx);
}

/// Register platform-appropriate app shortcuts without stealing common Shell control keys.
pub fn bind_keys(cx: &mut App) {
    let primary = if cfg!(target_os = "macos") {
        "cmd"
    } else {
        "ctrl-shift"
    };
    cx.bind_keys([
        KeyBinding::new(&format!("{primary}-t"), NewLocal, Some("MantaSH")),
        KeyBinding::new(&format!("{primary}-n"), NewSsh, Some("MantaSH")),
        KeyBinding::new(
            if cfg!(target_os = "macos") {
                "cmd-shift-o"
            } else {
                "ctrl-shift-o"
            },
            OpenConnections,
            Some("MantaSH"),
        ),
        KeyBinding::new(&format!("{primary}-w"), CloseActive, Some("MantaSH")),
        KeyBinding::new(&format!("{primary}-d"), SplitRight, Some("MantaSH")),
        KeyBinding::new(
            if cfg!(target_os = "macos") {
                "cmd-shift-d"
            } else {
                "ctrl-alt-d"
            },
            SplitDown,
            Some("MantaSH"),
        ),
        KeyBinding::new(&format!("{primary}-,"), OpenSettings, Some("MantaSH")),
        KeyBinding::new(&format!("{primary}-s"), SaveFile, Some("MantaSH")),
        KeyBinding::new("ctrl-s", SaveFile, Some("MantaSHEditor")),
        KeyBinding::new(&format!("{primary}-q"), Quit, Some("MantaSH")),
        KeyBinding::new(&format!("{primary}-1"), ToggleFiles, Some("MantaSH")),
        KeyBinding::new(&format!("{primary}-2"), ToggleHistory, Some("MantaSH")),
        KeyBinding::new(&format!("{primary}-3"), ToggleSystem, Some("MantaSH")),
        KeyBinding::new(
            if cfg!(target_os = "macos") {
                "cmd-shift-0"
            } else {
                "ctrl-shift-0"
            },
            OpenWindowControls,
            Some("MantaSH"),
        ),
        KeyBinding::new(
            if cfg!(target_os = "macos") {
                "cmd-m"
            } else {
                "ctrl-shift-m"
            },
            MinimizeWindow,
            Some("MantaSH"),
        ),
        KeyBinding::new(
            if cfg!(target_os = "macos") {
                "ctrl-cmd-f"
            } else {
                "f11"
            },
            FullscreenWindow,
            Some("MantaSH"),
        ),
        KeyBinding::new("ctrl-tab", NextPane, Some("MantaSH")),
        KeyBinding::new("enter", OpenResourceDetails, Some("MantaSHResource")),
        KeyBinding::new("space", OpenResourceDetails, Some("MantaSHResource")),
        KeyBinding::new("escape", Escape, Some("MantaSHModal")),
        KeyBinding::new("tab", ModalNext, Some("MantaSHModal")),
        KeyBinding::new("shift-tab", ModalPrevious, Some("MantaSHModal")),
    ]);
    cx.bind_keys([
        // Root binds Tab to focus navigation before on_key_down runs. Unbind only
        // inside the terminal so its VT encoder receives Tab/Backtab for Shells and TUIs.
        KeyBinding::new("tab", NoAction, Some("MantaSHTerminal")),
        KeyBinding::new("shift-tab", NoAction, Some("MantaSHTerminal")),
        KeyBinding::new(
            if cfg!(target_os = "macos") {
                "cmd-a"
            } else {
                "ctrl-a"
            },
            gpui_component::input::SelectAll,
            Some("MantaSHFiles"),
        ),
        KeyBinding::new(
            &format!("{primary}-a"),
            gpui_component::input::SelectAll,
            Some("MantaSHTerminal"),
        ),
    ]);
}

/// Build real platform menus from the same actions used by the workbench.
pub(super) fn menus(language: Language, cx: &mut App) {
    use super::window_actions::ArrangeWindow;
    use crate::window_layout::{Command, Position};
    let t = |key| i18n::text(language, key);
    cx.set_menus(vec![
        Menu {
            name: "MantaSH".into(),
            items: vec![
                MenuItem::action(t("about_mantash"), ShowAboutModal),
                MenuItem::separator(),
                MenuItem::action(t("settings"), OpenSettings),
                MenuItem::separator(),
                MenuItem::os_submenu("Services", SystemMenuType::Services),
                MenuItem::separator(),
                MenuItem::action(
                    if language == Language::Zh {
                        "退出 MantaSH"
                    } else {
                        "Quit MantaSH"
                    },
                    Quit,
                ),
            ],
        },
        Menu {
            name: t("terminal").into(),
            items: vec![
                MenuItem::action(t("new_local"), NewLocal),
                MenuItem::action(t("new_ssh"), NewSsh),
                MenuItem::action(t("connection_library"), OpenConnections),
                MenuItem::separator(),
                MenuItem::action(t("split_right"), SplitRight),
                MenuItem::action(t("split_down"), SplitDown),
                MenuItem::separator(),
                MenuItem::action(t("close"), CloseActive),
            ],
        },
        Menu {
            name: if language == Language::Zh {
                "编辑"
            } else {
                "Edit"
            }
            .into(),
            items: vec![
                MenuItem::action(t("copy"), gpui_component::input::Copy),
                MenuItem::action(
                    if language == Language::Zh {
                        "粘贴"
                    } else {
                        "Paste"
                    },
                    gpui_component::input::Paste,
                ),
                MenuItem::action(t("select_all"), gpui_component::input::SelectAll),
                MenuItem::separator(),
                MenuItem::action(t("save"), SaveFile),
            ],
        },
        Menu {
            name: t("window_menu").into(),
            items: if cfg!(target_os = "macos") {
                vec![]
            } else {
                vec![
                    MenuItem::action(t("window_controls"), OpenWindowControls),
                    MenuItem::separator(),
                    MenuItem::action(
                        t("window_compact"),
                        ArrangeWindow {
                            command: Command::Compact,
                        },
                    ),
                    MenuItem::action(
                        t("window_standard"),
                        ArrangeWindow {
                            command: Command::Standard,
                        },
                    ),
                    MenuItem::action(
                        t("window_wide"),
                        ArrangeWindow {
                            command: Command::Wide,
                        },
                    ),
                    MenuItem::action(
                        t("window_fill"),
                        ArrangeWindow {
                            command: Command::Fill,
                        },
                    ),
                    MenuItem::separator(),
                    MenuItem::action(
                        t("window_left"),
                        ArrangeWindow {
                            command: Command::Position {
                                position: Position::Left,
                            },
                        },
                    ),
                    MenuItem::action(
                        t("window_center"),
                        ArrangeWindow {
                            command: Command::Position {
                                position: Position::Center,
                            },
                        },
                    ),
                    MenuItem::action(
                        t("window_right"),
                        ArrangeWindow {
                            command: Command::Position {
                                position: Position::Right,
                            },
                        },
                    ),
                    MenuItem::action(
                        t("window_restore"),
                        ArrangeWindow {
                            command: Command::Restore,
                        },
                    ),
                    MenuItem::separator(),
                    MenuItem::action(t("window_minimize"), MinimizeWindow),
                    MenuItem::action(t("window_toggle_fullscreen"), FullscreenWindow),
                ]
            },
        },
        Menu {
            name: if language == Language::Zh {
                "工具"
            } else {
                "Tools"
            }
            .into(),
            items: vec![
                MenuItem::action(t("files"), ToggleFiles),
                MenuItem::action(t("history"), ToggleHistory),
                MenuItem::action(t("system"), ToggleSystem),
            ],
        },
    ]);
    #[cfg(target_os = "macos")]
    window_native::install_system_menu(
        t("window_menu"),
        t("window_minimize"),
        t("window_zoom"),
        t("window_fullscreen"),
    );
}
