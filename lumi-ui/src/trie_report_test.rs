use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::test::{TestAppContextExt as _, TestWindowExt as _};
use gpui_kit::{
    AnyWindowHandle, AppContext as _, Bounds, ElementId, Entity, Pixels, ScrollDelta, Size,
    TestAppContext, WindowBounds, WindowOptions, point, px, size,
};
use lumi::web::{TrieTable, TrieTableRow};

use crate::LumiApp;
use crate::navigation::Page;
use crate::session::LedgerSession;
use crate::source::LocalLedger;
use crate::trie_report::{TrieLoad, TriePresentation, TrieReport, roots};

/// The query fixture of the `lumi` crate.
const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../lumi/src/query_test.bean");

fn row(level: usize, name: &str) -> TrieTableRow<String> {
    TrieTableRow {
        level,
        name: name.to_string(),
        numbers: vec![String::new()],
    }
}

#[test]
fn test_presentation_resolves_account_names() {
    let table = TrieTable {
        rows: vec![
            row(0, "Assets"),
            row(1, "Bank"),
            row(2, "Checking"),
            row(2, "Savings"),
            row(1, "Cash"),
            row(2, "Wallet"),
            row(3, "Coins"),
            row(1, "Investments"),
        ],
        currencies: vec!["USD".to_string()],
    };
    let presentation = TriePresentation::new("Assets", table);
    let accounts: Vec<&str> = presentation
        .rows
        .iter()
        .map(|row| row.account.as_ref())
        .collect();
    assert_eq!(
        accounts,
        [
            "Assets",
            "Assets:Bank",
            "Assets:Bank:Checking",
            "Assets:Bank:Savings",
            "Assets:Cash",
            "Assets:Cash:Wallet",
            "Assets:Cash:Wallet:Coins",
            "Assets:Investments",
        ]
    );
    assert_eq!(presentation.rows[2].name.as_ref(), "Checking");
    assert_eq!(presentation.rows[2].level, 2);
    assert_eq!(presentation.currencies, ["USD"]);
}

/// A session over the fixture ledger with its first load completed, and a
/// balance sheet report on it.
fn open_fixture(cx: &mut TestAppContext) -> (Entity<LedgerSession>, Entity<TrieReport>) {
    let session = cx.new(|cx| {
        let mut session = LedgerSession::new(Arc::new(LocalLedger::new(FIXTURE)));
        session.reload(cx);
        session
    });
    let report = cx.new(|cx| TrieReport::new(Page::BalanceSheet, session.clone(), cx));
    cx.run_until_parked();
    (session, report)
}

#[gpui_kit::test]
fn test_report_loads_after_the_session(cx: &mut TestAppContext) {
    let (session, report) = open_fixture(cx);
    cx.read(|cx| {
        let session = session.read(cx);
        assert_eq!(session.revision(), 1);
        assert!(!session.is_reloading());
        assert_eq!(session.last_error(), None);

        let report = report.read(cx);
        // Assets, then Liabilities and Equity, each hierarchy depth first.
        assert_eq!(
            report.accounts(),
            [
                "Assets",
                "Assets:Bank",
                "Assets:Bank:Checking",
                "Assets:Cash",
                "Assets:Investments",
                "Assets:Investments:Brokerage",
                "Liabilities",
                "Liabilities:CreditCard",
                "Equity",
                "Equity:Opening-Balances",
            ]
        );
        assert!(matches!(report.tables[0], TrieLoad::Ready(_)));
    });
}

#[gpui_kit::test]
fn test_report_follows_show_closed_and_reloads(cx: &mut TestAppContext) {
    let (session, report) = open_fixture(cx);
    let generation = cx.read(|cx| report.read(cx).generation);

    // Showing closed accounts is a new request; the closed account appears.
    report.update(cx, |report, cx| report.set_show_closed(true, cx));
    cx.run_until_parked();
    cx.read(|cx| {
        let report = report.read(cx);
        assert!(report.is_showing_closed());
        assert_eq!(report.generation, generation + 1);
        assert!(report.accounts().iter().any(|a| a == "Assets:Old:Savings"));
    });

    // A session notification without a new revision is not a new request.
    session.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    cx.read(|cx| assert_eq!(report.read(cx).generation, generation + 1));

    // A reload is: the revision moves on and the report queries again.
    session.update(cx, |session, cx| session.reload(cx));
    cx.run_until_parked();
    cx.read(|cx| {
        assert_eq!(session.read(cx).revision(), 2);
        assert_eq!(report.read(cx).generation, generation + 2);
        assert_eq!(report.read(cx).accounts().len(), 12);
    });
}

#[gpui_kit::test]
fn test_missing_ledger_is_reported(cx: &mut TestAppContext) {
    let session = cx.new(|cx| {
        let mut session =
            LedgerSession::new(Arc::new(LocalLedger::new("/nonexistent/ledger.bean")));
        session.reload(cx);
        session
    });
    let report = cx.new(|cx| TrieReport::new(Page::BalanceSheet, session.clone(), cx));
    cx.run_until_parked();
    cx.read(|cx| {
        let session = session.read(cx);
        assert_eq!(session.revision(), 0);
        assert!(!session.is_loaded());
        assert!(
            session
                .last_error()
                .is_some_and(|error| error.contains("/nonexistent/ledger.bean")),
            "{:?}",
            session.last_error()
        );
        // Nothing was queried: there is no ledger to query.
        assert!(report.read(cx).tables.is_empty());
        assert_eq!(report.read(cx).generation, 0);
    });
}

