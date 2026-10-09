//! Rust backend. The output is one file for `rustc` (2021 edition, no crates).
//!
//! - `int` is `i64` and wraps: the program is built with overflow checks off (`nyra run` does that;
//!   the file's first line says how to build it by hand);
//! - `str` is `Rc<String>` and `[T]` is `Rc<Vec<T>>`: assigning one is cheap, and a write goes
//!   through `Rc::make_mut`, which copies a shared value first (copy on write, like the C backend's
//!   reference counts); structs derive `Clone` and `PartialEq` (plain ones are `Copy`);
//! - parameters are passed by value (a cheap clone); an `inout` parameter is `&mut T`;
//! - variables are declared where they are first needed, with their type.

use std::fmt::Write;

use super::names;
use super::scope::{self, mentions, range_for, Info};
use crate::ir::{Arg, BinOp, Expr, Func, LocalId, Module, Place, PureFn, RtOp, Step, Stmt, StmtKind, Ty, UnOp};

const RESERVED: &[&str] = &[
    "as",
    "break",
    "const",
    "continue",
    "crate",
    "else",
    "enum",
    "extern",
    "false",
    "fn",
    "for",
    "if",
    "impl",
    "in",
    "let",
    "loop",
    "match",
    "mod",
    "move",
    "mut",
    "pub",
    "ref",
    "return",
    "self",
    "Self",
    "static",
    "struct",
    "super",
    "trait",
    "true",
    "type",
    "unsafe",
    "use",
    "where",
    "while",
    "async",
    "await",
    "dyn",
    "abstract",
    "become",
    "box",
    "do",
    "final",
    "macro",
    "override",
    "priv",
    "typeof",
    "unsized",
    "virtual",
    "yield",
    "try",
    "union",
    "gen",
    "Rc",
    "Vec",
    "String",
    "Str",
    "Option",
    "Some",
    "None",
    "Ok",
    "Err",
    "Result",
    "Default",
    "Clone",
    "Copy",
    "PartialEq",
    "std",
    "core",
    "drop",
    "print",
    "println",
    "eprintln",
    "format",
    "vec",
    "char",
    "i64",
    "f64",
    "bool",
    "str",
    "u32",
    "usize",
    "i32",
    "Box",
];

/// The Rust runtime, emitted after the program (`@FILE@` becomes the source path).
const RUNTIME: &str = include_str!("../rt/rs/runtime.rs");
const STD: &str = include_str!("../rt/rs/std.rs");
const JSON: &str = include_str!("../rt/rs/json.rs");

/// A Nyra name as a Rust identifier: `ny...` names belong to the runtime, and Rust's own words get a `_`.
fn name(n: &str) -> String {
    let id = names::ascii(n);
    if RESERVED.contains(&n) || n.starts_with("ny") || n.starts_with("Ny") {
        format!("{id}_")
    } else {
        id
    }
}

fn rstype(t: Ty) -> String {
    match t {
        Ty::Int => "i64".into(),
        Ty::Float => "f64".into(),
        Ty::Bool => "bool".into(),
        Ty::Char => "char".into(),
        Ty::Str => "Str".into(),
        Ty::Array(_) => format!("Rc<Vec<{}>>", rstype(t.elem().expect("an array"))),
        Ty::Map(_) => {
            let (k, v) = t.map_kv().expect("a map");
            format!("Rc<NyMap<{}, {}>>", rstype(k), rstype(v))
        }
        Ty::Struct(_) => name(&t.struct_name().expect("a struct")),
        other => unreachable!("the Rust backend got the type `{}`", other.name()),
    }
}

