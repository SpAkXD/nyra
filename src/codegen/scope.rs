//! Facts about a function's statements that the block-structured backends (Python, TypeScript,
//! Rust, Go) share:
//!
//! - which loops count from `a` to `b` (they become `for i in range(a, b)`, `for i in a..b`, ...);
//! - how often each local is read and written;
//! - where each local can be declared: at its first assignment (`let x = ...`), or without a
//!   value before the first statement of the innermost block that mentions it everywhere.
//!
//! The IR declares every local function-wide; these backends declare them where a person would.
//! A local that only one block mentions is assigned before it is read in every round of a loop
//! (lowering makes a new local for every declaration), so declaring it inside is safe.

use std::collections::HashMap;

use crate::ir::{Arg, BinOp, Expr, Func, LocalId, Place, Step, Stmt, StmtKind};

/// `i = a`, optionally `t = b`, then `loop { while i < b-or-t; body; step: i = i + 1 }`: a loop
/// `for i in a..b`. `len` is the number of statements of the block it covers.
pub struct RangeFor<'a> {
    pub var: LocalId,
    pub start: &'a Expr,
    /// The end bound (the temporary's value when the bound was stored in one only the loop reads).
    pub end: &'a Expr,
    pub body: &'a [Stmt],
    /// The statement that stores the bound in a temporary, when the bound is not inlined:
    /// printed before the loop.
    pub pre: Option<&'a Stmt>,
    pub len: usize,
}

/// Recognizes a counted loop starting at `ss[k]`. Only lowering's `for i in a..b` has this shape:
/// the variable is changed by the step alone, and the bound is evaluated once, before the loop.
pub fn range_for<'a>(ss: &'a [Stmt], k: usize, info: &Info) -> Option<RangeFor<'a>> {
    let StmtKind::Set(var, start) = &ss.get(k)?.kind else { return None };
    let mut next = k + 1;
    let mut temp: Option<(LocalId, &Expr)> = None;
    if let Some(Stmt { kind: StmtKind::Set(t, value), .. }) = ss.get(next) {
        temp = Some((*t, value));
        next += 1;
    }
    let StmtKind::Loop { head, cond, body, step } = &ss.get(next)?.kind else { return None };
    if !head.is_empty() {
        return None;
    }
    let Expr::Binary(BinOp::ILt, lhs, bound) = cond else { return None };
    if !matches!(**lhs, Expr::Local(l) if l == *var) {
        return None;
    }
    let [Stmt { kind: StmtKind::Set(sv, Expr::Binary(BinOp::IAdd, x, one)), .. }] = step.as_slice() else { return None };
    if sv != var || !matches!(**x, Expr::Local(l) if l == *var) || !matches!(**one, Expr::Int(1)) {
        return None;
    }
    // the variable changes only in the step, and only the loop reads it (condition, step, body)
    let v = var.0 as usize;
    if info.writes[v] != 2 || writes_in(body, *var) || info.reads[v] != 2 + reads_in(body, *var) {
        return None;
    }
    let mut pre = None;
    let end: &Expr = match (temp, &**bound) {
        (Some((t, value)), Expr::Local(b)) if t == *b => {
            let ti = t.0 as usize;
            if info.names[ti] || mentions(value, *var) {
                return None;
            }
            // a compiler temporary that only the condition reads: its value is the bound, when
            // the body cannot change what that value reads; else it stays, before the loop
            if info.reads[ti] != 1 || info.writes[ti] != 1 || writes_any(body, value) {
                pre = Some(&ss[k + 1]);
                bound
            } else {
                value
            }
        }
        (Some(_), _) => return None,
        (None, b) if b.is_const() => b,
        _ => return None,
    };
    if mentions(end, *var) || mentions(start, *var) {
        return None;
    }
    Some(RangeFor { var: *var, start, end, body, pre, len: next + 1 - k })
}

