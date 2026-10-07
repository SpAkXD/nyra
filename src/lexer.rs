//! Turns source text into tokens. Newlines end statements, except inside `( )`.

use crate::ast::{BinOp, Span};
use crate::diag::Diag;
use crate::hints;

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Int(i64),
    Float(f64),
    Str(String),
    /// A string containing `{expr}` parts.
    Interp(Vec<StrPart>),
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
    // punctuation
    LParen,
    RParen,
    LBrace,
    RBrace,
    Comma,
    Colon,
    Arrow,
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
            Tok::Fn | Tok::Let | Tok::Var | Tok::If | Tok::Else | Tok::While | Tok::For | Tok::In | Tok::Ret
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
            Tok::Ret => "ret",
            Tok::True => "true",
            Tok::False => "false",
            Tok::LParen => "(",
            Tok::RParen => ")",
            Tok::LBrace => "{",
            Tok::RBrace => "}",
            Tok::Comma => ",",
            Tok::Colon => ":",
            Tok::Arrow => "->",
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

pub fn lex(src: &str) -> (Vec<Token>, Vec<Diag>) {
    let cs: Vec<char> = src.chars().collect();
    let mut toks: Vec<Token> = Vec::new();
    let mut errs = Vec::new();
    let (mut i, mut line, mut col) = (0usize, 1usize, 1usize);
    let mut paren_depth = 0usize;

    let ends_line = |toks: &Vec<Token>| matches!(toks.last().map(|t| &t.tok), None | Some(Tok::Newline));

    while i < cs.len() {
        let c = cs[i];
        let span = Span { line, col };
        let next = cs.get(i + 1).copied().unwrap_or('\0');

        if c == '\n' {
            if paren_depth == 0 && !ends_line(&toks) {
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
            if i + 1 < cs.len() && cs[i] == '.' && cs[i + 1].is_ascii_digit() {
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
                            "the smallest `int` has no literal: write `-9223372036854775807 - 1`; the largest is 9223372036854775807"
                                .to_string()
                        } else {
                            format!("keep `int` values within 9223372036854775807, or write a float: `{text}.0`")
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
                "ret" => Tok::Ret,
                "true" => Tok::True,
                "false" => Tok::False,
                _ => Tok::Ident(text),
            };
            toks.push(Token { tok, span });
            continue;
        }

        if c == '"' {
            i += 1;
            col += 1;
            let mut parts: Vec<StrPart> = Vec::new();
            let mut s = String::new();
            let mut closed = false;
            while i < cs.len() && cs[i] != '\n' {
                let ch = cs[i];
                let here = Span { line, col };
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
                        _ => {
                            let hint = match esc {
                                '{' | '}' => "braces are doubled, not escaped: write `{{` or `}}` for a literal brace".to_string(),
                                '\'' => "a single quote needs no escape: write `'`".to_string(),
                                'u' | 'x' | '0' => format!(
                                    "Nyra has no `\\{esc}` escape: type the character itself (strings are UTF-8), e.g. \"\u{e9}\""
                                ),
                                _ => format!(
                                    "the escapes are `\\n` `\\t` `\\r` `\\\\` and `\\\"`; to write a literal backslash, double it: `\\\\{esc}`"
                                ),
                            };
                            errs.push(
                                Diag::new("E0004", format!("unknown escape `\\{esc}` in a string"), Span { line, col })
                                    .hint(hint),
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
                    errs.push(
                        Diag::new("E0006", "unmatched `}` in a string: there is no `{` for it to close", here)
                            .hint("write `}}` to print a literal `}`"),
                    );
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
                    while j < cs.len() && cs[j] != '\n' && cs[j] != '"' {
                        match cs[j] {
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
                    match end {
                        Some(e) => {
                            let code: String = cs[start..e].iter().collect();
                            if code.trim().is_empty() {
                                errs.push(
                                    Diag::new("E0006", "empty `{}` in a string: there is no expression to insert", here)
                                        .hint("put an expression between the braces, e.g. `{x}`, or write `{{}}` to print literal braces"),
                                );
                            } else {
                                if !s.is_empty() {
                                    parts.push(StrPart::Lit(std::mem::take(&mut s)));
                                }
                                parts.push(StrPart::Code(code, Span { line, col: col + 1 }));
                            }
                            col += e + 1 - i;
                            i = e + 1;
                        }
                        None => {
                            // a quote before any `}`: either a quote inside `{ }` (if a `}` follows
                            // later on the line) or a `{` that is never closed
                            let close_later = cs[j..].iter().take_while(|c| **c != '\n').position(|c| *c == '}');
                            match close_later {
                                Some(k) if cs.get(j) == Some(&'"') => {
                                    errs.push(
                                        Diag::new("E0006", "quotes are not allowed inside `{ }` in a string", here)
                                            .hint("put the text in a variable first (`let t = \"x\"`), then write `{t}` in the string"),
                                    );
                                    // skip the whole `{ ... }`, so its quotes do not start new strings
                                    let e = j + k;
                                    col += e + 1 - i;
                                    i = e + 1;
                                }
                                _ => {
                                    errs.push(
                                        Diag::new("E0006", "unclosed `{` in a string: no matching `}` before the end of the string", here)
                                            .hint("add the closing `}`, as in `{x}`, or write `{{` to print a literal `{`"),
                                    );
                                    col += j - i;
                                    i = j;
                                }
                            }
                        }
                    }
                    continue;
                }
                s.push(ch);
                i += 1;
                col += 1;
            }
            if !closed {
                errs.push(
                    Diag::new("E0002", "unterminated string: the closing `\"` is missing before the end of the line", span)
                        .hint("add `\"` at the end of the text; a string cannot continue on the next line (write `\\n` for a line break)"),
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
            ('.', '.') => Some(Tok::DotDot),
            ('=', '=') => Some(Tok::Eq),
            ('!', '=') => Some(Tok::Ne),
            ('<', '=') => Some(Tok::Le),
            ('>', '=') => Some(Tok::Ge),
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

        let one = match c {
            '(' => Some(Tok::LParen),
            ')' => Some(Tok::RParen),
            '{' => Some(Tok::LBrace),
            '}' => Some(Tok::RBrace),
            ',' => Some(Tok::Comma),
            ':' => Some(Tok::Colon),
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
                if tok == Tok::LParen {
                    paren_depth += 1;
                } else if tok == Tok::RParen {
                    paren_depth = paren_depth.saturating_sub(1);
                }
                toks.push(Token { tok, span });
            }
            None if c == ';' => errs.push(
                Diag::new("E0005", "unexpected `;`: Nyra has no semicolons", span)
                    .hint("delete the `;`: a new line already ends a statement, so put each statement on its own line"),
            ),
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
                        errs.push(Diag::new("E0001", msg, span).hint(hints::quoted_text(c, &text)));
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
    (toks, errs)
}

/// E0001 for the character at `cs[i]`, with a hint about what was probably meant.
fn bad_char(c: char, cs: &[char], i: usize, span: Span) -> Diag {
    let before = i.checked_sub(1).map(|j| cs[j]);
    let after = cs.get(i + 1).copied();
    let msg = match hints::invisible_char(c) {
        Some(name) => format!("unexpected invisible character U+{:04X} ({name})", c as u32),
        None if c == '`' => "unexpected character '`' (a backtick)".to_string(),
        None => format!("unexpected character `{c}`"),
    };
    let hint = if c == '.' && after.is_some_and(|a| a.is_ascii_digit()) {
        let digits: String = cs[i + 1..].iter().take_while(|d| d.is_ascii_digit()).collect();
        format!("a float needs digits on both sides of the dot: write `0.{digits}`")
    } else if c == '.' && before.is_some_and(|b| b.is_ascii_digit()) {
        let mut start = i;
        while start > 0 && cs[start - 1].is_ascii_digit() {
            start -= 1;
        }
        let digits: String = cs[start..i].iter().collect();
        format!("a float needs digits on both sides of the dot: write `{digits}.0`")
    } else if c == '.' {
        // `console.log(x)`, `Math.sqrt(x)`, `s.len()`: name the Nyra way when the word before the dot is known
        let mut start = i;
        while start > 0 && (cs[start - 1].is_alphanumeric() || cs[start - 1] == '_') {
            start -= 1;
        }
        let word: String = cs[start..i].iter().collect();
        match word.as_str() {
            "console" => "print with `print(x)`: it takes one value and ends the line".to_string(),
            "Math" => "Nyra has no `Math`: write the function you need yourself (see docs/AI_GUIDE.md section 6)".to_string(),
            _ => hints::bad_char(c),
        }
    } else {
        hints::bad_char(c)
    };
    Diag::new("E0001", msg, span).hint(hint)
}
