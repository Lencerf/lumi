//! The lumi user interface, built on GPUI Kit.
//!
//! The crate holds everything a native application and a web application
//! share: the [`LumiApp`] window, its reports, and the [`source::LedgerSource`]
//! seam through which they read the ledger. A shell supplies the platform
//! entry point, an asset source, and a source for the ledger, then opens one
//! window with [`LumiApp`].

mod app;
mod assets;
mod completion;
mod entry_editor;
mod errors_report;
mod highlight;
mod holdings_report;
mod journal_report;
mod menu;
mod navigation;
mod session;
pub mod source;
mod style;
mod trie_report;

pub use app::{
    CloseWindow, GoBack, Hide, HideOthers, LumiApp, Minimize, OpenLedger, PathOpener, Quit,
    Refresh, ShowAll, ShowBalanceSheet, ShowErrors, ShowHoldings, ShowIncome, ShowJournal,
    ToggleFullScreen, Zoom, ledger_name_for_path,
};
pub use assets::ExtraIcons;
#[cfg(not(target_family = "wasm"))]
pub use assets::LumiAssets;
pub use entry_editor::{CancelEntry, EntryEditor, SaveEntry};
pub use errors_report::ErrorsReport;
pub use holdings_report::HoldingsReport;
pub use journal_report::{
    ActivateRow, EditEntry, JournalReport, OpenAccount, RECORD_TYPES, ShowInContext, TogglePostings,
};
pub use navigation::{Page, ReportEvent};
pub use session::LedgerSession;
pub use trie_report::TrieReport;

pub use gpui_kit::component::input::Search;

use gpui_kit::component::input::ShowCompletions;
use gpui_kit::{App, KeyBinding};

/// Initializes GPUI Kit, registers key bindings, and installs the application
/// menu bar. Call it once, before opening a window.
pub fn init(cx: &mut App) {
    cx.set_app_identity("io.github.lencerf.lumi", "Lumi");
    gpui_kit::init(cx);
    style::configure_theme(cx);
    highlight::register_language(cx);

    #[cfg(target_os = "macos")]
    cx.bind_keys([
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-w", CloseWindow, None),
        KeyBinding::new("cmd-h", Hide, None),
        KeyBinding::new("alt-cmd-h", HideOthers, None),
        KeyBinding::new("cmd-m", Minimize, None),
        KeyBinding::new("ctrl-cmd-f", ToggleFullScreen, None),
        KeyBinding::new("cmd-f", Search, Some(app::KEY_CONTEXT)),
        KeyBinding::new("cmd-f", Search, Some(journal_report::KEY_CONTEXT)),
        KeyBinding::new("cmd-f", Search, Some(holdings_report::KEY_CONTEXT)),
        KeyBinding::new("cmd-[", GoBack, Some(app::KEY_CONTEXT)),
        KeyBinding::new("cmd-o", OpenLedger, Some(app::KEY_CONTEXT)),
        KeyBinding::new("cmd-r", Refresh, Some(app::KEY_CONTEXT)),
        KeyBinding::new("cmd-1", ShowBalanceSheet, Some(app::KEY_CONTEXT)),
        KeyBinding::new("cmd-2", ShowIncome, Some(app::KEY_CONTEXT)),
        KeyBinding::new("cmd-3", ShowJournal, Some(app::KEY_CONTEXT)),
        KeyBinding::new("cmd-4", ShowHoldings, Some(app::KEY_CONTEXT)),
        KeyBinding::new("cmd-5", ShowErrors, Some(app::KEY_CONTEXT)),
        KeyBinding::new("cmd-s", SaveEntry, Some(entry_editor::KEY_CONTEXT)),
    ]);

    #[cfg(not(target_os = "macos"))]
    cx.bind_keys([
        KeyBinding::new("ctrl-q", Quit, None),
        KeyBinding::new("ctrl-w", CloseWindow, None),
        KeyBinding::new("f11", ToggleFullScreen, None),
        KeyBinding::new("ctrl-f", Search, Some(app::KEY_CONTEXT)),
        KeyBinding::new("ctrl-f", Search, Some(journal_report::KEY_CONTEXT)),
        KeyBinding::new("ctrl-f", Search, Some(holdings_report::KEY_CONTEXT)),
        KeyBinding::new("alt-left", GoBack, Some(app::KEY_CONTEXT)),
        KeyBinding::new("ctrl-o", OpenLedger, Some(app::KEY_CONTEXT)),
        KeyBinding::new("ctrl-r", Refresh, Some(app::KEY_CONTEXT)),
        KeyBinding::new("alt-1", ShowBalanceSheet, Some(app::KEY_CONTEXT)),
        KeyBinding::new("alt-2", ShowIncome, Some(app::KEY_CONTEXT)),
        KeyBinding::new("alt-3", ShowJournal, Some(app::KEY_CONTEXT)),
        KeyBinding::new("alt-4", ShowHoldings, Some(app::KEY_CONTEXT)),
        KeyBinding::new("alt-5", ShowErrors, Some(app::KEY_CONTEXT)),
        KeyBinding::new("ctrl-s", SaveEntry, Some(entry_editor::KEY_CONTEXT)),
    ]);

    cx.bind_keys([
        KeyBinding::new("escape", CancelEntry, Some(entry_editor::KEY_CONTEXT)),
        KeyBinding::new(
            "ctrl-space",
            ShowCompletions,
            Some(entry_editor::KEY_CONTEXT),
        ),
        KeyBinding::new("enter", ActivateRow, Some(journal_report::KEY_CONTEXT)),
        KeyBinding::new("enter", ActivateRow, Some(holdings_report::KEY_CONTEXT)),
    ]);

    menu::init(cx);
}
