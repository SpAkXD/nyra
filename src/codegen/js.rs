//! JavaScript backend. Output runs on Node.js or in the browser.

use std::cell::Cell;
use std::fmt::Write;

use super::{bare, names};
use crate::ast::Span;
use crate::ir::{Arg, BinOp, Expr, Func, LocalId, Module, Place, PureFn, RtOp, Step, Stmt, StmtKind, Structs, Ty, UnOp};

/// The largest int JavaScript numbers hold exactly (`Number.MAX_SAFE_INTEGER`).
const SAFE_INT: u64 = (1 << 53) - 1;

const RESERVED: &[&str] = &[
    "arguments",
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "enum",
    "eval",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "function",
    "if",
    "implements",
    "import",
    "in",
    "instanceof",
    "interface",
    "let",
    "new",
    "null",
    "package",
    "private",
    "protected",
    "public",
    "return",
    "static",
    "super",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "typeof",
    "var",
    "void",
    "while",
    "with",
    "yield",
    "undefined",
    "NaN",
    "Infinity",
    "console",
    "Math",
    "String",
    "Number",
    "Object",
    "Array",
    "JSON",
    "Symbol",
    "BigInt",
    "Error",
    "RangeError",
    "globalThis",
    "process",
    "require",
    "module",
    "exports",
    "NyPanic",
    "NY_SURR",
    "NY_ESC",
    "NyExit",
    "Buffer",
    "TextDecoder",
    "Atomics",
    "SharedArrayBuffer",
    "Int32Array",
    "Uint32Array",
    "Uint8Array",
    "ArrayBuffer",
    "DataView",
    "Date",
    "performance",
    "crypto",
];

/// The JavaScript runtime, emitted before every program (`@FILE@` becomes the source path).
const PRELUDE: &str = include_str!("../rt/js/core.js");
const STRINGS: &str = include_str!("../rt/js/str.js");
const ARRAYS: &str = include_str!("../rt/js/arr.js");
const STD: &str = include_str!("../rt/js/std.js");
const JSON: &str = include_str!("../rt/js/json.js");

/// A Nyra name as a JavaScript identifier. Nyra allows any letter (`x²`, `größe`); characters
/// JavaScript does not accept in names are spelled out as `_u{hex}_`.
fn ident(n: &str) -> String {
    let ok = |c: char, first: bool| c == '_' || c == '$' || c.is_ascii_alphabetic() || (!first && c.is_ascii_digit());
    if n.chars().enumerate().all(|(i, c)| ok(c, i == 0)) {
        return n.to_string();
    }
    let mut out = String::from("nyU_");
    for c in n.chars() {
        if ok(c, false) {
            out.push(c);
        } else {
            out.push_str(&format!("_u{:x}_", c as u32));
        }
    }
    out
}

/// `ny...` names belong to nyra (helpers `ny_*`, struct classes `nyS_*`), so user names that start
/// with `ny` get a `_`, like reserved words.
fn name(n: &str) -> String {
    if RESERVED.contains(&n) || n.starts_with("ny") {
        format!("{}_", ident(n))
    } else {
        ident(n)
    }
}

/// The type descriptor `ny_fmt` prints a value with (a char is a number in JavaScript).
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

/// The type as the JSON runtime reads it: "i", "f", "b", "c", "s", ["a", T], or a struct's class.
fn jdesc(t: Ty) -> String {
    match t {
        Ty::Int => "\"i\"".into(),
        Ty::Float => "\"f\"".into(),
        Ty::Bool => "\"b\"".into(),
        Ty::Char => "\"c\"".into(),
        Ty::Str => "\"s\"".into(),
        Ty::Array(_) => format!("[\"a\", {}]", jdesc(t.elem().expect("an array"))),
        _ => format!("nyS_{}", ident(&t.struct_name().expect("a struct"))),
    }
}

/// The JavaScript name of a field: names the runtime uses on objects get a `_`.
fn jfield(name: &str) -> String {
    if name.starts_with("ny") || matches!(name, "constructor" | "prototype" | "__proto__") {
        format!("{}_", ident(name))
    } else {
        ident(name)
    }
}

