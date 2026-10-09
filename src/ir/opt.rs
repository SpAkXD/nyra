//! Simple, safe optimizations on the IR. Every pass keeps the program's output identical on
//! every backend; `NYRA_OPT=0` turns them all off (tests compare both ways).
//!
//! - `fold`: expressions and runtime operations whose operands are constants (`eval` computes
//!   them exactly as the runtimes do; an operation that would fail stays, so the error still
//!   happens when the program runs); constant int, bool and string parts of `print` and string
//!   building become text. Floats fold only into values every backend writes exactly (no NaN,
//!   infinity or -0), and never into text: only the runtimes format them.
//! - `propagate`: a variable set once, to a constant, before everything else reads it, is
//!   replaced by the constant (`let n = 1000` makes `% n` a division by a constant).
//! - `calls`: a call of a user function with constant arguments is run at compile time when it
//!   is pure and finishes within a step budget (`eval::Calls`); its result replaces the call.
//! - `branches`: an `if` or loop with a constant condition keeps only what can run.
//! - `dead`: statements after `return`, and empty `if`s (conditions are pure).
//! - reference counting: earlier releases and fewer `dup`/`drop` pairs (see below).
//! - `unused_fns`: functions that `main` can never reach.

use std::collections::HashMap;

use super::eval::{self, Val};
use super::{Arg, BinOp, Expr, Func, FuncId, LocalId, Module, Place, RtOp, Step, Stmt, StmtKind, StrId, UnOp};

pub fn optimize(m: &mut Module) {
    // folding makes constants, which propagate into more folding and calls: a few rounds
    for _ in 0..8 {
        let mut strs = Interner::new(&mut m.strs);
        for f in &mut m.funcs {
            stmts(&mut f.body, &mut strs);
        }
        let mut changed = false;
        for f in &mut m.funcs {
            changed |= propagate(f);
        }
        changed |= fold_calls(m);
        if !changed {
            break;
        }
    }
    for f in &mut m.funcs {
        let temp: Vec<bool> = f.locals.iter().map(|l| l.name.is_none()).collect();
        let managed: Vec<bool> = f.locals.iter().map(|l| m.structs.managed(l.ty)).collect();
        early_drops(&mut f.body, &temp);
        dup_drop_pairs(&mut f.body, &managed);
    }
    unused_fns(m);
}

// ---- reference counting ----------------------------------------------------------------------
//
// Lowering releases the temporaries of a statement at its end and gives an element read from an
// array its own owner (`dup`) for as long as the statement runs. Both are safe and often too
// much: `dp[i][j] = dp[i - 1][j] + 1` holds an extra owner of row `i - 1` while row `i` is
// written, and a row `dp[i]` read in the same statement would even be copied by that write
// (copy on write sees two owners). Two passes take the extra work out:
//
// - `early_drops`: a temporary is released right after the last statement that mentions it.
//   Nothing after that statement can use the temporary, so the release only happens earlier.
// - `dup_drop_pairs`: `dup x` followed by `drop x`, with only statements in between that
//   cannot free or change any value (reads, plain assignments, calls without `inout`), is an
//   owner that nobody needed: both go.

