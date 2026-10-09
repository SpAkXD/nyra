//! Go backend. The output is one `package main` file for `go run` / `go build` (Go 1.23+).
//!
//! - `int` is `int64` (Go wraps on overflow), `char` is `rune`, `str` is `string`;
//! - arrays are `*Array[T]`, a slice with a mark for values with more than one owner: a write
//!   to a shared array copies it first (copy on write, like the JavaScript backend); structs are
//!   Go values, and copying one marks the arrays inside it shared;
//! - an `inout` parameter is a pointer;
//! - variables are declared where they are first needed (`x := ...`); Go rejects variables that
//!   are never used, so a value nobody reads is only computed for its effects.

use std::fmt::Write;

use super::names;
use super::scope::{self, range_for, Info};
use crate::ir::{Arg, BinOp, Expr, Func, LocalId, Module, Place, PureFn, RtOp, Step, Stmt, StmtKind, Structs, Ty, UnOp};

const RESERVED: &[&str] = &[
    "break",
    "case",
    "chan",
    "const",
    "continue",
    "default",
    "defer",
    "else",
    "fallthrough",
    "for",
    "func",
    "go",
    "goto",
    "if",
    "import",
    "interface",
    "map",
    "package",
    "range",
    "return",
    "select",
    "struct",
    "switch",
    "type",
    "var",
    "append",
    "bool",
    "byte",
    "cap",
    "clear",
    "close",
    "complex",
    "copy",
    "delete",
    "error",
    "false",
    "float32",
    "float64",
    "imag",
    "int",
    "int8",
    "int16",
    "int32",
    "int64",
    "iota",
    "len",
    "make",
    "max",
    "min",
    "new",
    "nil",
    "panic",
    "print",
    "println",
    "real",
    "recover",
    "rune",
    "string",
    "true",
    "uint",
    "uint8",
    "uint16",
    "uint32",
    "uint64",
    "uintptr",
    "any",
    "comparable",
    "init",
    "bufio",
    "fmt",
    "math",
    "os",
    "slices",
    "strconv",
    "strings",
    "utf8",
    "Array",
    "rand",
    "io",
    "big",
    "time",
];

/// The Go runtime, emitted after the program (`@FILE@` becomes the source path).
const RUNTIME: &str = include_str!("../rt/go/runtime.go");
const STD: &str = include_str!("../rt/go/std.go");
const JSON: &str = include_str!("../rt/go/json.go");

/// A Nyra name as a Go identifier: `ny...` names belong to the runtime, and Go's own words get a `_`.
fn name(n: &str) -> String {
    let id = names::ascii(n);
    if RESERVED.contains(&n) || n.starts_with("ny") || n.starts_with("Ny") {
        format!("{id}_")
    } else {
        id
    }
}

/// A function's name (Go's `main` stays `main`).
fn fn_name(n: &str) -> String {
    if n == "main" {
        n.to_string()
    } else {
        name(n)
    }
}

fn gotype(t: Ty) -> String {
    match t {
        Ty::Int => "int64".into(),
        Ty::Float => "float64".into(),
        Ty::Bool => "bool".into(),
        Ty::Char => "rune".into(),
        Ty::Str => "string".into(),
        Ty::Array(_) => format!("*Array[{}]", gotype(t.elem().expect("an array"))),
        Ty::Map(_) => {
            let (k, v) = t.map_kv().expect("a map");
            format!("*Map[{}, {}]", gotype(k), gotype(v))
        }
        Ty::Struct(_) => name(&t.struct_name().expect("a struct")),
        other => unreachable!("the Go backend got the type `{}`", other.name()),
    }
}

/// A Go string literal.
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

/// A Go rune literal.
fn rune_lit(c: u32) -> String {
    match char::from_u32(c) {
        Some('\'') => "'\\''".into(),
        Some('\\') => "'\\\\'".into(),
        Some('\n') => "'\\n'".into(),
        Some('\r') => "'\\r'".into(),
        Some('\t') => "'\\t'".into(),
        // (a quote or parenthesis is spelled out, so `bare` never mistakes it for a delimiter)
        Some(ch) if c >= 0x20 && c != 0x7f && !matches!(ch, '"' | '(' | ')') => format!("'{ch}'"),
        _ => format!("'\\U{c:08x}'"),
    }
}

/// True for an expression of literals only: Go computes it exactly at compile time (and
/// rejects an overflow or a division by zero), so arithmetic on it goes through `nyI64`/`nyF64`.
fn constant(e: &Expr) -> bool {
    match e {
        Expr::Int(_) | Expr::Float(_) | Expr::Bool(_) | Expr::Char(_) | Expr::Str(_) => true,
        Expr::Unary(_, x) | Expr::IntToFloat(x) => constant(x),
        Expr::Binary(_, a, b) => constant(a) && constant(b),
        _ => false,
    }
}

