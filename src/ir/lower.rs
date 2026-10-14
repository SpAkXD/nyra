//! Typed AST → IR. Effects become statements in left-to-right order; values become pure
//! expressions. Lowering also places the reference counting for managed values (strings,
//! arrays):
//!
//! - A local variable owns its value. Parameters are borrowed: the caller keeps them alive.
//! - A call or operation that makes a new value gives an *owned* temporary. It is moved into a
//!   variable when it is stored, and otherwise released (`Drop`) at the end of its statement.
//! - Storing a borrowed value (a variable, a parameter, an element) adds an owner (`Dup`).
//!   Operations that store into an array (`[a, b]`, `push`, element assignment) add the owner
//!   themselves, so their operands stay borrowed.
//! - Leaving a block releases the variables it declared; `ret`, `break` and `continue` release
//!   the blocks they leave. `ret x` of a local moves it out.
//!
//! Pure expressions are evaluated where they are used. When a later operand can change a
//! variable (`xs.pop()`), the operands before it are first copied into temporaries, so every
//! operand still sees the program state of its own turn (left-to-right evaluation).

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use super::{
    visit_locals, Arg, BinOp, Expr, Func, FuncId, Local, LocalId, Module, Place, PureFn, RtOp, StdFn, Step, Stmt, StmtKind, StrId,
    StructInfo, Structs, Ty, UnOp,
};
use crate::ast::{self, Span, Type};
use crate::helpers::H;

mod bounds;
mod lambda;

use bounds::{Iv, MODES};

/// Lowers a type-checked program. Fails for features the backends do not support yet.
pub fn lower(prog: &ast::Program) -> Result<Module, String> {
    // the functions that change script variables: a call of one changes variables
    let writers: HashSet<String> =
        prog.globals.uses.iter().filter(|(_, us)| us.iter().any(|u| u.inout)).map(|(f, _)| f.clone()).collect();
    WRITERS.with(|w| *w.borrow_mut() = writers);
    let ids: HashMap<String, FuncId> = prog.funcs.iter().enumerate().map(|(i, f)| (f.name.clone(), FuncId(i as u32))).collect();
    let structs = struct_table(prog);
    let mut strs = Strs::default();
    let mut funcs = Vec::new();
    for f in &prog.funcs {
        let mut l = Lower::new(&ids, &mut strs, &structs, &prog.globals);
        let mut func = l.func(f);
        if let Some(what) = l.unsupported {
            return Err(what);
        }
        prune_temps(&mut func);
        funcs.push(func);
    }
    Ok(Module { funcs, strs: strs.list, main: ids["main"], structs })
}

/// The program's structs, each after the structs it contains by value.
fn struct_table(prog: &ast::Program) -> Structs {
    let defs: HashMap<u32, &ast::StructDef> = prog
        .structs
        .iter()
        .filter_map(|s| match Type::structure(&s.name) {
            Type::Struct(id) => Some((id, s)),
            _ => None,
        })
        .collect();
    let mut table = Structs::default();
    // depth-first: a struct is added after the structs of its by-value fields (no cycles: E0222)
    fn add(id: u32, defs: &HashMap<u32, &ast::StructDef>, table: &mut Structs) {
        if table.0.iter().any(|(i, _)| *i == id) {
            return;
        }
        let Some(def) = defs.get(&id) else { return };
        for f in &def.fields {
            if let Type::Struct(inner) = f.ty {
                if inner != id {
                    add(inner, defs, table);
                }
            }
        }
        let fields: Vec<(String, Ty)> = def.fields.iter().map(|f| (f.name.clone(), f.ty)).collect();
        let managed = fields.iter().any(|(_, t)| table.managed(*t));
        let tuple = Ty::Struct(id).is_tuple();
        let option = Ty::Struct(id).is_option();
        table.0.push((
            id,
            StructInfo {
                name: def.name.clone(),
                fields,
                managed,
                tuple,
                option,
                variants: def.variants.clone(),
                payloads: def.payloads.clone(),
            },
        ));
    }
    let mut ids: Vec<u32> = defs.keys().copied().collect();
    ids.sort_unstable();
    for id in ids {
        add(id, &defs, &mut table);
    }
    table
}

#[derive(Default)]
struct Strs {
    list: Vec<String>,
    index: HashMap<String, StrId>,
}

impl Strs {
    fn intern(&mut self, s: &str) -> StrId {
        if let Some(&id) = self.index.get(s) {
            return id;
        }
        let id = StrId(self.list.len() as u32);
        self.list.push(s.to_string());
        self.index.insert(s.to_string(), id);
        id
    }
}

/// True if `e` reads one of `locals`.
fn reads_any(e: &Expr, locals: &[LocalId]) -> bool {
    if locals.is_empty() {
        return false;
    }
    let mut found = false;
    let mut s = [Stmt { kind: StmtKind::Return(Some(e.clone())), span: Span { line: 0, col: 0 } }];
    visit_locals(&mut s, &mut |l| found |= locals.contains(l));
    found
}

/// An int constant, also through negation (`-1`).
fn const_int(e: &Expr) -> Option<i64> {
    match e {
        Expr::Int(n) => Some(*n),
        Expr::Unary(UnOp::INeg, x) => const_int(x).map(i64::wrapping_neg),
        _ => None,
    }
}

/// A counter starts within this, so it stays within `COUNTER_MAX` (see `counters`).
const COUNTER_START: i128 = 1 << 51;
const COUNTER_MAX: i128 = (1 << 53) - 2;

thread_local! {
    /// The functions of the program being lowered that change script variables.
    static WRITERS: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
}

/// True if evaluating `e` can change a variable: a method that changes its receiver, or a
/// call with an `inout` argument or of a function that changes script variables.
fn mutates(e: &ast::Expr) -> bool {
    use ast::ExprKind as K;
    match &e.kind {
        K::Method(r, name, args) => {
            matches!(name.as_str(), "push" | "pop" | "insert" | "remove" | "swap" | "sort" | "reverse" | "sort_by" | "set")
                || mutates(r)
                || args.iter().any(mutates)
        }
        K::Call(name, args) => {
            WRITERS.with(|w| w.borrow().contains(name)) || args.iter().any(|a| matches!(a.kind, K::Inout(_)) || mutates(a))
        }
        K::Unary(_, x) | K::Field(x, _) | K::Labeled(_, x) | K::Inout(x) | K::Lambda(_, x) | K::Fmt(x, _) => mutates(x),
        K::In(a, b) => mutates(a) || mutates(b),
        K::Slice(b, lo, hi) => mutates(b) || lo.as_ref().is_some_and(|x| mutates(x)) || hi.as_ref().is_some_and(|x| mutates(x)),
        K::Comprehension(c) => {
            let src = match &c.src {
                ast::CompSrc::Each(x) => mutates(x),
                ast::CompSrc::Range(a, b, k) => mutates(a) || mutates(b) || k.as_ref().is_some_and(mutates),
            };
            src || mutates(&c.elem) || c.cond.as_ref().is_some_and(mutates)
        }
        K::Binary(_, a, b) | K::Index(a, b) => mutates(a) || mutates(b),
        K::If(c, a, b) => mutates(c) || mutates(a) || mutates(b),
        K::Bind(_, v, body) => mutates(v) || mutates(body),
        K::Match(..) => true,
        K::Array(xs) | K::Tuple(xs) => xs.iter().any(mutates),
        K::Some(x) => mutates(x),
        K::Coalesce(a, b) => mutates(a) || mutates(b),
        K::MapLit(pairs) => pairs.iter().any(|(k, v)| mutates(k) || mutates(v)),
        K::Interp(parts) => parts.iter().any(|p| matches!(p, ast::InterpPart::Expr(x) if mutates(x))),
        K::Int(_) | K::Float(_) | K::Bool(_) | K::Str(_) | K::Char(_) | K::Var(_) | K::None => false,
    }
}

/// Calls `f` on `e` and every expression inside it.
fn each_expr(e: &ast::Expr, f: &mut dyn FnMut(&ast::Expr)) {
    use ast::ExprKind as K;
    f(e);
    match &e.kind {
        K::Unary(_, x) | K::Field(x, _) | K::Labeled(_, x) | K::Inout(x) | K::Lambda(_, x) | K::Fmt(x, _) => each_expr(x, f),
        K::Binary(_, a, b) | K::Index(a, b) | K::In(a, b) => {
            each_expr(a, f);
            each_expr(b, f);
        }
        K::Slice(b, lo, hi) => {
            each_expr(b, f);
            lo.iter().chain(hi.iter()).for_each(|x| each_expr(x, f));
        }
        K::If(c, a, b) => {
            each_expr(c, f);
            each_expr(a, f);
            each_expr(b, f);
        }
        K::Bind(_, v, body) => {
            each_expr(v, f);
            each_expr(body, f);
        }
        K::Match(scrut, arms) => {
            each_expr(scrut, f);
            for arm in arms {
                for st in &arm.body {
                    if let ast::StmtKind::Expr(x) = &st.kind {
                        each_expr(x, f);
                    }
                }
            }
        }
        K::Call(_, xs) | K::Array(xs) | K::Tuple(xs) => xs.iter().for_each(|x| each_expr(x, f)),
        K::Some(x) => each_expr(x, f),
        K::Coalesce(a, b) => {
            each_expr(a, f);
            each_expr(b, f);
        }
        K::Method(r, _, xs) => {
            each_expr(r, f);
            xs.iter().for_each(|x| each_expr(x, f));
        }
        K::MapLit(pairs) => pairs.iter().for_each(|(k, v)| {
            each_expr(k, f);
            each_expr(v, f);
        }),
        K::Interp(parts) => parts.iter().for_each(|p| {
            if let ast::InterpPart::Expr(x) = p {
                each_expr(x, f)
            }
        }),
        K::Comprehension(c) => {
            match &c.src {
                ast::CompSrc::Each(x) => each_expr(x, f),
                ast::CompSrc::Range(a, b, k) => {
                    each_expr(a, f);
                    each_expr(b, f);
                    if let Some(k) = k {
                        each_expr(k, f);
                    }
                }
            }
            each_expr(&c.elem, f);
            if let Some(x) = &c.cond {
                each_expr(x, f);
            }
        }
        K::Int(_) | K::Float(_) | K::Bool(_) | K::Str(_) | K::Char(_) | K::Var(_) | K::None => {}
    }
}

/// Adds the variables that `stmts` and `exprs` may change (by assignment or as `inout`) to `names`.
fn assigned(stmts: &[ast::Stmt], exprs: &[&ast::Expr], names: &mut HashSet<String>) {
    let scan = |e: &ast::Expr, names: &mut HashSet<String>| {
        each_expr(e, &mut |x| {
            if let ast::ExprKind::Inout(p) = &x.kind {
                if let Some(root) = crate::check::data::place_root(p) {
                    names.insert(root.to_string());
                }
            }
        })
    };
    for e in exprs {
        scan(e, names);
    }
    for s in stmts {
        match &s.kind {
            ast::StmtKind::Let { value, .. } => scan(value, names),
            ast::StmtKind::Assign { target, value, .. } => {
                if let Some(root) = crate::check::data::place_root(target) {
                    names.insert(root.to_string());
                }
                scan(target, names);
                scan(value, names);
            }
            ast::StmtKind::If { cond, then, els } => {
                scan(cond, names);
                assigned(then, &[], names);
                if let Some(e) = els {
                    assigned(e, &[], names);
                }
            }
            ast::StmtKind::While { cond, body } => assigned(body, &[cond], names),
            ast::StmtKind::For { start, end, step, body, .. } => {
                let mut es = vec![start, end];
                es.extend(step.iter());
                assigned(body, &es, names);
            }
            ast::StmtKind::ForEach { iter, body, .. } => assigned(body, &[iter], names),
            ast::StmtKind::Arena(body) => assigned(body, &[], names),
            ast::StmtKind::Match { .. } => {}
            ast::StmtKind::Ret(Some(e)) | ast::StmtKind::Expr(e) => scan(e, names),
            ast::StmtKind::Ret(None) | ast::StmtKind::Break | ast::StmtKind::Continue => {}
        }
    }
}

