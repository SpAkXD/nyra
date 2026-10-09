//! Recursive-descent parser with precedence climbing for expressions.
//! On an error it records a diagnostic and skips to the next statement, so one
//! run reports as many errors as possible. Every error says what was expected,
//! what was found and (through `hints`) what to write instead.

// a parse error is a whole `Diag`: big, but errors are rare, and boxing each one would only add noise
#![allow(clippy::result_large_err)]

use crate::ast::*;
use crate::diag::{after, suggest_fix, Diag, Edit};
use crate::hints;
use crate::lexer::{self, StrPart, Tok, Token};

type PResult<T> = Result<T, Diag>;

const FORMAT_SPEC_HINT: &str =
    "a format specifier is `[[fill]align][+][0][width][,][.precision]` after the colon: `{x:>8}`, `{n:05}`, `{f:.2}`, `{n:,}`";

pub fn parse(toks: Vec<Token>) -> (Program, Vec<Diag>) {
    // every `struct Name` of the file, so a type can name a struct that is declared further down
    let structs: Vec<String> = toks
        .windows(2)
        .filter_map(|w| match (&w[0].tok, &w[1].tok) {
            (Tok::Struct | Tok::Enum, Tok::Ident(name)) => Some(name.clone()),
            _ => None,
        })
        .collect();
    let mut p = Parser {
        toks,
        structs,
        pos: 0,
        errs: Vec::new(),
        in_string: false,
        cut_blocks: 0,
        loop_depth: 0,
        nested: Vec::new(),
        depth: 0,
        too_deep: None,
        queue: Vec::new(),
        hidden: 0,
    };
    let prog = p.program();
    let mut errs = p.errs;
    // code nested too deeply: parsing stopped there, so what follows says nothing new
    if let Some(at) = p.too_deep {
        errs.retain(|d| d.code == "E0103" || (d.span.line, d.span.col) < (at.line, at.col));
        let mut seen = false;
        errs.retain(|d| d.code != "E0103" || !std::mem::replace(&mut seen, true));
    }
    (prog, errs)
}

/// The deepest nesting of expressions and blocks: parentheses, calls, unary operators, `[ ]`, blocks,
/// `else if` links, and each operator of a chain (`a + b + c` is two levels, `s.trim().upper()` too).
/// Deeper code is E0103, so neither the parser nor any later stage can run out of stack.
pub const MAX_NESTING: usize = 256;

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
    /// Functions defined inside a function body: they are ordinary top-level functions.
    nested: Vec<Func>,
    /// How deeply the code being parsed is nested (see `MAX_NESTING`).
    depth: usize,
    /// Where the code got nested too deeply (parsing stopped there).
    too_deep: Option<Span>,
    /// Statements that follow the one `stmt` returned: `let (a, b) = f()` is a hidden `let` of the
    /// whole value (returned) and one `let` per name (queued).
    queue: Vec<Stmt>,
    /// How many hidden names (`·t1`) were made: each destructuring or `if let` has its own.
    hidden: usize,
}

/// A pattern: `a`, `_` or `(a, (b, _))`.
enum Pat {
    Name(String, Span),
    Wild,
    Tuple(Vec<Pat>),
}

impl Parser {
    /// One level deeper; past `MAX_NESTING` an error, and the parser stops (it skips to the end).
    fn deeper(&mut self) -> PResult<()> {
        self.depth += 1;
        if self.depth <= MAX_NESTING {
            return Ok(());
        }
        let span = self.span();
        if self.too_deep.is_none() {
            self.too_deep = Some(span);
        }
        self.pos = self.toks.len() - 1;
        Err(Diag::new("E0103", format!("the code is nested more than {MAX_NESTING} levels deep"), span).hint(
            "split it up: give parts of a long expression names with `let`, and move deeply nested blocks into functions of their own",
        ))
    }

