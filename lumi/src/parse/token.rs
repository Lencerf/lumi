use logos::Logos;

/// Tokens in lumi accounting language.
#[derive(Debug, Clone, Copy, PartialEq, Hash, Logos)]
pub enum Token {
    #[regex(r"[ \f\r\t\v]+")]
    WhiteSpace,

    #[token("include")]
    Include,

    #[token("commodity")]
    Commodity,

    #[token("option")]
    Option,

    #[token("event")]
    Event,

    #[token("note")]
    Note,

    #[token("document")]
    Document,

    #[token("open")]
    Open,

    #[token("close")]
    Close,

    #[token("pushtag")]
    PushTag,

    #[token("poptag")]
    PopTag,

    #[token("balance")]
    Balance,

    #[token("pad")]
    Pad,

    #[token("txn")]
    Txn,

    #[token("*")]
    Asterisk,

    #[token("?")]
    QuestionMark,

    #[token("!")]
    ExclamationMark,

    #[token("@")]
    AtUnit,

    #[token("@@")]
    AtTotal,

    #[token("{")]
    LBrace,

    #[token("{{")]
    LLBrace,

    #[token("}")]
    RBrace,

    #[token("}}")]
    RRBrace,

    // A comment deliberately extends to the end of the line, which logos
    // (>= 0.16) otherwise flags as an unbounded greedy repetition.
    #[regex(r";[^\n]*", allow_greedy = true)]
    Comment,

    #[token(",")]
    Comma,

    #[token("\n")]
    NewLine,

    #[regex(r#""[^"]*""#)]
    String,

    #[regex(r"#\S+")]
    Tag,

    #[regex(r"\^\S+")]
    Link,

    #[regex(r"\d\d\d\d-\d\d-\d\d")]
    Date,

    #[regex(r#"[^a-z,#\^":;{}\s\d\-\+\.][^,#\^":;{}\s]*(:[^,#\^":;{}\s]+)+"#)]
    Account,

    #[regex(r#"[^A-Z,#\^":;{}\s\d\-\+\.][^,#\^":;{}\s]*:"#)]
    MetaLabel,

    // This catch-all pattern can also match a single `*`, `?`, `!`, or `@`.
    // Lower its priority so that the dedicated tokens above win those ties
    // (logos still prefers the longest match, so e.g. `@@@` is a `Currency`).
    #[regex(r#"[^a-z,#\^":;{}\s\d\-\+\.][^,#\^":;{}\s]*"#, priority = 1)]
    Currency,

    #[regex(r"[\-\+]?\d+(\.\d*)?")]
    #[regex(r"[\-\+]?\.\d+")]
    Number,

    /// Produced by [`Lexer`](super::Lexer) for input that does not match any
    /// of the patterns above. This variant has no pattern of its own: logos
    /// reports such input as `Err(())`, which the lexer maps to this variant.
    Error,
}

/// Yields every token in `src` — including whitespace, comments, newlines, and
/// unrecognized slices mapped to [`Token::Error`] — together with its byte
/// range in `src`.
pub fn tokenize(src: &str) -> impl Iterator<Item = (Token, std::ops::Range<usize>)> + '_ {
    Token::lexer(src)
        .spanned()
        .map(|(result, span)| (result.unwrap_or(Token::Error), span))
}
