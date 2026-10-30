use gpui_kit::component::input::{Editor, EditorState, InputEvent};
use gpui_kit::test::{TestEventExt as _, TestWindowExt as _};
use gpui_kit::{
    AppContext as _, Bounds, ClipboardItem, Entity, EntityInputHandler as _,
    InteractiveElement as _, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent,
    ParentElement as _, PlatformInput, Point, Render, Styled as _, Subscription, TestAppContext,
    Window, WindowBounds, WindowOptions, div, point, px, size,
};

use crate::input_guard::EditDragGuard;

struct GuardedEditorHarness {
    state: Entity<EditorState>,
    guard: EditDragGuard,
    _subscription: Subscription,
}

impl GuardedEditorHarness {
    fn new(initial: &str, window: &mut Window, cx: &mut gpui_kit::Context<Self>) -> Self {
        let state = cx.new(|cx| {
            let mut state = EditorState::new(window, cx).default_value(initial.to_string());
            state.set_line_number(false, window, cx);
            state
        });
        let guard = EditDragGuard::new(&state);
        let guard_for_sub = guard.clone();
        let _subscription = cx.subscribe(&state, move |_, _, event, _| {
            if matches!(event, InputEvent::Change) {
                guard_for_sub.cancel();
            }
        });
        Self {
            state,
            guard,
            _subscription,
        }
    }
}

impl Render for GuardedEditorHarness {
    fn render(
        &mut self,
        _: &mut Window,
        _: &mut gpui_kit::Context<Self>,
    ) -> impl gpui_kit::IntoElement {
        div().size_full().p_6().child(
            div()
                .relative()
                .h(px(240.))
                .w(px(640.))
                .capture_action(self.guard.paste_capture())
                .child(self.guard.element())
                .child(
                    Editor::new(&self.state)
                        .size_full()
                        .aria_label("Guarded editor"),
                ),
        )
    }
}

fn char_bounds(
    state: &Entity<EditorState>,
    range_utf16: std::ops::Range<usize>,
    window: &mut Window,
    cx: &mut gpui_kit::App,
) -> Bounds<gpui_kit::Pixels> {
    state.update(cx, |state, cx| {
        let text_bounds = state.text_bounds().expect("laid-out text bounds");
        state
            .bounds_for_range(range_utf16, text_bounds, window, cx)
            .expect("laid-out character bounds")
    })
}

fn mid_left(bounds: Bounds<gpui_kit::Pixels>) -> Point<gpui_kit::Pixels> {
    point(bounds.origin.x, bounds.origin.y + bounds.size.height * 0.5)
}

fn mid_right(bounds: Bounds<gpui_kit::Pixels>) -> Point<gpui_kit::Pixels> {
    point(
        bounds.origin.x + bounds.size.width,
        bounds.origin.y + bounds.size.height * 0.5,
    )
}

fn paste_shortcut() -> &'static str {
    if cfg!(target_os = "macos") {
        "cmd-v"
    } else {
        "ctrl-v"
    }
}

