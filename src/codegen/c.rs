//! C99 backend. Output is plain, readable C that gcc/clang/tcc can compile.

use std::fmt::Write;

use super::{bare, names, scope::mentions};
use crate::ast::Span;
use crate::ir::{Arg, BinOp, Expr, Func, LocalId, Module, Place, PureFn, RtOp, Step, Stmt, StmtKind, StructInfo, Ty, UnOp};

/// The C runtime, emitted before every program (`@FILE@` becomes the source path).
const PRELUDE: &str = include_str!("../rt/c/core.c");
const STRINGS: &str = include_str!("../rt/c/str.c");
const ARRAYS: &str = include_str!("../rt/c/arr.c");
const STD: &str = include_str!("../rt/c/std.c");
const MAPS: &str = include_str!("../rt/c/map.c");
const JSON: &str = include_str!("../rt/c/json.c");

const RESERVED: &[&str] = &[
    "auto",
    "break",
    "case",
    "char",
    "const",
    "continue",
    "default",
    "do",
    "double",
    "else",
    "enum",
    "extern",
    "float",
    "for",
    "goto",
    "if",
    "inline",
    "int",
    "long",
    "register",
    "restrict",
    "return",
    "short",
    "signed",
    "sizeof",
    "static",
    "struct",
    "switch",
    "typedef",
    "union",
    "unsigned",
    "void",
    "volatile",
    "while",
    "bool",
    "true",
    "false",
    "NULL",
    "EOF",
    "errno",
    "stdin",
    "stdout",
    "stderr",
    "main",
    "printf",
    "puts",
    "putchar",
    "fwrite",
    "fputs",
    "snprintf",
    "sprintf",
    "strcmp",
    "strcpy",
    "strtod",
    "memcpy",
    "memmove",
    "memcmp",
    "strlen",
    "atoi",
    "malloc",
    "realloc",
    "free",
    "exit",
    "getenv",
    "int64_t",
    "uint32_t",
    "uint64_t",
    "size_t",
    "DBL_MAX",
    "INT64_MAX",
    "INT64_MIN",
];

/// More C words a user name must not be: keywords of newer C and compilers, common macros.
const RESERVED_MORE: &[&str] = &["asm", "typeof", "fortran", "alignas", "alignof", "noreturn", "thread_local", "complex", "imaginary"];

fn var(name: &str) -> String {
    // `ny...` names belong to nyra (user functions are `ny_<name>`, the runtime is `nyrt_*`).
    // Names starting with `_` are reserved in C, and an all-caps name may be a macro of the
    // C library (`INT32_MAX`, `WIN32`, `BUFSIZ`): both get a suffix too.
    let macro_like =
        name.chars().any(|c| c.is_ascii_uppercase()) && name.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
    if RESERVED.contains(&name) || RESERVED_MORE.contains(&name) || name.starts_with("ny") || name.starts_with('_') || macro_like {
        format!("{name}_")
    } else {
        name.to_string()
    }
}

fn ctype(t: Ty) -> String {
    match t {
        Ty::Int => "int64_t".into(),
        Ty::Float => "double".into(),
        Ty::Bool => "bool".into(),
        Ty::Char => "nyrt_char".into(),
        Ty::Str => "nyrt_str*".into(),
        Ty::Array(_) => "nyrt_arr*".into(),
        Ty::Map(_) => "nyrt_map*".into(),
        Ty::Struct(_) => format!("nyS_{}", t.struct_name().expect("a struct")),
        other => unreachable!("the C backend got the type `{}`", other.name()),
    }
}

/// `nyrt_str_retain(x);`, `nyS_User_retain(&x);`: one more owner for the managed value at the lvalue `x`.
fn retain(t: Ty, x: &str) -> String {
    match t {
        Ty::Struct(_) => format!("{}_retain(&{x});", ctype(t)),
        _ => format!("nyrt_{}_retain({x});", rt_name(t)),
    }
}

/// Makes the managed value at the lvalue `x` permanent (`keep(x)`).
fn keep(t: Ty, x: &str) -> String {
    match t {
        Ty::Struct(_) => format!("{}_keep(&{x});", ctype(t)),
        _ => format!("nyrt_{}_keep({x});", rt_name(t)),
    }
}

/// One owner less for the managed value at the lvalue `x`.
fn release(t: Ty, x: &str) -> String {
    match t {
        Ty::Struct(_) => format!("{}_release(&{x});", ctype(t)),
        _ => format!("nyrt_{}_release({x});", rt_name(t)),
    }
}

/// The runtime's name for a type in `nyrt_put_<ty>` / `nyrt_buf_<ty>` / `nyrt_<ty>_retain`.
fn rt_name(t: Ty) -> &'static str {
    match t {
        Ty::Int => "int",
        Ty::Float => "float",
        Ty::Bool => "bool",
        Ty::Char => "char",
        Ty::Str => "str",
        Ty::Array(_) => "arr",
        Ty::Map(_) => "map",
        other => unreachable!("no runtime functions for `{}` yet", other.name()),
    }
}

/// The element type descriptor of an array of `elem`.
fn desc(elem: Ty) -> String {
    match elem {
        Ty::Struct(_) => format!("&nyT_{}", elem.struct_name().expect("a struct")),
        _ => format!("&nyrt_T_{}", rt_name(elem)),
    }
}

/// The C name of a field (escaped like a variable).
fn field(info: &StructInfo, k: usize) -> String {
    var(&info.fields[k].0)
}

