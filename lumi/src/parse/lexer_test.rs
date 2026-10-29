use crate::parse::lexer::Lexer;
use crate::parse::token::{Token, tokenize};
use crate::{ErrorType, SrcFile};

/// A token, its text, the `(line, col)` at which it starts, and the location
/// right after it.
type LocatedToken<'a> = (Token, &'a str, (usize, usize), (usize, usize));

fn new_lexer(src: &str) -> Lexer<'_, Token> {
    Lexer::new(src, SrcFile::new("test.bean".to_string()))
}

/// Lexes `src` and returns every token the parser would see, together with
/// its text.
fn lex(src: &str) -> Vec<(Token, &str)> {
    let mut lexer = new_lexer(src);
    let mut tokens = Vec::new();
    while let Ok(token) = lexer.peek() {
        tokens.push(token);
        lexer.consume();
    }
    tokens
}

/// Like [`lex`], but also returns where each token starts and ends.
fn lex_with_locations(src: &str) -> Vec<LocatedToken<'_>> {
    let mut lexer = new_lexer(src);
    let mut tokens = Vec::new();
    while let Ok((token, text)) = lexer.peek() {
        let start = lexer.location();
        lexer.consume();
        let end = lexer.last_token_end();
        tokens.push((token, text, (start.line, start.col), (end.line, end.col)));
    }
    tokens
}

#[test]
fn test_keywords() {
    let src = "include commodity option event note document open close \
               pushtag poptag balance pad txn";
    let tokens: Vec<Token> = lex(src).into_iter().map(|(t, _)| t).collect();
    assert_eq!(
        tokens,
        [
            Token::Include,
            Token::Commodity,
            Token::Option,
            Token::Event,
            Token::Note,
            Token::Document,
            Token::Open,
            Token::Close,
            Token::PushTag,
            Token::PopTag,
            Token::Balance,
            Token::Pad,
            Token::Txn,
        ]
    );
}

#[test]
fn test_punctuation() {
    assert_eq!(
        lex("* ! ? @ @@ { {{ } }} ,"),
        [
            (Token::Asterisk, "*"),
            (Token::ExclamationMark, "!"),
            (Token::QuestionMark, "?"),
            (Token::AtUnit, "@"),
            (Token::AtTotal, "@@"),
            (Token::LBrace, "{"),
            (Token::LLBrace, "{{"),
            (Token::RBrace, "}"),
            (Token::RRBrace, "}}"),
            (Token::Comma, ","),
        ]
    );
}

/// `*`, `!`, `?` and `@` are also valid single-character currencies, so the
/// dedicated tokens must win the tie; longer matches are currencies.
#[test]
fn test_single_char_tokens_beat_currency() {
    assert_eq!(lex("*A"), [(Token::Currency, "*A")]);
    assert_eq!(lex("@@@"), [(Token::Currency, "@@@")]);
    assert_eq!(lex("@USD"), [(Token::Currency, "@USD")]);
    assert_eq!(
        lex("2024-01-02 * \"n\""),
        [
            (Token::Date, "2024-01-02"),
            (Token::Asterisk, "*"),
            (Token::String, "\"n\""),
        ]
    );
    assert_eq!(
        lex("1 USD @ 2 EUR"),
        [
            (Token::Number, "1"),
            (Token::Currency, "USD"),
            (Token::AtUnit, "@"),
            (Token::Number, "2"),
            (Token::Currency, "EUR"),
        ]
    );
}

#[test]
fn test_accounts_currencies_and_meta_labels() {
    assert_eq!(
        lex("Assets:Cash Assets:Bank:Checking Equity:Opening-Balances Assets:401k"),
        [
            (Token::Account, "Assets:Cash"),
            (Token::Account, "Assets:Bank:Checking"),
            (Token::Account, "Equity:Opening-Balances"),
            (Token::Account, "Assets:401k"),
        ]
    );
    assert_eq!(
        lex("USD VTSAX AAPL-2 U.S. X"),
        [
            (Token::Currency, "USD"),
            (Token::Currency, "VTSAX"),
            (Token::Currency, "AAPL-2"),
            (Token::Currency, "U.S."),
            (Token::Currency, "X"),
        ]
    );
    assert_eq!(
        lex("name: foo-bar: a1: open: txn:"),
        [
            (Token::MetaLabel, "name:"),
            (Token::MetaLabel, "foo-bar:"),
            (Token::MetaLabel, "a1:"),
            (Token::MetaLabel, "open:"),
            (Token::MetaLabel, "txn:"),
        ]
    );
    // A currency followed by a colon is not an account.
    assert_eq!(
        lex("Assets:"),
        [(Token::Currency, "Assets"), (Token::Error, ":")]
    );
}