/// Calls `f` with every local that `s` (and the statements nested in it) reads or writes.
fn stmt_locals(s: &Stmt, f: &mut dyn FnMut(LocalId)) {
    fn expr(e: &Expr, f: &mut dyn FnMut(LocalId)) {
        match e {
            Expr::Local(l) => f(*l),
            Expr::Unary(_, x) | Expr::IntToFloat(x) | Expr::Field(x, _, _) => expr(x, f),
            Expr::Binary(_, a, b) => {
                expr(a, f);
                expr(b, f);
            }
            Expr::Select(c, a, b) => {
                expr(c, f);
                expr(a, f);
                expr(b, f);
            }
            Expr::Pure(_, args) => args.iter().for_each(|a| expr(a, f)),
            Expr::Int(_) | Expr::Float(_) | Expr::Bool(_) | Expr::Char(_) | Expr::Str(_) => {}
        }
    }
    fn place(p: &Place, f: &mut dyn FnMut(LocalId)) {
        f(p.root);
        for s in &p.path {
            if let Step::Index(i, _) | Step::Key(i, _) = s {
                expr(i, f);
            }
        }
    }
    match &s.kind {
        StmtKind::Set(l, e) => {
            f(*l);
            expr(e, f);
        }
        StmtKind::Call { dst, args, .. } => {
            dst.iter().for_each(|d| f(*d));
            for a in args {
                match a {
                    Arg::Val(e) => expr(e, f),
                    Arg::InOut(p) => place(p, f),
                }
            }
        }
        StmtKind::Op { dst, args, .. } => {
            dst.iter().for_each(|d| f(*d));
            args.iter().for_each(|a| expr(a, f));
        }
        StmtKind::Store { place: p, value } => {
            place(p, f);
            expr(value, f);
        }
        StmtKind::Mutate { dst, place: p, args, .. } => {
            dst.iter().for_each(|d| f(*d));
            place(p, f);
            args.iter().for_each(|a| expr(a, f));
        }
        StmtKind::If { cond, then, els } => {
            expr(cond, f);
            then.iter().chain(els).for_each(|s| stmt_locals(s, f));
        }
        StmtKind::Loop { head, cond, body, step } => {
            expr(cond, f);
            head.iter().chain(body).chain(step).for_each(|s| stmt_locals(s, f));
        }
        StmtKind::ForEach { var, iter, body } => {
            f(*var);
            expr(iter, f);
            body.iter().for_each(|s| stmt_locals(s, f));
        }
        StmtKind::Return(Some(e)) => expr(e, f),
        StmtKind::Dup(l) | StmtKind::Drop(l) | StmtKind::Free(l) | StmtKind::Keep(l) => f(*l),
        StmtKind::Return(None) | StmtKind::Break | StmtKind::Continue => {}
    }
}

fn stmt_mentions(s: &Stmt, l: LocalId) -> bool {
    let mut found = false;
    stmt_locals(s, &mut |x| found |= x == l);
    found
}

/// The blocks nested in a statement.
fn blocks_mut(s: &mut Stmt) -> Vec<&mut Vec<Stmt>> {
    match &mut s.kind {
        StmtKind::If { then, els, .. } => vec![then, els],
        StmtKind::Loop { head, body, step, .. } => vec![head, body, step],
        StmtKind::ForEach { body, .. } => vec![body],
        _ => Vec::new(),
    }
}

/// Moves the release of each temporary up to right after the last statement that mentions it.
fn early_drops(ss: &mut Vec<Stmt>, temp: &[bool]) {
    for s in ss.iter_mut() {
        for b in blocks_mut(s) {
            early_drops(b, temp);
        }
    }
    for k in 0..ss.len() {
        let StmtKind::Drop(t) = ss[k].kind else { continue };
        if !temp[t.0 as usize] {
            continue;
        }
        let mut j = k;
        while j > 0
            && !stmt_mentions(&ss[j - 1], t)
            && !matches!(ss[j - 1].kind, StmtKind::Return(_) | StmtKind::Break | StmtKind::Continue)
        {
            j -= 1;
        }
        // a `dup` stays right after the statement that set its local (backends pair them up)
        while j < k && matches!(ss[j].kind, StmtKind::Dup(_)) {
            j += 1;
        }
        if j < k {
            let s = ss.remove(k);
            ss.insert(j, s);
        }
    }
}

/// True if `s` cannot free or change any value that already exists, nor write `l`: it only
/// reads, computes new values, assigns plain locals or calls functions without `inout`.
fn harmless(s: &Stmt, l: LocalId, managed: &[bool]) -> bool {
    let all = |ss: &[Stmt]| ss.iter().all(|s| harmless(s, l, managed));
    match &s.kind {
        StmtKind::Set(d, _) => *d != l && !managed[d.0 as usize],
        StmtKind::Op { dst, .. } => *dst != Some(l),
        StmtKind::Call { dst, args, .. } => *dst != Some(l) && args.iter().all(|a| matches!(a, Arg::Val(_))),
        StmtKind::Dup(d) => *d != l,
        StmtKind::If { then, els, .. } => all(then) && all(els),
        StmtKind::Loop { head, body, step, .. } => all(head) && all(body) && all(step),
        StmtKind::ForEach { var, body, .. } => *var != l && all(body),
        StmtKind::Store { .. }
        | StmtKind::Mutate { .. }
        | StmtKind::Drop(_)
        | StmtKind::Free(_)
        | StmtKind::Keep(_)
        | StmtKind::Return(_)
        | StmtKind::Break
        | StmtKind::Continue => false,
    }
}

