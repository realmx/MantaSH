//! In-list manual sorting for the connection library rows.
//!
//! One gesture stays attached to a stable profile UUID: pressing a row starts
//! tracking, vertical midpoints of the visible rows choose an insertion slot,
//! and dropping inside the viewport reorders (and persists) the saved list.
//! Escape, releasing outside the viewport or losing the button cancels it.
use super::*;

/// A row drag gesture; never leaves the list or creates floating state.
#[derive(Clone)]
pub(super) struct ConnectionDrag {
    pub gesture: Id,
    pub row: Id,
    pub start: Point<Pixels>,
    pub pointer: Point<Pixels>,
    pub moved: bool,
    pub slot: usize,
}

impl Workbench {
    /// Start tracking a press on a library row. The row's own selection logic
    /// has already run; only movement past the threshold turns this into a
    /// reorder, so plain clicks keep their meaning.
    pub(super) fn begin_connection_drag(&mut self, row: Id, event: &MouseDownEvent) {
        self.connection_suppress_click = None;
        if self.connection_drag.is_some() {
            return;
        }
        self.connection_drag = Some(ConnectionDrag {
            gesture: Id::new_v4(),
            row,
            start: event.position,
            pointer: event.position,
            moved: false,
            slot: 0,
        });
    }
    /// Window-level move continuation: update the slot from the pointer and
    /// keep edge-hover scrolling alive while the button is held.
    pub(super) fn move_connection_drag(&mut self, pointer: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(mut drag) = self.connection_drag.take() else {
            return;
        };
        drag.pointer = pointer;
        drag.moved |= (pointer.y - drag.start.y).abs() >= px(4.);
        if drag.moved {
            let base = self.connection_scroll.0.borrow().base_handle.clone();
            let viewport = base.bounds();
            let max = f32::from(base.max_offset().height);
            if max > 0. {
                let edge = px(24.);
                // Scroll offsets run negative while scrolled down ([-max, 0]):
                // hovering the TOP edge must move the offset toward zero
                // (scroll up) and the BOTTOM edge further negative (scroll
                // down). The tab strip's horizontal version uses +step at its
                // LEFT edge for the same reason; the first version mirrored
                // the sign backwards and scrolled opposite the pointer.
                let step = if pointer.y < viewport.top() + edge {
                    12.
                } else if pointer.y > viewport.bottom() - edge {
                    -12.
                } else {
                    0.
                };
                if step != 0. {
                    let old = base.offset();
                    base.set_offset(point(old.x, px((f32::from(old.y) + step).clamp(-max, 0.))));
                }
            }
            // uniform_list rows share one height and start at the viewport
            // edge, so each row's painted midpoint projects exactly:
            // mid(ix) = viewport.top + ix*H + offset + H/2. The base handle's
            // child_bounds stay empty for uniform lists, so the height comes
            // from the list state's own last_item_size.
            let visible = self.visible_connections(cx);
            if !visible.is_empty()
                && let Some(height) = self
                    .connection_scroll
                    .0
                    .borrow()
                    .last_item_size
                    // contents.height = row_height * item_count (the list adds
                    // no vertical padding of its own); `item` is the list's
                    // own viewport, not one row.
                    .map(|size| size.contents.height / visible.len() as f32)
                    .filter(|height| *height > px(0.))
            {
                let top0 = viewport.top() + base.offset().y;
                let y = pointer.y.clamp(viewport.top(), viewport.bottom());
                let rows_above =
                    ((f32::from(y - top0) + f32::from(height) / 2.) / f32::from(height)).floor();
                drag.slot = (rows_above as isize).clamp(0, visible.len() as isize) as usize;
            }
            cx.notify();
        }
        self.connection_drag = Some(drag);
    }
    /// Commit a drop inside the viewport; anything else cancels without
    /// touching the saved order. Selection identities survive the reorder.
    pub(super) fn finish_connection_drag(
        &mut self,
        pointer: Point<Pixels>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(drag) = self.connection_drag.take() else {
            return false;
        };
        if !drag.moved {
            return true;
        }
        self.connection_suppress_click = Some(drag.row);
        let base = self.connection_scroll.0.borrow().base_handle.clone();
        if !base.bounds().contains(&pointer) {
            cx.notify();
            return true;
        }
        let visible: Vec<Id> = self
            .visible_connections(cx)
            .iter()
            .map(|profile| profile.id)
            .collect();
        let Some(from) = self.profiles.iter().position(|p| p.id == drag.row) else {
            cx.notify();
            return true;
        };
        // The slot counts visible rows; translate it into an insertion point
        // of the full saved list (a filter may hide rows between them).
        let before_target = visible.get(drag.slot).copied();
        let profile = self.profiles.remove(from);
        let to = match before_target {
            Some(id) => self.profiles.iter().position(|p| p.id == id),
            None => visible
                .last()
                .and_then(|id| self.profiles.iter().position(|p| p.id == *id))
                .map(|ix| ix + 1),
        }
        .unwrap_or(self.profiles.len());
        self.profiles.insert(to.min(self.profiles.len()), profile);
        self.backend.save_profiles(self.profiles.clone());
        cx.notify();
        true
    }
    /// Drop the gesture without committing its proposed insertion slot.
    pub(super) fn cancel_connection_drag(&mut self, cx: &mut Context<Self>) {
        if let Some(drag) = self.connection_drag.take() {
            if drag.moved {
                self.connection_suppress_click = Some(drag.row);
            }
            cx.notify();
        }
    }
    /// Keep edge scrolling alive while the pointer rests at a viewport edge.
    pub(super) fn spawn_connection_drag_ticker(&self, cx: &mut Context<Self>) {
        let gesture = self
            .connection_drag
            .as_ref()
            .map(|drag| drag.gesture)
            .unwrap_or_else(Id::new_v4);
        cx.spawn(async move |view, cx| {
            loop {
                gpui::Timer::after(Duration::from_millis(40)).await;
                let keep = view
                    .update(cx, |this, cx| {
                        let Some(drag) = this
                            .connection_drag
                            .as_ref()
                            .filter(|drag| drag.gesture == gesture)
                        else {
                            return false;
                        };
                        if drag.moved {
                            let pointer = drag.pointer;
                            this.move_connection_drag(pointer, cx);
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
}
