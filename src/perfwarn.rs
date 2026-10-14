//! Performance warnings: code that is correct but slow in a way the compiler cannot fix. They
//! never stop the build (they are `diag::warn`ings: printed to stderr, listed under `"warnings"`
//! in `--json`). A warning is given only where the pattern is nearly certain to be slow, so a
//! program that is fine stays quiet:
//!
//! - E0360: `xs.contains(x)` / `xs.index_of(x)` in a loop of 1000 rounds or more, where `xs` is an
//!   array that the function builds with `push`. Every search scans the array: a map finds a key at once.
//! - E0361: `s = x + s` in a loop: putting text in front copies the whole string every round on
//!   every backend (appending, `s = s + x` and `s += x`, is amortized O(1)).
//! - E0362: `s += x` / `s = s + x` in a loop of 1000 rounds or more when the target is Go, whose
//!   strings are copied by every concatenation.
//!
//! A loop is "big" when it is a counted loop (`for i in 0..n` with a known `n`) or a `while i < n`
//! loop of 1000 rounds or more, "small" when it is a counted loop of fewer than 100 rounds, and
//! unknown otherwise (`while`, `for x in xs`): the size of the data is not known, so only E0361 is
//! reported there. Nothing is reported in a small loop.

use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::diag::{warn, Diag};

/// Rounds from which a counted loop is "big" (a search in it is likely to scan a long array).
const BIG: i64 = 1000;
/// Rounds below which a counted loop is "small" (never reported).
const SMALL: i64 = 100;

#[derive(Clone, Copy, PartialEq)]
enum Size {
    Small,
    Unknown,
    Big,
}

struct Loop {
    size: Size,
}

struct Pass {
    go: bool,
    /// Immutable ints with a known value (`let n = 100000`).
    consts: HashMap<String, i64>,
    /// The arrays that the function being checked makes longer somewhere.
    built: HashSet<String>,
    /// The arrays whose length the function compares with a small number (`xs.len() < 3`): kept short.
    capped: HashSet<String>,
    /// Names declared in the function (parameters, `let` and `var`) with their types.
    locals: HashMap<String, Type>,
    loops: Vec<Loop>,
}

/// Adds the performance warnings of a checked program. `go`: the target is Go.
pub fn check(prog: &Program, go: bool) {
    // the immutable ints of a script's top level are visible in every function
    let mut script = HashMap::new();
    if prog.script {
        for f in prog.funcs.iter().filter(|f| f.name == "main") {
            for s in &f.body {
                if let StmtKind::Let { name, mutable: false, value, .. } = &s.kind {
                    if let Some(n) = constant(value, &script) {
                        script.insert(name.clone(), n);
                    }
                }
            }
        }
    }
    for f in &prog.funcs {
        let mut p = Pass {
            go,
            consts: script.clone(),
            built: HashSet::new(),
            capped: HashSet::new(),
            locals: HashMap::new(),
            loops: Vec::new(),
        };
        for param in &f.params {
            p.locals.insert(param.name.clone(), param.ty);
            p.consts.remove(&param.name);
        }
        p.declare(&f.body);
        grown(&f.body, &mut p.built);
        p.capped = capped(&f.body, &p.consts);
        p.block(&f.body);
    }
}

/// The value of an int expression made of literals and known immutable ints.
fn constant(e: &Expr, consts: &HashMap<String, i64>) -> Option<i64> {
    match &e.kind {
        ExprKind::Int(n) => Some(*n),
        ExprKind::Var(v) => consts.get(v).copied(),
        ExprKind::Unary(UnOp::Neg, x) => constant(x, consts)?.checked_neg(),
        ExprKind::Binary(op, a, b) => {
            let (a, b) = (constant(a, consts)?, constant(b, consts)?);
            match op {
                BinOp::Add => a.checked_add(b),
                BinOp::Sub => a.checked_sub(b),
                BinOp::Mul => a.checked_mul(b),
                _ => None,
            }
        }
        _ => None,
    }
}

