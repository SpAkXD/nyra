//! Type checker. Annotates every expression with its type and collects all
//! errors in one pass. Nyra never converts types implicitly and never allows
//! shadowing: one name means one thing.

use std::collections::HashMap;

use crate::ast::*;
use crate::diag::{suggest, Diag};

pub const BUILTINS: &[&str] = &["print", "int", "float"];

struct Sig {
    params: Vec<Type>,
    ret: Type,
}

struct Var {
    ty: Type,
    mutable: bool,
}

struct Checker {
    fns: HashMap<String, Sig>,
    scopes: Vec<HashMap<String, Var>>,
    errs: Vec<Diag>,
    ret: Type,
}

pub fn check(prog: &mut Program) -> Vec<Diag> {
    let mut c = Checker { fns: HashMap::new(), scopes: Vec::new(), errs: Vec::new(), ret: Type::Void };

    for f in &prog.funcs {
        if BUILTINS.contains(&f.name.as_str()) {
            c.errs.push(
                Diag::new("E0206", format!("`{}` is a builtin function and cannot be redefined", f.name), f.span)
                    .hint("pick another name"),
            );
        } else if c.fns.contains_key(&f.name) {
            c.errs.push(Diag::new("E0206", format!("function `{}` is defined twice", f.name), f.span));
        } else {
            let sig = Sig { params: f.params.iter().map(|p| p.ty).collect(), ret: f.ret };
            c.fns.insert(f.name.clone(), sig);
        }
    }

    match prog.funcs.iter().find(|f| f.name == "main") {
        None => c.errs.push(
            Diag::new("E0208", "missing `fn main()`", Span { line: 1, col: 1 })
                .hint("every program starts at `fn main() { ... }`"),
        ),
        Some(f) if !f.params.is_empty() || f.ret != Type::Void => c.errs.push(Diag::new(
            "E0211",
            "`main` must take no parameters and return nothing",
            f.span,
        )),
        _ => {}
    }

    for f in &mut prog.funcs {
        c.func(f);
    }
    c.errs
}

/// True if every path through the block ends in `ret`.
fn returns(b: &[Stmt]) -> bool {
    match b.last().map(|s| &s.kind) {
        Some(StmtKind::Ret(_)) => true,
        Some(StmtKind::If { then, els: Some(e), .. }) => returns(then) && returns(e),
        _ => false,
    }
}

impl Checker {
    fn lookup(&self, name: &str) -> Option<&Var> {
        self.scopes.iter().rev().find_map(|s| s.get(name))
    }

    fn undefined_var(&self, name: &str, span: Span) -> Diag {
        let d = Diag::new("E0201", format!("undefined variable `{name}`"), span);
        match suggest(name, self.scopes.iter().flat_map(|s| s.keys().map(String::as_str))) {
            Some(h) => d.hint(h),
            None => d.hint(format!("declare it first: `let {name} = ...`")),
        }
    }

    fn declare(&mut self, name: &str, ty: Type, mutable: bool, span: Span) {
        if self.lookup(name).is_some() {
            self.errs.push(Diag::new("E0206", format!("`{name}` is already defined"), span).hint(format!(
                "Nyra has no shadowing: pick a new name, or reassign with `{name} = ...` (needs `var`)"
            )));
        } else if self.fns.contains_key(name) || BUILTINS.contains(&name) {
            self.errs.push(
                Diag::new("E0206", format!("`{name}` is already the name of a function"), span)
                    .hint("variables and functions must have different names"),
            );
        }
        self.scopes.last_mut().unwrap().insert(name.to_string(), Var { ty, mutable });
    }

    fn expect_ty(&mut self, want: Type, got: Type, span: Span, ctx: &str) {
        if got == want || got == Type::Unknown || want == Type::Unknown {
            return;
        }
        let mut d = Diag::new("E0203", format!("{ctx}: expected `{}`, found `{}`", want.name(), got.name()), span);
        match (want, got) {
            (Type::Float, Type::Int) => d = d.hint("convert with `float(x)`"),
            (Type::Int, Type::Float) => d = d.hint("convert with `int(x)` (truncates toward zero)"),
            _ => {}
        }
        self.errs.push(d);
    }

