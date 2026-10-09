//! The type checker's knowledge of data: structs, arrays, maps, strings and chars as values with
//! methods, places (what an assignment or a mutating method may change), `inout` arguments,
//! `free`/`keep`/`arena`, and the freed-variable analysis.

use std::collections::HashMap;

use crate::ast::*;

/// A method of a built-in type: parameter types, result type, and whether it changes the receiver.
pub struct MSig {
    pub params: Vec<Type>,
    pub ret: Type,
    pub mutates: bool,
}

pub const ARRAY_METHODS: &[&str] = &[
    "len", "push", "pop", "insert", "remove", "swap", "slice", "contains", "index_of", "repeat", "sort", "reverse", "join", "reversed",
    "map", "filter", "count", "any", "all", "find_index", "sort_by", "fold", "sum", "min", "max",
];
pub const STR_METHODS: &[&str] = &[
    "len", "chars", "codes", "slice", "contains", "starts_with", "ends_with", "index_of", "split", "replace", "trim", "upper",
    "lower", "repeat", "pad_left", "pad_right", "reversed", "count", "any", "all", "find_index",
];
/// The methods that take a lambda, plus `sum`, `min` and `max`, which run the same kind of loop
/// (checked in `check/lambda.rs`, lowered to loops in `ir/lower.rs`).
pub const LAMBDA_METHODS: &[&str] = &["map", "filter", "count", "any", "all", "find_index", "sort_by", "fold", "sum", "min", "max"];
/// The lambda methods of a string: each character is tested.
pub const STR_LAMBDA_METHODS: &[&str] = &["count", "any", "all", "find_index"];
pub const MAP_METHODS: &[&str] = &["len", "has", "get", "set", "remove", "keys", "values"];
pub const CHAR_METHODS: &[&str] = &["code", "upper", "lower", "is_digit", "is_letter", "is_upper", "is_lower", "is_space"];

/// The method `name` of type `t`, if it has one.
pub fn method_sig(t: Type, name: &str) -> Option<MSig> {
    use Type::{Bool, Char, Int, Str, Void};
    let m = |params: Vec<Type>, ret: Type, mutates: bool| Some(MSig { params, ret, mutates });
    match t {
        Type::Array(_) => {
            let e = t.elem()?;
            match name {
                "len" => m(vec![], Int, false),
                "push" => m(vec![e], Void, true),
                "pop" => m(vec![], e, true),
                "insert" => m(vec![Int, e], Void, true),
                "remove" => m(vec![Int], e, true),
                "swap" => m(vec![Int, Int], Void, true),
                "slice" => m(vec![Int, Int], t, false),
                "contains" => m(vec![e], Bool, false),
                "index_of" => m(vec![e], Int, false),
                "repeat" => m(vec![Int], t, false),
                "sort" | "reverse" => m(vec![], Void, true),
                "join" => m(vec![Str], Str, false),
                "reversed" => m(vec![], t, false),
                // the types depend on the lambda: see `check/lambda.rs`
                "sum" | "min" | "max" => m(vec![], e, false),
                "map" | "filter" | "sort_by" | "count" | "any" | "all" | "find_index" => m(vec![Type::Unknown], Type::Unknown, name == "sort_by"),
                "fold" => m(vec![Type::Unknown, Type::Unknown], Type::Unknown, false),
                _ => None,
            }
        }
        Str => match name {
            "len" => m(vec![], Int, false),
            "chars" => m(vec![], Type::array(Char), false),
            "codes" => m(vec![], Type::array(Int), false),
            "slice" => m(vec![Int, Int], Str, false),
            "contains" | "starts_with" | "ends_with" => m(vec![Str], Bool, false),
            "index_of" => m(vec![Str], Int, false),
            "split" => m(vec![Str], Type::array(Str), false),
            "replace" => m(vec![Str, Str], Str, false),
            "trim" | "upper" | "lower" => m(vec![], Str, false),
            "repeat" => m(vec![Int], Str, false),
            "pad_left" | "pad_right" => m(vec![Int], Str, false),
            "reversed" => m(vec![], Str, false),
            "count" | "any" | "all" | "find_index" => m(vec![Type::Unknown], Type::Unknown, false),
            _ => None,
        },
        Type::Map(_) => {
            let (k, v) = t.map_kv()?;
            match name {
                "len" => m(vec![], Int, false),
                "has" => m(vec![k], Bool, false),
                "get" => m(vec![k], v, false),
                "set" => m(vec![k, v], Void, true),
                "remove" => m(vec![k], Void, true),
                "keys" => m(vec![], Type::array(k), false),
                "values" => m(vec![], Type::array(v), false),
                _ => None,
            }
        }
        Char => match name {
            "code" => m(vec![], Int, false),
            "upper" | "lower" => m(vec![], Char, false),
            "is_digit" | "is_letter" | "is_upper" | "is_lower" | "is_space" => m(vec![], Bool, false),
            _ => None,
        },
        _ => None,
    }
}

