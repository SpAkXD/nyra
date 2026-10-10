//! Examples: `bool` conditions written next to a function with `ex`, which must be true.
//!
//! ```text
//! fn sq(x: int) -> int = x * x   ex sq(3) == 9, sq(-2) == 4
//! ```
//!
//! An example is never compiled into the program. It cannot see variables (nor call a function that
//! uses the script's variables: E0254), so it only holds constants and calls, and the compiler can always run it: every
//! `nyra check`, `run` and `build` evaluates every example, and a false one is a compile error
//! with both values (E0250). `nyra test` reports the same, with how many passed.
//!
//! How: each example becomes a function of its own (`ex#3`; for `a == b`, one function per side,
//! so the error can show both values), which is lowered to the IR together with the program and
//! run by the IR interpreter (`ir::interp`) with a budget of steps and of nested calls. The IR is
//! what every backend compiles, so an example computes exactly what the program would.

use crate::ast;
use crate::ast::{BinOp, Expr, ExprKind, Forall, Func, InterpPart, Param, Program, Span, Stmt, StmtKind, Type, UnOp};
use crate::diag::{json_str, render_json_errors, Diag};
use crate::ir::interp::{self, Interp, Limits, RuntimeError, Stop, Value};
use crate::ir::{self, FuncId};

/// What one example may cost. Examples run while the program compiles, so they must be quick.
pub const LIMITS: Limits = Limits::new(1_000_000, 10_000).memory(256 << 20);

/// A property example (`ex for n in 0..200: ...`) runs up to 100,000 inputs: it gets ten times the steps.
const PROPERTY_LIMITS: Limits = Limits::new(10_000_000, 10_000).memory(256 << 20);

/// The stack of the thread that runs the examples (deep recursion needs room).
const STACK: usize = 512 << 20;

#[derive(Default)]
pub struct Outcome {
    /// Examples in the program.
    pub total: usize,
    pub passed: usize,
    /// One error per example that failed.
    pub errors: Vec<Diag>,
    /// Examples that could not be run: the backends do not support something they use (or a
    /// compiler bug). They are not reported as errors.
    pub skipped: usize,
}

/// What is known about one example before it runs.
struct Plan {
    /// The condition as source text, e.g. `sq(3) == 9`.
    text: String,
    span: Span,
    /// The function to look at when it fails: the first function of the program it calls.
    suspect: Option<String>,
    /// It calls no function of the program: only the example itself can be wrong.
    builtins_only: bool,
    /// For `left op right`: the operator and the text of each side; `None` for any other condition.
    cmp: Option<(BinOp, String, String)>,
    /// The right side is a literal, so its value need not be repeated.
    right_literal: bool,
    /// The left side calls a function of the program: its name, its parameters and the arguments.
    call: Option<(String, Vec<String>, Vec<String>)>,
    /// The index of its (first) function in the module.
    func: usize,
    /// A property example: the variable and the range of its inputs.
    forall: Option<Forall>,
}

enum Res {
    Pass,
    /// The condition is false; for a comparison, the values of both sides as Nyra code.
    False(Option<(String, String)>),
    /// A runtime error, and the function it happened in (`None`: in the example itself).
    Error(RuntimeError, Option<String>),
    Steps,
    Depth,
    Memory,
    Skipped,
}

