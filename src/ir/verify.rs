//! Checks that an IR module is well formed: ids in range, operand and call types,
//! and that functions with a result return on every path. A failure is a compiler bug.

use super::{Arg, BinOp, Expr, Func, LocalId, Module, Place, PureFn, RtOp, Step, Stmt, StmtKind, Structs, Ty, UnOp};

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

    /// The element type of an array operand.
    fn elem(&self, e: &Expr, what: &str) -> Result<Ty, String> {
        let t = self.ty(e)?;
        t.elem().ok_or_else(|| format!("{what}: expected an array, found {}", t.name()))
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
                match op.operand() {
                    Some(t) => {
                        self.expect(a, t, "left operand")?;
                        self.expect(b, t, "right operand")?;
                    }
                    None => {
                        let t = self.ty(a)?;
                        if !Structs::aggregate(t) {
                            return Err(format!("deep equality on {}", t.name()));
                        }
                        self.expect(b, t, "right operand")?;
                    }
                }
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
            Expr::Field(x, k, t) => {
                let st = self.ty(x)?;
                let info = self.m.structs.get(st).ok_or_else(|| format!("field of {}", st.name()))?;
                match info.fields.get(*k as usize) {
                    Some((_, ft)) if ft == t => *t,
                    _ => return Err(format!("{} has no field #{k} of type {}", st.name(), t.name())),
                }
            }
            Expr::Pure(p, args) => {
                let (params, ret) = p.sig();
                match p {
                    PureFn::ArrLen | PureFn::ArrContains | PureFn::ArrIndexOf => {
                        let want = if *p == PureFn::ArrLen { 1 } else { 2 };
                        if args.len() != want {
                            return Err(format!("{} takes {want} operands", p.name()));
                        }
                        let elem = self.elem(&args[0], p.name())?;
                        if want == 2 {
                            self.expect(&args[1], elem, p.name())?;
                        }
                    }
                    _ => {
                        if args.len() != params.len() {
                            return Err(format!("{} takes {} operands", p.name(), params.len()));
                        }
                        for (a, t) in args.iter().zip(params) {
                            self.expect(a, *t, p.name())?;
                        }
                    }
                }
                ret
            }
        })
    }

    /// The type of the value a place names, checking its steps.
    fn place(&self, p: &Place) -> Result<Ty, String> {
        let mut t = self.local(p.root)?;
        for s in &p.path {
            t = match s {
                Step::Index(i, _) => {
                    self.expect(i, Ty::Int, "index")?;
                    t.elem().ok_or_else(|| format!("index into {}", t.name()))?
                }
                Step::Field(k) => {
                    let info = self.m.structs.get(t).ok_or_else(|| format!("field of {}", t.name()))?;
                    info.fields.get(*k as usize).map(|f| f.1).ok_or("field out of range")?
                }
            };
        }
        Ok(t)
    }

    fn stmts(&self, ss: &[Stmt], loops: usize) -> Result<(), String> {
        for s in ss {
            self.stmt(s, loops).map_err(|e| format!("{e} (line {})", s.span.line))?;
        }
        Ok(())
    }

    fn args(&self, args: &[Expr], want: &[Ty], what: &str) -> Result<(), String> {
        if args.len() != want.len() {
            return Err(format!("{what} takes {} operands, got {}", want.len(), args.len()));
        }
        for (a, t) in args.iter().zip(want) {
            self.expect(a, *t, what)?;
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
                    match a {
                        Arg::Val(e) if !p.inout => self.expect(e, p.ty, "argument")?,
                        Arg::InOut(place) if p.inout => {
                            if self.place(place)? != p.ty {
                                return Err(format!("inout argument of {} has the wrong type", callee.name));
                            }
                        }
                        _ => return Err(format!("argument kinds of {} do not match its `inout` parameters", callee.name)),
                    }
                }
                match (dst, callee.ret) {
                    (Some(d), Some(r)) if self.local(*d)? != r => Err(format!("result of {} has the wrong type", callee.name)),
                    (Some(_), None) => Err(format!("{} returns nothing", callee.name)),
                    _ => Ok(()),
                }
            }
            StmtKind::Op { dst, op, args } => {
                if op.mutates() {
                    return Err(format!("{} changes a place: it must be a `Mutate`", op.name()));
                }
                let dst_ty = dst.map(|d| self.local(d)).transpose()?;
                let ret = match op {
                    RtOp::Print | RtOp::Format => {
                        if args.is_empty() && *op == RtOp::Print {
                            return Err("print needs parts".into());
                        }
                        for a in args {
                            self.ty(a)?;
                        }
                        op.sig().1
                    }
                    RtOp::StrChars | RtOp::StrCodes => {
                        self.args(args, &[Ty::Str], op.name())?;
                        Some(Ty::array(if *op == RtOp::StrChars { Ty::Char } else { Ty::Int }))
                    }
                    RtOp::StrSplit => {
                        self.args(args, &[Ty::Str, Ty::Str], op.name())?;
                        Some(Ty::array(Ty::Str))
                    }
                    RtOp::ArrNew => {
                        // the element type comes from the destination (`[]` has no elements)
                        let t = dst_ty.ok_or("arr_new needs a destination")?;
                        let elem = t.elem().ok_or("arr_new must write an array")?;
                        for a in args {
                            self.expect(a, elem, "array element")?;
                        }
                        Some(t)
                    }
                    RtOp::StructNew => {
                        let t = dst_ty.ok_or("struct_new needs a destination")?;
                        let info = self.m.structs.get(t).ok_or("struct_new must write a struct")?;
                        let fields: Vec<Ty> = info.fields.iter().map(|f| f.1).collect();
                        self.args(args, &fields, "struct field")?;
                        Some(t)
                    }
                    RtOp::ArrGet => {
                        let elem = self.elem(args.first().ok_or("arr_get needs operands")?, "arr_get")?;
                        self.args(&args[1..], &[Ty::Int], "arr_get index")?;
                        Some(elem)
                    }
                    RtOp::ArrSlice | RtOp::ArrRepeat => {
                        let t = self.ty(args.first().ok_or("missing array")?)?;
                        self.elem(&args[0], op.name())?;
                        let rest: &[Ty] = if *op == RtOp::ArrSlice { &[Ty::Int, Ty::Int] } else { &[Ty::Int] };
                        self.args(&args[1..], rest, op.name())?;
                        Some(t)
                    }
                    RtOp::ArrConcat => {
                        let t = self.ty(args.first().ok_or("missing array")?)?;
                        self.elem(&args[0], op.name())?;
                        self.args(&args[1..], &[t], op.name())?;
                        Some(t)
                    }
                    RtOp::ArrJoin => {
                        let elem = self.elem(args.first().ok_or("missing array")?, op.name())?;
                        if !matches!(elem, Ty::Str | Ty::Char) {
                            return Err("join needs [str] or [char]".into());
                        }
                        self.args(&args[1..], &[Ty::Str], op.name())?;
                        Some(Ty::Str)
                    }
                    _ => {
                        let (params, ret) = op.sig();
                        self.args(args, params, op.name())?;
                        ret
                    }
                };
                if dst_ty.is_some() && dst_ty != ret {
                    return Err(format!("{} writes the wrong type", op.name()));
                }
                Ok(())
            }
            StmtKind::Store { place, value } => {
                if place.path.is_empty() {
                    return Err("a store needs a path (a whole local is `Set`)".into());
                }
                let t = self.place(place)?;
                self.expect(value, t, "stored value")
            }
            StmtKind::Mutate { dst, op, place, args } => {
                let t = self.place(place)?;
                let dst_ty = dst.map(|d| self.local(d)).transpose()?;
                let ret = match op {
                    RtOp::StrAppend => {
                        if t != Ty::Str {
                            return Err("str_append on a non-string".into());
                        }
                        self.args(args, &[Ty::Str], "str_append")?;
                        None
                    }
                    _ => {
                        let elem = t.elem().ok_or_else(|| format!("{} on {}", op.name(), t.name()))?;
                        let (want, ret): (Vec<Ty>, Option<Ty>) = match op {
                            RtOp::ArrPush => (vec![elem], None),
                            RtOp::ArrPop => (vec![], Some(elem)),
                            RtOp::ArrInsert => (vec![Ty::Int, elem], None),
                            RtOp::ArrRemove => (vec![Ty::Int], Some(elem)),
                            RtOp::ArrSort | RtOp::ArrReverse => (vec![], None),
                            RtOp::ArrAppend => (vec![t], None),
                            _ => return Err(format!("{} does not change a place", op.name())),
                        };
                        if *op == RtOp::ArrSort && !matches!(elem, Ty::Int | Ty::Float | Ty::Str | Ty::Char) {
                            return Err(format!("sort of [{}]", elem.name()));
                        }
                        self.args(args, &want, op.name())?;
                        ret
                    }
                };
                if dst_ty.is_some() && dst_ty != ret {
                    return Err(format!("{} writes the wrong type", op.name()));
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
                let it = self.ty(iter)?;
                let want = match it {
                    Ty::Str => Ty::Char,
                    t => t.elem().ok_or_else(|| format!("for-each over {}", t.name()))?,
                };
                if !matches!(iter, Expr::Local(_)) {
                    return Err("a for-each loop goes over a local it owns".into());
                }
                if self.local(*var)? != want {
                    return Err("the for-each variable has the wrong type".into());
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
                if self.m.managed(self.local(*l)?) {
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
