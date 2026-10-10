//! Python backend. The output runs on CPython 3.8 and later.
//!
//! - `int` is Python's `int`, wrapped to 64 bits after arithmetic (`ny_i64`, around a whole sum
//!   or product: wrapping is the same at every step or once at the end);
//! - a `char` is a one-character `str`, so it prints and compares like a character;
//! - arrays are `NyList` (a `list`) and structs are classes; both are copied on write through a
//!   mark on values with more than one owner, like the JavaScript backend;
//! - an `inout` parameter is returned: `x, y = swap(x, y)`.

use std::fmt::Write;

use super::names;
use super::scope::{self, range_for, Info};
use crate::ir::{Arg, BinOp, Expr, Func, LocalId, Module, Place, PureFn, RtOp, Step, Stmt, StmtKind, Structs, Ty, UnOp};

const RESERVED: &[&str] = &[
    "False",
    "None",
    "True",
    "and",
    "as",
    "assert",
    "async",
    "await",
    "break",
    "class",
    "continue",
    "def",
    "del",
    "elif",
    "else",
    "except",
    "finally",
    "for",
    "from",
    "global",
    "if",
    "import",
    "in",
    "is",
    "lambda",
    "nonlocal",
    "not",
    "or",
    "pass",
    "raise",
    "return",
    "try",
    "while",
    "with",
    "yield",
    "match",
    "case",
    "print",
    "len",
    "str",
    "int",
    "float",
    "bool",
    "list",
    "dict",
    "set",
    "tuple",
    "range",
    "ord",
    "chr",
    "isinstance",
    "getattr",
    "setattr",
    "hasattr",
    "repr",
    "abs",
    "all",
    "any",
    "zip",
    "map",
    "enumerate",
    "super",
    "object",
    "type",
    "self",
    "other",
    "math",
    "json",
    "os",
    "re",
    "sys",
    "Exception",
    "MemoryError",
    "annotations",
    "min",
    "max",
    "sum",
    "iter",
    "next",
    "open",
    "input",
    "id",
    "hash",
    "format",
];

/// The Python runtime, emitted after the program (`@FILE@` becomes the source path).
const RUNTIME: &str = include_str!("../rt/py/runtime.py");
const STD: &str = include_str!("../rt/py/std.py");
const JSON: &str = include_str!("../rt/py/json.py");

/// A Nyra name as a Python identifier: `ny...` names belong to the runtime, and names Python
/// itself uses (keywords, builtins the program calls) get a `_`.
fn name(n: &str) -> String {
    let id = names::ascii(n);
    if RESERVED.contains(&n) || n.starts_with("ny") || n.starts_with("Ny") || n.starts_with("__") {
        format!("{id}_")
    } else {
        id
    }
}

/// The type hint of a value.
fn hint(t: Ty) -> String {
    match t {
        Ty::Int => "int".into(),
        Ty::Float => "float".into(),
        Ty::Bool => "bool".into(),
        Ty::Char | Ty::Str => "str".into(),
        Ty::Array(_) => format!("list[{}]", hint(t.elem().expect("an array"))),
        Ty::Map(_) => {
            let (k, v) = t.map_kv().expect("a map");
            format!("dict[{}, {}]", hint(k), hint(v))
        }
        Ty::Struct(_) => name(&t.struct_name().expect("a struct")),
        other => unreachable!("the Python backend got the type `{}`", other.name()),
    }
}

/// The type descriptor `ny_show` prints a value with (a char and a str look alike here).
fn tdesc(t: Ty) -> String {
    match t {
        Ty::Int => "i".into(),
        Ty::Float => "f".into(),
        Ty::Bool => "b".into(),
        Ty::Char => "c".into(),
        Ty::Str => "s".into(),
        Ty::Array(_) => format!("[{}", tdesc(t.elem().expect("an array"))),
        // a map: the key type is one letter
        Ty::Map(_) => {
            let (k, v) = t.map_kv().expect("a map");
            format!("{{{}{}", tdesc(k), tdesc(v))
        }
        _ => "S".into(),
    }
}

/// The type as the JSON runtime reads it: "i", "f", "b", "c", "s", ("a", T), ("m", K, V), or a
/// struct's (tuple's, optional's, enum's) class.
fn jdesc(t: Ty) -> String {
    match t {
        Ty::Int => "\"i\"".into(),
        Ty::Float => "\"f\"".into(),
        Ty::Bool => "\"b\"".into(),
        Ty::Char => "\"c\"".into(),
        Ty::Str => "\"s\"".into(),
        Ty::Array(_) => format!("(\"a\", {})", jdesc(t.elem().expect("an array"))),
        Ty::Map(_) => {
            let (k, v) = t.map_kv().expect("a map");
            format!("(\"m\", {}, {})", jdesc(k), jdesc(v))
        }
        _ => name(&t.struct_name().expect("a struct")),
    }
}