    fn cond(&mut self, t: Type, span: Span) {
        if t != Type::Bool && t != Type::Unknown {
            self.errs.push(
                Diag::new("E0209", format!("condition must be `bool`, found `{}`", t.name()), span)
                    .hint("compare explicitly, e.g. `x != 0`"),
            );
        }
    }

    fn func(&mut self, f: &mut Func) {
        self.ret = f.ret;
        self.scopes = vec![HashMap::new()];
        for p in &f.params {
            self.declare(&p.name, p.ty, false, p.span);
        }
        self.block(&mut f.body);
        if f.ret != Type::Void && !returns(&f.body) {
            self.errs.push(
                Diag::new("E0207", format!("function `{}` must return `{}` on every path", f.name, f.ret.name()), f.span)
                    .hint("end the function (and both branches of a final `if`/`else`) with `ret`"),
            );
        }
    }

    fn block(&mut self, b: &mut [Stmt]) {
        self.scopes.push(HashMap::new());
        for s in b.iter_mut() {
            self.stmt(s);
        }
        self.scopes.pop();
    }

    fn stmt(&mut self, s: &mut Stmt) {
        let span = s.span;
        match &mut s.kind {
            StmtKind::Let { name, mutable, ty, value } => {
                let got = self.expr(value);
                if got == Type::Void {
                    self.errs.push(Diag::new("E0203", "cannot store the result of a function that returns nothing", value.span));
                }
                let t = match ty {
                    Some(t) => {
                        self.expect_ty(*t, got, value.span, "type mismatch");
                        *t
                    }
                    None => got,
                };
                self.declare(name, t, *mutable, span);
            }
            StmtKind::Assign { name, value } => {
                let got = self.expr(value);
                match self.lookup(name).map(|v| (v.ty, v.mutable)) {
                    None => {
                        let d = self.undefined_var(name, span);
                        self.errs.push(d);
                    }
                    Some((t, mutable)) => {
                        if !mutable {
                            self.errs.push(
                                Diag::new("E0205", format!("cannot assign to `{name}`: it was declared with `let`"), span)
                                    .hint(format!("declare it with `var {name} = ...` to make it mutable")),
                            );
                        }
                        self.expect_ty(t, got, value.span, "type mismatch in assignment");
                    }
                }
            }
            StmtKind::If { cond, then, els } => {
                let t = self.expr(cond);
                self.cond(t, cond.span);
                self.block(then);
                if let Some(e) = els {
                    self.block(e);
                }
            }
            StmtKind::While { cond, body } => {
                let t = self.expr(cond);
                self.cond(t, cond.span);
                self.block(body);
            }
            StmtKind::For { var, start, end, body } => {
                let a = self.expr(start);
                self.expect_ty(Type::Int, a, start.span, "range start");
                let b = self.expr(end);
                self.expect_ty(Type::Int, b, end.span, "range end");
                self.scopes.push(HashMap::new());
                self.declare(var, Type::Int, false, span);
                self.block(body);
                self.scopes.pop();
            }
            StmtKind::Ret(value) => match value {
                Some(e) => {
                    let t = self.expr(e);
                    if self.ret == Type::Void {
                        self.errs.push(
                            Diag::new("E0207", "`ret` with a value in a function that returns nothing", e.span)
                                .hint("add a return type to the signature, e.g. `fn f() -> int`"),
                        );
                    } else {
                        self.expect_ty(self.ret, t, e.span, "wrong return type");
                    }
                }
                None => {
                    if self.ret != Type::Void {
                        self.errs.push(Diag::new("E0207", format!("`ret` needs a `{}` value", self.ret.name()), span));
                    }
                }
            },
            StmtKind::Expr(e) => {
                self.expr(e);
            }
        }
    }

