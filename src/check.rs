//! Type checker. Annotates every expression with its type and collects all
//! errors in one pass. Nyra never converts types implicitly and never allows
//! shadowing: one name means one thing. Every error names the variables, functions
//! and types involved and says how to fix the program.

use std::collections::HashMap;

use crate::ast::*;
use crate::diag::{after, suggest, suggest_fix, Diag, Edit};
use crate::helpers::{self, H};
use crate::hints;
use crate::stdlib;
use data::StructInfo;

pub mod data;
mod globals;
mod lambda;

pub const BUILTINS: &[&str] = &["print", "int", "float", "str", "char", "free", "keep"];
/// Builtins a program may also define itself (its own definition wins).
pub const MATH: &[&str] = &["abs", "min", "max"];

/// An `enum`: its variants, in order. A value is a struct: the number of its variant (`tag`), then
/// the values of the variants that carry any, in slots named `Circle_0`, `Rect_0`, `Rect_1`, ...
struct EnumInfo {
    variants: Vec<String>,
    /// The types of the values each variant carries.
    payload: Vec<Vec<Type>>,
    span: Span,
}

impl EnumInfo {
    /// True if some variant carries values.
    fn has_payload(&self) -> bool {
        self.payload.iter().any(|p| !p.is_empty())
    }

    /// The name of the field that holds value `i` of variant `v`.
    fn slot(&self, v: usize, i: usize) -> String {
        format!("{}_{i}", self.variants[v])
    }

