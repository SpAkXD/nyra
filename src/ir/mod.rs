//! Intermediate representation between the type checker and the backends.
//!
//! A structured, typed tree. Every effect (calling a function, printing, building a
//! string, an operation that can fail at runtime) is its own statement, and statements
//! run in order. Expressions are pure: they cannot call user code, print, allocate or fail,
//! so a backend may emit them in any evaluation order (C!) and later passes may fold or drop
//! them. This is what makes evaluation order and runtime errors identical on every backend.
//!
//! Memory is explicit: values of managed types (strings) are reference counted, and lowering
//! inserts `Dup` (retain) and `Drop` (release) statements. C executes them; JS ignores them.

pub mod lower;
pub mod opt;
pub mod print;
pub mod verify;

use crate::ast::Span;
pub use crate::ast::Type as Ty;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct FuncId(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct LocalId(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct StrId(pub u32);

/// True for types whose values own heap memory and are reference counted.
pub fn managed(t: Ty) -> bool {
    matches!(t, Ty::Str)
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
    /// Assign a local (no reference counting: `Dup`/`Drop` are separate statements).
    Set(LocalId, Expr),
    Call { dst: Option<LocalId>, func: FuncId, args: Vec<Expr> },
    Op { dst: Option<LocalId>, op: RtOp, args: Vec<Expr> },
    If { cond: Expr, then: Vec<Stmt>, els: Vec<Stmt> },
    /// Each round: run `head`, leave if `cond` is false, run `body`, then `step`.
    /// `continue` goes to `step`.
    Loop { head: Vec<Stmt>, cond: Expr, body: Vec<Stmt>, step: Vec<Stmt> },
    /// `for var in iter`: `iter` is a string (`var` gets each `char`).
    ForEach { var: LocalId, iter: Expr, body: Vec<Stmt> },
    Break,
    Continue,
    Return(Option<Expr>),
    /// One more owner for the value in a local (C: retain).
    Dup(LocalId),
    /// One owner less (C: release; frees at zero).
    Drop(LocalId),
    /// `free(x)`: release now and leave the local empty.
    Free(LocalId),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RtOp {
    /// Prints the parts and a newline, without building a string.
    Print,
    /// Builds a new string from the parts (interpolation, `str(x)`). `dst: str`, owned.
    Format,
    /// int `/` whose divisor may be 0 (runtime error E0241). `dst: int`.
    DivInt,
    /// int `%` whose divisor may be 0 (runtime error E0241). `dst: int`.
    RemInt,
    /// `int(x)` of a float: NaN or out of range is runtime error E0245. `dst: int`.
    FloatToInt,
    /// `a + b` on strings: a new string.
    StrConcat,
    /// `s += t`: appends to the local `dst` (also `args[0]`) in place when it is the only owner.
    StrAppend,
    /// `s[i]`: the i-th character (E0240).
    StrAt,
    /// `s.slice(a, b)` (E0240), new string.
    StrSlice,
    /// `s.replace(old, new)` (E0243 for an empty `old`), new string.
    StrReplace,
    StrTrim,
    StrUpper,
    StrLower,
    /// `s.repeat(n)` (E0243 for n < 0), new string.
    StrRepeat,
    /// `int(s)` (E0244).
    StrToInt,
    /// `float(s)` (E0244).
    StrToFloat,
    /// `char(n)` (E0246).
    CharFrom,
}

impl RtOp {
    pub fn name(self) -> &'static str {
        match self {
            RtOp::Print => "print",
            RtOp::Format => "format",
            RtOp::DivInt => "div_int",
            RtOp::RemInt => "rem_int",
            RtOp::FloatToInt => "float_to_int",
            RtOp::StrConcat => "str_concat",
            RtOp::StrAppend => "str_append",
            RtOp::StrAt => "str_at",
            RtOp::StrSlice => "str_slice",
            RtOp::StrReplace => "str_replace",
            RtOp::StrTrim => "str_trim",
            RtOp::StrUpper => "str_upper",
            RtOp::StrLower => "str_lower",
            RtOp::StrRepeat => "str_repeat",
            RtOp::StrToInt => "str_to_int",
            RtOp::StrToFloat => "str_to_float",
            RtOp::CharFrom => "char_from",
        }
    }

    /// The operand types and the result type (`None`: writes nowhere).
    pub fn sig(self) -> (&'static [Ty], Option<Ty>) {
        use Ty::{Char, Float, Int, Str};
        match self {
            RtOp::Print | RtOp::Format => (&[], if self == RtOp::Format { Some(Str) } else { None }),
            RtOp::DivInt | RtOp::RemInt => (&[Int, Int], Some(Int)),
            RtOp::FloatToInt => (&[Float], Some(Int)),
            RtOp::StrConcat | RtOp::StrAppend => (&[Str, Str], Some(Str)),
            RtOp::StrAt => (&[Str, Int], Some(Char)),
            RtOp::StrSlice => (&[Str, Int, Int], Some(Str)),
            RtOp::StrReplace => (&[Str, Str, Str], Some(Str)),
            RtOp::StrTrim | RtOp::StrUpper | RtOp::StrLower => (&[Str], Some(Str)),
            RtOp::StrRepeat => (&[Str, Int], Some(Str)),
            RtOp::StrToInt => (&[Str], Some(Int)),
            RtOp::StrToFloat => (&[Str], Some(Float)),
            RtOp::CharFrom => (&[Int], Some(Char)),
        }
    }

    /// True if the result is a new value the destination owns (it must be dropped).
    pub fn owned_result(self) -> bool {
        matches!(
            self,
            RtOp::Format
                | RtOp::StrConcat
                | RtOp::StrSlice
                | RtOp::StrReplace
                | RtOp::StrTrim
                | RtOp::StrUpper
                | RtOp::StrLower
                | RtOp::StrRepeat
        )
    }
}

/// Pure functions of the runtime: no allocation, no failure, no effect.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PureFn {
    /// characters in a string
    StrLen,
    StrContains,
    StrStartsWith,
    StrEndsWith,
    /// character position or -1
    StrIndexOf,
    CharCode,
    CharUpper,
    CharLower,
    CharIsDigit,
    CharIsLetter,
    CharIsUpper,
    CharIsLower,
    CharIsSpace,
}

impl PureFn {
    pub fn name(self) -> &'static str {
        match self {
            PureFn::StrLen => "str_len",
            PureFn::StrContains => "str_contains",
            PureFn::StrStartsWith => "str_starts_with",
            PureFn::StrEndsWith => "str_ends_with",
            PureFn::StrIndexOf => "str_index_of",
            PureFn::CharCode => "char_code",
            PureFn::CharUpper => "char_upper",
            PureFn::CharLower => "char_lower",
            PureFn::CharIsDigit => "char_is_digit",
            PureFn::CharIsLetter => "char_is_letter",
            PureFn::CharIsUpper => "char_is_upper",
            PureFn::CharIsLower => "char_is_lower",
            PureFn::CharIsSpace => "char_is_space",
        }
    }

    pub fn sig(self) -> (&'static [Ty], Ty) {
        use Ty::{Bool, Char, Int, Str};
        match self {
            PureFn::StrLen => (&[Str], Int),
            PureFn::StrContains | PureFn::StrStartsWith | PureFn::StrEndsWith => (&[Str, Str], Bool),
            PureFn::StrIndexOf => (&[Str, Str], Int),
            PureFn::CharCode => (&[Char], Int),
            PureFn::CharUpper | PureFn::CharLower => (&[Char], Char),
            _ => (&[Char], Bool),
        }
    }
}

