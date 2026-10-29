use std::collections::HashMap;

use rust_decimal::Decimal;

use crate::Ledger;
use crate::query::{completions, journal, positions, trie, trie_table};
use crate::web::{FilterOptions, TrieOptions};

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src/query_test.bean");

fn fixture() -> Ledger {
    let (ledger, errors) = Ledger::from_file(FIXTURE);
    assert!(errors.is_empty(), "fixture has errors: {errors:?}");
    ledger
}

fn dec(s: &str) -> Decimal {
    s.parse().unwrap()
}

/// Compact view of a journal item: `(date, narration, changes, balance)`
/// with the per-currency maps sorted.
type Row = (
    String,
    String,
    Vec<(String, Decimal)>,
    Vec<(String, Decimal)>,
);

fn page(ledger: &Ledger, account: Option<&str>, options: &FilterOptions) -> (Vec<Row>, usize) {
    let (items, total) = journal(ledger, account, options);
    let sorted = |map: &HashMap<&str, Decimal>| {
        let mut list: Vec<_> = map.iter().map(|(c, n)| (c.to_string(), *n)).collect();
        list.sort();
        list
    };
    let rows = items
        .iter()
        .map(|item| {
            (
                item.txn.date().to_string(),
                item.txn.narration().to_string(),
                sorted(&item.changes),
                sorted(&item.balance),
            )
        })
        .collect();
    (rows, total)
}

fn dates(rows: &[Row]) -> Vec<&str> {
    rows.iter().map(|r| r.0.as_str()).collect()
}

fn filter(entries: usize, page: usize, old_first: bool) -> FilterOptions {
    FilterOptions {
        entries: Some(entries),
        page: Some(page),
        old_first: Some(old_first),
        ..Default::default()
    }
}

#[test]
fn test_positions() {
    let ledger = fixture();
    let positions = positions(ledger.balance_sheet());
    assert_eq!(positions.len(), 10);

    let mut checking: Vec<_> = positions["Assets:Bank:Checking"]
        .iter()
        .map(|p| (p.currency.as_str(), p.number, p.cost.is_some()))
        .collect();
    checking.sort();
    assert_eq!(
        checking,
        [
            ("EUR", dec("-300.00"), false),
            ("USD", dec("1920.00"), false)
        ]
    );

    // A position held at cost keeps its cost.
    let brokerage = &positions["Assets:Investments:Brokerage"];
    assert_eq!(brokerage.len(), 1);
    assert_eq!(brokerage[0].currency, "VTI");
    assert_eq!(brokerage[0].number, dec("10"));
    let cost = brokerage[0].cost.as_ref().unwrap();
    assert_eq!(cost.amount.number, dec("250.00"));
    assert_eq!(cost.amount.currency, "USD");

    // Closed and paid-off accounts still appear, with a zero balance.
    assert_eq!(positions["Assets:Old:Savings"][0].number, Decimal::ZERO);
    assert_eq!(positions["Liabilities:CreditCard"][0].number, Decimal::ZERO);
}

#[test]
fn test_trie_aggregates_sub_accounts_in_cost_currency() {
    let ledger = fixture();
    let (root, currencies) = trie(&ledger, "Assets", &TrieOptions::default());
    let mut currencies: Vec<_> = currencies.into_iter().collect();
    currencies.sort();
    assert_eq!(currencies, ["EUR", "USD"]);

    assert_eq!(root.nodes.len(), 1);
    let assets = &root.nodes["Assets"];
    // 100 + 1000 + 500 + 3000 - 45.50 - 2500 + 500 - 500 - 80 cash flows, plus
    // 10 VTI held at 250 USD each counted in USD.
    assert_eq!(assets.numbers["USD"], dec("4474.50"));
    assert_eq!(assets.numbers["EUR"], dec("-300.00"));
    assert_eq!(assets.nodes["Investments"].numbers["USD"], dec("2500.00"));
    assert_eq!(
        assets.nodes["Investments"].nodes["Brokerage"].numbers["USD"],
        dec("2500.00")
    );
    assert_eq!(assets.nodes["Cash"].numbers["USD"], dec("54.50"));
    // Closed accounts are left out by default.
    assert!(!assets.nodes.contains_key("Old"));
}

