//! The program's own code in the browser: `update` and the live views.
//!
//! A program with interactive state has script `var`s, an `enum Msg`, one `fn update(m: Msg)`, and
//! view functions without parameters that read the state. The web library writes each event as
//! `data-on="<the Msg as JSON>"` and wraps each live view's HTML in an element with
//! `data-live="<the function's name>"`. The build then compiles `update`, those views, the function
//! that renders a view's value to HTML, and everything they call, with the JavaScript backend, and
//! adds `runtime.js`, which sends each event to `update` and re-renders the live regions. The state
//! starts from the values the script variables had when the build ran the program.

use std::collections::BTreeSet;

use crate::ast::{self, Type};
use crate::codegen::js;
use crate::ir::{self, FuncId, StmtKind};

/// Event delegation and the keyed morph of live regions (see the file).
pub const RUNTIME_JS: &str = include_str!("runtime.js");

/// The name of the function that receives the events.
pub const UPDATE: &str = "update";

/// The functions that may render a view's value to HTML, in the order they are tried: the web
/// library's renderer of its `Node` type (docs/WEB.md, Build contract).
pub const RENDERERS: [&str; 4] = ["web._render", "web.render", "_render", "render"];

/// The script variables whose values the browser may need, found before the program runs: the ones
/// `update` uses, and the ones of each function without parameters that returns a value (a possible
/// live view; which ones are live, the pages say). Empty without an `update`.
pub fn state_vars(prog: &ast::Program) -> Vec<String> {
    if !prog.funcs.iter().any(|f| f.name == UPDATE) {
        return Vec::new();
    }
    let mut vars: BTreeSet<usize> = BTreeSet::new();
    for f in &prog.funcs {
        let view = f.params.is_empty() && f.ret != Type::Void && !f.name.contains('.') && f.name != "main";
        if f.name == UPDATE || view {
            vars.extend(prog.globals.uses.get(&f.name).into_iter().flatten().map(|u| u.var));
        }
    }
    vars.into_iter().map(|i| prog.globals.vars[i].name.clone()).collect()
}

/// The client JavaScript (runtime parts, the program's functions, `runtime.js`), or `None` when the
/// pages have no events. `views` are the names in `data-live`; `state` the values the build
/// recorded (`_state`).
pub fn build(
    prog: &ast::Program,
    views: &[String],
    events: bool,
    state: &[(String, String)],
    file: &str,
) -> Result<Option<String>, String> {
    if !events {
        return Ok(None);
    }
    // lowered without the optimizer, which drops the functions the script itself never calls
    let module = ir::lower::lower(prog).map_err(|what| format!("not supported yet: {what}"))?;
    if let Err(e) = ir::verify::verify(&module) {
        return Err(format!("internal error: the compiler produced invalid IR ({e}); please report this bug"));
    }
    let m = &module;
    let find = |name: &str| m.funcs.iter().position(|f| f.name == name).map(|i| FuncId(i as u32));
    let globals_of = |name: &str| -> Vec<(String, bool)> {
        prog.globals
            .uses
            .get(name)
            .map(|us| us.iter().map(|u| (prog.globals.vars[u.var].name.clone(), u.inout)).collect())
            .unwrap_or_default()
    };
    let Some(update) = find(UPDATE) else {
        return Err("a page has an event (`on:`), but the program has no `fn update(m: Msg)` to handle it".into());
    };
    let uf = m.func(update);
    let ug = globals_of(UPDATE);
    if uf.params != 1 + ug.len() || !matches!(uf.locals[0].ty, Type::Struct(_)) {
        return Err("`update` must take exactly one parameter, the `Msg` of the event: `fn update(m: Msg)`".into());
    }
    let mut roots = vec![update];
    let mut view_list = Vec::new();
    for v in views {
        let g = globals_of(v);
        let Some(id) = find(v).filter(|id| m.func(*id).params == g.len() && m.func(*id).ret.is_some()) else {
            return Err(format!(
                "a live region names `{v}`, which is not a function of the program without parameters that returns a value"
            ));
        };
        let ret = m.func(id).ret.expect("checked above");
        let render = if ret == Type::Str {
            None
        } else {
            let r = RENDERERS.iter().find_map(|n| find(n).filter(|r| m.func(*r).params == 1 && m.func(*r).locals[0].ty == ret));
            match r {
                Some(r) => Some(r),
                None => {
                    return Err(format!(
                        "the live view `{v}` returns `{}`, and the web library has no `_render` function that turns it into HTML",
                        ret.name()
                    ))
                }
            }
        };
        roots.push(id);
        roots.extend(render);
        view_list.push((v.clone(), id, g, render));
    }
    let funcs = reachable(m, &roots);
    // the values the functions start from
    let mut names: Vec<String> = ug.iter().map(|(n, _)| n.clone()).collect();
    for (_, _, g, _) in &view_list {
        for (n, _) in g {
            if !names.contains(n) {
                names.push(n.clone());
            }
        }
    }
    let mut st = Vec::new();
    for n in names {
        let Some(var) = prog.globals.vars.iter().find(|v| v.name == n) else { continue };
        let Some((_, json)) = state.iter().rev().find(|(k, _)| *k == n) else {
            return Err(format!(
                "internal error: the build did not record the value of the script variable `{n}`; please report this bug"
            ));
        };
        st.push((n, var.ty, json.clone()));
    }
    let client = js::Client { funcs, update: Some((update, ug)), views: view_list, state: st };
    let code = js::gen_client(m, &client);
    // runtime errors name the program's file, without the folder it was built in
    let name = std::path::Path::new(file).file_name().and_then(|n| n.to_str()).unwrap_or(file);
    let runtime = js::runtime(name);
    let roots_text = format!("{code}\n{RUNTIME_JS}");
    let mut out = String::from("(() => {\n");
    out.push_str(&shake(&runtime, &roots_text));
    out.push_str(&roots_text);
    out.push_str("})();\n");
    Ok(Some(out))
}