/// One class per struct: copying one level (copy on write), deep equality and printing.
fn classes(m: &Module, out: &mut String) {
    for (_, s) in &m.structs.0 {
        let n = format!("nyS_{}", ident(&s.name));
        let fields: Vec<String> = s.fields.iter().map(|(f, _)| jfield(f)).collect();
        let _ = writeln!(out, "class {n} {{");
        // parameters by position: a field may be called like a reserved word (`class`)
        let params: Vec<String> = (0..fields.len()).map(|i| format!("p{i}")).collect();
        let sets: String = fields.iter().enumerate().map(|(i, f)| format!(" this.{f} = p{i};")).collect();
        let _ = writeln!(out, "    constructor({}) {{{sets} }}", params.join(", "));
        // the copy shares the aggregate fields, so they are marked shared
        let copies: Vec<String> = s
            .fields
            .iter()
            .zip(&fields)
            .map(|((_, t), f)| if Structs::aggregate(*t) { format!("ny_sh(this.{f})") } else { format!("this.{f}") })
            .collect();
        let _ = writeln!(out, "    ny_cp() {{ return new {n}({}); }}", copies.join(", "));
        let eqs: Vec<String> = s
            .fields
            .iter()
            .zip(&fields)
            .map(|((_, t), f)| if Structs::aggregate(*t) { format!("ny_eq(this.{f}, o.{f})") } else { format!("this.{f} === o.{f}") })
            .collect();
        let eq = if eqs.is_empty() { "true".to_string() } else { eqs.join(" && ") };
        let _ = writeln!(out, "    ny_eq(o) {{ return {eq}; }}");
        let parts: Vec<String> =
            s.fields.iter().zip(&fields).map(|((name, t), f)| format!("\"{name}: \" + ny_fmt(this.{f}, \"{}\")", tdesc(*t))).collect();
        let body = if parts.is_empty() { String::new() } else { format!(" + {}", parts.join(" + \", \" + ")) };
        let _ = writeln!(out, "    ny_fmt() {{ return \"{}(\"{body} + \")\"; }}", s.name);
        out.push_str("}\n");
    }
    if !m.structs.0.is_empty() {
        out.push('\n');
    }
}

/// `file` is the source path as given to nyra; runtime errors report it.
pub fn gen(m: &Module, file: &str) -> String {
    let mut out = PRELUDE.replace("@FILE@", &crate::diag::json_str(file));
    out.push('\n');
    out.push_str(STRINGS);
    out.push_str(ARRAYS);
    let std = m.uses_std();
    if std {
        out.push_str(STD);
    }
    let json = m.uses_json();
    if json {
        out.push_str(JSON);
    }
    out.push('\n');
    classes(m, &mut out);
    if json {
        // the fields of each struct for `json.str` and `json.parse`, after every class exists
        for (_, s) in &m.structs.0 {
            let fields: Vec<String> =
                s.fields.iter().map(|(f, t)| format!("[{}, \"{}\", {}]", crate::diag::json_str(f), jfield(f), jdesc(*t))).collect();
            let _ = writeln!(out, "nyS_{}.ny_jf = [{}];", ident(&s.name), fields.join(", "));
        }
        out.push('\n');
    }
    for f in &m.funcs {
        let n = names::locals(f, name, "ny_");
        let _ = writeln!(out, "function {}({}) {{", name(&f.name), n[..f.params].join(", "));
        // every local is declared at the top; statements only assign
        if f.locals.len() > f.params {
            let _ = writeln!(out, "    let {};", n[f.params..].join(", "));
        }
        // an `inout` parameter is a box: the value is `p.v`
        let uses: Vec<String> =
            n.iter().enumerate().map(|(i, x)| if i < f.params && f.locals[i].inout { format!("{x}.v") } else { x.clone() }).collect();
        let mut g = Gen { m, f, names: &uses, out: String::new(), indent: 1, tmp: 0, span: Cell::new(f.span) };
        g.stmts(&f.body);
        out.push_str(&g.out);
        out.push_str("}\n\n");
    }
    // `os.exit(n)` throws `NyExit` (only programs that use the standard library can)
    let exit = if std {
        "    if (e instanceof NyExit) {\n        if (typeof process !== \"undefined\") process.exitCode = e.code;\n    } else {\n"
    } else {
        ""
    };
    let (inner, close) = if std { ("    ", "    }\n") } else { ("", "") };
    let _ = write!(
        out,
        concat!(
            "try {{\n",
            "    {}();\n",
            "}} catch (e) {{\n",
            "{}",
            "{}    console.error(ny_rescue(e).message);\n",
            "{}    // exitCode instead of process.exit(), so buffered stdout is never cut off\n",
            "{}    if (typeof process !== \"undefined\") process.exitCode = 101;\n",
            "{}",
            "}}\n",
        ),
        name(&m.func(m.main).name),
        exit,
        inner,
        inner,
        inner,
        close
    );
    out
}