/// The typedef, the reference counting, equality and printing of every struct, and the type
/// descriptor that arrays of it use.
fn structs(m: &Module, out: &mut String) {
    if m.structs.0.is_empty() {
        return;
    }
    // by-value fields come first: the order of `m.structs`
    for (_, s) in &m.structs.0 {
        let n = format!("nyS_{}", s.name);
        let fields: String = if s.fields.is_empty() {
            " char nyrt_unused;".to_string()
        } else {
            (0..s.fields.len()).map(|k| format!(" {} {};", ctype(s.fields[k].1), field(s, k))).collect()
        };
        let _ = writeln!(out, "typedef struct {n} {{{fields} }} {n};");
    }
    for (_, s) in &m.structs.0 {
        let n = format!("nyS_{}", s.name);
        if s.managed {
            let _ =
                writeln!(out, "static void {n}_retain(void *p);\nstatic void {n}_release(void *p);\nstatic void {n}_keep(void *p);");
        }
        let _ = writeln!(out, "static bool {n}_eq(const void *a, const void *b);");
        let _ = writeln!(out, "static void {n}_fmt(nyrt_buf *o, const void *p);");
        let (r, d, k) = if s.managed {
            (format!("{n}_retain"), format!("{n}_release"), format!("{n}_keep"))
        } else {
            ("NULL".into(), "NULL".into(), "NULL".into())
        };
        let _ = writeln!(out, "static const nyrt_type nyT_{} = {{ sizeof({n}), {r}, {d}, {n}_eq, {n}_fmt, NULL, {k} }};", s.name);
    }
    for (_, s) in &m.structs.0 {
        let n = format!("nyS_{}", s.name);
        if s.managed {
            for (what, f) in [("retain", retain as fn(Ty, &str) -> String), ("release", release), ("keep", keep)] {
                let _ = writeln!(out, "static void {n}_{what}(void *p) {{\n    {n} *v = p;");
                for (k, (_, t)) in s.fields.iter().enumerate() {
                    if m.managed(*t) {
                        let _ = writeln!(out, "    {}", f(*t, &format!("v->{}", field(s, k))));
                    }
                }
                out.push_str("}\n");
            }
        }
        let eqs: Vec<String> = s
            .fields
            .iter()
            .enumerate()
            .map(|(k, (_, t))| {
                let f = field(s, k);
                match t {
                    Ty::Str => format!("nyrt_str_eq(x->{f}, y->{f})"),
                    Ty::Array(_) => format!("nyrt_arr_eq(x->{f}, y->{f})"),
                    Ty::Map(_) => format!("nyrt_map_eq(x->{f}, y->{f})"),
                    Ty::Struct(_) => format!("{}_eq(&x->{f}, &y->{f})", ctype(*t)),
                    _ => format!("x->{f} == y->{f}"),
                }
            })
            .collect();
        let body = if eqs.is_empty() { "(void)x; (void)y; return true;".to_string() } else { format!("return {};", eqs.join(" && ")) };
        let _ = writeln!(out, "static bool {n}_eq(const void *a, const void *b) {{\n    const {n} *x = a, *y = b;\n    {body}\n}}");
        // `Point(x: 1, y: 2)`: the way the value is written in Nyra
        let _ = writeln!(out, "static void {n}_fmt(nyrt_buf *o, const void *p) {{\n    const {n} *v = p;\n    (void)v;");
        if !s.variants.is_empty() {
            // an enum value prints as its variant: `Dir.N`
            let names: Vec<String> = s.variants.iter().map(|v| string_lit(&format!("{}.{v}", s.name))).collect();
            let f = field(s, 0);
            let _ = writeln!(
                out,
                "    static const char *const names[] = {{ {} }};\n    nyrt_buf_lit(o, names[v->{f}], (int64_t)strlen(names[v->{f}]));\n}}",
                names.join(", ")
            );
            continue;
        }
        // (a tuple prints as `(1, "a")`: no name, no field names)
        let head = if s.tuple {
            "(".to_string()
        } else if s.option {
            "Some(".to_string()
        } else {
            format!("{}(", s.name)
        };
        if s.option {
            // `none`, or `Some(value)`
            let _ = writeln!(out, "    if (!v->{}) {{ nyrt_buf_lit(o, \"none\", 4); return; }}", field(s, 0));
        }
        let _ = writeln!(out, "    nyrt_buf_lit(o, {}, {});", string_lit(&head), head.len());
        for (k, (fname, t)) in s.fields.iter().enumerate() {
            if s.option && k == 0 {
                continue;
            }
            let sep = if k > 0 { ", " } else { "" };
            let label = if s.tuple {
                sep.to_string()
            } else if s.option {
                String::new()
            } else {
                format!("{sep}{fname}: ")
            };
            if !label.is_empty() {
                let _ = writeln!(out, "    nyrt_buf_lit(o, {}, {});", string_lit(&label), label.len());
            }
            let f = field(s, k);
            let line = match t {
                Ty::Int => format!("nyrt_buf_int(o, v->{f});"),
                Ty::Float => format!("nyrt_buf_float(o, v->{f});"),
                Ty::Bool => format!("nyrt_buf_bool(o, v->{f});"),
                Ty::Char => format!("nyrt_buf_repr_char(o, v->{f});"),
                Ty::Str => format!("nyrt_buf_repr_str(o, v->{f});"),
                Ty::Array(_) => format!("nyrt_buf_arr(o, v->{f});"),
                Ty::Map(_) => format!("nyrt_buf_map(o, v->{f});"),
                _ => format!("{}_fmt(o, &v->{f});", ctype(*t)),
            };
            let _ = writeln!(out, "    {line}");
        }
        out.push_str("    nyrt_buf_lit(o, \")\", 1);\n}\n");
    }
    out.push('\n');
}

/// The arrays and structs whose values `json.str` writes or `json.parse` reads, with the ones
/// inside them: each gets a generated encoder `nyJE_<k>` and decoder `nyJD_<k>`.
fn json_types(m: &Module) -> Vec<Ty> {
    fn add(m: &Module, t: Ty, v: &mut Vec<Ty>) {
        if !matches!(t, Ty::Array(_) | Ty::Struct(_)) || v.contains(&t) {
            return;
        }
        v.push(t);
        match t {
            Ty::Array(_) => add(m, t.elem().expect("an array"), v),
            _ => {
                for (_, ft) in &m.structs.get(t).expect("a struct").fields {
                    add(m, *ft, v);
                }
            }
        }
    }
    fn walk(m: &Module, f: &Func, ss: &[Stmt], v: &mut Vec<Ty>) {
        for s in ss {
            match &s.kind {
                StmtKind::Op { op: RtOp::JsonStr, args, .. } => add(m, args[0].ty(f), v),
                StmtKind::Op { op: RtOp::JsonParse, dst: Some(d), .. } => add(m, f.local(*d).ty, v),
                StmtKind::If { then, els, .. } => {
                    walk(m, f, then, v);
                    walk(m, f, els, v);
                }
                StmtKind::Loop { head, body, step, .. } => {
                    walk(m, f, head, v);
                    walk(m, f, body, v);
                    walk(m, f, step, v);
                }
                StmtKind::ForEach { body, .. } => walk(m, f, body, v),
                _ => {}
            }
        }
    }
    let mut v = Vec::new();
    for f in &m.funcs {
        walk(m, f, &f.body, &mut v);
    }
    v
}

/// The JSON encoder (`enc`) or decoder of a type.
fn jfn(json: &[Ty], t: Ty, enc: bool) -> String {
    let (rt, gen) = if enc { ("nyrt_jenc", "nyJE") } else { ("nyrt_jdec", "nyJD") };
    match t {
        Ty::Array(_) | Ty::Struct(_) => format!("{gen}_{}", json.iter().position(|x| *x == t).expect("collected by json_types")),
        _ => format!("{rt}_{}", rt_name(t)),
    }
}

