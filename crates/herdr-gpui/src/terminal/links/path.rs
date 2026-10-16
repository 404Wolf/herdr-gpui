//! Text in one row of a terminal that may name a local file, the way a
//! compiler, a test runner, `git status`, or `ls` prints one. Any word may:
//! reading the row is all this does, and whether a file is there is asked,
//! off the UI thread, once the link modifier is held over the word.

use super::super::selection::separates;
use std::{borrow::Cow, ops::Range};

/// The longest path read from a row, Linux's `PATH_MAX`.
const MAX_PATH_BYTES: usize = 4096;

/// A file path read from a row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PlainPath<'a> {
    /// The bytes of the row it covers, a `:line` or `:line:column` suffix
    /// included.
    pub(super) range: Range<usize>,
    /// The path without that suffix, with shell escapes such as `\ ` removed.
    pub(super) path: Cow<'a, str>,
    /// The line the suffix names.
    pub(super) line: Option<u32>,
    /// Whether the path runs to the end of the row, where it may continue on
    /// the next one.
    pub(super) open: bool,
}

/// The file path in one row's `text` covering the byte at `hit`. A separator
/// after a backslash belongs to the path, as in the `/tmp/a\ b` a file drop
/// or a shell's completion prints.
pub(super) fn plain_path(text: &str, hit: usize) -> Option<PlainPath<'_>> {
    let mut start = 0;
    let mut end = text.len();
    let mut chars = text.char_indices();
    while let Some((index, c)) = chars.next() {
        if c == '\\' {
            chars.next();
        } else if separates(c) {
            if index >= hit {
                end = index;
                break;
            }
            start = index + c.len_utf8();
        }
    }
    // Punctuation closing a sentence, or the colon after a location, is not
    // part of the path before it, unless it is escaped.
    let mut token = &text[start..end];
    while let Some(rest) = token.strip_suffix(['.', ':', '!', '?'])
        && !rest.ends_with('\\')
    {
        token = rest;
    }
    if hit < start || hit >= start + token.len() {
        return None;
    }
    let (escaped, line) = without_location(token);
    let path = unescape(escaped);
    looks_like_path(&path).then_some(PlainPath {
        range: start..start + token.len(),
        path,
        line,
        open: end == text.len(),
    })
}

/// `text` with the backslash before each `shell_special` character removed;
/// any other backslash is part of the name.
fn unescape(text: &str) -> Cow<'_, str> {
    if !text.contains('\\') {
        return Cow::Borrowed(text);
    }
    let mut path = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\'
            && let Some(&next) = chars.peek()
            && shell_special(next)
        {
            path.push(next);
            chars.next();
        } else {
            path.push(c);
        }
    }
    Cow::Owned(path)
}

/// `path` as one shell word, each `shell_special` character after a
/// backslash: what a file drop pastes, and what `plain_path` reads back.
pub(crate) fn escape_into(path: &str, out: &mut String) {
    for c in path.chars() {
        if shell_special(c) {
            out.push('\\');
        }
        out.push(c);
    }
}

/// Characters a pasted path escapes: every character that ends a path here,
/// such as a comma or the narrow no-break space in macOS screenshot names,
/// and every one a POSIX shell, bash, or zsh would expand or quote on.
pub(crate) fn shell_special(c: char) -> bool {
    separates(c)
        || matches!(
            c,
            '\\' | '$' | '&' | '*' | '?' | '!' | '#' | '~' | '^' | '='
        )
}

/// `token` without a trailing `:line` or `:line:column`, and that line.
fn without_location(token: &str) -> (&str, Option<u32>) {
    let mut path = token;
    let mut line = None;
    for _ in 0..2 {
        match path.rsplit_once(':') {
            Some((rest, number))
                if !number.is_empty()
                    && number.len() <= 9
                    && number.bytes().all(|b| b.is_ascii_digit()) =>
            {
                path = rest;
                // The leftmost number is the line; a column follows it.
                line = number.parse().ok();
            }
            _ => break,
        }
    }
    (path, line)
}

fn looks_like_path(path: &str) -> bool {
    if path.is_empty() || path.len() > MAX_PATH_BYTES {
        return false;
    }
    // A scheme, an `scp` host, or a drive letter is not a local path here.
    if path.contains(':') {
        return false;
    }
    // `~user` names another account's home, which is not resolved.
    if path.starts_with('~') && path != "~" && !path.starts_with("~/") {
        return false;
    }
    let name = path
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or_default();
    !matches!(name, "" | "." | ".." | "~")
}
