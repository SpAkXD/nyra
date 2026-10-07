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
                    format!("{prefix}{n}_{count}")
                }
            }
            None => format!("{prefix}t{i}"),
        })
        .collect()
}
