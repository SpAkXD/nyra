//! Type checker. Annotates every expression with its type and collects all
//! errors in one pass. Nyra never converts types implicitly and never allows
//! shadowing: one name means one thing. Every error names the variables, functions
//! and types involved and says how to fix the program.

use std::collections::HashMap;

use crate::ast::*;
use crate::diag::{after, suggest, suggest_fix, Diag, Edit};
use crate::hints;
use crate::stdlib;
use data::StructInfo;

pub mod data;
mod globals;
mod lambda;

pub const BUILTINS: &[&str] = &["print", "int", "float", "str", "char", "free", "keep"];
/// Builtins a program may also define itself (its own definition wins).
pub const MATH: &[&str] = &["abs", "min", "max"];

struct Sig {
    params: Vec<Type>,
    names: Vec<String>,
    inout: Vec<bool>,
    ret: Type,
    span: Span,
}

impl Sig {
    /// `add(a: int, b: int)`
    fn show(&self, name: &str) -> String {
        let ps: Vec<String> = self
            .names
            .iter()
            .zip(&self.params)
            .zip(&self.inout)
            .map(|((n, t), io)| format!("{}{n}: {}", if *io { "inout " } else { "" }, t.name()))
            .collect();
        format!("{name}({})", ps.join(", "))
    }
}

/// How a name was introduced. The messages differ for parameters, loop variables and `let`/`var`.
#[derive(Clone, Copy, PartialEq)]
enum Decl {
    Let,
    Var,
    Param,
    /// an `inout` parameter: changeable, like a `var`
    Inout,
    Loop,
    /// a parameter of a lambda (`x` in `x => x * 2`)
    Lambda,
}

struct Var {
    ty: Type,
    decl: Decl,
    span: Span,
    /// How many `arena` blocks enclose the declaration.
    arena: usize,
}

/// Where a value is used, for the "type mismatch in ..." message.
enum Ctx<'a> {
    Let(&'a str, Type),
    Assign(&'a str),
    /// assignment to a field or an element: the target as written
    AssignTo(String),
    Arg {
        f: &'a str,
        idx: usize,
        param: &'a str,
    },
    MethodArg {
        m: &'a str,
        idx: usize,
    },
    Field {
        s: &'a str,
        f: &'a str,
    },
    Ret,
    Range(&'static str),
}

/// The state of a variable after `free`: where it was freed, and whether only on some paths.
#[derive(Clone, Copy, PartialEq)]
struct Freed {
    line: usize,
    maybe: bool,
}

struct Checker {
    fns: HashMap<String, Sig>,
    structs: HashMap<String, StructInfo>,
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
    /// How many `arena` blocks enclose the current statement.
    arena_depth: usize,
    /// Variables that were freed with `free(x)` and not assigned since.
    freed: HashMap<String, Freed>,
    /// How many lambdas enclose the current expression: inside one, nothing can be changed.
    lambda_depth: usize,
    /// The imported standard modules and where their `use` line is.
    modules: Vec<(String, Span)>,
    /// Script variables: what functions see of them and do with them (see `globals.rs`).
    g: globals::State,
}

pub fn check(prog: &mut Program) -> Vec<Diag> {
    let mut c = Checker {
        fns: HashMap::new(),
        structs: HashMap::new(),
        scopes: Vec::new(),
        errs: Vec::new(),
        ret: Type::Void,
        fname: String::new(),
        decls: Vec::new(),
        chain: None,
        arena_depth: 0,
        freed: HashMap::new(),
        lambda_depth: 0,
        modules: Vec::new(),
        g: globals::State::new(prog),
    };

    // the imported modules: their intrinsics are functions named `module.name`
    for u in &prog.uses {
        if stdlib::is_module(&u.module) && !c.modules.iter().any(|(m, _)| *m == u.module) {
            c.modules.push((u.module.clone(), u.span));
            for f in stdlib::StdFn::ALL.iter().filter(|f| f.path().0 == u.module) {
                let sig = Sig {
                    params: f.params().iter().map(|(_, t)| t.ty()).collect(),
                    names: f.params().iter().map(|(n, _)| n.to_string()).collect(),
                    inout: vec![false; f.params().len()],
                    ret: f.ret().ty(),
                    span: u.span,
                };
                c.fns.insert(f.full_name().to_string(), sig);
            }
        }
    }

    // structs first: functions and bodies refer to them
    for sd in &prog.structs {
        if !sd.name.starts_with(|ch: char| ch.is_uppercase()) {
            let mut fixed = sd.name.clone();
            if let Some(first) = fixed.get(..1) {
                fixed = first.to_uppercase() + &fixed[1..];
            }
            // the uses of the old name are fixed after it (they then differ from the struct only in case)
            let free = !prog.structs.iter().any(|o| o.name == fixed) && !prog.funcs.iter().any(|f| f.name == fixed);
            c.errs.push(
                Diag::new("E0221", format!("struct name `{}` must start with an uppercase letter", sd.name), sd.span)
                    .hint(format!("write `struct {fixed}`: type names start uppercase, variables and functions lowercase"))
                    .fix_opt(free.then(|| Edit::replace(sd.span, &sd.name, fixed.clone()))),
            );
        }
        if let Some(first) = c.structs.get(&sd.name) {
            c.errs.push(
                Diag::new(
                    "E0206",
                    format!("struct `{}` is defined twice (the first one is on line {})", sd.name, first.span.line),
                    sd.span,
                )
                .hint("rename one of them: a struct name can be used only once"),
            );
            continue;
        }
        let mut fields: Vec<(String, Type, Span)> = Vec::new();
        for f in &sd.fields {
            if let Some((_, _, at)) = fields.iter().find(|(n, _, _)| *n == f.name) {
                c.errs.push(
                    Diag::new(
                        "E0220",
                        format!("field `{}` is defined twice in struct `{}` (first on line {})", f.name, sd.name, at.line),
                        f.span,
                    )
                    .hint(format!("rename one of them, e.g. `{}2`, or remove the duplicate", f.name)),
                );
                continue;
            }
            fields.push((f.name.clone(), f.ty, f.span));
        }
        c.structs.insert(sd.name.clone(), StructInfo { fields, span: sd.span });
    }
    for sd in &prog.structs {
        for f in &sd.fields {
            c.check_type(f.ty, f.span);
        }
        if c.structs.contains_key(&sd.name) && data::contains_itself(&sd.name, &c.structs) {
            c.errs.push(
                Diag::new("E0222", format!("struct `{}` contains itself, so its size would be infinite", sd.name), sd.span).hint(
                    format!("store the nested values in an array instead, e.g. `children: [{}]` (an array can be empty)", sd.name),
                ),
            );
        }
    }

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
        } else if let Some(sd) = c.structs.get(&f.name) {
            c.errs.push(
                Diag::new("E0206", format!("`{}` is already the name of a struct (line {})", f.name, sd.span.line), f.span)
                    .hint("a function cannot share a struct's name: rename the function (functions start lowercase)"),
            );
        } else {
            // a type that is not defined is reported here, once: the calls then see it as unknown
            let params: Vec<Type> = f.params.iter().map(|p| if c.check_type(p.ty, p.span) { p.ty } else { Type::Unknown }).collect();
            let ret = if f.ret == Type::Void || c.check_type(f.ret, f.span) { f.ret } else { Type::Unknown };
            let sig = Sig {
                params,
                names: f.params.iter().map(|p| p.name.clone()).collect(),
                inout: f.params.iter().map(|p| p.inout).collect(),
                ret,
                span: f.span,
            };
            c.fns.insert(f.name.clone(), sig);
        }
    }

    // a program with statements and its own `fn main` has it under another name (`USER_MAIN`)
    let entry = prog.funcs.iter().find(|f| f.name == USER_MAIN).or_else(|| prog.funcs.iter().find(|f| f.name == "main"));
    match entry {
        None => c.errs.push(
            Diag::new(
                "E0208",
                "nothing to run: the program has no statements at the top level and no `fn main()`",
                Span { line: 1, col: 1 },
            )
            .hint("write the program's statements at the top level, e.g. `print(\"hello\")`: they run in order"),
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
                .hint("write `fn main() { ... }`: put the values it needs inside it with `let`, and stop early with a plain `return`"),
            );
        }
        _ => {}
    }

    // a script's `main` first: its top-level variables (and their types) are what the functions
    // see. Its errors still come where `main` stands among the functions.
    let script_main = if prog.script { prog.funcs.iter().position(|f| f.name == "main") } else { None };
    let mut main_errs = Vec::new();
    if let Some(i) = script_main {
        let before = c.errs.len();
        c.g.in_main = true;
        c.func(&mut prog.funcs[i]);
        c.g.in_main = false;
        main_errs = c.errs.split_off(before);
    }
    for (i, f) in prog.funcs.iter_mut().enumerate() {
        if Some(i) == script_main {
            c.errs.append(&mut main_errs);
        } else if f.name.contains('.') {
            // a function of a bundled module (`math.exp`): an error in it is a bug of the compiler,
            // not of the program, and has no position in the user's file
            let before = c.errs.len();
            c.func(f);
            let module = f.name.split('.').next().unwrap_or_default().to_string();
            for d in &mut c.errs[before..] {
                d.msg = format!("internal error in the standard module `{module}`, function `{}`: {}", f.name, d.msg);
                d.hint = Some(format!(
                    "this is a bug in the compiler, not in your program; please report it (your program's `use {module}` line is where it shows)"
                ));
                d.fix.clear();
            }
        } else {
            c.func(f);
        }
    }
    for ex in &mut prog.examples {
        c.example(&mut ex.expr, ex.forall.as_ref());
    }
    prog.globals = c.finish_globals();
    // the user's `main` is named `USER_MAIN` inside; messages call it `main`
    let hidden = format!("`{USER_MAIN}`");
    for d in &mut c.errs {
        if d.msg.contains(&hidden) {
            d.msg = d.msg.replace(&hidden, "`main`");
        }
        if let Some(h) = d.hint.as_mut().filter(|h| h.contains(&hidden)) {
            *h = h.replace(&hidden, "`main`");
        }
    }
    c.errs
}

