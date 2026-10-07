//! Typed AST → IR. Effects become statements in left-to-right order; values become pure expressions.

use std::collections::HashMap;

use super::{visit_locals, BinOp, Expr, Func, FuncId, Local, LocalId, Module, RtOp, Stmt, StmtKind, StrId, Ty, UnOp};
use crate::ast::{self, Span, Type};

/// Lowers a type-checked program.
pub fn lower(prog: &ast::Program) -> Module {
    let ids: HashMap<String, FuncId> =
        prog.funcs.iter().enumerate().map(|(i, f)| (f.name.clone(), FuncId(i as u32))).collect();
    let mut strs = Strs::default();
    let mut funcs = Vec::new();
    for f in &prog.funcs {
        let mut func = Lower::new(&ids, &mut strs).func(f);
        prune_temps(&mut func);
        funcs.push(func);
    }
    Module { funcs, strs: strs.list, main: ids["main"] }
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

fn ty(t: Type) -> Ty {
    match t {
        Type::Int => Ty::Int,
        Type::Float => Ty::Float,
        Type::Bool => Ty::Bool,
        Type::Str => Ty::Str,
        Type::Void | Type::Unknown => panic!("the checker left a `{}` value to lower", t.name()),
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

struct Lower<'a> {
    ids: &'a HashMap<String, FuncId>,
    strs: &'a mut Strs,
    locals: Vec<Local>,
    scopes: Vec<HashMap<String, LocalId>>,
}

impl<'a> Lower<'a> {
    fn new(ids: &'a HashMap<String, FuncId>, strs: &'a mut Strs) -> Self {
        Lower { ids, strs, locals: Vec::new(), scopes: vec![HashMap::new()] }
    }

    fn func(mut self, f: &ast::Func) -> Func {
        for p in &f.params {
            self.declare(&p.name, ty(p.ty));
        }
        let mut body = Vec::new();
        self.block(&f.body, &mut body);
        let ret = if f.ret == Type::Void { None } else { Some(ty(f.ret)) };
        Func { name: f.name.clone(), params: f.params.len(), ret, locals: self.locals, body, span: f.span }
    }

    fn new_local(&mut self, name: Option<String>, t: Ty) -> LocalId {
        let id = LocalId(self.locals.len() as u32);
        self.locals.push(Local { name, ty: t });
        id
    }

    fn declare(&mut self, name: &str, t: Ty) -> LocalId {
        let id = self.new_local(Some(name.to_string()), t);
        self.scopes.last_mut().expect("a scope is open").insert(name.to_string(), id);
        id
    }

    fn temp(&mut self, t: Ty) -> LocalId {
        self.new_local(None, t)
    }

    fn lookup(&self, name: &str) -> LocalId {
        self.scopes
            .iter()
            .rev()
            .find_map(|s| s.get(name).copied())
            .unwrap_or_else(|| panic!("`{name}` reached lowering without being declared"))
    }

    fn block(&mut self, stmts: &[ast::Stmt], out: &mut Vec<Stmt>) {
        self.scopes.push(HashMap::new());
        for s in stmts {
            self.stmt(s, out);
        }
        self.scopes.pop();
    }

    /// Writes `v` into `dst`. If `v` is the temporary the last statement just produced,
    /// that statement writes straight into `dst` instead (no extra copy).
    fn assign(&mut self, dst: LocalId, v: Expr, span: Span, out: &mut Vec<Stmt>) {
        if let Expr::Local(t) = v {
            if t == dst {
                return;
            }
            if self.locals[t.0 as usize].name.is_none() {
                if let Some(Stmt { kind: StmtKind::Call { dst: d, .. } | StmtKind::Op { dst: d, .. }, .. }) =
                    out.last_mut()
                {
                    if *d == Some(t) {
                        *d = Some(dst);
                        return;
                    }
                }
            }
        }
        out.push(Stmt { kind: StmtKind::Set(dst, v), span });
    }

    fn stmt(&mut self, s: &ast::Stmt, out: &mut Vec<Stmt>) {
        let span = s.span;
        match &s.kind {
            ast::StmtKind::Let { name, ty: t, value, .. } => {
                // No shadowing, so the value cannot refer to the new name.
                let id = self.declare(name, ty(t.unwrap_or(value.ty)));
                let v = self.expr(value, Some(id), out);
                self.assign(id, v, span, out);
            }
            ast::StmtKind::Assign { name, value } => {
                let id = self.lookup(name);
                let v = self.expr(value, Some(id), out);
                self.assign(id, v, span, out);
            }
            ast::StmtKind::If { cond, then, els } => {
                let cond = self.expr(cond, None, out);
                let mut t = Vec::new();
                self.block(then, &mut t);
                let mut e = Vec::new();
                if let Some(els) = els {
                    self.block(els, &mut e);
                }
                out.push(Stmt { kind: StmtKind::If { cond, then: t, els: e }, span });
            }
            ast::StmtKind::While { cond, body } => {
                let mut head = Vec::new();
                let cond = self.expr(cond, None, &mut head);
                let mut b = Vec::new();
                self.block(body, &mut b);
                out.push(Stmt { kind: StmtKind::Loop { head, cond, body: b, step: Vec::new() }, span });
            }
            ast::StmtKind::For { var, start, end, body } => {
                // `for i in a..b`: both bounds are evaluated once, before the loop.
                let a = self.expr(start, None, out);
                let b = self.expr(end, None, out);
                self.scopes.push(HashMap::new());
                let i = self.declare(var, Ty::Int);
                out.push(Stmt { kind: StmtKind::Set(i, a), span });
                let last = if matches!(b, Expr::Int(_)) {
                    b
                } else {
                    let t = self.temp(Ty::Int);
                    out.push(Stmt { kind: StmtKind::Set(t, b), span });
                    Expr::Local(t)
                };
                let cond = Expr::Binary(BinOp::ILt, Box::new(Expr::Local(i)), Box::new(last));
                let mut bd = Vec::new();
                self.block(body, &mut bd);
                let next = Expr::Binary(BinOp::IAdd, Box::new(Expr::Local(i)), Box::new(Expr::Int(1)));
                let step = vec![Stmt { kind: StmtKind::Set(i, next), span }];
                self.scopes.pop();
                out.push(Stmt { kind: StmtKind::Loop { head: Vec::new(), cond, body: bd, step }, span });
            }
            ast::StmtKind::Ret(v) => {
                let v = v.as_ref().map(|e| self.expr(e, None, out));
                out.push(Stmt { kind: StmtKind::Return(v), span });
            }
            ast::StmtKind::Expr(e) => {
                // A call whose result is not used writes nowhere.
                if let ast::ExprKind::Call(name, args) = &e.kind {
                    if let Some(&func) = self.ids.get(name) {
                        let args = self.args(args, out);
                        out.push(Stmt { kind: StmtKind::Call { dst: None, func, args }, span: e.span });
                        return;
                    }
                }
                // effects only: the (pure) value itself is unused
                self.expr(e, None, out);
            }
        }
    }

    fn args(&mut self, args: &[ast::Expr], out: &mut Vec<Stmt>) -> Vec<Expr> {
        let mut v = Vec::with_capacity(args.len());
        for a in args {
            v.push(self.expr(a, None, out));
        }
        v
    }

    /// The parts of an interpolated string: text becomes string literals.
    fn parts(&mut self, parts: &[ast::InterpPart], out: &mut Vec<Stmt>) -> Vec<Expr> {
        let mut v = Vec::with_capacity(parts.len());
        for p in parts {
            match p {
                ast::InterpPart::Lit(s) => v.push(Expr::Str(self.strs.intern(s))),
                ast::InterpPart::Expr(x) => v.push(self.expr(x, None, out)),
            }
        }
        v
    }

    /// Lowers `e`: its effects are appended to `out` and a pure expression for its value is
    /// returned. With `dst`, a top-level call or runtime operation writes straight into `dst`.
    fn expr(&mut self, e: &ast::Expr, dst: Option<LocalId>, out: &mut Vec<Stmt>) -> Expr {
        let span = e.span;
        match &e.kind {
            ast::ExprKind::Int(n) => Expr::Int(*n),
            ast::ExprKind::Float(f) => Expr::Float(*f),
            ast::ExprKind::Bool(b) => Expr::Bool(*b),
            ast::ExprKind::Str(s) => Expr::Str(self.strs.intern(s)),
            ast::ExprKind::Var(name) => Expr::Local(self.lookup(name)),
            ast::ExprKind::Interp(parts) => {
                let args = self.parts(parts, out);
                let d = dst.unwrap_or_else(|| self.temp(Ty::Str));
                out.push(Stmt { kind: StmtKind::Op { dst: Some(d), op: RtOp::Format, args }, span });
                Expr::Local(d)
            }
            ast::ExprKind::Unary(op, x) => {
                let v = Box::new(self.expr(x, None, out));
                match (op, x.ty) {
                    (ast::UnOp::Not, _) => Expr::Unary(UnOp::Not, v),
                    (ast::UnOp::Neg, Type::Float) => Expr::Unary(UnOp::FNeg, v),
                    (ast::UnOp::Neg, _) => Expr::Unary(UnOp::INeg, v),
                }
            }
            ast::ExprKind::Binary(op, l, r) => self.binary(*op, l, r, e, dst, out),
            ast::ExprKind::If(c, a, b) => {
                let c = self.expr(c, None, out);
                let (mut ta, mut tb) = (Vec::new(), Vec::new());
                let av = self.expr(a, None, &mut ta);
                let bv = self.expr(b, None, &mut tb);
                if ta.is_empty() && tb.is_empty() {
                    return Expr::Select(Box::new(c), Box::new(av), Box::new(bv));
                }
                // A branch has effects: only the taken branch may run them.
                let d = dst.unwrap_or_else(|| self.temp(ty(e.ty)));
                self.assign(d, av, span, &mut ta);
                self.assign(d, bv, span, &mut tb);
                out.push(Stmt { kind: StmtKind::If { cond: c, then: ta, els: tb }, span });
                Expr::Local(d)
            }
            ast::ExprKind::Call(name, args) => self.call(name, args, e, dst, out),
        }
    }

    fn binary(
        &mut self,
        op: ast::BinOp,
        l: &ast::Expr,
        r: &ast::Expr,
        e: &ast::Expr,
        dst: Option<LocalId>,
        out: &mut Vec<Stmt>,
    ) -> Expr {
        use ast::BinOp as A;
        let span = e.span;
        if matches!(op, A::And | A::Or) {
            let a = self.expr(l, None, out);
            let mut rhs = Vec::new();
            let b = self.expr(r, None, &mut rhs);
            let iop = if op == A::And { BinOp::And } else { BinOp::Or };
            if rhs.is_empty() {
                return Expr::Binary(iop, Box::new(a), Box::new(b));
            }
            // The right side has effects: run them only when its value is needed.
            let t = self.temp(Ty::Bool);
            out.push(Stmt { kind: StmtKind::Set(t, a), span });
            let cond = if op == A::And {
                Expr::Local(t)
            } else {
                Expr::Unary(UnOp::Not, Box::new(Expr::Local(t)))
            };
            self.assign(t, b, span, &mut rhs);
            out.push(Stmt { kind: StmtKind::If { cond, then: rhs, els: Vec::new() }, span });
            return Expr::Local(t);
        }

        let a = self.expr(l, None, out);
        let b = self.expr(r, None, out);
        let t = l.ty;
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
                    let d = dst.unwrap_or_else(|| self.temp(Ty::Int));
                    let rop = if op == A::Div { RtOp::DivInt } else { RtOp::RemInt };
                    out.push(Stmt { kind: StmtKind::Op { dst: Some(d), op: rop, args: vec![a, b] }, span });
                    Expr::Local(d)
                }
            };
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
            (A::Eq, _) => BinOp::SEq,
            (A::Ne, Type::Int) => BinOp::INe,
            (A::Ne, Type::Float) => BinOp::FNe,
            (A::Ne, Type::Bool) => BinOp::BNe,
            (A::Ne, _) => BinOp::SNe,
            (A::Lt, Type::Int) => BinOp::ILt,
            (A::Lt, _) => BinOp::FLt,
            (A::Le, Type::Int) => BinOp::ILe,
            (A::Le, _) => BinOp::FLe,
            (A::Gt, Type::Int) => BinOp::IGt,
            (A::Gt, _) => BinOp::FGt,
            (A::Ge, Type::Int) => BinOp::IGe,
            (A::Ge, _) => BinOp::FGe,
            (A::Mod, _) | (A::And | A::Or, _) => unreachable!("rejected by the checker or handled above"),
        };
        Expr::Binary(iop, Box::new(a), Box::new(b))
    }

    fn call(
        &mut self,
        name: &str,
        args: &[ast::Expr],
        e: &ast::Expr,
        dst: Option<LocalId>,
        out: &mut Vec<Stmt>,
    ) -> Expr {
        let span = e.span;
        match name {
            "print" => {
                let parts = match &args[0].kind {
                    ast::ExprKind::Interp(p) => self.parts(p, out),
                    _ => vec![self.expr(&args[0], None, out)],
                };
                out.push(Stmt { kind: StmtKind::Op { dst: None, op: RtOp::Print, args: parts }, span });
                // `print` returns nothing, so this value is never used
                Expr::Bool(false)
            }
            "int" => {
                let x = self.expr(&args[0], None, out);
                if args[0].ty != Type::Float {
                    return x;
                }
                let d = dst.unwrap_or_else(|| self.temp(Ty::Int));
                out.push(Stmt { kind: StmtKind::Op { dst: Some(d), op: RtOp::FloatToInt, args: vec![x] }, span });
                Expr::Local(d)
            }
            "float" => {
                let x = self.expr(&args[0], None, out);
                if args[0].ty == Type::Int {
                    Expr::IntToFloat(Box::new(x))
                } else {
                    x
                }
            }
            _ => {
                let func = self.ids[name];
                let args = self.args(args, out);
                if e.ty == Type::Void {
                    out.push(Stmt { kind: StmtKind::Call { dst: None, func, args }, span });
                    return Expr::Bool(false);
                }
                let d = dst.unwrap_or_else(|| self.temp(ty(e.ty)));
                out.push(Stmt { kind: StmtKind::Call { dst: Some(d), func, args }, span });
                Expr::Local(d)
            }
        }
    }
}

/// Removes temporaries that ended up unused (`assign` redirects some results straight into
/// variables) and renumbers the rest, so the generated code declares only what it uses.
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
