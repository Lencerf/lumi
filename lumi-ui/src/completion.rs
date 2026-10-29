use std::cell::RefCell;
use std::collections::HashSet;
use std::ops::Range;
use std::rc::Rc;

use anyhow::Result;
use gpui_kit::component::input::{
    CompletionHighlighter, CompletionProvider, Rope, RopeExt as _, highlight_characters,
    highlight_contains,
};
use gpui_kit::{App, Task, Window};
use lsp_types::{
    CompletionContext, CompletionItem, CompletionItemKind, CompletionResponse, CompletionTextEdit,
    CompletionTriggerKind, TextEdit,
};
use lumi::parse::{Token, tokenize};
use lumi::web::Completions;

/// Maximum number of suggestions shown in the completion menu at once.
const MAX_COMPLETION_ITEMS: usize = 50;

/// Provides Beancount auto-completion items for [`crate::EntryEditor`].
#[derive(Default)]
pub(crate) struct BeancountCompletionProvider {
    data: RefCell<Completions>,
}

impl BeancountCompletionProvider {
    pub(crate) fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }

    pub(crate) fn set_completions(&self, completions: Completions) {
        *self.data.borrow_mut() = completions;
    }

    pub(crate) fn complete_at(
        &self,
        text: &Rope,
        offset: usize,
        invoked: bool,
    ) -> Vec<CompletionItem> {
        let Some(target) = detect_target(text, offset, invoked) else {
            return Vec::new();
        };
        let data = self.data.borrow();
        build_items(&data, text, target)
    }
}

