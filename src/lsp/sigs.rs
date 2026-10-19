//! Signatures of what a program can call: its own functions, the builtins, the methods of the
//! built-in types and the functions of the standard modules (with `use web`, the web library's
//! functions, which are called by their bare names and take most of their options by name).

use crate::ast::{Func, Type};
use crate::check::data;
use crate::stdlib::{self, StdFn};
use crate::{edit, examples, lexer, parser, webcheck};

/// The words of the language.
pub const KEYWORDS: &[&str] = &[
    "fn", "let", "var", "if", "else", "while", "for", "in", "step", "return", "true", "false", "struct", "enum", "match", "inout",
    "break", "continue", "arena", "none", "use", "pub", "ex",
];

#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub ty: String,
    pub default: Option<String>,
    pub inout: bool,
}

impl Param {
    fn label(&self) -> String {
        let io = if self.inout { "inout " } else { "" };
        match &self.default {
            Some(d) => format!("{io}{}: {} = {d}", self.name, self.ty),
            None if self.ty.is_empty() => format!("{io}{}", self.name),
            None => format!("{io}{}: {}", self.name, self.ty),
        }
    }
}

/// A function, builtin or method as it is called.
#[derive(Debug, Clone)]
pub struct Sig {
    /// As written in a call: `area`, `math.sqrt`, `page`, `push`.
    pub name: String,
    pub params: Vec<Param>,
    /// Empty when it returns nothing.
    pub ret: String,
    /// The comment above its definition, or a short description.
    pub doc: String,
    /// For a method: the type it belongs to (`[int]`, `str`).
    pub recv: Option<String>,
}

impl Sig {
    /// `fn area(w: int, h: int) -> int`; a long one has a parameter per line.
    pub fn label(&self) -> String {
        let ps: Vec<String> = self.params.iter().map(Param::label).collect();
        let ret = if self.ret.is_empty() { String::new() } else { format!(" -> {}", self.ret) };
        let name = match &self.recv {
            Some(r) => format!("{r}.{}", self.name),
            None => self.name.clone(),
        };
        let one = format!("fn {name}({}){ret}", ps.join(", "));
        if one.chars().count() <= 90 {
            return one;
        }
        format!("fn {name}(\n{}\n){ret}", ps.iter().map(|p| format!("    {p},")).collect::<Vec<_>>().join("\n"))
    }

    /// The label on one line, for signature help (whose parameters are found in it by offset).
    pub fn line(&self) -> (String, Vec<(usize, usize)>) {
        let name = match &self.recv {
            Some(r) => format!("{r}.{}", self.name),
            None => self.name.clone(),
        };
        let mut out = format!("{name}(");
        let mut at = Vec::new();
        for (i, p) in self.params.iter().enumerate() {
            if i > 0 {
                out += ", ";
            }
            let start = out.encode_utf16().count();
            out += &p.label();
            at.push((start, out.encode_utf16().count()));
        }
        out += ")";
        if !self.ret.is_empty() {
            out += &format!(" -> {}", self.ret);
        }
        (out, at)
    }
}

/// The signature of a function of the program, called `name`.
pub fn of_func(name: &str, f: &Func, doc: String) -> Sig {
    Sig {
        name: name.to_string(),
        params: f
            .params
            .iter()
            .map(|p| Param {
                name: p.name.clone(),
                ty: p.ty.name(),
                default: p.default.as_ref().map(examples::source),
                inout: p.inout,
            })
            .collect(),
        ret: if f.ret == Type::Void { String::new() } else { f.ret.name() },
        doc,
        recv: None,
    }
}