/// The `var`s of a function body that are counters: every change is `x += 1` or `x -= 1` (no
/// other assignment, no `inout x`). Started at a small value, such a variable cannot get near
/// 2^53 in any real run time (that takes months of counting), so `x + 1` needs no check.
fn counters(body: &[ast::Stmt], exclude: &HashSet<String>) -> HashSet<String> {
    fn walk(ss: &[ast::Stmt], cand: &mut HashSet<String>, bad: &mut HashSet<String>) {
        for s in ss {
            let mut exprs: Vec<&ast::Expr> = Vec::new();
            match &s.kind {
                ast::StmtKind::Let { name, mutable: true, ty, value } if ty.unwrap_or(value.ty) == Type::Int => {
                    cand.insert(name.clone());
                    exprs.push(value);
                }
                ast::StmtKind::Let { value, .. } => exprs.push(value),
                ast::StmtKind::Assign { target, op, value } => {
                    let step = matches!(op, Some(ast::BinOp::Add | ast::BinOp::Sub)) && matches!(value.kind, ast::ExprKind::Int(1));
                    match &target.kind {
                        ast::ExprKind::Var(_) if step => {}
                        ast::ExprKind::Var(x) => {
                            bad.insert(x.clone());
                        }
                        _ => {
                            if let Some(root) = crate::check::data::place_root(target) {
                                bad.insert(root.to_string());
                            }
                        }
                    }
                    exprs.push(target);
                    exprs.push(value);
                }
                ast::StmtKind::If { cond, then, els } => {
                    exprs.push(cond);
                    walk(then, cand, bad);
                    if let Some(e) = els {
                        walk(e, cand, bad);
                    }
                }
                ast::StmtKind::While { cond, body } => {
                    exprs.push(cond);
                    walk(body, cand, bad);
                }
                ast::StmtKind::For { start, end, step, body, .. } => {
                    exprs.extend([start, end]);
                    exprs.extend(step.iter());
                    walk(body, cand, bad);
                }
                ast::StmtKind::ForEach { iter, body, .. } => {
                    exprs.push(iter);
                    walk(body, cand, bad);
                }
                ast::StmtKind::Arena(body) => walk(body, cand, bad),
                ast::StmtKind::Match { .. } => {}
                ast::StmtKind::Ret(Some(e)) | ast::StmtKind::Expr(e) => exprs.push(e),
                ast::StmtKind::Ret(None) | ast::StmtKind::Break | ast::StmtKind::Continue => {}
            }
            for e in exprs {
                each_expr(e, &mut |x| {
                    if let ast::ExprKind::Inout(p) = &x.kind {
                        if let Some(root) = crate::check::data::place_root(p) {
                            bad.insert(root.to_string());
                        }
                    }
                });
            }
        }
    }
    let (mut cand, mut bad) = (HashSet::new(), exclude.clone());
    walk(body, &mut cand, &mut bad);
    cand.retain(|c| !bad.contains(c));
    cand
}

/// A block of the function being lowered.
#[derive(Default)]
struct Scope {
    names: HashMap<String, LocalId>,
    /// Managed values this scope owns, in the order they were created: released at its end.
    owned: Vec<LocalId>,
    /// The body of a loop: `break` and `continue` release the scopes up to here.
    loop_body: bool,
}

struct Lower<'a> {
    ids: &'a HashMap<String, FuncId>,
    strs: &'a mut Strs,
    structs: &'a Structs,
    locals: Vec<Local>,
    scopes: Vec<Scope>,
    /// Owned temporaries of the statement being lowered: released at its end unless moved.
    pending: Vec<LocalId>,
    /// The first feature the backends cannot generate yet.
    unsupported: Option<String>,
    /// Owned temporaries of the `map` steps of the chain loop being lowered: a `break` out of
    /// that loop (`any`, `all`, `find_index`) releases them first.
    chain_live: Vec<LocalId>,
    globals: &'a ast::Globals,
    /// The hidden parameters of the function being lowered, by script variable.
    hidden: HashMap<usize, LocalId>,
    /// What is known about int locals at this point of the function: the bounds they always
    /// keep (`base`), narrowed by facts: a condition that held (`if n < 2 { ret }` leaves
    /// `n >= 2`), or an index that was checked (after `xs[j]`, `0 <= j < len`).
    bounds: HashMap<LocalId, [Iv; MODES]>,
    /// The bounds a local keeps for its whole life: `let`s, `for` counters, counters (see `counters`).
    base: HashMap<LocalId, [Iv; MODES]>,
    /// The int locals facts may be kept about: parameters (not `inout`), `let`s, loop counters,
    /// and `var`s that only this function changes (not script variables a function changes).
    factual: HashSet<LocalId>,
    /// Every local whose facts were dropped, in order (see `restore`).
    dropped: Vec<LocalId>,
    /// The `var`s of the function that only count up or down by one.
    counters: HashSet<String>,
    /// Script variables that some function changes.
    changed: HashSet<String>,
}

impl<'a> Lower<'a> {
    fn new(ids: &'a HashMap<String, FuncId>, strs: &'a mut Strs, structs: &'a Structs, globals: &'a ast::Globals) -> Self {
        Lower {
            ids,
            strs,
            structs,
            locals: Vec::new(),
            scopes: vec![Scope::default()],
            pending: Vec::new(),
            unsupported: None,
            chain_live: Vec::new(),
            globals,
            hidden: HashMap::new(),
            bounds: HashMap::new(),
            base: HashMap::new(),
            factual: HashSet::new(),
            dropped: Vec::new(),
            counters: HashSet::new(),
            changed: HashSet::new(),
        }
    }

    /// `l` keeps the bounds `b` for its whole life.
    fn set_base(&mut self, l: LocalId, b: [Iv; MODES]) {
        self.base.insert(l, b);
        self.bounds.insert(l, b);
        self.factual.insert(l);
    }

    /// `l` changed: only its lifelong bounds are still known.
    fn forget(&mut self, l: LocalId) {
        match self.base.get(&l) {
            Some(b) => {
                self.bounds.insert(l, *b);
            }
            None => {
                self.bounds.remove(&l);
            }
        }
        self.dropped.push(l);
    }

    /// The facts as they were (`saved`, when `dropped` had `mark` entries), minus those about the
    /// locals that changed since: the code in between may not run, but what it changed stays changed.
    fn restore(&mut self, saved: HashMap<LocalId, [Iv; MODES]>, mark: usize) {
        self.bounds = saved;
        let changed: Vec<LocalId> = self.dropped[mark..].to_vec();
        self.dropped.truncate(mark);
        for l in changed {
            self.forget(l);
        }
    }

    /// The facts now, to `restore` later.
    fn save(&self) -> (HashMap<LocalId, [Iv; MODES]>, usize) {
        (self.bounds.clone(), self.dropped.len())
    }

    /// Before a loop: forgets the facts about the variables its code changes (they hold for the
    /// first round only).
    fn forget_changed(&mut self, stmts: &[ast::Stmt], exprs: &[&ast::Expr]) {
        let mut names = HashSet::new();
        assigned(stmts, exprs, &mut names);
        for n in names {
            if let Some(l) = self.scopes.iter().rev().find_map(|s| s.names.get(&n).copied()) {
                self.forget(l);
            }
        }
    }

    /// An index into a string or an array passed its check: `0 <= i < len` from here on.
    fn indexed(&mut self, i: &Expr) {
        if let Expr::Local(l) = i {
            if self.factual.contains(l) {
                let mut b = self.bound(i);
                for (m, v) in b.iter_mut().enumerate() {
                    let ix = bounds::index(m);
                    v.lo = v.lo.max(ix.lo);
                    v.hi = v.hi.min(ix.hi);
                }
                self.bounds.insert(*l, b);
            }
        }
    }

    /// Narrows the bounds of the locals that `c` compares, assuming it is `truth`.
    fn assume(&mut self, c: &Expr, truth: bool) {
        use BinOp::*;
        match c {
            Expr::Binary(And, a, b) if truth => {
                self.assume(a, true);
                self.assume(b, true);
            }
            Expr::Binary(Or, a, b) if !truth => {
                self.assume(a, false);
                self.assume(b, false);
            }
            Expr::Unary(UnOp::Not, x) => self.assume(x, !truth),
            Expr::Binary(op @ (ILt | ILe | IGt | IGe | IEq), a, b) => {
                let op = match (op, truth) {
                    (_, true) => *op,
                    (ILt, false) => IGe,
                    (ILe, false) => IGt,
                    (IGt, false) => ILe,
                    (IGe, false) => ILt,
                    _ => return, // `!=` says nothing about a range
                };
                let flip = match op {
                    ILt => IGt,
                    ILe => IGe,
                    IGt => ILt,
                    IGe => ILe,
                    _ => IEq,
                };
                self.narrow(a, op, b);
                self.narrow(b, flip, a);
            }
            _ => {}
        }
    }

    /// `x op y` holds: narrows the bounds of `x` when it is a local facts are kept about.
    fn narrow(&mut self, x: &Expr, op: BinOp, y: &Expr) {
        let Expr::Local(l) = x else { return };
        if !self.factual.contains(l) {
            return;
        }
        let (mut xb, yb) = (self.bound(x), self.bound(y));
        for m in 0..MODES {
            let (v, w) = (&mut xb[m], yb[m]);
            match op {
                BinOp::ILt => v.hi = v.hi.min(w.hi - 1),
                BinOp::ILe => v.hi = v.hi.min(w.hi),
                BinOp::IGt => v.lo = v.lo.max(w.lo + 1),
                BinOp::IGe => v.lo = v.lo.max(w.lo),
                _ => {
                    v.lo = v.lo.max(w.lo);
                    v.hi = v.hi.min(w.hi);
                }
            }
        }
        self.bounds.insert(*l, xb);
    }

    /// The bounds of an int expression on every backend.
    fn bound(&self, e: &Expr) -> [Iv; MODES] {
        bounds::all(e, &|l| self.bounds.get(&l).copied())
    }

    /// int `a + b`, `a - b`, `a * b`: the plain operator when the result provably stays in range on
    /// every backend, else the checked operation (an overflow is a runtime error).
    fn int_arith(&mut self, op: BinOp, a: Expr, b: Expr, dst: Option<LocalId>, span: Span, out: &mut Vec<Stmt>) -> Expr {
        let (x, y) = (self.bound(&a), self.bound(&b));
        if (0..MODES).all(|m| bounds::arith(op, x[m], y[m]).fits(m)) {
            return Expr::Binary(op, Box::new(a), Box::new(b));
        }
        let rop = match op {
            BinOp::IAdd => RtOp::AddInt,
            BinOp::ISub => RtOp::SubInt,
            _ => RtOp::MulInt,
        };
        self.op(rop, vec![a, b], Ty::Int, dst, span, out)
    }

    /// int `-a`: plain unless `a` may be the smallest int (whose negation overflows).
    fn int_neg(&mut self, a: Expr, dst: Option<LocalId>, span: Span, out: &mut Vec<Stmt>) -> Expr {
        let x = self.bound(&a);
        if (0..MODES).all(|m| bounds::neg(x[m]).fits(m)) {
            return Expr::Unary(UnOp::INeg, Box::new(a));
        }
        self.op(RtOp::NegInt, vec![a], Ty::Int, dst, span, out)
    }

    fn not_yet(&mut self, what: &str) -> Expr {
        if self.unsupported.is_none() {
            self.unsupported = Some(format!("{what} can be type-checked, but the backends cannot generate them yet"));
        }
        Expr::Int(0)
    }

    fn managed(&self, t: Ty) -> bool {
        self.structs.managed(t)
    }

    fn func(&mut self, f: &ast::Func) -> Func {
        // parameters: borrowed (an `inout` one is the caller's place), so not owned by any scope
        for p in &f.params {
            let id = self.new_local(Some(p.name.clone()), p.ty);
            self.locals[id.0 as usize].inout = p.inout;
            self.scopes[0].names.insert(p.name.clone(), id);
            if !p.inout && p.ty == Type::Int {
                self.factual.insert(id);
            }
        }
        // a script variable that a function changes is no counter of the script, and no facts
        // are kept about it (any call may change it)
        self.changed =
            self.globals.uses.values().flatten().filter(|u| u.inout).map(|u| self.globals.vars[u.var].name.clone()).collect();
        self.counters = counters(&f.body, &self.changed);
        // the script variables it uses: hidden parameters after the others, `inout` when changed
        let globals = self.globals;
        let uses = globals.uses.get(&f.name).map_or(&[][..], Vec::as_slice);
        for u in uses {
            let g = &globals.vars[u.var];
            let id = self.new_local(Some(u.name.clone()), g.ty);
            self.locals[id.0 as usize].inout = u.inout;
            self.hidden.insert(u.var, id);
            if u.name == g.name {
                self.scopes[0].names.insert(u.name.clone(), id);
            }
        }
        let mut body = Vec::new();
        // `var n: int`: the body works on its own copy of the argument
        let copies = f.params.iter().any(|p| p.mutable);
        if copies {
            self.scopes.push(Scope::default());
            for p in f.params.iter().filter(|p| p.mutable) {
                let src = self.scopes[0].names[&p.name];
                let copy = self.declare(&p.name, p.ty);
                self.init(copy, Expr::Local(src), p.span, &mut body);
            }
        }
        self.block(&f.body, &mut body, false);
        if copies {
            let scope = self.scopes.pop().expect("pushed above");
            if !terminates(&body) {
                let span = f.span;
                for l in scope.owned.into_iter().rev() {
                    body.push(Stmt { kind: StmtKind::Drop(l), span });
                }
            }
        }
        let ret = if f.ret == Type::Void { None } else { Some(f.ret) };
        Func {
            name: f.name.clone(),
            params: f.params.len() + uses.len(),
            ret,
            locals: std::mem::take(&mut self.locals),
            body,
            span: f.span,
        }
    }

