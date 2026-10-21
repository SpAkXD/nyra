//! Turns source text into tokens. Newlines end statements, except inside `( )` (and not in a
//! `{ }` block inside those, such as a child block).

use crate::ast::{BinOp, Span};
use crate::diag::{Diag, Edit};
use crate::hints;

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Int(i64),
    Float(f64),
    Str(String),
    /// A string containing `{expr}` parts.
    Interp(Vec<StrPart>),
    /// `'a'`: one code point
    Char(u32),
    Ident(String),
    // keywords
    Fn,
    Let,
    Var,
    If,
    Else,
    While,
    For,
    In,
    Ret,
    True,
    False,
    Struct,
    Inout,
    Break,
    Continue,
    Arena,
    Enum,
    Match,
    // punctuation
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Colon,
    /// `?` of a ternary: `c ? a : b`
    Question,
    Dot,
    Arrow,
    /// `=>` of a lambda: `x => x * 2`
    FatArrow,
    DotDot,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Assign,
    /// `+=` `-=` `*=` `/=` `%=`
    OpAssign(BinOp),
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    Not,
    /// `?` of an optional type `int?`
    /// `??`
    Coalesce,
    Newline,
    Eof,
}

impl Tok {
    /// How a token is named in an error message: "found `x`", "found keyword `let`", "found end of line".
    pub fn describe(&self) -> String {
        match self {
            Tok::Int(n) => format!("number `{n}`"),
            Tok::Float(f) => format!("number `{f:?}`"),
            Tok::Str(_) | Tok::Interp(_) => "a string".into(),
            Tok::Char(c) => format!("character `{}`", char_literal(*c)),
            Tok::Ident(s) => format!("`{s}`"),
            Tok::Newline => "end of line".into(),
            Tok::Eof => "end of file".into(),
            Tok::OpAssign(op) => format!("`{}=`", op.symbol()),
            t if t.is_keyword() => format!("keyword `{}`", t.text()),
            other => format!("`{}`", other.text()),
        }
    }

    /// The words that cannot be used as names.
    pub fn is_keyword(&self) -> bool {
        matches!(
            self,
            Tok::Fn
                | Tok::Let
                | Tok::Var
                | Tok::If
                | Tok::Else
                | Tok::While
                | Tok::For
                | Tok::In
                | Tok::Ret
                | Tok::Struct
                | Tok::Inout
                | Tok::Break
                | Tok::Continue
                | Tok::Arena
                | Tok::Enum
                | Tok::Match
        )
    }

