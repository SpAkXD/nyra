//! Recursive-descent parser with precedence climbing for expressions.
//! On an error it records a diagnostic and skips to the next statement, so one
//! run reports as many errors as possible. Every error says what was expected,
//! what was found and (through `hints`) what to write instead.

use crate::ast::*;
use crate::diag::{after, suggest_fix, Diag, Edit};
use crate::hints;
use crate::lexer::{self, StrPart, Tok, Token};

type PResult<T> = Result<T, Diag>;

const FORMAT_SPEC_HINT: &str = "format specifiers like `{x:.2f}` do not exist: a float always prints in its shortest form";

pub fn parse(toks: Vec<Token>) -> (Program, Vec<Diag>) {
    // every `struct Name` of the file, so a type can name a struct that is declared further down
    let structs: Vec<String> = toks
        .windows(2)
        .filter_map(|w| match (&w[0].tok, &w[1].tok) {
            (Tok::Struct, Tok::Ident(name)) => Some(name.clone()),
            _ => None,
        })
        .collect();
    let mut p = Parser { toks, structs, pos: 0, errs: Vec::new(), in_string: false, cut_blocks: 0, loop_depth: 0 };
    let prog = p.program();
    (prog, p.errs)
}

struct Parser {
    toks: Vec<Token>,
    /// The names of the structs the file declares.
    structs: Vec<String>,
    pos: usize,
    errs: Vec<Diag>,
    /// True while parsing the code inside `{ }` of a string: the end of that code is the closing `}`.
    in_string: bool,
    /// Blocks that were cut short by a `fn`. The `}` that is left over for each of them at the
    /// top level was already accounted for by that error and is not reported again.
    cut_blocks: usize,
    /// How many loops enclose the current statement (`break`/`continue` need one).
    loop_depth: usize,
}

impl Parser {
    fn peek(&self) -> &Tok {
        &self.toks[self.pos].tok
    }

    fn peek_at(&self, n: usize) -> &Tok {
        let i = (self.pos + n).min(self.toks.len() - 1);
        &self.toks[i].tok
    }

    fn span(&self) -> Span {
        self.toks[self.pos].span
    }

    fn bump(&mut self) -> Token {
        let t = self.toks[self.pos].clone();
        if self.pos < self.toks.len() - 1 {
            self.pos += 1;
        }
        t
    }

    fn at(&self, t: &Tok) -> bool {
        self.peek() == t
    }

    fn skip_newlines(&mut self) {
        while self.at(&Tok::Newline) {
            self.bump();
        }
    }

    fn prev(&self) -> Option<&Token> {
        self.pos.checked_sub(1).map(|i| &self.toks[i])
    }

    /// The last token before the current one that is not a line break.
    fn prev_nonl(&self) -> Option<&Token> {
        self.toks[..self.pos].iter().rev().find(|t| t.tok != Tok::Newline)
    }

    /// True if the previous token is `tok` and the current one follows it with no space between.
    fn glued_to(&self, tok: &Tok) -> bool {
        self.prev().is_some_and(|p| {
            p.tok == *tok && p.span.line == self.span().line && p.span.col + p.tok.text().len() == self.span().col
        })
    }

    /// The current token as it is named in "found ...". Inside the `{ }` of a string, the end
    /// of the code is the closing `}`.
    fn found(&self) -> String {
        match self.peek() {
            Tok::Newline | Tok::Eof if self.in_string => "`}`".to_string(),
            t => t.describe(),
        }
    }

    /// `expected X, found Y` at the current token. The hint is filled in when what was found is a
    /// known mistake (`return`, `and`, `=` instead of `==`, `0xFF`, ...); callers may add others.
    fn unexpected(&self, expected: &str) -> Diag {
        let d = Diag::new("E0101", format!("expected {expected}, found {}", self.found()), self.span());
        match self.found_hint() {
            Some(h) => d.hint(h).fix(self.found_fix().unwrap_or_default()),
            None => d,
        }
    }

    /// `not done`: the word `not` was read as a name, and the value after it is unexpected.
    fn after_not(&self) -> Option<Span> {
        let p = self.prev()?;
        let starts_value = matches!(
            self.peek(),
            Tok::Ident(_) | Tok::Int(_) | Tok::Float(_) | Tok::Str(_) | Tok::Interp(_) | Tok::Char(_) | Tok::True | Tok::False
                | Tok::LParen | Tok::Not
        );
        (p.tok == Tok::Ident("not".into()) && p.span.line == self.span().line && starts_value).then_some(p.span)
    }

    fn found_hint(&self) -> Option<String> {
        if self.after_not().is_some() {
            return hints::word("not");
        }
        match self.peek() {
            Tok::Ident(w) => self.number_suffix(w).map(|(h, _)| h).or_else(|| hints::word(w)),
            Tok::Assign if self.arrow_ahead() => Some(
                "`=>` does not exist: Nyra has no lambdas or closures; define a named function, e.g. `fn double(x: int) -> int = x * 2`"
                    .into(),
            ),
            Tok::Assign if self.glued_to(&Tok::Eq) || self.glued_to(&Tok::Ne) => {
                Some("`===` and `!==` do not exist: compare with `==` and `!=`".into())
            }
            Tok::Assign => Some("`=` assigns to a variable on a line of its own (`x = 1`); to compare two values write `==`".into()),
            Tok::Slash if matches!(self.peek_at(1), Tok::Star) => {
                Some("Nyra has no block comments: start every comment line with `//`".into())
            }
            Tok::LBrace => self.struct_literal_hint(),
            _ => None,
        }
    }

    /// The fix that goes with `found_hint`, where there is exactly one way to repair the mistake.
    fn found_fix(&self) -> Option<Vec<Edit>> {
        let here = self.span();
        if let Some(not) = self.after_not() {
            return Some(vec![Edit::range(not, here, "not", "!")]);
        }
        let edit = match self.peek() {
            Tok::Ident(w) => match self.number_suffix(w) {
                Some((_, value)) => {
                    let prev = self.prev()?;
                    Edit::replace(prev.span, &format!("{}{w}", source_text(&prev.tok)?), value?)
                }
                None => Edit::replace(here, w, word_fix(w)?),
            },
            // `a === b`: the third `=` goes
            Tok::Assign if !self.arrow_ahead() && (self.glued_to(&Tok::Eq) || self.glued_to(&Tok::Ne)) => Edit::replace(here, "=", ""),
            Tok::LBrace => return self.struct_literal_fix(),
            _ => return None,
        };
        Some(vec![edit])
    }

    /// `Point { x: 1, y: 2 }` as `Point(x: 1, y: 2)`, for a declared struct whose braces hold
    /// `name: value` pairs separated by commas (new lines only after `{` or a comma, or before `}`).
    fn struct_literal_fix(&self) -> Option<Vec<Edit>> {
        let name_tok = self.prev()?;
        let Tok::Ident(name) = &name_tok.tok else { return None };
        if !self.structs.contains(name) {
            return None;
        }
        let mut depth = 0usize;
        let mut close = None;
        let mut expect_field = true;
        for i in self.pos + 1..self.toks.len() {
            let t = &self.toks[i].tok;
            match t {
                Tok::LBrace => return None,
                Tok::RBrace if depth == 0 => {
                    close = Some(i);
                    break;
                }
                Tok::LParen | Tok::LBracket => depth += 1,
                Tok::RParen | Tok::RBracket | Tok::RBrace => depth = depth.checked_sub(1)?,
                Tok::Eof => return None,
                _ => {}
            }
            if depth > 0 || matches!(t, Tok::LParen | Tok::LBracket) {
                continue;
            }
            match t {
                Tok::Newline => {
                    let next_closes = self.toks[i + 1..].iter().find(|t| t.tok != Tok::Newline).is_some_and(|t| t.tok == Tok::RBrace);
                    if !(expect_field || next_closes) {
                        return None;
                    }
                }
                Tok::Comma => expect_field = true,
                Tok::Ident(_) if expect_field => {
                    if self.toks.get(i + 1)?.tok != Tok::Colon {
                        return None;
                    }
                    expect_field = false;
                }
                _ if expect_field => return None,
                _ => {}
            }
        }
        let close = close?;
        let first = self.toks[self.pos + 1..close].iter().find(|t| t.tok != Tok::Newline)?;
        let last = self.toks[self.pos + 1..close].iter().rev().find(|t| t.tok != Tok::Newline)?;
        let open = Edit::range(after(name_tok.span, name), first.span, "{", "(");
        let rbrace = self.toks[close].span;
        let shut = match source_text(&last.tok) {
            Some(t) => Edit::range(last.span, after(rbrace, "}"), &format!("{t}}}"), format!("{t})")),
            None => Edit::replace(rbrace, "}", ")"),
        };
        Some(vec![open, shut])
    }