/// True if every mention of `l` in `s` reads into it: an element (`arr_get(l, i)`), a field or
/// its length. Such a borrowed value may be a reference in a backend (Rust); other uses may not.
fn only_projected(s: &Stmt, l: LocalId) -> bool {
    fn count(e: &Expr, l: LocalId, n: &mut usize) {
        match e {
            Expr::Field(x, _, _) if matches!(**x, Expr::Local(y) if y == l) => *n += 1,
            Expr::Pure(super::PureFn::ArrLen, args) if matches!(args.as_slice(), [Expr::Local(y)] if *y == l) => *n += 1,
            Expr::Unary(_, x) | Expr::IntToFloat(x) | Expr::Field(x, _, _) => count(x, l, n),
            Expr::Binary(_, a, b) => {
                count(a, l, n);
                count(b, l, n);
            }
            Expr::Select(c, a, b) => {
                count(c, l, n);
                count(a, l, n);
                count(b, l, n);
            }
            Expr::Pure(_, args) => args.iter().for_each(|a| count(a, l, n)),
            _ => {}
        }
    }
    fn walk(s: &Stmt, l: LocalId, n: &mut usize) {
        let e = |x: &Expr, n: &mut usize| count(x, l, n);
        match &s.kind {
            StmtKind::Set(_, x) => e(x, n),
            StmtKind::Call { args, .. } => args.iter().for_each(|a| {
                if let Arg::Val(x) = a {
                    e(x, n)
                }
            }),
            StmtKind::Op { op, args, .. } => {
                for (k, x) in args.iter().enumerate() {
                    if k == 0 && *op == RtOp::ArrGet && matches!(x, Expr::Local(y) if *y == l) {
                        *n += 1;
                    } else {
                        e(x, n);
                    }
                }
            }
            StmtKind::If { cond, then, els } => {
                e(cond, n);
                then.iter().chain(els).for_each(|s| walk(s, l, n));
            }
            StmtKind::Loop { head, cond, body, step } => {
                e(cond, n);
                head.iter().chain(body).chain(step).for_each(|s| walk(s, l, n));
            }
            StmtKind::ForEach { iter, body, .. } => {
                e(iter, n);
                body.iter().for_each(|s| walk(s, l, n));
            }
            _ => {}
        }
    }
    let (mut all, mut projected) = (0, 0);
    stmt_locals(s, &mut |x| all += usize::from(x == l));
    walk(s, l, &mut projected);
    all == projected
}

/// Removes `dup x` ... `drop x` when everything in between is `harmless` and only reads
/// elements, fields or the length of `x`.
fn dup_drop_pairs(ss: &mut Vec<Stmt>, managed: &[bool]) {
    for s in ss.iter_mut() {
        for b in blocks_mut(s) {
            dup_drop_pairs(b, managed);
        }
    }
    // inner pairs first: once they are gone, the pairs around them can go too
    let mut a = ss.len();
    while a > 0 {
        a -= 1;
        let StmtKind::Dup(l) = ss[a].kind else { continue };
        let mut b = a + 1;
        while b < ss.len()
            && !matches!(ss[b].kind, StmtKind::Drop(d) if d == l)
            && harmless(&ss[b], l, managed)
            && only_projected(&ss[b], l)
        {
            b += 1;
        }
        if b < ss.len() && matches!(ss[b].kind, StmtKind::Drop(d) if d == l) {
            ss.remove(b);
            ss.remove(a);
        }
    }
}

struct Interner<'a> {
    strs: &'a mut Vec<String>,
    index: HashMap<String, StrId>,
}

impl<'a> Interner<'a> {
    fn new(strs: &'a mut Vec<String>) -> Self {
        let index = strs.iter().enumerate().map(|(i, s)| (s.clone(), StrId(i as u32))).collect();
        Interner { strs, index }
    }