    fn new_local(&mut self, name: Option<String>, t: Ty) -> LocalId {
        let id = LocalId(self.locals.len() as u32);
        self.locals.push(Local { name, ty: t, inout: false });
        id
    }

    fn ty_of(&self, l: LocalId) -> Ty {
        self.locals[l.0 as usize].ty
    }

    /// A variable of the current scope; a managed one is owned by it.
    fn declare(&mut self, name: &str, t: Ty) -> LocalId {
        let id = self.new_local(Some(name.to_string()), t);
        let managed = self.managed(t);
        let scope = self.scopes.last_mut().expect("a scope is open");
        scope.names.insert(name.to_string(), id);
        if managed {
            scope.owned.push(id);
        }
        id
    }

    /// A variable that borrows its value (a loop variable): never released.
    fn declare_borrowed(&mut self, name: &str, t: Ty) -> LocalId {
        let id = self.new_local(Some(name.to_string()), t);
        self.scopes.last_mut().expect("a scope is open").names.insert(name.to_string(), id);
        id
    }

    fn temp(&mut self, t: Ty) -> LocalId {
        self.new_local(None, t)
    }

    /// A temporary holding a new value: owned until moved, released at the end of the statement.
    fn owned_temp(&mut self, t: Ty) -> LocalId {
        let id = self.temp(t);
        if self.managed(t) {
            self.pending.push(id);
        }
        id
    }

    fn lookup(&self, name: &str) -> LocalId {
        self.scopes
            .iter()
            .rev()
            .find_map(|s| s.names.get(name).copied())
            .unwrap_or_else(|| panic!("`{name}` reached lowering without being declared"))
    }

    /// If `v` is an owned temporary of this statement, takes it over (no release at the end).
    fn take(&mut self, v: &Expr) -> bool {
        if let Expr::Local(t) = v {
            if let Some(i) = self.pending.iter().position(|p| p == t) {
                self.pending.remove(i);
                return true;
            }
        }
        false
    }

    /// Releases the owned temporaries of the statement that just ended.
    fn end_statement(&mut self, span: Span, out: &mut Vec<Stmt>) {
        for t in std::mem::take(&mut self.pending).into_iter().rev() {
            out.push(Stmt { kind: StmtKind::Drop(t), span });
        }
    }

    /// `dst = v` for a fresh local (nothing to release first). The local takes ownership:
    /// an owned temporary moves, a borrowed value gets one more owner.
    fn init(&mut self, dst: LocalId, v: Expr, span: Span, out: &mut Vec<Stmt>) {
        if let Expr::Local(t) = v {
            if t == dst {
                return;
            }
        }
        let t = self.ty_of(dst);
        let managed = self.managed(t);
        let moved = self.take(&v);
        // a temporary made by the last statement: let that statement write into `dst` directly
        // (also for plain values: `x = f(x)` becomes one call writing `x`)
        if moved || !managed {
            if let Expr::Local(tmp) = v {
                if self.locals[tmp.0 as usize].name.is_none() {
                    if let Some(Stmt {
                        kind: StmtKind::Call { dst: d, .. } | StmtKind::Op { dst: d, .. } | StmtKind::Mutate { dst: d, .. },
                        ..
                    }) = out.last_mut()
                    {
                        if *d == Some(tmp) {
                            *d = Some(dst);
                            return;
                        }
                    }
                }
            }
        }
        let borrowed_managed = managed && !moved && !matches!(v, Expr::Str(_));
        out.push(Stmt { kind: StmtKind::Set(dst, v), span });
        if borrowed_managed {
            out.push(Stmt { kind: StmtKind::Dup(dst), span });
        }
    }

    /// Copies the current value of `v` into a temporary of this statement, so that a later
    /// operand that changes a variable cannot change it.
    fn snapshot(&mut self, v: Expr, t: Ty, span: Span, out: &mut Vec<Stmt>) -> Expr {
        // constants and compiler temporaries never change
        match &v {
            _ if v.is_const() => return v,
            Expr::Local(l) if self.locals[l.0 as usize].name.is_none() => return v,
            _ => {}
        }
        let tmp = self.temp(t);
        self.init(tmp, v, span, out);
        if self.managed(t) {
            self.pending.push(tmp);
        }
        Expr::Local(tmp)
    }

    /// A value about to be stored into a place, held by its own owner first. A value read from
    /// the place's own variable (`u.vals.push(u)`, `c.xs[0].ys = c.xs`) would otherwise be the
    /// very array the store changes, and end up containing itself.
    fn held(&mut self, v: Expr, t: Ty, span: Span, out: &mut Vec<Stmt>) -> Expr {
        if self.managed(t) {
            self.snapshot(v, t, span, out)
        } else {
            v
        }
    }

    /// Lowers operands left to right. An operand followed by one that can change a variable is
    /// snapshotted first.
    fn operands(&mut self, es: &[&ast::Expr], out: &mut Vec<Stmt>) -> Vec<Expr> {
        let mut v = Vec::with_capacity(es.len());
        for (i, e) in es.iter().enumerate() {
            let x = self.expr(e, None, out);
            let x = if es[i + 1..].iter().any(|later| mutates(later)) { self.snapshot(x, e.ty, e.span, out) } else { x };
            v.push(x);
        }
        v
    }

    /// Lowers a block in a new scope; at its end the scope's variables are released (and what
    /// its conditions told about bounds is forgotten).
    fn block(&mut self, stmts: &[ast::Stmt], out: &mut Vec<Stmt>, loop_body: bool) {
        self.scopes.push(Scope { loop_body, ..Scope::default() });
        let (bounds, mark) = self.save();
        for s in stmts {
            self.stmt(s, out);
        }
        self.restore(bounds, mark);
        let scope = self.scopes.pop().expect("pushed above");
        if !terminates(out) {
            let span = out.last().map_or(Span { line: 0, col: 0 }, |s| s.span);
            for l in scope.owned.into_iter().rev() {
                out.push(Stmt { kind: StmtKind::Drop(l), span });
            }
        }
    }

    /// Releases what the open scopes own, innermost first, down to (and including) the
    /// innermost loop body when `to_loop`, else all of them. `keep` is moved out instead.
    fn release_scopes(&mut self, to_loop: bool, keep: Option<LocalId>, span: Span, out: &mut Vec<Stmt>) {
        for scope in self.scopes.iter().rev() {
            for l in scope.owned.iter().rev() {
                if Some(*l) != keep {
                    out.push(Stmt { kind: StmtKind::Drop(*l), span });
                }
            }
            if to_loop && scope.loop_body {
                break;
            }
        }
    }

    /// A condition: if computing it left owned temporaries, it is stored in a `bool` first so
    /// they can be released before the branch.
    fn cond(&mut self, e: &ast::Expr, out: &mut Vec<Stmt>) -> Expr {
        let c = self.expr(e, None, out);
        if self.pending.is_empty() {
            return c;
        }
        let b = self.temp(Ty::Bool);
        out.push(Stmt { kind: StmtKind::Set(b, c), span: e.span });
        self.end_statement(e.span, out);
        Expr::Local(b)
    }

    fn stmt(&mut self, s: &ast::Stmt, out: &mut Vec<Stmt>) {
        let span = s.span;
        match &s.kind {
            ast::StmtKind::Let { name, ty: t, value, mutable } => {
                // No shadowing, so the value cannot refer to the new name.
                let t = t.unwrap_or(value.ty);
                let id = self.declare(name, t);
                let v = self.expr(value, Some(id), out);
                // an immutable int keeps the bounds of its value; a counter that starts small stays
                // below 2^53 (see `counters`)
                if t == Type::Int {
                    let b = if matches!(v, Expr::Local(l) if l == id) { [Iv::full(0), Iv::full(1)] } else { self.bound(&v) };
                    if !*mutable {
                        self.set_base(id, b);
                    } else if self.counters.contains(name) && b.iter().all(|iv| iv.lo >= -COUNTER_START && iv.hi <= COUNTER_START) {
                        self.set_base(id, [Iv { lo: -COUNTER_MAX, hi: COUNTER_MAX }; MODES]);
                    } else if !self.changed.contains(name) {
                        self.factual.insert(id);
                    }
                }
                self.init(id, v, span, out);
                self.end_statement(span, out);
            }
            ast::StmtKind::Assign { target, op, value } => {
                self.assign(target, *op, value, span, out);
                self.end_statement(span, out);
                if let ast::ExprKind::Var(x) = &target.kind {
                    let l = self.lookup(x);
                    self.forget(l);
                }
            }
            ast::StmtKind::If { cond, then, els } => {
                let cond = self.cond(cond, out);
                let (before, mark) = self.save();
                self.assume(&cond, true);
                let mut t = Vec::new();
                self.block(then, &mut t, false);
                self.restore(before.clone(), mark);
                self.assume(&cond, false);
                let mut e = Vec::new();
                if let Some(els) = els {
                    self.block(els, &mut e, false);
                }
                // after `if c { ...; ret }` the rest of the block knows that `c` is false
                let leaves = |b: &[ast::Stmt]| {
                    b.last().is_some_and(|s| matches!(s.kind, ast::StmtKind::Ret(_) | ast::StmtKind::Break | ast::StmtKind::Continue))
                };
                if els.is_some() || !leaves(then) {
                    self.restore(before, mark);
                }
                out.push(Stmt { kind: StmtKind::If { cond, then: t, els: e }, span });
            }
            ast::StmtKind::While { cond, body } => {
                // what the loop changes is unknown from the second round on
                self.forget_changed(body, &[cond]);
                let mut head = Vec::new();
                let cond = self.cond(cond, &mut head);
                let (before, mark) = self.save();
                self.assume(&cond, true);
                let mut b = Vec::new();
                self.block(body, &mut b, true);
                self.restore(before, mark);
                out.push(Stmt { kind: StmtKind::Loop { head, cond, body: b, step: Vec::new() }, span });
            }
            ast::StmtKind::For { var, start, end, step, body } => {
                self.scopes.push(Scope::default());
                let i = self.declare(var, Ty::Int);
                let (cond, next) = self.range(i, start, end, step.as_ref(), span, out);
                self.end_statement(span, out);
                self.forget_changed(body, &[]);
                let mut bd = Vec::new();
                self.block(body, &mut bd, true);
                let step = vec![Stmt { kind: StmtKind::Set(i, next), span }];
                self.scopes.pop();
                out.push(Stmt { kind: StmtKind::Loop { head: Vec::new(), cond, body: bd, step }, span });
            }
            ast::StmtKind::ForEach { var, index, iter, body } => {
                // The loop keeps its own reference to the string or array (`it`), so the body
                // may change the variable it came from without changing what the loop visits.
                let (elem, ity) = match iter.ty {
                    Type::Str => (Ty::Char, iter.ty),
                    // a map: its keys, in insertion order
                    t @ Type::Map(_) => {
                        let k = t.map_kv().expect("a map").0;
                        (k, Ty::array(k))
                    }
                    t => (t.elem().expect("the checker allows strings, arrays and maps"), t),
                };
                self.scopes.push(Scope::default());
                let mut v = self.expr(iter, None, out);
                if ity != iter.ty {
                    v = self.op(RtOp::MapKeys, vec![v], ity, None, span, out);
                }
                let it = self.temp(ity);
                self.init(it, v, span, out);
                if self.managed(ity) {
                    self.scopes.last_mut().expect("pushed").owned.push(it);
                }
                self.end_statement(span, out);
                // `for i, x in xs`: `i` counts from 0, one up at the start of every round
                let counter = index.as_ref().map(|i| {
                    let id = self.declare(i, Ty::Int);
                    out.push(Stmt { kind: StmtKind::Set(id, Expr::Int(-1)), span });
                    self.set_base(id, [bounds::index(0), bounds::index(1)]);
                    id
                });
                self.forget_changed(body, &[]);
                self.scopes.push(Scope { loop_body: true, ..Scope::default() });
                let x = self.declare_borrowed(var, elem);
                let (bounds, mark) = self.save();
                let mut bd = Vec::new();
                if let Some(i) = counter {
                    let next = Expr::Binary(BinOp::IAdd, Box::new(Expr::Local(i)), Box::new(Expr::Int(1)));
                    bd.push(Stmt { kind: StmtKind::Set(i, next), span });
                }
                for st in body {
                    self.stmt(st, &mut bd);
                }
                self.restore(bounds, mark);
                let inner = self.scopes.pop().expect("pushed");
                if !bd.last().is_some_and(|s| matches!(s.kind, StmtKind::Return(_) | StmtKind::Break | StmtKind::Continue)) {
                    for l in inner.owned.into_iter().rev() {
                        bd.push(Stmt { kind: StmtKind::Drop(l), span });
                    }
                }
                out.push(Stmt { kind: StmtKind::ForEach { var: x, iter: Expr::Local(it), body: bd }, span });
                let outer = self.scopes.pop().expect("pushed");
                for l in outer.owned.into_iter().rev() {
                    out.push(Stmt { kind: StmtKind::Drop(l), span });
                }
            }
            ast::StmtKind::Break | ast::StmtKind::Continue => {
                self.release_scopes(true, None, span, out);
                let kind = if matches!(s.kind, ast::StmtKind::Break) { StmtKind::Break } else { StmtKind::Continue };
                out.push(Stmt { kind, span });
            }
            ast::StmtKind::Match { .. } => unreachable!("the checker turns `match` into `if`"),
            ast::StmtKind::Arena(body) => {
                // An arena only changes when memory is returned, never what a program does. The
                // checker keeps values made inside from escaping except through `ret`; here its
                // values are freed by reference counting at `}` (a bulk allocator can come later).
                let mut b = Vec::new();
                self.block(body, &mut b, false);
                out.extend(b);
            }
            ast::StmtKind::Ret(v) => {
                let Some(e) = v else {
                    self.end_statement(span, out);
                    self.release_scopes(false, None, span, out);
                    out.push(Stmt { kind: StmtKind::Return(None), span });
                    return;
                };
                let t = e.ty;
                let v = self.expr(e, None, out);
                let mut keep = None;
                let v = if self.managed(t) {
                    if self.take(&v) {
                        v
                    } else if let Expr::Local(x) = v {
                        if self.scopes.iter().any(|s| s.owned.contains(&x)) {
                            // `ret x` of a local: the value moves out
                            keep = Some(x);
                            v
                        } else {
                            let r = self.temp(t);
                            self.init(r, Expr::Local(x), span, out);
                            Expr::Local(r)
                        }
                    } else {
                        let r = self.temp(t);
                        self.init(r, v, span, out);
                        Expr::Local(r)
                    }
                } else if self.pending.is_empty() && self.scopes.iter().all(|s| s.owned.is_empty()) {
                    v
                } else {
                    // the value may read something that is about to be released
                    let r = self.temp(t);
                    out.push(Stmt { kind: StmtKind::Set(r, v), span });
                    Expr::Local(r)
                };
                self.end_statement(span, out);
                self.release_scopes(false, keep, span, out);
                out.push(Stmt { kind: StmtKind::Return(Some(v)), span });
            }
            ast::StmtKind::Expr(e) => {
                if let ast::ExprKind::Call(name, args) = &e.kind {
                    // `free(x)` / `keep(x)`
                    if name == "free" || name == "keep" {
                        if let Some(ast::ExprKind::Var(x)) = args.first().map(|a| &a.kind) {
                            let id = self.lookup(x);
                            if self.managed(self.ty_of(id)) {
                                let kind = if name == "free" { StmtKind::Free(id) } else { StmtKind::Keep(id) };
                                out.push(Stmt { kind, span: e.span });
                            }
                        }
                        return;
                    }
                    // a call whose plain result is not used writes nowhere
                    if let Some(&func) = self.ids.get(name) {
                        if !self.managed(e.ty) {
                            let args = self.call_args(name, args, out);
                            out.push(Stmt { kind: StmtKind::Call { dst: None, func, args }, span: e.span });
                            self.end_statement(span, out);
                            return;
                        }
                    }
                }
                // effects only: the (pure) value itself is unused; an unused new value is
                // released at the end of the statement
                let v = self.expr(e, None, out);
                if let Expr::Local(t) = v {
                    let fresh = self.locals[t.0 as usize].name.is_none() && !self.pending.contains(&t);
                    if fresh && self.managed(self.ty_of(t)) && produced_owned(out, t) {
                        self.pending.push(t);
                    }
                }
                self.end_statement(span, out);
            }
        }
    }