    pub fn text(&self) -> &'static str {
        match self {
            Tok::Fn => "fn",
            Tok::Let => "let",
            Tok::Var => "var",
            Tok::If => "if",
            Tok::Else => "else",
            Tok::While => "while",
            Tok::For => "for",
            Tok::In => "in",
            Tok::Ret => "return",
            Tok::True => "true",
            Tok::False => "false",
            Tok::Struct => "struct",
            Tok::Inout => "inout",
            Tok::Break => "break",
            Tok::Continue => "continue",
            Tok::Arena => "arena",
            Tok::Enum => "enum",
            Tok::Match => "match",
            Tok::LParen => "(",
            Tok::RParen => ")",
            Tok::LBrace => "{",
            Tok::RBrace => "}",
            Tok::LBracket => "[",
            Tok::RBracket => "]",
            Tok::Comma => ",",
            Tok::Colon => ":",
            Tok::Question => "?",
            Tok::Dot => ".",
            Tok::Arrow => "->",
            Tok::FatArrow => "=>",
            Tok::DotDot => "..",
            Tok::Plus => "+",
            Tok::Minus => "-",
            Tok::Star => "*",
            Tok::Slash => "/",
            Tok::Percent => "%",
            Tok::Assign => "=",
            Tok::Eq => "==",
            Tok::Ne => "!=",
            Tok::Lt => "<",
            Tok::Le => "<=",
            Tok::Gt => ">",
            Tok::Ge => ">=",
            Tok::And => "&&",
            Tok::Or => "||",
            Tok::Not => "!",
            Tok::Coalesce => "??",
            _ => "",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum StrPart {
    Lit(String),
    /// Source of an expression inside `{ }`, and where it starts.
    Code(String, Span),
}

#[derive(Debug, Clone)]
pub struct Token {
    pub tok: Tok,
    pub span: Span,
}

/// The inside of `{ }` in a string is CSS, not code, when a space follows the `{` and it is a list
/// of `property: value` (separated by `;`, a space after each colon): `"50% { rotate: 3deg }"` is
/// text, so the bodies of `keyframes(...)` and `css(...)` in `use web` read as written.
fn css_block(code: &str) -> bool {
    if !code.starts_with([' ', '\t']) {
        return false;
    }
    let mut decls = code.split(';').map(str::trim).filter(|d| !d.is_empty()).peekable();
    decls.peek().is_some()
        && decls.all(|d| {
            let Some((p, v)) = d.split_once(':') else { return false };
            let p = p.trim_end();
            !p.is_empty()
                && p.chars().all(|c| c.is_ascii_lowercase() || c == '-')
                && v.starts_with([' ', '\t'])
                && !v.trim().is_empty()
        })
}

/// Where a string that is not closed on its line ends, when it was written over several lines:
/// the first `"` of a later line, if what follows it closes the call or the item (`")`, `",`) or
/// ends the line. `nl` is the line break after the opening line.
fn multiline_end(cs: &[char], nl: usize) -> Option<usize> {
    if cs.get(nl) != Some(&'\n') {
        return None;
    }
    let mut k = nl + 1;
    for _ in 0..200 {
        if k >= cs.len() {
            return None;
        }
        let end = cs[k..].iter().position(|c| *c == '\n').map_or(cs.len(), |p| k + p);
        if let Some(q) = (k..end).find(|&j| cs[j] == '"' && (j == k || cs[j - 1] != '\\')) {
            let after: String = cs[q + 1..end].iter().collect();
            let after = after.trim();
            return (after.is_empty() || after.starts_with(')') || after.starts_with(',')).then_some(q);
        }
        k = end + 1;
    }
    None
}

/// The text of a string written over several lines as the text of a one-line string: each line
/// break `\n`, and the braces of a `{ ... }` that spanned lines doubled (CSS rules: they were text,
/// as a `{ }` that is not closed on its line is, and on one line they would insert a value).
fn one_line(text: &str) -> String {
    let cs: Vec<char> = text.chars().collect();
    let mut double = vec![false; cs.len()];
    let mut open: Vec<usize> = Vec::new();
    for (i, c) in cs.iter().enumerate() {
        match c {
            '{' => open.push(i),
            '}' => {
                if let Some(o) = open.pop() {
                    if cs[o..i].contains(&'\n') {
                        double[o] = true;
                        double[i] = true;
                    }
                }
            }
            _ => {}
        }
    }
    let mut out = String::new();
    for (i, c) in cs.iter().enumerate() {
        match c {
            '\r' => {}
            '\n' => out.push_str("\\n"),
            _ if double[i] => {
                out.push(*c);
                out.push(*c);
            }
            _ => out.push(*c),
        }
    }
    out
}

/// True for a line of CSS written as code (`h1 { color: red; }`, `color: red;`, `@media ...`),
/// which belongs in `css("...")`.
fn css_line(line: &str) -> bool {
    let t = line.trim();
    if t.starts_with("@media") || t.starts_with("@import") || t.starts_with("@font-face") || t.starts_with("@keyframes") {
        return true;
    }
    let selector =
        |s: &str| !s.trim().is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || " .#,:>*_-[]=\"'()+~%".contains(c));
    // `h1, .x { font-weight: 700; }` or `.card {` followed by declarations
    if let Some((sel, body)) = t.split_once('{') {
        let words = sel.trim();
        let first = words.split([' ', ',', ':', '.', '>']).next().unwrap_or("");
        const TAGS: &[&str] = &[
            "html", "body", "h1", "h2", "h3", "h4", "h5", "h6", "p", "a", "img", "nav", "header", "footer", "main", "div", "span",
            "ul", "ol", "li", "button", "input", "textarea", "table", "td", "th", "form", "label", "strong", "em",
        ];
        let css_selector = words.starts_with(['.', '#', '*', ':', '[']) || TAGS.contains(&first);
        let one_line_rule = body.contains(':') && body.contains(';') && body.trim_end().ends_with('}');
        return selector(words) && !words.contains('(') && (one_line_rule || (body.trim().is_empty() && css_selector));
    }
    // `font-family: 'Fraunces', serif;`
    if let Some((prop, value)) = t.strip_suffix(';').and_then(|d| d.split_once(':')) {
        return prop.len() > 1
            && prop.chars().all(|c| c.is_ascii_lowercase() || c == '-')
            && !value.trim().is_empty()
            && !value.contains('=');
    }
    false
}

/// The column of the character at offset `p`.
fn col_of(cs: &[char], p: usize) -> usize {
    p - cs[..p].iter().rposition(|c| *c == '\n').map_or(0, |n| n + 1) + 1
}

pub fn lex(src: &str) -> (Vec<Token>, Vec<Diag>) {
    let cs: Vec<char> = src.chars().collect();
    let mut toks: Vec<Token> = Vec::new();
    let mut errs = Vec::new();
    let (mut i, mut line, mut col) = (0usize, 1usize, 1usize);
    // the open `(` (true) and `{` (false), innermost last: a new line inside `( )` is not the end
    // of a statement, one inside a `{ }` is again
    let mut open: Vec<bool> = Vec::new();

    let ends_line = |toks: &Vec<Token>| matches!(toks.last().map(|t| &t.tok), None | Some(Tok::Newline));

    while i < cs.len() {
        let c = cs[i];
        let span = Span { line, col };
        let next = cs.get(i + 1).copied().unwrap_or('\0');

        if c == '\n' {
            if open.last() != Some(&true) && !ends_line(&toks) {
                toks.push(Token { tok: Tok::Newline, span });
            }
            i += 1;
            line += 1;
            col = 1;
            continue;
        }
        if c.is_whitespace() {
            i += 1;
            col += 1;
            continue;
        }
        // CSS written as code (it belongs in `css("...")`): one error for the rule or block, which
        // is skipped, so its `;`, `#` and `@` are not errors too
        if ends_line(&toks) && open.last() != Some(&true) {
            let end = |k: usize| cs[k..].iter().position(|c| *c == '\n').map_or(cs.len(), |p| k + p);
            let first: String = cs[i..end(i)].iter().collect();
            if css_line(&first) {
                errs.push(Diag::new("E0101", "CSS outside a string", span).hint("CSS goes in `css(\"...\")`"));
                // the lines of the rule, up to the `}` that closes it
                let mut depth = 0i32;
                let mut k = i;
                loop {
                    let e = end(k);
                    let text: String = cs[k..e].iter().collect();
                    depth += text.matches('{').count() as i32 - text.matches('}').count() as i32;
                    if depth <= 0 || e >= cs.len() {
                        i = e;
                        break;
                    }
                    line += 1;
                    k = e + 1;
                }
                continue;
            }
        }
        if c == '/' && next == '/' {
            while i < cs.len() && cs[i] != '\n' {
                i += 1;
                col += 1;
            }
            continue;
        }

        if c.is_ascii_digit() {
            let start = i;
            while i < cs.len() && cs[i].is_ascii_digit() {
                i += 1;
            }
            let mut is_float = false;
            // after a `.` the digits are a tuple position (`t.0.1` is `t.0` then `.1`), never a float
            let position = matches!(toks.last().map(|t| &t.tok), Some(Tok::Dot));
            if !position && i + 1 < cs.len() && cs[i] == '.' && cs[i + 1].is_ascii_digit() {
                is_float = true;
                i += 1;
                while i < cs.len() && cs[i].is_ascii_digit() {
                    i += 1;
                }
            }
            let text: String = cs[start..i].iter().collect();
            col += i - start;
            let tok = if is_float {
                Tok::Float(text.parse().unwrap_or(0.0))
            } else {
                match text.parse::<i64>() {
                    Ok(n) => Tok::Int(n),
                    Err(_) => {
                        let hint = if text == "9223372036854775808" {
                            "the smallest `int` has no literal: write `-9223372036854775807 - 1`".to_string()
                        } else {
                            format!("write a float: `{text}.0`")
                        };
                        errs.push(
                            Diag::new(
                                "E0003",
                                format!("integer `{text}` is too large for `int` (the largest is 9223372036854775807)"),
                                span,
                            )
                            .hint(hint),
                        );
                        Tok::Int(0)
                    }
                }
            };
            toks.push(Token { tok, span });
            continue;
        }

        if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < cs.len() && (cs[i].is_alphanumeric() || cs[i] == '_') {
                i += 1;
            }
            let text: String = cs[start..i].iter().collect();
            col += i - start;
            let tok = match text.as_str() {
                "fn" => Tok::Fn,
                "let" => Tok::Let,
                "var" => Tok::Var,
                "if" => Tok::If,
                "else" => Tok::Else,
                "while" => Tok::While,
                "for" => Tok::For,
                "in" => Tok::In,
                "return" | "ret" => Tok::Ret,
                "true" => Tok::True,
                "false" => Tok::False,
                "struct" => Tok::Struct,
                "inout" => Tok::Inout,
                "break" => Tok::Break,
                "continue" => Tok::Continue,
                "arena" => Tok::Arena,
                "enum" => Tok::Enum,
                "match" => Tok::Match,
                _ => Tok::Ident(text),
            };
            toks.push(Token { tok, span });
            continue;
        }

        if c == '\'' {
            let (tok, len) = char_lit(&cs, i, span, &mut errs);
            toks.push(Token { tok, span });
            i += len;
            col += len;
            continue;
        }

        if c == '"' {
            i += 1;
            col += 1;
            let str_start = i;
            let mut parts: Vec<StrPart> = Vec::new();
            let mut s = String::new();
            let mut closed = false;
            while i < cs.len() && cs[i] != '\n' {
                let ch = cs[i];
                if ch == '"' {
                    i += 1;
                    col += 1;
                    closed = true;
                    break;
                }
                if ch == '\\' {
                    let esc = cs.get(i + 1).copied().unwrap_or('\n');
                    if esc == '\n' {
                        i += 1;
                        col += 1;
                        break;
                    }
                    match esc {
                        'n' => s.push('\n'),
                        't' => s.push('\t'),
                        'r' => s.push('\r'),
                        '\\' => s.push('\\'),
                        '"' => s.push('"'),
                        '0' => s.push('\0'),
                        _ => {
                            let hint = match esc {
                                '{' | '}' => "braces are doubled, not escaped: write `{{` or `}}` for a literal brace".to_string(),
                                '\'' => "a single quote needs no escape: write `'`".to_string(),
                                'u' | 'x' => format!(
                                    "Nyra has no `\\{esc}` escape: type the character itself (strings are UTF-8), e.g. \"\u{e9}\""
                                ),
                                _ => "escapes: `\\n` `\\t` `\\r` `\\0` `\\\\` `\\\"`; a backslash is `\\\\`".to_string(),
                            };
                            // `\{` and `\'` have one meaning: a literal brace, a quote
                            let fixed = match esc {
                                '{' => Some("{{"),
                                '}' => Some("}}"),
                                '\'' => Some("'"),
                                _ => None,
                            };
                            let at = Span { line, col };
                            errs.push(
                                Diag::new("E0004", format!("unknown escape `\\{esc}` in a string"), at)
                                    .hint(hint)
                                    .fix_opt(fixed.map(|f| Edit::replace(at, &format!("\\{esc}"), f))),
                            );
                        }
                    }
                    i += 2;
                    col += 2;
                    continue;
                }
                if (ch == '{' || ch == '}') && cs.get(i + 1) == Some(&ch) {
                    s.push(ch);
                    i += 2;
                    col += 2;
                    continue;
                }
                if ch == '}' {
                    // a `}` that closes nothing is text: `"expected } here"`
                    s.push(ch);
                    i += 1;
                    col += 1;
                    continue;
                }
                if ch == '{' {
                    // interpolation: find the matching `}` on this line
                    let start = i + 1;
                    let mut j = start;
                    let mut depth = 0usize;
                    let mut end = None;
                    while j < cs.len() && cs[j] != '\n' {
                        match cs[j] {
                            // a string or char literal inside the code (`{xs.join(", ")}`): skip it whole,
                            // so its quotes and braces belong to it
                            q @ ('"' | '\'') => {
                                let mut k = j + 1;
                                while k < cs.len() && cs[k] != q && cs[k] != '\n' {
                                    k += if cs[k] == '\\' { 2 } else { 1 };
                                }
                                if k >= cs.len() || cs[k] != q {
                                    break;
                                }
                                j = k;
                            }
                            '{' => depth += 1,
                            '}' if depth == 0 => {
                                end = Some(j);
                                break;
                            }
                            '}' => depth -= 1,
                            _ => {}
                        }
                        j += 1;
                    }
                    // `{x}` inserts x. A `{` that cannot start one is text: it is never closed on the
                    // line (`"fn main() {"`), closes nothing (`"{}"`), or is followed by the string's own
                    // closing quote (`s == "{"`, `"{" + x`)
                    let code: String = end.map(|e| cs[start..e].iter().collect()).unwrap_or_default();
                    // code that starts with a string literal is one only when it is used: `{"q".upper()}`
                    let quoted = |c: &str| {
                        let t = c.trim_start();
                        let Some(rest) = t.strip_prefix('"') else { return false };
                        let after = rest.find('"').map(|k| rest[k + 1..].trim_start());
                        !after.is_some_and(|a| a.starts_with('.') || a.starts_with('['))
                    };
                    match end {
                        Some(e) if !code.trim().is_empty() && !quoted(&code) && !css_block(&code) => {
                            if !s.is_empty() {
                                parts.push(StrPart::Lit(std::mem::take(&mut s)));
                            }
                            parts.push(StrPart::Code(code, Span { line, col: col + 1 }));
                            col += e + 1 - i;
                            i = e + 1;
                        }
                        _ => {
                            s.push('{');
                            i += 1;
                            col += 1;
                        }
                    }
                    continue;
                }
                s.push(ch);
                i += 1;
                col += 1;
            }
            // a string written over several lines (`css("` ... `")`): one error, with the line breaks
            // written as `\n` as the fix, and the lexer goes on after its closing quote
            if let Some(end) = (!closed).then(|| multiline_end(&cs, i)).flatten() {
                let old: String = cs[str_start..end].iter().collect();
                let text = one_line(&old);
                let lines = old.matches('\n').count();
                let close = Span { line: line + lines, col: col_of(&cs, end) };
                errs.push(
                    Diag::new("E0002", format!("a string over {} lines: a string ends on its line", lines + 1), span)
                        .hint("write each line break as `\\n`")
                        .fix(vec![Edit::range(Span { line: span.line, col: span.col + 1 }, close, &old, text)]),
                );
                toks.push(Token { tok: Tok::Str(old), span });
                line = close.line;
                col = close.col + 1;
                i = end + 1;
                continue;
            }
            if !closed {
                errs.push(
                    Diag::new("E0002", "unterminated string: the closing `\"` is missing before the end of the line", span)
                        .hint("close it with `\"` on this line (a line break is `\\n`)"),
                );
            }
            let tok = if parts.is_empty() {
                Tok::Str(s)
            } else {
                if !s.is_empty() {
                    parts.push(StrPart::Lit(s));
                }
                Tok::Interp(parts)
            };
            toks.push(Token { tok, span });
            continue;
        }

        let two = match (c, next) {
            ('-', '>') => Some(Tok::Arrow),
            ('=', '>') => Some(Tok::FatArrow),
            ('.', '.') => Some(Tok::DotDot),
            ('=', '=') => Some(Tok::Eq),
            ('!', '=') => Some(Tok::Ne),
            ('<', '=') => Some(Tok::Le),
            ('>', '=') => Some(Tok::Ge),
            ('?', '?') => Some(Tok::Coalesce),
            ('&', '&') => Some(Tok::And),
            ('|', '|') => Some(Tok::Or),
            ('+', '=') => Some(Tok::OpAssign(BinOp::Add)),
            ('-', '=') => Some(Tok::OpAssign(BinOp::Sub)),
            ('*', '=') => Some(Tok::OpAssign(BinOp::Mul)),
            ('/', '=') => Some(Tok::OpAssign(BinOp::Div)),
            ('%', '=') => Some(Tok::OpAssign(BinOp::Mod)),
            _ => None,
        };
        if let Some(tok) = two {
            toks.push(Token { tok, span });
            i += 2;
            col += 2;
            continue;
        }

        // a single `.` is a token (fields and methods), except in the float typos `.5` and `5.`
        if c == '.' {
            // (not in `t.1.0`: after a `.` the digits are a tuple position)
            let after_digit = i > 0
                && cs[i - 1].is_ascii_digit()
                && matches!(toks.last().map(|t| &t.tok), Some(Tok::Int(_)))
                && !(toks.len() >= 2 && toks[toks.len() - 2].tok == Tok::Dot);
            let before_digit =
                next.is_ascii_digit() && !(i > 0 && (cs[i - 1].is_alphanumeric() || matches!(cs[i - 1], '_' | ')' | ']')));
            if !after_digit && !before_digit {
                toks.push(Token { tok: Tok::Dot, span });
                i += 1;
                col += 1;
                continue;
            }
        }

        let one = match c {
            '(' => Some(Tok::LParen),
            ')' => Some(Tok::RParen),
            '{' => Some(Tok::LBrace),
            '}' => Some(Tok::RBrace),
            '[' => Some(Tok::LBracket),
            ']' => Some(Tok::RBracket),
            ',' => Some(Tok::Comma),
            ':' => Some(Tok::Colon),
            '?' => Some(Tok::Question),
            '+' => Some(Tok::Plus),
            '-' => Some(Tok::Minus),
            '*' => Some(Tok::Star),
            '/' => Some(Tok::Slash),
            '%' => Some(Tok::Percent),
            '=' => Some(Tok::Assign),
            '<' => Some(Tok::Lt),
            '>' => Some(Tok::Gt),
            '!' => Some(Tok::Not),
            _ => None,
        };
        match one {
            Some(tok) => {
                match tok {
                    Tok::LParen => open.push(true),
                    Tok::LBrace => open.push(false),
                    // a `)` or `}` that closes nothing is the parser's to report
                    Tok::RParen if open.last() == Some(&true) => {
                        open.pop();
                    }
                    Tok::RBrace if open.last() == Some(&false) => {
                        open.pop();
                    }
                    _ => {}
                }
                toks.push(Token { tok, span });
            }
            None if c == ';' => errs.push(semicolon(&cs, i, span, usize::from(open.last() == Some(&true)))),
            None if c == '#' && hash_comment(&cs, i) => {
                // a comment of another language: one error for the `#`, and the rest of the line is skipped
                errs.push(
                    Diag::new("E0001", "unexpected character `#`", span)
                        .hint(hints::bad_char('#'))
                        .fix(vec![Edit::replace(span, "#", "//")]),
                );
                while i < cs.len() && cs[i] != '\n' {
                    i += 1;
                    col += 1;
                }
                continue;
            }
            None if c == '|' && closure_params(&cs, i).is_some() => {
                // `|x| x * 2` (Rust, Ruby): one error for the parameter list, which is skipped
                let (len, params) = closure_params(&cs, i).unwrap_or_default();
                let old: String = cs[i..i + len].iter().collect();
                let new = if params.len() == 1 { format!("{} =>", params[0]) } else { format!("({}) =>", params.join(", ")) };
                errs.push(
                    Diag::new("E0001", "unexpected character `|`: a lambda is written `x => x * 2`", span)
                        .hint(format!("write `{new}` instead of `{old}`: the parameters, `=>`, then the body"))
                        .fix(vec![Edit::replace(span, &old, new)]),
                );
                i += len;
                col += len;
                continue;
            }
            None => {
                // a quote from another language: report the whole literal once, not each quote
                if let Some(close) = hints::quote_close(c) {
                    let line_rest = cs[i + 1..].iter().take_while(|x| **x != '\n');
                    if let Some(k) = line_rest.clone().position(|x| *x == close) {
                        let text: String = cs[i + 1..i + 1 + k].iter().collect();
                        let msg = if c == '`' {
                            "unexpected character '`' (a backtick)".to_string()
                        } else {
                            format!("unexpected character `{c}`")
                        };
                        let old: String = cs[i..i + k + 2].iter().collect();
                        let fix = hints::quoted_fix(c, &text).map(|new| Edit::replace(span, &old, new));
                        errs.push(Diag::new("E0001", msg, span).hint(hints::quoted_text(c, &text)).fix_opt(fix));
                        i += k + 2;
                        col += k + 2;
                        continue;
                    }
                }
                errs.push(bad_char(c, &cs, i, span));
            }
        }
        i += 1;
        col += 1;
    }