    /// `Point { x: 1, y: 2 }`: a struct is built like a call. The hint shows the call with the field
    /// names that were written between the braces.
    fn struct_literal_hint(&self) -> Option<String> {
        let Tok::Ident(name) = &self.prev()?.tok else { return None };
        if !name.starts_with(|c: char| c.is_uppercase()) {
            return None;
        }
        let mut fields: Vec<String> = Vec::new();
        let mut depth = 0usize;
        for i in self.pos..self.toks.len() {
            match &self.toks[i].tok {
                Tok::LBrace | Tok::LParen | Tok::LBracket => depth += 1,
                Tok::RBrace | Tok::RParen | Tok::RBracket => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        break;
                    }
                }
                Tok::Ident(f) if depth == 1 && self.toks.get(i + 1).is_some_and(|t| t.tok == Tok::Colon) => {
                    fields.push(format!("{f}: ..."));
                }
                Tok::Eof => break,
                _ => {}
            }
        }
        let call = if fields.is_empty() { format!("{name}(field: value, ...)") } else { format!("{name}({})", fields.join(", ")) };
        Some(format!("a struct is built like a call, with every field named: `{call}`"))
    }

    /// True if the current token is an `=` that is directly followed by `>`: a `=>` arrow.
    fn arrow_ahead(&self) -> bool {
        let next = self.toks.get(self.pos + 1);
        self.at(&Tok::Assign)
            && next.is_some_and(|n| n.tok == Tok::Gt && n.span.line == self.span().line && n.span.col == self.span().col + 1)
    }

    /// `0xFF`, `1_000`, `1e5` and friends lex as a number followed by a word: say what to write,
    /// and the number to write instead when there is one.
    fn number_suffix(&self, word: &str) -> Option<(String, Option<String>)> {
        let prev = self.prev()?;
        let (text, whole) = match prev.tok {
            Tok::Int(n) => (n.to_string(), Some(n)),
            Tok::Float(f) => (format!("{f:?}"), None),
            _ => return None,
        };
        let cur = self.span();
        if prev.span.line != cur.line || prev.span.col + text.len() != cur.col {
            return None;
        }
        let first = word.chars().next()?;
        let rest = &word[first.len_utf8()..];
        let digits = |radix: u32| !rest.is_empty() && rest.chars().all(|c| c.is_digit(radix));
        match (first, whole) {
            ('x' | 'X', Some(0)) if digits(16) => i64::from_str_radix(rest, 16)
                .ok()
                .map(|v| (format!("hexadecimal literals do not exist: write the decimal value `{v}`"), Some(v.to_string()))),
            ('b' | 'B', Some(0)) if digits(2) => i64::from_str_radix(rest, 2)
                .ok()
                .map(|v| (format!("binary literals do not exist: write the decimal value `{v}`"), Some(v.to_string()))),
            ('o' | 'O', Some(0)) if digits(8) => i64::from_str_radix(rest, 8)
                .ok()
                .map(|v| (format!("octal literals do not exist: write the decimal value `{v}`"), Some(v.to_string()))),
            ('_', _) if rest.chars().all(|c| c.is_ascii_digit() || c == '_') && rest.contains(|c: char| c.is_ascii_digit()) => {
                let joined = format!("{text}{}", rest.replace('_', ""));
                Some((format!("digit separators do not exist: write `{joined}`"), Some(joined)))
            }
            ('e' | 'E', _) => {
                let exp = if rest.is_empty() {
                    // `1e-5` / `1e+5`: the sign and the digits are separate tokens
                    match (self.peek_at(1), self.peek_at(2)) {
                        (Tok::Plus, Tok::Int(k)) => Some(*k),
                        (Tok::Minus, Tok::Int(k)) => Some(-*k),
                        _ => None,
                    }
                } else if digits(10) {
                    rest.parse::<i64>().ok()
                } else {
                    None
                }?;
                let value = text.parse::<f64>().ok()? * 10f64.powi(exp as i32);
                let shown = format!("{value:?}");
                if (0..=15).contains(&exp) && !shown.contains('e') {
                    // `1e+5`: the sign and the digits are tokens of their own, so only `1e5` gets a fix
                    let fix = (!rest.is_empty()).then(|| shown.clone());
                    Some((format!("exponent notation does not exist: write the number in full, `{shown}`"), fix))
                } else {
                    Some(("exponent notation does not exist: write the number in full, with a dot (`0.00001`, `100000.0`)".into(), None))
                }
            }
            _ => None,
        }
    }

    fn expect(&mut self, t: Tok, expected: &str) -> PResult<Span> {
        if self.at(&t) {
            Ok(self.bump().span)
        } else {
            Err(self.unexpected(expected))
        }
    }

    /// A name. A keyword is described as such, since "expected a variable name, found keyword `in`"
    /// says exactly why `in` does not work.
    fn ident(&mut self, what: &str, hint: &str) -> PResult<(String, Span)> {
        if let Tok::Ident(name) = self.peek().clone() {
            return Ok((name, self.bump().span));
        }
        let t = self.peek().clone();
        let hint = if t.is_keyword() || matches!(t, Tok::True | Tok::False) {
            format!("`{}` is a reserved word and cannot be used as a name: pick another, e.g. `{}_value`", t.text(), t.text())
        } else {
            hint.to_string()
        };
        Err(self.unexpected(what).hint(hint))
    }

    // ---- top level -------------------------------------------------------

    fn program(&mut self) -> Program {
        let mut funcs = Vec::new();
        let mut structs = Vec::new();
        loop {
            self.skip_newlines();
            match self.peek() {
                Tok::Eof => break,
                Tok::Fn => match self.func() {
                    Ok(f) => funcs.push(f),
                    Err(d) => {
                        self.errs.push(d);
                        self.sync_top();
                    }
                },
                Tok::Struct => match self.struct_def() {
                    Ok(sd) => structs.push(sd),
                    Err(d) => {
                        self.errs.push(d);
                        self.sync_top();
                    }
                },
                // the `}` of a block that a nested `fn` cut short: that error already covers it
                Tok::RBrace if self.cut_blocks > 0 => {
                    self.cut_blocks -= 1;
                    self.bump();
                }
                _ => {
                    let d = self.top_level_error();
                    self.errs.push(d);
                    self.sync_top();
                }
            }
        }
        Program { funcs, structs }
    }

    /// Something other than `fn` at the top level of the file.
    fn top_level_error(&self) -> Diag {
        let d = self.unexpected("`fn` or `struct`");
        let name_after = |n: usize| match self.peek_at(n) {
            Tok::Ident(w) => Some(w.clone()),
            _ => None,
        };
        let hint = match self.peek() {
            Tok::Let | Tok::Var => {
                let name = name_after(1).unwrap_or_else(|| "name".into());
                format!("there are no global variables: a constant is a function, e.g. `fn {name}() -> int = 10`")
            }
            Tok::RBrace => "this `}` closes nothing: remove it, or add the `{` it belongs to".to_string(),
            Tok::Ident(w) => {
                if let (true, Some(fname), Tok::LParen) = (hints::is_type_word(w), name_after(1), self.peek_at(2)) {
                    // C-style `int main() {`
                    if w == "void" {
                        format!("functions start with `fn`: `fn {fname}(...) {{ ... }}` (no `->` part when nothing is returned)")
                    } else {
                        let ty = hints::nyra_type(w).unwrap_or(w);
                        format!("functions start with `fn` and the return type comes after `->`: `fn {fname}(...) -> {ty} {{ ... }}`")
                    }
                } else if let Some(h) = hints::top_level_word(w) {
                    // `def f(`, `function f(`: the keyword of another language
                    if ["function", "func", "fun", "def"].contains(&w.as_str())
                        && name_after(1).is_some()
                        && matches!(self.peek_at(2), Tok::LParen)
                    {
                        return d.hint(h).fix(vec![Edit::replace(self.span(), w, "fn")]);
                    }
                    h
                } else if w == "main" && matches!(self.peek_at(1), Tok::LParen) {
                    "`main` runs by itself when the program starts: define it with `fn main() { ... }` and do not call it".to_string()
                } else if matches!(self.peek_at(1), Tok::LParen) {
                    format!("`{w}(...)` is a call, and calls go inside a function: `fn main() {{ {w}(...) }}`")
                } else {
                    return d.or_hint("a file may only contain `fn` definitions: put statements inside `fn main() { ... }`");
                }
            }
            _ => return d.or_hint("a file may only contain `fn` definitions: put statements inside `fn main() { ... }`"),
        };
        d.hint(hint)
    }

    fn sync_top(&mut self) {
        if !matches!(self.peek(), Tok::Fn | Tok::Struct) {
            self.bump();
        }
        while !matches!(self.peek(), Tok::Fn | Tok::Struct | Tok::Eof) {
            self.bump();
        }
    }

    /// `struct Name { field: type, ... }` (fields separated by commas or new lines)
    fn struct_def(&mut self) -> PResult<StructDef> {
        self.expect(Tok::Struct, "`struct`")?;
        let (name, span) = self.ident("a struct name", "name the struct after `struct`: `struct Point { x: int, y: int }`")?;
        self.expect(Tok::LBrace, &format!("`{{` after `struct {name}`"))
            .map_err(|d| d.or_hint(format!("a struct lists its fields in braces: `struct {name} {{ x: int, y: int }}`")))?;
        let mut fields = Vec::new();
        loop {
            self.skip_newlines();
            match self.peek() {
                Tok::RBrace => {
                    self.bump();
                    break;
                }
                Tok::Eof | Tok::Fn | Tok::Struct => {
                    return Err(self
                        .unexpected(&format!("`}}` to close `struct {name}`"))
                        .hint("add the missing `}` after the last field"))
                }
                _ => {}
            }
            let (fname, fspan) = self.ident("a field name", "fields look like `x: int`, separated by commas or new lines")?;
            if !self.at(&Tok::Colon) {
                return Err(self.unexpected("`:` and a type").or_hint(format!("every field needs a type: `{fname}: int`")));
            }
            self.bump();
            let ty = self.ty(&format!("write the field's type after the colon: `{fname}: int`"))?;
            fields.push(Field { name: fname, ty, span: fspan });
            match self.peek() {
                Tok::Comma | Tok::Newline => {
                    self.bump();
                }
                Tok::RBrace => {}
                _ => {
                    return Err(self
                        .unexpected("`,`, a new line or `}` after a field")
                        .or_hint(format!("separate the fields with commas or new lines: `struct {name} {{ x: int, y: int }}`")))
                }
            }
        }
        Ok(StructDef { name, fields, span })
    }

    fn func(&mut self) -> PResult<Func> {
        self.expect(Tok::Fn, "`fn`")?;
        let (name, span) = self.ident("a function name", "name the function after `fn`: `fn main() { ... }`")?;
        self.expect(Tok::LParen, "`(`")
            .map_err(|d| d.or_hint(format!("the parameter list follows the name: `fn {name}(a: int) {{ ... }}`")))?;
        let mut params = Vec::new();
        while !self.at(&Tok::RParen) {
            let inout = self.at(&Tok::Inout);
            if inout {
                self.bump();
            }
            let (pname, pspan) = self.ident("a parameter name", "parameters look like `a: int, b: float`")?;
            if !self.at(&Tok::Colon) {
                return Err(self.missing_param_type(&pname, pspan));
            }
            self.bump();
            let ty = self.ty(&format!("every parameter needs a type: `{pname}: int`"))?;
            params.push(Param { name: pname, ty, inout, span: pspan });
            if !self.at(&Tok::RParen) {
                let hint = if matches!(self.peek(), Tok::LBrace | Tok::Arrow | Tok::Assign | Tok::Eof | Tok::Newline) {
                    "close the parameter list with `)`"
                } else {
                    "separate parameters with commas: `fn f(a: int, b: int)`"
                };
                let expected = format!("`,` or `)` in the parameters of `{name}`");
                self.expect(Tok::Comma, &expected).map_err(|d| d.or_hint(hint))?;
            }
        }
        self.bump();
        let ret = if self.at(&Tok::Arrow) {
            self.bump();
            self.ty("write the return type after `->`, e.g. `fn f() -> int`")?
        } else {
            Type::Void
        };
        let body = if self.at(&Tok::Assign) {
            // one-line function: the expression is the body (and the return value)
            self.bump();
            let e = self.expr().map_err(|d| self.one_line_fn_error(d))?;
            self.end_stmt_after(
                None,
                Some("a one-line function is a single expression: use a block `{ ... }` for several statements"),
            )?;
            let espan = e.span;
            let kind = if ret == Type::Void { StmtKind::Expr(e) } else { StmtKind::Ret(Some(e)) };
            vec![Stmt { kind, span: espan }]
        } else {
            self.block(&format!("fn {name}"))?
        };
        Ok(Func { name, params, ret, body, span })
    }

    /// `int a` (C) or `a int` (no colon) in a parameter list.
    fn missing_param_type(&self, pname: &str, pspan: Span) -> Diag {
        let d = self.unexpected("`:` and a type");
        match self.peek() {
            Tok::Ident(second) if hints::is_type_word(pname) => d
                .hint(format!("write the name first, then the type: `{second}: {pname}`"))
                .fix(vec![Edit::range(pspan, after(self.span(), second), &format!("{pname}{second}"), format!("{second}: {pname}"))]),
            Tok::Ident(second) if hints::is_type_word(second) => d
                .hint(format!("put a colon between the name and the type: `{pname}: {second}`"))
                .fix(vec![Edit::replace(pspan, pname, format!("{pname}:"))]),
            _ => d.or_hint(format!("every parameter needs a type: `{pname}: int`")),
        }
    }

    /// The expression of a one-line function went wrong right where it starts.
    fn one_line_fn_error(&self, d: Diag) -> Diag {
        if d.code != "E0101" {
            return d;
        }
        match self.peek() {
            Tok::Newline => d.hint("the expression must start on the same line as `=`: `fn f() -> int = 1`"),
            Tok::Ret => d.hint("a one-line function returns its expression itself, so there is no `ret`: `fn f(x: int) -> int = x`"),
            Tok::LBrace => d.hint("`=` is followed by an expression, not a block: for a block write `fn f() { ... }` without the `=`"),
            Tok::Gt if self.glued_to(&Tok::Assign) => {
                d.hint("`=>` does not exist: a one-line function is written `fn f(x: int) -> int = x * 2`")
            }
            _ => d,
        }
    }

    fn ty(&mut self, hint: &str) -> PResult<Type> {
        let span = self.span();
        if self.at(&Tok::LBracket) {
            self.bump();
            let elem = self.ty("an array type names the type of its elements: `[int]`")?;
            if self.at(&Tok::Colon) {
                return Err(Diag::new("E0102", "map types like `[str: int]` are not in Nyra yet", span).hint(
                    "maps come in the next version: for now use an array of structs, e.g. `[Entry]` with `struct Entry { key: str, value: int }`",
                ));
            }
            self.expect(Tok::RBracket, "`]` to close the array type").map_err(|d| d.or_hint("an array type is written `[int]`"))?;
            return Ok(Type::array(elem));
        }
        let (name, name_span) = self.ident("a type", hint)?;
        match name.as_str() {
            "int" => Ok(Type::Int),
            "float" => Ok(Type::Float),
            "bool" => Ok(Type::Bool),
            "str" => Ok(Type::Str),
            "char" => Ok(Type::Char),
            // a struct (the checker reports names that are not defined, and struct names that start lowercase)
            n if (n.starts_with(|c: char| c.is_uppercase()) && hints::nyra_type(n).is_none())
                || self.structs.iter().any(|s| s == n) =>
            {
                Ok(Type::structure(n))
            }
            _ => {
                // `point` where `struct Point` is declared: the closest struct, else what other languages call the type
                let (hint, fix) = match suggest_fix(&name, self.structs.iter().map(String::as_str)) {
                    Some((h, f)) => (h, f.map(str::to_string)),
                    None => (hints::type_name(&name), hints::type_fix(&name).map(str::to_string)),
                };
                Err(Diag::new(
                    "E0102",
                    format!("unknown type `{name}`: expected `int`, `float`, `bool`, `str`, `char`, an array `[T]` or a struct"),
                    span,
                )
                .hint(hint)
                .fix_opt(fix.map(|t| Edit::replace(name_span, &name, t))))
            }
        }
    }

    // ---- statements ------------------------------------------------------

    /// The `{` of a block. `what` says whose body it is (`fn main`, `if`, `else`, `while`, `for`).
    /// A `{` alone on the next line is reported once and then parsed anyway, so it does not
    /// cause a second error.
    fn open_brace(&mut self, what: &str) -> PResult<Span> {
        if self.at(&Tok::LBrace) {
            return Ok(self.bump().span);
        }
        let is_fn = what.starts_with("fn ");
        let expected = if what == "else" {
            "`{` or `if` after `else`".to_string()
        } else if is_fn {
            format!("`{{` or `=` to start the body of `{what}`")
        } else {
            format!("`{{` to start the body of this `{what}`")
        };
        let example = match what {
            "else" => "} else {".to_string(),
            "if" => "if x > 0 {".to_string(),
            "while" => "while n > 0 {".to_string(),
            "for" => "for i in 0..10 {".to_string(),
            "arena" => "arena {".to_string(),
            w => format!("{w}(...) {{"),
        };
        let allman = matches!(self.peek(), Tok::Newline) && matches!(self.peek_at(1), Tok::LBrace);
        let here = self.span();
        let specific = match self.peek() {
            Tok::Newline if allman => {
                // join the lines: the token before the line break, a space, then the `{`
                let fix = self.prev().and_then(|p| {
                    let t = source_text(&p.tok)?;
                    Some(Edit::range(p.span, self.toks[self.pos + 1].span, &t, format!("{t} ")))
                });
                Some((format!("put the `{{` on the same line as the header: `{example}`"), fix))
            }
            Tok::Newline if is_fn => Some((
                format!("a function body is missing: write `{example} ... }}`, or `= expression` for a one-line function"),
                None,
            )),
            Tok::Newline => Some((format!("a body in braces is missing: `{example} ... }}`"), None)),
            Tok::Colon if is_fn => {
                // `fn f(a: int): int {`
                let typed = matches!(self.peek_at(1), Tok::LBracket) || matches!(self.peek_at(1), Tok::Ident(_));
                let fix = typed.then(|| Edit::replace(here, ":", " ->"));
                Some(("the return type is written after an arrow: `fn f(a: int) -> int {`".to_string(), fix))
            }
            Tok::Ident(t) if is_fn && hints::is_type_word(t) => {
                let fix = (t != "void").then(|| Edit::replace(here, t, format!("-> {t}")));
                Some((format!("the return type is written after an arrow: `fn f() -> {t} {{`"), fix))
            }
            _ => None,
        };
        let d = self.unexpected(&expected);
        let d = match specific {
            Some((h, fix)) => d.hint(h).fix_opt(fix),
            None => {
                // `if x = 1 {`: there is no assignment in a condition, so it is a comparison
                let compare = matches!(what, "if" | "while")
                    && self.at(&Tok::Assign)
                    && !self.arrow_ahead()
                    && !self.glued_to(&Tok::Eq)
                    && !self.glued_to(&Tok::Ne)
                    && !matches!(self.peek_at(1), Tok::Assign);
                let d = d.or_hint(format!("braces are required, even around one statement: `{example} ... }}`"));
                if compare {
                    d.fix(vec![Edit::replace(here, "=", "==")])
                } else {
                    d
                }
            }
        };
        if allman {
            self.errs.push(d);
            self.bump();
            return Ok(self.bump().span);
        }
        Err(d)
    }

    fn block(&mut self, what: &str) -> PResult<Vec<Stmt>> {
        let open = self.open_brace(what)?;
        let mut stmts = Vec::new();
        loop {
            self.skip_newlines();
            match self.peek() {
                Tok::RBrace => {
                    self.bump();
                    return Ok(stmts);
                }
                Tok::Eof | Tok::Fn | Tok::Struct => return Err(self.unclosed_block(open, what)),
                _ => match self.stmt() {
                    Ok(s) => stmts.push(s),
                    Err(d) => {
                        self.errs.push(d);
                        self.sync_stmt();
                    }
                },
            }
        }
    }

    /// The file (or the next `fn`) came before the `}` of a block.
    fn unclosed_block(&mut self, open: Span, what: &str) -> Diag {
        let nested = self.at(&Tok::Fn);
        if nested {
            self.cut_blocks += 1;
        }
        let d = self.unexpected(&format!("`}}` to close the `{{` of `{what}` opened at {}:{}", open.line, open.col));
        d.hint(if nested {
            "functions cannot be nested: add the missing `}` before this `fn`, or move the inner function to the top level"
        } else {
            "add the missing `}`: every `{` needs a matching `}`"
        })
    }

    fn sync_stmt(&mut self) {
        let mut depth = 0usize;
        loop {
            match self.peek() {
                Tok::Eof => return,
                // a `fn` that starts a line is the next function; one in the middle of a line
                // (`let f = fn(x) x`) belongs to the broken statement
                Tok::Fn | Tok::Struct if self.prev().is_some_and(|p| p.tok == Tok::Newline) => return,
                Tok::LBrace => depth += 1,
                Tok::RBrace => {
                    if depth == 0 {
                        return;
                    }
                    depth -= 1;
                }
                Tok::Newline if depth == 0 => {
                    self.bump();
                    return;
                }
                _ => {}
            }
            self.bump();
        }
    }

    fn end_stmt(&mut self) -> PResult<()> {
        self.end_stmt_after(None, None)
    }

    /// The end of a statement. `first` is the name the statement started with, if it started with
    /// one; `generic` replaces the general "one statement per line" hint.
    fn end_stmt_after(&mut self, first: Option<(&str, Span)>, generic: Option<&str>) -> PResult<()> {
        match self.peek() {
            Tok::Newline => {
                self.bump();
                Ok(())
            }
            Tok::RBrace | Tok::Eof => Ok(()),
            _ => Err(self.after_statement_error(first, generic)),
        }
    }

    fn after_statement_error(&self, first: Option<(&str, Span)>, generic: Option<&str>) -> Diag {
        let mut d = self.unexpected("end of line");
        if let Some((w, at)) = first {
            let word_hint = match (w, self.peek()) {
                ("const" | "final" | "static" | "val", Tok::Ident(name)) => Some(format!(
                    "write `let {name} = ...` for a value that never changes, or `var {name} = ...` for one that does"
                )),
                _ => hints::word(w),
            };
            if let Some(h) = word_hint {
                // `return x`, `elif x == 1`: the first word is the real mistake
                d.msg = format!("`{w}` is not part of Nyra: expected end of line after it, found {}", self.found());
                // `const x = 1` is a `let`: it never changes (`static` means other things in other languages)
                let declares = matches!(self.peek(), Tok::Ident(_)) && matches!(self.peek_at(1), Tok::Assign | Tok::Colon);
                let to = match w {
                    "return" => Some("ret"),
                    "elif" | "elsif" | "elseif" => Some("else if"),
                    "const" | "final" | "val" if declares => Some("let"),
                    _ => None,
                };
                return d.hint(h).fix_opt(to.map(|t| Edit::replace(at, w, t)));
            }
        }
        let first = first.map(|(w, _)| w);
        let t = self.peek();
        let after_cast = self.pos >= 3
            && self.toks[self.pos - 1].tok == Tok::RParen
            && self.toks[self.pos - 3].tok == Tok::LParen
            && matches!(&self.toks[self.pos - 2].tok, Tok::Ident(w) if hints::nyra_type(w).is_some());
        let hint = match (t, first) {
            _ if after_cast => {
                let Tok::Ident(w) = &self.toks[self.pos - 2].tok else { unreachable!() };
                let ty = hints::nyra_type(w).unwrap_or("float");
                format!("Nyra has no casts: convert with a function call, e.g. `{ty}(x)`")
            }
            (Tok::Let | Tok::Var | Tok::If | Tok::While | Tok::For | Tok::Ret, _) => {
                format!("put `{}` on a new line: Nyra has one statement per line", t.text())
            }
            (Tok::Colon, _) if matches!(self.peek_at(1), Tok::Colon) => {
                "`::` paths do not exist: Nyra has no modules or namespaces, so call every function by its plain name".to_string()
            }
            (Tok::Colon, Some(f)) if matches!(self.peek_at(1), Tok::Assign) => {
                format!("`:=` does not exist: declare a variable with `let {f} = ...` (or `var {f} = ...` to change it later)")
            }
            (Tok::Colon, Some(_)) => "to declare a variable write `let x: int = 5` (or `var`)".to_string(),
            (Tok::RParen, _) => "this `)` closes nothing: remove it, or add the `(` it belongs to".to_string(),
            (Tok::Ident(name), Some(f)) if hints::nyra_type(f).is_some() => format!(
                "declare variables with `let` or `var`, and put the type after the name: `let {name}: {} = ...` (or just `let {name} = ...`)",
                hints::nyra_type(f).unwrap_or("int")
            ),
            (Tok::Int(_) | Tok::Float(_) | Tok::Str(_) | Tok::Ident(_) | Tok::True | Tok::False, Some(f)) => {
                let arg = match t {
                    Tok::Int(n) => n.to_string(),
                    Tok::Float(x) => format!("{x:?}"),
                    Tok::Str(s) if s.len() <= 20 && !s.contains(['"', '\\', '{', '}']) => format!("\"{s}\""),
                    Tok::Ident(x) => x.clone(),
                    Tok::True => "true".to_string(),
                    Tok::False => "false".to_string(),
                    _ => "...".to_string(),
                };
                format!("to call `{f}` put the argument in parentheses: `{f}({arg})`; to start a new statement, begin a new line")
            }
            _ => match generic {
                Some(g) => g.to_string(),
                None => format!("one statement per line: start {} on a new line", t.describe()),
            },
        };
        if d.hint.is_some() {
            return d;
        }
        // `print "hi"`: a value alone on a line does nothing, so it is the argument
        let fix = match (first, self.prev()) {
            (Some("print"), Some(p))
                if matches!(t, Tok::Int(_) | Tok::Float(_) | Tok::Str(_) | Tok::Ident(_) | Tok::True | Tok::False)
                    && matches!(self.peek_at(1), Tok::Newline | Tok::RBrace | Tok::Eof)
                    && !after_cast =>
            {
                source_text(t).filter(|_| p.span.line == self.span().line).map(|arg| {
                    vec![
                        Edit::range(after(p.span, "print"), self.span(), "", "("),
                        Edit::replace(self.span(), &arg, format!("{arg})")),
                    ]
                })
            }
            _ => None,
        };
        d.hint(hint).fix(fix.unwrap_or_default())
    }

    fn stmt(&mut self) -> PResult<Stmt> {
        let span = self.span();
        let kind = match self.peek().clone() {
            Tok::Let | Tok::Var => {
                let kw = self.bump().tok;
                let mutable = kw == Tok::Var;
                let word = kw.text();
                let (name, name_span) = self.ident("a variable name", &format!("write the name after `{word}`: `{word} x = 0`"))?;
                let ty = if self.at(&Tok::Colon) {
                    self.bump();
                    Some(self.ty(&format!("write the type after the colon: `{word} {name}: int = 0`"))?)
                } else {
                    None
                };
                if !self.at(&Tok::Assign) {
                    let shown = match ty {
                        Some(t) => format!("{word} {name}: {} = ...", t.name()),
                        None => format!("{word} {name} = ..."),
                    };
                    let hint = if name == "mut" {
                        "Nyra has no `mut`: write `var x = ...` for a variable that changes".to_string()
                    } else {
                        format!("a variable needs a starting value: `{shown}`")
                    };
                    let d = self.unexpected("`=`").or_hint(hint.clone());
                    // `let mut x = 0` is `var x = 0`
                    if name == "mut" && matches!(self.peek(), Tok::Ident(_)) && d.hint.as_deref() == Some(hint.as_str()) {
                        let mut fix = vec![Edit::range(name_span, self.span(), "mut", "")];
                        if !mutable {
                            fix.insert(0, Edit::replace(span, "let", "var"));
                        }
                        return Err(d.fix(fix));
                    }
                    return Err(d);
                }
                self.bump();
                let value = self.expr()?;
                self.end_stmt()?;
                StmtKind::Let { name, mutable, ty, value }
            }
            Tok::If => return self.if_stmt(),
            Tok::While => {
                self.bump();
                let cond = self.expr()?;
                let body = self.loop_body("while")?;
                StmtKind::While { cond, body }
            }
            Tok::For => {
                self.bump();
                let (var, _) = self.ident("a loop variable", "loops look like `for i in 0..10 { ... }` or `for x in xs { ... }`")?;
                self.expect(Tok::In, "`in`").map_err(|d| {
                    let hint = self.two_variables_hint(&var);
                    d.or_hint(hint.unwrap_or_else(|| "loops look like `for i in 0..10 { ... }` or `for x in xs { ... }`".into()))
                })?;
                let start = self.expr()?;
                if let ExprKind::Call(f, args) = &start.kind {
                    if f == "range" {
                        let example = range_example(args);
                        let hint = match &example {
                            Some(r) => format!("`range(...)` does not exist: write the range directly, `for i in {r} {{ ... }}`"),
                            None => "`range(...)` does not exist: write the range directly, `for i in 0..10 { ... }`".to_string(),
                        };
                        let written: Option<Vec<String>> = args.iter().map(simple_arg).collect();
                        let space = if self.at(&Tok::LBrace) { " " } else { "" };
                        let fix = example.zip(written).map(|(r, w)| {
                            Edit::range(start.span, self.span(), &format!("range({})", w.join(",")), format!("{r}{space}"))
                        });
                        return Err(self.unexpected("`..`").hint(hint).fix_opt(fix));
                    }
                }
                if self.at(&Tok::DotDot) {
                    self.bump();
                    let end = self.expr()?;
                    let body = self.loop_body("for")?;
                    StmtKind::For { var, start, end, body }
                } else {
                    let body = self.loop_body("for")?;
                    StmtKind::ForEach { var, iter: start, body }
                }
            }
            Tok::Break | Tok::Continue => {
                let t = self.bump().tok;
                if self.loop_depth == 0 {
                    return Err(Diag::new("E0101", format!("`{}` must be inside a loop", t.text()), span)
                        .hint("`break` and `continue` work in `while` and `for` loops; to leave a function early write `ret`"));
                }
                self.end_stmt()?;
                if t == Tok::Break {
                    StmtKind::Break
                } else {
                    StmtKind::Continue
                }
            }
            Tok::Arena => {
                self.bump();
                StmtKind::Arena(self.block("arena")?)
            }
            Tok::Ret => {
                self.bump();
                let value = if matches!(self.peek(), Tok::Newline | Tok::RBrace | Tok::Eof) {
                    None
                } else {
                    Some(self.expr()?)
                };
                self.end_stmt()?;
                StmtKind::Ret(value)
            }
            first => {
                let first = match first {
                    Tok::Ident(w) => Some((w, span)),
                    _ => None,
                };
                let e = self.expr()?;
                // `place = value` / `place op= value` (the checker makes sure it is a place)
                if self.at(&Tok::Assign) || matches!(self.peek(), Tok::OpAssign(_)) {
                    let op = match self.bump().tok {
                        Tok::OpAssign(op) => Some(op),
                        _ => None,
                    };
                    let value = self.expr()?;
                    self.end_stmt()?;
                    StmtKind::Assign { target: e, op, value }
                } else {
                    self.end_stmt_after(first.as_ref().map(|(w, at)| (w.as_str(), *at)), None)?;
                    StmtKind::Expr(e)
                }
            }
        };
        Ok(Stmt { kind, span })
    }

    /// `for i, x in xs` (or `enumerate(xs)`): a loop has one variable. Shows how to get the position and the element.
    fn two_variables_hint(&self, first: &str) -> Option<String> {
        if !self.at(&Tok::Comma) {
            return None;
        }
        let Tok::Ident(second) = &self.toks.get(self.pos + 1)?.tok else { return None };
        if self.toks.get(self.pos + 2)?.tok != Tok::In {
            return None;
        }
        let at = |n: usize| self.toks.get(self.pos + n).map(|t| &t.tok);
        let seq = match (at(3), at(4), at(5)) {
            (Some(Tok::Ident(f)), Some(Tok::LParen), Some(Tok::Ident(inner))) if f == "enumerate" => inner.clone(),
            (Some(Tok::Ident(s)), next, _) if next != Some(&Tok::LParen) && next != Some(&Tok::Dot) => s.clone(),
            _ => "xs".to_string(),
        };
        Some(format!(
            "a loop has one variable: to get the position and the element write `for {first} in 0..{seq}.len() {{ let {second} = {seq}[{first}] ... }}`"
        ))
    }

    /// The body of a loop: `break` and `continue` are allowed inside.
    fn loop_body(&mut self, what: &str) -> PResult<Vec<Stmt>> {
        self.loop_depth += 1;
        let body = self.block(what);
        self.loop_depth -= 1;
        body
    }

    fn if_stmt(&mut self) -> PResult<Stmt> {
        let span = self.expect(Tok::If, "`if`")?;
        let cond = self.expr()?;
        let then = self.block("if")?;
        // `else` may sit on the line after the closing `}`.
        let save = self.pos;
        self.skip_newlines();
        let els = if self.at(&Tok::Else) {
            self.bump();
            if self.at(&Tok::If) {
                Some(vec![self.if_stmt()?])
            } else {
                Some(self.block("else")?)
            }
        } else {
            self.pos = save;
            None
        };
        Ok(Stmt { kind: StmtKind::If { cond, then, els }, span })
    }

    // ---- expressions -----------------------------------------------------

    fn expr(&mut self) -> PResult<Expr> {
        self.binary(1)
    }

    fn binop(t: &Tok) -> Option<(BinOp, u8)> {
        Some(match t {
            Tok::Or => (BinOp::Or, 1),
            Tok::And => (BinOp::And, 2),
            Tok::Eq => (BinOp::Eq, 3),
            Tok::Ne => (BinOp::Ne, 3),
            Tok::Lt => (BinOp::Lt, 4),
            Tok::Le => (BinOp::Le, 4),
            Tok::Gt => (BinOp::Gt, 4),
            Tok::Ge => (BinOp::Ge, 4),
            Tok::Plus => (BinOp::Add, 5),
            Tok::Minus => (BinOp::Sub, 5),
            Tok::Star => (BinOp::Mul, 6),
            Tok::Slash => (BinOp::Div, 6),
            Tok::Percent => (BinOp::Mod, 6),
            _ => return None,
        })
    }

    fn binary(&mut self, min_prec: u8) -> PResult<Expr> {
        let mut lhs = self.unary()?;
        while let Some((op, prec)) = Self::binop(self.peek()) {
            if prec < min_prec {
                break;
            }
            let span = self.bump().span;
            self.skip_newlines();
            let rhs = self.binary(prec + 1)?;
            lhs = Expr::new(ExprKind::Binary(op, Box::new(lhs), Box::new(rhs)), span);
        }
        Ok(lhs)
    }

    fn unary(&mut self) -> PResult<Expr> {
        let span = self.span();
        let op = match self.peek() {
            Tok::Minus => UnOp::Neg,
            Tok::Not => UnOp::Not,
            _ => return self.primary(),
        };
        self.bump();
        let inner = self.unary()?;
        Ok(Expr::new(ExprKind::Unary(op, Box::new(inner)), span))
    }

    /// `if c { a } else if d { b } else { c }` used as a value.
    fn if_expr(&mut self) -> PResult<Expr> {
        let span = self.expect(Tok::If, "`if`")?;
        let cond = self.expr()?;
        let then = self.branch_expr()?;
        let save = self.pos;
        self.skip_newlines();
        if !self.at(&Tok::Else) {
            self.pos = save;
            return Err(Diag::new(
                "E0212",
                "an `if` used as a value needs an `else`: without it there is no value when the condition is false",
                span,
            )
            .hint("add the other branch: `let x = if cond { 1 } else { 0 }`"));
        }
        self.bump();
        let els = if self.at(&Tok::If) { self.if_expr()? } else { self.branch_expr()? };
        Ok(Expr::new(ExprKind::If(Box::new(cond), Box::new(then), Box::new(els)), span))
    }

    /// `{ expr }`: one branch of an `if` used as a value.
    fn branch_expr(&mut self) -> PResult<Expr> {
        self.expect(Tok::LBrace, "`{`").map_err(|d| {
            d.or_hint("each branch of an `if` used as a value is written in braces: `if c { a } else { b }`")
        })?;
        self.skip_newlines();
        let e = match self.expr() {
            Ok(e) => e,
            Err(d) if matches!(self.peek(), Tok::RBrace) => {
                // an empty branch: skip the rest of this `if`, so one mistake gives one error
                self.skip_if_value_rest();
                return Err(d.hint("an `if` used as a value needs a value in every branch: `if c { 1 } else { 2 }`"));
            }
            Err(d) => return Err(d),
        };
        self.skip_newlines();
        if !self.at(&Tok::RBrace) {
            let d = Diag::new(
                "E0212",
                format!(
                    "each branch of an `if` used as a value must be exactly one expression, found {} after it",
                    self.found()
                ),
                self.span(),
            )
            .hint("compute the value first: `var x = 0`, then an `if` statement that sets it (`if c { x = 1 } else { x = 2 }`)");
            self.skip_if_value_rest();
            return Err(d);
        }
        self.bump();
        Ok(e)
    }

    /// After a bad branch of an `if` used as a value: skip to the end of this branch and
    /// any `else` branches after it, so one mistake gives one error.
    fn skip_if_value_rest(&mut self) {
        let mut depth = 1usize;
        while depth > 0 && !matches!(self.peek(), Tok::Eof | Tok::Fn) {
            match self.bump().tok {
                Tok::LBrace => depth += 1,
                Tok::RBrace => depth -= 1,
                _ => {}
            }
        }
        loop {
            let save = self.pos;
            self.skip_newlines();
            if !self.at(&Tok::Else) {
                self.pos = save;
                return;
            }
            // `else { ... }` or `else if cond { ... }`: skip to the block, then over it
            while !matches!(self.peek(), Tok::LBrace | Tok::Eof | Tok::Fn) {
                self.bump();
            }
            if !self.at(&Tok::LBrace) {
                return;
            }
            self.bump();
            let mut depth = 1usize;
            while depth > 0 && !matches!(self.peek(), Tok::Eof | Tok::Fn) {
                match self.bump().tok {
                    Tok::LBrace => depth += 1,
                    Tok::RBrace => depth -= 1,
                    _ => {}
                }
            }
        }
    }

    /// Parses the expression inside `{ }` of an interpolated string.
    fn sub_expr(&mut self, code: &str, base: Span) -> PResult<Expr> {
        let shift = |s: Span| Span { line: base.line, col: base.col + s.col - 1 };
        let spec = code.contains(':');
        let (mut toks, errs) = lexer::lex(code);
        if let Some((first, rest)) = errs.split_first() {
            for d in rest {
                self.errs.push(d.clone().moved(shift));
            }
            let d = first.clone().moved(shift);
            return Err(if spec { d.hint(FORMAT_SPEC_HINT) } else { d });
        }
        for t in &mut toks {
            t.span = shift(t.span);
        }
        let mut sub = Parser { toks, structs: Vec::new(), pos: 0, errs: Vec::new(), in_string: true, cut_blocks: 0, loop_depth: 0 };
        let e = sub.expr();
        self.errs.append(&mut sub.errs);
        let e = e?;
        if !matches!(sub.peek(), Tok::Newline | Tok::Eof) {
            let d = sub.unexpected("`}` after the expression");
            let hint = if spec && matches!(sub.peek(), Tok::Colon) {
                FORMAT_SPEC_HINT
            } else {
                "only one expression can go inside `{ }` in a string: build the parts separately, e.g. `\"{a} {b}\"`"
            };
            return Err(d.hint(hint));
        }
        Ok(e)
    }

    fn primary(&mut self) -> PResult<Expr> {
        let span = self.span();
        let kind = match self.peek().clone() {
            Tok::Int(n) => {
                self.bump();
                ExprKind::Int(n)
            }
            Tok::Float(f) => {
                self.bump();
                ExprKind::Float(f)
            }
            Tok::Str(s) => {
                self.bump();
                ExprKind::Str(s)
            }
            Tok::Char(c) => {
                self.bump();
                ExprKind::Char(c)
            }
            Tok::LBracket => {
                let open = self.bump().span;
                let mut items = Vec::new();
                loop {
                    self.skip_newlines();
                    if self.at(&Tok::RBracket) {
                        self.bump();
                        break;
                    }
                    items.push(self.expr()?);
                    self.skip_newlines();
                    match self.peek() {
                        Tok::Comma => {
                            self.bump();
                        }
                        Tok::RBracket => {
                            self.bump();
                            break;
                        }
                        _ => {
                            return Err(self
                                .unexpected(&format!("`,` or `]` in the array opened at {}:{}", open.line, open.col))
                                .or_hint("separate the elements with commas: `[1, 2, 3]`"))
                        }
                    }
                }
                ExprKind::Array(items)
            }
            Tok::Interp(parts) => {
                self.bump();
                let mut out = Vec::new();
                for p in parts {
                    match p {
                        StrPart::Lit(s) => out.push(InterpPart::Lit(s)),
                        StrPart::Code(code, base) => out.push(InterpPart::Expr(self.sub_expr(&code, base)?)),
                    }
                }
                ExprKind::Interp(out)
            }
            Tok::True => {
                self.bump();
                ExprKind::Bool(true)
            }
            Tok::False => {
                self.bump();
                ExprKind::Bool(false)
            }
            Tok::Ident(name) => {
                self.bump();
                if self.at(&Tok::LParen) {
                    let open = self.bump().span;
                    ExprKind::Call(name.clone(), self.args(&name, open)?)
                } else {
                    ExprKind::Var(name)
                }
            }
            Tok::LParen => {
                let open = self.bump().span;
                let e = self.expr()?;
                self.expect(Tok::RParen, &format!("`)` to close the `(` opened at {}:{}", open.line, open.col))
                    .map_err(|d| d.or_hint("add the missing `)`: every `(` needs a matching `)`"))?;
                return self.postfix(e);
            }
            Tok::If => return self.if_expr(),
            _ => return Err(self.expression_expected()),
        };
        self.postfix(Expr::new(kind, span))
    }

    /// The arguments of a call after its `(`: `a`, `inout place` or `label: value`.
    fn args(&mut self, name: &str, open: Span) -> PResult<Vec<Expr>> {
        let mut args = Vec::new();
        while !self.at(&Tok::RParen) {
            let span = self.span();
            let arg = if self.at(&Tok::Inout) {
                self.bump();
                let place = self.expr()?;
                Expr::new(ExprKind::Inout(Box::new(place)), span)
            } else if matches!(self.peek(), Tok::Ident(_))
                && matches!(self.peek_at(1), Tok::Colon)
                && !matches!(self.peek_at(2), Tok::Colon)
            {
                let Tok::Ident(label) = self.bump().tok else { unreachable!() };
                self.bump();
                let value = self.expr()?;
                Expr::new(ExprKind::Labeled(label, Box::new(value)), span)
            } else {
                self.expr()?
            };
            args.push(arg);
            if !self.at(&Tok::RParen) {
                if !self.at(&Tok::Comma) {
                    return Err(self.call_error(name, open));
                }
                self.bump();
            }
        }
        self.bump();
        Ok(args)
    }

    /// Field reads `.name`, method calls `.name(args)` and indexing `[i]` after a value.
    fn postfix(&mut self, mut e: Expr) -> PResult<Expr> {
        loop {
            match self.peek() {
                Tok::Dot => {
                    self.bump();
                    let span = self.span();
                    let (name, _) = self.ident(
                        "a field or method name after `.`",
                        "write a field (`p.x`) or a method call (`xs.len()`) after the dot",
                    )?;
                    if self.at(&Tok::LParen) {
                        let open = self.bump().span;
                        let args = self.args(&name, open)?;
                        e = Expr::new(ExprKind::Method(Box::new(e), name, args), span);
                    } else {
                        e = Expr::new(ExprKind::Field(Box::new(e), name), span);
                    }
                }
                Tok::LBracket => {
                    let span = self.bump().span;
                    self.skip_newlines();
                    let index = self.expr()?;
                    self.skip_newlines();
                    self.expect(Tok::RBracket, "`]` to close the index")
                        .map_err(|d| d.or_hint("an index is written `xs[i]`"))?;
                    e = Expr::new(ExprKind::Index(Box::new(e), Box::new(index)), span);
                }
                _ => return Ok(e),
            }
        }
    }

    /// Something that is not a `,` or `)` after an argument.
    fn call_error(&self, name: &str, open: Span) -> Diag {
        let d = self.unexpected(&format!("`,` or `)` in the arguments of `{name}`"));
        if matches!(self.peek(), Tok::RBrace | Tok::Eof | Tok::Fn | Tok::Newline) {
            d.hint(format!("close the call with `)`: the `(` at {}:{} is never closed", open.line, open.col))
        } else if name == "print" {
            d.or_hint("`print` takes one value: to show several, put them in one string, `print(\"{a} {b}\")`")
        } else {
            d.or_hint(format!("separate arguments with commas: `{name}(a, b)`"))
        }
    }

    /// True if the two tokens before the current one are `-` `-` with no space between them.
    fn double_minus_before(&self) -> bool {
        let n = self.pos;
        n >= 2
            && self.toks[n - 1].tok == Tok::Minus
            && self.toks[n - 2].tok == Tok::Minus
            && self.toks[n - 2].span.line == self.toks[n - 1].span.line
            && self.toks[n - 2].span.col + 1 == self.toks[n - 1].span.col
    }

    /// "expected an expression" with the most likely reason.
    fn expression_expected(&self) -> Diag {
        let d = self.unexpected("an expression");
        let t = self.peek();
        let starts_line = matches!(self.prev().map(|p| &p.tok), Some(Tok::Newline) | None);
        let before = self.prev_nonl().map(|p| &p.tok);
        // `i++` / `i--` as a statement of their own: `i += 1` / `i -= 1`
        let n = self.pos;
        let counter = |at: usize| -> Option<(Span, String)> {
            let starts = at == 0 || matches!(self.toks[at - 1].tok, Tok::Newline | Tok::LBrace);
            match &self.toks[at].tok {
                Tok::Ident(v) if starts => Some((self.toks[at].span, v.clone())),
                _ => None,
            }
        };
        let ends = |at: usize| matches!(self.toks.get(at).map(|t| &t.tok), Some(Tok::Newline | Tok::RBrace | Tok::Eof));
        if matches!(t, Tok::Plus) && self.glued_to(&Tok::Plus) {
            let fix = (n >= 2 && ends(n + 1)).then(|| counter(n - 2)).flatten().map(|(at, v)| {
                Edit::range(after(at, &v), after(self.span(), "+"), "++", " += 1")
            });
            return d.hint("`++` does not exist: write `i += 1`").fix_opt(fix);
        }
        if matches!(t, Tok::Star) && self.glued_to(&Tok::Star) {
            return d.hint("there is no `**` operator: multiply (`x * x * x`) or write a loop (see the `pow` recipe in docs/AI_GUIDE.md section 6)");
        }
        if matches!(t, Tok::Assign) && self.glued_to(&Tok::DotDot) {
            return d.hint("`..=` does not exist: the end of a range is exclusive, so `0..10` counts 0 to 9");
        }
        if matches!(t, Tok::Newline | Tok::RBrace | Tok::Eof | Tok::RParen) && self.double_minus_before() {
            let fix = (n >= 3 && ends(n)).then(|| counter(n - 3)).flatten().map(|(at, v)| {
                Edit::range(after(at, &v), after(self.toks[n - 1].span, "-"), "--", " -= 1")
            });
            return d.hint("`--` does not exist: write `i -= 1`").fix_opt(fix);
        }
        let generic = "an expression is a value such as `5`, `x + 1` or `f(x)`";
        match t {
            Tok::Else => d.hint("`else` must follow the `}` of an `if` (`} else {`): look for a missing `}` or a statement between them"),
            Tok::RParen if matches!(before, Some(Tok::LParen)) => {
                d.hint("empty parentheses are not a value: put an expression inside, or remove them")
            }
            Tok::Fn => d.hint("functions are not values (there are no lambdas or closures): define a named `fn` at the top level"),
            Tok::While | Tok::For | Tok::Let | Tok::Var | Tok::Ret => {
                d.hint(format!("`{}` starts a statement, not a value: put it on its own line", t.text()))
            }
            Tok::LBrace => d.hint("a `{ }` block is not a value: for a conditional value write `if cond { a } else { b }`"),
            Tok::Plus | Tok::Star | Tok::Slash | Tok::Percent | Tok::Eq | Tok::Ne | Tok::Lt | Tok::Le | Tok::Gt | Tok::Ge
            | Tok::And | Tok::Or
                if starts_line =>
            {
                d.hint(format!(
                    "a line cannot start with `{}`: end the previous line with the operator instead (a line may break after an operator, never before one)",
                    t.text()
                ))
            }
            Tok::Plus | Tok::Star | Tok::Slash | Tok::Percent | Tok::Eq | Tok::Ne | Tok::Lt | Tok::Le | Tok::Gt | Tok::Ge
            | Tok::And | Tok::Or | Tok::Comma
                if before.is_some_and(|p| Self::binop(p).is_some()) =>
            {
                d.hint(format!(
                    "two operators in a row: `{}` needs a value on its right, but `{}` follows it",
                    before.map(|p| p.text()).unwrap_or(""),
                    t.text()
                ))
            }
            Tok::Newline | Tok::Eof | Tok::RBrace | Tok::RParen | Tok::Comma => match before {
                Some(p) if Self::binop(p).is_some() => {
                    d.hint(format!("`{}` needs a value on its right side: `a {} b`", p.text(), p.text()))
                }
                Some(Tok::Assign) => d.hint("write a value after `=`: `let x = 0`"),
                Some(Tok::LParen) | Some(Tok::Comma) => d.hint("write the missing value, or remove the extra `,`"),
                Some(Tok::DotDot) => d.hint("write the end of the range after `..`: `0..10`"),
                Some(Tok::Minus) => d.hint("`-` needs a value after it: `-x`"),
                _ => d.or_hint(generic),
            },
            _ => d.or_hint(generic),
        }
    }
}

