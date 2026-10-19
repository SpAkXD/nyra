//! Names in a buffer, found from its tokens so that a file with errors still has them: every
//! declaration (`let`, `var`, parameters, loop variables, lambda parameters and pattern names)
//! with the part of the file where it can be used, and the function each position is in.

use crate::ast::Span;
use crate::edit;
use crate::lexer::{self, StrPart, Tok, Token};

use super::text::Lines;

/// The tokens of a text, and those of the code inside `{ }` of its strings, in order. Line
/// breaks and the end are dropped.
pub fn tokens(src: &str) -> Vec<Token> {
    let (toks, _) = lexer::lex(src);
    let mut out = Vec::with_capacity(toks.len());
    for t in toks {
        match &t.tok {
            Tok::Newline | Tok::Eof => {}
            Tok::Interp(parts) => {
                let inner: Vec<Token> = parts
                    .iter()
                    .filter_map(|p| match p {
                        StrPart::Code(code, at) => Some((code, *at)),
                        StrPart::Lit(_) => None,
                    })
                    .flat_map(|(code, at)| {
                        let (ts, _) = lexer::lex(code);
                        ts.into_iter().filter(|t| !matches!(t.tok, Tok::Newline | Tok::Eof)).map(move |mut t| {
                            t.span = if t.span.line == 1 {
                                Span { line: at.line, col: at.col + t.span.col - 1 }
                            } else {
                                Span { line: at.line + t.span.line - 1, col: t.span.col }
                            };
                            t
                        })
                    })
                    .collect();
                out.push(t);
                out.extend(inner);
            }
            _ => out.push(t),
        }
    }
    out
}

fn before(a: Span, b: Span) -> bool {
    (a.line, a.col) < (b.line, b.col)
}

/// How a name was declared.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Let,
    Var,
    Param,
    /// `inout x: T`
    Inout,
    Loop,
    /// a lambda's parameter or a name of a `match` pattern
    Bound,
}