/// The app in a window of `window_size`, opened on `page` of the fixture
/// ledger.
fn open_app(
    page: Page,
    window_size: Size<Pixels>,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<LumiApp>) {
    cx.update(crate::init);
    cx.update(|cx| {
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    Default::default(),
                    window_size,
                ))),
                ..Default::default()
            },
            cx,
            |window, cx| {
                cx.new(|cx| {
                    LumiApp::new(
                        Arc::new(LocalLedger::new(FIXTURE)),
                        "query_test.bean",
                        page,
                        window,
                        cx,
                    )
                })
            },
        )
        .expect("open test window")
    })
}

#[gpui_kit::test]
async fn test_app_navigates_and_refreshes_through_the_ui(cx: &mut TestAppContext) {
    let (handle, app) = open_app(Page::BalanceSheet, size(px(1200.), px(800.)), cx);
    cx.run_until_parked();
    cx.read(|cx| {
        assert_eq!(app.read(cx).page(), &Page::BalanceSheet);
        assert_eq!(app.read(cx).session().read(cx).revision(), 1);
    });

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        // The sidebar registers its items as group-menu-item; the second
        // destination is the income statement.
        window.within("lumi-sidebar").click("0-0-1", cx);
    })
    .unwrap();
    cx.read(|cx| assert_eq!(app.read(cx).page(), &Page::Income));

    cx.update_window(handle, |_, window, cx| {
        window.click("refresh", cx);
    })
    .unwrap();
    cx.wait_for(handle, Duration::from_secs(5), |_, cx| {
        app.read(cx).session().read(cx).revision() == 2
    })
    .await;
    cx.read(|cx| assert!(!app.read(cx).session().read(cx).is_reloading()));

    // Every sidebar destination opens, and an account name in the balance
    // sheet opens that account's journal.
    for (ix, page) in Page::sidebar().into_iter().enumerate() {
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.within("lumi-sidebar").click(format!("0-0-{ix}"), cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.read(|cx| assert_eq!(app.read(cx).page(), &page));
        // Render the page with its data loaded. (The test platform never
        // activates accessibility, so this does not build that tree.)
        cx.update_window(handle, |_, window, cx| window.render_frame(cx))
            .unwrap();
    }
    cx.update_window(handle, |_, window, cx| {
        window.within("lumi-sidebar").click("0-0-0", cx);
        window.render_frame(cx);
        window.click((ElementId::from("account"), "Assets:Cash"), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.read(|cx| assert_eq!(app.read(cx).page(), &Page::Account("Assets:Cash".into())));
}

/// GPUI derives accessibility node ids from element paths and rejects
/// duplicates, and `Table` has no id of its own, so sibling tables need
/// distinct scopes. The root row is the first row of every table, so the
/// root buttons' parents share a path unless the tables are scoped apart.
#[gpui_kit::test]
fn test_sibling_tables_have_distinct_paths(cx: &mut TestAppContext) {
    for page in [Page::BalanceSheet, Page::Income] {
        let (handle, _app) = open_app(page.clone(), size(px(1200.), px(800.)), cx);
        cx.run_until_parked();
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let parents: HashSet<Vec<ElementId>> = roots(&page)
                .map(|root| {
                    let button = window.find((ElementId::from("account"), root));
                    let path = button.path();
                    path[..path.len() - 1].to_vec()
                })
                .collect();
            assert_eq!(
                parents.len(),
                roots(&page).count(),
                "{page:?}: the root rows of sibling tables share a path: {parents:?}"
            );
        })
        .unwrap();
    }
}

/// The report scrolls both ways: the window is too short for the Assets
/// hierarchy and too narrow for the two columns of the balance sheet.
#[gpui_kit::test]
fn test_report_scrolls_both_ways(cx: &mut TestAppContext) {
    let (handle, _app) = open_app(Page::BalanceSheet, size(px(600.), px(240.)), cx);
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let last_asset = (ElementId::from("account"), "Assets:Investments:Brokerage");
        let equity = (ElementId::from("account"), "Equity");
        let over = (ElementId::from("account"), "Assets");
        let last_asset_y = window.find(last_asset.clone()).bounds().origin.y;
        let equity_x = window.find(equity.clone()).bounds().origin.x;

        window.scroll(over.clone(), ScrollDelta::Lines(point(0., -1.)), cx);
        assert!(
            window.find(last_asset.clone()).bounds().origin.y < last_asset_y,
            "scrolling down moves the rows up"
        );
        assert_eq!(window.find(equity.clone()).bounds().origin.x, equity_x);

        window.scroll(over, ScrollDelta::Lines(point(-1., 0.)), cx);
        assert!(
            window.find(equity).bounds().origin.x < equity_x,
            "scrolling right moves the second column left"
        );
    })
    .unwrap();
}
