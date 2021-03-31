use logos::Logos;

#[derive(Debug, PartialEq, Logos, Clone, Copy)]
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

    #[regex(r";[^\n]*")]
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

    #[regex(r#"[^a-z,#\^":;{}\s\d\-\+\.][^,#\^":;{}\s]*"#)]
    Currency,

    #[regex(r"[\-\+]?\d+(\.\d*)?")]
    #[regex(r"[\-\+]?\.\d+")]
    Number,

    #[error]
    Error,
}
