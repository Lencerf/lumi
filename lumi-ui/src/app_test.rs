use std::fs;
use std::path::{Path, PathBuf};

use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{
    AnyWindowHandle, AppContext as _, Bounds, ElementId, Entity, SharedString, TestAppContext,
    WindowBounds, WindowOptions, px, size,
};

use crate::app::{
    LumiApp, ShowBalanceSheet, ShowEditor, ShowErrors, ShowHoldings, ShowIncome, ShowJournal,
    ledger_name_for_path,
};
use crate::navigation::{Page, ReportEvent};

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../lumi/src/query_test.bean");

const VAULT_LEDGER: &str = "\
2025-01-01 open Assets:Vault
2025-01-01 open Equity:Opening

2025-01-02 * \"Seed vault\"
  Assets:Vault              500.00 USD
  Equity:Opening           -500.00 USD
";

struct TempLedger(PathBuf);

impl TempLedger {
    fn new(name: &str, contents: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("lumi-app-test-{}-{name}.bean", std::process::id()));
        fs::write(&path, contents).unwrap();
        Self(path)
    }

    fn file_name(&self) -> SharedString {
        ledger_name_for_path(&self.0)
    }
}

impl Drop for TempLedger {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn open_app(
    path: Option<&Path>,
    page: Page,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<LumiApp>) {
    cx.update(crate::init);
    let path = path.map(Path::to_path_buf);
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
            move |window, cx| cx.new(|cx| LumiApp::new_local(path.as_deref(), page, window, cx)),
        )
        .expect("open test window")
    });
    cx.run_until_parked();
    (handle, app)
}

#[test]
fn test_ledger_name_for_path() {
    assert_eq!(
        ledger_name_for_path(Path::new("/ledgers/2026/main.bean")),
        SharedString::from("main.bean")
    );
    assert_eq!(
        ledger_name_for_path(Path::new("main.bean")),
        SharedString::from("main.bean")
    );
    assert_eq!(
        ledger_name_for_path(Path::new("/")),
        SharedString::from("/")
    );
}