impl CompletionProvider for BeancountCompletionProvider {
    fn completions(
        &self,
        text: &Rope,
        offset: usize,
        trigger: CompletionContext,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Task<Result<CompletionResponse>> {
        let invoked = trigger.trigger_kind == CompletionTriggerKind::INVOKED;
        let items = self.complete_at(text, offset, invoked);
        Task::ready(Ok(CompletionResponse::Array(items)))
    }

    fn is_completion_trigger(&self, _offset: usize, new_text: &str, _cx: &mut App) -> bool {
        !new_text.is_empty() && !new_text.contains('\n')
    }
}

/// Highlights matched characters in each completion item using the full token
/// query stored in [`CompletionItem::filter_text`].
pub(crate) fn highlighter() -> Rc<CompletionHighlighter> {
    Rc::new(|menu_query, item| {
        let query = item.filter_text.as_deref().unwrap_or(menu_query);
        let contiguous = highlight_contains(query, item);
        if !contiguous.is_empty() {
            contiguous
        } else {
            highlight_characters(query, item)
        }
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CompletionTarget {
    Account {
        replace: Range<usize>,
        query: String,
        indent: bool,
    },
    Currency {
        replace: Range<usize>,
        query: String,
    },
    Payee {
        replace: Range<usize>,
        query: String,
    },
    PayeeOrNarration {
        replace: Range<usize>,
        query: String,
    },
    Narration {
        payee: Option<String>,
        replace: Range<usize>,
        query: String,
    },
    Tag {
        replace: Range<usize>,
        query: String,
        indent: bool,
    },
    Link {
        replace: Range<usize>,
        query: String,
        indent: bool,
    },
}

pub(crate) fn detect_target(text: &Rope, offset: usize, invoked: bool) -> Option<CompletionTarget> {
    let clamped = offset.min(text.len());
    let point = text.offset_to_point(clamped);
    let line_start = text.line_start_offset(point.row);
    let line_raw = text.slice_line(point.row).to_string();
    let line = line_raw.strip_suffix('\r').unwrap_or(&line_raw);
    let col = snap_char_boundary(line, point.column.min(line.len()));

    let mut in_quotes = false;
    for (_, ch) in line[..col].char_indices() {
        if ch == '"' {
            in_quotes = !in_quotes;
        } else if ch == ';' && !in_quotes {
            return None;
        }
    }
    if in_quotes {
        return detect_header_string_target(line, col, line_start);
    }

    let code = strip_unquoted_comment(line);
    if code.starts_with([' ', '\t']) {
        detect_indented_target(code, col, line_start, invoked)
    } else {
        detect_directive_target(code, col, line_start).or_else(|| {
            let follows_entry_line =
                point.row > 0 && !text.slice_line(point.row - 1).to_string().trim().is_empty();
            follows_entry_line
                .then(|| detect_unindented_posting_target(code, col, line_start, invoked))
                .flatten()
        })
    }
}

fn detect_header_string_target(
    line: &str,
    col: usize,
    line_start: usize,
) -> Option<CompletionTarget> {
    if line.starts_with([' ', '\t']) {
        return None;
    }

    let quotes_before: Vec<usize> = line[..col]
        .char_indices()
        .filter_map(|(idx, ch)| (ch == '"').then_some(idx))
        .collect();
    if quotes_before.len() != 1 && quotes_before.len() != 3 {
        return None;
    }

    let prefix = &line[..quotes_before[0]];
    let spans = whitespace_spans(prefix);
    if spans.len() != 2
        || !is_iso_date(&prefix[spans[0].clone()])
        || !matches!(&prefix[spans[1].clone()], "*" | "!" | "?" | "txn")
    {
        return None;
    }

    let mut quotes_after = Vec::new();
    let mut in_q = true;
    for (rel, ch) in line[col..].char_indices() {
        if ch == '"' {
            in_q = !in_q;
            quotes_after.push(col + rel);
        } else if ch == ';' && !in_q {
            break;
        }
    }

    if quotes_before.len() == 1 {
        let content_start = quotes_before[0] + 1;
        let query = line[content_start..col].to_string();
        let (replace_end, has_second_string) = if quotes_after.len() % 2 == 1 {
            if quotes_after.len() > 3 {
                return None;
            }
            let close_quote = quotes_after[0];
            let has_second =
                quotes_after.len() == 3 && line[close_quote + 1..quotes_after[1]].trim().is_empty();
            (close_quote + 1, has_second)
        } else {
            if quotes_after.len() > 2 {
                return None;
            }
            let has_second =
                quotes_after.len() == 2 && line[col..quotes_after[0]].trim().is_empty();
            (col, has_second)
        };
        let replace = (line_start + content_start)..(line_start + replace_end);
        if has_second_string {
            Some(CompletionTarget::Payee { replace, query })
        } else {
            Some(CompletionTarget::PayeeOrNarration { replace, query })
        }
    } else {
        if !line[quotes_before[1] + 1..quotes_before[2]]
            .trim()
            .is_empty()
        {
            return None;
        }
        let payee = &line[quotes_before[0] + 1..quotes_before[1]];
        let content_start = quotes_before[2] + 1;
        let query = line[content_start..col].to_string();
        let replace_end = if quotes_after.len() % 2 == 1 {
            if quotes_after.len() > 1 {
                return None;
            }
            quotes_after[0] + 1
        } else {
            if !quotes_after.is_empty() {
                return None;
            }
            col
        };
        Some(CompletionTarget::Narration {
            payee: (!payee.is_empty()).then(|| payee.to_string()),
            replace: (line_start + content_start)..(line_start + replace_end),
            query,
        })
    }
}

fn detect_indented_target(
    code: &str,
    col: usize,
    line_start: usize,
    invoked: bool,
) -> Option<CompletionTarget> {
    let indent_end = code.len() - code.trim_start_matches([' ', '\t']).len();
    if col < indent_end {
        return None;
    }

    if code[indent_end..].starts_with(['#', '^']) {
        if all_sigil_tokens(&code[indent_end..col]) {
            return detect_sigil_token(code, col, line_start, false);
        }
        return None;
    }

    let first_end = indent_end
        + code[indent_end..]
            .find(|ch: char| ch.is_whitespace() || matches!(ch, '"' | '{' | '}'))
            .unwrap_or(code.len() - indent_end);

    if col <= first_end {
        let query = &code[indent_end..col];
        if query.is_empty() && !invoked {
            return None;
        }
        return Some(CompletionTarget::Account {
            replace: (line_start + indent_end)..(line_start + first_end),
            query: query.to_string(),
            indent: false,
        });
    }

    detect_currency_after_number(code, col, line_start)
}

fn detect_unindented_posting_target(
    code: &str,
    col: usize,
    line_start: usize,
    invoked: bool,
) -> Option<CompletionTarget> {
    let first_end = code
        .find(|ch: char| ch.is_whitespace() || matches!(ch, '"' | '{' | '}'))
        .unwrap_or(code.len());
    let token = &code[..first_end];

    if token.starts_with(['#', '^']) {
        if all_sigil_tokens(&code[..col]) {
            let indent = !code[..col].contains(char::is_whitespace);
            return detect_sigil_token(code, col, line_start, indent);
        }
        return None;
    }

    if is_iso_date(token)
        || token.starts_with(['*', '!'])
        || matches!(
            token,
            "option" | "include" | "commodity" | "pushtag" | "poptag" | "plugin"
        )
    {
        return None;
    }

    if col <= first_end {
        let query = &code[..col];
        if query.is_empty() && !invoked {
            return None;
        }
        return Some(CompletionTarget::Account {
            replace: line_start..(line_start + first_end),
            query: query.to_string(),
            indent: true,
        });
    }

    if token.contains(':') && token.chars().next().is_some_and(char::is_uppercase) {
        return detect_currency_after_number(code, col, line_start);
    }

    None
}

fn detect_directive_target(code: &str, col: usize, line_start: usize) -> Option<CompletionTarget> {
    let spans = whitespace_spans(code);
    let first = spans.first()?;

    if &code[first.clone()] == "commodity" && col > first.end {
        return Some(currency_arg_target(code, col, line_start, spans.get(1)));
    }
    if matches!(&code[first.clone()], "pushtag" | "poptag") && col > first.end {
        return tag_arg_target(code, col, line_start, spans.get(1));
    }

    if !is_iso_date(&code[first.clone()]) {
        return None;
    }
    let second = spans.get(1)?;
    if col <= second.end {
        return None;
    }

    match &code[second.clone()] {
        "*" | "!" | "?" | "txn" => detect_sigil_token(code, col, line_start, false),
        "commodity" => Some(currency_arg_target(code, col, line_start, spans.get(2))),
        kw @ ("open" | "close" | "balance" | "pad" | "note" | "document") => {
            let Some(third) = spans.get(2) else {
                return Some(CompletionTarget::Account {
                    replace: (line_start + col)..(line_start + col),
                    query: String::new(),
                    indent: false,
                });
            };
            if col <= third.end {
                let start = third.start.min(col);
                return Some(CompletionTarget::Account {
                    replace: (line_start + start)..(line_start + third.end),
                    query: code[start..col].to_string(),
                    indent: false,
                });
            }
            match kw {
                "pad" => {
                    let Some(fourth) = spans.get(3) else {
                        return Some(CompletionTarget::Account {
                            replace: (line_start + col)..(line_start + col),
                            query: String::new(),
                            indent: false,
                        });
                    };
                    if col <= fourth.end {
                        let start = fourth.start.min(col);
                        return Some(CompletionTarget::Account {
                            replace: (line_start + start)..(line_start + fourth.end),
                            query: code[start..col].to_string(),
                            indent: false,
                        });
                    }
                    None
                }
                "open" => {
                    let Some(fourth) = spans.get(3) else {
                        return Some(CompletionTarget::Currency {
                            replace: (line_start + col)..(line_start + col),
                            query: String::new(),
                        });
                    };
                    if col <= fourth.end {
                        return Some(comma_currency_target(code, col, line_start, fourth));
                    }
                    None
                }
                "balance" => detect_currency_after_number(code, col, line_start),
                _ => None,
            }
        }
        _ => None,
    }
}

fn tag_arg_target(
    code: &str,
    col: usize,
    line_start: usize,
    arg: Option<&Range<usize>>,
) -> Option<CompletionTarget> {
    match arg {
        None => Some(CompletionTarget::Tag {
            replace: (line_start + col)..(line_start + col),
            query: String::new(),
            indent: false,
        }),
        Some(span) if col <= span.end => {
            if code[span.clone()].starts_with('^') {
                return None;
            }
            let start = span.start.min(col);
            let typed = &code[start..col];
            let query = typed.strip_prefix('#').unwrap_or(typed);
            Some(CompletionTarget::Tag {
                replace: (line_start + start)..(line_start + span.end),
                query: query.to_string(),
                indent: false,
            })
        }
        Some(_) => None,
    }
}

fn detect_sigil_token(
    code: &str,
    col: usize,
    line_start: usize,
    indent: bool,
) -> Option<CompletionTarget> {
    let clamped = col.min(code.len());
    let tok_start = code[..clamped]
        .char_indices()
        .rev()
        .take_while(|(_, ch)| !ch.is_whitespace())
        .last()
        .map_or(clamped, |(idx, _)| idx);
    if clamped <= tok_start {
        return None;
    }
    let tok_end = clamped
        + code[clamped..]
            .chars()
            .take_while(|ch| !ch.is_whitespace())
            .map(char::len_utf8)
            .sum::<usize>();
    let typed = &code[tok_start..clamped];
    let replace = (line_start + tok_start)..(line_start + tok_end);
    if let Some(query) = typed.strip_prefix('#') {
        Some(CompletionTarget::Tag {
            replace,
            query: query.to_string(),
            indent,
        })
    } else {
        typed.strip_prefix('^').map(|query| CompletionTarget::Link {
            replace,
            query: query.to_string(),
            indent,
        })
    }
}

fn all_sigil_tokens(slice: &str) -> bool {
    slice
        .split_whitespace()
        .all(|tok| tok.starts_with(['#', '^']))
}

fn currency_arg_target(
    code: &str,
    col: usize,
    line_start: usize,
    arg: Option<&Range<usize>>,
) -> CompletionTarget {
    match arg {
        Some(span) if col <= span.end => {
            let start = span.start.min(col);
            CompletionTarget::Currency {
                replace: (line_start + start)..(line_start + span.end),
                query: code[start..col].to_string(),
            }
        }
        _ => CompletionTarget::Currency {
            replace: (line_start + col)..(line_start + col),
            query: String::new(),
        },
    }
}

fn comma_currency_target(
    code: &str,
    col: usize,
    line_start: usize,
    span: &Range<usize>,
) -> CompletionTarget {
    let clamped_start = span.start.min(col);
    let seg_start = code[clamped_start..col]
        .rfind(',')
        .map_or(clamped_start, |idx| clamped_start + idx + 1);
    let seg_end = code[col..span.end]
        .find(',')
        .map_or(span.end, |idx| col + idx);
    CompletionTarget::Currency {
        replace: (line_start + seg_start)..(line_start + seg_end),
        query: code[seg_start..col].to_string(),
    }
}

fn detect_currency_after_number(
    code: &str,
    col: usize,
    line_start: usize,
) -> Option<CompletionTarget> {
    let tok_start = code[..col]
        .char_indices()
        .rev()
        .take_while(|(_, ch)| is_currency_char(*ch))
        .last()
        .map_or(col, |(idx, _)| idx);
    let tok_end = col
        + code[col..]
            .chars()
            .take_while(|ch| is_currency_char(*ch))
            .map(char::len_utf8)
            .sum::<usize>();

    let before = code[..tok_start].trim_end_matches([' ', '\t']);
    if before.len() == tok_start {
        // Require whitespace between the number and its currency.
        return None;
    }

    let prev = before
        .rsplit_once(|ch: char| ch.is_whitespace() || matches!(ch, '{' | ',' | '@'))
        .map_or(before, |(_, tail)| tail);
    if !is_beancount_number(prev) {
        return None;
    }

    Some(CompletionTarget::Currency {
        replace: (line_start + tok_start)..(line_start + tok_end),
        query: code[tok_start..col].to_string(),
    })
}

fn build_items(data: &Completions, text: &Rope, target: CompletionTarget) -> Vec<CompletionItem> {
    match target {
        CompletionTarget::Account {
            replace,
            query,
            indent,
        } => rank_candidates(&data.accounts, &query)
            .into_iter()
            .take(MAX_COMPLETION_ITEMS)
            .map(|account| {
                let new_text = if indent {
                    format!("  {account}")
                } else {
                    account.to_string()
                };
                make_item(
                    account,
                    new_text,
                    CompletionItemKind::STRUCT,
                    None,
                    &query,
                    text,
                    &replace,
                )
            })
            .collect(),
        CompletionTarget::Currency { replace, query } => rank_candidates(&data.currencies, &query)
            .into_iter()
            .take(MAX_COMPLETION_ITEMS)
            .map(|currency| {
                make_item(
                    currency,
                    currency.to_string(),
                    CompletionItemKind::UNIT,
                    None,
                    &query,
                    text,
                    &replace,
                )
            })
            .collect(),
        CompletionTarget::Payee { replace, query } => rank_candidates(&data.payees, &query)
            .into_iter()
            .take(MAX_COMPLETION_ITEMS)
            .map(|payee| {
                make_item(
                    payee,
                    format!("{payee}\""),
                    CompletionItemKind::VALUE,
                    Some("payee"),
                    &query,
                    text,
                    &replace,
                )
            })
            .collect(),
        CompletionTarget::PayeeOrNarration { replace, query } => {
            rank_payees_and_narrations(&data.payees, &data.narrations, &query)
                .into_iter()
                .take(MAX_COMPLETION_ITEMS)
                .map(|(candidate, detail)| {
                    make_item(
                        candidate,
                        format!("{candidate}\""),
                        CompletionItemKind::VALUE,
                        Some(detail),
                        &query,
                        text,
                        &replace,
                    )
                })
                .collect()
        }
        CompletionTarget::Narration {
            payee,
            replace,
            query,
        } => rank_narrations(data, payee.as_deref(), &query)
            .into_iter()
            .take(MAX_COMPLETION_ITEMS)
            .map(|narration| {
                make_item(
                    narration,
                    format!("{narration}\""),
                    CompletionItemKind::VALUE,
                    Some("narration"),
                    &query,
                    text,
                    &replace,
                )
            })
            .collect(),
        CompletionTarget::Tag {
            replace,
            query,
            indent,
        } => rank_sigil_candidates(&data.tags, '#', &query)
            .into_iter()
            .take(MAX_COMPLETION_ITEMS)
            .map(|tag| {
                let new_text = if indent {
                    format!("  {tag}")
                } else {
                    tag.to_string()
                };
                make_item(
                    tag,
                    new_text,
                    CompletionItemKind::CONSTANT,
                    None,
                    &query,
                    text,
                    &replace,
                )
            })
            .collect(),
        CompletionTarget::Link {
            replace,
            query,
            indent,
        } => rank_sigil_candidates(&data.links, '^', &query)
            .into_iter()
            .take(MAX_COMPLETION_ITEMS)
            .map(|link| {
                let new_text = if indent {
                    format!("  {link}")
                } else {
                    link.to_string()
                };
                make_item(
                    link,
                    new_text,
                    CompletionItemKind::REFERENCE,
                    None,
                    &query,
                    text,
                    &replace,
                )
            })
            .collect(),
    }
}

fn make_item(
    label: &str,
    new_text: String,
    kind: CompletionItemKind,
    detail: Option<&str>,
    query: &str,
    text: &Rope,
    replace: &Range<usize>,
) -> CompletionItem {
    let range = lsp_types::Range {
        start: text.offset_to_position(replace.start),
        end: text.offset_to_position(replace.end),
    };
    CompletionItem {
        label: label.to_string(),
        kind: Some(kind),
        detail: detail.map(str::to_string),
        filter_text: Some(query.to_string()),
        text_edit: Some(CompletionTextEdit::Edit(TextEdit { range, new_text })),
        ..Default::default()
    }
}

fn rank_candidates<'a>(candidates: &'a [String], query: &str) -> Vec<&'a str> {
    if query.is_empty() {
        return candidates.iter().map(String::as_str).collect();
    }
    let query_lower = query.to_lowercase();
    let mut scored: Vec<(u8, usize, &'a str)> = candidates
        .iter()
        .enumerate()
        .filter_map(|(ix, candidate)| {
            match_tier(candidate, &query_lower).map(|tier| (tier, ix, candidate.as_str()))
        })
        .collect();
    scored.sort_by_key(|(tier, ix, _)| (*tier, *ix));
    scored
        .into_iter()
        .map(|(_, _, candidate)| candidate)
        .collect()
}

fn rank_payees_and_narrations<'a>(
    payees: &'a [String],
    narrations: &'a [String],
    query: &str,
) -> Vec<(&'a str, &'static str)> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    if query.is_empty() {
        for payee in payees {
            if seen.insert(payee.as_str()) {
                out.push((payee.as_str(), "payee"));
            }
        }
        for narration in narrations {
            if seen.insert(narration.as_str()) {
                out.push((narration.as_str(), "narration"));
            }
        }
        return out;
    }

    let query_lower = query.to_lowercase();
    let mut scored: Vec<(u8, u8, usize, &'a str, &'static str)> = payees
        .iter()
        .enumerate()
        .filter_map(|(ix, payee)| {
            match_tier(payee, &query_lower).map(|tier| (tier, 0, ix, payee.as_str(), "payee"))
        })
        .chain(narrations.iter().enumerate().filter_map(|(ix, narration)| {
            match_tier(narration, &query_lower)
                .map(|tier| (tier, 1, ix, narration.as_str(), "narration"))
        }))
        .collect();
    scored.sort_by_key(|(tier, group, ix, _, _)| (*tier, *group, *ix));
    for (_, _, _, candidate, detail) in scored {
        if seen.insert(candidate) {
            out.push((candidate, detail));
        }
    }
    out
}

fn rank_narrations<'a>(data: &'a Completions, payee: Option<&str>, query: &str) -> Vec<&'a str> {
    let preferred = payee
        .and_then(|p| data.payee_narrations.get(p))
        .map_or(&[][..], Vec::as_slice);
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    if query.is_empty() {
        for narration in preferred.iter().chain(data.narrations.iter()) {
            if seen.insert(narration.as_str()) {
                out.push(narration.as_str());
            }
        }
        return out;
    }

    let query_lower = query.to_lowercase();
    let mut scored: Vec<(u8, u8, usize, &'a str)> = preferred
        .iter()
        .enumerate()
        .filter_map(|(ix, narration)| {
            match_tier(narration, &query_lower).map(|tier| (0, tier, ix, narration.as_str()))
        })
        .chain(
            data.narrations
                .iter()
                .enumerate()
                .filter_map(|(ix, narration)| {
                    match_tier(narration, &query_lower)
                        .map(|tier| (1, tier, ix, narration.as_str()))
                }),
        )
        .collect();
    scored.sort_by_key(|(group, tier, ix, _)| (*group, *tier, *ix));
    for (_, _, _, narration) in scored {
        if seen.insert(narration) {
            out.push(narration);
        }
    }
    out
}