/// Each struct: the type, and for a struct with arrays inside how copying marks them shared and
/// how it compares; for every struct how it prints.
fn structs(m: &Module, out: &mut String) {
    for (_, s) in &m.structs.0 {
        let n = name(&s.name);
        if s.fields.is_empty() {
            let _ = writeln!(out, "type {n} struct{{}}\n");
        } else {
            let _ = writeln!(out, "type {n} struct {{");
            for (f, t) in &s.fields {
                let _ = writeln!(out, "\t{} {}", name(f), gotype(*t));
            }
            out.push_str("}\n\n");
        }
        if s.managed {
            // a copy shares the arrays inside with the original
            let _ = writeln!(out, "func (v {n}) nyShare() {{");
            for (f, t) in &s.fields {
                if Structs::aggregate(*t) && m.managed(*t) {
                    let _ = writeln!(out, "\tv.{}.nyShare()", name(f));
                }
            }
            out.push_str("}\n\n");
            let eqs: Vec<String> = s
                .fields
                .iter()
                .map(|(f, t)| {
                    let f = name(f);
                    match t {
                        Ty::Array(_) | Ty::Map(_) => format!("nyEqual(v.{f}, w.{f})"),
                        Ty::Struct(_) if m.managed(*t) => format!("v.{f}.nyEq(w.{f})"),
                        _ => format!("v.{f} == w.{f}"),
                    }
                })
                .collect();
            let _ = writeln!(out, "func (v {n}) nyEq(o any) bool {{\n\tw := o.({n})\n\treturn {}\n}}\n", eqs.join(" && "));
        }
        let _ = writeln!(out, "func (v {n}) nyShowIn(b *strings.Builder) {{");
        if s.option {
            // `none`, or `Some(value)`
            let _ = writeln!(out, "\tif !v.{} {{\n\t\tb.WriteString(\"none\")\n\t\treturn\n\t}}", name(&s.fields[0].0));
        }
        let head = if s.tuple {
            "(".to_string()
        } else if s.option {
            "Some(".to_string()
        } else {
            format!("{}(", s.name)
        };
        let _ = writeln!(out, "\tb.WriteString({})", lit(&head));
        for (k, (f, _)) in s.fields.iter().enumerate() {
            if s.option && k == 0 {
                continue;
            }
            let sep = if k > 0 { ", " } else { "" };
            let label = if s.tuple {
                sep.to_string()
            } else if s.option {
                String::new()
            } else {
                format!("{sep}{f}: ")
            };
            if !label.is_empty() {
                let _ = writeln!(out, "\tb.WriteString({})", lit(&label));
            }
            let _ = writeln!(out, "\tnyShowAny(b, v.{})", name(f));
        }
        out.push_str("\tb.WriteByte(')')\n}\n\n");
    }
}

/// `file` is the source path as given to nyra; runtime errors report it.
pub fn gen(m: &Module, file: &str) -> String {
    // the standard library needs more of Go's packages (Go rejects unused imports)
    let std = m.uses_std();
    let more = if std { "\t\"crypto/rand\"\n\t\"io\"\n\t\"math/big\"\n\t\"time\"\n" } else { "" };
    let mut out = format!(
        concat!(
            "// generated by nyra: Go (run with `go run file.go`)\n",
            "package main\n\n",
            "import (\n\t\"bufio\"\n\t\"fmt\"\n\t\"math\"\n\t\"os\"\n\t\"slices\"\n\t\"strconv\"\n\t\"strings\"\n\t\"unicode/utf8\"\n{})\n\n",
        ),
        more
    );
    structs(m, &mut out);
    let json = m.uses_json();
    if json {
        json_funcs(m, &mut out);
    }
    for f in &m.funcs {
        let info = Info::new(f);
        let n = names::scoped(f, name, "ny_", &info.loop_var);
        let params: Vec<String> = (0..f.params)
            .map(|i| {
                let t = gotype(f.locals[i].ty);
                if f.locals[i].inout {
                    format!("{} *{t}", n[i])
                } else {
                    format!("{} {t}", n[i])
                }
            })
            .collect();
        let ret = f.ret.map_or(String::new(), |t| format!(" {}", gotype(t)));
        let _ = writeln!(out, "func {}({}){ret} {{", fn_name(&f.name), params.join(", "));
        let mut g = Gen { m, f, info: &info, names: &n, out: String::new(), indent: 1, tmp: 0 };
        if f.name == m.func(m.main).name {
            g.line("defer nyOut.Flush()");
        }
        g.stmts(&f.body);
        // Go wants a function with a result to end in a `return`
        if f.ret.is_some() && !matches!(f.body.last().map(|s| &s.kind), Some(StmtKind::Return(_))) {
            g.line("panic(\"unreachable\")");
        }
        out.push_str(&g.out);
        out.push_str("}\n\n");
    }
    out.push_str(&RUNTIME.replace("@FILE@", &lit(file)));
    if std {
        out.push_str(STD);
    }
    if json {
        out.push_str(JSON);
    }
    out
}

