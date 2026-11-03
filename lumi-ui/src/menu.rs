#[cfg(target_os = "macos")]
use gpui_kit::SystemMenuType;
use gpui_kit::component::GlobalState;
use gpui_kit::component::input::{Copy, Cut, Paste, Redo, Search, SelectAll, Undo};
use gpui_kit::{AnyWindowHandle, App, Menu, MenuItem, OsAction};

use crate::app::{
    CloseWindow, GoBack, GoForward, Hide, HideOthers, Minimize, OpenLedger, Quit, Refresh, ShowAll,
    ShowBalanceSheet, ShowEditor, ShowErrors, ShowHoldings, ShowIncome, ShowJournal,
    ToggleFullScreen, Zoom,
};

fn target_window(cx: &App) -> Option<AnyWindowHandle> {
    cx.active_window().or_else(|| cx.windows().first().copied())
}

fn with_target_window(cx: &mut App, f: fn(&mut gpui_kit::Window)) {
    let Some(window) = target_window(cx) else {
        return;
    };
    if window.update(cx, |_, window, _| f(window)).is_err() {
        cx.defer(move |cx| {
            window.update(cx, |_, window, _| f(window)).ok();
        });
    }
}

/// Registers application-level window/quit action handlers and installs both
/// the native menu bar (macOS) and the in-window [`AppMenuBar`] menu list
/// (Windows and Linux). Must run after [`App::bind_keys`] so menu items pick up
/// their configured keyboard shortcuts.
pub(crate) fn init(cx: &mut App) {
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.on_action(|_: &Hide, cx| cx.hide());
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
    cx.on_action(|_: &CloseWindow, cx| with_target_window(cx, |window| window.remove_window()));
    cx.on_action(|_: &Minimize, cx| with_target_window(cx, |window| window.minimize_window()));
    cx.on_action(|_: &Zoom, cx| with_target_window(cx, |window| window.zoom_window()));
    cx.on_action(|_: &ToggleFullScreen, cx| {
        with_target_window(cx, |window| window.toggle_fullscreen())
    });

    GlobalState::global_mut(cx).set_app_menus(app_menus().into_iter().map(Menu::owned).collect());
    cx.set_menus(app_menus());
}

#[cfg(target_os = "macos")]
fn app_menus() -> Vec<Menu> {
    vec![
        Menu::new("Lumi").items([
            MenuItem::os_submenu("Services", SystemMenuType::Services),
            MenuItem::separator(),
            MenuItem::action("Hide Lumi", Hide),
            MenuItem::action("Hide Others", HideOthers),
            MenuItem::action("Show All", ShowAll),
            MenuItem::separator(),
            MenuItem::action("Quit Lumi", Quit),
        ]),
        Menu::new("File").items([
            MenuItem::action("Open Ledger…", OpenLedger),
            MenuItem::action("Reload Ledger", Refresh),
            MenuItem::separator(),
            MenuItem::action("Close Window", CloseWindow),
        ]),
        edit_menu(),
        view_menu(),
        Menu::new("Window").items([
            MenuItem::action("Minimize", Minimize),
            MenuItem::action("Zoom", Zoom),
        ]),
    ]
}

#[cfg(not(target_os = "macos"))]
fn app_menus() -> Vec<Menu> {
    vec![
        Menu::new("File").items([
            MenuItem::action("Open Ledger…", OpenLedger),
            MenuItem::action("Reload Ledger", Refresh),
            MenuItem::separator(),
            MenuItem::action("Close Window", CloseWindow),
            MenuItem::action("Quit Lumi", Quit),
        ]),
        edit_menu(),
        view_menu(),
        Menu::new("Window").items([
            MenuItem::action("Minimize", Minimize),
            MenuItem::action("Maximize", Zoom),
        ]),
    ]
}

fn edit_menu() -> Menu {
    Menu::new("Edit").items([
        MenuItem::os_action("Undo", Undo, OsAction::Undo),
        MenuItem::os_action("Redo", Redo, OsAction::Redo),
        MenuItem::separator(),
        MenuItem::os_action("Cut", Cut, OsAction::Cut),
        MenuItem::os_action("Copy", Copy, OsAction::Copy),
        MenuItem::os_action("Paste", Paste, OsAction::Paste),
        MenuItem::os_action("Select All", SelectAll, OsAction::SelectAll),
        MenuItem::separator(),
        MenuItem::action("Find…", Search),
    ])
}

fn view_menu() -> Menu {
    Menu::new("View").items([
        MenuItem::action("Balance Sheet", ShowBalanceSheet),
        MenuItem::action("Income Statement", ShowIncome),
        MenuItem::action("Journal", ShowJournal),
        MenuItem::action("Holdings", ShowHoldings),
        MenuItem::action("Errors", ShowErrors),
        MenuItem::action("Editor", ShowEditor),
        MenuItem::separator(),
        MenuItem::action("Back", GoBack),
        MenuItem::action("Forward", GoForward),
        MenuItem::separator(),
        MenuItem::action("Toggle Full Screen", ToggleFullScreen),
    ])
}

#[cfg(test)]
#[path = "menu_test.rs"]
mod tests;
