//! Intermediate representation between the type checker and the backends.
//!
//! A structured, typed tree. Every effect (calling a function, printing, building a
//! string, an operation that can fail at runtime) is its own statement, and statements
//! run in order. Expressions are pure: they cannot call user code, print, allocate or fail,
//! so a backend may emit them in any evaluation order (C!) and later passes may fold or drop
//! them. This is what makes evaluation order and runtime errors identical on every backend.
//!
//! Memory is explicit: values of managed types (strings, arrays, structs that contain them)
//! are reference counted, and lowering inserts `Dup` (one more owner) and `Drop` (one owner
//! less) statements. Writes into arrays first make them unique (copy on write). C executes
//! the counting; JavaScript marks values that have more than one owner as shared, so that a
//! write copies them first.

pub mod lower;
pub mod opt;
pub mod print;
pub mod verify;

use crate::ast::Span;
pub use crate::ast::Type as Ty;
pub use crate::stdlib::StdFn;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct FuncId(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct LocalId(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct StrId(pub u32);

/// A struct definition, for the backends and for `managed`.
pub struct StructInfo {
    pub name: String,
    pub fields: Vec<(String, Ty)>,
    /// A field owns heap memory (directly or through a nested struct).
    pub managed: bool,
}

/// The structs of a program by type id (`Ty::Struct(id)`), in an order where each struct comes
/// after every struct it contains by value (C needs that order for its typedefs).
#[derive(Default)]
pub struct Structs(pub Vec<(u32, StructInfo)>);

impl Structs {
    pub fn get(&self, t: Ty) -> Option<&StructInfo> {
        match t {
            Ty::Struct(id) => self.0.iter().find(|(i, _)| *i == id).map(|(_, s)| s),
            _ => None,
        }
    }

    /// True for types whose values own heap memory: strings, arrays, and structs with such a field.
    pub fn managed(&self, t: Ty) -> bool {
        match t {
            Ty::Str | Ty::Array(_) => true,
            Ty::Struct(_) => self.get(t).is_some_and(|s| s.managed),
            _ => false,
        }
    }

    /// True for aggregates: arrays and structs (deep equality, copy on write in JavaScript).
    pub fn aggregate(t: Ty) -> bool {
        matches!(t, Ty::Array(_) | Ty::Struct(_))
    }
}

pub struct Module {
    pub funcs: Vec<Func>,
    /// Interned string literals (ids are indexes).
    pub strs: Vec<String>,
    pub main: FuncId,
    pub structs: Structs,
}

impl Module {
    pub fn func(&self, id: FuncId) -> &Func {
        &self.funcs[id.0 as usize]
    }

    pub fn str(&self, id: StrId) -> &str {
        &self.strs[id.0 as usize]
    }

    pub fn managed(&self, t: Ty) -> bool {
        self.structs.managed(t)
    }

    /// True if some statement of the program runs an operation for which `f` is true (a backend
    /// adds a part of its runtime only when the program needs it).
    pub fn uses(&self, f: &dyn Fn(RtOp) -> bool) -> bool {
        fn any(ss: &[Stmt], f: &dyn Fn(RtOp) -> bool) -> bool {
            ss.iter().any(|s| match &s.kind {
                StmtKind::Op { op, .. } | StmtKind::Mutate { op, .. } => f(*op),
                StmtKind::If { then, els, .. } => any(then, f) || any(els, f),
                StmtKind::Loop { head, body, step, .. } => any(head, f) || any(body, f) || any(step, f),
                StmtKind::ForEach { body, .. } => any(body, f),
                _ => false,
            })
        }
        self.funcs.iter().any(|func| any(&func.body, f))
    }

    /// True if the program calls the standard library (`RtOp::Std`).
    pub fn uses_std(&self) -> bool {
        self.uses(&|op| matches!(op, RtOp::Std(_)))
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
    /// An `inout` parameter: the local is the caller's place (C: a pointer, JS: a box).
    pub inout: bool,
}

/// A call argument: a value (borrowed by the callee), or a place for an `inout` parameter.
#[derive(Clone, Debug)]
pub enum Arg {
    Val(Expr),
    InOut(Place),
}

pub struct Stmt {
    pub kind: StmtKind,
    /// Source position, used by runtime errors.
    pub span: Span,
}

/// Where a write goes: a local, then elements and fields. Index expressions are pure and
/// already evaluated (constants or temporaries).
#[derive(Clone, Debug)]
pub struct Place {
    pub root: LocalId,
    pub path: Vec<Step>,
}

#[derive(Clone, Debug)]
pub enum Step {
    /// `[i]` of an array; the span is the `[` (an index out of bounds is runtime error E0240).
    Index(Expr, Span),
    /// A field of a struct, by position.
    Field(u32),
}

impl Place {
    pub fn local(root: LocalId) -> Place {
        Place { root, path: Vec::new() }
    }
}

pub enum StmtKind {
    /// Assign a local (no reference counting: `Dup`/`Drop` are separate statements).
    Set(LocalId, Expr),
    Call { dst: Option<LocalId>, func: FuncId, args: Vec<Arg> },
    Op { dst: Option<LocalId>, op: RtOp, args: Vec<Expr> },
    /// `place = value` for a place below a local (a whole local is `Set`). Every array on the
    /// way is made unique first (copy on write) and every index is checked. The value is
    /// borrowed: the place becomes one more owner, and its old value has one owner less.
    Store { place: Place, value: Expr },
    /// An operation that changes a place in place: `xs.push(v)`, `xs.pop()`, `xs += ys`,
    /// `s += t`. The place is made unique first, like for a `Store`. `dst` gets the result.
    Mutate { dst: Option<LocalId>, op: RtOp, place: Place, args: Vec<Expr> },
    If { cond: Expr, then: Vec<Stmt>, els: Vec<Stmt> },
    /// Each round: run `head`, leave if `cond` is false, run `body`, then `step`.
    /// `continue` goes to `step`.
    Loop { head: Vec<Stmt>, cond: Expr, body: Vec<Stmt>, step: Vec<Stmt> },
    /// `for var in iter`: a string gives each `char`, an array each element. `iter` is a local
    /// the loop owns, so the body may change the variable it came from; `var` borrows from it.
    ForEach { var: LocalId, iter: Expr, body: Vec<Stmt> },
    Break,
    Continue,
    Return(Option<Expr>),
    /// One more owner for the value in a local (C: retain; JavaScript: mark an aggregate shared).
    Dup(LocalId),
    /// One owner less (C: release; frees at zero).
    Drop(LocalId),
    /// `free(x)`: release now and leave the local empty.
    Free(LocalId),
    /// `keep(x)`: the value and everything in it is never freed (C: reference count 0, the
    /// mark of literals; the leak check no longer counts it).
    Keep(LocalId),
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
    /// `s += t` (a `Mutate`): appends in place when the string has only one owner.
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
    /// `s.repeat(n)` (E0243 for n < 0, E0249 when too long), new string.
    StrRepeat,
    /// `int(s)` (E0244).
    StrToInt,
    /// `float(s)` (E0244).
    StrToFloat,
    /// `char(n)` (E0246).
    CharFrom,
    /// `s.chars()`: a new `[char]`.
    StrChars,
    /// `s.codes()`: a new `[int]`.
    StrCodes,
    /// `s.split(sep)` (E0243 for an empty separator): a new `[str]`.
    StrSplit,
    /// `[a, b, c]`: a new array; it becomes one more owner of each element.
    ArrNew,
    /// `xs[i]` (E0240). The element is borrowed: lowering adds a `Dup` for managed elements.
    ArrGet,
    /// `xs.slice(a, b)` (E0240), new array.
    ArrSlice,
    /// `xs.repeat(n)` (E0243, E0249), new array.
    ArrRepeat,
    /// `a + b` on arrays: a new array.
    ArrConcat,
    /// `xs.join(sep)` of `[str]` or `[char]`: a new string.
    ArrJoin,
    /// `xs.push(v)` (a `Mutate`); the array becomes one more owner of `v`.
    ArrPush,
    /// `xs.pop()` (a `Mutate`, E0242): the last element moves out into `dst` (owned).
    ArrPop,
    /// `xs.insert(i, v)` (a `Mutate`, E0240 unless 0 <= i <= len).
    ArrInsert,
    /// `xs.remove(i)` (a `Mutate`, E0240): the element moves out into `dst` (owned).
    ArrRemove,
    /// `xs.sort()` (a `Mutate`): stable merge sort, the same algorithm on every backend.
    ArrSort,
    ArrReverse,
    /// `xs += ys` (a `Mutate`): appends in place when the array has only one owner.
    ArrAppend,
    /// `xs.swap(i, j)` (a `Mutate`, E0240).
    ArrSwap,
    /// The `step` of a range: 0 is runtime error E0243. Writes nowhere.
    CheckStep,
    /// `s.pad_left(n, c)` / `s.pad_right(n, c)`: `c` added until `s` has `n` characters (never shorter).
    StrPadLeft,
    StrPadRight,
    /// `Point(x: 1, y: 2)`: the fields in declaration order; the struct becomes one more owner
    /// of each managed field value.
    StructNew,
    /// A standard library function (`fs.read(path)`): the runtime function `std_<module>_<name>`
    /// of the backend, called with the operands and the position (a failure is a runtime error).
    Std(StdFn),
    /// `json.str(v)`: the JSON text of a value of any type (`dst: str`, owned).
    JsonStr,
    /// `json.parse(text)`: a value of the destination's type read from JSON text (E0345), owned.
    JsonParse,
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
            RtOp::StrChars => "str_chars",
            RtOp::StrCodes => "str_codes",
            RtOp::StrSplit => "str_split",
            RtOp::ArrNew => "arr_new",
            RtOp::ArrGet => "arr_get",
            RtOp::ArrSlice => "arr_slice",
            RtOp::ArrRepeat => "arr_repeat",
            RtOp::ArrConcat => "arr_concat",
            RtOp::ArrJoin => "arr_join",
            RtOp::ArrPush => "arr_push",
            RtOp::ArrPop => "arr_pop",
            RtOp::ArrInsert => "arr_insert",
            RtOp::ArrRemove => "arr_remove",
            RtOp::ArrSort => "arr_sort",
            RtOp::ArrReverse => "arr_reverse",
            RtOp::ArrAppend => "arr_append",
            RtOp::ArrSwap => "arr_swap",
            RtOp::CheckStep => "check_step",
            RtOp::StrPadLeft => "str_pad_left",
            RtOp::StrPadRight => "str_pad_right",
            RtOp::StructNew => "struct_new",
            RtOp::Std(f) => f.full_name(),
            RtOp::JsonStr => "json_str",
            RtOp::JsonParse => "json_parse",
        }
    }

    /// The operand types and the result type (`None`: writes nowhere) of the operations with
    /// fixed types. Array operations are checked by `verify` against the element type.
    pub fn sig(self) -> (&'static [Ty], Option<Ty>) {
        use Ty::{Char, Float, Int, Str};
        match self {
            RtOp::Print | RtOp::Format => (&[], if self == RtOp::Format { Some(Str) } else { None }),
            RtOp::DivInt | RtOp::RemInt => (&[Int, Int], Some(Int)),
            RtOp::FloatToInt => (&[Float], Some(Int)),
            RtOp::StrConcat => (&[Str, Str], Some(Str)),
            RtOp::StrAppend => (&[Str], None),
            RtOp::StrAt => (&[Str, Int], Some(Char)),
            RtOp::StrSlice => (&[Str, Int, Int], Some(Str)),
            RtOp::StrReplace => (&[Str, Str, Str], Some(Str)),
            RtOp::StrTrim | RtOp::StrUpper | RtOp::StrLower => (&[Str], Some(Str)),
            RtOp::StrRepeat => (&[Str, Int], Some(Str)),
            RtOp::StrToInt => (&[Str], Some(Int)),
            RtOp::StrToFloat => (&[Str], Some(Float)),
            RtOp::CharFrom => (&[Int], Some(Char)),
            RtOp::StrPadLeft | RtOp::StrPadRight => (&[Str, Int, Char], Some(Str)),
            RtOp::CheckStep => (&[Int], None),
            _ => (&[], None),
        }
    }

    /// True if the result is a value the destination owns (it must be dropped when managed).
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
                | RtOp::StrChars
                | RtOp::StrCodes
                | RtOp::StrSplit
                | RtOp::ArrNew
                | RtOp::ArrSlice
                | RtOp::ArrRepeat
                | RtOp::ArrConcat
                | RtOp::ArrJoin
                | RtOp::ArrPop
                | RtOp::ArrRemove
                | RtOp::StructNew
                | RtOp::StrPadLeft
                | RtOp::StrPadRight
                | RtOp::JsonStr
                | RtOp::JsonParse
        ) || matches!(self, RtOp::Std(f) if f.owned())
    }

    /// True for the operations that change a place (`Mutate`).
    pub fn mutates(self) -> bool {
        matches!(
            self,
            RtOp::StrAppend
                | RtOp::ArrPush
                | RtOp::ArrPop
                | RtOp::ArrInsert
                | RtOp::ArrRemove
                | RtOp::ArrSort
                | RtOp::ArrReverse
                | RtOp::ArrAppend
                | RtOp::ArrSwap
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
    /// elements in an array
    ArrLen,
    /// `xs.contains(v)` (deep equality)
    ArrContains,
    /// `xs.index_of(v)`: the first index or -1
    ArrIndexOf,
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
            PureFn::ArrLen => "arr_len",
            PureFn::ArrContains => "arr_contains",
            PureFn::ArrIndexOf => "arr_index_of",
        }
    }

    /// The operand types (empty for the array functions, which `verify` checks itself) and the result.
    pub fn sig(self) -> (&'static [Ty], Ty) {
        use Ty::{Bool, Char, Int, Str};
        match self {
            PureFn::StrLen => (&[Str], Int),
            PureFn::StrContains | PureFn::StrStartsWith | PureFn::StrEndsWith => (&[Str, Str], Bool),
            PureFn::StrIndexOf => (&[Str, Str], Int),
            PureFn::CharCode => (&[Char], Int),
            PureFn::CharUpper | PureFn::CharLower => (&[Char], Char),
            PureFn::ArrLen | PureFn::ArrIndexOf => (&[], Int),
            PureFn::ArrContains => (&[], Bool),
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
    /// A field of a struct value, by position, with the field's type. Borrowed like a variable.
    Field(Box<Expr>, u32, Ty),
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
    /// arrays and structs: equal when every element or field is equal (no identity shortcut,
    /// so a NaN inside never equals itself, on every backend)
    DeepEq,
    DeepNe,
}

impl BinOp {
    /// The operand type (`None` for the deep equalities, which take any aggregate).
    pub fn operand(self) -> Option<Ty> {
        use BinOp::*;
        Some(match self {
            IAdd | ISub | IMul | IDiv | IRem | IEq | INe | ILt | ILe | IGt | IGe => Ty::Int,
            FAdd | FSub | FMul | FDiv | FEq | FNe | FLt | FLe | FGt | FGe => Ty::Float,
            BEq | BNe | And | Or => Ty::Bool,
            CEq | CNe | CLt | CLe | CGt | CGe => Ty::Char,
            SEq | SNe | SLt | SLe | SGt | SGe => Ty::Str,
            DeepEq | DeepNe => return None,
        })
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
            IEq | FEq | BEq | CEq | SEq | DeepEq => "==",
            INe | FNe | BNe | CNe | SNe | DeepNe => "!=",
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
            Expr::Field(_, _, t) => *t,
        }
    }

    /// True for a literal (it reads no local).
    pub fn is_const(&self) -> bool {
        matches!(self, Expr::Int(_) | Expr::Float(_) | Expr::Bool(_) | Expr::Char(_) | Expr::Str(_))
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
            StmtKind::Call { dst, args, .. } => {
                if let Some(d) = dst {
                    f(d);
                }
                for a in args {
                    match a {
                        Arg::Val(e) => expr_locals(e, f),
                        Arg::InOut(p) => place_locals(p, f),
                    }
                }
            }
            StmtKind::Op { dst, args, .. } => {
                if let Some(d) = dst {
                    f(d);
                }
                for a in args {
                    expr_locals(a, f);
                }
            }
            StmtKind::Store { place, value } => {
                place_locals(place, f);
                expr_locals(value, f);
            }
            StmtKind::Mutate { dst, place, args, .. } => {
                if let Some(d) = dst {
                    f(d);
                }
                place_locals(place, f);
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
            StmtKind::Dup(l) | StmtKind::Drop(l) | StmtKind::Free(l) | StmtKind::Keep(l) => f(l),
            StmtKind::Return(None) | StmtKind::Break | StmtKind::Continue => {}
        }
    }
}

fn place_locals(p: &mut Place, f: &mut dyn FnMut(&mut LocalId)) {
    f(&mut p.root);
    for s in &mut p.path {
        if let Step::Index(e, _) = s {
            expr_locals(e, f);
        }
    }
}

fn expr_locals(e: &mut Expr, f: &mut dyn FnMut(&mut LocalId)) {
    match e {
        Expr::Local(l) => f(l),
        Expr::Unary(_, x) | Expr::IntToFloat(x) | Expr::Field(x, _, _) => expr_locals(x, f),
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
