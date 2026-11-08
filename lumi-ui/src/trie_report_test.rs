use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::component::calendar::Date;
use gpui_kit::component::date_picker::DatePickerEvent;
use gpui_kit::test::{TestAppContextExt as _, TestEventExt as _, TestWindowExt as _};
use gpui_kit::{
    AnyWindowHandle, AppContext as _, Bounds, ElementId, Entity, MouseButton, Pixels, ScrollDelta,
    Size, TestAppContext, WindowBounds, WindowOptions, point, px, size,
};
use lumi::web::{
    ChartDatePreset, ChartDateRange, NaiveDate, TimeInterval, TrieTable, TrieTableRow,
};

use crate::LumiApp;
use crate::chart::{
    BalanceSheetMode, DEFAULT_BALANCE_SHEET_PLOT_HEIGHT, DEFAULT_INCOME_PLOT_HEIGHT,
    IncomeChartMode, MAX_CHART_PLOT_HEIGHT, MIN_CHART_PLOT_HEIGHT,
};
use crate::navigation::Page;
use crate::session::LedgerSession;
use crate::source::LocalLedger;
use crate::trie_report::{TrieLoad, TriePresentation, TrieReport, TrieRow, roots};

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

    let tree_lines: Vec<String> = presentation.rows.iter().map(TrieRow::tree_line).collect();
    assert_eq!(
        tree_lines,
        [
            "Assets",
            "├── Bank",
            "│   ├── Checking",
            "│   └── Savings",
            "├── Cash",
            "│   └── Wallet",
            "│       └── Coins",
            "└── Investments",
        ]
    );
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

/// The report scrolls both ways when the user narrows the window: vertical
/// scrolling moves the hierarchy while horizontal scrolling moves the table
/// columns independently.
#[gpui_kit::test]
fn test_report_scrolls_both_ways(cx: &mut TestAppContext) {
    let (handle, _app) = open_app(Page::BalanceSheet, size(px(600.), px(500.)), cx);
    cx.run_until_parked();
    cx.executor()
        .advance_clock(LumiApp::AUTO_WIDTH_ANIMATION_DURATION);
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.viewport_size().width > px(600.),
            "opening Balance Sheet auto-widens a too-narrow initial window"
        );
        // Simulate the user manually narrowing the window to 600px.
        window.resize(size(px(600.), px(500.)));
        window.bounds_changed(cx);
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

#[gpui_kit::test]
fn test_balance_and_income_views_render_full_height_tree_guides(cx: &mut TestAppContext) {
    // Balance sheet: verify root has no guides, child accounts have `level`
    // full-height guide columns immediately to the left of their button.
    let (bs_handle, _bs_app) = open_app(Page::BalanceSheet, size(px(1200.), px(800.)), cx);
    cx.run_until_parked();
    cx.update_window(bs_handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window
                .try_find((ElementId::from("tree-guide"), "Assets:0"))
                .is_none()
        );

        let assets_x = window
            .find((ElementId::from("account"), "Assets"))
            .bounds()
            .origin
            .x;
        let checking_x = window
            .find((ElementId::from("account"), "Assets:Bank:Checking"))
            .bounds()
            .origin
            .x;
        let g0 = window
            .find((ElementId::from("tree-guide"), "Assets:Bank:Checking:0"))
            .bounds();
        let g1 = window
            .find((ElementId::from("tree-guide"), "Assets:Bank:Checking:1"))
            .bounds();
        assert!(
            g0.size.height >= px(28.),
            "guide stretches across row height: {g0:?}"
        );
        assert_eq!(g0.size.height, g1.size.height);
        assert_eq!(g1.origin.x, g0.origin.x + g0.size.width);
        assert_eq!(
            checking_x,
            assets_x + g0.size.width + g1.size.width,
            "two tree-guide columns precede a level-2 account component"
        );
    })
    .unwrap();

    // Income statement: verify both Income and Expenses hierarchies render guides.
    let (inc_handle, _inc_app) = open_app(Page::Income, size(px(1200.), px(800.)), cx);
    cx.run_until_parked();
    cx.update_window(inc_handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window
                .try_find((ElementId::from("tree-guide"), "Income:0"))
                .is_none()
        );
        let inc_guide = window
            .find((ElementId::from("tree-guide"), "Income:Salary:0"))
            .bounds();
        let exp_guide = window
            .find((ElementId::from("tree-guide"), "Expenses:Food:0"))
            .bounds();
        assert!(inc_guide.size.height >= px(28.));
        assert!(exp_guide.size.height >= px(28.));
    })
    .unwrap();
}

