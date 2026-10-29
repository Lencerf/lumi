use std::collections::HashMap;
use std::ops::Range;

use gpui_kit::component::input::{HighlightStyleResolver, Rope};
use gpui_kit::{HighlightStyle, hsla};

use crate::highlight::{BeancountHighlighter, LANGUAGE, highlighter_factory};

struct TestResolver {
    styles: HashMap<&'static str, HighlightStyle>,
}

impl TestResolver {
    fn new() -> Self {
        let names = [
            "constant",
            "keyword",
            "type",
            "string",
            "number",
            "attribute",
            "tag",
            "link_uri",
            "property",
            "comment",
        ];
        let styles = names
            .into_iter()
            .enumerate()
            .map(|(ix, name)| {
                let hue = (ix as f32 + 1.0) / 16.0;
                (
                    name,
                    HighlightStyle {
                        color: Some(hsla(hue, 0.7, 0.5, 1.0)),
                        ..HighlightStyle::default()
                    },
                )
            })
            .collect();
        Self { styles }
    }
}

impl HighlightStyleResolver for TestResolver {
    fn style(&self, name: &str) -> Option<HighlightStyle> {
        self.styles.get(name).copied()
    }
}

fn assert_contiguous_coverage(range: &Range<usize>, runs: &[(Range<usize>, HighlightStyle)]) {
    if range.is_empty() {
        assert_eq!(runs, &[(range.clone(), HighlightStyle::default())]);
        return;
    }
    assert!(!runs.is_empty());
    assert_eq!(runs.first().unwrap().0.start, range.start);
    assert_eq!(runs.last().unwrap().0.end, range.end);
    for (ix, (run, _)) in runs.iter().enumerate() {
        assert!(run.start < run.end, "empty run at {ix}: {run:?}");
        if let Some((next, _)) = runs.get(ix + 1) {
            assert_eq!(
                run.end, next.start,
                "gap or overlap between {run:?} and {next:?}"
            );
        }
    }
}

#[test]
fn test_transaction_tokens_receive_distinct_semantic_styles() {
    let src = "\
2025-06-01 * \"Safeway\" \"Groceries\" #food ^inv-1 ; receipt
  note: \"weekly\"
  Expenses:Food:Groceries  42.50 USD @ 0.92 EUR
  Assets:Bank:Checking";
    let rope = Rope::from(src);
    let spans = BeancountHighlighter::lex_rope(&rope);
    let labeled: Vec<(&str, &str)> = spans
        .iter()
        .map(|(span, kind)| (&src[span.clone()], *kind))
        .collect();
    assert_eq!(
        labeled,
        [
            ("2025-06-01", "constant"),
            ("*", "keyword"),
            ("\"Safeway\"", "string"),
            ("\"Groceries\"", "string"),
            ("#food", "tag"),
            ("^inv-1", "link_uri"),
            ("; receipt", "comment"),
            ("note:", "property"),
            ("\"weekly\"", "string"),
            ("Expenses:Food:Groceries", "type"),
            ("42.50", "number"),
            ("USD", "attribute"),
            ("@", "keyword"),
            ("0.92", "number"),
            ("EUR", "attribute"),
            ("Assets:Bank:Checking", "type"),
        ]
    );
}

#[test]
fn test_unclosed_quote_stays_on_its_own_line() {
    let src = "2025-06-01 * \"Payee in progress\n  Assets:Cash  -10.00 USD";
    let rope = Rope::from(src);
    let spans = BeancountHighlighter::lex_rope(&rope);
    let labeled: Vec<(&str, &str)> = spans
        .iter()
        .map(|(span, kind)| (&src[span.clone()], *kind))
        .collect();
    assert_eq!(
        labeled,
        [
            ("2025-06-01", "constant"),
            ("*", "keyword"),
            ("\"Payee in progress", "string"),
            ("Assets:Cash", "type"),
            ("-10.00", "number"),
            ("USD", "attribute"),
        ]
    );
}

#[test]
fn test_resolve_styles_covers_every_byte_and_clips_subranges() {
    let src = "2025-06-01 balance Assets:Cash 54.50 USD\n";
    let rope = Rope::from(src);
    let spans = BeancountHighlighter::lex_rope(&rope);
    let resolver = TestResolver::new();

    let full = 0..src.len();
    let runs = BeancountHighlighter::resolve_styles(&spans, &full, &resolver);
    assert_contiguous_coverage(&full, &runs);

    // Subrange starting and ending mid-token still covers the exact slice.
    let sub = 5..35;
    let sub_runs = BeancountHighlighter::resolve_styles(&spans, &sub, &resolver);
    assert_contiguous_coverage(&sub, &sub_runs);

    let empty = 0..0;
    let empty_runs = BeancountHighlighter::resolve_styles(&spans, &empty, &resolver);
    assert_contiguous_coverage(&empty, &empty_runs);
}

#[test]
fn test_factory_matches_beancount_only() {
    let factory = highlighter_factory();
    assert_eq!(
        factory(LANGUAGE).map(|highlighter| highlighter.language()),
        Some(LANGUAGE.into())
    );
    assert!(factory("BEANCOUNT").is_some());
    assert!(factory("rust").is_none());
}
