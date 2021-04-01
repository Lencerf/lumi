use super::Token;
use crate::{Error, ErrorLevel, ErrorType, Location, Source, SrcFile};
use logos::{Lexer as LogosLexer, Logos};

pub struct Lexer<'source, Token: Logos<'source>> {
    llex: LogosLexer<'source, Token>,
    location: Location,
    last_token_end: Location,
    peeked_token: Option<(Token, &'source str)>,
    file: SrcFile,
}

impl<'source> Lexer<'source, Token> {
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

    pub fn last_token_end(&self) -> Location {
        self.last_token_end
    }

    pub fn location(&self) -> Location {
        self.location
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

    #[inline]
    pub fn consume(&mut self) {
        let (_, text) = self.peeked_token.take().unwrap();
        let count = text.chars().count();
        self.location.col += count;
        self.last_token_end = self.location;
        self.skip_comment_space();
    }

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
