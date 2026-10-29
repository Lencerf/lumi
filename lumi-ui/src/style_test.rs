use gpui_kit::TestAppContext;
use gpui_kit::component::ActiveTheme as _;

#[gpui_kit::test]
fn test_configure_theme_removes_table_row_border(cx: &mut TestAppContext) {
    cx.update(crate::init);
    cx.read(|cx| {
        assert_eq!(cx.theme().table_row_border, cx.theme().transparent);
    });
}