    fn get(&self, id: StrId) -> &str {
        &self.strs[id.0 as usize]
    }

    fn intern(&mut self, s: String) -> StrId {
        if let Some(&id) = self.index.get(&s) {
            return id;
        }
        let id = StrId(self.strs.len() as u32);
        self.strs.push(s.clone());
        self.index.insert(s, id);
        id
    }
}

/// Folds, simplifies branches and drops dead statements in a block.
fn stmts(ss: &mut Vec<Stmt>, strs: &mut Interner) {
    let mut out = Vec::with_capacity(ss.len());
    for mut s in std::mem::take(ss) {
        let (mut keep, mut stop) = (true, false);
        match &mut s.kind {
            StmtKind::Set(_, e) => fold(e, strs),
            StmtKind::Call { args, .. } => {
                for a in args {
                    match a {
                        Arg::Val(e) => fold(e, strs),
                        Arg::InOut(p) => fold_place(p, strs),
                    }
                }
            }
            StmtKind::Op { op, args, dst } => {
                args.iter_mut().for_each(|a| fold(a, strs));
                if matches!(op, RtOp::Print | RtOp::PrintNoLine | RtOp::Format) {
                    text_parts(args, strs);
                }
                match fold_op(*op, args, *dst, strs) {
                    Folded::No => {}
                    Folded::Set(d, e) => s.kind = StmtKind::Set(d, e),
                    Folded::Gone => keep = false,
                }
            }
            StmtKind::Store { place, value } => {
                fold_place(place, strs);
                fold(value, strs);
            }
            StmtKind::Mutate { place, args, .. } => {
                fold_place(place, strs);
                args.iter_mut().for_each(|a| fold(a, strs));
            }
            StmtKind::If { cond, then, els } => {
                fold(cond, strs);
                stmts(then, strs);
                stmts(els, strs);
                if let Expr::Bool(c) = cond {
                    // a constant condition: keep the branch that runs, inline
                    let taken = if *c { std::mem::take(then) } else { std::mem::take(els) };
                    stop = taken.last().is_some_and(|l| matches!(l.kind, StmtKind::Return(_) | StmtKind::Break | StmtKind::Continue));
                    out.extend(taken);
                    keep = false;
                } else if then.is_empty() && els.is_empty() {
                    keep = false;
                }
            }
            StmtKind::Loop { head, cond, body, step } => {
                stmts(head, strs);
                fold(cond, strs);
                stmts(body, strs);
                stmts(step, strs);
                if matches!(cond, Expr::Bool(false)) {
                    // the loop never runs its body: only the first `head` happens
                    out.append(head);
                    keep = false;
                }
            }
            StmtKind::ForEach { iter, body, .. } => {
                fold(iter, strs);
                stmts(body, strs);
            }
            StmtKind::Break | StmtKind::Continue => stop = true,
            StmtKind::Dup(_) | StmtKind::Drop(_) | StmtKind::Free(_) | StmtKind::Keep(_) => {}
            StmtKind::Return(v) => {
                if let Some(e) = v {
                    fold(e, strs);
                }
                stop = true; // everything after a return is dead
            }
        }
        if keep {
            out.push(s);
        }
        if stop {
            break;
        }
    }
    *ss = out;
}

enum Folded {
    No,
    /// The operation becomes `dst = value`.
    Set(LocalId, Expr),
    /// The operation does nothing (a `check_step` that passes).
    Gone,
}

