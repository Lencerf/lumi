use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::component::input::RopeExt as _;
use gpui_kit::test::{
    TestAppContextExt as _, TestEventExt as _, TestQueryExt as _, TestWindowExt as _,
};
use gpui_kit::{
    AnyWindowHandle, AppContext as _, Bounds, ClipboardItem, Entity, EntityInputHandler as _,
    Modifiers, MouseButton, MouseDownEvent, PlatformInput, ScrollDelta, ScrollEvent,
    TestAppContext, TouchPhase, WindowBounds, WindowOptions, point, px, size,
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

#[gpui_kit::test]
fn test_clicking_location_in_entry_editor_jumps_to_file_and_line_in_source_editor(
    cx: &mut TestAppContext,
) {
    let ledger = MultiFileLedger::new("jump-to-line");
    fs::write(
        &ledger.expenses_path,
        "2025-01-10 * \"Blue Bottle\" \"Latte\" #coffee\n  Expenses:Food:Coffee   6.50 USD\n  Assets:Cash           -6.50 USD\n\n2025-01-20 * \"Tartine\" \"Croissant\"\n  Expenses:Food:Coffee   5.25 USD\n  Assets:Cash           -5.25 USD\n",
    )
    .unwrap();
    fs::write(
        &ledger.main_path,
        "option \"title\" \"Multi-file ledger\"\noption \"operating_currency\" \"USD\"\n\ninclude \"accounts.bean\"\ninclude \"2025/expenses.bean\"\n\n2025-01-05 balance Assets:Cash 0 USD\n",
    )
    .unwrap();

    // Start on Journal so `Page::Editor` has not been created yet.
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

    // Newest entry (row 0) is `"Croissant"` at `2025/expenses.bean:5`.
    let expected_expenses_loc = format!("Open {}:5 in editor", ledger.expenses_str());
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(("edit", 0usize), cx);
    })
    .unwrap();
    cx.run_until_parked();

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.find("open-in-editor").label(),
            Some(expected_expenses_loc.as_str())
        );
        window.click("open-in-editor", cx);
    })
    .unwrap();
    cx.run_until_parked();

    // Dialog closes, app switches to Editor, selects `2025/expenses.bean`, and
    // places the cursor on line 5 with focus in the editor.
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("open-in-editor").is_none());
        assert!(window.try_find_by_label("Entry text").is_none());
        assert_eq!(
            window.find("source-file-select").value(),
            Some("2025/expenses.bean")
        );
        assert_eq!(window.find_by_label("Source text").focused(), Some(true));
    })
    .unwrap();
    cx.read(|cx| {
        assert_eq!(app.read(cx).page(), &Page::Editor);
        let editor = app.read(cx).editor().expect("editor page is open");
        assert_eq!(
            editor.read(cx).selected_file().map(|s| s.as_ref()),
            Some(ledger.expenses_str().as_str())
        );
        assert_eq!(editor.read(cx).cursor_line(cx), 5);
    });

    // Return to Journal while `SourceEditor` is already cached, open the
    // oldest entry (`main.bean:7`), verify Duplicate hides the link, then jump
    // from a fresh Edit Entry dialog to `main.bean` line 7.
    cx.update_window(handle, |_, window, cx| {
        window.dispatch_action(Box::new(ShowJournal), cx);
    })
    .unwrap();
    cx.run_until_parked();

    let expected_main_loc = format!("Open {}:7 in editor", ledger.main_str());
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(("edit", 2usize), cx);
    })
    .unwrap();
    cx.run_until_parked();

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.find("open-in-editor").label(),
            Some(expected_main_loc.as_str())
        );
        window.click("duplicate-entry", cx);
    })
    .unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("open-in-editor").is_none());
        window.click("cancel", cx);
    })
    .unwrap();

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(("edit", 2usize), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("open-in-editor", cx);
    })
    .unwrap();
    cx.run_until_parked();

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("source-file-select").value(), Some("main.bean"));
        assert_eq!(window.find_by_label("Source text").focused(), Some(true));
    })
    .unwrap();
    cx.read(|cx| {
        assert_eq!(app.read(cx).page(), &Page::Editor);
        let editor = app.read(cx).editor().expect("editor page is open");
        assert_eq!(
            editor.read(cx).selected_file().map(|s| s.as_ref()),
            Some(ledger.main_str().as_str())
        );
        assert_eq!(editor.read(cx).cursor_line(cx), 7);
    });
}

