//! Contracts: `requires` and `ensures` lines on a function.
//!
//! ```text
//! fn withdraw(balance: int, amount: int) -> int
//!     requires amount > 0, amount <= balance
//!     ensures result >= 0, result == balance - amount
//! = balance - amount
//! ```
//!
//! `requires` states what the arguments must be, `ensures` what the function guarantees about its
//! result (`result`) and its `inout` parameters (`old(x)` is the value `x` had when the call began).
//! The lines go between the signature and `= expr`, or first in a `{ }` body.
//!
//! - **Run time.** Lowering (`expand`) turns a function with a contract into a wrapper that checks
//!   the `requires` conditions, calls the original body (a function of its own, `name__nybody`),
//!   checks the `ensures` conditions and returns the result. A broken condition stops the program
//!   with runtime error E0420 (`requires`: the caller is wrong) or E0421 (`ensures`: the function
//!   is wrong), the call with its argument values and the condition. Every backend runs the same
//!   IR, so every backend checks, with the same message.
//! - **Compile time.** `search` looks for a counterexample: it builds inputs for each function
//!   (boundary values, the literals of the function and its contract, the arguments of its `ex`
//!   lines, then seeded random values), keeps those that satisfy `requires` (a separate function
//!   `pre#name` computes that), and calls the function in the IR interpreter with a budget of
//!   steps. The first input that breaks an `ensures` (E0421) or stops the function with a runtime
//!   error (E0422: an overflow, an index out of bounds, ...) is shrunk to a small one and reported
//!   as a compile error. The search is deterministic: the same program always gives the same
//!   counterexample. Functions that read files, input, the clock, randomness or arguments, or
//!   script variables, are not searched (their contracts are still checked at run time).

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::ast::{self, Cond, Expr, ExprKind, Func, Globals, InterpPart, Param, Program, Span, Stmt, StmtKind, Type, UnOp};
use crate::diag::Diag;
use crate::ir::interp::{self, Interp, Limits, RuntimeError, Stop, Value};
use crate::ir::{self, FuncId, Module, RtOp, Ty};

/// The return value, in `ensures`.
pub const RESULT: &str = "result";

/// The call that stops the program when a condition is broken: `contract#fail(kind, message)`,
/// kind 0 for `requires` (E0420), 1 for `ensures` (E0421). Only lowering makes it.
pub const FAIL: &str = "contract#fail";

/// The suffix of the function that holds the original body of a function with a contract.
const BODY: &str = "__nybody";

/// Inputs tried per function by default (`NYRA_CONTRACT_TRIES` changes it; 0 turns the search off).
pub const DEFAULT_TRIES: usize = 300;

/// The steps the search may spend on one function, and on shrinking its counterexample.
const STEPS: u64 = 3_000_000;
const SHRINK_STEPS: u64 = 1_000_000;
/// Candidate inputs tried while shrinking one counterexample.
const SHRINK_RUNS: usize = 2_000;

/// The hidden variable that holds `old(x)` (the value of `inout` parameter `x` at the call).
pub fn old_name(param: &str) -> String {
    format!("\u{b7}old\u{b7}{param}")
}

/// The name a function shows in messages: the body of `f` is `f`, the program's own `main` is `main`.
pub fn display_name(name: &str) -> &str {
    let n = name.find(BODY).map_or(name, |i| &name[..i]);
    if n == ast::USER_MAIN {
        "main"
    } else {
        n
    }
}

/// How many inputs the search tries per function.
pub fn tries() -> usize {
    std::env::var("NYRA_CONTRACT_TRIES").ok().and_then(|v| v.trim().parse().ok()).unwrap_or(DEFAULT_TRIES)
}

// ---- checking ----------------------------------------------------------------------------------------

/// Before the checker types the conditions of `f`: errors for `result` and `old(...)` where they
/// mean nothing (E0423), and `old(x)` of an `inout` parameter becomes its hidden variable.
pub fn prepare(f: &mut Func) -> Vec<Diag> {
    let mut errs = Vec::new();
    let fname = display_name(&f.name).to_string();
    let result_param = f.params.iter().find(|p| p.name == RESULT).map(|p| p.span);
    if let (Some(at), false) = (result_param, f.contract.ensures.is_empty()) {
        errs.push(
            Diag::new(
                "E0423",
                format!("`{fname}` has a parameter named `result`, but in `ensures` `result` is the value it returns"),
                at,
            )
            .hint("rename the parameter, e.g. `res`, so that `ensures` can talk about the return value"),
        );
    }
    // (a condition with such a mistake is not checked further: its other errors would only repeat it)
    f.contract.requires.retain_mut(|c| {
        let before = errs.len();
        c.expr.each_mut(&mut |e| match &e.kind {
            ExprKind::Var(n) if n == RESULT && result_param.is_none() => errs.push(
                Diag::new("E0423", format!("`result` in `requires` of `{fname}`: there is no result yet"), e.span)
                    .hint("`requires` is checked before the call has a result: move this condition to an `ensures` line"),
            ),
            ExprKind::Call(n, _) if n == "old" => errs.push(
                Diag::new("E0423", "`old(...)` in `requires`: it is only for `ensures`", e.span)
                    .hint("in `requires` a parameter still has the value it got at the call: write `x` instead of `old(x)`"),
            ),
            _ => {}
        });
        errs.len() == before
    });
    let void = f.ret == Type::Void;
    let inout: Vec<String> = f.params.iter().filter(|p| p.inout).map(|p| p.name.clone()).collect();
    let names: Vec<String> = f.params.iter().map(|p| p.name.clone()).collect();
    f.contract.ensures.retain_mut(|c| {
        let before = errs.len();
        c.expr.each_mut(&mut |e| {
            let replace = match &e.kind {
                ExprKind::Var(n) if n == RESULT && void && result_param.is_none() => {
                    errs.push(Diag::new("E0423", format!("`result` in `ensures` of `{fname}`, which returns nothing"), e.span).hint(
                        format!(
                            "declare what `{fname}` returns (`-> int`), or state what it does to an `inout` parameter with `old(xs)`"
                        ),
                    ));
                    None
                }
                ExprKind::Call(n, args) if n == "old" => match args.as_slice() {
                    [Expr { kind: ExprKind::Var(x), .. }] if inout.contains(x) => Some(old_name(x)),
                    [Expr { kind: ExprKind::Var(x), .. }] if names.contains(x) => {
                        errs.push(Diag::new("E0423", format!("`old({x})`: `{x}` is not an `inout` parameter"), e.span).hint(format!(
                            "only an `inout` parameter can change during the call; `{x}` keeps the value it got, so write `{x}`"
                        )));
                        None
                    }
                    _ => {
                        errs.push(
                            Diag::new("E0423", "`old(...)` takes the name of an `inout` parameter", e.span)
                                .hint("write `old(xs)` for an `inout` parameter `xs`: the value it had when the call started"),
                        );
                        None
                    }
                },
                _ => None,
            };
            if let Some(name) = replace {
                e.kind = ExprKind::Var(name);
            }
        });
        errs.len() == before
    });
    errs
}

