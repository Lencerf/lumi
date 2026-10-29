use gpui_kit::component::input::Rope;
use lsp_types::CompletionTextEdit;
use lumi::web::Completions;

use crate::completion::{
    BeancountCompletionProvider, CompletionTarget, detect_target, highlighter,
};

fn sample_completions() -> Completions {
    Completions {
        accounts: vec![
            "Assets:Bank:Checking".to_string(),
            "Assets:Cash".to_string(),
            "Expenses:Food:Groceries".to_string(),
            "Expenses:Food:Restaurants".to_string(),
            "Liabilities:CreditCard".to_string(),
        ],
        currencies: vec!["USD".to_string(), "EUR".to_string(), "VTI".to_string()],
        payees: vec![
            "Airline".to_string(),
            "Grocer".to_string(),
            "Bistro".to_string(),
        ],
        narrations: vec![
            "Pay card".to_string(),
            "Flight to Paris".to_string(),
            "Dinner".to_string(),
            "Weekly groceries".to_string(),
        ],
        payee_narrations: [
            (
                "Grocer".to_string(),
                vec!["Weekly groceries".to_string(), "Party snacks".to_string()],
            ),
            ("Airline".to_string(), vec!["Flight to Paris".to_string()]),
        ]
        .into_iter()
        .collect(),
    }
}

/// Splits `marked` at the first `|` into `(clean_text, cursor_byte_offset)`.
fn cursor_case(marked: &str) -> (Rope, usize) {
    let pos = marked.find('|').expect("missing cursor marker |");
    let mut clean = String::with_capacity(marked.len() - 1);
    clean.push_str(&marked[..pos]);
    clean.push_str(&marked[pos + 1..]);
    (Rope::from(clean.as_str()), pos)
}

#[test]
fn test_indented_posting_account_detection_and_ranking() {
    let provider = BeancountCompletionProvider::new();
    provider.set_completions(sample_completions());

    // Blank indentation does not auto-open while typing, unless explicitly invoked.
    let (rope, offset) = cursor_case("2025-06-01 * \"Grocer\"\n  |");
    assert_eq!(detect_target(&rope, offset, false), None);
    assert_eq!(
        detect_target(&rope, offset, true),
        Some(CompletionTarget::Account {
            replace: 24..24,
            query: String::new(),
        })
    );

    // Colon-segment abbreviation matches Assets:Bank:Checking cleanly.
    let (rope, offset) = cursor_case("2025-06-01 * \"Grocer\"\n  a:b:c|  -40.00 USD");
    let items = provider.complete_at(&rope, offset, false);
    let labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();
    assert_eq!(labels, ["Assets:Bank:Checking"]);

    // Editing mid-token replaces the whole account token without touching the amount.
    let (rope, offset) =
        cursor_case("2025-06-01 * \"Grocer\"\n  Expenses:Food:Gro|ceries  80.00 USD");
    let items = provider.complete_at(&rope, offset, false);
    assert_eq!(items[0].label, "Expenses:Food:Groceries");
    let Some(CompletionTextEdit::Edit(edit)) = &items[0].text_edit else {
        panic!("expected explicit text edit");
    };
    assert_eq!(edit.range.start.line, 1);
    assert_eq!(edit.range.start.character, 2);
    assert_eq!(edit.range.end.character, 25);

    // Segment prefix ranks Checking ahead of substring-only matches.
    let (rope, offset) = cursor_case("  check|");
    let items = provider.complete_at(&rope, offset, false);
    assert_eq!(items[0].label, "Assets:Bank:Checking");
}

#[test]
fn test_currency_detection_in_postings_costs_prices_and_directives() {
    let provider = BeancountCompletionProvider::new();
    provider.set_completions(sample_completions());

    for case in [
        "  Assets:Cash  100.00 |",
        "  Assets:Cash  -1,000.50 |",
        "  Assets:Cash  10 VTI {250.00 |}",
        "  Assets:Cash  100.00 EUR @ 1.10 |",
        "  Assets:Cash  100.00 EUR @@ 110 |",
        "2024-03-15 balance Assets:Cash 54.50 |",
        "2024-01-01 commodity |",
        "commodity |",
    ] {
        let (rope, offset) = cursor_case(case);
        let items = provider.complete_at(&rope, offset, false);
        let labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();
        assert_eq!(labels, ["USD", "EUR", "VTI"], "case: {case}");
    }

    // Comma-separated currency list on `open` replaces only the active segment.
    let (rope, offset) = cursor_case("2024-01-01 open Assets:Cash USD,E|UR");
    let items = provider.complete_at(&rope, offset, false);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].label, "EUR");
    let Some(CompletionTextEdit::Edit(edit)) = &items[0].text_edit else {
        panic!("expected explicit text edit");
    };
    assert_eq!(edit.range.start.character, 32);
    assert_eq!(edit.range.end.character, 35);

    // Dates inside cost specs do not trigger currency completion.
    let (rope, offset) = cursor_case("  Assets:Cash  10 VTI {250.00 USD, 2024-03-01 |}");
    assert!(provider.complete_at(&rope, offset, false).is_empty());

    // Comments never trigger completions.
    let (rope, offset) = cursor_case("  Assets:Cash  100.00 ; 50 |");
    assert!(provider.complete_at(&rope, offset, false).is_empty());
}