#[gpui_kit::test]
fn test_jumping_to_file_line_places_target_at_top_unless_near_eof(cx: &mut TestAppContext) {
    let ledger = MultiFileLedger::new("scroll-top-and-eof");
    // Build a 120-line `2025/expenses.bean`:
    // - Lines 1..=56: 56 comment lines
    // - Lines 57..=59: `2025-03-01` ("Middle entry", journal row 0) at line 57
    // - Lines 60..=117: 58 comment lines
    // - Lines 118..=120: `2025-02-28` ("Bottom entry", journal row 1) at line 118
    let mut lines: Vec<String> = (1..=56).map(|n| format!("; padding {n}")).collect();
    lines.push("2025-03-01 * \"Blue Bottle\" \"Middle entry\"".into());
    lines.push("  Expenses:Food:Coffee   5.00 USD".into());
    lines.push("  Assets:Cash           -5.00 USD".into());
    lines.extend((60..=117).map(|n| format!("; padding {n}")));
    lines.push("2025-02-28 * \"Blue Bottle\" \"Bottom entry\"".into());
    lines.push("  Expenses:Food:Coffee   6.00 USD".into());
    lines.push("  Assets:Cash           -6.00 USD".into());
    assert_eq!(lines.len(), 120);
    fs::write(&ledger.expenses_path, lines.join("\n")).unwrap();

    cx.update(crate::init);
    let (handle, app): (AnyWindowHandle, Entity<LumiApp>) = cx.update(|cx| {
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    Default::default(),
                    size(px(1200.), px(600.)),
                ))),
                ..Default::default()
            },
            cx,
            |window, cx| {
                cx.new(|cx| {
                    LumiApp::new(
                        Arc::new(LocalLedger::new(ledger.main_str())),
                        "main.bean",
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

    // Jump to row 0 (`2025-03-01` at line 57, well above the last viewport).
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(("edit", 0usize), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("open-in-editor", cx);
    })
    .unwrap();
    cx.run_until_parked();

    // Render frames so initial layout + deferred top-aligned scroll settle.
    for _ in 0..3 {
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
        })
        .unwrap();
    }
    cx.read(|cx| {
        let editor = app.read(cx).editor().expect("editor page is open");
        assert_eq!(editor.read(cx).cursor_line(cx), 57);
        let visible = editor
            .read(cx)
            .editor_state()
            .read(cx)
            .visible_row_range()
            .expect("editor is laid out");
        // 1-based line 57 is 0-based row 56 -> scrolled to be the first line in view.
        assert_eq!(visible.start, 56);
    });

    // Now return to Journal and jump to row 1 (`2025-02-28` at line 118, within
    // the last 3 lines of the 120-line file): scroll stops at EOF rather than
    // placing line 118 at the top with blank space below.
    cx.update_window(handle, |_, window, cx| {
        window.dispatch_action(Box::new(ShowJournal), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(("edit", 1usize), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("open-in-editor", cx);
    })
    .unwrap();
    cx.run_until_parked();

    for _ in 0..2 {
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
        })
        .unwrap();
    }
    cx.read(|cx| {
        let editor = app.read(cx).editor().expect("editor page is open");
        assert_eq!(editor.read(cx).cursor_line(cx), 118);
        let state = editor.read(cx).editor_state().read(cx);
        let visible = state.visible_row_range().expect("editor is laid out");
        let total_lines = state.text().lines_len();
        assert!(visible.start > 56);
        assert!(visible.start < 117);
        assert!(visible.contains(&117));
        assert_eq!(visible.end, total_lines);
    });
}

#[gpui_kit::test]
fn test_source_editor_paste_shorter_text_does_not_keep_n_chars_selected_on_stale_drag(
    cx: &mut TestAppContext,
) {
    let ledger = MultiFileLedger::new("paste-shorter-drag");
    let (handle, app) = open_editor(&ledger, cx);
    let editor = cx.read(|cx| app.read(cx).editor().expect("editor page is open"));

    cx.update_window(handle, |_, window, cx| {
        editor.update(cx, |editor, cx| {
            editor.select_file(ledger.expenses_str(), window, cx)
        });
    })
    .unwrap();
    cx.run_until_parked();

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let state = editor.read(cx).editor_state().clone();
        // `"Blue"` in `2025-01-10 * "Blue Bottle" ...` is 4 chars; `"Bottle"` at `19..25` is 6 chars.
        let word_bounds = state.update(cx, |state, cx| {
            let text_bounds = state.text_bounds().expect("text bounds");
            state
                .bounds_for_range(19..25, text_bounds, window, cx)
                .expect("word bounds")
        });
        let click_pos = point(
            word_bounds.origin.x + word_bounds.size.width * 0.5,
            word_bounds.origin.y + word_bounds.size.height * 0.5,
        );
        let drag_end = point(
            word_bounds.origin.x + word_bounds.size.width,
            word_bounds.origin.y + word_bounds.size.height * 0.5,
        );

        window.dispatch_event(
            PlatformInput::MouseDown(MouseDownEvent {
                button: MouseButton::Left,
                position: click_pos,
                modifiers: Modifiers::default(),
                click_count: 2,
                first_mouse: false,
            }),
            cx,
        );
        window.render_frame(cx);
        assert_eq!(state.read(cx).selected_range(), 19..25);

        cx.write_to_clipboard(ClipboardItem::new_string("Cup".into()));
        let paste = if cfg!(target_os = "macos") {
            "cmd-v"
        } else {
            "ctrl-v"
        };
        window.press(paste, cx);
        window.pointer_move(drag_end, Some(MouseButton::Left), cx);
        window.pointer_up(drag_end, MouseButton::Left, cx);

        assert!(
            state
                .read(cx)
                .value()
                .starts_with("2025-01-10 * \"Blue Cup\" \"Latte\"")
        );
        assert_eq!(state.read(cx).selected_range(), 22..22);
    })
    .unwrap();
}