    /// `i` from `a` to `b` (exclusive) by `k`, for `for i in a..b step k`: the bounds and the step
    /// are evaluated once, before the loop. Sets `i` and returns the loop condition and the value
    /// of `i` in the next round.
    ///
    /// That next value never overflows: `i + 1` stays at most `b` while `i < b`; with a bigger step,
    /// a round whose `i + k` would reach (or pass) the end sets `i` to the end instead, which ends
    /// the loop the same way.
    fn range(
        &mut self,
        i: LocalId,
        start: &ast::Expr,
        end: &ast::Expr,
        step: Option<&ast::Expr>,
        span: Span,
        out: &mut Vec<Stmt>,
    ) -> (Expr, Expr) {
        let a = self.expr(start, None, out);
        let b = self.expr(end, None, out);
        let k = step.map(|k| self.expr(k, None, out));
        let (ab, bb) = (self.bound(&a), self.bound(&b));
        out.push(Stmt { kind: StmtKind::Set(i, a), span });

        let last = if matches!(b, Expr::Int(_)) {
            b
        } else {
            let t = self.temp(Ty::Int);
            out.push(Stmt { kind: StmtKind::Set(t, b), span });
            Expr::Local(t)
        };
        let k = match k {
            None => Expr::Int(1),
            Some(k) if const_int(&k).is_some_and(|n| n != 0) => Expr::Int(const_int(&k).unwrap_or(1)),
            Some(k) => {
                let t = self.temp(Ty::Int);
                out.push(Stmt { kind: StmtKind::Set(t, k), span });
                let kspan = step.as_ref().map_or(span, |s| s.span);
                out.push(Stmt { kind: StmtKind::Op { dst: None, op: RtOp::CheckStep, args: vec![Expr::Local(t)] }, span: kspan });
                Expr::Local(t)
            }
        };
        let i_ = || Box::new(Expr::Local(i));
        let cond = match &k {
            Expr::Int(n) if *n > 0 => Expr::Binary(BinOp::ILt, i_(), Box::new(last.clone())),
            Expr::Int(_) => Expr::Binary(BinOp::IGt, i_(), Box::new(last.clone())),
            _ => {
                // the direction is known only at run time
                let up = Expr::Binary(
                    BinOp::And,
                    Box::new(Expr::Binary(BinOp::IGt, Box::new(k.clone()), Box::new(Expr::Int(0)))),
                    Box::new(Expr::Binary(BinOp::ILt, i_(), Box::new(last.clone()))),
                );
                let down = Expr::Binary(
                    BinOp::And,
                    Box::new(Expr::Binary(BinOp::ILt, Box::new(k.clone()), Box::new(Expr::Int(0)))),
                    Box::new(Expr::Binary(BinOp::IGt, i_(), Box::new(last.clone()))),
                );
                Expr::Binary(BinOp::Or, Box::new(up), Box::new(down))
            }
        };
        // the counter stays between the start and the end (strictly before the end)
        let iv: [Iv; MODES] = std::array::from_fn(|m| match &k {
            Expr::Int(n) if *n > 0 => Iv { lo: ab[m].lo, hi: bb[m].hi - 1 },
            Expr::Int(_) => Iv { lo: bb[m].lo + 1, hi: ab[m].hi },
            _ => Iv { lo: ab[m].lo.min(bb[m].lo), hi: ab[m].hi.max(bb[m].hi) },
        });
        self.set_base(i, iv);
        let add = |x: Expr, y: Expr| Expr::Binary(BinOp::IAdd, Box::new(x), Box::new(y));
        if let Expr::Int(n @ (1 | -1)) = k {
            return (cond, add(Expr::Local(i), Expr::Int(n)));
        }
        // `stop`: from there on, one more step reaches the end, so `i + k` is computed only below it
        // (above it when counting down). It is `last - k`, kept within the ints of every backend:
        // counting up `last < -SAFE + k ? -SAFE : last - k`, down `last > SAFE + k ? SAFE : last - k`.
        const SAFE: i64 = (1 << 53) - 1;
        let sub = |x: Expr, y: Expr| Expr::Binary(BinOp::ISub, Box::new(x), Box::new(y));
        let lt = |x: Expr, y: Expr| Expr::Binary(BinOp::ILt, Box::new(x), Box::new(y));
        let gt = |x: Expr, y: Expr| Expr::Binary(BinOp::IGt, Box::new(x), Box::new(y));
        let select = |c: Expr, x: Expr, y: Expr| Expr::Select(Box::new(c), Box::new(x), Box::new(y));
        let up_stop = |k: Expr| select(lt(last.clone(), add(Expr::Int(-SAFE), k.clone())), Expr::Int(-SAFE), sub(last.clone(), k));
        let down_stop = |k: Expr| select(gt(last.clone(), add(Expr::Int(SAFE), k.clone())), Expr::Int(SAFE), sub(last.clone(), k));
        let stop = self.temp(Ty::Int);
        match &k {
            Expr::Int(n) if *n > 0 => out.push(Stmt { kind: StmtKind::Set(stop, up_stop(k.clone())), span }),
            Expr::Int(_) => out.push(Stmt { kind: StmtKind::Set(stop, down_stop(k.clone())), span }),
            // the direction is known only at run time: compute only the right one
            _ => {
                let then = vec![Stmt { kind: StmtKind::Set(stop, up_stop(k.clone())), span }];
                let els = vec![Stmt { kind: StmtKind::Set(stop, down_stop(k.clone())), span }];
                out.push(Stmt { kind: StmtKind::If { cond: gt(k.clone(), Expr::Int(0)), then, els }, span });
            }
        }
        let ends = match &k {
            Expr::Int(n) if *n > 0 => Expr::Binary(BinOp::IGe, i_(), Box::new(Expr::Local(stop))),
            Expr::Int(_) => Expr::Binary(BinOp::ILe, i_(), Box::new(Expr::Local(stop))),
            _ => select(
                gt(k.clone(), Expr::Int(0)),
                Expr::Binary(BinOp::IGe, i_(), Box::new(Expr::Local(stop))),
                Expr::Binary(BinOp::ILe, i_(), Box::new(Expr::Local(stop))),
            ),
        };
        (cond, select(ends, last, add(Expr::Local(i), k)))
    }

    /// `target = value` / `target op= value`.
    fn assign(&mut self, target: &ast::Expr, op: Option<ast::BinOp>, value: &ast::Expr, span: Span, out: &mut Vec<Stmt>) {
        let t = target.ty;
        if let ast::ExprKind::Var(name) = &target.kind {
            let id = self.lookup(name);
            match op {
                None if self.managed(t) => {
                    // evaluate the new value first (it may read the old one), then replace
                    let v = self.expr(value, None, out);
                    let moved = self.take(&v);
                    if !moved && !matches!(v, Expr::Str(_)) {
                        // a borrowed value: one more owner before the old value goes
                        let tmp = self.temp(t);
                        out.push(Stmt { kind: StmtKind::Set(tmp, v), span });
                        out.push(Stmt { kind: StmtKind::Dup(tmp), span });
                        out.push(Stmt { kind: StmtKind::Drop(id), span });
                        out.push(Stmt { kind: StmtKind::Set(id, Expr::Local(tmp)), span });
                    } else {
                        out.push(Stmt { kind: StmtKind::Drop(id), span });
                        out.push(Stmt { kind: StmtKind::Set(id, v), span });
                    }
                }
                None => {
                    let v = self.expr(value, Some(id), out);
                    self.init(id, v, span, out);
                }
                Some(_) if t == Type::Str || t.elem().is_some() => {
                    // `s += t`, `xs += ys`: in place when the variable is the only owner
                    let v = self.expr(value, None, out);
                    let op = if t == Type::Str { RtOp::StrAppend } else { RtOp::ArrAppend };
                    out.push(Stmt { kind: StmtKind::Mutate { dst: None, op, place: Place::local(id), args: vec![v] }, span });
                }
                Some(op) => {
                    let rhs = self.expr(value, None, out);
                    let v = self.binop(op, Expr::Local(id), rhs, t, span, Some(id), out);
                    self.init(id, v, span, out);
                }
            }
            return;
        }
        // `m[k] = v`, `m[k] += v`: the map is the place, the key and the value its operands
        if let ast::ExprKind::Index(base, key) = &target.kind {
            if base.ty.map_kv().is_some() {
                let later = mutates(key) || mutates(value);
                let place = self.place(base, later, out);
                let k = self.expr(key, None, out);
                let k = if mutates(value) { self.snapshot(k, key.ty, key.span, out) } else { k };
                let v = match op {
                    None => self.expr(value, None, out),
                    Some(op) => {
                        let map = self.read(&place, base.ty, out);
                        let old = self.map_read(RtOp::MapGet, vec![map, k.clone()], t, target.span, out);
                        let old = if mutates(value) { self.snapshot(old, t, span, out) } else { old };
                        let rhs = self.expr(value, None, out);
                        self.binop(op, old, rhs, t, span, None, out)
                    }
                };
                let v = self.held(v, t, span, out);
                out.push(Stmt { kind: StmtKind::Mutate { dst: None, op: RtOp::MapSet, place, args: vec![k, v] }, span });
                return;
            }
        }
        // an element or field: the indexes first (left to right), then the value
        let place = self.place(target, mutates(value), out);
        // once the store ran, its indexes were valid
        let indexes: Vec<Expr> =
            place.path.iter().filter_map(|s| if let Step::Index(i, _) = s { Some(i.clone()) } else { None }).collect();
        match op {
            None => {
                let v = self.expr(value, None, out);
                let v = self.held(v, t, span, out);
                out.push(Stmt { kind: StmtKind::Store { place, value: v }, span });
                for i in &indexes {
                    self.indexed(i);
                }
            }
            Some(_) if t == Type::Str || t.elem().is_some() => {
                let v = self.expr(value, None, out);
                let v = self.held(v, t, span, out);
                let op = if t == Type::Str { RtOp::StrAppend } else { RtOp::ArrAppend };
                out.push(Stmt { kind: StmtKind::Mutate { dst: None, op, place, args: vec![v] }, span });
            }
            Some(op) => {
                // read once, then the value, then compute and store
                let old = self.read(&place, t, out);
                let old = if mutates(value) { self.snapshot(old, t, span, out) } else { old };
                let rhs = self.expr(value, None, out);
                let v = self.binop(op, old, rhs, t, span, None, out);
                out.push(Stmt { kind: StmtKind::Store { place, value: v }, span });
            }
        }
    }

