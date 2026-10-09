//! The standard library: modules a program imports with `use name` and calls as `name.f(x)`.
//!
//! Most functions are *intrinsics*: one runtime function per backend (`StdFn`), called like any
//! other runtime operation (`RtOp::Std`). The math functions that hosts compute differently
//! (`exp`, `log`, `sin`, ...) are written in Nyra itself (`src/std/math.nyra`): every backend then
//! runs the same IEEE operations, so the results are identical everywhere. `json.str` and
//! `json.parse` work on any type and are lowered by the backends from the value's type.
//!
//! A module's Nyra functions are added to the program as functions named `module.name`
//! (a name no program can write), helpers starting with `_` stay private.

use crate::ast::{Expr, ExprKind, Func, InterpPart, Program, Stmt, StmtKind, Type};
use crate::diag::Diag;

/// Every standard module, sorted.
pub const MODULES: &[&str] = &["fs", "input", "json", "math", "os", "random", "text", "time"];

/// "`fs`, `input`, ... and `time`" for messages.
pub fn module_list() -> String {
    let names: Vec<String> = MODULES.iter().map(|m| format!("`{m}`")).collect();
    format!("{} and {}", names[..names.len() - 1].join(", "), names[names.len() - 1])
}

pub fn is_module(name: &str) -> bool {
    MODULES.contains(&name)
}

/// The type of a parameter or result of an intrinsic.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum T {
    Int,
    Float,
    Bool,
    Str,
    /// `[str]`
    Strs,
    Void,
}

impl T {
    pub fn ty(self) -> Type {
        match self {
            T::Int => Type::Int,
            T::Float => Type::Float,
            T::Bool => Type::Bool,
            T::Str => Type::Str,
            T::Strs => Type::array(Type::Str),
            T::Void => Type::Void,
        }
    }
}

macro_rules! std_fns {
    ($($v:ident: $m:literal . $n:literal ($($p:literal: $pt:ident),*) -> $r:ident;)*) => {
        /// A standard function implemented by each backend's runtime.
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        pub enum StdFn { $($v),* }

        impl StdFn {
            pub const ALL: &'static [StdFn] = &[$(StdFn::$v),*];

            /// The module and the function name: `("fs", "read")`.
            pub fn path(self) -> (&'static str, &'static str) {
                match self { $(StdFn::$v => ($m, $n)),* }
            }

            /// The parameters (name, type).
            pub fn params(self) -> &'static [(&'static str, T)] {
                match self { $(StdFn::$v => &[$(($p, T::$pt)),*]),* }
            }

            pub fn ret(self) -> T {
                match self { $(StdFn::$v => T::$r),* }
            }

            /// `fs.read`
            pub fn full_name(self) -> &'static str {
                match self { $(StdFn::$v => concat!($m, ".", $n)),* }
            }

            /// The runtime's name for it, after the backend's prefix: `fs_read`.
            pub fn rt_name(self) -> &'static str {
                match self { $(StdFn::$v => concat!($m, "_", $n)),* }
            }
        }
    };
}

