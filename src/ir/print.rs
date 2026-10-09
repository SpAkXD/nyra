//! Text form of the IR, printed with `NYRA_DUMP=ir` (debugging; later the playground's IR tab).
//!
//! ```text
//! fn main() {
//!     %0: int = call note("a", 1)
//!     %1: int = call note("b", 2)
//!     %2: int = call add(%0, %1)
//!     print(%2)
//! }
//! ```

use std::fmt::Write;

use super::{Arg, Expr, Func, LocalId, Module, Place, Step, Stmt, StmtKind, UnOp};
use crate::diag::json_str;

pub fn print(m: &Module) -> String {
    let mut out = String::new();
    for f in &m.funcs {
        let params: Vec<String> = (0..f.params)
            .map(|i| {
                let inout = if f.locals[i].inout { "inout " } else { "" };
                format!("{inout}{}: {}", name(f, LocalId(i as u32)), f.locals[i].ty.name())
            })
            .collect();
        let ret = f.ret.map(|t| format!(" -> {}", t.name())).unwrap_or_default();
        let _ = writeln!(out, "fn {}({}){ret} {{", f.name, params.join(", "));
        stmts(m, f, &f.body, 1, &mut out);
        out.push_str("}\n");
    }
    out
}

fn name(f: &Func, l: LocalId) -> String {
    match &f.local(l).name {
        Some(n) => n.clone(),
        None => format!("%{}", l.0),
    }
}

/// `x = `, or `%3: int = ` for a temporary.
fn target(f: &Func, l: LocalId) -> String {
    match &f.local(l).name {
        Some(n) => format!("{n} = "),
        None => format!("%{}: {} = ", l.0, f.local(l).ty.name()),
    }
}

fn stmts(m: &Module, f: &Func, ss: &[Stmt], depth: usize, out: &mut String) {
    let pad = "    ".repeat(depth);
    for s in ss {
        match &s.kind {
            StmtKind::Set(l, e) => {
                let _ = writeln!(out, "{pad}{}{}", target(f, *l), expr(m, f, e));
            }
            StmtKind::Call { dst, func, args } => {
                let d = dst.map(|d| target(f, d)).unwrap_or_default();
                let args: Vec<String> = args
                    .iter()
                    .map(|a| match a {
                        Arg::Val(e) => expr(m, f, e),
                        Arg::InOut(p) => format!("inout {}", place(m, f, p)),
                    })
                    .collect();
                let _ = writeln!(out, "{pad}{d}call {}({})", m.func(*func).name, args.join(", "));
            }
            StmtKind::Op { dst, op, args } => {
                let d = dst.map(|d| target(f, d)).unwrap_or_default();
                let _ = writeln!(out, "{pad}{d}{}({})", op.name(), list(m, f, args));
            }
            StmtKind::Store { place: p, value } => {
                let _ = writeln!(out, "{pad}{} = {}", place(m, f, p), expr(m, f, value));
            }
            StmtKind::Mutate { dst, op, place: p, args } => {
                let d = dst.map(|d| target(f, d)).unwrap_or_default();
                let mut all = vec![place(m, f, p)];
                all.extend(args.iter().map(|a| expr(m, f, a)));
                let _ = writeln!(out, "{pad}{d}{}(inout {})", op.name(), all.join(", "));
            }
            StmtKind::If { cond, then, els } => {
                let _ = writeln!(out, "{pad}if {} {{", expr(m, f, cond));
                stmts(m, f, then, depth + 1, out);
                if !els.is_empty() {
                    let _ = writeln!(out, "{pad}}} else {{");
                    stmts(m, f, els, depth + 1, out);
                }
                let _ = writeln!(out, "{pad}}}");
            }
            StmtKind::Loop { head, cond, body, step } => {
                let _ = writeln!(out, "{pad}loop {{");
                stmts(m, f, head, depth + 1, out);
                let _ = writeln!(out, "{pad}    while {}", expr(m, f, cond));
                stmts(m, f, body, depth + 1, out);
                if !step.is_empty() {
                    let _ = writeln!(out, "{pad}  step:");
                    stmts(m, f, step, depth + 1, out);
                }
                let _ = writeln!(out, "{pad}}}");
            }
            StmtKind::ForEach { var, iter, body } => {
                let _ = writeln!(out, "{pad}for {} in {} {{", name(f, *var), expr(m, f, iter));
                stmts(m, f, body, depth + 1, out);
                let _ = writeln!(out, "{pad}}}");
            }
            StmtKind::Break => {
                let _ = writeln!(out, "{pad}break");
            }
            StmtKind::Continue => {
                let _ = writeln!(out, "{pad}continue");
            }
            StmtKind::Dup(l) => {
                let _ = writeln!(out, "{pad}dup {}", name(f, *l));
            }
            StmtKind::Drop(l) => {
                let _ = writeln!(out, "{pad}drop {}", name(f, *l));
            }
            StmtKind::Free(l) => {
                let _ = writeln!(out, "{pad}free {}", name(f, *l));
            }
            StmtKind::Keep(l) => {
                let _ = writeln!(out, "{pad}keep {}", name(f, *l));
            }
            StmtKind::Return(None) => {
                let _ = writeln!(out, "{pad}return");
            }
            StmtKind::Return(Some(e)) => {
                let _ = writeln!(out, "{pad}return {}", expr(m, f, e));
            }
        }
    }
}