/// The encoders and decoders of the arrays and structs in `json`.
fn json_funcs(m: &Module, json: &[Ty], out: &mut String) {
    for k in 0..json.len() {
        let _ = writeln!(out, "static void nyJE_{k}(nyrt_buf *b, const void *v);\nstatic void nyJD_{k}(nyrt_jp *p, void *out);");
    }
    for (k, t) in json.iter().enumerate() {
        if let Some(elem) = t.elem() {
            let _ = writeln!(
                out,
                "static void nyJE_{k}(nyrt_buf *b, const void *v) {{ nyrt_jenc_arr(b, v, {}); }}\nstatic void nyJD_{k}(nyrt_jp *p, void *out) {{ nyrt_jdec_arr(p, out, {}, {}); }}",
                jfn(json, elem, true),
                desc(elem),
                jfn(json, elem, false)
            );
            continue;
        }
        let info = m.structs.get(*t).expect("a struct");
        let n = ctype(*t);
        let _ = writeln!(out, "static void nyJE_{k}(nyrt_buf *b, const void *p) {{\n    const {n} *v = p;\n    (void)v;");
        for (i, (fname, ft)) in info.fields.iter().enumerate() {
            let key = format!("{}{}:", if i == 0 { "{" } else { "," }, crate::diag::json_str(fname));
            let _ = writeln!(
                out,
                "    nyrt_buf_lit(b, {}, {});\n    {}(b, &v->{});",
                string_lit(&key),
                key.len(),
                jfn(json, *ft, true),
                field(info, i)
            );
        }
        if info.fields.is_empty() {
            out.push_str("    nyrt_buf_lit(b, \"{\", 1);\n");
        }
        out.push_str("    nyrt_buf_lit(b, \"}\", 1);\n}\n");
        let _ = writeln!(
            out,
            "static void nyJD_{k}(nyrt_jp *p, void *out) {{\n    {n} v = {{0}};\n    bool seen[{}] = {{0}};",
            info.fields.len().max(1)
        );
        out.push_str(
            "    if (nyrt_jopen(p, '{', \"an object\")) {\n        do {\n            nyrt_str *k = nyrt_jkey(p);\n            ",
        );
        for (i, (fname, ft)) in info.fields.iter().enumerate() {
            let f = field(info, i);
            let again = if m.managed(*ft) { format!("if (seen[{i}]) {} ", release(*ft, &format!("v.{f}"))) } else { String::new() };
            let _ = write!(
                out,
                "if (k->len == {} && memcmp(k->data, {}, {}) == 0) {{\n                {again}nyrt_jpush_key(p, {});\n                {}(p, &v.{f});\n                nyrt_jpop(p);\n                seen[{i}] = true;\n            }} else ",
                fname.len(),
                string_lit(fname),
                fname.len(),
                string_lit(fname),
                jfn(json, *ft, false)
            );
        }
        out.push_str("nyrt_jskip(p);\n            nyrt_str_release(k);\n        } while (nyrt_jnext(p, '}'));\n    }\n");
        for (i, (fname, _)) in info.fields.iter().enumerate() {
            let _ = writeln!(out, "    if (!seen[{i}]) nyrt_jmissing(p, {});", string_lit(fname));
        }
        let _ = writeln!(out, "    *({n} *)out = v;\n}}");
    }
    out.push('\n');
}

/// The C name of a function: `ny_<name>` for the program's own, `nyM_<module>_<name>` for the
/// standard library's functions written in Nyra (named `module.name`).
fn fn_name(name: &str) -> String {
    match name.split_once('.') {
        Some((m, n)) => format!("nyM_{m}_{n}"),
        None => format!("ny_{name}"),
    }
}