// ---- run time: the wrapper ---------------------------------------------------------------------------

/// What lowering compiles in place of the functions with a contract.
#[derive(Default)]
pub struct Expanded {
    /// By index into `prog.funcs`: the wrapper that checks the contract, under the function's name.
    pub wrappers: HashMap<usize, Func>,
    /// The original bodies, as functions of their own (lowered after the program's functions).
    pub bodies: Vec<Func>,
    /// The program's script-variable uses, with those of the bodies (the same as their function's).
    pub globals: Option<Globals>,
}

/// Splits every function with a contract into a wrapper and its body (see the module comment).
pub fn expand(prog: &Program) -> Expanded {
    let mut out = Expanded::default();
    if prog.funcs.iter().all(|f| f.contract.is_empty()) {
        return out;
    }
    let taken: HashSet<&str> = prog.funcs.iter().map(|f| f.name.as_str()).collect();
    let mut uses = prog.globals.uses.clone();
    for (i, f) in prog.funcs.iter().enumerate() {
        if f.contract.is_empty() {
            continue;
        }
        let mut name = format!("{}{BODY}", f.name);
        while taken.contains(name.as_str()) {
            name.push('_');
        }
        if let Some(u) = prog.globals.uses.get(&f.name) {
            uses.insert(name.clone(), u.clone());
        }
        out.wrappers.insert(i, wrapper(f, &name));
        let mut body = f.clone();
        body.name = name;
        body.contract = Default::default();
        out.bodies.push(body);
    }
    out.globals = Some(Globals { vars: prog.globals.vars.clone(), uses });
    out
}

fn typed(kind: ExprKind, ty: Type, span: Span) -> Expr {
    Expr { kind, span, ty }
}

fn var(name: &str, ty: Type, span: Span) -> Expr {
    typed(ExprKind::Var(name.to_string()), ty, span)
}

/// A value in a message: text and characters in quotes, as in the program.
fn shown(parts: &mut Vec<InterpPart>, v: Expr) {
    let quote = match v.ty {
        Type::Str => Some("\""),
        Type::Char => Some("'"),
        _ => None,
    };
    match quote {
        Some(q) => {
            parts.push(InterpPart::Lit(q.to_string()));
            parts.push(InterpPart::Expr(v));
            parts.push(InterpPart::Lit(q.to_string()));
        }
        None => parts.push(InterpPart::Expr(v)),
    }
}

/// True if `name` is read in one of the conditions.
fn reads(conds: &[Cond], name: &str) -> bool {
    let mut found = false;
    for c in conds {
        c.expr.clone().each_mut(&mut |e| found |= matches!(&e.kind, ExprKind::Var(n) if n == name));
    }
    found
}

/// `fn f(params) -> T { requires checks; let old·x = x; let result = body(params); ensures checks; return result }`
fn wrapper(f: &Func, body: &str) -> Func {
    let span = f.span;
    let name = display_name(&f.name);
    // the call as text: `withdraw(5, 3)`
    let mut call = vec![InterpPart::Lit(format!("{name}("))];
    for (k, p) in f.params.iter().enumerate() {
        if k > 0 {
            call.push(InterpPart::Lit(", ".into()));
        }
        shown(&mut call, var(&p.name, p.ty, span));
    }
    call.push(InterpPart::Lit(")".into()));
    let check = |c: &Cond, kind: i64, parts: Vec<InterpPart>| {
        let at = c.span;
        let args = vec![typed(ExprKind::Int(kind), Type::Int, at), typed(ExprKind::Interp(parts), Type::Str, at)];
        let fail = typed(ExprKind::Call(FAIL.into(), args), Type::Void, at);
        let cond = typed(ExprKind::Unary(UnOp::Not, Box::new(c.expr.clone())), Type::Bool, at);
        Stmt { kind: StmtKind::If { cond, then: vec![Stmt { kind: StmtKind::Expr(fail), span: at }], els: None }, span: at }
    };
    let mut stmts = Vec::new();
    for c in &f.contract.requires {
        let mut parts = call.clone();
        parts.push(InterpPart::Lit(format!(" breaks requires {}", c.text)));
        stmts.push(check(c, 0, parts));
    }
    for p in f.params.iter().filter(|p| p.inout) {
        let old = old_name(&p.name);
        if reads(&f.contract.ensures, &old) {
            let value = var(&p.name, p.ty, span);
            stmts.push(Stmt { kind: StmtKind::Let { name: old, mutable: false, ty: Some(p.ty), value }, span });
        }
    }
    let args = f
        .params
        .iter()
        .map(|p| {
            let v = var(&p.name, p.ty, span);
            if p.inout {
                typed(ExprKind::Inout(Box::new(v)), p.ty, span)
            } else {
                v
            }
        })
        .collect();
    let run = typed(ExprKind::Call(body.to_string(), args), f.ret, span);
    let void = f.ret == Type::Void;
    if void {
        stmts.push(Stmt { kind: StmtKind::Expr(run), span });
    } else {
        stmts.push(Stmt { kind: StmtKind::Let { name: RESULT.into(), mutable: false, ty: Some(f.ret), value: run }, span });
    }
    // after the call an `inout` argument has changed: the call shows the value it had (when
    // `old(x)` kept it) or its name, and the message ends with its value now
    let saved = |p: &Param| reads(&f.contract.ensures, &old_name(&p.name));
    let mut after = vec![InterpPart::Lit(format!("{name}("))];
    for (k, p) in f.params.iter().enumerate() {
        if k > 0 {
            after.push(InterpPart::Lit(", ".into()));
        }
        match (p.inout, saved(p)) {
            (false, _) => shown(&mut after, var(&p.name, p.ty, span)),
            (true, true) => shown(&mut after, var(&old_name(&p.name), p.ty, span)),
            (true, false) => after.push(InterpPart::Lit(format!("inout {}", p.name))),
        }
    }
    after.push(InterpPart::Lit(")".into()));
    for c in &f.contract.ensures {
        let mut parts = after.clone();
        parts.push(InterpPart::Lit(format!(" breaks ensures {}", c.text)));
        let mut sep = ": ";
        if !void {
            parts.push(InterpPart::Lit(format!("{sep}result is ")));
            shown(&mut parts, var(RESULT, f.ret, span));
            sep = ", ";
        }
        for p in f.params.iter().filter(|p| p.inout) {
            parts.push(InterpPart::Lit(format!("{sep}{} is ", p.name)));
            shown(&mut parts, var(&p.name, p.ty, span));
            sep = ", ";
        }
        stmts.push(check(c, 1, parts));
    }
    if !void {
        stmts.push(Stmt { kind: StmtKind::Ret(Some(var(RESULT, f.ret, span))), span });
    }
    let params = f.params.iter().map(|p| Param { mutable: false, default: None, ..p.clone() }).collect();
    Func { name: f.name.clone(), params, ret: f.ret, body: stmts, span, contract: Default::default() }
}

