//! Editing entries of a ledger file in place.
//!
//! An entry's text is the line it starts on and the indented lines after it,
//! which hold its metadata and postings; a blank or unindented line ends it,
//! unless a string is still open. That is the text [`entry_text`] returns,
//! [`replace_entry_text`] replaces, and [`delete_entry_text`] removes, while
//! [`append_entry_text`] adds a new entry at the end of the file, so an editor
//! can work on one entry without the rest of the file.

use std::ops::Range;
use std::{fmt, fs, io};

use chrono::{Local, NaiveDate};

use crate::options::{CUSTOM_LUMI_OPTION, LUMI_OPTION_DECIMAL_POINT_COLUMN};
use crate::parse::{Token, tokenize};

/// Why an entry's text couldn't be read or replaced.
#[derive(Debug)]
pub enum EditError {
    /// The file couldn't be read or written.
    Io { path: String, error: io::Error },
    /// The file has no such line.
    NoSuchLine { path: String, line: usize },
    /// The entry's text in the file is not the text that was edited: the
    /// file changed since it was read. Nothing was written.
    Changed { path: String, line: usize },
    /// The file's text on disk is not the text that was read into the editor.
    /// Nothing was written.
    FileChanged { path: String },
}

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EditError::Io { path, error } => write!(f, "Couldn't access {path}: {error}"),
            EditError::NoSuchLine { path, line } => {
                write!(f, "{path} has no line {line}.")
            }
            EditError::Changed { path, line } => write!(
                f,
                "The entry at {path}:{line} changed since it was read. Reload the ledger and edit it again."
            ),
            EditError::FileChanged { path } => write!(
                f,
                "The file {path} changed on disk since it was read. Reload the ledger and edit it again."
            ),
        }
    }
}

impl std::error::Error for EditError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            EditError::Io { error, .. } => Some(error),
            _ => None,
        }
    }
}

/// The full text of the ledger source file at `path`, with `\r\n` line breaks
/// normalized to `\n` for editing.
pub fn file_text(path: &str) -> Result<String, EditError> {
    let raw = fs::read_to_string(path).map_err(|error| EditError::Io {
        path: path.to_string(),
        error,
    })?;
    Ok(raw.replace("\r\n", "\n"))
}

/// Replaces the full text of the ledger source file at `path` with
/// `replacement`.
///
/// `original` must be the text [`file_text`] returned. If the file on disk has
/// changed since, nothing is written and [`EditError::FileChanged`] is
/// returned. Existing `\r\n` line breaks in the file are preserved.
pub fn replace_file_text(path: &str, original: &str, replacement: &str) -> Result<(), EditError> {
    let file = read(path)?;
    let current = file.text.replace("\r\n", "\n");
    if current != original {
        return Err(EditError::FileChanged {
            path: path.to_string(),
        });
    }
    let normalized = replacement.replace("\r\n", "\n");
    let out = if file.newline == "\r\n" {
        normalized.replace('\n', "\r\n")
    } else {
        normalized
    };
    fs::write(path, out).map_err(|error| EditError::Io {
        path: path.to_string(),
        error,
    })
}

/// The text of the entry starting on `line` (counted from 1) of the file at
/// `path`, without its final line break. Lines are joined with `\n`
/// whatever the file uses.
pub fn entry_text(path: &str, line: usize) -> Result<String, EditError> {
    let file = read(path)?;
    let lines = file.lines(line, path)?;
    Ok(lines.join("\n"))
}