std_fns! {
    InputLine: "input"."line"() -> Str;
    InputAll: "input"."all"() -> Str;
    InputLines: "input"."lines"() -> Strs;
    InputEof: "input"."eof"() -> Bool;
    OsArgs: "os"."args"() -> Strs;
    OsEnv: "os"."env"("name": Str) -> Str;
    OsHasEnv: "os"."has_env"("name": Str) -> Bool;
    OsExit: "os"."exit"("code": Int) -> Void;
    FsRead: "fs"."read"("path": Str) -> Str;
    FsWrite: "fs"."write"("path": Str, "text": Str) -> Void;
    FsAppend: "fs"."append"("path": Str, "text": Str) -> Void;
    FsExists: "fs"."exists"("path": Str) -> Bool;
    FsList: "fs"."list"("dir": Str) -> Strs;
    FsRemove: "fs"."remove"("path": Str) -> Void;
    FsMkdir: "fs"."mkdir"("path": Str) -> Void;
    TimeNowMs: "time"."now_ms"() -> Int;
    TimeMonoMs: "time"."mono_ms"() -> Float;
    TimeSleepMs: "time"."sleep_ms"("ms": Int) -> Void;
    RandomRandom: "random"."random"() -> Float;
    RandomRange: "random"."range"("lo": Int, "hi": Int) -> Int;
    RandomSeed: "random"."seed"("n": Int) -> Void;
    MathSqrt: "math"."sqrt"("x": Float) -> Float;
    MathFloor: "math"."floor"("x": Float) -> Float;
    MathCeil: "math"."ceil"("x": Float) -> Float;
    MathRound: "math"."round"("x": Float) -> Float;
    MathTrunc: "math"."trunc"("x": Float) -> Float;
    TextFixed: "text"."fixed"("x": Float, "digits": Int) -> Str;
    TextIsInt: "text"."is_int"("s": Str) -> Bool;
    TextIsFloat: "text"."is_float"("s": Str) -> Bool;
}

impl StdFn {
    pub fn from_name(full: &str) -> Option<StdFn> {
        StdFn::ALL.iter().copied().find(|f| f.full_name() == full)
    }

    /// True if the result is a new string or array (the destination owns it).
    pub fn owned(self) -> bool {
        matches!(self.ret(), T::Str | T::Strs)
    }
}

/// The constants of a module.
pub fn constant(module: &str, name: &str) -> Option<f64> {
    match (module, name) {
        ("math", "pi") => Some(std::f64::consts::PI),
        ("math", "e") => Some(std::f64::consts::E),
        ("math", "inf") => Some(f64::INFINITY),
        _ => None,
    }
}

/// The functions of `json`: they take or give a value of any type.
pub const JSON_FNS: &[&str] = &["parse", "str"];

/// The Nyra source of a module's functions that are written in Nyra.
fn source(module: &str) -> Option<&'static str> {
    match module {
        "math" => Some(include_str!("std/math.nyra")),
        _ => None,
    }
}

/// Every public name of a module (functions and constants), sorted, for hints and listings.
pub fn names(module: &str) -> Vec<String> {
    let mut v: Vec<String> = StdFn::ALL.iter().filter(|f| f.path().0 == module).map(|f| f.path().1.to_string()).collect();
    for c in ["pi", "e", "inf"] {
        if constant(module, c).is_some() {
            v.push(c.to_string());
        }
    }
    if module == "json" {
        v.extend(JSON_FNS.iter().map(|s| s.to_string()));
    }
    if let Some(src) = source(module) {
        for f in parse_source(src).funcs {
            if !f.name.starts_with('_') {
                v.push(f.name);
            }
        }
    }
    v.sort();
    v
}

fn parse_source(src: &str) -> Program {
    let (toks, errs) = crate::lexer::lex(src);
    assert!(errs.is_empty(), "the standard library does not lex: {errs:?}");
    let (prog, errs) = crate::parser::parse(toks);
    assert!(errs.is_empty(), "the standard library does not parse: {errs:?}");
    prog
}