#[gpui_kit::test]
fn test_pasting_shorter_text_over_double_clicked_word_does_not_reselect_on_stale_drag(
    cx: &mut TestAppContext,
) {
    cx.update(crate::init);
    let initial = "2025-01-10 * \"CoffeeShop\" \"Morning\"";
    // `"CoffeeShop"` is 10 ASCII bytes/UTF-16 units at `14..24`.
    let word_range = 14..24;
    let (handle, harness) = cx.update(|cx| {
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    Default::default(),
                    size(px(800.), px(400.)),
                ))),
                ..Default::default()
            },
            cx,
            |window, cx| cx.new(|cx| GuardedEditorHarness::new(initial, window, cx)),
        )
        .expect("open test window")
    });
    cx.run_until_parked();

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let state = harness.read(cx).state.clone();
        let word_bounds = char_bounds(&state, word_range.clone(), window, cx);
        let click_pos = point(
            word_bounds.origin.x + word_bounds.size.width * 0.4,
            word_bounds.origin.y + word_bounds.size.height * 0.5,
        );
        let drag_end_pos = mid_right(word_bounds);

        // Double-click press selects the 10-char word `"CoffeeShop"` while
        // `LeftMouseUp` has not yet arrived (matching macOS Tap-to-Drag /
        // Three-Finger Drag / fast Cmd+V).
        window.dispatch_event(
            PlatformInput::MouseDown(MouseDownEvent {
                button: MouseButton::Left,
                position: click_pos,
                modifiers: Modifiers::default(),
                click_count: 2,
                first_mouse: false,
            }),
            cx,
        );
        window.render_frame(cx);
        assert_eq!(state.read(cx).selected_range(), word_range);

        // Paste 3 characters (`M = 3 < N = 10`) over the 10-char selection.
        cx.write_to_clipboard(ClipboardItem::new_string("Tea".into()));
        window.press(paste_shortcut(), cx);
        assert_eq!(state.read(cx).value(), "2025-01-10 * \"Tea\" \"Morning\"");
        assert_eq!(state.read(cx).selected_range(), 17..17);

        // Replay `gpui_macos`'s 16ms `synthetic_drag` left-button `MouseMoveEvent`
        // at the end of the original 10-char word: selection must stay collapsed
        // at `17..17` instead of snapping back to `14..24` (10 chars selected).
        window.pointer_move(drag_end_pos, Some(MouseButton::Left), cx);
        assert_eq!(state.read(cx).selected_range(), 17..17);

        // Release mouse outside the editor hitbox (which leaves `selected_word_range`
        // set inside `InputBaseState`), then Shift+Click after `"Tea\" "` (`offset 20`):
        // selection extends cleanly from `17..20` instead of snapping back to `14..24`.
        window.dispatch_event(
            PlatformInput::MouseUp(MouseUpEvent {
                button: MouseButton::Left,
                position: point(px(2.), px(2.)),
                modifiers: Modifiers::default(),
                click_count: 2,
            }),
            cx,
        );
        window.render_frame(cx);

        let target_pos = mid_left(char_bounds(&state, 20..21, window, cx));
        window.dispatch_event(
            PlatformInput::MouseDown(MouseDownEvent {
                button: MouseButton::Left,
                position: target_pos,
                modifiers: Modifiers {
                    shift: true,
                    ..Modifiers::default()
                },
                click_count: 1,
                first_mouse: false,
            }),
            cx,
        );
        window.pointer_up(target_pos, MouseButton::Left, cx);
        assert_eq!(state.read(cx).selected_range(), 17..20);
    })
    .unwrap();
}

#[gpui_kit::test]
fn test_pasting_shorter_text_over_dragged_selection_does_not_reselect_and_preserves_next_drag(
    cx: &mut TestAppContext,
) {
    cx.update(crate::init);
    let initial = "2025-01-10 * \"CoffeeShop\" \"Morning\"";
    let word_range = 14..24;
    let (handle, harness) = cx.update(|cx| {
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    Default::default(),
                    size(px(800.), px(400.)),
                ))),
                ..Default::default()
            },
            cx,
            |window, cx| cx.new(|cx| GuardedEditorHarness::new(initial, window, cx)),
        )
        .expect("open test window")
    });
    cx.run_until_parked();

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let state = harness.read(cx).state.clone();
        let word_bounds = char_bounds(&state, word_range.clone(), window, cx);
        let drag_start = mid_left(word_bounds);
        let drag_end = mid_right(word_bounds);

        // Drag across `"CoffeeShop"` (10 chars) without releasing the left button yet.
        window.pointer_down(drag_start, MouseButton::Left, cx);
        window.pointer_move(drag_end, Some(MouseButton::Left), cx);
        assert_eq!(state.read(cx).selected_range(), word_range);

        // Paste `"Tea"` (3 chars, `M < N`) while the drag is still active, followed
        // by a stale macOS `synthetic_drag` move at `drag_end` and then button release.
        cx.write_to_clipboard(ClipboardItem::new_string("Tea".into()));
        window.press(paste_shortcut(), cx);
        window.pointer_move(drag_end, Some(MouseButton::Left), cx);
        assert_eq!(state.read(cx).value(), "2025-01-10 * \"Tea\" \"Morning\"");
        assert_eq!(state.read(cx).selected_range(), 17..17);

        window.pointer_up(drag_end, MouseButton::Left, cx);
        assert_eq!(state.read(cx).selected_range(), 17..17);

        // Starting a fresh mouse drag afterwards selects `"Morning"` (`20..27`) normally.
        let next_start = mid_left(char_bounds(&state, 20..20, window, cx));
        let next_end = mid_left(char_bounds(&state, 27..27, window, cx));
        window.pointer_down(next_start, MouseButton::Left, cx);
        window.pointer_move(next_end, Some(MouseButton::Left), cx);
        window.pointer_up(next_end, MouseButton::Left, cx);
        assert_eq!(state.read(cx).selected_range(), 20..27);
    })
    .unwrap();
}