    /// The place `e` names (a variable, then elements). Index values are fixed first, so the
    /// rest of the statement cannot change which element is meant.
    fn place(&mut self, e: &ast::Expr, fix: bool, out: &mut Vec<Stmt>) -> Place {
        match &e.kind {
            ast::ExprKind::Var(name) => Place::local(self.lookup(name)),
            ast::ExprKind::Index(base, index) => {
                let mut p = self.place(base, fix || mutates(index), out);
                let i = self.expr(index, None, out);
                let i = if fix && !i.is_const() { self.snapshot(i, index.ty, index.span, out) } else { i };
                // `m[k]` of a map: the value under the key
                p.path.push(if base.ty.map_kv().is_some() { Step::Key(i, e.span) } else { Step::Index(i, e.span) });
                p
            }
            ast::ExprKind::Field(base, name) => {
                let mut p = self.place(base, fix, out);
                p.path.push(Step::Field(self.field_index(base.ty, name)));
                p
            }
            _ => unreachable!("the checker allows only variables, elements and fields as places"),
        }
    }

    /// The current value at a place (for `xs[i] += v`): checked reads down the path.
    fn read(&mut self, place: &Place, t: Ty, out: &mut Vec<Stmt>) -> Expr {
        let mut cur = Expr::Local(place.root);
        let mut cur_ty = self.ty_of(place.root);
        for step in &place.path {
            match step {
                Step::Index(i, span) => {
                    let elem = cur_ty.elem().expect("an index step reads an array");
                    let d = self.temp(elem);
                    out.push(Stmt { kind: StmtKind::Op { dst: Some(d), op: RtOp::ArrGet, args: vec![cur, i.clone()] }, span: *span });
                    cur = Expr::Local(d);
                    cur_ty = elem;
                }
                Step::Key(k, span) => {
                    let (_, vt) = cur_ty.map_kv().expect("a key step reads a map");
                    let d = self.temp(vt);
                    out.push(Stmt { kind: StmtKind::Op { dst: Some(d), op: RtOp::MapGet, args: vec![cur, k.clone()] }, span: *span });
                    cur = Expr::Local(d);
                    cur_ty = vt;
                }
                Step::Field(k) => {
                    let ft = self.structs.get(cur_ty).expect("a field step reads a struct").fields[*k as usize].1;
                    cur = Expr::Field(Box::new(cur), *k, ft);
                    cur_ty = ft;
                }
            }
        }
        debug_assert!(cur_ty == t);
        cur
    }

    /// The field values of a struct construction (`name: value`), left to right.
    fn fields(&mut self, args: &[ast::Expr], out: &mut Vec<Stmt>) -> Vec<Expr> {
        let refs: Vec<&ast::Expr> = args
            .iter()
            .map(|a| match &a.kind {
                ast::ExprKind::Labeled(_, v) => v.as_ref(),
                _ => a,
            })
            .collect();
        self.operands(&refs, out)
    }

    /// The arguments of a call to the user function `callee`, left to right, then the script
    /// variables it uses (hidden parameters). An `inout` argument is a place; a plain argument
    /// that reads a variable passed `inout` is copied first, so it keeps the value it had even
    /// when the callee changes the variable through the `inout` one.
    fn call_args(&mut self, callee: &str, args: &[ast::Expr], out: &mut Vec<Stmt>) -> Vec<Arg> {
        let globals = self.globals;
        let hidden: Vec<(LocalId, bool)> = globals
            .uses
            .get(callee)
            .map_or(&[][..], Vec::as_slice)
            .iter()
            .map(|u| {
                // in a function its own hidden parameter, in the script the variable itself
                let l = match self.hidden.get(&u.var) {
                    Some(&l) => l,
                    None => self.lookup(&globals.vars[u.var].name),
                };
                (l, u.inout)
            })
            .collect();
        // the variables the callee can change
        let mut changed: Vec<LocalId> = hidden.iter().filter(|(_, io)| *io).map(|(l, _)| *l).collect();
        for a in args {
            if let ast::ExprKind::Inout(p) = &a.kind {
                if let Some(root) = crate::check::data::place_root(p) {
                    changed.push(self.lookup(root));
                }
            }
        }
        let mut v = Vec::with_capacity(args.len() + hidden.len());
        for (i, a) in args.iter().enumerate() {
            let later = args[i + 1..].iter().any(mutates);
            match &a.kind {
                ast::ExprKind::Inout(p) => {
                    let place = self.place(p, later, out);
                    v.push(Arg::InOut(place));
                }
                _ => {
                    let x = self.expr(a, None, out);
                    let x = if later || reads_any(&x, &changed) { self.snapshot(x, a.ty, a.span, out) } else { x };
                    v.push(Arg::Val(x));
                }
            }
        }
        for (l, inout) in hidden {
            v.push(if inout { Arg::InOut(Place::local(l)) } else { Arg::Val(Expr::Local(l)) });
        }
        // the callee may change what it gets `inout`
        for a in &v {
            if let Arg::InOut(p) = a {
                self.forget(p.root);
            }
        }
        v
    }

    /// The position of field `name` in struct type `t`.
    fn field_index(&self, t: Ty, name: &str) -> u32 {
        let info = self.structs.get(t).expect("the checker knows every struct");
        info.fields.iter().position(|(n, _)| n == name).expect("the checker knows every field") as u32
    }

    /// The parts of an interpolated string: text becomes string literals.
    fn parts(&mut self, parts: &[ast::InterpPart], out: &mut Vec<Stmt>) -> Vec<Expr> {
        let mut v = Vec::with_capacity(parts.len());
        for (i, p) in parts.iter().enumerate() {
            match p {
                ast::InterpPart::Lit(s) => v.push(Expr::Str(self.strs.intern(s))),
                ast::InterpPart::Expr(x) => {
                    let e = self.expr(x, None, out);
                    let later = parts[i + 1..].iter().any(|p| matches!(p, ast::InterpPart::Expr(y) if mutates(y)));
                    v.push(if later { self.snapshot(e, x.ty, x.span, out) } else { e });
                }
            }
        }
        v
    }

    /// An operation whose result goes into `dst` or a new temporary (owned if it makes a value).
    fn op(&mut self, op: RtOp, args: Vec<Expr>, t: Ty, dst: Option<LocalId>, span: Span, out: &mut Vec<Stmt>) -> Expr {
        let d = match dst {
            Some(d) => d,
            None if op.owned_result() => self.owned_temp(t),
            None => self.temp(t),
        };
        out.push(Stmt { kind: StmtKind::Op { dst: Some(d), op, args }, span });
        Expr::Local(d)
    }

    /// `xs[i]`: a checked read. A managed element gets one more owner, so the value stays valid
    /// whatever the rest of the statement does to the array.
    fn elem(&mut self, xs: Expr, i: Expr, t: Ty, dst: Option<LocalId>, span: Span, out: &mut Vec<Stmt>) -> Expr {
        let d = dst.unwrap_or_else(|| self.temp(t));
        self.indexed(&i);
        out.push(Stmt { kind: StmtKind::Op { dst: Some(d), op: RtOp::ArrGet, args: vec![xs, i] }, span });
        if self.managed(t) {
            out.push(Stmt { kind: StmtKind::Dup(d), span });
            if dst.is_none() {
                self.pending.push(d);
            }
        }
        Expr::Local(d)
    }

    /// `m[k]` / `m.get(k, default)`: the value is borrowed, so a managed one gets one more owner
    /// (like an array element).
    fn map_read(&mut self, op: RtOp, args: Vec<Expr>, t: Ty, span: Span, out: &mut Vec<Stmt>) -> Expr {
        let d = self.temp(t);
        out.push(Stmt { kind: StmtKind::Op { dst: Some(d), op, args }, span });
        if self.managed(t) {
            out.push(Stmt { kind: StmtKind::Dup(d), span });
            self.pending.push(d);
        }
        Expr::Local(d)
    }

