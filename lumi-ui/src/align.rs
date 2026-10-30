use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gpui_kit::component::input::{EditorState, Redo, RopeExt as _, Undo};
use gpui_kit::{App, Context, Entity, Subscription, Window};
use lumi::edit::{
    PostingAlignment, align_posting_line, ends_in_string, parse_decimal_point_column_in_text,
};

use crate::completion::BeancountCompletionProvider;

/// Auto-aligns edited Beancount posting lines in an [`EditorState`] so that
/// each posting amount's decimal point sits at `decimal-point-column`
/// (default `50`, 1-based), keeping at least 1 space after long account names.
pub(crate) struct PostingAligner {
    prev_text: Rc<RefCell<String>>,
    aligning: Rc<Cell<bool>>,
    skip_align: Rc<Cell<bool>>,
    scan_buffer_option: bool,
    _subscription: Subscription,
}

impl PostingAligner {
    pub(crate) fn new<V: 'static>(
        text: &Entity<EditorState>,
        scan_buffer_option: bool,
        cx: &mut Context<V>,
    ) -> Self {
        let prev_text = Rc::new(RefCell::new(text.read(cx).value().to_string()));
        let aligning = Rc::new(Cell::new(false));
        let skip_align = Rc::new(Cell::new(false));

        // Whenever `EditorState` notifies without `InputEvent::Change` (such as
        // `set_value`), `cx.defer` runs after any `InputEvent::Change` already
        // queued in the same `flush_effects` cycle, keeping `prev_text` in sync
        // with non-interactive buffer replacements without clobbering the
        // pre-edit snapshot before `on_change` runs.
        let prev_for_observe = prev_text.clone();
        let aligning_for_observe = aligning.clone();
        let subscription = cx.observe(text, move |_, state, cx| {
            if aligning_for_observe.get() {
                return;
            }
            let prev = prev_for_observe.clone();
            let aligning = aligning_for_observe.clone();
            cx.defer(move |cx| {
                if !aligning.get() {
                    *prev.borrow_mut() = state.read(cx).value().to_string();
                }
            });
        });

        Self {
            prev_text,
            aligning,
            skip_align,
            scan_buffer_option,
            _subscription: subscription,
        }
    }

    /// Pass to `InteractiveElement::capture_action` on an ancestor container of
    /// the guarded `Editor` so `Undo` replays history without immediately
    /// re-aligning the undone text.
    pub(crate) fn undo_capture(&self) -> impl Fn(&Undo, &mut Window, &mut App) + 'static {
        let skip = self.skip_align.clone();
        move |_, _, cx| {
            arm_history_skip(&skip, cx);
            cx.propagate();
        }
    }

    /// Pass to `InteractiveElement::capture_action` on an ancestor container of
    /// the guarded `Editor` so `Redo` replays history without re-running
    /// alignment.
    pub(crate) fn redo_capture(&self) -> impl Fn(&Redo, &mut Window, &mut App) + 'static {
        let skip = self.skip_align.clone();
        move |_, _, cx| {
            arm_history_skip(&skip, cx);
            cx.propagate();
        }
    }

    /// Inspects the lines modified since the previous buffer snapshot and
    /// adjusts separator spaces on any edited posting lines.
    pub(crate) fn on_change(
        &self,
        text: &Entity<EditorState>,
        provider: &BeancountCompletionProvider,
        window: &mut Window,
        cx: &mut App,
    ) {
        let reopen_completions = provider.take_requested_on_last_edit();
        if self.aligning.get() {
            return;
        }
        if self.skip_align.replace(false) {
            *self.prev_text.borrow_mut() = text.read(cx).value().to_string();
            return;
        }

        let (cur_str, cursor, has_selection) = {
            let state = text.read(cx);
            (
                state.value().to_string(),
                state.cursor(),
                !state.selected_range().is_empty(),
            )
        };
        if has_selection {
            *self.prev_text.borrow_mut() = cur_str;
            return;
        }

        let prev_str = self.prev_text.borrow().clone();
        if cur_str == prev_str {
            return;
        }

        let (prefix_len, suffix_len, only_deleted_spaces) = diff_summary(&prev_str, &cur_str);
        let decimal_col = self
            .scan_buffer_option
            .then(|| parse_decimal_point_column_in_text(&cur_str))
            .flatten()
            .unwrap_or_else(|| provider.decimal_point_column());

        let alignments: Vec<(usize, PostingAlignment)> = {
            let state = text.read(cx);
            let rope = state.text();
            let first_row = rope.offset_to_point(prefix_len.min(rope.len())).row;
            let edit_end = cur_str.len().saturating_sub(suffix_len).min(rope.len());
            let last_row = rope.offset_to_point(edit_end).row;

            let mut in_string = false;
            for row in 0..first_row {
                let raw = rope.slice_line(row).to_string();
                let line = raw.strip_suffix('\r').unwrap_or(&raw);
                in_string = ends_in_string(line, in_string);
            }

            let mut pending = Vec::new();
            for row in first_row..=last_row {
                let line_start = rope.line_start_offset(row);
                let raw = rope.slice_line(row).to_string();
                let line = raw.strip_suffix('\r').unwrap_or(&raw);
                let started_in_string = in_string;
                in_string = ends_in_string(line, in_string);
                if started_in_string {
                    continue;
                }
                if let Some(alignment) = align_posting_line(line, decimal_col) {
                    pending.push((line_start, alignment));
                }
            }
            pending
        };

        if alignments.is_empty() {
            *self.prev_text.borrow_mut() = cur_str;
            return;
        }

        self.aligning.set(true);
        text.update(cx, |state, cx| {
            let mut new_cursor = cursor;
            for (line_start, alignment) in alignments.into_iter().rev() {
                let abs_ws_start = line_start + alignment.ws_range.start;
                let abs_ws_end = line_start + alignment.ws_range.end;
                new_cursor = adjusted_cursor_for_alignment(
                    new_cursor,
                    abs_ws_start..abs_ws_end,
                    alignment.desired_spaces,
                    only_deleted_spaces,
                );
                let spaces = " ".repeat(alignment.desired_spaces);
                state.set_selected_range(abs_ws_start..abs_ws_end, cx);
                state.replace(spaces, window, cx);
            }
            state.set_selected_range(new_cursor..new_cursor, cx);

            if reopen_completions {
                let items = provider.complete_at(state.text(), new_cursor, false);
                if !items.is_empty() {
                    let word_len: usize = state
                        .text()
                        .chars_at(new_cursor)
                        .reversed()
                        .take_while(|ch| ch.is_alphanumeric() || *ch == '_')
                        .map(char::len_utf8)
                        .sum();
                    let start_offset = new_cursor - word_len;
                    let query = state.text().slice(start_offset..new_cursor).to_string();
                    state.present_completion_items(start_offset, query, items, cx);
                }
            }
        });
        *self.prev_text.borrow_mut() = text.read(cx).value().to_string();
        self.aligning.set(false);
    }
}