/// A runtime operation with constant operands, computed now. Integer `/` and `%` by a constant
/// other than 0 and -1 become the plain operators (they cannot fail).
fn fold_op(op: RtOp, args: &[Expr], dst: Option<LocalId>, strs: &mut Interner) -> Folded {
    if matches!(op, RtOp::Print | RtOp::ArrNew | RtOp::StructNew) {
        return Folded::No;
    }
    if let (RtOp::DivInt | RtOp::RemInt, Some(d), [a, Expr::Int(k)]) = (op, dst, args) {
        let a = Box::new(a.clone());
        return match (op, *k) {
            (_, 0) => Folded::No,
            // `MIN / -1` overflows: the runtime checks it
            (RtOp::DivInt, -1) => Folded::No,
            (_, -1) => Folded::Set(d, Expr::Int(0)),
            (RtOp::DivInt, _) => Folded::Set(d, Expr::Binary(BinOp::IDiv, a, Box::new(Expr::Int(*k)))),
            _ => Folded::Set(d, Expr::Binary(BinOp::IRem, a, Box::new(Expr::Int(*k)))),
        };
    }
    let Some(vals) = args.iter().map(|a| Val::of(a, strs.strs)).collect::<Option<Vec<_>>>() else { return Folded::No };
    match (eval::op(op, vals), dst) {
        (Ok(None), _) => Folded::Gone,
        (Ok(Some(v)), Some(d)) => match v.to_expr(&mut |t| strs.intern(t)) {
            Some(e) => Folded::Set(d, e),
            None => Folded::No,
        },
        _ => Folded::No,
    }
}

/// Calls `f` on every expression of the statements (not on loop iterators: they stay locals).
fn exprs_mut(ss: &mut [Stmt], f: &mut dyn FnMut(&mut Expr)) {
    fn place(p: &mut Place, f: &mut dyn FnMut(&mut Expr)) {
        for s in &mut p.path {
            if let Step::Index(i, _) | Step::Key(i, _) = s {
                f(i);
            }
        }
    }
    for s in ss {
        match &mut s.kind {
            StmtKind::Set(_, e) => f(e),
            StmtKind::Call { args, .. } => {
                for a in args {
                    match a {
                        Arg::Val(e) => f(e),
                        Arg::InOut(p) => place(p, f),
                    }
                }
            }
            StmtKind::Op { args, .. } => args.iter_mut().for_each(&mut *f),
            StmtKind::Store { place: p, value } => {
                place(p, f);
                f(value);
            }
            StmtKind::Mutate { place: p, args, .. } => {
                place(p, f);
                args.iter_mut().for_each(&mut *f);
            }
            StmtKind::If { cond, then, els } => {
                f(cond);
                exprs_mut(then, f);
                exprs_mut(els, f);
            }
            StmtKind::Loop { head, cond, body, step } => {
                exprs_mut(head, f);
                f(cond);
                exprs_mut(body, f);
                exprs_mut(step, f);
            }
            StmtKind::ForEach { body, .. } => exprs_mut(body, f),
            StmtKind::Return(Some(e)) => f(e),
            _ => {}
        }
    }
}

/// How often each local gets a value (a parameter once, from the caller).
fn writes(f: &Func) -> Vec<u32> {
    fn walk(ss: &[Stmt], n: &mut [u32]) {
        for s in ss {
            let mut w = |l: LocalId| n[l.0 as usize] += 1;
            match &s.kind {
                StmtKind::Set(l, _) | StmtKind::Free(l) | StmtKind::Keep(l) => w(*l),
                StmtKind::Call { dst, args, .. } => {
                    dst.iter().for_each(|d| w(*d));
                    for a in args {
                        if let Arg::InOut(p) = a {
                            w(p.root);
                        }
                    }
                }
                StmtKind::Op { dst, .. } => dst.iter().for_each(|d| w(*d)),
                StmtKind::Store { place, .. } => w(place.root),
                StmtKind::Mutate { dst, place, .. } => {
                    dst.iter().for_each(|d| w(*d));
                    w(place.root);
                }
                StmtKind::If { then, els, .. } => {
                    walk(then, n);
                    walk(els, n);
                }
                StmtKind::Loop { head, body, step, .. } => {
                    walk(head, n);
                    walk(body, n);
                    walk(step, n);
                }
                StmtKind::ForEach { var, body, .. } => {
                    w(*var);
                    walk(body, n);
                }
                _ => {}
            }
        }
    }
    let mut n = vec![0; f.locals.len()];
    n.iter_mut().take(f.params).for_each(|w| *w = 1);
    walk(&f.body, &mut n);
    n
}

