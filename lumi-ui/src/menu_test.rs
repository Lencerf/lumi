use std::sync::Arc;

use gpui_kit::{
    Action as _, AppContext as _, Bounds, Keystroke, OwnedMenuItem, TestAppContext, WindowBounds,
    WindowOptions, px, size,
};

use crate::app::{
    CloseWindow, LumiApp, Quit, ShowBalanceSheet, ShowEditor, ShowErrors, ShowHoldings, ShowIncome,
    ShowJournal,
};
use crate::navigation::Page;
use crate::source::LocalLedger;

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../lumi/src/query_test.bean");

#[gpui_kit::test]
fn test_init_installs_application_menus_and_quit_binding(cx: &mut TestAppContext) {
    cx.update(crate::init);
    cx.read(|cx| {
        let menus = cx.get_menus().expect("menus are installed");
        let names: Vec<&str> = menus.iter().map(|menu| menu.name.as_ref()).collect();
        assert_eq!(names, ["Lumi", "File", "Edit", "View", "Window"]);

        let app_items: Vec<&str> = menus[0]
            .items
            .iter()
            .filter_map(|item| match item {
                OwnedMenuItem::Action { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .collect();
        assert!(app_items.contains(&"Quit Lumi"));
        assert!(app_items.contains(&"Hide Lumi"));

        let edit_items: Vec<&str> = menus[2]
            .items
            .iter()
            .filter_map(|item| match item {
                OwnedMenuItem::Action { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .collect();
        assert!(edit_items.contains(&"Find…"));

        #[cfg(target_os = "macos")]
        let (quit_key, close_key) = ("cmd-q", "cmd-w");
        #[cfg(not(target_os = "macos"))]
        let (quit_key, close_key) = ("ctrl-q", "ctrl-w");

        let quit_bindings = cx.all_bindings_for_input(&[Keystroke::parse(quit_key).unwrap()]);
        assert_eq!(quit_bindings.len(), 1);
        assert!(quit_bindings[0].action().partial_eq(&Quit));

        let close_bindings = cx.all_bindings_for_input(&[Keystroke::parse(close_key).unwrap()]);
        assert_eq!(close_bindings.len(), 1);
        assert!(close_bindings[0].action().partial_eq(&CloseWindow));
    });

    cx.update(|cx| {
        assert!(cx.is_action_available(&Quit));
        assert!(cx.is_action_available(&CloseWindow));
        // Dispatching Quit reaches the global listener without panicking.
        cx.dispatch_action(&Quit);
    });
}

#[gpui_kit::test]
fn test_view_actions_and_close_window_operate_on_the_active_window(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let (handle, app) = cx.update(|cx| {
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    Default::default(),
                    size(px(1200.), px(800.)),
                ))),
                ..Default::default()
            },
            cx,
            |window, cx| {
                cx.new(|cx| {
                    LumiApp::new(
                        Arc::new(LocalLedger::new(FIXTURE)),
                        "query_test.bean",
                        Page::BalanceSheet,
                        window,
                        cx,
                    )
                })
            },
        )
        .expect("open test window")
    });
    cx.run_until_parked();

    for (action, expected) in [
        (ShowIncome.boxed_clone(), Page::Income),
        (ShowJournal.boxed_clone(), Page::Journal),
        (ShowHoldings.boxed_clone(), Page::Holdings),
        (ShowErrors.boxed_clone(), Page::Errors),
        (ShowEditor.boxed_clone(), Page::Editor),
        (ShowBalanceSheet.boxed_clone(), Page::BalanceSheet),
    ] {
        cx.update_window(handle, |_, window, cx| {
            window.dispatch_action(action, cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.read(|cx| assert_eq!(app.read(cx).page(), &expected));
    }

    cx.read(|cx| assert_eq!(cx.windows().len(), 1));
    cx.update(|cx| cx.dispatch_action(&CloseWindow));
    cx.run_until_parked();
    cx.read(|cx| assert_eq!(cx.windows().len(), 0));
}
