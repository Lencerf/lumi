use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{FontWeight, Styled as _, TestAppContext, px};

use crate::style::{table_card, table_header, table_row_bg};

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
    });
}
