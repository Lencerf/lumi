use std::ops::Range;
use std::rc::Rc;

use gpui_kit::component::input::language_config::LanguageConfig;
use gpui_kit::component::input::{
    BracketPair, EditorState, FoldRange, HighlightStyleResolver, InputEdit, InputHighlighter,
    InputHighlighterFactory, Rope, RopeExt as _, set_language_config,
};
use gpui_kit::{App, Context, HighlightStyle, SharedString, Window};
use lumi::parse::{Token, tokenize};

/// Language identifier registered with GPUI Kit for Beancount entries.
pub(crate) const LANGUAGE: &str = "beancount";

/// Registers editing rules for [`LANGUAGE`] so structural braces match without
/// unexpected quote or parenthesis auto-insertion in free-form narrations.
pub(crate) fn register_language(cx: &mut App) {
    set_language_config(
        LANGUAGE,
        LanguageConfig::default()
            .brackets([BracketPair::new("{", "}")])
            .auto_closing_pairs([]),
        cx,
    );
}

/// Builds an [`InputHighlighterFactory`] that highlights [`LANGUAGE`] buffers
/// with [`BeancountHighlighter`].
pub(crate) fn highlighter_factory() -> InputHighlighterFactory {
    Rc::new(|language| {
        language
            .eq_ignore_ascii_case(LANGUAGE)
            .then(|| Box::new(BeancountHighlighter::default()) as Box<dyn InputHighlighter>)
    })
}

/// Line-oriented Beancount syntax highlighter backed by [`lumi::parse::tokenize`].
#[derive(Default)]
pub(crate) struct BeancountHighlighter {
    spans: Vec<(Range<usize>, &'static str)>,
}

impl BeancountHighlighter {
    pub(crate) fn lex_rope(text: &Rope) -> Vec<(Range<usize>, &'static str)> {
        let mut spans = Vec::new();
        let line_count = text.lines_len();
        for row in 0..line_count {
            let line_offset = text.line_start_offset(row);
            let line = text.slice_line(row).to_string();
            let content = line.strip_suffix('\r').unwrap_or(&line);
            for (token, span) in tokenize(content) {
                if token == Token::Error && content.as_bytes().get(span.start) == Some(&b'"') {
                    // Keep an unterminated quote styled to the end of its own
                    // line without bleeding into postings below it.
                    spans.push((
                        line_offset + span.start..line_offset + content.len(),
                        "string",
                    ));
                    break;
                }
                if let Some(style_name) = token_highlight_name(token) {
                    spans.push((line_offset + span.start..line_offset + span.end, style_name));
                }
            }
        }
        spans
    }

    pub(crate) fn resolve_styles(
        spans: &[(Range<usize>, &'static str)],
        range: &Range<usize>,
        resolver: &dyn HighlightStyleResolver,
    ) -> Vec<(Range<usize>, HighlightStyle)> {
        if range.start >= range.end {
            return vec![(range.start..range.start, HighlightStyle::default())];
        }

        let mut runs = Vec::new();
        let mut cursor = range.start;
        let first = spans.partition_point(|(span, _)| span.end <= range.start);
        for (span, name) in &spans[first..] {
            if span.start >= range.end {
                break;
            }
            let start = span.start.max(cursor);
            let end = span.end.min(range.end);
            if start >= end {
                continue;
            }
            if cursor < start {
                runs.push((cursor..start, HighlightStyle::default()));
            }
            runs.push((start..end, resolver.style(name).unwrap_or_default()));
            cursor = end;
        }
        if cursor < range.end {
            runs.push((cursor..range.end, HighlightStyle::default()));
        }
        runs
    }
}

impl InputHighlighter for BeancountHighlighter {
    fn language(&self) -> SharedString {
        LANGUAGE.into()
    }

    fn update(
        &mut self,
        _edit: Option<InputEdit>,
        text: &Rope,
        _folding: bool,
        _window: &mut Window,
        _cx: &mut Context<EditorState>,
    ) {
        self.spans = Self::lex_rope(text);
    }

    fn update_batch(
        &mut self,
        edits: &[(InputEdit, Rope)],
        _folding: bool,
        _window: &mut Window,
        _cx: &mut Context<EditorState>,
    ) {
        if let Some((_, text)) = edits.last() {
            self.spans = Self::lex_rope(text);
        }
    }

    fn styles(
        &self,
        range: &Range<usize>,
        resolver: &dyn HighlightStyleResolver,
    ) -> Vec<(Range<usize>, HighlightStyle)> {
        Self::resolve_styles(&self.spans, range, resolver)
    }

    fn fold_ranges(&self, _text: &Rope) -> Vec<FoldRange> {
        Vec::new()
    }
}

fn token_highlight_name(token: Token) -> Option<&'static str> {
    match token {
        Token::Date => Some("constant"),
        Token::Include
        | Token::Commodity
        | Token::Option
        | Token::Plugin
        | Token::Custom
        | Token::Event
        | Token::Note
        | Token::Document
        | Token::Open
        | Token::Close
        | Token::PushTag
        | Token::PopTag
        | Token::Balance
        | Token::Pad
        | Token::Txn
        | Token::Asterisk
        | Token::QuestionMark
        | Token::ExclamationMark
        | Token::AtUnit
        | Token::AtTotal => Some("keyword"),
        Token::Account => Some("type"),
        Token::String => Some("string"),
        Token::Number => Some("number"),
        Token::Currency => Some("attribute"),
        Token::Tag => Some("tag"),
        Token::Link => Some("link_uri"),
        Token::MetaLabel => Some("property"),
        Token::Comment => Some("comment"),
        Token::WhiteSpace
        | Token::LBrace
        | Token::LLBrace
        | Token::RBrace
        | Token::RRBrace
        | Token::Comma
        | Token::NewLine
        | Token::Error => None,
    }
}

#[cfg(test)]
#[path = "highlight_test.rs"]
mod tests;