#[test]
fn test_directive_account_positions() {
    let provider = BeancountCompletionProvider::new();
    provider.set_completions(sample_completions());

    for case in [
        "2024-01-01 open Cash|",
        "2024-01-01 close Cash|",
        "2024-01-01 balance Cash|",
        "2024-01-01 pad Assets:Bank:Checking Cash|",
        "2024-01-01 note Cash|",
        "2024-01-01 document Cash|",
    ] {
        let (rope, offset) = cursor_case(case);
        let items = provider.complete_at(&rope, offset, false);
        assert_eq!(items.len(), 1, "case: {case}");
        assert_eq!(items[0].label, "Assets:Cash", "case: {case}");
    }
}

#[test]
fn test_completion_highlighter_uses_full_token_query() {
    let provider = BeancountCompletionProvider::new();
    provider.set_completions(sample_completions());
    let hl = highlighter();

    let (rope, offset) = cursor_case("  a:b:c|");
    let items = provider.complete_at(&rope, offset, false);
    // Even if the menu's incremental query only saw ":c", filter_text carries "a:b:c".
    let ranges = hl(":c", &items[0]);
    assert_eq!(ranges, vec![0..1, 6..8, 11..13]);
}

#[test]
fn test_single_header_string_completes_payees_and_narrations_and_closes_quote() {
    let provider = BeancountCompletionProvider::new();
    provider.set_completions(sample_completions());

    // Unclosed 1st string matches both payees (Tier 0) and narrations (Tier 1 word-prefix).
    let (rope, offset) = cursor_case("2025-06-01 * \"Gro|");
    let items = provider.complete_at(&rope, offset, false);
    let summary: Vec<(&str, Option<&str>)> = items
        .iter()
        .map(|item| (item.label.as_str(), item.detail.as_deref()))
        .collect();
    assert_eq!(
        summary,
        [
            ("Grocer", Some("payee")),
            ("Weekly groceries", Some("narration")),
        ]
    );
    let Some(CompletionTextEdit::Edit(edit)) = &items[0].text_edit else {
        panic!("expected explicit text edit");
    };
    assert_eq!(edit.new_text, "Grocer\"");
    assert_eq!(edit.range.start.character, 14);
    assert_eq!(edit.range.end.character, 17);

    // Closed 1st string includes the existing closing quote in its edit range so it is not doubled.
    let (rope, offset) = cursor_case("2025-06-01 * \"Par|is\"");
    let items = provider.complete_at(&rope, offset, false);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].label, "Flight to Paris");
    let Some(CompletionTextEdit::Edit(edit)) = &items[0].text_edit else {
        panic!("expected explicit text edit");
    };
    assert_eq!(edit.new_text, "Flight to Paris\"");
    assert_eq!(edit.range.start.character, 14);
    assert_eq!(edit.range.end.character, 20);
}

#[test]
fn test_two_header_strings_scope_first_to_payees_and_second_to_payee_narrations() {
    let provider = BeancountCompletionProvider::new();
    provider.set_completions(sample_completions());

    // Closed 1st string when a 2nd string exists offers payees only.
    let (rope, offset) = cursor_case("2025-06-01 * \"Gro|cer\" \"Weekly groceries\"");
    let items = provider.complete_at(&rope, offset, false);
    let labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();
    assert_eq!(labels, ["Grocer"]);
    assert_eq!(items[0].detail.as_deref(), Some("payee"));

    // Unclosed 1st string inserted before an existing narration replaces only up to cursor.
    let (rope, offset) = cursor_case("2025-06-01 * \"Gro| \"Weekly groceries\"");
    let items = provider.complete_at(&rope, offset, false);
    let labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();
    assert_eq!(labels, ["Grocer"]);
    let Some(CompletionTextEdit::Edit(edit)) = &items[0].text_edit else {
        panic!("expected explicit text edit");
    };
    assert_eq!(edit.range.start.character, 14);
    assert_eq!(edit.range.end.character, 17);
    assert_eq!(edit.new_text, "Grocer\"");

    // Opening the 2nd string immediately ranks the chosen payee's narrations first.
    let (rope, offset) = cursor_case("2025-06-01 txn \"Grocer\" \"|");
    let items = provider.complete_at(&rope, offset, false);
    let labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();
    assert_eq!(
        labels,
        [
            "Weekly groceries",
            "Party snacks",
            "Pay card",
            "Flight to Paris",
            "Dinner",
        ]
    );
    assert!(
        items
            .iter()
            .all(|item| item.detail.as_deref() == Some("narration"))
    );

    // Non-transaction directives with quoted strings do not suggest payees or narrations.
    for non_txn in [
        "option \"title\" \"Gro|",
        "2024-01-01 event \"location\" \"Par|",
        "2024-01-01 note Assets:Cash \"Gro|",
        "  Assets:Cash \"Gro|",
    ] {
        let (rope, offset) = cursor_case(non_txn);
        assert!(
            provider.complete_at(&rope, offset, false).is_empty(),
            "unexpected completion for: {non_txn}"
        );
    }
}