#[test]
fn test_trie_table_rows() {
    let ledger = fixture();
    let rows = |table: &crate::web::TrieTable<&str>| -> Vec<(usize, String, Vec<String>)> {
        table
            .rows
            .iter()
            .map(|row| (row.level, row.name.to_string(), row.numbers.clone()))
            .collect()
    };
    let cells =
        |numbers: &[&str]| -> Vec<String> { numbers.iter().map(|s| s.to_string()).collect() };

    let table = trie_table(&ledger, "Assets", &TrieOptions::default()).unwrap();
    assert_eq!(table.currencies, ["EUR", "USD"]);
    // Depth-first, children sorted by name, zero balances left blank.
    assert_eq!(
        rows(&table),
        [
            (0, "Assets".to_string(), cells(&["-300.00", "4474.50"])),
            (1, "Bank".to_string(), cells(&["-300.00", "1920.00"])),
            (2, "Checking".to_string(), cells(&["-300.00", "1920.00"])),
            (1, "Cash".to_string(), cells(&["", "54.50"])),
            (1, "Investments".to_string(), cells(&["", "2500.00"])),
            (2, "Brokerage".to_string(), cells(&["", "2500.00"])),
        ]
    );

    let options = TrieOptions {
        show_closed: Some(true),
    };
    let table = trie_table(&ledger, "Assets", &options).unwrap();
    assert_eq!(
        &rows(&table)[6..],
        [
            (1, "Old".to_string(), cells(&["", ""])),
            (2, "Savings".to_string(), cells(&["", ""])),
        ]
    );

    let table = trie_table(&ledger, "Expenses", &TrieOptions::default()).unwrap();
    assert_eq!(
        rows(&table),
        [
            (0, "Expenses".to_string(), cells(&["300.00", "125.50"])),
            (1, "Food".to_string(), cells(&["", "125.50"])),
            (2, "Groceries".to_string(), cells(&["", "80.00"])),
            (2, "Restaurants".to_string(), cells(&["", "45.50"])),
            (1, "Travel".to_string(), cells(&["300.00", ""])),
        ]
    );

    // An account whose balance is zero everywhere has no currency columns.
    let table = trie_table(&ledger, "Liabilities", &TrieOptions::default()).unwrap();
    assert!(table.currencies.is_empty());
    assert_eq!(
        rows(&table),
        [
            (0, "Liabilities".to_string(), vec![]),
            (1, "CreditCard".to_string(), vec![]),
        ]
    );

    assert!(trie_table(&ledger, "Nope", &TrieOptions::default()).is_none());
}

#[test]
fn test_journal_newest_first_by_default() {
    let ledger = fixture();
    let (rows, total) = page(&ledger, None, &FilterOptions::default());
    assert_eq!(total, 9);
    let dates: Vec<_> = rows.iter().map(|r| r.0.as_str()).collect();
    assert_eq!(
        dates,
        [
            "2025-06-01",
            "2025-05-20",
            "2024-04-01",
            "2024-03-15",
            "2024-03-01",
            "2024-02-10",
            "2024-02-03",
            "2024-01-15",
            "2024-01-01",
        ]
    );
    // Without an account there is no running balance.
    assert!(rows.iter().all(|r| r.2.is_empty() && r.3.is_empty()));
}

#[test]
fn test_journal_running_balance() {
    let ledger = fixture();
    let (rows, total) = page(&ledger, Some("Assets:Cash"), &FilterOptions::default());
    assert_eq!(total, 3);
    let usd = |n: &str| vec![("USD".to_string(), dec(n))];
    assert_eq!(
        rows,
        [
            // A balance assertion changes nothing but still shows the balance.
            (
                "2024-03-15".to_string(),
                String::new(),
                vec![],
                usd("54.50")
            ),
            (
                "2024-02-10".to_string(),
                "Dinner".to_string(),
                usd("-45.50"),
                usd("54.50")
            ),
            (
                "2024-01-01".to_string(),
                "Opening balances".to_string(),
                usd("100.00"),
                usd("100.00")
            ),
        ]
    );

    // A parent account aggregates its sub-accounts; postings held at cost are
    // not counted in the running balance. The groceries transaction touches
    // no Assets account, so 8 of the 9 transactions match.
    let (rows, total) = page(&ledger, Some("Assets"), &filter(1, 1, true));
    assert_eq!(total, 8);
    assert_eq!(rows[0].2, vec![("USD".to_string(), dec("1600.00"))]);
    let (rows, _) = page(&ledger, Some("Assets"), &filter(1, 4, true));
    assert_eq!(rows[0].1, "Buy shares");
    assert_eq!(rows[0].2, vec![("USD".to_string(), dec("-2500.00"))]);
}