/// Replaces the reads of every local that is set once, to a constant, by a statement of the
/// function's outermost block: nothing can read it before that statement (no use before a
/// value), and nothing changes it after.
fn propagate(f: &mut Func) -> bool {
    let n = writes(f);
    let mut value: Vec<Option<Expr>> = vec![None; f.locals.len()];
    for s in &f.body {
        if let StmtKind::Set(l, e) = &s.kind {
            if e.is_const() && n[l.0 as usize] == 1 {
                value[l.0 as usize] = Some(e.clone());
            }
        }
    }
    if value.iter().all(Option::is_none) {
        return false;
    }
    let mut changed = false;
    fn subst(e: &mut Expr, value: &[Option<Expr>], changed: &mut bool) {
        match e {
            Expr::Local(l) => {
                if let Some(v) = &value[l.0 as usize] {
                    *e = v.clone();
                    *changed = true;
                }
            }
            Expr::Unary(_, x) | Expr::IntToFloat(x) | Expr::Field(x, _, _) => subst(x, value, changed),
            Expr::Binary(_, a, b) => {
                subst(a, value, changed);
                subst(b, value, changed);
            }
            Expr::Select(c, a, b) => {
                subst(c, value, changed);
                subst(a, value, changed);
                subst(b, value, changed);
            }
            Expr::Pure(_, args) => args.iter_mut().for_each(|a| subst(a, value, changed)),
            _ => {}
        }
    }
    exprs_mut(&mut f.body, &mut |e| subst(e, &value, &mut changed));
    changed
}

/// Runs calls with constant arguments at compile time (see `eval::Calls`); a call that finishes
/// becomes its result, or disappears when its result is unused.
fn fold_calls(m: &mut Module) -> bool {
    fn each_call(ss: &[Stmt], f: &mut dyn FnMut(FuncId, &[Arg])) {
        for s in ss {
            match &s.kind {
                StmtKind::Call { func, args, .. } => f(*func, args),
                StmtKind::If { then, els, .. } => {
                    each_call(then, f);
                    each_call(els, f);
                }
                StmtKind::Loop { head, body, step, .. } => {
                    each_call(head, f);
                    each_call(body, f);
                    each_call(step, f);
                }
                StmtKind::ForEach { body, .. } => each_call(body, f),
                _ => {}
            }
        }
    }
    // first every result (the module stays as it is), then the changes, in the same order
    let mut results: Vec<Option<Option<Val>>> = Vec::new();
    {
        let mut run = eval::Calls::new(m);
        for f in &m.funcs {
            each_call(&f.body, &mut |func, args| {
                let vals: Option<Vec<Val>> = args
                    .iter()
                    .map(|a| match a {
                        Arg::Val(e) => Val::of(e, &m.strs),
                        Arg::InOut(_) => None,
                    })
                    .collect();
                results.push(vals.and_then(|v| run.call(func, v)));
            });
        }
    }
    if results.iter().all(Option::is_none) {
        return false;
    }
    fn apply(ss: &mut Vec<Stmt>, results: &[Option<Option<Val>>], k: &mut usize, strs: &mut Interner, changed: &mut bool) {
        let mut out = Vec::with_capacity(ss.len());
        for mut s in std::mem::take(ss) {
            match &mut s.kind {
                StmtKind::Call { dst, .. } => {
                    let r = &results[*k];
                    *k += 1;
                    match (r, *dst) {
                        (Some(Some(v)), Some(d)) => {
                            if let Some(e) = v.to_expr(&mut |t| strs.intern(t)) {
                                s.kind = StmtKind::Set(d, e);
                                *changed = true;
                            }
                        }
                        (Some(_), None) => {
                            // finished, no effects, result unused
                            *changed = true;
                            continue;
                        }
                        _ => {}
                    }
                }
                StmtKind::If { then, els, .. } => {
                    apply(then, results, k, strs, changed);
                    apply(els, results, k, strs, changed);
                }
                StmtKind::Loop { head, body, step, .. } => {
                    apply(head, results, k, strs, changed);
                    apply(body, results, k, strs, changed);
                    apply(step, results, k, strs, changed);
                }
                StmtKind::ForEach { body, .. } => apply(body, results, k, strs, changed),
                _ => {}
            }
            out.push(s);
        }
        *ss = out;
    }
    let (mut k, mut changed) = (0, false);
    let mut strs = Interner::new(&mut m.strs);
    for f in &mut m.funcs {
        apply(&mut f.body, &results, &mut k, &mut strs, &mut changed);
    }
    changed
}