#[gpui_kit::test]
fn test_starting_without_a_ledger_then_selecting_one_from_the_empty_state(cx: &mut TestAppContext) {
    let (handle, app) = open_app(None, Page::BalanceSheet, cx);

    cx.read(|cx| {
        let app = app.read(cx);
        assert_eq!(app.ledger_name(), None);
        let session = app.session().read(cx);
        assert!(!session.has_source());
        assert!(!session.is_loaded());
        assert_eq!(session.revision(), 0);
    });

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("refresh").disabled(), Some(true));
        assert!(window.try_find("open-ledger").is_some());
        window.click("open-ledger-empty", cx);
    })
    .unwrap();

    assert!(cx.did_prompt_for_paths());
    cx.simulate_path_prompt_response(|options| {
        assert!(options.files);
        assert!(!options.directories);
        assert!(!options.multiple);
        Some(vec![PathBuf::from(FIXTURE)])
    });
    cx.run_until_parked();

    cx.read(|cx| {
        let app = app.read(cx);
        assert_eq!(
            app.ledger_name(),
            Some(&SharedString::from("query_test.bean"))
        );
        let session = app.session().read(cx);
        assert!(session.has_source());
        assert!(session.is_loaded());
        assert_eq!(session.revision(), 1);
    });

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("open-ledger-empty").is_none());
        assert_ne!(window.find("refresh").disabled(), Some(true));
        assert!(
            window
                .try_find((ElementId::from("account"), "Assets:Cash"))
                .is_some()
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn test_clicking_the_root_file_in_the_sidebar_switches_ledgers(cx: &mut TestAppContext) {
    let vault = TempLedger::new("vault", VAULT_LEDGER);
    let (handle, app) = open_app(
        Some(Path::new(FIXTURE)),
        Page::Account("Assets:Cash".into()),
        cx,
    );

    cx.read(|cx| {
        let app = app.read(cx);
        assert_eq!(
            app.ledger_name(),
            Some(&SharedString::from("query_test.bean"))
        );
        assert_eq!(app.page(), &Page::Account("Assets:Cash".into()));
        assert_eq!(app.session().read(cx).revision(), 1);
    });

    // Cancelling the file picker leaves the current ledger and page untouched.
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("open-ledger", cx);
    })
    .unwrap();
    assert!(cx.did_prompt_for_paths());
    cx.simulate_path_prompt_response(|_| None);
    cx.run_until_parked();

    cx.read(|cx| {
        let app = app.read(cx);
        assert_eq!(
            app.ledger_name(),
            Some(&SharedString::from("query_test.bean"))
        );
        assert_eq!(app.page(), &Page::Account("Assets:Cash".into()));
        assert_eq!(app.session().read(cx).revision(), 1);
    });

    // Choosing another root file reloads into it and resets an account page to
    // the balance sheet of the new chart of accounts.
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("open-ledger", cx);
    })
    .unwrap();
    let vault_path = vault.0.clone();
    cx.simulate_path_prompt_response(move |_| Some(vec![vault_path]));
    cx.run_until_parked();

    let expected_name = vault.file_name();
    cx.read(|cx| {
        let app = app.read(cx);
        assert_eq!(app.ledger_name(), Some(&expected_name));
        assert_eq!(app.page(), &Page::BalanceSheet);
        assert_eq!(app.session().read(cx).revision(), 2);
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window
                .try_find((ElementId::from("account"), "Assets:Vault"))
                .is_some()
        );
        assert!(
            window
                .try_find((ElementId::from("account"), "Assets:Cash"))
                .is_none()
        );
    })
    .unwrap();

    // Switching while on a report page keeps that page and loads the new
    // ledger's entries into it.
    cx.update_window(handle, |_, window, cx| {
        window.within("lumi-sidebar").click("0-0-2", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.read(|cx| {
        let report = app.read(cx).journal().expect("the journal is shown");
        assert_eq!(report.read(cx).total(cx), 1);
    });

    let shortcut = if cfg!(target_os = "macos") {
        "cmd-o"
    } else {
        "ctrl-o"
    };
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.press(shortcut, cx);
    })
    .unwrap();
    assert!(cx.did_prompt_for_paths());
    cx.simulate_path_prompt_response(|_| Some(vec![PathBuf::from(FIXTURE)]));
    cx.run_until_parked();

    cx.read(|cx| {
        let app = app.read(cx);
        assert_eq!(
            app.ledger_name(),
            Some(&SharedString::from("query_test.bean"))
        );
        assert_eq!(app.page(), &Page::Journal);
        let report = app.journal().expect("the journal is shown");
        assert_eq!(report.read(cx).total(cx), 9);
    });
}

#[gpui_kit::test]
fn test_go_back_returns_from_account_view_to_the_last_view(cx: &mut TestAppContext) {
    let (handle, app) = open_app(Some(Path::new(FIXTURE)), Page::Income, cx);

    // Universal Back/Forward buttons start disabled before any navigation.
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("go-back").disabled(), Some(true));
        assert_eq!(window.find("go-forward").disabled(), Some(true));
        window.click((ElementId::from("account"), "Expenses:Food"), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.read(|cx| assert_eq!(app.read(cx).page(), &Page::Account("Expenses:Food".into())));

    // Expand the first transaction in Expenses:Food and open its counter-account.
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("go-back").disabled(), None);
        assert_eq!(window.find("go-forward").disabled(), Some(true));
        window.click(("postings", 0usize), cx);
    })
    .unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(("posting-account", 1usize), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.read(|cx| assert_eq!(app.read(cx).page(), &Page::Account("Assets:Cash".into())));

    // First Back returns to Expenses:Food; second returns to Income statement.
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("go-back", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.read(|cx| assert_eq!(app.read(cx).page(), &Page::Account("Expenses:Food".into())));

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("go-back").disabled(), None);
        assert_eq!(window.find("go-forward").disabled(), None);
        window.click("go-back", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.read(|cx| assert_eq!(app.read(cx).page(), &Page::Income));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("go-back").disabled(), Some(true));
        assert_eq!(window.find("go-forward").disabled(), None);
    })
    .unwrap();
}

