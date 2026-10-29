use gpui_kit::component::input::{Copy, Cut, Paste, Redo, Search, SelectAll, Undo};
use gpui_kit::{AnyWindowHandle, App, Menu, MenuItem, OsAction, SystemMenuType};

use crate::app::{
    CloseWindow, GoBack, Hide, HideOthers, Minimize, OpenLedger, Quit, Refresh, ShowAll,
    ShowBalanceSheet, ShowErrors, ShowHoldings, ShowIncome, ShowJournal, ToggleFullScreen, Zoom,
};

fn target_window(cx: &App) -> Option<AnyWindowHandle> {
    cx.active_window().or_else(|| cx.windows().first().copied())
}

/// Registers application-level window/quit action handlers and installs the
/// native menu bar. Must run after [`App::bind_keys`] so menu items pick up
/// their configured keyboard shortcuts.
pub(crate) fn init(cx: &mut App) {
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.on_action(|_: &Hide, cx| cx.hide());
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
    cx.on_action(|_: &CloseWindow, cx| {
        if let Some(window) = target_window(cx) {
            window
                .update(cx, |_, window, _| window.remove_window())
                .ok();
        }
    });
    cx.on_action(|_: &Minimize, cx| {
        if let Some(window) = target_window(cx) {
            window
                .update(cx, |_, window, _| window.minimize_window())
                .ok();
        }
    });
    cx.on_action(|_: &Zoom, cx| {
        if let Some(window) = target_window(cx) {
            window.update(cx, |_, window, _| window.zoom_window()).ok();
        }
    });
    cx.on_action(|_: &ToggleFullScreen, cx| {
        if let Some(window) = target_window(cx) {
            window
                .update(cx, |_, window, _| window.toggle_fullscreen())
                .ok();
        }
    });

    cx.set_menus(app_menus());
}

fn app_menus() -> [Menu; 5] {
    [
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
        ]),
        Menu::new("View").items([
            MenuItem::action("Balance Sheet", ShowBalanceSheet),
            MenuItem::action("Income Statement", ShowIncome),
            MenuItem::action("Journal", ShowJournal),
            MenuItem::action("Holdings", ShowHoldings),
            MenuItem::action("Errors", ShowErrors),
            MenuItem::separator(),
            MenuItem::action("Back", GoBack),
            MenuItem::separator(),
            MenuItem::action("Toggle Full Screen", ToggleFullScreen),
        ]),
        Menu::new("Window").items([
            MenuItem::action("Minimize", Minimize),
            MenuItem::action("Zoom", Zoom),
        ]),
    ]
}

#[cfg(test)]
#[path = "menu_test.rs"]
mod tests;