/// The decoder of a type for `json.parse`: a function `func(*nyJP) T`.
fn jdec(t: Ty) -> String {
    match t {
        Ty::Int => "nyJInt".into(),
        Ty::Float => "nyJFloat".into(),
        Ty::Bool => "nyJBool".into(),
        Ty::Char => "nyJChar".into(),
        Ty::Str => "nyJStr".into(),
        Ty::Array(_) => format!("nyJArr({})", jdec(t.elem().expect("an array"))),
        _ => format!("nyJD_{}", name(&t.struct_name().expect("a struct"))),
    }
}

/// How each struct is written as JSON (a method) and read from it (a function).
fn json_funcs(m: &Module, out: &mut String) {
    for (_, s) in &m.structs.0 {
        let n = name(&s.name);
        let _ = writeln!(out, "func (v {n}) nyJEnc(b *strings.Builder) {{");
        for (k, (f, _)) in s.fields.iter().enumerate() {
            let key = format!("{}{}:", if k == 0 { "{" } else { "," }, crate::diag::json_str(f));
            let _ = writeln!(out, "\tb.WriteString({})\n\tnyJEnc(b, v.{})", lit(&key), name(f));
        }
        if s.fields.is_empty() {
            out.push_str("\tb.WriteByte('{')\n");
        }
        out.push_str("\tb.WriteByte('}')\n}\n\n");
        let _ = writeln!(out, "func nyJD_{n}(p *nyJP) {n} {{\n\tvar v {n}");
        if !s.fields.is_empty() {
            let _ = writeln!(out, "\tvar seen [{}]bool", s.fields.len());
        }
        out.push_str("\tif p.open('{', \"an object\") {\n\t\tfor {\n\t\t\tswitch p.key() {\n");
        for (k, (f, t)) in s.fields.iter().enumerate() {
            let _ = writeln!(
                out,
                "\t\t\tcase {}:\n\t\t\t\tp.path = append(p.path, {})\n\t\t\t\tv.{} = {}(p)\n\t\t\t\tp.path = p.path[:len(p.path)-1]\n\t\t\t\tseen[{k}] = true",
                lit(f),
                lit(&format!(".{f}")),
                name(f),
                jdec(*t)
            );
        }
        out.push_str("\t\t\tdefault:\n\t\t\t\tp.skip()\n\t\t\t}\n\t\t\tif !p.next('}') {\n\t\t\t\tbreak\n\t\t\t}\n\t\t}\n\t}\n");
        for (k, (f, _)) in s.fields.iter().enumerate() {
            let _ = writeln!(out, "\tif !seen[{k}] {{\n\t\tp.missing({})\n\t}}", lit(f));
        }
        out.push_str("\treturn v\n}\n\n");
    }
}

struct Gen<'a> {
    m: &'a Module,
    f: &'a Func,
    info: &'a Info,
    names: &'a [String],
    out: String,
    indent: usize,
    /// Counter for helper names (element pointers).
    tmp: usize,
}

impl<'a> Gen<'a> {
    fn line(&mut self, s: &str) {
        for _ in 0..self.indent {
            self.out.push('\t');
        }
        self.out.push_str(s);
        self.out.push('\n');
    }

    fn block(&mut self, ss: &'a [Stmt]) {
        self.indent += 1;
        self.stmts(ss);
        self.indent -= 1;
    }

    fn stmts(&mut self, ss: &'a [Stmt]) {
        let mut k = 0;
        while k < ss.len() {
            let s = &ss[k];
            for &l in self.info.before(s) {
                if !self.info.unread(l) {
                    let line = format!("var {} {}", self.names[l.0 as usize], gotype(self.f.local(l).ty));
                    self.line(&line);
                }
            }
            if let Some(r) = range_for(ss, k, self.info) {
                if let Some(p) = r.pre {
                    self.stmt(p);
                }
                let i = self.names[r.var.0 as usize].clone();
                let line = format!("for {i} := {}; {i} < {}; {i}++ {{", self.typed_int(r.start), self.arg(r.end));
                self.line(&line);
                self.block(r.body);
                self.line("}");
                k += r.len;
                continue;
            }
            // `x = v` and then `dup x`: one statement that marks the value shared
            if let (StmtKind::Set(l, e), Some(Stmt { kind: StmtKind::Dup(d), .. })) = (&s.kind, ss.get(k + 1)) {
                if l == d {
                    let v = self.owned(e);
                    self.set(s, *l, v);
                    k += 2;
                    continue;
                }
            }
            self.stmt(s);
            k += 1;
        }
    }