/// Per-function counts and declaration points.
pub struct Info {
    /// Reads of each local (in expressions, places, `Dup`).
    pub reads: Vec<u32>,
    /// Assignments of each local (`Set`, a destination, `free`).
    pub writes: Vec<u32>,
    /// The reads that are only a `Dup` (a mark, not a use of the value).
    pub dups: Vec<u32>,
    /// Locals that are changed in place (the root of a `Store`, `Mutate` or `inout` argument).
    pub changed: Vec<bool>,
    /// Locals with a Nyra name (the rest are compiler temporaries).
    pub names: Vec<bool>,
    /// Locals declared by their loop (`for x in xs`, a counted `for`).
    pub loop_var: Vec<bool>,
    /// Statement → the local it declares with its value (`let x = ...`).
    declares: HashMap<*const Stmt, LocalId>,
    /// Statement → the locals to declare (without a value) right before it.
    before: HashMap<*const Stmt, Vec<LocalId>>,
}

impl Info {
    pub fn new(f: &Func) -> Info {
        let n = f.locals.len();
        let mut info = Info {
            reads: vec![0; n],
            writes: vec![0; n],
            dups: vec![0; n],
            changed: vec![false; n],
            names: f.locals.iter().map(|l| l.name.is_some()).collect(),
            loop_var: vec![false; n],
            declares: HashMap::new(),
            before: HashMap::new(),
        };
        reads(&f.body, &mut |l| info.reads[l.0 as usize] += 1);
        count_dups(&f.body, &mut info.dups);
        writes(&f.body, &mut info);
        let mut w = Walk { info: &mut info, parent: vec![0], lca: vec![None; n] };
        w.block(&f.body, 0);
        let lca = std::mem::take(&mut w.lca);
        let mut done = vec![false; n];
        done[..f.params].fill(true);
        let mut d = Declare { info: &mut info, lca: &lca, done, scopes: 0 };
        d.block(&f.body, 0);
        info
    }

    /// The local `s` declares with its value, if any.
    pub fn declares(&self, s: &Stmt) -> Option<LocalId> {
        self.declares.get(&(s as *const Stmt)).copied()
    }

    /// The locals to declare without a value right before `s`.
    pub fn before(&self, s: &Stmt) -> &[LocalId] {
        self.before.get(&(s as *const Stmt)).map_or(&[], |v| v.as_slice())
    }

    /// True if nothing uses the local's value (it is only written, or only marked by `Dup`).
    pub fn unread(&self, l: LocalId) -> bool {
        self.reads[l.0 as usize] == self.dups[l.0 as usize]
    }
}

/// Calls `f` on every read of a local in `ss` and below: in expressions, place roots and
/// indexes (a change in place reads the old value), `Dup`.
pub fn reads(ss: &[Stmt], f: &mut dyn FnMut(LocalId)) {
    for s in ss {
        match &s.kind {
            StmtKind::Set(_, e) => each_local(e, f),
            StmtKind::Call { args, .. } => {
                for a in args {
                    match a {
                        Arg::Val(e) => each_local(e, f),
                        Arg::InOut(p) => place_locals(p, f),
                    }
                }
            }
            StmtKind::Op { args, .. } => args.iter().for_each(|a| each_local(a, f)),
            StmtKind::Store { place, value } => {
                place_locals(place, f);
                each_local(value, f);
            }
            StmtKind::Mutate { place, args, .. } => {
                place_locals(place, f);
                args.iter().for_each(|a| each_local(a, f));
            }
            StmtKind::If { cond, then, els } => {
                each_local(cond, f);
                reads(then, f);
                reads(els, f);
            }
            StmtKind::Loop { head, cond, body, step } => {
                reads(head, f);
                each_local(cond, f);
                reads(body, f);
                reads(step, f);
            }
            StmtKind::ForEach { iter, body, .. } => {
                each_local(iter, f);
                reads(body, f);
            }
            StmtKind::Return(Some(e)) => each_local(e, f),
            StmtKind::Dup(l) => f(*l),
            StmtKind::Drop(_)
            | StmtKind::Free(_)
            | StmtKind::Keep(_)
            | StmtKind::Return(None)
            | StmtKind::Break
            | StmtKind::Continue => {}
        }
    }
}

fn count_dups(ss: &[Stmt], dups: &mut [u32]) {
    for s in ss {
        if let StmtKind::Dup(l) = s.kind {
            dups[l.0 as usize] += 1;
        }
        for b in children(s) {
            count_dups(b, dups);
        }
    }
}