/// `names` are the plain names; an `inout` parameter is a pointer.
fn signature(f: &Func, names: &[String]) -> String {
    let params = if f.params == 0 {
        "void".to_string()
    } else {
        (0..f.params)
            .map(|i| {
                let star = if f.locals[i].inout { "*" } else { "" };
                format!("{}{star} {}", ctype(f.locals[i].ty), names[i])
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    format!("static {} {}({params})", f.ret.map_or("void".to_string(), ctype), fn_name(&f.name))
}

/// `file` is the source path as given to nyra; runtime errors report it.
pub fn gen(m: &Module, file: &str) -> String {
    let mut out = PRELUDE.replace("@FILE@", &string_lit(file));
    out.push_str(STRINGS);
    out.push('\n');
    out.push_str(ARRAYS);
    out.push('\n');
    out.push_str(MAPS);
    out.push('\n');
    let std = m.uses_std();
    if std {
        out.push_str(STD);
        out.push('\n');
    }
    structs(m, &mut out);
    let json = json_types(m);
    if m.uses_json() {
        out.push_str(JSON);
        out.push('\n');
        json_funcs(m, &json, &mut out);
    }
    // string literals: read-only objects that are never freed (reference count 0); `const` also
    // lets the C compiler see that releasing one never reaches free()
    for (i, s) in m.strs.iter().enumerate() {
        let _ = writeln!(
            out,
            "static const nyrt_str nyL_{i} = {{0, {}, {}, {}, (char*){}}};",
            s.len(),
            s.len(),
            s.chars().count(),
            string_lit(s)
        );
    }
    if !m.strs.is_empty() {
        out.push('\n');
    }
    let names: Vec<Vec<String>> = m.funcs.iter().map(|f| names::locals(f, var, "nyrt_")).collect();
    for (f, n) in m.funcs.iter().zip(&names) {
        let _ = writeln!(out, "{};", signature(f, n));
    }
    out.push('\n');
    for (f, n) in m.funcs.iter().zip(&names) {
        let _ = writeln!(out, "{} {{", signature(f, n));
        // every local is declared at the top; statements only assign
        for (i, l) in f.locals.iter().enumerate().skip(f.params) {
            let init = match l.ty {
                Ty::Struct(_) => " = {0}",
                t if m.managed(t) => " = NULL",
                _ => "",
            };
            let _ = writeln!(out, "    {} {}{init};", ctype(l.ty), n[i]);
        }
        // an `inout` parameter is used through its pointer: `(*p)`
        let uses: Vec<String> =
            n.iter().enumerate().map(|(i, x)| if i < f.params && f.locals[i].inout { format!("(*{x})") } else { x.clone() }).collect();
        let mut g = Gen {
            m,
            f,
            names: &uses,
            out: String::new(),
            indent: 1,
            tmp: 0,
            moving: false,
            unique: Vec::new(),
            lens: Vec::new(),
            json: &json,
        };
        g.stmts(&f.body);
        out.push_str(&g.out);
        out.push_str("}\n\n");
    }
    // the standard library reads the program's arguments
    let (params, args) = if std { ("int argc, char **argv", "\n    nyrt_argc = argc;\n    nyrt_argv = argv;") } else { ("void", "") };
    let _ = write!(
        out,
        "int main({params}) {{{args}\n    nyrt_init();\n    {}();\n    nyrt_leak_check();\n    return 0;\n}}\n",
        fn_name(&m.func(m.main).name)
    );
    out
}

struct Gen<'a> {
    m: &'a Module,
    f: &'a Func,
    names: &'a [String],
    out: String,
    indent: usize,
    /// Counter for helper names (loop cursors, element pointers).
    tmp: usize,
    /// The statement being generated gives its value away (`push`/store of a temporary that is
    /// released right after): no retain, and the release is skipped.
    moving: bool,
    /// Arrays that an enclosing loop made unique before it started (see `hoist`).
    unique: Vec<LocalId>,
    /// Arrays whose length an enclosing loop cannot change, with the C local holding it.
    lens: Vec<(LocalId, String)>,
    /// The arrays and structs that are written as or read from JSON (see `json_types`).
    json: &'a [Ty],
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
            if self.format_append(&ss[k..]) {
                k += 3;
                continue;
            }
            if self.moves(&ss[k..]) {
                self.moving = true;
                self.stmt(&ss[k]);
                self.moving = false;
                k += 2;
                continue;
            }
            self.stmt(&ss[k]);
            k += 1;
        }
    }

    /// `s += "{x}"` (also `s += str(x)`): the parts go straight into `s` when it has no other
    /// owner, instead of into a new string that is appended and freed. The IR is
    /// `t = format(..); s += t; drop t`, with parts that do not read `s`.
    fn format_append(&mut self, ss: &[Stmt]) -> bool {
        let [a, b, c, ..] = ss else { return false };
        let StmtKind::Op { dst: Some(t), op: RtOp::Format, args: parts } = &a.kind else { return false };
        let StmtKind::Mutate { dst: None, op: RtOp::StrAppend, place, args } = &b.kind else { return false };
        if !place.path.is_empty() || !matches!(args.as_slice(), [Expr::Local(x)] if x == t) || place.root == *t {
            return false;
        }
        if !matches!(c.kind, StmtKind::Drop(d) if d == *t) || parts.iter().any(|p| mentions(p, place.root)) {
            return false;
        }
        let s = self.local(place.root).to_string();
        self.line("{");
        self.indent += 1;
        self.line(&format!("nyrt_buf nyrt_b = nyrt_buf_on(&{s});"));
        for p in parts {
            let line = self.put("nyrt_buf", p);
            self.line(&line);
        }
        self.line(&format!("nyrt_buf_back(&nyrt_b, &{s});"));
        self.indent -= 1;
        self.line("}");
        true
    }

    /// True for `xs.push(t)` or `xs[i] = t` of a temporary `t` followed by `drop t`: the value
    /// moves into the array instead of getting one more owner and losing one.
    fn moves(&self, ss: &[Stmt]) -> bool {
        let [a, b, ..] = ss else { return false };
        let (place, value) = match &a.kind {
            StmtKind::Mutate { dst: None, op: RtOp::ArrPush, place, args } if args.len() == 1 => (place, &args[0]),
            StmtKind::Store { place, value } if !place.path.is_empty() => (place, value),
            _ => return false,
        };
        let Expr::Local(t) = value else { return false };
        self.f.local(*t).name.is_none()
            && place.root != *t
            && matches!(b.kind, StmtKind::Drop(d) if d == *t)
            && !place.path.iter().any(|s| matches!(s, Step::Index(i, _) if mentions(i, *t)))
    }

    /// `NYRT_ELEMS(T, xs)[nyrt_ix(xs, i, line, col)]`: element `i` of the array `xs` (an
    /// expression without effects), its index checked (E0240). `root`: the local `xs` is, if
    /// it is one (a loop may hold its length in a C local).
    fn elem(&self, xs: &str, root: Option<LocalId>, elem: Ty, i: &Expr, span: Span) -> String {
        let (i, line, col) = (self.arg(i), span.line, span.col);
        let ix = match root.and_then(|r| self.cached_len(r)) {
            Some(n) => format!("nyrt_ixn({xs}, {i}, {n}, {line}, {col})"),
            None => format!("nyrt_ix({xs}, {i}, {line}, {col})"),
        };
        format!("NYRT_ELEMS({}, {xs})[{ix}]", ctype(elem))
    }

    /// A pointer to element `i` of the array at the lvalue `xs`: returns `(*p)`.
    fn elem_slot(&mut self, xs: &str, root: Option<LocalId>, elem: Ty, i: &Expr, span: Span) -> String {
        let p = self.fresh("p");
        let line = format!("{} *{p} = &{};", ctype(elem), self.elem(xs, root, elem, i, span));
        self.line(&line);
        format!("(*{p})")
    }

    fn cached_len(&self, l: LocalId) -> Option<&str> {
        self.lens.iter().rev().find(|(x, _)| *x == l).map(|(_, n)| n.as_str())
    }

    /// Before a loop: every array local that the loop only indexes, measures and writes
    /// elements of (no new value, no other owner, no call that sees it) is made unique once,
    /// before the loop, instead of before every write (when the loop writes it), and its length
    /// is read once (when the loop cannot change it). Copy on write still happens: at most once,
    /// on the way into the loop. Returns how many entries to drop afterwards, and whether a C
    /// block was opened.
    fn hoist(&mut self, s: &Stmt) -> (usize, usize, bool) {
        let (u0, l0) = (self.unique.len(), self.lens.len());
        let mut opened = false;
        for (k, local) in self.f.locals.iter().enumerate() {
            let x = LocalId(k as u32);
            if !matches!(local.ty, Ty::Array(_)) {
                continue;
            }
            let mut u = ArrUse::default();
            if !stmt_ok(s, x, &mut u) || !u.mentioned {
                continue;
            }
            let lv = self.local(x).to_string();
            let unique = u.writes && !self.unique.contains(&x);
            let len = !u.resizes && self.cached_len(x).is_none();
            if (unique || len) && !opened {
                self.line("{");
                self.indent += 1;
                opened = true;
            }
            if unique {
                self.line(&format!("nyrt_arr_mut(&{lv});"));
                self.unique.push(x);
            }
            if len {
                let n = self.fresh("n");
                self.line(&format!("int64_t {n} = {lv}->len;"));
                self.lens.push((x, n));
            }
        }
        (self.unique.len() - u0, self.lens.len() - l0, opened)
    }

    fn unhoist(&mut self, (u, l, opened): (usize, usize, bool)) {
        self.unique.truncate(self.unique.len() - u);
        self.lens.truncate(self.lens.len() - l);
        if opened {
            self.indent -= 1;
            self.line("}");
        }
    }

    fn local(&self, l: LocalId) -> &str {
        &self.names[l.0 as usize]
    }

    fn ty(&self, e: &Expr) -> Ty {
        e.ty(self.f)
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

    /// The address of a value of type `t`: `&x` for a local, a compound literal otherwise.
    fn addr(&self, e: &Expr, t: Ty) -> String {
        match e {
            Expr::Local(l) => format!("&{}", self.local(*l)),
            // a struct value initializes an element of a one-element array (`(T){v}` would
            // initialize the struct's first field with it)
            _ if matches!(t, Ty::Struct(_)) => format!("&({}[1]){{{}}}[0]", ctype(t), self.arg(e)),
            _ => format!("&({}){{{}}}", ctype(t), self.arg(e)),
        }
    }

    /// One more owner for a managed value (a struct through its address).
    fn retain_value(&self, t: Ty, e: &Expr) -> String {
        match t {
            Ty::Struct(_) => format!("{}_retain({});", ctype(t), self.addr(e, t)),
            _ => retain(t, &self.arg(e)),
        }
    }

    fn fresh(&mut self, prefix: &str) -> String {
        self.tmp += 1;
        format!("nyrt_{prefix}{}", self.tmp)
    }

    fn stmt(&mut self, s: &Stmt) {
        let at = format!("{}, {}", s.span.line, s.span.col);
        match &s.kind {
            StmtKind::Set(l, e) => {
                let line = format!("{} = {};", self.local(*l), bare(&self.expr(e)));
                self.line(&line);
            }
            StmtKind::Call { dst, func, args } => {
                // an `inout` argument passes a pointer to the place (every array on the way unique)
                let inout = args.iter().any(|a| matches!(a, Arg::InOut(_)));
                if inout {
                    self.line("{");
                    self.indent += 1;
                }
                let mut parts = Vec::with_capacity(args.len());
                for a in args {
                    match a {
                        Arg::Val(e) => parts.push(self.arg(e)),
                        Arg::InOut(p) => {
                            let lv = self.place(p);
                            parts.push(format!("&{lv}"));
                        }
                    }
                }
                let line = self.assign(*dst, format!("{}({})", fn_name(&self.m.func(*func).name), parts.join(", ")));
                self.line(&line);
                if inout {
                    self.indent -= 1;
                    self.line("}");
                }
            }
            StmtKind::Op { dst, op, args } => self.op(*dst, *op, args, &at, s.span),
            StmtKind::Store { place, value } => {
                self.line("{");
                self.indent += 1;
                let (parent, last) = self.place_parent(place);
                let t = self.ty(value);
                let root = if place.path.len() == 1 { Some(place.root) } else { None };
                let slot = match last {
                    Some((i, span)) => self.elem_slot(&parent, root, t, &i, span),
                    None => parent,
                };
                let v = self.arg(value);
                if self.m.managed(t) {
                    // the new value gets its owner before the old one loses its own (`xs[0] = xs[0]`)
                    if !self.moving {
                        let line = self.retain_value(t, value);
                        self.line(&line);
                    }
                    self.line(&release(t, &slot));
                }
                self.line(&format!("{slot} = {v};"));
                self.indent -= 1;
                self.line("}");
            }
            StmtKind::Mutate { dst, op, place, args } => self.mutate(*dst, *op, place, args, &at),
            StmtKind::If { .. } => self.if_chain(s),
            StmtKind::Loop { head, cond, body, step } => {
                let h = self.hoist(s);
                self.lp(head, cond, body, step);
                self.unhoist(h);
            }
            StmtKind::ForEach { var, iter, body } => {
                let h = self.hoist(s);
                let it = self.expr(iter);
                let v = self.local(*var).to_string();
                match self.ty(iter) {
                    Ty::Str => {
                        let (p, adv) = (self.fresh("p"), self.fresh("adv"));
                        self.line(&format!("for (int64_t {p} = 0; {p} < {it}->len; ) {{"));
                        self.indent += 1;
                        self.line(&format!("int64_t {adv};"));
                        self.line(&format!("{v} = nyrt_utf8_decode({it}->data + {p}, &{adv});"));
                        self.line(&format!("{p} += {adv};"));
                    }
                    t => {
                        let elem = t.elem().expect("verified: an array");
                        let i = self.fresh("i");
                        self.line(&format!("for (int64_t {i} = 0; {i} < {it}->len; {i}++) {{"));
                        self.indent += 1;
                        self.line(&format!("{v} = (({}*){it}->data)[{i}];", ctype(elem)));
                    }
                }
                self.stmts(body);
                self.indent -= 1;
                self.line("}");
                self.unhoist(h);
            }
            StmtKind::Break => self.line("break;"),
            StmtKind::Continue => self.line("continue;"),
            StmtKind::Return(None) => self.line("return;"),
            StmtKind::Return(Some(e)) => {
                let line = format!("return {};", bare(&self.expr(e)));
                self.line(&line);
            }
            StmtKind::Dup(l) => {
                let line = retain(self.f.local(*l).ty, self.local(*l));
                self.line(&line);
            }
            StmtKind::Drop(l) => {
                let line = release(self.f.local(*l).ty, self.local(*l));
                self.line(&line);
            }
            StmtKind::Keep(l) => {
                let t = self.f.local(*l).ty;
                let x = self.local(*l).to_string();
                let line = match t {
                    Ty::Struct(_) => format!("{}_keep(&{x});", ctype(t)),
                    _ => format!("nyrt_{}_keep({x});", rt_name(t)),
                };
                self.line(&line);
            }
            StmtKind::Free(l) => {
                let x = self.local(*l).to_string();
                let t = self.f.local(*l).ty;
                self.line(&release(t, &x));
                let empty = if matches!(t, Ty::Struct(_)) { format!("({}){{0}}", ctype(t)) } else { "NULL".to_string() };
                self.line(&format!("{x} = {empty};"));
            }
        }
    }

    /// Emits what makes every array above the last step of `p` unique and checks its indexes.
    /// Returns a C lvalue for the value that holds the last step, and the last index (`None`
    /// when the place is the whole local).
    fn place_parent(&mut self, p: &Place) -> (String, Option<(Expr, crate::ast::Span)>) {
        let mut lv = self.local(p.root).to_string();
        let mut t = self.f.local(p.root).ty;
        let n = p.path.len();
        for (k, step) in p.path.iter().enumerate() {
            match step {
                Step::Index(i, span) => {
                    // this array is about to change below this point (unless a loop made the
                    // local unique already)
                    if !(k == 0 && self.unique.contains(&p.root)) {
                        self.line(&format!("nyrt_arr_mut(&{lv});"));
                    }
                    if k + 1 == n {
                        return (lv, Some((i.clone(), *span)));
                    }
                    let elem = t.elem().expect("verified: an array");
                    let root = if k == 0 { Some(p.root) } else { None };
                    lv = self.elem_slot(&lv, root, elem, i, *span);
                    t = elem;
                }
                Step::Field(k) => {
                    let info = self.m.structs.get(t).expect("verified: a struct");
                    lv = format!("{lv}.{}", field(info, *k as usize));
                    t = info.fields[*k as usize].1;
                }
            }
        }
        (lv, None)
    }

    /// An lvalue for the whole place (the last element too), every array on the way unique.
    fn place(&mut self, p: &Place) -> String {
        let (parent, last) = self.place_parent(p);
        match last {
            None => parent,
            Some((i, span)) => {
                let t = self.place_ty(p);
                let root = if p.path.len() == 1 { Some(p.root) } else { None };
                self.elem_slot(&parent, root, t, &i, span)
            }
        }
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

    fn mutate(&mut self, dst: Option<LocalId>, op: RtOp, place: &Place, args: &[Expr], at: &str) {
        self.line("{");
        self.indent += 1;
        let t = self.place_ty(place);
        let elem = t.elem();
        if op == RtOp::ArrPush {
            // the value first: it may read the array's length
            let e = elem.expect("verified: an array");
            let line = format!("{} nyrt_v = {};", ctype(e), self.arg(&args[0]));
            self.line(&line);
        }
        let lv = self.place(place);
        let a: Vec<String> = args.iter().map(|x| self.arg(x)).collect();
        let d = dst.map(|d| self.local(d).to_string());
        let hoisted = place.path.is_empty() && self.unique.contains(&place.root);
        if elem.is_some() && op != RtOp::ArrAppend && !hoisted {
            self.line(&format!("nyrt_arr_mut(&{lv});"));
        }
        let call = match op {
            RtOp::StrAppend => format!("nyrt_str_append(&{lv}, {});", a[0]),
            RtOp::ArrAppend => format!("nyrt_arr_append(&{lv}, {});", a[0]),
            RtOp::ArrPush => {
                // in place while there is room; the new element gets its owner from the array
                let e = elem.expect("verified: an array");
                let slot = format!("NYRT_ELEMS({}, {lv})[{lv}->len]", ctype(e));
                self.line(&format!("if (NYRT_UNLIKELY({lv}->len == {lv}->cap)) nyrt_arr_grow(&{lv}, 1);"));
                self.line(&format!("{slot} = nyrt_v;"));
                if self.m.managed(e) && !self.moving {
                    self.line(&retain(e, &slot));
                }
                format!("{lv}->len++;")
            }
            RtOp::ArrPop if d.is_some() => {
                let e = elem.expect("verified: an array");
                self.line(&format!("if (NYRT_UNLIKELY({lv}->len == 0)) nyrt_pop_empty({at});"));
                format!("{} = NYRT_ELEMS({}, {lv})[--{lv}->len];", d.as_deref().unwrap_or_default(), ctype(e))
            }
            RtOp::ArrSwap => {
                let e = elem.expect("verified: an array");
                let ct = ctype(e);
                self.line(&format!("int64_t nyrt_i = nyrt_ix({lv}, {}, {at}), nyrt_j = nyrt_ix({lv}, {}, {at});", a[0], a[1]));
                self.line(&format!("{ct} nyrt_x = NYRT_ELEMS({ct}, {lv})[nyrt_i];"));
                self.line(&format!("NYRT_ELEMS({ct}, {lv})[nyrt_i] = NYRT_ELEMS({ct}, {lv})[nyrt_j];"));
                format!("NYRT_ELEMS({ct}, {lv})[nyrt_j] = nyrt_x;")
            }
            RtOp::ArrInsert => {
                format!("nyrt_arr_insert(&{lv}, {}, {}, {at});", a[0], self.addr(&args[1], elem.expect("an array")))
            }
            RtOp::ArrPop | RtOp::ArrRemove => {
                // the element moves out into `dst` (or into a scratch value when it is unused)
                let out = match &d {
                    Some(d) => format!("&{d}"),
                    None => format!("&({}){{0}}", ctype(elem.expect("an array"))),
                };
                if op == RtOp::ArrPop {
                    format!("nyrt_arr_pop(&{lv}, {out}, {at});")
                } else {
                    format!("nyrt_arr_remove(&{lv}, {}, {out}, {at});", a[0])
                }
            }
            RtOp::ArrSort => format!("nyrt_arr_sort(&{lv});"),
            RtOp::ArrSortBy => format!("nyrt_arr_sort_by(&{lv}, {});", a[0]),
            RtOp::MapSet | RtOp::MapRemove => {
                let (k, v) = t.map_kv().expect("verified: a map");
                if op == RtOp::MapSet {
                    format!("nyrt_map_set(&{lv}, {}, {});", self.addr(&args[0], k), self.addr(&args[1], v))
                } else {
                    format!("nyrt_map_remove(&{lv}, {});", self.addr(&args[0], k))
                }
            }
            RtOp::ArrReverse => format!("nyrt_arr_reverse(&{lv});"),
            other => unreachable!("{} does not change a place", other.name()),
        };
        self.line(&call);
        self.indent -= 1;
        self.line("}");
    }

    fn op(&mut self, dst: Option<LocalId>, op: RtOp, args: &[Expr], at: &str, span: Span) {
        let a: Vec<String> = args.iter().map(|x| self.arg(x)).collect();
        let call = match op {
            RtOp::Print => {
                // each part straight to stdout: no temporary string
                for p in args {
                    let line = self.put("nyrt_put", p);
                    self.line(&line);
                }
                self.line("putchar('\\n');");
                return;
            }
            // `str(n)`, `str(c)`: small ints and ASCII chars are immortal strings, made once
            RtOp::Format if dst.is_some() && args.len() == 1 && matches!(self.ty(&args[0]), Ty::Int | Ty::Char) => {
                let f = if self.ty(&args[0]) == Ty::Int { "nyrt_int_str" } else { "nyrt_char_str" };
                format!("{f}({})", a[0])
            }
            RtOp::PrintNoLine => {
                for p in args {
                    let line = self.put("nyrt_put", p);
                    self.line(&line);
                }
                return;
            }
            RtOp::Format => {
                let d = dst.map(|d| self.local(d).to_string()).unwrap_or_default();
                self.line("{");
                self.indent += 1;
                self.line("nyrt_buf nyrt_b = nyrt_buf_new();");
                for p in args {
                    let line = self.put("nyrt_buf", p);
                    self.line(&line);
                }
                self.line(&format!("{d} = nyrt_buf_done(&nyrt_b);"));
                self.indent -= 1;
                self.line("}");
                return;
            }
            RtOp::ArrNew => {
                let d = dst.map(|d| self.local(d).to_string()).expect("verified: a destination");
                let t = self.f.local(dst.expect("checked")).ty;
                let elem = t.elem().expect("verified: an array");
                self.line(&format!("{d} = nyrt_arr_new({}, {});", desc(elem), args.len()));
                for (k, x) in args.iter().enumerate() {
                    // the array becomes one more owner of each element
                    let slot = format!("NYRT_ELEMS({}, {d})[{k}]", ctype(elem));
                    let line = format!("{slot} = {};", self.arg(x));
                    self.line(&line);
                    if self.m.managed(elem) {
                        self.line(&retain(elem, &slot));
                    }
                }
                if !args.is_empty() {
                    self.line(&format!("{d}->len = {};", args.len()));
                }
                return;
            }
            RtOp::MapNew => {
                let d = dst.map(|d| self.local(d).to_string()).expect("verified: a destination");
                let t = self.f.local(dst.expect("checked")).ty;
                let (k, v) = t.map_kv().expect("verified: a map");
                self.line(&format!("{d} = nyrt_map_new({}, {});", desc(k), desc(v)));
                for pair in args.chunks(2) {
                    let line = format!("nyrt_map_set(&{d}, {}, {});", self.addr(&pair[0], k), self.addr(&pair[1], v));
                    self.line(&line);
                }
                return;
            }
            RtOp::StructNew => {
                let d = dst.map(|d| self.local(d).to_string()).expect("verified: a destination");
                let t = self.f.local(dst.expect("checked")).ty;
                let fields = if a.is_empty() { "0".to_string() } else { a.join(", ") };
                self.line(&format!("{d} = ({}){{{fields}}};", ctype(t)));
                if self.m.managed(t) {
                    // the struct becomes one more owner of each field value
                    self.line(&retain(t, &d));
                }
                return;
            }
            RtOp::ArrGet => {
                let elem = self.ty(&args[0]).elem().expect("verified: an array");
                let root = if let Expr::Local(l) = args[0] { Some(l) } else { None };
                self.elem(&a[0], root, elem, &args[1], span)
            }
            RtOp::MapGet | RtOp::MapGetOr => {
                let (k, v) = self.ty(&args[0]).map_kv().expect("verified: a map");
                if op == RtOp::MapGet {
                    format!("*({}*)nyrt_map_at({}, {}, {at})", ctype(v), a[0], self.addr(&args[1], k))
                } else {
                    format!("*({}*)nyrt_map_or({}, {}, {})", ctype(v), a[0], self.addr(&args[1], k), self.addr(&args[2], v))
                }
            }
            RtOp::MapKeys => format!("nyrt_map_list({}, false)", a[0]),
            RtOp::MapValues => format!("nyrt_map_list({}, true)", a[0]),
            RtOp::DivInt => format!("nyrt_div({}, {}, {at})", a[0], a[1]),
            RtOp::AddInt => format!("nyrt_add({}, {}, {at})", a[0], a[1]),
            RtOp::SubInt => format!("nyrt_sub({}, {}, {at})", a[0], a[1]),
            RtOp::MulInt => format!("nyrt_mul({}, {}, {at})", a[0], a[1]),
            RtOp::NegInt => format!("nyrt_neg({}, {at})", a[0]),
            RtOp::RemInt => format!("nyrt_mod({}, {}, {at})", a[0], a[1]),
            RtOp::FloatToInt => format!("nyrt_f2i({}, {at})", a[0]),
            RtOp::StrConcat => format!("nyrt_str_concat({}, {})", a[0], a[1]),
            RtOp::StrAt => format!("nyrt_str_at({}, {}, {at})", a[0], a[1]),
            RtOp::StrSlice => format!("nyrt_str_slice({}, {}, {}, {at})", a[0], a[1], a[2]),
            RtOp::StrReplace => format!("nyrt_str_replace({}, {}, {}, {at})", a[0], a[1], a[2]),
            RtOp::StrTrim => format!("nyrt_str_trim({})", a[0]),
            RtOp::StrUpper => format!("nyrt_str_upper({})", a[0]),
            RtOp::StrLower => format!("nyrt_str_lower({})", a[0]),
            RtOp::StrRepeat => format!("nyrt_str_repeat({}, {}, {at})", a[0], a[1]),
            RtOp::StrToInt => format!("nyrt_str_to_int({}, {at})", a[0]),
            RtOp::StrToFloat => format!("nyrt_str_to_float({}, {at})", a[0]),
            RtOp::CharFrom => format!("nyrt_char_from({}, {at})", a[0]),
            RtOp::StrChars => format!("nyrt_str_chars({})", a[0]),
            RtOp::StrCodes => format!("nyrt_str_codes({})", a[0]),
            RtOp::StrSplit => format!("nyrt_str_split({}, {}, {at})", a[0], a[1]),
            RtOp::CheckStep => format!("nyrt_check_step({}, {at})", a[0]),
            RtOp::CheckSome => format!("nyrt_check_some({}, {at})", a[0]),
            RtOp::CheckNonEmpty => format!("nyrt_check_non_empty({}, {}, {at})", a[0], a[1]),
            RtOp::StrPadLeft => format!("nyrt_str_pad({}, {}, {}, true)", a[0], a[1], a[2]),
            RtOp::StrPadRight => format!("nyrt_str_pad({}, {}, {}, false)", a[0], a[1], a[2]),
            RtOp::ArrSlice => format!("nyrt_arr_slice({}, {}, {}, {at})", a[0], a[1], a[2]),
            RtOp::ArrRepeat => format!("nyrt_arr_repeat({}, {}, {at})", a[0], a[1]),
            RtOp::ArrConcat => format!("nyrt_arr_concat({}, {})", a[0], a[1]),
            RtOp::ArrJoin => format!("nyrt_arr_join({}, {})", a[0], a[1]),
            RtOp::Std(f) => {
                let mut parts = a.clone();
                parts.push(at.to_string());
                format!("nyrt_std_{}({})", f.rt_name(), parts.join(", "))
            }
            RtOp::JsonStr => {
                let d = self.local(dst.expect("verified: a destination")).to_string();
                let t = self.ty(&args[0]);
                let line = format!("{}(&nyrt_b, {});", jfn(self.json, t, true), self.addr(&args[0], t));
                self.line("{");
                self.indent += 1;
                self.line("nyrt_buf nyrt_b = nyrt_buf_new();");
                self.line(&line);
                self.line(&format!("{d} = nyrt_buf_done(&nyrt_b);"));
                self.indent -= 1;
                self.line("}");
                return;
            }
            RtOp::JsonParse => {
                let d = dst.expect("verified: a destination");
                let t = self.f.local(d).ty;
                let line = format!("nyrt_jparse({}, {}, &{}, {at});", a[0], jfn(self.json, t, false), self.local(d));
                self.line(&line);
                return;
            }
            other => unreachable!("{} is a `Mutate`", other.name()),
        };
        let line = self.assign(dst, call);
        self.line(&line);
    }

    /// `nyrt_put_int(x);` / `nyrt_buf_int(&b, x);` for one part of a print or format.
    fn put(&self, prefix: &str, p: &Expr) -> String {
        // the buffer is `nyrt_b`: user names never start with `ny`, so it cannot hide one
        let target = if prefix == "nyrt_buf" { "&nyrt_b, " } else { "" };
        if let Expr::Str(id) = p {
            let s = self.m.str(*id);
            return format!("{prefix}_lit({target}{}, {});", string_lit(s), s.len());
        }
        let t = self.ty(p);
        if let Ty::Struct(_) = t {
            let a = self.addr(p, t);
            return if prefix == "nyrt_buf" {
                format!("{}_fmt(&nyrt_b, {a});", ctype(t))
            } else {
                format!("nyrt_put_fmt({}_fmt, {a});", ctype(t))
            };
        }
        format!("{prefix}_{}({target}{});", rt_name(t), bare(&self.expr(p)))
    }

    fn if_chain(&mut self, s: &Stmt) {
        let mut cur = s;
        let mut head = "if";
        loop {
            let StmtKind::If { cond, then, els } = &cur.kind else { unreachable!() };
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

    fn expr(&self, e: &Expr) -> String {
        match e {
            Expr::Int(i64::MIN) => "INT64_MIN".to_string(),
            Expr::Int(n) => {
                if *n > i32::MAX as i64 || *n < i32::MIN as i64 {
                    format!("INT64_C({n})")
                } else {
                    n.to_string()
                }
            }
            Expr::Float(f) if f.is_infinite() => (if *f > 0.0 { "(1.0 / 0.0)" } else { "(-1.0 / 0.0)" }).to_string(),
            Expr::Float(f) => format!("{f:?}"),
            Expr::Bool(b) => b.to_string(),
            Expr::Char(c) => c.to_string(),
            Expr::Str(id) => format!("((nyrt_str*)&nyL_{})", id.0),
            Expr::Local(l) => self.local(*l).to_string(),
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
                // two small int literals would be computed in C's 32-bit `int`: make one 64-bit
                if let Expr::Int(n) = **ea {
                    if !a.starts_with("INT64") {
                        a = format!("INT64_C({n})");
                    }
                }
                match op {
                    BinOp::SEq => format!("nyrt_str_eq({}, {})", bare(&a), bare(&b)),
                    BinOp::SNe => format!("(!nyrt_str_eq({}, {}))", bare(&a), bare(&b)),
                    BinOp::SLt | BinOp::SLe | BinOp::SGt | BinOp::SGe => {
                        format!("(nyrt_str_cmp({}, {}) {} 0)", bare(&a), bare(&b), op.symbol())
                    }
                    BinOp::DeepEq | BinOp::DeepNe => {
                        let (x, y): (&Expr, &Expr) = (ea, eb);
                        let t = x.ty(self.f);
                        let eq = match t {
                            Ty::Struct(_) => format!("{}_eq({}, {})", ctype(t), self.addr(x, t), self.addr(y, t)),
                            Ty::Map(_) => format!("nyrt_map_eq({}, {})", bare(&a), bare(&b)),
                            _ => format!("nyrt_arr_eq({}, {})", bare(&a), bare(&b)),
                        };
                        if *op == BinOp::DeepEq {
                            eq
                        } else {
                            format!("(!{eq})")
                        }
                    }
                    _ => format!("({a} {} {b})", op.symbol()),
                }
            }
            Expr::Select(c, a, b) => format!("({} ? {} : {})", self.expr(c), self.expr(a), self.expr(b)),
            Expr::IntToFloat(x) => format!("((double)({}))", self.expr(x)),
            Expr::Field(x, k, _) => {
                let info = self.m.structs.get(x.ty(self.f)).expect("verified: a struct");
                format!("{}.{}", self.expr(x), field(info, *k as usize))
            }
            Expr::Pure(p, args) => {
                let a: Vec<String> = args.iter().map(|x| self.arg(x)).collect();
                match p {
                    PureFn::StrLen => format!("{}->nchars", self.expr(&args[0])),
                    PureFn::StrContains => format!("nyrt_str_contains({}, {})", a[0], a[1]),
                    PureFn::StrStartsWith => format!("nyrt_str_starts_with({}, {})", a[0], a[1]),
                    PureFn::StrEndsWith => format!("nyrt_str_ends_with({}, {})", a[0], a[1]),
                    PureFn::StrIndexOf => format!("nyrt_str_index_of({}, {})", a[0], a[1]),
                    PureFn::CharCode => format!("((int64_t)({}))", a[0]),
                    PureFn::CharUpper => format!("nyrt_char_upper({})", a[0]),
                    PureFn::CharLower => format!("nyrt_char_lower({})", a[0]),
                    PureFn::CharIsDigit => format!("nyrt_char_is_digit({})", a[0]),
                    PureFn::CharIsLetter => format!("nyrt_char_is_letter({})", a[0]),
                    PureFn::CharIsUpper => format!("nyrt_char_is_upper({})", a[0]),
                    PureFn::CharIsLower => format!("nyrt_char_is_lower({})", a[0]),
                    PureFn::CharIsSpace => format!("nyrt_is_space({})", a[0]),
                    PureFn::ArrLen => match &args[0] {
                        Expr::Local(l) if self.cached_len(*l).is_some() => self.cached_len(*l).unwrap_or_default().to_string(),
                        x => format!("{}->len", self.expr(x)),
                    },
                    PureFn::MapLen => format!("{}->len", self.expr(&args[0])),
                    PureFn::MapHas => {
                        let k = self.ty(&args[0]).map_kv().expect("verified: a map").0;
                        format!("nyrt_map_has({}, {})", a[0], self.addr(&args[1], k))
                    }
                    PureFn::ArrContains | PureFn::ArrIndexOf => {
                        let elem = self.ty(&args[0]).elem().expect("verified: an array");
                        let f = if *p == PureFn::ArrContains { "contains" } else { "index_of" };
                        format!("nyrt_arr_{f}({}, {})", a[0], self.addr(&args[1], elem))
                    }
                }
            }
        }
    }
}

fn string_lit(s: &str) -> String {
    let mut out = String::from("\"");
    for b in s.bytes() {
        match b {
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            b'\n' => out.push_str("\\n"),
            b'\t' => out.push_str("\\t"),
            b'\r' => out.push_str("\\r"),
            b'?' => out.push_str("\\?"),
            0x20..=0x7e => out.push(b as char),
            _ => {
                let _ = write!(out, "\\{b:03o}");
            }
        }
    }
    out.push('"');
    out
}

/// How a loop uses an array local (see `Gen::hoist`).
#[derive(Default)]
struct ArrUse {
    mentioned: bool,
    /// It writes elements (or changes the array in place).
    writes: bool,
    /// It changes the length (`push`, `pop`, `insert`, `remove`, `+=`).
    resizes: bool,
}

/// True if every mention of `x` in `e` is its length or a search in it.
fn expr_ok(e: &Expr, x: LocalId, u: &mut ArrUse) -> bool {
    match e {
        Expr::Local(l) => *l != x,
        Expr::Pure(PureFn::ArrLen | PureFn::ArrContains | PureFn::ArrIndexOf, args) if matches!(args[0], Expr::Local(l) if l == x) => {
            u.mentioned = true;
            args[1..].iter().all(|a| expr_ok(a, x, u))
        }
        Expr::Unary(_, a) | Expr::IntToFloat(a) | Expr::Field(a, _, _) => expr_ok(a, x, u),
        Expr::Binary(_, a, b) => expr_ok(a, x, u) && expr_ok(b, x, u),
        Expr::Select(c, a, b) => expr_ok(c, x, u) && expr_ok(a, x, u) && expr_ok(b, x, u),
        Expr::Pure(_, args) => args.iter().all(|a| expr_ok(a, x, u)),
        Expr::Int(_) | Expr::Float(_) | Expr::Bool(_) | Expr::Char(_) | Expr::Str(_) => true,
    }
}

fn place_ok(p: &Place, x: LocalId, u: &mut ArrUse) -> bool {
    p.path.iter().all(|s| match s {
        Step::Index(i, _) => expr_ok(i, x, u),
        Step::Field(_) => true,
    })
}

/// True if `s` uses the array local `x` only by reading elements, its length or searching it,
/// and by writing elements or changing it in place: never a new value for `x`, another owner
/// of its array, or a call that sees it. `u` collects how.
fn stmt_ok(s: &Stmt, x: LocalId, u: &mut ArrUse) -> bool {
    let all = |ss: &[Stmt], u: &mut ArrUse| ss.iter().all(|s| stmt_ok(s, x, u));
    match &s.kind {
        StmtKind::Set(d, e) => *d != x && expr_ok(e, x, u),
        StmtKind::Call { dst, args, .. } => {
            *dst != Some(x)
                && args.iter().all(|a| match a {
                    Arg::Val(e) => expr_ok(e, x, u),
                    Arg::InOut(p) => p.root != x && place_ok(p, x, u),
                })
        }
        StmtKind::Op { dst, op, args } => {
            *dst != Some(x)
                && args.iter().enumerate().all(|(k, a)| {
                    if k == 0 && *op == RtOp::ArrGet && matches!(a, Expr::Local(l) if *l == x) {
                        u.mentioned = true;
                        true
                    } else {
                        expr_ok(a, x, u)
                    }
                })
        }
        StmtKind::Store { place, value } => {
            if place.root == x {
                u.mentioned = true;
                u.writes = true;
            }
            place_ok(place, x, u) && expr_ok(value, x, u)
        }
        StmtKind::Mutate { dst, op, place, args } => {
            if place.root == x {
                u.mentioned = true;
                u.writes = true;
                let resize = matches!(op, RtOp::ArrPush | RtOp::ArrPop | RtOp::ArrInsert | RtOp::ArrRemove | RtOp::ArrAppend);
                u.resizes |= place.path.is_empty() && resize;
            }
            *dst != Some(x) && place_ok(place, x, u) && args.iter().all(|a| expr_ok(a, x, u))
        }
        StmtKind::If { cond, then, els } => expr_ok(cond, x, u) && all(then, u) && all(els, u),
        StmtKind::Loop { head, cond, body, step } => expr_ok(cond, x, u) && all(head, u) && all(body, u) && all(step, u),
        StmtKind::ForEach { var, iter, body } => *var != x && expr_ok(iter, x, u) && all(body, u),
        StmtKind::Return(Some(e)) => expr_ok(e, x, u),
        StmtKind::Dup(l) | StmtKind::Drop(l) | StmtKind::Free(l) | StmtKind::Keep(l) => *l != x,
        StmtKind::Return(None) | StmtKind::Break | StmtKind::Continue => true,
    }
}
