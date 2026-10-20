//! Typed output and JSON Schemas.
//!
//! **Typed output.** A program whose `fn main() -> T` returns a value prints that value as JSON
//! on standard output: exactly one JSON document and a line break, nothing else. What `print`
//! writes goes to standard error instead, so a caller can read standard output as data and still
//! see the program's messages. The value is written by `json.str`, so it is valid JSON of the
//! shape of T by construction (see `schema`). The interpreter, the native (C) and the JavaScript
//! targets do this; the other targets refuse such a program with a clear message.
//!
//! `wrap_typed_main` adds the entry that does it to the IR: a function that calls `main` and
//! returns the JSON text of its value (`RtOp::JsonStr`), which becomes the module's entry. Each
//! backend's start-up code writes that text, after the program, to the real standard output.
//!
//! **Schemas.** `schema` gives the JSON Schema (draft 2020-12) of the JSON `json.str` writes for
//! a type, or that `json.parse` reads (`Mode::Input`): `int` an integer, `float` a number (written
//! as `null` when it is NaN or infinite), `str` a string, `char` a string of one character, `[T]` an
//! array, `[str: V]` an object, other maps an array of `[key, value]` pairs, tuples arrays of fixed
//! length, `T?` the value or `null`, a struct an object with every field, an enum the name of a
//! variant or `{"Variant": [values]}`. A struct or enum that contains itself is a `$ref` into
//! `$defs`. `nyra schema file.nyra` prints the schema of `main`'s type; `--tools` the MCP tools of
//! `nyra serve --mcp` with theirs.

#[cfg(not(target_arch = "wasm32"))]
use std::process::ExitCode;

use crate::ast::{self, Type};
use crate::ir::{self, Expr, Func, FuncId, Local, LocalId, Module, RtOp, Stmt, StmtKind, Ty};
use crate::json::{obj, Json};

/// The name of the entry `wrap_typed_main` adds.
pub const ENTRY: &str = "__typed_main";

/// The type `main` returns, if it returns one (a script's `main` included, see the parser).
pub fn typed_main(prog: &ast::Program) -> Option<Type> {
    let f = prog.funcs.iter().find(|f| f.name == "main")?;
    (f.ret != Type::Void && f.ret != Type::Unknown).then_some(f.ret)
}

/// When `main` returns a value: a new entry returns its JSON text (see the module comment).
pub fn wrap_typed_main(m: &mut Module) {
    let main = m.func(m.main);
    let Some(t) = main.ret else { return };
    if main.name == ENTRY {
        return;
    }
    let span = main.span;
    let (r, text) = (LocalId(0), LocalId(1));
    let mut body = vec![
        Stmt { kind: StmtKind::Call { dst: Some(r), func: m.main, args: Vec::new() }, span },
        Stmt { kind: StmtKind::Op { dst: Some(text), op: RtOp::JsonStr, args: vec![Expr::Local(r)] }, span },
    ];
    if m.managed(t) {
        body.push(Stmt { kind: StmtKind::Drop(r), span });
    }
    body.push(Stmt { kind: StmtKind::Return(Some(Expr::Local(text))), span });
    m.funcs.push(Func {
        name: ENTRY.to_string(),
        params: 0,
        ret: Some(Ty::Str),
        locals: vec![Local { name: None, ty: t, inout: false }, Local { name: None, ty: Ty::Str, inout: false }],
        body,
        span,
    });
    m.main = FuncId(m.funcs.len() as u32 - 1);
}

// ---- JSON Schema ----------------------------------------------------------------------------------

/// What the schema describes: JSON a program writes (`json.str`, typed output, a tool's result) or
/// JSON it reads (`json.parse`, a tool's arguments). They differ in one place: a float that is NaN
/// or infinite is written as `null`, and `null` is never read as a float.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Input,
    Output,
}

/// Builds schemas of several types that share one `$defs`.
pub struct Gen<'a> {
    m: &'a Module,
    mode: Mode,
    /// The structs and enums that contain themselves: they are written once, in `$defs`.
    recursive: Vec<u32>,
    defs: Vec<(String, Json)>,
    done: Vec<u32>,
}

