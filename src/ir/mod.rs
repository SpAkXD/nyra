//! Intermediate representation between the type checker and the backends.
//!
//! A structured, typed tree. Every effect (calling a function, printing, building a
//! string, an operation that can fail at runtime) is its own statement, and statements
//! run in order. Expressions are pure: they cannot call, print, allocate or fail, so a
//! backend may emit them in any evaluation order (C!) and later passes may fold or drop
//! them. This is what makes evaluation order and runtime errors identical on every backend.

pub mod lower;
pub mod print;
pub mod verify;

use crate::ast::Span;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct FuncId(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct LocalId(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct StrId(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ty {
    Int,
    Float,
    Bool,
    Str,
}

impl Ty {
    pub fn name(self) -> &'static str {
        match self {
            Ty::Int => "int",
            Ty::Float => "float",
            Ty::Bool => "bool",
            Ty::Str => "str",
        }
    }
}

pub struct Module {
    pub funcs: Vec<Func>,
    /// Interned string literals (ids are indexes).
    pub strs: Vec<String>,
    pub main: FuncId,
}

impl Module {
    pub fn func(&self, id: FuncId) -> &Func {
        &self.funcs[id.0 as usize]
    }

    pub fn str(&self, id: StrId) -> &str {
        &self.strs[id.0 as usize]
    }
}

pub struct Func {
    pub name: String,
    /// `locals[..params]` are the parameters.
    pub params: usize,
    /// `None`: returns nothing.
    pub ret: Option<Ty>,
    /// Every variable and temporary of the function (function-wide, declared up front).
    pub locals: Vec<Local>,
    pub body: Vec<Stmt>,
    pub span: Span,
}

impl Func {
    pub fn local(&self, id: LocalId) -> &Local {
        &self.locals[id.0 as usize]
    }
}

pub struct Local {
    /// The Nyra name; `None` for a compiler temporary.
    pub name: Option<String>,
    pub ty: Ty,
}

pub struct Stmt {
    pub kind: StmtKind,
    /// Source position, used by runtime errors.
    pub span: Span,
}

pub enum StmtKind {
    /// Declare-or-assign a local.
    Set(LocalId, Expr),
    Call { dst: Option<LocalId>, func: FuncId, args: Vec<Expr> },
    Op { dst: Option<LocalId>, op: RtOp, args: Vec<Expr> },
    If { cond: Expr, then: Vec<Stmt>, els: Vec<Stmt> },
    /// Each round: run `head`, leave if `cond` is false, run `body`, then `step`.
    Loop { head: Vec<Stmt>, cond: Expr, body: Vec<Stmt>, step: Vec<Stmt> },
    Return(Option<Expr>),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RtOp {
    /// Prints the parts (int/float/bool/str) and a newline, without building a string.
    Print,
    /// Builds a string from the parts (interpolation). `dst: str`.
    Format,
    /// int `/` whose divisor may be 0 (runtime error E0241). `dst: int`.
    DivInt,
    /// int `%` whose divisor may be 0 (runtime error E0241). `dst: int`.
    RemInt,
    /// `int(x)` of a float: NaN or out of range is runtime error E0245. `dst: int`.
    FloatToInt,
}

impl RtOp {
    pub fn name(self) -> &'static str {
        match self {
            RtOp::Print => "print",
            RtOp::Format => "format",
            RtOp::DivInt => "div_int",
            RtOp::RemInt => "rem_int",
            RtOp::FloatToInt => "float_to_int",
        }
    }
}

/// A pure expression.
#[derive(Clone, Debug)]
pub enum Expr {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(StrId),
    Local(LocalId),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    /// `if c { a } else { b }` with pure branches.
    Select(Box<Expr>, Box<Expr>, Box<Expr>),
    IntToFloat(Box<Expr>),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UnOp {
    /// Wraps on overflow.
    INeg,
    FNeg,
    Not,
}

/// Type-specific operators, so backends never need the AST's types.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BinOp {
    // int arithmetic wraps; IDiv/IRem only appear with a constant divisor other than 0 and -1
    IAdd,
    ISub,
    IMul,
    IDiv,
    IRem,
    FAdd,
    FSub,
    FMul,
    FDiv,
    IEq,
    INe,
    ILt,
    ILe,
    IGt,
    IGe,
    FEq,
    FNe,
    FLt,
    FLe,
    FGt,
    FGe,
    BEq,
    BNe,
    /// Both sides are pure, so no short-circuit is needed.
    And,
    Or,
    /// String value equality.
    SEq,
    SNe,
}

impl BinOp {
    pub fn operand(self) -> Ty {
        use BinOp::*;
        match self {
            IAdd | ISub | IMul | IDiv | IRem | IEq | INe | ILt | ILe | IGt | IGe => Ty::Int,
            FAdd | FSub | FMul | FDiv | FEq | FNe | FLt | FLe | FGt | FGe => Ty::Float,
            BEq | BNe | And | Or => Ty::Bool,
            SEq | SNe => Ty::Str,
        }
    }

    pub fn result(self) -> Ty {
        use BinOp::*;
        match self {
            IAdd | ISub | IMul | IDiv | IRem => Ty::Int,
            FAdd | FSub | FMul | FDiv => Ty::Float,
            _ => Ty::Bool,
        }
    }

    /// The operator as written in C and Nyra (JS uses `===`/`!==` for the equalities).
    pub fn symbol(self) -> &'static str {
        use BinOp::*;
        match self {
            IAdd | FAdd => "+",
            ISub | FSub => "-",
            IMul | FMul => "*",
            IDiv | FDiv => "/",
            IRem => "%",
            IEq | FEq | BEq | SEq => "==",
            INe | FNe | BNe | SNe => "!=",
            ILt | FLt => "<",
            ILe | FLe => "<=",
            IGt | FGt => ">",
            IGe | FGe => ">=",
            And => "&&",
            Or => "||",
        }
    }
}

impl Expr {
    pub fn ty(&self, f: &Func) -> Ty {
        match self {
            Expr::Int(_) => Ty::Int,
            Expr::Float(_) | Expr::IntToFloat(_) => Ty::Float,
            Expr::Bool(_) => Ty::Bool,
            Expr::Str(_) => Ty::Str,
            Expr::Local(l) => f.local(*l).ty,
            Expr::Unary(UnOp::INeg, _) => Ty::Int,
            Expr::Unary(UnOp::FNeg, _) => Ty::Float,
            Expr::Unary(UnOp::Not, _) => Ty::Bool,
            Expr::Binary(op, _, _) => op.result(),
            Expr::Select(_, a, _) => a.ty(f),
        }
    }
}

/// Calls `f` on every local that `stmts` (and the statements nested in them) read or write.
pub fn visit_locals(stmts: &mut [Stmt], f: &mut dyn FnMut(&mut LocalId)) {
    for s in stmts {
        match &mut s.kind {
            StmtKind::Set(l, e) => {
                f(l);
                expr_locals(e, f);
            }
            StmtKind::Call { dst, args, .. } | StmtKind::Op { dst, args, .. } => {
                if let Some(d) = dst {
                    f(d);
                }
                for a in args {
                    expr_locals(a, f);
                }
            }
            StmtKind::If { cond, then, els } => {
                expr_locals(cond, f);
                visit_locals(then, f);
                visit_locals(els, f);
            }
            StmtKind::Loop { head, cond, body, step } => {
                visit_locals(head, f);
                expr_locals(cond, f);
                visit_locals(body, f);
                visit_locals(step, f);
            }
            StmtKind::Return(Some(e)) => expr_locals(e, f),
            StmtKind::Return(None) => {}
        }
    }
}

fn expr_locals(e: &mut Expr, f: &mut dyn FnMut(&mut LocalId)) {
    match e {
        Expr::Local(l) => f(l),
        Expr::Unary(_, x) | Expr::IntToFloat(x) => expr_locals(x, f),
        Expr::Binary(_, a, b) => {
            expr_locals(a, f);
            expr_locals(b, f);
        }
        Expr::Select(c, a, b) => {
            expr_locals(c, f);
            expr_locals(a, f);
            expr_locals(b, f);
        }
        Expr::Int(_) | Expr::Float(_) | Expr::Bool(_) | Expr::Str(_) => {}
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_example_lowers_to_valid_ir() {
        let mut checked = 0;
        for entry in std::fs::read_dir("examples").unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|e| e != "nyra") {
                continue;
            }
            let src = std::fs::read_to_string(&path).unwrap();
            let prog = crate::compile(&src).unwrap_or_else(|d| panic!("{}: {d:?}", path.display()));
            let m = super::lower::lower(&prog);
            super::verify::verify(&m).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            assert!(!super::print::print(&m).is_empty());
            checked += 1;
        }
        assert!(checked > 0);
    }
}
