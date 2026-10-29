use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::test::{TestAppContextExt as _, TestQueryExt as _, TestWindowExt as _};
use gpui_kit::{
    AnyWindowHandle, AppContext as _, Bounds, Entity, TestAppContext, WindowBounds, WindowOptions,
    px, size,
};

use crate::LumiApp;
use crate::entry_editor::EntryEditor;
use crate::navigation::Page;
use crate::source::LocalLedger;

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../lumi/src/query_test.bean");

/// The last entry of the fixture, which the journal lists first.
const PAY_CARD: &str = "2025-06-01 * \"Pay card\"\n  Liabilities:CreditCard     80.00 USD\n  Assets:Bank:Checking      -80.00 USD";

/// A copy of the fixture this test may write to; removed when dropped.
struct TempLedger(PathBuf);

impl TempLedger {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "lumi-entry-editor-test-{}-{name}.bean",
            std::process::id()
        ));
        fs::copy(FIXTURE, &path).unwrap();
        Self(path)
    }

    fn path(&self) -> &str {
        self.0.to_str().unwrap()
    }

    fn text(&self) -> String {
        fs::read_to_string(&self.0).unwrap()
    }
}

impl Drop for TempLedger {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// The app in a window, on the journal of `ledger`, with its first page
/// loaded.
fn open_journal(
    ledger: &TempLedger,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<LumiApp>) {
    cx.update(crate::init);
    let path = ledger.path().to_string();
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
                        Arc::new(LocalLedger::new(path)),
                        "ledger.bean",
                        Page::Journal,
                        window,
                        cx,
                    )
                })
            },
        )
        .expect("open test window")
    });
    cx.run_until_parked();
    (handle, app)
}

#[gpui_kit::test]
async fn test_the_date_opens_the_entry_for_editing_and_save_writes_it_back(
    cx: &mut TestAppContext,
) {
    let ledger = TempLedger::new("save");
    let (handle, app) = open_journal(&ledger, cx);

    // The first row is the newest transaction; its date opens the editor,
    // which shows the entry's text once it has been read.
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(("edit", 0usize), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let text = window.find_by_label("Entry text");
        assert_eq!(text.value(), Some(PAY_CARD));
        assert_eq!(text.focused(), Some(true));
        assert_eq!(window.find("save-entry").disabled(), Some(true));

        // Typing changes the text and enables Save.
        window.press("end", cx);
        window.input(" #edited", cx);
        assert!(
            window
                .find_by_label("Entry text")
                .value()
                .is_some_and(|value| value.starts_with("2025-06-01 * \"Pay card\" #edited\n"))
        );
        assert_ne!(window.find("save-entry").disabled(), Some(true));
        window.click("save-entry", cx);
    })
    .unwrap();

    // Saving writes the file, reloads the ledger and closes the dialog.
    cx.wait_for(handle, Duration::from_secs(5), |_, cx| {
        app.read(cx).session().read(cx).revision() == 2
    })
    .await;
    cx.run_until_parked();
    let text = ledger.text();
    assert!(
        text.contains("2025-06-01 * \"Pay card\" #edited\n"),
        "{text}"
    );
    assert_eq!(
        text.lines().count(),
        fs::read_to_string(FIXTURE).unwrap().lines().count()
    );
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("save-entry").is_none());
        assert!(window.try_find_by_label("Entry text").is_none());
    })
    .unwrap();
    cx.read(|cx| {
        let report = app.read(cx).journal().expect("the journal is shown");
        let entries = report.read(cx).entries(cx);
        assert_eq!(entries[0].txn.tags(), &["#edited"]);
    });
}

#[gpui_kit::test]
async fn test_the_shortcut_saves_and_a_changed_file_is_not_overwritten(cx: &mut TestAppContext) {
    let ledger = TempLedger::new("shortcut");
    let (handle, app) = open_journal(&ledger, cx);
    let session = cx.read(|cx| app.read(cx).session().clone());
    let txn = cx.read(|cx| {
        let report = app.read(cx).journal().expect("the journal is shown");
        report.read(cx).entries(cx)[0].txn.clone()
    });
    #[cfg(target_os = "macos")]
    let save = "cmd-s";
    #[cfg(not(target_os = "macos"))]
    let save = "ctrl-s";

    // Opened directly, the editor reads the entry; the save shortcut
    // writes it like the button does.
    let editor = cx
        .update_window(handle, |_, window, cx| {
            EntryEditor::open(&txn, session.clone(), window, cx)
        })
        .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            editor.read(cx).original().map(|text| text.as_ref()),
            Some(PAY_CARD)
        );
        assert!(!editor.read(cx).is_changed(cx));
        window.press(save, cx);
        // Unchanged: nothing to save.
        assert!(!editor.read(cx).is_saving());
        window.press("end", cx);
        window.input(" #edited", cx);
        assert!(editor.read(cx).is_changed(cx));
        window.press(save, cx);
        assert!(editor.read(cx).is_saving());
    })
    .unwrap();
    cx.wait_for(handle, Duration::from_secs(5), |_, cx| {
        session.read(cx).revision() == 2
    })
    .await;
    assert!(
        ledger
            .text()
            .contains("2025-06-01 * \"Pay card\" #edited\n")
    );

    // A second editor on the same entry; the file changes under it, so its
    // save is refused and it stays open with the reason.
    let txn = cx.read(|cx| {
        let report = app.read(cx).journal().expect("the journal is shown");
        report.read(cx).entries(cx)[0].txn.clone()
    });
    let editor = cx
        .update_window(handle, |_, window, cx| {
            EntryEditor::open(&txn, session.clone(), window, cx)
        })
        .unwrap();
    cx.run_until_parked();
    let before = ledger
        .text()
        .replace("Pay card\" #edited", "Pay card\" #elsewhere");
    fs::write(ledger.path(), &before).unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.press("end", cx);
        window.input(" #again", cx);
        window.click("save-entry", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let editor = editor.read(cx);
        assert!(!editor.is_saving());
        assert!(
            editor
                .error()
                .is_some_and(|error| error.contains("changed since it was read")),
            "{:?}",
            editor.error()
        );
        assert!(window.try_find("save-entry").is_some());
    })
    .unwrap();
    assert_eq!(ledger.text(), before);
    cx.read(|cx| assert_eq!(session.read(cx).revision(), 2));
}

#[gpui_kit::test]
fn test_the_context_menu_and_a_balance_row_offer_editing(cx: &mut TestAppContext) {
    let ledger = TempLedger::new("menu");
    let (handle, app) = open_journal(&ledger, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        // The balance directive is the fourth newest entry.
        let report = app.read(cx).journal().expect("the journal is shown");
        let entries = report.read(cx).entries(cx);
        let balance_ix = entries
            .iter()
            .position(|entry| entry.txn.flag() == lumi::TxnFlag::Balance)
            .expect("the fixture has a balance directive");
        window.click(("edit", balance_ix), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.find_by_label("Entry text").value(),
            Some("2024-03-15 balance Assets:Cash 54.50 USD")
        );
        window.click("cancel", cx);
        assert!(window.try_find_by_label("Entry text").is_none());
    })
    .unwrap();
}