    /// Lowers `e`: its effects are appended to `out` and a pure expression for its value is
    /// returned. With `dst` (a fresh variable, or a plain value's target), a top-level call or
    /// operation writes straight into `dst`.
    fn expr(&mut self, e: &ast::Expr, dst: Option<LocalId>, out: &mut Vec<Stmt>) -> Expr {
        let span = e.span;
        match &e.kind {
            ast::ExprKind::Int(n) => Expr::Int(*n),
            ast::ExprKind::Float(f) => Expr::Float(*f),
            ast::ExprKind::Bool(b) => Expr::Bool(*b),
            ast::ExprKind::Char(c) => Expr::Char(*c),
            ast::ExprKind::Str(s) => Expr::Str(self.strs.intern(s)),
            ast::ExprKind::Var(name) => Expr::Local(self.lookup(name)),
            ast::ExprKind::Interp(parts) => {
                let args = self.parts(parts, out);
                self.op(RtOp::Format, args, Ty::Str, dst, span, out)
            }
            ast::ExprKind::Unary(op, x) => {
                let v = Box::new(self.expr(x, None, out));
                match (op, x.ty) {
                    (ast::UnOp::Not, _) => Expr::Unary(UnOp::Not, v),
                    (ast::UnOp::Neg, Type::Float) => Expr::Unary(UnOp::FNeg, v),
                    (ast::UnOp::Neg, _) => self.int_neg(*v, dst, span, out),
                }
            }
            ast::ExprKind::Binary(op, l, r) => {
                if matches!(op, ast::BinOp::And | ast::BinOp::Or) {
                    return self.logic(*op, l, r, span, out);
                }
                let v = self.operands(&[l.as_ref(), r.as_ref()], out);
                let [a, b]: [Expr; 2] = v.try_into().expect("two operands");
                self.binop(*op, a, b, l.ty, span, dst, out)
            }
            ast::ExprKind::Bind(name, value, body) => {
                // the name keeps the value it has now (a copy unless it is a temporary already)
                let v = self.expr(value, None, out);
                let v = self.snapshot(v, value.ty, value.span, out);
                let l = match v {
                    Expr::Local(l) => l,
                    other => {
                        let t = self.temp(value.ty);
                        out.push(Stmt { kind: StmtKind::Set(t, other), span });
                        t
                    }
                };
                self.scopes.last_mut().expect("a scope is open").names.insert(name.clone(), l);
                self.expr(body, dst, out)
            }
            ast::ExprKind::Match(..) => unreachable!("the checker turns `match` into `if`"),
            ast::ExprKind::If(c, a, b) => {
                let c = self.expr(c, None, out);
                let saved = std::mem::take(&mut self.pending);
                let (mut ta, mut tb) = (Vec::new(), Vec::new());
                // only one branch runs: neither shows anything about bounds afterwards
                let (facts, mark) = self.save();
                let av = self.expr(a, None, &mut ta);
                self.restore(facts.clone(), mark);
                let pa = std::mem::take(&mut self.pending);
                let bv = self.expr(b, None, &mut tb);
                self.restore(facts, mark);
                let pb = std::mem::take(&mut self.pending);
                if ta.is_empty() && tb.is_empty() && pa.is_empty() && pb.is_empty() {
                    self.pending = saved;
                    return Expr::Select(Box::new(c), Box::new(av), Box::new(bv));
                }
                // A branch has effects: only the taken branch may run them.
                let t = e.ty;
                let d = match dst {
                    Some(d) => d,
                    None => self.temp(t),
                };
                // each branch moves (or copies) its value into `d` and releases its own temporaries
                for (branch, value, pend) in [(&mut ta, av, pa), (&mut tb, bv, pb)] {
                    self.pending = pend;
                    self.init(d, value, span, branch);
                    self.end_statement(span, branch);
                }
                self.pending = saved;
                if dst.is_none() && self.managed(t) {
                    self.pending.push(d);
                }
                out.push(Stmt { kind: StmtKind::If { cond: c, then: ta, els: tb }, span });
                Expr::Local(d)
            }
            ast::ExprKind::Call(name, args) => self.call(name, args, e, dst, out),
            ast::ExprKind::Index(base, index) => {
                let v = self.operands(&[base.as_ref(), index.as_ref()], out);
                let [xs, i]: [Expr; 2] = v.try_into().expect("two operands");
                if base.ty == Type::Str {
                    let c = self.op(RtOp::StrAt, vec![xs, i.clone()], Ty::Char, dst, span, out);
                    self.indexed(&i);
                    return c;
                }
                if base.ty.map_kv().is_some() {
                    return self.map_read(RtOp::MapGet, vec![xs, i], e.ty, span, out);
                }
                self.elem(xs, i, e.ty, dst, span, out)
            }
            ast::ExprKind::Method(recv, name, args) => self.method(recv, name, args, e, dst, out),
            ast::ExprKind::Array(items) => {
                let refs: Vec<&ast::Expr> = items.iter().collect();
                let elems = self.operands(&refs, out);
                self.op(RtOp::ArrNew, elems, e.ty, dst, span, out)
            }
            ast::ExprKind::None => {
                let inner = e.ty.option_inner().expect("`none` has an optional type");
                let d = self.default_value(inner, span, out);
                self.op(RtOp::StructNew, vec![Expr::Bool(false), d], e.ty, dst, span, out)
            }
            ast::ExprKind::Some(x) => {
                let v = self.expr(x, None, out);
                self.op(RtOp::StructNew, vec![Expr::Bool(true), v], e.ty, dst, span, out)
            }
            ast::ExprKind::Coalesce(a, b) => self.coalesce(a, b, e, dst, out),
            ast::ExprKind::In(item, container) => {
                let v = self.operands(&[item.as_ref(), container.as_ref()], out);
                let [x, c]: [Expr; 2] = v.try_into().expect("two operands");
                match container.ty {
                    // a character searched for in text is the one-character string
                    Type::Str => {
                        let needle = if item.ty == Type::Char { self.op(RtOp::Format, vec![x], Ty::Str, None, span, out) } else { x };
                        Expr::Pure(PureFn::StrContains, vec![c, needle])
                    }
                    Type::Map(_) => Expr::Pure(PureFn::MapHas, vec![c, x]),
                    _ => Expr::Pure(PureFn::ArrContains, vec![c, x]),
                }
            }
            ast::ExprKind::Slice(base, lo, hi) => {
                let mut refs: Vec<&ast::Expr> = vec![base.as_ref()];
                refs.extend(lo.iter().map(|x| x.as_ref()));
                refs.extend(hi.iter().map(|x| x.as_ref()));
                let mut vals = self.operands(&refs, out).into_iter();
                let v = vals.next().expect("the base");
                let a = if lo.is_some() { vals.next().expect("the start") } else { Expr::Int(0) };
                let (op, len) = if base.ty == Type::Str { (RtOp::StrSlice, PureFn::StrLen) } else { (RtOp::ArrSlice, PureFn::ArrLen) };
                let b = if hi.is_some() { vals.next().expect("the end") } else { Expr::Pure(len, vec![v.clone()]) };
                self.op(op, vec![v, a, b], e.ty, dst, span, out)
            }
            ast::ExprKind::Fmt(x, spec) => {
                let v = self.expr(x, None, out);
                // the text of the value: a float with decimals is rounded exactly like `text.fixed`
                let body = match (x.ty, spec.prec) {
                    (Type::Float, Some(p)) => {
                        let digits = vec![v, Expr::Int(p as i64)];
                        self.op(RtOp::Std(StdFn::TextFixed), digits, Ty::Str, None, span, out)
                    }
                    (Type::Str, _) => v,
                    _ => self.op(RtOp::Format, vec![v], Ty::Str, None, span, out),
                };
                if !spec.needs_helper() {
                    return body;
                }
                let number = matches!(x.ty, Type::Int | Type::Float);
                let align = spec.align.unwrap_or(if number { '>' } else { '<' });
                // `{n:<05}` has an explicit alignment: the 0 is then just the fill
                let zero_fill = spec.zero && spec.align.is_some();
                let fill = spec.fill.unwrap_or(if zero_fill { '0' } else { ' ' });
                let args = vec![
                    Arg::Val(body),
                    Arg::Val(Expr::Bool(spec.plus)),
                    Arg::Val(Expr::Bool(spec.comma)),
                    Arg::Val(Expr::Int(spec.width as i64)),
                    Arg::Val(Expr::Char(align as u32)),
                    Arg::Val(Expr::Char(fill as u32)),
                    Arg::Val(Expr::Bool(spec.zero && spec.align.is_none())),
                ];
                self.call_helper(H::Fmt, args, Ty::Str, dst, span, out)
            }
            ast::ExprKind::Tuple(items) => {
                let refs: Vec<&ast::Expr> = items.iter().collect();
                let vals = self.operands(&refs, out);
                self.op(RtOp::StructNew, vals, e.ty, dst, span, out)
            }
            ast::ExprKind::MapLit(pairs) => {
                let refs: Vec<&ast::Expr> = pairs.iter().flat_map(|(k, v)| [k, v]).collect();
                let parts = self.operands(&refs, out);
                self.op(RtOp::MapNew, parts, e.ty, dst, span, out)
            }
            ast::ExprKind::Field(base, name) => {
                let k = self.field_index(base.ty, name);
                let b = self.expr(base, None, out);
                Expr::Field(Box::new(b), k, e.ty)
            }
            ast::ExprKind::Labeled(..) | ast::ExprKind::Inout(..) => self.not_yet("`inout` arguments"),
            ast::ExprKind::Lambda(..) => self.not_yet("lambdas outside a method call"),
            ast::ExprKind::Comprehension(c) => {
                // a loop that may run no round
                let (facts, mark) = self.save();
                let v = self.comprehension(c, e, out);
                self.restore(facts, mark);
                v
            }
        }
    }

    /// `a && b`, `a || b`: if `b` has effects, they only run when its value is needed.
    fn logic(&mut self, op: ast::BinOp, l: &ast::Expr, r: &ast::Expr, span: Span, out: &mut Vec<Stmt>) -> Expr {
        let a = self.expr(l, None, out);
        let saved = std::mem::take(&mut self.pending);
        let mut rhs = Vec::new();
        // `b` may not run: what it shows about bounds is not known afterwards
        let (facts, mark) = self.save();
        let b = self.expr(r, None, &mut rhs);
        self.restore(facts, mark);
        let iop = if op == ast::BinOp::And { BinOp::And } else { BinOp::Or };
        if rhs.is_empty() && self.pending.is_empty() {
            self.pending = saved;
            return Expr::Binary(iop, Box::new(a), Box::new(b));
        }
        let t = self.temp(Ty::Bool);
        out.push(Stmt { kind: StmtKind::Set(t, a), span });
        let cond = if op == ast::BinOp::And { Expr::Local(t) } else { Expr::Unary(UnOp::Not, Box::new(Expr::Local(t))) };
        rhs.push(Stmt { kind: StmtKind::Set(t, b), span });
        self.end_statement(span, &mut rhs);
        self.pending = saved;
        out.push(Stmt { kind: StmtKind::If { cond, then: rhs, els: Vec::new() }, span });
        Expr::Local(t)
    }

    /// A binary operator on lowered operands of type `t`.
    #[allow(clippy::too_many_arguments)]
    fn binop(&mut self, op: ast::BinOp, a: Expr, b: Expr, t: Type, span: Span, dst: Option<LocalId>, out: &mut Vec<Stmt>) -> Expr {
        use ast::BinOp as A;
        if t == Type::Int && matches!(op, A::Div | A::Mod) {
            return match const_int(&b) {
                // x / -1 is -x (MIN / -1 overflows), x % -1 is 0
                Some(-1) if op == A::Div => self.int_neg(a, dst, span, out),
                Some(-1) => Expr::Int(0),
                Some(k) if k != 0 => {
                    let iop = if op == A::Div { BinOp::IDiv } else { BinOp::IRem };
                    Expr::Binary(iop, Box::new(a), Box::new(b))
                }
                _ => {
                    let rop = if op == A::Div { RtOp::DivInt } else { RtOp::RemInt };
                    self.op(rop, vec![a, b], Ty::Int, dst, span, out)
                }
            };
        }
        if op == A::Add && t == Type::Str {
            return self.op(RtOp::StrConcat, vec![a, b], Ty::Str, dst, span, out);
        }
        if op == A::Add && t.elem().is_some() {
            return self.op(RtOp::ArrConcat, vec![a, b], t, dst, span, out);
        }
        // tuples are ordered element by element: a generated helper function decides
        if t.is_tuple() && matches!(op, A::Lt | A::Le | A::Gt | A::Ge) {
            return self.call_helper(H::Cmp(op.symbol(), t), vec![Arg::Val(a), Arg::Val(b)], Ty::Bool, dst, span, out);
        }
        if Structs::aggregate(t) {
            let iop = if op == A::Eq { BinOp::DeepEq } else { BinOp::DeepNe };
            return Expr::Binary(iop, Box::new(a), Box::new(b));
        }
        if t == Type::Int && matches!(op, A::Add | A::Sub | A::Mul) {
            let iop = match op {
                A::Add => BinOp::IAdd,
                A::Sub => BinOp::ISub,
                _ => BinOp::IMul,
            };
            return self.int_arith(iop, a, b, dst, span, out);
        }
        let iop = match (op, t) {
            (A::Add, Type::Int) => BinOp::IAdd,
            (A::Add, _) => BinOp::FAdd,
            (A::Sub, Type::Int) => BinOp::ISub,
            (A::Sub, _) => BinOp::FSub,
            (A::Mul, Type::Int) => BinOp::IMul,
            (A::Mul, _) => BinOp::FMul,
            (A::Div, _) => BinOp::FDiv,
            (A::Eq, Type::Int) => BinOp::IEq,
            (A::Eq, Type::Float) => BinOp::FEq,
            (A::Eq, Type::Bool) => BinOp::BEq,
            (A::Eq, Type::Char) => BinOp::CEq,
            (A::Eq, _) => BinOp::SEq,
            (A::Ne, Type::Int) => BinOp::INe,
            (A::Ne, Type::Float) => BinOp::FNe,
            (A::Ne, Type::Bool) => BinOp::BNe,
            (A::Ne, Type::Char) => BinOp::CNe,
            (A::Ne, _) => BinOp::SNe,
            (A::Lt, Type::Int) => BinOp::ILt,
            (A::Lt, Type::Char) => BinOp::CLt,
            (A::Lt, Type::Str) => BinOp::SLt,
            (A::Lt, _) => BinOp::FLt,
            (A::Le, Type::Int) => BinOp::ILe,
            (A::Le, Type::Char) => BinOp::CLe,
            (A::Le, Type::Str) => BinOp::SLe,
            (A::Le, _) => BinOp::FLe,
            (A::Gt, Type::Int) => BinOp::IGt,
            (A::Gt, Type::Char) => BinOp::CGt,
            (A::Gt, Type::Str) => BinOp::SGt,
            (A::Gt, _) => BinOp::FGt,
            (A::Ge, Type::Int) => BinOp::IGe,
            (A::Ge, Type::Char) => BinOp::CGe,
            (A::Ge, Type::Str) => BinOp::SGe,
            (A::Ge, _) => BinOp::FGe,
            (A::Mod, _) | (A::And | A::Or, _) => unreachable!("rejected by the checker or handled above"),
        };
        Expr::Binary(iop, Box::new(a), Box::new(b))
    }