/// The names that statements make longer arrays: `xs.push(v)`, `xs.insert(i, v)`, `xs += ys`,
/// `xs = xs + ys`.
fn grown(ss: &[Stmt], out: &mut HashSet<String>) {
    for s in ss {
        match &s.kind {
            StmtKind::Expr(Expr { kind: ExprKind::Method(recv, name, _), .. }) if matches!(name.as_str(), "push" | "insert") => {
                if let ExprKind::Var(v) = &recv.kind {
                    out.insert(v.clone());
                }
            }
            StmtKind::Assign { target, op, value } if matches!(value.ty, Type::Array(_)) => {
                if let ExprKind::Var(v) = &target.kind {
                    let appends = match op {
                        Some(BinOp::Add) => true,
                        None => starts_with_var(value, v),
                        _ => false,
                    };
                    if appends {
                        out.insert(v.clone());
                    }
                }
            }
            StmtKind::If { then, els, .. } => {
                grown(then, out);
                if let Some(e) = els {
                    grown(e, out);
                }
            }
            StmtKind::While { body, .. } | StmtKind::For { body, .. } | StmtKind::ForEach { body, .. } | StmtKind::Arena(body) => {
                grown(body, out)
            }
            _ => {}
        }
    }
}

/// The arrays of a function whose length is compared with a number below 100 (`xs.len() < 3`,
/// `xs.len() == 0` does not count): the function keeps them short, so searching them is cheap.
fn capped(ss: &[Stmt], consts: &HashMap<String, i64>) -> HashSet<String> {
    fn on_expr(e: &Expr, consts: &HashMap<String, i64>, out: &mut HashSet<String>) {
        if let ExprKind::Binary(BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge, a, b) = &e.kind {
            for (len, limit) in [(a, b), (b, a)] {
                if let ExprKind::Method(recv, name, args) = &len.kind {
                    if let (ExprKind::Var(xs), "len", true) = (&recv.kind, name.as_str(), args.is_empty()) {
                        if constant(limit, consts).is_some_and(|n| n < SMALL) {
                            out.insert(xs.clone());
                        }
                    }
                }
            }
        }
        each_child(e, &mut |c| on_expr(c, consts, out));
    }
    fn on_block(ss: &[Stmt], consts: &HashMap<String, i64>, out: &mut HashSet<String>) {
        for s in ss {
            match &s.kind {
                StmtKind::Let { value, .. } => on_expr(value, consts, out),
                StmtKind::Assign { target, value, .. } => {
                    on_expr(target, consts, out);
                    on_expr(value, consts, out);
                }
                StmtKind::Match { scrut, arms } => {
                    on_expr(scrut, consts, out);
                    for arm in arms {
                        on_block(&arm.body, consts, out);
                    }
                }
                StmtKind::If { cond, then, els } => {
                    on_expr(cond, consts, out);
                    on_block(then, consts, out);
                    if let Some(e) = els {
                        on_block(e, consts, out);
                    }
                }
                StmtKind::While { cond, body } => {
                    on_expr(cond, consts, out);
                    on_block(body, consts, out);
                }
                StmtKind::For { start, end, step, body, .. } => {
                    on_expr(start, consts, out);
                    on_expr(end, consts, out);
                    step.iter().for_each(|k| on_expr(k, consts, out));
                    on_block(body, consts, out);
                }
                StmtKind::ForEach { iter, body, .. } => {
                    on_expr(iter, consts, out);
                    on_block(body, consts, out);
                }
                StmtKind::Arena(body) => on_block(body, consts, out),
                StmtKind::Ret(Some(e)) | StmtKind::Expr(e) => on_expr(e, consts, out),
                StmtKind::Ret(None) | StmtKind::Break | StmtKind::Continue => {}
            }
        }
    }
    let mut out = HashSet::new();
    on_block(ss, consts, &mut out);
    out
}

/// Calls `f` with each direct sub-expression of `e`.
fn each_child<'a>(e: &'a Expr, f: &mut dyn FnMut(&'a Expr)) {
    match &e.kind {
        ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Bool(_) | ExprKind::Str(_) | ExprKind::Char(_) | ExprKind::Var(_) => {}
        ExprKind::Interp(parts) => {
            for p in parts {
                if let InterpPart::Expr(x) = p {
                    f(x);
                }
            }
        }
        ExprKind::Unary(_, x) | ExprKind::Field(x, _) | ExprKind::Labeled(_, x) | ExprKind::Inout(x) | ExprKind::Lambda(_, x) => f(x),
        ExprKind::Binary(_, a, b) | ExprKind::Index(a, b) => {
            f(a);
            f(b);
        }
        ExprKind::Call(_, args) | ExprKind::Array(args) | ExprKind::Tuple(args) => {
            for a in args {
                f(a);
            }
        }
        ExprKind::None => {}
        ExprKind::Some(x) | ExprKind::Fmt(x, _) => f(x),
        ExprKind::Coalesce(a, b) | ExprKind::In(a, b) => {
            f(a);
            f(b);
        }
        ExprKind::Slice(x, a, b) => {
            f(x);
            for e in [a, b].into_iter().flatten() {
                f(e);
            }
        }
        ExprKind::If(c, a, b) => {
            f(c);
            f(a);
            f(b);
        }
        ExprKind::Bind(_, v, body) => {
            f(v);
            f(body);
        }
        ExprKind::Match(scrut, arms) => {
            f(scrut);
            for arm in arms {
                for st in &arm.body {
                    if let StmtKind::Expr(x) = &st.kind {
                        f(x);
                    }
                }
            }
        }
        ExprKind::MapLit(items) => {
            for (k, v) in items {
                f(k);
                f(v);
            }
        }
        ExprKind::Method(r, _, args) => {
            f(r);
            for a in args {
                f(a);
            }
        }
        ExprKind::Comprehension(c) => {
            f(&c.elem);
            match &c.src {
                CompSrc::Each(x) => f(x),
                CompSrc::Range(a, b, k) => {
                    f(a);
                    f(b);
                    if let Some(k) = k {
                        f(k);
                    }
                }
            }
            if let Some(x) = &c.cond {
                f(x);
            }
        }
    }
}