/// The methods of a type, for "did you mean" and "the methods are" hints.
pub fn methods_of(t: Type) -> &'static [&'static str] {
    match t {
        Type::Array(_) => ARRAY_METHODS,
        Type::Str => STR_METHODS,
        Type::Char => CHAR_METHODS,
        Type::Map(_) => MAP_METHODS,
        _ => &[],
    }
}

/// A struct's fields, in declaration order.
#[derive(Clone)]
pub struct StructInfo {
    pub fields: Vec<(String, Type, Span)>,
    pub span: Span,
}

/// True if values of `t` own heap memory (strings, arrays, structs that contain one).
pub fn managed(t: Type, structs: &HashMap<String, StructInfo>) -> bool {
    managed_in(t, structs, &mut Vec::new())
}

fn managed_in(t: Type, structs: &HashMap<String, StructInfo>, seen: &mut Vec<String>) -> bool {
    match t {
        Type::Str | Type::Array(_) | Type::Map(_) => true,
        Type::Struct(_) => {
            let Some(name) = t.struct_name() else { return false };
            if seen.contains(&name) {
                return false;
            }
            seen.push(name.clone());
            structs.get(&name).is_some_and(|s| s.fields.iter().any(|(_, ft, _)| managed_in(*ft, structs, seen)))
        }
        _ => false,
    }
}

/// True if struct `start` contains itself by value (through fields that are structs, not arrays).
pub fn contains_itself(start: &str, structs: &HashMap<String, StructInfo>) -> bool {
    fn visit(name: &str, start: &str, structs: &HashMap<String, StructInfo>, seen: &mut Vec<String>) -> bool {
        let Some(info) = structs.get(name) else { return false };
        for (_, ft, _) in &info.fields {
            if let Some(n) = ft.struct_name() {
                if n == start {
                    return true;
                }
                if !seen.contains(&n) {
                    seen.push(n.clone());
                    if visit(&n, start, structs, seen) {
                        return true;
                    }
                }
            }
        }
        false
    }
    visit(start, start, structs, &mut Vec::new())
}

/// The variable a place starts from: `xs` for `xs[i].tags`. `None` if `e` is not a place
/// (a call, a literal, ...). Strings are never places below the variable (they are immutable).
pub fn place_root(e: &Expr) -> Option<&str> {
    match &e.kind {
        ExprKind::Var(n) => Some(n),
        ExprKind::Field(b, _) => place_root(b),
        ExprKind::Index(b, _) if b.ty != Type::Str => place_root(b),
        _ => None,
    }
}

/// Every `free(x)` in a block and the blocks nested in it (for loops: a `free` in one round
/// reaches the uses of the next round), with whether `x` is also assigned somewhere in it.
pub fn frees_in(stmts: &[Stmt], out: &mut Vec<(String, Span)>, assigned: &mut Vec<String>) {
    for s in stmts {
        match &s.kind {
            StmtKind::Expr(e) => {
                if let ExprKind::Call(f, args) = &e.kind {
                    if f == "free" {
                        if let Some(ExprKind::Var(n)) = args.first().map(|a| &a.kind) {
                            out.push((n.clone(), e.span));
                        }
                    }
                }
            }
            StmtKind::Assign { target, op: None, .. } => {
                if let ExprKind::Var(n) = &target.kind {
                    assigned.push(n.clone());
                }
            }
            StmtKind::If { then, els, .. } => {
                frees_in(then, out, assigned);
                if let Some(e) = els {
                    frees_in(e, out, assigned);
                }
            }
            StmtKind::While { body, .. }
            | StmtKind::For { body, .. }
            | StmtKind::ForEach { body, .. }
            | StmtKind::Arena(body) => frees_in(body, out, assigned),
            _ => {}
        }
    }
}

/// A short, source-like description of a char for messages: `'a'`.
pub fn show_char(c: u32) -> String {
    crate::lexer::char_literal(c)
}