    fn call(&mut self, name: &str, args: &[ast::Expr], e: &ast::Expr, dst: Option<LocalId>, out: &mut Vec<Stmt>) -> Expr {
        let span = e.span;
        match name {
            "print" => {
                // `print(a, b)`: the values left to right, a space between them; a last `end: e`
                // is printed instead of the newline
                let (args, end) = match args.split_last() {
                    Some((last, rest)) if matches!(&last.kind, ast::ExprKind::Labeled(l, _) if l == "end") => (rest, Some(last)),
                    _ => (args, None),
                };
                let mut parts = Vec::new();
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        parts.push(Expr::Str(self.strs.intern(" ")));
                    }
                    let later = args[i + 1..].iter().chain(end).any(mutates);
                    match &a.kind {
                        // an interpolated string: its parts directly (a later value that changes
                        // a variable gets the string built first)
                        ast::ExprKind::Interp(p) if !later => {
                            let ps = self.parts(p, out);
                            parts.extend(ps);
                        }
                        _ => {
                            let x = self.expr(a, None, out);
                            parts.push(if later { self.snapshot(x, a.ty, a.span, out) } else { x });
                        }
                    }
                }
                let op = match end {
                    Some(e) => {
                        let ast::ExprKind::Labeled(_, v) = &e.kind else { unreachable!("matched above") };
                        let x = self.expr(v, None, out);
                        parts.push(x);
                        RtOp::PrintNoLine
                    }
                    None => RtOp::Print,
                };
                out.push(Stmt { kind: StmtKind::Op { dst: None, op, args: parts }, span });
                // `print` returns nothing, so this value is never used
                Expr::Bool(false)
            }
            "str" => {
                let x = self.expr(&args[0], None, out);
                if args[0].ty == Type::Str {
                    return x;
                }
                self.op(RtOp::Format, vec![x], Ty::Str, dst, span, out)
            }
            "char" => {
                let x = self.expr(&args[0], None, out);
                if args[0].ty == Type::Char {
                    return x;
                }
                self.op(RtOp::CharFrom, vec![x], Ty::Char, dst, span, out)
            }
            "int" => {
                let x = self.expr(&args[0], None, out);
                match args[0].ty {
                    Type::Float => self.op(RtOp::FloatToInt, vec![x], Ty::Int, dst, span, out),
                    Type::Str => self.op(RtOp::StrToInt, vec![x], Ty::Int, dst, span, out),
                    _ => x,
                }
            }
            "float" => {
                let x = self.expr(&args[0], None, out);
                match args[0].ty {
                    Type::Int => Expr::IntToFloat(Box::new(x)),
                    Type::Str => self.op(RtOp::StrToFloat, vec![x], Ty::Float, dst, span, out),
                    _ => x,
                }
            }
            "free" | "keep" => Expr::Bool(false),
            // `zip(a, b)` (unless the program defines its own): a generated helper function
            "zip" if !self.ids.contains_key(name) => {
                let vals = self.operands(&args.iter().collect::<Vec<_>>(), out);
                let h = H::Zip(args.iter().map(|a| a.ty).collect());
                self.call_helper(h, vals.into_iter().map(Arg::Val).collect(), e.ty, dst, span, out)
            }
            // the builtins `abs`, `min` and `max` (unless the program defines its own): pure choices
            "abs" | "min" | "max" if !self.ids.contains_key(name) => {
                let v = self.operands(&args.iter().collect::<Vec<_>>(), out);
                let float = e.ty == Type::Float;
                let lt = if float { BinOp::FLt } else { BinOp::ILt };
                let b = |x: &Expr| Box::new(x.clone());
                if name == "abs" {
                    let x = &v[0];
                    let zero = if float { Expr::Float(0.0) } else { Expr::Int(0) };
                    // -x fails only for the smallest int, which is negative: abs of it overflows too
                    let neg = if float { Expr::Unary(UnOp::FNeg, b(x)) } else { self.int_neg(x.clone(), None, span, out) };
                    return Expr::Select(Box::new(Expr::Binary(lt, b(x), Box::new(zero))), Box::new(neg), b(x));
                }
                let (x, y) = (&v[0], &v[1]);
                // min: `b < a ? b : a` keeps the first on a tie; max: `a < b ? b : a`
                let cond = if name == "min" { Expr::Binary(lt, b(y), b(x)) } else { Expr::Binary(lt, b(x), b(y)) };
                Expr::Select(Box::new(cond), b(y), b(x))
            }
            // `json.str(v)` of any value, `json.parse(text)` into the type the checker gave the call
            "json.str" | "json.parse" => {
                let x = self.expr(&args[0], None, out);
                let op = if name == "json.str" { RtOp::JsonStr } else { RtOp::JsonParse };
                self.op(op, vec![x], e.ty, dst, span, out)
            }
            // a standard library function: its operands left to right, then the runtime call
            _ if StdFn::from_name(name).is_some() => {
                let f = StdFn::from_name(name).expect("matched above");
                let refs: Vec<&ast::Expr> = args.iter().collect();
                let vals = self.operands(&refs, out);
                if e.ty == Type::Void {
                    out.push(Stmt { kind: StmtKind::Op { dst: None, op: RtOp::Std(f), args: vals }, span });
                    return Expr::Bool(false);
                }
                self.op(RtOp::Std(f), vals, e.ty, dst, span, out)
            }
            _ => {
                let Some(&func) = self.ids.get(name) else {
                    // `Point(x: 1, y: 2)`: the fields in declaration order (the checker made sure)
                    let mut fields = self.fields(args, out);
                    let info = self.structs.get(e.ty).expect("a struct");
                    // an enum value: the number of the variant and its values; the other variants' fields hold zeros
                    if !info.variants.is_empty() && info.fields.len() > fields.len() {
                        let Some(Expr::Int(k)) = fields.first().cloned() else {
                            unreachable!("the checker writes the number of the variant")
                        };
                        let layout: Vec<(Ty, bool)> = info
                            .payloads
                            .iter()
                            .enumerate()
                            .flat_map(|(v, _)| info.slots(v).map(move |i| (i, v == k as usize)))
                            .map(|(i, own)| (info.fields[i].1, own))
                            .collect();
                        let mut given = fields.drain(1..).collect::<Vec<_>>().into_iter();
                        for (t, own) in layout {
                            let v = if own {
                                given.next().expect("the checker counted the values")
                            } else {
                                self.default_value(t, span, out)
                            };
                            fields.push(v);
                        }
                    }
                    return self.op(RtOp::StructNew, fields, e.ty, dst, span, out);
                };
                let args = self.call_args(name, args, out);
                if e.ty == Type::Void {
                    out.push(Stmt { kind: StmtKind::Call { dst: None, func, args }, span });
                    return Expr::Bool(false);
                }
                let d = match dst {
                    Some(d) => d,
                    None => self.owned_temp(e.ty),
                };
                out.push(Stmt { kind: StmtKind::Call { dst: Some(d), func, args }, span });
                Expr::Local(d)
            }
        }
    }

    /// The value a `none` holds in its `val`: zeros, empty text, empty arrays, structs of those.
    fn default_value(&mut self, t: Ty, span: Span, out: &mut Vec<Stmt>) -> Expr {
        match t {
            Ty::Int => Expr::Int(0),
            Ty::Float => Expr::Float(0.0),
            Ty::Bool => Expr::Bool(false),
            Ty::Char => Expr::Char(0),
            Ty::Str => Expr::Str(self.strs.intern("")),
            Ty::Array(_) => self.op(RtOp::ArrNew, Vec::new(), t, None, span, out),
            Ty::Map(_) => self.op(RtOp::MapNew, Vec::new(), t, None, span, out),
            Ty::Struct(_) => {
                let fields: Vec<Ty> = self.structs.get(t).expect("a struct").fields.iter().map(|(_, ft)| *ft).collect();
                let vals: Vec<Expr> = fields.into_iter().map(|ft| self.default_value(ft, span, out)).collect();
                self.op(RtOp::StructNew, vals, t, None, span, out)
            }
            _ => Expr::Int(0),
        }
    }

    /// `a ?? b`: `b` runs only when `a` holds nothing.
    fn coalesce(&mut self, a: &ast::Expr, b: &ast::Expr, e: &ast::Expr, dst: Option<LocalId>, out: &mut Vec<Stmt>) -> Expr {
        let span = e.span;
        let inner = a.ty.option_inner().expect("`??` has an optional on its left");
        let av = self.expr(a, None, out);
        let av = if mutates(b) || !matches!(av, Expr::Local(_)) { self.snapshot(av, a.ty, a.span, out) } else { av };
        let has = Expr::Field(Box::new(av.clone()), 0, Ty::Bool);
        // `a ?? b` with an optional `b` is an optional; else the value itself
        let found = if e.ty == a.ty { av.clone() } else { Expr::Field(Box::new(av), 1, inner) };
        let saved = std::mem::take(&mut self.pending);
        let (facts, mark) = self.save();
        let mut tb = Vec::new();
        let bv = self.expr(b, None, &mut tb);
        self.restore(facts, mark);
        let pb = std::mem::take(&mut self.pending);
        if tb.is_empty() && pb.is_empty() {
            self.pending = saved;
            return Expr::Select(Box::new(has), Box::new(found), Box::new(bv));
        }
        // `b` has effects: only the taken branch runs them
        let d = match dst {
            Some(d) => d,
            None => self.temp(e.ty),
        };
        let mut ta = Vec::new();
        self.init(d, found, span, &mut ta);
        self.pending = pb;
        self.init(d, bv, span, &mut tb);
        self.end_statement(span, &mut tb);
        self.pending = saved;
        if dst.is_none() && self.managed(e.ty) {
            self.pending.push(d);
        }
        out.push(Stmt { kind: StmtKind::If { cond: has, then: ta, els: tb }, span });
        Expr::Local(d)
    }

    /// `m.get(k)`: `Some(value)` or `none`.
    fn map_get_option(&mut self, all: Vec<Expr>, v: Ty, opt: Ty, dst: Option<LocalId>, span: Span, out: &mut Vec<Stmt>) -> Expr {
        let d = match dst {
            Some(d) => d,
            None => self.temp(opt),
        };
        let cond = Expr::Pure(PureFn::MapHas, all.clone());
        let item = self.temp(v);
        let then = vec![
            Stmt { kind: StmtKind::Op { dst: Some(item), op: RtOp::MapGet, args: all }, span },
            Stmt { kind: StmtKind::Op { dst: Some(d), op: RtOp::StructNew, args: vec![Expr::Bool(true), Expr::Local(item)] }, span },
        ];
        // the default of the empty case is made (and released) in its own branch
        let saved = std::mem::take(&mut self.pending);
        let mut els = Vec::new();
        let dv = self.default_value(v, span, &mut els);
        els.push(Stmt { kind: StmtKind::Op { dst: Some(d), op: RtOp::StructNew, args: vec![Expr::Bool(false), dv] }, span });
        self.end_statement(span, &mut els);
        self.pending = saved;
        out.push(Stmt { kind: StmtKind::If { cond, then, els }, span });
        if dst.is_none() && self.managed(opt) {
            self.pending.push(d);
        }
        Expr::Local(d)
    }

    /// A second owner of the array `v` in a temporary of this statement: a helper that replaces the
    /// array it changes (`inout`) can still read the old one through it.
    fn extra_owner(&mut self, v: Expr, t: Ty, span: Span, out: &mut Vec<Stmt>) -> Expr {
        let k = self.temp(t);
        out.push(Stmt { kind: StmtKind::Set(k, v), span });
        out.push(Stmt { kind: StmtKind::Dup(k), span });
        self.pending.push(k);
        Expr::Local(k)
    }

    /// Sorts the array in `place` by the tuples `keys` (one per element), with a generated helper.
    fn sort_by_keys(&mut self, place: Place, arr: Ty, key: Ty, keys: Expr, span: Span, out: &mut Vec<Stmt>) -> Expr {
        self.forget(place.root);
        self.call_helper(H::SortKeyed(arr, key), vec![Arg::InOut(place), Arg::Val(keys)], Ty::Void, None, span, out)
    }

    /// A call of a generated helper function (see `helpers.rs`).
    fn call_helper(&mut self, h: H, args: Vec<Arg>, ret: Ty, dst: Option<LocalId>, span: Span, out: &mut Vec<Stmt>) -> Expr {
        let func = *self.ids.get(&h.name()).unwrap_or_else(|| panic!("the helper {} was not generated", h.name()));
        if ret == Ty::Void {
            out.push(Stmt { kind: StmtKind::Call { dst: None, func, args }, span });
            return Expr::Bool(false);
        }
        let d = match dst {
            Some(d) => d,
            None => self.owned_temp(ret),
        };
        out.push(Stmt { kind: StmtKind::Call { dst: Some(d), func, args }, span });
        Expr::Local(d)
    }

    fn method(
        &mut self,
        recv: &ast::Expr,
        name: &str,
        args: &[ast::Expr],
        e: &ast::Expr,
        dst: Option<LocalId>,
        out: &mut Vec<Stmt>,
    ) -> Expr {
        let span = e.span;
        if lambda::is_chain_method(recv.ty, name) {
            // a loop that may run no round
            let (facts, mark) = self.save();
            let v = self.chain_method(recv, name, args, e, out);
            self.restore(facts, mark);
            return v;
        }
        // `opt.is_some()`, `opt.is_none()`, `opt.unwrap()`
        if let Some(inner) = recv.ty.option_inner() {
            let v = self.expr(recv, None, out);
            let has = Expr::Field(Box::new(v.clone()), 0, Ty::Bool);
            return match name {
                "is_some" => has,
                "is_none" => Expr::Unary(UnOp::Not, Box::new(has)),
                _ => {
                    out.push(Stmt { kind: StmtKind::Op { dst: None, op: RtOp::CheckSome, args: vec![has] }, span });
                    Expr::Field(Box::new(v), 1, inner)
                }
            };
        }
        // `xs.sort()` of tuples: the helper function sorts the array by the tuples themselves
        if name == "sort" && recv.ty.elem().is_some_and(Ty::is_tuple) {
            let place = self.place(recv, false, out);
            let cur = self.read(&place, recv.ty, out);
            let keys = self.extra_owner(cur, recv.ty, span, out);
            return self.sort_by_keys(place, recv.ty, recv.ty.elem().expect("an array"), keys, span, out);
        }
        // `xs.sorted()`: a sorted copy
        if name == "sorted" {
            let v = self.expr(recv, None, out);
            let len = Expr::Pure(PureFn::ArrLen, vec![v.clone()]);
            let Expr::Local(copy) = self.op(RtOp::ArrSlice, vec![v, Expr::Int(0), len], recv.ty, None, span, out) else {
                unreachable!()
            };
            let elem = recv.ty.elem().expect("an array");
            if elem.is_tuple() {
                let keys = self.extra_owner(Expr::Local(copy), recv.ty, span, out);
                self.sort_by_keys(Place::local(copy), recv.ty, elem, keys, span, out);
            } else {
                out.push(Stmt {
                    kind: StmtKind::Mutate { dst: None, op: RtOp::ArrSort, place: Place::local(copy), args: Vec::new() },
                    span,
                });
            }
            return Expr::Local(copy);
        }
        // `xs.chunks(n)`, `m.items()`, `s.trim(chars)`: generated helper functions
        if matches!(name, "chunks" | "items") || (name == "trim" && !args.is_empty()) {
            let mut refs: Vec<&ast::Expr> = vec![recv];
            refs.extend(args.iter());
            let mut vals = self.operands(&refs, out);
            let h = match name {
                "chunks" => H::Chunks(recv.ty),
                "items" => H::Items(recv.ty),
                _ => {
                    // a character to cut is the one-character string
                    if args[0].ty == Type::Char {
                        let c = vals.pop().expect("one argument");
                        let s = self.op(RtOp::Format, vec![c], Ty::Str, None, span, out);
                        vals.push(s);
                    }
                    H::TrimChars
                }
            };
            return self.call_helper(h, vals.into_iter().map(Arg::Val).collect(), e.ty, dst, span, out);
        }
        if recv.ty.elem().is_some() && matches!(name, "push" | "pop" | "insert" | "remove" | "swap" | "sort" | "reverse") {
            // changes the receiver: a place, fixed before the arguments run
            let later = args.iter().any(mutates);
            let place = self.place(recv, later, out);
            let refs: Vec<&ast::Expr> = args.iter().collect();
            let vals = self.operands(&refs, out);
            let vals: Vec<Expr> = vals.into_iter().zip(args).map(|(v, a)| self.held(v, a.ty, a.span, out)).collect();
            let op = match name {
                "push" => RtOp::ArrPush,
                "pop" => RtOp::ArrPop,
                "insert" => RtOp::ArrInsert,
                "remove" => RtOp::ArrRemove,
                "swap" => RtOp::ArrSwap,
                "sort" => RtOp::ArrSort,
                _ => RtOp::ArrReverse,
            };
            if e.ty == Type::Void {
                out.push(Stmt { kind: StmtKind::Mutate { dst: None, op, place, args: vals }, span });
                return Expr::Bool(false);
            }
            let d = match dst {
                Some(d) => d,
                None => self.owned_temp(e.ty),
            };
            out.push(Stmt { kind: StmtKind::Mutate { dst: Some(d), op, place, args: vals }, span });
            return Expr::Local(d);
        }
        if name == "reversed" {
            // a copy, reversed in place: `xs.slice(0, len)` / the characters, then `reverse`
            let v = self.expr(recv, None, out);
            let (arr, t) = if recv.ty == Type::Str { (RtOp::StrChars, Type::array(Ty::Char)) } else { (RtOp::ArrSlice, recv.ty) };
            let args = if arr == RtOp::StrChars {
                vec![v]
            } else {
                let len = Expr::Pure(PureFn::ArrLen, vec![v.clone()]);
                vec![v, Expr::Int(0), len]
            };
            let Expr::Local(copy) = self.op(arr, args, t, None, span, out) else { unreachable!() };
            out.push(Stmt {
                kind: StmtKind::Mutate { dst: None, op: RtOp::ArrReverse, place: Place::local(copy), args: Vec::new() },
                span,
            });
            if recv.ty == Type::Str {
                let sep = Expr::Str(self.strs.intern(""));
                return self.op(RtOp::ArrJoin, vec![Expr::Local(copy), sep], Ty::Str, dst, span, out);
            }
            return Expr::Local(copy);
        }
        if recv.ty.map_kv().is_some() && matches!(name, "set" | "remove") {
            // changes the map: a place, fixed before the arguments run
            let later = args.iter().any(mutates);
            let place = self.place(recv, later, out);
            let refs: Vec<&ast::Expr> = args.iter().collect();
            let vals = self.operands(&refs, out);
            let vals: Vec<Expr> = vals.into_iter().zip(args).map(|(v, a)| self.held(v, a.ty, a.span, out)).collect();
            let op = if name == "set" { RtOp::MapSet } else { RtOp::MapRemove };
            out.push(Stmt { kind: StmtKind::Mutate { dst: None, op, place, args: vals }, span });
            return Expr::Bool(false);
        }
        let mut refs: Vec<&ast::Expr> = vec![recv];
        refs.extend(args.iter());
        let mut all = self.operands(&refs, out);
        if let Some((_, v)) = recv.ty.map_kv() {
            return match name {
                "len" => Expr::Pure(PureFn::MapLen, all),
                "has" => Expr::Pure(PureFn::MapHas, all),
                "get" if all.len() == 2 => self.map_get_option(all, v, e.ty, dst, span, out),
                "get" => self.map_read(RtOp::MapGetOr, all, v, span, out),
                "keys" => self.op(RtOp::MapKeys, all, e.ty, dst, span, out),
                _ => self.op(RtOp::MapValues, all, e.ty, dst, span, out),
            };
        }
        if recv.ty == Type::Str {
            // a character searched for in text is the one-character string
            if matches!(name, "contains" | "starts_with" | "ends_with" | "index_of") && args[0].ty == Type::Char {
                let c = all.pop().expect("one argument");
                let s = self.op(RtOp::Format, vec![c], Ty::Str, None, span, out);
                all.push(s);
            }
            // `pad_left(n)` fills with spaces
            if matches!(name, "pad_left" | "pad_right") && args.len() == 1 {
                all.push(Expr::Char(' ' as u32));
            }
        }
        // `s.to_int()`, `s.to_float()`: `Some(number)` when `int(s)` / `float(s)` would work
        if recv.ty == Type::Str && matches!(name, "to_int" | "to_float") {
            let float = name == "to_float";
            let ok = self.temp(Ty::Bool);
            let check = if float { StdFn::TextIsFloat } else { StdFn::TextIsInt };
            out.push(Stmt { kind: StmtKind::Op { dst: Some(ok), op: RtOp::Std(check), args: vec![all[0].clone()] }, span });
            let d = match dst {
                Some(d) => d,
                None => self.temp(e.ty),
            };
            let n = self.temp(if float { Ty::Float } else { Ty::Int });
            let parse = if float { RtOp::StrToFloat } else { RtOp::StrToInt };
            let zero = if float { Expr::Float(0.0) } else { Expr::Int(0) };
            let then = vec![
                Stmt { kind: StmtKind::Op { dst: Some(n), op: parse, args: vec![all[0].clone()] }, span },
                Stmt { kind: StmtKind::Op { dst: Some(d), op: RtOp::StructNew, args: vec![Expr::Bool(true), Expr::Local(n)] }, span },
            ];
            let els =
                vec![Stmt { kind: StmtKind::Op { dst: Some(d), op: RtOp::StructNew, args: vec![Expr::Bool(false), zero] }, span }];
            out.push(Stmt { kind: StmtKind::If { cond: Expr::Local(ok), then, els }, span });
            return Expr::Local(d);
        }
        let pure = |f: PureFn| Some(f);
        let p = match (recv.ty, name) {
            (Type::Str, "len") => pure(PureFn::StrLen),
            (Type::Str, "contains") => pure(PureFn::StrContains),
            (Type::Str, "starts_with") => pure(PureFn::StrStartsWith),
            (Type::Str, "ends_with") => pure(PureFn::StrEndsWith),
            (Type::Str, "index_of") => pure(PureFn::StrIndexOf),
            (Type::Char, "code") => pure(PureFn::CharCode),
            (Type::Char, "upper") => pure(PureFn::CharUpper),
            (Type::Char, "lower") => pure(PureFn::CharLower),
            (Type::Char, "is_digit") => pure(PureFn::CharIsDigit),
            (Type::Char, "is_letter") => pure(PureFn::CharIsLetter),
            (Type::Char, "is_upper") => pure(PureFn::CharIsUpper),
            (Type::Char, "is_lower") => pure(PureFn::CharIsLower),
            (Type::Char, "is_space") => pure(PureFn::CharIsSpace),
            (_, "len") => pure(PureFn::ArrLen),
            (_, "contains") if recv.ty.elem().is_some() => pure(PureFn::ArrContains),
            (_, "index_of") if recv.ty.elem().is_some() => pure(PureFn::ArrIndexOf),
            _ => None,
        };
        if let Some(p) = p {
            return Expr::Pure(p, all);
        }
        let op = match (recv.ty, name) {
            (Type::Str, "slice") => RtOp::StrSlice,
            (Type::Str, "replace") => RtOp::StrReplace,
            (Type::Str, "trim") => RtOp::StrTrim,
            (Type::Str, "upper") => RtOp::StrUpper,
            (Type::Str, "lower") => RtOp::StrLower,
            (Type::Str, "repeat") => RtOp::StrRepeat,
            (Type::Str, "chars") => RtOp::StrChars,
            (Type::Str, "codes") => RtOp::StrCodes,
            (Type::Str, "split") => RtOp::StrSplit,
            (Type::Str, "pad_left") => RtOp::StrPadLeft,
            (Type::Str, "pad_right") => RtOp::StrPadRight,
            (_, "slice") => RtOp::ArrSlice,
            (_, "repeat") => RtOp::ArrRepeat,
            (_, "join") => RtOp::ArrJoin,
            _ => unreachable!("the checker knows every method"),
        };
        self.op(op, all, e.ty, dst, span, out)
    }
}