fn writes(ss: &[Stmt], info: &mut Info) {
    for s in ss {
        match &s.kind {
            StmtKind::Set(l, _) | StmtKind::Free(l) => info.writes[l.0 as usize] += 1,
            StmtKind::Call { dst, args, .. } => {
                for a in args {
                    if let Arg::InOut(p) = a {
                        info.changed[p.root.0 as usize] = true;
                    }
                }
                if let Some(d) = dst {
                    info.writes[d.0 as usize] += 1;
                }
            }
            StmtKind::Op { dst: Some(d), .. } => info.writes[d.0 as usize] += 1,
            StmtKind::Store { place, .. } => info.changed[place.root.0 as usize] = true,
            StmtKind::Mutate { dst, place, .. } => {
                info.changed[place.root.0 as usize] = true;
                if let Some(d) = dst {
                    info.writes[d.0 as usize] += 1;
                }
            }
            StmtKind::ForEach { var, .. } => {
                info.writes[var.0 as usize] += 1;
                info.loop_var[var.0 as usize] = true;
            }
            _ => {}
        }
        for b in children(s) {
            writes(b, info);
        }
    }
}

fn place_locals(p: &Place, f: &mut dyn FnMut(LocalId)) {
    f(p.root);
    for s in &p.path {
        if let Step::Index(i, _) = s {
            each_local(i, f);
        }
    }
}

/// Calls `f` on every local an expression reads.
pub fn each_local(e: &Expr, f: &mut dyn FnMut(LocalId)) {
    match e {
        Expr::Local(l) => f(*l),
        Expr::Unary(_, x) | Expr::IntToFloat(x) | Expr::Field(x, _, _) => each_local(x, f),
        Expr::Binary(_, a, b) => {
            each_local(a, f);
            each_local(b, f);
        }
        Expr::Select(c, a, b) => {
            each_local(c, f);
            each_local(a, f);
            each_local(b, f);
        }
        Expr::Pure(_, args) => args.iter().for_each(|a| each_local(a, f)),
        Expr::Int(_) | Expr::Float(_) | Expr::Bool(_) | Expr::Char(_) | Expr::Str(_) => {}
    }
}

/// True if `e` reads `l`.
pub fn mentions(e: &Expr, l: LocalId) -> bool {
    let mut found = false;
    each_local(e, &mut |x| found |= x == l);
    found
}

/// Reads of `l` in `ss` and below.
fn reads_in(ss: &[Stmt], l: LocalId) -> u32 {
    let mut n = 0;
    reads(ss, &mut |x| n += (x == l) as u32);
    n
}

/// Calls `f` on every local a statement (not the blocks inside it) reads or writes.
fn stmt_locals(s: &Stmt, f: &mut dyn FnMut(LocalId)) {
    match &s.kind {
        StmtKind::Set(l, e) => {
            each_local(e, f);
            f(*l);
        }
        StmtKind::Call { dst, args, .. } => {
            for a in args {
                match a {
                    Arg::Val(e) => each_local(e, f),
                    Arg::InOut(p) => place_locals(p, f),
                }
            }
            if let Some(d) = dst {
                f(*d);
            }
        }
        StmtKind::Op { dst, args, .. } => {
            args.iter().for_each(|a| each_local(a, f));
            if let Some(d) = dst {
                f(*d);
            }
        }
        StmtKind::Store { place, value } => {
            place_locals(place, f);
            each_local(value, f);
        }
        StmtKind::Mutate { dst, place, args, .. } => {
            place_locals(place, f);
            args.iter().for_each(|a| each_local(a, f));
            if let Some(d) = dst {
                f(*d);
            }
        }
        StmtKind::If { cond, .. } => each_local(cond, f),
        // a loop's condition belongs to the loop's own block
        StmtKind::Loop { .. } => {}
        StmtKind::ForEach { iter, .. } => each_local(iter, f),
        StmtKind::Return(Some(e)) => each_local(e, f),
        StmtKind::Dup(l) | StmtKind::Drop(l) | StmtKind::Free(l) | StmtKind::Keep(l) => f(*l),
        StmtKind::Return(None) | StmtKind::Break | StmtKind::Continue => {}
    }
}