/// A pure expression.
#[derive(Clone, Debug)]
pub enum Expr {
    Int(i64),
    Float(f64),
    Bool(bool),
    Char(u32),
    Str(StrId),
    Local(LocalId),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    /// `if c { a } else { b }` with pure branches.
    Select(Box<Expr>, Box<Expr>, Box<Expr>),
    IntToFloat(Box<Expr>),
    Pure(PureFn, Vec<Expr>),
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
    /// characters compare by code
    CEq,
    CNe,
    CLt,
    CLe,
    CGt,
    CGe,
    /// strings: equality by value, order by code points
    SEq,
    SNe,
    SLt,
    SLe,
    SGt,
    SGe,
}

impl BinOp {
    pub fn operand(self) -> Ty {
        use BinOp::*;
        match self {
            IAdd | ISub | IMul | IDiv | IRem | IEq | INe | ILt | ILe | IGt | IGe => Ty::Int,
            FAdd | FSub | FMul | FDiv | FEq | FNe | FLt | FLe | FGt | FGe => Ty::Float,
            BEq | BNe | And | Or => Ty::Bool,
            CEq | CNe | CLt | CLe | CGt | CGe => Ty::Char,
            SEq | SNe | SLt | SLe | SGt | SGe => Ty::Str,
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
            IEq | FEq | BEq | CEq | SEq => "==",
            INe | FNe | BNe | CNe | SNe => "!=",
            ILt | FLt | CLt | SLt => "<",
            ILe | FLe | CLe | SLe => "<=",
            IGt | FGt | CGt | SGt => ">",
            IGe | FGe | CGe | SGe => ">=",
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
            Expr::Char(_) => Ty::Char,
            Expr::Str(_) => Ty::Str,
            Expr::Local(l) => f.local(*l).ty,
            Expr::Unary(UnOp::INeg, _) => Ty::Int,
            Expr::Unary(UnOp::FNeg, _) => Ty::Float,
            Expr::Unary(UnOp::Not, _) => Ty::Bool,
            Expr::Binary(op, _, _) => op.result(),
            Expr::Select(_, a, _) => a.ty(f),
            Expr::Pure(p, _) => p.sig().1,
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
            StmtKind::ForEach { var, iter, body } => {
                f(var);
                expr_locals(iter, f);
                visit_locals(body, f);
            }
            StmtKind::Return(Some(e)) => expr_locals(e, f),
            StmtKind::Dup(l) | StmtKind::Drop(l) | StmtKind::Free(l) => f(l),
            StmtKind::Return(None) | StmtKind::Break | StmtKind::Continue => {}
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
        Expr::Pure(_, args) => {
            for a in args {
                expr_locals(a, f);
            }
        }
        Expr::Int(_) | Expr::Float(_) | Expr::Bool(_) | Expr::Char(_) | Expr::Str(_) => {}
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
            let Ok(mut m) = super::lower::lower(&prog) else { continue };
            super::verify::verify(&m).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            super::opt::optimize(&mut m);
            super::verify::verify(&m).unwrap_or_else(|e| panic!("{} after optimizing: {e}", path.display()));
            assert!(!super::print::print(&m).is_empty());
            checked += 1;
        }
        assert!(checked > 0);
    }
}