/// The comment lines right above a definition, without the `//`.
pub fn doc_of(text: &str, item: &edit::Item) -> String {
    text.get(item.doc..item.start)
        .unwrap_or("")
        .lines()
        .map(|l| l.trim().trim_start_matches("//").trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The public functions of a library written in Nyra, with their comments.
fn library(src: &str, prefix: &str) -> Vec<Sig> {
    let (toks, _) = lexer::lex(src);
    let (prog, _) = parser::parse(toks);
    let outline = edit::outline(src);
    prog.funcs
        .iter()
        .filter(|f| !f.name.starts_with('_') && f.name != "main")
        .map(|f| {
            let doc = outline.items.iter().find(|i| i.name == f.name).map(|i| doc_of(src, i)).unwrap_or_default();
            of_func(&format!("{prefix}{}", f.name), f, doc)
        })
        .collect()
}

thread_local! {
    static MATH: Vec<Sig> = library(include_str!("../std/math.nyra"), "math.");
    static WEB: Vec<Sig> = library(webcheck::SOURCE, "");
}

/// The functions of the web library, called by their bare names (`page`, `button`).
pub fn web() -> Vec<Sig> {
    WEB.with(|w| w.clone())
}

/// One function of the web library.
pub fn web_fn(name: &str) -> Option<Sig> {
    WEB.with(|w| w.iter().find(|s| s.name == name).cloned())
}

/// The members of a standard module: its functions (`math.sqrt`) and constants (`math.pi`, with
/// the type `float` and no parameters, marked by a `ret` and a doc of "constant").
pub fn module(m: &str) -> Vec<Sig> {
    let mut out: Vec<Sig> = StdFn::ALL
        .iter()
        .filter(|f| f.path().0 == m && !f.path().1.starts_with('_'))
        .map(|f| Sig {
            name: f.full_name().to_string(),
            params: f
                .params()
                .iter()
                .map(|(n, t)| Param { name: n.to_string(), ty: t.ty().name(), default: None, inout: false })
                .collect(),
            ret: if f.ret() == stdlib::T::Void { String::new() } else { f.ret().ty().name() },
            doc: String::new(),
            recv: None,
        })
        .collect();
    for c in ["pi", "e", "inf"] {
        if stdlib::constant(m, c).is_some() {
            out.push(Sig { name: format!("{m}.{c}"), params: Vec::new(), ret: "float".into(), doc: "constant".into(), recv: None });
        }
    }
    if m == "json" {
        let p = |n: &str, t: &str| Param { name: n.into(), ty: t.into(), default: None, inout: false };
        out.push(Sig {
            name: "json.str".into(),
            params: vec![p("value", "")],
            ret: "str".into(),
            doc: "the value as JSON text (any type)".into(),
            recv: None,
        });
        out.push(Sig {
            name: "json.parse".into(),
            params: vec![p("text", "str")],
            ret: "T".into(),
            doc: "a value of the type the program expects: `let p: Point = json.parse(text)`".into(),
            recv: None,
        });
    }
    if m == "math" {
        out.extend(MATH.with(|v| v.clone()));
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// A function of a standard module by its full name (`math.sqrt`).
pub fn module_fn(full: &str) -> Option<Sig> {
    let (m, _) = full.split_once('.')?;
    if !stdlib::is_module(m) {
        return None;
    }
    if m == "web" {
        return web_fn(full.strip_prefix("web.")?);
    }
    module(m).into_iter().find(|s| s.name == full)
}

fn simple(name: &str, params: &[(&str, &str)], ret: &str, doc: &str) -> Sig {
    Sig {
        name: name.into(),
        params: params.iter().map(|(n, t)| Param { name: n.to_string(), ty: t.to_string(), default: None, inout: false }).collect(),
        ret: ret.into(),
        doc: doc.into(),
        recv: None,
    }
}

/// The functions every program can call without a `use`.
pub fn builtins() -> Vec<Sig> {
    let mut print = simple("print", &[("values", "...")], "", "prints the values joined by a space, then a line break");
    print.params.push(Param { name: "end".into(), ty: "str".into(), default: Some("\"\\n\"".into()), inout: false });
    vec![
        print,
        simple("int", &[("x", "")], "int", "converts to `int`: a float is cut toward zero; `s.to_int()` reads a string"),
        simple("float", &[("x", "")], "float", "converts to `float`"),
        simple("str", &[("x", "")], "str", "the value as text"),
        simple("char", &[("code", "int")], "char", "the character with this code"),
        simple("abs", &[("x", "")], "", "the absolute value of an `int` or a `float`"),
        simple("min", &[("a", ""), ("b", "")], "", "the smaller of two values of the same type"),
        simple("max", &[("a", ""), ("b", "")], "", "the larger of two values of the same type"),
        simple("free", &[("x", "")], "", "frees the memory of an array, string or map now"),
        simple("keep", &[("x", "")], "", "keeps a value made inside an `arena` block after it ends"),
    ]
}

/// The names of the parameters of a method (the types come from the checker).
fn method_params(name: &str, recv: Type) -> &'static [&'static str] {
    let map = matches!(recv, Type::Map(_));
    match name {
        "push" => &["value"],
        "insert" => &["index", "value"],
        "remove" if map => &["key"],
        "remove" => &["index"],
        "swap" => &["i", "j"],
        "slice" => &["start", "end"],
        "contains" | "index_of" => &["x"],
        "starts_with" => &["prefix"],
        "ends_with" => &["suffix"],
        "repeat" => &["times"],
        "join" | "split" => &["sep"],
        "chunks" => &["size"],
        "replace" => &["old", "new"],
        "pad_left" | "pad_right" => &["width"],
        "has" | "get" => &["key"],
        "set" => &["key", "value"],
        "fold" => &["start", "f"],
        "map" | "filter" | "count" | "any" | "all" | "find_index" | "find" | "sort_by" | "sorted_by" | "min_by" | "max_by" => &["f"],
        _ => &[],
    }
}

/// The method `name` of the type `t`.
pub fn method(t: Type, name: &str) -> Option<Sig> {
    let sig = data::method_sig(t, name)?;
    let names = method_params(name, t);
    let lambda = data::LAMBDA_METHODS.contains(&name) && !matches!(name, "sum" | "min" | "max");
    let params = sig
        .params
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let n = names.get(i).copied().unwrap_or("x");
            let ty = if n == "f" && lambda {
                "x => ...".to_string()
            } else if p.is_unknown() {
                String::new()
            } else {
                p.name()
            };
            Param { name: n.to_string(), ty, default: None, inout: false }
        })
        .collect();
    let ret = if sig.ret == Type::Void || sig.ret.is_unknown() { String::new() } else { sig.ret.name() };
    let doc = if sig.mutates { "changes the value it is called on (which must be a `var`)".to_string() } else { String::new() };
    Some(Sig { name: name.to_string(), params, ret, doc, recv: Some(t.name()) })
}

/// Every method of the type `t`.
pub fn methods(t: Type) -> Vec<Sig> {
    data::methods_of(t).iter().filter_map(|m| method(t, m)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knows_the_libraries() {
        let sqrt = module_fn("math.sqrt").unwrap();
        assert_eq!(sqrt.label(), "fn math.sqrt(x: float) -> float");
        assert!(module("math").iter().any(|s| s.name == "math.atan2"));
        assert!(module("math").iter().any(|s| s.name == "math.pi"));
        let page = web_fn("page").unwrap();
        assert!(page.params.iter().any(|p| p.name == "title"));
        assert!(page.params.iter().any(|p| p.default.is_some()));
        let push = method(Type::array(Type::Int), "push").unwrap();
        assert_eq!(push.label(), "fn [int].push(value: int)");
        let (line, at) = push.line();
        assert_eq!(&line[at[0].0..at[0].1], "value: int");
    }
}
