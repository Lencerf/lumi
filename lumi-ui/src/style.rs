use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, Sizable as _};
use gpui_kit::{App, ElementId};

/// A compact inline button colored to show that a datum in a table — a date
/// or an account name — can be clicked.
pub(crate) fn interactive_button(id: impl Into<ElementId>, cx: &App) -> Button {
    let color = cx.theme().blue;
    let style = ButtonCustomVariant::new(cx)
        .foreground(color)
        .hover(color.opacity(0.12))
        .active(color.opacity(0.2));
    Button::new(id).custom(style).small().compact()
}
