//! Checks that an IR module is well formed: ids in range, operand and call types,
//! and that functions with a result return on every path. A failure is a compiler bug.

use super::{managed, BinOp, Expr, Func, LocalId, Module, RtOp, Stmt, StmtKind, Ty, UnOp};

pub fn verify(m: &Module) -> Result<(), String> {
    if m.main.0 as usize >= m.funcs.len() {
        return Err("`main` is out of range".into());
    }
    for f in &m.funcs {
        let v = Verifier { m, f };
        v.stmts(&f.body, 0).map_err(|e| format!("in fn {} (line {}): {e}", f.name, f.span.line))?;
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
            Expr::Char(_) => Ty::Char,
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
            Expr::Pure(p, args) => {
                let (params, ret) = p.sig();
                if args.len() != params.len() {
                    return Err(format!("{} takes {} operands", p.name(), params.len()));
                }
                for (a, t) in args.iter().zip(params) {
                    self.expect(a, *t, p.name())?;
                }
                ret
            }
        })
    }

    fn stmts(&self, ss: &[Stmt], loops: usize) -> Result<(), String> {
        for s in ss {
            self.stmt(s, loops).map_err(|e| format!("{e} (line {})", s.span.line))?;
        }
        Ok(())
    }

    fn stmt(&self, s: &Stmt, loops: usize) -> Result<(), String> {
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
                let (params, ret) = op.sig();
                match op {
                    RtOp::Print | RtOp::Format => {
                        if args.is_empty() && *op == RtOp::Print {
                            return Err("print needs parts".into());
                        }
                        for a in args {
                            self.ty(a)?;
                        }
                    }
                    _ => {
                        if args.len() != params.len() {
                            return Err(format!("{} takes {} operands", op.name(), params.len()));
                        }
                        for (a, t) in args.iter().zip(params) {
                            self.expect(a, *t, op.name())?;
                        }
                    }
                }
                if dst_ty != ret {
                    return Err(format!("{} writes the wrong type", op.name()));
                }
                if *op == RtOp::StrAppend && args.first().and_then(|a| if let Expr::Local(l) = a { Some(*l) } else { None }) != *dst {
                    return Err("str_append must append to its own destination".into());
                }
                Ok(())
            }
            StmtKind::If { cond, then, els } => {
                self.expect(cond, Ty::Bool, "if condition")?;
                self.stmts(then, loops)?;
                self.stmts(els, loops)
            }
            StmtKind::Loop { head, cond, body, step } => {
                self.stmts(head, loops + 1)?;
                self.expect(cond, Ty::Bool, "loop condition")?;
                self.stmts(body, loops + 1)?;
                self.stmts(step, loops + 1)
            }
            StmtKind::ForEach { var, iter, body } => {
                self.expect(iter, Ty::Str, "for-each iterable")?;
                if self.local(*var)? != Ty::Char {
                    return Err("for-each over a string needs a `char` variable".into());
                }
                self.stmts(body, loops + 1)
            }
            StmtKind::Break | StmtKind::Continue => {
                if loops == 0 {
                    Err("break/continue outside a loop".into())
                } else {
                    Ok(())
                }
            }
            StmtKind::Dup(l) | StmtKind::Drop(l) | StmtKind::Free(l) => {
                if managed(self.local(*l)?) {
                    Ok(())
                } else {
                    Err(format!("reference counting on a plain value (local %{})", l.0))
                }
            }
            StmtKind::Return(v) => match (v, self.f.ret) {
                (None, None) => Ok(()),
                (Some(e), Some(t)) => self.expect(e, t, "return value"),
                _ => Err("return value does not match the function".into()),
            },
        }
    }
}