/// Turns constant int and bool parts of a print/format into text and merges adjacent text.
fn text_parts(parts: &mut Vec<Expr>, strs: &mut Interner) {
    let mut out: Vec<Expr> = Vec::with_capacity(parts.len());
    for p in std::mem::take(parts) {
        let text = match &p {
            // (an int JavaScript cannot hold exactly stays: printing it stops the program there)
            Expr::Int(n) if eval::safe_int(*n) => Some(n.to_string()),
            Expr::Bool(b) => Some(b.to_string()),
            Expr::Char(c) => char::from_u32(*c).map(|c| c.to_string()),
            Expr::Str(id) => Some(strs.get(*id).to_string()),
            _ => None,
        };
        match (text, out.last()) {
            (Some(t), Some(Expr::Str(prev))) => {
                let merged = format!("{}{t}", strs.get(*prev));
                *out.last_mut().expect("checked above") = Expr::Str(strs.intern(merged));
            }
            (Some(t), _) => out.push(Expr::Str(strs.intern(t))),
            (None, _) => out.push(p),
        }
    }
    // `print("")` must still print an empty line, so never leave zero parts
    if out.is_empty() {
        out.push(Expr::Str(strs.intern(String::new())));
    }
    *parts = out;
}

fn fold_place(p: &mut Place, strs: &mut Interner) {
    for s in &mut p.path {
        if let Step::Index(i, _) | Step::Key(i, _) = s {
            fold(i, strs);
        }
    }
}

fn fold(e: &mut Expr, strs: &mut Interner) {
    match e {
        Expr::Unary(_, x) | Expr::IntToFloat(x) => fold(x, strs),
        Expr::Field(x, _, _) => {
            fold(x, strs);
            return;
        }
        Expr::Binary(_, a, b) => {
            fold(a, strs);
            fold(b, strs);
        }
        Expr::Select(c, a, b) => {
            fold(c, strs);
            fold(a, strs);
            fold(b, strs);
        }
        Expr::Pure(_, args) => {
            for a in args {
                fold(a, strs);
            }
        }
        _ => return,
    }
    if let Some(v) = constant(e, strs) {
        *e = v;
    }
}

/// The value of `e` if its operands are constants (one level; `fold` works bottom-up). Ints
/// follow `eval`: no overflow, and nothing beyond what JavaScript holds exactly.
fn constant(e: &Expr, strs: &mut Interner) -> Option<Expr> {
    use Expr::{Bool, Int};
    let safe = |r: Option<i64>| r.filter(|n| eval::safe_int(*n)).map(Int);
    let unsafe_operand = |x: &Expr| matches!(x, Int(n) if !eval::safe_int(*n));
    match e {
        Expr::Unary(_, x) if unsafe_operand(x) => return None,
        Expr::Binary(_, a, b) if unsafe_operand(a) || unsafe_operand(b) => return None,
        _ => {}
    }
    Some(match e {
        Expr::Unary(UnOp::INeg, x) => match **x {
            Int(n) => safe(n.checked_neg())?,
            _ => return None,
        },
        Expr::Unary(UnOp::Not, x) => match **x {
            Bool(b) => Bool(!b),
            _ => return None,
        },
        Expr::Select(c, a, b) => match **c {
            Bool(true) => (**a).clone(),
            Bool(false) => (**b).clone(),
            _ => return None,
        },
        Expr::Binary(op, a, b) => match (op, &**a, &**b) {
            (BinOp::IAdd, Int(x), Int(y)) => safe(x.checked_add(*y))?,
            (BinOp::ISub, Int(x), Int(y)) => safe(x.checked_sub(*y))?,
            (BinOp::IMul, Int(x), Int(y)) => safe(x.checked_mul(*y))?,
            // the IR guarantees a constant divisor other than 0 and -1 here
            (BinOp::IDiv, Int(x), Int(y)) => safe(x.checked_div(*y))?,
            (BinOp::IRem, Int(x), Int(y)) => safe(x.checked_rem(*y))?,
            (BinOp::IEq, Int(x), Int(y)) => Bool(x == y),
            (BinOp::INe, Int(x), Int(y)) => Bool(x != y),
            (BinOp::ILt, Int(x), Int(y)) => Bool(x < y),
            (BinOp::ILe, Int(x), Int(y)) => Bool(x <= y),
            (BinOp::IGt, Int(x), Int(y)) => Bool(x > y),
            (BinOp::IGe, Int(x), Int(y)) => Bool(x >= y),
            (BinOp::BEq, Bool(x), Bool(y)) => Bool(x == y),
            (BinOp::BNe, Bool(x), Bool(y)) => Bool(x != y),
            (BinOp::And, Bool(false), _) | (BinOp::And, _, Bool(false)) => Bool(false),
            (BinOp::And, Bool(true), x) | (BinOp::And, x, Bool(true)) => x.clone(),
            (BinOp::Or, Bool(true), _) | (BinOp::Or, _, Bool(true)) => Bool(true),
            (BinOp::Or, Bool(false), x) | (BinOp::Or, x, Bool(false)) => x.clone(),
            (BinOp::SEq, Expr::Str(x), Expr::Str(y)) => Bool(strs.get(*x) == strs.get(*y)),
            (BinOp::SNe, Expr::Str(x), Expr::Str(y)) => Bool(strs.get(*x) != strs.get(*y)),
            (BinOp::CEq, Expr::Char(x), Expr::Char(y)) => Bool(x == y),
            (BinOp::CNe, Expr::Char(x), Expr::Char(y)) => Bool(x != y),
            _ => return computed(e, strs),
        },
        _ => return computed(e, strs),
    })
}