#[test]
fn test_unicode_identifiers() {
    assert_eq!(
        lex("Assets:现金 Expenses:食品:午餐 Äquivalent:Konto"),
        [
            (Token::Account, "Assets:现金"),
            (Token::Account, "Expenses:食品:午餐"),
            (Token::Account, "Äquivalent:Konto"),
        ]
    );
    assert_eq!(
        lex("€ 元"),
        [(Token::Currency, "€"), (Token::Currency, "元")]
    );
}

#[test]
fn test_numbers_and_dates() {
    assert_eq!(
        lex("1 1.5 .5 -1 +1 -1.5 +.5 1. -0 0.00000001"),
        [
            (Token::Number, "1"),
            (Token::Number, "1.5"),
            (Token::Number, ".5"),
            (Token::Number, "-1"),
            (Token::Number, "+1"),
            (Token::Number, "-1.5"),
            (Token::Number, "+.5"),
            (Token::Number, "1."),
            (Token::Number, "-0"),
            (Token::Number, "0.00000001"),
        ]
    );
    assert_eq!(
        lex("1,000"),
        [
            (Token::Number, "1"),
            (Token::Comma, ","),
            (Token::Number, "000"),
        ]
    );
    assert_eq!(lex("2024-01-02"), [(Token::Date, "2024-01-02")]);
    // Not a date: lexed as a number followed by signed numbers.
    assert_eq!(
        lex("2024-1-2"),
        [
            (Token::Number, "2024"),
            (Token::Number, "-1"),
            (Token::Number, "-2"),
        ]
    );
    assert_eq!(
        lex("2024-01-02x"),
        [(Token::Date, "2024-01-02"), (Token::Error, "x")]
    );
}

