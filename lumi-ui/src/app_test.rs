use std::fs;
use std::path::{Path, PathBuf};

use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{
    AnyWindowHandle, AppContext as _, Bounds, ElementId, Entity, Pixels, ScrollDelta, SharedString,
    Size, TestAppContext, WindowBounds, WindowOptions, point, px, size,
};

use crate::app::{
    LumiApp, Refresh, ShowBalanceSheet, ShowEditor, ShowErrors, ShowHoldings, ShowIncome,
    ShowJournal, ledger_name_for_path,
};
use crate::navigation::{Page, ReportEvent};
use crate::style::report_horizontal_overhead;

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

const AUTO_WIDTH_LEDGER: &str = "\
2024-01-01 open Assets:Bank:Checking
2024-01-01 open Assets:Investments:GlobalBrokerageAccount
2024-01-01 open Liabilities:Mortgage:PrimaryResidence
2024-01-01 open Equity:Opening-Balances
2024-01-01 open Expenses:Housing

2024-01-02 * \"Opening multi-currency balances\"
  Assets:Bank:Checking                        125000.00 USD
  Assets:Bank:Checking                         48000.00 EUR
  Assets:Bank:Checking                         92000.00 CAD
  Assets:Bank:Checking                       8500000    JPY
  Liabilities:Mortgage:PrimaryResidence      -350000.00 USD
  Equity:Opening-Balances

2024-03-01 * \"Vanguard\" \"Quarterly international portfolio rebalance across developed markets\"
  Assets:Investments:GlobalBrokerageAccount  250.1234 VTI {250.00 USD, 2024-03-01} @ 255.00 USD
  Assets:Bank:Checking                        -62530.85 USD

2024-04-01 * \"Unbalanced inspection entry with detailed diagnostic location context\"
  Expenses:Housing                              1200.00 USD
  Assets:Bank:Checking                         -1100.00 USD
";

fn open_app_sized(
    path: &Path,
    page: Page,
    initial_size: Size<Pixels>,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<LumiApp>) {
    cx.update(crate::init);
    let path = path.to_path_buf();
    let (handle, app) = cx.update(|cx| {
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    Default::default(),
                    initial_size,
                ))),
                ..Default::default()
            },
            cx,
            move |window, cx| cx.new(|cx| LumiApp::new_local(Some(&path), page, window, cx)),
        )
        .expect("open test window")
    });
    cx.run_until_parked();
    (handle, app)
}