    fn local(&self, l: LocalId) -> String {
        let i = l.0 as usize;
        if i < self.f.params && self.f.locals[i].inout {
            format!("(*{})", self.names[i])
        } else {
            self.names[i].clone()
        }
    }

    fn ty(&self, e: &Expr) -> Ty {
        e.ty(self.f)
    }

    fn fresh(&mut self, prefix: &str) -> String {
        self.tmp += 1;
        format!("ny_{prefix}{}", self.tmp)
    }

    /// An int value that may be an untyped constant (`for i := 0; ...` must be an `int64`).
    fn typed_int(&self, e: &Expr) -> String {
        if constant(e) {
            format!("int64({})", self.arg(e))
        } else {
            self.arg(e)
        }
    }

    /// `x := value` when `s` declares `x`, else `x = value`. A local nobody reads is not
    /// declared (Go rejects it); the value is still computed for its effects.
    fn set(&mut self, s: &Stmt, l: LocalId, value: String) {
        // (an `inout` parameter is read by the caller)
        let inout = (l.0 as usize) < self.f.params && self.f.locals[l.0 as usize].inout;
        let line = if self.info.unread(l) && !inout {
            if constant_text(&value) {
                return;
            }
            format!("_ = {value}")
        } else if self.info.declares(s) == Some(l) {
            let n = &self.names[l.0 as usize];
            let t = self.f.local(l).ty;
            if t == Ty::Int && constant_text(&value) {
                format!("var {n} int64 = {value}")
            } else {
                format!("{n} := {value}")
            }
        } else {
            format!("{} = {value}", self.local(l))
        };
        self.line(&line);
    }

    /// `dst = value` or `value`
    fn assign(&mut self, s: &Stmt, dst: Option<LocalId>, value: String) {
        match dst {
            Some(d) => self.set(s, d, value),
            None => self.line(&value),
        }
    }

    fn arg(&self, e: &Expr) -> String {
        super::bare(&self.expr(e)).to_string()
    }

    /// A value that is about to get one more owner: arrays inside it are marked shared.
    fn owned(&self, e: &Expr) -> String {
        if self.m.managed(self.ty(e)) && Structs::aggregate(self.ty(e)) {
            format!("nyShare({})", self.arg(e))
        } else {
            self.arg(e)
        }
    }