/// `v + a + b ...`: a sum whose first operand is the variable `v`.
fn starts_with_var(e: &Expr, v: &str) -> bool {
    match &e.kind {
        ExprKind::Binary(BinOp::Add, a, _) => starts_with_var(a, v),
        ExprKind::Var(x) => x == v,
        _ => false,
    }
}

/// `a + b + v`: a sum whose last operand is the variable `v`, with more in front (not `v` itself).
fn ends_with_var(e: &Expr, v: &str) -> bool {
    matches!(&e.kind, ExprKind::Binary(BinOp::Add, a, b) if matches!(&b.kind, ExprKind::Var(x) if x == v) && !mentions(a, v))
}

fn mentions(e: &Expr, v: &str) -> bool {
    if matches!(&e.kind, ExprKind::Var(x) if x == v) {
        return true;
    }
    let mut found = false;
    each_child(e, &mut |c| found |= mentions(c, v));
    found
}

impl Pass {
    /// Records the names a block declares and its immutable ints.
    fn declare(&mut self, ss: &[Stmt]) {
        for s in ss {
            match &s.kind {
                StmtKind::Let { name, mutable, ty, value } => {
                    self.locals.insert(name.clone(), ty.unwrap_or(value.ty));
                    self.consts.remove(name);
                    if !mutable {
                        if let Some(n) = constant(value, &self.consts) {
                            self.consts.insert(name.clone(), n);
                        }
                    }
                }
                StmtKind::If { then, els, .. } => {
                    self.declare(then);
                    if let Some(e) = els {
                        self.declare(e);
                    }
                }
                StmtKind::While { body, .. } | StmtKind::Arena(body) => self.declare(body),
                StmtKind::For { var, body, .. } => {
                    self.locals.insert(var.clone(), Type::Int);
                    self.declare(body);
                }
                StmtKind::ForEach { var, index, body, .. } => {
                    self.locals.insert(var.clone(), Type::Unknown);
                    if let Some(i) = index {
                        self.locals.insert(i.clone(), Type::Int);
                    }
                    self.declare(body);
                }
                _ => {}
            }
        }
    }

    fn block(&mut self, ss: &[Stmt]) {
        for s in ss {
            self.stmt(s);
        }
    }

    fn in_loop(&mut self, size: Size, body: &[Stmt]) {
        self.loops.push(Loop { size });
        self.block(body);
        self.loops.pop();
    }

    /// `while i < 100000`: a loop that counts up (or down) to a known limit of 1000 or more.
    fn while_size(&self, cond: &Expr) -> Size {
        let big = |limit: &Expr| constant(limit, &self.consts).is_some_and(|n| n >= BIG);
        let counted = match &cond.kind {
            ExprKind::Binary(BinOp::Lt | BinOp::Le, _, b) => big(b),
            ExprKind::Binary(BinOp::Gt | BinOp::Ge, a, _) => big(a),
            ExprKind::Binary(BinOp::And, a, b) => self.while_size(a) == Size::Big || self.while_size(b) == Size::Big,
            _ => false,
        };
        if counted {
            Size::Big
        } else {
            Size::Unknown
        }
    }

