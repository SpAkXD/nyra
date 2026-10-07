//! Checks that an IR module is well formed: ids in range, operand and call types,
//! and that functions with a result return on every path. A failure is a compiler bug.

use super::{BinOp, Expr, Func, LocalId, Module, RtOp, Stmt, StmtKind, Ty, UnOp};

pub fn verify(m: &Module) -> Result<(), String> {
    if m.main.0 as usize >= m.funcs.len() {
        return Err("`main` is out of range".into());
    }
    for f in &m.funcs {
        let v = Verifier { m, f };
        v.stmts(&f.body).map_err(|e| format!("in fn {} (line {}): {e}", f.name, f.span.line))?;
        if f.ret.is_some() && !returns(&f.body) {
            return Err(format!("in fn {}: may end without returning a value", f.name));
        }
    }
    Ok(())
}

fn returns(b: &[Stmt]) -> bool {
    match b.last().map(|s| &s.kind) {
        Some(StmtKind::Return(_)) => true,
        Some(StmtKind::If { then, els, .. }) => returns(then) && returns(els),
        _ => false,
    }
}

struct Verifier<'a> {
    m: &'a Module,
    f: &'a Func,
}

impl Verifier<'_> {
    fn local(&self, l: LocalId) -> Result<Ty, String> {
        self.f.locals.get(l.0 as usize).map(|x| x.ty).ok_or_else(|| format!("local %{} is out of range", l.0))
    }

    fn expect(&self, e: &Expr, want: Ty, what: &str) -> Result<(), String> {
        let got = self.ty(e)?;
        if got == want {
            Ok(())
        } else {
            Err(format!("{what}: expected {}, found {}", want.name(), got.name()))
        }
    }

    fn ty(&self, e: &Expr) -> Result<Ty, String> {
        Ok(match e {
            Expr::Int(_) => Ty::Int,
            Expr::Float(_) => Ty::Float,
            Expr::Bool(_) => Ty::Bool,
            Expr::Str(s) => {
                if s.0 as usize >= self.m.strs.len() {
                    return Err(format!("string #{} is out of range", s.0));
                }
                Ty::Str
            }
            Expr::Local(l) => self.local(*l)?,
            Expr::Unary(op, x) => {
                let t = match op {
                    UnOp::INeg => Ty::Int,
                    UnOp::FNeg => Ty::Float,
                    UnOp::Not => Ty::Bool,
                };
                self.expect(x, t, "unary operand")?;
                t
            }
            Expr::Binary(op, a, b) => {
                self.expect(a, op.operand(), "left operand")?;
                self.expect(b, op.operand(), "right operand")?;
                if matches!(op, BinOp::IDiv | BinOp::IRem) && matches!(**b, Expr::Int(0) | Expr::Int(-1)) {
                    return Err("a pure int division by 0 or -1".into());
                }
                op.result()
            }
            Expr::Select(c, a, b) => {
                self.expect(c, Ty::Bool, "select condition")?;
                let t = self.ty(a)?;
                self.expect(b, t, "select branches")?;
                t
            }
            Expr::IntToFloat(x) => {
                self.expect(x, Ty::Int, "int-to-float operand")?;
                Ty::Float
            }
        })
    }

    fn stmts(&self, ss: &[Stmt]) -> Result<(), String> {
        for s in ss {
            self.stmt(s).map_err(|e| format!("{e} (line {})", s.span.line))?;
        }
        Ok(())
    }

    fn stmt(&self, s: &Stmt) -> Result<(), String> {
        match &s.kind {
            StmtKind::Set(l, e) => self.expect(e, self.local(*l)?, "assignment"),
            StmtKind::Call { dst, func, args } => {
                let callee = self.m.funcs.get(func.0 as usize).ok_or("callee is out of range")?;
                if args.len() != callee.params {
                    return Err(format!("{} takes {} arguments, got {}", callee.name, callee.params, args.len()));
                }
                for (a, p) in args.iter().zip(&callee.locals) {
                    self.expect(a, p.ty, "argument")?;
                }
                match (dst, callee.ret) {
                    (Some(d), Some(r)) if self.local(*d)? != r => Err(format!("result of {} has the wrong type", callee.name)),
                    (Some(_), None) => Err(format!("{} returns nothing", callee.name)),
                    _ => Ok(()),
                }
            }
            StmtKind::Op { dst, op, args } => {
                let dst_ty = dst.map(|d| self.local(d)).transpose()?;
                match op {
                    RtOp::Print => {
                        if dst.is_some() || args.is_empty() {
                            return Err("print takes parts and writes nowhere".into());
                        }
                        for a in args {
                            self.ty(a)?;
                        }
                    }
                    RtOp::Format => {
                        for a in args {
                            self.ty(a)?;
                        }
                        if dst_ty != Some(Ty::Str) {
                            return Err("format must write a str".into());
                        }
                    }
                    RtOp::DivInt | RtOp::RemInt => {
                        if args.len() != 2 || dst_ty != Some(Ty::Int) {
                            return Err(format!("{} takes two ints and writes an int", op.name()));
                        }
                        self.expect(&args[0], Ty::Int, "dividend")?;
                        self.expect(&args[1], Ty::Int, "divisor")?;
                    }
                    RtOp::FloatToInt => {
                        if args.len() != 1 || dst_ty != Some(Ty::Int) {
                            return Err("float_to_int takes one float and writes an int".into());
                        }
                        self.expect(&args[0], Ty::Float, "float_to_int operand")?;
                    }
                }
                Ok(())
            }
            StmtKind::If { cond, then, els } => {
                self.expect(cond, Ty::Bool, "if condition")?;
                self.stmts(then)?;
                self.stmts(els)
            }
            StmtKind::Loop { head, cond, body, step } => {
                self.stmts(head)?;
                self.expect(cond, Ty::Bool, "loop condition")?;
                self.stmts(body)?;
                self.stmts(step)
            }
            StmtKind::Return(v) => match (v, self.f.ret) {
                (None, None) => Ok(()),
                (Some(e), Some(t)) => self.expect(e, t, "return value"),
                _ => Err("return value does not match the function".into()),
            },
        }
    }
}
