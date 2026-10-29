use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, Sizable as _, Theme};
use gpui_kit::{App, Div, ElementId, Styled as _, div};

/// Customizes the global GPUI Kit theme tokens for lumi's report tables.
pub(crate) fn configure_theme(cx: &mut App) {
    Theme::update(cx, |theme| {
        // Alternating row backgrounds already separate rows, so omit the thin
        // horizontal divider between them.
        theme.table_row_border = theme.transparent;
    });
}

/// A sharp-cornered bordered container for report tables.
pub(crate) fn table_card(cx: &App) -> Div {
    let theme = cx.theme();
    div()
        .border_1()
        .border_color(theme.border)
        .bg(theme.tokens.table)
}

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

#[cfg(test)]
#[path = "style_test.rs"]
mod tests;