/// True if a statement in `ss` (or below) assigns or changes `l`.
pub fn writes_in(ss: &[Stmt], l: LocalId) -> bool {
    ss.iter().any(|s| {
        let direct = match &s.kind {
            StmtKind::Set(d, _) | StmtKind::Free(d) => *d == l,
            StmtKind::Call { dst, args, .. } => *dst == Some(l) || args.iter().any(|a| matches!(a, Arg::InOut(p) if p.root == l)),
            StmtKind::Op { dst, .. } => *dst == Some(l),
            StmtKind::Store { place, .. } => place.root == l,
            StmtKind::Mutate { dst, place, .. } => *dst == Some(l) || place.root == l,
            StmtKind::ForEach { var, .. } => *var == l,
            _ => false,
        };
        direct || children(s).iter().any(|b| writes_in(b, l))
    })
}

/// True if `ss` changes a local that `e` reads.
fn writes_any(ss: &[Stmt], e: &Expr) -> bool {
    let mut found = false;
    each_local(e, &mut |l| found |= writes_in(ss, l));
    found
}

/// The blocks directly inside a statement.
fn children(s: &Stmt) -> Vec<&[Stmt]> {
    match &s.kind {
        StmtKind::If { then, els, .. } => vec![then, els],
        StmtKind::Loop { head, body, step, .. } => vec![head, body, step],
        StmtKind::ForEach { body, .. } => vec![body],
        _ => Vec::new(),
    }
}

/// True for the `else` block of an `else if` chain: it is printed as part of its `if`.
pub fn else_if(els: &[Stmt]) -> bool {
    matches!(els, [s] if matches!(s.kind, StmtKind::If { .. }))
}

/// First pass: the innermost block (scope) that contains every mention of each local.
/// Scopes are numbered in walking order; `Declare` walks the same way.
struct Walk<'a> {
    info: &'a mut Info,
    /// Parent of each scope (scope 0 is the function body).
    parent: Vec<usize>,
    lca: Vec<Option<usize>>,
}

impl Walk<'_> {
    fn new_scope(&mut self, parent: usize) -> usize {
        self.parent.push(parent);
        self.parent.len() - 1
    }

    fn depth(&self, mut s: usize) -> usize {
        let mut d = 0;
        while s != 0 {
            s = self.parent[s];
            d += 1;
        }
        d
    }

    fn mention(&mut self, l: LocalId, scope: usize) {
        let i = l.0 as usize;
        if self.info.loop_var[i] {
            return;
        }
        self.lca[i] = Some(match self.lca[i] {
            None => scope,
            Some(mut a) => {
                let mut b = scope;
                let (mut da, mut db) = (self.depth(a), self.depth(b));
                while da > db {
                    a = self.parent[a];
                    da -= 1;
                }
                while db > da {
                    b = self.parent[b];
                    db -= 1;
                }
                while a != b {
                    a = self.parent[a];
                    b = self.parent[b];
                }
                a
            }
        });
    }

    fn mention_expr(&mut self, e: &Expr, scope: usize) {
        let mut ls = Vec::new();
        each_local(e, &mut |l| ls.push(l));
        for l in ls {
            self.mention(l, scope);
        }
    }

    fn block(&mut self, ss: &[Stmt], scope: usize) {
        let mut k = 0;
        while k < ss.len() {
            if let Some(r) = range_for(ss, k, self.info) {
                self.info.loop_var[r.var.0 as usize] = true;
                if let Some(p) = r.pre {
                    self.stmt(p, scope);
                }
                self.mention_expr(r.start, scope);
                self.mention_expr(r.end, scope);
                let inner = self.new_scope(scope);
                self.block(r.body, inner);
                k += r.len;
                continue;
            }
            self.stmt(&ss[k], scope);
            k += 1;
        }
    }

    fn stmt(&mut self, s: &Stmt, scope: usize) {
        let mut ls = Vec::new();
        stmt_locals(s, &mut |l| ls.push(l));
        for l in ls {
            self.mention(l, scope);
        }
        match &s.kind {
            StmtKind::If { then, els, .. } => {
                let t = self.new_scope(scope);
                self.block(then, t);
                if else_if(els) {
                    // `else if`: its condition is printed before any block opens
                    self.stmt(&els[0], scope);
                } else {
                    let e = self.new_scope(scope);
                    self.block(els, e);
                }
            }
            StmtKind::Loop { head, cond, body, step } => {
                // head, condition, body and step share the loop's block
                let inner = self.new_scope(scope);
                self.mention_expr(cond, inner);
                self.block(head, inner);
                self.block(body, inner);
                self.block(step, inner);
            }
            StmtKind::ForEach { body, .. } => {
                let inner = self.new_scope(scope);
                self.block(body, inner);
            }
            _ => {}
        }
    }
}

