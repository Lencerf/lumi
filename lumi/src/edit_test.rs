use std::fs;
use std::path::PathBuf;

use crate::edit::{EditError, entry_text, replace_entry_text};
use crate::{Ledger, TxnFlag};

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src/query_test.bean");

const OPENING: &str = "2024-01-01 * \"Opening balances\"
  Assets:Cash               100.00 USD
  Assets:Bank:Checking     1000.00 USD
  Assets:Old:Savings        500.00 USD
  Equity:Opening-Balances";

const BISTRO: &str = "2024-02-10 * \"Bistro\" \"Dinner\"
  Expenses:Food:Restaurants  45.50 USD
  Assets:Cash               -45.50 USD";

/// A file of this test's own, with `text` in it; removed when dropped.
struct TempFile(PathBuf);

impl TempFile {
    fn new(name: &str, text: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("lumi-edit-test-{}-{name}.bean", std::process::id()));
        fs::write(&path, text).unwrap();
        Self(path)
    }

    fn copy_of_fixture(name: &str) -> Self {
        Self::new(name, &fs::read_to_string(FIXTURE).unwrap())
    }

    fn path(&self) -> &str {
        self.0.to_str().unwrap()
    }

    fn text(&self) -> String {
        fs::read_to_string(&self.0).unwrap()
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

#[test]
fn test_entry_text_is_the_first_line_and_the_indented_lines_after_it() {
    assert_eq!(entry_text(FIXTURE, 15).unwrap(), OPENING);
    assert_eq!(entry_text(FIXTURE, 29).unwrap(), BISTRO);
    // A directive on one line.
    assert_eq!(
        entry_text(FIXTURE, 37).unwrap(),
        "2024-03-15 balance Assets:Cash 54.50 USD"
    );
    // The last entry, which the end of the file ends.
    assert_eq!(
        entry_text(FIXTURE, 49).unwrap(),
        "2025-06-01 * \"Pay card\"\n  Liabilities:CreditCard     80.00 USD\n  Assets:Bank:Checking      -80.00 USD"
    );
}

#[test]
fn test_entry_text_starts_where_the_parser_says_a_transaction_does() {
    let (ledger, errors) = Ledger::from_file(FIXTURE);
    assert!(errors.is_empty(), "{errors:?}");
    for txn in ledger.txns() {
        let text = entry_text(&txn.src().file, txn.src().start.line).unwrap();
        assert!(
            text.starts_with(&txn.date().to_string()),
            "{} does not start with {}",
            text,
            txn.date()
        );
        assert_eq!(
            text.lines().count(),
            // A balance directive is one line; lumi gives it one posting.
            if txn.flag() == TxnFlag::Balance {
                1
            } else {
                1 + txn.postings().len()
            }
        );
    }
}

#[test]
fn test_entry_text_follows_a_string_across_lines() {
    let file = TempFile::new(
        "strings",
        "2024-01-01 * \"A narration ; with a semicolon\nover two lines\"\n  Assets:Cash  1 USD ; a \"comment\"\n  Equity:Opening-Balances\n\n2024-01-02 * \"Next\"\n",
    );
    assert_eq!(
        entry_text(file.path(), 1).unwrap(),
        "2024-01-01 * \"A narration ; with a semicolon\nover two lines\"\n  Assets:Cash  1 USD ; a \"comment\"\n  Equity:Opening-Balances"
    );
    assert_eq!(entry_text(file.path(), 6).unwrap(), "2024-01-02 * \"Next\"");
}

#[test]
fn test_entry_text_reports_a_missing_line_or_file() {
    assert!(matches!(
        entry_text(FIXTURE, 0),
        Err(EditError::NoSuchLine { line: 0, .. })
    ));
    assert!(matches!(
        entry_text(FIXTURE, 52),
        Err(EditError::NoSuchLine { line: 52, .. })
    ));
    assert!(matches!(
        entry_text("/nonexistent/ledger.bean", 1),
        Err(EditError::Io { .. })
    ));
}

#[test]
fn test_replace_entry_text_rewrites_only_the_entry() {
    let file = TempFile::copy_of_fixture("replace");
    let before: Vec<String> = file.text().lines().map(str::to_string).collect();
    let replacement = "2024-02-10 * \"Bistro\" \"Dinner with friends\"\n  Expenses:Food:Restaurants  60.00 USD\n  Assets:Cash               -45.50 USD\n  Liabilities:CreditCard    -14.50 USD";

    replace_entry_text(file.path(), 29, BISTRO, replacement).unwrap();

    let after: Vec<String> = file.text().lines().map(str::to_string).collect();
    assert_eq!(after.len(), before.len() + 1);
    assert_eq!(after[..28], before[..28]);
    assert_eq!(after[28..32].join("\n"), replacement);
    assert_eq!(after[32..], before[31..]);
    assert!(file.text().ends_with('\n'));
    assert_eq!(entry_text(file.path(), 29).unwrap(), replacement);
    // The new text parses, and the ledger has the edited transaction.
    let (ledger, errors) = Ledger::from_file(file.path());
    assert!(errors.is_empty(), "{errors:?}");
    assert!(
        ledger
            .txns()
            .iter()
            .any(|txn| txn.narration() == "Dinner with friends" && txn.postings().len() == 3)
    );
}

#[test]
fn test_replace_entry_text_refuses_an_entry_that_changed() {
    let file = TempFile::copy_of_fixture("changed");
    let before = file.text();
    let stale = "2024-02-10 * \"Bistro\" \"Dinner\"\n  Expenses:Food:Restaurants  45.50 USD";
    assert!(matches!(
        replace_entry_text(file.path(), 29, stale, "2024-02-10 * \"Bistro\""),
        Err(EditError::Changed { line: 29, .. })
    ));
    assert_eq!(file.text(), before);
}

#[test]
fn test_replace_entry_text_keeps_the_line_breaks_and_the_end_of_the_file() {
    let file = TempFile::new(
        "crlf",
        "2024-01-01 open Assets:Cash\r\n\r\n2024-01-02 * \"One\"\r\n  Assets:Cash  1 USD\r\n  Equity:Opening-Balances\r\n\r\n2024-01-03 * \"Two\"\r\n  Assets:Cash  2 USD\r\n  Equity:Opening-Balances",
    );
    let one = "2024-01-02 * \"One\"\n  Assets:Cash  1 USD\n  Equity:Opening-Balances";
    assert_eq!(entry_text(file.path(), 3).unwrap(), one);

    replace_entry_text(
        file.path(),
        3,
        one,
        "2024-01-02 * \"One, edited\"\n  Assets:Cash  1 USD\n  Equity:Opening-Balances",
    )
    .unwrap();
    assert_eq!(
        file.text(),
        "2024-01-01 open Assets:Cash\r\n\r\n2024-01-02 * \"One, edited\"\r\n  Assets:Cash  1 USD\r\n  Equity:Opening-Balances\r\n\r\n2024-01-03 * \"Two\"\r\n  Assets:Cash  2 USD\r\n  Equity:Opening-Balances"
    );

    // The last entry can be replaced too, and the file still has no final
    // line break.
    let two = "2024-01-03 * \"Two\"\n  Assets:Cash  2 USD\n  Equity:Opening-Balances";
    replace_entry_text(
        file.path(),
        7,
        two,
        "2024-01-03 * \"Two, edited\"\n  Assets:Cash  2 USD\n  Equity:Opening-Balances",
    )
    .unwrap();
    assert!(file.text().ends_with(
        "2024-01-03 * \"Two, edited\"\r\n  Assets:Cash  2 USD\r\n  Equity:Opening-Balances"
    ));
}