#[gpui_kit::test]
fn test_source_editor_cmd_f_typing_sequential_chars_does_not_select_or_overwrite_first_char(
    cx: &mut TestAppContext,
) {
    let ledger = MultiFileLedger::new("cmd-f-typing");
    let (handle, app) = open_editor(&ledger, cx);
    let editor = cx.read(|cx| app.read(cx).editor().expect("editor page is open"));

    cx.update_window(handle, |_, window, cx| {
        editor.update(cx, |editor, cx| {
            editor.select_file(ledger.expenses_str(), window, cx)
        });
    })
    .unwrap();
    cx.run_until_parked();

    let find_shortcut = if cfg!(target_os = "macos") {
        "cmd-f"
    } else {
        "ctrl-f"
    };

    // Open search with no selection in the editor, then type `"u"`, `"s"`, `"d"`
    // across separate window updates + frames (matching real OS event delivery)
    // so any render-time re-selection (the bug in gpui-component's built-in
    // SearchPanel) or stale macOS drag would overwrite the 1st character on the
    // 2nd keystroke.
    let (state, search_input) = cx
        .update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let state = editor.read(cx).editor_state().clone();
            let search_input = editor.read(cx).search_input_state().clone();
            state.update(cx, |state, cx| {
                state.set_selected_range(0..0, cx);
                state.focus(window, cx);
            });
            window.render_frame(cx);

            window.press(find_shortcut, cx);
            window.render_frame(cx);
            assert!(editor.read(cx).is_search_open());
            assert!(editor.read(cx).is_search_focused(window, cx));
            assert_eq!(search_input.read(cx).value(), "");
            assert_eq!(search_input.read(cx).selected_range(), 0..0);
            (state, search_input)
        })
        .unwrap();

    cx.update_window(handle, |_, window, cx| {
        window.input("u", cx);
    })
    .unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(search_input.read(cx).value(), "u");
        assert_eq!(search_input.read(cx).selected_range(), 1..1);

        let left_edge = search_input.update(cx, |input, cx| {
            let text_bounds = input.text_bounds().expect("search text bounds");
            let char_bounds = input
                .bounds_for_range(0..1, text_bounds, window, cx)
                .expect("first char bounds");
            point(
                char_bounds.origin.x,
                char_bounds.origin.y + char_bounds.size.height * 0.5,
            )
        });
        window.pointer_move(left_edge, Some(MouseButton::Left), cx);
        assert_eq!(search_input.read(cx).selected_range(), 1..1);
    })
    .unwrap();

    cx.update_window(handle, |_, window, cx| {
        window.input("s", cx);
    })
    .unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(search_input.read(cx).value(), "us");
        assert_eq!(search_input.read(cx).selected_range(), 2..2);
        window.input("d", cx);
    })
    .unwrap();

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(editor.read(cx).search_query(cx), "usd");
        let matcher = &state.read(cx).search_session().matcher;
        assert_eq!(matcher.len(), 2);
        assert_eq!(matcher.label(), "1/2");

        // Enter / Shift+Enter cycle forward and backward through matches.
        window.press("enter", cx);
        window.render_frame(cx);
        assert_eq!(state.read(cx).search_session().matcher.label(), "2/2");

        window.press("shift-enter", cx);
        window.render_frame(cx);
        assert_eq!(state.read(cx).search_session().matcher.label(), "1/2");

        // Enabling case-sensitive matching causes lowercase `"usd"` not to match `"USD"`.
        window.click("source-search-case", cx);
        window.render_frame(cx);
        assert_eq!(state.read(cx).search_session().matcher.len(), 0);
        window.click("source-search-case", cx);
        window.render_frame(cx);
        assert_eq!(state.read(cx).search_session().matcher.len(), 2);

        // Escape closes the search bar and returns focus to the source editor.
        window.press("escape", cx);
        window.render_frame(cx);
        assert!(!editor.read(cx).is_search_open());
        assert!(!state.read(cx).search_session().is_active());
        assert_eq!(window.find_by_label("Source text").focused(), Some(true));

        // Pressing Cmd+F / Ctrl+F with single-line text selected (`"Bottle"` at 19..25)
        // seeds and selects `"Bottle"`, and typing `"L"` then `"a"` replaces `"Bottle"`
        // with `"La"` (never `"a"`).
        state.update(cx, |state, cx| {
            state.set_selected_range(19..25, cx);
            state.focus(window, cx);
        });
        window.render_frame(cx);
        window.press(find_shortcut, cx);
        window.render_frame(cx);
        assert_eq!(search_input.read(cx).value(), "Bottle");
        assert_eq!(search_input.read(cx).selected_range(), 0..6);
    })
    .unwrap();

    cx.update_window(handle, |_, window, cx| {
        window.input("L", cx);
    })
    .unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(search_input.read(cx).value(), "L");
        assert_eq!(search_input.read(cx).selected_range(), 1..1);
        window.input("a", cx);
    })
    .unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(search_input.read(cx).value(), "La");
        assert_eq!(search_input.read(cx).selected_range(), 2..2);
        assert_eq!(state.read(cx).search_session().matcher.len(), 1);
    })
    .unwrap();
}