fn rank_sigil_candidates<'a>(candidates: &'a [String], sigil: char, query: &str) -> Vec<&'a str> {
    if query.is_empty() {
        return candidates.iter().map(String::as_str).collect();
    }
    let query_lower = query.to_lowercase();
    let mut scored: Vec<(u8, usize, &'a str)> = candidates
        .iter()
        .enumerate()
        .filter_map(|(ix, candidate)| {
            let bare = candidate.strip_prefix(sigil).unwrap_or(candidate);
            match_tier(bare, &query_lower).map(|tier| (tier, ix, candidate.as_str()))
        })
        .collect();
    scored.sort_by_key(|(tier, ix, _)| (*tier, *ix));
    scored
        .into_iter()
        .map(|(_, _, candidate)| candidate)
        .collect()
}

fn match_tier(candidate: &str, query_lower: &str) -> Option<u8> {
    let candidate_lower = candidate.to_lowercase();
    if candidate_lower.starts_with(query_lower) {
        return Some(0);
    }
    if query_lower.contains(':') {
        if matches_colon_segments(&candidate_lower, query_lower) {
            return Some(1);
        }
    } else if candidate_lower
        .split(|ch: char| ch.is_whitespace() || matches!(ch, ':' | '-' | '_' | '/' | '.'))
        .filter(|segment| !segment.is_empty())
        .skip(1)
        .any(|segment| segment.starts_with(query_lower))
    {
        return Some(1);
    }
    if candidate_lower.contains(query_lower) {
        return Some(2);
    }
    None
}