/// `roots` and every function they call, in the module's order.
fn reachable(m: &ir::Module, roots: &[FuncId]) -> Vec<FuncId> {
    let mut seen: BTreeSet<u32> = BTreeSet::new();
    let mut todo: Vec<FuncId> = roots.to_vec();
    while let Some(f) = todo.pop() {
        if !seen.insert(f.0) {
            continue;
        }
        calls(&m.func(f).body, &mut todo);
    }
    seen.into_iter().map(FuncId).collect()
}

fn calls(ss: &[ir::Stmt], out: &mut Vec<FuncId>) {
    for s in ss {
        match &s.kind {
            StmtKind::Call { func, .. } => out.push(*func),
            StmtKind::If { then, els, .. } => {
                calls(then, out);
                calls(els, out);
            }
            StmtKind::Loop { head, body, step, .. } => {
                calls(head, out);
                calls(body, out);
                calls(step, out);
            }
            StmtKind::ForEach { body, .. } => calls(body, out),
            _ => {}
        }
    }
}

/// The top-level declarations of the JavaScript runtime that `roots` needs, directly or through
/// each other, in the runtime's order. A declaration starts at a line that begins with
/// `function`, `class`, `const` or `let` (the runtime's own style); a part that declares nothing
/// (`"use strict";`) is always kept.
pub fn shake(runtime: &str, roots: &str) -> String {
    let mut parts: Vec<(Vec<String>, String)> = Vec::new();
    let mut cur: (Vec<String>, String) = (Vec::new(), String::new());
    for line in runtime.lines() {
        if let Some(names) = declares(line) {
            if !cur.1.trim().is_empty() || !cur.0.is_empty() {
                parts.push(std::mem::take(&mut cur));
            }
            cur.0 = names;
        }
        cur.1.push_str(line);
        cur.1.push('\n');
    }
    parts.push(cur);
    let mut need: BTreeSet<String> = idents(roots);
    let mut keep = vec![false; parts.len()];
    loop {
        let mut more = false;
        for (k, (names, text)) in parts.iter().enumerate() {
            if keep[k] || !(names.is_empty() || names.iter().any(|n| need.contains(n))) {
                continue;
            }
            keep[k] = true;
            more = true;
            need.extend(idents(text));
        }
        if !more {
            break;
        }
    }
    let mut out = String::new();
    for (k, (_, text)) in parts.iter().enumerate() {
        if keep[k] {
            out.push_str(text);
        }
    }
    out
}

/// The names a line declares at the top level of the runtime, or `None` for another line.
fn declares(line: &str) -> Option<Vec<String>> {
    let word = |s: &str| -> String { s.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '$').collect() };
    for kw in ["function ", "class "] {
        if let Some(rest) = line.strip_prefix(kw) {
            return Some(vec![word(rest)]);
        }
    }
    for kw in ["const ", "let ", "var "] {
        if let Some(rest) = line.strip_prefix(kw) {
            // `let a = 1, b = 2;`: every name followed by `=`
            let names = rest
                .split(',')
                .filter_map(|p| {
                    let p = p.trim_start();
                    let n = word(p);
                    let after = p[n.len()..].trim_start();
                    (!n.is_empty() && after.starts_with('=') && !after.starts_with("==")).then_some(n)
                })
                .collect();
            return Some(names);
        }
    }
    None
}

/// The identifiers in a text (also in strings and comments: keeping too much is safe).
fn idents(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut cur = String::new();
    for c in text.chars().chain(std::iter::once(' ')) {
        if c.is_ascii_alphanumeric() || c == '_' || c == '$' {
            cur.push(c);
        } else if !cur.is_empty() {
            if !cur.starts_with(|c: char| c.is_ascii_digit()) {
                out.insert(std::mem::take(&mut cur));
            } else {
                cur.clear();
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shaking_keeps_what_is_called_and_its_helpers() {
        let rt = "\"use strict\";\nconst ny_a = 1, ny_b = 2;\nfunction ny_f(x) {\n    return ny_g(x) + ny_a;\n}\nfunction ny_g(x) { return x; }\nfunction ny_unused() {}\nclass NyThing {}\n";
        let out = shake(rt, "ny_f(3)");
        assert!(out.contains("\"use strict\"") && out.contains("function ny_f") && out.contains("function ny_g"));
        assert!(out.contains("const ny_a") && !out.contains("ny_unused") && !out.contains("NyThing"));
        assert_eq!(declares("let ny_in = null, ny_in_pos = 0;"), Some(vec!["ny_in".to_string(), "ny_in_pos".to_string()]));
        assert_eq!(declares("const x = f(a, { b: 1, c: 2 });"), Some(vec!["x".to_string()]));
    }
}
