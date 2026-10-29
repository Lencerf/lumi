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
use crate::app::ShowJournal;
use crate::navigation::Page;
use crate::source::LocalLedger;

struct MultiFileLedger {
    dir: PathBuf,
    main_path: PathBuf,
    accounts_path: PathBuf,
    expenses_path: PathBuf,
}

impl MultiFileLedger {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "lumi-source-editor-test-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("2025")).unwrap();

        let main_path = dir.join("main.bean");
        let accounts_path = dir.join("accounts.bean");
        let expenses_path = dir.join("2025").join("expenses.bean");

        fs::write(
            &accounts_path,
            "2024-01-01 open Assets:Cash USD\n2024-01-01 open Expenses:Food:Coffee USD\n",
        )
        .unwrap();
        fs::write(
            &expenses_path,
            "2025-01-10 * \"Blue Bottle\" \"Latte\" #coffee\n  Expenses:Food:Coffee   6.50 USD\n  Assets:Cash           -6.50 USD\n",
        )
        .unwrap();
        fs::write(
            &main_path,
            "option \"title\" \"Multi-file ledger\"\noption \"operating_currency\" \"USD\"\n\ninclude \"accounts.bean\"\ninclude \"2025/expenses.bean\"\n",
        )
        .unwrap();

        Self {
            dir,
            main_path,
            accounts_path,
            expenses_path,
        }
    }

    fn main_str(&self) -> String {
        self.main_path.to_str().unwrap().to_string()
    }

    fn accounts_str(&self) -> String {
        self.accounts_path.to_str().unwrap().to_string()
    }

    fn expenses_str(&self) -> String {
        self.expenses_path.to_str().unwrap().to_string()
    }
}

impl Drop for MultiFileLedger {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn open_editor(
    ledger: &MultiFileLedger,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<LumiApp>) {
    cx.update(crate::init);
    let root = ledger.main_str();
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
                        Arc::new(LocalLedger::new(root)),
                        "main.bean",
                        Page::Editor,
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
fn test_source_editor_dropdown_lists_all_included_files_and_preserves_unsaved_drafts(
    cx: &mut TestAppContext,
) {
    let ledger = MultiFileLedger::new("dropdown-drafts");
    let (handle, app) = open_editor(&ledger, cx);

    let main_expected = fs::read_to_string(&ledger.main_path).unwrap();
    let expenses_expected = fs::read_to_string(&ledger.expenses_path).unwrap();

    // Root file is selected first, and all included files are listed in the dropdown.
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("source-file-select").value(), Some("main.bean"));
        assert_eq!(
            window.find_by_label("Source text").value(),
            Some(main_expected.as_str())
        );
        assert_eq!(window.find("save-source").disabled(), Some(true));
        assert_eq!(window.find("revert-source").disabled(), Some(true));
    })
    .unwrap();
    cx.read(|cx| {
        let editor = app.read(cx).editor().expect("editor page is open");
        let files: Vec<String> = editor
            .read(cx)
            .files()
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(
            files,
            [
                ledger.main_str(),
                ledger.expenses_str(),
                ledger.accounts_str(),
            ]
        );
    });

    // Open the dropdown menu and select `2025/expenses.bean` (item index 1).
    cx.update_window(handle, |_, window, cx| {
        window.within("source-file-select").click("input", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.press("down", cx);
        window.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.find("source-file-select").value(),
            Some("2025/expenses.bean")
        );
        assert_eq!(
            window.find_by_label("Source text").value(),
            Some(expenses_expected.as_str())
        );

        // Append an unsaved comment line to `2025/expenses.bean`.
        let editor = app.read(cx).editor().unwrap();
        let state = editor.read(cx).editor_state().clone();
        state.update(cx, |state, cx| {
            let end = state.value().len();
            state.set_selected_range(end..end, cx);
            state.focus(window, cx);
        });
        window.render_frame(cx);
        window.input("; draft note", cx);
    })
    .unwrap();

    // Switch back to `main.bean` and then return to `2025/expenses.bean`:
    // the unsaved draft is preserved until reverted or saved.
    let editor = cx.read(|cx| app.read(cx).editor().unwrap());
    cx.update_window(handle, |_, window, cx| {
        editor.update(cx, |editor, cx| {
            editor.select_file(ledger.main_str(), window, cx)
        });
    })
    .unwrap();
    cx.run_until_parked();
    cx.read(|cx| assert_eq!(editor.read(cx).value(cx), main_expected));

    cx.update_window(handle, |_, window, cx| {
        editor.update(cx, |editor, cx| {
            editor.select_file(ledger.expenses_str(), window, cx)
        });
    })
    .unwrap();
    cx.run_until_parked();
    cx.read(|cx| {
        assert!(editor.read(cx).is_changed(cx));
        assert!(editor.read(cx).value(cx).ends_with("; draft note"));
    });

    // Clicking Revert restores the original file contents.
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("revert-source", cx);
    })
    .unwrap();
    cx.read(|cx| {
        assert!(!editor.read(cx).is_changed(cx));
        assert_eq!(editor.read(cx).value(cx), expenses_expected);
    });
}

