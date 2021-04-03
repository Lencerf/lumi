use super::Token;
use crate::{Error, ErrorLevel, ErrorType, Location, Source, SrcFile};
use getset::{CopyGetters, Getters};
use logos::{Lexer as LogosLexer, Logos};

/// A lexer based on [`logos::Lexer`](https://docs.rs/logos/0.12.0/logos/struct.Lexer.html)
/// that can peek tokens and track locations.
#[derive(Getters, CopyGetters)]
pub struct Lexer<'source, Token: Logos<'source>> {
    llex: LogosLexer<'source, Token>,

    /// Returns the current location of the lexer. Usually it is the starting
    /// location of the next token.
    #[getset(get_copy = "pub")]
    location: Location,

    /// Returns the ending location of last token consumed.
    #[getset(get_copy = "pub")]
    last_token_end: Location,

    peeked_token: Option<(Token, &'source str)>,

    /// Returns the source file path.
    #[getset(get = "pub")]
    file: SrcFile,
}

impl<'source> Lexer<'source, Token> {
    /// Creates a new [`Lexer`] from the contents (`src`) of the source and the
    /// path (`file`) to the file .
    pub fn new(src: &'source str, file: SrcFile) -> Self {
        let mut lexer = Lexer {
            llex: Token::lexer(src),
            location: (1, 1).into(),
            last_token_end: (1, 1).into(),
            peeked_token: None,
            file,
        };
        lexer.skip_comment_space();
        lexer
    }

    fn skip_comment_space(&mut self) {
        while let Some(token) = self.llex.next() {
            match token {
                Token::Comment => {}
                Token::NewLine => {
                    self.location.col = 1;
                    self.location.line += 1;
                }
                Token::WhiteSpace => self.location.col += self.llex.slice().len(),
                _ => {
                    self.peeked_token = Some((token, self.llex.slice()));
                    return;
                }
            }
        }
    }

    /// Returns the next token type and text without advancing the lexer. If it
    /// is already at the end of the source, [`None`] is returned.
    pub fn peek(&mut self) -> Result<(Token, &'source str), Error> {
        let error = Error {
            msg: "Unexpected end of file.".to_string(),
            src: Source {
                file: self.file.clone(),
                start: self.location,
                end: self.location,
            },
            r#type: ErrorType::Syntax,
            level: ErrorLevel::Error,
        };
        self.peeked_token.ok_or(error)
    }

    /// Consumes the peeked token and advances the lexer. Must be used after
    /// calling [`peek`](Lexer::peek).

    /// # Panics
    ///
    /// Panics if [`peek`](Lexer::peek) is not called before.
    #[inline]
    pub fn consume(&mut self) {
        let (_, text) = self.peeked_token.take().unwrap();
        let count = text.chars().count();
        self.location.col += count;
        self.last_token_end = self.location;
        self.skip_comment_space();
    }

    /// Returns the token type and text, and advances the lexer. Equivalent to
    /// [`peek`](Lexer::peek) + [`consume`](Lexer::consume). Returns
    /// [`None`] if the lexer is at the end of the source.
    pub fn take(&mut self, expected: Token) -> Result<&'source str, Error> {
        let (token, text) = self.peek()?;
        if token != expected {
            Err(Error {
                msg: format!("Expect {:?}, found {:?}({:?})", expected, &token, text),
                src: Source {
                    file: self.file.clone(),
                    start: self.location,
                    end: self.location.advance(text.chars().count()),
                },
                r#type: ErrorType::Syntax,
                level: ErrorLevel::Error,
            })
        } else {
            self.consume();
            Ok(text)
        }
    }
}