impl Kind {
    pub fn word(self) -> &'static str {
        match self {
            Kind::Let => "let",
            Kind::Var => "var",
            Kind::Param => "param",
            Kind::Inout => "inout param",
            Kind::Loop => "loop variable",
            Kind::Bound => "bound",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Decl {
    pub name: String,
    pub span: Span,
    pub kind: Kind,
    /// Where it can no longer be used (exclusive).
    pub end: Span,
    /// Declared at the top level of a script (outside every definition): a script variable that
    /// every function sees.
    pub global: bool,
    /// The function it belongs to (`None`: the script).
    pub func: Option<String>,
}

/// A definition of the file and the lines it covers.
pub struct Def {
    pub item: edit::Item,
    pub start: Span,
    pub end: Span,
    /// Where the name is.
    pub name_at: Span,
}

/// The names of one buffer.
pub struct Scopes {
    pub toks: Vec<Token>,
    pub defs: Vec<Def>,
    pub decls: Vec<Decl>,
}

impl Scopes {
    pub fn new(src: &str) -> Scopes {
        let toks = tokens(src);
        let lines = Lines::new(src);
        let outline = edit::outline(src);
        let defs: Vec<Def> = outline
            .items
            .into_iter()
            .map(|item| {
                let start = lines.span_of(item.start);
                let end = lines.span_of(item.end);
                // the name: the first name token after `fn` / `struct` / `enum`
                let name_at = toks
                    .iter()
                    .find(|t| !before(t.span, start) && matches!(&t.tok, Tok::Ident(n) if *n == item.name))
                    .map_or(start, |t| t.span);
                Def { item, start, end, name_at }
            })
            .collect();
        let mut s = Scopes { toks, defs, decls: Vec::new() };
        s.decls = s.find_decls();
        s
    }

    /// The function definition a position is in.
    pub fn def_at(&self, at: Span) -> Option<&Def> {
        self.defs.iter().find(|d| !before(at, d.start) && before(at, d.end) || at == d.end)
    }

    /// The index of the token at a position (also one that ends right there).
    pub fn token_at(&self, at: Span) -> Option<usize> {
        let mut found = None;
        for (i, t) in self.toks.iter().enumerate() {
            if t.span.line != at.line {
                if t.span.line > at.line {
                    break;
                }
                continue;
            }
            let len = tok_len(&t.tok);
            if t.span.col <= at.col && at.col < t.span.col + len.max(1) {
                return Some(i);
            }
            if t.span.col + len == at.col && matches!(t.tok, Tok::Ident(_)) {
                found = Some(i);
            }
        }
        found
    }

    /// The declaration that `name` at `at` means: a local of its function that is in scope, a
    /// parameter, or a script variable.
    pub fn resolve(&self, name: &str, at: Span) -> Option<&Decl> {
        let func = self.def_at(at).map(|d| d.item.name.clone());
        let in_scope = |d: &&Decl| d.name == name && !before(at, d.span) && (before(at, d.end) || d.span == at);
        let local = self.decls.iter().filter(|d| d.func == func).filter(in_scope).max_by_key(|d| (d.span.line, d.span.col));
        local.or_else(|| self.decls.iter().find(|d| d.global && d.name == name))
    }

    /// The names that can be used at `at`: locals in scope, parameters, script variables.
    pub fn visible(&self, at: Span) -> Vec<&Decl> {
        let func = self.def_at(at).map(|d| d.item.name.clone());
        let mut out: Vec<&Decl> = Vec::new();
        for d in &self.decls {
            let local = d.func == func && !before(at, d.span) && before(at, d.end);
            if (local || d.global) && !out.iter().any(|o| o.name == d.name) {
                out.push(d);
            }
        }
        out
    }

    fn find_decls(&self) -> Vec<Decl> {
        let toks = &self.toks;
        let n = toks.len();
        // the matching bracket of each bracket, and the innermost open bracket around each token
        let mut close = vec![n; n];
        let mut around: Vec<Option<usize>> = vec![None; n];
        let mut stack: Vec<usize> = Vec::new();
        for (i, t) in toks.iter().enumerate() {
            around[i] = stack.last().copied();
            match t.tok {
                Tok::LParen | Tok::LBracket | Tok::LBrace => stack.push(i),
                Tok::RParen | Tok::RBracket | Tok::RBrace => {
                    if let Some(o) = stack.pop() {
                        close[o] = i;
                        around[i] = stack.last().copied();
                    }
                }
                _ => {}
            }
        }
        let file_end = toks.last().map_or(Span { line: 1, col: 1 }, |t| Span { line: t.span.line + 1, col: 1 });
        let end_of = |i: usize| if i >= n { file_end } else { toks[i].span };
        // the innermost `{` around token i
        let brace_around = |mut i: usize| -> Option<usize> {
            while let Some(o) = around[i] {
                if toks[o].tok == Tok::LBrace {
                    return Some(o);
                }
                i = o;
            }
            None
        };
        let mut out = Vec::new();
        let ident = |i: usize| match toks.get(i).map(|t| &t.tok) {
            Some(Tok::Ident(s)) if s != "_" => Some(s.clone()),
            _ => None,
        };
        // the names in the group `( ... )` that starts at token o
        let group = |o: usize| -> Vec<(String, Span)> {
            (o + 1..close[o].min(n))
                .filter(|&k| matches!(toks.get(k + 1).map(|t| &t.tok), Some(Tok::Comma | Tok::RParen)))
                .filter_map(|k| ident(k).map(|s| (s, toks[k].span)))
                .collect()
        };
        // the definition of each token, found in one pass (both are in source order)
        let mut owner: Vec<Option<usize>> = vec![None; n];
        let mut d = 0;
        for (i, t) in toks.iter().enumerate() {
            while d < self.defs.len() && !before(t.span, self.defs[d].end) && t.span != self.defs[d].end {
                d += 1;
            }
            if d < self.defs.len() && !before(t.span, self.defs[d].start) {
                owner[i] = Some(d);
            }
        }
        for i in 0..n {
            let def = owner[i].map(|d| &self.defs[d]);
            let func = def.map(|d| d.item.name.clone());
            let in_def = def.is_some();
            let mut push = |name: String, span: Span, kind: Kind, end: Span, global: bool| {
                out.push(Decl { name, span, kind, end, global, func: func.clone() });
            };
            match &toks[i].tok {
                Tok::Let | Tok::Var => {
                    let kind = if toks[i].tok == Tok::Var { Kind::Var } else { Kind::Let };
                    let block = brace_around(i);
                    let end = block.map_or(file_end, |b| end_of(close[b]));
                    let global = !in_def && block.is_none();
                    if let Some(name) = ident(i + 1) {
                        push(name, toks[i + 1].span, kind, end, global);
                    } else if toks.get(i + 1).is_some_and(|t| t.tok == Tok::LParen) {
                        for (name, span) in group(i + 1) {
                            push(name, span, kind, end, global);
                        }
                    }
                }
                Tok::For => {
                    // the loop's body, or the end of the `[ ]` of a comprehension
                    let mut names = Vec::new();
                    let mut k = i + 1;
                    while k < n && !matches!(toks[k].tok, Tok::In | Tok::LBrace) {
                        if toks[k].tok == Tok::LParen {
                            names.extend(group(k));
                            k = close[k];
                        } else if let Some(name) = ident(k) {
                            names.push((name, toks[k].span));
                        }
                        k += 1;
                    }
                    let in_brackets = around[i].is_some_and(|o| toks[o].tok == Tok::LBracket);
                    let end = if in_brackets {
                        end_of(close[around[i].unwrap_or(0)])
                    } else {
                        let mut b = k;
                        let mut depth = 0usize;
                        while b < n {
                            match toks[b].tok {
                                Tok::LParen | Tok::LBracket => depth += 1,
                                Tok::RParen | Tok::RBracket => depth = depth.saturating_sub(1),
                                Tok::LBrace if depth == 0 => break,
                                _ => {}
                            }
                            b += 1;
                        }
                        if b < n {
                            end_of(close[b])
                        } else {
                            file_end
                        }
                    };
                    for (name, span) in names {
                        push(name, span, Kind::Loop, end, false);
                    }
                }
                Tok::Fn => {
                    // the parameters: `name: T`, `inout name: T`, `var name: T`
                    let Some(open) = (i + 1..n.min(i + 4)).find(|&k| toks[k].tok == Tok::LParen) else { continue };
                    let end = def.map_or(file_end, |d| d.end);
                    let mut k = open + 1;
                    while k < close[open].min(n) {
                        if around[k] == Some(open) && toks.get(k + 1).is_some_and(|t| t.tok == Tok::Colon) {
                            if let Some(name) = ident(k) {
                                let kind = if k > 0 && toks[k - 1].tok == Tok::Inout { Kind::Inout } else { Kind::Param };
                                push(name, toks[k].span, kind, end, false);
                            }
                        }
                        k += 1;
                    }
                }
                Tok::FatArrow if i > 0 => {
                    let end = around[i].map_or(file_end, |o| end_of(close[o]));
                    match &toks[i - 1].tok {
                        Tok::Ident(name) if name != "_" && (i < 2 || toks[i - 2].tok != Tok::Dot) => {
                            push(name.clone(), toks[i - 1].span, Kind::Bound, end, false);
                        }
                        Tok::RParen => {
                            if let Some(o) = (0..i - 1).rev().find(|&o| close[o] == i - 1) {
                                for (name, span) in group(o) {
                                    push(name, span, Kind::Bound, end, false);
                                }
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        out
    }
}

/// How many characters a token takes (names and keywords; other tokens their spelling).
pub fn tok_len(t: &Tok) -> usize {
    match t {
        Tok::Ident(s) => s.chars().count(),
        Tok::Int(n) => n.to_string().len(),
        t => t.text().chars().count().max(1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sp(line: usize, col: usize) -> Span {
        Span { line, col }
    }

    #[test]
    fn finds_declarations_and_their_scopes() {
        let src = "let g = 1\nfn f(a: int, inout b: [int]) -> int {\n    var t = a\n    for i, x in b {\n        t += x\n    }\n    let ys = b.map(v => v * 2)\n    return t\n}\nprint(\"{g}\")\n";
        let s = Scopes::new(src);
        let names: Vec<(&str, Kind, bool)> = s.decls.iter().map(|d| (d.name.as_str(), d.kind, d.global)).collect();
        assert_eq!(
            names,
            vec![
                ("g", Kind::Let, true),
                ("a", Kind::Param, false),
                ("b", Kind::Inout, false),
                ("t", Kind::Var, false),
                ("i", Kind::Loop, false),
                ("x", Kind::Loop, false),
                ("ys", Kind::Let, false),
                ("v", Kind::Bound, false),
            ]
        );
        // `x` is visible in the loop only
        assert_eq!(s.resolve("x", sp(5, 14)).map(|d| d.span), Some(sp(4, 12)));
        assert!(s.resolve("x", sp(8, 12)).is_none());
        assert_eq!(s.resolve("a", sp(3, 13)).map(|d| d.kind), Some(Kind::Param));
        assert_eq!(s.resolve("g", sp(3, 13)).map(|d| d.span), Some(sp(1, 5)));
        let at_return: Vec<&str> = s.visible(sp(8, 12)).iter().map(|d| d.name.as_str()).collect();
        assert_eq!(at_return, vec!["g", "a", "b", "t", "ys"]);
        // the code inside `{ }` of a string has tokens too
        let g = s.token_at(sp(10, 9)).unwrap();
        assert_eq!(s.toks[g].tok, Tok::Ident("g".into()));
    }
}
