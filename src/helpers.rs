//! Helper functions that the compiler writes in Nyra when a program needs them.
//!
//! Nyra has no generics, so an operation that works for many types (comparing two tuples, sorting
//! an array by a tuple key, `zip`, ...) is generated once per type, as ordinary Nyra source with
//! the types filled in. The checker parses the source and adds the function to the program under
//! the name `core.<kind>_<types>` (a name no program can write); lowering emits a call to it.
//! Every backend then runs the helper like any function of the program, so the behavior is the
//! same everywhere and memory is managed the usual way.

use crate::ast::{each_stmt_mut, Func, Span, Type};

/// A helper function: what it does and the types it is made for.
#[derive(Clone, PartialEq, Debug)]
pub enum H {
    /// `a < b`, `a <= b`, `a > b` or `a >= b` on two tuples of the type: lexicographic
    Cmp(&'static str, Type),
    /// `sort_keyed(inout xs: [T], ks: [K])`: sorts `xs` by the keys `ks` (tuples), stable
    SortKeyed(Type, Type),
    /// the sign, thousands separators and padding of a format specifier
    Fmt,
}

impl H {
    /// The program-wide name of the function.
    pub fn name(&self) -> String {
        match self {
            H::Cmp(op, t) => {
                let o = match *op {
                    "<" => "lt",
                    "<=" => "le",
                    ">" => "gt",
                    _ => "ge",
                };
                format!("core.cmp_{o}_{}", t.mangle())
            }
            H::SortKeyed(t, k) => format!("core.sort_{}_{}", t.mangle(), k.mangle()),
            H::Fmt => "core.fmt".to_string(),
        }
    }

    /// The Nyra source of the function, named `__h`.
    fn source(&self) -> String {
        match self {
            H::Cmp(op, t) => cmp_source(op, *t),
            H::SortKeyed(arr, key) => sort_keyed_source(*arr, *key),
            H::Fmt => FMT.to_string(),
        }
    }
}

/// The values of a tuple type that decide an order: `(".0", int)`, `(".1.0", str)`, ...
fn leaves(t: Type, path: &str, out: &mut Vec<(String, Type)>) {
    match t.tuple_elems() {
        Some(es) => es.iter().enumerate().for_each(|(i, e)| leaves(*e, &format!("{path}.{i}"), out)),
        None => out.push((path.to_string(), t)),
    }
}

/// True if values of the type have an order: numbers, text, chars, and bools or tuples of those
/// (inside a tuple).
pub fn orderable(t: Type) -> bool {
    match t.tuple_elems() {
        Some(es) => es.iter().all(|e| orderable(*e)),
        None => matches!(t, Type::Int | Type::Float | Type::Str | Type::Char | Type::Bool),
    }
}

const FMT: &str = r#"fn __h(s: str, plus: bool, comma: bool, width: int, align: char, fill: char, zero: bool) -> str {
    var t = s
    if comma {
        var start = 0
        if t.len() > 0 && (t[0] == '-' || t[0] == '+') { start = 1 }
        var end = start
        while end < t.len() && t[end].is_digit() { end += 1 }
        let count = end - start
        var out = ""
        for i in 0..count {
            if i > 0 && (count - i) % 3 == 0 { out += "," }
            out += str(t[start + i])
        }
        t = t.slice(0, start) + out + t.slice(end, t.len())
    }
    if plus && t.len() > 0 && t[0] != '-' { t = "+" + t }
    let n = t.len()
    if n >= width { ret t }
    if zero {
        var sign = ""
        var rest = t
        if n > 0 && (t[0] == '-' || t[0] == '+') {
            sign = t.slice(0, 1)
            rest = t.slice(1, n)
        }
        ret sign + rest.pad_left(width - sign.len(), '0')
    }
    if align == '<' { ret t.pad_right(width, fill) }
    if align == '^' { ret t.pad_left(n + (width - n) / 2, fill).pad_right(width, fill) }
    ret t.pad_left(width, fill)
}
"#;

fn cmp_source(op: &str, t: Type) -> String {
    let es = t.tuple_elems().unwrap_or_default();
    let tn = t.name();
    let mut s = format!("fn __h(a: {tn}, b: {tn}) -> bool {{\n");
    for i in 0..es.len() {
        let (x, y) = (format!("a.{i}"), format!("b.{i}"));
        let decided = if es[i] == Type::Bool {
            // false < true
            match op {
                "<" | "<=" => format!("!{x} && {y}"),
                _ => format!("{x} && !{y}"),
            }
        } else {
            let strict = if matches!(op, "<" | "<=") { "<" } else { ">" };
            format!("{x} {strict} {y}")
        };
        s += &format!("    if {x} != {y} {{ ret {decided} }}\n");
    }
    s += &format!("    ret {}\n}}\n", if op.contains('=') { "true" } else { "false" });
    s
}

fn sort_keyed_source(arr: Type, key: Type) -> String {
    let mut ls = Vec::new();
    leaves(key, "", &mut ls);
    let mut s = format!("fn __h(inout xs: {}, ks: {}) {{\n    var idx = [i for i in 0..xs.len()]\n", arr.name(), Type::array(key).name());
    // the last part of the key first: every sort is stable, so the first part decides in the end
    for (path, lt) in ls.iter().rev() {
        let k = if *lt == Type::Bool { format!("if ks[j]{path} {{ 1 }} else {{ 0 }}") } else { format!("ks[j]{path}") };
        s += &format!("    idx.sort_by(j => {k})\n");
    }
    s += &format!("    var out: {} = []\n    for j in idx {{ out.push(xs[j]) }}\n    xs = out\n}}\n", arr.name());
    s
}

/// The function for `h`, parsed, named, and with every position set to `at` (a runtime error in
/// it is reported where the program used the operation). `Err` is a bug in the source above.
pub fn build(h: &H, at: Span) -> Result<Func, String> {
    let src = h.source();
    let (toks, errs) = crate::lexer::lex(&src);
    if let Some(e) = errs.first() {
        return Err(format!("helper {} does not lex: {e:?}\n{src}", h.name()));
    }
    let (mut prog, errs) = crate::parser::parse(toks);
    if let Some(e) = errs.first() {
        return Err(format!("helper {} does not parse: {e:?}\n{src}", h.name()));
    }
    let mut f = prog.funcs.pop().ok_or("no function")?;
    f.name = h.name();
    f.span = at;
    each_stmt_mut(&mut f.body, &mut |s| s.span = at, &mut |e| e.span = at);
    for p in &mut f.params {
        p.span = at;
    }
    Ok(f)
}