#[test]
fn test_journal_pagination() {
    let ledger = fixture();
    // Oldest first: pages walk forward in time; the running balance carries over.
    let (rows, total) = page(&ledger, Some("Assets:Cash"), &filter(2, 1, true));
    assert_eq!(total, 3);
    assert_eq!(dates(&rows), ["2024-01-01", "2024-02-10"]);
    let (rows, _) = page(&ledger, Some("Assets:Cash"), &filter(2, 2, true));
    assert_eq!(dates(&rows), ["2024-03-15"]);
    assert_eq!(rows[0].3, vec![("USD".to_string(), dec("54.50"))]);

    // Newest first: page 1 holds the latest transactions, the last page the
    // remainder.
    let (rows, _) = page(&ledger, None, &filter(2, 1, false));
    assert_eq!(dates(&rows), ["2025-06-01", "2025-05-20"]);
    let (rows, _) = page(&ledger, None, &filter(2, 2, false));
    assert_eq!(dates(&rows), ["2024-04-01", "2024-03-15"]);
    let (rows, _) = page(&ledger, None, &filter(2, 5, false));
    assert_eq!(dates(&rows), ["2024-01-01"]);

    // Past the end: nothing, but the total is still reported.
    let (rows, total) = page(&ledger, None, &filter(2, 7, false));
    assert!(rows.is_empty());
    assert_eq!(total, 9);
    // Page 0 and a zero page size are clamped to 1.
    let (rows, _) = page(&ledger, None, &filter(2, 0, true));
    assert_eq!(dates(&rows), ["2024-01-01", "2024-01-15"]);
    let (rows, _) = page(&ledger, None, &filter(0, 1, true));
    assert_eq!(dates(&rows), ["2024-01-01"]);
}

#[test]
fn test_journal_filters() {
    let ledger = fixture();
    let year = FilterOptions {
        time: Some("2025".to_string()),
        ..Default::default()
    };
    let (rows, total) = page(&ledger, None, &year);
    assert_eq!(total, 2);
    assert_eq!(dates(&rows), ["2025-06-01", "2025-05-20"]);

    // The account option is a prefix filter and combines with the year.
    let expenses_2024 = FilterOptions {
        account: Some("Expenses".to_string()),
        time: Some("2024".to_string()),
        ..Default::default()
    };
    let (rows, total) = page(&ledger, None, &expenses_2024);
    assert_eq!(total, 2);
    assert_eq!(dates(&rows), ["2024-02-10", "2024-02-03"]);

    // The account option filters but does not add a running balance; the
    // account argument does both.
    assert!(rows.iter().all(|r| r.3.is_empty()));
    let (rows, _) = page(&ledger, Some("Expenses:Food"), &expenses_2024);
    assert_eq!(rows[0].3, vec![("USD".to_string(), dec("125.50"))]);

    // A time that is not a year is ignored.
    let bad_time = FilterOptions {
        time: Some("abc".to_string()),
        ..Default::default()
    };
    assert_eq!(page(&ledger, None, &bad_time).1, 9);
}

#[test]
fn test_journal_search_query_matches_date_account_payee_narration_and_currency() {
    let ledger = fixture();
    let search = |q: &str| FilterOptions {
        query: Some(q.to_string()),
        ..Default::default()
    };

    // Date substring.
    let (rows, total) = page(&ledger, None, &search("2024-03"));
    assert_eq!(total, 2);
    assert_eq!(dates(&rows), ["2024-03-15", "2024-03-01"]);

    // Account name (case-insensitive).
    let (rows, total) = page(&ledger, None, &search("brokerage"));
    assert_eq!(total, 1);
    assert_eq!(dates(&rows), ["2024-03-01"]);

    // Payee and narration (case-insensitive).
    let (rows, total) = page(&ledger, None, &search("bistro"));
    assert_eq!(total, 1);
    assert_eq!(dates(&rows), ["2024-02-10"]);

    let (rows, total) = page(&ledger, None, &search("flight to paris"));
    assert_eq!(total, 1);
    assert_eq!(dates(&rows), ["2025-05-20"]);

    // Currency on postings (including held-at-cost shares).
    let (rows, total) = page(&ledger, None, &search("vti"));
    assert_eq!(total, 1);
    assert_eq!(dates(&rows), ["2024-03-01"]);

    let (rows, total) = page(&ledger, None, &search("EUR"));
    assert_eq!(total, 1);
    assert_eq!(dates(&rows), ["2025-05-20"]);

    // Multi-word terms must all match across any fields of the entry.
    let (rows, total) = page(&ledger, None, &search("2024 cash 54.50"));
    assert_eq!(total, 1);
    assert_eq!(dates(&rows), ["2024-03-15"]);

    // Blank query matches all entries.
    assert_eq!(page(&ledger, None, &search("   ")).1, 9);
}