    fn stmt(&mut self, s: &'a Stmt) {
        let at = format!("{}, {}", s.span.line, s.span.col);
        match &s.kind {
            StmtKind::Set(l, e) => {
                let v = self.arg(e);
                self.set(s, *l, v);
            }
            StmtKind::Call { dst, func, args } => {
                let mut parts = Vec::with_capacity(args.len());
                for a in args {
                    match a {
                        Arg::Val(e) => parts.push(self.arg(e)),
                        Arg::InOut(p) => {
                            let lv = self.place(p, false);
                            parts.push(format!("&{lv}"));
                        }
                    }
                }
                let call = format!("{}({})", fn_name(&self.m.func(*func).name), parts.join(", "));
                match dst {
                    Some(d) => self.set(s, *d, call),
                    None => self.line(&call),
                }
            }
            StmtKind::Op { dst, op, args } => self.op(s, *dst, *op, args, &at),
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
                    RtOp::ArrPush => format!("{target}.items = append({target}.items, {})", self.owned(&args[0])),
                    RtOp::ArrPop => format!("nyPop({target}, {at})"),
                    RtOp::ArrInsert => format!("nyInsert({target}, {}, {}, {at})", a[0], self.owned(&args[1])),
                    RtOp::ArrRemove => format!("nyRemove({target}, {}, {at})", a[0]),
                    RtOp::ArrSort => {
                        if self.place_ty(place).elem() == Some(Ty::Float) {
                            format!("slices.SortStableFunc({target}.items, nyCmpFloat)")
                        } else {
                            format!("slices.Sort({target}.items)")
                        }
                    }
                    RtOp::ArrReverse => format!("slices.Reverse({target}.items)"),
                    RtOp::ArrSortBy => {
                        let k = self.ty(&args[0]).elem().expect("verified: keys");
                        let cmp = if k == Ty::Float { "nyCmpFloat".to_string() } else { format!("nyCmpOrd[{}]", gotype(k)) };
                        format!("nySortBy({target}, {}, {cmp})", a[0])
                    }
                    RtOp::ArrSwap => format!("nySwap({target}, {}, {}, {at})", a[0], a[1]),
                    RtOp::ArrAppend => format!("nyExtend({target}, {})", a[0]),
                    RtOp::MapSet => format!("{target}.set({}, {})", a[0], self.owned(&args[1])),
                    RtOp::MapRemove => format!("{target}.remove({})", a[0]),
                    other => unreachable!("{} does not change a place", other.name()),
                };
                self.assign(s, *dst, code);
            }
            StmtKind::If { .. } => self.if_chain(s),
            StmtKind::Loop { head, cond, body, step } => self.lp(head, cond, body, step),
            StmtKind::ForEach { var, iter, body } => {
                // a string gives its characters (runes), an array its elements
                let it = if self.ty(iter) == Ty::Str { self.arg(iter) } else { format!("{}.items", self.expr(iter)) };
                let line = if self.info.unread(*var) {
                    format!("for range {it} {{")
                } else {
                    format!("for _, {} := range {it} {{", self.names[var.0 as usize])
                };
                self.line(&line);
                self.block(body);
                self.line("}");
            }
            StmtKind::Break => self.line("break"),
            StmtKind::Continue => self.line("continue"),
            StmtKind::Return(None) => self.line("return"),
            StmtKind::Return(Some(e)) => {
                let line = format!("return {}", self.arg(e));
                self.line(&line);
            }
            // a second owner of a value with arrays inside marks them shared
            StmtKind::Dup(l) => {
                let t = self.f.local(*l).ty;
                if Structs::aggregate(t) && self.m.managed(t) && !self.info.unread(*l) {
                    let line = format!("nyShare({})", self.local(*l));
                    self.line(&line);
                }
            }
            StmtKind::Drop(_) | StmtKind::Keep(_) => {}
            StmtKind::Free(l) => {
                if !self.info.unread(*l) {
                    let t = self.f.local(*l).ty;
                    let zero = match t {
                        Ty::Str => "\"\"".to_string(),
                        Ty::Struct(_) => format!("{}{{}}", gotype(t)),
                        _ => "nil".to_string(),
                    };
                    let line = format!("{} = {zero} // free", self.local(*l));
                    self.line(&line);
                }
            }
        }
    }

    /// Emits what makes every array on the way to a place unique (copied when shared) and
    /// returns an assignable expression for the place. With `unique`, an array at the place is
    /// made unique too, so it can be changed.
    fn place(&mut self, p: &Place, unique: bool) -> String {
        let mut lv = self.local(p.root);
        let mut t = self.f.local(p.root).ty;
        let n = p.path.len();
        if t.elem().is_some() && (n > 0 || unique) {
            self.line(&format!("{lv} = nyUnique({lv})"));
        } else if t.map_kv().is_some() && n == 0 && unique {
            self.line(&format!("{lv} = nyMUnique({lv})"));
        }
        for (k, step) in p.path.iter().enumerate() {
            let last = k + 1 == n;
            match step {
                Step::Index(i, span) => {
                    t = t.elem().expect("verified: an array");
                    let elem = format!("{lv}.items[nyCheck({lv}, {}, {}, {})]", self.arg(i), span.line, span.col);
                    let array = t.elem().is_some();
                    let map = t.map_kv().is_some();
                    if last && !(unique && (array || map)) {
                        return elem;
                    }
                    // a pointer to the element, so the index is checked once
                    let ptr = self.fresh("p");
                    self.line(&format!("{ptr} := &{elem}"));
                    if array {
                        self.line(&format!("*{ptr} = nyUnique(*{ptr})"));
                    } else if map {
                        self.line(&format!("*{ptr} = nyMUnique(*{ptr})"));
                    }
                    lv = format!("(*{ptr})");
                }
                Step::Field(fi) => {
                    let info = self.m.structs.get(t).expect("verified: a struct");
                    t = info.fields[*fi as usize].1;
                    lv = format!("{lv}.{}", name(&info.fields[*fi as usize].0));
                    if t.elem().is_some() && (!last || unique) {
                        self.line(&format!("{lv} = nyUnique({lv})"));
                    } else if t.map_kv().is_some() && last && unique {
                        self.line(&format!("{lv} = nyMUnique({lv})"));
                    }
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
                Step::Field(k) => self.m.structs.get(t).expect("verified: a struct").fields[*k as usize].1,
            };
        }
        t
    }

    fn op(&mut self, s: &Stmt, dst: Option<LocalId>, op: RtOp, args: &[Expr], at: &str) {
        let a: Vec<String> = args.iter().map(|x| self.arg(x)).collect();
        let code = match op {
            RtOp::Print => {
                let line = format!("fmt.Fprintln(nyOut, {})", self.text(args, true));
                self.line(&line);
                return;
            }
            RtOp::PrintNoLine => {
                let line = format!("nyOut.WriteString({})", self.text(args, false));
                self.line(&line);
                return;
            }
            RtOp::Format => self.text(args, false),
            RtOp::DivInt => format!("nyDiv({}, {}, {at})", a[0], a[1]),
            RtOp::AddInt => format!("nyAdd({}, {}, {at})", a[0], a[1]),
            RtOp::SubInt => format!("nySub({}, {}, {at})", a[0], a[1]),
            RtOp::MulInt => format!("nyMul({}, {}, {at})", a[0], a[1]),
            RtOp::NegInt => format!("nyNeg({}, {at})", a[0]),
            RtOp::RemInt => format!("nyRem({}, {}, {at})", a[0], a[1]),
            RtOp::FloatToInt => format!("nyF2I({}, {at})", a[0]),
            RtOp::CheckStep => format!("nyCheckStep({}, {at})", a[0]),
            RtOp::CheckSome => format!("nyCheckSome({}, {at})", a[0]),
            RtOp::CheckNonEmpty => format!("nyCheckNonEmpty({}, {}, {at})", a[0], a[1]),
            RtOp::StrConcat => format!("{} + {}", self.expr(&args[0]), self.expr(&args[1])),
            RtOp::StrAt => format!("nyCharAt({}, {}, {at})", a[0], a[1]),
            RtOp::StrSlice => format!("nyStrSlice({}, {}, {}, {at})", a[0], a[1], a[2]),
            RtOp::StrReplace => format!("nyReplace({}, {}, {}, {at})", a[0], a[1], a[2]),
            RtOp::StrTrim => format!("strings.Trim({}, \" \\t\\n\\r\")", a[0]),
            RtOp::StrUpper => format!("nyUpper({})", a[0]),
            RtOp::StrLower => format!("nyLower({})", a[0]),
            RtOp::StrRepeat => format!("nyRepeatStr({}, {}, {at})", a[0], a[1]),
            RtOp::StrPadLeft => format!("nyPad({}, {}, {}, true)", a[0], a[1], a[2]),
            RtOp::StrPadRight => format!("nyPad({}, {}, {}, false)", a[0], a[1], a[2]),
            RtOp::StrToInt => format!("nyInt({}, {at})", a[0]),
            RtOp::StrToFloat => format!("nyFloat({}, {at})", a[0]),
            RtOp::CharFrom => format!("nyChr({}, {at})", a[0]),
            RtOp::StrChars => format!("nyChars({})", a[0]),
            RtOp::StrCodes => format!("nyCodes({})", a[0]),
            RtOp::StrSplit => format!("nySplit({}, {}, {at})", a[0], a[1]),
            RtOp::ArrNew => {
                let t = self.f.local(dst.expect("verified: a destination")).ty;
                let items: Vec<String> = args.iter().map(|x| self.owned(x)).collect();
                format!("nyArray[{}]({})", gotype(t.elem().expect("verified: an array")), items.join(", "))
            }
            RtOp::StructNew => {
                let t = self.f.local(dst.expect("verified: a destination")).ty;
                let info = self.m.structs.get(t).expect("verified: a struct");
                let fields: Vec<String> =
                    args.iter().zip(&info.fields).map(|(x, (f, _))| format!("{}: {}", name(f), self.owned(x))).collect();
                format!("{}{{{}}}", name(&info.name), fields.join(", "))
            }
            RtOp::ArrGet => format!("nyGet({}, {}, {at})", a[0], a[1]),
            RtOp::MapNew => {
                let t = self.f.local(dst.expect("verified: a destination")).ty;
                let (k, v) = t.map_kv().expect("verified: a map");
                let keys: Vec<String> = args.chunks(2).map(|p| self.arg(&p[0])).collect();
                let vals: Vec<String> = args.chunks(2).map(|p| self.owned(&p[1])).collect();
                format!("nyMOf([]{}{{{}}}, []{}{{{}}})", gotype(k), keys.join(", "), gotype(v), vals.join(", "))
            }
            RtOp::MapGet => format!("nyMGet({}, {}, {at})", a[0], a[1]),
            RtOp::MapGetOr => format!("nyMGetOr({}, {}, {})", a[0], a[1], a[2]),
            RtOp::MapKeys => format!("nyMKeys({})", a[0]),
            RtOp::MapValues => format!("nyMValues({})", a[0]),
            RtOp::ArrSlice => format!("nySlice({}, {}, {}, {at})", a[0], a[1], a[2]),
            RtOp::ArrRepeat => format!("nyRepeat({}, {}, {at})", a[0], a[1]),
            RtOp::ArrConcat => format!("nyConcat({}, {})", a[0], a[1]),
            RtOp::ArrJoin => {
                let f = if self.ty(&args[0]).elem() == Some(Ty::Char) { "nyJoinChars" } else { "nyJoin" };
                format!("{f}({}, {})", a[0], a[1])
            }
            RtOp::Std(f) => {
                let mut parts = a.clone();
                parts.push(at.to_string());
                format!("nyStd_{}({})", f.rt_name(), parts.join(", "))
            }
            RtOp::JsonStr => format!("nyJStrOf({})", a[0]),
            RtOp::JsonParse => {
                let t = self.f.local(dst.expect("verified: a destination")).ty;
                format!("nyJParse({}, {}, {at})", a[0], jdec(t))
            }
            other => unreachable!("{} is a `Mutate`", other.name()),
        };
        self.assign(s, dst, code);
    }

    /// The text of string parts (a print's line, an interpolation): a `+` of strings. For a
    /// print, one value that `fmt` shows the same way is passed as it is.
    fn text(&self, parts: &[Expr], print: bool) -> String {
        if parts.is_empty() {
            return "\"\"".into();
        }
        if let [p] = parts {
            if print && matches!(self.ty(p), Ty::Int | Ty::Str | Ty::Bool) && !matches!(p, Expr::Str(_)) {
                return self.arg(p);
            }
        }
        let mut pieces: Vec<String> = Vec::new();
        for p in parts {
            match p {
                Expr::Str(id) => {
                    let s = self.m.str(*id);
                    // neighbouring text is joined
                    if let Some(last) = pieces.last_mut() {
                        if last.starts_with('"') && last.ends_with('"') && last.len() >= 2 && !last.ends_with("\\\"") {
                            let joined = format!("{}{}", &last[..last.len() - 1], &lit(s)[1..]);
                            *last = joined;
                            continue;
                        }
                    }
                    pieces.push(lit(s));
                }
                _ => {
                    let x = self.arg(p);
                    pieces.push(match self.ty(p) {
                        Ty::Int => format!("strconv.FormatInt({x}, 10)"),
                        Ty::Float => format!("nyNum({x})"),
                        Ty::Bool => format!("strconv.FormatBool({x})"),
                        Ty::Char => format!("string({x})"),
                        Ty::Str => self.expr(p),
                        _ => format!("nyShow({x})"),
                    });
                }
            }
        }
        pieces.join(" + ")
    }

    fn if_chain(&mut self, s: &'a Stmt) {
        let mut cur = s;
        let mut head = "if";
        loop {
            let StmtKind::If { cond, then, els } = &cur.kind else { unreachable!() };
            let line = format!("{head} {} {{", self.arg(cond));
            self.line(&line);
            self.block(then);
            match els.as_slice() {
                [] => break,
                [next] if scope::else_if(els) => {
                    cur = next;
                    head = "} else if";
                }
                _ => {
                    self.line("} else {");
                    self.block(els);
                    break;
                }
            }
        }
        self.line("}");
    }

    fn lp(&mut self, head: &'a [Stmt], cond: &Expr, body: &'a [Stmt], step: &'a [Stmt]) {
        // the step goes in the `for` header, so `continue` runs it
        let steps: Vec<String> = step
            .iter()
            .map(|s| match &s.kind {
                StmtKind::Set(l, e) => format!("{} = {}", self.local(*l), self.arg(e)),
                _ => unreachable!("a loop step only assigns"),
            })
            .collect();
        let step = steps.join("; ");
        if head.is_empty() {
            let c = self.arg(cond);
            if step.is_empty() {
                self.line(&format!("for {c} {{"));
            } else {
                self.line(&format!("for ; {c}; {step} {{"));
            }
            self.block(body);
        } else {
            if step.is_empty() {
                self.line("for {");
            } else {
                self.line(&format!("for ; ; {step} {{"));
            }
            self.indent += 1;
            self.stmts(head);
            let line = format!("if !{} {{", self.expr(cond));
            self.line(&line);
            self.line("\tbreak");
            self.line("}");
            self.stmts(body);
            self.indent -= 1;
        }
        self.line("}");
    }

    fn expr(&self, e: &Expr) -> String {
        match e {
            Expr::Int(i64::MIN) => "math.MinInt64".to_string(),
            Expr::Int(n) => n.to_string(),
            Expr::Float(f) if f.is_infinite() => (if *f > 0.0 { "math.Inf(1)" } else { "math.Inf(-1)" }).to_string(),
            Expr::Float(f) if *f == 0.0 && f.is_sign_negative() => "math.Copysign(0, -1)".to_string(),
            Expr::Float(f) => format!("{f:?}"),
            Expr::Bool(b) => b.to_string(),
            Expr::Char(c) => rune_lit(*c),
            Expr::Str(id) => lit(self.m.str(*id)),
            Expr::Local(l) => self.local(*l),
            Expr::Unary(op, x) => {
                let mut v = self.expr(x);
                if constant(x) && *op != UnOp::Not {
                    v = if *op == UnOp::INeg { format!("nyI64({})", bare_s(&v)) } else { format!("nyF64({})", bare_s(&v)) };
                }
                match op {
                    UnOp::Not => format!("(!{v})"),
                    _ if v.starts_with('-') => format!("(-({v}))"),
                    _ => format!("(-{v})"),
                }
            }
            Expr::Binary(op, ea, eb) => {
                let (mut a, mut b) = (self.expr(ea), self.expr(eb));
                let int = matches!(op, BinOp::IAdd | BinOp::ISub | BinOp::IMul | BinOp::IDiv | BinOp::IRem);
                let float = matches!(op, BinOp::FAdd | BinOp::FSub | BinOp::FMul | BinOp::FDiv);
                // Go computes constant expressions exactly at compile time: keep Nyra's wrapping
                // and IEEE results (`1.0 / 0.0` is Infinity) by computing them at run time
                if (int || float) && constant(ea) && constant(eb) {
                    a = format!("{}({})", if int { "nyI64" } else { "nyF64" }, bare_s(&a));
                } else if *op == BinOp::FDiv && constant(eb) && !matches!(**eb, Expr::Float(d) if d != 0.0) {
                    b = format!("nyF64({})", bare_s(&b));
                }
                match op {
                    BinOp::DeepEq | BinOp::DeepNe => {
                        let t = ea.ty(self.f);
                        let eq = match t {
                            Ty::Struct(_) if self.m.managed(t) => format!("{a}.nyEq({})", bare_s(&b)),
                            Ty::Struct(_) => format!("({a} == {b})"),
                            _ => format!("nyEqual({}, {})", bare_s(&a), bare_s(&b)),
                        };
                        if *op == BinOp::DeepEq {
                            eq
                        } else {
                            format!("(!{eq})")
                        }
                    }
                    // the conversion keeps Go from fusing `a * b + c` into one rounding (it may on
                    // some CPUs), so floats compute exactly as on every other backend
                    BinOp::FMul => format!("float64({a} * {b})"),
                    _ => format!("({a} {} {b})", op.symbol()),
                }
            }
            Expr::Select(c, a, b) => {
                format!("nyIf[{}]({}, {}, {})", gotype(a.ty(self.f)), self.arg(c), self.arg(a), self.arg(b))
            }
            Expr::IntToFloat(x) => format!("float64({})", self.arg(x)),
            Expr::Field(x, k, _) => {
                let info = self.m.structs.get(x.ty(self.f)).expect("verified: a struct");
                format!("{}.{}", self.expr(x), name(&info.fields[*k as usize].0))
            }
            Expr::Pure(p, args) => {
                let a: Vec<String> = args.iter().map(|x| self.arg(x)).collect();
                match p {
                    PureFn::StrLen => format!("nyLen({})", a[0]),
                    PureFn::StrContains => format!("strings.Contains({}, {})", a[0], a[1]),
                    PureFn::StrStartsWith => format!("strings.HasPrefix({}, {})", a[0], a[1]),
                    PureFn::StrEndsWith => format!("strings.HasSuffix({}, {})", a[0], a[1]),
                    PureFn::StrIndexOf => format!("nyFind({}, {})", a[0], a[1]),
                    PureFn::CharCode => format!("int64({})", a[0]),
                    PureFn::CharUpper => format!("nyCharUpper({})", a[0]),
                    PureFn::CharLower => format!("nyCharLower({})", a[0]),
                    PureFn::CharIsDigit => format!("nyIsDigit({})", a[0]),
                    PureFn::CharIsLetter => format!("nyIsLetter({})", a[0]),
                    PureFn::CharIsUpper => format!("nyIsUpper({})", a[0]),
                    PureFn::CharIsLower => format!("nyIsLower({})", a[0]),
                    PureFn::CharIsSpace => format!("nyIsSpace({})", a[0]),
                    PureFn::ArrLen => format!("int64(len({}.items))", self.expr(&args[0])),
                    PureFn::MapLen => format!("int64({}.live)", self.expr(&args[0])),
                    PureFn::MapHas => format!("{}.has({})", self.expr(&args[0]), a[1]),
                    PureFn::ArrContains => format!("(nyIndexOf({}, {}) >= 0)", a[0], a[1]),
                    PureFn::ArrIndexOf => format!("nyIndexOf({}, {})", a[0], a[1]),
                }
            }
        }
    }
}

fn bare_s(s: &str) -> &str {
    super::bare(s)
}

/// True if generated text is a literal (an untyped Go constant).
fn constant_text(s: &str) -> bool {
    let t = s.trim_start_matches(['(', '-']).trim_end_matches(')');
    !t.is_empty()
        && t.chars().all(|c| {
            c.is_ascii_digit()
                || c == '.'
                || c == 'e'
                || c == '+'
                || c == '-'
                || c == ' '
                || c == '*'
                || c == '/'
                || c == '('
                || c == ')'
        })
}
