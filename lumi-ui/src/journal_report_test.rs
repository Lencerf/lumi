use std::sync::Arc;

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::table::TableDelegate as _;
use gpui_kit::test::{TestEventExt as _, TestQueryExt as _, TestWindowExt as _};
use gpui_kit::{
    AnyWindowHandle, AppContext as _, Bounds, Entity, Modifiers, MouseButton, MouseDownEvent,
    PlatformInput, Styled as _, TestAppContext, WindowBounds, WindowOptions, point, px, size,
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
            if let ReportEvent::OpenAccount(account) = event {
                opened.lock().unwrap().push(account.to_string());
            }
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
        window.click_at(("postings", 2usize), point(px(340.), px(10.)), cx);
    })
    .unwrap();

    cx.update_window(handle, |_, window, cx| {
        let report = app.read(cx).journal().expect("the journal is shown");
        let table = report.read(cx).table.clone();
        let even_bg = Some(cx.theme().table.into());
        let odd_bg = Some(cx.theme().table_even.into());
        let focused_bg = Some(cx.theme().table_active.into());
        {
            let delegate = table.read(cx).delegate();
            // Rows 0..=2 belong to transaction 0 (unfocused even); rows 3..=5
            // belong to transaction 1 (unfocused odd); rows 6..=8 belong to
            // transaction 2 (last clicked -> focused).
            assert_eq!(delegate.rows[0], JournalRow::Transaction(0));
            assert_eq!(delegate.rows[1], JournalRow::Posting(0, 0));
            assert_eq!(delegate.rows[2], JournalRow::Posting(0, 1));
            assert_eq!(delegate.rows[3], JournalRow::Transaction(1));
            assert_eq!(delegate.rows[4], JournalRow::Posting(1, 0));
            assert_eq!(delegate.rows[5], JournalRow::Posting(1, 1));
            assert_eq!(delegate.rows[6], JournalRow::Transaction(2));
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
        for row_ix in 6..=8 {
            let bg = table.update(cx, |state, cx| {
                state
                    .delegate_mut()
                    .render_tr(row_ix, window, cx)
                    .style()
                    .background
                    .clone()
            });
            assert_eq!(bg, focused_bg);
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

    // All 9 fixture entries (8 `*` transactions + 1 `balance` assertion) start
    // visible, and "None" is shown alongside the full selector.
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window
                .try_find((
                    gpui_kit::ElementId::from("journal-record-types-none"),
                    Page::Journal.id(),
                ))
                .is_some()
        );
        assert!(
            window
                .try_find((
                    gpui_kit::ElementId::from("journal-record-types-all"),
                    Page::Journal.id(),
                ))
                .is_none()
        );
    })
    .unwrap();
    cx.read(|cx| {
        let report = app.read(cx).journal().expect("journal is shown");
        assert_eq!(report.read(cx).total(cx), 9);
        assert_eq!(report.read(cx).visible_flags(cx).len(), 4);
    });

    // Clicking "None" clears every flag at once and swaps "None" for "All",
    // letting the user then pick a single flag (`bal`).
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(
            (
                gpui_kit::ElementId::from("journal-record-types-none"),
                Page::Journal.id(),
            ),
            cx,
        );
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window
                .try_find((
                    gpui_kit::ElementId::from("journal-record-types-all"),
                    Page::Journal.id(),
                ))
                .is_some()
        );
        assert!(
            window
                .try_find((
                    gpui_kit::ElementId::from("journal-record-types-none"),
                    Page::Journal.id(),
                ))
                .is_none()
        );
    })
    .unwrap();
    cx.read(|cx| {
        let report = app.read(cx).journal().expect("journal is shown");
        assert!(report.read(cx).visible_flags(cx).is_empty());
        assert_eq!(report.read(cx).total(cx), 0);
        assert!(report.read(cx).entries(cx).is_empty());
    });

    // Clicking "All" restores all 4 flags and swaps back to "None".
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

