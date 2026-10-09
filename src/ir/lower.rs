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

use std::collections::HashMap;

use super::{
    visit_locals, Arg, BinOp, Expr, Func, FuncId, Local, LocalId, Module, Place, PureFn, RtOp, StdFn, Step, Stmt, StmtKind,
    StrId, StructInfo, Structs, Ty, UnOp,
};
use crate::ast::{self, Span, Type};

/// Lowers a type-checked program. Fails for features the backends do not support yet.
pub fn lower(prog: &ast::Program) -> Result<Module, String> {
    let ids: HashMap<String, FuncId> =
        prog.funcs.iter().enumerate().map(|(i, f)| (f.name.clone(), FuncId(i as u32))).collect();
    let structs = struct_table(prog);
    let mut strs = Strs::default();
    let mut funcs = Vec::new();
    for f in &prog.funcs {
        let mut l = Lower::new(&ids, &mut strs, &structs);
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
        table.0.push((id, StructInfo { name: def.name.clone(), fields, managed }));
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

/// An int constant, also through negation (`-1`).
fn const_int(e: &Expr) -> Option<i64> {
    match e {
        Expr::Int(n) => Some(*n),
        Expr::Unary(UnOp::INeg, x) => const_int(x).map(i64::wrapping_neg),
        _ => None,
    }
}

/// True if evaluating `e` can change a variable: a method that changes its receiver, or a
/// call with an `inout` argument.
fn mutates(e: &ast::Expr) -> bool {
    use ast::ExprKind as K;
    match &e.kind {
        K::Method(r, name, args) => {
            matches!(name.as_str(), "push" | "pop" | "insert" | "remove" | "swap" | "sort" | "reverse" | "set")
                || mutates(r)
                || args.iter().any(mutates)
        }
        K::Call(_, args) => args.iter().any(|a| matches!(a.kind, K::Inout(_)) || mutates(a)),
        K::Unary(_, x) | K::Field(x, _) | K::Labeled(_, x) | K::Inout(x) => mutates(x),
        K::Binary(_, a, b) | K::Index(a, b) => mutates(a) || mutates(b),
        K::If(c, a, b) => mutates(c) || mutates(a) || mutates(b),
        K::Array(xs) => xs.iter().any(mutates),
        K::MapLit(pairs) => pairs.iter().any(|(k, v)| mutates(k) || mutates(v)),
        K::Interp(parts) => parts.iter().any(|p| matches!(p, ast::InterpPart::Expr(x) if mutates(x))),
        K::Int(_) | K::Float(_) | K::Bool(_) | K::Str(_) | K::Char(_) | K::Var(_) => false,
    }
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
}

impl<'a> Lower<'a> {
    fn new(ids: &'a HashMap<String, FuncId>, strs: &'a mut Strs, structs: &'a Structs) -> Self {
        Lower {
            ids,
            strs,
            structs,
            locals: Vec::new(),
            scopes: vec![Scope::default()],
            pending: Vec::new(),
            unsupported: None,
        }
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
        }
        let mut body = Vec::new();
        self.block(&f.body, &mut body, false);
        let ret = if f.ret == Type::Void { None } else { Some(f.ret) };
        Func { name: f.name.clone(), params: f.params.len(), ret, locals: std::mem::take(&mut self.locals), body, span: f.span }
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

    /// Lowers a block in a new scope; at its end the scope's variables are released.
    fn block(&mut self, stmts: &[ast::Stmt], out: &mut Vec<Stmt>, loop_body: bool) {
        self.scopes.push(Scope { loop_body, ..Scope::default() });
        for s in stmts {
            self.stmt(s, out);
        }
        let scope = self.scopes.pop().expect("pushed above");
        let ends = out.last().is_some_and(|s| matches!(s.kind, StmtKind::Return(_) | StmtKind::Break | StmtKind::Continue));
        if !ends {
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
            ast::StmtKind::Let { name, ty: t, value, .. } => {
                // No shadowing, so the value cannot refer to the new name.
                let t = t.unwrap_or(value.ty);
                let id = self.declare(name, t);
                let v = self.expr(value, Some(id), out);
                self.init(id, v, span, out);
                self.end_statement(span, out);
            }
            ast::StmtKind::Assign { target, op, value } => {
                self.assign(target, *op, value, span, out);
                self.end_statement(span, out);
            }
            ast::StmtKind::If { cond, then, els } => {
                let cond = self.cond(cond, out);
                let mut t = Vec::new();
                self.block(then, &mut t, false);
                let mut e = Vec::new();
                if let Some(els) = els {
                    self.block(els, &mut e, false);
                }
                out.push(Stmt { kind: StmtKind::If { cond, then: t, els: e }, span });
            }
            ast::StmtKind::While { cond, body } => {
                let mut head = Vec::new();
                let cond = self.cond(cond, &mut head);
                let mut b = Vec::new();
                self.block(body, &mut b, true);
                out.push(Stmt { kind: StmtKind::Loop { head, cond, body: b, step: Vec::new() }, span });
            }
            ast::StmtKind::For { var, start, end, step, body } => {
                // `for i in a..b step k`: the bounds and the step are evaluated once, before the loop.
                let a = self.expr(start, None, out);
                let b = self.expr(end, None, out);
                let k = step.as_ref().map(|k| self.expr(k, None, out));
                self.scopes.push(Scope::default());
                let i = self.declare(var, Ty::Int);
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
                self.end_statement(span, out);
                let i_ = || Box::new(Expr::Local(i));
                let cond = match &k {
                    Expr::Int(n) if *n > 0 => Expr::Binary(BinOp::ILt, i_(), Box::new(last)),
                    Expr::Int(_) => Expr::Binary(BinOp::IGt, i_(), Box::new(last)),
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
                            Box::new(Expr::Binary(BinOp::IGt, i_(), Box::new(last))),
                        );
                        Expr::Binary(BinOp::Or, Box::new(up), Box::new(down))
                    }
                };
                let mut bd = Vec::new();
                self.block(body, &mut bd, true);
                let next = Expr::Binary(BinOp::IAdd, Box::new(Expr::Local(i)), Box::new(k));
                let step = vec![Stmt { kind: StmtKind::Set(i, next), span }];
                self.scopes.pop();
                out.push(Stmt { kind: StmtKind::Loop { head: Vec::new(), cond, body: bd, step }, span });
            }
            ast::StmtKind::ForEach { var, iter, body } => {
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
                self.scopes.push(Scope { loop_body: true, ..Scope::default() });
                let x = self.declare_borrowed(var, elem);
                let mut bd = Vec::new();
                for st in body {
                    self.stmt(st, &mut bd);
                }
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
                            let args = self.call_args(args, out);
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
        match op {
            None => {
                let v = self.expr(value, None, out);
                let v = self.held(v, t, span, out);
                out.push(Stmt { kind: StmtKind::Store { place, value: v }, span });
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
                let i = if fix && !i.is_const() { self.snapshot(i, Ty::Int, index.span, out) } else { i };
                p.path.push(Step::Index(i, e.span));
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

    /// The arguments of a call to a user function, left to right. An `inout` argument is a
    /// place; next to one, every plain argument is copied first, so it keeps the value it had
    /// even when the callee changes the same variable through the `inout` one.
    fn call_args(&mut self, args: &[ast::Expr], out: &mut Vec<Stmt>) -> Vec<Arg> {
        let any_inout = args.iter().any(|a| matches!(a.kind, ast::ExprKind::Inout(_)));
        let mut v = Vec::with_capacity(args.len());
        for (i, a) in args.iter().enumerate() {
            let later = args[i + 1..].iter().any(mutates);
            match &a.kind {
                ast::ExprKind::Inout(p) => {
                    let place = self.place(p, later, out);
                    v.push(Arg::InOut(place));
                }
                _ => {
                    let x = self.expr(a, None, out);
                    let x = if later || any_inout { self.snapshot(x, a.ty, a.span, out) } else { x };
                    v.push(Arg::Val(x));
                }
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
                    (ast::UnOp::Neg, _) => Expr::Unary(UnOp::INeg, v),
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
            ast::ExprKind::If(c, a, b) => {
                let c = self.expr(c, None, out);
                let saved = std::mem::take(&mut self.pending);
                let (mut ta, mut tb) = (Vec::new(), Vec::new());
                let av = self.expr(a, None, &mut ta);
                let pa = std::mem::take(&mut self.pending);
                let bv = self.expr(b, None, &mut tb);
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
                    return self.op(RtOp::StrAt, vec![xs, i], Ty::Char, dst, span, out);
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
        }
    }

    /// `a && b`, `a || b`: if `b` has effects, they only run when its value is needed.
    fn logic(&mut self, op: ast::BinOp, l: &ast::Expr, r: &ast::Expr, span: Span, out: &mut Vec<Stmt>) -> Expr {
        let a = self.expr(l, None, out);
        let saved = std::mem::take(&mut self.pending);
        let mut rhs = Vec::new();
        let b = self.expr(r, None, &mut rhs);
        let iop = if op == ast::BinOp::And { BinOp::And } else { BinOp::Or };
        if rhs.is_empty() && self.pending.is_empty() {
            self.pending = saved;
            return Expr::Binary(iop, Box::new(a), Box::new(b));
        }
        let t = self.temp(Ty::Bool);
        out.push(Stmt { kind: StmtKind::Set(t, a), span });
        let cond = if op == ast::BinOp::And {
            Expr::Local(t)
        } else {
            Expr::Unary(UnOp::Not, Box::new(Expr::Local(t)))
        };
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
                // x / -1 wraps (x86 would trap on MIN / -1), x % -1 is 0
                Some(-1) if op == A::Div => Expr::Unary(UnOp::INeg, Box::new(a)),
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
        if Structs::aggregate(t) {
            let iop = if op == A::Eq { BinOp::DeepEq } else { BinOp::DeepNe };
            return Expr::Binary(iop, Box::new(a), Box::new(b));
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
                    let later = args[i + 1..].iter().chain(end).any(|x| mutates(x));
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
            // the builtins `abs`, `min` and `max` (unless the program defines its own): pure choices
            "abs" | "min" | "max" if !self.ids.contains_key(name) => {
                let v = self.operands(&args.iter().collect::<Vec<_>>(), out);
                let float = e.ty == Type::Float;
                let lt = if float { BinOp::FLt } else { BinOp::ILt };
                let b = |x: &Expr| Box::new(x.clone());
                if name == "abs" {
                    let x = &v[0];
                    let zero = if float { Expr::Float(0.0) } else { Expr::Int(0) };
                    let neg = Expr::Unary(if float { UnOp::FNeg } else { UnOp::INeg }, b(x));
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
                    let fields = self.fields(args, out);
                    return self.op(RtOp::StructNew, fields, e.ty, dst, span, out);
                };
                let args = self.call_args(args, out);
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

    fn method(&mut self, recv: &ast::Expr, name: &str, args: &[ast::Expr], e: &ast::Expr, dst: Option<LocalId>, out: &mut Vec<Stmt>) -> Expr {
        let span = e.span;
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
                "get" if all.len() == 2 => self.map_read(RtOp::MapGet, all, v, span, out),
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

/// True if the statement that wrote `t` (the last one that did) made a new value.
fn produced_owned(out: &[Stmt], t: LocalId) -> bool {
    match out.iter().rev().find_map(|s| match &s.kind {
        StmtKind::Op { dst: Some(d), op, .. } | StmtKind::Mutate { dst: Some(d), op, .. } if *d == t => Some(op.owned_result()),
        StmtKind::Call { dst: Some(d), .. } if *d == t => Some(true),
        _ => None,
    }) {
        Some(owned) => owned,
        None => false,
    }
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