#[gpui_kit::test]
fn test_balance_and_income_tables_render_aligned_full_height_column_separators(
    cx: &mut TestAppContext,
) {
    let (bs_handle, _bs_app) = open_app(Page::BalanceSheet, size(px(1200.), px(800.)), cx);
    cx.run_until_parked();
    cx.update_window(bs_handle, |_, window, cx| {
        window.render_frame(cx);
        // Column 0 ("Account") has no leading separator; currency columns 1..=N do.
        assert!(
            window
                .try_find((ElementId::from("trie-col-sep-head"), "Assets:0"))
                .is_none()
        );
        for col in [1usize, 2usize] {
            let head = window
                .find((
                    ElementId::from("trie-col-sep-head"),
                    format!("Assets:{col}"),
                ))
                .bounds();
            let row0 = window
                .find((
                    ElementId::from("trie-col-sep-cell"),
                    format!("Assets:0:{col}"),
                ))
                .bounds();
            let row1 = window
                .find((
                    ElementId::from("trie-col-sep-cell"),
                    format!("Assets:1:{col}"),
                ))
                .bounds();
            assert_eq!(head.size.width, px(1.));
            assert!(
                head.size.height >= px(24.),
                "header sep full height: {head:?}"
            );
            assert_eq!(row0.size.width, px(1.));
            assert_eq!(row1.size.width, px(1.));
            assert_eq!(row0.origin.x, head.origin.x);
            assert_eq!(row1.origin.x, head.origin.x);
            assert!(
                row1.origin.y <= row0.origin.y + row0.size.height,
                "no vertical gap between row0 ({row0:?}) and row1 ({row1:?})"
            );
        }
        // Liabilities has zero non-zero balances in the fixture (1 column only),
        // while Equity has 1 currency column.
        assert!(
            window
                .try_find((ElementId::from("trie-col-sep-head"), "Liabilities:1"))
                .is_none()
        );
        assert!(
            window
                .try_find((ElementId::from("trie-col-sep-head"), "Equity:1"))
                .is_some()
        );
    })
    .unwrap();

    let (inc_handle, _inc_app) = open_app(Page::Income, size(px(1200.), px(800.)), cx);
    cx.run_until_parked();
    cx.update_window(inc_handle, |_, window, cx| {
        window.render_frame(cx);
        for root in ["Income", "Expenses"] {
            let head = window
                .find((ElementId::from("trie-col-sep-head"), format!("{root}:1")))
                .bounds();
            let row1 = window
                .find((ElementId::from("trie-col-sep-cell"), format!("{root}:1:1")))
                .bounds();
            assert_eq!(head.size.width, px(1.));
            assert_eq!(row1.size.width, px(1.));
            assert_eq!(row1.origin.x, head.origin.x);
        }
    })
    .unwrap();
}

