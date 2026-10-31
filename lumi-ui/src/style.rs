use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants as _};
use gpui_kit::component::table::Column;
use gpui_kit::component::{ActiveTheme as _, Sizable as _, Size, StyledExt as _, Theme};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Div, ElementId, Hsla, InteractiveElement as _, IntoElement,
    ParentElement as _, Pixels, Stateful, Styled as _, div, px,
};
#[cfg(test)]
use gpui_kit::{SharedString, TestSupportExt as _};

/// Customizes the global GPUI Kit theme tokens for lumi's report tables.
pub(crate) fn configure_theme(cx: &mut App) {
    Theme::update(cx, |theme| {
        // Alternating row backgrounds already separate rows, so omit the thin
        // horizontal divider between them.
        theme.table_row_border = theme.transparent;
        // Deeper grey header and alternating white/grey rows across all tables.
        theme.table_head = theme.secondary;
        theme.table_head_foreground = theme.secondary_foreground;
        theme.table_even = if theme.is_dark() {
            theme.muted.opacity(0.45)
        } else {
            theme.muted
        };
        theme.table_active = theme
            .blue
            .opacity(if theme.is_dark() { 0.22 } else { 0.14 });
    });
}

/// A sharp-cornered paper-card container for report tables.
pub(crate) fn table_card(cx: &App) -> Div {
    let theme = cx.theme();
    div()
        .border_1()
        .border_color(theme.border)
        .bg(theme.tokens.table)
        .shadow_sm()
}

/// The shared header row style for `DataTable` delegates, matching
/// `TrieReport`'s medium-weight header.
pub(crate) fn table_header() -> Stateful<Div> {
    div().id("header").font_medium()
}

/// Base row container for `DataTable` delegates without a horizontal bottom
/// border reservation so vertical column separators span the full row height.
pub(crate) fn table_row(row_ix: usize) -> Stateful<Div> {
    div().id(("row", row_ix)).border_b_0()
}

/// Alternating white/grey row background by logical row index.
pub(crate) fn table_row_bg(row_ix: usize, cx: &App) -> Hsla {
    if row_ix.is_multiple_of(2) {
        cx.theme().table
    } else {
        cx.theme().table_even
    }
}

/// Vertical column separator color matching the base (white) row surface so it
/// shows on the grey header and grey alternating rows while blending into white rows.
pub(crate) fn table_col_separator_color(cx: &App) -> Hsla {
    cx.theme().table
}

enum ColSeparatorSide {
    Left(Pixels),
    Right(Pixels),
}

fn col_separator_line(
    top: Pixels,
    bottom: Pixels,
    side: ColSeparatorSide,
    #[cfg_attr(not(test), allow(unused_variables))] id: impl FnOnce() -> ElementId,
    cx: &App,
) -> AnyElement {
    let line = div()
        .absolute()
        .top(top)
        .bottom(bottom)
        .w(px(1.))
        .bg(table_col_separator_color(cx));
    let line = match side {
        ColSeparatorSide::Left(offset) => line.left(offset),
        ColSeparatorSide::Right(offset) => line.right(offset),
    };
    #[cfg(test)]
    let line = line.id(id()).test_support();
    line.into_any_element()
}

/// Header cell for a `Size::Small` `DataTable`, drawing `1px` vertical white
/// column separators across the full header row height.
pub(crate) fn data_table_th(columns: &[Column], col_ix: usize, cx: &App) -> Div {
    let pad = Size::Small.table_cell_padding();
    let cols_len = columns.len();
    let is_last = col_ix + 1 == cols_len;
    div()
        .size_full()
        .child(columns[col_ix].name.clone())
        .when(col_ix > 0, |this| {
            this.child(col_separator_line(
                -pad.top,
                -pad.bottom,
                ColSeparatorSide::Left(-pad.left),
                || ("col-sep-head", col_ix).into(),
                cx,
            ))
        })
        .when(is_last, |this| {
            this.child(col_separator_line(
                -pad.top,
                -pad.bottom,
                ColSeparatorSide::Right(-pad.right),
                || ("col-sep-head", cols_len).into(),
                cx,
            ))
        })
}

/// Body cell wrapper for a `Size::Small` `DataTable`, drawing `1px` vertical
/// white column separators aligned with [`data_table_th`].
pub(crate) fn data_table_td(
    #[cfg_attr(not(test), allow(unused_variables))] row_ix: usize,
    col_ix: usize,
    columns_count: usize,
    cell: impl IntoElement,
    cx: &App,
) -> Div {
    let pad = Size::Small.table_cell_padding();
    let is_last = col_ix + 1 == columns_count;
    div()
        .size_full()
        .child(cell)
        .when(col_ix > 0, |this| {
            this.child(col_separator_line(
                -pad.top,
                -pad.bottom,
                ColSeparatorSide::Left(-pad.left),
                || {
                    #[cfg(test)]
                    {
                        (
                            ElementId::from("col-sep-cell"),
                            SharedString::from(format!("{row_ix}:{col_ix}")),
                        )
                            .into()
                    }
                    #[cfg(not(test))]
                    {
                        "col-sep-cell".into()
                    }
                },
                cx,
            ))
        })
        .when(is_last, |this| {
            this.child(col_separator_line(
                -pad.top,
                -pad.bottom,
                ColSeparatorSide::Right(-pad.right),
                || {
                    #[cfg(test)]
                    {
                        (
                            ElementId::from("col-sep-cell"),
                            SharedString::from(format!("{row_ix}:{columns_count}")),
                        )
                            .into()
                    }
                    #[cfg(not(test))]
                    {
                        "col-sep-cell".into()
                    }
                },
                cx,
            ))
        })
}

/// Full-height `1px` left column separator for currency header cells in [`Table`].
pub(crate) fn trie_head_col_separator(
    #[cfg_attr(not(test), allow(unused_variables))] root: &'static str,
    #[cfg_attr(not(test), allow(unused_variables))] col_ix: usize,
    cx: &App,
) -> AnyElement {
    col_separator_line(
        px(0.),
        px(-1.),
        ColSeparatorSide::Left(px(0.)),
        || {
            #[cfg(test)]
            {
                (
                    ElementId::from("trie-col-sep-head"),
                    SharedString::from(format!("{root}:{col_ix}")),
                )
                    .into()
            }
            #[cfg(not(test))]
            {
                "trie-col-sep-head".into()
            }
        },
        cx,
    )
}

/// Full-height `1px` left column separator for currency data cells in [`Table`].
pub(crate) fn trie_cell_col_separator(
    #[cfg_attr(not(test), allow(unused_variables))] root: &'static str,
    row_ix: usize,
    #[cfg_attr(not(test), allow(unused_variables))] col_ix: usize,
    cx: &App,
) -> AnyElement {
    col_separator_line(
        if row_ix > 0 { px(-1.) } else { px(0.) },
        px(0.),
        ColSeparatorSide::Left(px(0.)),
        || {
            #[cfg(test)]
            {
                (
                    ElementId::from("trie-col-sep-cell"),
                    SharedString::from(format!("{root}:{row_ix}:{col_ix}")),
                )
                    .into()
            }
            #[cfg(not(test))]
            {
                "trie-col-sep-cell".into()
            }
        },
        cx,
    )
}

/// Background color for the user-focused record in report tables.
pub(crate) fn table_focused_row_bg(cx: &App) -> Hsla {
    cx.theme().table_active
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