/// A Rust string literal.
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
                let _ = write!(out, "\\u{{{:x}}}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A Rust char literal.
fn char_lit(c: u32) -> String {
    match char::from_u32(c) {
        Some('\'') => "'\\''".into(),
        Some('\\') => "'\\\\'".into(),
        Some('\n') => "'\\n'".into(),
        Some('\r') => "'\\r'".into(),
        Some('\t') => "'\\t'".into(),
        Some('"') => "'\\\"'".into(),
        // (a parenthesis is spelled out, so `bare` never mistakes it for one around an expression)
        Some(ch) if c >= 0x20 && c != 0x7f && ch != '(' && ch != ')' => format!("'{ch}'"),
        _ => format!("'\\u{{{c:x}}}'"),
    }
}

/// The text of a `format!` string (braces doubled).
fn fmt_text(s: &str) -> String {
    let l = lit(s);
    l[1..l.len() - 1].replace('{', "{{").replace('}', "}}")
}

/// Each struct: the type, and how it prints.
fn structs(m: &Module, out: &mut String) {
    for (_, s) in &m.structs.0 {
        let n = name(&s.name);
        let copy = if s.managed { "" } else { "Copy, " };
        let _ = writeln!(out, "#[derive(Clone, {copy}Default, PartialEq)]");
        if s.fields.is_empty() {
            let _ = writeln!(out, "struct {n} {{}}\n");
        } else {
            let _ = writeln!(out, "struct {n} {{");
            for (f, t) in &s.fields {
                let _ = writeln!(out, "    {}: {},", name(f), rstype(*t));
            }
            out.push_str("}\n\n");
        }
        let _ = writeln!(out, "impl NyShow for {n} {{\n    fn show_in(&self, out: &mut String) {{");
        let _ = writeln!(out, "        out.push_str({});", lit(&if s.tuple { "(".to_string() } else { format!("{}(", s.name) }));
        for (k, (f, _)) in s.fields.iter().enumerate() {
            let sep = if k > 0 { ", " } else { "" };
            let label = if s.tuple { sep.to_string() } else { format!("{sep}{f}: ") };
            if !label.is_empty() {
                let _ = writeln!(out, "        out.push_str({});", lit(&label));
            }
            let _ = writeln!(out, "        self.{}.show_in(out);", name(f));
        }
        out.push_str("        out.push(')');\n    }\n}\n\n");
    }
}

/// How each struct is written as JSON and read from it (`json.str`, `json.parse`).
fn json_impls(m: &Module, out: &mut String) {
    for (_, s) in &m.structs.0 {
        let n = name(&s.name);
        let _ = writeln!(out, "impl NyJson for {n} {{\n    fn ny_jenc(&self, out: &mut String) {{");
        for (k, (f, _)) in s.fields.iter().enumerate() {
            let key = format!("{}{}:", if k == 0 { "{" } else { "," }, crate::diag::json_str(f));
            let _ = writeln!(out, "        out.push_str({});\n        self.{}.ny_jenc(out);", lit(&key), name(f));
        }
        if s.fields.is_empty() {
            out.push_str("        out.push('{');\n");
        }
        out.push_str("        out.push('}');\n    }\n    fn ny_jdec(p: &mut NyJP) -> Self {\n");
        for (k, (_, t)) in s.fields.iter().enumerate() {
            let _ = writeln!(out, "        let mut f{k}: Option<{}> = None;", rstype(*t));
        }
        out.push_str("        if p.open(b'{', \"an object\") {\n            loop {\n                let k = p.key();\n                match k.as_str() {\n");
        for (k, (f, t)) in s.fields.iter().enumerate() {
            let _ = writeln!(
                out,
                "                    {} => {{\n                        p.path.push({}.to_string());\n                        f{k} = Some(<{} as NyJson>::ny_jdec(p));\n                        p.path.pop();\n                    }}",
                lit(f),
                lit(&format!(".{f}")),
                rstype(*t)
            );
        }
        out.push_str("                    _ => p.skip(),\n                }\n                if !p.next(b'}') {\n                    break;\n                }\n            }\n        }\n");
        let fields: Vec<String> = s
            .fields
            .iter()
            .enumerate()
            .map(|(k, (f, _))| format!("{}: f{k}.unwrap_or_else(|| p.missing({}))", name(f), lit(f)))
            .collect();
        let _ = writeln!(out, "        {n} {{ {} }}\n    }}\n}}\n", fields.join(", "));
    }
}

/// `file` is the source path as given to nyra; runtime errors report it.
pub fn gen(m: &Module, file: &str) -> String {
    let mut out = String::from(concat!(
        "// generated by nyra: Rust (build with `rustc -O -C overflow-checks=off file.rs`: Nyra ints wrap)\n",
        "#![allow(dead_code, unused_mut, unused_variables, unused_assignments, unused_parens, non_snake_case)]\n\n",
        "use std::rc::Rc;\n\n",
    ));
    structs(m, &mut out);
    let json = m.uses_json();
    if json {
        json_impls(m, &mut out);
    }
    for f in &m.funcs {
        let info = Info::new(f);
        let n = names::scoped(f, name, "ny_", &info.loop_var);
        let params: Vec<String> = (0..f.params)
            .map(|i| {
                let t = rstype(f.locals[i].ty);
                if f.locals[i].inout {
                    format!("{}: &mut {t}", n[i])
                } else {
                    format!("{}: {t}", n[i])
                }
            })
            .collect();
        let ret = f.ret.map_or(String::new(), |t| format!(" -> {}", rstype(t)));
        let _ = writeln!(out, "fn {}({}){ret} {{", name(&f.name), params.join(", "));
        let mut g = Gen {
            m,
            f,
            info: &info,
            names: &n,
            refs: vec![false; f.locals.len()],
            out: String::new(),
            indent: 1,
            tmp: 0,
            steps: Vec::new(),
        };
        g.stmts(&f.body, true);
        out.push_str(&g.out);
        out.push_str("}\n\n");
    }
    out.push_str(&RUNTIME.replace("@FILE@", &lit(file)));
    if m.uses_std() {
        out.push_str(STD);
    }
    if json {
        out.push_str(JSON);
    }
    out
}

/// Where a place is: a place expression (`x`, `p.tags`), or the target of a `&mut` reference.
enum Pos {
    Place(String),
    Ref(String),
}

impl Pos {
    /// `&mut` of the place.
    fn mut_ref(&self) -> String {
        match self {
            Pos::Place(p) => format!("&mut {p}"),
            Pos::Ref(r) => r.clone(),
        }
    }

    /// The place itself (assignable).
    fn lvalue(&self) -> String {
        match self {
            Pos::Place(p) => p.clone(),
            Pos::Ref(r) => format!("*{r}"),
        }
    }
}

/// How an operand of a change in place is used.
#[derive(Clone, Copy)]
enum Use {
    /// as it is (an int, an index)
    Value,
    /// stored (a clone of a managed value)
    Owned,
    /// as `&str`
    StrRef,
    /// as `&[T]`
    Borrow,
}

struct Gen<'a> {
    m: &'a Module,
    f: &'a Func,
    info: &'a Info,
    names: &'a [String],
    /// Locals that hold a reference into an array (an element read only to read further).
    refs: Vec<bool>,
    out: String,
    indent: usize,
    /// Counter for helper names (values computed before a place is borrowed).
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

    /// The statements of a block; `at_end`: nothing follows them in the printed block.
    fn block(&mut self, ss: &'a [Stmt], at_end: bool) {
        self.indent += 1;
        self.stmts(ss, at_end);
        self.indent -= 1;
    }

    fn stmts(&mut self, ss: &'a [Stmt], at_end: bool) {
        let mut k = 0;
        while k < ss.len() {
            let s = &ss[k];
            for &l in self.info.before(s) {
                let line = format!("let mut {}: {};", self.decl_name(l), rstype(self.f.local(l).ty));
                self.line(&line);
            }
            if let Some(r) = range_for(ss, k, self.info) {
                if let Some(p) = r.pre {
                    self.stmt(p);
                }
                let i = self.decl_name(r.var).to_string();
                let line = format!("for {i} in {}..{} {{", self.arg(r.start), self.arg(r.end));
                self.line(&line);
                self.steps.push(&[]);
                self.block(r.body, true);
                self.steps.pop();
                self.line("}");
                k += r.len;
                continue;
            }
            let dup_next = |l: &LocalId| matches!(ss.get(k + 1), Some(Stmt { kind: StmtKind::Dup(d), .. }) if d == l);
            match &s.kind {
                // `x = v` and then `dup x`: `x = v.clone()`
                StmtKind::Set(l, e) if dup_next(l) => {
                    let v = self.owned(e);
                    self.set(s, *l, v);
                    k += 2;
                    continue;
                }
                // an element and then `dup`: a clone of it
                StmtKind::Op { dst: Some(d), op: RtOp::ArrGet, args } if dup_next(d) => {
                    let v = format!("{}.clone()", self.at(args, s));
                    self.set(s, *d, v);
                    k += 2;
                    continue;
                }
                StmtKind::Drop(l) => {
                    if !self.drop_is_implicit(ss, k, *l, at_end) {
                        let line = format!("drop({});", self.local(*l));
                        self.line(&line);
                    }
                    k += 1;
                    continue;
                }
                _ => {}
            }
            self.stmt(s);
            k += 1;
        }
    }

    /// True if Rust drops the value at `ss[k]` anyway: only drops follow until the block ends
    /// (the local belongs to this block), until a `return`/`break`/`continue`, or until the local
    /// gets a new value.
    fn drop_is_implicit(&self, ss: &[Stmt], k: usize, l: LocalId, at_end: bool) -> bool {
        match ss[k + 1..].iter().find(|s| !matches!(s.kind, StmtKind::Drop(_))) {
            None => at_end,
            Some(s) => match &s.kind {
                StmtKind::Return(_) | StmtKind::Break | StmtKind::Continue => true,
                StmtKind::Set(d, e) => *d == l && !mentions(e, l),
                _ => false,
            },
        }
    }

    /// The name a local is declared with.
    fn decl_name(&self, l: LocalId) -> &str {
        &self.names[l.0 as usize]
    }

    /// A local as an expression: an `inout` parameter is used through its reference.
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

    /// Plain values (numbers, bools, chars, structs of them) are `Copy`.
    fn copy_type(&self, t: Ty) -> bool {
        !self.m.managed(t)
    }

    /// `let x: T = value;` when `s` declares `x`, else `x = value;`.
    fn set(&mut self, s: &Stmt, l: LocalId, value: String) {
        let i = l.0 as usize;
        let line = if self.info.declares(s) == Some(l) {
            let m = if self.info.writes[i] > 1 || self.info.changed[i] { "mut " } else { "" };
            format!("let {m}{}: {} = {value};", self.decl_name(l), rstype(self.f.local(l).ty))
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
        super::bare(&self.expr(e)).to_string()
    }

    /// A value to store somewhere: a clone of a managed one (cheap: one more reference).
    fn owned(&self, e: &Expr) -> String {
        match e {
            Expr::Str(id) => format!("ny_str({})", lit(self.m.str(*id))),
            _ if self.copy_type(self.ty(e)) => self.arg(e),
            _ => format!("{}.clone()", self.expr(e)),
        }
    }

    /// A value that moves: a compiler temporary gives its value away, anything else is cloned.
    fn moved(&self, e: &Expr) -> String {
        match e {
            Expr::Local(l) if !self.info.names[l.0 as usize] && (l.0 as usize) >= self.f.params && !self.refs[l.0 as usize] => {
                self.local(*l)
            }
            _ => self.owned(e),
        }
    }

    /// `&value` for a parameter of type `&[T]` or `&str`.
    fn borrow(&self, e: &Expr) -> String {
        match e {
            Expr::Str(id) => lit(self.m.str(*id)),
            _ => format!("&{}", self.expr(e)),
        }
    }

    /// A `&str` of a string value.
    fn str_ref(&self, e: &Expr) -> String {
        match e {
            Expr::Str(id) => lit(self.m.str(*id)),
            _ => format!("{}.as_str()", self.expr(e)),
        }
    }

    /// `ny_at(&xs, i, line, col)`: a reference to an element.
    fn at(&self, args: &[Expr], s: &Stmt) -> String {
        format!("ny_at({}, {}, {}, {})", self.borrow(&args[0]), self.arg(&args[1]), s.span.line, s.span.col)
    }

    /// An operand of a change of `root`: one that reads `root` is computed first, into a local,
    /// because `root` is borrowed mutably while the change runs.
    fn operand(&mut self, a: &Expr, root: LocalId, how: Use) -> String {
        if mentions(a, root) {
            let t = self.fresh("v");
            let v = match how {
                Use::Value => self.arg(a),
                _ => self.owned(a),
            };
            self.line(&format!("let {t} = {v};"));
            return match how {
                Use::Value | Use::Owned => t,
                Use::StrRef => format!("{t}.as_str()"),
                Use::Borrow => format!("&{t}"),
            };
        }
        match how {
            Use::Value => self.arg(a),
            Use::Owned => self.owned(a),
            Use::StrRef => self.str_ref(a),
            Use::Borrow => self.borrow(a),
        }
    }

    fn stmt(&mut self, s: &'a Stmt) {
        let at = format!("{}, {}", s.span.line, s.span.col);
        match &s.kind {
            StmtKind::Set(l, e) => {
                let v = self.moved(e);
                self.set(s, *l, v);
            }
            StmtKind::Call { dst, func, args } => {
                // the variables passed `inout` are borrowed mutably for the whole call
                let roots: Vec<LocalId> =
                    args.iter().filter_map(|a| if let Arg::InOut(p) = a { Some(p.root) } else { None }).collect();
                let mut parts = Vec::with_capacity(args.len());
                for a in args {
                    match a {
                        Arg::Val(e) => parts.push(self.owned(e)),
                        Arg::InOut(p) => {
                            let pos = self.place(p, &roots);
                            parts.push(pos.mut_ref());
                        }
                    }
                }
                let call = format!("{}({})", name(&self.m.func(*func).name), parts.join(", "));
                self.assign(s, *dst, call);
            }
            StmtKind::Op { dst, op, args } => self.op(s, *dst, *op, args, &at),
            StmtKind::Store { place, value } => {
                // (Rust evaluates the value before the place it is assigned to)
                let v = self.owned(value);
                let pos = self.place(place, &[place.root]);
                let line = format!("{} = {v};", pos.lvalue());
                self.line(&line);
            }
            StmtKind::Mutate { dst, op, place, args } => {
                let root = place.root;
                let code = match op {
                    RtOp::StrAppend => {
                        let t = self.operand(&args[0], root, Use::StrRef);
                        format!("Rc::make_mut({}).push_str({t})", self.place(place, &[root]).mut_ref())
                    }
                    RtOp::ArrPush => {
                        let v = self.operand(&args[0], root, Use::Owned);
                        format!("Rc::make_mut({}).push({v})", self.place(place, &[root]).mut_ref())
                    }
                    RtOp::ArrPop => format!("ny_pop({}, {at})", self.place(place, &[root]).mut_ref()),
                    RtOp::ArrInsert => {
                        let i = self.operand(&args[0], root, Use::Value);
                        let v = self.operand(&args[1], root, Use::Owned);
                        format!("ny_insert({}, {i}, {v}, {at})", self.place(place, &[root]).mut_ref())
                    }
                    RtOp::ArrRemove => {
                        let i = self.operand(&args[0], root, Use::Value);
                        format!("ny_remove({}, {i}, {at})", self.place(place, &[root]).mut_ref())
                    }
                    RtOp::ArrSort => {
                        let mr = self.place(place, &[root]).mut_ref();
                        if self.place_ty(place).elem() == Some(Ty::Float) {
                            format!("ny_sort_floats({mr})")
                        } else {
                            format!("Rc::make_mut({mr}).sort()")
                        }
                    }
                    RtOp::ArrReverse => format!("Rc::make_mut({}).reverse()", self.place(place, &[root]).mut_ref()),
                    RtOp::ArrSortBy => {
                        let keys = self.operand(&args[0], root, Use::Borrow);
                        let lt = if self.ty(&args[0]).elem() == Some(Ty::Float) { "ny_lt_float" } else { "ny_lt_ord" };
                        format!("ny_sort_by({}, {keys}, {lt})", self.place(place, &[root]).mut_ref())
                    }
                    RtOp::ArrSwap => {
                        let i = self.operand(&args[0], root, Use::Value);
                        let j = self.operand(&args[1], root, Use::Value);
                        format!("ny_swap({}, {i}, {j}, {at})", self.place(place, &[root]).mut_ref())
                    }
                    RtOp::ArrAppend => {
                        let ys = self.operand(&args[0], root, Use::Borrow);
                        format!("ny_append({}, {ys})", self.place(place, &[root]).mut_ref())
                    }
                    RtOp::MapSet => {
                        let k = self.operand(&args[0], root, Use::Owned);
                        let v = self.operand(&args[1], root, Use::Owned);
                        format!("Rc::make_mut({}).set({k}, {v})", self.place(place, &[root]).mut_ref())
                    }
                    RtOp::MapRemove => {
                        let k = self.operand(&args[0], root, Use::Owned);
                        format!("Rc::make_mut({}).remove(&{k})", self.place(place, &[root]).mut_ref())
                    }
                    other => unreachable!("{} does not change a place", other.name()),
                };
                self.assign(s, *dst, code);
            }
            StmtKind::If { .. } => self.if_chain(s),
            StmtKind::Loop { head, cond, body, step } => self.lp(head, cond, body, step),
            StmtKind::ForEach { var, iter, body } => {
                let v = self.decl_name(*var).to_string();
                let it = self.expr(iter);
                let line = match self.ty(iter) {
                    Ty::Str => format!("for {v} in {it}.chars() {{"),
                    t if self.copy_type(t.elem().expect("verified: an array")) => format!("for &{v} in {it}.iter() {{"),
                    _ => format!("for {v} in {it}.iter().cloned() {{"),
                };
                self.line(&line);
                self.steps.push(&[]);
                self.block(body, true);
                self.steps.pop();
                self.line("}");
            }
            StmtKind::Break => self.line("break;"),
            StmtKind::Continue => {
                // a counted loop that is not a `for` runs its step first
                if let Some(step) = self.steps.last().copied() {
                    self.stmts(step, false);
                }
                self.line("continue;");
            }
            StmtKind::Return(None) => self.line("return;"),
            StmtKind::Return(Some(e)) => {
                let line = format!("return {};", self.moved(e));
                self.line(&line);
            }
            // `Dup` comes right after the statement that gives the local its value (handled
            // there); Rust frees what nothing references any more, and `keep` changes nothing
            StmtKind::Dup(_) | StmtKind::Keep(_) | StmtKind::Drop(_) => {}
            StmtKind::Free(l) => {
                let line = format!("{} = Default::default(); // free", self.local(*l));
                self.line(&line);
            }
        }
    }

    /// Where a place is, for a write: every array on the way is made unique (`ny_at_mut`
    /// copies a shared one) and every index is checked. An index that reads one of the
    /// `borrowed` variables is computed first.
    fn place(&mut self, p: &Place, borrowed: &[LocalId]) -> Pos {
        let root = p.root.0 as usize;
        let mut pos = if root < self.f.params && self.f.locals[root].inout {
            Pos::Ref(self.decl_name(p.root).to_string())
        } else {
            Pos::Place(self.local(p.root))
        };
        let mut t = self.f.local(p.root).ty;
        for step in &p.path {
            match step {
                Step::Index(i, span) => {
                    let idx = if borrowed.iter().any(|r| mentions(i, *r)) {
                        let v = self.fresh("i");
                        let line = format!("let {v} = {};", self.arg(i));
                        self.line(&line);
                        v
                    } else {
                        self.arg(i)
                    };
                    pos = Pos::Ref(format!("ny_at_mut({}, {idx}, {}, {})", pos.mut_ref(), span.line, span.col));
                    t = t.elem().expect("verified: an array");
                }
                Step::Field(k) => {
                    let info = self.m.structs.get(t).expect("verified: a struct");
                    let f = name(&info.fields[*k as usize].0);
                    t = info.fields[*k as usize].1;
                    pos = Pos::Place(match pos {
                        Pos::Place(x) => format!("{x}.{f}"),
                        Pos::Ref(r) => format!("{r}.{f}"),
                    });
                }
            }
        }
        pos
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
                let (fmt, values) = self.format_parts(args);
                let line = if values.is_empty() {
                    format!("println!(\"{fmt}\");")
                } else {
                    format!("println!(\"{fmt}\", {});", values.join(", "))
                };
                self.line(&line);
                return;
            }
            RtOp::PrintNoLine => {
                let (fmt, values) = self.format_parts(args);
                let line = if values.is_empty() {
                    format!("print!(\"{fmt}\");")
                } else {
                    format!("print!(\"{fmt}\", {});", values.join(", "))
                };
                self.line(&line);
                return;
            }
            RtOp::Format => {
                let (fmt, values) = self.format_parts(args);
                if values.is_empty() {
                    // only text: the string itself (not a `format!` string with doubled braces)
                    let text: String = args
                        .iter()
                        .map(|p| match p {
                            Expr::Str(id) => self.m.str(*id).to_string(),
                            Expr::Int(n) => n.to_string(),
                            _ => String::new(),
                        })
                        .collect();
                    format!("ny_str({})", lit(&text))
                } else {
                    format!("Rc::new(format!(\"{fmt}\", {}))", values.join(", "))
                }
            }
            RtOp::DivInt => format!("ny_div({}, {}, {at})", a[0], a[1]),
            RtOp::AddInt => format!("ny_add({}, {}, {at})", a[0], a[1]),
            RtOp::SubInt => format!("ny_sub({}, {}, {at})", a[0], a[1]),
            RtOp::MulInt => format!("ny_mul({}, {}, {at})", a[0], a[1]),
            RtOp::NegInt => format!("ny_neg({}, {at})", a[0]),
            RtOp::RemInt => format!("ny_rem({}, {}, {at})", a[0], a[1]),
            RtOp::FloatToInt => format!("ny_f2i({}, {at})", a[0]),
            RtOp::StrConcat => format!("Rc::new(format!(\"{{}}{{}}\", {}, {}))", a[0], a[1]),
            RtOp::StrAt => format!("ny_char_at({}, {}, {at})", self.str_ref(&args[0]), a[1]),
            RtOp::StrSlice => format!("ny_str_slice({}, {}, {}, {at})", self.str_ref(&args[0]), a[1], a[2]),
            RtOp::StrReplace => {
                format!("ny_replace({}, {}, {}, {at})", self.str_ref(&args[0]), self.str_ref(&args[1]), self.str_ref(&args[2]))
            }
            RtOp::StrTrim => format!("ny_trim({})", self.str_ref(&args[0])),
            RtOp::StrUpper => format!("Rc::new({}.to_ascii_uppercase())", self.str_ref(&args[0])),
            RtOp::StrLower => format!("Rc::new({}.to_ascii_lowercase())", self.str_ref(&args[0])),
            RtOp::StrRepeat => format!("ny_str_repeat({}, {}, {at})", self.str_ref(&args[0]), a[1]),
            RtOp::StrToInt => format!("ny_int({}, {at})", self.str_ref(&args[0])),
            RtOp::StrToFloat => format!("ny_float({}, {at})", self.str_ref(&args[0])),
            RtOp::CharFrom => format!("ny_chr({}, {at})", a[0]),
            RtOp::StrChars => format!("Rc::new({}.chars().collect())", self.str_ref(&args[0])),
            RtOp::StrCodes => format!("Rc::new({}.chars().map(|c| c as i64).collect())", self.str_ref(&args[0])),
            RtOp::StrSplit => format!("ny_split({}, {}, {at})", self.str_ref(&args[0]), self.str_ref(&args[1])),
            RtOp::CheckStep => format!("ny_check_step({}, {at})", a[0]),
            RtOp::CheckNonEmpty => format!("ny_check_non_empty({}, {}, {at})", a[0], a[1]),
            RtOp::StrPadLeft => format!("ny_pad({}, {}, {}, true)", self.str_ref(&args[0]), a[1], a[2]),
            RtOp::StrPadRight => format!("ny_pad({}, {}, {}, false)", self.str_ref(&args[0]), a[1], a[2]),
            RtOp::ArrNew => {
                let items: Vec<String> = args.iter().map(|x| self.owned(x)).collect();
                format!("Rc::new(vec![{}])", items.join(", "))
            }
            RtOp::StructNew => {
                let t = self.f.local(dst.expect("verified: a destination")).ty;
                let info = self.m.structs.get(t).expect("verified: a struct");
                let fields: Vec<String> =
                    args.iter().zip(&info.fields).map(|(x, (f, _))| format!("{}: {}", name(f), self.owned(x))).collect();
                if fields.is_empty() {
                    format!("{} {{}}", name(&info.name))
                } else {
                    format!("{} {{ {} }}", name(&info.name), fields.join(", "))
                }
            }
            RtOp::ArrGet => {
                let elem = self.ty(&args[0]).elem().expect("verified: an array");
                let r = self.at(args, s);
                let d = dst.expect("verified: a destination");
                if self.copy_type(elem) {
                    format!("*{r}")
                } else if self.info.declares(s) == Some(d) {
                    // read only to read further (`xs[i][j] += 1`): a reference, so the array is
                    // not shared when it is written next
                    self.refs[d.0 as usize] = true;
                    let line = format!("let {} = {r};", self.decl_name(d));
                    self.line(&line);
                    return;
                } else {
                    format!("{r}.clone()")
                }
            }
            RtOp::MapNew => {
                let items: Vec<String> = args.chunks(2).map(|p| format!("({}, {})", self.owned(&p[0]), self.owned(&p[1]))).collect();
                format!("ny_mnew(vec![{}])", items.join(", "))
            }
            RtOp::MapGet => format!("ny_mget(&{}, &{}, {at}).clone()", self.expr(&args[0]), self.owned(&args[1])),
            RtOp::MapGetOr => {
                format!("ny_mget_or(&{}, &{}, &{}).clone()", self.expr(&args[0]), self.owned(&args[1]), self.owned(&args[2]))
            }
            RtOp::MapKeys => format!("ny_mkeys(&{})", self.expr(&args[0])),
            RtOp::MapValues => format!("ny_mvalues(&{})", self.expr(&args[0])),
            RtOp::ArrSlice => format!("ny_slice({}, {}, {}, {at})", self.borrow(&args[0]), a[1], a[2]),
            RtOp::ArrRepeat => format!("ny_repeat({}, {}, {at})", self.borrow(&args[0]), a[1]),
            RtOp::ArrConcat => format!("ny_concat({}, {})", self.borrow(&args[0]), self.borrow(&args[1])),
            RtOp::ArrJoin => {
                let f = if self.ty(&args[0]).elem() == Some(Ty::Char) { "ny_join_chars" } else { "ny_join" };
                format!("{f}({}, {})", self.borrow(&args[0]), self.str_ref(&args[1]))
            }
            RtOp::JsonStr => format!("ny_jstr(&{})", self.expr(&args[0])),
            RtOp::JsonParse => {
                let t = self.f.local(dst.expect("verified: a destination")).ty;
                format!("ny_jparse::<{}>({}, {at})", rstype(t), self.str_ref(&args[0]))
            }
            RtOp::Std(f) => {
                // strings are passed as `&str`
                let mut parts: Vec<String> =
                    args.iter().map(|x| if self.ty(x) == Ty::Str { self.str_ref(x) } else { self.arg(x) }).collect();
                parts.push(at.to_string());
                format!("ny_std_{}({})", f.rt_name(), parts.join(", "))
            }
            other => unreachable!("{} is a `Mutate`", other.name()),
        };
        self.assign(s, dst, code);
    }

    /// A `format!` string and its values for the parts of a print or interpolation.
    fn format_parts(&self, parts: &[Expr]) -> (String, Vec<String>) {
        let mut fmt = String::new();
        let mut values = Vec::new();
        for p in parts {
            match p {
                Expr::Str(id) => fmt.push_str(&fmt_text(self.m.str(*id))),
                Expr::Int(n) => fmt.push_str(&n.to_string()),
                _ => {
                    fmt.push_str("{}");
                    let x = self.arg(p);
                    values.push(match self.ty(p) {
                        Ty::Float => format!("ny_num({x})"),
                        Ty::Array(_) | Ty::Struct(_) | Ty::Map(_) => format!("ny_show(&{})", self.expr(p)),
                        _ => x,
                    });
                }
            }
        }
        (fmt, values)
    }

    fn if_chain(&mut self, s: &'a Stmt) {
        let mut cur = s;
        let mut head = "if";
        loop {
            let StmtKind::If { cond, then, els } = &cur.kind else { unreachable!() };
            let line = format!("{head} {} {{", self.arg(cond));
            self.line(&line);
            self.block(then, true);
            match els.as_slice() {
                [] => break,
                [next] if scope::else_if(els) => {
                    cur = next;
                    head = "} else if";
                }
                _ => {
                    self.line("} else {");
                    self.block(els, true);
                    break;
                }
            }
        }
        self.line("}");
    }

    fn lp(&mut self, head: &'a [Stmt], cond: &Expr, body: &'a [Stmt], step: &'a [Stmt]) {
        // `continue` runs the step first (see `Continue`)
        self.steps.push(step);
        if head.is_empty() {
            let line = format!("while {} {{", self.arg(cond));
            self.line(&line);
            self.indent += 1;
        } else {
            self.line("loop {");
            self.indent += 1;
            self.stmts(head, false);
            let line = format!("if !{} {{", self.expr(cond));
            self.line(&line);
            self.line("    break;");
            self.line("}");
        }
        self.stmts(body, step.is_empty());
        self.stmts(step, true);
        self.indent -= 1;
        self.line("}");
        self.steps.pop();
    }

    fn expr(&self, e: &Expr) -> String {
        match e {
            Expr::Int(i64::MIN) => "i64::MIN".to_string(),
            Expr::Int(n) => n.to_string(),
            Expr::Float(f) if f.is_infinite() => (if *f > 0.0 { "f64::INFINITY" } else { "f64::NEG_INFINITY" }).to_string(),
            Expr::Float(f) => format!("{f:?}"),
            Expr::Bool(b) => b.to_string(),
            Expr::Char(c) => char_lit(*c),
            Expr::Str(id) => format!("ny_str({})", lit(self.m.str(*id))),
            Expr::Local(l) => self.local(*l),
            Expr::Unary(op, x) => {
                let x = self.expr(x);
                match op {
                    UnOp::Not => format!("(!{x})"),
                    _ if x.starts_with('-') => format!("(-({x}))"),
                    _ => format!("(-{x})"),
                }
            }
            Expr::Binary(op, ea, eb) => {
                let (mut a, b) = (self.expr(ea), self.expr(eb));
                // two int literals would be computed as `i32`: make the first one an `i64`
                if let Expr::Int(n) = **ea {
                    if n != i64::MIN {
                        a = format!("{n}i64");
                    }
                }
                match op {
                    BinOp::SEq | BinOp::SNe | BinOp::SLt | BinOp::SLe | BinOp::SGt | BinOp::SGe => {
                        format!("({} {} {})", self.str_ref(ea), op.symbol(), self.str_ref(eb))
                    }
                    _ => format!("({a} {} {b})", op.symbol()),
                }
            }
            // a managed value is cloned in its branch (an `if` would move it out of its variable)
            Expr::Select(c, a, b) if !self.copy_type(a.ty(self.f)) => {
                format!("(if {} {{ {}.clone() }} else {{ {}.clone() }})", self.arg(c), self.expr(a), self.expr(b))
            }
            Expr::Select(c, a, b) => format!("(if {} {{ {} }} else {{ {} }})", self.arg(c), self.arg(a), self.arg(b)),
            Expr::IntToFloat(x) => format!("({} as f64)", self.expr(x)),
            Expr::Field(x, k, _) => {
                let info = self.m.structs.get(x.ty(self.f)).expect("verified: a struct");
                format!("{}.{}", self.expr(x), name(&info.fields[*k as usize].0))
            }
            Expr::Pure(p, args) => {
                let a: Vec<String> = args.iter().map(|x| self.arg(x)).collect();
                match p {
                    PureFn::StrLen => format!("ny_len({})", self.str_ref(&args[0])),
                    PureFn::StrContains => format!("{}.contains({})", self.str_ref(&args[0]), self.str_ref(&args[1])),
                    PureFn::StrStartsWith => format!("{}.starts_with({})", self.str_ref(&args[0]), self.str_ref(&args[1])),
                    PureFn::StrEndsWith => format!("{}.ends_with({})", self.str_ref(&args[0]), self.str_ref(&args[1])),
                    PureFn::StrIndexOf => format!("ny_find({}, {})", self.str_ref(&args[0]), self.str_ref(&args[1])),
                    PureFn::CharCode => format!("({} as i64)", self.expr(&args[0])),
                    PureFn::CharUpper => format!("{}.to_ascii_uppercase()", self.expr(&args[0])),
                    PureFn::CharLower => format!("{}.to_ascii_lowercase()", self.expr(&args[0])),
                    PureFn::CharIsDigit => format!("{}.is_ascii_digit()", self.expr(&args[0])),
                    PureFn::CharIsLetter => format!("{}.is_ascii_alphabetic()", self.expr(&args[0])),
                    PureFn::CharIsUpper => format!("{}.is_ascii_uppercase()", self.expr(&args[0])),
                    PureFn::CharIsLower => format!("{}.is_ascii_lowercase()", self.expr(&args[0])),
                    PureFn::CharIsSpace => format!("ny_is_space({})", a[0]),
                    PureFn::ArrLen | PureFn::MapLen => format!("({}.len() as i64)", self.expr(&args[0])),
                    PureFn::MapHas => format!("{}.has(&{})", self.expr(&args[0]), self.owned(&args[1])),
                    PureFn::ArrContains => format!("{}.contains(&{})", self.expr(&args[0]), self.expr(&args[1])),
                    PureFn::ArrIndexOf => format!("ny_index_of({}, &{})", self.borrow(&args[0]), self.expr(&args[1])),
                }
            }
        }
    }
}