#[gpui_kit::test]
fn test_source_editor_auto_aligns_posting_numbers_handles_long_accounts_and_undo(
    cx: &mut TestAppContext,
) {
    let ledger = MultiFileLedger::new("auto-align-default");
    fs::write(
        &ledger.accounts_path,
        "\
2024-01-01 open Assets:Cash USD
2024-01-01 open Expenses:Food:Coffee USD
2024-01-01 open Liabilities:WellsFargo:Bilt USD
",
    )
    .unwrap();
    let (handle, app) = open_editor(&ledger, cx);
    let editor = cx.read(|cx| app.read(cx).editor().unwrap().clone());
    let state = cx.read(|cx| editor.read(cx).editor_state().clone());

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        state.update(cx, |state, cx| {
            state.set_value("2025-02-01 * \"Cafe\"\n", window, cx);
            let end = state.text().len();
            state.set_selected_range(end..end, cx);
            state.focus(window, cx);
        });
        window.render_frame(cx);
    })
    .unwrap();

    // Type the user's exact posting character-by-character so every intermediate
    // keystroke (`-`, `-2`, `-2.`, `-2.36`, ` USD`) exercises live alignment.
    for ch in "    Liabilities:WellsFargo:Bilt -2.36 USD".chars() {
        let s = ch.to_string();
        cx.update_window(handle, |_, window, cx| {
            window.input(&s, cx);
        })
        .unwrap();
    }
    cx.run_until_parked();

    cx.read(|cx| {
        assert_eq!(
            editor.read(cx).value(cx).as_ref(),
            "2025-02-01 * \"Cafe\"\n    Liabilities:WellsFargo:Bilt                -2.36 USD"
        );
        let line = editor
            .read(cx)
            .value(cx)
            .lines()
            .nth(1)
            .unwrap()
            .to_string();
        assert_eq!(line.find('.').unwrap() + 1, 50);
    });

    // Editing the account name on an already-aligned posting re-aligns `.` at
    // column 50 while keeping the caret inside the account and opening completions.
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        state.update(cx, |state, cx| {
            let text = state.value();
            let after_account = text.find("Liabilities:WellsFargo:Bilt").unwrap()
                + "Liabilities:WellsFargo:Bilt".len();
            state.set_selected_range(after_account..after_account, cx);
            state.focus(window, cx);
        });
        window.press("backspace", cx);
        window.press("backspace", cx);
    })
    .unwrap();
    cx.run_until_parked();

    cx.read(|cx| {
        let doc = editor.read(cx).value(cx).to_string();
        let line = doc.lines().nth(1).unwrap();
        assert_eq!(
            line,
            "    Liabilities:WellsFargo:Bi                  -2.36 USD"
        );
        assert_eq!(line.find('.').unwrap() + 1, 50);
    });

    // Typing `l` back into `Liabilities:WellsFargo:Bi` keeps `.` at column 50
    // AND presents account completion items.
    cx.update_window(handle, |_, window, cx| {
        window.input("l", cx);
    })
    .unwrap();
    cx.run_until_parked();

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let st = state.read(cx);
        let doc = st.value().to_string();
        let line = doc.lines().nth(1).unwrap();
        assert_eq!(
            line,
            "    Liabilities:WellsFargo:Bil                 -2.36 USD"
        );
        assert_eq!(line.find('.').unwrap() + 1, 50);
        assert!(st.completion_menu_state().open);
        window.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();

    cx.read(|cx| {
        let doc = editor.read(cx).value(cx).to_string();
        let line = doc.lines().nth(1).unwrap();
        assert_eq!(
            line,
            "    Liabilities:WellsFargo:Bilt                -2.36 USD"
        );
        assert_eq!(line.find('.').unwrap() + 1, 50);
    });

    // When an account name is too long for column 50, at least 1 space is kept,
    // and Undo steps back without being immediately re-aligned.
    #[cfg(target_os = "macos")]
    let undo_shortcut = "cmd-z";
    #[cfg(not(target_os = "macos"))]
    let undo_shortcut = "ctrl-z";

    cx.update_window(handle, |_, window, cx| {
        state.update(cx, |state, cx| {
            state.set_value("2025-02-01 * \"Long\"\n", window, cx);
            let end = state.text().len();
            state.set_selected_range(end..end, cx);
            state.focus(window, cx);
        });
        window.render_frame(cx);
        window.input(
            "    Liabilities:WellsFargo:VeryLongSubAccountName:CreditCard     -2.36 USD",
            cx,
        );
    })
    .unwrap();
    cx.run_until_parked();

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            editor.read(cx).value(cx).as_ref(),
            "2025-02-01 * \"Long\"\n    Liabilities:WellsFargo:VeryLongSubAccountName:CreditCard -2.36 USD"
        );
        window.press(undo_shortcut, cx);
    })
    .unwrap();
    cx.run_until_parked();

    cx.read(|cx| {
        assert_eq!(
            editor.read(cx).value(cx).as_ref(),
            "2025-02-01 * \"Long\"\n    Liabilities:WellsFargo:VeryLongSubAccountName:CreditCard     -2.36 USD"
        );
    });
}