#[gpui_kit::test]
fn test_trie_report_formats_and_renders_three_digit_groups_with_narrow_gap(
    cx: &mut TestAppContext,
) {
    let table = TrieTable {
        rows: vec![
            TrieTableRow {
                level: 0,
                name: "Assets".to_string(),
                numbers: vec!["1123456.7891".to_string()],
            },
            TrieTableRow {
                level: 1,
                name: "Debt".to_string(),
                numbers: vec!["-123.45".to_string()],
            },
        ],
        currencies: vec!["USD".to_string()],
    };
    let presentation = TriePresentation::new("Assets", table);
    assert_eq!(presentation.rows[0].numbers[0].as_str(), "1 123 456.7891");
    assert_eq!(presentation.rows[1].numbers[0].as_str(), "-123.45");

    let (inc_handle, _inc_app) = open_app(Page::Income, size(px(1200.), px(800.)), cx);
    cx.run_until_parked();
    cx.update_window(inc_handle, |_, window, cx| {
        window.render_frame(cx);
        // `Income` root row has `-3000.00` -> `-3` + DIGIT_GROUP_GAP + `000.00`.
        assert!(window.try_find("number-hover-menu").is_none());
        {
            let mut inc_table = window.within((ElementId::from("trie-table"), "Income"));
            let mut body = inc_table.within(("table-body", 1usize));
            let mut row0 = body.within(("table-row", 0usize));
            let g0 = row0.find(("digit-group", 0usize)).bounds();
            let g1 = row0.find(("digit-group", 1usize)).bounds();
            assert_eq!(
                g1.origin.x - (g0.origin.x + g0.size.width),
                crate::number::DIGIT_GROUP_GAP
            );

            // Hovering over the financial number (`-3 000.00`) shows the hover menu
            // with a Copy number button that copies the ungrouped value (`-3000.00`).
            row0.hover(("financial-number", 0usize), cx);
        }
        window.render_frame(cx);
        assert!(window.try_find("number-hover-menu").is_some());
        assert!(window.try_find("copy-number-button").is_some());
        window.click("copy-number-button", cx);
        window.render_frame(cx);
        assert_eq!(
            cx.read_from_clipboard().and_then(|item| item.text()),
            Some("-3000.00".to_string())
        );

        // Moving the cursor away from the number and hover menu dismisses it.
        window.pointer_move(point(px(2.), px(2.)), None, cx);
        window.render_frame(cx);
        assert!(window.try_find("number-hover-menu").is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn test_income_statement_wide_multi_currency_balances_never_shrink_or_overlap(
    cx: &mut TestAppContext,
) {
    let ledger_text = "\
2024-01-01 open Assets:Bank CNY,USD
2024-01-01 open Income:Bonuses:CashBack CNY,USD
2024-01-01 open Income:Bonuses:Coupon USD
2024-01-01 open Income:Bonuses:GC USD
2024-01-01 open Income:Bonuses:Other CNY,USD
2024-01-01 open Income:Salary CNY,USD

2024-06-01 * \"Income\"
  Income:Bonuses:CashBack       -2.00 CNY
  Income:Bonuses:CashBack    -5378.89 USD
  Income:Bonuses:Coupon      -1607.25 USD
  Income:Bonuses:GC           -273.40 USD
  Income:Bonuses:Other      -10097.35 CNY
  Income:Bonuses:Other       -1100.11 USD
  Income:Salary              -1897.61 CNY
  Income:Salary           -2313959.72 USD
  Assets:Bank                11996.96 CNY
  Assets:Bank              2322319.37 USD
";
    let temp_path =
        std::env::temp_dir().join(format!("lumi-income-col-width-{}.bean", std::process::id()));
    std::fs::write(&temp_path, ledger_text).unwrap();

    cx.update(crate::init);
    let path_str = temp_path.to_str().unwrap().to_string();
    let (handle, _app) = cx.update(|cx| {
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    Default::default(),
                    size(px(1200.), px(800.)),
                ))),
                ..Default::default()
            },
            cx,
            move |window, cx| {
                cx.new(|cx| {
                    LumiApp::new(
                        Arc::new(LocalLedger::new(path_str)),
                        "income.bean",
                        Page::Income,
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
        let sep_cny_head = window
            .find((ElementId::from("trie-col-sep-head"), "Income:1"))
            .bounds();
        let sep_usd_head = window
            .find((ElementId::from("trie-col-sep-head"), "Income:2"))
            .bounds();
        let sep_cny_row0 = window
            .find((ElementId::from("trie-col-sep-cell"), "Income:0:1"))
            .bounds();
        let sep_usd_row0 = window
            .find((ElementId::from("trie-col-sep-cell"), "Income:0:2"))
            .bounds();
        assert_eq!(sep_cny_head.origin.x, sep_cny_row0.origin.x);
        assert_eq!(sep_usd_head.origin.x, sep_usd_row0.origin.x);

        let mut inc_table = window.within((ElementId::from("trie-table"), "Income"));
        let mut body = inc_table.within(("table-body", 1usize));
        let mut row0 = body.within(("table-row", 0usize));

        // Column 1 (`CNY` = `-11 996.96`) stays strictly inside `[sep_cny, sep_usd)`.
        let cny_cell = row0.within(("table-cell", 1usize));
        let cny_g0 = cny_cell.find(("digit-group", 0usize)).bounds();
        let cny_g1 = cny_cell.find(("digit-group", 1usize)).bounds();
        assert!(
            cny_g0.origin.x >= sep_cny_row0.origin.x + px(12.),
            "CNY left group should not crowd left separator: cny_g0={cny_g0:?}, sep={sep_cny_row0:?}"
        );
        assert!(
            cny_g1.origin.x + cny_g1.size.width <= sep_usd_row0.origin.x - px(4.),
            "CNY right group must stay left of USD separator: cny_g1={cny_g1:?}, sep_usd={sep_usd_row0:?}"
        );

        // Column 2 (`USD` = `-2 322 319.37`) expands past `100px` rather than
        // shrinking or overflowing into `CNY`.
        let usd_cell_bounds = row0.find(("table-cell", 2usize)).bounds();
        assert!(
            usd_cell_bounds.size.width > px(110.),
            "USD cell must not be shrunk to 100px: {usd_cell_bounds:?}"
        );
        let usd_cell = row0.within(("table-cell", 2usize));
        let usd_g0 = usd_cell.find(("digit-group", 0usize)).bounds();
        let usd_g1 = usd_cell.find(("digit-group", 1usize)).bounds();
        let usd_g2 = usd_cell.find(("digit-group", 2usize)).bounds();
        assert!(
            usd_g0.origin.x >= sep_usd_row0.origin.x + px(12.),
            "USD left group (`-2`) must stay well right of USD separator: usd_g0={usd_g0:?}, sep_usd={sep_usd_row0:?}"
        );
        assert_eq!(
            usd_g1.origin.x - (usd_g0.origin.x + usd_g0.size.width),
            crate::number::DIGIT_GROUP_GAP
        );
        assert_eq!(
            usd_g2.origin.x - (usd_g1.origin.x + usd_g1.size.width),
            crate::number::DIGIT_GROUP_GAP
        );
        assert!(
            usd_g2.origin.x + usd_g2.size.width <= usd_cell_bounds.origin.x + usd_cell_bounds.size.width,
            "USD right group must fit inside USD cell: usd_g2={usd_g2:?}, cell={usd_cell_bounds:?}"
        );
    })
    .unwrap();

    let _ = std::fs::remove_file(&temp_path);
}

#[gpui_kit::test]
fn test_balance_sheet_chart_renders_and_filters_currencies(cx: &mut TestAppContext) {
    let (handle, app) = open_app(Page::BalanceSheet, size(px(1200.), px(800.)), cx);
    cx.run_until_parked();

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);

        let chart_bounds = window
            .find((ElementId::from("chart-card"), "balance-sheet"))
            .bounds();
        let plot_bounds = window.find("balance-sheet-plot-area").bounds();
        let assets_mode_btn = (ElementId::from("balance-sheet-mode"), "assets");
        let liab_mode_btn = (ElementId::from("balance-sheet-mode"), "liabilities");
        let mode_btn_bounds = window.find(assets_mode_btn.clone()).bounds();
        let assets_bounds = window
            .find((ElementId::from("account"), "Assets"))
            .bounds();
        assert!(
            mode_btn_bounds.origin.y >= plot_bounds.origin.y + plot_bounds.size.height,
            "Assets|Liabilities toggle sits at the bottom of the chart below the plot: mode={mode_btn_bounds:?}, plot={plot_bounds:?}"
        );
        assert!(
            chart_bounds.origin.y + chart_bounds.size.height <= assets_bounds.origin.y,
            "balance sheet chart card sits above the Assets table: chart={chart_bounds:?}, assets={assets_bounds:?}"
        );

        let eur_checkbox = (ElementId::from("chart-currency"), "EUR");
        let usd_checkbox = (ElementId::from("chart-currency"), "USD");
        assert_eq!(window.find(eur_checkbox.clone()).checked(), Some(true));
        assert_eq!(window.find(usd_checkbox.clone()).checked(), Some(true));

        let report = app.read(cx).trie_report().expect("balance sheet shown");
        assert_eq!(report.read(cx).balance_sheet_mode(), BalanceSheetMode::Assets);
        assert_eq!(report.read(cx).selected_chart_currencies(), ["EUR", "USD"]);

        // Hovering over the plot area renders the crosshair/tooltip cleanly.
        window.hover("balance-sheet-plot-area", cx);
        window.render_frame(cx);

        // Switch bottom toggle to Liabilities: only USD has liabilities in the fixture.
        window.click(liab_mode_btn, cx);
        window.render_frame(cx);
        assert_eq!(
            report.read(cx).balance_sheet_mode(),
            BalanceSheetMode::Liabilities
        );
        assert_eq!(report.read(cx).selected_chart_currencies(), ["USD"]);
        assert!(window.try_find(eur_checkbox.clone()).is_none());
        assert!(window.try_find("balance-sheet-plot-area").is_some());
        window.hover("balance-sheet-plot-area", cx);
        window.render_frame(cx);

        // Switch back to Assets.
        window.click(assets_mode_btn, cx);
        window.render_frame(cx);
        assert_eq!(report.read(cx).balance_sheet_mode(), BalanceSheetMode::Assets);
        assert_eq!(report.read(cx).selected_chart_currencies(), ["EUR", "USD"]);

        // Deselect EUR: USD remains active and the plot stays rendered.
        window.click(eur_checkbox.clone(), cx);
        window.render_frame(cx);
        assert_eq!(window.find(eur_checkbox).checked(), Some(false));
        assert_eq!(window.find(usd_checkbox.clone()).checked(), Some(true));
        assert_eq!(report.read(cx).selected_chart_currencies(), ["USD"]);
        assert!(window.try_find("balance-sheet-plot-area").is_some());

        // Deselect USD too: plot area is replaced by the empty-selection prompt.
        window.click(usd_checkbox.clone(), cx);
        window.render_frame(cx);
        assert_eq!(window.find(usd_checkbox.clone()).checked(), Some(false));
        assert!(report.read(cx).selected_chart_currencies().is_empty());
        assert!(window.try_find("balance-sheet-plot-area").is_none());

        // Re-selecting USD restores the line plot.
        window.click(usd_checkbox, cx);
        window.render_frame(cx);
        assert_eq!(report.read(cx).selected_chart_currencies(), ["USD"]);
        assert!(window.try_find("balance-sheet-plot-area").is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
fn test_income_statement_bar_chart_switches_intervals_and_filters_currencies(
    cx: &mut TestAppContext,
) {
    let (handle, app) = open_app(Page::Income, size(px(1200.), px(800.)), cx);
    cx.run_until_parked();

    let initial_table_gen = cx.read(|cx| {
        let report = app.read(cx).trie_report().expect("income report shown");
        assert_eq!(report.read(cx).income_chart_mode(), IncomeChartMode::Income);
        assert_eq!(report.read(cx).income_interval(), TimeInterval::Monthly);
        // Income in the fixture only has USD (`Income:Salary`).
        assert_eq!(report.read(cx).selected_chart_currencies(), ["USD"]);
        report.read(cx).generation
    });

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let chart_bounds = window
            .find((ElementId::from("chart-card"), "income"))
            .bounds();
        let plot_bounds = window.find("income-expense-plot-area").bounds();
        let income_mode_btn = (ElementId::from("income-chart-mode"), "income");
        let expenses_mode_btn = (ElementId::from("income-chart-mode"), "expenses");
        let mode_btn_bounds = window.find(income_mode_btn).bounds();
        let income_bounds = window
            .find((ElementId::from("account"), "Income"))
            .bounds();
        assert!(
            mode_btn_bounds.origin.y >= plot_bounds.origin.y + plot_bounds.size.height,
            "Income|Expenses toggle sits at the bottom of the chart below the plot: mode={mode_btn_bounds:?}, plot={plot_bounds:?}"
        );
        assert!(
            chart_bounds.origin.y + chart_bounds.size.height <= income_bounds.origin.y,
            "income bar chart sits above the Income table: chart={chart_bounds:?}, income={income_bounds:?}"
        );

        // Switch bottom toggle to Expenses (which has both EUR and USD in the fixture).
        window.click(expenses_mode_btn, cx);
        window.render_frame(cx);
        let report = app.read(cx).trie_report().expect("income report shown");
        assert_eq!(
            report.read(cx).income_chart_mode(),
            IncomeChartMode::Expenses
        );
        assert_eq!(report.read(cx).selected_chart_currencies(), ["EUR", "USD"]);

        // Deselect EUR in the checklist and hover the bar chart.
        window.click((ElementId::from("chart-currency"), "EUR"), cx);
        window.render_frame(cx);
        window.hover("income-expense-plot-area", cx);
        window.render_frame(cx);
    })
    .unwrap();

    // Switch across Daily -> Quarterly -> Yearly; verify Expenses mode and
    // currency selection persist and hierarchy tables are not needlessly re-queried.
    for (short_id, expected) in [
        ("daily", TimeInterval::Daily),
        ("quarterly", TimeInterval::Quarterly),
        ("yearly", TimeInterval::Yearly),
    ] {
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.click((ElementId::from("income-interval"), short_id), cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let report = app.read(cx).trie_report().expect("income report shown");
            assert_eq!(
                report.read(cx).income_chart_mode(),
                IncomeChartMode::Expenses
            );
            assert_eq!(report.read(cx).income_interval(), expected);
            assert_eq!(report.read(cx).selected_chart_currencies(), ["USD"]);
            assert_eq!(
                report.read(cx).generation,
                initial_table_gen,
                "changing chart interval should not re-query hierarchy tables"
            );
            assert!(window.try_find("income-expense-plot-area").is_some());
            window.hover("income-expense-plot-area", cx);
            window.render_frame(cx);
        })
        .unwrap();
    }
}

#[gpui_kit::test]
fn test_balance_sheet_and_income_charts_support_date_range_presets_and_custom_picker(
    cx: &mut TestAppContext,
) {
    let ymd = |y, m, d| NaiveDate::from_ymd_opt(y, m, d).unwrap();

    // 1. Balance Sheet chart date range presets + custom DatePicker.
    let (bs_handle, bs_app) = open_app(Page::BalanceSheet, size(px(1200.), px(800.)), cx);
    cx.run_until_parked();

    cx.update_window(bs_handle, |_, window, cx| {
        let report = bs_app.read(cx).trie_report().expect("balance sheet shown");
        report.update(cx, |r, cx| r.set_chart_today(ymd(2025, 6, 15), cx));
        window.render_frame(cx);

        assert_eq!(report.read(cx).chart_date_preset(), ChartDatePreset::All);
        assert_eq!(report.read(cx).chart_date_range(), ChartDateRange::ALL);
        assert!(window.try_find("chart-custom-date-picker").is_none());

        for (short_id, preset, expected_range) in [
            (
                "1y",
                ChartDatePreset::OneYear,
                ChartDateRange::between(ymd(2024, 6, 15), ymd(2025, 6, 15)),
            ),
            (
                "3y",
                ChartDatePreset::ThreeYears,
                ChartDateRange::between(ymd(2022, 6, 15), ymd(2025, 6, 15)),
            ),
            (
                "10y",
                ChartDatePreset::TenYears,
                ChartDateRange::between(ymd(2015, 6, 15), ymd(2025, 6, 15)),
            ),
            (
                "ytd",
                ChartDatePreset::YearToDate,
                ChartDateRange::between(ymd(2025, 1, 1), ymd(2025, 6, 15)),
            ),
        ] {
            window.click((ElementId::from("chart-range"), short_id), cx);
            window.render_frame(cx);
            assert_eq!(report.read(cx).chart_date_preset(), preset);
            assert_eq!(report.read(cx).chart_date_range(), expected_range);
            assert!(window.try_find("balance-sheet-plot-area").is_some());
        }

        // Clicking `Custom` reveals the range DatePicker seeded from the prior
        // range (`2025-01-01 ..= 2025-06-15`).
        window.click((ElementId::from("chart-range"), "custom"), cx);
        window.render_frame(cx);
        assert_eq!(report.read(cx).chart_date_preset(), ChartDatePreset::Custom);
        assert!(window.try_find("chart-custom-date-picker").is_some());

        // Open the DatePicker popup and pick its first quick preset ("Past 1 year").
        let picker = report
            .read(cx)
            .chart_date_picker()
            .expect("date picker initialized")
            .clone();
        window.click(("date-picker", picker.entity_id()), cx);
        window.render_frame(cx);
        window.click(("preset", 0usize), cx);
    })
    .unwrap();
    cx.run_until_parked();

    cx.update_window(bs_handle, |_, window, cx| {
        window.render_frame(cx);
        let report = bs_app.read(cx).trie_report().expect("balance sheet shown");
        assert_eq!(
            report.read(cx).chart_date_range(),
            ChartDateRange::between(ymd(2024, 6, 15), ymd(2025, 6, 15))
        );
        assert!(window.try_find("balance-sheet-plot-area").is_some());

        // Choosing a custom range strictly before the ledger (`2020`) keeps the
        // chart card and range controls mounted while hiding the plot area.
        let picker = report
            .read(cx)
            .chart_date_picker()
            .expect("date picker initialized")
            .clone();
        picker.update(cx, |state, cx| {
            state.set_date(
                Date::Range(Some(ymd(2020, 1, 1)), Some(ymd(2020, 12, 31))),
                window,
                cx,
            );
            cx.emit(DatePickerEvent::Change(state.date_time()));
        });
    })
    .unwrap();
    cx.run_until_parked();

    cx.update_window(bs_handle, |_, window, cx| {
        window.render_frame(cx);
        let report = bs_app.read(cx).trie_report().expect("balance sheet shown");
        assert_eq!(
            report.read(cx).chart_date_range(),
            ChartDateRange::between(ymd(2020, 1, 1), ymd(2020, 12, 31))
        );
        assert!(
            window
                .try_find((ElementId::from("chart-card"), "balance-sheet"))
                .is_some()
        );
        assert!(window.try_find("chart-date-range-bar").is_some());
        assert!(window.try_find("balance-sheet-plot-area").is_none());

        // Clicking `All` restores the full timeline and hides the custom picker.
        window.click((ElementId::from("chart-range"), "all"), cx);
        window.render_frame(cx);
        assert_eq!(report.read(cx).chart_date_preset(), ChartDatePreset::All);
        assert_eq!(report.read(cx).chart_date_range(), ChartDateRange::ALL);
        assert!(window.try_find("chart-custom-date-picker").is_none());
        assert!(window.try_find("balance-sheet-plot-area").is_some());
    })
    .unwrap();

    // 2. Income Statement chart date range presets + empty-range resilience.
    let (inc_handle, inc_app) = open_app(Page::Income, size(px(1200.), px(800.)), cx);
    cx.run_until_parked();

    let initial_table_gen = cx.read(|cx| {
        inc_app
            .read(cx)
            .trie_report()
            .expect("income report shown")
            .read(cx)
            .generation
    });

    cx.update_window(inc_handle, |_, window, cx| {
        let report = inc_app.read(cx).trie_report().expect("income report shown");
        report.update(cx, |r, cx| r.set_chart_today(ymd(2025, 6, 15), cx));
        window.render_frame(cx);
        window.click((ElementId::from("chart-range"), "ytd"), cx);
    })
    .unwrap();
    cx.run_until_parked();

    cx.update_window(inc_handle, |_, window, cx| {
        window.render_frame(cx);
        let report = inc_app.read(cx).trie_report().expect("income report shown");
        assert_eq!(
            report.read(cx).chart_date_preset(),
            ChartDatePreset::YearToDate
        );
        assert_eq!(
            report.read(cx).chart_date_range(),
            ChartDateRange::between(ymd(2025, 1, 1), ymd(2025, 6, 15))
        );
        assert_eq!(
            report.read(cx).generation,
            initial_table_gen,
            "changing chart date range should not re-query hierarchy tables"
        );
        // Ledger-wide Income currency (`USD`) stays available even though 2025
        // only had EUR Expenses.
        assert_eq!(report.read(cx).selected_chart_currencies(), ["USD"]);
        assert!(window.try_find("income-expense-plot-area").is_some());

        // Move `today` to 2026 where YTD has zero postings: chart card and
        // range controls stay visible while the plot area shows the empty message.
        report.update(cx, |r, cx| r.set_chart_today(ymd(2026, 6, 15), cx));
    })
    .unwrap();
    cx.run_until_parked();

    cx.update_window(inc_handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window
                .try_find((ElementId::from("chart-card"), "income"))
                .is_some()
        );
        assert!(window.try_find("chart-date-range-bar").is_some());
        assert!(window.try_find("income-expense-plot-area").is_none());

        window.click((ElementId::from("chart-range"), "all"), cx);
    })
    .unwrap();
    cx.run_until_parked();

    cx.update_window(inc_handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("income-expense-plot-area").is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
fn test_dragging_chart_bottom_border_adjusts_balance_sheet_and_income_chart_heights(
    cx: &mut TestAppContext,
) {
    // 1. Balance Sheet chart: drag bottom border down, up, and past min/max bounds.
    let (bs_handle, _bs_app) = open_app(Page::BalanceSheet, size(px(1200.), px(900.)), cx);
    cx.run_until_parked();

    cx.update_window(bs_handle, |_, window, cx| {
        window.render_frame(cx);
        let initial_card = window
            .find((ElementId::from("chart-card"), "balance-sheet"))
            .bounds();
        let initial_plot = window.find("balance-sheet-plot-area").bounds();
        let handle_bounds = window
            .find((ElementId::from("chart-resize-handle"), "balance-sheet"))
            .bounds();
        assert_eq!(initial_plot.size.height, DEFAULT_BALANCE_SHEET_PLOT_HEIGHT);

        // The resize handle straddles the chart card's bottom border line.
        let border_point = point(
            initial_card.origin.x + initial_card.size.width * 0.5,
            initial_card.origin.y + initial_card.size.height - px(1.),
        );
        assert!(handle_bounds.contains(&border_point));

        // Drag the bottom border down by 90px.
        let drag_down = point(border_point.x, border_point.y + px(90.));
        window.pointer_down(border_point, MouseButton::Left, cx);
        window.pointer_move(drag_down, Some(MouseButton::Left), cx);
        window.pointer_up(drag_down, MouseButton::Left, cx);
        window.render_frame(cx);

        let taller_card = window
            .find((ElementId::from("chart-card"), "balance-sheet"))
            .bounds();
        let taller_plot = window.find("balance-sheet-plot-area").bounds();
        assert_eq!(
            taller_plot.size.height,
            DEFAULT_BALANCE_SHEET_PLOT_HEIGHT + px(90.)
        );
        assert_eq!(taller_card.size.height, initial_card.size.height + px(90.));

        // Drag the bottom border up by 130px (`180 + 90 - 130 = 140px`).
        let handle_center = window
            .find((ElementId::from("chart-resize-handle"), "balance-sheet"))
            .bounds()
            .center();
        let drag_up = point(handle_center.x, handle_center.y - px(130.));
        window.pointer_down(handle_center, MouseButton::Left, cx);
        window.pointer_move(drag_up, Some(MouseButton::Left), cx);
        window.pointer_up(drag_up, MouseButton::Left, cx);
        window.render_frame(cx);

        let shorter_plot = window.find("balance-sheet-plot-area").bounds();
        assert_eq!(
            shorter_plot.size.height,
            DEFAULT_BALANCE_SHEET_PLOT_HEIGHT - px(40.)
        );

        // Drag far upward and far downward to verify clamping to `[MIN_CHART_PLOT_HEIGHT, MAX_CHART_PLOT_HEIGHT]`.
        let handle_center = window
            .find((ElementId::from("chart-resize-handle"), "balance-sheet"))
            .bounds()
            .center();
        let far_up = point(handle_center.x, handle_center.y - px(500.));
        window.pointer_down(handle_center, MouseButton::Left, cx);
        window.pointer_move(far_up, Some(MouseButton::Left), cx);
        window.pointer_up(far_up, MouseButton::Left, cx);
        window.render_frame(cx);
        assert_eq!(
            window.find("balance-sheet-plot-area").bounds().size.height,
            MIN_CHART_PLOT_HEIGHT
        );

        let handle_center = window
            .find((ElementId::from("chart-resize-handle"), "balance-sheet"))
            .bounds()
            .center();
        let far_down = point(handle_center.x, handle_center.y + px(900.));
        window.pointer_down(handle_center, MouseButton::Left, cx);
        window.pointer_move(far_down, Some(MouseButton::Left), cx);
        window.pointer_up(far_down, MouseButton::Left, cx);
        window.render_frame(cx);
        assert_eq!(
            window.find("balance-sheet-plot-area").bounds().size.height,
            MAX_CHART_PLOT_HEIGHT
        );
    })
    .unwrap();

    // 2. Income Statement bar chart: drag bottom border down to expand height.
    let (inc_handle, _inc_app) = open_app(Page::Income, size(px(1200.), px(900.)), cx);
    cx.run_until_parked();

    cx.update_window(inc_handle, |_, window, cx| {
        window.render_frame(cx);
        let initial_card = window
            .find((ElementId::from("chart-card"), "income"))
            .bounds();
        let initial_plot = window.find("income-expense-plot-area").bounds();
        assert_eq!(initial_plot.size.height, DEFAULT_INCOME_PLOT_HEIGHT);

        let handle_center = window
            .find((ElementId::from("chart-resize-handle"), "income"))
            .bounds()
            .center();
        let drag_down = point(handle_center.x, handle_center.y + px(75.));
        window.pointer_down(handle_center, MouseButton::Left, cx);
        window.pointer_move(drag_down, Some(MouseButton::Left), cx);
        window.pointer_up(drag_down, MouseButton::Left, cx);
        window.render_frame(cx);

        let resized_card = window
            .find((ElementId::from("chart-card"), "income"))
            .bounds();
        let resized_plot = window.find("income-expense-plot-area").bounds();
        assert_eq!(
            resized_plot.size.height,
            DEFAULT_INCOME_PLOT_HEIGHT + px(75.)
        );
        assert_eq!(resized_card.size.height, initial_card.size.height + px(75.));
    })
    .unwrap();
}