/// Type-checks nothing: `prog` must have passed `check::check`. Runs every example and gives one
/// error per example that is not true.
pub fn run(prog: &mut Program) -> Outcome {
    let total = prog.examples.len();
    if total == 0 {
        return Outcome::default();
    }
    let base = prog.funcs.len();
    let mut plans = Vec::with_capacity(total);
    // how to put each condition back together: the operator of a split comparison
    let mut split: Vec<Option<(BinOp, Span, Type)>> = Vec::with_capacity(total);
    for (i, ex) in prog.examples.iter_mut().enumerate() {
        let e = std::mem::replace(&mut ex.expr, Expr::new(ExprKind::Bool(true), Span { line: 0, col: 0 }));
        let forall = ex.forall.clone();
        let text = source(&e);
        let span = start(&e);
        let func = prog.funcs.len() - base;
        let call_of = |x: &Expr, funcs: &[Func]| match &x.kind {
            ExprKind::Call(name, args) => funcs[..base]
                .iter()
                .find(|f| f.name == *name)
                .map(|f| (name.clone(), f.params.iter().map(|p| p.name.clone()).collect(), args.iter().map(source).collect())),
            _ => None,
        };
        let user_fns: Vec<&str> = prog.funcs[..base].iter().map(|f| f.name.as_str()).collect();
        let called = first_call(&e, &user_fns);
        let builtins_only = called.is_none();
        let suspect = called;
        let Expr { kind, span: op_span, ty } = e;
        match kind {
            ExprKind::Binary(op, l, r) if matches!(op, BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge) => {
                let right_literal = literal(&r);
                plans.push(Plan {
                    text,
                    span,
                    suspect,
                    builtins_only,
                    cmp: Some((op, source(&l), source(&r))),
                    right_literal,
                    call: call_of(&l, &prog.funcs),
                    func,
                    forall: forall.clone(),
                });
                split.push(Some((op, op_span, ty)));
                let (lt, rt) = (l.ty, r.ty);
                prog.funcs.push(synthetic(format!("ex#{i}.l"), *l, lt, span, &forall));
                prog.funcs.push(synthetic(format!("ex#{i}.r"), *r, rt, span, &forall));
            }
            kind => {
                let e = Expr { kind, span: op_span, ty };
                let call = call_of(&e, &prog.funcs);
                plans.push(Plan {
                    text,
                    span,
                    suspect,
                    builtins_only,
                    cmp: None,
                    right_literal: false,
                    call,
                    func,
                    forall: forall.clone(),
                });
                split.push(None);
                prog.funcs.push(synthetic(format!("ex#{i}"), e, Type::Bool, span, &forall));
            }
        }
    }

    let module = ir::lower::lower(prog);

    // put the conditions back where they were
    let mut synth = prog.funcs.drain(base..).map(|f| match f.body.into_iter().next() {
        Some(Stmt { kind: StmtKind::Ret(Some(e)), .. }) => e,
        _ => unreachable!("an example function returns its condition"),
    });
    for (ex, how) in prog.examples.iter_mut().zip(split) {
        ex.expr = match how {
            Some((op, span, ty)) => {
                let (l, r) = (synth.next().expect("left side"), synth.next().expect("right side"));
                Expr { kind: ExprKind::Binary(op, Box::new(l), Box::new(r)), span, ty }
            }
            None => synth.next().expect("a condition"),
        };
    }
    drop(synth);

    let results = match module {
        Ok(m) => evaluate(&m, &plans, base),
        Err(_) => None,
    };
    let Some(results) = results else {
        return Outcome { total, skipped: total, ..Outcome::default() };
    };
    let mut out = Outcome { total, ..Outcome::default() };
    for (i, (plan, (res, input))) in plans.iter().zip(results).enumerate() {
        match res {
            Res::Pass => out.passed += 1,
            Res::Skipped => out.skipped += 1,
            res => {
                // an example of an imported file is reported with that file
                let mut d = report(plan, res, input);
                d.file = prog.examples.get(i).and_then(|e| e.file.clone());
                out.errors.push(d);
            }
        }
    }
    out
}

/// `fn ex#3() -> T = expr`; for a property example `fn ex#3(n: int) -> T = expr`
fn synthetic(name: String, e: Expr, ret: Type, span: Span, forall: &Option<Forall>) -> Func {
    let s = e.span;
    let params = match forall {
        Some(f) => vec![Param { name: f.var.clone(), ty: Type::Int, inout: false, mutable: false, span: f.span }],
        None => Vec::new(),
    };
    Func { name, params, ret, body: vec![Stmt { kind: StmtKind::Ret(Some(e)), span: s }], span }
}

/// Runs every example, on a thread with a large stack. `None` if that thread cannot run.
#[cfg(not(target_arch = "wasm32"))]
fn evaluate(m: &ir::Module, plans: &[Plan], base: usize) -> Option<Vec<(Res, Option<i64>)>> {
    let types = ast::type_tables();
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("nyra-examples".into())
            .stack_size(STACK)
            .spawn_scoped(scope, || {
                // (array and struct types are numbered per thread: `json.parse` needs them)
                ast::install_type_tables(types);
                plans.iter().map(|p| one(m, p, base)).collect()
            })
            .ok()?
            .join()
            .ok()
    })
}

/// Runs every example. WebAssembly has no threads: they run on the caller's stack, which the
/// build makes large (`tools/build_wasm.py`).
#[cfg(target_arch = "wasm32")]
fn evaluate(m: &ir::Module, plans: &[Plan], base: usize) -> Option<Vec<(Res, Option<i64>)>> {
    Some(plans.iter().map(|p| one(m, p, base)).collect())
}

