//! Turns source text into tokens. Newlines end statements, except inside `( )`.

use crate::ast::{BinOp, Span};
use crate::diag::Diag;

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Int(i64),
    Float(f64),
    Str(String),
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
    pub fn describe(&self) -> String {
        match self {
            Tok::Int(n) => format!("number `{n}`"),
            Tok::Float(f) => format!("number `{f}`"),
            Tok::Str(_) => "a string".into(),
            Tok::Ident(s) => format!("`{s}`"),
            Tok::Newline => "end of line".into(),
            Tok::Eof => "end of file".into(),
            Tok::OpAssign(op) => format!("`{}=`", op.symbol()),
            other => format!("`{}`", other.text()),
        }
    }

    fn text(&self) -> &'static str {
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
                        errs.push(
                            Diag::new("E0003", format!("number `{text}` is too large for `int`"), span)
                                .hint("int is 64-bit: the max is 9223372036854775807"),
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
                        _ => errs.push(
                            Diag::new("E0004", format!("unknown escape `\\{esc}`"), Span { line, col })
                                .hint("valid escapes: \\n \\t \\r \\\\ \\\""),
                        ),
                    }
                    i += 2;
                    col += 2;
                    continue;
                }
                s.push(ch);
                i += 1;
                col += 1;
            }
            if !closed {
                errs.push(
                    Diag::new("E0002", "unterminated string", span)
                        .hint("close the string with `\"` on the same line"),
                );
            }
            toks.push(Token { tok: Tok::Str(s), span });
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
                Diag::new("E0005", "Nyra has no semicolons", span)
                    .hint("remove the `;` and put each statement on its own line"),
            ),
            None => errs.push(Diag::new("E0001", format!("unexpected character `{c}`"), span)),
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
