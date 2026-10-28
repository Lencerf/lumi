use std::sync::Arc;

use gpui_kit::{AppContext as _, Entity, TestAppContext};
use lumi::web::Position;
use rust_decimal::Decimal;

use crate::holdings_report::{HoldingsReport, holdings};
use crate::navigation::ReportEvent;
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