    let end = Span { line, col };
    if !ends_line(&toks) {
        toks.push(Token { tok: Tok::Newline, span: end });
    }
    toks.push(Token { tok: Tok::Eof, span: end });
    (continue_lines(toks), errs)
}

/// A line that starts with a binary operator or `.` continues the line before it, and so does a
/// line after one that ends with a binary operator: no statement can start or end with one.
///     let det = a * x
///             - b * y
///     let n = xs.filter(x => x > 0)
///               .len()
fn continue_lines(toks: Vec<Token>) -> Vec<Token> {
    let binary = |t: &Tok| {
        matches!(
            t,
            Tok::Plus
                | Tok::Star
                | Tok::Slash
                | Tok::Percent
                | Tok::Eq
                | Tok::Ne
                | Tok::Lt
                | Tok::Le
                | Tok::Gt
                | Tok::Ge
                | Tok::And
                | Tok::Or
                | Tok::Question
        )
    };
    // `-` too: a statement cannot start with a negation. Not `++` or `--` (`i--`, `--i`), which
    // the parser explains.
    let op = |t: Option<&Token>, after: Option<&Token>| {
        t.is_some_and(|t| (binary(&t.tok) || t.tok == Tok::Minus) && after.is_none_or(|a| a.tok != t.tok))
    };
    let mut out: Vec<Token> = Vec::with_capacity(toks.len());
    for (i, t) in toks.iter().enumerate() {
        if t.tok == Tok::Newline && i > 0 {
            // (a line may also start with the `:` of a ternary)
            let starts =
                op(toks.get(i + 1), toks.get(i + 2)) || toks.get(i + 1).is_some_and(|n| matches!(n.tok, Tok::Dot | Tok::Colon));
            let ends = op(toks.get(i - 1), i.checked_sub(2).map(|k| &toks[k]));
            if starts || ends {
                continue;
            }
        }
        out.push(t.clone());
        // `print("no") ret` in a one-line block: `ret`, `break` and `continue` end a line anyway
        if !matches!(t.tok, Tok::Newline | Tok::LBrace | Tok::FatArrow)
            && matches!(toks.get(i + 1).map(|n| &n.tok), Some(Tok::Ret | Tok::Break | Tok::Continue))
            && toks.get(i + 1).is_some_and(|n| n.span.line == t.span.line)
        {
            out.push(Token { tok: Tok::Newline, span: toks[i + 1].span });
        }
    }
    out
}