fn one(m: &ir::Module, plan: &Plan, base: usize) -> (Res, Option<i64>) {
    let id = FuncId((base + plan.func) as u32);
    let Some(f) = &plan.forall else {
        return (check(m, plan, &mut Interp::new(m, LIMITS), id, None), None);
    };
    // a property: every input in turn, the first one that fails is reported
    let mut it = Interp::new(m, PROPERTY_LIMITS);
    for k in 0..f.count() {
        let n = f.value(k);
        match check(m, plan, &mut it, id, Some(n)) {
            Res::Pass => {}
            other => return (other, Some(n)),
        }
    }
    (Res::Pass, None)
}

/// One run of an example (for a property: with one input).
fn check(m: &ir::Module, plan: &Plan, it: &mut Interp, id: FuncId, input: Option<i64>) -> Res {
    let args = || input.map(|n| vec![Value::Int(n)]).unwrap_or_default();
    let run = |it: &mut Interp, id: FuncId| match it.call(id, args()) {
        Ok(Some(v)) => Ok(v),
        Ok(None) => Err(Res::Skipped),
        Err(Stop::Error(e)) => {
            let name = e.func.map(|f| m.func(f).name.clone()).filter(|n| !n.starts_with("ex#"));
            Err(Res::Error(*e, name))
        }
        Err(Stop::Steps) => Err(Res::Steps),
        Err(Stop::Depth) => Err(Res::Depth),
        Err(Stop::Memory) => Err(Res::Memory),
        Err(Stop::Bug(_) | Stop::Output | Stop::Time | Stop::Exit(_)) => Err(Res::Skipped),
    };
    let Some((op, ..)) = &plan.cmp else {
        return match run(it, id) {
            Ok(Value::Bool(true)) => Res::Pass,
            Ok(Value::Bool(false)) => Res::False(None),
            Ok(_) => Res::Skipped,
            Err(r) => r,
        };
    };
    let l = match run(it, id) {
        Ok(v) => v,
        Err(r) => return r,
    };
    let r = match run(it, FuncId(id.0 + 1)) {
        Ok(v) => v,
        Err(r) => return r,
    };
    let ord = interp::compare(&l, &r);
    use std::cmp::Ordering::*;
    let ok = match op {
        BinOp::Eq => interp::equal(&l, &r),
        BinOp::Ne => !interp::equal(&l, &r),
        BinOp::Lt => ord == Some(Less),
        BinOp::Le => matches!(ord, Some(Less | Equal)),
        BinOp::Gt => ord == Some(Greater),
        _ => matches!(ord, Some(Greater | Equal)),
    };
    if ok {
        Res::Pass
    } else {
        Res::False(Some((interp::show(m, &l), interp::show(m, &r))))
    }
}