#[gpui_kit::test]
async fn test_source_editor_supports_highlighting_autocomplete_and_save_reloads_ledger(
    cx: &mut TestAppContext,
) {
    let ledger = MultiFileLedger::new("autocomplete-save");
    let (handle, app) = open_editor(&ledger, cx);
    let editor = cx.read(|cx| app.read(cx).editor().expect("editor page is open"));

    // Select `2025/expenses.bean` and verify Beancount language highlighting + line numbers.
    cx.update_window(handle, |_, window, cx| {
        editor.update(cx, |editor, cx| {
            editor.select_file(ledger.expenses_str(), window, cx)
        });
    })
    .unwrap();
    cx.run_until_parked();
    cx.read(|cx| {
        let state = editor.read(cx).editor_state().read(cx);
        assert!(state.is_code_editor());
        assert_eq!(state.language_name(), "beancount");
    });

    // Append a new transaction using payee, narration, and account auto-completions.
    cx.update_window(handle, |_, window, cx| {
        let state = editor.read(cx).editor_state().clone();
        state.update(cx, |state, cx| {
            let updated = format!("{}2025-02-01 * ", state.value());
            let end = updated.len();
            state.set_value(updated, window, cx);
            state.set_selected_range(end..end, cx);
            state.focus(window, cx);
        });
        window.render_frame(cx);
        window.input("\"Blu", cx);
    })
    .unwrap();
    cx.run_until_parked();

    // Accept payee `"Blue Bottle"`.
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

    // Open narration string -> `"Latte"` (previously used with `"Blue Bottle"`) completes.
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.input(" \"Lat", cx);
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
        window.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();

    // Type posting line and accept account completion `Expenses:Food:Coffee`.
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let state = editor.read(cx).editor_state().clone();
        state.update(cx, |state, cx| {
            let next = format!("{}\n  Exp", state.value());
            let end = next.len();
            state.set_value(next, window, cx);
            state.set_selected_range(end..end, cx);
            state.focus(window, cx);
        });
        window.render_frame(cx);
        window.input("e", cx);
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
        window.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();

    // Finish the balancing posting and save via the keyboard shortcut (`cmd-s` / `ctrl-s`).
    #[cfg(target_os = "macos")]
    let save_shortcut = "cmd-s";
    #[cfg(not(target_os = "macos"))]
    let save_shortcut = "ctrl-s";

    cx.update_window(handle, |_, window, cx| {
        let state = editor.read(cx).editor_state().clone();
        state.update(cx, |state, cx| {
            let completed = format!("{}\n  Assets:Cash  -5.00 USD\n", state.value()).replace(
                "  Expenses:Food:Coffee\n",
                "  Expenses:Food:Coffee   5.00 USD\n",
            );
            state.set_value(completed, window, cx);
            state.focus(window, cx);
        });
        window.render_frame(cx);
        window.press(save_shortcut, cx);
    })
    .unwrap();

    cx.wait_for(handle, Duration::from_secs(5), |_, cx| {
        app.read(cx).session().read(cx).revision() == 2
    })
    .await;
    cx.run_until_parked();

    let saved_disk = fs::read_to_string(&ledger.expenses_path).unwrap();
    assert!(
        saved_disk.contains("2025-02-01 * \"Blue Bottle\" \"Latte\""),
        "{saved_disk}"
    );
    cx.read(|cx| assert!(!editor.read(cx).is_changed(cx)));

    // Navigating to Journal confirms the reloaded ledger now contains both transactions.
    cx.update_window(handle, |_, window, cx| {
        window.dispatch_action(Box::new(ShowJournal), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.read(|cx| {
        let journal = app.read(cx).journal().expect("journal is shown");
        assert_eq!(journal.read(cx).total(cx), 2);
        assert_eq!(
            journal.read(cx).entries(cx)[0].txn.date().to_string(),
            "2025-02-01"
        );
    });
}
