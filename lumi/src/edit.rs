//! Editing entries of a ledger file in place.
//!
//! An entry's text is the line it starts on and the indented lines after it,
//! which hold its metadata and postings; a blank or unindented line ends it,
//! unless a string is still open. That is the text [`entry_text`] returns,
//! [`replace_entry_text`] replaces, and [`delete_entry_text`] removes, while
//! [`append_entry_text`] adds a new entry at the end of the file, so an editor
//! can work on one entry without the rest of the file.

use std::{fmt, fs, io};

use chrono::{Local, NaiveDate};

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
fn ends_in_string(line: &str, mut in_string: bool) -> bool {
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
