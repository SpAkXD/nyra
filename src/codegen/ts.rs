//! TypeScript backend. The output runs on Node.js 22.6+ (which strips the types) or through `tsc`.
//!
//! It behaves exactly like the JavaScript backend (ints are numbers, a char is its code point,
//! arrays and structs are copied on write through a shared mark), but reads like TypeScript a
//! person would write: typed signatures and classes, variables declared where they are first
//! needed, counted loops as `for (let i = a; i < b; i++)`, and the runtime at the end of the file.

use std::cell::Cell;
use std::fmt::Write;

use super::scope::{self, range_for, Info};
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
    "Ref",
    "type",
    "declare",
    "namespace",
    "abstract",
    "as",
    "any",
    "boolean",
    "number",
    "string",
    "never",
    "unknown",
    "readonly",
    "keyof",
    "infer",
    "is",
    "asserts",
    "get",
    "set",
    "of",
    "constructor",
    "Boolean",
    "Map",
    "Set",
    "Date",
    "NyExit",
    "TextDecoder",
    "Atomics",
    "SharedArrayBuffer",
    "Int32Array",
    "Uint32Array",
    "Uint8Array",
    "ArrayBuffer",
    "DataView",
    "performance",
    "crypto",
];

/// The TypeScript runtime, emitted after the program (`@FILE@` becomes the source path).
const RUNTIME: &str = include_str!("../rt/ts/runtime.ts");
const STD: &str = include_str!("../rt/ts/std.ts");
const JSON: &str = include_str!("../rt/ts/json.ts");

/// A Nyra name as a TypeScript identifier. `ny...` names belong to the runtime (helpers `ny_*`),
/// so user names that start with `ny` get a `_`, like reserved words.
fn name(n: &str) -> String {
    let id = names::ascii(n);
    if RESERVED.contains(&n) || n.starts_with("ny") || n.starts_with("Ny") {
        format!("{id}_")
    } else {
        id
    }
}

/// The TypeScript name of a field: names the runtime uses on objects get a `_`.
fn field_name(n: &str) -> String {
    if n.starts_with("ny") || matches!(n, "constructor" | "prototype" | "__proto__") {
        format!("{}_", names::ascii(n))
    } else {
        names::ascii(n)
    }
}

fn tstype(t: Ty) -> String {
    match t {
        Ty::Int | Ty::Float | Ty::Char => "number".into(),
        Ty::Bool => "boolean".into(),
        Ty::Str => "string".into(),
        Ty::Array(_) => format!("{}[]", tstype(t.elem().expect("an array"))),
        Ty::Map(_) => {
            let (k, v) = t.map_kv().expect("a map");
            format!("Map<{}, {}>", tstype(k), tstype(v))
        }
        Ty::Struct(_) => name(&t.struct_name().expect("a struct")),
        other => unreachable!("the TypeScript backend got the type `{}`", other.name()),
    }
}

/// The type descriptor `ny_fmt` prints a value with (a char is a number here).
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
        _ => name(&t.struct_name().expect("a struct")),
    }
}

/// One class per struct: the fields, copying one level (copy on write), deep equality, printing.
fn classes(m: &Module, out: &mut String) {
    for (_, s) in &m.structs.0 {
        let n = name(&s.name);
        let fields: Vec<String> = s.fields.iter().map(|(f, _)| field_name(f)).collect();
        let _ = writeln!(out, "class {n} {{");
        for ((_, t), f) in s.fields.iter().zip(&fields) {
            let _ = writeln!(out, "    {f}: {};", tstype(*t));
        }
        // a parameter named like a reserved word (`class`) is spelled by position
        let params: Vec<String> =
            fields.iter().enumerate().map(|(i, f)| if RESERVED.contains(&f.as_str()) { format!("p{i}") } else { f.clone() }).collect();
        let typed: Vec<String> = params.iter().zip(&s.fields).map(|(p, (_, t))| format!("{p}: {}", tstype(*t))).collect();
        if s.fields.is_empty() {
            out.push_str("    constructor() {}\n");
        } else {
            let _ = writeln!(out, "    constructor({}) {{", typed.join(", "));
            for (f, p) in fields.iter().zip(&params) {
                let _ = writeln!(out, "        this.{f} = {p};");
            }
            out.push_str("    }\n");
        }
        // the copy shares the aggregate fields, so they are marked shared
        let copies: Vec<String> = s
            .fields
            .iter()
            .zip(&fields)
            .map(|((_, t), f)| if Structs::aggregate(*t) { format!("ny_share(this.{f})") } else { format!("this.{f}") })
            .collect();
        let _ = writeln!(out, "    ny_cp(): {n} {{\n        return new {n}({});\n    }}", copies.join(", "));
        let eqs: Vec<String> = s
            .fields
            .iter()
            .zip(&fields)
            .map(|((_, t), f)| if Structs::aggregate(*t) { format!("ny_eq(this.{f}, o.{f})") } else { format!("this.{f} === o.{f}") })
            .collect();
        let eq = if eqs.is_empty() { "true".to_string() } else { eqs.join(" && ") };
        let _ = writeln!(out, "    ny_eq(o: {n}): boolean {{\n        return {eq};\n    }}");
        let parts: Vec<String> = s
            .fields
            .iter()
            .zip(&fields)
            .enumerate()
            .map(|(k, ((fname, t), f))| {
                let sep = if k > 0 { ", " } else { "" };
                format!("{sep}{fname}: ${{ny_fmt(this.{f}, \"{}\")}}", tdesc(*t))
            })
            .collect();
        let _ = writeln!(out, "    ny_fmt(): string {{\n        return `{}({})`;\n    }}", template_text(&s.name), parts.concat());
        out.push_str("}\n\n");
    }
}