// ---- compile time: the search for a counterexample ---------------------------------------------------

/// A literal argument of an `ex` line.
#[derive(Clone, Debug)]
enum Lit {
    Int(i64),
    Float(f64),
    Bool(bool),
    Char(u32),
    Str(String),
}

/// A function whose contract `search` tries to break.
pub struct Target {
    /// Its name in the program.
    name: String,
    /// Its index in the module (its wrapper), and the index of `pre#name`, which tells whether
    /// arguments satisfy its `requires`.
    func: usize,
    pre: usize,
    span: Span,
    /// The literal arguments its `ex` lines call it with, by parameter.
    examples: Vec<Vec<Lit>>,
}

/// The functions whose contracts are searched; adds a function `pre#name` per function to
/// `prog.funcs` (the caller lowers the program and removes them again).
pub fn targets(prog: &mut Program) -> Vec<Target> {
    if tries() == 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    for i in 0..prog.funcs.len() {
        let f = &prog.funcs[i];
        if f.contract.is_empty() {
            continue;
        }
        // a function that uses script variables cannot be called on its own (as in an example, E0254),
        // and a function of a bundled standard module is not the program's
        let globals = prog.globals.uses.get(&f.name).is_some_and(|u| !u.is_empty());
        let bundled = f.name.contains('.') && !prog.files.contains_key(&f.name);
        if globals || bundled || f.params.iter().any(|p| p.ty.is_unknown()) {
            continue;
        }
        let examples = example_args(&prog.examples, &f.name, f.params.len());
        let span = f.span;
        let cond = f.contract.requires.iter().map(|c| c.expr.clone()).reduce(|a, b| {
            let at = a.span;
            typed(ExprKind::Binary(ast::BinOp::And, Box::new(a), Box::new(b)), Type::Bool, at)
        });
        let cond = cond.unwrap_or_else(|| typed(ExprKind::Bool(true), Type::Bool, span));
        let params = f.params.iter().map(|p| Param { inout: false, mutable: false, default: None, ..p.clone() }).collect();
        let pre = Func {
            name: format!("pre#{}", f.name),
            params,
            ret: Type::Bool,
            body: vec![Stmt { kind: StmtKind::Ret(Some(cond)), span }],
            span,
            contract: Default::default(),
        };
        out.push(Target { name: f.name.clone(), func: i, pre: prog.funcs.len(), span, examples });
        prog.funcs.push(pre);
    }
    out
}

/// The literal arguments of the calls of `name` in the examples, by parameter (for a property
/// example `ex for n in a..b: f(n)`, the first and last `n`).
fn example_args(examples: &[ast::Example], name: &str, params: usize) -> Vec<Vec<Lit>> {
    let mut out = vec![Vec::new(); params];
    for ex in examples {
        let mut e = ex.expr.clone();
        e.each_mut(&mut |x| {
            let ExprKind::Call(n, args) = &x.kind else { return };
            if n != name || args.len() != params {
                return;
            }
            for (k, a) in args.iter().enumerate() {
                let lit = match &a.kind {
                    ExprKind::Int(n) => Some(Lit::Int(*n)),
                    ExprKind::Unary(UnOp::Neg, inner) => match inner.kind {
                        ExprKind::Int(n) => Some(Lit::Int(n.wrapping_neg())),
                        ExprKind::Float(f) => Some(Lit::Float(-f)),
                        _ => None,
                    },
                    ExprKind::Float(f) => Some(Lit::Float(*f)),
                    ExprKind::Bool(b) => Some(Lit::Bool(*b)),
                    ExprKind::Char(c) => Some(Lit::Char(*c)),
                    ExprKind::Str(s) => Some(Lit::Str(s.clone())),
                    ExprKind::Var(v) => match &ex.forall {
                        Some(fa) if fa.var == *v && fa.count() > 0 => {
                            out[k].push(Lit::Int(fa.value(fa.count() - 1)));
                            Some(Lit::Int(fa.lo))
                        }
                        _ => None,
                    },
                    _ => None,
                };
                out[k].extend(lit);
            }
        });
    }
    out
}

/// Searches every target for a counterexample: one error per function whose contract breaks.
/// `files`: the imported file of each function that comes from one.
pub fn search(m: &Module, targets: &[Target], files: &HashMap<String, String>) -> Vec<Diag> {
    let tries = tries();
    let mut out = Vec::new();
    for t in targets {
        if let Some(mut d) = Search::new(m, t).run(tries) {
            if d.file.is_none() {
                d.file = files.get(&t.name).cloned();
            }
            out.push(d);
        }
    }
    out
}

