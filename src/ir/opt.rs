//! Simple, safe optimizations on the IR. Every pass keeps the program's output identical on
//! every backend; `NYRA_OPT=0` turns them all off (tests compare both ways).
//!
//! - `fold`: constant int/bool/string expressions; constant int and bool parts of `print` and
//!   string building become text. Floats are never folded: only the runtimes format and round them.
//! - `branches`: an `if` or loop with a constant condition keeps only what can run.
//! - `dead`: statements after `return`, and empty `if`s (conditions are pure).
//! - `unused_fns`: functions that `main` can never reach.

use std::collections::HashMap;

use super::{BinOp, Expr, FuncId, Module, RtOp, Stmt, StmtKind, StrId, UnOp};

pub fn optimize(m: &mut Module) {
    let mut strs = Interner::new(&mut m.strs);
    for f in &mut m.funcs {
        stmts(&mut f.body, &mut strs);
    }
    unused_fns(m);
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
            StmtKind::Call { args, .. } => args.iter_mut().for_each(|a| fold(a, strs)),
            StmtKind::Op { op, args, .. } => {
                args.iter_mut().for_each(|a| fold(a, strs));
                if matches!(op, RtOp::Print | RtOp::Format) {
                    text_parts(args, strs);
                }
            }
            StmtKind::If { cond, then, els } => {
                fold(cond, strs);
                stmts(then, strs);
                stmts(els, strs);
                if let Expr::Bool(c) = cond {
                    // a constant condition: keep the branch that runs, inline
                    let taken = if *c { std::mem::take(then) } else { std::mem::take(els) };
                    stop = taken.last().is_some_and(|l| matches!(l.kind, StmtKind::Return(_)));
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

/// Turns constant int and bool parts of a print/format into text and merges adjacent text.
fn text_parts(parts: &mut Vec<Expr>, strs: &mut Interner) {
    let mut out: Vec<Expr> = Vec::with_capacity(parts.len());
    for p in std::mem::take(parts) {
        let text = match &p {
            Expr::Int(n) => Some(n.to_string()),
            Expr::Bool(b) => Some(b.to_string()),
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

fn fold(e: &mut Expr, strs: &mut Interner) {
    match e {
        Expr::Unary(_, x) | Expr::IntToFloat(x) => fold(x, strs),
        Expr::Binary(_, a, b) => {
            fold(a, strs);
            fold(b, strs);
        }
        Expr::Select(c, a, b) => {
            fold(c, strs);
            fold(a, strs);
            fold(b, strs);
        }
        _ => return,
    }
    if let Some(v) = constant(e, strs) {
        *e = v;
    }
}

/// The value of `e` if its operands are constants (one level; `fold` works bottom-up).
fn constant(e: &Expr, strs: &mut Interner) -> Option<Expr> {
    use Expr::{Bool, Int};
    Some(match e {
        Expr::Unary(UnOp::INeg, x) => match **x {
            Int(n) => Int(n.wrapping_neg()),
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
            (BinOp::IAdd, Int(x), Int(y)) => Int(x.wrapping_add(*y)),
            (BinOp::ISub, Int(x), Int(y)) => Int(x.wrapping_sub(*y)),
            (BinOp::IMul, Int(x), Int(y)) => Int(x.wrapping_mul(*y)),
            // the IR guarantees a constant divisor other than 0 and -1 here
            (BinOp::IDiv, Int(x), Int(y)) => Int(x.wrapping_div(*y)),
            (BinOp::IRem, Int(x), Int(y)) => Int(x.wrapping_rem(*y)),
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
            _ => return None,
        },
        _ => return None,
    })
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
            _ => {}
        }
    }
}