/// Any other pure expression of constants, computed like the runtimes do (floats too, when the
/// result is a value every backend writes exactly).
fn computed(e: &Expr, strs: &mut Interner) -> Option<Expr> {
    let v = eval::expr(e, &mut |_| None, strs.strs).ok()?;
    v.to_expr(&mut |t| strs.intern(t))
}

/// Drops functions that `main` can never call, and renumbers the rest.
fn unused_fns(m: &mut Module) {
    let mut reached = vec![false; m.funcs.len()];
    let mut todo = vec![m.main];
    while let Some(id) = todo.pop() {
        if std::mem::replace(&mut reached[id.0 as usize], true) {
            continue;
        }
        calls(&m.funcs[id.0 as usize].body, &mut |f: FuncId| todo.push(f));
    }
    if reached.iter().all(|r| *r) {
        return;
    }
    let mut remap = vec![FuncId(0); m.funcs.len()];
    let mut kept = Vec::new();
    for (i, f) in std::mem::take(&mut m.funcs).into_iter().enumerate() {
        if reached[i] {
            remap[i] = FuncId(kept.len() as u32);
            kept.push(f);
        }
    }
    m.funcs = kept;
    m.main = remap[m.main.0 as usize];
    for f in &mut m.funcs {
        calls_mut(&mut f.body, &mut |id: &mut FuncId| *id = remap[id.0 as usize]);
    }
}

fn calls(ss: &[Stmt], f: &mut dyn FnMut(FuncId)) {
    for s in ss {
        match &s.kind {
            StmtKind::Call { func, .. } => f(*func),
            StmtKind::If { then, els, .. } => {
                calls(then, f);
                calls(els, f);
            }
            StmtKind::Loop { head, body, step, .. } => {
                calls(head, f);
                calls(body, f);
                calls(step, f);
            }
            StmtKind::ForEach { body, .. } => calls(body, f),
            _ => {}
        }
    }
}

fn calls_mut(ss: &mut [Stmt], f: &mut dyn FnMut(&mut FuncId)) {
    for s in ss {
        match &mut s.kind {
            StmtKind::Call { func, .. } => f(func),
            StmtKind::If { then, els, .. } => {
                calls_mut(then, f);
                calls_mut(els, f);
            }
            StmtKind::Loop { head, body, step, .. } => {
                calls_mut(head, f);
                calls_mut(body, f);
                calls_mut(step, f);
            }
            StmtKind::ForEach { body, .. } => calls_mut(body, f),
            _ => {}
        }
    }
}