/// The error for an example that did not pass.
fn report(p: &Plan, res: Res, input: Option<i64>) -> Diag {
    let text = &p.text;
    // a property example says which input failed
    let at = match (&p.forall, input) {
        (Some(f), Some(n)) => format!(" for {} = {n}", f.var),
        _ => String::new(),
    };
    let suspect = p.suspect.clone();
    let named = |what: &str| match &suspect {
        Some(f) => format!("`{f}`"),
        None => what.to_string(),
    };
    match res {
        Res::False(values) => {
            let (msg, actual, expected) = match (&p.cmp, values) {
                (Some((op, l, r)), Some((lv, rv))) => {
                    let msg = match op {
                        BinOp::Eq => format!("example `{text}` is false{at}: `{l}` is {lv}, not {rv}"),
                        _ if p.right_literal => format!("example `{text}` is false{at}: `{l}` is {lv}"),
                        _ => format!("example `{text}` is false{at}: `{l}` is {lv} and `{r}` is {rv}"),
                    };
                    let expected = match op {
                        BinOp::Eq => rv,
                        BinOp::Ne => format!("anything but {rv}"),
                        op => format!("{} {rv}", op.symbol()),
                    };
                    (msg, lv, expected)
                }
                _ => (format!("example `{text}` is false{at}"), "false".to_string(), "true".to_string()),
            };
            let hint = match &p.call {
                Some((f, ..)) if p.forall.is_some() => format!(
                    "if the property is right, `{f}` is wrong{at}: trace it with that value and fix it; change the example only if it expects the wrong thing"
                ),
                Some((f, params, args)) if !params.is_empty() && params.len() == args.len() => {
                    let with: Vec<String> = params.iter().zip(args).map(|(p, a)| format!("{p} = {a}")).collect();
                    format!(
                        "if the example is right, `{f}` is wrong for {}: trace `{f}` with these values and fix it; change the example only if it expects the wrong value",
                        with.join(", ")
                    )
                }
                Some((f, ..)) => format!(
                    "if the example is right, `{f}` is wrong: trace it and fix it; change the example only if it expects the wrong value"
                ),
                None if p.builtins_only => {
                    "the example calls no function of the program, so the example itself expects the wrong value: fix it".to_string()
                }
                None => format!(
                    "if the example is right, {} is wrong for these values: trace it and fix it; change the example only if it expects the wrong value",
                    named("a function it calls")
                ),
            };
            Diag::new("E0250", msg, p.span).hint(hint).values(actual, expected)
        }
        Res::Error(e, func) => {
            let place = match &func {
                // the standard library is not part of the program's file: it has no line to show
                Some(f) if f.contains('.') => format!("`{f}`, a function of the standard library"),
                Some(f) => format!("line {}:{} in `{f}`", e.span.line, e.span.col),
                None => format!("line {}:{}", e.span.line, e.span.col),
            };
            let fix = match func.or(suspect) {
                Some(f) => format!("fix `{f}`, or give it arguments it accepts"),
                None => "change the example so that it runs without errors".to_string(),
            };
            Diag::new("E0251", format!("example `{text}` stops with runtime error {}{at}: {} (at {place})", e.code, e.msg), p.span)
                .hint(format!("{}; an example must run without errors: {fix}", e.hint))
        }
        Res::Steps => Diag::new(
            "E0253",
            format!("example `{text}` did not finish{at} within {} steps", if p.forall.is_some() { PROPERTY_LIMITS.steps } else { LIMITS.steps }),
            p.span,
        )
        .hint(format!(
            "examples run while the program compiles, so they must be quick: look for a loop in {} that never ends (a `while` whose condition never changes), or use smaller inputs",
            named("the functions it calls")
        )),
        Res::Depth => Diag::new(
            "E0253",
            format!("example `{text}` did not finish{at}: more than {} calls were nested", LIMITS.depth),
            p.span,
        )
        .hint(format!(
            "look for a recursive call in {} that never reaches its base case, or use smaller inputs",
            named("the functions it calls")
        )),
        Res::Memory => Diag::new(
            "E0253",
            format!("example `{text}` used more than {} MiB of memory{at}", LIMITS.memory >> 20),
            p.span,
        )
        .hint(format!(
            "examples run while the program compiles, so they must be small: look for a value that grows without end in {}, or use smaller inputs",
            named("the functions it calls")
        )),
        Res::Pass | Res::Skipped => unreachable!("not an error"),
    }
}

/// `nyra test --json`: `{"ok":..,"examples":..,"passed":..,"failed":..,"errors":[..]}`.
pub fn json(out: &Outcome, file: &str) -> String {
    let skipped = if out.skipped > 0 { format!(",\"skipped\":{}", out.skipped) } else { String::new() };
    format!(
        "{{\"ok\":{},\"file\":{},\"examples\":{},\"passed\":{},\"failed\":{}{skipped},\"errors\":{}{}}}",
        out.errors.is_empty(),
        json_str(file),
        out.total,
        out.passed,
        out.errors.len(),
        render_json_errors(&out.errors, file),
        crate::diag::warnings_json(file)
    )
}

/// The summary line of `nyra test`.
pub fn summary(out: &Outcome, file: &str) -> String {
    if out.total == 0 {
        return format!("no examples in {file}: write them after a function, e.g. `fn sq(x: int) -> int = x * x  ex sq(3) == 9`");
    }
    let mut s = format!("{} example{}: {} passed", out.total, if out.total == 1 { "" } else { "s" }, out.passed);
    if !out.errors.is_empty() {
        s += &format!(", {} failed", out.errors.len());
    }
    if out.skipped > 0 {
        s += &format!(", {} not run (not supported by the backends yet)", out.skipped);
    }
    s
}