/// True if control never leaves the statements by running off their end: the last one leaves the
/// function or the loop round, or is an `if` whose branches all do (the scopes were released there).
fn terminates(ss: &[Stmt]) -> bool {
    match ss.last().map(|s| &s.kind) {
        Some(StmtKind::Return(_) | StmtKind::Break | StmtKind::Continue) => true,
        Some(StmtKind::If { then, els, .. }) => terminates(then) && terminates(els),
        _ => false,
    }
}

/// True if the statement that wrote `t` (the last one that did) made a new value.
fn produced_owned(out: &[Stmt], t: LocalId) -> bool {
    out.iter()
        .rev()
        .find_map(|s| match &s.kind {
            StmtKind::Op { dst: Some(d), op, .. } | StmtKind::Mutate { dst: Some(d), op, .. } if *d == t => Some(op.owned_result()),
            StmtKind::Call { dst: Some(d), .. } if *d == t => Some(true),
            _ => None,
        })
        .unwrap_or(false)
}

/// Removes temporaries that ended up unused (results written straight into variables) and
/// renumbers the rest, so the generated code declares only what it uses.
fn prune_temps(f: &mut Func) {
    let mut used = vec![false; f.locals.len()];
    for (i, l) in f.locals.iter().enumerate() {
        used[i] = i < f.params || l.name.is_some();
    }
    visit_locals(&mut f.body, &mut |l: &mut LocalId| used[l.0 as usize] = true);
    if used.iter().all(|u| *u) {
        return;
    }
    let mut remap = vec![0u32; f.locals.len()];
    let mut kept = Vec::new();
    for (i, l) in std::mem::take(&mut f.locals).into_iter().enumerate() {
        if used[i] {
            remap[i] = kept.len() as u32;
            kept.push(l);
        }
    }
    f.locals = kept;
    visit_locals(&mut f.body, &mut |l: &mut LocalId| l.0 = remap[l.0 as usize]);
}