    fn expr(&mut self, e: &mut Expr) -> Type {
        let span = e.span;
        let t = match &mut e.kind {
            ExprKind::Int(_) => Type::Int,
            ExprKind::Float(_) => Type::Float,
            ExprKind::Bool(_) => Type::Bool,
            ExprKind::Str(_) => Type::Str,
            ExprKind::Interp(parts) => {
                for p in parts.iter_mut() {
                    if let InterpPart::Expr(x) = p {
                        if self.expr(x) == Type::Void {
                            self.errs.push(
                                Diag::new("E0203", "cannot put a value of type `void` into a string", x.span)
                                    .hint("this function returns nothing"),
                            );
                        }
                    }
                }
                Type::Str
            }
            ExprKind::Var(name) => match self.lookup(name) {
                Some(v) => v.ty,
                None => {
                    let d = self.undefined_var(name, span);
                    self.errs.push(d);
                    Type::Unknown
                }
            },
            ExprKind::Unary(op, inner) => {
                let t = self.expr(inner);
                match (*op, t) {
                    (_, Type::Unknown) => Type::Unknown,
                    (UnOp::Neg, Type::Int | Type::Float) => t,
                    (UnOp::Not, Type::Bool) => Type::Bool,
                    (op, t) => {
                        let sym = if op == UnOp::Neg { "-" } else { "!" };
                        self.errs.push(Diag::new("E0210", format!("cannot apply `{sym}` to `{}`", t.name()), span));
                        Type::Unknown
                    }
                }
            }
            ExprKind::Binary(op, l, r) => {
                let lt = self.expr(l);
                let rt = self.expr(r);
                self.binary(*op, lt, rt, span)
            }
            ExprKind::Call(name, args) => self.call(name, args, span),
        };
        e.ty = t;
        t
    }

    fn binary(&mut self, op: BinOp, l: Type, r: Type, span: Span) -> Type {
        use Type::{Bool, Float, Int, Str, Unknown, Void};
        if l == Unknown || r == Unknown {
            return Unknown;
        }
        let res = match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => match (l, r) {
                (Int, Int) => Some(Int),
                (Float, Float) => Some(Float),
                _ => None,
            },
            BinOp::Mod => (l == Int && r == Int).then_some(Int),
            BinOp::Eq | BinOp::Ne => (l == r && l != Void).then_some(Bool),
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => match (l, r) {
                (Int, Int) | (Float, Float) => Some(Bool),
                _ => None,
            },
            BinOp::And | BinOp::Or => (l == Bool && r == Bool).then_some(Bool),
        };
        if let Some(t) = res {
            return t;
        }
        let mut d = Diag::new(
            "E0210",
            format!("cannot use `{}` on `{}` and `{}`", op.symbol(), l.name(), r.name()),
            span,
        );
        if matches!((l, r), (Int, Float) | (Float, Int)) {
            d = d.hint("Nyra never converts numbers implicitly: use `float(x)` or `int(x)`");
        } else if op == BinOp::Add && l == Str {
            d = d.hint("string concatenation is not supported yet");
        }
        self.errs.push(d);
        Unknown
    }

    fn call(&mut self, name: &str, args: &mut [Expr], span: Span) -> Type {
        let tys: Vec<Type> = args.iter_mut().map(|a| self.expr(a)).collect();

        if BUILTINS.contains(&name) {
            let ret = match name {
                "print" => Type::Void,
                "int" => Type::Int,
                _ => Type::Float,
            };
            if tys.len() != 1 {
                self.errs.push(Diag::new(
                    "E0204",
                    format!("`{name}` takes 1 argument but {} were given", tys.len()),
                    span,
                ));
                return ret;
            }
            let t = tys[0];
            let ok = match name {
                "print" => t != Type::Void,
                _ => matches!(t, Type::Int | Type::Float | Type::Unknown),
            };
            if !ok {
                self.errs.push(Diag::new(
                    "E0203",
                    format!("`{name}` cannot take a `{}` argument", t.name()),
                    args[0].span,
                ));
            }
            return ret;
        }

        let Some(sig) = self.fns.get(name) else {
            let d = Diag::new("E0202", format!("undefined function `{name}`"), span);
            let names = self.fns.keys().map(String::as_str).chain(BUILTINS.iter().copied());
            let d = match suggest(name, names) {
                Some(h) => d.hint(h),
                None => d,
            };
            self.errs.push(d);
            return Type::Unknown;
        };
        let (params, ret) = (sig.params.clone(), sig.ret);
        if params.len() != tys.len() {
            self.errs.push(Diag::new(
                "E0204",
                format!("`{name}` takes {} argument(s) but {} were given", params.len(), tys.len()),
                span,
            ));
        } else {
            for (i, (want, got)) in params.iter().zip(&tys).enumerate() {
                self.expect_ty(*want, *got, args[i].span, &format!("argument {} of `{name}`", i + 1));
            }
        }
        ret
    }
}
