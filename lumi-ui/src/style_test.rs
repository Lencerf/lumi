use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::table::{Column, ColumnSort};
use gpui_kit::{FontWeight, Pixels, Styled as _, TestAppContext, px};

use crate::style::{
    TableColumns, scale_widths_proportionally, table_card, table_col_separator_color,
    table_focused_row_bg, table_header, table_row, table_row_bg,
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
fn test_table_columns_scales_proportionally_when_wider_and_never_shrinks_when_narrower() {
    let widths_of =
        |cols: &TableColumns| -> Vec<Pixels> { cols.as_slice().iter().map(|c| c.width).collect() };

    let mut cols = TableColumns::new(vec![
        Column::new("level", "Level").width(px(100.)).sortable(),
        Column::new("message", "Message").width(px(500.)),
        Column::new("location", "Location").width(px(300.)),
    ]);
    assert_eq!(cols.natural_total_width(), px(900.));

    // Narrower than natural sum (900px): columns never shrink below natural widths.
    assert!(!cols.fit_to_width(px(700.)));
    assert_eq!(widths_of(&cols), [px(100.), px(500.), px(300.)]);

    // Exact fit (900px): columns match natural widths.
    assert!(!cols.fit_to_width(px(900.)));
    assert_eq!(widths_of(&cols), [px(100.), px(500.), px(300.)]);

    // Wider viewport (1350px = 1.5x of 900px): all columns widen proportionally.
    assert!(cols.fit_to_width(px(1350.)));
    assert_eq!(widths_of(&cols), [px(150.), px(750.), px(450.)]);
    assert!(!cols.fit_to_width(px(1350.)));

    // Shrinking back below natural sum (650px): columns return to natural widths (no shrinking).
    assert!(cols.fit_to_width(px(650.)));
    assert_eq!(widths_of(&cols), [px(100.), px(500.), px(300.)]);

    // Updating natural widths re-fits to the current viewport width.
    assert!(cols.fit_to_width(px(1200.)));
    assert!(cols.set_natural_widths(vec![px(80.), px(320.), px(200.)]));
    assert_eq!(cols.natural_widths(), [px(80.), px(320.), px(200.)]);
    assert_eq!(widths_of(&cols), [px(160.), px(640.), px(400.)]);

    // Dragging the divider between Message and Location 60px left grows Location by 60px
    // so total width stays at 1200px with no trailing empty column.
    assert!(cols.apply_resized_widths(&[px(160.), px(580.), px(400.)]));
    assert_eq!(widths_of(&cols), [px(160.), px(580.), px(460.)]);

    // Dragging the last column's right edge inward clamps back to 1200px; dragging it
    // wider (> 1200px) preserves the wider width for horizontal scrolling.
    assert!(cols.apply_resized_widths(&[px(160.), px(580.), px(350.)]));
    assert_eq!(widths_of(&cols), [px(160.), px(580.), px(460.)]);
    assert!(!cols.apply_resized_widths(&[px(160.), px(580.), px(560.)]));
    assert_eq!(widths_of(&cols), [px(160.), px(580.), px(560.)]);

    // Sort and column order stay synchronized across refreshes.
    cols.set_sort(0, ColumnSort::Ascending);
    assert_eq!(cols[0].sort, Some(ColumnSort::Ascending));
    cols.move_column(0, 2);
    assert_eq!(cols[2].key.as_ref(), "level");
    assert_eq!(cols[2].sort, Some(ColumnSort::Ascending));
    assert_eq!(cols.natural_widths()[2], px(80.));

    // Non-divisible proportional scaling conserves exact sum and never shrinks any column.
    let scaled = scale_widths_proportionally(&[px(73.), px(211.), px(149.)], px(701.));
    let sum: Pixels = scaled.iter().copied().sum();
    assert_eq!(sum, px(701.));
    assert!(scaled[0] >= px(73.) && scaled[1] >= px(211.) && scaled[2] >= px(149.));
}

#[test]
fn test_date_and_flag_columns_remain_at_content_width_when_table_widens() {
    let widths_of =
        |cols: &TableColumns| -> Vec<Pixels> { cols.as_slice().iter().map(|c| c.width).collect() };

    let mut cols = TableColumns::new(vec![
        Column::new("date", "Date").width(px(90.)),
        Column::new("flag", "Flag").width(px(50.)),
        Column::new("description", "Description").width(px(300.)),
        Column::new("position", "Position").width(px(200.)),
    ]);
    assert!(!cols[0].resizable);
    assert!(!cols[1].resizable);
    assert!(cols[2].resizable);
    assert!(cols[3].resizable);
    assert_eq!(cols.natural_total_width(), px(640.));

    // When viewport widens to 1140px (+500px extra over 640px natural sum),
    // Date (90px) and Flag (50px) stay fixed at their content widths while
    // Description (300 -> 600) and Position (200 -> 400) scale proportionally.
    assert!(cols.fit_to_width(px(1140.)));
    assert_eq!(widths_of(&cols), [px(90.), px(50.), px(600.), px(400.)]);

    // Updating natural content widths keeps Date and Flag pinned to their new
    // content widths and redistributes remaining viewport width across scalable columns.
    assert!(cols.set_natural_widths(vec![px(96.), px(44.), px(400.), px(100.)]));
    assert_eq!(widths_of(&cols), [px(96.), px(44.), px(800.), px(200.)]);

    // Narrowing below natural sum keeps all columns at their content widths.
    assert!(cols.fit_to_width(px(500.)));
    assert_eq!(widths_of(&cols), [px(96.), px(44.), px(400.), px(100.)]);
}
