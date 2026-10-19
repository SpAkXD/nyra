//! Checking one buffer: the stages of `nyra check` (lex, parse, imports, standard modules, types,
//! examples, slow-pattern warnings), kept apart so that a program with errors still gives what
//! the editor needs: the diagnostics, the checked program with the type of every expression, and
//! the `let` lines with the types of their variables.

use std::collections::HashMap;
use std::path::Path;

use crate::ast::{each_stmt_mut, Expr, ExprKind, Program, Span, StmtKind, Type};
use crate::diag::{self, Diag};
use crate::{check, examples, fix, lexer, modules, parser, perfwarn, stdlib, webcheck};

/// What an expression at a position is, and its type.
#[derive(Debug, Clone)]
pub enum Info {
    /// a variable
    Var(String, Type),
    /// `recv.name`
    Field { recv: Type, name: String, ty: Type },
    /// `recv.name(...)` of a built-in type
    Method { recv: Type, name: String },
    /// a call of a function (the full name: `math.sqrt`, `shapes.area`, `web.page`) or a struct
    Call { name: String },
}

pub struct Analysis {
    /// The errors (empty when the program compiles).
    pub errors: Vec<Diag>,
    pub warnings: Vec<Diag>,
    /// The program as far as it got: parsed, or checked when `checked`.
    pub prog: Option<Program>,
    /// The checker ran: the types in `prog` and `exprs` are known.
    pub checked: bool,
    /// The expressions of the file, by position (a field, method or call by the position of its name).
    pub exprs: HashMap<Span, Vec<Info>>,
    /// The `let` and `var` lines: the name and type of the variable, by the position of the keyword.
    pub lets: HashMap<Span, (String, Type)>,
}

impl Analysis {
    /// The type the checker found for the variable `name` used at `at`.
    pub fn var_at(&self, at: Span, name: &str) -> Option<Type> {
        self.exprs.get(&at)?.iter().find_map(|i| match i {
            Info::Var(n, t) if n == name && !t.is_unknown() => Some(*t),
            _ => None,
        })
    }

    /// The type of the variable `name` on the lines `from..=to`: where it is used or declared.
    pub fn var_type_in(&self, name: &str, from: usize, to: usize) -> Option<Type> {
        let mut best: Option<(Span, Type)> = None;
        let mut consider = |s: Span, t: Type| {
            if s.line >= from && s.line <= to && !t.is_unknown() && best.is_none_or(|(b, _)| (s.line, s.col) < (b.line, b.col)) {
                best = Some((s, t));
            }
        };
        for (s, (n, t)) in &self.lets {
            if n == name {
                consider(*s, *t);
            }
        }
        for (s, infos) in &self.exprs {
            for i in infos {
                if let Info::Var(n, t) = i {
                    if n == name {
                        consider(*s, *t);
                    }
                }
            }
        }
        best.map(|(_, t)| t)
    }
}

/// True for a name a program can write (the compiler's hidden names start with `·`).
fn visible(name: &str) -> bool {
    name.starts_with(|c: char| c.is_alphabetic() || c == '_')
}

/// Checks `src`, the text of the file at `path` (`None`: a buffer that is not a file yet).
pub fn analyze(src: &str, path: Option<&Path>) -> Analysis {
    modules::set_main(path);
    diag::clear_warnings();
    let mut a =
        Analysis { errors: Vec::new(), warnings: Vec::new(), prog: None, checked: false, exprs: HashMap::new(), lets: HashMap::new() };
    let (toks, errs) = lexer::lex(src);
    if !errs.is_empty() {
        a.errors = errs;
        return finish(a, src);
    }
    let (mut prog, errs) = parser::parse(toks);
    if !errs.is_empty() {
        a.errors = errs;
        a.prog = Some(prog);
        return finish(a, src);
    }
    let mut errs = modules::load(&mut prog);
    if errs.is_empty() {
        errs = stdlib::link(&mut prog);
    }
    if !errs.is_empty() {
        a.errors = errs;
        a.prog = Some(prog);
        return finish(a, src);
    }
    let motion = if prog.uses.iter().any(|u| u.module == "web") { webcheck::motion(src) } else { Vec::new() };
    let mut errs = check::check(&mut prog);
    errs.extend(motion);
    a.checked = true;
    // a file of definitions only is a module another file imports: "nothing to run" is no mistake there
    if !prog.public.is_empty() {
        errs.retain(|d| d.code != "E0208");
    }
    if errs.is_empty() && prog.funcs.iter().any(|f| f.name == "main" || f.name == crate::ast::USER_MAIN) {
        errs = examples::run(&mut prog).errors;
        if errs.is_empty() {
            perfwarn::check(&prog, false);
        }
    }
    a.errors = errs;
    index(&mut prog, &mut a);
    a.prog = Some(prog);
    finish(a, src)
}

/// The fixes are kept only where they apply exactly (as `nyra check` does), and the warnings collected.
fn finish(mut a: Analysis, src: &str) -> Analysis {
    // a fix is an edit of this file: one for a mistake in an imported file would edit the wrong text
    a.errors.iter_mut().filter(|d| d.file.is_some()).for_each(|d| d.fix.clear());
    fix::validate(&mut a.errors, src);
    a.warnings = diag::warnings();
    a
}

/// Fills `exprs` and `lets` from the program's own functions (not those of modules).
fn index(prog: &mut Program, a: &mut Analysis) {
    let exprs = &mut a.exprs;
    let lets = &mut a.lets;
    let files = &prog.files;
    for f in prog.funcs.iter_mut() {
        if f.name.contains('.') || files.contains_key(&f.name) {
            continue;
        }
        each_stmt_mut(
            &mut f.body,
            &mut |s| {
                if let StmtKind::Let { name, ty, value, .. } = &s.kind {
                    if visible(name) {
                        lets.insert(s.span, (name.clone(), ty.unwrap_or(value.ty)));
                    }
                }
            },
            &mut |e| record(e, exprs),
        );
    }
    for ex in prog.examples.iter_mut().filter(|e| e.file.is_none()) {
        ex.expr.each_mut(&mut |e| record(e, exprs));
    }
}

fn record(e: &mut Expr, exprs: &mut HashMap<Span, Vec<Info>>) {
    let info = match &e.kind {
        ExprKind::Var(n) if visible(n) => Info::Var(n.clone(), e.ty),
        ExprKind::Field(r, n) if visible(n) => Info::Field { recv: r.ty, name: n.clone(), ty: e.ty },
        ExprKind::Method(r, n, _) => Info::Method { recv: r.ty, name: n.clone() },
        ExprKind::Call(n, _) if visible(n) => Info::Call { name: n.clone() },
        _ => return,
    };
    exprs.entry(e.span).or_default().push(info);
}
