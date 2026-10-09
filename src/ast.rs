//! Syntax tree produced by the parser and annotated by the type checker.

use std::cell::RefCell;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    pub line: usize,
    pub col: usize,
}

/// A Nyra type. `Copy`: array element types and struct names are interned (see `Type::array`,
/// `Type::structure`), so `[[int]]` or `Point` is a small id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Type {
    Int,
    Float,
    Bool,
    Str,
    Char,
    Void,
    /// Produced after an error so one mistake doesn't cascade into many.
    Unknown,
    /// `[T]`; the id indexes the interned element types.
    Array(u32),
    /// A struct; the id indexes the interned struct names.
    Struct(u32),
    /// `[K: V]`; the id indexes the interned (key, value) pairs.
    Map(u32),
}

thread_local! {
    static ELEMS: RefCell<Vec<Type>> = const { RefCell::new(Vec::new()) };
    static STRUCTS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static MAPS: RefCell<Vec<(Type, Type)>> = const { RefCell::new(Vec::new()) };
}

impl Type {
    /// `[elem]`. An array of an unknown type is unknown.
    pub fn array(elem: Type) -> Type {
        if elem == Type::Unknown {
            return Type::Unknown;
        }
        ELEMS.with(|e| {
            let mut e = e.borrow_mut();
            let id = match e.iter().position(|t| *t == elem) {
                Some(i) => i,
                None => {
                    e.push(elem);
                    e.len() - 1
                }
            };
            Type::Array(id as u32)
        })
    }

    /// `[key: value]`. A map with an unknown part is unknown.
    pub fn map(key: Type, value: Type) -> Type {
        if key == Type::Unknown || value == Type::Unknown {
            return Type::Unknown;
        }
        MAPS.with(|m| {
            let mut m = m.borrow_mut();
            let id = match m.iter().position(|p| *p == (key, value)) {
                Some(i) => i,
                None => {
                    m.push((key, value));
                    m.len() - 1
                }
            };
            Type::Map(id as u32)
        })
    }

    /// The key and value types of a map.
    pub fn map_kv(self) -> Option<(Type, Type)> {
        match self {
            Type::Map(id) => Some(MAPS.with(|m| m.borrow()[id as usize])),
            _ => None,
        }
    }

    /// The struct type named `name` (whether or not it is defined: the checker says so).
    pub fn structure(name: &str) -> Type {
        STRUCTS.with(|s| {
            let mut s = s.borrow_mut();
            let id = match s.iter().position(|n| n == name) {
                Some(i) => i,
                None => {
                    s.push(name.to_string());
                    s.len() - 1
                }
            };
            Type::Struct(id as u32)
        })
    }

    /// The element type of an array.
    pub fn elem(self) -> Option<Type> {
        match self {
            Type::Array(id) => Some(ELEMS.with(|e| e.borrow()[id as usize])),
            _ => None,
        }
    }

    /// The name of a struct type.
    pub fn struct_name(self) -> Option<String> {
        match self {
            Type::Struct(id) => Some(STRUCTS.with(|s| s.borrow()[id as usize].clone())),
            _ => None,
        }
    }

    pub fn name(self) -> String {
        match self {
            Type::Int => "int".into(),
            Type::Float => "float".into(),
            Type::Bool => "bool".into(),
            Type::Str => "str".into(),
            Type::Char => "char".into(),
            Type::Void => "void".into(),
            Type::Unknown => "?".into(),
            Type::Array(_) => format!("[{}]", self.elem().map(Type::name).unwrap_or_default()),
            Type::Struct(_) => self.struct_name().unwrap_or_default(),
            Type::Map(_) => match self.map_kv() {
                Some((k, v)) => format!("[{}: {}]", k.name(), v.name()),
                None => String::new(),
            },
        }
    }

    /// True if values of this type contain something unknown (after an error).
    pub fn is_unknown(self) -> bool {
        match self {
            Type::Unknown => true,
            Type::Array(_) => self.elem().is_some_and(Type::is_unknown),
            Type::Map(_) => self.map_kv().is_some_and(|(k, v)| k.is_unknown() || v.is_unknown()),
            _ => false,
        }
    }
}