#[test]
fn test_strings_tags_and_links() {
    assert_eq!(
        lex(r#""hello" "" "with ; semicolon" "with # hash""#),
        [
            (Token::String, r#""hello""#),
            (Token::String, r#""""#),
            (Token::String, r#""with ; semicolon""#),
            (Token::String, r#""with # hash""#),
        ]
    );
    assert_eq!(lex("\"multi\nline\""), [(Token::String, "\"multi\nline\"")]);
    assert_eq!(lex("\"unterminated"), [(Token::Error, "\"unterminated")]);
    assert_eq!(
        lex("#tag #tag-1 ^link ^link-1"),
        [
            (Token::Tag, "#tag"),
            (Token::Tag, "#tag-1"),
            (Token::Link, "^link"),
            (Token::Link, "^link-1"),
        ]
    );
    assert_eq!(lex("# ^"), [(Token::Error, "#"), (Token::Error, "^")]);
}

#[test]
fn test_comments_whitespace_and_locations() {
    let src = "; a comment\n\
               2024-01-02 * \"Payee\"\t\"Narration\" ; trailing\n\
               \r\n  \
               Assets:Cash  -10.00 USD\r\n";
    assert_eq!(
        lex_with_locations(src),
        [
            (Token::Date, "2024-01-02", (2, 1), (2, 11)),
            (Token::Asterisk, "*", (2, 12), (2, 13)),
            (Token::String, "\"Payee\"", (2, 14), (2, 21)),
            (Token::String, "\"Narration\"", (2, 22), (2, 33)),
            (Token::Account, "Assets:Cash", (4, 3), (4, 14)),
            (Token::Number, "-10.00", (4, 16), (4, 22)),
            (Token::Currency, "USD", (4, 23), (4, 26)),
        ]
    );
    // Columns count characters, not bytes.
    assert_eq!(
        lex_with_locations("Assets:现金 1 元"),
        [
            (Token::Account, "Assets:现金", (1, 1), (1, 10)),
            (Token::Number, "1", (1, 11), (1, 12)),
            (Token::Currency, "元", (1, 13), (1, 14)),
        ]
    );
    assert!(lex("").is_empty());
    assert!(lex("  \n\t\n ; only a comment\n").is_empty());
}

#[test]
fn test_transaction() {
    let src = "2024-01-02 * \"Payee\" \"Narration\" #tag ^link\n  \
               meta: \"v\"\n  \
               Assets:Cash  -10.00 USD @ 1.2 EUR\n  \
               Expenses:Food  10 USD {5 EUR, 2023-01-01}\n  \
               Equity:X {{ 100 USD }}\n";
    assert_eq!(
        lex(src),
        [
            (Token::Date, "2024-01-02"),
            (Token::Asterisk, "*"),
            (Token::String, "\"Payee\""),
            (Token::String, "\"Narration\""),
            (Token::Tag, "#tag"),
            (Token::Link, "^link"),
            (Token::MetaLabel, "meta:"),
            (Token::String, "\"v\""),
            (Token::Account, "Assets:Cash"),
            (Token::Number, "-10.00"),
            (Token::Currency, "USD"),
            (Token::AtUnit, "@"),
            (Token::Number, "1.2"),
            (Token::Currency, "EUR"),
            (Token::Account, "Expenses:Food"),
            (Token::Number, "10"),
            (Token::Currency, "USD"),
            (Token::LBrace, "{"),
            (Token::Number, "5"),
            (Token::Currency, "EUR"),
            (Token::Comma, ","),
            (Token::Date, "2023-01-01"),
            (Token::RBrace, "}"),
            (Token::Account, "Equity:X"),
            (Token::LLBrace, "{{"),
            (Token::Number, "100"),
            (Token::Currency, "USD"),
            (Token::RRBrace, "}}"),
        ]
    );
}

#[test]
fn test_unrecognized_input_is_an_error_token() {
    assert_eq!(
        lex("plugin \"x\""),
        [(Token::Error, "plugin"), (Token::String, "\"x\"")]
    );
    assert_eq!(
        lex("foo bar"),
        [(Token::Error, "foo"), (Token::Error, "bar")]
    );
    assert_eq!(lex("- +"), [(Token::Error, "-"), (Token::Error, "+")]);
    // The longest valid prefix is matched first; the rest is an error.
    assert_eq!(lex("opened"), [(Token::Open, "open"), (Token::Error, "ed")]);
    assert_eq!(lex("txns"), [(Token::Txn, "txn"), (Token::Error, "s")]);
}

#[test]
fn test_take_and_peek() {
    let mut lexer = new_lexer("open Assets:Cash");
    assert_eq!(lexer.take(Token::Open).unwrap(), "open");
    let err = lexer.take(Token::Currency).unwrap_err();
    assert_eq!(err.r#type, ErrorType::Syntax);
    assert_eq!(err.src.start, (1, 6).into());
    assert_eq!(err.src.end, (1, 17).into());
    assert_eq!(lexer.take(Token::Account).unwrap(), "Assets:Cash");
    let eof = lexer.peek().unwrap_err();
    assert_eq!(eof.msg, "Unexpected end of file.");
    assert_eq!(eof.src.start, (1, 17).into());
}

#[test]
fn test_tokenize_preserves_whitespace_comments_and_byte_spans() {
    let src = "2024-01-02 * \"Coffee\" ; morning\n  Assets:Cash -4.50 USD";
    let got: Vec<(Token, &str)> = tokenize(src)
        .map(|(token, span)| (token, &src[span]))
        .collect();
    assert_eq!(
        got,
        [
            (Token::Date, "2024-01-02"),
            (Token::WhiteSpace, " "),
            (Token::Asterisk, "*"),
            (Token::WhiteSpace, " "),
            (Token::String, "\"Coffee\""),
            (Token::WhiteSpace, " "),
            (Token::Comment, "; morning"),
            (Token::NewLine, "\n"),
            (Token::WhiteSpace, "  "),
            (Token::Account, "Assets:Cash"),
            (Token::WhiteSpace, " "),
            (Token::Number, "-4.50"),
            (Token::WhiteSpace, " "),
            (Token::Currency, "USD"),
        ]
    );
}