/// Second pass: walks the scopes in the same order and declares each local at the first
/// statement of its scope that mentions it.
struct Declare<'a> {
    info: &'a mut Info,
    lca: &'a [Option<usize>],
    done: Vec<bool>,
    scopes: usize,
}

impl Declare<'_> {
    fn next_scope(&mut self) -> usize {
        self.scopes += 1;
        self.scopes
    }

    /// Declares, at statement `s` of `scope`, the locals of that scope that `s` mentions first;
    /// with their value when `s` assigns one of them (`fresh`).
    fn declare_at(&mut self, s: &Stmt, mentioned: &[LocalId], scope: usize, fresh: bool) {
        for &l in mentioned {
            let i = l.0 as usize;
            if self.done[i] || self.info.loop_var[i] || self.lca[i] != Some(scope) {
                continue;
            }
            self.done[i] = true;
            if fresh && assigns_fresh(s, l) {
                self.info.declares.insert(s as *const Stmt, l);
            } else {
                self.info.before.entry(s as *const Stmt).or_default().push(l);
            }
        }
    }

    fn block(&mut self, ss: &[Stmt], scope: usize) {
        let mut k = 0;
        while k < ss.len() {
            if let Some(r) = range_for(ss, k, self.info) {
                if let Some(p) = r.pre {
                    let mut ls = Vec::new();
                    all_locals(p, &mut |l| ls.push(l));
                    self.declare_at(p, &ls, scope, true);
                }
                // locals of the bounds: declared before the loop (the statement at `k`)
                let mut ls = Vec::new();
                each_local(r.start, &mut |l| ls.push(l));
                each_local(r.end, &mut |l| ls.push(l));
                self.declare_at(&ss[k], &ls, scope, false);
                let inner = self.next_scope();
                self.block(r.body, inner);
                k += r.len;
                continue;
            }
            let s = &ss[k];
            let mut ls = Vec::new();
            all_locals(s, &mut |l| ls.push(l));
            self.declare_at(s, &ls, scope, true);
            self.nested(s);
            k += 1;
        }
    }

    fn nested(&mut self, s: &Stmt) {
        match &s.kind {
            StmtKind::If { then, els, .. } => {
                let t = self.next_scope();
                self.block(then, t);
                if else_if(els) {
                    self.nested(&els[0]);
                } else {
                    let e = self.next_scope();
                    self.block(els, e);
                }
            }
            StmtKind::Loop { head, body, step, .. } => {
                let inner = self.next_scope();
                self.block(head, inner);
                self.block(body, inner);
                self.block(step, inner);
            }
            StmtKind::ForEach { body, .. } => {
                let inner = self.next_scope();
                self.block(body, inner);
            }
            _ => {}
        }
    }
}

/// Every local a statement and the blocks inside it mention (a loop's condition included).
fn all_locals(s: &Stmt, f: &mut dyn FnMut(LocalId)) {
    stmt_locals(s, f);
    if let StmtKind::Loop { cond, .. } = &s.kind {
        each_local(cond, f);
    }
    for b in children(s) {
        for x in b {
            all_locals(x, f);
        }
    }
}

/// True if `s` gives `l` a value without reading it: it can declare `l` with that value.
fn assigns_fresh(s: &Stmt, l: LocalId) -> bool {
    let mut n = 0;
    stmt_locals(s, &mut |x| n += (x == l) as u32);
    let once = n == 1;
    match &s.kind {
        StmtKind::Set(d, _) => *d == l && once,
        StmtKind::Call { dst, .. } | StmtKind::Op { dst, .. } | StmtKind::Mutate { dst, .. } => *dst == Some(l) && once,
        _ => false,
    }
}