    /// `Shape.Circle(float)`, `Shape.Rect(float, float)`, `Shape.Empty`
    fn show(&self, en: &str, v: usize) -> String {
        if self.payload[v].is_empty() {
            format!("{en}.{}", self.variants[v])
        } else {
            let ts: Vec<String> = self.payload[v].iter().map(|t| t.name()).collect();
            format!("{en}.{}({})", self.variants[v], ts.join(", "))
        }
    }
}

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
    /// Helper functions (see `helpers.rs`) that were asked for and are not checked yet.
    generated: Vec<Func>,
    /// The enums of the program (each is also a struct with one field, `tag`).
    enums: HashMap<String, EnumInfo>,
    /// How many `match` statements were turned into `if` chains (each has a hidden variable).
    matches: usize,
    /// The imported file of the functions that come from one, by function name.
    files: HashMap<String, String>,
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
        generated: Vec::new(),
        enums: HashMap::new(),
        matches: 0,
        files: prog.files.clone(),
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

    // enums first: a struct with one field, the number of the variant
    for ed in &prog.enums {
        if !ed.name.starts_with(|ch: char| ch.is_uppercase()) {
            let mut fixed = ed.name.clone();
            if let Some(first) = fixed.get(..1) {
                fixed = first.to_uppercase() + &fixed[1..];
            }
            c.errs.push(
                Diag::new("E0221", format!("enum name `{}` must start with an uppercase letter", ed.name), ed.span)
                    .hint(format!("write `enum {fixed}`: type names start uppercase, variants are written `{fixed}.Name`")),
            );
        }
        if let Some(first) = c.structs.get(&ed.name) {
            c.errs.push(
                Diag::new("E0206", format!("`{}` is already defined (line {})", ed.name, first.span.line), ed.span)
                    .hint("rename one of them: a struct or enum name can be used only once"),
            );
            continue;
        }
        if ed.variants.is_empty() {
            c.errs.push(
                Diag::new("E0284", format!("enum `{}` has no variants", ed.name), ed.span)
                    .hint(format!("list its cases: `enum {} {{ A, B }}`", ed.name)),
            );
        }
        let mut variants: Vec<String> = Vec::new();
        let mut payload: Vec<Vec<Type>> = Vec::new();
        let mut fields = vec![("tag".to_string(), Type::Int, ed.span)];
        for v in &ed.variants {
            let (name, at) = (&v.name, v.span);
            if variants.contains(name) {
                c.errs.push(
                    Diag::new("E0284", format!("variant `{name}` is defined twice in enum `{}`", ed.name), at)
                        .hint("rename one of them: the variants of an enum are all different"),
                );
                continue;
            }
            for (i, (t, tat)) in v.fields.iter().enumerate() {
                fields.push((format!("{name}_{i}"), *t, *tat));
            }
            variants.push(name.clone());
            payload.push(v.fields.iter().map(|(t, _)| *t).collect());
        }
        c.structs.insert(ed.name.clone(), StructInfo { fields, span: ed.span });
        c.enums.insert(ed.name.clone(), EnumInfo { variants, payload, span: ed.span });
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
    // the values the variants carry: known types, and no enum that contains itself by value
    for ed in &prog.enums {
        for v in &ed.variants {
            for (t, at) in &v.fields {
                c.check_type(*t, *at);
            }
        }
        if c.enums.contains_key(&ed.name) && data::contains_itself(&ed.name, &c.structs) {
            c.errs.push(
                Diag::new("E0222", format!("enum `{}` contains itself, so its size would be infinite", ed.name), ed.span).hint(format!(
                    "keep the nested values in an array instead, e.g. `Node([{}])` (an array can be empty)",
                    ed.name
                )),
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
        } else if f.name.contains('.') && !c.files.contains_key(&f.name) {
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
    // the helper functions the program asked for (checking one may ask for more)
    while let Some(mut f) = c.generated.pop() {
        c.func(&mut f);
        prog.funcs.push(f);
    }
    // an enum is a struct: the number of the variant, then the values the variants carry
    let mut enums: Vec<(&String, &EnumInfo)> = c.enums.iter().collect();
    enums.sort_by(|a, b| a.0.cmp(b.0));
    for (name, info) in enums {
        let fields = c.structs[name.as_str()].fields.iter().map(|(n, t, s)| Field { name: n.clone(), ty: *t, span: *s }).collect();
        let payloads = info.payload.iter().map(Vec::len).collect();
        prog.structs.push(StructDef { name: name.clone(), fields, span: info.span, variants: info.variants.clone(), payloads });
    }
    // the tuple types the program uses are structs
    let generated = |n: &String| n.starts_with(TUPLE_PREFIX) || n.starts_with(OPTION_PREFIX);
    let mut tuples: Vec<(&String, &StructInfo)> = c.structs.iter().filter(|(n, _)| generated(n)).collect();
    tuples.sort_by(|a, b| a.0.cmp(b.0));
    for (name, info) in tuples {
        let fields = info.fields.iter().map(|(n, t, s)| Field { name: n.clone(), ty: *t, span: *s }).collect();
        prog.structs.push(StructDef { name: name.clone(), fields, span: info.span, variants: Vec::new(), payloads: Vec::new() });
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
            StmtKind::Match { arms, .. } => arms.iter().for_each(|a| collect_decls(&a.body, out)),
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
        ExprKind::Str(_)
        | ExprKind::Interp(_)
        | ExprKind::If(..)
        | ExprKind::Comprehension(_)
        | ExprKind::MapLit(_)
        | ExprKind::Match(..)
        | ExprKind::Bind(..) => return None,
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
        ExprKind::Tuple(items) => {
            let a: Option<Vec<String>> = items.iter().map(show).collect();
            format!("({})", a?.join(", "))
        }
        ExprKind::None => "none".to_string(),
        ExprKind::Some(x) => return show(x),
        ExprKind::Coalesce(a, b) => format!("{} ?? {}", operand(a)?, operand(b)?),
        ExprKind::Fmt(..) | ExprKind::In(..) | ExprKind::Slice(..) => return None,
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
    let name = t.name();
    format!("{} `{name}`", if name.starts_with("int") { "an" } else { "a" })
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

/// What a variant pattern takes apart: the number of the variant, and the names that take its
/// values (name, where, type, the field of the enum value that holds it).
struct VPat {
    k: Option<usize>,
    binds: Vec<(String, Span, Type, String)>,
}

/// What the arms of one `match` have covered so far.
struct MatchState {
    /// 1: an enum, 2: a `bool`, 3: an `int`, `str` or `char`, 0: unknown or not matchable
    kind: u8,
    st: Type,
    enum_name: Option<String>,
    variants: Vec<String>,
    covered: Vec<String>,
    wild: bool,
}

/// `value.tag == k`: is the enum value `value` the variant number `k`.
fn tag_test(value: Expr, k: usize, at: Span) -> Expr {
    let mut tag = Expr::new(ExprKind::Field(Box::new(value), "tag".to_string()), at);
    tag.ty = Type::Int;
    let mut num = Expr::new(ExprKind::Int(k as i64), at);
    num.ty = Type::Int;
    let mut test = Expr::new(ExprKind::Binary(BinOp::Eq, Box::new(tag), Box::new(num)), at);
    test.ty = Type::Bool;
    test
}

/// The name in `Var(name)`.
fn b_name(b: &Expr) -> String {
    match &b.kind {
        ExprKind::Var(n) => n.clone(),
        _ => String::new(),
    }
}

/// Example values for the types of a variant's values: `0.0, 0.0`.
fn example_values(tys: &[Type]) -> String {
    let one = |t: &Type| match t {
        Type::Int => "0".to_string(),
        Type::Float => "0.0".to_string(),
        Type::Bool => "false".to_string(),
        Type::Str => "\"\"".to_string(),
        Type::Char => "'a'".to_string(),
        _ => "...".to_string(),
    };
    tys.iter().map(one).collect::<Vec<_>>().join(", ")
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
        } else if let (false, Some(sig)) = (self.fname.starts_with("core."), self.fns.get(name)) {
            self.errs.push(
                Diag::new("E0206", format!("`{name}` is already the name of a function (line {})", sig.span.line), span)
                    .hint(format!("a variable cannot share a function's name: rename the variable, e.g. `{name}_value`")),
            );
        } else if let (false, Some(sd)) = (self.fname.starts_with("core."), self.structs.get(name)) {
            self.errs.push(
                Diag::new("E0206", format!("`{name}` is already the name of a struct (line {})", sd.span.line), span)
                    .hint("variables start lowercase: rename the variable"),
            );
        } else if let (false, Some((_, at))) = (self.fname.starts_with("core."), self.modules.iter().find(|(m, _)| m == name)) {
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
                if let Some(inner) = t.option_inner() {
                    let ok = self.check_type(inner, span);
                    if ok {
                        self.register_option(t);
                    }
                    return ok;
                }
                if let Some(elems) = t.tuple_elems() {
                    let mut ok = true;
                    for e in elems {
                        ok &= self.check_type(e, span);
                    }
                    if ok {
                        self.register_tuple(t);
                    }
                    return ok;
                }
                let name = t.struct_name().unwrap_or_default();
                if self.structs.contains_key(&name) {
                    return true;
                }
                let names: Vec<&str> = self
                    .structs
                    .keys()
                    .map(String::as_str)
                    .filter(|n| !n.starts_with(TUPLE_PREFIX) && !n.starts_with(OPTION_PREFIX))
                    .collect();
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

    /// Makes the struct of the tuple type `t` known: `(int, str)` has the fields `_0` and `_1`.
    fn register_tuple(&mut self, t: Type) {
        let Some(elems) = t.tuple_elems() else { return };
        let name = t.struct_name().unwrap_or_default();
        if self.structs.contains_key(&name) {
            return;
        }
        let at = Span { line: 0, col: 0 };
        let fields = elems.iter().enumerate().map(|(i, e)| (format!("_{i}"), *e, at)).collect();
        self.structs.insert(name, StructInfo { fields, span: at });
    }

    /// Makes the struct of the optional type `t` known: the fields `has` and `val`.
    fn register_option(&mut self, t: Type) {
        let Some(inner) = t.option_inner() else { return };
        let name = t.struct_name().unwrap_or_default();
        if self.structs.contains_key(&name) {
            return;
        }
        let at = Span { line: 0, col: 0 };
        let fields = vec![("has".to_string(), Type::Bool, at), ("val".to_string(), inner, at)];
        self.structs.insert(name, StructInfo { fields, span: at });
    }

    /// A value of type `got` goes where `want` is needed: a `T` is wrapped into a `T?`.
    fn coerce(&mut self, e: &mut Expr, want: Type, got: Type) -> Type {
        let Some(inner) = want.option_inner() else { return got };
        if got != inner || got.is_unknown() || matches!(e.kind, ExprKind::None) {
            return got;
        }
        let old = std::mem::replace(&mut e.kind, ExprKind::Int(0));
        let mut value = Expr::new(old, e.span);
        value.ty = got;
        e.kind = ExprKind::Some(Box::new(value));
        e.ty = want;
        want
    }

    /// Makes the structs of the tuple types inside `t` known.
    fn register_in(&mut self, t: Type) {
        match t {
            Type::Array(_) => {
                if let Some(e) = t.elem() {
                    self.register_in(e)
                }
            }
            Type::Map(_) => {
                if let Some((k, v)) = t.map_kv() {
                    self.register_in(k);
                    self.register_in(v);
                }
            }
            _ => {
                for e in t.tuple_elems().unwrap_or_default() {
                    self.register_in(e);
                }
                self.register_tuple(t);
                if let Some(inner) = t.option_inner() {
                    self.register_in(inner);
                    self.register_option(t);
                }
            }
        }
    }

    /// The program needs the helper function `h` (see `helpers.rs`): it is added once, and checked
    /// with the other functions. `at` is where the program asked for it.
    fn need(&mut self, h: H, at: Span) {
        let name = h.name();
        if self.fns.contains_key(&name) {
            return;
        }
        let f = match helpers::build(&h, at) {
            Ok(f) => f,
            Err(bug) => panic!("compiler bug: {bug}"),
        };
        let sig = Sig {
            params: f.params.iter().map(|p| p.ty).collect(),
            names: f.params.iter().map(|p| p.name.clone()).collect(),
            inout: f.params.iter().map(|p| p.inout).collect(),
            ret: f.ret,
            span: at,
        };
        self.fns.insert(name, sig);
        self.generated.push(f);
    }

    /// A map key must be an `int`, `str`, `char` or `bool` (E0218).
    fn map_key(&mut self, k: Type, span: Span) -> bool {
        fn key_type(k: Type) -> bool {
            matches!(k, Type::Int | Type::Str | Type::Char | Type::Bool)
                || k.is_unknown()
                || k.tuple_elems().is_some_and(|es| es.into_iter().all(key_type))
        }
        if key_type(k) {
            return true;
        }
        let hint = match k {
            Type::Float => "a float is a bad key (rounding, NaN): use `int` keys, or the text `str(x)`".to_string(),
            _ if k.is_tuple() => {
                "a tuple is a key when each of its parts is an `int`, `str`, `char`, `bool` or such a tuple: use an `int` that stands for a float part, or the text `str(x)`".to_string()
            }
            _ => format!("use an `int` or a `str` that stands for the {}, e.g. an id or a name", k.name()),
        };
        self.errs.push(
            Diag::new(
                "E0218",
                format!("a map key must be `int`, `str`, `char`, `bool` or a tuple of those, found `{}`", k.name()),
                span,
            )
            .hint(hint),
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
        let before = self.errs.len();
        self.func_body(f);
        // a mistake in a function of an imported file is reported with that file
        if let Some(file) = self.files.get(&f.name) {
            for d in &mut self.errs[before..] {
                d.file.get_or_insert_with(|| file.clone());
            }
        }
    }

    fn func_body(&mut self, f: &mut Func) {
        self.ret = if self.defined(f.ret) { f.ret } else { Type::Unknown };
        self.fname = f.name.clone();
        self.decls.clear();
        self.freed.clear();
        collect_decls(&f.body, &mut self.decls);
        self.g.start_func(f);
        self.scopes = vec![HashMap::new()];
        for p in &f.params {
            let ty = if self.defined(p.ty) { p.ty } else { Type::Unknown };
            let decl = if p.inout {
                Decl::Inout
            } else if p.mutable {
                Decl::Var
            } else {
                Decl::Param
            };
            self.declare(&p.name, ty, decl, p.span);
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
        if matches!(s.kind, StmtKind::Match { .. }) {
            let StmtKind::Match { scrut, arms } = std::mem::replace(&mut s.kind, StmtKind::Break) else {
                unreachable!("matched above")
            };
            s.kind = self.match_stmt(scrut, arms, span);
            return;
        }
        match &mut s.kind {
            StmtKind::Let { name, mutable, ty, value } => {
                // a type that is not defined is reported once: the variable then has no known type
                let declared = *ty;
                let want = declared.filter(|w| self.check_type(*w, span));
                let got = self.expr_with(value, want);
                let got = match want {
                    Some(w) => self.coerce(value, w, got),
                    None => got,
                };
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
            StmtKind::Match { .. } => unreachable!("handled at the top of `stmt`"),
            StmtKind::Arena(body) => {
                self.arena_depth += 1;
                self.block(body);
                self.arena_depth -= 1;
            }
            StmtKind::Ret(value) => match value {
                Some(e) => {
                    let want = (self.ret != Type::Void).then_some(self.ret);
                    let t = self.expr_with(e, want);
                    let t = if self.ret == Type::Void { t } else { self.coerce(e, self.ret, t) };
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
        let got = if op.is_none() { self.coerce(value, tt, got) } else { got };
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
        // `match value { ... }` used as a value
        if matches!(e.kind, ExprKind::Match(..)) {
            let t = self.match_expr(e, want);
            e.ty = t;
            return t;
        }
        // `Dir.N` is a variant of the enum `Dir`, `Dir.all()` are all of them, and
        // `Shape.Circle(2.0)` builds a variant that carries values
        let is_enum = |c: &Checker, b: &Expr| {
            matches!(&b.kind, ExprKind::Var(en) if c.enums.contains_key(en.as_str()) && c.lookup(en).is_none())
        };
        let variant = match &e.kind {
            ExprKind::Field(b, v) if is_enum(self, b) => Some((b_name(b), Some(v.clone()))),
            ExprKind::Method(b, m, args) if m == "all" && args.is_empty() && is_enum(self, b) => Some((b_name(b), None)),
            ExprKind::Method(b, m, _) if m != "all" && is_enum(self, b) => Some((b_name(b), Some(m.clone()))),
            _ => None,
        };
        if let Some((en, v)) = variant {
            let ty = Type::structure(&en);
            let variants = self.enums[&en].variants.clone();
            let make = |k: usize, args: Vec<Expr>, span: Span| {
                let mut tag = Expr::new(ExprKind::Int(k as i64), span);
                tag.ty = Type::Int;
                let mut all = vec![tag];
                all.extend(args);
                let mut value = Expr::new(ExprKind::Call(en.clone(), all), span);
                value.ty = ty;
                value
            };
            return match v {
                Some(name) => match variants.iter().position(|x| *x == name) {
                    Some(k) => {
                        // `Shape.Circle(2.0)`: the values the variant carries
                        let given = match std::mem::replace(&mut e.kind, ExprKind::Int(0)) {
                            ExprKind::Method(_, _, args) => Some(args),
                            _ => None,
                        };
                        let info = &self.enums[&en];
                        let (shown, want) = (info.show(&en, k), info.payload[k].clone());
                        let mut args = match given {
                            Some(args) => args,
                            None if !want.is_empty() => {
                                self.errs.push(
                                    Diag::new(
                                        "E0286",
                                        format!(
                                            "the variant `{en}.{name}` carries {}, so it is written with them",
                                            count(want.len(), "value")
                                        ),
                                        span,
                                    )
                                    .hint(format!("write `{shown}`, e.g. `{en}.{name}({})`", example_values(&want))),
                                );
                                e.kind = ExprKind::Int(0);
                                e.ty = Type::Unknown;
                                return Type::Unknown;
                            }
                            None => Vec::new(),
                        };
                        let mut tys: Vec<Type> =
                            args.iter_mut().enumerate().map(|(i, a)| self.arg_type(a, want.get(i).copied())).collect();
                        if args.len() != want.len() {
                            let hint = if want.is_empty() {
                                format!("`{en}.{name}` carries no values: write it without parentheses")
                            } else {
                                format!("write `{shown}`, e.g. `{en}.{name}({})`", example_values(&want))
                            };
                            self.errs.push(
                                Diag::new(
                                    "E0204",
                                    format!(
                                        "`{shown}` takes {} but {} {} given",
                                        count(want.len(), "value"),
                                        args.len(),
                                        was_were(args.len())
                                    ),
                                    span,
                                )
                                .hint(hint),
                            );
                            e.kind = ExprKind::Int(0);
                            e.ty = Type::Unknown;
                            return Type::Unknown;
                        }
                        for (i, a) in args.iter_mut().enumerate() {
                            tys[i] = self.coerce(a, want[i], tys[i]);
                            self.expect_ty(want[i], tys[i], a, Ctx::Arg { f: &format!("{en}.{name}"), idx: i, param: "value" });
                        }
                        e.kind = make(k, args, span).kind;
                        e.ty = ty;
                        ty
                    }
                    None => {
                        if let ExprKind::Method(_, _, args) = &mut e.kind {
                            for a in args.iter_mut() {
                                self.arg_type(a, None);
                            }
                        }
                        let list: Vec<String> = variants.iter().map(|x| format!("`{en}.{x}`")).collect();
                        let (hint, fix) = match suggest_fix(&name, variants.iter().map(String::as_str)) {
                            Some((h, f)) => (h, f.map(|f| Edit::replace(span, &name, f))),
                            None => (format!("the variants of `{en}` are {}", list.join(", ")), None),
                        };
                        self.errs
                            .push(Diag::new("E0278", format!("enum `{en}` has no variant `{name}`"), span).hint(hint).fix_opt(fix));
                        e.ty = Type::Unknown;
                        Type::Unknown
                    }
                },
                None => {
                    if self.enums[&en].has_payload() {
                        let carrying: Vec<String> = (0..variants.len())
                            .filter(|k| !self.enums[&en].payload[*k].is_empty())
                            .map(|k| format!("`{}`", self.enums[&en].show(&en, k)))
                            .collect();
                        self.errs.push(
                            Diag::new("E0288", format!("`{en}.all()` is not available: some variants of `{en}` carry values"), span)
                                .hint(format!(
                                    "{} carries values, so there is no single value to list; list the variants you need by hand",
                                    carrying.join(", ")
                                )),
                        );
                        e.ty = Type::Unknown;
                        return Type::Unknown;
                    }
                    e.kind = ExprKind::Array((0..variants.len()).map(|k| make(k, Vec::new(), span)).collect());
                    e.ty = Type::array(ty);
                    e.ty
                }
            };
        }
        // `r.area()` is `area(r)` when no built-in method is called `area`
        if matches!(&e.kind, ExprKind::Method(_, n, _) if !data::is_method_name(n) && self.fns.contains_key(n.as_str())) {
            let ExprKind::Method(recv, name, mut args) = std::mem::replace(&mut e.kind, ExprKind::Int(0)) else {
                unreachable!("matched above")
            };
            let recv = *recv;
            // a function that changes its first parameter (`inout`) takes the receiver the same way
            let first = if self.fns[name.as_str()].inout.first().copied().unwrap_or(false) {
                let at = recv.span;
                Expr::new(ExprKind::Inout(Box::new(recv)), at)
            } else {
                recv
            };
            args.insert(0, first);
            e.kind = ExprKind::Call(name, args);
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
                None if self.enums.contains_key(name.as_str()) => {
                    let first = self.enums[name.as_str()].variants.first().cloned().unwrap_or_else(|| "A".into());
                    self.errs.push(
                        Diag::new("E0235", format!("`{name}` is an enum, not a value"), span)
                            .hint(format!("pick one of its variants: `{name}.{first}`")),
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
                // `m.get(k) == 3`: the plain value is compared as an optional one
                let (mut lt, mut rt) = (lt, rt);
                if matches!(op, BinOp::Eq | BinOp::Ne) {
                    if lt.option_inner() == Some(rt) {
                        rt = self.coerce(r, lt, rt);
                    } else if rt.option_inner() == Some(lt) {
                        lt = self.coerce(l, rt, lt);
                    }
                }
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
                // `if c { n } else { none }` where an optional is needed: the branches are optional
                let ta = match want {
                    Some(w) if w.is_option() => self.coerce(a, w, ta),
                    _ => ta,
                };
                let tb = self.expr_with(b, if ta.is_unknown() { want } else { Some(ta) });
                let tb = if ta.is_option() { self.coerce(b, ta, tb) } else { tb };
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
            ExprKind::Bind(name, value, body) => {
                let vt = self.expr(value);
                self.scopes.push(HashMap::new());
                let name = name.clone();
                self.declare(&name, vt, Decl::Let, span);
                let bt = self.expr_with(body, want);
                self.scopes.pop();
                bt
            }
            ExprKind::Match(..) => unreachable!("handled before"),
            ExprKind::Array(items) => self.array_lit(items, want, span),
            ExprKind::Tuple(items) => self.tuple_lit(items, want),
            ExprKind::None => match want {
                Some(w) if w.is_option() => w,
                Some(w) if w.is_unknown() => Type::Unknown,
                Some(w) => {
                    self.errs.push(
                        Diag::new("E0276", format!("`none` is not a value of type `{}`", w.name()), span)
                            .hint(format!("only an optional type can be `none`: declare it `{}?`", w.name())),
                    );
                    Type::Unknown
                }
                None => {
                    self.errs.push(
                        Diag::new("E0276", "cannot infer the type of `none`", span)
                            .hint("say which optional type it is where it is declared: `var best: int? = none`, or compare it with a value: `x == none`"),
                    );
                    Type::Unknown
                }
            },
            ExprKind::Some(inner) => {
                let t = self.expr(inner);
                let o = Type::option(t);
                self.register_option(o);
                o
            }
            ExprKind::Coalesce(a, b) => self.coalesce(a, b, span),
            ExprKind::In(item, container) => self.membership(item, container, span),
            ExprKind::Slice(base, lo, hi) => {
                let bt = self.expr(base);
                for bound in [lo, hi].into_iter().flatten() {
                    let it = self.expr(bound);
                    if it != Type::Int && !it.is_unknown() {
                        self.errs.push(
                            Diag::new("E0232", format!("a slice bound must be an `int`, found `{}`", it.name()), bound.span)
                                .hint("a slice is `xs[a..b]` with positions: 0 is the first element, `b` is not included"),
                        );
                    }
                }
                match bt {
                    t if t.is_unknown() => Type::Unknown,
                    Type::Str | Type::Array(_) => bt,
                    t => {
                        self.errs.push(
                            Diag::new("E0233", format!("cannot slice a value of type `{}`", t.name()), span)
                                .hint("only arrays (`xs[1..3]`) and strings (`s[1..3]`) can be sliced"),
                        );
                        Type::Unknown
                    }
                }
            }
            ExprKind::Fmt(inner, spec) => {
                let t = self.expr(inner);
                self.check_spec(t, inner, spec, span);
                Type::Str
            }
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
                let hidden = matches!(&base.kind, ExprKind::Var(v) if v.starts_with("\u{b7}t") || v.starts_with("\u{b7}l"));
                let hidden_option = matches!(&base.kind, ExprKind::Var(v) if v.starts_with("\u{b7}o"));
                let hidden_match = matches!(&base.kind, ExprKind::Var(v) if v.starts_with("\u{b7}m"));
                if hidden_match && !bt.is_unknown() {
                    // the number and the values of an enum value that a `match` takes apart
                    let found = bt.struct_name().and_then(|n| self.structs.get(&n)).and_then(|s| s.fields.iter().find(|f| f.0 == *name));
                    found.map_or(Type::Unknown, |f| f.1)
                } else if hidden_option && !bt.is_unknown() {
                    match bt.option_inner() {
                        Some(inner) => {
                            if name == "has" {
                                Type::Bool
                            } else {
                                inner
                            }
                        }
                        None => {
                            if !self.errs.iter().any(|d| d.code == "E0277" && d.span == span) {
                                self.errs.push(
                                    Diag::new("E0277", format!("`if let` needs an optional value, found {}", article(bt)), span).hint(
                                        "the value must be an optional `T?`, such as `m.get(k)`, `xs.find(x => ...)` or `s.to_int()`: `if let v = m.get(k) { ... }`",
                                    ),
                                );
                            }
                            Type::Unknown
                        }
                    }
                } else if bt.is_tuple() || (hidden && !bt.is_unknown()) {
                    let in_for = matches!(&base.kind, ExprKind::Var(v) if v.starts_with("\u{b7}l"));
                    self.tuple_field(bt, name, hidden, in_for, span)
                } else {
                    self.field_type(bt, name, shown, span)
                }
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
            let tv = match vt {
                Some(w) => self.coerce(v, w, tv),
                None => tv,
            };
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

    /// `x in xs`: the element of an array, the characters of a string, the key of a map.
    fn membership(&mut self, item: &mut Expr, container: &mut Expr, span: Span) -> Type {
        let ct = self.expr(container);
        let it = self.expr_with(item, ct.elem());
        if ct.is_unknown() || it.is_unknown() {
            return Type::Bool;
        }
        let shown = show(item).unwrap_or_else(|| "x".into());
        let boxed = show(container).unwrap_or_else(|| "xs".into());
        let want = match ct {
            Type::Array(_) => ct.elem(),
            Type::Map(_) => ct.map_kv().map(|(k, _)| k),
            Type::Str => Some(it).filter(|t| matches!(t, Type::Str | Type::Char)).or(Some(Type::Char)),
            _ => None,
        };
        let Some(want) = want else {
            self.errs.push(
                Diag::new("E0275", format!("`in` needs an array, a string or a map on its right, found `{}`", ct.name()), span)
                    .hint("to test a number against a range write `x >= a && x < b`"),
            );
            return Type::Bool;
        };
        if it == Type::Void {
            self.errs.push(Diag::new(
                "E0203",
                format!("{} returns nothing, so it cannot be searched for", call_text(item)),
                item.span,
            ));
        } else if it != want {
            let what = match ct {
                Type::Map(_) => "a key",
                Type::Str => "a character or a text",
                _ => "an element",
            };
            self.errs.push(
                Diag::new(
                    "E0275",
                    format!(
                        "cannot look for {} in `{}`: it holds `{}` values, and the left side is {}",
                        article(it),
                        ct.name(),
                        want.name(),
                        article(it)
                    ),
                    span,
                )
                .hint(format!("`{shown} in {boxed}` needs {what} of type `{}` on the left", want.name())),
            );
        }
        Type::Bool
    }

    /// The error for a variant name the enum does not have.
    fn unknown_variant(&self, en: &str, name: &str, span: Span) -> Diag {
        let variants = &self.enums[en].variants;
        let list: Vec<String> = variants.iter().map(|x| format!("`{en}.{x}`")).collect();
        let (hint, fix) = match suggest_fix(name, variants.iter().map(String::as_str)) {
            Some((h, f)) => (h, f.map(|f| Edit::replace(span, name, f))),
            None => (format!("the variants of `{en}` are {}", list.join(", ")), None),
        };
        Diag::new("E0278", format!("enum `{en}` has no variant `{name}`"), span).hint(hint).fix_opt(fix)
    }

    /// Reads `pat` as a pattern of the enum `en`: `Shape.Circle(r)`, `Shape.Rect(w, _)`, `Shape.Empty`.
    /// `None`: it is not written like a variant at all. After a mistake (reported here) the names it
    /// writes still exist, with an unknown type, so the arm gives no second error.
    fn variant_pattern(&mut self, pat: &Expr, en: &str) -> Option<VPat> {
        let (name, args, given) = match &pat.kind {
            ExprKind::Field(b, v) if matches!(&b.kind, ExprKind::Var(n) if n == en) && self.lookup(en).is_none() => (v, &[][..], false),
            ExprKind::Method(b, v, args) if matches!(&b.kind, ExprKind::Var(n) if n == en) && self.lookup(en).is_none() => {
                (v, &args[..], true)
            }
            _ => return None,
        };
        let span = pat.span;
        let unknown = |args: &[Expr]| -> Vec<(String, Span, Type, String)> {
            args.iter()
                .filter_map(|a| match &a.kind {
                    ExprKind::Var(n) if n != "_" => Some((n.clone(), a.span, Type::Unknown, String::new())),
                    _ => None,
                })
                .collect()
        };
        let Some(k) = self.enums[en].variants.iter().position(|v| v == name) else {
            let d = self.unknown_variant(en, name, span);
            self.errs.push(d);
            return Some(VPat { k: None, binds: unknown(args) });
        };
        let info = &self.enums[en];
        let (shown, want) = (info.show(en, k), info.payload[k].clone());
        let names = vec!["_"; want.len()].join(", ");
        if !given && !want.is_empty() {
            self.errs.push(
                Diag::new(
                    "E0287",
                    format!("the pattern `{en}.{name}` does not name the {} of the variant", count(want.len(), "value")),
                    span,
                )
                .hint(format!("a variant that carries values is matched with a name for each, `_` for one you do not need: `{en}.{name}({names})`"))
                .fix(vec![Edit::insert(after(span, name), format!("({names})")).after(name.to_string())]),
            );
            return Some(VPat { k: Some(k), binds: Vec::new() });
        }
        if given && args.len() != want.len() {
            let hint = if want.is_empty() {
                format!("`{en}.{name}` carries no values: write the pattern without parentheses")
            } else {
                format!("write one name for each value, `_` for one you do not need: `{en}.{name}({names})`")
            };
            self.errs.push(
                Diag::new(
                    "E0287",
                    format!("the pattern names {} but `{shown}` carries {}", count(args.len(), "value"), count(want.len(), "value")),
                    span,
                )
                .hint(hint),
            );
            return Some(VPat { k: Some(k), binds: unknown(args) });
        }
        let mut binds = Vec::new();
        for (i, a) in args.iter().enumerate() {
            match &a.kind {
                ExprKind::Var(n) if n == "_" => {}
                ExprKind::Var(n) => binds.push((n.clone(), a.span, want[i], self.enums[en].slot(k, i))),
                _ => {
                    self.errs.push(
                        Diag::new("E0287", "a variant pattern names its values, it does not compare them", a.span).hint(
                            "write a name (it takes the value) or `_` for each value; to test the value use an `if` in the arm",
                        ),
                    );
                    return Some(VPat { k: Some(k), binds: unknown(args) });
                }
            }
        }
        Some(VPat { k: Some(k), binds })
    }

    /// What a `match` can take: 1 an enum, 2 a `bool`, 3 an `int`, `str` or `char`, 0 unknown or not
    /// matchable (the latter is reported).
    fn match_kind(&mut self, st: Type, scrut: &Expr) -> u8 {
        if st.is_unknown() {
            0
        } else if st.struct_name().is_some_and(|n| self.enums.contains_key(&n)) {
            1
        } else if st == Type::Bool {
            2
        } else if matches!(st, Type::Int | Type::Str | Type::Char) {
            3
        } else {
            let shown = show(scrut).unwrap_or_else(|| "the value".into());
            self.errs.push(Diag::new("E0279", format!("cannot `match` a value of type `{}`", st.name()), scrut.span).hint(format!(
                "`match` works on an enum, a `bool`, an `int`, a `str` or a `char`; for other types compare with `if {shown} == ...`"
            )));
            0
        }
    }

    /// The patterns of one arm: reports what is wrong with them and keeps track of what the arms
    /// above and this one cover. Returns the numbers of the variants it tests (an enum) and the names
    /// it binds (name, where, type, the field of the enum value that holds it).
    fn arm_patterns(&mut self, m: &mut MatchState, arm: &mut MatchArm) -> (Vec<usize>, Vec<(String, Span, Type, String)>) {
        let (st, kind) = (m.st, m.kind);
        if m.wild || (kind == 1 && m.covered.len() == m.variants.len()) || (kind == 2 && m.covered.len() == 2) {
            self.errs.push(
                Diag::new("E0283", "this arm can never run: the arms above already cover every case", arm.span)
                    .hint("remove it, or move it above the `_` arm"),
            );
        }
        let mut ks: Vec<usize> = Vec::new();
        let mut binds: Vec<(String, Span, Type, String)> = Vec::new();
        for pat in arm.pats.iter_mut() {
            // a bare variant name: say what to write
            if let (ExprKind::Var(n), Some(en)) = (&pat.kind, &m.enum_name) {
                if self.lookup(n).is_none() {
                    let hint = if m.variants.contains(n) {
                        format!("a variant is written with its enum: `{en}.{n}`")
                    } else {
                        format!(
                            "the variants of `{en}` are {}",
                            m.variants.iter().map(|v| format!("`{en}.{v}`")).collect::<Vec<_>>().join(", ")
                        )
                    };
                    self.errs.push(Diag::new("E0278", format!("`{n}` is not a variant pattern of `{en}`"), pat.span).hint(hint));
                    continue;
                }
            }
            // a variant of the matched enum
            if let Some(en) = m.enum_name.clone() {
                if let Some(vp) = self.variant_pattern(pat, &en) {
                    if let Some(k) = vp.k {
                        let key = k.to_string();
                        if m.covered.contains(&key) {
                            self.errs.push(
                                Diag::new("E0283", "this pattern is already covered by an arm above", pat.span)
                                    .hint("remove it: it can never be reached"),
                            );
                        } else {
                            m.covered.push(key);
                        }
                        ks.push(k);
                    }
                    binds.extend(vp.binds);
                    continue;
                }
            }
            let pt = self.expr_with(pat, Some(st));
            if pt.is_unknown() || st.is_unknown() {
                continue;
            }
            if pt != st {
                self.errs.push(
                    Diag::new("E0279", format!("this pattern is {} but the value matched is {}", article(pt), article(st)), pat.span)
                        .hint(format!("the patterns of this `match` must be `{}` values", st.name())),
                );
                continue;
            }
            // what the pattern covers
            let key = match (&pat.kind, kind) {
                (ExprKind::Bool(b), 2) => Some(b.to_string()),
                (ExprKind::Int(_) | ExprKind::Str(_) | ExprKind::Char(_), 3) => show(pat),
                (ExprKind::Unary(UnOp::Neg, x), 3) if matches!(x.kind, ExprKind::Int(_)) => show(pat),
                _ => None,
            };
            match key {
                None if kind == 0 => {}
                None => {
                    let hint = match &m.enum_name {
                        Some(en) => format!(
                            "a pattern of `{en}` is one of its variants, e.g. `{en}.{}`",
                            m.variants.first().cloned().unwrap_or_default()
                        ),
                        None => "a pattern is a literal value (`1`, `\"a\"`, `'c'`, `true`) or `_`".to_string(),
                    };
                    self.errs.push(Diag::new("E0279", "this pattern is not a constant of the matched type", pat.span).hint(hint));
                }
                Some(k) if m.covered.contains(&k) => self.errs.push(
                    Diag::new("E0283", "this pattern is already covered by an arm above", pat.span).hint("remove it: it can never be reached"),
                ),
                Some(k) => m.covered.push(k),
            }
        }
        if arm.wild {
            m.wild = true;
        }
        // an arm with several patterns cannot name values: they would be different ones
        if arm.pats.len() > 1 && !binds.is_empty() {
            self.errs.push(
                Diag::new("E0287", "an arm with several patterns cannot name the values of a variant", binds[0].1)
                    .hint("write one arm for each variant, or use `_` for the values you do not need"),
            );
            // (the names stay, with no type, so the arm's body gives no second error)
            let mut seen: Vec<String> = Vec::new();
            binds.retain(|b| {
                let fresh = !seen.contains(&b.0);
                seen.push(b.0.clone());
                fresh
            });
            for b in binds.iter_mut() {
                b.2 = Type::Unknown;
            }
        }
        (ks, binds)
    }

    /// E0281 unless the arms cover every case. True if they do.
    fn match_exhaustive(&mut self, m: &MatchState, span: Span) -> bool {
        let missing: Vec<String> = match m.kind {
            1 => m
                .variants
                .iter()
                .enumerate()
                .filter(|(k, _)| !m.covered.contains(&k.to_string()))
                .map(|(_, v)| format!("`{}.{v}`", m.enum_name.as_deref().unwrap_or("")))
                .collect(),
            2 => ["true", "false"].iter().filter(|b| !m.covered.contains(&b.to_string())).map(|b| format!("`{b}`")).collect(),
            3 => vec!["every other value".to_string()],
            _ => Vec::new(),
        };
        let exhaustive = m.wild || missing.is_empty();
        if !exhaustive {
            self.errs.push(
                Diag::new("E0281", format!("this `match` does not cover {}", missing.join(", ")), span)
                    .hint("add an arm for each, or a last arm `_ => ...` that takes everything else"),
            );
        }
        exhaustive
    }

    /// The condition that selects an arm: the value is one of its variants, or equals one of its patterns.
    fn arm_test(&self, m: &MatchState, tmp: &str, ks: &[usize], pats: Vec<Expr>, span: Span) -> Option<Expr> {
        let var = |at: Span| {
            let mut v = Expr::new(ExprKind::Var(tmp.to_string()), at);
            v.ty = m.st;
            v
        };
        let mut tests: Vec<Expr> = Vec::new();
        if m.enum_name.is_some() {
            tests.extend(ks.iter().map(|k| tag_test(var(span), *k, span)));
        } else {
            for pat in pats {
                let at = pat.span;
                let mut test = Expr::new(ExprKind::Binary(BinOp::Eq, Box::new(var(at)), Box::new(pat)), at);
                test.ty = Type::Bool;
                tests.push(test);
            }
        }
        tests.into_iter().reduce(|c, test| {
            let at = test.span;
            let mut or = Expr::new(ExprKind::Binary(BinOp::Or, Box::new(c), Box::new(test)), at);
            or.ty = Type::Bool;
            or
        })
    }

    /// The hidden variable that holds the value being matched, in a scope of its own (the caller pops it).
    fn match_var(&mut self, st: Type, span: Span) -> String {
        self.matches += 1;
        let tmp = format!("\u{b7}m{}", self.matches);
        self.scopes.push(HashMap::new());
        self.declare(&tmp, st, Decl::Let, span);
        tmp
    }

    /// `match value { ... }`: checks the arms and returns the `if` chain that does the same.
    fn match_stmt(&mut self, mut scrut: Expr, mut arms: Vec<MatchArm>, span: Span) -> StmtKind {
        let st = self.expr(&mut scrut);
        let kind = self.match_kind(st, &scrut);
        let enum_name = st.struct_name().filter(|n| self.enums.contains_key(n));
        let variants: Vec<String> = enum_name.as_ref().map(|n| self.enums[n].variants.clone()).unwrap_or_default();
        let mut m = MatchState { kind, st, enum_name, variants, covered: Vec::new(), wild: false };
        // the value being matched is kept in a hidden variable; the arms read the values of a variant from it
        let tmp = self.match_var(st, span);
        let before = self.freed.clone();
        let mut joined: Option<HashMap<String, Freed>> = None;
        // what each arm tests: the numbers of its variants (enums)
        let mut tests: Vec<Vec<usize>> = Vec::new();
        for arm in arms.iter_mut() {
            let (ks, binds) = self.arm_patterns(&mut m, arm);
            self.freed = before.clone();
            // the names take the values of the variant; the arm sees them as `let`s
            self.scopes.push(HashMap::new());
            for (name, at, ty, _) in &binds {
                self.declare(name, *ty, Decl::Let, *at);
            }
            self.block(&mut arm.body);
            self.scopes.pop();
            let mut lets: Vec<Stmt> = Vec::new();
            for (name, at, ty, slot) in binds {
                if ty.is_unknown() {
                    continue;
                }
                let mut base = Expr::new(ExprKind::Var(tmp.clone()), at);
                base.ty = st;
                let mut value = Expr::new(ExprKind::Field(Box::new(base), slot), at);
                value.ty = ty;
                lets.push(Stmt { kind: StmtKind::Let { name, mutable: false, ty: None, value }, span: at });
            }
            lets.append(&mut arm.body);
            arm.body = lets;
            tests.push(ks);
            let after = std::mem::take(&mut self.freed);
            joined = Some(match joined {
                None => after,
                Some(j) => join(&j, &after),
            });
        }
        self.scopes.pop();
        let exhaustive = self.match_exhaustive(&m, span);
        self.freed = match joined {
            Some(j) if exhaustive => j,
            Some(j) => join(&before, &j),
            None => before,
        };
        // the arms as an `if` chain on the hidden copy of the value; the last arm of an exhaustive
        // match needs no test
        let mut chain: Option<Vec<Stmt>> = None;
        let n = arms.len();
        for (i, arm) in arms.into_iter().enumerate().rev() {
            if arm.wild || (i == n - 1 && exhaustive) {
                chain = Some(arm.body);
                continue;
            }
            let Some(cond) = self.arm_test(&m, &tmp, &tests[i], arm.pats, arm.span) else { continue };
            chain = Some(vec![Stmt { kind: StmtKind::If { cond, then: arm.body, els: chain.take() }, span: arm.span }]);
        }
        let mut out = vec![Stmt { kind: StmtKind::Let { name: tmp.clone(), mutable: false, ty: None, value: scrut }, span }];
        out.extend(chain.unwrap_or_default());
        StmtKind::Arena(out)
    }

    /// `match value { pattern => expr ... }` used as a value: the value of the arm that is chosen.
    /// It becomes `Bind`s and `If`s, so lowering never sees a `match`.
    fn match_expr(&mut self, e: &mut Expr, want: Option<Type>) -> Type {
        let ExprKind::Match(mut scrut, mut arms) = std::mem::replace(&mut e.kind, ExprKind::Int(0)) else {
            unreachable!("only called for a match")
        };
        let span = e.span;
        let st = self.expr(&mut scrut);
        let kind = self.match_kind(st, &scrut);
        let enum_name = st.struct_name().filter(|n| self.enums.contains_key(n));
        let variants: Vec<String> = enum_name.as_ref().map(|n| self.enums[n].variants.clone()).unwrap_or_default();
        let mut m = MatchState { kind, st, enum_name, variants, covered: Vec::new(), wild: false };
        let tmp = self.match_var(st, span);
        // each arm: its test, and its value with the names of its variant bound
        let mut parts: Vec<(Vec<usize>, Expr)> = Vec::new();
        let mut result: Option<Type> = None;
        let mut bad = false;
        for arm in arms.iter_mut() {
            let (ks, binds) = self.arm_patterns(&mut m, arm);
            self.scopes.push(HashMap::new());
            for (name, at, ty, _) in &binds {
                self.declare(name, *ty, Decl::Let, *at);
            }
            let mut body = match arm.body.pop() {
                Some(Stmt { kind: StmtKind::Expr(x), .. }) => x,
                _ => Expr::new(ExprKind::Int(0), arm.span),
            };
            let expect = result.or(want);
            let mut t = self.expr_with(&mut body, expect);
            if let Some(w) = expect.filter(|w| w.is_option()) {
                t = self.coerce(&mut body, w, t);
            }
            self.scopes.pop();
            if t == Type::Void {
                self.errs.push(
                    Diag::new(
                        "E0212",
                        format!("this arm of a `match` used as a value produces no value: {} returns nothing", call_text(&body)),
                        body.span,
                    )
                    .hint("each arm must be an expression with a value, e.g. `Dir.N => 1`; use a `match` statement for actions"),
                );
                bad = true;
            } else if t.is_unknown() {
                bad = true;
            } else {
                match result {
                    None => result = Some(t),
                    Some(r) if r == t => {}
                    Some(r) => {
                        self.errs.push(
                            Diag::new(
                                "E0212",
                                format!("the arms of this `match` have different types: `{}` and `{}`", r.name(), t.name()),
                                body.span,
                            )
                            .hint(format!("every arm must give the same type: change this one to `{}`", r.name())),
                        );
                        bad = true;
                    }
                }
            }
            // the names of the variant, bound around the value
            for (name, at, ty, slot) in binds.into_iter().rev() {
                if ty.is_unknown() {
                    continue;
                }
                let mut base = Expr::new(ExprKind::Var(tmp.clone()), at);
                base.ty = st;
                let mut value = Expr::new(ExprKind::Field(Box::new(base), slot), at);
                value.ty = ty;
                let bt = body.ty;
                let sp = body.span;
                body = Expr::new(ExprKind::Bind(name, Box::new(value), Box::new(body)), sp);
                body.ty = bt;
            }
            parts.push((ks, body));
        }
        self.scopes.pop();
        let exhaustive = self.match_exhaustive(&m, span);
        let Some(ty) = result.filter(|_| exhaustive && !bad) else {
            return Type::Unknown;
        };
        // the arms as an `if` chain; the last arm of an exhaustive match needs no test
        let mut chain: Option<Expr> = None;
        let n = arms.len();
        for (i, (arm, (ks, body))) in arms.into_iter().zip(parts).enumerate().rev() {
            if arm.wild || (i == n - 1 && exhaustive) {
                chain = Some(body);
                continue;
            }
            let Some(cond) = self.arm_test(&m, &tmp, &ks, arm.pats, arm.span) else { continue };
            let Some(els) = chain.take() else { continue };
            let mut branch = Expr::new(ExprKind::If(Box::new(cond), Box::new(body), Box::new(els)), arm.span);
            branch.ty = ty;
            chain = Some(branch);
        }
        let Some(chain) = chain else { return Type::Unknown };
        e.kind = ExprKind::Bind(tmp, scrut, Box::new(chain));
        ty
    }

    /// `a ?? b`: the value of the optional `a`, or `b`.
    fn coalesce(&mut self, a: &mut Expr, b: &mut Expr, span: Span) -> Type {
        let at = self.expr(a);
        if at.is_unknown() {
            self.expr(b);
            return Type::Unknown;
        }
        let Some(inner) = at.option_inner() else {
            self.expr(b);
            let shown = show(a).unwrap_or_else(|| "x".into());
            self.errs.push(
                Diag::new("E0277", format!("`??` needs an optional value on its left, found {}", article(at)), span)
                    .hint(format!("`{shown}` always has a value; `??` is for an optional `T?` such as `m.get(k) ?? 0`")),
            );
            return Type::Unknown;
        };
        let bt = self.expr_with(b, Some(inner));
        if bt.is_unknown() {
            return inner;
        }
        if bt == inner || bt == at {
            return bt;
        }
        let shown = show(b).unwrap_or_else(|| "the default".into());
        self.errs.push(
            Diag::new(
                "E0277",
                format!("the default of `??` must be {} or `{}`, found {}", article(inner), at.name(), article(bt)),
                b.span,
            )
            .hint(format!("`{shown}` does not fit: write a default of type `{}`", inner.name())),
        );
        inner
    }

    /// `{x:spec}`: the specifier must make sense for the type of `x`.
    fn check_spec(&mut self, t: Type, inner: &Expr, spec: &FmtSpec, span: Span) {
        if t.is_unknown() {
            return;
        }
        if t == Type::Void {
            self.errs.push(
                Diag::new("E0203", format!("{} returns nothing, so it cannot be put into a string", call_text(inner)), inner.span)
                    .hint("only values can go inside `{ }`: call it on its own line before the string"),
            );
            return;
        }
        let number = matches!(t, Type::Int | Type::Float);
        let shown = &spec.text;
        let mut bad = |why: String, hint: String| {
            self.errs.push(
                Diag::new("E0271", format!("the format specifier `{shown}` does not fit {}: {why}", article(t)), span).hint(hint),
            );
        };
        if spec.prec.is_some() && t != Type::Float {
            let hint = if t == Type::Int {
                "decimals are for floats: convert with `float(x)`, as in `{float(x):.2}`".to_string()
            } else {
                "only a float has decimals: `{x:.2}`; to cut a text use `s.slice(0, n)`".to_string()
            };
            bad("`.N` rounds a float to N decimals".to_string(), hint);
        } else if (spec.comma || spec.plus || spec.zero) && !number {
            let what = if spec.comma {
                "`,`"
            } else if spec.plus {
                "`+`"
            } else {
                "`0`"
            };
            bad(
                format!("{what} is for numbers"),
                "an `int` or a `float` can have separators, a sign and zeros; align a text with `<`, `>` or `^`".to_string(),
            );
        } else {
            match (spec.ty, t) {
                (Some('f'), Type::Float) | (Some('d'), Type::Int) | (Some('s'), Type::Str) | (None, _) => {}
                (Some('f'), _) => bad("`f` is for floats".to_string(), "write `{x:.2}`, with `x` a float".to_string()),
                (Some('d'), _) => bad("`d` is for ints".to_string(), "write `{n}` or `{n:5}`, with `n` an int".to_string()),
                (Some(_), _) => bad("`s` is for text".to_string(), "write `{x:>8}` without the letter".to_string()),
            }
        }
        if spec.needs_helper() {
            self.need(H::Fmt, span);
        }
    }

    /// `(a, b)`: the tuple type of the types of its values.
    fn tuple_lit(&mut self, items: &mut [Expr], want: Option<Type>) -> Type {
        let want_elems = want.and_then(Type::tuple_elems);
        let mut tys = Vec::new();
        let mut bad = false;
        for (i, it) in items.iter_mut().enumerate() {
            let t = self.expr_with(it, want_elems.as_ref().and_then(|w| w.get(i).copied()));
            if t == Type::Void {
                self.errs.push(
                    Diag::new("E0203", format!("{} returns nothing, so it cannot be an element of a tuple", call_text(it)), it.span)
                        .hint(self.no_value_hint(it)),
                );
                bad = true;
            } else if t.is_unknown() {
                bad = true;
            } else {
                tys.push(t);
            }
        }
        if bad {
            return Type::Unknown;
        }
        let t = Type::tuple(&tys);
        self.register_tuple(t);
        t
    }

    /// `t.0`: the type of a tuple's element. The field is renamed `_0`, the name of the struct field.
    /// A name like `0/2` comes from a pattern `(a, b)`: the 2 must be the tuple's size.
    fn tuple_field(&mut self, bt: Type, name: &mut String, pattern: bool, in_for: bool, span: Span) -> Type {
        let (pos, wanted) = match name.split_once('/') {
            Some((i, n)) => (i.to_string(), n.parse::<usize>().ok()),
            None => (name.trim_start_matches('_').to_string(), None),
        };
        let again = self.errs.iter().any(|d| d.code == "E0272" && d.span == span);
        let Some(elems) = bt.tuple_elems() else {
            if !again {
                self.errs.push(
                    Diag::new("E0272", format!("cannot take apart {}: a pattern like `(a, b)` needs a tuple", article(bt)), span).hint(
                        if in_for {
                            "`for (a, b) in xs` takes each element apart, so the elements must be tuples; for the position too write `for i, x in xs`"
                        } else {
                            "a pattern takes a tuple apart, e.g. the result of `fn f() -> (int, str)`; for a single value write `let a = ...`"
                        },
                    ),
                );
            }
            return Type::Unknown;
        };
        if let (Some(n), true) = (wanted, pattern) {
            if n != elems.len() {
                if again {
                    return Type::Unknown;
                }
                self.errs.push(
                    Diag::new(
                        "E0272",
                        format!(
                            "the pattern has {} but the tuple `{}` has {}",
                            count(n, "name"),
                            bt.name(),
                            count(elems.len(), "element")
                        ),
                        span,
                    )
                    .hint(format!(
                        "write one name for each element, `_` for one you do not need: `({})`",
                        vec!["_"; elems.len()].join(", ")
                    )),
                );
                return Type::Unknown;
            }
        }
        match pos.parse::<usize>() {
            Ok(i) if i < elems.len() => {
                *name = format!("_{i}");
                elems[i]
            }
            Ok(i) => {
                self.errs.push(
                    Diag::new(
                        "E0273",
                        format!("the tuple `{}` has {}, so `.{i}` does not exist", bt.name(), count(elems.len(), "element")),
                        span,
                    )
                    .hint(format!("the positions are `.0` to `.{}`", elems.len() - 1)),
                );
                Type::Unknown
            }
            Err(_) => {
                self.errs.push(
                    Diag::new("E0224", format!("`{}` has no field `{name}`", bt.name()), span)
                        .hint("the elements of a tuple are read by position: `t.0`, `t.1`, or taken apart with `let (a, b) = t`"),
                );
                Type::Unknown
            }
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
            // `[str?]`: the elements are optional, so a plain `"a"` is wrapped and `none` fits
            let hint = if want_elem.is_some_and(Type::is_option) { want_elem } else { first.or(want_elem) };
            let t = self.expr_with(it, hint);
            let t = match hint {
                Some(w) => self.coerce(it, w, t),
                None => t,
            };
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
        if bt.struct_name().is_some_and(|n| self.enums.contains_key(&n)) {
            self.errs.push(
                Diag::new("E0224", format!("`{}` is an enum and has no field `{name}`", bt.name()), span)
                    .hint("compare an enum value with `==`, or take it apart with `match`"),
            );
            return Type::Unknown;
        }
        if bt.is_option() {
            self.errs.push(
                Diag::new("E0224", format!("`{}` is an optional value and has no field `{name}`", bt.name()), span)
                    .hint("take the value out first: `x ?? default`, `if let v = x { v.field }` or `x.unwrap().field`"),
            );
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

    /// `opt.is_some()`, `opt.is_none()`, `opt.unwrap()`.
    fn option_method(&mut self, rt: Type, inner: Type, name: &str, args: &mut [Expr], span: Span) -> Type {
        for a in args.iter_mut() {
            self.arg_type(a, None);
        }
        if !matches!(name, "is_some" | "is_none" | "unwrap") {
            self.errs.push(
                Diag::new("E0227", format!("`{}` has no method `{name}`", rt.name()), span).hint(
                    "an optional value has `is_some()`, `is_none()` and `unwrap()`; to get the value use `x ?? default` or `if let v = x { ... }`",
                ),
            );
            return Type::Unknown;
        }
        if !args.is_empty() {
            self.errs.push(
                Diag::new("E0204", format!("`.{name}()` takes 0 arguments but {} {} given", args.len(), was_were(args.len())), span)
                    .hint(format!("call it as `.{name}()`")),
            );
        }
        if name == "unwrap" {
            inner
        } else {
            Type::Bool
        }
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
        if let Some(inner) = rt.option_inner() {
            return self.option_method(rt, inner, name, args, span);
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
                "sort" | "sorted" => {
                    (!data::sortable(e)).then_some("`[int]`, `[float]`, `[str]`, `[char]` or an array of tuples of those")
                }
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
        // methods that a generated helper function runs (see `helpers.rs`)
        match name {
            "chunks" => self.need(H::Chunks(rt), span),
            "items" if rt.map_kv().is_some() => self.need(H::Items(rt), span),
            "trim" if args.len() == 1 && rt == Type::Str => self.need(H::TrimChars, span),
            "sort" | "sorted" => {
                if let Some(e) = rt.elem().filter(|e| e.is_tuple() && helpers::orderable(*e)) {
                    self.need(H::SortKeyed(rt, e), span);
                }
            }
            _ => {}
        }
        // `s.pad_left(n)` fills with spaces; `s.pad_left(n, '0')` with a character
        let mut sig = sig;
        if rt == Type::Str && matches!(name, "pad_left" | "pad_right") && args.len() == 2 {
            sig.params.push(Type::Char);
        }
        // `s.trim("-_")` cuts the characters of the text from both ends
        if rt == Type::Str && name == "trim" && args.len() == 1 {
            sig.params.push(Type::Str);
        }
        // `m.get(k, default)`
        if let (Some((_, v)), "get", 2) = (rt.map_kv(), name, args.len()) {
            sig.params.push(v);
            sig.ret = v;
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
            let search = rt == Type::Str && (matches!(name, "contains" | "starts_with" | "ends_with" | "index_of") || name == "trim");
            for (i, (a, p)) in args.iter_mut().zip(&sig.params).enumerate() {
                let t = self.expr_with(a, Some(*p));
                let t = self.coerce(a, *p, t);
                if search && t == Type::Char {
                    continue;
                }
                self.expect_ty(*p, t, a, Ctx::MethodArg { m: name, idx: i });
            }
        }
        self.register_in(sig.ret);
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
                // tuples compare element by element, like text
                _ if l == r && l.is_tuple() && helpers::orderable(l) => {
                    self.need(H::Cmp(op.symbol(), l), span);
                    Some(Bool)
                }
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
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                "two `int`s, two `float`s, two `str`s, two `char`s or two tuples of those"
            }
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
        } else if matches!(op, BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge) && l.is_tuple() && l == r {
            "tuples are ordered when every element is a number, text, a char or a bool (or such a tuple): compare the other parts one by one"
                .to_string()
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

    /// `zip(a, b)`: the pairs of elements, as many as the shorter one has.
    fn zip(&mut self, args: &mut [Expr], span: Span) -> Type {
        let tys: Vec<Type> = args.iter_mut().map(|a| self.arg_type(a, None)).collect();
        if !(2..=3).contains(&tys.len()) {
            self.errs.push(
                Diag::new("E0204", format!("`zip` takes 2 or 3 arguments but {} {} given", tys.len(), was_were(tys.len())), span)
                    .hint("call it as `zip(xs, ys)`: it gives an array of pairs `[(x, y)]`"),
            );
            return Type::Unknown;
        }
        if tys.iter().any(|t| t.is_unknown()) {
            return Type::Unknown;
        }
        let mut elems = Vec::new();
        for (a, t) in args.iter().zip(&tys) {
            match t {
                Type::Array(_) => elems.push(t.elem().unwrap_or(Type::Unknown)),
                Type::Str => elems.push(Type::Char),
                _ => {
                    self.errs.push(
                        Diag::new("E0203", format!("`zip` needs arrays or strings, found `{}`", t.name()), a.span)
                            .hint("`zip(xs, ys)` pairs the elements of two arrays (or the characters of strings)"),
                    );
                    return Type::Unknown;
                }
            }
        }
        let pair = Type::tuple(&elems);
        self.register_tuple(pair);
        self.need(H::Zip(tys), span);
        Type::array(pair)
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
        // (the checker itself turns `Dir.N` into a call of `Dir` with the number: those are right)
        if self.enums.contains_key(name) && !matches!(args, [a] if matches!(a.kind, ExprKind::Int(_)) && a.ty == Type::Int) {
            for a in args.iter_mut() {
                self.arg_type(a, None);
            }
            let first = self.enums[name].variants.first().cloned().unwrap_or_else(|| "A".into());
            self.errs.push(
                Diag::new("E0235", format!("`{name}` is an enum: its values are its variants"), span)
                    .hint(format!("write `{name}.{first}`")),
            );
            return Type::Unknown;
        }
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
        if name == "zip" && !self.fns.contains_key(name) {
            return self.zip(args, span);
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
        let mut tys: Vec<Type> = args
            .iter_mut()
            .enumerate()
            .map(|(i, a)| {
                let w = params.get(i).copied();
                self.arg_type(a, w)
            })
            .collect();
        // a `T` goes into a parameter of type `T?`
        for (i, a) in args.iter_mut().enumerate() {
            if let (Some(&p), false) = (params.get(i), matches!(a.kind, ExprKind::Inout(_) | ExprKind::Labeled(..))) {
                tys[i] = self.coerce(a, p, tys[i]);
            }
        }
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
                            let t = self.coerce(v, want, t);
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