/// A hint for a name a module does not have, when another language spells it that way.
pub fn renamed(module: &str, name: &str) -> Option<&'static str> {
    Some(match (module, name) {
        ("random", "randint" | "randrange" | "int" | "integer" | "next_int" | "nextInt") => {
            "use `random.range(lo, hi)`: the upper bound is excluded, so a die is `random.range(1, 7)`"
        }
        ("random", "rand" | "uniform" | "float" | "next" | "nextDouble" | "next_float") => "use `random.random()`: a float from 0 up to (not including) 1",
        ("random", "srand" | "set_seed" | "seed_rng") => "use `random.seed(n)`",
        ("math", "abs" | "fabs" | "min" | "max") => "`abs`, `min` and `max` are builtins: call them without `math.`",
        ("math", "power" | "powf" | "powi") => "use `math.pow(x, y)`",
        ("math", "ln") => "use `math.log(x)`: the natural logarithm",
        ("math", "PI" | "Pi") => "the constant is `math.pi`",
        ("math", "E") => "the constant is `math.e`",
        ("math", "INFINITY" | "Infinity" | "infinity" | "Inf") => "the constant is `math.inf`",
        ("os", "argv" | "Args" | "arguments") => "use `os.args()`: the arguments after the program name, as `[str]`",
        ("os", "getenv" | "Getenv" | "environ" | "get_env") => "use `os.env(name)`: the variable's value, or \"\" when it is not set",
        ("os", "Exit" | "quit") => "use `os.exit(code)`",
        ("fs", "read_file" | "readFile" | "readFileSync" | "read_to_string" | "ReadFile" | "open" | "load") => "use `fs.read(path)`",
        ("fs", "write_file" | "writeFile" | "writeFileSync" | "WriteFile" | "save") => "use `fs.write(path, text)`",
        ("fs", "append_file" | "appendFile" | "appendFileSync") => "use `fs.append(path, text)`",
        ("fs", "readdir" | "read_dir" | "listdir" | "ls" | "ReadDir") => "use `fs.list(dir)`: the names in the folder, sorted",
        ("fs", "rm" | "unlink" | "delete" | "remove_file" | "rmdir") => "use `fs.remove(path)`",
        ("fs", "mkdirs" | "makedirs" | "create_dir" | "MkdirAll") => "use `fs.mkdir(path)`",
        ("fs", "is_file" | "isfile" | "isdir" | "is_dir" | "existsSync") => "use `fs.exists(path)`",
        ("input", "readline" | "read_line" | "readLine" | "get_line" | "getline" | "next_line") => "use `input.line()`",
        ("input", "read" | "read_all" | "readAll" | "text") => "use `input.all()`",
        ("input", "readlines" | "read_lines" | "all_lines") => "use `input.lines()`",
        ("input", "at_end" | "is_eof" | "done" | "empty" | "end") => "use `input.eof()`",
        ("json", "dumps" | "dump" | "stringify" | "encode" | "to_string" | "Marshal") => "use `json.str(value)`",
        ("json", "loads" | "load" | "decode" | "from_str" | "Unmarshal") => "use `json.parse(text)` where the type is known: `let p: Point = json.parse(text)`",
        ("time", "time" | "now" | "millis" | "current_ms" | "time_ms") => "use `time.now_ms()`: milliseconds since 1970",
        ("time", "perf_counter" | "monotonic" | "clock" | "performance" | "nanos" | "instant") => "use `time.mono_ms()`: a monotonic clock in milliseconds, for timing code",
        ("time", "sleep" | "Sleep" | "wait") => "use `time.sleep_ms(ms)`",
        ("text", "format" | "to_fixed" | "toFixed" | "round") => "use `text.fixed(x, digits)`: `text.fixed(3.14159, 2)` is \"3.14\"",
        _ => return None,
    })
}

/// Adds the Nyra-written functions of the imported modules to the program, named
/// `module.name`. Reports `use` lines that name no module (E0300).
pub fn link(prog: &mut Program) -> Vec<Diag> {
    let mut errs = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for u in prog.uses.clone() {
        if !is_module(&u.module) {
            let hint = match crate::diag::suggest(&u.module, MODULES.iter().copied()) {
                Some(s) => format!("{s} The standard modules are {}", module_list()),
                None if u.module == "str" || u.module == "string" || u.module == "strings" => {
                    "string methods need no import (`s.split(\",\")`); the `text` module has `text.fixed`, `text.is_int` and `text.is_float`".to_string()
                }
                None if matches!(u.module.as_str(), "io" | "sys" | "stdin") => {
                    "standard input is the `input` module (`input.line()`), arguments and exit codes are in `os`".to_string()
                }
                None => format!("the standard modules are {}", module_list()),
            };
            errs.push(Diag::new("E0300", format!("module `{}` not found", u.module), u.span).hint(hint));
            continue;
        }
        if seen.contains(&u.module) {
            continue;
        }
        seen.push(u.module.clone());
        if let Some(src) = source(&u.module) {
            let lib = parse_source(src);
            let own: Vec<String> = lib.funcs.iter().map(|f| f.name.clone()).collect();
            for mut f in lib.funcs {
                f.name = format!("{}.{}", u.module, f.name);
                rename_calls(&mut f, &u.module, &own);
                prog.funcs.push(f);
            }
        }
    }
    errs
}

