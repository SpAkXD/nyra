//! Backend names for IR locals. A Nyra variable keeps its own name (escaped by the backend);
//! a second variable with the same name (sibling blocks may reuse names) and every compiler
//! temporary get a name with the backend's reserved prefix, so they never collide with user
//! names (the backends escape user names that start with the prefix).

use std::collections::HashMap;

use crate::ir::Func;

pub fn locals(f: &Func, escape: fn(&str) -> String, prefix: &str) -> Vec<String> {
    let mut seen: HashMap<&str, usize> = HashMap::new();
    f.locals
        .iter()
        .enumerate()
        .map(|(i, l)| match &l.name {
            Some(n) => {
                let count = seen.entry(n.as_str()).or_insert(0);
                *count += 1;
                if *count == 1 {
                    escape(n)
                } else {
                    format!("{prefix}{}_{count}", escape(n))
                }
            }
            None => format!("{prefix}t{i}"),
        })
        .collect()
}

/// Like `locals`, for languages with block scopes, where each local is declared in the block
/// that uses it: a loop variable (`loop_var`, declared by its own loop) keeps its plain name even
/// when an earlier local had it, so two loops can both count with `i`.
pub fn scoped(f: &Func, escape: fn(&str) -> String, prefix: &str, loop_var: &[bool]) -> Vec<String> {
    let mut seen: HashMap<&str, usize> = HashMap::new();
    f.locals
        .iter()
        .enumerate()
        .map(|(i, l)| match &l.name {
            Some(n) => {
                let count = seen.entry(n.as_str()).or_insert(0);
                *count += 1;
                if *count == 1 || loop_var[i] {
                    escape(n)
                } else {
                    format!("{prefix}{}_{count}", escape(n))
                }
            }
            None => format!("{prefix}t{i}"),
        })
        .collect()
}

/// A Nyra name as an ASCII identifier: letters the target does not accept in names (Nyra allows
/// `x²`, `größe`) are spelled out as `_u{hex}_`, after the prefix `nyU_`.
pub fn ascii(n: &str) -> String {
    let ok = |c: char, first: bool| c == '_' || c.is_ascii_alphabetic() || (!first && c.is_ascii_digit());
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