    /// The rounds of `for _ in a..b step k` when they are all known.
    fn size(&self, start: &Expr, end: &Expr, step: &Option<Expr>) -> Size {
        let (Some(a), Some(b)) = (constant(start, &self.consts), constant(end, &self.consts)) else { return Size::Unknown };
        let k = match step {
            None => 1,
            Some(k) => match constant(k, &self.consts) {
                Some(k) if k != 0 => k,
                _ => return Size::Unknown,
            },
        };
        let span = (b as i128 - a as i128) * k.signum() as i128;
        let rounds = if span <= 0 { 0 } else { (span + k.unsigned_abs() as i128 - 1) / k.unsigned_abs() as i128 };
        if rounds < SMALL as i128 {
            Size::Small
        } else if rounds >= BIG as i128 {
            Size::Big
        } else {
            Size::Unknown
        }
    }

    fn stmt(&mut self, s: &Stmt) {
        match &s.kind {
            StmtKind::Let { value, .. } => self.expr(value),
            StmtKind::Assign { target, op, value } => {
                self.expr(target);
                self.expr(value);
                self.assign(target, *op, value);
            }
            StmtKind::Match { scrut, arms } => {
                self.expr(scrut);
                for arm in arms {
                    self.block(&arm.body);
                }
            }
            StmtKind::If { cond, then, els } => {
                self.expr(cond);
                self.block(then);
                if let Some(e) = els {
                    self.block(e);
                }
            }
            StmtKind::While { cond, body } => {
                // (the condition runs every round, so it belongs to the loop)
                self.loops.push(Loop { size: self.while_size(cond) });
                self.expr(cond);
                self.block(body);
                self.loops.pop();
            }
            StmtKind::For { start, end, step, body, .. } => {
                self.expr(start);
                self.expr(end);
                if let Some(k) = step {
                    self.expr(k);
                }
                let size = self.size(start, end, step);
                self.in_loop(size, body);
            }
            StmtKind::ForEach { iter, body, .. } => {
                self.expr(iter);
                self.in_loop(Size::Unknown, body);
            }
            StmtKind::Arena(body) => self.block(body),
            StmtKind::Ret(Some(e)) | StmtKind::Expr(e) => self.expr(e),
            StmtKind::Ret(None) | StmtKind::Break | StmtKind::Continue => {}
        }
    }

    /// True inside a loop that is not known to be small.
    fn busy(&self) -> bool {
        self.loops.iter().any(|l| l.size != Size::Small)
    }

    fn assign(&mut self, target: &Expr, op: Option<BinOp>, value: &Expr) {
        let ExprKind::Var(s) = &target.kind else { return };
        // (the checker does not record the type of an assignment's target: the variable's declaration has it)
        if self.locals.get(s) != Some(&Type::Str) || !self.busy() {
            return;
        }
        if op.is_none() && ends_with_var(value, s) {
            warn(
                Diag::new("E0361", format!("`{s} = ... + {s}` in a loop copies `{s}` in every round"), value.span).hint(format!(
                    "putting text in front of a string is slow when it is done again and again: append instead (`{s} += part`) and build the result in the other order, or collect the parts in an array and `join` them at the end"
                )),
            );
        }
        let appends = op == Some(BinOp::Add) || (op.is_none() && starts_with_var(value, s));
        if self.go && appends && self.loops.iter().any(|l| l.size == Size::Big) {
            warn(
                Diag::new("E0362", format!("appending to `{s}` in a loop copies `{s}` in every round on the Go target"), target.span)
                    .hint("Go strings are immutable: collect the parts in an array (`parts.push(part)`) and `parts.join(\"\")` them after the loop"),
            );
        }
    }

    fn expr(&mut self, e: &Expr) {
        if let ExprKind::Method(recv, name, _) = &e.kind {
            if matches!(name.as_str(), "contains" | "index_of") && matches!(recv.ty, Type::Array(_)) {
                self.search(recv, name, e.span);
            }
        }
        let mut children: Vec<&Expr> = Vec::new();
        each_child(e, &mut |c| children.push(c));
        for c in children {
            self.expr(c);
        }
    }

    /// `xs.contains(..)` / `xs.index_of(..)` at `span`, `xs` being the receiver.
    fn search(&mut self, recv: &Expr, method: &str, span: Span) {
        let ExprKind::Var(xs) = &recv.kind else { return };
        // a long loop that searches an array the function builds up (a list of seen values)
        if self.loops.iter().any(|l| l.size == Size::Big)
            && self.built.contains(xs)
            && !self.capped.contains(xs)
            && self.locals.contains_key(xs)
        {
            warn(
                Diag::new("E0360", format!("`{xs}.{method}(..)` in a loop scans all of `{xs}` in every round"), span).hint(format!(
                    "a map finds a key without scanning: keep the values as keys (`var seen: [int: bool] = [:]`, `seen[x] = true`, `seen.has(x)`) instead of searching `{xs}`"
                )),
            );
        }
    }
}