/// The first function of the program that `e` calls, in the order it runs.
fn first_call(e: &Expr, fns: &[&str]) -> Option<String> {
    let all = |xs: &[Expr]| xs.iter().find_map(|x| first_call(x, fns));
    match &e.kind {
        ExprKind::Call(name, args) => all(args).or_else(|| fns.contains(&name.as_str()).then(|| name.clone())),
        ExprKind::Unary(_, x) | ExprKind::Field(x, _) | ExprKind::Labeled(_, x) | ExprKind::Inout(x) | ExprKind::Fmt(x, _) => {
            first_call(x, fns)
        }
        ExprKind::Binary(_, a, b) | ExprKind::Index(a, b) | ExprKind::In(a, b) | ExprKind::Coalesce(a, b) => {
            first_call(a, fns).or_else(|| first_call(b, fns))
        }
        ExprKind::Some(x) => first_call(x, fns),
        ExprKind::None => None,
        ExprKind::Slice(b, lo, hi) => first_call(b, fns)
            .or_else(|| lo.as_ref().and_then(|x| first_call(x, fns)))
            .or_else(|| hi.as_ref().and_then(|x| first_call(x, fns))),
        ExprKind::If(c, a, b) => first_call(c, fns).or_else(|| first_call(a, fns)).or_else(|| first_call(b, fns)),
        ExprKind::Bind(_, v, body) => first_call(v, fns).or_else(|| first_call(body, fns)),
        ExprKind::Match(scrut, arms) => first_call(scrut, fns).or_else(|| {
            arms.iter().find_map(|a| {
                a.body.iter().find_map(|s| match &s.kind {
                    StmtKind::Expr(x) => first_call(x, fns),
                    _ => None,
                })
            })
        }),
        ExprKind::Method(r, _, args) => first_call(r, fns).or_else(|| all(args)),
        ExprKind::Array(xs) | ExprKind::Tuple(xs) => all(xs),
        ExprKind::MapLit(kvs) => kvs.iter().find_map(|(k, v)| first_call(k, fns).or_else(|| first_call(v, fns))),
        ExprKind::Interp(parts) => parts.iter().find_map(|p| match p {
            InterpPart::Expr(x) => first_call(x, fns),
            InterpPart::Lit(_) => None,
        }),
        ExprKind::Lambda(_, body) => first_call(body, fns),
        ExprKind::Comprehension(c) => {
            let src = match &c.src {
                ast::CompSrc::Each(x) => first_call(x, fns),
                ast::CompSrc::Range(a, b, k) => {
                    first_call(a, fns).or_else(|| first_call(b, fns)).or_else(|| k.as_ref().and_then(|k| first_call(k, fns)))
                }
            };
            src.or_else(|| first_call(&c.elem, fns)).or_else(|| c.cond.as_ref().and_then(|x| first_call(x, fns)))
        }
        ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Bool(_) | ExprKind::Str(_) | ExprKind::Char(_) | ExprKind::Var(_) => None,
    }
}

/// A literal, also a negative number: its value is what it says.
fn literal(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Bool(_) | ExprKind::Str(_) | ExprKind::Char(_) => true,
        ExprKind::Unary(UnOp::Neg, x) => matches!(x.kind, ExprKind::Int(_) | ExprKind::Float(_)),
        _ => false,
    }
}

/// Where an expression starts: the span of `a + b` is the `+`, of `xs.len()` the `len`.
fn start(e: &Expr) -> Span {
    match &e.kind {
        ExprKind::Binary(_, l, _) => start(l),
        ExprKind::Method(r, ..) | ExprKind::Field(r, _) | ExprKind::Index(r, _) => start(r),
        _ => e.span,
    }
}

fn prec(op: BinOp) -> u8 {
    match op {
        BinOp::Or => 1,
        BinOp::And => 2,
        BinOp::Eq | BinOp::Ne => 3,
        BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => 4,
        BinOp::Add | BinOp::Sub => 5,
        BinOp::Mul | BinOp::Div | BinOp::Mod => 6,
    }
}