#[gpui_kit::test]
fn test_auto_window_and_column_widths_across_sidebar_and_account_views(cx: &mut TestAppContext) {
    let ledger = TempLedger::new("auto-width", AUTO_WIDTH_LEDGER);

    // 0. Upon opening Balance Sheet directly (as on app launch), Liabilities (1
    //    currency) and Equity (4 currencies) have the exact same card width.
    let (bs_open_handle, _bs_open_app) =
        open_app_sized(&ledger.0, Page::BalanceSheet, size(px(1280.), px(760.)), cx);
    cx.update_window(bs_open_handle, |_, window, cx| {
        window.render_frame(cx);
        let liab_card = window
            .find((ElementId::from("trie-table"), "Liabilities"))
            .bounds();
        let equity_card = window
            .find((ElementId::from("trie-table"), "Equity"))
            .bounds();
        assert_eq!(
            liab_card.size.width, equity_card.size.width,
            "Upon open, Liabilities and Equity tables have identical widths: liab={liab_card:?}, equity={equity_card:?}"
        );
        assert_eq!(liab_card.origin.x, equity_card.origin.x);
    })
    .unwrap();

    // Start in a narrow 680px window on Holdings.
    let (handle, app) = open_app_sized(&ledger.0, Page::Holdings, size(px(680.), px(760.)), cx);

    // 1. Holdings auto-fits its columns and window width on open.
    let holdings_win_width = cx
        .update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let holdings = app.read(cx).holdings().expect("holdings open");
            let cols = holdings.read(cx).column_widths(cx);
            let naturals = holdings.read(cx).natural_column_widths(cx);
            assert_eq!(
                cols, naturals,
                "Holdings columns sit at exact content-fitted widths when window fits table"
            );
            let expected_win = report_horizontal_overhead(window)
                + naturals.iter().copied().sum::<Pixels>()
                + px(2.);
            assert!(
                (window.viewport_size().width - expected_win).abs() <= px(2.),
                "Holdings window width {:?} matches table width {:?}",
                window.viewport_size().width,
                expected_win
            );
            window.viewport_size().width
        })
        .unwrap();

    // 2. Clicking an account button enters the Account view and widens the
    //    window so all 6 account columns (`Date`, `Flag`, `Description`,
    //    `Position`, `Change`, `Balance`) fit their contents.
    cx.update_window(handle, |_, window, cx| {
        let row_ix = (0..10usize)
            .find(|&ix| {
                window
                    .try_find(("holding-account", ix))
                    .and_then(|el| el.label().map(|s| s.to_string()))
                    .as_deref()
                    == Some("Open account Assets:Investments:GlobalBrokerageAccount")
            })
            .expect("brokerage holding row");
        window.click(("holding-account", row_ix), cx);
    })
    .unwrap();
    cx.run_until_parked();
    let acct_win_width = cx
        .update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(
                app.read(cx).page(),
                &Page::Account("Assets:Investments:GlobalBrokerageAccount".into())
            );
            let journal = app.read(cx).journal().expect("account journal open");
            let cols = journal.read(cx).column_widths(cx);
            let naturals = journal.read(cx).natural_column_widths(cx);
            assert_eq!(cols.len(), 6);
            assert_eq!(cols, naturals);
            let expected_win = report_horizontal_overhead(window)
                + naturals.iter().copied().sum::<Pixels>()
                + px(2.);
            assert!(
                (window.viewport_size().width - expected_win).abs() <= px(2.)
                    && window.viewport_size().width > holdings_win_width,
                "Account view window width {:?} widens to fit all 6 columns ({expected_win:?})",
                window.viewport_size().width
            );
            window.viewport_size().width
        })
        .unwrap();

    // 3. Opening Multi-Currency Balance Sheet from the left sidebar widens the
    //    window further so both side-by-side hierarchy columns fit, with
    //    Liabilities and Equity sharing the exact same width.
    cx.update_window(handle, |_, window, cx| {
        window.within("lumi-sidebar").click("0-0-0", cx);
    })
    .unwrap();
    cx.run_until_parked();
    let bs_win_width = cx
        .update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(app.read(cx).page(), &Page::BalanceSheet);
            let w = window.viewport_size().width;
            assert!(
                w > acct_win_width,
                "Balance Sheet ({w:?}) widens past narrower Account view ({acct_win_width:?})"
            );
            let assets_card = window.find((ElementId::from("trie-table"), "Assets")).bounds();
            let liab_card = window
                .find((ElementId::from("trie-table"), "Liabilities"))
                .bounds();
            let equity_card = window
                .find((ElementId::from("trie-table"), "Equity"))
                .bounds();
            assert_eq!(liab_card.size.width, equity_card.size.width);
            assert!(
                liab_card.origin.x + liab_card.size.width <= w,
                "Right-hand Balance Sheet column fits inside auto-adjusted window: liab={liab_card:?}, win={w:?}"
            );
            assert!(assets_card.origin.x < liab_card.origin.x);
            w
        })
        .unwrap();

    // 4. Navigating back to narrower Holdings via the sidebar does NOT narrow
    //    the window since it is already wide enough; Holdings columns widen
    //    proportionally to fill the existing window.
    cx.update_window(handle, |_, window, cx| {
        window.within("lumi-sidebar").click("0-0-3", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let w = window.viewport_size().width;
        assert_eq!(
            w, bs_win_width,
            "Switching to narrower Holdings keeps existing window width {bs_win_width:?}, got {w:?}"
        );
        let holdings = app.read(cx).holdings().expect("holdings open");
        let cols = holdings.read(cx).column_widths(cx);
        let naturals = holdings.read(cx).natural_column_widths(cx);
        for (&col, &nat) in cols.iter().zip(&naturals) {
            assert!(col > nat);
        }
    })
    .unwrap();

    // 5. Opening Journal from the sidebar and toggling "Show postings" reveals
    //    wide lot/price strings in `Position` (`250.1234 VTI {250.00 USD, ...}`):
    //    when the expanded table exceeds the current window, both columns and
    //    window widen automatically.
    cx.update_window(handle, |_, window, cx| {
        window.within("lumi-sidebar").click("0-0-2", cx);
    })
    .unwrap();
    cx.run_until_parked();
    let pos_col_collapsed = cx
        .update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let journal = app.read(cx).journal().expect("journal open");
            journal.read(cx).natural_column_widths(cx)[3]
        })
        .unwrap();

    cx.update_window(handle, |_, window, cx| {
        window.click((ElementId::from("show-postings"), "journal"), cx);
    })
    .unwrap();
    cx.run_until_parked();
    let journal_expanded_win = cx
        .update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let journal = app.read(cx).journal().expect("journal open");
            let naturals = journal.read(cx).natural_column_widths(cx);
            let cols = journal.read(cx).column_widths(cx);
            assert!(
                naturals[3] > pos_col_collapsed + px(80.),
                "Position column widens for lot/price notation: before={pos_col_collapsed:?}, after={:?}",
                naturals[3]
            );
            for (&col, &nat) in cols.iter().zip(&naturals) {
                assert!(col >= nat);
            }
            window.viewport_size().width
        })
        .unwrap();

    // 6. Switching from wide Journal to narrower Errors does NOT narrow the
    //    window, and Errors' 3 columns scale proportionally to fill it.
    cx.update_window(handle, |_, window, cx| {
        window.within("lumi-sidebar").click("0-0-4", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.viewport_size().width, journal_expanded_win,
            "Switching from Journal to Errors must not narrow the window"
        );
        let errors = app.read(cx).errors().expect("errors open");
        let cols = errors.read(cx).column_widths(cx);
        let naturals = errors.read(cx).natural_column_widths(cx);
        assert_eq!(cols.len(), 3);
        let natural_win =
            report_horizontal_overhead(window) + naturals.iter().copied().sum::<Pixels>() + px(2.);
        assert!(
            natural_win < journal_expanded_win,
            "Errors natural width ({natural_win:?}) is narrower than Journal ({journal_expanded_win:?})"
        );
        for (&col, &nat) in cols.iter().zip(&naturals) {
            assert!(col > nat);
        }
    })
    .unwrap();
}

