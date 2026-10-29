use std::sync::Arc;

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::table::TableDelegate as _;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{
    AnyWindowHandle, AppContext as _, Bounds, Entity, FontWeight, Styled as _, TestAppContext,
    WindowBounds, WindowOptions, px, size,
};
use lumi::web::Position;
use rust_decimal::Decimal;

use crate::LumiApp;
use crate::holdings_report::{HoldingsReport, holdings};
use crate::navigation::{Page, ReportEvent};
use crate::session::LedgerSession;
use crate::source::{LocalLedger, Positions};

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../lumi/src/query_test.bean");

fn position(number: &str, currency: &str) -> Position {
    Position {
        currency: currency.to_string(),
        number: number.parse().unwrap(),
        cost: None,
    }
}

#[test]
fn test_holdings_rows() {
    let mut positions = Positions::new();
    positions.insert(
        "Assets:Cash".into(),
        vec![
            position("0", "EUR"),
            position("54.50", "USD"),
            position("10", "CNY"),
        ],
    );
    positions.insert("Liabilities:Card".into(), vec![position("-80", "USD")]);
    positions.insert("Expenses:Food".into(), vec![position("80", "USD")]);
    positions.insert("Equity:Opening".into(), vec![position("-1", "USD")]);
    positions.insert(
        "Assets:Brokerage".into(),
        vec![Position {
            currency: "VTI".into(),
            number: Decimal::from(10),
            cost: Some(lumi::UnitCost {
                amount: lumi::Amount {
                    number: "250.00".parse().unwrap(),
                    currency: "USD".into(),
                },
                date: "2024-03-01".parse().unwrap(),
            }),
        }],
    );

    let rows = holdings(positions);
    let summary: Vec<(&str, &str, &str, &str, &str)> = rows
        .iter()
        .map(|row| {
            (
                row.account.as_ref(),
                row.units.as_ref(),
                row.cost.as_ref(),
                row.acquired.as_ref(),
                row.book_value.as_ref(),
            )
        })
        .collect();
    // Assets and liabilities only, by account then currency; zero positions
    // are left out; a position at cost shows its cost and book value.
    assert_eq!(
        summary,
        [
            (
                "Assets:Brokerage",
                "10 VTI",
                "250.00 USD",
                "2024-03-01",
                "2500.00 USD"
            ),
            ("Assets:Cash", "10 CNY", "", "", "10 CNY"),
            ("Assets:Cash", "54.50 USD", "", "", "54.50 USD"),
            ("Liabilities:Card", "-80 USD", "", "", "-80 USD"),
        ]
    );
}

fn open_holdings(cx: &mut TestAppContext) -> Entity<HoldingsReport> {
    cx.update(crate::init);
    let session = cx.new(|cx| {
        let mut session = LedgerSession::new(Arc::new(LocalLedger::new(FIXTURE)));
        session.reload(cx);
        session
    });
    let (report, _) =
        cx.add_window_view(|window, cx| HoldingsReport::new(session.clone(), window, cx));
    cx.run_until_parked();
    report
}

#[gpui_kit::test]
fn test_holdings_load_and_open_accounts(cx: &mut TestAppContext) {
    let report = open_holdings(cx);
    cx.read(|cx| {
        assert_eq!(
            report.read(cx).accounts(cx),
            [
                "Assets:Bank:Checking",
                "Assets:Bank:Checking",
                "Assets:Cash",
                "Assets:Investments:Brokerage",
            ]
        );
    });

    let opened = Arc::new(std::sync::Mutex::new(Vec::new()));
    let _subscription = cx.update(|cx| {
        let opened = opened.clone();
        cx.subscribe(&report, move |_, event: &ReportEvent, _| {
            let ReportEvent::OpenAccount(account) = event;
            opened.lock().unwrap().push(account.to_string());
        })
    });
    report.update(cx, |report, cx| report.open_row(2, cx));
    cx.run_until_parked();
    assert_eq!(opened.lock().unwrap().as_slice(), ["Assets:Cash"]);
}

#[gpui_kit::test]
fn test_clicking_holding_account_button_opens_its_account(cx: &mut TestAppContext) {
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
                        Page::Holdings,
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
        window.click(("holding-account", 3usize), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.read(|cx| {
        assert_eq!(
            app.read(cx).page(),
            &Page::Account("Assets:Investments:Brokerage".into())
        );
    });
}

#[gpui_kit::test]
fn test_holdings_rows_alternate_background_and_header_uses_medium_weight(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let session = cx.new(|cx| {
        let mut session = LedgerSession::new(Arc::new(LocalLedger::new(FIXTURE)));
        session.reload(cx);
        session
    });
    let (report, window) =
        cx.add_window_view(|window, cx| HoldingsReport::new(session.clone(), window, cx));
    window.run_until_parked();

    window.update(|window, cx| {
        let table = report.read(cx).table.clone();
        let even_bg = Some(cx.theme().table.into());
        let odd_bg = Some(cx.theme().muted.into());

        let header_weight = table.update(cx, |state, cx| {
            state
                .delegate_mut()
                .render_header(window, cx)
                .style()
                .text
                .font_weight
        });
        assert_eq!(header_weight, Some(FontWeight::MEDIUM));

        for row_ix in 0..4 {
            let bg = table.update(cx, |state, cx| {
                state
                    .delegate_mut()
                    .render_tr(row_ix, window, cx)
                    .style()
                    .background
                    .clone()
            });
            let expected = if row_ix % 2 == 0 { &even_bg } else { &odd_bg };
            assert_eq!(&bg, expected);
        }
        let filler_bg = table.update(cx, |state, cx| {
            state
                .delegate_mut()
                .render_tr(4, window, cx)
                .style()
                .background
                .clone()
        });
        assert_eq!(filler_bg, None);
    });
}

#[gpui_kit::test]
fn test_holdings_search_shortcut_filters_by_currency_date_and_account(cx: &mut TestAppContext) {
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
                        Page::Holdings,
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

    cx.simulate_keystrokes(handle, find_shortcut);
    cx.update_window(handle, |_, window, cx| {
        let report = app.read(cx).holdings().expect("holdings report is shown");
        assert!(report.read(cx).is_search_focused(window, cx));
    })
    .unwrap();

    // Filter by currency and acquisition date ("2024-03 VTI").
    cx.simulate_keystrokes(handle, "2 0 2 4 - 0 3 space v t i");
    cx.run_until_parked();
    cx.read(|cx| {
        let report = app.read(cx).holdings().expect("holdings report is shown");
        assert_eq!(
            report.read(cx).accounts(cx),
            ["Assets:Investments:Brokerage"]
        );
    });

    // Escape clears the query and restores all holdings.
    cx.simulate_keystrokes(handle, "escape");
    cx.run_until_parked();
    cx.read(|cx| {
        let report = app.read(cx).holdings().expect("holdings report is shown");
        assert_eq!(report.read(cx).accounts(cx).len(), 4);
    });
}
