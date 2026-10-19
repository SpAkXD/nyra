//! Positions. The compiler counts lines and columns from 1, columns in characters (`Span`); LSP
//! counts both from 0, and columns in UTF-16 code units unless the client accepts characters
//! (`utf-32`). Byte offsets are what `edit::outline` gives.

use crate::ast::Span;
use crate::json::{obj, Json};

/// How the client counts columns.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Enc {
    Utf16,
    Utf32,
}

/// The start of each line of a text (byte offsets).
pub struct Lines<'a> {
    text: &'a str,
    starts: Vec<usize>,
}

impl<'a> Lines<'a> {
    pub fn new(text: &'a str) -> Lines<'a> {
        let mut starts = vec![0];
        starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
        Lines { text, starts }
    }

    pub fn count(&self) -> usize {
        self.starts.len()
    }

    /// Line `l` (0-based) without its line break; empty past the end.
    pub fn line(&self, l: usize) -> &'a str {
        let Some(&s) = self.starts.get(l) else { return "" };
        let e = self.starts.get(l + 1).map_or(self.text.len(), |n| n - 1);
        self.text[s..e].trim_end_matches('\r')
    }

    /// The (1-based) line and column of a byte offset.
    pub fn span_of(&self, off: usize) -> Span {
        let off = off.min(self.text.len());
        let l = self.starts.partition_point(|&s| s <= off) - 1;
        let col = self.text[self.starts[l]..off].chars().count() + 1;
        Span { line: l + 1, col }
    }

    /// The byte offset of a position (clamped to its line).
    pub fn offset(&self, s: Span) -> usize {
        let l = s.line.saturating_sub(1);
        let Some(&start) = self.starts.get(l) else { return self.text.len() };
        let line = self.line(l);
        let within: usize = line.chars().take(s.col.saturating_sub(1)).map(char::len_utf8).sum();
        start + within
    }

    /// An LSP position for a compiler position.
    pub fn lsp(&self, s: Span, enc: Enc) -> (usize, usize) {
        let l = s.line.saturating_sub(1);
        if l >= self.count() {
            let last = self.count() - 1;
            return (last, units(self.line(last), usize::MAX, enc));
        }
        (l, units(self.line(l), s.col.saturating_sub(1), enc))
    }

    /// The compiler position of an LSP position.
    pub fn span(&self, line: usize, col: usize, enc: Enc) -> Span {
        let text = self.line(line);
        let mut units_seen = 0;
        let mut chars = 0;
        for c in text.chars() {
            if units_seen >= col {
                break;
            }
            units_seen += match enc {
                Enc::Utf16 => c.len_utf16(),
                Enc::Utf32 => 1,
            };
            chars += 1;
        }
        Span { line: line + 1, col: chars + 1 }
    }

    /// The LSP range `{start, end}` from one compiler position to another.
    pub fn range(&self, a: Span, b: Span, enc: Enc) -> Json {
        range_json(self.lsp(a, enc), self.lsp(b, enc))
    }

    /// The range of the whole text.
    pub fn whole(&self, enc: Enc) -> Json {
        let last = self.count() - 1;
        range_json((0, 0), (last, units(self.line(last), usize::MAX, enc)))
    }
}

/// How many columns the first `chars` characters of `line` take.
fn units(line: &str, chars: usize, enc: Enc) -> usize {
    line.chars()
        .take(chars)
        .map(|c| match enc {
            Enc::Utf16 => c.len_utf16(),
            Enc::Utf32 => 1,
        })
        .sum()
}

pub fn pos_json((line, col): (usize, usize)) -> Json {
    obj([("line", (line as i64).into()), ("character", (col as i64).into())])
}

pub fn range_json(a: (usize, usize), b: (usize, usize)) -> Json {
    obj([("start", pos_json(a)), ("end", pos_json(b))])
}

/// True for the characters of a name.
pub fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Where the token that starts at `s` ends: the end of a name or number, of a string or character
/// literal on the same line, of a two-character operator, else one character later.
pub fn token_end(lines: &Lines, s: Span) -> Span {
    let line: Vec<char> = lines.line(s.line.saturating_sub(1)).chars().collect();
    let i = s.col.saturating_sub(1);
    let Some(&c) = line.get(i) else { return s };
    let mut j = i + 1;
    if is_word(c) {
        while line.get(j).is_some_and(|c| is_word(*c)) {
            j += 1;
        }
    } else if c == '"' || c == '\'' {
        while j < line.len() && line[j] != c {
            j += if line[j] == '\\' { 2 } else { 1 };
        }
        j = (j + 1).min(line.len());
    } else if let Some(&d) = line.get(i + 1) {
        let pair: String = [c, d].iter().collect();
        if ["->", "=>", "..", "==", "!=", "<=", ">=", "&&", "||", "??", "+=", "-=", "*=", "/=", "%="].contains(&pair.as_str()) {
            j = i + 2;
        }
    }
    Span { line: s.line, col: j + 1 }
}

/// The name around a position (`col` may be just after it), with its start column.
pub fn word_at(line: &str, col: usize) -> Option<(String, usize)> {
    let chars: Vec<char> = line.chars().collect();
    let i = col.saturating_sub(1);
    let at = if chars.get(i).is_some_and(|c| is_word(*c)) {
        i
    } else if i > 0 && chars.get(i - 1).is_some_and(|c| is_word(*c)) {
        i - 1
    } else {
        return None;
    };
    let mut s = at;
    while s > 0 && is_word(chars[s - 1]) {
        s -= 1;
    }
    let mut e = at;
    while e < chars.len() && is_word(chars[e]) {
        e += 1;
    }
    Some((chars[s..e].iter().collect(), s + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_positions_both_ways() {
        let text = "let a = \"😀\"\r\nprint(a)\n";
        let l = Lines::new(text);
        assert_eq!(l.count(), 3);
        assert_eq!(l.line(0), "let a = \"😀\"");
        assert_eq!(l.lsp(Span { line: 1, col: 11 }, Enc::Utf16), (0, 11));
        assert_eq!(l.lsp(Span { line: 1, col: 11 }, Enc::Utf32), (0, 10));
        assert_eq!(l.span(0, 11, Enc::Utf16), Span { line: 1, col: 11 });
        assert_eq!(l.span(1, 6, Enc::Utf16), Span { line: 2, col: 7 });
        assert_eq!(l.span_of(l.offset(Span { line: 2, col: 7 })), Span { line: 2, col: 7 });
        // past the last line: the end of the text
        assert_eq!(l.lsp(Span { line: 9, col: 1 }, Enc::Utf16), (2, 0));
    }

    #[test]
    fn finds_token_ends_and_words() {
        let text = "x += \"a b\" ?? foo_1";
        let l = Lines::new(text);
        assert_eq!(token_end(&l, Span { line: 1, col: 3 }).col, 5);
        assert_eq!(token_end(&l, Span { line: 1, col: 6 }).col, 11);
        assert_eq!(token_end(&l, Span { line: 1, col: 15 }).col, 20);
        assert_eq!(word_at(text, 20), Some(("foo_1".to_string(), 15)));
        assert_eq!(word_at(text, 2), Some(("x".to_string(), 1)));
        assert_eq!(word_at(text, 4), None);
    }
}