/// `grid[%3][%4]`, `p.name`.
fn place(m: &Module, f: &Func, p: &Place) -> String {
    let mut s = name(f, p.root);
    let mut t = f.local(p.root).ty;
    for step in &p.path {
        match step {
            Step::Index(i, _) => {
                let _ = write!(s, "[{}]", expr(m, f, i));
                t = t.elem().unwrap_or(t);
            }
            Step::Key(i, _) => {
                let _ = write!(s, "{{{}}}", expr(m, f, i));
                t = t.map_kv().map_or(t, |(_, v)| v);
            }
            Step::Field(k) => {
                let field = m.structs.get(t).and_then(|info| info.fields.get(*k as usize));
                match field {
                    Some((n, ft)) => {
                        let _ = write!(s, ".{n}");
                        t = *ft;
                    }
                    None => {
                        let _ = write!(s, ".#{k}");
                    }
                }
            }
        }
    }
    s
}

fn list(m: &Module, f: &Func, es: &[Expr]) -> String {
    es.iter().map(|e| expr(m, f, e)).collect::<Vec<_>>().join(", ")
}

fn expr(m: &Module, f: &Func, e: &Expr) -> String {
    match e {
        Expr::Int(n) => n.to_string(),
        Expr::Float(x) => format!("{x:?}"),
        Expr::Bool(b) => b.to_string(),
        Expr::Char(c) => crate::lexer::char_literal(*c),
        Expr::Pure(p, args) => format!("{}({})", p.name(), list(m, f, args)),
        Expr::Str(id) => json_str(m.str(*id)),
        Expr::Local(l) => name(f, *l),
        Expr::Unary(UnOp::Not, x) => format!("!{}", expr(m, f, x)),
        Expr::Unary(_, x) => format!("-{}", expr(m, f, x)),
        Expr::Binary(op, a, b) => format!("({} {} {})", expr(m, f, a), op.symbol(), expr(m, f, b)),
        Expr::Select(c, a, b) => format!("select({}, {}, {})", expr(m, f, c), expr(m, f, a), expr(m, f, b)),
        Expr::IntToFloat(x) => format!("float({})", expr(m, f, x)),
        Expr::Field(x, k, _) => {
            let t = x.ty(f);
            match m.structs.get(t).and_then(|s| s.fields.get(*k as usize)) {
                Some((n, _)) => format!("{}.{n}", expr(m, f, x)),
                None => format!("{}.#{k}", expr(m, f, x)),
            }
        }
    }
}