fn arm_history_skip(skip: &Rc<Cell<bool>>, cx: &mut App) {
    skip.set(true);
    // Two nested `cx.defer` hops ensure the reset runs after any
    // `InputEvent::Change` queued during the bubble phase of `Undo`/`Redo`,
    // while still clearing synchronously within `flush_effects` if `Undo`/`Redo`
    // was a no-op on an empty stack.
    let skip_outer = skip.clone();
    cx.defer(move |cx| {
        let skip_inner = skip_outer.clone();
        cx.defer(move |_| {
            skip_inner.set(false);
        });
    });
}

pub(crate) fn diff_summary(prev: &str, cur: &str) -> (usize, usize, bool) {
    let prev_bytes = prev.as_bytes();
    let cur_bytes = cur.as_bytes();
    let mut prefix = prev_bytes
        .iter()
        .zip(cur_bytes.iter())
        .take_while(|(a, b)| a == b)
        .count();
    while prefix > 0 && (!prev.is_char_boundary(prefix) || !cur.is_char_boundary(prefix)) {
        prefix -= 1;
    }

    let mut suffix = prev_bytes[prefix..]
        .iter()
        .rev()
        .zip(cur_bytes[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    while suffix > 0
        && (!prev.is_char_boundary(prev.len() - suffix)
            || !cur.is_char_boundary(cur.len() - suffix))
    {
        suffix -= 1;
    }

    let deleted = &prev[prefix..prev.len() - suffix];
    let inserted = &cur[prefix..cur.len() - suffix];
    let only_deleted_spaces =
        inserted.is_empty() && !deleted.is_empty() && deleted.bytes().all(|b| b == b' ');
    (prefix, suffix, only_deleted_spaces)
}

pub(crate) fn adjusted_cursor_for_alignment(
    cursor: usize,
    abs_ws: std::ops::Range<usize>,
    desired_spaces: usize,
    only_deleted_spaces: bool,
) -> usize {
    let new_abs_ws_end = abs_ws.start + desired_spaces;
    if cursor <= abs_ws.start {
        cursor
    } else if cursor > abs_ws.end {
        (cursor as isize + desired_spaces as isize - abs_ws.len() as isize) as usize
    } else if cursor < abs_ws.end {
        cursor.min(new_abs_ws_end)
    } else if only_deleted_spaces && abs_ws.len() < desired_spaces {
        cursor
    } else {
        new_abs_ws_end
    }
}

#[cfg(test)]
#[path = "align_test.rs"]
mod tests;