#[derive(Debug)]
pub struct Program {
    pub funcs: Vec<Func>,
    pub structs: Vec<StructDef>,
    /// The `use name` lines: the standard modules the program imports.
    pub uses: Vec<Use>,
}

/// `use math`: the module's functions are then called as `math.sqrt(x)`.
#[derive(Debug, Clone)]
pub struct Use {
    pub module: String,
    pub span: Span,
}

#[derive(Debug)]
pub struct StructDef {
    pub name: String,
    pub fields: Vec<Field>,
    pub span: Span,
}

#[derive(Debug)]
pub struct Field {
    pub name: String,
    pub ty: Type,
    pub span: Span,
}

#[derive(Debug)]
pub struct Func {
    pub name: String,
    pub params: Vec<Param>,
    pub ret: Type,
    pub body: Vec<Stmt>,
    pub span: Span,
}

#[derive(Debug)]
pub struct Param {
    pub name: String,
    pub ty: Type,
    /// `inout name: T`: the function changes the caller's variable.
    pub inout: bool,
    pub span: Span,
}

#[derive(Debug)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

#[derive(Debug)]
pub enum StmtKind {
    Let { name: String, mutable: bool, ty: Option<Type>, value: Expr },
    /// `target = value`, or `target op= value` (the target is evaluated once).
    Assign { target: Expr, op: Option<BinOp>, value: Expr },
    If { cond: Expr, then: Vec<Stmt>, els: Option<Vec<Stmt>> },
    While { cond: Expr, body: Vec<Stmt> },
    /// `for var in start..end`
    /// `for var in start..end` or `for var in start..end step k` (`k` may be negative)
    For { var: String, start: Expr, end: Expr, step: Option<Expr>, body: Vec<Stmt> },
    /// `for var in iter` over an array or a string
    ForEach { var: String, iter: Expr, body: Vec<Stmt> },
    Break,
    Continue,
    /// `arena { ... }`: everything allocated inside is freed together at `}`.
    Arena(Vec<Stmt>),
    Ret(Option<Expr>),
    Expr(Expr),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

impl BinOp {
    pub fn symbol(self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Mod => "%",
            BinOp::Eq => "==",
            BinOp::Ne => "!=",
            BinOp::Lt => "<",
            BinOp::Le => "<=",
            BinOp::Gt => ">",
            BinOp::Ge => ">=",
            BinOp::And => "&&",
            BinOp::Or => "||",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
}

#[derive(Debug)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
    /// Filled in by the type checker; backends rely on it.
    pub ty: Type,
}

impl Expr {
    pub fn new(kind: ExprKind, span: Span) -> Self {
        Expr { kind, span, ty: Type::Unknown }
    }
}

#[derive(Debug)]
pub enum InterpPart {
    Lit(String),
    Expr(Expr),
}

#[derive(Debug)]
pub enum ExprKind {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    /// `'a'`: one Unicode code point
    Char(u32),
    /// `"text {expr} text"`
    Interp(Vec<InterpPart>),
    Var(String),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    /// `f(a, b)`, also struct construction `Point(x: 1, y: 2)` and builtins
    Call(String, Vec<Expr>),
    /// `if c { a } else { b }` used as a value
    If(Box<Expr>, Box<Expr>, Box<Expr>),
    /// `[a, b, c]`
    Array(Vec<Expr>),
    /// `["a": 1, "b": 2]`, `[:]`
    MapLit(Vec<(Expr, Expr)>),
    /// `base[index]`
    Index(Box<Expr>, Box<Expr>),
    /// `base.name`
    Field(Box<Expr>, String),
    /// `receiver.name(args)`
    Method(Box<Expr>, String, Vec<Expr>),
    /// `name: value`: only as an argument (struct construction)
    Labeled(String, Box<Expr>),
    /// `inout place`: only as an argument
    Inout(Box<Expr>),
}