#[gpui_kit::test]
fn test_manual_window_widen_scales_columns_proportionally_and_narrow_scrolls_tables_and_charts(
    cx: &mut TestAppContext,
) {
    let ledger = TempLedger::new("manual-resize", AUTO_WIDTH_LEDGER);
    let (handle, app) = open_app_sized(&ledger.0, Page::Holdings, size(px(800.), px(760.)), cx);

    // --- Rule 2 on DataTable (`Holdings`) ---
    let (holdings_natural_win, holdings_naturals) = cx
        .update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let holdings = app.read(cx).holdings().expect("holdings open");
            (
                window.viewport_size().width,
                holdings.read(cx).natural_column_widths(cx),
            )
        })
        .unwrap();

    // User manually widens the window by +360px.
    let manual_wide = holdings_natural_win + px(360.);
    cx.update_window(handle, |_, window, cx| {
        window.resize(size(manual_wide, px(760.)));
        window.bounds_changed(cx);
        window.render_frame(cx);

        let holdings = app.read(cx).holdings().expect("holdings open");
        let scaled = holdings.read(cx).column_widths(cx);
        assert_eq!(scaled.len(), holdings_naturals.len());
        for (ix, (&w_scaled, &w_nat)) in scaled.iter().zip(&holdings_naturals).enumerate() {
            assert!(
                w_scaled > w_nat,
                "Column {ix} must widen proportionally when window widens: scaled={w_scaled:?}, nat={w_nat:?}"
            );
        }
        let expected_table_inner = manual_wide - report_horizontal_overhead(window) - px(2.);
        let sum_scaled: Pixels = scaled.iter().copied().sum();
        assert!(
            (sum_scaled - expected_table_inner).abs() <= px(1.),
            "Scaled columns fill table card with no trailing gap: sum={sum_scaled:?}, inner={expected_table_inner:?}"
        );
        // Check proportionality: ratio of col[0]/sum matches natural[0]/natural_sum within rounding.
        let nat_sum: f32 = holdings_naturals.iter().map(|&w| f32::from(w)).sum();
        let expected_col0 =
            f32::from(holdings_naturals[0]) / nat_sum * f32::from(expected_table_inner);
        assert!(
            (f32::from(scaled[0]) - expected_col0).abs() <= 2.0,
            "col[0] {:?} scales proportionally (~{expected_col0})",
            scaled[0]
        );
    })
    .unwrap();

    // Refreshing the ledger while manually resized preserves the user's manual window width.
    cx.update_window(handle, |_, window, cx| {
        window.dispatch_action(Box::new(Refresh), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.viewport_size().width, manual_wide);
    })
    .unwrap();

    // --- Rule 2 & Rule 3 on `BalanceSheet` (Chart + Hierarchy Tables) ---
    let (handle, app) =
        open_app_sized(&ledger.0, Page::BalanceSheet, size(px(1280.), px(760.)), cx);
    let (bs_natural_win, assets_nat_w, liab_nat_w) = cx
        .update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let assets_w = window
                .find((ElementId::from("trie-table"), "Assets"))
                .bounds()
                .size
                .width;
            let liab_w = window
                .find((ElementId::from("trie-table"), "Liabilities"))
                .bounds()
                .size
                .width;
            let equity_w = window
                .find((ElementId::from("trie-table"), "Equity"))
                .bounds()
                .size
                .width;
            assert_eq!(liab_w, equity_w, "Liabilities and Equity match upon open");
            (window.viewport_size().width, assets_w, liab_w)
        })
        .unwrap();

    // Rule 2: User widens Balance Sheet window by +400px -> chart and both
    // hierarchy table cards widen proportionally to span the exact same width,
    // and Liabilities and Equity still have the exact same width.
    let bs_wide = bs_natural_win + px(400.);
    cx.update_window(handle, |_, window, cx| {
        window.resize(size(bs_wide, px(760.)));
        window.bounds_changed(cx);
        window.render_frame(cx);

        let chart_bounds = window
            .find((ElementId::from("chart-card"), "balance-sheet"))
            .bounds();
        let assets_bounds = window
            .find((ElementId::from("trie-table"), "Assets"))
            .bounds();
        let liab_bounds = window
            .find((ElementId::from("trie-table"), "Liabilities"))
            .bounds();
        let equity_bounds = window
            .find((ElementId::from("trie-table"), "Equity"))
            .bounds();
        assert_eq!(
            liab_bounds.size.width, equity_bounds.size.width,
            "Liabilities and Equity match when widened"
        );
        assert!(assets_bounds.size.width > assets_nat_w + px(100.));
        assert!(liab_bounds.size.width > liab_nat_w + px(80.));
        let chart_right = chart_bounds.origin.x + chart_bounds.size.width;
        let tables_right = liab_bounds.origin.x + liab_bounds.size.width;
        assert!(
            (chart_right - tables_right).abs() <= px(2.),
            "Widened Balance Sheet tables align with widened chart right edge: chart_right={chart_right:?}, tables_right={tables_right:?}"
        );
    })
    .unwrap();

    // Rule 3 on Balance Sheet: User narrows window to 560px (`available = 320px`).
    // Chart card and table scroll container shrink with the window, hierarchy
    // columns DO NOT shrink below their natural widths, Liabilities and Equity
    // keep the same width, and both chart-level and table-level horizontal
    // scrolling work independently!
    cx.update_window(handle, |_, window, cx| {
        window.resize(size(px(560.), px(760.)));
        window.bounds_changed(cx);
        window.render_frame(cx);

        let available = px(560.) - report_horizontal_overhead(window);
        let chart_card = window
            .find((ElementId::from("chart-card"), "balance-sheet"))
            .bounds();
        assert!(
            (chart_card.size.width - available).abs() <= px(2.),
            "Chart card shrinks with narrow window: card={:?}, available={available:?}",
            chart_card.size.width
        );
        let assets_narrow = window
            .find((ElementId::from("trie-table"), "Assets"))
            .bounds();
        let liab_narrow = window
            .find((ElementId::from("trie-table"), "Liabilities"))
            .bounds();
        let equity_narrow = window
            .find((ElementId::from("trie-table"), "Equity"))
            .bounds();
        assert_eq!(
            liab_narrow.size.width, equity_narrow.size.width,
            "Liabilities and Equity match when narrowed"
        );
        assert_eq!(
            assets_narrow.size.width, assets_nat_w,
            "Assets column card does NOT shrink below natural width when window narrows"
        );
        assert_eq!(
            liab_narrow.size.width, liab_nat_w,
            "Liabilities column card does NOT shrink below natural width when window narrows"
        );

        let plot_before_x = window.find("balance-sheet-plot-area").bounds().origin.x;
        let liab_before_x = liab_narrow.origin.x;

        // Horizontal scroll at the chart level moves the plot left without moving the tables.
        window.scroll(
            (ElementId::from("chart-scroll"), "balance-sheet"),
            ScrollDelta::Pixels(point(px(-90.), px(0.))),
            cx,
        );
        let plot_after_x = window.find("balance-sheet-plot-area").bounds().origin.x;
        assert!(
            plot_after_x < plot_before_x - px(40.),
            "Chart scrolls horizontally when narrowed: before={plot_before_x:?}, after={plot_after_x:?}"
        );
        assert_eq!(
            window
                .find((ElementId::from("trie-table"), "Liabilities"))
                .bounds()
                .origin
                .x,
            liab_before_x,
            "Scrolling the chart horizontally does not move the table below"
        );

        // Horizontal scroll at the table level moves the tables left without moving the chart.
        window.scroll(
            (ElementId::from("trie-tables-scroll"), "balance-sheet"),
            ScrollDelta::Pixels(point(px(-120.), px(0.))),
            cx,
        );
        let liab_after_x = window
            .find((ElementId::from("trie-table"), "Liabilities"))
            .bounds()
            .origin
            .x;
        assert!(
            liab_after_x < liab_before_x - px(60.),
            "Tables scroll horizontally when narrowed: before={liab_before_x:?}, after={liab_after_x:?}"
        );
        assert_eq!(
            window.find("balance-sheet-plot-area").bounds().origin.x,
            plot_after_x,
            "Scrolling the table horizontally does not move the chart above"
        );
    })
    .unwrap();

    // --- Rule 3 on `Account` view (`DataTable` + Account Balance chart) ---
    cx.update_window(handle, |_, window, cx| {
        window.scroll(
            (ElementId::from("trie-tables-scroll"), "balance-sheet"),
            ScrollDelta::Pixels(point(px(120.), px(0.))),
            cx,
        );
        window.click((ElementId::from("account"), "Assets:Bank:Checking"), cx);
    })
    .unwrap();
    cx.run_until_parked();
    let acct_naturals = cx
        .update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let journal = app.read(cx).journal().expect("account view open");
            journal.read(cx).natural_column_widths(cx)
        })
        .unwrap();

    cx.update_window(handle, |_, window, cx| {
        window.resize(size(px(560.), px(760.)));
        window.bounds_changed(cx);
        window.render_frame(cx);

        let journal = app.read(cx).journal().expect("account view open");
        assert_eq!(
            journal.read(cx).column_widths(cx),
            acct_naturals,
            "Account DataTable columns never shrink below natural content widths when window narrows"
        );
        let plot_before_x = window.find("account-balance-plot-area").bounds().origin.x;
        window.scroll(
            (ElementId::from("chart-scroll"), "Assets:Bank:Checking"),
            ScrollDelta::Pixels(point(px(-80.), px(0.))),
            cx,
        );
        let plot_after_x = window.find("account-balance-plot-area").bounds().origin.x;
        assert!(
            plot_after_x < plot_before_x - px(40.),
            "Account chart scrolls horizontally when narrowed: before={plot_before_x:?}, after={plot_after_x:?}"
        );
    })
    .unwrap();
}
