//! MantaSH's bounded extension for explicit, independently undoable text transformations.
use super::*;

/// UTF-8 selection (including direction) and the actual input scroll offset.
#[derive(Clone, Debug, PartialEq)]
pub struct EditSnapshot {
    pub selection: Range<usize>,
    pub reversed: bool,
    pub scroll: Point<Pixels>,
}

impl InputState {
    pub fn edit_snapshot(&self) -> EditSnapshot {
        EditSnapshot {
            selection: self.selected_range.into(),
            reversed: self.selection_reversed,
            scroll: self.scroll_handle.offset(),
        }
    }

    pub(super) fn restore_edit_view(&mut self, view: &EditSnapshot, cx: &mut Context<Self>) {
        self.selected_range = view.selection.clone().into();
        self.selection_reversed = view.reversed;
        self.scroll_handle.set_offset(view.scroll);
        self.update_preferred_column();
        cx.notify();
    }

    /// Restore a view for unchanged text without inserting content or stealing focus.
    pub fn set_edit_snapshot(&mut self, view: &EditSnapshot, cx: &mut Context<Self>) -> bool {
        let text = self.value();
        if view.selection.start > view.selection.end
            || !text.is_char_boundary(view.selection.start)
            || !text.is_char_boundary(view.selection.end)
        {
            return false;
        }
        self.restore_edit_view(view, cx);
        true
    }

    /// Replace a multiline document as one independent undo/redo step. Callers must
    /// validate the source revision; active IME composition is never overwritten.
    pub fn apply_edit_transaction(
        &mut self,
        text: &str,
        after: EditSnapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.disabled || !self.mode.is_multi_line() || self.ime_marked_range.is_some()
            || after.selection.start > after.selection.end
            || !text.is_char_boundary(after.selection.start)
            || !text.is_char_boundary(after.selection.end)
        {
            return false;
        }
        let before = self.edit_snapshot();
        let range = self.range_to_utf16(&(0..self.text.len()));
        self.history.checkpoint();
        self.history.clear_redos();
        self.replace_text_in_range_silent(Some(range), text, window, cx);
        if let Some(change) = self.history.last_change_mut() {
            change.edit_views = Some((before, after.clone()));
        }
        self.history.checkpoint();
        self.restore_edit_view(&after, cx);
        true
    }
}