struct Gen<'a> {
    m: &'a Module,
    f: &'a Func,
    names: &'a [String],
    out: String,
    indent: usize,
    /// Counter for helper names (loop variables, element references).
    tmp: usize,
    /// The position of the statement being generated (an int literal JavaScript cannot hold
    /// exactly stops the program there: E0256).
    span: Cell<Span>,
}

impl Gen<'_> {
    fn line(&mut self, s: &str) {
        for _ in 0..self.indent {
            self.out.push_str("    ");
        }
        self.out.push_str(s);
        self.out.push('\n');
    }

    fn block(&mut self, ss: &[Stmt]) {
        self.indent += 1;
        self.stmts(ss);
        self.indent -= 1;
    }

    fn stmts(&mut self, ss: &[Stmt]) {
        for s in ss {
            self.stmt(s);
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

    /// `dst = value;` or `value;`
    fn assign(&self, dst: Option<LocalId>, value: String) -> String {
        match dst {
            Some(d) => format!("{} = {value};", self.local(d)),
            None => format!("{value};"),
        }
    }

    fn arg(&self, e: &Expr) -> String {
        bare(&self.expr(e)).to_string()
    }

    /// A value that is about to get one more owner: an aggregate is marked shared.
    fn owned(&self, e: &Expr) -> String {
        if Structs::aggregate(self.ty(e)) {
            format!("ny_sh({})", self.arg(e))
        } else {
            self.arg(e)
        }
    }

    fn stmt(&mut self, s: &Stmt) {
        self.span.set(s.span);
        let at = format!("{}, {}", s.span.line, s.span.col);
        match &s.kind {
            StmtKind::Set(l, e) => {
                // A struct with only plain fields is not reference counted, so it never gets a `Dup`:
                // a copy of one marks it shared here, so a write to either copy copies it first.
                let t = self.f.local(*l).ty;
                let plain_struct = matches!(t, Ty::Struct(_)) && !self.m.managed(t);
                let v = if plain_struct { self.owned(e) } else { bare(&self.expr(e)).to_string() };
                let line = format!("{} = {v};", self.local(*l));
                self.line(&line);
            }
            StmtKind::Call { dst, func, args } => {
                // an `inout` argument goes in a box `{v: x}`; the callee's value is written back after
                let inout = args.iter().any(|a| matches!(a, Arg::InOut(_)));
                if !inout {
                    let args: Vec<String> = args
                        .iter()
                        .map(|a| match a {
                            Arg::Val(e) => self.arg(e),
                            Arg::InOut(_) => unreachable!("checked above"),
                        })
                        .collect();
                    let line = self.assign(*dst, format!("{}({})", name(&self.m.func(*func).name), args.join(", ")));
                    self.line(&line);
                    return;
                }
                self.line("{");
                self.indent += 1;
                let mut parts = Vec::with_capacity(args.len());
                let mut boxes = Vec::new();
                for a in args {
                    match a {
                        Arg::Val(e) => parts.push(self.arg(e)),
                        Arg::InOut(p) => {
                            let lv = self.place(p, false);
                            let b = self.fresh("b");
                            self.line(&format!("const {b} = {{v: {lv}}};"));
                            parts.push(b.clone());
                            boxes.push((b, lv));
                        }
                    }
                }
                let call = format!("{}({})", name(&self.m.func(*func).name), parts.join(", "));
                // the result is assigned after the write-back (`x = f(inout x)` keeps the result)
                let r = self.fresh("r");
                self.line(&format!("const {r} = {call};"));
                for (b, lv) in boxes {
                    self.line(&format!("{lv} = {b}.v;"));
                }
                if let Some(d) = dst {
                    let line = format!("{} = {r};", self.local(*d));
                    self.line(&line);
                }
                self.indent -= 1;
                self.line("}");
            }
            StmtKind::Op { dst, op, args } => self.op(*dst, *op, args, &at),
            StmtKind::Store { place, value } => {
                self.line("{");
                self.indent += 1;
                let lv = self.place(place, false);
                let line = format!("{lv} = {};", self.owned(value));
                self.line(&line);
                self.indent -= 1;
                self.line("}");
            }
            StmtKind::Mutate { dst, op, place, args } => {
                self.line("{");
                self.indent += 1;
                // a string is replaced, an array is changed where it is (after copying it if shared)
                let target = self.place(place, *op != RtOp::StrAppend);
                let a: Vec<String> = args.iter().map(|x| self.arg(x)).collect();
                let code = match op {
                    RtOp::StrAppend => format!("{target} += {}", a[0]),
                    RtOp::ArrPush => format!("{target}.push({})", self.owned(&args[0])),
                    RtOp::ArrPop => format!("ny_pop({target}, {at})"),
                    RtOp::ArrInsert => format!("ny_insert({target}, {}, {}, {at})", a[0], self.owned(&args[1])),
                    RtOp::ArrRemove => format!("ny_remove({target}, {}, {at})", a[0]),
                    RtOp::ArrSort => {
                        let elem = self.place_ty(place).elem().expect("verified: an array");
                        let lt = match elem {
                            Ty::Str => "ny_lt_str",
                            Ty::Float => "ny_lt_float",
                            _ => "ny_lt_num",
                        };
                        format!("ny_sort({target}, {lt})")
                    }
                    RtOp::ArrSortBy => {
                        let lt = match self.ty(&args[0]).elem() {
                            Some(Ty::Str) => "ny_lt_str",
                            Some(Ty::Float) => "ny_lt_float",
                            _ => "ny_lt_num",
                        };
                        format!("ny_sort_by({target}, {}, {lt})", a[0])
                    }
                    RtOp::ArrReverse => format!("{target}.reverse()"),
                    RtOp::ArrSwap => format!("ny_swap({target}, {}, {}, {at})", a[0], a[1]),
                    RtOp::ArrAppend => format!("ny_append({target}, {})", a[0]),
                    RtOp::MapSet => format!("{target}.set({}, {})", a[0], self.owned(&args[1])),
                    RtOp::MapRemove => format!("{target}.delete({})", a[0]),
                    other => unreachable!("{} does not change a place", other.name()),
                };
                let line = self.assign(*dst, code);
                self.line(&line);
                self.indent -= 1;
                self.line("}");
            }
            StmtKind::If { .. } => self.if_chain(s),
            StmtKind::Loop { head, cond, body, step } => self.lp(head, cond, body, step),
            StmtKind::ForEach { var, iter, body } => {
                // `for...of` walks a string by code points (like Nyra) and an array by elements
                let c = self.fresh("c");
                let line = format!("for (const {c} of {}) {{", self.arg(iter));
                self.line(&line);
                self.indent += 1;
                let value = if self.ty(iter) == Ty::Str { format!("{c}.codePointAt(0)") } else { c };
                let line = format!("{} = {value};", self.local(*var));
                self.line(&line);
                self.stmts(body);
                self.indent -= 1;
                self.line("}");
            }
            StmtKind::Break => self.line("break;"),
            StmtKind::Continue => self.line("continue;"),
            StmtKind::Return(None) => self.line("return;"),
            StmtKind::Return(Some(e)) => {
                let line = format!("return {};", bare(&self.expr(e)));
                self.line(&line);
            }
            // the garbage collector frees memory; a second owner of an aggregate marks it shared
            StmtKind::Dup(l) => {
                if Structs::aggregate(self.f.local(*l).ty) {
                    let line = format!("ny_sh({});", self.local(*l));
                    self.line(&line);
                }
            }
            StmtKind::Drop(_) | StmtKind::Keep(_) => {}
            StmtKind::Free(l) => {
                let line = format!("{} = undefined;", self.local(*l));
                self.line(&line);
            }
        }
    }

    /// Emits the copy-on-write steps for a place. Returns an assignable expression for it, or,
    /// with `unique`, the place's own array, unique so it can be changed.
    fn place(&mut self, p: &Place, unique: bool) -> String {
        let mut lv = self.local(p.root).to_string();
        let mut t = self.f.local(p.root).ty;
        if p.path.is_empty() {
            if unique && Structs::aggregate(t) {
                self.line(&format!("if ({lv}.ny_s) {lv} = ny_cp({lv});"));
            }
            return lv;
        }
        // the root changes below this point
        self.line(&format!("if ({lv}.ny_s) {lv} = ny_cp({lv});"));
        let n = p.path.len();
        for (k, step) in p.path.iter().enumerate() {
            let key = match step {
                Step::Key(key, span) => {
                    // `m[k]`: the value under the key, unique (a missing key is E0248)
                    let (kt, vt) = t.map_kv().expect("verified: a map");
                    t = vt;
                    let r = self.fresh("p");
                    self.line(&format!("const {r} = ny_mu({lv}, {}, \"{}\", {}, {});", self.arg(key), tdesc(kt), span.line, span.col));
                    lv = r;
                    continue;
                }
                Step::Index(i, span) => {
                    t = t.elem().expect("verified: an array");
                    format!("ny_ck({lv}, {}, {}, {})", self.arg(i), span.line, span.col)
                }
                Step::Field(fi) => {
                    let info = self.m.structs.get(t).expect("verified: a struct");
                    let key = format!("\"{}\"", jfield(&info.fields[*fi as usize].0));
                    t = info.fields[*fi as usize].1;
                    key
                }
            };
            if k + 1 == n && !unique {
                return format!("{lv}[{key}]");
            }
            let r = self.fresh("p");
            self.line(&format!("const {r} = ny_u({lv}, {key});"));
            lv = r;
        }
        let _ = t;
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
            RtOp::Print => self.print(args),
            RtOp::PrintNoLine => format!("ny_write({})", self.template(args)),
            RtOp::Format => self.template(args),
            RtOp::DivInt => format!("ny_div({}, {}, {at})", a[0], a[1]),
            RtOp::AddInt => format!("ny_add({}, {}, {at})", a[0], a[1]),
            RtOp::SubInt => format!("ny_sub({}, {}, {at})", a[0], a[1]),
            RtOp::MulInt => format!("ny_mul({}, {}, {at})", a[0], a[1]),
            RtOp::NegInt => format!("ny_neg({}, {at})", a[0]),
            RtOp::RemInt => format!("ny_mod({}, {}, {at})", a[0], a[1]),
            RtOp::FloatToInt => format!("ny_f2i({}, {at})", a[0]),
            RtOp::StrConcat => format!("{} + {}", self.expr(&args[0]), self.expr(&args[1])),
            RtOp::StrAt => format!("ny_str_at({}, {}, {at})", a[0], a[1]),
            RtOp::StrSlice => format!("ny_str_slice({}, {}, {}, {at})", a[0], a[1], a[2]),
            RtOp::StrReplace => format!("ny_str_replace({}, {}, {}, {at})", a[0], a[1], a[2]),
            RtOp::StrTrim => format!("ny_str_trim({})", a[0]),
            RtOp::StrUpper => format!("ny_str_upper({})", a[0]),
            RtOp::StrLower => format!("ny_str_lower({})", a[0]),
            RtOp::StrRepeat => format!("ny_str_repeat({}, {}, {at})", a[0], a[1]),
            RtOp::StrToInt => format!("ny_str_to_int({}, {at})", a[0]),
            RtOp::StrToFloat => format!("ny_str_to_float({}, {at})", a[0]),
            RtOp::CharFrom => format!("ny_char_from({}, {at})", a[0]),
            RtOp::StrChars | RtOp::StrCodes => format!("ny_chars({})", a[0]),
            RtOp::StrSplit => format!("ny_split({}, {}, {at})", a[0], a[1]),
            RtOp::CheckStep => format!("ny_check_step({}, {at})", a[0]),
            RtOp::CheckNonEmpty => format!("ny_check_non_empty({}, {}, {at})", a[0], a[1]),
            RtOp::StrPadLeft => format!("ny_pad({}, {}, {}, true)", a[0], a[1], a[2]),
            RtOp::StrPadRight => format!("ny_pad({}, {}, {}, false)", a[0], a[1], a[2]),
            RtOp::ArrNew => {
                let items: Vec<String> = args.iter().map(|x| self.owned(x)).collect();
                format!("[{}]", items.join(", "))
            }
            RtOp::StructNew => {
                let t = self.f.local(dst.expect("verified: a destination")).ty;
                let fields: Vec<String> = args.iter().map(|x| self.owned(x)).collect();
                format!("new nyS_{}({})", ident(&t.struct_name().expect("a struct")), fields.join(", "))
            }
            RtOp::ArrGet => {
                // an element that is a plain struct now has two owners (no `Dup` follows for it)
                let elem = self.ty(&args[0]).elem().expect("verified: an array");
                if matches!(elem, Ty::Struct(_)) && !self.m.managed(elem) {
                    format!("ny_sh(ny_get({}, {}, {at}))", a[0], a[1])
                } else {
                    format!("ny_get({}, {}, {at})", a[0], a[1])
                }
            }
            RtOp::ArrSlice => format!("ny_aslice({}, {}, {}, {at})", a[0], a[1], a[2]),
            RtOp::ArrRepeat => format!("ny_arep({}, {}, {at})", a[0], a[1]),
            RtOp::ArrConcat => format!("ny_aconcat({}, {})", a[0], a[1]),
            RtOp::ArrJoin => {
                if self.ty(&args[0]).elem() == Some(Ty::Char) {
                    format!("ny_join_char({}, {})", a[0], a[1])
                } else {
                    format!("{}.join({})", self.expr(&args[0]), a[1])
                }
            }
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
            RtOp::MapNew => {
                let items: Vec<String> = args.iter().map(|x| self.owned(x)).collect();
                format!("ny_mnew([{}])", items.join(", "))
            }
            RtOp::MapGet | RtOp::MapGetOr => {
                let (k, v) = self.ty(&args[0]).map_kv().expect("verified: a map");
                let get = if op == RtOp::MapGet {
                    format!("ny_mget({}, {}, \"{}\", {at})", a[0], a[1], tdesc(k))
                } else {
                    format!("ny_mgetor({}, {}, {})", a[0], a[1], a[2])
                };
                // a value that is a plain struct now has two owners (no `Dup` follows for it)
                if matches!(v, Ty::Struct(_)) && !self.m.managed(v) {
                    format!("ny_sh({get})")
                } else {
                    get
                }
            }
            RtOp::MapKeys => format!("[...{}.keys()]", self.expr(&args[0])),
            RtOp::MapValues => format!("ny_shall([...{}.values()])", self.expr(&args[0])),
            other => unreachable!("{} is a `Mutate`", other.name()),
        };
        let line = self.assign(dst, code);
        self.line(&line);
    }

    fn if_chain(&mut self, s: &Stmt) {
        let mut cur = s;
        let mut head = "if";
        loop {
            let StmtKind::If { cond, then, els } = &cur.kind else { unreachable!() };
            self.span.set(cur.span);
            let line = format!("{head} ({}) {{", bare(&self.expr(cond)));
            self.line(&line);
            self.block(then);
            match els.as_slice() {
                [] => break,
                [next] if matches!(next.kind, StmtKind::If { .. }) => {
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

    fn lp(&mut self, head: &[Stmt], cond: &Expr, body: &[Stmt], step: &[Stmt]) {
        let full = self.expr(cond);
        let c = bare(&full).to_string();
        // the step goes in the `for` header, so `continue` runs it
        let steps: Vec<String> = step
            .iter()
            .map(|s| match &s.kind {
                StmtKind::Set(l, e) => format!("{} = {}", self.local(*l), bare(&self.expr(e))),
                _ => unreachable!("a loop step only assigns"),
            })
            .collect();
        let step = steps.join(", ");
        match (head.is_empty(), step.is_empty()) {
            (true, true) => {
                self.line(&format!("while ({c}) {{"));
                self.block(body);
            }
            (true, false) => {
                self.line(&format!("for (; {c}; {step}) {{"));
                self.block(body);
            }
            (false, _) => {
                self.line(&format!("for (;; {step}) {{"));
                self.indent += 1;
                self.stmts(head);
                self.line(&format!("if (!{full}) break;"));
                self.stmts(body);
                self.indent -= 1;
            }
        }
        self.line("}");
    }

    /// The text of one value as `print` shows it (numbers through `String()`, so `-0` is `0`).
    fn shown(&self, p: &Expr) -> String {
        let x = self.arg(p);
        match self.ty(p) {
            Ty::Int | Ty::Float => format!("String({x})"),
            Ty::Char => format!("ny_char_str({x})"),
            Ty::Bool | Ty::Str => x,
            t => format!("ny_fmt({x}, \"{}\")", tdesc(t)),
        }
    }

    fn print(&self, parts: &[Expr]) -> String {
        if let [p] = parts {
            return format!("console.log({})", self.shown(p));
        }
        format!("console.log({})", self.template(parts))
    }

    /// A template literal for string parts: text is escaped, values become `${...}`.
    fn template(&self, parts: &[Expr]) -> String {
        let mut s = String::from("`");
        for p in parts {
            if let Expr::Str(id) = p {
                for c in self.m.str(*id).chars() {
                    match c {
                        '\\' => s.push_str("\\\\"),
                        '`' => s.push_str("\\`"),
                        '$' => s.push_str("\\$"),
                        '\n' => s.push_str("\\n"),
                        '\r' => s.push_str("\\r"),
                        '\t' => s.push_str("\\t"),
                        c if (c as u32) < 0x20 => s.push_str(&format!("\\u{:04x}", c as u32)),
                        c => s.push(c),
                    }
                }
                continue;
            }
            s.push_str("${");
            match self.ty(p) {
                Ty::Int | Ty::Float | Ty::Bool | Ty::Str => s.push_str(&self.arg(p)),
                _ => s.push_str(&self.shown(p)),
            }
            s.push('}');
        }
        s.push('`');
        s
    }

    fn expr(&self, e: &Expr) -> String {
        match e {
            // every int is a safe integer here: a bigger literal would be rounded
            Expr::Int(n) if n.unsigned_abs() > SAFE_INT => {
                let at = self.span.get();
                format!("ny_unsafe_int(\"{n}\", {}, {})", at.line, at.col)
            }
            Expr::Int(n) => n.to_string(),
            Expr::Float(f) if f.is_infinite() => (if *f > 0.0 { "Infinity" } else { "(-Infinity)" }).to_string(),
            Expr::Float(f) => format!("{f:?}"),
            Expr::Bool(b) => b.to_string(),
            Expr::Char(c) => c.to_string(),
            Expr::Str(id) => crate::diag::json_str(self.m.str(*id)),
            Expr::Local(l) => self.local(*l).to_string(),
            Expr::Unary(op, x) => {
                let x = self.expr(x);
                // never print `--x` (that would be a decrement)
                let neg = if x.starts_with('-') { format!("-({x})") } else { format!("-{x}") };
                match op {
                    // `+ 0` turns an int -0 into 0 (C has no -0 for ints)
                    UnOp::INeg => format!("({neg} + 0)"),
                    UnOp::FNeg => format!("({neg})"),
                    UnOp::Not => format!("(!{x})"),
                }
            }
            Expr::Binary(op, a, b) => {
                let (a, b) = (self.expr(a), self.expr(b));
                match op {
                    BinOp::IMul => format!("({a} * {b} + 0)"),
                    BinOp::IDiv => format!("(Math.trunc({a} / {b}) + 0)"),
                    BinOp::IRem => format!("({a} % {b} + 0)"),
                    BinOp::IEq | BinOp::FEq | BinOp::BEq | BinOp::CEq | BinOp::SEq => format!("({a} === {b})"),
                    BinOp::INe | BinOp::FNe | BinOp::BNe | BinOp::CNe | BinOp::SNe => format!("({a} !== {b})"),
                    // code point order (JavaScript's `<` compares UTF-16 units)
                    BinOp::SLt | BinOp::SLe | BinOp::SGt | BinOp::SGe => {
                        format!("(ny_str_cmp({}, {}) {} 0)", bare(&a), bare(&b), op.symbol())
                    }
                    BinOp::DeepEq => format!("ny_eq({}, {})", bare(&a), bare(&b)),
                    BinOp::DeepNe => format!("(!ny_eq({}, {}))", bare(&a), bare(&b)),
                    _ => format!("({a} {} {b})", op.symbol()),
                }
            }
            Expr::Select(c, a, b) => format!("({} ? {} : {})", self.expr(c), self.expr(a), self.expr(b)),
            Expr::IntToFloat(x) => self.expr(x),
            Expr::Field(x, k, _) => {
                let info = self.m.structs.get(x.ty(self.f)).expect("verified: a struct");
                format!("{}.{}", self.expr(x), jfield(&info.fields[*k as usize].0))
            }
            Expr::Pure(p, args) => {
                let a: Vec<String> = args.iter().map(|x| self.arg(x)).collect();
                match p {
                    PureFn::StrLen => format!("ny_len({})", a[0]),
                    PureFn::StrContains => format!("{}.includes({})", self.expr(&args[0]), a[1]),
                    PureFn::StrStartsWith => format!("{}.startsWith({})", self.expr(&args[0]), a[1]),
                    PureFn::StrEndsWith => format!("{}.endsWith({})", self.expr(&args[0]), a[1]),
                    PureFn::StrIndexOf => format!("ny_str_index_of({}, {})", a[0], a[1]),
                    PureFn::CharCode => self.expr(&args[0]),
                    PureFn::CharUpper => format!("ny_char_upper({})", a[0]),
                    PureFn::CharLower => format!("ny_char_lower({})", a[0]),
                    PureFn::CharIsDigit => format!("ny_char_is_digit({})", a[0]),
                    PureFn::CharIsLetter => format!("ny_char_is_letter({})", a[0]),
                    PureFn::CharIsUpper => format!("ny_char_is_upper({})", a[0]),
                    PureFn::CharIsLower => format!("ny_char_is_lower({})", a[0]),
                    PureFn::CharIsSpace => format!("ny_is_space({})", a[0]),
                    PureFn::MapLen => format!("{}.size", self.expr(&args[0])),
                    PureFn::MapHas => format!("{}.has({})", self.expr(&args[0]), a[1]),
                    PureFn::ArrLen => format!("{}.length", self.expr(&args[0])),
                    PureFn::ArrContains => format!("(ny_aindex({}, {}) >= 0)", a[0], a[1]),
                    PureFn::ArrIndexOf => format!("ny_aindex({}, {})", a[0], a[1]),
                }
            }
        }
    }
}
