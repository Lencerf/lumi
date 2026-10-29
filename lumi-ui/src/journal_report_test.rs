use std::sync::Arc;

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::table::TableDelegate as _;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{
    AnyWindowHandle, AppContext as _, Bounds, Entity, Styled as _, TestAppContext, WindowBounds,
    WindowOptions, point, px, size,
};
use lumi::TxnFlag;

use crate::LumiApp;
use crate::journal_report::{JournalReport, JournalRow};
use crate::navigation::{Page, ReportEvent};
use crate::session::LedgerSession;
use crate::source::LocalLedger;

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../lumi/src/query_test.bean");

/// A loaded session over the fixture and a journal report for `page`, with
/// its first page of entries fetched.
fn open_journal(
    page: Page,
    cx: &mut TestAppContext,
) -> (Entity<LedgerSession>, Entity<JournalReport>) {
    cx.update(crate::init);
    let session = cx.new(|cx| {
        let mut session = LedgerSession::new(Arc::new(LocalLedger::new(FIXTURE)));
        session.reload(cx);
        session
    });
    let (report, _) =
        cx.add_window_view(|window, cx| JournalReport::new(page, session.clone(), window, cx));
    cx.run_until_parked();
    (session, report)
}

#[gpui_kit::test]
fn test_journal_lists_newest_first(cx: &mut TestAppContext) {
    let (_session, report) = open_journal(Page::Journal, cx);
    cx.read(|cx| {
        let report = report.read(cx);
        assert_eq!(report.total(cx), 9);
        let dates: Vec<String> = report
            .entries(cx)
            .iter()
            .map(|entry| entry.txn.date().to_string())
            .collect();
        assert_eq!(dates[0], "2025-06-01");
        assert_eq!(dates[8], "2024-01-01");
        // Nothing is expanded, so there is one row per transaction, and a
        // journal of everything carries no running balance.
        let delegate = report.table.read(cx).delegate();
        assert_eq!(delegate.rows.len(), 9);
        assert!(
            report
                .entries(cx)
                .iter()
                .all(|entry| entry.balance.is_empty())
        );
    });
}

#[gpui_kit::test]
fn test_postings_expand_per_transaction_and_all(cx: &mut TestAppContext) {
    let (_session, report) = open_journal(Page::Journal, cx);

    // Expanding the first transaction ("Pay card") inserts its two postings.
    report.update(cx, |report, cx| report.toggle_postings(0, cx));
    cx.read(|cx| {
        let delegate = report.read(cx).table.read(cx).delegate();
        assert_eq!(delegate.rows.len(), 11);
        assert_eq!(delegate.rows[0], JournalRow::Transaction(0));
        assert_eq!(delegate.rows[1], JournalRow::Posting(0, 0));
        assert_eq!(delegate.rows[2], JournalRow::Posting(0, 1));
        assert_eq!(delegate.rows[3], JournalRow::Transaction(1));
    });
    report.update(cx, |report, cx| report.toggle_postings(0, cx));
    cx.read(|cx| assert_eq!(report.read(cx).table.read(cx).delegate().rows.len(), 9));

    // Showing all postings: every transaction except the balance assertion.
    report.update(cx, |report, cx| report.set_show_postings(true, cx));
    cx.read(|cx| {
        let report = report.read(cx);
        assert!(report.is_showing_postings(cx));
        let entries = report.entries(cx);
        let postings: usize = entries
            .iter()
            .filter(|entry| entry.txn.flag() != TxnFlag::Balance)
            .map(|entry| entry.txn.postings().len())
            .sum();
        assert_eq!(report.table.read(cx).delegate().rows.len(), 9 + postings);
    });
}