/// `|a, b|` at `cs[i]`: the length of the parameter list of a closure (Rust, Ruby) and its names.
fn closure_params(cs: &[char], i: usize) -> Option<(usize, Vec<String>)> {
    // only where a value starts (`map(|x| ...`, `f(a, |x| ...`, `= |x| ...`), not in `a | b | c`
    let before = cs[..i].iter().rev().find(|c| **c != ' ' && **c != '\t');
    if !matches!(before, Some('(' | ',' | '=')) {
        return None;
    }
    let close = cs[i + 1..].iter().take_while(|c| **c != '\n').position(|c| *c == '|')? + i + 1;
    let inside: String = cs[i + 1..close].iter().collect();
    let names: Vec<String> = inside.split(',').map(|n| n.trim().to_string()).collect();
    let word = |n: &String| {
        n.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_') && n.chars().all(|c| c.is_alphanumeric() || c == '_')
    };
    names.iter().all(word).then_some((close + 1 - i, names))
}

/// E0001 for the character at `cs[i]`, with a hint about what was probably meant.
fn bad_char(c: char, cs: &[char], i: usize, span: Span) -> Diag {
    let before = i.checked_sub(1).map(|j| cs[j]);
    let after = cs.get(i + 1).copied();
    if let (true, Some(ty)) = (c == '?', optional_type(cs, i)) {
        return Diag::new("E0262", format!("`{ty}?` is an optional type: Nyra has no optional values and no null"), span)
            .hint(hints::OPTION_HINT);
    }
    let msg = match hints::invisible_char(c) {
        Some(name) => format!("unexpected invisible character U+{:04X} ({name})", c as u32),
        None if c == '`' => "unexpected character '`' (a backtick)".to_string(),
        None => format!("unexpected character `{c}`"),
    };
    let (hint, fix) = if c == '.' && after.is_some_and(|a| a.is_ascii_digit()) {
        let digits: String = cs[i + 1..].iter().take_while(|d| d.is_ascii_digit()).collect();
        // `1...5` is a range typo, not a float
        let fix = (before != Some('.')).then(|| Edit::replace(span, ".", "0."));
        (format!("a float needs digits on both sides of the dot: write `0.{digits}`"), fix)
    } else if c == '.' && before.is_some_and(|b| b.is_ascii_digit()) {
        let mut start = i;
        while start > 0 && cs[start - 1].is_ascii_digit() {
            start -= 1;
        }
        let digits: String = cs[start..i].iter().collect();
        // `5.len()` is not a float either
        let fix = after.is_none_or(|a| !(a.is_alphanumeric() || a == '_' || a == '.')).then(|| Edit::replace(span, ".", ".0"));
        (format!("a float needs digits on both sides of the dot: write `{digits}.0`"), fix)
    } else {
        (hints::bad_char(c), hints::char_fix(c).map(|to| Edit::replace(span, &c.to_string(), to)))
    };
    Diag::new("E0001", msg, span).hint(hint).fix_opt(fix)
}