#[gpui_kit::test]
fn test_show_in_context_clears_filters_and_locates_entry_page(cx: &mut TestAppContext) {
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

    // Two rows per page splits the 9-entry fixture into 5 pages (newest first:
    // page 1 has 2025-06-01 & 2025-05-20; page 2 has 2024-04-01 & 2024-03-15;
    // page 5 has 2024-01-01 "Opening balances").
    let report = cx.read(|cx| app.read(cx).journal().expect("journal is shown"));
    report.update(cx, |report, cx| report.set_page_size(2, cx));
    cx.run_until_parked();

    // Filter out balance assertions AND search for "Opening balances" so only
    // 1 entry is shown on page 1.
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click((gpui_kit::ElementId::from("record-type"), "bal"), cx);
        report.update(cx, |report, cx| {
            report.set_search_query("opening balances", window, cx)
        });
    })
    .unwrap();
    cx.run_until_parked();
    cx.read(|cx| {
        let report = report.read(cx);
        assert_eq!(report.total(cx), 1);
        assert_eq!(report.page_number(cx), 1);
        assert_eq!(report.entries(cx)[0].txn.narration(), "Opening balances");
    });

    // Right-click the description of the record and choose "Show in context".
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.right_click(("postings", 0usize), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let mut menu = window.within("popup-menu");
        assert!(menu.try_find_by_label("Edit entry").is_some());
        assert!(menu.try_find_by_label("Show postings").is_some());
        let item_id = menu
            .find_by_label("Show in context")
            .path()
            .last()
            .unwrap()
            .clone();
        menu.click(item_id, cx);
    })
    .unwrap();
    cx.run_until_parked();

    // Search and record-type filters are cleared, all 9 entries are back, and
    // the table jumps to page 5 with "Opening balances" selected.
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find_by_label("Search entries").value(), Some(""));
    })
    .unwrap();
    cx.read(|cx| {
        let report = report.read(cx);
        assert_eq!(report.search_query(cx), "");
        assert_eq!(report.visible_flags(cx).len(), 4);
        assert_eq!(report.total(cx), 9);
        assert_eq!(report.page_number(cx), 5);
        assert_eq!(report.entries(cx)[0].txn.narration(), "Opening balances");
        assert_eq!(report.table.read(cx).selected_row(), Some(0));
    });

    // Also verify on a balance assertion: isolate `bal`, right-click its
    // description, and "Show in context" lands on page 2 with row 1 selected.
    report.update(cx, |report, cx| {
        report.set_visible_flags([TxnFlag::Balance], cx);
    });
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.right_click(("balance-account", 0usize), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let mut menu = window.within("popup-menu");
        assert!(menu.try_find_by_label("Edit entry").is_some());
        let item_id = menu
            .find_by_label("Show in context")
            .path()
            .last()
            .unwrap()
            .clone();
        menu.click(item_id, cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.read(|cx| {
        let report = report.read(cx);
        assert_eq!(report.visible_flags(cx).len(), 4);
        assert_eq!(report.total(cx), 9);
        assert_eq!(report.page_number(cx), 2);
        assert_eq!(report.entries(cx)[1].txn.flag(), TxnFlag::Balance);
        assert_eq!(report.table.read(cx).selected_row(), Some(1));
    });
}

#[gpui_kit::test]
fn test_focused_record_highlighted_and_kept_in_view_across_filters(cx: &mut TestAppContext) {
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

    // Two rows per page over 9 entries -> 5 pages (newest first:
    // page 1: 2025-06-01, 2025-05-20;
    // page 2: 2024-04-01, 2024-03-15 (bal);
    // page 3: 2024-03-01, 2024-02-10;
    // page 4: 2024-02-03, 2024-01-15 ("Salary");
    // page 5: 2024-01-01 ("Opening balances")).
    let report = cx.read(|cx| app.read(cx).journal().expect("journal is shown"));
    report.update(cx, |report, cx| report.set_page_size(2, cx));
    report.update(cx, |report, cx| report.open_page_number(4, cx));
    cx.run_until_parked();

    // Right-click row 1 ("Salary", 2024-01-15) on page 4 -> it becomes focused
    // and receives the distinct focused background color.
    let salary_src = cx.read(|cx| report.read(cx).entries(cx)[1].txn.src().clone());
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.right_click(("postings", 1usize), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        let focused_bg = Some(cx.theme().table_active.into());
        let unfocused_bg = Some(cx.theme().table.into());
        let table = report.read(cx).table.clone();
        let row0_bg = table.update(cx, |state, cx| {
            state
                .delegate_mut()
                .render_tr(0, window, cx)
                .style()
                .background
                .clone()
        });
        let row1_bg = table.update(cx, |state, cx| {
            state
                .delegate_mut()
                .render_tr(1, window, cx)
                .style()
                .background
                .clone()
        });
        assert_eq!(row0_bg, unfocused_bg);
        assert_eq!(row1_bg, focused_bg);
        assert_eq!(report.read(cx).focused_entry(cx), Some(salary_src.clone()));
    })
    .unwrap();

    // Navigate away to page 1 manually; then toggle `bal` off (leaving 8 `*`
    // transactions across 4 pages, with "Salary" now at index 6 -> page 4).
    // Because "Salary" is focused and still matches, the table jumps to page 4!
    report.update(cx, |report, cx| report.open_page_number(1, cx));
    cx.run_until_parked();
    cx.read(|cx| assert_eq!(report.read(cx).page_number(cx), 1));

    report.update(cx, |report, cx| {
        report.set_visible_flags([TxnFlag::Posted], cx);
    });
    cx.run_until_parked();
    cx.read(|cx| {
        let report = report.read(cx);
        assert_eq!(report.total(cx), 8);
        assert_eq!(report.page_number(cx), 4);
        assert_eq!(report.entries(cx)[0].txn.narration(), "Salary");
        assert_eq!(report.table.read(cx).selected_row(), Some(0));
    });

    // Narrow search to "2024": 6 matching `*` entries (pages 1..=3), with
    // "Salary" at index 4 -> page 3!
    cx.update_window(handle, |_, window, cx| {
        report.update(cx, |report, cx| report.set_search_query("2024", window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    cx.read(|cx| {
        let report = report.read(cx);
        assert_eq!(report.total(cx), 6);
        assert_eq!(report.page_number(cx), 3);
        assert_eq!(report.entries(cx)[0].txn.narration(), "Salary");
    });

    // Filter out "Salary" by searching for "Broker": falls back to page 1,
    // while still remembering "Salary" so clearing search returns to page 4!
    cx.update_window(handle, |_, window, cx| {
        report.update(cx, |report, cx| {
            report.set_search_query("Broker", window, cx)
        });
    })
    .unwrap();
    cx.run_until_parked();
    cx.read(|cx| {
        let report = report.read(cx);
        assert_eq!(report.total(cx), 1);
        assert_eq!(report.page_number(cx), 1);
        assert_eq!(report.focused_entry(cx), Some(salary_src.clone()));
    });

    cx.update_window(handle, |_, window, cx| {
        report.update(cx, |report, cx| report.set_search_query("", window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    cx.read(|cx| {
        let report = report.read(cx);
        assert_eq!(report.page_number(cx), 4);
        assert_eq!(report.entries(cx)[0].txn.narration(), "Salary");
    });

    // Verify the same remembered-focus behavior in an Account view
    // (`Assets:Bank:Checking` has 6 entries -> 3 pages at 2 rows/page, with
    // "Opening balances" at row 1 of page 3).
    let (_acct_session, acct_report) =
        open_journal(Page::Account("Assets:Bank:Checking".into()), cx);
    acct_report.update(cx, |report, cx| report.set_page_size(2, cx));
    acct_report.update(cx, |report, cx| report.open_page_number(3, cx));
    cx.run_until_parked();

    // Activate/click row 1 ("Opening balances") on page 3, move to page 1, and
    // search for "USD" (5 USD entries -> "Opening balances" is on page 3, row 0).
    acct_report.update(cx, |report, cx| report.activate_row(1, cx));
    acct_report.update(cx, |report, cx| report.open_page_number(1, cx));
    cx.run_until_parked();
    cx.read(|cx| assert_eq!(acct_report.read(cx).page_number(cx), 1));

    acct_report.update(cx, |report, cx| report.apply_search_query("USD".into(), cx));
    cx.run_until_parked();
    cx.read(|cx| {
        let report = acct_report.read(cx);
        assert_eq!(report.total(cx), 5);
        assert_eq!(report.page_number(cx), 3);
        assert_eq!(report.entries(cx)[0].txn.narration(), "Opening balances");
    });
}

#[gpui_kit::test]
fn test_journal_cmd_f_typing_sequential_chars_does_not_select_or_overwrite_first_char(
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

    let find_shortcut = if cfg!(target_os = "macos") {
        "cmd-f"
    } else {
        "ctrl-f"
    };

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let report = app.read(cx).journal().expect("journal is shown");
        let search_input = report.read(cx).search_input_state().clone();

        // Simulate an un-ended macOS trackpad click inside the search input before Cmd+F.
        let origin = search_input.read(cx).text_bounds().expect("search bounds");
        let click_pos = point(origin.origin.x + px(4.), origin.origin.y + px(8.));
        window.dispatch_event(
            PlatformInput::MouseDown(MouseDownEvent {
                button: MouseButton::Left,
                position: click_pos,
                modifiers: Modifiers::default(),
                click_count: 1,
                first_mouse: false,
            }),
            cx,
        );

        window.press(find_shortcut, cx);
        window.render_frame(cx);
        assert!(report.read(cx).is_search_focused(window, cx));
        assert_eq!(search_input.read(cx).selected_range(), 0..0);

        window.input("b", cx);
        window.render_frame(cx);
        window.pointer_move(click_pos, Some(MouseButton::Left), cx);
        assert_eq!(search_input.read(cx).value(), "b");
        assert_eq!(search_input.read(cx).selected_range(), 1..1);

        window.input("r", cx);
        window.render_frame(cx);
        window.pointer_up(click_pos, MouseButton::Left, cx);
        assert_eq!(search_input.read(cx).value(), "br");
        assert_eq!(search_input.read(cx).selected_range(), 2..2);
    })
    .unwrap();
    cx.run_until_parked();

    cx.read(|cx| {
        let report = app.read(cx).journal().expect("journal is shown");
        assert_eq!(report.read(cx).search_query(cx), "br");
        assert_eq!(report.read(cx).total(cx), 1);
        assert_eq!(report.read(cx).entries(cx)[0].txn.payee(), "Broker");
    });
}