#[gpui_kit::test]
fn test_lumi_option_decimal_point_column_customizes_source_and_entry_editors(
    cx: &mut TestAppContext,
) {
    let ledger = MultiFileLedger::new("custom-decimal-col");
    fs::write(
        &ledger.main_path,
        "\
option \"title\" \"Custom column ledger\"
option \"operating_currency\" \"USD\"
1994-05-30 custom \"lumi-option\" \"decimal-point-column\" \"60\"

include \"accounts.bean\"
include \"2025/expenses.bean\"
",
    )
    .unwrap();

    let (handle, app) = open_editor(&ledger, cx);
    let editor = cx.read(|cx| app.read(cx).editor().unwrap().clone());
    let state = cx.read(|cx| editor.read(cx).editor_state().clone());

    cx.read(|cx| {
        assert_eq!(app.read(cx).session().read(cx).error_count(), 0);
    });

    // Switch to `2025/expenses.bean` (an included file that does not itself
    // repeat the option) and verify it inherits column 60 from the ledger.
    let expenses_path = ledger.expenses_str();
    cx.update_window(handle, |_, window, cx| {
        editor.update(cx, |ed, cx| ed.select_file(&expenses_path, window, cx));
    })
    .unwrap();
    cx.run_until_parked();

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        state.update(cx, |state, cx| {
            let end = state.text().len();
            state.set_selected_range(end..end, cx);
            state.focus(window, cx);
        });
        window.input("    Liabilities:WellsFargo:Bilt -2.36 USD", cx);
    })
    .unwrap();
    cx.run_until_parked();

    cx.read(|cx| {
        let doc = editor.read(cx).value(cx).to_string();
        let last_line = doc.lines().last().unwrap();
        assert_eq!(last_line.find('.').unwrap() + 1, 60);
    });

    // Switch back to `main.bean` and edit `"60"` -> `"45"` live without saving;
    // typing a posting in `main.bean` immediately uses the unsaved column 45.
    let main_path = ledger.main_str();
    cx.update_window(handle, |_, window, cx| {
        editor.update(cx, |ed, cx| ed.select_file(&main_path, window, cx));
    })
    .unwrap();
    cx.run_until_parked();

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        state.update(cx, |state, cx| {
            let updated = state.value().replace("\"60\"", "\"45\"");
            state.set_value(format!("{updated}\n2025-02-02 * \"Test\"\n"), window, cx);
            let end = state.text().len();
            state.set_selected_range(end..end, cx);
            state.focus(window, cx);
        });
        window.render_frame(cx);
        window.input("    Liabilities:WellsFargo:Bilt -2.36 USD", cx);
    })
    .unwrap();
    cx.run_until_parked();

    cx.read(|cx| {
        let doc = editor.read(cx).value(cx).to_string();
        let last_line = doc.lines().last().unwrap();
        assert_eq!(last_line.find('.').unwrap() + 1, 45);
    });

    // EntryEditor also honors the ledger's `decimal-point-column` (`60`).
    cx.update_window(handle, |_, window, cx| {
        window.dispatch_action(Box::new(ShowJournal), cx);
    })
    .unwrap();
    cx.run_until_parked();

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(("edit", 0usize), cx);
    })
    .unwrap();
    cx.run_until_parked();

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.press("end", cx);
        // Move to end of the entry and append a new posting line.
        let val = window
            .find_by_label("Entry text")
            .value()
            .unwrap()
            .to_string();
        assert!(val.contains("2025-01-10"));
    })
    .unwrap();

    // Type a new posting in EntryEditor via focused keystrokes at EOF.
    cx.update_window(handle, |_, window, cx| {
        #[cfg(target_os = "macos")]
        window.press("cmd-down", cx);
        #[cfg(not(target_os = "macos"))]
        window.press("ctrl-end", cx);
        window.input("\n    Liabilities:WellsFargo:Bilt -2.36 USD", cx);
    })
    .unwrap();
    cx.run_until_parked();

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let val = window
            .find_by_label("Entry text")
            .value()
            .unwrap()
            .to_string();
        let last_line = val.lines().last().unwrap();
        assert_eq!(last_line.find('.').unwrap() + 1, 60);
    })
    .unwrap();
}