#[gpui_kit::test]
fn test_universal_back_forward_across_views_and_forward_branch_clearing(cx: &mut TestAppContext) {
    let vault = TempLedger::new("history-vault", VAULT_LEDGER);
    let (handle, app) = open_app(Some(Path::new(FIXTURE)), Page::BalanceSheet, cx);

    // Navigate across sidebar reports and an account view:
    // BalanceSheet -> Income -> Account(Expenses:Food) -> Journal -> Editor
    cx.update_window(handle, |_, window, cx| {
        window.dispatch_action(Box::new(ShowIncome), cx);
    })
    .unwrap();
    cx.run_until_parked();

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click((ElementId::from("account"), "Expenses:Food"), cx);
    })
    .unwrap();
    cx.run_until_parked();

    cx.update_window(handle, |_, window, cx| {
        window.dispatch_action(Box::new(ShowJournal), cx);
    })
    .unwrap();
    cx.run_until_parked();

    cx.update_window(handle, |_, window, cx| {
        window.dispatch_action(Box::new(ShowEditor), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.read(|cx| assert_eq!(app.read(cx).page(), &Page::Editor));

    let (back_key, forward_key) = if cfg!(target_os = "macos") {
        ("cmd-[", "cmd-]")
    } else {
        ("alt-left", "alt-right")
    };

    // Step back twice: Editor -> Journal -> Account(Expenses:Food).
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("go-back", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.read(|cx| assert_eq!(app.read(cx).page(), &Page::Journal));

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.press(back_key, cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.read(|cx| assert_eq!(app.read(cx).page(), &Page::Account("Expenses:Food".into())));

    // Step forward once via shortcut (Account -> Journal), then once via button (Journal -> Editor).
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.press(forward_key, cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.read(|cx| assert_eq!(app.read(cx).page(), &Page::Journal));

    // Re-selecting the already-active view is a no-op and preserves Forward history.
    cx.update_window(handle, |_, window, cx| {
        window.dispatch_action(Box::new(ShowJournal), cx);
    })
    .unwrap();
    cx.run_until_parked();

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("go-forward").disabled(), None);
        window.click("go-forward", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.read(|cx| assert_eq!(app.read(cx).page(), &Page::Editor));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("go-forward").disabled(), Some(true));
    })
    .unwrap();

    // Go back twice to Account(Expenses:Food), then visit Holdings: forward stack clears.
    for _ in 0..2 {
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.click("go-back", cx);
        })
        .unwrap();
        cx.run_until_parked();
    }
    cx.read(|cx| assert_eq!(app.read(cx).page(), &Page::Account("Expenses:Food".into())));

    cx.update_window(handle, |_, window, cx| {
        window.dispatch_action(Box::new(ShowHoldings), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.read(|cx| assert_eq!(app.read(cx).page(), &Page::Holdings));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("go-back").disabled(), None);
        assert_eq!(window.find("go-forward").disabled(), Some(true));
    })
    .unwrap();

    // Switching root ledgers resets both Back and Forward stacks.
    let vault_path = vault.0.clone();
    cx.update_window(handle, |_, window, cx| {
        app.update(cx, |app, cx| app.open_path(&vault_path, window, cx));
        window.render_frame(cx);
        assert_eq!(window.find("go-back").disabled(), Some(true));
        assert_eq!(window.find("go-forward").disabled(), Some(true));
    })
    .unwrap();
}

