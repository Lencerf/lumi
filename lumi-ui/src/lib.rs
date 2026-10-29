//! The lumi user interface, built on GPUI Kit.
//!
//! The crate holds everything a native application and a web application
//! share: the [`LumiApp`] window, its reports, and the [`source::LedgerSource`]
//! seam through which they read the ledger. A shell supplies the platform
//! entry point, an asset source, and a source for the ledger, then opens one
//! window with [`LumiApp`].

mod app;
mod assets;
mod entry_editor;
mod errors_report;
mod highlight;
mod holdings_report;
mod journal_report;
mod navigation;
mod session;
pub mod source;
mod style;
mod trie_report;

pub use app::{GoBack, LumiApp, OpenLedger, PathOpener, Refresh, ledger_name_for_path};
pub use assets::ExtraIcons;
#[cfg(not(target_family = "wasm"))]
pub use assets::LumiAssets;
pub use entry_editor::{EntryEditor, SaveEntry};
pub use errors_report::ErrorsReport;
pub use holdings_report::HoldingsReport;
pub use journal_report::{ActivateRow, EditEntry, JournalReport, OpenAccount, TogglePostings};
pub use navigation::{Page, ReportEvent};
pub use session::LedgerSession;
pub use trie_report::TrieReport;

use gpui_kit::{App, KeyBinding};

/// Initializes GPUI Kit and registers the key bindings of the lumi views.
/// Call it once, before opening a window.
pub fn init(cx: &mut App) {
    gpui_kit::init(cx);
    highlight::register_language(cx);
    #[cfg(target_os = "macos")]
    let (back, open, refresh, save) = ("cmd-[", "cmd-o", "cmd-r", "cmd-s");
    #[cfg(not(target_os = "macos"))]
    let (back, open, refresh, save) = ("alt-left", "ctrl-o", "ctrl-r", "ctrl-s");
    cx.bind_keys([
        KeyBinding::new(back, GoBack, Some(app::KEY_CONTEXT)),
        KeyBinding::new(open, OpenLedger, Some(app::KEY_CONTEXT)),
        KeyBinding::new(refresh, Refresh, Some(app::KEY_CONTEXT)),
        KeyBinding::new(save, SaveEntry, Some(entry_editor::KEY_CONTEXT)),
        KeyBinding::new("enter", ActivateRow, Some(journal_report::KEY_CONTEXT)),
        KeyBinding::new("enter", ActivateRow, Some(holdings_report::KEY_CONTEXT)),
    ]);
}
