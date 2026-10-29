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

#[gpui_kit::test]
async fn test_delete_removes_the_entry_and_reloads_the_ledger(cx: &mut TestAppContext) {
    let ledger = TempLedger::new("delete");
    let (handle, app) = open_journal(&ledger, cx);
    cx.read(|cx| {
        let report = app.read(cx).journal().expect("the journal is shown");
        assert_eq!(report.read(cx).total(cx), 9);
    });

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(("edit", 0usize), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_ne!(window.find("delete-entry").disabled(), Some(true));
        window.click("delete-entry", cx);
    })
    .unwrap();

    cx.wait_for(handle, Duration::from_secs(5), |_, cx| {
        app.read(cx).session().read(cx).revision() == 2
    })
    .await;
    cx.run_until_parked();
    assert!(!ledger.text().contains("Pay card"));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("delete-entry").is_none());
    })
    .unwrap();
    cx.read(|cx| {
        let report = app.read(cx).journal().expect("the journal is shown");
        assert_eq!(report.read(cx).total(cx), 8);
        assert_ne!(report.read(cx).entries(cx)[0].txn.narration(), "Pay card");
    });
}

#[gpui_kit::test]
async fn test_duplicate_dates_the_copy_today_and_appends_on_save(cx: &mut TestAppContext) {
    let ledger = TempLedger::new("duplicate");
    let (handle, app) = open_journal(&ledger, cx);
    let expected = lumi::edit::with_today(PAY_CARD);

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(("edit", 0usize), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("duplicate-entry", cx);
    })
    .unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.find_by_label("Entry text").value(),
            Some(expected.as_str())
        );
        // Save is enabled right away for the new copy, and Delete is gone.
        assert_ne!(window.find("save-entry").disabled(), Some(true));
        assert!(window.try_find("delete-entry").is_none());
        assert!(window.try_find("duplicate-entry").is_none());
        window.click("save-entry", cx);
    })
    .unwrap();

    cx.wait_for(handle, Duration::from_secs(5), |_, cx| {
        app.read(cx).session().read(cx).revision() == 2
    })
    .await;
    cx.run_until_parked();
    let text = ledger.text();
    assert!(text.contains(PAY_CARD), "the original entry remains");
    assert!(text.ends_with(&format!("{expected}\n")), "{text}");
    cx.read(|cx| {
        let report = app.read(cx).journal().expect("the journal is shown");
        assert_eq!(report.read(cx).total(cx), 10);
    });
}

#[gpui_kit::test]
fn test_entry_editor_configures_beancount_code_editor(cx: &mut TestAppContext) {
    let ledger = TempLedger::new("highlight");
    let (handle, app) = open_journal(&ledger, cx);
    let session = cx.read(|cx| app.read(cx).session().clone());
    let txn = cx.read(|cx| {
        let report = app.read(cx).journal().expect("the journal is shown");
        report.read(cx).entries(cx)[0].txn.clone()
    });

    let editor = cx
        .update_window(handle, |_, window, cx| {
            EntryEditor::open(&txn, session, window, cx)
        })
        .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let state = editor.read(cx).editor_state().read(cx);
        assert!(state.is_code_editor());
        assert_eq!(state.language_name(), "beancount");
    })
    .unwrap();
}

#[gpui_kit::test]
fn test_account_and_currency_completions_insert_and_escape_keeps_dialog_open(
    cx: &mut TestAppContext,
) {
    let ledger = TempLedger::new("acct-curr-complete");
    let (handle, app) = open_journal(&ledger, cx);
    let session = cx.read(|cx| app.read(cx).session().clone());
    let txn = cx.read(|cx| {
        let report = app.read(cx).journal().expect("the journal is shown");
        report.read(cx).entries(cx)[0].txn.clone()
    });

    let editor = cx
        .update_window(handle, |_, window, cx| {
            EntryEditor::open(&txn, session, window, cx)
        })
        .unwrap();
    cx.run_until_parked();

    // Move to the end of the entry, add a new indented posting line, and type
    // a colon-segment abbreviation for Expenses:Food:Groceries.
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let state = editor.read(cx).editor_state().clone();
        state.update(cx, |state, cx| {
            state.set_value("2025-06-01 * \"Test\"\n  ", window, cx);
            let end = state.text().len();
            state.set_selected_range(end..end, cx);
            state.focus(window, cx);
        });
        window.render_frame(cx);
        window.input("e:f:g", cx);
    })
    .unwrap();
    cx.run_until_parked();

    // Render the completion popup and confirm the selected account with Enter.
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            editor
                .read(cx)
                .editor_state()
                .read(cx)
                .completion_menu_state()
                .open
        );
        window.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();

    // Type an amount and space to open currency completion, select EUR (2nd),
    // then also verify Escape on an open menu closes only the menu, not the dialog.
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.find_by_label("Entry text").value(),
            Some("2025-06-01 * \"Test\"\n  Expenses:Food:Groceries")
        );
        window.input("  12.50 ", cx);
    })
    .unwrap();
    cx.run_until_parked();

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            editor
                .read(cx)
                .editor_state()
                .read(cx)
                .completion_menu_state()
                .open
        );
        window.press("escape", cx);
    })
    .unwrap();
    cx.run_until_parked();

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        // The completion menu closed, while the EntryEditor dialog remained open.
        assert!(
            !editor
                .read(cx)
                .editor_state()
                .read(cx)
                .completion_menu_state()
                .open
        );
        assert!(window.try_find("save-entry").is_some());

        // Type `E` to reopen currency completion filtered to EUR and confirm.
        window.input("E", cx);
    })
    .unwrap();
    cx.run_until_parked();

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.find_by_label("Entry text").value(),
            Some("2025-06-01 * \"Test\"\n  Expenses:Food:Groceries  12.50 EUR")
        );
        // With no completion menu open, Escape closes the dialog as usual.
        window.press("escape", cx);
        assert!(window.try_find_by_label("Entry text").is_none());
    })
    .unwrap();
}