/// Calls to the module's own functions get the module's name: `exp(x)` becomes `math.exp(x)`.
fn rename_calls(f: &mut Func, module: &str, own: &[String]) {
    fn stmts(ss: &mut [Stmt], m: &str, own: &[String]) {
        for s in ss {
            match &mut s.kind {
                StmtKind::Let { value, .. } => expr(value, m, own),
                StmtKind::Assign { target, value, .. } => {
                    expr(target, m, own);
                    expr(value, m, own);
                }
                StmtKind::If { cond, then, els } => {
                    expr(cond, m, own);
                    stmts(then, m, own);
                    if let Some(e) = els {
                        stmts(e, m, own);
                    }
                }
                StmtKind::While { cond, body } => {
                    expr(cond, m, own);
                    stmts(body, m, own);
                }
                StmtKind::For { start, end, step, body, .. } => {
                    expr(start, m, own);
                    expr(end, m, own);
                    if let Some(k) = step {
                        expr(k, m, own);
                    }
                    stmts(body, m, own);
                }
                StmtKind::ForEach { iter, body, .. } => {
                    expr(iter, m, own);
                    stmts(body, m, own);
                }
                StmtKind::Arena(body) => stmts(body, m, own),
                StmtKind::Ret(Some(e)) | StmtKind::Expr(e) => expr(e, m, own),
                StmtKind::Ret(None) | StmtKind::Break | StmtKind::Continue => {}
            }
        }
    }
    fn expr(e: &mut Expr, m: &str, own: &[String]) {
        match &mut e.kind {
            ExprKind::Call(name, args) => {
                if own.contains(name) {
                    *name = format!("{m}.{name}");
                }
                args.iter_mut().for_each(|a| expr(a, m, own));
            }
            ExprKind::Interp(parts) => {
                for p in parts {
                    if let InterpPart::Expr(x) = p {
                        expr(x, m, own);
                    }
                }
            }
            ExprKind::Unary(_, x) | ExprKind::Field(x, _) | ExprKind::Labeled(_, x) | ExprKind::Inout(x) => expr(x, m, own),
            ExprKind::Binary(_, a, b) | ExprKind::Index(a, b) => {
                expr(a, m, own);
                expr(b, m, own);
            }
            ExprKind::If(c, a, b) => {
                expr(c, m, own);
                expr(a, m, own);
                expr(b, m, own);
            }
            ExprKind::Array(xs) => xs.iter_mut().for_each(|x| expr(x, m, own)),
            ExprKind::MapLit(pairs) => {
                for (k, v) in pairs {
                    expr(k, m, own);
                    expr(v, m, own);
                }
            }
            ExprKind::Method(r, _, args) => {
                expr(r, m, own);
                args.iter_mut().for_each(|a| expr(a, m, own));
            }
            ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Bool(_) | ExprKind::Str(_) | ExprKind::Char(_) | ExprKind::Var(_) => {}
        }
    }
    stmts(&mut f.body, module, own);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_module_with_nyra_source_checks_cleanly() {
        for m in MODULES {
            if source(m).is_none() {
                continue;
            }
            let src = format!("use {m}\nfn main() {{\n}}\n");
            if let Err(d) = crate::compile(&src) {
                panic!("std module `{m}` has errors: {d:?}");
            }
        }
    }

    #[test]
    fn every_function_has_a_unique_name() {
        for (i, f) in StdFn::ALL.iter().enumerate() {
            assert_eq!(StdFn::from_name(&f.full_name()), Some(*f));
            assert!(StdFn::ALL[..i].iter().all(|g| g.full_name() != f.full_name()));
            assert!(is_module(f.path().0));
        }
    }
}
