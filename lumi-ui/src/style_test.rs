use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{Styled as _, TestAppContext, px};

use crate::style::table_card;

#[gpui_kit::test]
fn test_configure_theme_removes_table_row_border(cx: &mut TestAppContext) {
    cx.update(crate::init);
    cx.read(|cx| {
        assert_eq!(cx.theme().table_row_border, cx.theme().transparent);
    });
}

#[gpui_kit::test]
fn test_table_card_has_sharp_corners_and_border(cx: &mut TestAppContext) {
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
    });
}
