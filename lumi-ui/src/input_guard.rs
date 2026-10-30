use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::base::input::{InputBaseState, InputModeKind};
use gpui_kit::component::input::Paste;
use gpui_kit::{
    App, Entity, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Styled as _, Window, canvas,
};

/// Prevents stale macOS mouse-drag events (`synthetic_drag` / delayed trackpad
/// button release) from extending or restoring a selection after text has been
/// pasted or edited.
///
/// In `gpui_macos`, every left-button `MouseMoveEvent` schedules a 16ms
/// repeating `synthetic_drag` task that replays `MouseMoveEvent` until
/// `LeftMouseUp` arrives (which trackpad Three-Finger Drag / Tap-to-Drag holds
/// active after finger lift, or which can race with a quick keyboard `Cmd+V`).
/// Because `InputBaseState` leaves `selecting = true` and `selected_word_range`
/// set across edits and pastes, a replayed `MouseMoveEvent` at the drag-end
/// coordinate would otherwise re-extend the selection over `N` characters when
/// pasting `M < N` characters.
///
/// Placing [`Self::element`] immediately before an `Editor` or `Input` stops
/// those post-edit left-drag `MouseMoveEvent`s during GPUI's `Capture` phase
/// while still allowing `MouseUpEvent` and subsequent `MouseDownEvent`s through.
#[derive(Clone)]
pub(crate) struct EditDragGuard {
    cancel_drag: Rc<Cell<bool>>,
    word_selected: Rc<Cell<bool>>,
    word_invalidated: Rc<Cell<bool>>,
    clear_word_anchor: Rc<dyn Fn(&mut App)>,
}

impl EditDragGuard {
    pub(crate) fn new<M: InputModeKind>(input: &Entity<InputBaseState<M>>) -> Self {
        let weak = input.downgrade();
        Self {
            cancel_drag: Rc::new(Cell::new(false)),
            word_selected: Rc::new(Cell::new(false)),
            word_invalidated: Rc::new(Cell::new(false)),
            clear_word_anchor: Rc::new(move |cx: &mut App| {
                if let Some(state) = weak.upgrade() {
                    state.update(cx, |state, cx| {
                        let range = state.selected_range();
                        if range.is_empty() {
                            state.set_selected_range(range, cx);
                        }
                    });
                }
            }),
        }
    }

    /// Marks any in-flight mouse selection drag as superseded by an edit or
    /// programmatic selection change.
    pub(crate) fn cancel(&self) {
        self.cancel_drag.set(true);
        if self.word_selected.get() {
            self.word_invalidated.set(true);
        }
    }

    /// Pass to `InteractiveElement::capture_action` on an ancestor container of
    /// the guarded `Editor` or `Input` so even no-op `Paste` actions cancel any
    /// active drag while still propagating `Paste` down to `InputBaseState`.
    pub(crate) fn paste_capture(&self) -> impl Fn(&Paste, &mut Window, &mut App) + 'static {
        let guard = self.clone();
        move |_, _, cx| {
            guard.cancel();
            cx.propagate();
        }
    }

    /// Out-of-flow zero-size element that must be added as an earlier sibling
    /// before the guarded `Editor` or `Input` so its capture-phase mouse
    /// listeners run before `TextElement`'s listeners.
    pub(crate) fn element(&self) -> impl IntoElement {
        let cancel_drag = self.cancel_drag.clone();
        let word_selected = self.word_selected.clone();
        let word_invalidated = self.word_invalidated.clone();
        let clear_word_anchor = self.clear_word_anchor.clone();
        canvas(
            |_, _, _| (),
            move |_, _, window: &mut Window, _| {
                let cancel_on_down = cancel_drag.clone();
                let word_on_down = word_selected.clone();
                let invalidated_on_down = word_invalidated.clone();
                let clear_fn = clear_word_anchor.clone();
                window.on_mouse_event(move |event: &MouseDownEvent, phase, _, cx| {
                    if !phase.capture() {
                        return;
                    }
                    cancel_on_down.set(false);
                    if event.button == MouseButton::Left {
                        if event.click_count == 2 {
                            word_on_down.set(true);
                            invalidated_on_down.set(false);
                        } else if event.modifiers.shift {
                            if word_on_down.replace(false) && invalidated_on_down.replace(false) {
                                clear_fn(cx);
                            }
                        } else {
                            word_on_down.set(false);
                            invalidated_on_down.set(false);
                        }
                    }
                });

                let cancel_on_up = cancel_drag.clone();
                window.on_mouse_event(move |_: &MouseUpEvent, phase, _, _| {
                    if phase.capture() {
                        cancel_on_up.set(false);
                    }
                });

                let cancel_on_move = cancel_drag.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                    if phase.capture()
                        && event.pressed_button == Some(MouseButton::Left)
                        && cancel_on_move.get()
                    {
                        cx.stop_propagation();
                    }
                });
            },
        )
        .absolute()
        .size_0()
    }
}

#[cfg(test)]
#[path = "input_guard_test.rs"]
mod tests;