/// A small, fast, seeded generator (SplitMix64): the same seed gives the same inputs everywhere.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number below `n` (`n` > 0).
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.below(xs.len() as u64) as usize]
    }
}

/// What one call with one input showed.
enum Attempt {
    /// The input does not satisfy `requires` (or `requires` itself cannot be computed for it).
    Rejected,
    Passed,
    /// A counterexample: the function broke `ensures` or stopped with a runtime error.
    Failed(RuntimeError),
    /// The budget of steps (or memory) is used up.
    Exhausted,
    /// Something compile time cannot run (a file, the clock, ...): the function is not searched.
    Unsupported,
}

/// The constants of the functions a target reaches: inputs near them are likely to matter.
#[derive(Default)]
struct Lits {
    ints: Vec<i64>,
    floats: Vec<f64>,
    chars: Vec<char>,
    strs: Vec<String>,
}

/// Up to this many constants of each kind.
const MAX_LITS: usize = 24;

fn push_unique<T: PartialEq>(v: &mut Vec<T>, x: T) {
    if v.len() < MAX_LITS && !v.contains(&x) {
        v.push(x);
    }
}

struct Search<'m> {
    m: &'m Module,
    t: &'m Target,
    rng: Rng,
    lits: Lits,
    /// Values tried first, per parameter.
    pools: Vec<Vec<Value>>,
    tys: Vec<Ty>,
}

/// The standard modules whose functions compile time does not call: their results depend on the world.
const WORLD: [&str; 5] = ["fs", "os", "input", "time", "random"];