/// Text inside a template literal.
fn template_text(text: &str) -> String {
    let mut s = String::new();
    for c in text.chars() {
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
    s
}

/// `file` is the source path as given to nyra; runtime errors report it.
pub fn gen(m: &Module, file: &str) -> String {
    let mut out = String::from("// generated by nyra: TypeScript (run with `node file.ts` on Node.js 22.6+, or compile with tsc)\n\n");
    classes(m, &mut out);
    let json = m.uses_json();
    if json {
        // the fields of each struct for `json.str` and `json.parse`, after every class exists
        for (_, s) in &m.structs.0 {
            let fields: Vec<String> = s
                .fields
                .iter()
                .map(|(f, t)| format!("[{}, \"{}\", {}]", crate::diag::json_str(f), field_name(f), jdesc(*t)))
                .collect();
            let _ = writeln!(out, "({} as any).ny_jf = [{}];", name(&s.name), fields.join(", "));
        }
        out.push('\n');
    }
    for f in &m.funcs {
        let info = Info::new(f);
        let n = names::scoped(f, name, "ny_", &info.loop_var);
        let params: Vec<String> = (0..f.params)
            .map(|i| {
                let t = tstype(f.locals[i].ty);
                if f.locals[i].inout {
                    format!("{}: Ref<{t}>", n[i])
                } else {
                    format!("{}: {t}", n[i])
                }
            })
            .collect();
        let ret = f.ret.map_or("void".to_string(), tstype);
        let _ = writeln!(out, "function {}({}): {ret} {{", name(&f.name), params.join(", "));
        // an `inout` parameter is a box: the value is `p.v`
        let uses: Vec<String> =
            n.iter().enumerate().map(|(i, x)| if i < f.params && f.locals[i].inout { format!("{x}.v") } else { x.clone() }).collect();
        let mut g = Gen { m, f, info: &info, names: &uses, out: String::new(), indent: 1, tmp: 0, span: Cell::new(f.span) };
        g.stmts(&f.body);
        out.push_str(&g.out);
        out.push_str("}\n\n");
    }
    out.push_str(&RUNTIME.replace("@FILE@", &crate::diag::json_str(file)));
    let std = m.uses_std();
    if std {
        out.push_str(STD);
    }
    if json {
        out.push_str(JSON);
    }
    // `os.exit(n)` throws `NyExit` (only programs that use the standard library can)
    let exit = if std {
        "    if (e instanceof NyExit) {\n        if (ny_process !== undefined) ny_process.exitCode = e.code;\n    } else {\n"
    } else {
        ""
    };
    let (inner, close) = if std { ("    ", "    }\n") } else { ("", "") };
    let _ = write!(
        out,
        concat!(
            "\n// ---- entry point ----\n",
            "try {{\n",
            "    {}();\n",
            "}} catch (e) {{\n",
            "{}",
            "{}    console.error(ny_rescue(e).message);\n",
            "{}    // exitCode instead of process.exit(), so buffered stdout is never cut off\n",
            "{}    if (ny_process !== undefined) ny_process.exitCode = 101;\n",
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
    info: &'a Info,
    names: &'a [String],
    out: String,
    indent: usize,
    /// Counter for helper names (boxes, element references).
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
        let mut k = 0;
        while k < ss.len() {
            self.span.set(ss[k].span);
            self.declare_before(&ss[k]);
            if let Some(r) = range_for(ss, k, self.info) {
                if let Some(p) = r.pre {
                    self.stmt(p);
                }
                let i = self.local(r.var).to_string();
                let line = format!("for (let {i} = {}; {i} < {}; {i}++) {{", self.arg(r.start), self.arg(r.end));
                self.line(&line);
                self.block(r.body);
                self.line("}");
                k += r.len;
                continue;
            }
            // `x = v` and then `dup x`: one statement that marks the value shared
            if let (StmtKind::Set(l, e), Some(Stmt { kind: StmtKind::Dup(d), .. })) = (&ss[k].kind, ss.get(k + 1)) {
                if l == d && Structs::aggregate(self.f.local(*l).ty) {
                    let v = format!("ny_share({})", self.arg(e));
                    self.set(&ss[k], *l, v);
                    k += 2;
                    continue;
                }
            }
            self.stmt(&ss[k]);
            k += 1;
        }
    }

    /// `let x: T;` for the locals that are first assigned inside blocks below this statement.
    fn declare_before(&mut self, s: &Stmt) {
        for &l in self.info.before(s) {
            let line = format!("let {}: {};", self.local(l), tstype(self.f.local(l).ty));
            self.line(&line);
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

    /// `x = value;`, or its declaration `const x = value;` when `s` declares `x`.
    fn set(&mut self, s: &Stmt, l: LocalId, value: String) {
        let i = l.0 as usize;
        let line = if self.info.declares(s) == Some(l) {
            let kind = if self.info.writes[i] == 1 && !self.info.changed[i] { "const" } else { "let" };
            // an empty array literal needs its type
            let ty = if value == "[]" { format!(": {}", tstype(self.f.local(l).ty)) } else { String::new() };
            format!("{kind} {}{ty} = {value};", self.local(l))
        } else {
            format!("{} = {value};", self.local(l))
        };
        self.line(&line);
    }

    /// `dst = value;` or `value;`
    fn assign(&mut self, s: &Stmt, dst: Option<LocalId>, value: String) {
        match dst {
            Some(d) => self.set(s, d, value),
            None => self.line(&format!("{value};")),
        }
    }

    fn arg(&self, e: &Expr) -> String {
        bare(&self.expr(e)).to_string()
    }

    /// A value that is about to get one more owner: an aggregate is marked shared.
    fn owned(&self, e: &Expr) -> String {
        if Structs::aggregate(self.ty(e)) {
            format!("ny_share({})", self.arg(e))
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
                let v = if plain_struct { self.owned(e) } else { self.arg(e) };
                self.set(s, *l, v);
            }
            StmtKind::Call { dst, func, args } => {
                let callee = name(&self.m.func(*func).name);
                // an `inout` argument goes in a box `{ v: x }`; the callee's value is written back after
                let mut parts = Vec::with_capacity(args.len());
                let mut boxes = Vec::new();
                for a in args {
                    match a {
                        Arg::Val(e) => parts.push(self.arg(e)),
                        Arg::InOut(p) => {
                            let lv = self.place(p, false);
                            let b = self.fresh("b");
                            self.line(&format!("const {b} = {{ v: {lv} }};"));
                            parts.push(b.clone());
                            boxes.push((b, lv, p.root));
                        }
                    }
                }
                let call = format!("{callee}({})", parts.join(", "));
                // the result is assigned after the write-back when it goes to an `inout` variable
                let late = matches!(dst, Some(d) if boxes.iter().any(|(_, _, r)| r == d));
                if late {
                    let r = self.fresh("r");
                    self.line(&format!("const {r} = {call};"));
                    for (b, lv, _) in &boxes {
                        self.line(&format!("{lv} = {b}.v;"));
                    }
                    self.assign(s, *dst, r);
                } else {
                    self.assign(s, *dst, call);
                    for (b, lv, _) in &boxes {
                        self.line(&format!("{lv} = {b}.v;"));
                    }
                }
            }
            StmtKind::Op { dst, op, args } => self.op(s, *dst, *op, args, &at),
            StmtKind::Store { place, value } => {
                let lv = self.place(place, false);
                let line = format!("{lv} = {};", self.owned(value));
                self.line(&line);
            }
            StmtKind::Mutate { dst, op, place, args } => {
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
                    RtOp::ArrAppend => format!("ny_append({target}, {})", a[0]),
                    RtOp::MapSet => format!("{target}.set({}, {})", a[0], self.owned(&args[1])),
                    RtOp::MapRemove => format!("{target}.delete({})", a[0]),
                    RtOp::ArrSwap => format!("ny_swap({target}, {}, {}, {at})", a[0], a[1]),
                    other => unreachable!("{} does not change a place", other.name()),
                };
                self.assign(s, *dst, code);
            }
            StmtKind::If { .. } => self.if_chain(s),
            StmtKind::Loop { head, cond, body, step } => self.lp(head, cond, body, step),
            StmtKind::ForEach { var, iter, body } => {
                // a string gives its characters (code points), an array its elements
                let it = if self.ty(iter) == Ty::Str { format!("ny_chars({})", self.arg(iter)) } else { self.arg(iter) };
                let line = format!("for (const {} of {it}) {{", self.local(*var));
                self.line(&line);
                self.block(body);
                self.line("}");
            }
            StmtKind::Break => self.line("break;"),
            StmtKind::Continue => self.line("continue;"),
            StmtKind::Return(None) => self.line("return;"),
            StmtKind::Return(Some(e)) => {
                let line = format!("return {};", self.arg(e));
                self.line(&line);
            }
            // the garbage collector frees memory; a second owner of an aggregate marks it shared
            StmtKind::Dup(l) => {
                if Structs::aggregate(self.f.local(*l).ty) {
                    let line = format!("ny_share({});", self.local(*l));
                    self.line(&line);
                }
            }
            StmtKind::Drop(_) | StmtKind::Keep(_) => {}
            StmtKind::Free(l) => {
                let line = format!("{} = undefined as any; // free", self.local(*l));
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
                self.line(&format!("{lv} = ny_unique({lv});"));
            }
            return lv;
        }
        // the root changes below this point
        self.line(&format!("{lv} = ny_unique({lv});"));
        let n = p.path.len();
        for (k, step) in p.path.iter().enumerate() {
            let last = k + 1 == n && !unique;
            match step {
                Step::Index(i, span) => {
                    t = t.elem().expect("verified: an array");
                    let key = format!("ny_ck({lv}, {}, {}, {})", self.arg(i), span.line, span.col);
                    if last {
                        return format!("{lv}[{key}]");
                    }
                    let r = self.fresh("p");
                    self.line(&format!("const {r} = ny_unique_in({lv}, {key});"));
                    lv = r;
                }
                Step::Field(fi) => {
                    let info = self.m.structs.get(t).expect("verified: a struct");
                    let f = field_name(&info.fields[*fi as usize].0);
                    t = info.fields[*fi as usize].1;
                    if last {
                        return format!("{lv}.{f}");
                    }
                    let r = self.fresh("p");
                    self.line(&format!("const {r} = ny_unique_in({lv}, \"{f}\");"));
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
                Step::Field(k) => self.m.structs.get(t).expect("verified: a struct").fields[*k as usize].1,
            };
        }
        t
    }

    fn op(&mut self, s: &Stmt, dst: Option<LocalId>, op: RtOp, args: &[Expr], at: &str) {
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
                format!("new {}({})", name(&t.struct_name().expect("a struct")), fields.join(", "))
            }
            RtOp::ArrGet => {
                // an element that is a plain struct now has two owners (no `Dup` follows for it)
                let elem = self.ty(&args[0]).elem().expect("verified: an array");
                if matches!(elem, Ty::Struct(_)) && !self.m.managed(elem) {
                    format!("ny_share(ny_get({}, {}, {at}))", a[0], a[1])
                } else {
                    format!("ny_get({}, {}, {at})", a[0], a[1])
                }
            }
            RtOp::ArrSlice => format!("ny_slice({}, {}, {}, {at})", a[0], a[1], a[2]),
            RtOp::ArrRepeat => format!("ny_repeat({}, {}, {at})", a[0], a[1]),
            RtOp::ArrConcat => format!("ny_concat({}, {})", a[0], a[1]),
            RtOp::ArrJoin => {
                if self.ty(&args[0]).elem() == Some(Ty::Char) {
                    format!("ny_join_chars({}, {})", a[0], a[1])
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
                    format!("ny_share({get})")
                } else {
                    get
                }
            }
            RtOp::MapKeys => format!("[...{}.keys()]", self.expr(&args[0])),
            RtOp::MapValues => format!("ny_share_all([...{}.values()])", self.expr(&args[0])),
            other => unreachable!("{} is a `Mutate`", other.name()),
        };
        self.assign(s, dst, code);
    }

    fn if_chain(&mut self, s: &Stmt) {
        let mut cur = s;
        let mut head = "if";
        loop {
            let StmtKind::If { cond, then, els } = &cur.kind else { unreachable!() };
            self.span.set(cur.span);
            let line = format!("{head} ({}) {{", self.arg(cond));
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

    fn lp(&mut self, head: &[Stmt], cond: &Expr, body: &[Stmt], step: &[Stmt]) {
        let full = self.expr(cond);
        let c = bare(&full).to_string();
        // the step goes in the `for` header, so `continue` runs it
        let steps: Vec<String> = step
            .iter()
            .map(|s| match &s.kind {
                StmtKind::Set(l, e) => format!("{} = {}", self.local(*l), self.arg(e)),
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
                s.push_str(&template_text(self.m.str(*id)));
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
                format!("{}.{}", self.expr(x), field_name(&info.fields[*k as usize].0))
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
                    PureFn::ArrContains => format!("(ny_index_of({}, {}) >= 0)", a[0], a[1]),
                    PureFn::ArrIndexOf => format!("ny_index_of({}, {})", a[0], a[1]),
                }
            }
        }
    }
}