impl<'a> Gen<'a> {
    pub fn new(m: &'a Module, mode: Mode) -> Gen<'a> {
        let mut recursive = Vec::new();
        for (id, _) in &m.structs.0 {
            if reaches(m, Ty::Struct(*id), *id, &mut Vec::new()) {
                recursive.push(*id);
            }
        }
        Gen { m, mode, recursive, defs: Vec::new(), done: Vec::new() }
    }

    /// The schema of `t`. Its `$defs` (if any) are given by `defs`.
    pub fn schema(&mut self, t: Ty) -> Json {
        let ty = |name: &str| obj([("type", name.into())]);
        match t {
            Ty::Int => ty("integer"),
            Ty::Float if self.mode == Mode::Output => {
                obj([("type", Json::Arr(vec!["number".into(), "null".into()])), ("description", "null when NaN or infinite".into())])
            }
            Ty::Float => ty("number"),
            Ty::Bool => ty("boolean"),
            Ty::Str => ty("string"),
            Ty::Char => obj([("type", "string".into()), ("minLength", Json::from(1)), ("maxLength", Json::from(1))]),
            Ty::Array(_) => {
                let items = self.schema(t.elem().unwrap_or(Ty::Unknown));
                obj([("type", "array".into()), ("items", items)])
            }
            Ty::Map(_) => {
                let (k, v) = t.map_kv().unwrap_or((Ty::Unknown, Ty::Unknown));
                if k == Ty::Str {
                    let v = self.schema(v);
                    obj([("type", "object".into()), ("additionalProperties", v)])
                } else {
                    let pair = vec![self.schema(k), self.schema(v)];
                    obj([("type", "array".into()), ("items", fixed(pair))])
                }
            }
            Ty::Struct(id) => {
                if self.recursive.contains(&id) {
                    let name = self.def_name(id);
                    if !self.done.contains(&id) {
                        self.done.push(id);
                        let body = self.structure(id);
                        self.defs.push((name.clone(), body));
                    }
                    return obj([("$ref", format!("#/$defs/{name}").into())]);
                }
                self.structure(id)
            }
            // (`void` and unknown types have no values: nothing matches)
            Ty::Void | Ty::Unknown => Json::Bool(false),
        }
    }

    fn def_name(&self, id: u32) -> String {
        let name = self.m.structs.get(Ty::Struct(id)).map_or(String::new(), |s| s.name.clone());
        name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).collect()
    }

    /// A struct, tuple, optional or enum, written out.
    fn structure(&mut self, id: u32) -> Json {
        let Some(info) = self.m.structs.get(Ty::Struct(id)) else { return Json::Bool(false) };
        let fields: Vec<(String, Ty)> = info.fields.iter().map(|(n, t)| (n.clone(), *t)).collect();
        if info.tuple {
            let parts = fields.iter().map(|(_, t)| self.schema(*t)).collect();
            return obj([("type", "array".into()), ("prefixItems", Json::Arr(parts))]).merge(fixed_len(fields.len()));
        }
        if info.option {
            let inner = self.schema(fields[1].1);
            return obj([("anyOf", Json::Arr(vec![inner, obj([("type", "null".into())])]))]);
        }
        let name = info.name.clone();
        if !info.variants.is_empty() {
            let (variants, payloads, starts) = (info.variants.clone(), info.payloads.clone(), info.slot_starts());
            let plain: Vec<Json> =
                variants.iter().zip(&payloads).filter(|(_, n)| **n == 0).map(|(v, _)| Json::from(v.as_str())).collect();
            let mut options = Vec::new();
            if !plain.is_empty() {
                options.push(obj([("type", "string".into()), ("enum", Json::Arr(plain))]));
            }
            for (k, v) in variants.iter().enumerate() {
                if payloads[k] == 0 {
                    continue;
                }
                let values: Vec<Json> = (starts[k]..starts[k] + payloads[k]).map(|i| self.schema(fields[i].1)).collect();
                let values = obj([("type", "array".into()), ("prefixItems", Json::Arr(values))]).merge(fixed_len(payloads[k]));
                options.push(obj([
                    ("type", "object".into()),
                    ("properties", Json::Obj(vec![(v.clone(), values)])),
                    ("required", Json::Arr(vec![v.as_str().into()])),
                    ("additionalProperties", false.into()),
                ]));
            }
            if options.len() == 1 {
                let only = options.pop().unwrap_or(Json::Null);
                return obj([("title", name.into())]).merge(only);
            }
            return obj([("title", name.into()), ("oneOf", Json::Arr(options))]);
        }
        let props: Vec<(String, Json)> = fields.iter().map(|(n, t)| (n.clone(), self.schema(*t))).collect();
        let required = fields.iter().map(|(n, _)| Json::from(n.as_str())).collect();
        obj([
            ("title", name.into()),
            ("type", "object".into()),
            ("properties", Json::Obj(props)),
            ("required", Json::Arr(required)),
            ("additionalProperties", false.into()),
        ])
    }

    /// The `$defs` the schemas so far refer to (empty: none).
    pub fn defs(&self) -> Vec<(String, Json)> {
        self.defs.clone()
    }
}