impl<'m> Search<'m> {
    fn new(m: &'m Module, t: &'m Target) -> Self {
        let seed = t.name.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3));
        Search { m, t, rng: Rng(seed), lits: Lits::default(), pools: Vec::new(), tys: Vec::new() }
    }

    fn run(mut self, tries: usize) -> Option<Diag> {
        let (fid, pre) = (FuncId(self.t.func as u32), FuncId(self.t.pre as u32));
        let f = self.m.funcs.get(fid.0 as usize)?;
        self.tys = f.locals[..f.params].iter().map(|l| l.ty).collect();
        if !self.tys.iter().all(|t| self.supported(*t, 0)) {
            return None;
        }
        let reach = reachable(self.m, &[fid, pre]);
        if reach.iter().any(|&g| touches_world(&self.m.funcs[g].body)) {
            return None;
        }
        for &g in &reach {
            literals(self.m, &self.m.funcs[g].body, &mut self.lits);
        }
        self.pools = (0..self.tys.len()).map(|k| self.pool(k)).collect();
        let mut it = Interp::new(self.m, Limits::new(STEPS, 10_000).memory(256 << 20));
        let widest = self.pools.iter().map(Vec::len).max().unwrap_or(0);
        let mut found = None;
        for i in 0..tries {
            let args = self.input(i, widest);
            match attempt(&mut it, fid, pre, &args) {
                Attempt::Rejected | Attempt::Passed => {}
                Attempt::Failed(e) => {
                    found = Some((args, e));
                    break;
                }
                Attempt::Exhausted => break,
                Attempt::Unsupported => return None,
            }
        }
        let (args, e) = found?;
        let (args, e) = self.shrink(fid, pre, args, e);
        Some(self.report(&args, e))
    }

    /// The `i`-th input: the pools side by side first (`f(0, 0)`, `f(1, 1)`, ...), then random
    /// mixtures of pool values, copies of other arguments and new random values.
    fn input(&mut self, i: usize, widest: usize) -> Vec<Value> {
        let n = self.tys.len();
        if i < widest {
            return (0..n).map(|k| self.pools[k][i % self.pools[k].len()].clone()).collect();
        }
        let mut args: Vec<Value> = Vec::with_capacity(n);
        for k in 0..n {
            let same: Vec<usize> = (0..k).filter(|&j| self.tys[j] == self.tys[k]).collect();
            let v = match self.rng.below(20) {
                0..=8 => self.rng.pick(&self.pools[k]).clone(),
                9..=11 if !same.is_empty() => {
                    let j = *self.rng.pick(&same);
                    args[j].clone()
                }
                _ => self.random(self.tys[k], 0),
            };
            args.push(v);
        }
        args
    }

    /// True for a type whose values the search can make.
    fn supported(&self, t: Ty, depth: usize) -> bool {
        if depth > 8 {
            return true;
        }
        match t {
            Ty::Int | Ty::Float | Ty::Bool | Ty::Char | Ty::Str => true,
            Ty::Array(_) => t.elem().is_some_and(|e| self.supported(e, depth + 1)),
            Ty::Map(_) => t.map_kv().is_some_and(|(k, v)| self.supported(k, depth + 1) && self.supported(v, depth + 1)),
            Ty::Struct(_) => self.m.structs.get(t).is_some_and(|s| s.fields.iter().all(|(_, ft)| self.supported(*ft, depth + 1))),
            _ => false,
        }
    }

    fn int_pool(&self) -> Vec<i64> {
        let mut v: Vec<i64> = Vec::new();
        for &n in &self.lits.ints {
            for x in [Some(n), n.checked_add(1), n.checked_sub(1)].into_iter().flatten() {
                if !v.contains(&x) {
                    v.push(x);
                }
            }
        }
        let fixed = [
            0,
            1,
            -1,
            2,
            -2,
            3,
            10,
            -10,
            100,
            -100,
            1000,
            255,
            65_536,
            i32::MAX as i64,
            i32::MIN as i64,
            ir::eval::SAFE_INT,
            -ir::eval::SAFE_INT,
            i64::MAX,
            i64::MIN,
            i64::MAX - 1,
            i64::MIN + 1,
        ];
        for x in fixed {
            if !v.contains(&x) {
                v.push(x);
            }
        }
        v
    }

    /// The values tried first for parameter `k`: those of its `ex` lines, then values of its type
    /// that often matter.
    fn pool(&mut self, k: usize) -> Vec<Value> {
        let t = self.tys[k];
        let mut v: Vec<Value> = Vec::new();
        for lit in self.t.examples[k].clone() {
            let x = match (lit, t) {
                (Lit::Int(n), Ty::Int) => Value::Int(n),
                (Lit::Int(n), Ty::Float) => Value::Float(n as f64),
                (Lit::Float(f), Ty::Float) => Value::Float(f),
                (Lit::Bool(b), Ty::Bool) => Value::Bool(b),
                (Lit::Char(c), Ty::Char) => match char::from_u32(c) {
                    Some(c) => Value::Char(c),
                    None => continue,
                },
                (Lit::Str(s), Ty::Str) => Value::Str(Rc::new(s)),
                _ => continue,
            };
            v.push(x);
        }
        v.extend(self.values(t, 0));
        let mut unique: Vec<Value> = Vec::new();
        for x in v {
            if !unique.iter().any(|u| same_value(u, &x)) {
                unique.push(x);
            }
        }
        unique
    }

    /// A few values of type `t` that often matter: boundaries, the program's constants, empty and
    /// short arrays and strings.
    fn values(&mut self, t: Ty, depth: usize) -> Vec<Value> {
        match t {
            Ty::Int => self.int_pool().into_iter().map(Value::Int).collect(),
            Ty::Float => {
                let mut v: Vec<f64> = self.lits.floats.clone();
                v.extend([0.0, 1.0, -1.0, 0.5, -0.5, 2.0, 0.1, 10.0, 100.0, -100.0, 1e-9, 1e9, -1e9, 1e300, -1e300]);
                v.into_iter().map(Value::Float).collect()
            }
            Ty::Bool => vec![Value::Bool(false), Value::Bool(true)],
            Ty::Char => {
                let mut v = self.lits.chars.clone();
                v.extend(['a', 'z', 'A', 'Z', '0', '9', ' ', '\n', '_', '-', '.', '\u{e9}']);
                v.into_iter().map(Value::Char).collect()
            }
            Ty::Str => {
                let mut v = self.lits.strs.clone();
                for s in ["", "a", "ab", "abc", " ", "A", "0", "123", "-1", "a b", "hello", "Hello, World!", "\u{e9}", "\n"] {
                    v.push(s.to_string());
                }
                v.into_iter().map(|s| Value::Str(Rc::new(s))).collect()
            }
            Ty::Array(_) => {
                let Some(e) = t.elem() else { return Vec::new() };
                let mut v = vec![Value::arr(Vec::new())];
                if depth < 3 {
                    let xs = self.values(e, depth + 1);
                    if let Some(a) = xs.first() {
                        v.push(Value::arr(vec![a.clone()]));
                    }
                    if xs.len() >= 2 {
                        v.push(Value::arr(vec![xs[0].clone(), xs[1].clone()]));
                        v.push(Value::arr(vec![xs[1].clone(), xs[0].clone()]));
                        v.push(Value::arr(vec![xs[1].clone(), xs[1].clone()]));
                    }
                    if xs.len() >= 4 {
                        v.push(Value::arr(xs[..4].to_vec()));
                    }
                }
                v
            }
            Ty::Map(_) => {
                let Some((kt, vt)) = t.map_kv() else { return Vec::new() };
                let mut v = vec![Value::map(Vec::new())];
                if depth < 3 {
                    let (ks, vs) = (self.values(kt, depth + 1), self.values(vt, depth + 1));
                    if let (Some(k), Some(x)) = (ks.first(), vs.first()) {
                        v.push(Value::map(vec![(k.clone(), x.clone())]));
                    }
                    if ks.len() >= 2 && vs.len() >= 2 {
                        v.push(Value::map(vec![(ks[0].clone(), vs[1].clone()), (ks[1].clone(), vs[0].clone())]));
                    }
                }
                v
            }
            Ty::Struct(id) => {
                let Some(info) = self.m.structs.get(t) else { return Vec::new() };
                let fields: Vec<Ty> = info.fields.iter().map(|(_, ft)| *ft).collect();
                let mut v = vec![self.zero(t, 0)];
                if depth >= 3 {
                    return v;
                }
                if info.option {
                    v.extend(
                        self.values(fields[1], depth + 1).into_iter().take(6).map(|x| Value::strukt(id, vec![Value::Bool(true), x])),
                    );
                    return v;
                }
                if !info.variants.is_empty() {
                    // each variant once, with the first values of its fields
                    for k in 0..info.variants.len() {
                        let mut fs: Vec<Value> = fields.iter().map(|ft| self.zero(*ft, depth + 1)).collect();
                        fs[0] = Value::Int(k as i64);
                        for slot in info.slots(k) {
                            if let Some(x) = self.values(fields[slot], depth + 1).into_iter().nth(1) {
                                fs[slot] = x;
                            }
                        }
                        v.push(Value::strukt(id, fs));
                    }
                    return v;
                }
                // the j-th value of every field, for a few j
                let per: Vec<Vec<Value>> = fields.iter().map(|ft| self.values(*ft, depth + 1)).collect();
                for j in 1..5 {
                    if per.iter().all(|p| !p.is_empty()) {
                        v.push(Value::strukt(id, per.iter().map(|p| p[j % p.len()].clone()).collect()));
                    }
                }
                v
            }
            _ => Vec::new(),
        }
    }

    /// The value a `none` holds and arrays start from: zeros, empty text and collections.
    fn zero(&self, t: Ty, depth: usize) -> Value {
        match t {
            Ty::Int => Value::Int(0),
            Ty::Float => Value::Float(0.0),
            Ty::Bool => Value::Bool(false),
            Ty::Char => Value::Char('\0'),
            Ty::Str => Value::Str(Rc::new(String::new())),
            Ty::Array(_) => Value::arr(Vec::new()),
            Ty::Map(_) => Value::map(Vec::new()),
            Ty::Struct(id) if depth < 64 => {
                let fields: Vec<Ty> = self.m.structs.get(t).map(|s| s.fields.iter().map(|(_, ft)| *ft).collect()).unwrap_or_default();
                Value::strukt(id, fields.into_iter().map(|ft| self.zero(ft, depth + 1)).collect())
            }
            _ => Value::Unset,
        }
    }

    /// A random value of type `t`.
    fn random(&mut self, t: Ty, depth: usize) -> Value {
        match t {
            Ty::Int => {
                let sign = if self.rng.below(2) == 0 { 1 } else { -1 };
                let n = match self.rng.below(10) {
                    0..=3 => self.rng.below(11) as i64,
                    4..=6 => self.rng.below(1001) as i64,
                    7 => {
                        let pool = self.int_pool();
                        return Value::Int(*self.rng.pick(&pool));
                    }
                    // any size: a random number of random bits
                    _ => (self.rng.next() >> self.rng.below(64)) as i64,
                };
                Value::Int(n.wrapping_mul(sign))
            }
            Ty::Float => {
                let x = match self.rng.below(4) {
                    0 => self.rng.below(21) as f64 - 10.0,
                    1 => (self.rng.below(2_000_001) as f64 - 1_000_000.0) / 1000.0,
                    2 => {
                        let e = self.rng.below(41) as i32 - 20;
                        (self.rng.below(1000) as f64 / 100.0) * 10f64.powi(e)
                    }
                    _ => (self.rng.below(2_000_001) as f64 - 1_000_000.0) / 10.0,
                };
                Value::Float(if self.rng.below(2) == 0 { x } else { -x })
            }
            Ty::Bool => Value::Bool(self.rng.below(2) == 1),
            Ty::Char => Value::Char(self.random_char()),
            Ty::Str => {
                let n = self.rng.below(9);
                let s: String = (0..n).map(|_| self.random_char()).collect();
                Value::Str(Rc::new(s))
            }
            Ty::Array(_) => {
                let Some(e) = t.elem() else { return Value::arr(Vec::new()) };
                let n = if depth >= 3 { 0 } else { self.rng.below(7) };
                Value::arr((0..n).map(|_| self.random(e, depth + 1)).collect())
            }
            Ty::Map(_) => {
                let Some((kt, vt)) = t.map_kv() else { return Value::map(Vec::new()) };
                let n = if depth >= 3 { 0 } else { self.rng.below(5) };
                Value::map((0..n).map(|_| (self.random(kt, depth + 1), self.random(vt, depth + 1))).collect())
            }
            Ty::Struct(id) => {
                let Some(info) = self.m.structs.get(t) else { return Value::Unset };
                let fields: Vec<Ty> = info.fields.iter().map(|(_, ft)| *ft).collect();
                if depth >= 4 {
                    return self.zero(t, 0);
                }
                if info.option {
                    if self.rng.below(4) == 0 {
                        return self.zero(t, 0);
                    }
                    let x = self.random(fields[1], depth + 1);
                    return Value::strukt(id, vec![Value::Bool(true), x]);
                }
                if !info.variants.is_empty() {
                    let k = self.rng.below(info.variants.len() as u64) as usize;
                    let slots = info.slots(k);
                    let mut fs: Vec<Value> = fields.iter().map(|ft| self.zero(*ft, 0)).collect();
                    fs[0] = Value::Int(k as i64);
                    for slot in slots {
                        fs[slot] = self.random(fields[slot], depth + 1);
                    }
                    return Value::strukt(id, fs);
                }
                Value::strukt(id, fields.into_iter().map(|ft| self.random(ft, depth + 1)).collect())
            }
            _ => Value::Unset,
        }
    }

    fn random_char(&mut self) -> char {
        const SOME: &[char] =
            &['a', 'b', 'c', 'x', 'y', 'z', 'A', 'B', 'Z', '0', '1', '9', ' ', '-', '_', '.', ',', '!', '\u{e9}', '\n'];
        if !self.lits.chars.is_empty() && self.rng.below(4) == 0 {
            return *self.rng.pick(&self.lits.chars);
        }
        *self.rng.pick(SOME)
    }

    /// Makes a counterexample smaller, one argument at a time, while it still fails the same way.
    fn shrink(&self, fid: FuncId, pre: FuncId, mut args: Vec<Value>, mut err: RuntimeError) -> (Vec<Value>, RuntimeError) {
        let mut it = Interp::new(self.m, Limits::new(SHRINK_STEPS, 10_000).memory(256 << 20));
        let mut runs = 0;
        'again: loop {
            for k in 0..args.len() {
                for cand in smaller(self.m, &args[k]) {
                    if runs >= SHRINK_RUNS {
                        break 'again;
                    }
                    runs += 1;
                    let mut next = args.clone();
                    next[k] = cand;
                    match attempt(&mut it, fid, pre, &next) {
                        Attempt::Failed(e) if e.code == err.code && e.span == err.span => {
                            args = next;
                            err = e;
                            continue 'again;
                        }
                        Attempt::Exhausted | Attempt::Unsupported => break 'again,
                        _ => {}
                    }
                }
            }
            break;
        }
        (args, err)
    }

    fn report(&self, args: &[Value], e: RuntimeError) -> Diag {
        let name = display_name(&self.t.name);
        let shown: Vec<String> = args.iter().map(|v| interp::show(self.m, v)).collect();
        let call = format!("{name}({})", shown.join(", "));
        if e.code == "E0421" {
            return Diag::new("E0421", e.msg, e.span).hint(format!(
                "`{call}` meets every `requires`: fix the function for it, the `ensures` if it is wrong, or rule it out with `requires`"
            ));
        }
        let place = match e.func.map(|f| display_name(&self.m.func(f).name).to_string()) {
            Some(f) if f == name && e.span.line > 0 => format!(" (at {}:{})", e.span.line, e.span.col),
            Some(f) if !f.contains('#') && e.span.line > 0 => format!(" (at {}:{} in `{f}`)", e.span.line, e.span.col),
            _ => String::new(),
        };
        Diag::new("E0422", format!("`{call}` stops with runtime error {}: {}{place}", e.code, e.msg), self.t.span)
            .hint("a counterexample: handle it, or rule it out with `requires`")
    }
}