#[gpui_kit::test]
fn test_activating_rows(cx: &mut TestAppContext) {
    let (_session, report) = open_journal(Page::Journal, cx);
    let opened = Arc::new(std::sync::Mutex::new(Vec::new()));
    let _subscription = cx.update(|cx| {
        let opened = opened.clone();
        cx.subscribe(&report, move |_, event: &ReportEvent, _| {
            let ReportEvent::OpenAccount(account) = event;
            opened.lock().unwrap().push(account.to_string());
        })
    });

    // A transaction row toggles its postings; a posting row opens its account.
    report.update(cx, |report, cx| report.activate_row(0, cx));
    cx.read(|cx| assert_eq!(report.read(cx).table.read(cx).delegate().rows.len(), 11));
    let first_posting_account = cx.read(|cx| {
        report.read(cx).entries(cx)[0].txn.postings()[0]
            .account
            .to_string()
    });
    report.update(cx, |report, cx| report.activate_row(1, cx));
    cx.run_until_parked();
    assert_eq!(opened.lock().unwrap().as_slice(), [first_posting_account]);

    // The balance assertion (2024-03-15, the 6th newest) has nothing to expand.
    let balance_row = cx.read(|cx| {
        let delegate = report.read(cx).table.read(cx).delegate();
        delegate
            .rows
            .iter()
            .position(|row| {
                matches!(row, JournalRow::Transaction(ix)
                    if delegate.entries[*ix].txn.flag() == TxnFlag::Balance)
            })
            .unwrap()
    });
    let rows_before = cx.read(|cx| report.read(cx).table.read(cx).delegate().rows.len());
    report.update(cx, |report, cx| report.activate_row(balance_row, cx));
    cx.read(|cx| {
        assert_eq!(
            report.read(cx).table.read(cx).delegate().rows.len(),
            rows_before
        )
    });
}

#[gpui_kit::test]
fn test_journal_pages(cx: &mut TestAppContext) {
    let (session, report) = open_journal(Page::Journal, cx);
    let dates = |report: &JournalReport, cx: &gpui_kit::App| -> Vec<String> {
        report
            .entries(cx)
            .iter()
            .map(|entry| entry.txn.date().to_string())
            .collect()
    };
    cx.read(|cx| {
        let report = report.read(cx);
        assert_eq!(report.page_number(cx), 1);
        assert_eq!(report.page_size(cx), lumi::web::DEFAULT_ENTRIES_PER_PAGE);
        assert_eq!(report.entries(cx).len(), 9);
    });

    // Four per page: 9 transactions make three pages, newest first.
    report.update(cx, |report, cx| report.set_page_size(4, cx));
    cx.run_until_parked();
    cx.read(|cx| {
        let report = report.read(cx);
        assert_eq!(report.page_number(cx), 1);
        assert_eq!(
            dates(report, cx),
            ["2025-06-01", "2025-05-20", "2024-04-01", "2024-03-15"]
        );
    });
    report.update(cx, |report, cx| report.open_page_number(3, cx));
    cx.run_until_parked();
    cx.read(|cx| {
        let report = report.read(cx);
        assert_eq!(report.page_number(cx), 3);
        assert_eq!(dates(report, cx), ["2024-01-01"]);
        assert_eq!(report.total(cx), 9);
    });

    // Growing the page keeps the first shown transaction on the page.
    report.update(cx, |report, cx| report.set_page_size(20, cx));
    cx.run_until_parked();
    cx.read(|cx| {
        let report = report.read(cx);
        assert_eq!(report.page_number(cx), 1);
        assert_eq!(report.entries(cx).len(), 9);
    });

    // A page past the end falls back to the last page.
    report.update(cx, |report, cx| report.open_page_number(7, cx));
    cx.run_until_parked();
    cx.read(|cx| assert_eq!(report.read(cx).page_number(cx), 1));

    // A reload reads the current page again rather than starting over.
    report.update(cx, |report, cx| report.set_page_size(4, cx));
    report.update(cx, |report, cx| report.open_page_number(2, cx));
    cx.run_until_parked();
    session.update(cx, |session, cx| session.reload(cx));
    cx.run_until_parked();
    cx.read(|cx| {
        let report = report.read(cx);
        assert_eq!(report.page_number(cx), 2);
        assert_eq!(
            dates(report, cx),
            ["2024-03-01", "2024-02-10", "2024-02-03", "2024-01-15"]
        );
    });
}

#[gpui_kit::test]
fn test_account_journal_carries_running_balance(cx: &mut TestAppContext) {
    let (session, report) = open_journal(Page::Account("Assets:Cash".into()), cx);
    cx.read(|cx| {
        let report = report.read(cx);
        assert_eq!(report.total(cx), 3);
        let entries = report.entries(cx);
        // Newest first: the balance assertion, dinner, then opening balances.
        assert_eq!(entries[1].txn.narration(), "Dinner");
        assert_eq!(entries[1].balance["USD"].to_string(), "54.50");
        assert_eq!(entries[1].changes["USD"].to_string(), "-45.50");
        assert_eq!(report.table.read(cx).delegate().columns.len(), 6);
    });

    // A reload starts the table over on the new revision.
    session.update(cx, |session, cx| session.reload(cx));
    cx.run_until_parked();
    cx.read(|cx| {
        assert_eq!(session.read(cx).revision(), 2);
        assert_eq!(report.read(cx).total(cx), 3);
        assert_eq!(report.read(cx).entries(cx).len(), 3);
    });
}