#[gpui_kit::test]
async fn test_source_editor_highlights_error_lines_shows_hover_shifts_on_edits_and_clears_on_save(
    cx: &mut TestAppContext,
) {
    let ledger = MultiFileLedger::new("error-highlights-and-hover");
    // `main.bean` has a `plugin` directive (`ErrorLevel::Info` on line 3), which
    // must NOT be highlighted as an error.
    fs::write(
        &ledger.main_path,
        "\
option \"title\" \"Error highlights ledger\"
option \"operating_currency\" \"USD\"
plugin \"beancount.plugins.auto_accounts\"

include \"accounts.bean\"
include \"2025/expenses.bean\"
",
    )
    .unwrap();
    // `2025/expenses.bean` has:
    // - Lines 1..=3: valid transaction
    // - Lines 5..=7: unbalanced transaction (`ErrorLevel::Error` at start line 5 only)
    // - Line 9: failed balance assertion (`ErrorLevel::Error` at line 9)
    fs::write(
        &ledger.expenses_path,
        "\
2025-01-10 * \"Blue Bottle\" \"Latte\"
  Expenses:Food:Coffee   6.50 USD
  Assets:Cash           -6.50 USD

2025-01-12 * \"Tartine\" \"Unbalanced pastry\"
  Expenses:Food:Coffee   5.00 USD
  Assets:Cash           -4.00 USD

2025-01-15 balance Assets:Cash 0.00 USD
",
    )
    .unwrap();

    let (handle, app) = open_editor(&ledger, cx);
    let editor = cx.read(|cx| app.read(cx).editor().expect("editor page is open"));

    // Render `main.bean` and verify `ErrorLevel::Info` on line 3 is NOT highlighted.
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
    })
    .unwrap();
    cx.read(|cx| {
        assert!(editor.read(cx).error_lines(cx).is_empty());
        assert!(editor.read(cx).painted_error_lines().is_empty());
    });

    // Switch to `2025/expenses.bean`.
    let expenses_path = ledger.expenses_str();
    cx.update_window(handle, |_, window, cx| {
        editor.update(cx, |ed, cx| ed.select_file(&expenses_path, window, cx));
    })
    .unwrap();
    cx.run_until_parked();

    // Render two frames so editor layout and backdrop painting settle.
    for _ in 0..2 {
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
        })
        .unwrap();
    }

    // Only starting lines 5 and 9 (`ErrorLevel::Error`) are highlighted — NOT
    // lines 6 or 7 of the multi-line unbalanced transaction.
    let (line5_center, line6_center, line9_right_whitespace) = cx
        .update_window(handle, |_, window, cx| {
            let ed = editor.read(cx);
            let error_lines = ed.error_lines(cx);
            let line_numbers: Vec<usize> = error_lines.iter().map(|(line, _)| *line).collect();
            assert_eq!(line_numbers, [5, 9]);
            assert!(
                error_lines[0].1[0].contains("balance")
                    || error_lines[0].1[0].contains("USD")
                    || !error_lines[0].1[0].is_empty()
            );

            let painted = ed.painted_error_lines();
            let painted_lines: Vec<usize> = painted.iter().map(|(line, _)| *line).collect();
            assert_eq!(painted_lines, [5, 9]);
            for (_, rect) in &painted {
                // Full-width light-red band across the editor card.
                assert!(
                    rect.size.width > px(800.),
                    "rect width = {:?}",
                    rect.size.width
                );
                assert!(rect.size.height > px(10.));
            }

            let state = ed.editor_state().clone();
            let (line5_glyph, line6_glyph, input_bounds) = state.update(cx, |state, cx| {
                let text_bounds = state.text_bounds().expect("text bounds");
                let l5_start = state.text().line_start_offset(4);
                let l6_start = state.text().line_start_offset(5);
                let b5 = state
                    .bounds_for_range(l5_start..l5_start + 4, text_bounds, window, cx)
                    .expect("line 5 glyph bounds");
                let b6 = state
                    .bounds_for_range(l6_start..l6_start + 4, text_bounds, window, cx)
                    .expect("line 6 glyph bounds");
                (b5, b6, state.input_bounds())
            });
            // Painted row 5 background aligns vertically with line 5's glyphs.
            assert!((painted[0].1.origin.y - line5_glyph.origin.y).abs() < px(1.));

            let p5 = point(
                line5_glyph.origin.x + px(10.),
                line5_glyph.origin.y + line5_glyph.size.height * 0.5,
            );
            let p6 = point(
                line6_glyph.origin.x + px(10.),
                line6_glyph.origin.y + line6_glyph.size.height * 0.5,
            );
            // Far right whitespace on line 9 (well past the end of text on line 9).
            let p9_right = point(
                input_bounds.right() - px(40.),
                painted[1].1.origin.y + painted[1].1.size.height * 0.5,
            );
            (p5, p6, p9_right)
        })
        .unwrap();

    // Hovering over line 5 shows the error detail hover for line 5.
    let expected_line5_msg = cx.read(|cx| editor.read(cx).error_lines(cx)[0].1[0].to_string());
    let expected_line9_msg = cx.read(|cx| editor.read(cx).error_lines(cx)[1].1[0].to_string());

    cx.update_window(handle, |_, window, cx| {
        assert!(window.try_find("source-editor-error-hover").is_none());
        window.pointer_move(line5_center, None, cx);
        window.render_frame(cx);
        assert_eq!(
            window.find("source-editor-error-hover").label(),
            Some(expected_line5_msg.as_str())
        );

        // Moving to line 6 (posting of the unbalanced transaction) hides the hover.
        window.pointer_move(line6_center, None, cx);
        window.render_frame(cx);
        assert!(window.try_find("source-editor-error-hover").is_none());

        // Hovering over empty space to the right of text on line 9 shows line 9's error detail.
        window.pointer_move(line9_right_whitespace, None, cx);
        window.render_frame(cx);
        assert_eq!(
            window.find("source-editor-error-hover").label(),
            Some(expected_line9_msg.as_str())
        );

        // Moving outside the editor body (into the top toolbar) hides the hover.
        window.pointer_move(point(px(400.), px(20.)), None, cx);
        window.render_frame(cx);
        assert!(window.try_find("source-editor-error-hover").is_none());
    })
    .unwrap();

    // Insert two comment lines at the very start of `2025/expenses.bean`:
    // error lines shift `[5, 9] -> [7, 11]` before saving, and switching files
    // preserves the shifted lines on the unsaved draft.
    cx.update_window(handle, |_, window, cx| {
        let state = editor.read(cx).editor_state().clone();
        state.update(cx, |state, cx| {
            state.set_selected_range(0..0, cx);
            state.focus(window, cx);
        });
        window.render_frame(cx);
        window.input("; header 1\n; header 2\n", cx);
        window.render_frame(cx);

        let shifted: Vec<usize> = editor
            .read(cx)
            .painted_error_lines()
            .into_iter()
            .map(|(line, _)| line)
            .collect();
        assert_eq!(shifted, [7, 11]);

        // Old line 5 is no longer an error line; shifted line 7 shows the hover.
        window.pointer_move(line5_center, None, cx);
        window.render_frame(cx);
        assert!(window.try_find("source-editor-error-hover").is_none());

        let line7_rect = editor.read(cx).painted_error_lines()[0].1;
        window.pointer_move(
            point(
                line5_center.x,
                line7_rect.origin.y + line7_rect.size.height * 0.5,
            ),
            None,
            cx,
        );
        window.render_frame(cx);
        assert_eq!(
            window.find("source-editor-error-hover").label(),
            Some(expected_line5_msg.as_str())
        );
    })
    .unwrap();

    // Switch to `main.bean` and back to `2025/expenses.bean`: unsaved draft keeps `[7, 11]`.
    let main_path = ledger.main_str();
    cx.update_window(handle, |_, window, cx| {
        editor.update(cx, |ed, cx| ed.select_file(&main_path, window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        editor.update(cx, |ed, cx| ed.select_file(&expenses_path, window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    cx.read(|cx| {
        let lines: Vec<usize> = editor
            .read(cx)
            .error_lines(cx)
            .into_iter()
            .map(|(line, _)| line)
            .collect();
        assert_eq!(lines, [7, 11]);
    });

    // Clicking Revert restores `[5, 9]`.
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("revert-source", cx);
        window.render_frame(cx);
        let reverted: Vec<usize> = editor
            .read(cx)
            .painted_error_lines()
            .into_iter()
            .map(|(line, _)| line)
            .collect();
        assert_eq!(reverted, [5, 9]);
    })
    .unwrap();

    // Fix both errors in `2025/expenses.bean` and save -> ledger reloads and
    // error highlights + hover clear completely.
    #[cfg(target_os = "macos")]
    let save_shortcut = "cmd-s";
    #[cfg(not(target_os = "macos"))]
    let save_shortcut = "ctrl-s";

    cx.update_window(handle, |_, window, cx| {
        let state = editor.read(cx).editor_state().clone();
        state.update(cx, |state, cx| {
            let fixed = state.value().replace("-4.00 USD", "-5.00 USD").replace(
                "balance Assets:Cash 0.00 USD",
                "balance Assets:Cash -11.50 USD",
            );
            state.set_value(fixed, window, cx);
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

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(editor.read(cx).error_lines(cx).is_empty());
        assert!(editor.read(cx).painted_error_lines().is_empty());
        window.pointer_move(line5_center, None, cx);
        window.render_frame(cx);
        assert!(window.try_find("source-editor-error-hover").is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn test_source_editor_error_line_edits_enter_col0_vs_eol_delete_undo_and_same_line_errors(
    cx: &mut TestAppContext,
) {
    let ledger = MultiFileLedger::new("error-line-edit-edge-cases");
    // Line 1 of `2025/expenses.bean` is `2025-01-05 pad Assets:Missing1 Assets:Missing2`,
    // which triggers TWO `ErrorLevel::Error` diagnostics on line 1 (both unknown accounts).
    fs::write(
        &ledger.expenses_path,
        "2025-01-05 pad Assets:Missing1 Assets:Missing2\n",
    )
    .unwrap();

    let (handle, app) = open_editor(&ledger, cx);
    let editor = cx.read(|cx| app.read(cx).editor().expect("editor page is open"));
    let expenses_path = ledger.expenses_str();

    cx.update_window(handle, |_, window, cx| {
        editor.update(cx, |ed, cx| ed.select_file(&expenses_path, window, cx));
    })
    .unwrap();
    cx.run_until_parked();

    for _ in 0..2 {
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
        })
        .unwrap();
    }

    // Multiple errors on line 1 merge into a single highlighted row (never double-painted),
    // and hovering line 1 displays both error messages.
    cx.update_window(handle, |_, window, cx| {
        let ed = editor.read(cx);
        let errors = ed.error_lines(cx);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].0, 1);
        assert!(
            errors[0].1.len() >= 2,
            "expected >= 2 errors on line 1, got {:?}",
            errors[0].1
        );
        let painted = ed.painted_error_lines();
        assert_eq!(painted.len(), 1);
        assert_eq!(painted[0].0, 1);

        let hover_pt = point(
            painted[0].1.origin.x + px(80.),
            painted[0].1.origin.y + painted[0].1.size.height * 0.5,
        );
        window.pointer_move(hover_pt, None, cx);
        window.render_frame(cx);
        let hover_label = window
            .find("source-editor-error-hover")
            .label()
            .expect("hover label")
            .to_string();
        for msg in &errors[0].1 {
            assert!(
                hover_label.contains(msg.as_ref()),
                "hover label {hover_label:?} missing {msg:?}"
            );
        }
    })
    .unwrap();

    #[cfg(target_os = "macos")]
    let undo_shortcut = "cmd-z";
    #[cfg(not(target_os = "macos"))]
    let undo_shortcut = "ctrl-z";

    // 1. Pressing Enter at the END of line 1 (`column > 0`) keeps the error on line 1.
    cx.update_window(handle, |_, window, cx| {
        let state = editor.read(cx).editor_state().clone();
        state.update(cx, |state, cx| {
            let line1_end = state.text().line_end_offset(0);
            state.set_selected_range(line1_end..line1_end, cx);
            state.focus(window, cx);
        });
        window.render_frame(cx);
        window.press("enter", cx);
        window.render_frame(cx);
        assert_eq!(editor.read(cx).error_lines(cx)[0].0, 1);

        // Undo back to clean.
        window.press(undo_shortcut, cx);
        window.render_frame(cx);
        assert!(!editor.read(cx).is_changed(cx));
        assert_eq!(editor.read(cx).error_lines(cx)[0].0, 1);

        // 2. Pressing Enter at COLUMN 0 of line 1 shifts the error down to line 2,
        // and Backspace pulls it back to line 1.
        state.update(cx, |state, cx| {
            state.set_selected_range(0..0, cx);
            state.focus(window, cx);
        });
        window.render_frame(cx);
        window.press("enter", cx);
        window.render_frame(cx);
        assert_eq!(editor.read(cx).error_lines(cx)[0].0, 2);

        window.press("backspace", cx);
        window.render_frame(cx);
        assert_eq!(editor.read(cx).error_lines(cx)[0].0, 1);

        // 3. Selecting all and deleting line 1 drops the highlight; Undo back to
        // original restores it.
        state.update(cx, |state, cx| {
            let len = state.text().len();
            state.set_selected_range(0..len, cx);
            state.focus(window, cx);
        });
        window.render_frame(cx);
        window.press("backspace", cx);
        window.render_frame(cx);
        assert!(editor.read(cx).error_lines(cx).is_empty());
        assert!(editor.read(cx).painted_error_lines().is_empty());

        window.press(undo_shortcut, cx);
        window.render_frame(cx);
        assert!(!editor.read(cx).is_changed(cx));
        assert_eq!(editor.read(cx).error_lines(cx).len(), 1);
        assert_eq!(editor.read(cx).error_lines(cx)[0].0, 1);
    })
    .unwrap();
}

#[gpui_kit::test]
fn test_source_editor_error_highlight_and_hover_follow_elastic_overscroll_near_eof(
    cx: &mut TestAppContext,
) {
    let ledger = MultiFileLedger::new("error-elastic-overscroll-eof");
    // Build a 60-line file with an error near EOF (line 58) so scrolling past
    // the end stretches the text upward via elastic overscroll (`scroll_bounce`).
    let mut lines: Vec<String> = (1..=57).map(|i| format!("; comment line {i}")).collect();
    lines.push("2025-01-15 balance Assets:Cash 999.00 USD".to_string()); // line 58 (error)
    lines.push("; trailing comment 59".to_string());
    lines.push("; trailing comment 60".to_string());
    fs::write(&ledger.expenses_path, format!("{}\n", lines.join("\n"))).unwrap();

    let (handle, app) = open_editor(&ledger, cx);
    let editor = cx.read(|cx| app.read(cx).editor().expect("editor page is open"));
    let expenses_path = ledger.expenses_str();

    cx.update_window(handle, |_, window, cx| {
        editor.update(cx, |ed, cx| ed.select_file(&expenses_path, window, cx));
    })
    .unwrap();
    cx.run_until_parked();

    cx.update_window(handle, |_, window, cx| {
        let state = editor.read(cx).editor_state().clone();
        state.update(cx, |state, cx| state.set_scroll_bounce(true, cx));
        window.render_frame(cx);
        window.render_frame(cx);

        // First scroll to the clamped EOF without overscroll (MouseWheel / Ended)
        // to measure the resting y of line 58 at the bottom of the file.
        let scroll_pt = {
            let b = state.read(cx).input_bounds();
            point(b.origin.x + px(120.), b.origin.y + px(120.))
        };
        state.update(cx, |state, cx| {
            state.set_scroll_offset(point(px(0.), px(-100_000.)), cx);
        });
        window.render_frame(cx);

        let resting_painted_y = {
            let painted = editor.read(cx).painted_error_lines();
            assert_eq!(painted.len(), 1);
            assert_eq!(painted[0].0, 58);
            painted[0].1.origin.y
        };
        let resting_glyph_y = state.update(cx, |state, cx| {
            let tb = state.text_bounds().unwrap();
            let off = state.text().line_start_offset(57);
            state
                .bounds_for_range(off..off + 4, tb, window, cx)
                .unwrap()
                .origin
                .y
        });
        assert!((resting_painted_y - resting_glyph_y).abs() < px(1.));

        // Now perform a trackpad swipe upward past EOF (`TouchPhase::Started` +
        // `TouchPhase::Moved`), pulling line 58 further up via elastic stretch.
        for (phase, dy) in [(TouchPhase::Started, -20.0), (TouchPhase::Moved, -5000.0)] {
            window.dispatch_event(
                PlatformInput::Scroll(ScrollEvent {
                    position: scroll_pt,
                    delta: ScrollDelta::Pixels(point(px(0.), px(dy))),
                    touch_phase: phase,
                    ..Default::default()
                }),
                cx,
            );
        }
        window.render_frame(cx);

        let stretched_painted = editor.read(cx).painted_error_lines();
        assert_eq!(stretched_painted.len(), 1);
        let stretched_painted_y = stretched_painted[0].1.origin.y;
        let stretched_glyph_y = state.update(cx, |state, cx| {
            let tb = state.text_bounds().unwrap();
            let off = state.text().line_start_offset(57);
            state
                .bounds_for_range(off..off + 4, tb, window, cx)
                .unwrap()
                .origin
                .y
        });

        // Elastic overscroll actually pulled the text upward past the resting EOF:
        assert!(
            stretched_glyph_y < resting_glyph_y - px(10.),
            "expected elastic stretch upward: stretched={stretched_glyph_y:?}, resting={resting_glyph_y:?}"
        );
        // And the light-red error background moved up by the exact same stretch!
        assert!(
            (stretched_painted_y - stretched_glyph_y).abs() < px(1.),
            "error background {stretched_painted_y:?} did not follow stretched glyph {stretched_glyph_y:?}"
        );

        // Hovering over the stretched error line hits line 58 at its stretched position.
        window.pointer_move(
            point(
                stretched_painted[0].1.origin.x + px(100.),
                stretched_painted_y + stretched_painted[0].1.size.height * 0.5,
            ),
            None,
            cx,
        );
        window.render_frame(cx);
        assert!(window.try_find("source-editor-error-hover").is_some());

        // Release the trackpad gesture (`TouchPhase::Ended`) and let the bounce
        // spring back to rest.
        window.dispatch_event(
            PlatformInput::Scroll(ScrollEvent {
                position: scroll_pt,
                delta: ScrollDelta::Pixels(point(px(0.), px(0.))),
                touch_phase: TouchPhase::Ended,
                ..Default::default()
            }),
            cx,
        );
    })
    .unwrap();

    for _ in 0..120 {
        cx.executor().advance_clock(Duration::from_millis(16));
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
        })
        .unwrap();
    }

    cx.update_window(handle, |_, window, cx| {
        let state = editor.read(cx).editor_state().clone();
        let final_painted_y = editor.read(cx).painted_error_lines()[0].1.origin.y;
        let final_glyph_y = state.update(cx, |state, cx| {
            let tb = state.text_bounds().unwrap();
            let off = state.text().line_start_offset(57);
            state
                .bounds_for_range(off..off + 4, tb, window, cx)
                .unwrap()
                .origin
                .y
        });
        assert!((final_painted_y - final_glyph_y).abs() < px(1.));
    })
    .unwrap();
}