/// Runs `pre` and then the function on `args`.
fn attempt(it: &mut Interp, fid: FuncId, pre: FuncId, args: &[Value]) -> Attempt {
    match it.call(pre, args.to_vec()) {
        Ok(Some(Value::Bool(true))) => {}
        Ok(_) | Err(Stop::Error(_) | Stop::Depth) => return Attempt::Rejected,
        Err(Stop::Steps | Stop::Memory | Stop::Time) => return Attempt::Exhausted,
        Err(_) => return Attempt::Unsupported,
    }
    match it.call(fid, args.to_vec()) {
        Ok(_) => Attempt::Passed,
        Err(Stop::Error(e)) => Attempt::Failed(*e),
        // too deep for compile time: not a finding
        Err(Stop::Depth) => Attempt::Rejected,
        Err(Stop::Steps | Stop::Memory | Stop::Time) => Attempt::Exhausted,
        Err(_) => Attempt::Unsupported,
    }
}

/// Two inputs are the same (also two NaNs, which `==` never finds equal).
fn same_value(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Float(x), Value::Float(y)) => x.to_bits() == y.to_bits(),
        _ => interp::equal(a, b),
    }
}

/// Values "smaller" than `v`, most promising first: toward 0, shorter, fewer entries.
fn smaller(m: &Module, v: &Value) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    match v {
        Value::Int(n) => {
            // n - d for d = n, n/2, n/4, ..., 1: from 0 back up to n - 1
            let n = *n as i128;
            let mut d = n;
            while d != 0 {
                out.push(Value::Int((n - d) as i64));
                d /= 2;
            }
        }
        Value::Float(x) => {
            for y in [0.0, x.trunc(), (x / 2.0).trunc(), x / 2.0] {
                if y.abs() < x.abs() || (y == 0.0 && *x != 0.0) || (y != *x && y.fract() == 0.0 && x.fract() != 0.0) {
                    out.push(Value::Float(y));
                }
            }
        }
        Value::Bool(true) => out.push(Value::Bool(false)),
        Value::Char(c) if *c != 'a' => out.push(Value::Char('a')),
        Value::Str(s) => {
            let cs: Vec<char> = s.chars().collect();
            if !cs.is_empty() {
                let text = |xs: &[char]| Value::Str(Rc::new(xs.iter().collect()));
                out.push(text(&[]));
                out.push(text(&cs[..cs.len() / 2]));
                out.push(text(&cs[cs.len() / 2..]));
                out.push(text(&cs[1..]));
                out.push(text(&cs[..cs.len() - 1]));
                for i in 0..cs.len().min(8) {
                    if cs[i] != 'a' {
                        let mut ys = cs.clone();
                        ys[i] = 'a';
                        out.push(text(&ys));
                    }
                }
            }
        }
        Value::Arr(xs) => {
            if !xs.is_empty() {
                out.push(Value::arr(Vec::new()));
                out.push(Value::arr(xs[..xs.len() / 2].to_vec()));
                out.push(Value::arr(xs[xs.len() / 2..].to_vec()));
                for i in 0..xs.len().min(16) {
                    let mut ys = xs.to_vec();
                    ys.remove(i);
                    out.push(Value::arr(ys));
                }
                for i in 0..xs.len().min(8) {
                    for s in smaller(m, &xs[i]).into_iter().take(4) {
                        let mut ys = xs.to_vec();
                        ys[i] = s;
                        out.push(Value::arr(ys));
                    }
                }
            }
        }
        Value::Map(_) => {
            let es = v.map_entries().unwrap_or_default();
            if !es.is_empty() {
                out.push(Value::map(Vec::new()));
                for i in 0..es.len().min(8) {
                    let mut ys = es.clone();
                    ys.remove(i);
                    out.push(Value::map(ys));
                }
            }
        }
        Value::Struct(id, fields) => {
            let info = m.structs.get(Ty::Struct(*id));
            let option = info.is_some_and(|s| s.option);
            let tagged = info.is_some_and(|s| !s.variants.is_empty());
            if option && matches!(fields.first(), Some(Value::Bool(true))) {
                // `none`: the value a `none` holds does not matter, so the one there stays
                let mut fs = fields.to_vec();
                fs[0] = Value::Bool(false);
                out.push(Value::strukt(*id, fs));
            }
            // (the number of an enum's variant stays: another variant has other fields)
            for i in usize::from(option || tagged)..fields.len() {
                for s in smaller(m, &fields[i]).into_iter().take(4) {
                    let mut fs = fields.to_vec();
                    fs[i] = s;
                    out.push(Value::strukt(*id, fs));
                }
            }
        }
        _ => {}
    }
    out.retain(|x| !same_value(x, v));
    out
}