#[gpui_kit::test]
fn test_clicking_account_buttons_in_journal_opens_their_accounts(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let (handle, app): (AnyWindowHandle, Entity<LumiApp>) = cx.update(|cx| {
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
                        Arc::new(LocalLedger::new(FIXTURE)),
                        "query_test.bean",
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

    // Expand the first transaction and click its first posting's account button.
    let first_account: gpui_kit::SharedString = cx.read(|cx| {
        let report = app.read(cx).journal().expect("the journal is shown");
        report.read(cx).entries(cx)[0].txn.postings()[0]
            .account
            .to_string()
            .into()
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(("postings", 0usize), cx);
    })
    .unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(("posting-account", 1usize), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.read(|cx| {
        assert_eq!(app.read(cx).page(), &Page::Account(first_account));
    });

    // Return to Journal and click the balance assertion's account button.
    cx.update_window(handle, |_, window, cx| {
        window.within("lumi-sidebar").click("0-0-2", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let report = app.read(cx).journal().expect("the journal is shown");
        let balance_ix = report
            .read(cx)
            .entries(cx)
            .iter()
            .position(|entry| entry.txn.flag() == TxnFlag::Balance)
            .unwrap();
        window.click(("balance-account", balance_ix), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.read(|cx| assert_eq!(app.read(cx).page(), &Page::Account("Assets:Cash".into())));
}

#[gpui_kit::test]
fn test_description_cell_toggles_postings_and_sub_rows_match_parent_background(
    cx: &mut TestAppContext,
) {
    cx.update(crate::init);
    let (handle, app): (AnyWindowHandle, Entity<LumiApp>) = cx.update(|cx| {
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
                        Arc::new(LocalLedger::new(FIXTURE)),
                        "query_test.bean",
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

    // The clickable description target spans the entire Description cell, so a
    // click near its right edge (past the narration text) expands and collapses
    // the transaction's sub-rows.
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let bounds = window.find(("postings", 0usize)).bounds();
        assert_eq!(bounds.size.width, px(380.));
        window.click_at(("postings", 0usize), point(px(340.), px(10.)), cx);
        window.click_at(("postings", 1usize), point(px(340.), px(10.)), cx);
    })
    .unwrap();

    cx.update_window(handle, |_, window, cx| {
        let report = app.read(cx).journal().expect("the journal is shown");
        let table = report.read(cx).table.clone();
        let even_bg = Some(cx.theme().table.into());
        let odd_bg = Some(cx.theme().table_even.into());
        {
            let delegate = table.read(cx).delegate();
            // Rows 0..=2 belong to transaction 0; rows 3..=5 belong to transaction 1.
            assert_eq!(delegate.rows[0], JournalRow::Transaction(0));
            assert_eq!(delegate.rows[1], JournalRow::Posting(0, 0));
            assert_eq!(delegate.rows[2], JournalRow::Posting(0, 1));
            assert_eq!(delegate.rows[3], JournalRow::Transaction(1));
            assert_eq!(delegate.rows[4], JournalRow::Posting(1, 0));
            assert_eq!(delegate.rows[5], JournalRow::Posting(1, 1));
        }
        for row_ix in 0..=2 {
            let bg = table.update(cx, |state, cx| {
                state
                    .delegate_mut()
                    .render_tr(row_ix, window, cx)
                    .style()
                    .background
                    .clone()
            });
            assert_eq!(bg, even_bg);
        }
        for row_ix in 3..=5 {
            let bg = table.update(cx, |state, cx| {
                state
                    .delegate_mut()
                    .render_tr(row_ix, window, cx)
                    .style()
                    .background
                    .clone()
            });
            assert_eq!(bg, odd_bg);
        }

        // Clicking the description cell again collapses its sub-rows.
        window.render_frame(cx);
        window.click_at(("postings", 0usize), point(px(340.), px(10.)), cx);
    })
    .unwrap();
    cx.read(|cx| {
        let report = app.read(cx).journal().expect("the journal is shown");
        assert_eq!(
            report.read(cx).table.read(cx).delegate().rows[1],
            JournalRow::Transaction(1)
        );
    });
}

#[gpui_kit::test]
fn test_search_shortcut_focuses_search_box_and_filters_journal_and_account_views(
    cx: &mut TestAppContext,
) {
    cx.update(crate::init);
    let (handle, app): (AnyWindowHandle, Entity<LumiApp>) = cx.update(|cx| {
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
                        Arc::new(LocalLedger::new(FIXTURE)),
                        "query_test.bean",
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
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
    })
    .unwrap();

    #[cfg(target_os = "macos")]
    let find_shortcut = "cmd-f";
    #[cfg(not(target_os = "macos"))]
    let find_shortcut = "ctrl-f";

    // Pressing cmd+f / ctrl+f focuses the journal search input so typing filters
    // immediately by currency ("VTI").
    cx.simulate_keystrokes(handle, find_shortcut);
    cx.update_window(handle, |_, window, cx| {
        let report = app.read(cx).journal().expect("journal is shown");
        assert!(report.read(cx).is_search_focused(window, cx));
    })
    .unwrap();

    cx.simulate_keystrokes(handle, "v t i");
    cx.run_until_parked();
    cx.read(|cx| {
        let report = app.read(cx).journal().expect("journal is shown");
        assert_eq!(report.read(cx).search_query(cx), "vti");
        assert_eq!(report.read(cx).total(cx), 1);
        assert_eq!(report.read(cx).entries(cx)[0].txn.payee(), "Broker");
    });

    // Pressing Escape clears the search box and restores all entries.
    cx.simulate_keystrokes(handle, "escape");
    cx.run_until_parked();
    cx.read(|cx| {
        let report = app.read(cx).journal().expect("journal is shown");
        assert_eq!(report.read(cx).search_query(cx), "");
        assert_eq!(report.read(cx).total(cx), 9);
    });

    // Opening an account view also supports cmd+f / ctrl+f search over that account.
    cx.update_window(handle, |_, window, cx| {
        window.dispatch_action(
            Box::new(crate::journal_report::OpenAccount("Assets:Cash".into())),
            cx,
        );
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
    })
    .unwrap();

    cx.simulate_keystrokes(handle, find_shortcut);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
    })
    .unwrap();
    cx.simulate_keystrokes(handle, "b i s t r o");
    cx.run_until_parked();
    cx.read(|cx| {
        assert_eq!(app.read(cx).page(), &Page::Account("Assets:Cash".into()));
        let report = app.read(cx).journal().expect("account journal is shown");
        assert_eq!(report.read(cx).total(cx), 1);
        assert_eq!(report.read(cx).entries(cx)[0].txn.narration(), "Dinner");
    });
}

#[gpui_kit::test]
fn test_record_type_selector_toggles_entry_flags_and_resets_all(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let (handle, app): (AnyWindowHandle, Entity<LumiApp>) = cx.update(|cx| {
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
                        Arc::new(LocalLedger::new(FIXTURE)),
                        "query_test.bean",
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

    // All 9 fixture entries (8 `*` transactions + 1 `balance` assertion) start visible.
    cx.read(|cx| {
        let report = app.read(cx).journal().expect("journal is shown");
        assert_eq!(report.read(cx).total(cx), 9);
        assert_eq!(report.read(cx).visible_flags(cx).len(), 4);
    });

    // Clicking `bal` toggles balance assertions off -> 8 cleared transactions remain.
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click((gpui_kit::ElementId::from("record-type"), "bal"), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.read(|cx| {
        let report = app.read(cx).journal().expect("journal is shown");
        assert_eq!(report.read(cx).total(cx), 8);
        assert!(
            !report
                .read(cx)
                .visible_flags(cx)
                .contains(&TxnFlag::Balance)
        );
    });

    // Clicking `*` off and `bal` back on leaves only the 1 balance assertion.
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click((gpui_kit::ElementId::from("record-type"), "*"), cx);
    })
    .unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click((gpui_kit::ElementId::from("record-type"), "bal"), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.read(|cx| {
        let report = app.read(cx).journal().expect("journal is shown");
        assert_eq!(report.read(cx).total(cx), 1);
        assert_eq!(report.read(cx).entries(cx)[0].txn.flag(), TxnFlag::Balance);
    });

    // Clicking "All" re-enables every record type.
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(
            (
                gpui_kit::ElementId::from("journal-record-types-all"),
                Page::Journal.id(),
            ),
            cx,
        );
    })
    .unwrap();
    cx.run_until_parked();
    cx.read(|cx| {
        let report = app.read(cx).journal().expect("journal is shown");
        assert_eq!(report.read(cx).visible_flags(cx).len(), 4);
        assert_eq!(report.read(cx).total(cx), 9);
    });
}