/// Replaces the text of the entry starting on `line` (counted from 1) of
/// the file at `path` with `replacement`.
///
/// `original` must be the entry's text as [`entry_text`] returned it. If the
/// entry's text differs from it, the file changed since it was read; nothing
/// is written and [`EditError::Changed`] is returned.
///
/// The file keeps its line breaks: `replacement` is split on `\n` and
/// written with the file's own line break after every line.
pub fn replace_entry_text(
    path: &str,
    line: usize,
    original: &str,
    replacement: &str,
) -> Result<(), EditError> {
    let file = read(path)?;
    let lines = file.lines(line, path)?;
    if lines.join("\n") != original {
        return Err(EditError::Changed {
            path: path.to_string(),
            line,
        });
    }
    let start = line - 1;
    let end = start + lines.len();
    let mut text = String::with_capacity(file.text.len() + replacement.len());
    for line in &file.lines[..start] {
        text.push_str(line);
        text.push_str(file.newline);
    }
    for line in replacement.split('\n') {
        text.push_str(line.strip_suffix('\r').unwrap_or(line));
        text.push_str(file.newline);
    }
    for line in &file.lines[end..] {
        text.push_str(line);
        text.push_str(file.newline);
    }
    if !file.ends_with_newline && !text.is_empty() {
        text.truncate(text.len() - file.newline.len());
    }
    fs::write(path, text).map_err(|error| EditError::Io {
        path: path.to_string(),
        error,
    })
}

/// Deletes the entry starting on `line` (counted from 1) of the file at
/// `path`, together with the blank lines that separated it from its neighbor.
///
/// `original` must be the entry's text as [`entry_text`] returned it. If the
/// entry's text differs from it, the file changed since it was read; nothing
/// is written and [`EditError::Changed`] is returned.
pub fn delete_entry_text(path: &str, line: usize, original: &str) -> Result<(), EditError> {
    let file = read(path)?;
    let lines = file.lines(line, path)?;
    if lines.join("\n") != original {
        return Err(EditError::Changed {
            path: path.to_string(),
            line,
        });
    }
    let mut start = line - 1;
    let mut end = start + lines.len();
    while end < file.lines.len() && file.lines[end].trim().is_empty() {
        end += 1;
    }
    if end == file.lines.len() {
        while start > 0 && file.lines[start - 1].trim().is_empty() {
            start -= 1;
        }
    }
    let mut text = String::with_capacity(file.text.len());
    for line in file.lines[..start].iter().chain(&file.lines[end..]) {
        text.push_str(line);
        text.push_str(file.newline);
    }
    if !file.ends_with_newline && !text.is_empty() {
        text.truncate(text.len() - file.newline.len());
    }
    fs::write(path, text).map_err(|error| EditError::Io {
        path: path.to_string(),
        error,
    })
}

/// Appends `entry` to the file at `path`, separated from any preceding
/// content by one blank line, keeping the file's own line breaks.
pub fn append_entry_text(path: &str, entry: &str) -> Result<(), EditError> {
    let file = read(path)?;
    let entry = entry.trim_end_matches(['\r', '\n']);
    let mut text = String::with_capacity(file.text.len() + entry.len() + 4);
    let has_content = file.lines.iter().any(|line| !line.trim().is_empty());
    if has_content {
        text.push_str(&file.text);
        if !file.ends_with_newline {
            text.push_str(file.newline);
        }
        let ends_with_blank = file.lines.last().is_some_and(|line| line.trim().is_empty());
        if !ends_with_blank {
            text.push_str(file.newline);
        }
    }
    for line in entry.split('\n') {
        text.push_str(line.strip_suffix('\r').unwrap_or(line));
        text.push_str(file.newline);
    }
    if has_content && !file.ends_with_newline && !text.is_empty() {
        text.truncate(text.len() - file.newline.len());
    }
    fs::write(path, text).map_err(|error| EditError::Io {
        path: path.to_string(),
        error,
    })
}

/// `entry` with the `YYYY-MM-DD` date at the start of its first line replaced
/// by `date`, or `entry` unchanged when it does not begin with a date.
pub fn with_date(entry: &str, date: NaiveDate) -> String {
    if entry.is_char_boundary(10)
        && NaiveDate::parse_from_str(&entry[..10], "%Y-%m-%d").is_ok()
        && (entry.len() == 10 || entry[10..].starts_with(char::is_whitespace))
    {
        format!("{date}{}", &entry[10..])
    } else {
        entry.to_string()
    }
}