/// True if every path through the block ends in `ret`.
fn returns(b: &[Stmt]) -> bool {
    match b.last().map(|s| &s.kind) {
        Some(StmtKind::Ret(_)) => true,
        Some(StmtKind::If { then, els: Some(e), .. }) => returns(then) && returns(e),
        Some(StmtKind::Arena(body)) => returns(body),
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
            StmtKind::While { body, .. } | StmtKind::Arena(body) => collect_decls(body, out),
            StmtKind::For { var, body, .. } => {
                out.push((var.clone(), s.span));
                collect_decls(body, out);
            }
            StmtKind::ForEach { var, index, body, .. } => {
                if let Some(i) = index {
                    out.push((i.clone(), s.span));
                }
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
        ExprKind::Char(c) => data::show_char(*c),
        ExprKind::Str(s) if s.len() <= 16 && !s.contains(['"', '\\', '\n', '\t', '\r']) => format!("\"{s}\""),
        ExprKind::Str(_) | ExprKind::Interp(_) | ExprKind::If(..) | ExprKind::Comprehension(_) | ExprKind::MapLit(_) => return None,
        ExprKind::Var(n) => n.clone(),
        ExprKind::Call(n, args) => {
            let a: Option<Vec<String>> = args.iter().map(show).collect();
            format!("{n}({})", a?.join(", "))
        }
        ExprKind::Unary(op, x) => format!("{}{}", if *op == UnOp::Neg { "-" } else { "!" }, operand(x)?),
        ExprKind::Binary(op, l, r) => format!("{} {} {}", operand(l)?, op.symbol(), operand(r)?),
        ExprKind::Array(items) => {
            let a: Option<Vec<String>> = items.iter().map(show).collect();
            format!("[{}]", a?.join(", "))
        }
        ExprKind::Index(b, i) => format!("{}[{}]", operand(b)?, show(i)?),
        ExprKind::Field(b, f) => format!("{}.{f}", operand(b)?),
        ExprKind::Method(r, m, args) => {
            let a: Option<Vec<String>> = args.iter().map(show).collect();
            format!("{}.{m}({})", operand(r)?, a?.join(", "))
        }
        ExprKind::Labeled(l, v) => format!("{l}: {}", show(v)?),
        ExprKind::Inout(v) => format!("inout {}", show(v)?),
        ExprKind::Lambda(ps, body) => match ps.as_slice() {
            [(p, _)] => format!("{p} => {}", show(body)?),
            _ => format!("({}) => {}", ps.iter().map(|(p, _)| p.as_str()).collect::<Vec<_>>().join(", "), show(body)?),
        },
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
                if s.contains(['"', '\'']) {
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
        Type::Char => "' '",
        Type::Array(_) => "[]",
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

/// The value of a negative int written in the program (`-1`), if the expression is one.
fn negative_const(e: &Expr) -> Option<i64> {
    match &e.kind {
        ExprKind::Unary(UnOp::Neg, x) => match x.kind {
            ExprKind::Int(n) if n > 0 => Some(-n),
            _ => None,
        },
        ExprKind::Int(n) if *n < 0 => Some(*n),
        _ => None,
    }
}

/// Where an expression starts in the source: the span of `a + b` is the `+`, of `xs.len()` the `len`.
/// (A parenthesized expression starts before this; edits that insert here check the text before.)
fn start(e: &Expr) -> Span {
    match &e.kind {
        ExprKind::Binary(_, l, _) => start(l),
        ExprKind::Method(r, ..) | ExprKind::Field(r, _) | ExprKind::Index(r, _) => start(r),
        _ => e.span,
    }
}

/// `2` written as `2.0`, where a `float` is needed: the only change that keeps the number.
fn float_literal(e: &Expr) -> Option<Edit> {
    match e.kind {
        ExprKind::Int(n) => Some(Edit::replace(e.span, &n.to_string(), format!("{n}.0"))),
        _ => None,
    }
}

/// `"a"` written as `'a'`, where a `char` is needed.
fn char_literal(e: &Expr) -> Option<Edit> {
    let ExprKind::Str(text) = &e.kind else { return None };
    let mut cs = text.chars();
    match (cs.next(), cs.next()) {
        (Some(c), None) if !c.is_control() && !matches!(c, '\'' | '\\' | '"' | '{' | '}') => {
            Some(Edit::replace(e.span, &format!("\"{c}\""), format!("'{c}'")))
        }
        _ => None,
    }
}

/// `show` of the callee if the expression is a call, else a generic phrase.
fn call_text(e: &Expr) -> String {
    match (&e.kind, show(e)) {
        (_, Some(s)) if matches!(e.kind, ExprKind::Call(..) | ExprKind::Method(..)) => format!("`{s}`"),
        (ExprKind::Call(n, _), _) => format!("`{n}(...)`"),
        (ExprKind::Method(_, m, _), _) => format!("`.{m}(...)`"),
        _ => "this expression".to_string(),
    }
}

impl Checker {
    /// A variable by name: a local, or in a function, a script variable it can see.
    fn lookup(&self, name: &str) -> Option<&Var> {
        self.local(name).or_else(|| self.global_of(name).map(|g| &self.g.vars[g].1))
    }

    /// A variable of the function being checked.
    fn local(&self, name: &str) -> Option<&Var> {
        self.scopes.iter().rev().find_map(|s| s.get(name))
    }

    /// E0261 for a position that is a negative constant: `xs[-1]`, `s.slice(-2, 5)`. Nyra counts
    /// from 0 and never from the end, so such a position is out of bounds whatever the length is.
    fn negative_position(&mut self, base: &Expr, pos: &Expr, slice: bool) {
        let Some(n) = negative_const(pos) else { return };
        let what = if slice { "a slice position" } else { "an index" };
        // the position counted from the end is spelled with the length; the fix repeats the base,
        // so it is only written for a plain variable
        let var = match &base.kind {
            ExprKind::Var(v) => Some(v.clone()),
            _ => None,
        };
        let from_end = var.as_ref().map(|v| format!("{v}.len() - {}", -n));
        let hint = match (&var, &from_end, slice) {
            (Some(v), Some(e), false) => format!("Nyra has no positions counted from the end: write `{v}[{e}]` for the element {} from the end", -n),
            (Some(_), Some(e), true) => format!("Nyra has no positions counted from the end: write `{e}` (the length minus {})", -n),
            (_, _, false) => "Nyra has no positions counted from the end: write the length minus n, e.g. `xs[xs.len() - 1]` is the last element".to_string(),
            _ => "Nyra has no positions counted from the end: write the length minus n, e.g. `xs.slice(xs.len() - 2, xs.len())` is the last two".to_string(),
        };
        let fix = from_end.map(|e| Edit::replace(pos.span, &format!("-{}", -n), e));
        self.errs.push(
            Diag::new("E0261", format!("{what} cannot be negative: {n} is always out of bounds"), pos.span).hint(hint).fix_opt(fix),
        );
    }

    /// `"${root}/logs"` where `root` is undefined: text written for a template engine (shell,
    /// JavaScript), not a value. The fix doubles the braces so the string keeps the text.
    fn literal_dollar_brace(&mut self, x: &Expr, before: usize) {
        fn path(e: &Expr) -> Option<(String, Span)> {
            match &e.kind {
                ExprKind::Var(n) => Some((n.clone(), e.span)),
                ExprKind::Field(b, f) => path(b).map(|(t, s)| (format!("{t}.{f}"), s)),
                _ => None,
            }
        }
        let Some((text, root)) = path(x) else { return };
        let Some(d) = self.errs[before..].iter_mut().find(|d| d.code == "E0201" && d.span == root && d.fix.is_empty()) else {
            return;
        };
        if root.col < 2 {
            return;
        }
        let open = Span { line: root.line, col: root.col - 1 };
        d.hint = Some(format!(
            "`{{{text}}}` inserts the value of `{text}`; to keep `${{{text}}}` as text, double the braces: `${{{{{text}}}}}`"
        ));
        d.fix = vec![Edit::replace(open, &format!("{{{text}}}"), format!("{{{{{text}}}}}")).after("$")];
    }

    fn undefined_var(&self, name: &str, span: Span, assigning: bool) -> Diag {
        let d = Diag::new("E0201", format!("undefined variable `{name}`"), span);
        // the name of a function: the call parentheses are probably missing
        if let Some(sig) = self.fns.get(name) {
            let args = if sig.params.is_empty() { String::new() } else { sig.names.join(", ") };
            // without parameters there is only one call to write
            let fix = (sig.params.is_empty() && !assigning).then(|| Edit::replace(span, name, format!("{name}()")));
            return d.hint(format!("`{name}` is a function, not a variable: call it as `{name}({args})`")).fix_opt(fix);
        }
        if BUILTINS.contains(&name) {
            return d.hint(format!("`{name}` is a function, not a variable: call it as `{name}(x)`"));
        }
        // a script variable this function cannot see, or a local of `main`
        if let Some(d) = self.global_undefined(name, span) {
            return d;
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
            let to = match name {
                "True" => Some("true"),
                "False" => Some("false"),
                _ => None,
            };
            return d.hint(h).fix_opt(to.map(|t| Edit::replace(span, name, t)));
        }
        if let Some((h, fix)) = suggest_fix(name, self.scopes.iter().flat_map(|s| s.keys().map(String::as_str))) {
            return d.hint(h).fix_opt(fix.map(|f| Edit::replace(span, name, f)));
        }
        d.hint(if assigning {
            format!("declare it first with `var {name} = ...`: `{name} = ...` can only change a variable that exists")
        } else {
            format!("declare it first: `let {name} = ...`")
        })
    }

    fn declare(&mut self, name: &str, ty: Type, decl: Decl, span: Span) {
        // a script variable does not count: declaring the name hides it in this function
        let prev = self.local(name).map(|v| (v.decl, v.span));
        let in_std = self.fname.contains('.');
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
                Decl::Param | Decl::Inout => (
                    format!("`{name}` is already a parameter of `{}` (Nyra has no shadowing)", self.fname),
                    "use a different name: parameters cannot be declared again or changed (copy one into a `var` to change it)".to_string(),
                ),
                Decl::Loop => (
                    format!("`{name}` is already the loop variable of an enclosing `for` (line {})", pspan.line),
                    format!("use a different name for the inner variable, e.g. `{}`", if name == "i" { "j" } else { "inner" }),
                ),
                Decl::Lambda => (
                    format!("`{name}` is already the parameter of an enclosing lambda (line {})", pspan.line),
                    "use a different name for the inner variable or parameter".to_string(),
                ),
            };
            // a lambda's parameter is the easiest one to rename
            let hint = if decl == Decl::Lambda {
                format!(
                    "give the lambda's parameter a name that is not used yet, e.g. `{}`",
                    lambda::fresh_name(name, |n| self.lookup(n).is_some())
                )
            } else {
                hint
            };
            self.errs.push(Diag::new("E0206", msg, span).hint(hint));
        } else if in_std {
            // the names inside a bundled module live in the module's own namespace: they cannot
            // clash with the program's functions, structs and modules
        } else if let Some(sig) = self.fns.get(name) {
            self.errs.push(
                Diag::new("E0206", format!("`{name}` is already the name of a function (line {})", sig.span.line), span)
                    .hint(format!("a variable cannot share a function's name: rename the variable, e.g. `{name}_value`")),
            );
        } else if let Some(sd) = self.structs.get(name) {
            self.errs.push(
                Diag::new("E0206", format!("`{name}` is already the name of a struct (line {})", sd.span.line), span)
                    .hint("variables start lowercase: rename the variable"),
            );
        } else if let Some((_, at)) = self.modules.iter().find(|(m, _)| m == name) {
            self.errs.push(
                Diag::new("E0206", format!("`{name}` is already the name of a module (`use {name}` on line {})", at.line), span)
                    .hint(format!("a variable cannot share a module's name: rename the variable, e.g. `{name}_value`")),
            );
        }
        self.freed.remove(name);
        let arena = self.arena_depth;
        self.scopes.last_mut().unwrap().insert(name.to_string(), Var { ty, decl, span, arena });
    }

    /// Reports struct types that are not defined. True if the type is fine.
    fn check_type(&mut self, t: Type, span: Span) -> bool {
        match t {
            Type::Array(_) => t.elem().is_some_and(|e| self.check_type(e, span)),
            Type::Map(_) => {
                let Some((k, v)) = t.map_kv() else { return false };
                // a bad key is reported, but the type stays usable (no second error for `[:]`)
                self.map_key(k, span);
                self.check_type(v, span)
            }
            Type::Struct(_) => {
                let name = t.struct_name().unwrap_or_default();
                if self.structs.contains_key(&name) {
                    return true;
                }
                let names: Vec<&str> = self.structs.keys().map(String::as_str).collect();
                let hint = if hints::nyra_type(&name).is_some() || hints::is_type_word(&name) {
                    hints::type_name(&name)
                } else if let Some(s) = suggest(&name, names.iter().copied()) {
                    s
                } else if names.is_empty() {
                    format!(
                        "define it: `struct {name} {{ field: int }}`, or use `int`, `float`, `bool`, `str`, `char` or an array `[T]`"
                    )
                } else {
                    let mut sorted = names.clone();
                    sorted.sort();
                    format!(
                        "the structs are {}; or define `struct {name} {{ ... }}`",
                        sorted.iter().map(|n| format!("`{n}`")).collect::<Vec<_>>().join(", ")
                    )
                };
                self.errs
                    .push(Diag::new("E0102", format!("unknown type `{name}`: no struct with this name is defined"), span).hint(hint));
                false
            }
            _ => true,
        }
    }

    fn managed(&self, t: Type) -> bool {
        data::managed(t, &self.structs)
    }

    /// True if a value of `t` holds a map (`json` does not handle maps yet).
    fn has_map(&self, t: Type) -> bool {
        fn go(c: &Checker, t: Type, seen: &mut Vec<String>) -> bool {
            match t {
                Type::Map(_) => true,
                Type::Array(_) => t.elem().is_some_and(|e| go(c, e, seen)),
                Type::Struct(_) => {
                    let Some(n) = t.struct_name() else { return false };
                    if seen.contains(&n) {
                        return false;
                    }
                    seen.push(n.clone());
                    c.structs.get(&n).is_some_and(|s| s.fields.iter().any(|(_, ft, _)| go(c, *ft, seen)))
                }
                _ => false,
            }
        }
        go(self, t, &mut Vec::new())
    }

    /// A map key must be an `int`, `str`, `char` or `bool` (E0218).
    fn map_key(&mut self, k: Type, span: Span) -> bool {
        if matches!(k, Type::Int | Type::Str | Type::Char | Type::Bool) || k.is_unknown() {
            return true;
        }
        let hint = match k {
            Type::Float => "a float is a bad key (rounding, NaN): use `int` keys, or the text `str(x)`".to_string(),
            _ => format!("use an `int` or a `str` that stands for the {}, e.g. an id or a name", k.name()),
        };
        self.errs.push(
            Diag::new("E0218", format!("a map key must be `int`, `str`, `char` or `bool`, found `{}`", k.name()), span).hint(hint),
        );
        false
    }

    /// A value of type `got` is used where `want` is needed. `e` is the expression, for the hint.
    fn expect_ty(&mut self, want: Type, got: Type, e: &Expr, ctx: Ctx) {
        if got == want || got.is_unknown() || want.is_unknown() {
            return;
        }
        let place = match &ctx {
            Ctx::Let(name, ty) => format!("`let {name}: {}`", ty.name()),
            Ctx::Assign(name) => format!("the assignment to `{name}`"),
            Ctx::AssignTo(target) => format!("the assignment to `{target}`"),
            Ctx::Arg { f, idx, param } => format!("argument {} (`{param}`) of `{f}`", idx + 1),
            Ctx::MethodArg { m, idx } => format!("argument {} of `.{m}(...)`", idx + 1),
            Ctx::Field { s, f } => format!("field `{f}` of `{s}(...)`"),
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
        let fix = match (want, got) {
            (Type::Float, Type::Int) => float_literal(e),
            (Type::Char, Type::Str) => char_literal(e),
            _ => None,
        };
        self.errs.push(
            Diag::new("E0203", format!("type mismatch in {place}: expected `{}`, found `{}`", want.name(), got.name()), e.span)
                .hint(hint)
                .fix_opt(fix),
        );
    }

    /// Why a call that returns nothing cannot give a value, and what to do.
    fn no_value_hint(&self, e: &Expr) -> String {
        match &e.kind {
            ExprKind::Call(n, _) if self.fns.contains_key(n.as_str()) => format!(
                "`{n}` has no return type, so it returns nothing: call it on its own line, or declare `fn {n}(...) -> int` and `return` a value"
            ),
            ExprKind::Call(n, _) => format!("`{n}` only has an effect and returns nothing: use it as a statement on its own line"),
            ExprKind::Method(_, m, _) => {
                format!("`.{m}()` changes its receiver and returns nothing: call it on its own line, then use the value")
            }
            _ => "only a function with a return type (`-> int`) has a value".to_string(),
        }
    }

    fn mismatch_hint(&self, want: Type, got: Type, e: &Expr, ctx: &Ctx) -> String {
        use Type::{Bool, Char, Float, Int, Str};
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
                    Some(s) if !s.contains('"') => format!("build text with interpolation: `\"{{{s}}}\"`, or convert with `str({s})`"),
                    _ => "build text with interpolation: store the value in a variable `x`, then write `\"{x}\"`".to_string(),
                }
            }
            (Str, Char) => {
                return match &s {
                    Some(s) => format!("convert the character to text with `str({s})`"),
                    None => "convert the character to text with `str(c)`".to_string(),
                }
            }
            (Char, Str) => {
                if let ExprKind::Str(text) = &e.kind {
                    if text.chars().count() == 1 {
                        let c = text.chars().next().unwrap_or(' ') as u32;
                        return format!("a character is written in single quotes: `{}`", data::show_char(c));
                    }
                }
                return "a `char` is one character: take one from a string with `s[i]`".to_string();
            }
            (Int | Float, Str) => {
                if let Some(n) = number_text(e, want) {
                    return format!("write the number without quotes: `{n}`");
                }
                let f = if want == Int { "int" } else { "float" };
                if matches!(e.kind, ExprKind::Str(_)) {
                    // parsing literal text that is not a number would always fail
                    return format!("this text is not a number: write an `{f}` here, or give the variable the type you need");
                }
                return match &s {
                    Some(s) => format!("parse the text with `{f}({s})` (a runtime error if it is not a number)"),
                    None => format!("parse the text with `{f}(...)` (a runtime error if it is not a number)"),
                };
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
            (Int, Char) => {
                return match &s {
                    Some(s) => format!("a `char` is not an `int`: its code is `{s}.code()`"),
                    None => "a `char` is not an `int`: its code is `c.code()`".to_string(),
                }
            }
            (Type::Array(_), _) if want.elem() == Some(got) => {
                return match &s {
                    Some(s) => format!("put the value in an array: `[{s}]`"),
                    None => "put the value in an array: `[x]`".to_string(),
                }
            }
            _ => {}
        }
        match ctx {
            Ctx::Let(name, _) => {
                format!("change the value to {}, or change the declared type: `let {name}: {} = ...`", article(want), got.name())
            }
            Ctx::Assign(name) => {
                format!("`{name}` holds {} values: assign {}, or declare `{name}` with the type you need", want.name(), article(want))
            }
            Ctx::AssignTo(target) => format!("`{target}` holds {} values: assign {}", want.name(), article(want)),
            Ctx::Arg { f, param, .. } => format!("pass {} for `{param}`, or change the parameter type in `fn {f}`", article(want)),
            Ctx::MethodArg { m, .. } => format!("`.{m}(...)` needs {} here", article(want)),
            Ctx::Field { s, f } => {
                format!("field `{f}` of `{s}` holds {}: pass one, or change the field's type in `struct {s}`", article(want))
            }
            Ctx::Ret => format!("return {}, or change the return type of `{}` to `{}`", article(want), self.fname, got.name()),
            Ctx::Range(_) => "the bounds of a range are `int` values: `for i in 0..10`".to_string(),
        }
    }

    /// An `if` or `while` condition.
    fn cond(&mut self, t: Type, e: &Expr, what: &str) {
        if t == Type::Bool || t.is_unknown() {
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
            (Type::Array(_), Some(s)) => format!("compare its length: `{s}.len() > 0`"),
            _ => "compare explicitly, e.g. `x != 0`".to_string(),
        };
        self.errs.push(Diag::new("E0209", format!("the condition of {what} must be `bool`, found `{}`", t.name()), e.span).hint(hint));
    }

    /// False for a struct that is not defined (reported already), also inside an array type.
    fn defined(&self, t: Type) -> bool {
        match t {
            Type::Array(_) => t.elem().is_none_or(|e| self.defined(e)),
            Type::Struct(_) => t.struct_name().is_some_and(|n| self.structs.contains_key(&n)),
            _ => true,
        }
    }

    fn func(&mut self, f: &mut Func) {
        self.ret = if self.defined(f.ret) { f.ret } else { Type::Unknown };
        self.fname = f.name.clone();
        self.decls.clear();
        self.freed.clear();
        collect_decls(&f.body, &mut self.decls);
        self.g.start_func(f);
        self.scopes = vec![HashMap::new()];
        for p in &f.params {
            let ty = if self.defined(p.ty) { p.ty } else { Type::Unknown };
            self.declare(&p.name, ty, if p.inout { Decl::Inout } else { Decl::Param }, p.span);
        }
        if self.g.in_main {
            // a script: each top-level statement is numbered, so a call can be compared with the
            // declarations of the script variables it uses
            self.scopes.push(HashMap::new());
            for (i, s) in f.body.iter_mut().enumerate() {
                self.g.top_stmt = i;
                self.stmt(s);
            }
            self.scopes.pop();
        } else {
            self.block(&mut f.body);
        }
        self.g.end_func(&f.name);
        if f.ret != Type::Void && !returns(&f.body) {
            let value = sample(f.ret);
            // the last line computes the value: it only lacks the `return`
            let fix = match f.body.last() {
                Some(Stmt { kind: StmtKind::Expr(e), span }) if e.ty == f.ret => Some(Edit::insert(*span, "return ")),
                _ => None,
            };
            let hint = match f.body.last().map(|s| &s.kind) {
                Some(StmtKind::Expr(e)) if e.ty == f.ret => match show(e) {
                    Some(s) => format!(
                        "the last line computes a value, but Nyra does not return it by itself: write `return {s}` (or make this a one-line function: `fn {}(...) -> {} = {s}`)",
                        f.name,
                        f.ret.name()
                    ),
                    None => "the last line computes a value, but Nyra does not return it by itself: start it with `return`".to_string(),
                },
                Some(StmtKind::If { els: None, .. }) => {
                    format!("the last `if` has no `else`: add `return {value}` after it, or an `else {{ return {value} }}` branch")
                }
                Some(StmtKind::If { .. }) => "every branch of the last `if`/`else` must end with `return`".to_string(),
                Some(StmtKind::While { .. } | StmtKind::For { .. } | StmtKind::ForEach { .. }) => {
                    format!("a loop may run zero times: add `return {value}` after the loop")
                }
                _ => format!("end the function with `return`, e.g. `return {value}`"),
            };
            self.errs.push(
                Diag::new(
                    "E0207",
                    format!("function `{}` is declared `-> {}` but not every path ends with `return`", f.name, f.ret.name()),
                    f.span,
                )
                .hint(hint)
                .fix_opt(fix),
            );
        }
    }

    /// An `ex` condition: it sees no variables, and it must be a `bool` (it is run by `examples.rs`).
    fn example(&mut self, e: &mut Expr, forall: Option<&Forall>) {
        self.ret = Type::Void;
        self.fname = String::new();
        self.decls.clear();
        self.freed.clear();
        self.scopes = vec![HashMap::new()];
        self.g.start_example();
        // a property example sees its variable, an `int`
        if let Some(f) = forall {
            self.declare(&f.var, Type::Int, Decl::Let, f.span);
        }
        let t = self.expr_with(e, Some(Type::Bool));
        self.g.end_func("");
        if t != Type::Bool && !t.is_unknown() {
            let what = show(e).map_or("this example".to_string(), |s| format!("`{s}`"));
            let hint = match (t, show(e)) {
                (Type::Void, _) => {
                    "an example checks a value: call a function that returns one and compare the result, e.g. `ex sq(3) == 9`"
                        .to_string()
                }
                (_, Some(s)) => format!("compare it with the value you expect: `ex {s} == ...`"),
                _ => "write a condition that must be true, e.g. `ex sq(3) == 9`".to_string(),
            };
            let got = if t == Type::Void { "returns nothing".to_string() } else { format!("is {}", article(t)) };
            self.errs
                .push(Diag::new("E0252", format!("an example must be a `bool` condition, but {what} {got}"), start(e)).hint(hint));
        }
    }

    fn block(&mut self, b: &mut [Stmt]) {
        self.scopes.push(HashMap::new());
        for s in b.iter_mut() {
            self.stmt(s);
        }
        self.scopes.pop();
    }

    /// A loop body. A `free(x)` inside reaches the uses of the next round, so variables freed in
    /// the body (and not assigned in it) count as maybe-freed while the body is checked.
    fn loop_body(&mut self, body: &mut [Stmt], vars: &[(&str, Type, Span)]) {
        let before = self.freed.clone();
        let (mut frees, mut assigned) = (Vec::new(), Vec::new());
        data::frees_in(body, &mut frees, &mut assigned);
        for (name, at) in frees {
            if !assigned.contains(&name) && self.lookup(&name).is_some() {
                self.freed.entry(name).or_insert(Freed { line: at.line, maybe: true });
            }
        }
        self.scopes.push(HashMap::new());
        for &(name, t, span) in vars {
            self.declare(name, t, Decl::Loop, span);
        }
        self.block(body);
        self.scopes.pop();
        // the loop may run zero times: anything freed only inside is maybe-freed after it
        let after = std::mem::take(&mut self.freed);
        self.freed = join(&before, &after);
    }

    fn stmt(&mut self, s: &mut Stmt) {
        let span = s.span;
        match &mut s.kind {
            StmtKind::Let { name, mutable, ty, value } => {
                // a type that is not defined is reported once: the variable then has no known type
                let declared = *ty;
                let want = declared.filter(|w| self.check_type(*w, span));
                let got = self.expr_with(value, want);
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
                if let Some(want) = want {
                    if got != Type::Void {
                        self.expect_ty(want, got, value, Ctx::Let(name, want));
                    }
                    t = want;
                } else if declared.is_some() {
                    t = Type::Unknown;
                }
                self.declare(name, t, if *mutable { Decl::Var } else { Decl::Let }, span);
                if self.g.in_main && self.scopes.len() == 2 {
                    self.g.add_global(name, t, *mutable, span);
                }
            }
            StmtKind::Assign { target, op, value } => self.assign(target, *op, value, span),
            StmtKind::If { cond, then, els } => {
                let t = self.expr(cond);
                self.cond(t, cond, "`if`");
                let before = self.freed.clone();
                self.block(then);
                let after_then = std::mem::replace(&mut self.freed, before);
                if let Some(e) = els {
                    self.block(e);
                }
                let after_else = std::mem::take(&mut self.freed);
                self.freed = join(&after_then, &after_else);
            }
            StmtKind::While { cond, body } => {
                let t = self.expr(cond);
                self.cond(t, cond, "`while`");
                self.loop_body(body, &[]);
            }
            StmtKind::For { var, start, end, step, body } => {
                let a = self.expr(start);
                self.range_bound(a, start, "start");
                let b = self.expr(end);
                self.range_bound(b, end, "end");
                if let Some(k) = step {
                    let t = self.expr(k);
                    self.range_bound(t, k, "step");
                }
                self.loop_body(body, &[(var, Type::Int, span)]);
            }
            StmtKind::ForEach { var, index, iter, body } => {
                let it = self.expr(iter);
                let elem = match it {
                    t if t.is_unknown() => Type::Unknown,
                    Type::Str => Type::Char,
                    Type::Array(_) => it.elem().unwrap_or(Type::Unknown),
                    // a map gives its keys, in insertion order
                    Type::Map(_) => it.map_kv().map_or(Type::Unknown, |(k, _)| k),
                    t => {
                        let hint = match (t, show(iter)) {
                            (Type::Int, Some(s)) => format!("to count, loop over a range: `for {var} in 0..{s}`"),
                            (Type::Int, None) => format!("to count, loop over a range: `for {var} in 0..n`"),
                            _ => "a `for` loop goes over a range `a..b`, an array or a string".to_string(),
                        };
                        self.errs.push(
                            Diag::new(
                                "E0234",
                                format!("cannot loop over `{}`: `for {var} in ...` needs an array, a string or a range", t.name()),
                                iter.span,
                            )
                            .hint(hint),
                        );
                        Type::Unknown
                    }
                };
                match index {
                    Some(i) => self.loop_body(body, &[(i, Type::Int, span), (var, elem, span)]),
                    None => self.loop_body(body, &[(var, elem, span)]),
                }
            }
            StmtKind::Break | StmtKind::Continue => {}
            StmtKind::Arena(body) => {
                self.arena_depth += 1;
                self.block(body);
                self.arena_depth -= 1;
            }
            StmtKind::Ret(value) => match value {
                Some(e) => {
                    let want = (self.ret != Type::Void).then_some(self.ret);
                    let t = self.expr_with(e, want);
                    if self.ret == Type::Void {
                        let shown = if t.is_unknown() || t == Type::Void { "int".to_string() } else { t.name() };
                        self.errs.push(
                            Diag::new(
                                "E0207",
                                format!("`return` returns a value, but `{}` has no return type (it returns nothing)", self.fname),
                                e.span,
                            )
                            .hint(format!(
                                "add the return type to the signature: `fn {}(...) -> {shown}`, or write `return` without a value",
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
                                format!("`return` needs a value: `{}` returns `{}`", self.fname, self.ret.name()),
                                span,
                            )
                            .hint(format!(
                                "write `return {}` (or any other `{}` value)",
                                sample(self.ret),
                                self.ret.name()
                            )),
                        );
                    }
                }
            },
            StmtKind::Expr(e) => {
                self.expr(e);
            }
        }
    }

    /// A range bound must be an `int` (a `char` bound gets the `code()` hint).
    fn range_bound(&mut self, t: Type, e: &Expr, which: &'static str) {
        if t == Type::Char {
            let s = show(e).unwrap_or_else(|| "c".into());
            self.errs.push(
                Diag::new("E0210", format!("the {which} of a range must be an `int`, found `char`"), e.span)
                    .hint(format!("count over the codes: `{s}.code()`, and turn a code back into a character with `char(n)`")),
            );
            return;
        }
        self.expect_ty(Type::Int, t, e, Ctx::Range(which));
    }

    /// `target = value` and `target op= value`.
    fn assign(&mut self, target: &mut Expr, op: Option<BinOp>, value: &mut Expr, span: Span) {
        let var_name = match &target.kind {
            ExprKind::Var(n) => Some(n.clone()),
            _ => None,
        };
        if let Some(name) = &var_name {
            self.note_use(name, true);
        }
        let tt = match &var_name {
            Some(name) => match self.lookup(name).map(|v| (v.ty, v.decl, v.span, v.arena)) {
                None => {
                    let d = self.undefined_var(name, span, true);
                    self.errs.push(d);
                    Type::Unknown
                }
                Some((t, decl, dspan, arena)) => {
                    if !matches!(decl, Decl::Var | Decl::Inout) {
                        let (msg, hint) = match decl {
                            Decl::Param => (
                                format!("cannot assign to `{name}`: parameters cannot be changed"),
                                format!("copy it into a variable first (`var m = {name}`), then change `m`; or declare the parameter `inout {name}: {}` to change the caller's variable", t.name()),
                            ),
                            Decl::Loop => (
                                format!("cannot assign to `{name}`: it is the loop variable of a `for`"),
                                "loop variables cannot be changed: use a `while` loop with a `var` counter instead".to_string(),
                            ),
                            Decl::Lambda => (
                                format!("cannot assign to `{name}`: it is the parameter of a lambda"),
                                "a lambda only computes a value from its parameters".to_string(),
                            ),
                            _ => (
                                format!("cannot assign to `{name}`: it was declared with `let` on line {}", dspan.line),
                                format!("declare it with `var` to make it changeable: `var {name} = ...` on line {}", dspan.line),
                            ),
                        };
                        // it changes, so it was meant to be a `var`
                        let fix = (decl == Decl::Let).then(|| Edit::replace(dspan, "let", "var"));
                        self.errs.push(Diag::new("E0205", msg, span).hint(hint).fix_opt(fix));
                    } else if self.outer_arena_var(arena, t) {
                        self.errs.push(self.arena_error(name, span));
                    }
                    // `x += 1` reads `x`: a freed `x` cannot be read
                    if op.is_some() {
                        self.check_freed(name, span);
                    }
                    target.ty = t;
                    t
                }
            },
            None => {
                let t = self.expr(target);
                self.check_place(target, "assign to", span);
                t
            }
        };
        let got = self.expr_with(value, if op.is_none() { Some(tt) } else { None });
        match op {
            None => match &var_name {
                Some(name) => {
                    self.expect_ty(tt, got, value, Ctx::Assign(name));
                    // a new value brings a freed variable back
                    self.freed.remove(name);
                }
                None => {
                    let shown = show(target).unwrap_or_else(|| "this place".into());
                    self.expect_ty(tt, got, value, Ctx::AssignTo(shown));
                }
            },
            Some(op) => {
                self.binary(op, tt, got, span, target, value, true);
            }
        }
    }

    /// True if `t` is managed and the variable was declared outside the innermost `arena` block.
    fn outer_arena_var(&self, declared_in: usize, t: Type) -> bool {
        self.arena_depth > 0 && declared_in < self.arena_depth && self.managed(t)
    }

    fn arena_error(&self, name: &str, span: Span) -> Diag {
        Diag::new(
            "E0238",
            format!("`{name}` cannot be changed inside this `arena` block: it was declared outside it"),
            span,
        )
        .hint(format!(
            "values created in an `arena` are freed at its `}}`: change `{name}` after the block, or return the result from a function whose body is the `arena` (`return` copies it out)"
        ))
    }

    /// `e` is changed (assigned into, a mutating method, `inout`): it must be a place whose root
    /// variable can change.
    fn check_place(&mut self, e: &Expr, what: &str, span: Span) {
        if self.lambda_depth > 0 {
            self.errs.push(lambda::changes(e, what, span));
            return;
        }
        // a value inside a map can change in place (`m[k].push(x)`, `m[k].n += 1`), but not be passed
        // `inout`: the callee would hold a reference into the map
        if what == "pass `inout`" && map_step(e) {
            let shown = show(e).unwrap_or_else(|| "m[k]".into());
            self.errs.push(
                Diag::new("E0229", format!("cannot {what} `{shown}`: a value inside a map cannot be passed `inout`"), span)
                    .hint("change a copy and store it back: `var v = m[k]`, `f(inout v)`, then `m[k] = v`"),
            );
            return;
        }
        if let ExprKind::Index(b, _) = &e.kind {
            if b.ty == Type::Str {
                self.errs.push(
                    Diag::new("E0229", format!("cannot {what} a character of a string: strings are immutable"), span)
                        .hint("build a new string instead, e.g. `s = s.slice(0, i) + \"x\" + s.slice(i + 1, s.len())`"),
                );
                return;
            }
        }
        let Some(root) = data::place_root(e) else {
            if !e.ty.is_unknown() {
                self.errs.push(
                    Diag::new(
                        "E0229",
                        format!("cannot {what} this expression: only a variable, a field or an element can change"),
                        span,
                    )
                    .hint("store the value in a `var` first, then change the variable"),
                );
            }
            return;
        };
        let root = root.to_string();
        self.note_use(&root, true);
        let Some((t, decl, dspan, arena)) = self.lookup(&root).map(|v| (v.ty, v.decl, v.span, v.arena)) else { return };
        let shown = show(e).unwrap_or_else(|| root.clone());
        match decl {
            Decl::Var | Decl::Inout => {
                if self.outer_arena_var(arena, t) {
                    self.errs.push(self.arena_error(&root, span));
                }
            }
            Decl::Let => self.errs.push(
                Diag::new("E0205", format!("cannot {what} `{shown}`: `{root}` was declared with `let` on line {}", dspan.line), span)
                    .hint(format!("declare it with `var` to make it changeable: `var {root} = ...` on line {}", dspan.line))
                    .fix(vec![Edit::replace(dspan, "let", "var")]),
            ),
            Decl::Param => self.errs.push(
                Diag::new("E0205", format!("cannot {what} `{shown}`: `{root}` is a parameter, and parameters cannot be changed"), span)
                    .hint(format!(
                        "copy it into a variable first (`var m = {root}`), or declare the parameter `inout {root}: {}` to change the caller's value",
                        t.name()
                    )),
            ),
            Decl::Loop => self.errs.push(
                Diag::new("E0205", format!("cannot {what} `{shown}`: `{root}` is the loop variable of a `for`"), span)
                    .hint("the loop variable is a copy of each element: change the array itself, e.g. `xs[i] = ...` in `for i, x in xs`"),
            ),
            Decl::Lambda => {}
        }
    }

    /// Reports a use of a variable after `free`.
    fn check_freed(&mut self, name: &str, span: Span) {
        let Some(f) = self.freed.get(name).copied() else { return };
        let msg = if f.maybe {
            format!("`{name}` may have been freed (line {}): it is freed on some paths before this use", f.line)
        } else {
            format!("`{name}` was freed at line {} and cannot be used any more", f.line)
        };
        self.errs.push(
            Diag::new("E0239", msg, span)
                .hint(format!("give it a new value first (`{name} = ...`, needs `var`), or move `free({name})` after its last use")),
        );
    }

    fn expr(&mut self, e: &mut Expr) -> Type {
        self.expr_with(e, None)
    }

    /// The type of `e`. `want` is the type the context expects; only `[]` needs it.
    fn expr_with(&mut self, e: &mut Expr, want: Option<Type>) -> Type {
        let span = e.span;
        // `math.sqrt(x)`, `math.pi`: an item of a module
        if let Some(t) = self.module_item(e, want) {
            e.ty = t;
            return t;
        }
        let t = match &mut e.kind {
            ExprKind::Int(_) => Type::Int,
            ExprKind::Float(_) => Type::Float,
            ExprKind::Bool(_) => Type::Bool,
            ExprKind::Str(_) => Type::Str,
            ExprKind::Char(_) => Type::Char,
            ExprKind::Interp(parts) => {
                for p in parts.iter_mut() {
                    if let InterpPart::Expr(x) = p {
                        let before = self.errs.len();
                        let ty = self.expr(x);
                        self.literal_dollar_brace(x, before);
                        if ty == Type::Void {
                            self.errs.push(
                                Diag::new(
                                    "E0203",
                                    format!("{} returns nothing, so it cannot be put into a string", call_text(x)),
                                    x.span,
                                )
                                .hint("only values can go inside `{ }`: call it on its own line before the string"),
                            );
                        }
                    }
                }
                Type::Str
            }
            ExprKind::Var(name) => match self.lookup(name) {
                Some(v) => {
                    let t = v.ty;
                    let name = name.clone();
                    self.note_use(&name, false);
                    self.check_freed(&name, span);
                    t
                }
                None if self.modules.iter().any(|(m, _)| m == name) => {
                    let first = stdlib::names(name).into_iter().next().unwrap_or_default();
                    self.errs.push(
                        Diag::new("E0307", format!("`{name}` is a module, not a value"), span)
                            .hint(format!("use the items of the module by their names, e.g. `{name}.{first}`")),
                    );
                    Type::Unknown
                }
                None if self.structs.contains_key(name.as_str()) => {
                    let fields: Vec<String> = self.structs[name.as_str()].fields.iter().map(|(f, _, _)| format!("{f}: ...")).collect();
                    self.errs.push(
                        Diag::new("E0235", format!("`{name}` is a type, not a value"), span)
                            .hint(format!("build a value with all its fields: `{name}({})`", fields.join(", "))),
                    );
                    Type::Unknown
                }
                None => {
                    let d = self.undefined_var(name, span, false);
                    self.errs.push(d);
                    Type::Unknown
                }
            },
            ExprKind::Unary(op, inner) => {
                let t = self.expr(inner);
                match (*op, t) {
                    (_, t) if t.is_unknown() => Type::Unknown,
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
                // `xs == []`, `xs + []`: the other side tells the empty array its type
                let same_side = matches!(op, BinOp::Eq | BinOp::Ne | BinOp::Add);
                let lt = self.expr_with(l, if same_side { want.filter(|_| *op == BinOp::Add) } else { None });
                let rt = self.expr_with(r, if same_side && !lt.is_unknown() { Some(lt) } else { None });
                let t = self.binary(*op, lt, rt, span, l, r, false);
                if outer {
                    self.chain = None;
                }
                t
            }
            ExprKind::Call(name, args) => self.call(name, args, span, want),
            ExprKind::If(cond, a, b) => {
                let c = self.expr(cond);
                self.cond(c, cond, "a conditional value (`if` or `? :`)");
                let ta = self.expr_with(a, want);
                let tb = self.expr_with(b, if ta.is_unknown() { want } else { Some(ta) });
                if ta == Type::Void || tb == Type::Void {
                    let (which, branch) = if ta == Type::Void { ("first", &**a) } else { ("second", &**b) };
                    self.errs.push(
                        Diag::new(
                            "E0212",
                            format!("the {which} branch of this conditional value (`if` or `? :`) produces no value: {} returns nothing", call_text(branch)),
                            span,
                        )
                        .hint("each branch must be an expression with a value, e.g. `if c { 1 } else { 2 }`; use an `if` statement for actions"),
                    );
                    Type::Unknown
                } else if ta.is_unknown() || tb.is_unknown() {
                    Type::Unknown
                } else if ta != tb {
                    let fix = match (ta, tb) {
                        (Type::Int, Type::Float) => float_literal(a),
                        (Type::Float, Type::Int) => float_literal(b),
                        _ => None,
                    };
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
                        _ => format!(
                            "both branches must have the same type: change one so both are `{}` (or both `{}`)",
                            ta.name(),
                            tb.name()
                        ),
                    };
                    self.errs.push(
                        Diag::new(
                            "E0212",
                            format!(
                                "the branches of this conditional value (`if` or `? :`) have different types: `{}` and `{}`",
                                ta.name(),
                                tb.name()
                            ),
                            span,
                        )
                        .hint(hint)
                        .fix_opt(fix),
                    );
                    Type::Unknown
                } else {
                    ta
                }
            }
            ExprKind::Array(items) => self.array_lit(items, want, span),
            ExprKind::MapLit(pairs) => self.map_lit(pairs, want, span),
            ExprKind::Index(base, index) => {
                let bt = self.expr(base);
                let it = match bt.map_kv() {
                    Some((k, _)) => self.expr_with(index, Some(k)),
                    None => self.expr(index),
                };
                if matches!(bt, Type::Array(_) | Type::Str) {
                    self.negative_position(base, index, false);
                }
                if bt.map_kv().is_none() && it != Type::Int && !it.is_unknown() {
                    let hint = match (it, show(index)) {
                        (Type::Float, Some(s)) => format!("convert it: `[int({s})]`"),
                        (Type::Char, Some(s)) => format!("an index is a position: to use the character's code write `[{s}.code()]`"),
                        _ => "an index is an `int` position: 0 is the first element".to_string(),
                    };
                    self.errs
                        .push(Diag::new("E0232", format!("an index must be an `int`, found `{}`", it.name()), index.span).hint(hint));
                }
                match bt {
                    t if t.is_unknown() => Type::Unknown,
                    Type::Str => Type::Char,
                    Type::Array(_) => bt.elem().unwrap_or(Type::Unknown),
                    Type::Map(_) => {
                        let (k, v) = bt.map_kv().unwrap_or((Type::Unknown, Type::Unknown));
                        if it != k && !it.is_unknown() {
                            let hint = match (k, show(index)) {
                                (Type::Str, Some(s)) => format!("the keys are text: `[str({s})]`"),
                                _ => format!("the keys of this map are `{}` values", k.name()),
                            };
                            self.errs.push(
                                Diag::new(
                                    "E0232",
                                    format!("a key of `{}` must be `{}`, found `{}`", bt.name(), k.name(), it.name()),
                                    index.span,
                                )
                                .hint(hint),
                            );
                        }
                        v
                    }
                    t => {
                        let shown = show(base);
                        let hint = match (t, shown) {
                            (Type::Struct(_), shown) => {
                                // a struct has fields, not positions: name its first field
                                let field = t
                                    .struct_name()
                                    .and_then(|n| self.structs.get(&n))
                                    .and_then(|s| s.fields.first().map(|(f, _, _)| f.clone()));
                                match (shown, field) {
                                    (Some(b), Some(f)) => format!("a struct has fields, not positions: read one with a dot, e.g. `{b}.{f}`"),
                                    _ => "a struct has fields, not positions: read one with a dot, e.g. `p.x`".to_string(),
                                }
                            }
                            (Type::Int, Some(b)) => format!(
                                "only arrays (`xs[0]`) and strings (`s[0]`, a `char`) can be indexed; to index the digits of `{b}`, turn it into text first: `str({b})`"
                            ),
                            _ => "only arrays (`xs[0]`) and strings (`s[0]`, a `char`) can be indexed".to_string(),
                        };
                        self.errs.push(Diag::new("E0233", format!("cannot index a value of type `{}`", t.name()), span).hint(hint));
                        Type::Unknown
                    }
                }
            }
            ExprKind::Field(base, name) => {
                let bt = self.expr(base);
                let shown = show(base);
                self.field_type(bt, name, shown, span)
            }
            ExprKind::Method(recv, name, args) => self.method(recv, name, args, span),
            ExprKind::Labeled(label, v) => {
                self.expr(v);
                self.errs.push(
                    Diag::new("E0226", format!("`{label}: value` is only for building structs"), span)
                        .hint("pass the value without a name: `f(x)`; names are only used in `Point(x: 1, y: 2)`"),
                );
                Type::Unknown
            }
            ExprKind::Lambda(..) => {
                self.errs.push(lambda::misplaced(span));
                Type::Unknown
            }
            ExprKind::Comprehension(c) => self.comprehension(c),
            ExprKind::Inout(v) => {
                self.expr(v);
                self.errs.push(
                    Diag::new("E0237", "`inout` here does not match an `inout` parameter", span)
                        .hint("write `inout` only before an argument whose parameter is declared `inout name: type`"),
                );
                Type::Unknown
            }
        };
        e.ty = t;
        t
    }

    /// `module.name(args)` or `module.name`, where `module` is a standard module (and not a
    /// variable). `None` for every other expression. A call becomes a call of the function named
    /// `module.name`, a constant becomes its value.
    fn module_item(&mut self, e: &mut Expr, want: Option<Type>) -> Option<Type> {
        let (m, name, call) = match &e.kind {
            ExprKind::Method(r, n, _) | ExprKind::Field(r, n) => match &r.kind {
                ExprKind::Var(m) => (m.clone(), n.clone(), matches!(e.kind, ExprKind::Method(..))),
                _ => return None,
            },
            _ => return None,
        };
        if self.lookup(&m).is_some() || !stdlib::is_module(&m) {
            return None;
        }
        let span = e.span;
        let full = format!("{m}.{name}");
        let check_args = |c: &mut Checker, e: &mut Expr| {
            if let ExprKind::Method(_, _, args) = &mut e.kind {
                for a in args.iter_mut() {
                    c.arg_type(a, None);
                }
            }
        };
        // inside a bundled module (`math.exp` calling `math.floor`) the module is always the module
        if !self.fname.contains('.') && !self.modules.iter().any(|(x, _)| *x == m) {
            // a function of the program that has the module's name: `fs.x` is then its own mistake
            if self.fns.contains_key(&m) || self.decls.iter().any(|(d, _)| *d == m) {
                return None;
            }
            let rspan = match &e.kind {
                ExprKind::Method(r, ..) | ExprKind::Field(r, _) => r.span,
                _ => span,
            };
            self.errs.push(
                Diag::new("E0201", format!("undefined variable `{m}`"), rspan)
                    .hint(format!("`{m}` is a standard module: add `use {m}` at the top of the file"))
                    .fix(vec![Edit::insert(
                        Span { line: 1, col: 1 },
                        format!(
                            "use {m}
"
                        ),
                    )]),
            );
            check_args(self, e);
            return Some(Type::Unknown);
        }
        if let Some(v) = stdlib::constant(&m, &name) {
            if call {
                let no_args = matches!(&e.kind, ExprKind::Method(_, _, a) if a.is_empty());
                check_args(self, e);
                self.errs.push(
                    Diag::new("E0307", format!("`{full}` is a constant, not a function"), span)
                        .hint(format!("drop the parentheses: `{full}`"))
                        .fix_opt(no_args.then(|| Edit::replace(span, &format!("{name}()"), name.clone()))),
                );
                return Some(Type::Unknown);
            }
            e.kind = ExprKind::Float(v);
            return Some(Type::Float);
        }
        let public = !name.starts_with('_');
        let json = m == "json" && stdlib::JSON_FNS.contains(&name.as_str());
        if !public || !(json || self.fns.contains_key(&full)) {
            let items = stdlib::names(&m);
            let (hint, fix) = match stdlib::renamed(&m, &name) {
                Some(h) => (h.to_string(), None),
                None => match suggest_fix(&name, items.iter().map(String::as_str)) {
                    Some((h, f)) => (h, f.map(|f| Edit::replace(span, &name, f))),
                    None => (
                        format!("the items of `{m}` are {}", items.iter().map(|i| format!("`{i}`")).collect::<Vec<_>>().join(", ")),
                        None,
                    ),
                },
            };
            check_args(self, e);
            self.errs.push(Diag::new("E0306", format!("module `{m}` has no `{name}`"), span).hint(hint).fix_opt(fix));
            return Some(Type::Unknown);
        }
        if !call {
            let no_params = self.fns.get(&full).is_some_and(|s| s.params.is_empty());
            self.errs.push(
                Diag::new("E0307", format!("`{full}` is a function, not a value"), span)
                    .hint(format!("call it: `{full}({})`", if no_params { "" } else { "..." }))
                    .fix_opt(no_params.then(|| Edit::replace(span, &name, format!("{name}()")))),
            );
            return Some(Type::Unknown);
        }
        let ExprKind::Method(_, _, args) = std::mem::replace(&mut e.kind, ExprKind::Int(0)) else { unreachable!("checked above") };
        e.kind = ExprKind::Call(full.clone(), args);
        let ExprKind::Call(_, args) = &mut e.kind else { unreachable!("just set") };
        if json {
            return Some(self.json_call(&name, args, span, want));
        }
        Some(self.call(&full, args, span, want))
    }

    /// `json.str(value)` of any value; `json.parse(text)` gives the type the context needs.
    fn json_call(&mut self, name: &str, args: &mut [Expr], span: Span, want: Option<Type>) -> Type {
        let parse = name == "parse";
        let tys: Vec<Type> = args.iter_mut().map(|a| self.arg_type(a, if parse { Some(Type::Str) } else { None })).collect();
        let shown = if parse { "json.parse(text)" } else { "json.str(value)" };
        for a in args.iter() {
            match &a.kind {
                ExprKind::Inout(_) => self
                    .errs
                    .push(Diag::new("E0237", format!("the argument of `json.{name}` is not `inout`"), a.span).hint("remove `inout`")),
                ExprKind::Labeled(label, _) => self.errs.push(
                    Diag::new("E0226", format!("named argument `{label}:` in a call to `json.{name}`"), a.span)
                        .hint(format!("write the value alone: `{shown}`")),
                ),
                _ => {}
            }
        }
        if tys.len() != 1 {
            self.errs.push(
                Diag::new(
                    "E0204",
                    format!("`json.{name}` takes exactly 1 argument but {} {} given", tys.len(), was_were(tys.len())),
                    span,
                )
                .hint(format!("call it as `{shown}`")),
            );
            return if parse { want.unwrap_or(Type::Unknown) } else { Type::Str };
        }
        let t = if parse { want.unwrap_or(Type::Unknown) } else { tys[0] };
        if self.has_map(t) {
            self.errs.push(
                Diag::new("E0309", format!("`json.{name}` cannot handle the map type in `{}` yet", t.name()), span)
                    .hint("use a struct for a JSON object with known keys, or an array of structs such as `[Entry]` with `struct Entry { key: str, value: int }`"),
            );
            return if parse { t } else { Type::Str };
        }
        if !parse {
            if tys[0] == Type::Void {
                let msg = format!("`json.str` needs a value, but {} returns nothing", call_text(&args[0]));
                self.errs.push(Diag::new("E0203", msg, args[0].span).hint(self.no_value_hint(&args[0])));
            }
            return Type::Str;
        }
        self.expect_ty(Type::Str, tys[0], &args[0], Ctx::Arg { f: "json.parse", idx: 0, param: "text" });
        match want {
            Some(t) if t.is_unknown() || (t != Type::Void && self.defined(t)) => t,
            _ => {
                self.errs.push(
                    Diag::new("E0309", "`json.parse` needs to know the type it reads", span).hint(
                        "give the value a type: `let p: Point = json.parse(text)`, `let xs: [int] = json.parse(text)`, or pass it where that type is expected",
                    ),
                );
                Type::Unknown
            }
        }
    }

    /// `[k: v, k2: v2]`: the keys have one type and the values have one type; `[:]` takes its type
    /// from the context.
    fn map_lit(&mut self, pairs: &mut [(Expr, Expr)], want: Option<Type>, span: Span) -> Type {
        let want_kv = want.and_then(Type::map_kv);
        if pairs.is_empty() {
            return match want {
                Some(t @ Type::Map(_)) => t,
                Some(t) if t.is_unknown() => Type::Unknown,
                _ => {
                    self.errs.push(
                        Diag::new("E0230", "cannot infer the type of the empty map `[:]`", span)
                            .hint("write its type where it is declared, e.g. `var counts: [str: int] = [:]`"),
                    );
                    Type::Unknown
                }
            };
        }
        let (mut kt, mut vt): (Option<Type>, Option<Type>) = (want_kv.map(|p| p.0), want_kv.map(|p| p.1));
        let mut bad = false;
        for (k, v) in pairs.iter_mut() {
            let tk = self.expr_with(k, kt);
            let tv = self.expr_with(v, vt);
            for (t, slot, e, what) in [(tk, &mut kt, &*k, "key"), (tv, &mut vt, &*v, "value")] {
                if t == Type::Void || t.is_unknown() {
                    bad = true;
                    continue;
                }
                match *slot {
                    None => *slot = Some(t),
                    Some(first) if first != t => {
                        self.errs.push(
                            Diag::new(
                                "E0231",
                                format!("the {what}s of a map must all have one type: `{}` and `{}`", first.name(), t.name()),
                                e.span,
                            )
                            .hint(format!("convert this {what} to `{}`, or use a struct for values of different types", first.name())),
                        );
                        bad = true;
                    }
                    _ => {}
                }
            }
        }
        match (kt, vt) {
            (Some(k), Some(v)) if !bad => {
                if !self.map_key(k, pairs[0].0.span) {
                    return Type::Unknown;
                }
                Type::map(k, v)
            }
            _ => Type::Unknown,
        }
    }

    /// `[a, b, c]`: every element has one type. `[]` takes its type from the context.
    fn array_lit(&mut self, items: &mut [Expr], want: Option<Type>, span: Span) -> Type {
        let want_elem = want.and_then(Type::elem);
        if items.is_empty() {
            return match want {
                Some(t @ Type::Array(_)) => t,
                Some(t) if t.is_unknown() => Type::Unknown,
                _ => {
                    self.errs.push(
                        Diag::new("E0230", "cannot infer the type of the empty array `[]`", span)
                            .hint("write its type where it is declared, e.g. `var xs: [int] = []`"),
                    );
                    Type::Unknown
                }
            };
        }
        let mut first: Option<Type> = None;
        // the first element as the program wrote it, and whether it is an int literal (for the hint)
        let mut first_shown: Option<(String, bool)> = None;
        // `[1, 2, 2.5]`: the int literals before the first float, which all become floats
        let mut int_literals: Option<Vec<Edit>> = Some(Vec::new());
        let mut bad = false;
        for it in items.iter_mut() {
            let t = self.expr_with(it, first.or(want_elem));
            if t == Type::Void {
                self.errs.push(
                    Diag::new("E0203", format!("{} returns nothing, so it cannot be an array element", call_text(it)), it.span)
                        .hint(self.no_value_hint(it)),
                );
                bad = true;
                continue;
            }
            if t.is_unknown() {
                bad = true;
                continue;
            }
            match first {
                None => {
                    first = Some(t);
                    first_shown = show(it).map(|s| (s, matches!(it.kind, ExprKind::Int(_))));
                }
                Some(f) if f != t => {
                    let fix = match (f, t) {
                        (Type::Float, Type::Int) => float_literal(it).map(|e| vec![e]),
                        (Type::Int, Type::Float) => int_literals.take(),
                        _ => None,
                    };
                    let hint = match (f, t) {
                        (Type::Float, Type::Int) => match show(it) {
                            Some(s) if matches!(it.kind, ExprKind::Int(_)) => format!("write it as a float: `{s}.0`"),
                            Some(s) => format!("convert it: `float({s})`"),
                            None => "convert it with `float(...)`".to_string(),
                        },
                        (Type::Int, Type::Float) => match &first_shown {
                            Some((s, true)) => format!("make every element a float: write the first as `{s}.0`"),
                            Some((s, false)) => format!("make every element a float: convert the first with `float({s})`"),
                            None => "make every element a float: convert the first with `float(...)`".to_string(),
                        },
                        (Type::Str, Type::Int | Type::Float | Type::Bool | Type::Char) => match show(it) {
                            Some(s) => format!("an array holds one type: write this element as text, `str({s})`, or use a struct to group values of different types"),
                            None => "an array holds one type: write this element as text with `str(...)`, or use a struct to group values of different types".to_string(),
                        },
                        _ => format!(
                            "an array holds one type: convert this element to `{}`, or use a struct to group different types",
                            f.name()
                        ),
                    };
                    self.errs.push(
                        Diag::new(
                            "E0231",
                            format!("array elements must all have one type: the first is `{}`, this one is `{}`", f.name(), t.name()),
                            it.span,
                        )
                        .hint(hint)
                        .fix(fix.unwrap_or_default()),
                    );
                    bad = true;
                }
                _ => {}
            }
            if let Some(lits) = &mut int_literals {
                match float_literal(it) {
                    Some(e) if t == Type::Int => lits.push(e),
                    _ if t == Type::Int => int_literals = None,
                    _ => {}
                }
            }
        }
        match first {
            Some(t) if !bad => Type::array(t),
            _ => Type::Unknown,
        }
    }

    /// `base.name`: `shown` is the base as the program wrote it, for the hint.
    fn field_type(&mut self, bt: Type, name: &str, shown: Option<String>, span: Span) -> Type {
        if bt.is_unknown() {
            return Type::Unknown;
        }
        if let Some(sname) = bt.struct_name() {
            let Some(info) = self.structs.get(&sname) else { return Type::Unknown };
            if let Some((_, t, _)) = info.fields.iter().find(|(f, _, _)| f == name) {
                return *t;
            }
            let names: Vec<&str> = info.fields.iter().map(|(f, _, _)| f.as_str()).collect();
            let (hint, fix) = match suggest_fix(name, names.iter().copied()) {
                Some((h, f)) => (h, f.map(|f| Edit::replace(span, name, f))),
                None if names.is_empty() => (format!("`{sname}` has no fields"), None),
                None => (
                    format!("the fields of `{sname}` are {}", names.iter().map(|n| format!("`{n}`")).collect::<Vec<_>>().join(", ")),
                    None,
                ),
            };
            self.errs.push(Diag::new("E0224", format!("`{sname}` has no field `{name}`"), span).hint(hint).fix_opt(fix));
            return Type::Unknown;
        }
        if let Some(sig) = data::method_sig(bt, name) {
            // `xs.len`: with no arguments to pass, the call is certain
            let fix = sig.params.is_empty().then(|| Edit::replace(span, name, format!("{name}()")));
            self.errs.push(
                Diag::new("E0236", format!("method `{name}` must be called"), span)
                    .hint(format!("add the parentheses: `.{name}()`"))
                    .fix_opt(fix),
            );
            return Type::Unknown;
        }
        let r = shown.unwrap_or_else(|| match bt {
            Type::Array(_) => "xs".into(),
            Type::Str => "s".into(),
            _ => "c".into(),
        });
        // `xs.length`: what other languages write as a field is a method here
        let renamed = hints::method_rename(bt, name).filter(|m| data::method_sig(bt, m).is_some_and(|s| s.params.is_empty()));
        let hint = match hints::method(bt, name, &r) {
            Some(h) => {
                let fix = renamed.map(|m| Edit::replace(span, name, format!("{m}()")));
                self.errs.push(Diag::new("E0224", format!("`{}` has no field `{name}`", bt.name()), span).hint(h).fix_opt(fix));
                return Type::Unknown;
            }
            None => match bt {
                Type::Array(_) | Type::Str | Type::Char => format!(
                    "`{}` has methods, not fields: {}",
                    bt.name(),
                    data::methods_of(bt).iter().map(|m| format!("`.{m}()`")).collect::<Vec<_>>().join(" ")
                ),
                _ => "only structs have fields: `p.x`".to_string(),
            },
        };
        self.errs.push(Diag::new("E0224", format!("`{}` has no field `{name}`", bt.name()), span).hint(hint));
        Type::Unknown
    }

    /// `recv.name(args)` on an array, a string or a char.
    fn method(&mut self, recv: &mut Expr, name: &str, args: &mut [Expr], span: Span) -> Type {
        // `console.log(x)`, `Math.sqrt(x)`: a library object of another language
        if let ExprKind::Var(n) = &recv.kind {
            let unknown = self.lookup(n).is_none() && !self.fns.contains_key(n.as_str()) && !self.decls.iter().any(|(d, _)| d == n);
            if let (true, Some(h)) = (unknown, hints::receiver(n)) {
                // `console.log(x)` prints one value, like `print(x)`
                let prints = matches!((n.as_str(), name), ("console", "log") | ("fmt", "Println") | ("Console", "WriteLine"))
                    && matches!(args, [a] if !matches!(a.kind, ExprKind::Labeled(..) | ExprKind::Inout(_)));
                let fix = prints.then(|| Edit::range(recv.span, after(span, name), &format!("{n}.{name}"), "print"));
                self.errs.push(Diag::new("E0201", format!("undefined variable `{n}`"), recv.span).hint(h).fix_opt(fix));
                for a in args.iter_mut() {
                    self.expr(a);
                }
                return Type::Unknown;
            }
        }
        let rt = self.expr(recv);
        if rt.is_unknown() {
            // the receiver is already reported (`Point.new(x: 1)`): its arguments are not judged again
            for a in args.iter_mut() {
                self.arg_type(a, None);
            }
            return Type::Unknown;
        }
        if let Some(t) = self.lambda_method(recv, rt, name, args, span) {
            return t;
        }
        if name == "slice" && matches!(rt, Type::Array(_) | Type::Str) && args.len() == 2 {
            for a in args.iter() {
                self.negative_position(recv, a, true);
            }
        }
        let Some(sig) = data::method_sig(rt, name) else {
            // (a lambda argument is not judged: the method is the mistake)
            for a in args.iter_mut().filter(|a| !matches!(a.kind, ExprKind::Lambda(..))) {
                self.expr(a);
            }
            let methods = data::methods_of(rt);
            let shown = show(recv).unwrap_or_else(|| match rt {
                Type::Array(_) => "xs".into(),
                Type::Str => "s".into(),
                Type::Char => "c".into(),
                _ => "x".into(),
            });
            // a method of another language that is the same operation under another name
            let arity = |m: &str| data::method_sig(rt, m).is_some_and(|s| s.params.len() == args.len());
            let mut fix = None;
            let hint = if rt == Type::Str && data::CHAR_METHODS.contains(&name) {
                format!("`{name}()` is a `char` method: use a character, `'A'.{name}()` or `s[0].{name}()`; all codes of a string: `s.codes()`")
            } else if let Some(h) = hints::method(rt, name, &shown) {
                fix = hints::method_rename(rt, name).filter(|m| arity(m)).map(|m| vec![Edit::replace(span, name, m)]);
                // `n.to_string()` is `str(n)`
                let lower = name.to_ascii_lowercase().replace('_', "");
                if let (ExprKind::Var(v), true, []) = (&recv.kind, matches!(lower.as_str(), "tostring" | "tostr" | "asstring"), &*args)
                {
                    let call = format!("{name}()");
                    fix = Some(vec![Edit::range(recv.span, after(span, &call), &format!("{v}.{call}"), format!("str({v})"))]);
                }
                h
            } else if let Some((h, f)) = suggest_fix(name, methods.iter().copied()) {
                fix = f.filter(|m| arity(m)).map(|m| vec![Edit::replace(span, name, m)]);
                h
            } else if let Some(sname) = rt.struct_name() {
                format!("structs have no methods: write a function `fn {name}(x: {sname})` and call `{name}(x)`")
            } else if methods.is_empty() {
                format!("`{}` has no methods", rt.name())
            } else {
                format!(
                    "the methods of `{}` are {}",
                    rt.name(),
                    methods.iter().map(|m| format!("`{m}`")).collect::<Vec<_>>().join(" ")
                )
            };
            self.errs.push(
                Diag::new("E0227", format!("`{}` has no method `{name}`", rt.name()), span).hint(hint).fix(fix.unwrap_or_default()),
            );
            return Type::Unknown;
        };
        // element types that some methods need
        if let Some(e) = rt.elem() {
            let bad = match name {
                "sort" => (!matches!(e, Type::Int | Type::Float | Type::Str | Type::Char))
                    .then_some("`[int]`, `[float]`, `[str]` or `[char]`"),
                "join" => (!matches!(e, Type::Str | Type::Char)).then_some("`[str]` or `[char]`"),
                _ => None,
            };
            if let Some(needs) = bad {
                let hint = if name == "join" {
                    let (r, sep) =
                        (show(recv).unwrap_or_else(|| "xs".into()), args.first().and_then(show).unwrap_or_else(|| "\", \"".into()));
                    format!("turn the elements into text first: `var parts: [str] = []`, `for x in {r} {{ parts.push(str(x)) }}`, then `parts.join({sep})`")
                } else {
                    "sort by a key yourself: e.g. loop and insert each element at its place".to_string()
                };
                self.errs.push(Diag::new("E0228", format!("`{name}` needs {needs}, found `{}`", rt.name()), span).hint(hint));
            }
        }
        // `s.pad_left(n)` fills with spaces; `s.pad_left(n, '0')` with a character
        let mut sig = sig;
        if rt == Type::Str && matches!(name, "pad_left" | "pad_right") && args.len() == 2 {
            sig.params.push(Type::Char);
        }
        // `m.get(k, default)`
        if let (Some((_, v)), "get", 2) = (rt.map_kv(), name, args.len()) {
            sig.params.push(v);
        }
        if args.len() != sig.params.len() {
            for a in args.iter_mut() {
                self.expr(a);
            }
            let shown: Vec<String> = sig.params.iter().map(|t| t.name()).collect();
            self.errs.push(
                Diag::new(
                    "E0204",
                    format!(
                        "`.{name}()` takes {} but {} {} given",
                        count(sig.params.len(), "argument"),
                        args.len(),
                        was_were(args.len())
                    ),
                    span,
                )
                .hint(format!("call it as `.{name}({})`", shown.join(", "))),
            );
        } else {
            // the text searches also take a character: `"aeiou".contains(c)`
            let search = rt == Type::Str && matches!(name, "contains" | "starts_with" | "ends_with" | "index_of");
            for (i, (a, p)) in args.iter_mut().zip(&sig.params).enumerate() {
                let t = self.expr_with(a, Some(*p));
                if search && t == Type::Char {
                    continue;
                }
                self.expect_ty(*p, t, a, Ctx::MethodArg { m: name, idx: i });
            }
        }
        if sig.mutates {
            if data::place_root(recv).is_some() {
                self.check_place(recv, &format!("call `.{name}()` on"), span);
            } else {
                self.errs.push(
                    Diag::new("E0229", format!("cannot call `.{name}()` on a temporary value: it changes its receiver"), span)
                        .hint("store the value in a `var` first, then call the method on the variable"),
                );
            }
        }
        sig.ret
    }

    #[allow(clippy::too_many_arguments)]
    fn binary(&mut self, op: BinOp, l: Type, r: Type, span: Span, le: &Expr, re: &Expr, compound: bool) -> Type {
        use Type::{Bool, Char, Float, Int, Str, Void};
        if l.is_unknown() || r.is_unknown() {
            return Type::Unknown;
        }
        let res = match op {
            BinOp::Add => match (l, r) {
                (Int, Int) => Some(Int),
                (Float, Float) => Some(Float),
                (Str, Str) => Some(Str),
                (Type::Array(_), Type::Array(_)) if l == r => Some(l),
                _ => None,
            },
            BinOp::Sub | BinOp::Mul | BinOp::Div => match (l, r) {
                (Int, Int) => Some(Int),
                (Float, Float) => Some(Float),
                _ => None,
            },
            BinOp::Mod => (l == Int && r == Int).then_some(Int),
            BinOp::Eq | BinOp::Ne => (l == r && l != Void).then_some(Bool),
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => match (l, r) {
                (Int, Int) | (Float, Float) | (Str, Str) | (Char, Char) => Some(Bool),
                _ => None,
            },
            BinOp::And | BinOp::Or => (l == Bool && r == Bool).then_some(Bool),
        };
        if let Some(t) = res {
            return t;
        }

        let sym = if compound { format!("{}=", op.symbol()) } else { op.symbol().to_string() };
        let needs = match op {
            BinOp::Add => "two `int`s, two `float`s, two `str`s or two arrays of one type",
            BinOp::Sub | BinOp::Mul | BinOp::Div => "two `int`s or two `float`s",
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => "two `int`s, two `float`s, two `str`s or two `char`s",
            BinOp::Mod => "two `int`s",
            BinOp::Eq | BinOp::Ne => "two values of the same type",
            BinOp::And | BinOp::Or => "two `bool`s",
        };
        let msg = format!("cannot use `{sym}` on `{}` and `{}`: `{sym}` needs {needs}", l.name(), r.name());
        // a literal that only needs another spelling (`2` as `2.0`, `"a"` as `'a'`)
        let mut literal_fix = None;
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
                    (Some(a), Some(b)) => {
                        format!("comparisons do not chain: write `{a} {} {b} && {b} {} {rs}`", lop.symbol(), op.symbol())
                    }
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
                        literal_fix = float_literal(re);
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
                    (Some(a), Some(b)) => {
                        literal_fix = float_literal(if int_left { le } else { re });
                        format!("use one type on both sides, e.g. `{a} {sym} {b}` (Nyra never converts numbers implicitly)")
                    }
                    _ => "Nyra never converts numbers implicitly: use `float(x)` on the `int` side, or `int(x)` on the `float` side"
                        .to_string(),
                }
            }
        } else if op == BinOp::Add && (l == Char || r == Char) {
            if l == Char && r == Int {
                let c = ls.clone().unwrap_or_else(|| "c".into());
                let n = rs.clone().unwrap_or_else(|| "n".into());
                format!("characters have no arithmetic: compute with the code and convert back, `char({c}.code() + {n})`")
            } else {
                let c = if l == Char { ls.clone() } else { rs.clone() }.unwrap_or_else(|| "c".into());
                format!("convert the character to text first: `str({c})` (or use interpolation, `\"{{a}}{{b}}\"`)")
            }
        } else if op == BinOp::Add && (l == Str || r == Str) {
            let other = if l == Str { &rs } else { &ls };
            match other {
                Some(o) if !o.contains('"') => {
                    format!("`+` joins two strings: convert the other side with `str({o})`, or use interpolation")
                }
                _ => {
                    let whole = self.chain.as_ref().filter(|(nodes, _)| nodes.contains(&span)).and_then(|(_, t)| t.clone());
                    match whole {
                        Some(t) => format!(
                            "`+` joins two strings: convert the other side with `str(...)`, or use interpolation, e.g. `\"{t}\"`"
                        ),
                        None => "`+` joins two strings: convert the other side with `str(...)`, or use interpolation".to_string(),
                    }
                }
            }
        } else if op == BinOp::Add && (l.elem().is_some() || r.elem().is_some()) {
            let (arr, other, os) = if l.elem().is_some() { (l, r, &rs) } else { (r, l, &ls) };
            if arr.elem() == Some(other) {
                match os {
                    Some(o) => format!("add one element: `xs + [{o}]` (or `xs.push({o})` on a `var`)"),
                    None => "add one element: `xs + [x]` (or `xs.push(x)` on a `var`)".to_string(),
                }
            } else {
                format!("`+` joins two arrays of the same type: `{}` and `{}` differ", l.name(), r.name())
            }
        } else if op == BinOp::Mod && (l == Float || r == Float) {
            "`%` works on `int` only: convert a float with `int(x)`, or compute a float remainder as `a - b * float(int(a / b))`"
                .to_string()
        } else if matches!(op, BinOp::And | BinOp::Or) {
            let fix = |e: &Expr, t: Type, s: &Option<String>| -> Option<String> {
                let s = s.as_ref()?;
                // `a + b != 0` needs no parentheses, `a < b != 0` would
                let loose = matches!(
                    &e.kind,
                    ExprKind::Binary(
                        BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge | BinOp::And | BinOp::Or,
                        ..
                    )
                );
                let wrapped = if loose { format!("({s})") } else { s.clone() };
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
            } else if (l == Char && r == Str) || (l == Str && r == Char) {
                let s = if l == Str { &ls } else { &rs };
                match s.as_deref().and_then(|t| t.strip_prefix('"')).and_then(|t| t.strip_suffix('"')) {
                    Some(t) if t.chars().count() == 1 => {
                        literal_fix = char_literal(if l == Str { le } else { re });
                        format!("a character is written in single quotes: `'{t}'`")
                    }
                    _ => "compare a `char` with a `char` (`c == 'a'`), or text with text (`str(c) == s`)".to_string(),
                }
            } else {
                format!("`{sym}` compares two values of one type: convert one side (`float(x)`, `int(x)`) or write the literal with the right type")
            }
        } else if matches!(op, BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge) && (l.elem().is_some() || l.struct_name().is_some()) {
            "only numbers, strings and characters are ordered: compare elements or fields instead".to_string()
        } else {
            format!("both sides of `{sym}` need the same numeric type: check the types of the two operands")
        };
        self.errs.push(Diag::new("E0210", msg, span).hint(hint).fix_opt(literal_fix));
        Type::Unknown
    }

    /// The type of an argument: `inout x` and `label: v` are typed as what they wrap.
    fn arg_type(&mut self, a: &mut Expr, want: Option<Type>) -> Type {
        let t = match &mut a.kind {
            ExprKind::Inout(v) | ExprKind::Labeled(_, v) => self.expr_with(v, want),
            _ => return self.expr_with(a, want),
        };
        a.ty = t;
        t
    }

    /// `abs(x)`, `min(a, b)`, `max(a, b)` on two `int`s or two `float`s.
    fn math(&mut self, name: &str, args: &mut [Expr], span: Span) -> Type {
        let tys: Vec<Type> = args.iter_mut().map(|a| self.arg_type(a, None)).collect();
        let want = if name == "abs" { 1 } else { 2 };
        let example = if name == "abs" {
            "abs(x)"
        } else if name == "min" {
            "min(a, b)"
        } else {
            "max(a, b)"
        };
        if tys.len() != want {
            self.errs.push(
                Diag::new(
                    "E0204",
                    format!("`{name}` takes {} but {} {} given", count(want, "argument"), tys.len(), was_were(tys.len())),
                    span,
                )
                .hint(format!("call it as `{example}`")),
            );
            return Type::Unknown;
        }
        if tys.iter().any(|t| t.is_unknown()) {
            return Type::Unknown;
        }
        let t = tys[0];
        if !matches!(t, Type::Int | Type::Float) || tys.iter().any(|x| *x != t) {
            let found = tys.iter().map(|t| format!("`{}`", t.name())).collect::<Vec<_>>().join(" and ");
            self.errs.push(
                Diag::new("E0203", format!("`{name}` needs two `int`s or two `float`s, found {found}"), args[0].span)
                    .hint("convert first: `float(n)` or `int(x)`, so both are the same number type"),
            );
            return Type::Unknown;
        }
        t
    }

    fn call(&mut self, name: &str, args: &mut [Expr], span: Span, want: Option<Type>) -> Type {
        if self.structs.contains_key(name) {
            return self.construct(name, args, span);
        }
        if BUILTINS.contains(&name) {
            return self.builtin(name, args, span);
        }
        // `abs`, `min` and `max` are builtins unless the program defines its own
        if MATH.contains(&name) && !self.fns.contains_key(name) {
            return self.math(name, args, span);
        }

        let Some(sig) = self.fns.get(name) else {
            let tys: Vec<Type> = args.iter_mut().map(|a| self.arg_type(a, None)).collect();
            let d = Diag::new("E0202", format!("undefined function `{name}`"), span);
            // a struct's name too: `point(...)` after `struct Point` was renamed
            let names =
                self.fns.keys().map(String::as_str).chain(BUILTINS.iter().copied()).chain(self.structs.keys().map(String::as_str));
            let mut fix = Vec::new();
            let hint = if self.lookup(name).is_some() {
                format!("`{name}` is a variable, not a function: remove the parentheses (or give the function another name)")
            } else if let Some(h) = hints::undefined_function(name) {
                let plain = matches!(args, [a] if !matches!(a.kind, ExprKind::Labeled(..) | ExprKind::Inout(_)));
                match (name, &*args) {
                    ("println" | "puts" | "writeln", _) if plain => fix.push(Edit::replace(span, name, "print")),
                    // `len(xs)` is `xs.len()`
                    ("len" | "length" | "size", [a]) if matches!(tys[0], Type::Array(_) | Type::Str) => {
                        if let ExprKind::Var(v) = &a.kind {
                            fix.push(Edit::range(span, a.span, &format!("{name}("), ""));
                            fix.push(Edit::range(after(a.span, v), after(a.span, &format!("{v})")), ")", ".len()"));
                        }
                    }
                    _ => {}
                }
                h
            } else if let Some((h, f)) = suggest_fix(name, names) {
                fix.extend(f.map(|f| Edit::replace(span, name, f)));
                h
            } else if let Some(fields) = declared_fields(name, args, &tys) {
                // `Vec2(x: 1.0, y: 2.0)` and no struct `Vec2`: it was probably meant as a struct
                format!("no struct `{name}` is defined: declare it before use, `struct {name} {{ {fields} }}`")
            } else {
                format!("define it: `fn {name}(...) {{ ... }}` (the builtins are `print`, `int`, `float`, `str`, `char`, `free` and `keep`)")
            };
            self.errs.push(d.hint(hint).fix(fix));
            return Type::Unknown;
        };
        let _ = want;
        let (params, names, inout, ret, shown) = (sig.params.clone(), sig.names.clone(), sig.inout.clone(), sig.ret, sig.show(name));
        let tys: Vec<Type> = args
            .iter_mut()
            .enumerate()
            .map(|(i, a)| {
                let w = params.get(i).copied();
                self.arg_type(a, w)
            })
            .collect();
        if params.len() != tys.len() {
            let hint = if tys.len() < params.len() {
                let missing: Vec<String> =
                    names[tys.len()..].iter().zip(&params[tys.len()..]).map(|(n, t)| format!("`{n}: {}`", t.name())).collect();
                format!("also pass {}: the call is `{name}({})`", missing.join(", "), names.join(", "))
            } else {
                format!("remove the extra argument(s): the signature is `fn {shown}`")
            };
            self.errs.push(
                Diag::new(
                    "E0204",
                    format!("`{shown}` takes {} but {} {} given", count(params.len(), "argument"), tys.len(), was_were(tys.len())),
                    span,
                )
                .hint(hint),
            );
            return ret;
        }
        let plain = without_names(name, &names, args);
        let in_order = args.iter().zip(&names).all(|(a, n)| match &a.kind {
            ExprKind::Labeled(l, _) => l == n,
            _ => true,
        });
        // which variables the `inout` arguments change: each at most once per call
        let mut roots: Vec<(String, Span)> = Vec::new();
        // the script variables among them
        let mut ginout: Vec<(usize, Span)> = Vec::new();
        for (i, a) in args.iter_mut().enumerate() {
            let want = params[i];
            match (&mut a.kind, inout[i]) {
                (ExprKind::Inout(place), true) => {
                    let place: &mut Expr = place;
                    self.expect_ty(want, tys[i], place, Ctx::Arg { f: name, idx: i, param: &names[i] });
                    if data::place_root(place).is_none() {
                        self.errs.push(
                            Diag::new(
                                "E0229",
                                format!("`inout` needs a variable, a field or an element, but argument {} is a computed value", i + 1),
                                a.span,
                            )
                            .hint("store the value in a `var` first, then pass `inout` that variable"),
                        );
                        continue;
                    }
                    self.check_place(place, "pass `inout`", a.span);
                    let root = data::place_root(place).unwrap_or_default().to_string();
                    if let Some((_, first)) = roots.iter().find(|(r, _)| *r == root) {
                        self.errs.push(
                            Diag::new(
                                "E0237",
                                format!("`inout` arguments must be different variables: `{root}` is passed twice (also at column {})", first.col),
                                a.span,
                            )
                            .hint(if matches!(place.kind, ExprKind::Index(..)) {
                                format!("two elements of one array: to exchange them write `{root}.swap(i, j)`; else change one through a temporary (copy into a `var`, call, then assign back)")
                            } else {
                                "change one of them through a temporary: copy into a `var`, call, then assign back".to_string()
                            }),
                        );
                    } else {
                        if let Some(g) = self.global_ref(&root) {
                            ginout.push((g, a.span));
                        }
                        roots.push((root, a.span));
                    }
                }
                (ExprKind::Inout(_), false) => self.errs.push(
                    Diag::new("E0237", format!("parameter `{}` of `{name}` is not `inout`", names[i]), a.span).hint(format!(
                        "remove `inout` here, or declare the parameter `inout {}: {}`",
                        names[i],
                        want.name()
                    )),
                ),
                (_, true) => {
                    let s = show(a).unwrap_or_else(|| "x".into());
                    self.errs.push(
                        Diag::new("E0237", format!("argument {} of `{name}` is `inout`: the call must say so", i + 1), a.span)
                            .hint(format!("write `inout {s}`: the call shows what can change, e.g. `{name}(inout {s})`")),
                    );
                }
                (ExprKind::Labeled(label, v), false) => {
                    let label = label.clone();
                    let hint = match &plain {
                        Some(call) => format!("names are only for building structs: write the values in parameter order, `{call}`"),
                        None => {
                            format!("names are only for building structs: write the value alone, `{name}(...)` in parameter order")
                        }
                    };
                    // the names are already in parameter order: dropping them changes nothing else
                    let fix = in_order.then(|| Edit::range(a.span, start(v), &format!("{label}:"), ""));
                    self.errs.push(
                        Diag::new("E0226", format!("named argument `{label}:` in a call to function `{name}`"), a.span)
                            .hint(hint)
                            .fix_opt(fix),
                    );
                }
                (_, false) => {
                    self.expect_ty(want, tys[i], a, Ctx::Arg { f: name, idx: i, param: &names[i] });
                }
            }
        }
        self.record_call(name, span, ginout);
        ret
    }

    /// `Point(x: 1, y: 2)`: every field, named, in declaration order.
    fn construct(&mut self, name: &str, args: &mut [Expr], span: Span) -> Type {
        let fields = self.structs[name].fields.clone();
        let template = || -> String {
            let fs: Vec<String> = fields.iter().map(|(f, _, _)| format!("{f}: ...")).collect();
            format!("{name}({})", fs.join(", "))
        };
        let mut seen: Vec<String> = Vec::new();
        let mut order_reported = false;
        // `Point(1, 2)`: one value per field, in order, so each value gets its field's name
        let fits = args.len() == fields.len()
            && args.iter().zip(&fields).all(|(a, (f, _, _))| match &a.kind {
                ExprKind::Labeled(l, _) => l == f,
                ExprKind::Inout(_) => false,
                _ => true,
            });
        let labels: Vec<Edit> = if fits {
            args.iter()
                .enumerate()
                .filter(|(_, a)| !matches!(a.kind, ExprKind::Labeled(..)))
                .map(|(i, a)| {
                    let before = if i == 0 { format!("{name}(") } else { ",".to_string() };
                    Edit::insert(start(a), format!("{}: ", fields[i].0)).after(before)
                })
                .collect()
        } else {
            Vec::new()
        };
        for (i, a) in args.iter_mut().enumerate() {
            let a_span = a.span;
            match &mut a.kind {
                ExprKind::Labeled(label, v) => {
                    let label = label.clone();
                    match fields.iter().position(|(f, _, _)| *f == label) {
                        None => {
                            self.expr(v);
                            let names: Vec<&str> = fields.iter().map(|(f, _, _)| f.as_str()).collect();
                            let (hint, fix) = match suggest_fix(&label, names.iter().copied()) {
                                Some((h, f)) => (h, f.map(|f| Edit::replace(a_span, &label, f))),
                                None => (format!("build it as `{}`", template()), None),
                            };
                            self.errs
                                .push(Diag::new("E0224", format!("`{name}` has no field `{label}`"), a_span).hint(hint).fix_opt(fix));
                        }
                        Some(j) => {
                            if j != i && !order_reported {
                                order_reported = true;
                                self.errs.push(
                                    Diag::new(
                                        "E0225",
                                        format!("the fields of `{name}(...)` must be named in declaration order: `{}` is field {} of `{name}`", label, j + 1),
                                        a_span,
                                    )
                                    .hint(format!("write them in this order: `{}`", template())),
                                );
                            }
                            let want = fields[j].1;
                            let t = self.expr_with(v, Some(want));
                            a.ty = t;
                            let v: &Expr = v;
                            self.expect_ty(want, t, v, Ctx::Field { s: name, f: &label });
                            seen.push(label);
                        }
                    }
                }
                _ => {
                    self.arg_type(a, None);
                    if !order_reported {
                        order_reported = true;
                        self.errs.push(
                            Diag::new("E0225", format!("the fields of `{name}(...)` must be named, in declaration order"), a_span)
                                .hint(format!("name every field: `{}`", template()))
                                .fix(labels.clone()),
                        );
                    }
                }
            }
        }
        if !order_reported {
            let missing: Vec<&str> = fields.iter().map(|(f, _, _)| f.as_str()).filter(|f| !seen.iter().any(|s| s == f)).collect();
            if !missing.is_empty() {
                self.errs.push(
                    Diag::new(
                        "E0223",
                        format!(
                            "missing {} in `{name}(...)`: {}",
                            if missing.len() == 1 { "field" } else { "fields" },
                            missing.iter().map(|f| format!("`{f}`")).collect::<Vec<_>>().join(", ")
                        ),
                        span,
                    )
                    .hint(format!("every field needs a value: `{}`", template())),
                );
            }
        }
        Type::structure(name)
    }

    fn builtin(&mut self, name: &str, args: &mut [Expr], span: Span) -> Type {
        let ret = match name {
            "print" | "free" | "keep" => Type::Void,
            "int" => Type::Int,
            "float" => Type::Float,
            "str" => Type::Str,
            _ => Type::Char,
        };
        // `print(a, b, end: "")`: the last argument may replace the line end
        if name == "print" {
            if let Some((last, rest)) = args.split_last_mut() {
                if matches!(&last.kind, ExprKind::Labeled(l, _) if l == "end") {
                    let t = self.arg_type(last, Some(Type::Str));
                    if let ExprKind::Labeled(_, v) = &last.kind {
                        self.expect_ty(Type::Str, t, v, Ctx::Arg { f: "print", idx: rest.len(), param: "end" });
                    }
                    if rest.is_empty() {
                        self.errs.push(
                            Diag::new("E0204", "`print` needs a value before `end:`", span)
                                .hint("give it the value to print: `print(\"text\", end: \"\")`"),
                        );
                        return ret;
                    }
                    return self.builtin("print", rest, span);
                }
            }
        }
        let tys: Vec<Type> = args.iter_mut().map(|a| self.arg_type(a, None)).collect();
        for a in args.iter() {
            match &a.kind {
                ExprKind::Inout(_) => self
                    .errs
                    .push(Diag::new("E0237", format!("the argument of `{name}` is not `inout`"), a.span).hint("remove `inout`")),
                ExprKind::Labeled(label, _) => self.errs.push(
                    Diag::new("E0226", format!("named argument `{label}:` in a call to `{name}`"), a.span).hint(if name == "print" {
                        "write the values alone: `print(a, b)`; the only named argument is a last `end:`, as in `print(a, end: \"\")`"
                            .to_string()
                    } else {
                        format!("write the value alone: `{name}(x)`")
                    }),
                ),
                _ => {}
            }
        }
        // `print(a, b, c)`: several values on one line, separated by spaces
        if name == "print" && tys.len() > 1 {
            for (a, t) in args.iter().zip(&tys) {
                if *t == Type::Void {
                    let msg = format!("`print` needs a value to show, but {} returns nothing", call_text(a));
                    self.errs.push(Diag::new("E0203", msg, a.span).hint(self.no_value_hint(a)));
                }
            }
            return ret;
        }
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
                "free" | "keep" => format!("`{name}` takes one variable: `{name}(x)`"),
                _ => format!("`{name}` converts one value: `{name}(x)`"),
            };
            self.errs.push(
                Diag::new("E0204", format!("`{name}` takes exactly 1 argument but {} {} given", tys.len(), was_were(tys.len())), span)
                    .hint(hint),
            );
            return ret;
        }
        let t = tys[0];
        if name == "free" || name == "keep" {
            self.free_keep(name, &args[0], t, span);
            return ret;
        }
        if t.is_unknown() {
            return ret;
        }
        if t == Type::Void {
            let msg = match name {
                "print" => format!("`print` needs a value to show, but {} returns nothing", call_text(&args[0])),
                "str" => format!("`str` needs a value to convert, but {} returns nothing", call_text(&args[0])),
                _ => format!("`{name}` needs a number, but {} returns nothing", call_text(&args[0])),
            };
            self.errs.push(Diag::new("E0203", msg, args[0].span).hint(self.no_value_hint(&args[0])));
            return ret;
        }
        let ok = match name {
            "print" | "str" => true,
            "int" | "float" => matches!(t, Type::Int | Type::Float | Type::Str),
            _ => matches!(t, Type::Int | Type::Char),
        };
        if ok {
            return ret;
        }
        let s = show(&args[0]);
        let (msg, hint) = match (name, t) {
            ("int" | "float", Type::Char) => {
                (format!("`{name}(c)` is ambiguous for a `char`: the character's code, or the digit it shows?"), {
                    let c = s.clone().unwrap_or_else(|| "c".into());
                    format!("the code: `{c}.code()`; a digit's value: `{c}.code() - '0'.code()`")
                })
            }
            ("int" | "float", Type::Bool) => {
                let (one, zero) = if name == "int" { ("1", "0") } else { ("1.0", "0.0") };
                (
                    format!("`{name}(x)` needs an `int`, a `float` or a `str`, found `bool`"),
                    match &s {
                        Some(s) => format!("choose the numbers yourself: `if {s} {{ {one} }} else {{ {zero} }}`"),
                        None => format!("choose the numbers yourself: `if flag {{ {one} }} else {{ {zero} }}`"),
                    },
                )
            }
            ("int" | "float", _) => (
                format!("`{name}(x)` needs an `int`, a `float` or a `str`, found `{}`", t.name()),
                "only numbers and text that holds a number can be converted".to_string(),
            ),
            (_, Type::Float) => (
                "`char(x)` needs an `int` code, found `float`".to_string(),
                match &s {
                    Some(s) => format!("convert the number first: `char(int({s}))`"),
                    None => "convert the number first: `char(int(x))`".to_string(),
                },
            ),
            (_, Type::Str) => (
                "`char(x)` needs an `int` code, found `str`".to_string(),
                "take a character from a string by index: `s[0]`".to_string(),
            ),
            _ => (
                format!("`char(x)` needs an `int` code, found `{}`", t.name()),
                "`char(65)` is `'A'`: pass a character code".to_string(),
            ),
        };
        self.errs.push(Diag::new("E0203", msg, args[0].span).hint(hint));
        ret
    }

    /// `free(x)` / `keep(x)`: `x` is a local variable that owns heap memory.
    fn free_keep(&mut self, name: &str, arg: &Expr, t: Type, span: Span) {
        let ExprKind::Var(var) = &arg.kind else {
            if !t.is_unknown() {
                self.errs.push(
                    Diag::new("E0238", format!("`{name}` needs a local variable"), arg.span)
                        .hint("to drop one element or field early, assign an empty value instead: `xs[i] = []`, `p.name = \"\"`"),
                );
            }
            return;
        };
        let Some((decl, arena)) = self.lookup(var).map(|v| (v.decl, v.arena)) else { return };
        let problem = match decl {
            _ if self.global_of(var).is_some() => Some((
                format!("cannot {name} script variable `{var}` in function `{}`: the script still needs it", self.fname),
                format!("only variables declared in this function can be freed or kept; call `{name}({var})` at the top level of the script"),
            )),
            Decl::Param => Some((format!("cannot {name} parameter `{var}`: the caller owns it"), "only variables declared with `let` or `var` in this function can be freed or kept".to_string())),
            Decl::Inout => Some((format!("cannot {name} `inout` parameter `{var}`: the caller still needs a value"), format!("assign a new value instead: `{var} = ...`"))),
            Decl::Loop => Some((format!("cannot {name} the loop variable `{var}`"), "the loop variable is a copy of each element: there is nothing to free".to_string())),
            Decl::Lambda => Some((format!("cannot {name} the lambda parameter `{var}`"), "a lambda only reads values: there is nothing to free".to_string())),
            _ if !t.is_unknown() && !self.managed(t) => Some((
                format!("nothing to {name}: `{var}` is {}, which owns no heap memory", article(t)),
                "only strings, arrays and structs that contain them can be freed or kept".to_string(),
            )),
            _ if self.arena_depth > 0 && arena < self.arena_depth => {
                Some((format!("`{var}` cannot be changed inside this `arena` block: it was declared outside it"), format!("call `{name}({var})` after the `arena` block")))
            }
            _ => None,
        };
        if let Some((msg, hint)) = problem {
            self.errs.push(Diag::new("E0238", msg, span).hint(hint));
            return;
        }
        if name == "free" {
            self.freed.insert(var.clone(), Freed { line: span.line, maybe: false });
        }
    }
}