fn matches_colon_segments(candidate_lower: &str, query_lower: &str) -> bool {
    let mut segments = candidate_lower.split(':');
    for part in query_lower.split(':') {
        if part.is_empty() {
            continue;
        }
        if !segments.any(|segment| segment.starts_with(part)) {
            return false;
        }
    }
    true
}

fn is_currency_char(ch: char) -> bool {
    !ch.is_whitespace() && !matches!(ch, ',' | '#' | '^' | '"' | ':' | ';' | '{' | '}')
}

fn is_beancount_number(token: &str) -> bool {
    if token.is_empty() {
        return false;
    }
    let mut saw_number = false;
    let mut covered = 0usize;
    for (kind, span) in tokenize(token) {
        if span.start != covered {
            return false;
        }
        match kind {
            Token::Number => saw_number = true,
            Token::Comma => {}
            _ => return false,
        }
        covered = span.end;
    }
    saw_number && covered == token.len()
}

fn is_iso_date(token: &str) -> bool {
    let mut tokens = tokenize(token);
    matches!(
        (tokens.next(), tokens.next()),
        (Some((Token::Date, span)), None) if span == (0..token.len())
    )
}

fn strip_unquoted_comment(line: &str) -> &str {
    let mut in_quotes = false;
    for (idx, ch) in line.char_indices() {
        if ch == '"' {
            in_quotes = !in_quotes;
        } else if ch == ';' && !in_quotes {
            return &line[..idx];
        }
    }
    line
}

fn whitespace_spans(line: &str) -> Vec<Range<usize>> {
    let mut spans = Vec::new();
    let mut start: Option<usize> = None;
    for (idx, ch) in line.char_indices() {
        if ch.is_whitespace() {
            if let Some(s) = start.take() {
                spans.push(s..idx);
            }
        } else if start.is_none() {
            start = Some(idx);
        }
    }
    if let Some(s) = start {
        spans.push(s..line.len());
    }
    spans
}

fn snap_char_boundary(text: &str, offset: usize) -> usize {
    let mut idx = offset.min(text.len());
    while idx > 0 && !text.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

#[cfg(test)]
#[path = "completion_test.rs"]
mod tests;
