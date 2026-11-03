use std::sync::Arc;

use gpui_kit::component::GlobalState;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{
    Action as _, AppContext as _, Bounds, Keystroke, OwnedMenu, OwnedMenuItem, TestAppContext,
    WindowBackgroundAppearance, WindowBounds, WindowDecorations, WindowOptions, px, size,
};

use crate::app::{
    CloseWindow, GoBack, GoForward, LumiApp, Quit, ShowBalanceSheet, ShowEditor, ShowErrors,
    ShowHoldings, ShowIncome, ShowJournal,
};
use crate::navigation::Page;
use crate::source::LocalLedger;

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../lumi/src/query_test.bean");

fn action_names(menu: &OwnedMenu) -> Vec<&str> {
    menu.items
        .iter()
        .filter_map(|item| match item {
            OwnedMenuItem::Action { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect()
}

#[gpui_kit::test]
fn test_init_installs_application_menus_and_quit_binding(cx: &mut TestAppContext) {
    cx.update(crate::init);
    cx.read(|cx| {
        let menus = cx.get_menus().expect("menus are installed");
        let names: Vec<&str> = menus.iter().map(|menu| menu.name.as_ref()).collect();
        let app_menu_names: Vec<&str> = GlobalState::global(cx)
            .app_menus()
            .iter()
            .map(|menu| menu.name.as_ref())
            .collect();
        assert_eq!(app_menu_names, names);

        #[cfg(target_os = "macos")]
        {
            assert_eq!(names, ["Lumi", "File", "Edit", "View", "Window"]);
            let app_items = action_names(&menus[0]);
            assert!(app_items.contains(&"Quit Lumi"));
            assert!(app_items.contains(&"Hide Lumi"));
            let edit_items = action_names(&menus[2]);
            assert!(edit_items.contains(&"Find…"));
            let view_items = action_names(&menus[3]);
            assert!(view_items.contains(&"Back"));
            assert!(view_items.contains(&"Forward"));
        }

        #[cfg(not(target_os = "macos"))]
        {
            assert_eq!(names, ["File", "Edit", "View", "Window"]);
            let file_items = action_names(&menus[0]);
            assert!(file_items.contains(&"Close Window"));
            assert!(file_items.contains(&"Quit Lumi"));
            let edit_items = action_names(&menus[1]);
            assert!(edit_items.contains(&"Find…"));
            let view_items = action_names(&menus[2]);
            assert!(view_items.contains(&"Back"));
            assert!(view_items.contains(&"Forward"));
        }

        #[cfg(target_os = "macos")]
        let (quit_key, close_key, back_key, forward_key) = ("cmd-q", "cmd-w", "cmd-[", "cmd-]");
        #[cfg(not(target_os = "macos"))]
        let (quit_key, close_key, back_key, forward_key) =
            ("ctrl-q", "ctrl-w", "alt-left", "alt-right");

        let quit_bindings = cx.all_bindings_for_input(&[Keystroke::parse(quit_key).unwrap()]);
        assert_eq!(quit_bindings.len(), 1);
        assert!(quit_bindings[0].action().partial_eq(&Quit));

        let close_bindings = cx.all_bindings_for_input(&[Keystroke::parse(close_key).unwrap()]);
        assert_eq!(close_bindings.len(), 1);
        assert!(close_bindings[0].action().partial_eq(&CloseWindow));

        let back_bindings = cx.all_bindings_for_input(&[Keystroke::parse(back_key).unwrap()]);
        assert_eq!(back_bindings.len(), 1);
        assert!(back_bindings[0].action().partial_eq(&GoBack));

        let forward_bindings = cx.all_bindings_for_input(&[Keystroke::parse(forward_key).unwrap()]);
        assert_eq!(forward_bindings.len(), 1);
        assert!(forward_bindings[0].action().partial_eq(&GoForward));
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

#[cfg(not(any(target_os = "macos", target_family = "wasm")))]
#[gpui_kit::test]
fn test_window_options_and_in_window_title_and_menu_bar(cx: &mut TestAppContext) {
    let bounds = WindowBounds::Windowed(Bounds::new(Default::default(), size(px(1200.), px(800.))));
    let options = LumiApp::window_options("Lumi".into(), bounds);
    assert!(options.app_owns_titlebar_drag);
    assert_eq!(options.app_id.as_deref(), Some("io.github.lencerf.lumi"));
    let titlebar = options.titlebar.as_ref().expect("titlebar options");
    assert_eq!(titlebar.title.as_deref(), Some("Lumi"));
    assert!(titlebar.appears_transparent);
    #[cfg(target_os = "linux")]
    {
        assert_eq!(options.window_decorations, Some(WindowDecorations::Client));
        assert!(matches!(
            options.window_background,
            WindowBackgroundAppearance::Transparent
        ));
    }

    cx.update(crate::init);
    let (handle, app) = cx.update(|cx| {
        gpui_kit::open_window(options, cx, |window, cx| {
            cx.new(|cx| {
                LumiApp::new(
                    Arc::new(LocalLedger::new(FIXTURE)),
                    "query_test.bean",
                    Page::BalanceSheet,
                    window,
                    cx,
                )
            })
        })
        .expect("open test window")
    });
    cx.run_until_parked();

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("title-bar").is_some());
        assert!(window.try_find("window-controls").is_some());
        assert_eq!(
            window.find("window-title").label(),
            Some("query_test.bean — Lumi")
        );

        let labels: Vec<String> = (0usize..4)
            .map(|ix| {
                window
                    .within("app-menu-bar")
                    .within(ix)
                    .find("menu")
                    .label()
                    .unwrap()
                    .to_string()
            })
            .collect();
        assert_eq!(labels, ["File", "Edit", "View", "Window"]);

        // Clicking View -> Journal dispatches ShowJournal through LumiApp's
        // preserved focus context.
        window
            .within("app-menu-bar")
            .within(2usize)
            .click("menu", cx);
        assert_eq!(
            window.within("popup-menu").find(2usize).label(),
            Some("Journal")
        );
        window.within("popup-menu").click(2usize, cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.read(|cx| assert_eq!(app.read(cx).page(), &Page::Journal));

    // Clicking File -> Close Window closes the active window.
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window
            .within("app-menu-bar")
            .within(0usize)
            .click("menu", cx);
        assert_eq!(
            window.within("popup-menu").find(3usize).label(),
            Some("Close Window")
        );
        window.within("popup-menu").click(3usize, cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.read(|cx| assert!(cx.windows().is_empty()));
}