/// The fields of the struct that `Vec2(x: 1.0, y: 2.0)` was meant to build, as they are declared:
/// `x: float, y: float`. `None` unless the name looks like a struct and every argument is named.
fn declared_fields(name: &str, args: &[Expr], tys: &[Type]) -> Option<String> {
    if !name.starts_with(|c: char| c.is_uppercase()) {
        return None;
    }
    let fields: Option<Vec<String>> = args
        .iter()
        .zip(tys)
        .map(|(a, t)| match &a.kind {
            ExprKind::Labeled(label, _) if !t.is_unknown() && *t != Type::Void => Some(format!("{label}: {}", t.name())),
            _ => None,
        })
        .collect();
    fields.filter(|f| !f.is_empty()).map(|f| f.join(", "))
}

/// `area(width: 3, height: 4)` written as `area(3, 4)`: the values without their names, in the order of
/// the parameters when every parameter is named once. `None` if a value is too long to show.
fn without_names(name: &str, params: &[String], args: &[Expr]) -> Option<String> {
    let label = |a: &Expr| match &a.kind {
        ExprKind::Labeled(l, _) => Some(l.clone()),
        _ => None,
    };
    let value = |a: &Expr| match &a.kind {
        ExprKind::Labeled(_, v) => show(v),
        _ => show(a),
    };
    let named: Option<Vec<&Expr>> = params.iter().map(|p| args.iter().find(|a| label(a).as_deref() == Some(p.as_str()))).collect();
    let ordered: Vec<&Expr> = match named {
        Some(v) if v.len() == args.len() => v,
        _ => args.iter().collect(),
    };
    let values: Option<Vec<String>> = ordered.into_iter().map(value).collect();
    Some(format!("{name}({})", values?.join(", ")))
}

/// The freed variables after two paths join: freed on both = freed, on one = maybe freed.
fn join(a: &HashMap<String, Freed>, b: &HashMap<String, Freed>) -> HashMap<String, Freed> {
    let mut out = HashMap::new();
    for (name, fa) in a {
        let maybe = match b.get(name) {
            Some(fb) => fa.maybe || fb.maybe,
            None => true,
        };
        out.insert(name.clone(), Freed { line: fa.line, maybe });
    }
    for (name, fb) in b {
        out.entry(name.clone()).or_insert(Freed { line: fb.line, maybe: true });
    }
    out
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

/// True if the place `e` is, or reaches into, a value inside a map: `m[k]`, `m[k].x`, `m[k][0]`.
fn map_step(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Index(b, _) if b.ty.map_kv().is_some() => true,
        ExprKind::Index(b, _) | ExprKind::Field(b, _) => map_step(b),
        _ => false,
    }
}