#[gpui_kit::test]
fn test_back_forward_remembers_last_ten_places(cx: &mut TestAppContext) {
    let (handle, app) = open_app(Some(Path::new(FIXTURE)), Page::BalanceSheet, cx);

    // Visit 11 more distinct places after starting on BalanceSheet (P0..P11 = 12 total).
    let sequence = [
        Page::Income,
        Page::Journal,
        Page::Holdings,
        Page::Errors,
        Page::Editor,
        Page::Account("Assets:Cash".into()),
        Page::Account("Assets:Bank:Checking".into()),
        Page::Account("Expenses:Food".into()),
        Page::Account("Income:Salary".into()),
        Page::BalanceSheet,
        Page::Journal,
    ];

    for next in &sequence {
        let next = next.clone();
        cx.update_window(handle, |_, window, cx| match &next {
            Page::BalanceSheet => window.dispatch_action(Box::new(ShowBalanceSheet), cx),
            Page::Income => window.dispatch_action(Box::new(ShowIncome), cx),
            Page::Journal => window.dispatch_action(Box::new(ShowJournal), cx),
            Page::Holdings => window.dispatch_action(Box::new(ShowHoldings), cx),
            Page::Errors => window.dispatch_action(Box::new(ShowErrors), cx),
            Page::Editor => window.dispatch_action(Box::new(ShowEditor), cx),
            Page::Account(acct) => app.update(cx, |app, cx| {
                app.on_report_event(
                    &app.session().clone(),
                    &ReportEvent::OpenAccount(acct.clone()),
                    window,
                    cx,
                );
            }),
        })
        .unwrap();
        cx.run_until_parked();
    }
    cx.read(|cx| assert_eq!(app.read(cx).page(), &Page::Journal));

    // Only the last 10 places before the current view (sequence[0..10]) are kept;
    // the initial Page::BalanceSheet (P0) has rolled off the history cap.
    let expected_back: Vec<Page> = sequence[..10].iter().rev().cloned().collect();
    for expected in &expected_back {
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(window.find("go-back").disabled(), None);
            window.click("go-back", cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.read(|cx| assert_eq!(app.read(cx).page(), expected));
    }

    // After 10 Back steps we are at `Page::Income` (sequence[0]), and Back is now disabled.
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("go-back").disabled(), Some(true));
        assert_eq!(window.find("go-forward").disabled(), None);
    })
    .unwrap();

    // Forward steps through all 10 places right back to the final `Page::Journal`.
    for expected in &sequence[1..] {
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(window.find("go-forward").disabled(), None);
            window.click("go-forward", cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.read(|cx| assert_eq!(app.read(cx).page(), expected));
    }

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("go-back").disabled(), None);
        assert_eq!(window.find("go-forward").disabled(), Some(true));
    })
    .unwrap();
}

#[gpui_kit::test]
fn test_open_path_loads_bean_file_on_cold_start_and_switches_when_running(cx: &mut TestAppContext) {
    let vault = TempLedger::new("finder-vault", VAULT_LEDGER);
    let (handle, app) = open_app(None, Page::BalanceSheet, cx);

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("open-ledger-empty").is_some());
    })
    .unwrap();

    // Simulate the OS file manager delivering a `.bean` path right after launch.
    cx.update_window(handle, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.open_path(Path::new(FIXTURE), window, cx);
        });
    })
    .unwrap();
    cx.run_until_parked();

    let expected_fixture_path: SharedString = FIXTURE.into();
    cx.read(|cx| {
        let app = app.read(cx);
        assert_eq!(
            app.ledger_name(),
            Some(&SharedString::from("query_test.bean"))
        );
        assert_eq!(app.ledger_path(), Some(&expected_fixture_path));
        let session = app.session().read(cx);
        assert!(session.has_source());
        assert!(session.is_loaded());
        assert_eq!(session.revision(), 1);
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("open-ledger-empty").is_none());
        assert!(
            window
                .try_find((ElementId::from("account"), "Assets:Cash"))
                .is_some()
        );
    })
    .unwrap();

    // Simulate double-clicking another `.bean` file while the app is already open.
    let vault_path = vault.0.clone();
    cx.update_window(handle, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.open_path(&vault_path, window, cx);
        });
    })
    .unwrap();
    cx.run_until_parked();

    let expected_vault_name = vault.file_name();
    let expected_vault_path: SharedString = vault.0.to_string_lossy().into_owned().into();
    cx.read(|cx| {
        let app = app.read(cx);
        assert_eq!(app.ledger_name(), Some(&expected_vault_name));
        assert_eq!(app.ledger_path(), Some(&expected_vault_path));
        assert_eq!(app.session().read(cx).revision(), 2);
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window
                .try_find((ElementId::from("account"), "Assets:Vault"))
                .is_some()
        );
        assert!(
            window
                .try_find((ElementId::from("account"), "Assets:Cash"))
                .is_none()
        );
    })
    .unwrap();
}