#[test]
fn test_completions_extracts_open_accounts_currencies_payees_and_narrations() {
    let ledger = fixture();
    let got = completions(&ledger);

    // Closed account Assets:Old:Savings is excluded; open accounts are sorted.
    assert_eq!(
        got.accounts,
        [
            "Assets:Bank:Checking",
            "Assets:Cash",
            "Assets:Investments:Brokerage",
            "Equity:Opening-Balances",
            "Expenses:Food:Groceries",
            "Expenses:Food:Restaurants",
            "Expenses:Travel",
            "Income:Salary",
            "Liabilities:CreditCard",
        ]
    );

    // Operating and most-recently-posted currencies appear first.
    assert_eq!(got.currencies, ["USD", "EUR", "VTI"]);

    // Payees and narrations are distinct, non-empty, and newest-first.
    assert_eq!(
        got.payees,
        ["Airline", "Broker", "Bistro", "Grocer", "Employer"]
    );
    assert_eq!(
        got.narrations,
        [
            "Pay card",
            "Flight to Paris",
            "Close old account",
            "Buy shares",
            "Dinner",
            "Weekly groceries",
            "Salary",
            "Opening balances",
        ]
    );
    assert_eq!(got.payee_narrations["Airline"], ["Flight to Paris"]);
    assert_eq!(got.payee_narrations["Grocer"], ["Weekly groceries"]);
    assert!(!got.payee_narrations.contains_key(""));
    assert_eq!(got.tags, ["#paris-2025", "#groceries"]);
    assert_eq!(got.links, ["^flight-receipt", "^grocery-receipt"]);
}

#[test]
fn test_completions_deduplicates_payee_narrations_and_includes_declared_commodities() {
    let path = std::env::temp_dir().join(format!(
        "lumi-completions-query-test-{}.bean",
        std::process::id()
    ));
    std::fs::write(
        &path,
        "\
2024-01-01 commodity CHF
2024-01-01 open Assets:Cash USD,GBP
2024-01-01 open Expenses:Coffee
pushtag #coffee-run
2024-01-02 * \"Cafe\" \"Latte\" #morning ^inv-1
  Expenses:Coffee  5.00 EUR @ 1.10 USD
  Assets:Cash     -5.50 USD
poptag #coffee-run
2024-01-05 * \"Cafe\" \"Espresso\" #afternoon ^inv-2
  Expenses:Coffee  3.50 USD
  Assets:Cash     -3.50 USD
2024-01-09 * \"Cafe\" \"Latte\" #morning ^inv-1
  Expenses:Coffee  5.00 USD
  Assets:Cash     -5.00 USD
",
    )
    .unwrap();
    let (ledger, errors) = Ledger::from_file(path.to_str().unwrap());
    let _ = std::fs::remove_file(&path);
    assert!(errors.is_empty(), "{errors:?}");

    let got = completions(&ledger);
    assert_eq!(got.payees, ["Cafe"]);
    assert_eq!(got.narrations, ["Latte", "Espresso"]);
    assert_eq!(got.payee_narrations["Cafe"], ["Latte", "Espresso"]);
    assert_eq!(got.tags, ["#morning", "#afternoon", "#coffee-run"]);
    assert_eq!(got.links, ["^inv-1", "^inv-2"]);
    // Used currencies (USD, EUR from @ price) come before declared-only (CHF, GBP).
    assert_eq!(got.currencies, ["USD", "EUR", "CHF", "GBP"]);
}