/// `entry` with its leading `YYYY-MM-DD` date replaced by today's local date.
pub fn with_today(entry: &str) -> String {
    with_date(entry, Local::now().date_naive())
}

/// Byte range of horizontal whitespace between a posting's account and number
/// within a single line, together with the desired space count to place the
/// number's decimal point at a 1-based target column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostingAlignment {
    pub ws_range: Range<usize>,
    pub desired_spaces: usize,
}

/// Inspects a single line (without trailing `\r`/`\n`, though any trailing line
/// break is ignored) and returns the whitespace replacement needed to align a
/// posting number's decimal point at 1-based `decimal_point_column`, keeping at
/// least 1 space between the account name and the number.
///
/// Returns `None` if `line` is not an indented posting with a number, or if its
/// separator is already `desired_spaces` ASCII spaces.
pub fn align_posting_line(line: &str, decimal_point_column: usize) -> Option<PostingAlignment> {
    let line = line.trim_end_matches(['\r', '\n']);
    let indent_end = line
        .bytes()
        .take_while(|b| matches!(b, b' ' | b'\t'))
        .count();
    if indent_end == 0 {
        return None;
    }

    let body = &line[indent_end..];
    let mut tokens = tokenize(body);
    let (first_tok, _) = tokens.next()?;
    if matches!(
        first_tok,
        Token::Asterisk | Token::ExclamationMark | Token::QuestionMark
    ) {
        if tokens.next()?.0 != Token::WhiteSpace {
            return None;
        }
        if tokens.next()?.0 != Token::Account {
            return None;
        }
    } else if first_tok != Token::Account {
        return None;
    }

    let (ws_tok, rel_ws_span) = tokens.next()?;
    if ws_tok != Token::WhiteSpace {
        return None;
    }
    let ws_range = (indent_end + rel_ws_span.start)..(indent_end + rel_ws_span.end);
    let num_str = leading_posting_number(&line[ws_range.end..])?;

    let prefix_cols = line[..ws_range.start].chars().count();
    let int_part_cols = match num_str.find('.') {
        Some(dot_idx) => num_str[..dot_idx].chars().count(),
        None => num_str.chars().count(),
    };
    let target = decimal_point_column.max(1) as isize;
    let desired_spaces =
        ((target - 1) - prefix_cols as isize - int_part_cols as isize).max(1) as usize;

    let current_ws = &line[ws_range.clone()];
    if current_ws.len() == desired_spaces && current_ws.bytes().all(|b| b == b' ') {
        return None;
    }
    Some(PostingAlignment {
        ws_range,
        desired_spaces,
    })
}

fn leading_posting_number(rest: &str) -> Option<&str> {
    let bytes = rest.as_bytes();
    let mut idx = usize::from(matches!(bytes.first(), Some(b'-' | b'+')));
    if bytes.get(idx).is_some_and(u8::is_ascii_digit) {
        while idx < bytes.len() {
            if bytes[idx].is_ascii_digit()
                || (bytes[idx] == b',' && bytes.get(idx + 1).is_some_and(u8::is_ascii_digit))
            {
                idx += 1;
            } else {
                break;
            }
        }
        if bytes.get(idx) == Some(&b'.') {
            idx += 1;
            while bytes.get(idx).is_some_and(u8::is_ascii_digit) {
                idx += 1;
            }
        }
    } else if bytes.get(idx) == Some(&b'.') && bytes.get(idx + 1).is_some_and(u8::is_ascii_digit) {
        idx += 2;
        while bytes.get(idx).is_some_and(u8::is_ascii_digit) {
            idx += 1;
        }
    } else {
        return None;
    }

    if idx == bytes.len() || matches!(bytes[idx], b' ' | b'\t' | b';' | b'{') {
        Some(&rest[..idx])
    } else {
        None
    }
}