/// The functions `start` calls, directly or not (and `start`), by index.
fn reachable(m: &Module, start: &[FuncId]) -> Vec<usize> {
    let mut seen: Vec<usize> = start.iter().map(|f| f.0 as usize).collect();
    let mut i = 0;
    while i < seen.len() {
        let mut calls = Vec::new();
        each_stmt(&m.funcs[seen[i]].body, &mut |s| {
            if let ir::StmtKind::Call { func, .. } = &s.kind {
                calls.push(func.0 as usize);
            }
        });
        for c in calls {
            if !seen.contains(&c) {
                seen.push(c);
            }
        }
        i += 1;
    }
    seen
}

/// True if the statements use a standard module whose results depend on the world (`WORLD`).
fn touches_world(body: &[ir::Stmt]) -> bool {
    let mut found = false;
    each_stmt(body, &mut |s| {
        if let ir::StmtKind::Op { op: RtOp::Std(f), .. } | ir::StmtKind::Mutate { op: RtOp::Std(f), .. } = &s.kind {
            found |= WORLD.contains(&f.path().0);
        }
    });
    found
}

/// Calls `f` on every statement, also the nested ones.
fn each_stmt(ss: &[ir::Stmt], f: &mut dyn FnMut(&ir::Stmt)) {
    for s in ss {
        f(s);
        match &s.kind {
            ir::StmtKind::If { then, els, .. } => {
                each_stmt(then, f);
                each_stmt(els, f);
            }
            ir::StmtKind::Loop { head, body, step, .. } => {
                each_stmt(head, f);
                each_stmt(body, f);
                each_stmt(step, f);
            }
            ir::StmtKind::ForEach { body, .. } => each_stmt(body, f),
            _ => {}
        }
    }
}