/// `0..10` for `range(10)` and `1..n` for `range(1, n)`, when the arguments are simple.
fn range_example(args: &[Expr]) -> Option<String> {
    match args {
        [end] => Some(format!("0..{}", simple_arg(end)?)),
        [start, end] => Some(format!("{}..{}", simple_arg(start)?, simple_arg(end)?)),
        _ => None,
    }
}

/// A number or a name, as written.
fn simple_arg(e: &Expr) -> Option<String> {
    match &e.kind {
        ExprKind::Int(n) => Some(n.to_string()),
        ExprKind::Var(v) => Some(v.clone()),
        _ => None,
    }
}

/// The source text of a token when the token alone determines it (`return`, `)`, `42`, `"hi"`).
/// Edits compare it with the file, so a token written another way (`007`) only loses its fix.
fn source_text(tok: &Tok) -> Option<String> {
    match tok {
        Tok::Ident(s) => Some(s.clone()),
        Tok::Int(n) => Some(n.to_string()),
        Tok::Float(f) => Some(format!("{f:?}")),
        Tok::Str(s) => {
            let mut out = String::from("\"");
            for c in s.chars() {
                match c {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\n"),
                    '\t' => out.push_str("\\t"),
                    '\r' => out.push_str("\\r"),
                    '{' => out.push_str("{{"),
                    '}' => out.push_str("}}"),
                    c => out.push(c),
                }
            }
            out.push('"');
            Some(out)
        }
        t => Some(t.text().to_string()).filter(|s| !s.is_empty()),
    }
}

/// The Nyra symbol for a word of another language, when the word can only mean it.
fn word_fix(w: &str) -> Option<&'static str> {
    match w {
        "and" => Some("&&"),
        "or" => Some("||"),
        _ => None,
    }
}