    /// Runs `f`, then goes back to the nesting depth from before it.
    fn same_depth<T>(&mut self, f: impl FnOnce(&mut Self) -> PResult<T>) -> PResult<T> {
        let d = self.depth;
        let r = f(self);
        self.depth = d;
        r
    }

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
        self.prev()
            .is_some_and(|p| p.tok == *tok && p.span.line == self.span().line && p.span.col + p.tok.text().len() == self.span().col)
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
            Tok::Ident(_)
                | Tok::Int(_)
                | Tok::Float(_)
                | Tok::Str(_)
                | Tok::Interp(_)
                | Tok::Char(_)
                | Tok::True
                | Tok::False
                | Tok::LParen
                | Tok::Not
        );
        (p.tok == Tok::Ident("not".into()) && p.span.line == self.span().line && starts_value).then_some(p.span)
    }

    fn found_hint(&self) -> Option<String> {
        if self.after_not().is_some() {
            return hints::word("not");
        }
        match self.peek() {
            Tok::Ident(w) => self.number_suffix(w).map(|(h, _)| h).or_else(|| hints::word(w)),
            Tok::FatArrow => Some(
                "`=>` only starts the body of a lambda, which is an argument of an array method: `xs.map(x => x * 2)`; elsewhere define a named function, e.g. `fn double(x: int) -> int = x * 2`"
                    .into(),
            ),
            // `x -> x * 2` (Java, Kotlin): a lambda arrow is `=>`
            Tok::Arrow if matches!(self.prev().map(|p| &p.tok), Some(Tok::Ident(_))) => {
                Some("a lambda is written with `=>`: `x => x * 2`".into())
            }
            Tok::Assign if self.glued_to(&Tok::Eq) || self.glued_to(&Tok::Ne) => {
                Some("`===` and `!==` do not exist: compare with `==` and `!=`".into())
            }
            Tok::Assign => Some("`=` assigns to a variable on a line of its own (`x = 1`); to compare two values write `==`".into()),
            Tok::Slash if matches!(self.peek_at(1), Tok::Star) => {
                Some("Nyra has no block comments: start every comment line with `//`".into())
            }
            Tok::LBrace => self.struct_literal_hint(),
            Tok::Question => Some(
                "`?` goes right after a type to make it optional (`int?`); Nyra has no `c ? a : b`: write `if cond { a } else { b }`".into(),
            ),
            Tok::Coalesce => Some("`??` needs a value on both sides: `x ?? default`".into()),
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
            Tok::Assign if self.glued_to(&Tok::Eq) || self.glued_to(&Tok::Ne) => Edit::replace(here, "=", ""),
            Tok::Arrow if matches!(self.prev().map(|p| &p.tok), Some(Tok::Ident(_))) => Edit::replace(here, "->", "=>"),
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
                    Some((
                        "exponent notation does not exist: write the number in full, with a dot (`0.00001`, `100000.0`)".into(),
                        None,
                    ))
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
        let mut enums = Vec::new();
        let mut public: Vec<String> = Vec::new();
        let mut examples = Vec::new();
        let mut uses = Vec::new();
        let mut top: Vec<Stmt> = Vec::new();
        loop {
            self.skip_newlines();
            match self.peek() {
                Tok::Eof => break,
                // `ex f(3) == 9`: examples, usually of the function right before them
                _ if self.example_ahead() => match self.examples() {
                    Ok(list) => examples.extend(list),
                    Err(d) => {
                        self.errs.push(d);
                        self.sync_stmt();
                    }
                },
                // `use math` (and other languages' `import math`, `from math import sqrt`)
                Tok::Ident(w) if self.starts_use(w) => match self.use_line() {
                    Ok(u) => uses.push(u),
                    Err(d) => {
                        self.errs.push(d);
                        self.sync_stmt();
                    }
                },
                // `pub fn`, `pub struct`, `pub enum`: usable from the files that import this one
                Tok::Ident(w) if w == "pub" && matches!(self.peek_at(1), Tok::Fn | Tok::Struct | Tok::Enum) => {
                    self.bump();
                    match self.peek() {
                        Tok::Fn => match self.func() {
                            Ok(f) => {
                                public.push(f.name.clone());
                                funcs.push(f);
                            }
                            Err(d) => {
                                self.errs.push(d);
                                self.sync_top();
                            }
                        },
                        Tok::Struct => match self.struct_def() {
                            Ok(sd) => {
                                public.push(sd.name.clone());
                                structs.push(sd);
                            }
                            Err(d) => {
                                self.errs.push(d);
                                self.sync_top();
                            }
                        },
                        _ => match self.enum_def() {
                            Ok(ed) => {
                                public.push(ed.name.clone());
                                enums.push(ed);
                            }
                            Err(d) => {
                                self.errs.push(d);
                                self.sync_top();
                            }
                        },
                    }
                }
                Tok::Ident(w) if w == "pub" => {
                    let d = Diag::new("E0332", "`pub` goes right before `fn`, `struct` or `enum`", self.span())
                        .hint("only definitions can be exported: `pub fn area(r: Rect) -> int = ...`; an import cannot be re-exported, write a small wrapper function");
                    self.errs.push(d);
                    self.sync_top();
                }
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
                Tok::Enum => match self.enum_def() {
                    Ok(ed) => enums.push(ed),
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
                // a statement at the top level: the program is a script, its statements form `main`
                _ if self.starts_script_statement() => match self.stmt() {
                    Ok(s) => {
                        top.push(s);
                        top.append(&mut self.queue);
                    }
                    Err(d) => {
                        self.errs.push(d);
                        self.sync_stmt();
                    }
                },
                _ => {
                    let d = self.top_level_error();
                    self.errs.push(d);
                    self.sync_top();
                }
            }
        }
        let mut script = false;
        if let Some(first) = top.first() {
            let span = first.span;
            if let Some(main) = funcs.iter().find(|f| f.name == "main") {
                let hint = match &first.kind {
                    StmtKind::Let { name, .. } => format!(
                        "a program with `fn main` has no script variables: move `{name}` into `main`, or drop `fn main` and write its statements at the top level (a script), whose variables every function can use"
                    ),
                    StmtKind::Expr(e) if matches!(&e.kind, ExprKind::Call(f, _) if f == "main") => {
                        "`main` runs by itself when the program starts: do not call it".to_string()
                    }
                    _ => format!(
                        "a program is either a script (statements at the top level) or has `fn main` (line {}): move these statements into `main`",
                        main.span.line
                    ),
                };
                self.errs.push(Diag::new("E0101", "statements at the top level and a `fn main` in the same program", span).hint(hint));
            } else {
                funcs.push(Func { name: "main".to_string(), params: Vec::new(), ret: Type::Void, body: top, span });
                script = true;
            }
        }
        funcs.append(&mut self.nested);
        Program { funcs, structs, enums, examples, uses, script, globals: Default::default(), public, files: Default::default() }
    }

    /// True at `ex` followed by the start of a condition on the same line: a line of examples.
    /// (`ex` stays an ordinary name: `ex = 1`, `ex(2)` and `ex.len()` are not examples.)
    fn example_ahead(&self) -> bool {
        let Tok::Ident(w) = self.peek() else { return false };
        let next = &self.toks[(self.pos + 1).min(self.toks.len() - 1)];
        w == "ex"
            && next.span.line == self.span().line
            && matches!(
                next.tok,
                Tok::Ident(_)
                    | Tok::Int(_)
                    | Tok::Float(_)
                    | Tok::Str(_)
                    | Tok::Interp(_)
                    | Tok::Char(_)
                    | Tok::True
                    | Tok::False
                    | Tok::Not
                    | Tok::Minus
            )
    }

    /// `ex cond, cond, ...`: one or more `bool` conditions; a line may break after a comma.
    fn examples(&mut self) -> PResult<Vec<Example>> {
        self.bump();
        let mut list = Vec::new();
        loop {
            let expr = self.expr()?;
            list.push(Example { expr, file: None });
            if !self.at(&Tok::Comma) {
                break;
            }
            self.bump();
            self.skip_newlines();
        }
        self.end_stmt_after(None, Some("separate the examples with commas: `ex sq(3) == 9, sq(-2) == 4`"))?;
        Ok(list)
    }

    /// True if the word here starts an import line: `use name`, `import name`, `from name import ...`.
    fn starts_use(&self, w: &str) -> bool {
        matches!(w, "use" | "import" | "from") && matches!(self.peek_at(1), Tok::Ident(_) | Tok::Str(_))
            // `use ./shapes`, `use ../util/text`
            || (w == "use" && matches!(self.peek_at(1), Tok::Dot | Tok::DotDot) && matches!(self.peek_at(2), Tok::Slash))
    }

    /// `use name`: one standard module per line. Other languages' forms are reported with the Nyra
    /// spelling (`import math` is `use math`).
    fn use_line(&mut self) -> PResult<Use> {
        let span = self.span();
        let Tok::Ident(word) = self.bump().tok else { unreachable!("starts_use checked it") };
        // `use ./shapes` (also written `use "./shapes"`): a file of the project
        if matches!(self.peek(), Tok::Dot | Tok::DotDot)
            || (word == "use" && matches!(self.peek(), Tok::Str(p) if p.starts_with("./") || p.starts_with("../")))
        {
            return self.use_path(span);
        }
        if let Tok::Str(path) = self.peek().clone() {
            return Err(Diag::new(
                "E0305",
                format!(
                    "`{word} \"{path}\"`: {} is not a path Nyra imports",
                    if word == "use" { "this" } else { "an import like this" }
                ),
                self.span(),
            )
            .hint(format!(
                "to import a file write its path from this file's folder: `use ./shapes`; the standard modules are {}: `use math`",
                crate::stdlib::module_list()
            )));
        }
        let (module, mspan) = self.ident("a module name", "write the module after `use`: `use math`")?;
        if word == "from" {
            // `from math import sqrt`: Nyra imports the module and names it at each call
            return Err(Diag::new("E0302", format!("`from {module} import ...` does not exist in Nyra"), span)
                .hint(format!("write `use {module}` and call the functions with the module name: `{module}.name(...)`")));
        }
        let mut d = None;
        if word == "import" {
            d = Some(
                Diag::new("E0302", format!("`import {module}`: Nyra spells it `use {module}`"), span)
                    .hint(format!("write `use {module}`"))
                    .fix(vec![Edit::replace(span, "import", "use")]),
            );
        }
        if !matches!(self.peek(), Tok::Newline | Tok::Eof) {
            let rest = match self.peek() {
                Tok::Dot => format!("`use {module}` imports the whole module: call its functions as `{module}.name(...)`, there is no `use {module}.name`"),
                Tok::Comma => "one module per line: `use math` and `use text` on two lines".to_string(),
                Tok::Ident(w) if w == "as" => format!("modules cannot be renamed: write `use {module}` and call `{module}.name(...)`"),
                _ => format!("a `use` line names one module: `use {module}`"),
            };
            return Err(Diag::new("E0302", format!("unexpected {} after `use {module}`", self.found()), self.span()).hint(rest));
        }
        if let Some(d) = d {
            // the line is otherwise fine: the program is still read as if it said `use`
            self.errs.push(d);
        }
        Ok(Use { module, span: mspan, path: None })
    }

    /// `use ./shapes`, `use ../util/text` or `use "./shapes"` after the word `use`: a file of the project.
    fn use_path(&mut self, span: Span) -> PResult<Use> {
        let mut path = String::new();
        if let Tok::Str(p) = self.peek().clone() {
            self.bump();
            path = p;
        } else {
            loop {
                match self.peek().clone() {
                    Tok::Dot => path.push('.'),
                    Tok::DotDot => path.push_str(".."),
                    Tok::Slash => path.push('/'),
                    Tok::Ident(n) => path.push_str(&n),
                    _ => break,
                }
                self.bump();
            }
        }
        let name = path.rsplit('/').next().unwrap_or("").to_string();
        let valid = name.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_')
            && name.chars().all(|c| c.is_alphanumeric() || c == '_')
            && (path.starts_with("./") || path.starts_with("../"));
        if !valid {
            return Err(Diag::new("E0305", format!("`{path}` is not a path Nyra imports"), span)
                .hint("write the path from this file's folder, with `/` and without `.nyra`: `use ./shapes`, `use ../util/text`"));
        }
        if !matches!(self.peek(), Tok::Newline | Tok::Eof) {
            return Err(Diag::new("E0302", format!("unexpected {} after `use {path}`", self.found()), self.span())
                .hint(format!("a `use` line names one module: `use {path}`; call its functions as `{name}.f(...)`")));
        }
        Ok(Use { module: name, span, path: Some(path) })
    }

    /// True if the token here starts a statement that may stand at the top level of a script.
    /// Words of other languages (`import`, `class`, `int main(`) keep their own errors.
    fn starts_script_statement(&self) -> bool {
        match self.peek() {
            Tok::Let | Tok::Var | Tok::If | Tok::While | Tok::For | Tok::Arena | Tok::Ret | Tok::LParen | Tok::Match => true,
            Tok::Ident(w) => {
                let c_style = hints::is_type_word(w) && matches!(self.peek_at(1), Tok::Ident(_));
                hints::top_level_word(w).is_none() && !c_style
            }
            Tok::RBrace | Tok::Eof => false,
            _ => false,
        }
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
                format!("a program with `fn main` has no script variables: move `{name}` into `main`, or drop `fn main` and write its statements at the top level")
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
        if !matches!(self.peek(), Tok::Fn | Tok::Struct | Tok::Enum) {
            self.bump();
        }
        while !matches!(self.peek(), Tok::Fn | Tok::Struct | Tok::Enum | Tok::Eof) {
            self.bump();
        }
    }

    /// `enum Dir { N, E, S, W }` (variants separated by commas or new lines)
    fn enum_def(&mut self) -> PResult<EnumDef> {
        self.expect(Tok::Enum, "`enum`")?;
        let (name, span) = self.ident("an enum name", "name the enum after `enum`: `enum Dir { N, E, S, W }`")?;
        self.expect(Tok::LBrace, &format!("`{{` after `enum {name}`"))
            .map_err(|d| d.or_hint(format!("an enum lists its variants in braces: `enum {name} {{ A, B, C }}`")))?;
        let mut variants = Vec::new();
        loop {
            self.skip_newlines();
            match self.peek() {
                Tok::RBrace => {
                    self.bump();
                    break;
                }
                Tok::Eof | Tok::Fn | Tok::Struct | Tok::Enum => {
                    return Err(self
                        .unexpected(&format!("`}}` to close `enum {name}`"))
                        .hint("add the missing `}` after the last variant"))
                }
                _ => {}
            }
            let (vname, vspan) =
                self.ident("a variant name", "variants are plain names, separated by commas or new lines: `enum Dir { N, E }`")?;
            if self.at(&Tok::LParen) {
                return Err(self
                    .unexpected("`,`, a new line or `}` after a variant")
                    .hint("a variant carries no values yet: for data, use a struct next to the enum"));
            }
            variants.push((vname, vspan));
            match self.peek() {
                Tok::Comma | Tok::Newline => {
                    self.bump();
                }
                Tok::RBrace => {}
                _ => {
                    return Err(self
                        .unexpected("`,`, a new line or `}` after a variant")
                        .or_hint(format!("separate the variants with commas or new lines: `enum {name} {{ A, B }}`")))
                }
            }
        }
        Ok(EnumDef { name, variants, span })
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
        Ok(StructDef { name, fields, span, variants: Vec::new() })
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
            // `var n: int`: a copy the function may change
            if inout && self.at(&Tok::Var) {
                return Err(self
                    .unexpected("a parameter name")
                    .hint("`inout` changes the caller's variable and `var` is a copy the function may change: use one of them"));
            }
            let mutable = !inout && self.at(&Tok::Var);
            if mutable {
                self.bump();
            }
            let (pname, pspan) = self.ident("a parameter name", "parameters look like `a: int, b: float`")?;
            if !self.at(&Tok::Colon) {
                return Err(self.missing_param_type(&pname, pspan));
            }
            self.bump();
            let ty = self.ty(&format!("every parameter needs a type: `{pname}: int`"))?;
            params.push(Param { name: pname, ty, inout, mutable, span: pspan });
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
            // `fn sq(x: int) -> int = x * x  ex sq(3) == 9`: the examples are read next
            if !self.example_ahead() {
                self.end_stmt_after(
                    None,
                    Some("a one-line function is a single expression: use a block `{ ... }` for several statements"),
                )?;
            }
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
            _ => d,
        }
    }

    fn ty(&mut self, hint: &str) -> PResult<Type> {
        self.same_depth(|p| p.ty_inner(hint))
    }

    /// A type, then any number of `?`: `int?` is an optional `int`.
    fn ty_inner(&mut self, hint: &str) -> PResult<Type> {
        let mut t = self.ty_base(hint)?;
        while self.at(&Tok::Question) {
            self.bump();
            t = Type::option(t);
        }
        Ok(t)
    }

    fn ty_base(&mut self, hint: &str) -> PResult<Type> {
        let span = self.span();
        if self.at(&Tok::LParen) {
            // `(int, str)`: a tuple type
            self.deeper()?;
            self.bump();
            let mut elems = Vec::new();
            loop {
                elems.push(self.ty("a tuple type lists its element types: `(int, str)`")?);
                if self.at(&Tok::Comma) {
                    self.bump();
                    continue;
                }
                break;
            }
            self.expect(Tok::RParen, "`)` to close the tuple type").map_err(|d| d.or_hint("a tuple type is written `(int, str)`"))?;
            if elems.len() < 2 {
                return Err(Diag::new("E0101", "a tuple type needs at least two element types", span)
                    .hint("write `(int, str)`; for one value use its type alone"));
            }
            return Ok(Type::tuple(&elems));
        }
        if self.at(&Tok::LBracket) {
            self.deeper()?;
            self.bump();
            let elem = self.ty("an array type names the type of its elements: `[int]`")?;
            if self.at(&Tok::Colon) {
                // `[K: V]`: a map
                self.bump();
                let value = self.ty("a map type names its key and value types: `[str: int]`")?;
                self.expect(Tok::RBracket, "`]` to close the map type")
                    .map_err(|d| d.or_hint("a map type is written `[str: int]`"))?;
                return Ok(Type::map(elem, value));
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
            Tok::FatArrow if is_fn => Some((
                "`=>` does not start a function body: a one-line function is written `fn f(x: int) -> int = x * 2`".to_string(),
                None,
            )),
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
        self.same_depth(|p| {
            p.deeper()?;
            p.block_body(what)
        })
    }

    fn block_body(&mut self, what: &str) -> PResult<Vec<Stmt>> {
        let open = self.open_brace(what)?;
        let mut stmts = Vec::new();
        loop {
            self.skip_newlines();
            match self.peek() {
                Tok::RBrace => {
                    self.bump();
                    return Ok(stmts);
                }
                // an indented `fn` inside a body is a helper written in place: it becomes an ordinary
                // function (it sees no locals around it). A `fn` at the start of a line is the next
                // function after a missing `}`.
                Tok::Fn if self.span().col > 1 => {
                    let depth = std::mem::take(&mut self.loop_depth);
                    match self.func() {
                        Ok(f) => self.nested.push(f),
                        Err(d) => {
                            self.errs.push(d);
                            self.sync_stmt();
                        }
                    }
                    self.loop_depth = depth;
                }
                Tok::Eof if self.too_deep.is_some() => return Ok(stmts),
                Tok::Eof | Tok::Fn | Tok::Struct => return Err(self.unclosed_block(open, what)),
                _ => match self.stmt() {
                    Ok(s) => {
                        stmts.push(s);
                        stmts.append(&mut self.queue);
                    }
                    Err(d) => {
                        self.queue.clear();
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
                ("const" | "final" | "static" | "val", Tok::Ident(name)) => {
                    Some(format!("write `let {name} = ...` for a value that never changes, or `var {name} = ...` for one that does"))
                }
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
            (_, Some("ex")) => {
                "examples go outside functions: put `ex ...` on its own line after the function's closing `}`, or at the end of a one-line function".to_string()
            }
            _ if after_cast => {
                let Tok::Ident(w) = &self.toks[self.pos - 2].tok else { unreachable!() };
                let ty = hints::nyra_type(w).unwrap_or("float");
                format!("Nyra has no casts: convert with a function call, e.g. `{ty}(x)`")
            }
            (Tok::Let | Tok::Var | Tok::If | Tok::While | Tok::For | Tok::Ret, _) => {
                format!("put `{}` on a new line: Nyra has one statement per line", t.text())
            }
            (Tok::Colon, _) if matches!(self.peek_at(1), Tok::Colon) => {
                "`::` paths do not exist: call a module's function with a dot, `math.sqrt(x)`, and other functions by their plain name".to_string()
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
        if let Tok::Ident(w) = self.peek() {
            if matches!(w.as_str(), "use" | "import") && matches!(self.peek_at(1), Tok::Ident(_)) {
                let w = w.clone();
                return Err(Diag::new("E0302", format!("`{w}` inside a block: imports go at the top of the file"), span)
                    .hint("move the `use` line to the top of the file, outside every function"));
            }
        }
        let kind = match self.peek().clone() {
            Tok::Let | Tok::Var => {
                let kw = self.bump().tok;
                let mutable = kw == Tok::Var;
                let word = kw.text();
                if self.at(&Tok::LParen) {
                    return self.let_pattern(mutable, word, span);
                }
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
                const LOOPS: &str = "loops look like `for i in 0..10 { ... }`, `for x in xs { ... }` or `for i, x in xs { ... }`";
                // a loop variable is a name, or a tuple pattern: `for (k, v) in pairs`
                let (first, first_pat) = self.loop_var(LOOPS)?;
                // `for i, x in xs`: the position, then the element
                let (index, var, var_pat) = if self.at(&Tok::Comma) && matches!(self.peek_at(1), Tok::Ident(_) | Tok::LParen) {
                    self.bump();
                    let (second, second_pat) = self.loop_var(LOOPS)?;
                    if first_pat.is_some() {
                        return Err(Diag::new("E0101", "the position of `for i, x in xs` is a name, not a pattern", span)
                            .hint("write the position first, then the element: `for i, (a, b) in pairs`"));
                    }
                    (Some(first), second, second_pat)
                } else {
                    (None, first, first_pat)
                };
                self.expect(Tok::In, "`in`").map_err(|d| d.or_hint(LOOPS))?;
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
                if self.at(&Tok::DotDot) && var_pat.is_some() {
                    return Err(self.unexpected("`{` to start the body of this `for`").hint(
                        "a range gives one number at a time: a pattern like `(a, b)` goes with an array of tuples, `for (a, b) in pairs`",
                    ));
                }
                if self.at(&Tok::DotDot) && index.is_some() {
                    return Err(self.unexpected("`{` to start the body of this `for`").hint(
                        "`for i, x in xs` goes over an array or a string; over a range the variable is already the position: `for i in a..b`",
                    ));
                }
                if self.at(&Tok::DotDot) {
                    self.bump();
                    let end = self.expr()?;
                    let step = if matches!(self.peek(), Tok::Ident(w) if w == "step") {
                        self.bump();
                        Some(self.expr()?)
                    } else {
                        None
                    };
                    let body = self.loop_body("for")?;
                    StmtKind::For { var, start, end, step, body }
                } else {
                    let mut body = self.loop_body("for")?;
                    if let Some(pat) = &var_pat {
                        // the names of the pattern are `let`s at the start of each round
                        let mut binds = Vec::new();
                        self.bind(pat, &var, false, span, &mut binds);
                        binds.append(&mut body);
                        body = binds;
                    }
                    StmtKind::ForEach { var, index, iter: start, body }
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
            Tok::Match => {
                self.bump();
                return self.match_stmt(span);
            }
            Tok::Ret => {
                self.bump();
                let value = if matches!(self.peek(), Tok::Newline | Tok::RBrace | Tok::Eof) { None } else { Some(self.expr()?) };
                self.end_stmt()?;
                StmtKind::Ret(value)
            }
            first => {
                let first = match first {
                    Tok::Ident(w) => Some((w, span)),
                    _ => None,
                };
                let e = self.expr()?;
                // `(a, b) = (b, a)`: every target gets its part of the value
                if matches!(e.kind, ExprKind::Tuple(_)) && self.at(&Tok::Assign) {
                    self.bump();
                    let value = self.expr()?;
                    self.end_stmt()?;
                    let ExprKind::Tuple(targets) = e.kind else { unreachable!("matched above") };
                    let tmp = self.hidden_name();
                    let mut rest = Vec::new();
                    self.assign_parts(targets, &tmp, span, &mut rest);
                    self.queue.append(&mut rest);
                    return Ok(Stmt { kind: StmtKind::Let { name: tmp, mutable: false, ty: None, value }, span });
                }
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

    /// `match value { pattern, pattern => body  _ => body }` after the word `match`.
    fn match_stmt(&mut self, span: Span) -> PResult<Stmt> {
        let scrut = self.expr()?;
        self.expect(Tok::LBrace, "`{` to start the arms of this `match`").map_err(|d| {
            d.or_hint("a match looks like `match d { Dir.N => print(\"north\") _ => print(\"other\") }`, one arm per line")
        })?;
        let open = self.pos - 1;
        let mut arms = Vec::new();
        loop {
            self.skip_newlines();
            if self.at(&Tok::RBrace) {
                self.bump();
                break;
            }
            if matches!(self.peek(), Tok::Eof | Tok::Fn | Tok::Struct | Tok::Enum) {
                return Err(self.unexpected("`}` to close the `match`").hint("add the missing `}` after the last arm"));
            }
            match self.match_arm() {
                Ok(arm) => arms.push(arm),
                Err(d) => {
                    // one mistake, one error: skip the rest of the match
                    self.errs.push(d);
                    self.queue.clear();
                    let mut depth = 0usize;
                    let mut k = open;
                    while k < self.toks.len() - 1 {
                        match self.toks[k].tok {
                            Tok::LBrace => depth += 1,
                            Tok::RBrace => {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                            }
                            _ => {}
                        }
                        k += 1;
                    }
                    self.pos = (k + 1).min(self.toks.len() - 1);
                    break;
                }
            }
        }
        self.end_stmt()?;
        Ok(Stmt { kind: StmtKind::Match { scrut, arms }, span })
    }

    /// One arm of a `match`: `pattern, pattern => body`.
    fn match_arm(&mut self) -> PResult<MatchArm> {
        let arm_span = self.span();
        let mut pats = Vec::new();
        let mut wild = false;
        loop {
            match (self.peek().clone(), self.peek_at(1).clone()) {
                (Tok::Ident(w), Tok::FatArrow | Tok::Comma) if w == "_" => {
                    self.bump();
                    wild = true;
                }
                // a bare name: the checker explains what to write (`Dir.N`)
                (Tok::Ident(w), Tok::FatArrow | Tok::Comma) => {
                    let at = self.bump().span;
                    pats.push(Expr::new(ExprKind::Var(w), at));
                }
                _ => pats.push(self.expr()?),
            }
            if self.at(&Tok::Comma) {
                self.bump();
                continue;
            }
            break;
        }
        self.expect(Tok::FatArrow, "`=>` after the pattern")
            .map_err(|d| d.or_hint("an arm looks like `Dir.N => print(\"north\")`, or `Dir.E, Dir.W => { ... }`"))?;
        let body = if self.at(&Tok::LBrace) {
            let b = self.block("match arm")?;
            if !matches!(self.peek(), Tok::Newline | Tok::RBrace) {
                return Err(self.unexpected("a new line after the arm").hint("one arm per line"));
            }
            b
        } else {
            let first = self.stmt()?;
            let mut b = vec![first];
            b.append(&mut self.queue);
            b
        };
        Ok(MatchArm { pats, wild, body, span: arm_span })
    }

    /// A name no program can write, for the value a pattern takes apart.
    fn hidden_name(&mut self) -> String {
        self.hidden += 1;
        format!("\u{b7}t{}", self.hidden)
    }

    /// `a`, `_` or `(a, (b, _))`.
    fn pattern(&mut self, hint: &str) -> PResult<Pat> {
        self.same_depth(|p| {
            p.deeper()?;
            if !p.at(&Tok::LParen) {
                let (name, span) = p.ident("a name or a tuple pattern", hint)?;
                return Ok(if name == "_" { Pat::Wild } else { Pat::Name(name, span) });
            }
            let open = p.bump().span;
            let mut items = vec![p.pattern(hint)?];
            while p.at(&Tok::Comma) {
                p.bump();
                items.push(p.pattern(hint)?);
            }
            p.expect(Tok::RParen, "`)` to close the pattern").map_err(|d| d.or_hint(hint.to_string()))?;
            if items.len() < 2 {
                return Err(Diag::new("E0101", "a tuple pattern names at least two values", open)
                    .hint("write `(a, b)`: one name per element of the tuple, `_` for one you do not need"));
            }
            Ok(Pat::Tuple(items))
        })
    }

    /// The variable of a `for`: a name, or a pattern (then the loop variable is a hidden name).
    fn loop_var(&mut self, hint: &str) -> PResult<(String, Option<Pat>)> {
        if !self.at(&Tok::LParen) {
            return Ok((self.ident("a loop variable", hint)?.0, None));
        }
        let pat = self.pattern(hint)?;
        Ok((self.hidden_name().replace("t", "l"), Some(pat)))
    }

    /// `let (a, b) = value` / `var (a, b) = value`: a hidden `let` of the value, then one
    /// declaration per name (queued).
    fn let_pattern(&mut self, mutable: bool, word: &str, span: Span) -> PResult<Stmt> {
        let hint = format!("a pattern looks like `{word} (a, b) = f()`");
        let pat = self.pattern(&hint)?;
        let ty = if self.at(&Tok::Colon) {
            self.bump();
            Some(self.ty("write the tuple type after the colon: `let (a, b): (int, str) = f()`")?)
        } else {
            None
        };
        if !self.at(&Tok::Assign) {
            return Err(self.unexpected("`=`").or_hint(hint));
        }
        self.bump();
        let value = self.expr()?;
        self.end_stmt()?;
        let tmp = self.hidden_name();
        let mut rest = Vec::new();
        self.bind(&pat, &tmp, mutable, span, &mut rest);
        self.queue.append(&mut rest);
        Ok(Stmt { kind: StmtKind::Let { name: tmp, mutable: false, ty, value }, span })
    }

    /// Declarations for the names of `pat`, which takes apart the value of the variable `src`.
    fn bind(&mut self, pat: &Pat, src: &str, mutable: bool, span: Span, out: &mut Vec<Stmt>) {
        let Pat::Tuple(items) = pat else { return };
        let n = items.len();
        for (i, item) in items.iter().enumerate() {
            let part = |name: &str| part_of(name, i, n, span);
            match item {
                Pat::Wild => {}
                Pat::Name(name, at) => {
                    out.push(Stmt { kind: StmtKind::Let { name: name.clone(), mutable, ty: None, value: part(src) }, span: *at });
                }
                Pat::Tuple(_) => {
                    let inner = self.hidden_name();
                    out.push(Stmt { kind: StmtKind::Let { name: inner.clone(), mutable: false, ty: None, value: part(src) }, span });
                    self.bind(item, &inner, mutable, span, out);
                }
            }
        }
    }

    /// `(a, b) = value`: `a = src.0`, `b = src.1` (a nested tuple takes a hidden variable).
    fn assign_parts(&mut self, targets: Vec<Expr>, src: &str, span: Span, out: &mut Vec<Stmt>) {
        let n = targets.len();
        for (i, target) in targets.into_iter().enumerate() {
            let part = |name: &str| part_of(name, i, n, span);
            match target.kind {
                ExprKind::Var(ref n) if n == "_" => {}
                ExprKind::Tuple(inner_targets) => {
                    let inner = self.hidden_name();
                    out.push(Stmt { kind: StmtKind::Let { name: inner.clone(), mutable: false, ty: None, value: part(src) }, span });
                    self.assign_parts(inner_targets, &inner, span, out);
                }
                _ => {
                    let at = target.span;
                    out.push(Stmt { kind: StmtKind::Assign { target, op: None, value: part(src) }, span: at });
                }
            }
        }
    }

    /// The body of a loop: `break` and `continue` are allowed inside.
    fn loop_body(&mut self, what: &str) -> PResult<Vec<Stmt>> {
        self.loop_depth += 1;
        let body = self.block(what);
        self.loop_depth -= 1;
        body
    }

    fn if_stmt(&mut self) -> PResult<Stmt> {
        let mut stmts = self.same_depth(|p| p.if_chain())?;
        let first = stmts.remove(0);
        self.queue.append(&mut stmts);
        Ok(first)
    }

    /// `if`, and each `else if` one level deeper (the rest of the chain is inside the `else`).
    /// `if let v = opt { ... }` is a hidden `let` of `opt`, then an `if` on whether it holds a
    /// value, whose block starts with `let v = <the value>`: the hidden `let` comes first.
    fn if_chain(&mut self) -> PResult<Vec<Stmt>> {
        let span = self.expect(Tok::If, "`if`")?;
        let mut stmts = Vec::new();
        let mut bound = None;
        let cond = if self.at(&Tok::Let) {
            self.bump();
            let hint = "`if let v = expr {`: the name takes the value when there is one";
            let (name, nspan) = self.ident("a name for the value", hint)?;
            self.expect(Tok::Assign, "`=`").map_err(|d| d.or_hint(hint))?;
            let value = self.expr()?;
            let tmp = self.hidden_name().replace('t', "o");
            stmts.push(Stmt { kind: StmtKind::Let { name: tmp.clone(), mutable: false, ty: None, value }, span });
            bound = Some((name, nspan, tmp.clone()));
            Expr::new(ExprKind::Field(Box::new(Expr::new(ExprKind::Var(tmp), span)), "has".to_string()), span)
        } else {
            self.expr()?
        };
        let mut then = self.block("if")?;
        if let Some((name, nspan, tmp)) = bound {
            let value = Expr::new(ExprKind::Field(Box::new(Expr::new(ExprKind::Var(tmp), span)), "val".to_string()), span);
            then.insert(0, Stmt { kind: StmtKind::Let { name, mutable: false, ty: None, value }, span: nspan });
        }
        // `else` may sit on the line after the closing `}`.
        let save = self.pos;
        self.skip_newlines();
        let els = if self.at(&Tok::Else) {
            self.bump();
            if self.at(&Tok::If) {
                self.deeper()?;
                Some(self.if_chain()?)
            } else {
                Some(self.block("else")?)
            }
        } else {
            self.pos = save;
            None
        };
        stmts.push(Stmt { kind: StmtKind::If { cond, then, els }, span });
        Ok(stmts)
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
            Tok::Plus => (BinOp::Add, 6),
            Tok::Minus => (BinOp::Sub, 6),
            Tok::Star => (BinOp::Mul, 7),
            Tok::Slash => (BinOp::Div, 7),
            Tok::Percent => (BinOp::Mod, 7),
            _ => return None,
        })
    }

    fn binary(&mut self, min_prec: u8) -> PResult<Expr> {
        // each operator of a chain nests the expression one level deeper
        self.same_depth(|p| {
            let mut lhs = p.unary()?;
            loop {
                // `x in xs` ranks with the comparisons
                if p.at(&Tok::In) && 4 >= min_prec {
                    p.deeper()?;
                    let span = p.bump().span;
                    p.skip_newlines();
                    let rhs = p.binary(5)?;
                    lhs = Expr::new(ExprKind::In(Box::new(lhs), Box::new(rhs)), span);
                    continue;
                }
                // `a ?? b` ranks above the comparisons and below `+`; it groups to the right
                if p.at(&Tok::Coalesce) && 5 >= min_prec {
                    p.deeper()?;
                    let span = p.bump().span;
                    p.skip_newlines();
                    let rhs = p.binary(5)?;
                    lhs = Expr::new(ExprKind::Coalesce(Box::new(lhs), Box::new(rhs)), span);
                    continue;
                }
                let Some((op, prec)) = Self::binop(p.peek()) else { break };
                if prec < min_prec {
                    break;
                }
                p.deeper()?;
                let span = p.bump().span;
                p.skip_newlines();
                let rhs = p.binary(prec + 1)?;
                lhs = Expr::new(ExprKind::Binary(op, Box::new(lhs), Box::new(rhs)), span);
            }
            Ok(lhs)
        })
    }

    fn unary(&mut self) -> PResult<Expr> {
        self.same_depth(|p| {
            p.deeper()?;
            p.unary_inner()
        })
    }

    fn unary_inner(&mut self) -> PResult<Expr> {
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
        self.same_depth(|p| p.if_expr_chain())
    }

    fn if_expr_chain(&mut self) -> PResult<Expr> {
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
        let els = if self.at(&Tok::If) {
            self.deeper()?;
            self.if_expr_chain()?
        } else {
            self.branch_expr()?
        };
        Ok(Expr::new(ExprKind::If(Box::new(cond), Box::new(then), Box::new(els)), span))
    }

    /// `{ expr }`: one branch of an `if` used as a value.
    fn branch_expr(&mut self) -> PResult<Expr> {
        self.expect(Tok::LBrace, "`{`")
            .map_err(|d| d.or_hint("each branch of an `if` used as a value is written in braces: `if c { a } else { b }`"))?;
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
                format!("each branch of an `if` used as a value must be exactly one expression, found {} after it", self.found()),
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
        // `{x:>8}`: the value, then the format specifier
        let (code, spec_text) = match split_spec(code) {
            Some(i) => (&code[..i], Some(&code[i + 1..])),
            None => (code, None),
        };
        let shift = |s: Span| Span { line: base.line, col: base.col + s.col - 1 };
        let spec = false;
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
        let mut sub = Parser {
            toks,
            structs: Vec::new(),
            pos: 0,
            errs: Vec::new(),
            in_string: true,
            cut_blocks: 0,
            loop_depth: 0,
            nested: Vec::new(),
            depth: self.depth,
            too_deep: None,
            queue: Vec::new(),
            hidden: 0,
        };
        let e = sub.expr();
        self.errs.append(&mut sub.errs);
        if let Some(at) = sub.too_deep {
            // the code in the string was nested too deeply: stop here too
            self.too_deep.get_or_insert(at);
            self.pos = self.toks.len() - 1;
        }
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
        match spec_text.filter(|t| !t.is_empty()) {
            Some(text) => {
                let spec = parse_spec(text, base)?;
                Ok(Expr::new(ExprKind::Fmt(Box::new(e), spec), base))
            }
            None => Ok(e),
        }
    }

    /// The rest of a map literal `[k: v, k2: v2]`, after its first key (the next token is `:`).
    fn map_literal(&mut self, first: Expr, open: Span, span: Span) -> PResult<Expr> {
        let mut pairs = Vec::new();
        let mut key = first;
        loop {
            self.expect(Tok::Colon, "`:` after the key").map_err(|d| d.or_hint("a map literal is written `[\"a\": 1, \"b\": 2]`"))?;
            self.skip_newlines();
            let value = self.expr()?;
            pairs.push((key, value));
            self.skip_newlines();
            match self.peek() {
                Tok::Comma => {
                    self.bump();
                    self.skip_newlines();
                    if self.at(&Tok::RBracket) {
                        self.bump();
                        break;
                    }
                    key = self.expr()?;
                    self.skip_newlines();
                }
                Tok::RBracket => {
                    self.bump();
                    break;
                }
                _ => {
                    return Err(self
                        .unexpected(&format!("`,` or `]` in the map opened at {}:{}", open.line, open.col))
                        .or_hint("separate the entries with commas: `[\"a\": 1, \"b\": 2]`"))
                }
            }
        }
        Ok(Expr::new(ExprKind::MapLit(pairs), span))
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
                // `[:]`: an empty map
                if self.at(&Tok::Colon) && matches!(self.peek_at(1), Tok::RBracket) {
                    self.bump();
                    self.bump();
                    return Ok(Expr::new(ExprKind::MapLit(Vec::new()), span));
                }
                let mut items = Vec::new();
                loop {
                    self.skip_newlines();
                    if self.at(&Tok::RBracket) {
                        self.bump();
                        break;
                    }
                    let item = self.expr()?;
                    // `[k: v, ...]`: a map literal
                    if items.is_empty() && self.at(&Tok::Colon) {
                        return self.map_literal(item, open, span);
                    }
                    items.push(item);
                    self.skip_newlines();
                    if items.len() == 1 && self.at(&Tok::For) {
                        let elem = items.pop().expect("one element");
                        return self.comprehension(elem, open, span);
                    }
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
            Tok::Ident(name) if name == "none" && !matches!(self.peek_at(1), Tok::LParen | Tok::FatArrow) => {
                self.bump();
                ExprKind::None
            }
            Tok::Ident(name) if matches!(self.peek_at(1), Tok::FatArrow) => {
                self.bump();
                return self.lambda(vec![(name, span)], span);
            }
            // `lambda x: x * 2` (Python)
            Tok::Ident(name) if name == "lambda" && matches!(self.peek_at(1), Tok::Ident(_) | Tok::Colon) => {
                return Err(self.python_lambda());
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
            Tok::LParen if self.lambda_ahead() => {
                // `(a, b) => body`
                self.bump();
                let mut params = Vec::new();
                while !self.at(&Tok::RParen) {
                    let p = self.bump();
                    if let Tok::Ident(n) = p.tok {
                        params.push((n, p.span));
                    }
                }
                self.bump();
                return self.lambda(params, span);
            }
            Tok::LParen => {
                let open = self.bump().span;
                let e = self.expr()?;
                // `(a, b)`: a comma makes a tuple
                if self.at(&Tok::Comma) {
                    let mut items = vec![e];
                    while self.at(&Tok::Comma) {
                        self.bump();
                        items.push(self.expr()?);
                    }
                    self.expect(Tok::RParen, &format!("`)` to close the tuple opened at {}:{}", open.line, open.col))
                        .map_err(|d| d.or_hint("separate the values with commas: `(1, \"a\")`"))?;
                    return self.postfix(Expr::new(ExprKind::Tuple(items), open));
                }
                self.expect(Tok::RParen, &format!("`)` to close the `(` opened at {}:{}", open.line, open.col))
                    .map_err(|d| d.or_hint("add the missing `)`: every `(` needs a matching `)`"))?;
                return self.postfix(e);
            }
            Tok::If => return self.if_expr(),
            _ => return Err(self.expression_expected()),
        };
        self.postfix(Expr::new(kind, span))
    }

    /// `[elem for x in src if cond]` after `elem`, at `for`.
    fn comprehension(&mut self, elem: Expr, open: Span, span: Span) -> PResult<Expr> {
        self.bump();
        let (var, vspan) = self.ident("a loop variable", "a comprehension looks like `[x * x for x in xs if x > 0]`")?;
        if self.at(&Tok::Comma) {
            return Err(self
                .unexpected("`in`")
                .hint("a comprehension has one variable; for the position too, use a loop: `for i, x in xs { ... }`"));
        }
        self.expect(Tok::In, "`in`").map_err(|d| d.or_hint("a comprehension looks like `[x * x for x in xs if x > 0]`"))?;
        let first = self.expr()?;
        let src = if self.at(&Tok::DotDot) {
            self.bump();
            let end = self.expr()?;
            let step = if matches!(self.peek(), Tok::Ident(w) if w == "step") {
                self.bump();
                Some(self.expr()?)
            } else {
                None
            };
            CompSrc::Range(first, end, step)
        } else {
            CompSrc::Each(first)
        };
        self.skip_newlines();
        let cond = if self.at(&Tok::If) {
            self.bump();
            Some(self.expr()?)
        } else {
            None
        };
        self.skip_newlines();
        if !self.at(&Tok::RBracket) {
            let d = self.unexpected(&format!("`]` to close the comprehension opened at {}:{}", open.line, open.col));
            return Err(if self.at(&Tok::For) {
                d.hint("a comprehension has one `for`: for nested loops write `for` statements and `push`")
            } else {
                d.or_hint("a comprehension looks like `[x * x for x in xs if x > 0]`")
            });
        }
        self.bump();
        let comp = Comp { elem, var: [(var, vspan)], src, cond };
        self.postfix(Expr::new(ExprKind::Comprehension(Box::new(comp)), span))
    }

    /// True at `(a, b) =>`: the parameters of a lambda (names separated by commas).
    fn lambda_ahead(&self) -> bool {
        let mut k = 1;
        loop {
            match self.peek_at(k) {
                Tok::RParen => return matches!(self.peek_at(k + 1), Tok::FatArrow),
                Tok::Ident(_) => {}
                _ => return false,
            }
            match self.peek_at(k + 1) {
                Tok::Comma => k += 2,
                Tok::RParen => return matches!(self.peek_at(k + 2), Tok::FatArrow),
                _ => return false,
            }
        }
    }

    /// The `=>` and the body of a lambda whose parameters are read.
    fn lambda(&mut self, params: Vec<(String, Span)>, span: Span) -> PResult<Expr> {
        self.expect(Tok::FatArrow, "`=>`")?;
        let body = self.expr()?;
        Ok(Expr::new(ExprKind::Lambda(params, Box::new(body)), span))
    }

    /// `lambda x: x * 2` at the word `lambda`: Nyra writes `x => x * 2`.
    fn python_lambda(&self) -> Diag {
        let at = self.span();
        let d = Diag::new("E0101", "`lambda` is not part of Nyra: a lambda is written `x => x * 2`", at);
        let generic = "write the parameters, `=>` and the body: `x => x * 2` or `(a, b) => a + b`";
        // `lambda a, b:` becomes `(a, b) =>`
        let mut names = Vec::new();
        let mut k = 1;
        loop {
            let Tok::Ident(n) = self.peek_at(k) else { return d.hint(generic) };
            names.push(n.clone());
            match self.peek_at(k + 1) {
                Tok::Comma => k += 2,
                Tok::Colon => break,
                _ => return d.hint(generic),
            }
        }
        let Some(colon) = self.toks.get(self.pos + k + 1).filter(|t| t.span.line == at.line) else { return d.hint(generic) };
        let params = if names.len() == 1 { names[0].clone() } else { format!("({})", names.join(", ")) };
        let old = format!("lambda {}:", names.join(", "));
        d.hint(format!("write `{params} => ...`: the parameters, `=>`, then the body")).fix(vec![Edit::range(
            at,
            after(colon.span, ":"),
            &old,
            format!("{params} =>"),
        )])
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
    fn postfix(&mut self, e: Expr) -> PResult<Expr> {
        self.same_depth(|p| p.postfix_chain(e))
    }

    /// `.field`, `.method(..)` and `[index]` after a value, each one level deeper.
    fn postfix_chain(&mut self, mut e: Expr) -> PResult<Expr> {
        loop {
            if matches!(self.peek(), Tok::Dot | Tok::LBracket) {
                self.deeper()?;
            }
            match self.peek() {
                Tok::Dot => {
                    self.bump();
                    let span = self.span();
                    // `t.0`: the position in a tuple
                    if let Tok::Int(n) = self.peek().clone() {
                        self.bump();
                        e = Expr::new(ExprKind::Field(Box::new(e), n.to_string()), span);
                        continue;
                    }
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
                    // `xs[..b]`
                    let lo = if self.at(&Tok::DotDot) { None } else { Some(self.expr()?) };
                    self.skip_newlines();
                    // `xs[a..b]`, `xs[a..]`, `xs[..b]`
                    if self.at(&Tok::DotDot) {
                        self.bump();
                        self.skip_newlines();
                        let hi = if self.at(&Tok::RBracket) { None } else { Some(Box::new(self.expr()?)) };
                        self.skip_newlines();
                        self.expect(Tok::RBracket, "`]` to close the slice")
                            .map_err(|d| d.or_hint("a slice is written `xs[a..b]`"))?;
                        e = Expr::new(ExprKind::Slice(Box::new(e), lo.map(Box::new), hi), span);
                        continue;
                    }
                    let index = lo.expect("an index starts here");
                    self.expect(Tok::RBracket, "`]` to close the index").map_err(|d| d.or_hint("an index is written `xs[i]`"))?;
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
            let fix = (n >= 2 && ends(n + 1))
                .then(|| counter(n - 2))
                .flatten()
                .map(|(at, v)| Edit::range(after(at, &v), after(self.span(), "+"), "++", " += 1"));
            return d.hint("`++` does not exist: write `i += 1`").fix_opt(fix);
        }
        if matches!(t, Tok::Star) && self.glued_to(&Tok::Star) {
            return d.hint("there is no `**` operator: multiply (`x * x * x`) or write a loop (see the `pow` recipe in docs/AI_GUIDE.md section 6)");
        }
        if matches!(t, Tok::Assign) && self.glued_to(&Tok::DotDot) {
            return d.hint("`..=` does not exist: the end of a range is exclusive, so `0..10` counts 0 to 9");
        }
        if matches!(t, Tok::Newline | Tok::RBrace | Tok::Eof | Tok::RParen) && self.double_minus_before() {
            let fix = (n >= 3 && ends(n))
                .then(|| counter(n - 3))
                .flatten()
                .map(|(at, v)| Edit::range(after(at, &v), after(self.toks[n - 1].span, "-"), "--", " -= 1"));
            return d.hint("`--` does not exist: write `i -= 1`").fix_opt(fix);
        }
        let generic = "an expression is a value such as `5`, `x + 1` or `f(x)`";
        match t {
            Tok::Else => d.hint("`else` must follow the `}` of an `if` (`} else {`): look for a missing `}` or a statement between them"),
            Tok::RParen if matches!(before, Some(Tok::LParen)) => {
                d.hint("empty parentheses are not a value: put an expression inside, or remove them")
            }
            Tok::Fn => d.hint(
                "functions are not values: an array method takes a lambda, `xs.map(x => x * 2)`; anything else needs a named `fn` at the top level",
            ),
            Tok::LBrace if before == Some(&Tok::FatArrow) => {
                d.hint("the body of a lambda is one expression, without braces or `ret`: `x => x * 2`")
            }
            Tok::Match => d.hint(
                "`match` is a statement, not a value: `ret` the value or assign it in the arms, or write `if cond { a } else { b }` for a value",
            ),
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

/// Where the format specifier starts in the code of `{ }`: the first `:` outside brackets and
/// string or character literals.
fn split_spec(code: &str) -> Option<usize> {
    let mut depth = 0usize;
    let mut chars = code.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '"' | '\'' => {
                while let Some((_, d)) = chars.next() {
                    if d == '\\' {
                        chars.next();
                    } else if d == c {
                        break;
                    }
                }
            }
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            ':' if depth == 0 => return Some(i),
            _ => {}
        }
    }
    None
}

/// The format specifier `[[fill]align][+][0][width][,][.precision][type]` (Python's, without the
/// parts Nyra has no use for).
fn parse_spec(text: &str, at: Span) -> PResult<FmtSpec> {
    let bad =
        |why: String| Diag::new("E0270", format!("invalid format specifier `{text}` in a string: {why}"), at).hint(FORMAT_SPEC_HINT);
    let cs: Vec<char> = text.chars().collect();
    let mut spec = FmtSpec {
        text: text.to_string(),
        fill: None,
        align: None,
        plus: false,
        zero: false,
        width: 0,
        comma: false,
        prec: None,
        ty: None,
    };
    let mut i = 0;
    let is_align = |c: char| matches!(c, '<' | '>' | '^');
    if cs.len() >= 2 && is_align(cs[1]) {
        spec.fill = Some(cs[0]);
        spec.align = Some(cs[1]);
        i = 2;
    } else if !cs.is_empty() && is_align(cs[0]) {
        spec.align = Some(cs[0]);
        i = 1;
    }
    if cs.get(i) == Some(&'+') {
        spec.plus = true;
        i += 1;
    }
    if cs.get(i) == Some(&'0') {
        spec.zero = true;
        i += 1;
    }
    let digits = |i: &mut usize| -> Option<usize> {
        let start = *i;
        while cs.get(*i).is_some_and(|c| c.is_ascii_digit()) {
            *i += 1;
        }
        (*i > start).then(|| cs[start..*i].iter().collect::<String>().parse::<usize>().unwrap_or(usize::MAX))
    };
    if let Some(w) = digits(&mut i) {
        if w > 100_000 {
            return Err(bad(format!("a width of {w} is too large (the most is 100000)")));
        }
        spec.width = w;
    }
    if cs.get(i) == Some(&',') {
        spec.comma = true;
        i += 1;
    }
    if cs.get(i) == Some(&'.') {
        i += 1;
        match digits(&mut i) {
            Some(p) if p <= 100 => spec.prec = Some(p),
            Some(p) => return Err(bad(format!("{p} decimals are too many (the most is 100)"))),
            None => return Err(bad("the `.` needs the number of decimals after it: `.2`".to_string())),
        }
    }
    if let Some(&t) = cs.get(i) {
        if matches!(t, 'f' | 'd' | 's') {
            spec.ty = Some(t);
            i += 1;
        }
    }
    if i < cs.len() {
        let rest: String = cs[i..].iter().collect();
        return Err(bad(format!("`{rest}` is not understood here (the order is fill and align, `+`, `0`, width, `,`, `.decimals`)")));
    }
    Ok(spec)
}

/// `name.i`: the i-th part of a tuple held by the variable `name`.
/// The field is named `i/n`: the pattern has `n` parts, which the checker compares with the tuple.
fn part_of(name: &str, i: usize, n: usize, span: Span) -> Expr {
    Expr::new(ExprKind::Field(Box::new(Expr::new(ExprKind::Var(name.to_string()), span)), format!("{i}/{n}")), span)
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
