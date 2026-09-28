//! Horizontal tab reordering consumes its pointer gesture before the native title bar can move.
use super::*;

/// A gesture remains attached to one stable tab identity and never creates a floating window.
#[derive(Clone)]
pub(super) struct TabDrag {
    pub gesture: Id,
    pub tab: Id,
    pub start: Point<Pixels>,
    pub pointer: Point<Pixels>,
    pub moved: bool,
    pub slot: usize,
    pub before: Option<Id>,
}

impl Workbench {
    /// Feed AppKit the final clipped tab/button regions; blank title-bar space is deliberately absent.
    #[cfg(target_os = "macos")]
    pub(super) fn native_titlebar_geometry(&self) -> AnyElement {
        let routing = self.native_titlebar.clone();
        let scroll = self.tab_strip.scroll.clone();
        let controls = self.tab_strip.control_bounds.clone();
        let count = self.tabs.len();
        canvas(
            |_, _, _| (),
            move |_, _, _, _| {
                if let Some(routing) = &routing {
                    let viewport = scroll.bounds();
                    let offset = scroll.offset();
                    let mut regions = Vec::new();
                    for index in 0..count {
                        if let Some(mut rect) = scroll.bounds_for_item(index) {
                            rect.origin += offset;
                            let left = rect.left().max(viewport.left());
                            let right = rect.right().min(viewport.right());
                            let top = rect.top().max(viewport.top());
                            let bottom = rect.bottom().min(viewport.bottom());
                            if right > left && bottom > top {
                                regions.push(Bounds::new(
                                    point(left, top),
                                    size(right - left, bottom - top),
                                ));
                            }
                        }
                    }
                    regions.extend(controls.borrow().values().copied());
                    routing.set_regions(regions);
                }
            },
        )
        .absolute()
        .size_full()
        .into_any_element()
    }
    /// Capture pointer presses for in-strip sorting and selection; keyboard activation stays native.
    pub(super) fn begin_tab_drag(
        &mut self,
        tab: Id,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.prevent_default();
        cx.stop_propagation();
        self.tab_strip.suppress_click = None;
        if self.modal.is_some() || !self.tabs.iter().any(|item| item.id == tab) {
            return;
        }
        let gesture = Id::new_v4();
        self.tab_strip.drag = Some(TabDrag {
            gesture,
            tab,
            start: event.position,
            pointer: event.position,
            moved: false,
            slot: 0,
            before: None,
        });
        // Continue scrolling at the horizontal edges while the pointer is held still.
        cx.spawn(async move |view, cx| {
            loop {
                gpui::Timer::after(Duration::from_millis(40)).await;
                let keep = view
                    .update(cx, |this, cx| {
                        let Some(drag) = this
                            .tab_strip
                            .drag
                            .as_ref()
                            .filter(|drag| drag.gesture == gesture)
                        else {
                            return false;
                        };
                        if drag.moved {
                            this.move_tab_drag(drag.pointer, cx);
                        }
                        true
                    })
                    .unwrap_or(false);
                if !keep {
                    break;
                }
            }
        })
        .detach();
    }
    /// Use only horizontal midpoints to choose an insertion slot; vertical motion cannot reorder tabs.
    pub(super) fn move_tab_drag(&mut self, pointer: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(mut drag) = self.tab_strip.drag.take() else {
            return;
        };
        drag.pointer = pointer;
        drag.moved |= (pointer.x - drag.start.x).abs() >= px(4.);
        if drag.moved {
            let bounds = self.tab_strip.scroll.bounds();
            if bounds.contains(&pointer) {
                let edge = px(24.).min(bounds.size.width / 4.);
                let step = if pointer.x < bounds.left() + edge {
                    px(12.)
                } else if pointer.x > bounds.right() - edge {
                    px(-12.)
                } else {
                    px(0.)
                };
                let scroll = &self.tab_strip.scroll;
                let old = scroll.offset();
                scroll.set_offset(point(
                    (old.x + step).clamp(-scroll.max_offset().width, px(0.)),
                    old.y,
                ));
            }
            let offset = self.tab_strip.scroll.offset().x;
            let x = pointer.x.clamp(bounds.left(), bounds.right());
            let mut slot = 0;
            let mut before = None;
            for (index, tab) in self
                .tabs
                .iter()
                .enumerate()
                .filter(|(_, tab)| tab.id != drag.tab)
            {
                if let Some(rect) = self.tab_strip.scroll.bounds_for_item(index) {
                    if x >= rect.left() + offset + rect.size.width / 2. {
                        slot += 1;
                    } else {
                        before = Some(tab.id);
                        break;
                    }
                }
            }
            drag.slot = slot;
            drag.before = before;
            cx.notify();
        }
        self.tab_strip.drag = Some(drag);
    }
    /// Opaque tab hitboxes exclude native window dragging, so explicitly forward their wheel motion.
    pub(super) fn scroll_tabs_from_pointer(
        &mut self,
        event: &ScrollWheelEvent,
        cx: &mut Context<Self>,
    ) {
        let delta = event.delta.pixel_delta(px(self.prefs.ui_size * 2.));
        let movement = if delta.x.abs() > delta.y.abs() {
            delta.x
        } else {
            delta.y
        };
        let scroll = &self.tab_strip.scroll;
        scroll.set_offset(point(
            (scroll.offset().x + movement).clamp(-scroll.max_offset().width, px(0.)),
            px(0.),
        ));
        cx.stop_propagation();
        cx.notify();
    }
    /// Commit a drop inside the strip, preserving the selected session and all pane identities.
    pub(super) fn finish_tab_drag(
        &mut self,
        pointer: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(drag) = self.tab_strip.drag.take() else {
            return false;
        };
        self.tab_strip.completed_drags = self.tab_strip.completed_drags.wrapping_add(1);
        if !drag.moved {
            // The capture boundary owns pointer clicks as well as drags; keyboard clicks
            // continue through the native label button.
            if let Some(index) = self.tabs.iter().position(|tab| tab.id == drag.tab)
                && let Some(mut bounds) = self.tab_strip.scroll.bounds_for_item(index)
            {
                bounds.origin += self.tab_strip.scroll.offset();
                if bounds.contains(&pointer) {
                    self.active = index;
                    self.focus_active(window, cx);
                    self.changed(cx);
                }
            }
            self.tab_strip.suppress_click = Some(drag.tab);
            return true;
        }
        self.tab_strip.suppress_click = Some(drag.tab);
        if self.tab_strip.scroll.bounds().contains(&pointer) {
            let active = self.tabs.get(self.active).map(|tab| tab.id);
            if let Some(from) = self.tabs.iter().position(|tab| tab.id == drag.tab) {
                let to = drag.slot.min(self.tabs.len() - 1);
                if from != to {
                    let tab = self.tabs.remove(from);
                    self.tabs.insert(to, tab);
                    self.active = self
                        .tabs
                        .iter()
                        .position(|tab| Some(tab.id) == active)
                        .unwrap_or(0);
                    // Sorting is not a tab switch; keep the user's drag-scroll position.
                    self.tab_strip.active =
                        self.tabs.get(self.active).map(|tab| (tab.id, self.active));
                    self.changed(cx);
                }
            }
        }
        cx.notify();
        true
    }
    /// Cancel an interrupted gesture without committing its proposed insertion position.
    pub(super) fn cancel_tab_drag(&mut self, cx: &mut Context<Self>) {
        if let Some(drag) = self.tab_strip.drag.take() {
            self.tab_strip.completed_drags = self.tab_strip.completed_drags.wrapping_add(1);
            if drag.moved {
                self.tab_strip.suppress_click = Some(drag.tab);
            }
            cx.notify();
        }
    }
    /// Capture moves/releases across the window so leaving the strip cannot select terminal text.
    pub(super) fn tab_drag_capture(&self, cx: &Context<Self>) -> AnyElement {
        let view = cx.entity().downgrade();
        canvas(
            |_, _, _| (),
            move |_, _, window, _| {
                #[cfg(debug_assertions)]
                {
                    let observing = view.clone();
                    window.on_mouse_event(move |event: &MouseDownEvent, phase, _, cx| {
                        if phase.capture() {
                            let _ = observing.update(cx, |this, _| {
                                if let Some(qa) = &mut this.qa {
                                    qa.pointer_events.push((
                                        "down".into(),
                                        [event.position.x.into(), event.position.y.into()],
                                    ));
                                    if qa.pointer_events.len() > 12 {
                                        qa.pointer_events.remove(0);
                                    }
                                }
                            });
                        }
                    });
                }
                let moving = view.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                    if phase.capture() {
                        let _ = moving.update(cx, |this, cx| {
                            if this.tab_strip.drag.is_none() {
                                return;
                            }
                            if event.pressed_button == Some(MouseButton::Left) {
                                this.move_tab_drag(event.position, cx);
                            } else {
                                this.cancel_tab_drag(cx);
                            }
                            window.prevent_default();
                            cx.stop_propagation();
                        });
                    }
                });
                let releasing = view.clone();
                window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
                    if phase.capture() && event.button == MouseButton::Left {
                        let _ = releasing.update(cx, |this, cx| {
                            if this.finish_tab_drag(event.position, window, cx) {
                                window.prevent_default();
                                // Let native buttons clear their pressed state on mouse-up. The
                                // captured tab ID suppresses the click that follows a drag.
                            }
                        });
                    }
                });
            },
        )
        .absolute()
        .size_full()
        .into_any_element()
    }
}