/// The type written right before the `?` at `cs[i]` (`int` of `int?`, `[str]` of `[str]?`), if it is one:
/// a type word of any language, a name that starts uppercase, or an array type.
fn optional_type(cs: &[char], i: usize) -> Option<String> {
    let word_char = |c: char| c.is_alphanumeric() || c == '_';
    let mut start = i;
    if cs.get(i.checked_sub(1)?) == Some(&']') {
        // `[int]?`: back to the `[` that opens it
        let mut depth = 0usize;
        while start > 0 {
            start -= 1;
            match cs[start] {
                ']' => depth += 1,
                '[' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(cs[start..i].iter().collect());
                    }
                }
                '\n' => return None,
                _ => {}
            }
        }
        return None;
    }
    while start > 0 && word_char(cs[start - 1]) {
        start -= 1;
    }
    let w: String = cs[start..i].iter().collect();
    let first = w.chars().next()?;
    (hints::is_type_word(&w) || hints::nyra_type(&w).is_some() || first.is_uppercase()).then_some(w)
}

/// E0005 for the `;` at `cs[i]`. At the end of a line (or before a `}`) it is deleted; between two
/// statements on one line it becomes a line break. Inside `( )` or in the header of a `for`, `while`
/// or `if` (`for i = 0; i < n; i++ {`) there is no fix: that line needs rewriting, not a deletion.
fn semicolon(cs: &[char], i: usize, span: Span, paren_depth: usize) -> Diag {
    let d = Diag::new("E0005", "unexpected `;`: Nyra has no semicolons", span).hint("a new line ends a statement");
    let rest: String = cs[i + 1..].iter().take_while(|c| **c != '\n').collect();
    let after = rest.trim_start();
    let line_start = cs[..i].iter().rposition(|c| *c == '\n').map_or(0, |p| p + 1);
    let line: String = cs[line_start..i].iter().collect();
    if after.is_empty() || after.starts_with("//") || after.starts_with('}') || after.starts_with(';') {
        // a `;` alone on its line goes with the line; otherwise with the spaces before it
        let whole_line = line.trim().is_empty() && after.is_empty() && i + 1 + rest.chars().count() < cs.len();
        let edit = if whole_line {
            Edit::range(Span { line: span.line, col: 1 }, Span { line: span.line + 1, col: 1 }, ";", "")
        } else {
            let spaces = line.chars().rev().take_while(|c| *c == ' ' || *c == '\t').count();
            Edit::range(Span { line: span.line, col: span.col - spaces }, Span { line: span.line, col: span.col + 1 }, ";", "")
        };
        return d.fix(vec![edit]);
    }
    let indent: String = line.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
    let header = ["for", "while", "if", "} else if"]
        .iter()
        .any(|k| line.trim_start().strip_prefix(k).is_some_and(|r| r.starts_with([' ', '('])));
    if paren_depth > 0 || header {
        return d;
    }
    let spaces = rest.chars().count() - after.chars().count();
    let end = Span { line: span.line, col: span.col + 1 + spaces };
    d.fix(vec![Edit::range(span, end, ";", format!("\n{indent}"))])
}