/// True if a value of type `t` can contain a value of the struct `target`.
fn reaches(m: &Module, t: Ty, target: u32, seen: &mut Vec<u32>) -> bool {
    let parts: Vec<Ty> = match t {
        Ty::Array(_) => t.elem().into_iter().collect(),
        Ty::Map(_) => t.map_kv().map(|(k, v)| vec![k, v]).unwrap_or_default(),
        Ty::Struct(id) => {
            if seen.contains(&id) {
                return false;
            }
            seen.push(id);
            m.structs.get(t).map(|s| s.fields.iter().map(|f| f.1).collect()).unwrap_or_default()
        }
        _ => Vec::new(),
    };
    parts.into_iter().any(|p| p == Ty::Struct(target) || reaches(m, p, target, seen))
}

/// `"minItems": n, "maxItems": n, "items": false`
fn fixed_len(n: usize) -> Json {
    obj([("items", false.into()), ("minItems", Json::from(n as i64)), ("maxItems", Json::from(n as i64))])
}

/// An array of exactly these elements.
fn fixed(parts: Vec<Json>) -> Json {
    let n = parts.len();
    obj([("type", "array".into()), ("prefixItems", Json::Arr(parts))]).merge(fixed_len(n))
}

trait Merge {
    fn merge(self, other: Json) -> Json;
}

impl Merge for Json {
    /// The fields of both objects.
    fn merge(self, other: Json) -> Json {
        match (self, other) {
            (Json::Obj(mut a), Json::Obj(b)) => {
                a.extend(b);
                Json::Obj(a)
            }
            (a, _) => a,
        }
    }
}

/// A whole schema document for `t`: `$schema`, the schema, and its `$defs`.
pub fn document(m: &Module, t: Ty, mode: Mode, title: &str) -> Json {
    let mut g = Gen::new(m, mode);
    let body = g.schema(t);
    let mut fields = vec![
        ("$schema".to_string(), Json::from("https://json-schema.org/draft/2020-12/schema")),
        ("title".to_string(), Json::from(title)),
    ];
    match body {
        Json::Obj(fs) => fields.extend(fs.into_iter().filter(|(k, _)| k != "title")),
        other => return other,
    }
    let defs = g.defs();
    if !defs.is_empty() {
        fields.push(("$defs".to_string(), Json::Obj(defs)));
    }
    Json::Obj(fields)
}

/// JSON with two spaces of indentation, for people.
pub fn pretty(j: &Json) -> String {
    fn go(j: &Json, indent: usize, out: &mut String) {
        let pad = "  ".repeat(indent + 1);
        match j {
            Json::Arr(items) if !items.is_empty() && !items.iter().all(|i| matches!(i, Json::Str(_) | Json::Num(_))) => {
                out.push_str("[\n");
                for (i, item) in items.iter().enumerate() {
                    out.push_str(&pad);
                    go(item, indent + 1, out);
                    out.push_str(if i + 1 < items.len() { ",\n" } else { "\n" });
                }
                out.push_str(&"  ".repeat(indent));
                out.push(']');
            }
            Json::Arr(items) => {
                let parts: Vec<String> = items.iter().map(|i| i.to_string()).collect();
                out.push_str(&format!("[{}]", parts.join(", ")));
            }
            Json::Obj(fields) if !fields.is_empty() => {
                out.push_str("{\n");
                for (i, (k, v)) in fields.iter().enumerate() {
                    out.push_str(&pad);
                    out.push_str(&crate::diag::json_str(k));
                    out.push_str(": ");
                    go(v, indent + 1, out);
                    out.push_str(if i + 1 < fields.len() { ",\n" } else { "\n" });
                }
                out.push_str(&"  ".repeat(indent));
                out.push('}');
            }
            other => out.push_str(&other.to_string()),
        }
    }
    let mut out = String::new();
    go(j, 0, &mut out);
    out.push('\n');
    out
}

// ---- nyra schema ----------------------------------------------------------------------------------

#[cfg(not(target_arch = "wasm32"))]
const USAGE: &str = "\
usage: nyra schema <file.nyra> [--compact]
       nyra schema --tools <file.nyra> [--compact]

Prints the JSON Schema (draft 2020-12) of what the program prints: the type of `fn main() -> T`,
whose value the program writes as one JSON document on standard output. With --tools: the MCP
tools that `nyra serve --mcp file.nyra` serves (its `pub fn`s), with the schemas of their
arguments and results (`nyra serve --mcp --list` prints the same).
";