/// Scans `text` for the first valid
/// `YYYY-MM-DD custom "lumi-option" "decimal-point-column" "<N>"` directive
/// outside multi-line strings and returns `N`.
pub fn parse_decimal_point_column_in_text(text: &str) -> Option<usize> {
    let mut in_string = false;
    for raw_line in text.lines() {
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        let started_in_string = in_string;
        in_string = ends_in_string(line, in_string);
        if started_in_string
            || line.starts_with(char::is_whitespace)
            || !line.contains(CUSTOM_LUMI_OPTION)
        {
            continue;
        }
        let tokens: Vec<(Token, &str)> = tokenize(line)
            .filter(|(tok, _)| !matches!(tok, Token::WhiteSpace | Token::Comment))
            .map(|(tok, span)| (tok, &line[span]))
            .collect();
        if let [
            (Token::Date, date_raw),
            (Token::Custom, _),
            (Token::String, kind_raw),
            (Token::String, key_raw),
            (val_tok @ (Token::String | Token::Number), val_raw),
        ] = tokens.as_slice()
            && NaiveDate::parse_from_str(date_raw, "%Y-%m-%d").is_ok()
            && unquote_token_string(kind_raw) == CUSTOM_LUMI_OPTION
            && unquote_token_string(key_raw) == LUMI_OPTION_DECIMAL_POINT_COLUMN
        {
            let val = if *val_tok == Token::String {
                unquote_token_string(val_raw)
            } else {
                *val_raw
            };
            if let Some(col) = val.trim().parse::<usize>().ok().filter(|&col| col >= 1) {
                return Some(col);
            }
        }
    }
    None
}

fn unquote_token_string(s: &str) -> &str {
    let mut chars = s.chars();
    chars.next();
    chars.next_back();
    chars.as_str()
}

/// A file read as lines, remembering how to write it back unchanged.
struct File {
    text: String,
    /// The lines without their line breaks.
    lines: Vec<String>,
    /// The line break the file uses: that of its first line, else `\n`.
    newline: &'static str,
    ends_with_newline: bool,
}

fn read(path: &str) -> Result<File, EditError> {
    let text = fs::read_to_string(path).map_err(|error| EditError::Io {
        path: path.to_string(),
        error,
    })?;
    let newline = if text
        .split_inclusive('\n')
        .next()
        .is_some_and(|first| first.ends_with("\r\n"))
    {
        "\r\n"
    } else {
        "\n"
    };
    let lines = text
        .split_inclusive('\n')
        .map(|line| {
            let line = line.strip_suffix('\n').unwrap_or(line);
            line.strip_suffix('\r').unwrap_or(line).to_string()
        })
        .collect();
    Ok(File {
        ends_with_newline: text.ends_with('\n'),
        text,
        lines,
        newline,
    })
}

impl File {
    /// The lines of the entry starting on `line` (counted from 1).
    fn lines(&self, line: usize, path: &str) -> Result<&[String], EditError> {
        if line == 0 || line > self.lines.len() {
            return Err(EditError::NoSuchLine {
                path: path.to_string(),
                line,
            });
        }
        let start = line - 1;
        Ok(&self.lines[start..start + entry_line_count(&self.lines[start..])])
    }
}

/// How many of `lines`, which start with an entry's first line, belong to
/// the entry: the first, then every line that is indented and not blank,
/// or that continues a string.
fn entry_line_count(lines: &[String]) -> usize {
    let mut in_string = false;
    for (count, line) in lines.iter().enumerate() {
        if count > 0
            && !in_string
            && (line.trim().is_empty() || !line.starts_with(char::is_whitespace))
        {
            return count;
        }
        in_string = ends_in_string(line, in_string);
    }
    lines.len()
}

/// Whether `line` ends inside a string, given whether it started in one. A
/// `;` outside a string starts a comment, which runs to the end of the line.
pub fn ends_in_string(line: &str, mut in_string: bool) -> bool {
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' if in_string => {
                chars.next();
            }
            '"' => in_string = !in_string,
            ';' if !in_string => break,
            _ => {}
        }
    }
    in_string
}

#[cfg(test)]
#[path = "edit_test.rs"]
mod tests;