/// True if the `#` at `cs[i]` starts a comment of another language (`# note`, `#note` at the start
/// of a line, `## title`, a `#!` first line), as opposed to `#[derive]`, `#include` or a `#` inside
/// an expression.
fn hash_comment(cs: &[char], i: usize) -> bool {
    let line_start = cs[..i].iter().rposition(|c| *c == '\n').map_or(0, |p| p + 1);
    let first_on_line = cs[line_start..i].iter().all(|c| *c == ' ' || *c == '\t');
    let next = cs.get(i + 1).copied().unwrap_or('\n');
    if first_on_line {
        let word: String = cs[i + 1..].iter().take_while(|c| c.is_ascii_alphabetic()).collect();
        let directive =
            ["include", "define", "import", "pragma", "if", "ifdef", "ifndef", "endif", "else", "undef"].contains(&word.as_str());
        let shebang = next == '!' && i == 0;
        return shebang || !(directive || next == '[' || next == '!');
    }
    cs[i - 1].is_whitespace() && (next.is_whitespace() || next == '#')
}

/// A char literal as it is written in source: `'a'`, `'\n'`, `'\''`.
pub fn char_literal(c: u32) -> String {
    match char::from_u32(c) {
        Some('\n') => r"'\n'".into(),
        Some('\t') => r"'\t'".into(),
        Some('\r') => r"'\r'".into(),
        Some('\\') => r"'\\'".into(),
        Some('\'') => r"'\''".into(),
        Some(ch) => format!("'{ch}'"),
        None => format!("char({c})"),
    }
}

