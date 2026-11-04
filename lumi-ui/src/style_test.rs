use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::table::{Column, ColumnSort};
use gpui_kit::{FontWeight, Pixels, Styled as _, TestAppContext, px};

use crate::style::{
    TableColumns, table_card, table_col_separator_color, table_focused_row_bg, table_header,
    table_row, table_row_bg,
};

#[gpui_kit::test]
fn test_configure_theme_sets_table_palette_and_removes_row_border(cx: &mut TestAppContext) {
    cx.update(crate::init);
    cx.read(|cx| {
        let theme = cx.theme();
        assert_eq!(theme.table_row_border, theme.transparent);
        assert_eq!(theme.table_head, theme.secondary);
        assert_eq!(theme.table_head_foreground, theme.secondary_foreground);
        assert_eq!(theme.table_even, theme.muted);
        assert_eq!(table_row_bg(0, cx), theme.table);
        assert_eq!(table_row_bg(1, cx), theme.muted);
        assert_eq!(table_col_separator_color(cx), theme.table);
        assert_eq!(table_col_separator_color(cx), table_row_bg(0, cx));
        assert_ne!(table_col_separator_color(cx), theme.table_head);
        assert_ne!(table_col_separator_color(cx), table_row_bg(1, cx));
        assert_eq!(table_focused_row_bg(cx), theme.table_active);
        assert_ne!(table_focused_row_bg(cx), table_row_bg(0, cx));
        assert_ne!(table_focused_row_bg(cx), table_row_bg(1, cx));
    });
}

#[gpui_kit::test]
fn test_table_card_and_header_match_paper_card_style(cx: &mut TestAppContext) {
    cx.update(crate::init);
    cx.read(|cx| {
        let mut card = table_card(cx);
        let style = card.style();
        assert_eq!(style.corner_radii.top_left, None);
        assert_eq!(style.corner_radii.top_right, None);
        assert_eq!(style.corner_radii.bottom_right, None);
        assert_eq!(style.corner_radii.bottom_left, None);
        assert_eq!(style.border_widths.top, Some(px(1.).into()));
        assert_eq!(style.border_widths.right, Some(px(1.).into()));
        assert_eq!(style.border_widths.bottom, Some(px(1.).into()));
        assert_eq!(style.border_widths.left, Some(px(1.).into()));
        assert_eq!(style.border_color, Some(cx.theme().border));
        assert!(
            style
                .box_shadow
                .as_ref()
                .is_some_and(|shadows| !shadows.is_empty())
        );

        let mut header = table_header();
        assert_eq!(header.style().text.font_weight, Some(FontWeight::MEDIUM));

        let mut row = table_row(0);
        assert_eq!(row.style().border_widths.bottom, Some(px(0.).into()));
    });
}

#[test]
fn test_table_columns_fits_viewport_and_keeps_resize_flush_with_right_edge() {
    let widths_of =
        |cols: &TableColumns| -> Vec<Pixels> { cols.as_slice().iter().map(|c| c.width).collect() };

    let mut cols = TableColumns::new(
        vec![
            Column::new("level", "Level").width(px(90.)).sortable(),
            Column::new("message", "Message").width(px(500.)),
            Column::new("location", "Location").width(px(300.)),
        ],
        "message",
    );

    // Narrower than base sum (890px): preserves minimum base width for horizontal scroll.
    assert!(!cols.fit_to_width(px(700.)));
    assert_eq!(widths_of(&cols), [px(90.), px(500.), px(300.)]);

    // Wider viewport (1200px): flexible column absorbs the extra 310px.
    assert!(cols.fit_to_width(px(1200.)));
    assert_eq!(widths_of(&cols), [px(90.), px(810.), px(300.)]);
    assert!(!cols.fit_to_width(px(1200.)));

    // Dragging the divider between Message and Location 60px left grows Location by 60px
    // so total width stays at 1200px with no trailing empty column.
    assert!(cols.apply_resized_widths(&[px(90.), px(750.), px(300.)]));
    assert_eq!(widths_of(&cols), [px(90.), px(750.), px(360.)]);

    // Dragging the last column's right edge inward clamps back to 1200px; dragging it
    // wider (> 1200px) preserves the wider width for horizontal scrolling.
    assert!(cols.apply_resized_widths(&[px(90.), px(750.), px(250.)]));
    assert_eq!(widths_of(&cols), [px(90.), px(750.), px(360.)]);
    assert!(!cols.apply_resized_widths(&[px(90.), px(750.), px(460.)]));
    assert_eq!(widths_of(&cols), [px(90.), px(750.), px(460.)]);

    // Sort and column order stay synchronized across refreshes.
    cols.set_sort(0, ColumnSort::Ascending);
    assert_eq!(cols[0].sort, Some(ColumnSort::Ascending));
    cols.move_column(0, 2);
    assert_eq!(cols[2].key.as_ref(), "level");
    assert_eq!(cols[2].sort, Some(ColumnSort::Ascending));
}