/// True if a value of this type can hold a float (Python's `==` on lists and `in` treat the
/// same NaN object as equal to itself; Nyra never does).
fn has_float(m: &Module, t: Ty) -> bool {
    fn go(m: &Module, t: Ty, seen: &mut Vec<Ty>) -> bool {
        match t {
            Ty::Float => true,
            Ty::Array(_) => go(m, t.elem().expect("an array"), seen),
            Ty::Map(_) => go(m, t.map_kv().expect("a map").1, seen),
            // a struct can contain itself through an array
            Ty::Struct(_) if seen.contains(&t) => false,
            Ty::Struct(_) => {
                seen.push(t);
                m.structs.get(t).is_some_and(|s| s.fields.iter().any(|(_, ft)| go(m, *ft, seen)))
            }
            _ => false,
        }
    }
    go(m, t, &mut Vec::new())
}

/// A Python string literal (double quotes).
fn lit(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                let _ = write!(out, "\\x{:02x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// One value inside an f-string's braces, as `print` shows it (ints, strs, chars and structs
/// show themselves).
fn shown_in_fstring(t: Ty, x: &str) -> String {
    match t {
        Ty::Int | Ty::Str | Ty::Char | Ty::Struct(_) => x.to_string(),
        Ty::Float => format!("ny_num({x})"),
        Ty::Bool => format!("ny_bool({x})"),
        _ => format!("ny_show({x}, '{}')", tdesc(t)),
    }
}

/// One class per struct: a constructor, copying one level (copy on write), equality, printing.
fn classes(m: &Module, out: &mut String) {
    for (_, s) in &m.structs.0 {
        let n = name(&s.name);
        let fields: Vec<String> = s.fields.iter().map(|(f, _)| name(f)).collect();
        let _ = writeln!(out, "class {n}:");
        out.push_str("    ny_shared = False\n\n");
        let params: String = s.fields.iter().zip(&fields).map(|((_, t), f)| format!(", {f}: {}", hint(*t))).collect();
        let _ = writeln!(out, "    def __init__(self{params}):");
        if fields.is_empty() {
            out.push_str("        pass\n");
        }
        for f in &fields {
            let _ = writeln!(out, "        self.{f} = {f}");
        }
        // the copy shares the aggregate fields, so they are marked shared
        let copies: Vec<String> = s
            .fields
            .iter()
            .zip(&fields)
            .map(|((_, t), f)| if Structs::aggregate(*t) { format!("ny_share(self.{f})") } else { format!("self.{f}") })
            .collect();
        let _ = writeln!(out, "\n    def ny_copy(self) -> {n}:\n        return {n}({})", copies.join(", "));
        let eqs: Vec<String> = s
            .fields
            .iter()
            .zip(&fields)
            .map(|((_, t), f)| {
                if Structs::aggregate(*t) && has_float(m, *t) {
                    format!("ny_eq(self.{f}, other.{f})")
                } else {
                    format!("self.{f} == other.{f}")
                }
            })
            .collect();
        let eq = if eqs.is_empty() { "True".to_string() } else { eqs.join(" and ") };
        let _ = writeln!(out, "\n    def __eq__(self, other) -> bool:\n        return {eq}");
        // a tuple of exact parts can be a key of a map
        if s.tuple && s.fields.iter().all(|(_, t)| key_part(m, *t)) {
            let parts: Vec<String> = fields.iter().map(|f| format!("self.{f}")).collect();
            let _ = writeln!(out, "\n    def __hash__(self) -> int:\n        return hash(({},))", parts.join(", "));
        }
        let mut pieces = vec![Piece::Text(if s.tuple { "(".to_string() } else { format!("{}(", s.name) })];
        for (k, ((fname, t), f)) in s.fields.iter().zip(&fields).enumerate() {
            let sep = if k > 0 { ", " } else { "" };
            pieces.push(Piece::Text(if s.tuple { sep.to_string() } else { format!("{sep}{fname}: ") }));
            let value = match t {
                Ty::Char | Ty::Str => format!("ny_show(self.{f}, '{}')", tdesc(*t)),
                _ => shown_in_fstring(*t, &format!("self.{f}")),
            };
            pieces.push(Piece::Value(value));
        }
        pieces.push(Piece::Text(")".into()));
        if !s.variants.is_empty() {
            let names: Vec<String> = s.variants.iter().map(|v| lit(&format!("{}.{v}", s.name))).collect();
            // `Shape.Circle(2)`: the values of a variant that carries any, as a struct prints its fields
            let mut cases = String::new();
            for (v, vname) in s.variants.iter().enumerate().filter(|(v, _)| s.payloads.get(*v).is_some_and(|n| *n > 0)) {
                let mut ps = vec![Piece::Text(format!("{}.{vname}(", s.name))];
                for (j, k) in s.slots(v).enumerate() {
                    if j > 0 {
                        ps.push(Piece::Text(", ".into()));
                    }
                    let t = s.fields[k].1;
                    ps.push(Piece::Value(match t {
                        Ty::Char | Ty::Str => format!("ny_show(self.{}, '{}')", fields[k], tdesc(t)),
                        _ => shown_in_fstring(t, &format!("self.{}", fields[k])),
                    }));
                }
                ps.push(Piece::Text(")".into()));
                let _ = writeln!(cases, "        if self.{} == {v}:\n            return {}", fields[0], fstring(&ps));
            }
            let _ = writeln!(out, "\n    def __repr__(self) -> str:\n{cases}        return [{}][self.{}]\n\n", names.join(", "), fields[0]);
        } else if s.option {
            // `none`, or `Some(value)`
            let (has, val) = (&fields[0], &fields[1]);
            let t = s.fields[1].1;
            let value = match t {
                Ty::Char | Ty::Str => format!("ny_show(self.{val}, '{}')", tdesc(t)),
                _ => shown_in_fstring(t, &format!("self.{val}")),
            };
            let some = [Piece::Text("Some(".into()), Piece::Value(value), Piece::Text(")".into())];
            let _ = writeln!(
                out,
                "\n    def __repr__(self) -> str:\n        if not self.{has}:\n            return \"none\"\n        return {}\n\n",
                fstring(&some)
            );
        } else {
            let _ = writeln!(out, "\n    def __repr__(self) -> str:\n        return {}\n\n", fstring(&pieces));
        }
    }
}

/// A piece of an f-string: text, or a Python expression shown inside `{...}`.
enum Piece {
    Text(String),
    Value(String),
}

/// An f-string of pieces, or a plain literal when there is no value. A value that needs a quote
/// or a backslash (which f-strings before Python 3.12 cannot hold) makes it a `+` of `str`s.
fn fstring(pieces: &[Piece]) -> String {
    if !pieces.iter().any(|p| matches!(p, Piece::Value(_))) {
        let text: String = pieces.iter().map(|p| if let Piece::Text(t) = p { t.as_str() } else { "" }).collect();
        return lit(&text);
    }
    if pieces.iter().any(|p| matches!(p, Piece::Value(v) if v.contains('"') || v.contains('\\'))) {
        let parts: Vec<String> = pieces
            .iter()
            .map(|p| match p {
                Piece::Text(t) => lit(t),
                Piece::Value(v) => format!("str({v})"),
            })
            .collect();
        return format!("({})", parts.join(" + "));
    }
    let mut out = String::from("f\"");
    for p in pieces {
        match p {
            Piece::Text(t) => {
                let l = lit(t);
                out.push_str(&l[1..l.len() - 1].replace('{', "{{").replace('}', "}}"));
            }
            Piece::Value(v) => {
                let _ = write!(out, "{{{v}}}");
            }
        }
    }
    out.push('"');
    out
}

/// `file` is the source path as given to nyra; runtime errors report it.
pub fn gen(m: &Module, file: &str) -> String {
    let mut out = String::from(concat!(
        "# generated by nyra: Python 3 (run with `python3 file.py`)\n",
        "from __future__ import annotations\n\n",
        "import json\nimport math\nimport os\nimport re\nimport sys\n\n\n",
    ));
    classes(m, &mut out);
    let json = m.uses_json();
    if json {
        // the fields of each struct for `json.str` and `json.parse`, after every class exists
        for (_, s) in &m.structs.0 {
            let fields: Vec<String> = s.fields.iter().map(|(f, t)| format!("({}, \"{}\", {})", lit(f), name(f), jdesc(*t))).collect();
            let _ = writeln!(out, "{}.ny_jf = [{}]", name(&s.name), fields.join(", "));
            let c = name(&s.name);
            if s.tuple {
                let _ = writeln!(out, "{c}.ny_jk = \"t\"");
            } else if s.option {
                let _ = writeln!(out, "{c}.ny_jk = \"o\"");
            } else if !s.variants.is_empty() {
                let vs: Vec<String> = s
                    .variants
                    .iter()
                    .enumerate()
                    .map(|(v, vname)| {
                        let idx: Vec<String> = s.slots(v).map(|k| format!("{k}, ")).collect();
                        format!("({}, [{}])", lit(vname), idx.concat())
                    })
                    .collect();
                let _ = writeln!(out, "{c}.ny_jk = \"e\"\n{c}.ny_jn = {}\n{c}.ny_jv = [{}]", lit(&s.name), vs.join(", "));
            }
        }
        out.push_str("\n\n");
    }
    for f in &m.funcs {
        let info = Info::new(f);
        // Python variables belong to the whole function, and two Nyra variables with the same
        // name never live at the same time, so they can share it
        let n: Vec<String> = f
            .locals
            .iter()
            .enumerate()
            .map(|(i, l)| match &l.name {
                Some(x) => name(x),
                None => format!("ny_t{i}"),
            })
            .collect();
        let params: Vec<String> = (0..f.params).map(|i| format!("{}: {}", n[i], hint(f.locals[i].ty))).collect();
        let outs: Vec<LocalId> = (0..f.params).filter(|&i| f.locals[i].inout).map(|i| LocalId(i as u32)).collect();
        let ret_hint = {
            let mut parts: Vec<String> = f.ret.map(hint).into_iter().collect();
            parts.extend(outs.iter().map(|l| hint(f.locals[l.0 as usize].ty)));
            match parts.len() {
                0 => "None".to_string(),
                1 => parts.remove(0),
                _ => format!("tuple[{}]", parts.join(", ")),
            }
        };
        let _ = writeln!(out, "def {}({}) -> {ret_hint}:", name(&f.name), params.join(", "));
        let mut g = Gen { m, f, info: &info, names: &n, outs: &outs, out: String::new(), indent: 1, tmp: 0, steps: Vec::new() };
        g.stmts(&f.body);
        // a function that changes `inout` parameters returns them, also when it ends without `ret`
        if !outs.is_empty() && f.ret.is_none() && !matches!(f.body.last().map(|s| &s.kind), Some(StmtKind::Return(_))) {
            let line = g.return_line(None);
            g.line(&line);
        }
        if g.out.is_empty() {
            g.line("pass");
        }
        out.push_str(&g.out);
        out.push_str("\n\n");
    }
    out.push_str(&RUNTIME.replace("@FILE@", &lit(file)));
    if m.uses_std() {
        out.push_str(STD);
    }
    if json {
        out.push_str(JSON);
    }
    let _ = write!(out, "\n\nif __name__ == \"__main__\":\n    ny_main({})\n", name(&m.func(m.main).name));
    out
}

struct Gen<'a> {
    m: &'a Module,
    f: &'a Func,
    info: &'a Info,
    names: &'a [String],
    /// The `inout` parameters, returned with the result.
    outs: &'a [LocalId],
    out: String,
    indent: usize,
    /// Counter for helper names (element references).
    tmp: usize,
    /// The step of each enclosing loop: `continue` runs it first.
    steps: Vec<&'a [Stmt]>,
}

impl<'a> Gen<'a> {
    fn line(&mut self, s: &str) {
        for _ in 0..self.indent {
            self.out.push_str("    ");
        }
        self.out.push_str(s);
        self.out.push('\n');
    }

    fn block(&mut self, ss: &'a [Stmt]) {
        self.indent += 1;
        let before = self.out.len();
        self.stmts(ss);
        if self.out.len() == before {
            self.line("pass");
        }
        self.indent -= 1;
    }

    fn stmts(&mut self, ss: &'a [Stmt]) {
        let mut k = 0;
        while k < ss.len() {
            if let Some(r) = range_for(ss, k, self.info) {
                if let Some(p) = r.pre {
                    self.stmt(p);
                }
                let i = self.local(r.var).to_string();
                let start = self.arg(r.start);
                let line = if start == "0" {
                    format!("for {i} in range({}):", self.arg(r.end))
                } else {
                    format!("for {i} in range({start}, {}):", self.arg(r.end))
                };
                self.line(&line);
                self.steps.push(&[]);
                self.block(r.body);
                self.steps.pop();
                k += r.len;
                continue;
            }
            // `x = v` and then `dup x`: one statement that marks the value shared
            if let (StmtKind::Set(l, e), Some(Stmt { kind: StmtKind::Dup(d), .. })) = (&ss[k].kind, ss.get(k + 1)) {
                if l == d && Structs::aggregate(self.f.local(*l).ty) {
                    let line = format!("{} = ny_share({})", self.local(*l), self.arg(e));
                    self.line(&line);
                    k += 2;
                    continue;
                }
            }
            self.stmt(&ss[k]);
            k += 1;
        }
    }

    fn local(&self, l: LocalId) -> &str {
        &self.names[l.0 as usize]
    }

    fn ty(&self, e: &Expr) -> Ty {
        e.ty(self.f)
    }

    fn fresh(&mut self, prefix: &str) -> String {
        self.tmp += 1;
        format!("ny_{prefix}{}", self.tmp)
    }

    /// `dst = value` or `value`
    fn assign(&mut self, dst: Option<LocalId>, value: String) {
        let line = match dst {
            Some(d) => format!("{} = {value}", self.local(d)),
            None => value,
        };
        self.line(&line);
    }

    fn arg(&self, e: &Expr) -> String {
        super::bare(&self.expr(e)).to_string()
    }

    /// A value that is about to get one more owner: an aggregate is marked shared.
    fn owned(&self, e: &Expr) -> String {
        if Structs::aggregate(self.ty(e)) {
            format!("ny_share({})", self.arg(e))
        } else {
            self.arg(e)
        }
    }

    /// `return v` in this function: with the `inout` parameters after the result.
    fn return_line(&self, v: Option<String>) -> String {
        let mut parts: Vec<String> = v.into_iter().collect();
        parts.extend(self.outs.iter().map(|l| self.local(*l).to_string()));
        if parts.is_empty() {
            "return".to_string()
        } else {
            format!("return {}", parts.join(", "))
        }
    }

    fn stmt(&mut self, s: &'a Stmt) {
        let at = format!("{}, {}", s.span.line, s.span.col);
        match &s.kind {
            StmtKind::Set(l, e) => {
                // A struct with only plain fields is not reference counted, so it never gets a `Dup`:
                // a copy of one marks it shared here, so a write to either copy copies it first.
                let t = self.f.local(*l).ty;
                let plain_struct = matches!(t, Ty::Struct(_)) && !self.m.managed(t);
                let v = if plain_struct { self.owned(e) } else { self.arg(e) };
                let line = format!("{} = {v}", self.local(*l));
                self.line(&line);
            }
            StmtKind::Call { dst, func, args } => self.call(*dst, self.m.func(*func), args),
            StmtKind::Op { dst, op, args } => self.op(*dst, *op, args, &at),
            StmtKind::Store { place, value } => {
                let lv = self.place(place, false);
                let line = format!("{lv} = {}", self.owned(value));
                self.line(&line);
            }
            StmtKind::Mutate { dst, op, place, args } => {
                // a string is replaced, an array is changed where it is (after copying it if shared)
                let target = self.place(place, *op != RtOp::StrAppend);
                let a: Vec<String> = args.iter().map(|x| self.arg(x)).collect();
                let code = match op {
                    RtOp::StrAppend => format!("{target} += {}", a[0]),
                    RtOp::ArrPush => format!("{target}.append({})", self.owned(&args[0])),
                    RtOp::ArrPop => format!("ny_pop({target}, {at})"),
                    RtOp::ArrInsert => format!("ny_insert({target}, {}, {}, {at})", a[0], self.owned(&args[1])),
                    RtOp::ArrRemove => format!("ny_remove({target}, {}, {at})", a[0]),
                    RtOp::ArrSort => {
                        if self.place_ty(place).elem() == Some(Ty::Float) {
                            format!("{target}.sort(key=ny_float_key)")
                        } else {
                            format!("{target}.sort()")
                        }
                    }
                    RtOp::ArrSortBy => {
                        if self.ty(&args[0]).elem() == Some(Ty::Float) {
                            format!("ny_sort_by({target}, {}, ny_float_key)", a[0])
                        } else {
                            format!("ny_sort_by({target}, {})", a[0])
                        }
                    }
                    RtOp::ArrReverse => format!("{target}.reverse()"),
                    RtOp::ArrAppend => format!("ny_extend({target}, {})", a[0]),
                    RtOp::ArrSwap => format!("ny_swap({target}, {}, {}, {at})", a[0], a[1]),
                    RtOp::MapSet => {
                        // a tuple used as a key has one more owner: a later change of the variable it came from copies it
                        let key = if matches!(self.ty(&args[0]), Ty::Struct(_)) { format!("ny_share({})", a[0]) } else { a[0].clone() };
                        format!("{target}[{key}] = {}", self.owned(&args[1]))
                    }
                    RtOp::MapRemove => format!("{target}.pop({}, None)", a[0]),
                    other => unreachable!("{} does not change a place", other.name()),
                };
                self.assign(*dst, code);
            }
            StmtKind::If { .. } => self.if_chain(s),
            StmtKind::Loop { head, cond, body, step } => self.lp(head, cond, body, step),
            StmtKind::ForEach { var, iter, body } => {
                // a string gives its characters, an array its elements
                let line = format!("for {} in {}:", self.local(*var), self.arg(iter));
                self.line(&line);
                self.steps.push(&[]);
                self.block(body);
                self.steps.pop();
            }
            StmtKind::Break => self.line("break"),
            StmtKind::Continue => {
                // a counted loop that is not a `for` runs its step first
                if let Some(step) = self.steps.last().copied() {
                    self.stmts(step);
                }
                self.line("continue");
            }
            StmtKind::Return(e) => {
                let v = e.as_ref().map(|e| self.arg(e));
                let line = self.return_line(v);
                self.line(&line);
            }
            // the garbage collector frees memory; a second owner of an aggregate marks it shared
            StmtKind::Dup(l) => {
                if Structs::aggregate(self.f.local(*l).ty) {
                    let line = format!("ny_share({})", self.local(*l));
                    self.line(&line);
                }
            }
            StmtKind::Drop(_) | StmtKind::Keep(_) => {}
            StmtKind::Free(l) => {
                let line = format!("{} = None  # free", self.local(*l));
                self.line(&line);
            }
        }
    }

    /// A call. `inout` arguments are passed by value and come back as results:
    /// `x, y = swap(x, y)`, `n, xs = take(xs)`.
    fn call(&mut self, dst: Option<LocalId>, callee: &Func, args: &[Arg]) {
        let mut parts = Vec::with_capacity(args.len());
        let mut targets = Vec::new();
        let mut roots = Vec::new();
        for a in args {
            match a {
                Arg::Val(e) => parts.push(self.arg(e)),
                Arg::InOut(p) => {
                    let lv = self.place(p, false);
                    parts.push(lv.clone());
                    targets.push(lv);
                    roots.push(p.root);
                }
            }
        }
        let call = format!("{}({})", name(&callee.name), parts.join(", "));
        if targets.is_empty() {
            self.assign(dst, call);
            return;
        }
        let mut late = None;
        if callee.ret.is_some() {
            let first = match dst {
                // the result goes to a variable that is also passed `inout`: assigned after it
                Some(d) if roots.contains(&d) => {
                    let r = self.fresh("r");
                    late = Some((d, r.clone()));
                    r
                }
                Some(d) => self.local(d).to_string(),
                None => "_".to_string(),
            };
            targets.insert(0, first);
        }
        self.line(&format!("{} = {call}", targets.join(", ")));
        if let Some((d, r)) = late {
            let line = format!("{} = {r}", self.local(d));
            self.line(&line);
        }
    }

    /// Emits the copy-on-write steps for a place. Returns an assignable expression for it, or,
    /// with `unique`, the place's own array, unique so it can be changed.
    fn place(&mut self, p: &Place, unique: bool) -> String {
        let mut lv = self.local(p.root).to_string();
        let mut t = self.f.local(p.root).ty;
        if p.path.is_empty() {
            if unique && Structs::aggregate(t) {
                self.line(&format!("{lv} = ny_unique({lv})"));
            }
            return lv;
        }
        // the root changes below this point
        self.line(&format!("{lv} = ny_unique({lv})"));
        let n = p.path.len();
        for (k, step) in p.path.iter().enumerate() {
            let last = k + 1 == n && !unique;
            match step {
                Step::Key(key, span) => {
                    // `m[k]`: the value under the key, unique (a missing key is E0248)
                    let (kt, vt) = t.map_kv().expect("verified: a map");
                    t = vt;
                    let r = self.fresh("p");
                    self.line(&format!(
                        "{r} = ny_unique_key({lv}, {}, \"{}\", {}, {})",
                        self.arg(key),
                        tdesc(kt),
                        span.line,
                        span.col
                    ));
                    lv = r;
                }
                Step::Index(i, span) => {
                    t = t.elem().expect("verified: an array");
                    let key = format!("ny_ck({lv}, {}, {}, {})", self.arg(i), span.line, span.col);
                    if last {
                        return format!("{lv}[{key}]");
                    }
                    let r = self.fresh("p");
                    self.line(&format!("{r} = ny_unique_item({lv}, {key})"));
                    lv = r;
                }
                Step::Field(fi) => {
                    let info = self.m.structs.get(t).expect("verified: a struct");
                    let f = name(&info.fields[*fi as usize].0);
                    t = info.fields[*fi as usize].1;
                    if last {
                        return format!("{lv}.{f}");
                    }
                    let r = self.fresh("p");
                    self.line(&format!("{r} = ny_unique_attr({lv}, \"{f}\")"));
                    lv = r;
                }
            }
        }
        lv
    }

    fn place_ty(&self, p: &Place) -> Ty {
        let mut t = self.f.local(p.root).ty;
        for s in &p.path {
            t = match s {
                Step::Index(..) => t.elem().expect("verified: an array"),
                Step::Key(..) => t.map_kv().expect("verified: a map").1,
                Step::Field(k) => self.m.structs.get(t).expect("verified: a struct").fields[*k as usize].1,
            };
        }
        t
    }

    fn op(&mut self, dst: Option<LocalId>, op: RtOp, args: &[Expr], at: &str) {
        let a: Vec<String> = args.iter().map(|x| self.arg(x)).collect();
        let code = match op {
            RtOp::Print => format!("print({})", self.print_arg(args)),
            RtOp::PrintNoLine => format!("print({}, end=\"\")", self.text(args)),
            RtOp::Format => self.text(args),
            RtOp::DivInt => format!("ny_div({}, {}, {at})", a[0], a[1]),
            RtOp::AddInt => format!("ny_add({}, {}, {at})", a[0], a[1]),
            RtOp::SubInt => format!("ny_sub({}, {}, {at})", a[0], a[1]),
            RtOp::MulInt => format!("ny_mul({}, {}, {at})", a[0], a[1]),
            RtOp::NegInt => format!("ny_neg({}, {at})", a[0]),
            RtOp::RemInt => format!("ny_mod({}, {}, {at})", a[0], a[1]),
            RtOp::FloatToInt => format!("ny_f2i({}, {at})", a[0]),
            RtOp::StrConcat => format!("{} + {}", self.expr(&args[0]), self.expr(&args[1])),
            RtOp::StrAt => format!("ny_char_at({}, {}, {at})", a[0], a[1]),
            RtOp::StrSlice => format!("ny_str_slice({}, {}, {}, {at})", a[0], a[1], a[2]),
            RtOp::StrReplace => format!("ny_replace({}, {}, {}, {at})", a[0], a[1], a[2]),
            RtOp::StrTrim => format!("ny_trim({})", a[0]),
            RtOp::StrUpper => format!("ny_upper({})", a[0]),
            RtOp::StrLower => format!("ny_lower({})", a[0]),
            RtOp::StrRepeat => format!("ny_str_repeat({}, {}, {at})", a[0], a[1]),
            RtOp::StrToInt => format!("ny_int({}, {at})", a[0]),
            RtOp::StrToFloat => format!("ny_float({}, {at})", a[0]),
            RtOp::CharFrom => format!("ny_chr({}, {at})", a[0]),
            RtOp::StrChars => format!("NyList({})", a[0]),
            RtOp::StrCodes => format!("NyList(map(ord, {}))", a[0]),
            RtOp::StrSplit => format!("ny_split({}, {}, {at})", a[0], a[1]),
            RtOp::CheckStep => format!("ny_check_step({}, {at})", a[0]),
            RtOp::CheckSome => format!("ny_check_some({}, {at})", a[0]),
            RtOp::CheckNonEmpty => format!("ny_check_non_empty({}, {}, {at})", a[0], a[1]),
            RtOp::StrPadLeft => format!("ny_pad({}, {}, {}, True)", a[0], a[1], a[2]),
            RtOp::StrPadRight => format!("ny_pad({}, {}, {}, False)", a[0], a[1], a[2]),
            RtOp::ArrNew => {
                let items: Vec<String> = args.iter().map(|x| self.owned(x)).collect();
                if items.is_empty() {
                    "NyList()".to_string()
                } else {
                    format!("NyList([{}])", items.join(", "))
                }
            }
            RtOp::StructNew => {
                let t = self.f.local(dst.expect("verified: a destination")).ty;
                let fields: Vec<String> = args.iter().map(|x| self.owned(x)).collect();
                format!("{}({})", name(&t.struct_name().expect("a struct")), fields.join(", "))
            }
            RtOp::ArrGet => {
                // an element that is a plain struct now has two owners (no `Dup` follows for it)
                let elem = self.ty(&args[0]).elem().expect("verified: an array");
                if matches!(elem, Ty::Struct(_)) && !self.m.managed(elem) {
                    format!("ny_share(ny_at({}, {}, {at}))", a[0], a[1])
                } else {
                    format!("ny_at({}, {}, {at})", a[0], a[1])
                }
            }
            RtOp::ArrSlice => format!("ny_slice({}, {}, {}, {at})", a[0], a[1], a[2]),
            RtOp::ArrRepeat => format!("ny_repeat({}, {}, {at})", a[0], a[1]),
            RtOp::ArrConcat => format!("ny_concat({}, {})", a[0], a[1]),
            // a char is a one-character str: `[str]` and `[char]` join alike
            RtOp::ArrJoin => format!("{}.join({})", self.expr(&args[1]), a[0]),
            RtOp::MapNew => {
                let items: Vec<String> = args
                    .chunks(2)
                    .map(|p| {
                        let key = self.arg(&p[0]);
                        let key = if matches!(self.ty(&p[0]), Ty::Struct(_)) { format!("ny_share({key})") } else { key };
                        format!("({key}, {})", self.owned(&p[1]))
                    })
                    .collect();
                format!("NyDict([{}])", items.join(", "))
            }
            RtOp::MapGet | RtOp::MapGetOr => {
                let (k, v) = self.ty(&args[0]).map_kv().expect("verified: a map");
                let get = if op == RtOp::MapGet {
                    format!("ny_mget({}, {}, \"{}\", {at})", a[0], a[1], tdesc(k))
                } else {
                    format!("{}.get({}, {})", self.expr(&args[0]), a[1], a[2])
                };
                // a value that is a plain struct now has two owners (no `Dup` follows for it)
                if matches!(v, Ty::Struct(_)) && !self.m.managed(v) {
                    format!("ny_share({get})")
                } else {
                    get
                }
            }
            RtOp::MapKeys => format!("NyList({})", a[0]),
            RtOp::MapValues => format!("ny_share_all(NyList({}.values()))", self.expr(&args[0])),
            RtOp::Std(f) => {
                let mut parts = a.clone();
                parts.push(at.to_string());
                format!("ny_std_{}({})", f.rt_name(), parts.join(", "))
            }
            RtOp::JsonStr => format!("ny_jenc({}, {})", a[0], jdesc(self.ty(&args[0]))),
            RtOp::JsonParse => {
                let t = self.f.local(dst.expect("verified: a destination")).ty;
                format!("ny_jparse({}, {}, {at})", a[0], jdesc(t))
            }
            other => unreachable!("{} is a `Mutate`", other.name()),
        };
        self.assign(dst, code);
    }

    fn if_chain(&mut self, s: &'a Stmt) {
        let mut cur = s;
        let mut head = "if";
        loop {
            let StmtKind::If { cond, then, els } = &cur.kind else { unreachable!() };
            let line = format!("{head} {}:", self.arg(cond));
            self.line(&line);
            self.block(then);
            match els.as_slice() {
                [] => break,
                [next] if scope::else_if(els) => {
                    cur = next;
                    head = "elif";
                }
                _ => {
                    self.line("else:");
                    self.block(els);
                    break;
                }
            }
        }
    }

    fn lp(&mut self, head: &'a [Stmt], cond: &Expr, body: &'a [Stmt], step: &'a [Stmt]) {
        let c = self.arg(cond);
        // `continue` runs the step first (see `Continue`)
        self.steps.push(step);
        if head.is_empty() {
            self.line(&format!("while {c}:"));
            self.indent += 1;
        } else {
            self.line("while True:");
            self.indent += 1;
            self.stmts(head);
            let not = self.expr(&Expr::Unary(UnOp::Not, Box::new(cond.clone())));
            self.line(&format!("if {}:", super::bare(&not)));
            self.line("    break");
        }
        let before = self.out.len();
        self.stmts(body);
        self.stmts(step);
        if self.out.len() == before && head.is_empty() {
            self.line("pass");
        }
        self.indent -= 1;
        self.steps.pop();
    }

    /// The argument of `print`: one value as itself, several as an f-string.
    fn print_arg(&self, parts: &[Expr]) -> String {
        if let [p] = parts {
            let x = self.arg(p);
            return match self.ty(p) {
                Ty::Int | Ty::Str | Ty::Char | Ty::Struct(_) => x,
                t => shown_in_fstring(t, &x),
            };
        }
        self.text(parts)
    }

    /// The text of string parts, as an f-string.
    fn text(&self, parts: &[Expr]) -> String {
        let pieces: Vec<Piece> = parts
            .iter()
            .map(|p| match p {
                Expr::Str(id) => Piece::Text(self.m.str(*id).to_string()),
                _ => Piece::Value(shown_in_fstring(self.ty(p), &self.arg(p))),
            })
            .collect();
        fstring(&pieces)
    }

    /// An int expression without the final wrap: sums, differences and products wrap the same
    /// whether each step wraps or only the whole, so only the outermost one calls `ny_i64`.
    fn int_tree(&self, e: &Expr) -> String {
        match e {
            Expr::Binary(op @ (BinOp::IAdd | BinOp::ISub | BinOp::IMul), a, b) => {
                format!("({} {} {})", self.int_tree(a), op.symbol(), self.int_tree(b))
            }
            Expr::Unary(UnOp::INeg, x) => format!("(-{})", self.int_tree(x)),
            _ => self.expr(e),
        }
    }

    fn expr(&self, e: &Expr) -> String {
        match e {
            Expr::Int(n) => n.to_string(),
            Expr::Float(f) if f.is_infinite() => (if *f > 0.0 { "math.inf" } else { "(-math.inf)" }).to_string(),
            Expr::Float(f) => format!("{f:?}"),
            Expr::Bool(b) => (if *b { "True" } else { "False" }).to_string(),
            Expr::Char(c) => lit(&char::from_u32(*c).map(String::from).unwrap_or_default()),
            Expr::Str(id) => lit(self.m.str(*id)),
            Expr::Local(l) => self.local(*l).to_string(),
            Expr::Unary(UnOp::INeg, _) | Expr::Binary(BinOp::IAdd | BinOp::ISub | BinOp::IMul, _, _) => {
                format!("ny_i64({})", super::bare(&self.int_tree(e)))
            }
            Expr::Unary(op, x) => {
                let x = self.expr(x);
                match op {
                    UnOp::FNeg => format!("(-{x})"),
                    _ => format!("(not {x})"),
                }
            }
            Expr::Binary(op, ea, eb) => {
                let (a, b) = (self.expr(ea), self.expr(eb));
                match op {
                    BinOp::IDiv => format!("ny_quot({}, {})", super::bare(&a), super::bare(&b)),
                    BinOp::IRem => format!("ny_rem({}, {})", super::bare(&a), super::bare(&b)),
                    BinOp::FDiv => match **eb {
                        Expr::Float(d) if d != 0.0 => format!("({a} / {b})"),
                        _ => format!("ny_fdiv({}, {})", super::bare(&a), super::bare(&b)),
                    },
                    BinOp::And => format!("({a} and {b})"),
                    BinOp::Or => format!("({a} or {b})"),
                    BinOp::DeepEq | BinOp::DeepNe if has_float(self.m, ea.ty(self.f)) => {
                        let eq = format!("ny_eq({}, {})", super::bare(&a), super::bare(&b));
                        if *op == BinOp::DeepEq {
                            eq
                        } else {
                            format!("(not {eq})")
                        }
                    }
                    _ => format!("({a} {} {b})", op.symbol()),
                }
            }
            Expr::Select(c, a, b) => format!("({} if {} else {})", self.expr(a), self.expr(c), self.expr(b)),
            Expr::IntToFloat(x) => format!("float({})", self.arg(x)),
            Expr::Field(x, k, _) => {
                let info = self.m.structs.get(x.ty(self.f)).expect("verified: a struct");
                format!("{}.{}", self.expr(x), name(&info.fields[*k as usize].0))
            }
            Expr::Pure(p, args) => {
                let a: Vec<String> = args.iter().map(|x| self.arg(x)).collect();
                match p {
                    PureFn::StrLen | PureFn::ArrLen | PureFn::MapLen => format!("len({})", a[0]),
                    PureFn::MapHas => format!("({} in {})", self.expr(&args[1]), self.expr(&args[0])),
                    PureFn::StrContains => format!("({} in {})", self.expr(&args[1]), self.expr(&args[0])),
                    PureFn::StrStartsWith => format!("{}.startswith({})", self.expr(&args[0]), a[1]),
                    PureFn::StrEndsWith => format!("{}.endswith({})", self.expr(&args[0]), a[1]),
                    PureFn::StrIndexOf => format!("{}.find({})", self.expr(&args[0]), a[1]),
                    PureFn::CharCode => format!("ord({})", a[0]),
                    PureFn::CharUpper => format!("ny_char_upper({})", a[0]),
                    PureFn::CharLower => format!("ny_char_lower({})", a[0]),
                    PureFn::CharIsDigit => format!("ny_is_digit({})", a[0]),
                    PureFn::CharIsLetter => format!("ny_is_letter({})", a[0]),
                    PureFn::CharIsUpper => format!("ny_is_upper({})", a[0]),
                    PureFn::CharIsLower => format!("ny_is_lower({})", a[0]),
                    PureFn::CharIsSpace => format!("ny_is_space({})", a[0]),
                    PureFn::ArrContains => {
                        if has_float(self.m, self.ty(&args[1])) {
                            format!("(ny_index_of({}, {}) >= 0)", a[0], a[1])
                        } else {
                            format!("({} in {})", self.expr(&args[1]), self.expr(&args[0]))
                        }
                    }
                    PureFn::ArrIndexOf => format!("ny_index_of({}, {})", a[0], a[1]),
                }
            }
        }
    }
}

/// True for a type that can be (a part of) a map key: `int`, `str`, `char`, `bool` or a tuple of those.
fn key_part(m: &Module, t: Ty) -> bool {
    match t {
        Ty::Int | Ty::Str | Ty::Char | Ty::Bool => true,
        Ty::Struct(_) => m.structs.get(t).is_some_and(|s| s.tuple && s.fields.iter().all(|(_, ft)| key_part(m, *ft))),
        _ => false,
    }
}
