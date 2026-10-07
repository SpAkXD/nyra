//! Type checker. Annotates every expression with its type and collects all
//! errors in one pass. Nyra never converts types implicitly and never allows
//! shadowing: one name means one thing. Every error names the variables, functions
//! and types involved and says how to fix the program.

use std::collections::HashMap;

use crate::ast::*;
use crate::diag::{suggest, Diag};
use crate::hints;

pub const BUILTINS: &[&str] = &["print", "int", "float"];

struct Sig {
    params: Vec<Type>,
    names: Vec<String>,
    ret: Type,
    span: Span,
}

impl Sig {
    /// `add(a: int, b: int)`
    fn show(&self, name: &str) -> String {
        let ps: Vec<String> = self.names.iter().zip(&self.params).map(|(n, t)| format!("{n}: {}", t.name())).collect();
        format!("{name}({})", ps.join(", "))
    }
}

/// How a name was introduced. The messages differ for parameters, loop variables and `let`/`var`.
#[derive(Clone, Copy, PartialEq)]
enum Decl {
    Let,
    Var,
    Param,
    Loop,
}

struct Var {
    ty: Type,
    decl: Decl,
    span: Span,
}

/// Where a value is used, for the "type mismatch in ..." message.
enum Ctx<'a> {
    Let(&'a str, Type),
    Assign(&'a str),
    Arg { f: &'a str, idx: usize, param: &'a str },
    Ret,
    Range(&'static str),
}

struct Checker {
    fns: HashMap<String, Sig>,
    scopes: Vec<HashMap<String, Var>>,
    errs: Vec<Diag>,
    ret: Type,
    /// The function being checked.
    fname: String,
    /// Every `let`, `var` and loop variable of that function, for "declared later" hints.
    decls: Vec<(String, Span)>,
    /// The `+` nodes of the `a + b + c` chain being checked, and the string that joins its parts
    /// with `{ }` (if that can be written), so an error on any `+` of the chain suggests the whole fix.
    chain: Option<(Vec<Span>, Option<String>)>,
}

pub fn check(prog: &mut Program) -> Vec<Diag> {
    let mut c = Checker {
        fns: HashMap::new(),
        scopes: Vec::new(),
        errs: Vec::new(),
        ret: Type::Void,
        fname: String::new(),
        decls: Vec::new(),
        chain: None,
    };

    for f in &prog.funcs {
        if BUILTINS.contains(&f.name.as_str()) {
            c.errs.push(
                Diag::new("E0206", format!("`{}` is a builtin function and cannot be defined again", f.name), f.span)
                    .hint(format!("give your function another name, e.g. `fn my_{}(...)`", f.name)),
            );
        } else if let Some(first) = c.fns.get(&f.name) {
            c.errs.push(
                Diag::new(
                    "E0206",
                    format!("function `{}` is defined twice (the first one is on line {})", f.name, first.span.line),
                    f.span,
                )
                .hint(format!(
                    "rename one of them, e.g. `{}_2`: Nyra has no overloading, so a function name can be used only once",
                    f.name
                )),
            );
        } else {
            let sig = Sig {
                params: f.params.iter().map(|p| p.ty).collect(),
                names: f.params.iter().map(|p| p.name.clone()).collect(),
                ret: f.ret,
                span: f.span,
            };
            c.fns.insert(f.name.clone(), sig);
        }
    }

    match prog.funcs.iter().find(|f| f.name == "main") {
        None => c.errs.push(
            Diag::new("E0208", "missing `fn main()`: a program starts running at `main`", Span { line: 1, col: 1 })
                .hint("add the entry point: `fn main() { print(\"hello\") }`"),
        ),
        Some(f) if !f.params.is_empty() || f.ret != Type::Void => {
            let mut problems = Vec::new();
            if !f.params.is_empty() {
                let ps: Vec<String> = f.params.iter().map(|p| format!("`{}: {}`", p.name, p.ty.name())).collect();
                problems.push(format!("takes {}", ps.join(", ")));
            }
            if f.ret != Type::Void {
                problems.push(format!("returns `{}`", f.ret.name()));
            }
            c.errs.push(
                Diag::new(
                    "E0211",
                    format!("`main` must take no parameters and return nothing, but it {}", problems.join(" and ")),
                    f.span,
                )
                .hint("write `fn main() { ... }`: put the values it needs inside it with `let`, and stop early with a plain `ret`"),
            );
        }
        _ => {}
    }

    for f in &mut prog.funcs {
        c.func(f);
    }
    c.errs
}

/// True if every path through the block ends in `ret`.
fn returns(b: &[Stmt]) -> bool {
    match b.last().map(|s| &s.kind) {
        Some(StmtKind::Ret(_)) => true,
        Some(StmtKind::If { then, els: Some(e), .. }) => returns(then) && returns(e),
        _ => false,
    }
}

/// Every name a function declares, with the position of its declaration.
fn collect_decls(b: &[Stmt], out: &mut Vec<(String, Span)>) {
    for s in b {
        match &s.kind {
            StmtKind::Let { name, .. } => out.push((name.clone(), s.span)),
            StmtKind::If { then, els, .. } => {
                collect_decls(then, out);
                if let Some(e) = els {
                    collect_decls(e, out);
                }
            }
            StmtKind::While { body, .. } => collect_decls(body, out),
            StmtKind::For { var, body, .. } => {
                out.push((var.clone(), s.span));
                collect_decls(body, out);
            }
            _ => {}
        }
    }
}

// ---- small helpers for readable messages -----------------------------------

/// Source-like text of a small expression, for hints. `None` when it is long or complicated.
fn show(e: &Expr) -> Option<String> {
    let s = match &e.kind {
        ExprKind::Int(n) => n.to_string(),
        ExprKind::Float(f) => {
            let t = format!("{f:?}");
            if t.contains(['e', 'n', 'i']) {
                return None;
            }
            t
        }
        ExprKind::Bool(b) => b.to_string(),
        ExprKind::Str(s) if s.len() <= 16 && !s.contains(['"', '\\', '\n', '\t', '\r']) => format!("\"{s}\""),
        ExprKind::Str(_) | ExprKind::Interp(_) | ExprKind::If(..) => return None,
        ExprKind::Var(n) => n.clone(),
        ExprKind::Call(n, args) => {
            let a: Option<Vec<String>> = args.iter().map(show).collect();
            format!("{n}({})", a?.join(", "))
        }
        ExprKind::Unary(op, x) => format!("{}{}", if *op == UnOp::Neg { "-" } else { "!" }, operand(x)?),
        ExprKind::Binary(op, l, r) => format!("{} {} {}", operand(l)?, op.symbol(), operand(r)?),
    };
    (s.len() <= 48).then_some(s)
}

/// `show`, in parentheses when it is itself an operator expression.
fn operand(e: &Expr) -> Option<String> {
    let s = show(e)?;
    Some(if matches!(e.kind, ExprKind::Binary(..)) { format!("({s})") } else { s })
}

/// The text of a string built from these parts with `{ }`, if every part can be written inside one.
fn template(parts: &[&Expr]) -> Option<String> {
    let mut out = String::new();
    for p in parts {
        match &p.kind {
            ExprKind::Str(s) if !s.contains(['"', '\\', '\n', '\t', '\r']) => out += &s.replace('{', "{{").replace('}', "}}"),
            _ => {
                let s = show(p)?;
                if s.contains('"') {
                    return None;
                }
                out += &format!("{{{s}}}");
            }
        }
    }
    Some(out)
}

fn count(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

fn was_were(n: usize) -> &'static str {
    if n == 1 {
        "was"
    } else {
        "were"
    }
}

/// "an `int`", "a `float`"
fn article(t: Type) -> String {
    format!("{} `{}`", if t == Type::Int { "an" } else { "a" }, t.name())
}

/// A value of the type, for "write `ret 0`" hints.
fn sample(t: Type) -> &'static str {
    match t {
        Type::Int => "0",
        Type::Float => "0.0",
        Type::Bool => "false",
        Type::Str => "\"\"",
        _ => "...",
    }
}

/// The text of a string literal that is a number of the wanted type (`"3"` for `int`), as a number literal.
fn number_text(e: &Expr, want: Type) -> Option<String> {
    let ExprKind::Str(text) = &e.kind else { return None };
    let t = text.trim();
    if t.is_empty() || !t.chars().all(|c| c.is_ascii_digit() || c == '.' || c == '-') {
        return None;
    }
    match want {
        Type::Int => t.parse::<i64>().ok().map(|n| n.to_string()),
        Type::Float if t.parse::<f64>().is_ok() => Some(if t.contains('.') { t.to_string() } else { format!("{t}.0") }),
        _ => None,
    }
}

/// `show` of the callee if the expression is a call, else a generic phrase.
fn call_text(e: &Expr) -> String {
    match (&e.kind, show(e)) {
        (_, Some(s)) if matches!(e.kind, ExprKind::Call(..)) => format!("`{s}`"),
        (ExprKind::Call(n, _), _) => format!("`{n}(...)`"),
        _ => "this expression".to_string(),
    }
}

impl Checker {
    fn lookup(&self, name: &str) -> Option<&Var> {
        self.scopes.iter().rev().find_map(|s| s.get(name))
    }

    fn undefined_var(&self, name: &str, span: Span, assigning: bool) -> Diag {
        let d = Diag::new("E0201", format!("undefined variable `{name}`"), span);
        // the name of a function: the call parentheses are probably missing
        if let Some(sig) = self.fns.get(name) {
            let args = if sig.params.is_empty() { String::new() } else { sig.names.join(", ") };
            return d.hint(format!("`{name}` is a function, not a variable: call it as `{name}({args})`"));
        }
        if BUILTINS.contains(&name) {
            return d.hint(format!("`{name}` is a function, not a variable: call it as `{name}(x)`"));
        }
        // declared somewhere else in this function: say where, and why it is not visible here
        let first = self.decls.iter().filter(|(n, _)| n == name).map(|(_, s)| *s).min_by_key(|s| (s.line, s.col));
        if let Some(at) = first {
            return d.hint(if (at.line, at.col) < (span.line, span.col) {
                format!(
                    "`{name}` was declared on line {} inside a block that has already ended: declare it before that block to use it here",
                    at.line
                )
            } else {
                format!("`{name}` is declared later, on line {}: move that declaration above this line", at.line)
            });
        }
        if let Some(h) = hints::undefined_variable(name) {
            return d.hint(h);
        }
        if let Some(h) = suggest(name, self.scopes.iter().flat_map(|s| s.keys().map(String::as_str))) {
            return d.hint(h);
        }
        d.hint(if assigning {
            format!("declare it first with `var {name} = ...`: `{name} = ...` can only change a variable that exists")
        } else {
            format!("declare it first: `let {name} = ...`")
        })
    }

    fn declare(&mut self, name: &str, ty: Type, decl: Decl, span: Span) {
        let prev = self.lookup(name).map(|v| (v.decl, v.span));
        if let Some((pdecl, pspan)) = prev {
            let (msg, hint) = match pdecl {
                Decl::Let => (
                    format!("`{name}` is already declared on line {} (Nyra has no shadowing)", pspan.line),
                    format!(
                        "use a different name for the new variable, e.g. `{name}2`; to change the old one, declare it with `var` and assign: `{name} = ...`"
                    ),
                ),
                Decl::Var => (
                    format!("`{name}` is already declared on line {} (Nyra has no shadowing)", pspan.line),
                    format!("assign to the existing variable (`{name} = ...`) instead of declaring it again, or use a different name"),
                ),
                Decl::Param => (
                    format!("`{name}` is already a parameter of `{}` (Nyra has no shadowing)", self.fname),
                    "use a different name: parameters cannot be declared again or changed (copy one into a `var` to change it)".to_string(),
                ),
                Decl::Loop => (
                    format!("`{name}` is already the loop variable of an enclosing `for` (line {})", pspan.line),
                    format!("use a different name for the inner variable, e.g. `{}`", if name == "i" { "j" } else { "inner" }),
                ),
            };
            self.errs.push(Diag::new("E0206", msg, span).hint(hint));
        } else if let Some(sig) = self.fns.get(name) {
            self.errs.push(
                Diag::new("E0206", format!("`{name}` is already the name of a function (line {})", sig.span.line), span)
                    .hint(format!("a variable cannot share a function's name: rename the variable, e.g. `{name}_value`")),
            );
        } else if BUILTINS.contains(&name) {
            self.errs.push(
                Diag::new("E0206", format!("`{name}` is already the name of a builtin function"), span)
                    .hint(format!("a variable cannot share a builtin's name: rename the variable, e.g. `{name}_value`")),
            );
        }
        self.scopes.last_mut().unwrap().insert(name.to_string(), Var { ty, decl, span });
    }

    /// A value of type `got` is used where `want` is needed. `e` is the expression, for the hint.
    fn expect_ty(&mut self, want: Type, got: Type, e: &Expr, ctx: Ctx) {
        if got == want || got == Type::Unknown || want == Type::Unknown {
            return;
        }
        let place = match &ctx {
            Ctx::Let(name, ty) => format!("`let {name}: {}`", ty.name()),
            Ctx::Assign(name) => format!("the assignment to `{name}`"),
            Ctx::Arg { f, idx, param } => format!("argument {} (`{param}`) of `{f}`", idx + 1),
            Ctx::Ret => format!("the return value of `{}`", self.fname),
            Ctx::Range(which) => format!("the {which} of a range"),
        };
        if got == Type::Void {
            self.errs.push(
                Diag::new(
                    "E0203",
                    format!("type mismatch in {place}: expected `{}`, but {} returns nothing", want.name(), call_text(e)),
                    e.span,
                )
                .hint(self.no_value_hint(e)),
            );
            return;
        }
        let hint = self.mismatch_hint(want, got, e, &ctx);
        self.errs.push(
            Diag::new(
                "E0203",
                format!("type mismatch in {place}: expected `{}`, found `{}`", want.name(), got.name()),
                e.span,
            )
            .hint(hint),
        );
    }

    /// Why a call that returns nothing cannot give a value, and what to do.
    fn no_value_hint(&self, e: &Expr) -> String {
        match &e.kind {
            ExprKind::Call(n, _) if self.fns.contains_key(n.as_str()) => format!(
                "`{n}` has no return type, so it returns nothing: call it on its own line, or declare `fn {n}(...) -> int` and `ret` a value"
            ),
            ExprKind::Call(n, _) => format!("`{n}` only has an effect and returns nothing: use it as a statement on its own line"),
            _ => "only a function with a return type (`-> int`) has a value".to_string(),
        }
    }

    fn mismatch_hint(&self, want: Type, got: Type, e: &Expr, ctx: &Ctx) -> String {
        use Type::{Bool, Float, Int, Str};
        let s = show(e);
        match (want, got) {
            (Float, Int) => match (&e.kind, &s) {
                (ExprKind::Int(n), _) => return format!("write the number as a float: `{n}.0`"),
                (_, Some(s)) => return format!("convert it with `float`: `float({s})`"),
                _ => return "convert it with `float(...)`".to_string(),
            },
            (Int, Float) => match &s {
                Some(s) => return format!("convert it with `int`: `int({s})` (truncates toward zero), or write an int"),
                None => return "convert it with `int(...)` (truncates toward zero)".to_string(),
            },
            (Str, Int | Float | Bool) => {
                return match &s {
                    Some(s) if !s.contains('"') => format!("build text with interpolation: `\"{{{s}}}\"`"),
                    _ => "build text with interpolation: store the value in a variable `x`, then write `\"{x}\"`".to_string(),
                }
            }
            (Int | Float, Str) => {
                if let Some(n) = number_text(e, want) {
                    return format!("write the number without quotes: `{n}`");
                }
            }
            (Bool, Int | Float | Str) if matches!(e.kind, ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Str(_)) => {
                return "write `true` or `false`: a `bool` is never made from a number or text".to_string()
            }
            (Bool, Int) | (Bool, Float) | (Bool, Str) => {
                let zero = match got {
                    Int => "0",
                    Float => "0.0",
                    _ => "\"\"",
                };
                return match &s {
                    Some(s) => format!("compare it to get a `bool`: `{s} != {zero}`"),
                    None => format!("compare it to get a `bool`: `x != {zero}`"),
                };
            }
            (Int, Bool) | (Float, Bool) => {
                let (one, zero) = if want == Int { ("1", "0") } else { ("1.0", "0.0") };
                return match &s {
                    Some(s) => format!("choose the numbers yourself: `if {s} {{ {one} }} else {{ {zero} }}`"),
                    None => format!("choose the numbers yourself: `if flag {{ {one} }} else {{ {zero} }}`"),
                };
            }
            _ => {}
        }
        let advice = match ctx {
            Ctx::Let(name, _) => format!("change the value to {}, or change the declared type: `let {name}: {} = ...`", article(want), got.name()),
            Ctx::Assign(name) => format!("`{name}` holds {} values: assign {}, or declare `{name}` with the type you need", want.name(), article(want)),
            Ctx::Arg { f, param, .. } => format!("pass {} for `{param}`, or change the parameter type in `fn {f}`", article(want)),
            Ctx::Ret => format!("return {}, or change the return type of `{}` to `{}`", article(want), self.fname, got.name()),
            Ctx::Range(_) => "the bounds of a range are `int` values: `for i in 0..10`".to_string(),
        };
        if got == Str && matches!(want, Int | Float) {
            format!("text is not a number and Nyra cannot parse text into numbers (yet): {advice}")
        } else {
            advice
        }
    }

    /// An `if` or `while` condition.
    fn cond(&mut self, t: Type, e: &Expr, what: &str) {
        if t == Type::Bool || t == Type::Unknown {
            return;
        }
        let s = show(e);
        let hint = match (t, &s) {
            (Type::Void, _) => self.no_value_hint(e),
            (Type::Int, _) if matches!(e.kind, ExprKind::Int(_)) => {
                "a condition is `true`, `false` or a comparison such as `x != 0`".to_string()
            }
            (Type::Int, Some(s)) => format!("compare it: `{s} != 0`"),
            (Type::Float, Some(s)) => format!("compare it: `{s} != 0.0`"),
            (Type::Str, Some(s)) => format!("compare it: `{s} != \"\"`"),
            _ => "compare explicitly, e.g. `x != 0`".to_string(),
        };
        self.errs.push(
            Diag::new("E0209", format!("the condition of {what} must be `bool`, found `{}`", t.name()), e.span).hint(hint),
        );
    }

    fn func(&mut self, f: &mut Func) {
        self.ret = f.ret;
        self.fname = f.name.clone();
        self.decls.clear();
        collect_decls(&f.body, &mut self.decls);
        self.scopes = vec![HashMap::new()];
        for p in &f.params {
            self.declare(&p.name, p.ty, Decl::Param, p.span);
        }
        self.block(&mut f.body);
        if f.ret != Type::Void && !returns(&f.body) {
            let value = sample(f.ret);
            let hint = match f.body.last().map(|s| &s.kind) {
                Some(StmtKind::Expr(e)) if e.ty == f.ret => match show(e) {
                    Some(s) => format!(
                        "the last line computes a value, but Nyra does not return it by itself: write `ret {s}` (or make this a one-line function: `fn {}(...) -> {} = {s}`)",
                        f.name,
                        f.ret.name()
                    ),
                    None => "the last line computes a value, but Nyra does not return it by itself: start it with `ret`".to_string(),
                },
                Some(StmtKind::If { els: None, .. }) => {
                    format!("the last `if` has no `else`: add `ret {value}` after it, or an `else {{ ret {value} }}` branch")
                }
                Some(StmtKind::If { .. }) => "every branch of the last `if`/`else` must end with `ret`".to_string(),
                Some(StmtKind::While { .. } | StmtKind::For { .. }) => {
                    format!("a loop may run zero times: add `ret {value}` after the loop")
                }
                _ => format!("end the function with `ret`, e.g. `ret {value}`"),
            };
            self.errs.push(
                Diag::new(
                    "E0207",
                    format!("function `{}` is declared `-> {}` but not every path ends with `ret`", f.name, f.ret.name()),
                    f.span,
                )
                .hint(hint),
            );
        }
    }

    fn block(&mut self, b: &mut [Stmt]) {
        self.scopes.push(HashMap::new());
        for s in b.iter_mut() {
            self.stmt(s);
        }
        self.scopes.pop();
    }

    fn stmt(&mut self, s: &mut Stmt) {
        let span = s.span;
        match &mut s.kind {
            StmtKind::Let { name, mutable, ty, value } => {
                let got = self.expr(value);
                let mut t = got;
                if got == Type::Void {
                    self.errs.push(
                        Diag::new(
                            "E0203",
                            format!("{} returns nothing, so there is no value to store in `{name}`", call_text(value)),
                            value.span,
                        )
                        .hint(self.no_value_hint(value)),
                    );
                    t = Type::Unknown;
                }
                if let Some(want) = ty {
                    if got != Type::Void {
                        self.expect_ty(*want, got, value, Ctx::Let(name, *want));
                    }
                    t = *want;
                }
                self.declare(name, t, if *mutable { Decl::Var } else { Decl::Let }, span);
            }
            StmtKind::Assign { name, value } => {
                let got = self.expr(value);
                match self.lookup(name).map(|v| (v.ty, v.decl, v.span)) {
                    None => {
                        let d = self.undefined_var(name, span, true);
                        self.errs.push(d);
                    }
                    Some((t, decl, dspan)) => {
                        if decl != Decl::Var {
                            let (msg, hint) = match decl {
                                Decl::Param => (
                                    format!("cannot assign to `{name}`: parameters cannot be changed"),
                                    format!("copy it into a variable first (`var m = {name}`), then change `m`"),
                                ),
                                Decl::Loop => (
                                    format!("cannot assign to `{name}`: it is the loop variable of a `for`"),
                                    "loop variables cannot be changed: use a `while` loop with a `var` counter instead".to_string(),
                                ),
                                _ => (
                                    format!("cannot assign to `{name}`: it was declared with `let` on line {}", dspan.line),
                                    format!("declare it with `var` to make it changeable: `var {name} = ...` on line {}", dspan.line),
                                ),
                            };
                            self.errs.push(Diag::new("E0205", msg, span).hint(hint));
                        }
                        self.expect_ty(t, got, value, Ctx::Assign(name));
                    }
                }
            }
            StmtKind::If { cond, then, els } => {
                let t = self.expr(cond);
                self.cond(t, cond, "`if`");
                self.block(then);
                if let Some(e) = els {
                    self.block(e);
                }
            }
            StmtKind::While { cond, body } => {
                let t = self.expr(cond);
                self.cond(t, cond, "`while`");
                self.block(body);
            }
            StmtKind::For { var, start, end, body } => {
                let a = self.expr(start);
                self.expect_ty(Type::Int, a, start, Ctx::Range("start"));
                let b = self.expr(end);
                self.expect_ty(Type::Int, b, end, Ctx::Range("end"));
                self.scopes.push(HashMap::new());
                self.declare(var, Type::Int, Decl::Loop, span);
                self.block(body);
                self.scopes.pop();
            }
            StmtKind::Ret(value) => match value {
                Some(e) => {
                    let t = self.expr(e);
                    if self.ret == Type::Void {
                        let shown = if t == Type::Unknown || t == Type::Void { "int" } else { t.name() };
                        self.errs.push(
                            Diag::new(
                                "E0207",
                                format!("`ret` returns a value, but `{}` has no return type (it returns nothing)", self.fname),
                                e.span,
                            )
                            .hint(format!(
                                "add the return type to the signature: `fn {}(...) -> {shown}`, or write `ret` without a value",
                                self.fname
                            )),
                        );
                    } else {
                        let want = self.ret;
                        self.expect_ty(want, t, e, Ctx::Ret);
                    }
                }
                None => {
                    if self.ret != Type::Void {
                        self.errs.push(
                            Diag::new(
                                "E0207",
                                format!("`ret` needs a value: `{}` returns `{}`", self.fname, self.ret.name()),
                                span,
                            )
                            .hint(format!("write `ret {}` (or any other `{}` value)", sample(self.ret), self.ret.name())),
                        );
                    }
                }
            },
            StmtKind::Expr(e) => {
                self.expr(e);
            }
        }
    }

    fn expr(&mut self, e: &mut Expr) -> Type {
        let span = e.span;
        let t = match &mut e.kind {
            ExprKind::Int(_) => Type::Int,
            ExprKind::Float(_) => Type::Float,
            ExprKind::Bool(_) => Type::Bool,
            ExprKind::Str(_) => Type::Str,
            ExprKind::Interp(parts) => {
                for p in parts.iter_mut() {
                    if let InterpPart::Expr(x) = p {
                        if self.expr(x) == Type::Void {
                            self.errs.push(
                                Diag::new(
                                    "E0203",
                                    format!("{} returns nothing, so it cannot be put into a string", call_text(x)),
                                    x.span,
                                )
                                .hint("only values (`int`, `float`, `bool`, `str`) can go inside `{ }`: call it on its own line before the string"),
                            );
                        }
                    }
                }
                Type::Str
            }
            ExprKind::Var(name) => match self.lookup(name) {
                Some(v) => v.ty,
                None => {
                    let d = self.undefined_var(name, span, false);
                    self.errs.push(d);
                    Type::Unknown
                }
            },
            ExprKind::Unary(op, inner) => {
                let t = self.expr(inner);
                match (*op, t) {
                    (_, Type::Unknown) => Type::Unknown,
                    (UnOp::Neg, Type::Int | Type::Float) => t,
                    (UnOp::Not, Type::Bool) => Type::Bool,
                    (op, t) => {
                        let s = show(inner);
                        let (msg, hint) = if op == UnOp::Neg {
                            (
                                format!("cannot apply `-` to `{}`: `-` needs an `int` or a `float`", t.name()),
                                if t == Type::Bool {
                                    format!("to flip a `bool` use `!`: `!{}`", s.unwrap_or_else(|| "flag".into()))
                                } else if t == Type::Void {
                                    self.no_value_hint(inner)
                                } else {
                                    "only numbers can be negated".to_string()
                                },
                            )
                        } else {
                            (
                                format!("cannot apply `!` to `{}`: `!` needs a `bool`", t.name()),
                                match (t, s) {
                                    (Type::Int, Some(s)) => format!("compare instead: `{s} == 0`"),
                                    (Type::Float, Some(s)) => format!("compare instead: `{s} == 0.0`"),
                                    (Type::Str, Some(s)) => format!("compare instead: `{s} == \"\"`"),
                                    (Type::Void, _) => self.no_value_hint(inner),
                                    _ => "`!` flips a `bool`: compare first, e.g. `!(x > 0)`".to_string(),
                                },
                            )
                        };
                        self.errs.push(Diag::new("E0210", msg, span).hint(hint));
                        Type::Unknown
                    }
                }
            }
            ExprKind::Binary(op, l, r) => {
                let outer = *op == BinOp::Add && self.chain.is_none();
                if outer {
                    let (mut parts, mut nodes) = (Vec::new(), vec![span]);
                    flatten_add(l, &mut parts, &mut nodes);
                    flatten_add(r, &mut parts, &mut nodes);
                    self.chain = Some((nodes, template(&parts)));
                }
                let lt = self.expr(l);
                let rt = self.expr(r);
                let t = self.binary(*op, lt, rt, span, l, r);
                if outer {
                    self.chain = None;
                }
                t
            }
            ExprKind::Call(name, args) => self.call(name, args, span),
            ExprKind::If(cond, a, b) => {
                let c = self.expr(cond);
                self.cond(c, cond, "an `if` value");
                let (ta, tb) = (self.expr(a), self.expr(b));
                if ta == Type::Void || tb == Type::Void {
                    let (which, branch) = if ta == Type::Void { ("first", &**a) } else { ("second", &**b) };
                    self.errs.push(
                        Diag::new(
                            "E0212",
                            format!("the {which} branch of this `if` value produces no value: {} returns nothing", call_text(branch)),
                            span,
                        )
                        .hint("each branch must be an expression with a value, e.g. `if c { 1 } else { 2 }`; use an `if` statement for actions"),
                    );
                    Type::Unknown
                } else if ta == Type::Unknown || tb == Type::Unknown {
                    Type::Unknown
                } else if ta != tb {
                    let hint = match (ta, tb) {
                        (Type::Int, Type::Float) => match (&a.kind, show(a)) {
                            (ExprKind::Int(n), _) => format!("make both branches `float`: write the first as `{n}.0`"),
                            (_, Some(s)) => format!("make both branches `float`: convert the first with `float({s})`"),
                            _ => "make both branches `float`: convert the first with `float(...)`".to_string(),
                        },
                        (Type::Float, Type::Int) => match (&b.kind, show(b)) {
                            (ExprKind::Int(n), _) => format!("make both branches `float`: write the second as `{n}.0`"),
                            (_, Some(s)) => format!("make both branches `float`: convert the second with `float({s})`"),
                            _ => "make both branches `float`: convert the second with `float(...)`".to_string(),
                        },
                        _ => format!("both branches must have the same type: change one so both are `{}` (or both `{}`)", ta.name(), tb.name()),
                    };
                    self.errs.push(
                        Diag::new(
                            "E0212",
                            format!("the branches of this `if` value have different types: `{}` and `{}`", ta.name(), tb.name()),
                            span,
                        )
                        .hint(hint),
                    );
                    Type::Unknown
                } else {
                    ta
                }
            }
        };
        e.ty = t;
        t
    }

    fn binary(&mut self, op: BinOp, l: Type, r: Type, span: Span, le: &Expr, re: &Expr) -> Type {
        use Type::{Bool, Float, Int, Str, Unknown, Void};
        if l == Unknown || r == Unknown {
            return Unknown;
        }
        let res = match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => match (l, r) {
                (Int, Int) => Some(Int),
                (Float, Float) => Some(Float),
                _ => None,
            },
            BinOp::Mod => (l == Int && r == Int).then_some(Int),
            BinOp::Eq | BinOp::Ne => (l == r && l != Void).then_some(Bool),
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => match (l, r) {
                (Int, Int) | (Float, Float) => Some(Bool),
                _ => None,
            },
            BinOp::And | BinOp::Or => (l == Bool && r == Bool).then_some(Bool),
        };
        if let Some(t) = res {
            return t;
        }

        // `x += e` is `x = x + e` with the left side at the statement's own position
        let compound = matches!(le.kind, ExprKind::Var(_)) && le.span == span;
        let sym = if compound { format!("{}=", op.symbol()) } else { op.symbol().to_string() };
        let needs = match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                "two `int`s or two `float`s"
            }
            BinOp::Mod => "two `int`s",
            BinOp::Eq | BinOp::Ne => "two values of the same type",
            BinOp::And | BinOp::Or => "two `bool`s",
        };
        let msg = format!("cannot use `{sym}` on `{}` and `{}`: `{sym}` needs {needs}", l.name(), r.name());
        let (ls, rs) = (show(le), show(re));
        let arithmetic = matches!(
            op,
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge | BinOp::Eq | BinOp::Ne
        );
        let chained = matches!(
            (op, &le.kind),
            (BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge, ExprKind::Binary(BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge, ..))
        );
        let hint: String = if l == Void || r == Void {
            self.no_value_hint(if l == Void { le } else { re })
        } else if chained {
            match (&le.kind, &ls, &rs) {
                (ExprKind::Binary(lop, ll, lr), _, Some(rs)) => match (operand(ll), operand(lr)) {
                    (Some(a), Some(b)) => format!("comparisons do not chain: write `{a} {} {b} && {b} {} {rs}`", lop.symbol(), op.symbol()),
                    _ => "comparisons do not chain: write `a < b && b < c`".to_string(),
                },
                _ => "comparisons do not chain: write `a < b && b < c`".to_string(),
            }
        } else if matches!((l, r), (Int, Float) | (Float, Int)) && arithmetic {
            let int_left = l == Int;
            if compound {
                let name = ls.clone().unwrap_or_else(|| "x".into());
                match (int_left, &rs) {
                    (true, Some(rs)) => format!("`{name}` is an `int`: convert the value (`{name} {sym} int({rs})` truncates toward zero) or declare `{name}` as a float"),
                    (false, _) if matches!(re.kind, ExprKind::Int(_)) => {
                        let n = if let ExprKind::Int(n) = re.kind { n } else { 0 };
                        format!("`{name}` is a `float`: write the value as a float, `{name} {sym} {n}.0`")
                    }
                    (false, Some(rs)) => format!("`{name}` is a `float`: convert the value, `{name} {sym} float({rs})`"),
                    _ => "Nyra never converts numbers implicitly: use `float(x)` or `int(x)` on the value".to_string(),
                }
            } else {
                let fix = |e: &Expr, s: &Option<String>| -> Option<String> {
                    match (&e.kind, s) {
                        (ExprKind::Int(n), _) => Some(format!("{n}.0")),
                        (_, Some(s)) => Some(format!("float({s})")),
                        _ => None,
                    }
                };
                let (a, b) = if int_left { (fix(le, &ls), operand(re)) } else { (operand(le), fix(re, &rs)) };
                match (a, b) {
                    (Some(a), Some(b)) => format!("use one type on both sides, e.g. `{a} {sym} {b}` (Nyra never converts numbers implicitly)"),
                    _ => "Nyra never converts numbers implicitly: use `float(x)` on the `int` side, or `int(x)` on the `float` side".to_string(),
                }
            }
        } else if (op == BinOp::Add) && (l == Str || r == Str) {
            let whole = self.chain.as_ref().filter(|(nodes, _)| nodes.contains(&span)).and_then(|(_, t)| t.clone());
            match whole {
                Some(t) => format!("Nyra cannot add strings: join text with interpolation, e.g. `\"{t}\"`"),
                None => "Nyra cannot add strings: join text with interpolation, e.g. `\"{a}{b}\"`".to_string(),
            }
        } else if matches!(op, BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge) && l == Str && r == Str {
            "strings can only be compared with `==` and `!=`: there is no ordering for text".to_string()
        } else if op == BinOp::Mod && l == Float && r == Float {
            "`%` works on `int` only; for floats compute the remainder as `a - b * float(int(a / b))`".to_string()
        } else if matches!(op, BinOp::And | BinOp::Or) {
            let fix = |e: &Expr, t: Type, s: &Option<String>| -> Option<String> {
                let s = s.as_ref()?;
                let simple = matches!(
                    e.kind,
                    ExprKind::Var(_) | ExprKind::Call(..) | ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Str(_) | ExprKind::Bool(_)
                );
                let wrapped = if simple { s.clone() } else { format!("({s})") };
                match t {
                    Bool => Some(s.clone()),
                    Int => Some(format!("{wrapped} != 0")),
                    Float => Some(format!("{wrapped} != 0.0")),
                    Str => Some(format!("{wrapped} != \"\"")),
                    _ => None,
                }
            };
            match (fix(le, l, &ls), fix(re, r, &rs)) {
                (Some(a), Some(b)) => format!("compare each side first: `{a} {sym} {b}`"),
                _ => format!("both sides of `{sym}` must be `bool`: compare each side first, e.g. `x != 0 {sym} y != 0`"),
            }
        } else if matches!(op, BinOp::Eq | BinOp::Ne) {
            if l == Bool || r == Bool {
                "compare a `bool` only with another `bool` (`flag == true`, or just `flag`)".to_string()
            } else {
                format!("`{sym}` compares two values of one type: convert one side (`float(x)`, `int(x)`) or write the literal with the right type")
            }
        } else {
            format!("both sides of `{sym}` need the same numeric type: check the types of the two operands")
        };
        self.errs.push(Diag::new("E0210", msg, span).hint(hint));
        Unknown
    }

    fn call(&mut self, name: &str, args: &mut [Expr], span: Span) -> Type {
        let tys: Vec<Type> = args.iter_mut().map(|a| self.expr(a)).collect();

        if BUILTINS.contains(&name) {
            let ret = match name {
                "print" => Type::Void,
                "int" => Type::Int,
                _ => Type::Float,
            };
            if tys.len() != 1 {
                let hint = match name {
                    "print" if tys.is_empty() => "give it a value to print: `print(\"hello\")`".to_string(),
                    "print" => {
                        let parts: Vec<&Expr> = args.iter().collect();
                        // `print(a, b)` -> one string with a space between the parts
                        let spaced: Vec<String> = parts.iter().filter_map(|p| template(&[*p])).collect();
                        if spaced.len() == parts.len() {
                            format!("`print` takes one value: put the parts in one string, `print(\"{}\")`", spaced.join(" "))
                        } else {
                            "`print` takes one value: put the parts in one string with interpolation, `print(\"{a} {b}\")`".to_string()
                        }
                    }
                    _ => format!("`{name}` converts one value: `{name}(x)`"),
                };
                self.errs.push(
                    Diag::new(
                        "E0204",
                        format!("`{name}` takes exactly 1 argument but {} {} given", tys.len(), was_were(tys.len())),
                        span,
                    )
                    .hint(hint),
                );
                return ret;
            }
            let t = tys[0];
            match (name, t) {
                (_, Type::Unknown) => {}
                ("print", Type::Void) => self.errs.push(
                    Diag::new(
                        "E0203",
                        format!("`print` needs a value to show, but {} returns nothing", call_text(&args[0])),
                        args[0].span,
                    )
                    .hint(self.no_value_hint(&args[0])),
                ),
                ("print", _) => {}
                (_, Type::Int | Type::Float) => {}
                (_, Type::Void) => self.errs.push(
                    Diag::new(
                        "E0203",
                        format!("`{name}` needs a number, but {} returns nothing", call_text(&args[0])),
                        args[0].span,
                    )
                    .hint(self.no_value_hint(&args[0])),
                ),
                (_, got) => {
                    let (one, zero) = if name == "int" { ("1", "0") } else { ("1.0", "0.0") };
                    let hint = match (got, show(&args[0])) {
                        (Type::Bool, Some(s)) => format!("choose the numbers yourself: `if {s} {{ {one} }} else {{ {zero} }}`"),
                        (Type::Bool, None) => format!("choose the numbers yourself: `if flag {{ {one} }} else {{ {zero} }}`"),
                        (Type::Str, _) => match number_text(&args[0], if name == "int" { Type::Int } else { Type::Float }) {
                            Some(n) => format!("write the number without quotes: `{n}`"),
                            None => "Nyra cannot turn text into a number (yet): use a number instead of the text".to_string(),
                        },
                        _ => "only numbers can be converted".to_string(),
                    };
                    self.errs.push(
                        Diag::new(
                            "E0203",
                            format!("`{name}(x)` needs an `int` or a `float`, found `{}`", got.name()),
                            args[0].span,
                        )
                        .hint(hint),
                    );
                }
            }
            return ret;
        }

        let Some(sig) = self.fns.get(name) else {
            let d = Diag::new("E0202", format!("undefined function `{name}`"), span);
            let names = self.fns.keys().map(String::as_str).chain(BUILTINS.iter().copied());
            let hint = if self.lookup(name).is_some() {
                format!("`{name}` is a variable, not a function: remove the parentheses (or give the function another name)")
            } else if let Some(h) = hints::undefined_function(name) {
                h
            } else if let Some(h) = suggest(name, names) {
                h
            } else {
                format!("define it: `fn {name}(...) {{ ... }}` (the only builtins are `print`, `int` and `float`)")
            };
            self.errs.push(d.hint(hint));
            return Type::Unknown;
        };
        let (params, names, ret, shown) = (sig.params.clone(), sig.names.clone(), sig.ret, sig.show(name));
        if params.len() != tys.len() {
            let hint = if tys.len() < params.len() {
                let missing: Vec<String> = names[tys.len()..]
                    .iter()
                    .zip(&params[tys.len()..])
                    .map(|(n, t)| format!("`{n}: {}`", t.name()))
                    .collect();
                format!("also pass {}: the call is `{name}({})`", missing.join(", "), names.join(", "))
            } else {
                format!("remove the extra argument(s): the signature is `fn {shown}`")
            };
            self.errs.push(
                Diag::new(
                    "E0204",
                    format!(
                        "`{shown}` takes {} but {} {} given",
                        count(params.len(), "argument"),
                        tys.len(),
                        was_were(tys.len())
                    ),
                    span,
                )
                .hint(hint),
            );
        } else {
            for (i, (want, got)) in params.iter().zip(&tys).enumerate() {
                self.expect_ty(*want, *got, &args[i], Ctx::Arg { f: name, idx: i, param: &names[i] });
            }
        }
        ret
    }
}

/// The operands of a chain of `+`, left to right, and the position of each `+`.
fn flatten_add<'a>(e: &'a Expr, parts: &mut Vec<&'a Expr>, nodes: &mut Vec<Span>) {
    match &e.kind {
        ExprKind::Binary(BinOp::Add, l, r) => {
            nodes.push(e.span);
            flatten_add(l, parts, nodes);
            flatten_add(r, parts, nodes);
        }
        _ => parts.push(e),
    }
}