/// The constants of the statements.
fn literals(m: &Module, body: &[ir::Stmt], lits: &mut Lits) {
    fn expr(m: &Module, e: &ir::Expr, lits: &mut Lits) {
        match e {
            ir::Expr::Int(n) => push_unique(&mut lits.ints, *n),
            ir::Expr::Float(x) => push_unique(&mut lits.floats, *x),
            ir::Expr::Char(c) => {
                if let Some(c) = char::from_u32(*c) {
                    push_unique(&mut lits.chars, c);
                }
            }
            // (short texts only: a long one is a message, not a value that matters)
            ir::Expr::Str(id) if m.str(*id).chars().count() <= 16 => push_unique(&mut lits.strs, m.str(*id).to_string()),
            ir::Expr::Unary(_, x) | ir::Expr::IntToFloat(x) | ir::Expr::Field(x, _, _) => expr(m, x, lits),
            ir::Expr::Binary(_, a, b) => {
                expr(m, a, lits);
                expr(m, b, lits);
            }
            ir::Expr::Select(c, a, b) => {
                expr(m, c, lits);
                expr(m, a, lits);
                expr(m, b, lits);
            }
            ir::Expr::Pure(_, args) => args.iter().for_each(|a| expr(m, a, lits)),
            _ => {}
        }
    }
    each_stmt(body, &mut |s| match &s.kind {
        ir::StmtKind::Set(_, e) | ir::StmtKind::Return(Some(e)) => expr(m, e, lits),
        ir::StmtKind::If { cond, .. } | ir::StmtKind::Loop { cond, .. } => expr(m, cond, lits),
        ir::StmtKind::Call { args, .. } => {
            for a in args {
                if let ir::Arg::Val(e) = a {
                    expr(m, e, lits);
                }
            }
        }
        // (not the parts of a message: `RtOp::Format` texts are words, not values)
        ir::StmtKind::Op { op, args, .. } if *op != RtOp::Format && *op != RtOp::Print => args.iter().for_each(|a| expr(m, a, lits)),
        ir::StmtKind::Store { value, .. } => expr(m, value, lits),
        ir::StmtKind::Mutate { args, .. } => args.iter().for_each(|a| expr(m, a, lits)),
        _ => {}
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ints_shrink_toward_zero_in_halving_steps() {
        let m = Module { funcs: Vec::new(), strs: Vec::new(), main: FuncId(0), structs: ir::Structs::default() };
        let got: Vec<i64> = smaller(&m, &Value::Int(100))
            .into_iter()
            .map(|v| match v {
                Value::Int(n) => n,
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(got, vec![0, 50, 75, 88, 94, 97, 99]);
        // the smallest int has no positive counterpart: it still shrinks without overflowing
        let min = smaller(&m, &Value::Int(i64::MIN));
        assert!(matches!(min.first(), Some(Value::Int(0))));
        assert!(smaller(&m, &Value::Int(0)).is_empty());
    }

    #[test]
    fn texts_and_arrays_shrink_to_shorter_ones() {
        let m = Module { funcs: Vec::new(), strs: Vec::new(), main: FuncId(0), structs: ir::Structs::default() };
        let s = smaller(&m, &Value::Str(Rc::new("abcd".into())));
        assert!(matches!(&s[0], Value::Str(t) if t.is_empty()));
        assert!(s.iter().all(|v| matches!(v, Value::Str(t) if t.chars().count() <= 4 && t.as_str() != "abcd")));
        let a = smaller(&m, &Value::arr(vec![Value::Int(5), Value::Int(7)]));
        assert!(matches!(&a[0], Value::Arr(xs) if xs.is_empty()));
        assert!(a.iter().any(|v| matches!(v, Value::Arr(xs) if xs.len() == 1)));
    }

    #[test]
    fn the_generator_is_seeded() {
        let (mut a, mut b) = (Rng(42), Rng(42));
        let xs: Vec<u64> = (0..5).map(|_| a.next()).collect();
        let ys: Vec<u64> = (0..5).map(|_| b.next()).collect();
        assert_eq!(xs, ys);
        assert!(xs.windows(2).all(|w| w[0] != w[1]));
    }

    /// Compiles `src` and gives its errors.
    fn errors(src: &str) -> Vec<Diag> {
        match crate::compile(src) {
            Ok(_) => Vec::new(),
            Err(d) => d,
        }
    }

    #[test]
    fn a_broken_ensures_is_found_and_shrunk() {
        let errs = errors(
            "fn withdraw(balance: int, amount: int) -> int\n    requires amount > 0, amount <= balance\n    ensures result > 0\n= balance - amount\nprint(withdraw(10, 3))\n",
        );
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert_eq!(errs[0].code, "E0421");
        assert_eq!(errs[0].msg, "withdraw(1, 1) breaks ensures result > 0: result is 0");
        assert_eq!((errs[0].span.line, errs[0].span.col), (3, 13));
    }

    #[test]
    fn a_crash_is_found_and_a_contract_that_holds_is_quiet() {
        let errs = errors("fn neg(x: int) -> int\n    ensures result + x == 0\n= -x\nprint(neg(3))\n");
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert_eq!(errs[0].code, "E0422");
        assert!(errs[0].msg.starts_with("`neg(-9223372036854775808)` stops with runtime error E0255"), "{}", errs[0].msg);
        let ok = errors("fn neg(x: int) -> int\n    requires x > -1000000000\n    ensures result + x == 0\n= -x\nprint(neg(3))\n");
        assert!(ok.is_empty(), "{ok:?}");
    }

    #[test]
    fn arrays_strings_and_structs_are_searched() {
        let errs = errors("fn first(xs: [int]) -> int\n    ensures result <= xs.len()\n= xs[0]\nprint(first([1]))\n");
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert_eq!(errs[0].code, "E0422", "{errs:?}");
        assert!(errs[0].msg.starts_with("`first([])` stops with runtime error E0240"), "{}", errs[0].msg);
        let errs = errors(
            "struct P { x: int, name: str }\nfn label(p: P) -> str\n    requires p.x >= 0\n    ensures result.len() > 0\n= p.name\nprint(label(P(x: 1, name: \"a\")))\n",
        );
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert_eq!(errs[0].msg, "label(P(x: 0, name: \"\")) breaks ensures result.len() > 0: result is \"\"");
    }
}
