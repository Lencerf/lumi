use std::sync::Arc;

use gpui_kit::{AppContext as _, Entity, TestAppContext};
use lumi::TxnFlag;

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
        assert_eq!(report.read(cx).table.read(cx).delegate().loaded_pages, 1);
    });
}
