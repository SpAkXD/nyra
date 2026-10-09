//! Syntax tree produced by the parser and annotated by the type checker.

use std::cell::RefCell;
use std::collections::HashMap;

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
    /// The element types of the tuple types, by struct id (a tuple type is a struct, see `Type::tuple`).
    static TUPLES: RefCell<HashMap<u32, Vec<Type>>> = RefCell::new(HashMap::new());
    /// The inner type of the optional types, by struct id.
    static OPTIONS: RefCell<HashMap<u32, Type>> = RefCell::new(HashMap::new());
}

/// The name prefix of the structs that stand for tuple types. No program can write it as a type.
pub const TUPLE_PREFIX: &str = "Tup_";
/// The same for optional types `T?` (a struct with a flag `has` and a value `val`).
pub const OPTION_PREFIX: &str = "Opt_";

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

    /// The tuple type `(a, b, ...)`. A tuple is a struct whose fields are `_0`, `_1`, ...: the name
    /// spells the element types, so equal tuple types are the same type. A tuple of an unknown
    /// type is unknown.
    pub fn tuple(elems: &[Type]) -> Type {
        if elems.iter().any(|t| t.is_unknown()) {
            return Type::Unknown;
        }
        let name = format!("{TUPLE_PREFIX}t{}{}", elems.len(), elems.iter().map(|t| t.mangle()).collect::<String>());
        let t = Type::structure(&name);
        if let Type::Struct(id) = t {
            TUPLES.with(|m| {
                m.borrow_mut().entry(id).or_insert_with(|| elems.to_vec());
            });
        }
        t
    }

    /// The optional type `inner?`: a struct with the fields `has` and `val`. An optional of an
    /// unknown type (or of nothing) is unknown.
    pub fn option(inner: Type) -> Type {
        if inner.is_unknown() || inner == Type::Void {
            return Type::Unknown;
        }
        let name = format!("{OPTION_PREFIX}o{}", inner.mangle());
        let t = Type::structure(&name);
        if let Type::Struct(id) = t {
            OPTIONS.with(|m| {
                m.borrow_mut().entry(id).or_insert(inner);
            });
        }
        t
    }

    /// The type inside an optional type.
    pub fn option_inner(self) -> Option<Type> {
        match self {
            Type::Struct(id) => OPTIONS.with(|m| m.borrow().get(&id).copied()),
            _ => None,
        }
    }

    pub fn is_option(self) -> bool {
        self.option_inner().is_some()
    }

    /// The element types of a tuple type.
    pub fn tuple_elems(self) -> Option<Vec<Type>> {
        match self {
            Type::Struct(id) => TUPLES.with(|m| m.borrow().get(&id).cloned()),
            _ => None,
        }
    }

    pub fn is_tuple(self) -> bool {
        self.tuple_elems().is_some()
    }

    /// A short, unambiguous spelling of the type that is safe in a name: `i` int, `f` float,
    /// `b` bool, `s` str, `c` char, `a<T>` array, `m<K><V>` map, `t<n><T>...` tuple, `S<len><name>` struct.
    pub fn mangle(self) -> String {
        match self {
            Type::Int => "i".into(),
            Type::Float => "f".into(),
            Type::Bool => "b".into(),
            Type::Str => "s".into(),
            Type::Char => "c".into(),
            Type::Void => "v".into(),
            Type::Unknown => "u".into(),
            Type::Array(_) => format!("a{}", self.elem().map(Type::mangle).unwrap_or_default()),
            Type::Map(_) => match self.map_kv() {
                Some((k, v)) => format!("m{}{}", k.mangle(), v.mangle()),
                None => String::new(),
            },
            Type::Struct(_) => {
                let name = self.struct_name().unwrap_or_default();
                match (name.strip_prefix(TUPLE_PREFIX), name.strip_prefix(OPTION_PREFIX)) {
                    (Some(rest), _) if self.is_tuple() => rest.to_string(),
                    (_, Some(rest)) if self.is_option() => rest.to_string(),
                    _ => format!("S{}{name}", name.len()),
                }
            }
        }
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
            Type::Struct(_) => match (self.tuple_elems(), self.option_inner()) {
                (Some(es), _) => format!("({})", es.iter().map(|t| t.name()).collect::<Vec<_>>().join(", ")),
                (_, Some(inner)) => format!("{}?", inner.name()),
                _ => self.struct_name().unwrap_or_default(),
            },
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

/// The interned array element types, struct names and map types of the current thread. A thread
/// that runs code of another thread (the interpreter, on its own stack) installs a copy first,
/// else `Type::elem()` and friends would look up ids in empty tables.
#[derive(Clone)]
pub struct TypeTables {
    elems: Vec<Type>,
    structs: Vec<String>,
    maps: Vec<(Type, Type)>,
}

/// A copy of the type tables of this thread.
pub fn type_tables() -> TypeTables {
    TypeTables {
        elems: ELEMS.with(|e| e.borrow().clone()),
        structs: STRUCTS.with(|s| s.borrow().clone()),
        maps: MAPS.with(|m| m.borrow().clone()),
    }
}

/// Makes the tables of another thread the tables of this one.
pub fn install_type_tables(t: TypeTables) {
    ELEMS.with(|e| *e.borrow_mut() = t.elems);
    STRUCTS.with(|s| *s.borrow_mut() = t.structs);
    MAPS.with(|m| *m.borrow_mut() = t.maps);
}

/// The name of a program's own `fn main` when it also has statements at the top level: those
/// statements become the function `main` (they run first), which then calls this one.
pub const USER_MAIN: &str = "__main";

#[derive(Debug)]
pub struct Program {
    pub funcs: Vec<Func>,
    pub structs: Vec<StructDef>,
    /// The `enum` declarations.
    pub enums: Vec<EnumDef>,
    /// `ex` lines: checked at compile time, never compiled into the program.
    pub examples: Vec<Example>,
    /// The `use name` lines: the standard modules the program imports.
    pub uses: Vec<Use>,
    /// The program is a script: `main` is made of its top-level statements.
    pub script: bool,
    /// The script's top-level variables and the functions that use them (filled by the checker).
    pub globals: Globals,
    /// The names of the functions, structs and enums marked `pub`.
    pub public: Vec<String>,
    /// The file of the functions that come from another file (the loader fills it), by function name.
    pub files: HashMap<String, String>,
}

/// A `let` or `var` at the top level of a script: every function may read it (and change it,
/// when it is a `var`).
#[derive(Debug, Clone)]
pub struct Global {
    pub name: String,
    pub ty: Type,
    pub mutable: bool,
    pub span: Span,
    /// The index of its statement among the script's top-level statements.
    pub stmt: usize,
}

/// One script variable a function uses, directly or through the functions it calls. Lowering
/// passes it as a hidden parameter: `inout` when the function (or a callee) changes it.
#[derive(Debug, Clone)]
pub struct GlobalUse {
    /// Index into `Globals::vars`.
    pub var: usize,
    pub inout: bool,
    /// The hidden parameter's name: the variable's own name, unless the function declares a
    /// variable of that name itself (it then cannot see the script variable, only pass it on).
    pub name: String,
}

#[derive(Debug, Default)]
pub struct Globals {
    pub vars: Vec<Global>,
    /// For each function that uses script variables: what it uses, in declaration order.
    pub uses: HashMap<String, Vec<GlobalUse>>,
}

/// One example of `ex f(3) == 9, f(-2) == 4`: a `bool` condition that must be true.
#[derive(Debug)]
pub struct Example {
    pub expr: Expr,
    /// `ex for n in 0..200: f(n) >= 0`: the condition must hold for every `n` of the range.
    pub forall: Option<Forall>,
    /// The imported file the example is from (`None`: the program's own file).
    pub file: Option<String>,
}

/// The range of a property example: `for n in lo..hi step k`, written with whole-number literals.
#[derive(Debug, Clone)]
pub struct Forall {
    pub var: String,
    pub lo: i64,
    pub hi: i64,
    pub step: i64,
    pub span: Span,
}

impl Forall {
    /// How many values the range has.
    pub fn count(&self) -> u64 {
        let (lo, hi, step) = (self.lo as i128, self.hi as i128, self.step as i128);
        let n = if step > 0 && hi > lo {
            (hi - lo + step - 1) / step
        } else if step < 0 && lo > hi {
            (lo - hi + (-step) - 1) / (-step)
        } else {
            0
        };
        n as u64
    }

    /// The `k`-th value (`k < count()`).
    pub fn value(&self, k: u64) -> i64 {
        (self.lo as i128 + k as i128 * self.step as i128) as i64
    }

    /// `n in 0..200` or `n in 10..0 step -2`
    pub fn text(&self) -> String {
        if self.step == 1 {
            format!("{} in {}..{}", self.var, self.lo, self.hi)
        } else {
            format!("{} in {}..{} step {}", self.var, self.lo, self.hi, self.step)
        }
    }
}

/// Property examples run at most this many inputs.
pub const MAX_PROPERTY_INPUTS: u64 = 100_000;

/// `use math`: the module's functions are then called as `math.sqrt(x)`.
#[derive(Debug, Clone)]
pub struct Use {
    /// the module's name: a standard module, or the file name of one of your own (`shapes`)
    pub module: String,
    pub span: Span,
    /// `./shapes` or `../util/text` for `use ./shapes`: a file of the project; `None`: a standard module
    pub path: Option<String>,
}

#[derive(Debug)]
pub struct StructDef {
    pub name: String,
    pub fields: Vec<Field>,
    pub span: Span,
    /// The variants of an enum (its one field is the number of the variant); empty for a struct.
    pub variants: Vec<String>,
}

/// `enum Dir { N, E, S, W }`
#[derive(Debug)]
pub struct EnumDef {
    pub name: String,
    pub variants: Vec<(String, Span)>,
    pub span: Span,
}

/// One arm of a `match`: the patterns that select it (`Dir.N, Dir.S => ...`), or `_`.
#[derive(Debug)]
pub struct MatchArm {
    pub pats: Vec<Expr>,
    pub wild: bool,
    pub body: Vec<Stmt>,
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
    /// `var name: T`: a copy the function may change (the caller's variable is not touched).
    pub mutable: bool,
    pub span: Span,
}

#[derive(Debug)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

#[derive(Debug)]
pub enum StmtKind {
    Let {
        name: String,
        mutable: bool,
        ty: Option<Type>,
        value: Expr,
    },
    /// `target = value`, or `target op= value` (the target is evaluated once).
    Assign {
        target: Expr,
        op: Option<BinOp>,
        value: Expr,
    },
    If {
        cond: Expr,
        then: Vec<Stmt>,
        els: Option<Vec<Stmt>>,
    },
    While {
        cond: Expr,
        body: Vec<Stmt>,
    },
    /// `for var in start..end`
    /// `for var in start..end` or `for var in start..end step k` (`k` may be negative)
    For {
        var: String,
        start: Expr,
        end: Expr,
        step: Option<Expr>,
        body: Vec<Stmt>,
    },
    /// `for var in iter` over an array or a string; `for index, var in iter` also counts from 0
    ForEach {
        var: String,
        index: Option<String>,
        iter: Expr,
        body: Vec<Stmt>,
    },
    Break,
    Continue,
    /// `arena { ... }`: everything allocated inside is freed together at `}`.
    Arena(Vec<Stmt>),
    /// `match value { pattern => body ... }`: the checker turns it into `if` statements.
    Match {
        scrut: Expr,
        arms: Vec<MatchArm>,
    },
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

    /// Calls `f` on this expression, then on every expression inside it.
    pub fn each_mut(&mut self, f: &mut dyn FnMut(&mut Expr)) {
        f(self);
        match &mut self.kind {
            ExprKind::Unary(_, x)
            | ExprKind::Field(x, _)
            | ExprKind::Labeled(_, x)
            | ExprKind::Inout(x)
            | ExprKind::Lambda(_, x)
            | ExprKind::Some(x)
            | ExprKind::Fmt(x, _) => x.each_mut(f),
            ExprKind::Binary(_, a, b) | ExprKind::Index(a, b) | ExprKind::In(a, b) | ExprKind::Coalesce(a, b) => {
                a.each_mut(f);
                b.each_mut(f);
            }
            ExprKind::Slice(b, lo, hi) => {
                b.each_mut(f);
                if let Some(x) = lo {
                    x.each_mut(f);
                }
                if let Some(x) = hi {
                    x.each_mut(f);
                }
            }
            ExprKind::If(c, a, b) => {
                c.each_mut(f);
                a.each_mut(f);
                b.each_mut(f);
            }
            ExprKind::Call(_, xs) | ExprKind::Array(xs) | ExprKind::Tuple(xs) => xs.iter_mut().for_each(|x| x.each_mut(f)),
            ExprKind::Method(r, _, xs) => {
                r.each_mut(f);
                xs.iter_mut().for_each(|x| x.each_mut(f));
            }
            ExprKind::MapLit(pairs) => pairs.iter_mut().for_each(|(k, v)| {
                k.each_mut(f);
                v.each_mut(f);
            }),
            ExprKind::Interp(parts) => parts.iter_mut().for_each(|p| {
                if let InterpPart::Expr(x) = p {
                    x.each_mut(f)
                }
            }),
            ExprKind::Comprehension(c) => {
                match &mut c.src {
                    CompSrc::Each(x) => x.each_mut(f),
                    CompSrc::Range(a, b, k) => {
                        a.each_mut(f);
                        b.each_mut(f);
                        if let Some(k) = k {
                            k.each_mut(f);
                        }
                    }
                }
                c.elem.each_mut(f);
                if let Some(x) = &mut c.cond {
                    x.each_mut(f);
                }
            }
            ExprKind::Int(_)
            | ExprKind::Float(_)
            | ExprKind::Bool(_)
            | ExprKind::Str(_)
            | ExprKind::Char(_)
            | ExprKind::Var(_)
            | ExprKind::None => {}
        }
    }
}

/// Calls `f` on every statement of `stmts` and of the blocks nested in them, and `g` on every
/// expression in them.
pub fn each_stmt_mut(stmts: &mut [Stmt], f: &mut dyn FnMut(&mut Stmt), g: &mut dyn FnMut(&mut Expr)) {
    for s in stmts {
        f(s);
        match &mut s.kind {
            StmtKind::Let { value, .. } => value.each_mut(g),
            StmtKind::Assign { target, value, .. } => {
                target.each_mut(g);
                value.each_mut(g);
            }
            StmtKind::If { cond, then, els } => {
                cond.each_mut(g);
                each_stmt_mut(then, f, g);
                if let Some(e) = els {
                    each_stmt_mut(e, f, g);
                }
            }
            StmtKind::While { cond, body } => {
                cond.each_mut(g);
                each_stmt_mut(body, f, g);
            }
            StmtKind::For { start, end, step, body, .. } => {
                start.each_mut(g);
                end.each_mut(g);
                if let Some(k) = step {
                    k.each_mut(g);
                }
                each_stmt_mut(body, f, g);
            }
            StmtKind::ForEach { iter, body, .. } => {
                iter.each_mut(g);
                each_stmt_mut(body, f, g);
            }
            StmtKind::Arena(body) => each_stmt_mut(body, f, g),
            StmtKind::Match { scrut, arms } => {
                scrut.each_mut(g);
                for arm in arms {
                    arm.pats.iter_mut().for_each(|p| p.each_mut(g));
                    each_stmt_mut(&mut arm.body, f, g);
                }
            }
            StmtKind::Ret(Some(e)) | StmtKind::Expr(e) => e.each_mut(g),
            StmtKind::Ret(None) | StmtKind::Break | StmtKind::Continue => {}
        }
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
    /// `(a, b)`: a tuple of two or more values
    Tuple(Vec<Expr>),
    /// `none`: an optional value that holds nothing
    None,
    /// An optional value that holds this one. Made by the checker where a `T` goes into a `T?`.
    Some(Box<Expr>),
    /// `a ?? b`: the value of the optional `a`, or `b` when it holds nothing
    Coalesce(Box<Expr>, Box<Expr>),
    /// `value:spec` inside `{ }` of a string: the value as text, aligned, padded or rounded
    Fmt(Box<Expr>, FmtSpec),
    /// `x in xs`: is the element, character or key there
    In(Box<Expr>, Box<Expr>),
    /// `xs[a..b]`, `xs[a..]`, `xs[..b]`, and the same for a string
    Slice(Box<Expr>, Option<Box<Expr>>, Option<Box<Expr>>),
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
    /// `x => body` or `(a, b) => body`: only as an argument of the array methods that take one
    /// (`map`, `filter`, ...). Lowering turns it into a loop, so it never exists at run time.
    Lambda(Vec<(String, Span)>, Box<Expr>),
    /// `[elem for var in src if cond]`: a new array, built by a loop like `map` and `filter`
    Comprehension(Box<Comp>),
}

/// A format specifier, the part after the colon in `"{x:>8}"`: `[[fill]align][+][0][width][,][.precision][type]`.
#[derive(Debug, Clone, PartialEq)]
pub struct FmtSpec {
    /// the specifier as written
    pub text: String,
    pub fill: Option<char>,
    /// `<`, `>` or `^`
    pub align: Option<char>,
    /// `+`: a sign on positive numbers
    pub plus: bool,
    /// `0`: pad a number with zeros after its sign
    pub zero: bool,
    pub width: usize,
    /// `,`: thousands separators
    pub comma: bool,
    /// `.2`: decimals of a float
    pub prec: Option<usize>,
    /// `f`, `d` or `s`
    pub ty: Option<char>,
}

impl FmtSpec {
    /// True if the text needs the padding and grouping helper function, not just the digits.
    pub fn needs_helper(&self) -> bool {
        self.width > 0 || self.comma || self.plus
    }
}

/// `[elem for var in src if cond]`. Like a lambda's body, `elem` and `cond` only read variables.
#[derive(Debug)]
pub struct Comp {
    pub elem: Expr,
    /// The loop variable (one-element slice, so it reads like a lambda's parameters).
    pub var: [(String, Span); 1],
    pub src: CompSrc,
    pub cond: Option<Expr>,
}

#[derive(Debug)]
pub enum CompSrc {
    /// an array or a string
    Each(Expr),
    /// `a..b` or `a..b step k`
    Range(Expr, Expr, Option<Expr>),
}