/// An expression as Nyra source, with the parentheses it needs.
pub fn source(e: &Expr) -> String {
    let list = |xs: &[Expr]| xs.iter().map(source).collect::<Vec<_>>().join(", ");
    // an operand of `.`, `[ ]` or a unary operator
    let tight = |x: &Expr| match x.kind {
        ExprKind::Binary(..) | ExprKind::Unary(..) | ExprKind::If(..) => format!("({})", source(x)),
        _ => source(x),
    };
    match &e.kind {
        ExprKind::Int(n) => n.to_string(),
        ExprKind::Float(x) => {
            let t = format!("{x:?}");
            if t.contains('.') || t.contains('e') {
                t
            } else {
                format!("{t}.0")
            }
        }
        ExprKind::Bool(b) => b.to_string(),
        ExprKind::Str(s) => quote(s),
        ExprKind::Char(c) => {
            let ch = char::from_u32(*c).unwrap_or('?');
            match ch {
                '\'' => "'\\''".into(),
                '\\' => "'\\\\'".into(),
                '\n' => "'\\n'".into(),
                '\t' => "'\\t'".into(),
                '\r' => "'\\r'".into(),
                ch => format!("'{ch}'"),
            }
        }
        ExprKind::Interp(parts) => {
            let mut s = String::from("\"");
            for p in parts {
                match p {
                    InterpPart::Lit(t) => {
                        let q = quote(t);
                        s += &q[1..q.len() - 1];
                    }
                    InterpPart::Expr(x) => s += &format!("{{{}}}", source(x)),
                }
            }
            s + "\""
        }
        ExprKind::Var(n) => n.clone(),
        ExprKind::Unary(op, x) => format!("{}{}", if *op == UnOp::Neg { "-" } else { "!" }, tight(x)),
        ExprKind::Binary(op, l, r) => {
            let side = |x: &Expr, right: bool| match &x.kind {
                ExprKind::Binary(o, ..) if prec(*o) < prec(*op) || (right && prec(*o) == prec(*op)) => format!("({})", source(x)),
                ExprKind::If(..) => format!("({})", source(x)),
                _ => source(x),
            };
            format!("{} {} {}", side(l, false), op.symbol(), side(r, true))
        }
        ExprKind::Call(n, args) => format!("{n}({})", list(args)),
        ExprKind::If(c, a, b) => format!("if {} {{ {} }} else {{ {} }}", source(c), source(a), source(b)),
        ExprKind::Bind(_, _, body) => source(body),
        ExprKind::Match(scrut, _) => format!("match {} {{ ... }}", source(scrut)),
        ExprKind::Array(xs) => format!("[{}]", list(xs)),
        ExprKind::Tuple(xs) => format!("({})", list(xs)),
        ExprKind::Fmt(x, spec) => format!("{}:{}", source(x), spec.text),
        ExprKind::MapLit(kvs) if kvs.is_empty() => "[:]".to_string(),
        ExprKind::MapLit(kvs) => {
            let items: Vec<String> = kvs.iter().map(|(k, v)| format!("{}: {}", source(k), source(v))).collect();
            format!("[{}]", items.join(", "))
        }
        ExprKind::Index(b, i) => format!("{}[{}]", tight(b), source(i)),
        ExprKind::In(a, b) => format!("{} in {}", tight(a), tight(b)),
        ExprKind::None => "none".to_string(),
        ExprKind::Some(x) => source(x),
        ExprKind::Coalesce(a, b) => format!("{} ?? {}", tight(a), tight(b)),
        ExprKind::Slice(b, lo, hi) => {
            let bound = |x: &Option<Box<Expr>>| x.as_ref().map(|x| source(x)).unwrap_or_default();
            format!("{}[{}..{}]", tight(b), bound(lo), bound(hi))
        }
        ExprKind::Field(b, f) => format!("{}.{}", tight(b), f.strip_prefix('_').filter(|n| n.parse::<usize>().is_ok()).unwrap_or(f)),
        ExprKind::Method(r, m, args) => format!("{}.{m}({})", tight(r), list(args)),
        ExprKind::Labeled(l, v) => format!("{l}: {}", source(v)),
        ExprKind::Inout(v) => format!("inout {}", source(v)),
        ExprKind::Lambda(ps, body) => {
            let names: Vec<&str> = ps.iter().map(|(n, _)| n.as_str()).collect();
            if names.len() == 1 {
                format!("{} => {}", names[0], source(body))
            } else {
                format!("({}) => {}", names.join(", "), source(body))
            }
        }
        ExprKind::Comprehension(c) => {
            let src = match &c.src {
                ast::CompSrc::Each(x) => source(x),
                ast::CompSrc::Range(a, b, k) => match k {
                    Some(k) => format!("{}..{} step {}", source(a), source(b), source(k)),
                    None => format!("{}..{}", source(a), source(b)),
                },
            };
            let cond = c.cond.as_ref().map(|x| format!(" if {}", source(x))).unwrap_or_default();
            format!("[{} for {} in {src}{cond}]", source(&c.elem), c.var[0].0)
        }
    }
}

/// A string literal: quotes, escapes, and `{` `}` doubled.
fn quote(s: &str) -> String {
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
    out
}