/// Compiles a program for `schema` and `serve` (fixes with one certain repair are made in memory,
/// like `nyra run` does). Errors are printed; `None` then.
#[cfg(not(target_arch = "wasm32"))]
pub fn compile_file(file: &str, extra: &str) -> Result<(String, ast::Program), ExitCode> {
    let src = match std::fs::read_to_string(file) {
        Ok(s) => s,
        Err(e) => return Err(crate::fail(format!("cannot read `{file}`: {e}"))),
    };
    crate::modules::set_main(Some(std::path::Path::new(file)));
    let text = format!("{src}{extra}");
    match crate::compile(&text) {
        Ok(p) => Ok((src, p)),
        Err(diags) => match crate::fix::repair(&text, diags.clone(), crate::compile) {
            Some(r) => Ok((src, r.value)),
            None => {
                eprint!("{}", crate::diag::render_human(&diags, file, &text));
                eprintln!("nyra: {} error(s)", diags.len());
                Err(ExitCode::from(1))
            }
        },
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn cli(args: Vec<String>) -> ExitCode {
    let (mut tools, mut compact, mut file) = (false, false, None);
    for a in args {
        match a.as_str() {
            "-h" | "--help" => {
                print!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            "--tools" => tools = true,
            "--compact" => compact = true,
            _ if a.starts_with('-') => return crate::fail(format!("unknown option `{a}`\n\n{USAGE}")),
            _ if file.is_none() => file = Some(a),
            _ => return crate::fail(format!("`nyra schema` takes one file\n\n{USAGE}")),
        }
    }
    let Some(file) = file else {
        eprint!("{USAGE}");
        return ExitCode::from(2);
    };
    let doc = if tools {
        match crate::serve::Tools::load(&file) {
            Ok(t) => t.manifest(),
            Err(code) => return code,
        }
    } else {
        let prog = match compile_file(&file, "") {
            Ok((_, p)) => p,
            Err(code) => return code,
        };
        let m = match ir::lower::lower(&prog) {
            Ok(m) => m,
            Err(what) => return crate::fail(format!("not supported yet: {what}")),
        };
        let Some(t) = m.func(m.main).ret else {
            return crate::fail(format!(
                "`main` of {file} returns nothing, so the program prints no JSON value: write `fn main() -> T` (its value is printed as JSON), or use `nyra schema --tools` for the schemas of its `pub fn`s"
            ));
        };
        document(&m, t, Mode::Output, &t.name())
    };
    if compact {
        println!("{doc}");
    } else {
        print!("{}", pretty(&doc));
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module(src: &str) -> Module {
        let prog = crate::front(src).unwrap_or_else(|d| panic!("{d:?}"));
        ir::lower::lower(&prog).unwrap()
    }

    #[test]
    fn schemas_follow_what_json_str_writes() {
        let src = "struct P {\n    name: str\n    tags: [str]\n    score: float\n    best: str?,\n    pair: (int, char)\n    counts: [str: int]\n    ids: [int: bool]\n}\nenum Shape {\n    Circle(float)\n    Empty\n}\nfn main() -> [P] {\n    return []\n}\n";
        let m = module(src);
        let p = m.structs.0.iter().find(|(_, s)| s.name == "P").map(|(id, _)| Ty::Struct(*id)).unwrap();
        let text = document(&m, p, Mode::Output, "P").to_string();
        assert!(text.contains(r#""name":{"type":"string"}"#), "{text}");
        assert!(text.contains(r#""tags":{"type":"array","items":{"type":"string"}}"#), "{text}");
        assert!(text.contains(r#""score":{"type":["number","null"]"#), "{text}");
        assert!(text.contains(r#""counts":{"type":"object","additionalProperties":{"type":"integer"}}"#), "{text}");
        assert!(text.contains(r#""best":{"anyOf":[{"type":"string"},{"type":"null"}]}"#), "{text}");
        assert!(text.contains(r#""pair":{"type":"array","prefixItems":[{"type":"integer"},{"type":"string","minLength":1,"maxLength":1}],"items":false,"minItems":2,"maxItems":2}"#), "{text}");
        assert!(
            text.contains(r#""ids":{"type":"array","items":{"type":"array","prefixItems":[{"type":"integer"},{"type":"boolean"}]"#),
            "{text}"
        );
        assert!(text.contains(r#""required":["name","tags","score","best","pair","counts","ids"]"#), "{text}");
        let shape = m.structs.0.iter().find(|(_, s)| s.name == "Shape").map(|(id, _)| Ty::Struct(*id)).unwrap();
        let text = document(&m, shape, Mode::Input, "Shape").to_string();
        assert!(text.contains(r#""oneOf":[{"type":"string","enum":["Empty"]},{"type":"object","properties":{"Circle":{"type":"array","prefixItems":[{"type":"number"}]"#), "{text}");
    }

    #[test]
    fn a_struct_that_contains_itself_is_a_reference() {
        let m = module("struct Node {\n    v: int\n    kids: [Node]\n}\nfn main() -> Node {\n    return Node(v: 1, kids: [])\n}\n");
        let t = m.func(m.main).ret.unwrap();
        let text = document(&m, t, Mode::Output, "Node").to_string();
        assert!(text.contains(r##""$ref":"#/$defs/Node""##), "{text}");
        assert!(text.contains(r#""$defs":{"Node":{"title":"Node","type":"object""#), "{text}");
        assert!(text.contains(r##""kids":{"type":"array","items":{"$ref":"#/$defs/Node"}}"##), "{text}");
    }
}