/// The char literal starting at `cs[i]`: its token and how many characters it spans. Exactly one
/// character (or one escape) must be between the quotes, else E0007.
fn char_lit(cs: &[char], i: usize, span: Span, errs: &mut Vec<Diag>) -> (Tok, usize) {
    let mut j = i + 1;
    let mut chars: Vec<char> = Vec::new();
    let mut closed = false;
    while j < cs.len() && cs[j] != '\n' {
        match cs[j] {
            '\'' => {
                closed = true;
                j += 1;
                break;
            }
            '\\' => {
                let esc = cs.get(j + 1).copied().unwrap_or('\n');
                let ch = match esc {
                    'n' => '\n',
                    't' => '\t',
                    'r' => '\r',
                    '\\' => '\\',
                    '"' => '"',
                    '\'' => '\'',
                    '0' => '\0',
                    '\n' => break,
                    _ => {
                        let at = Span { line: span.line, col: span.col + (j - i) };
                        errs.push(
                            Diag::new("E0004", format!(r"unknown escape `\{esc}` in a character"), at)
                                .hint(r#"the escapes are `\n` `\t` `\r` `\0` `\\` `\"` and `\'`"#),
                        );
                        esc
                    }
                };
                chars.push(ch);
                j += 2;
            }
            ch => {
                chars.push(ch);
                j += 1;
            }
        }
    }
    let len = j - i;
    let text: String = chars.iter().collect();
    if !closed {
        errs.push(
            Diag::new("E0007", "unterminated character: the closing `'` is missing", span)
                .hint(r#"a character is one letter in single quotes, `'a'`; text uses double quotes: "like this""#),
        );
    } else if chars.len() != 1 {
        let (msg, hint) = if chars.is_empty() {
            (
                "empty character `''`: a character literal holds exactly one character".to_string(),
                r#"write a character between the quotes, e.g. `' '` for a space; empty text is `""`"#.to_string(),
            )
        } else {
            (
                format!("a character literal holds exactly one character, but `'{text}'` has {}", chars.len()),
                if text.contains('"') || text.contains('\\') {
                    r#"text uses double quotes: "like this""#.to_string()
                } else {
                    format!("text uses double quotes: \"{text}\"")
                },
            )
        };
        // `'hello'` is text; with braces it would become interpolation, so those are left alone
        let raw: String = cs[i + 1..j - 1].iter().collect();
        let fix = (chars.len() > 1 && !raw.contains(['"', '\\', '{', '}']))
            .then(|| Edit::replace(span, &format!("'{raw}'"), format!("\"{raw}\"")));
        errs.push(Diag::new("E0007", msg, span).hint(hint).fix_opt(fix));
    }
    let value = chars.first().map_or(0, |c| *c as u32);
    (Tok::Char(value), len)
}
