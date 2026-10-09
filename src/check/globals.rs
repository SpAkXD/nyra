//! Script variables ("globals"): the `let` and `var` statements at the top level of a script
//! (a program without `fn main`). Every function may read them, and change the `var`s:
//!
//! ```text
//! var pos = 0
//! fn advance() { pos += 1 }
//! advance()
//! ```
//!
//! - A function that declares a parameter or variable of the same name does not see the script
//!   variable; using both in one function is E0216.
//! - A call runs the function, so every script variable it uses (directly or through the
//!   functions it calls) must already be declared at the call: E0217. Examples run while
//!   compiling, before the script: they cannot call such a function (E0254).
//! - A script variable passed `inout` to a function that uses it is E0237, like any two `inout`
//!   arguments of one variable.
//!
//! The checker records what each function reads, changes and calls; `finish_globals` closes the
//! sets over the call graph (a fixpoint) and checks the calls. Lowering passes each script
//! variable a function uses as a hidden parameter: `inout` when it (or a callee) changes it.

use std::collections::{BTreeSet, HashSet};

use super::{collect_decls, Checker, Decl, Freed, Var};
use crate::ast::*;
use crate::diag::Diag;

/// A call of a user function.
struct Site {
    callee: String,
    span: Span,
    /// In a script's `main`: the index of the top-level statement it is in.
    top: Option<usize>,
    /// The script variables passed `inout`.
    inout: Vec<(usize, Span)>,
    /// In a script's `main`: the script variables that are freed at the call.
    freed: Vec<(usize, Freed)>,
    arena: bool,
    lambda: bool,
}

/// What one function does with script variables itself.
#[derive(Default)]
struct Facts {
    uses: BTreeSet<usize>,
    writes: BTreeSet<usize>,
    calls: Vec<Site>,
    /// The names it declares itself (parameters, variables, lambda parameters).
    own: HashSet<String>,
}

pub(super) struct State {
    script: bool,
    /// Checking the `main` of a script (its top-level statements).
    pub(super) in_main: bool,
    /// The top-level statement of the script being checked.
    pub(super) top_stmt: usize,
    /// The script variables, as declared, and the variable functions see.
    pub(super) vars: Vec<(Global, Var)>,
    in_example: bool,
    /// Every variable of `main` (script or `fn main`), for the hints of E0201.
    main_decls: Vec<(String, Span)>,
    explicit_main: bool,
    cur: Facts,
    /// Per function (`""`: the examples), in the order they were checked.
    facts: Vec<(String, Facts)>,
    /// Names of functions and structs: a hidden parameter must not take one.
    taken: HashSet<String>,
}

impl State {
    pub(super) fn new(prog: &Program) -> State {
        let mut main_decls = Vec::new();
        let main = prog.funcs.iter().find(|f| f.name == "main");
        if let Some(m) = main {
            collect_decls(&m.body, &mut main_decls);
        }
        let taken = prog.funcs.iter().map(|f| f.name.clone()).chain(prog.structs.iter().map(|s| s.name.clone())).collect();
        State {
            script: prog.script,
            in_main: false,
            top_stmt: 0,
            vars: Vec::new(),
            in_example: false,
            main_decls,
            explicit_main: !prog.script && main.is_some(),
            cur: Facts::default(),
            facts: Vec::new(),
            taken,
        }
    }

    pub(super) fn start_func(&mut self, f: &Func) {
        let mut own: HashSet<String> = f.params.iter().map(|p| p.name.clone()).collect();
        let mut decls = Vec::new();
        collect_decls(&f.body, &mut decls);
        own.extend(decls.into_iter().map(|(n, _)| n));
        stmts_exprs(&f.body, &mut |e| lambda_names(e, &mut own));
        self.in_example = false;
        self.cur = Facts { own, ..Facts::default() };
    }

    pub(super) fn start_example(&mut self) {
        self.in_example = true;
        self.cur = Facts::default();
    }

    pub(super) fn end_func(&mut self, name: &str) {
        let facts = std::mem::take(&mut self.cur);
        self.in_example = false;
        match self.facts.iter_mut().find(|(n, _)| n == name) {
            Some((_, f)) => {
                f.uses.extend(facts.uses);
                f.writes.extend(facts.writes);
                f.calls.extend(facts.calls);
                f.own.extend(facts.own);
            }
            None => self.facts.push((name.to_string(), facts)),
        }
    }

    pub(super) fn add_global(&mut self, name: &str, ty: Type, mutable: bool, span: Span) {
        if self.vars.iter().any(|(g, _)| g.name == name) {
            return;
        }
        let global = Global { name: name.to_string(), ty, mutable, span, stmt: self.top_stmt };
        let var = Var { ty, decl: if mutable { Decl::Var } else { Decl::Let }, span, arena: 0 };
        self.vars.push((global, var));
    }

    fn find(&self, name: &str) -> Option<usize> {
        self.vars.iter().position(|(g, _)| g.name == name)
    }
}

impl Checker {
    /// The script variable `name` refers to in the function being checked, if any.
    pub(super) fn global_of(&self, name: &str) -> Option<usize> {
        let g = &self.g;
        if !g.script || g.in_main || g.in_example || self.fname.is_empty() || g.cur.own.contains(name) {
            return None;
        }
        if self.local(name).is_some() {
            return None;
        }
        g.find(name)
    }

    /// Like `global_of`, also in a script's `main`, where the script variables are locals.
    pub(super) fn global_ref(&self, name: &str) -> Option<usize> {
        if !self.g.in_main {
            return self.global_of(name);
        }
        let i = self.g.find(name)?;
        let top = self.scopes.get(1).and_then(|s| s.get(name));
        top.is_some_and(|v| v.span == self.g.vars[i].0.span).then_some(i)
    }

    /// `name` is read (or changed) here: if it is a script variable, the function uses it.
    pub(super) fn note_use(&mut self, name: &str, write: bool) {
        if let Some(i) = self.global_of(name) {
            self.g.cur.uses.insert(i);
            if write {
                self.g.cur.writes.insert(i);
            }
        }
    }

    /// A call of the user function `callee`; `inout` are the script variables passed `inout`.
    pub(super) fn record_call(&mut self, callee: &str, span: Span, inout: Vec<(usize, Span)>) {
        if !self.g.script {
            return;
        }
        let mut freed = Vec::new();
        if self.g.in_main {
            for (n, f) in &self.freed {
                if let Some(i) = self.global_ref(n) {
                    freed.push((i, *f));
                }
            }
            freed.sort_by_key(|(i, _)| *i);
        }
        let site = Site {
            callee: callee.to_string(),
            span,
            top: self.g.in_main.then_some(self.g.top_stmt),
            inout,
            freed,
            arena: self.arena_depth > 0,
            lambda: self.lambda_depth > 0,
        };
        self.g.cur.calls.push(site);
    }

    /// E0216 for a script variable that the function hides with its own variable, and E0201 with
    /// a hint for a variable of `main` that a function cannot see. `None`: not such a name.
    pub(super) fn global_undefined(&self, name: &str, span: Span) -> Option<Diag> {
        let g = &self.g;
        if self.fname.is_empty() || g.in_main || g.in_example {
            return None;
        }
        // declared in this function too: E0216, or the hints about this function's own blocks
        let own = self.decls.iter().any(|(n, _)| n == name);
        if g.script {
            if let Some(i) = g.find(name) {
                if g.cur.own.contains(name) {
                    let line = g.vars[i].0.span.line;
                    let local =
                        self.decls.iter().find(|(n, _)| n == name).map(|(_, s)| format!(" (line {})", s.line)).unwrap_or_default();
                    return Some(
                        Diag::new(
                            "E0216",
                            format!(
                                "`{name}` is a script variable (line {line}), but `{}` declares its own `{name}`{local}, so it cannot see the script's",
                                self.fname
                            ),
                            span,
                        )
                        .hint(format!(
                            "a name means one thing in a function: to use the script variable here, rename the function's own `{name}` (e.g. `{name}2`); a function that declares `{name}` sees only its own"
                        )),
                    );
                }
            }
            if let Some((_, at)) = g.main_decls.iter().find(|(n, _)| n == name && !own) {
                return Some(Diag::new("E0201", format!("undefined variable `{name}`"), span).hint(format!(
                    "`{name}` is declared inside a block of the script (line {}): functions see only the variables declared at the top level of the script; declare it there, before the block",
                    at.line
                )));
            }
        } else if g.explicit_main && self.fname != "main" {
            if let Some((_, at)) = g.main_decls.iter().find(|(n, _)| n == name && !own) {
                return Some(Diag::new("E0201", format!("undefined variable `{name}`"), span).hint(format!(
                    "`{name}` is a variable of `fn main` (line {}), and functions cannot see it: pass it as a parameter, or drop `fn main` and write its statements at the top level (a script), whose variables every function can use",
                    at.line
                )));
            }
        }
        None
    }

    /// Closes what each function uses over the calls, checks every call, and says which script
    /// variables each function receives.
    pub(super) fn finish_globals(&mut self) -> Globals {
        if !self.g.script || self.g.vars.is_empty() {
            return Globals::default();
        }
        let facts = std::mem::take(&mut self.g.facts);
        let index = |name: &str| facts.iter().position(|(n, _)| n == name);
        let mut uses: Vec<BTreeSet<usize>> = facts.iter().map(|(_, f)| f.uses.clone()).collect();
        let mut writes: Vec<BTreeSet<usize>> = facts.iter().map(|(_, f)| f.writes.clone()).collect();
        let callees: Vec<Vec<usize>> = facts.iter().map(|(_, f)| f.calls.iter().filter_map(|s| index(&s.callee)).collect()).collect();
        loop {
            let mut changed = false;
            for (f, cs) in callees.iter().enumerate() {
                for &c in cs {
                    if c == f {
                        continue;
                    }
                    let (cu, cw) = (uses[c].clone(), writes[c].clone());
                    let (nu, nw) = (uses[f].len(), writes[f].len());
                    uses[f].extend(cu);
                    writes[f].extend(cw);
                    changed |= uses[f].len() != nu || writes[f].len() != nw;
                }
            }
            if !changed {
                break;
            }
        }

        let var = |i: usize| &self.g.vars[i].0;
        let mut errs = Vec::new();
        for (fname, f) in &facts {
            for s in &f.calls {
                let Some(c) = index(&s.callee) else { continue };
                let (cu, cw) = (&uses[c], &writes[c]);
                let Some(&first) = cu.iter().next() else { continue };
                let callee = &s.callee;
                if fname.is_empty() {
                    let v = &var(first).name;
                    errs.push(
                        Diag::new("E0254", format!("this example calls `{callee}`, which uses the script variable `{v}`"), s.span).hint(
                            "examples run while the program compiles, before the script, so they cannot call a function that uses script variables: give it the value as a parameter instead, or remove this example",
                        ),
                    );
                    continue;
                }
                if s.lambda {
                    if let Some(&w) = cw.iter().next() {
                        let v = &var(w).name;
                        errs.push(
                            Diag::new("E0214", format!("a lambda cannot call `{callee}`: it changes the script variable `{v}`, and inside a lambda variables are read-only"), s.span)
                                .hint(format!("call `{callee}` in a `for` loop instead")),
                        );
                    }
                }
                if s.arena {
                    if let Some(&w) = cw.iter().find(|&&w| self.managed(var(w).ty)) {
                        let v = &var(w).name;
                        errs.push(
                            Diag::new(
                                "E0238",
                                format!(
                                    "`{callee}` changes the script variable `{v}`, so it cannot be called inside this `arena` block"
                                ),
                                s.span,
                            )
                            .hint(format!("values created in an `arena` are freed at its `}}`: call `{callee}` after the block")),
                        );
                    }
                }
                for &(i, at) in &s.inout {
                    if cu.contains(&i) {
                        let v = &var(i).name;
                        errs.push(
                            Diag::new("E0237", format!("`{v}` is passed `inout` to `{callee}`, which also uses the script variable `{v}`"), at).hint(format!(
                                "`{callee}` sees `{v}` itself: change it there instead of passing it, or pass a copy (`var t = {v}`, `inout t`, then `{v} = t`)"
                            )),
                        );
                    }
                }
                if let Some(top) = s.top {
                    if let Some(&i) = cu.iter().find(|&&i| var(i).stmt >= top) {
                        let g = var(i);
                        let (v, line) = (&g.name, g.span.line);
                        let kw = if g.mutable { "var" } else { "let" };
                        let msg = if g.stmt == top {
                            format!("`{callee}` uses the script variable `{v}`, which this statement is still declaring (line {line})")
                        } else {
                            format!("`{callee}` uses the script variable `{v}`, which is declared later (line {line})")
                        };
                        errs.push(Diag::new("E0217", msg, s.span).hint(format!(
                            "a function can run only after the script variables it uses exist: move `{kw} {v} = ...` (line {line}) above this call, or the call below it"
                        )));
                    }
                }
                for &(i, fr) in &s.freed {
                    if cu.contains(&i) {
                        let v = &var(i).name;
                        let msg = if fr.maybe {
                            format!("`{callee}` uses the script variable `{v}`, which may have been freed (line {})", fr.line)
                        } else {
                            format!("`{callee}` uses the script variable `{v}`, which was freed at line {}", fr.line)
                        };
                        errs.push(Diag::new("E0239", msg, s.span).hint(format!(
                            "give `{v}` a new value first (`{v} = ...`, needs `var`), or move `free({v})` after this call"
                        )));
                    }
                }
            }
        }
        self.errs.extend(errs);

        // the hidden parameters
        let mut out = Globals { vars: self.g.vars.iter().map(|(g, _)| g.clone()).collect(), ..Globals::default() };
        for (k, (fname, f)) in facts.iter().enumerate() {
            if fname.is_empty() || (fname == "main") || uses[k].is_empty() {
                continue;
            }
            let mut names: HashSet<String> = HashSet::new();
            let list = uses[k]
                .iter()
                .map(|&i| {
                    let g = &out.vars[i];
                    let mut name = g.name.clone();
                    if f.own.contains(&name) {
                        // the function has its own variable of this name: the parameter gets another
                        name = (2..)
                            .map(|n| format!("{}{n}", g.name))
                            .find(|n| {
                                !f.own.contains(n)
                                    && !self.g.taken.contains(n)
                                    && !out.vars.iter().any(|v| v.name == *n)
                                    && !names.contains(n)
                            })
                            .unwrap_or_default();
                    }
                    names.insert(name.clone());
                    GlobalUse { var: i, inout: writes[k].contains(&i), name }
                })
                .collect();
            out.uses.insert(fname.clone(), list);
        }
        out
    }
}

/// Calls `f` on every expression at the top of the statements (nested blocks included).
fn stmts_exprs<'a>(b: &'a [Stmt], f: &mut dyn FnMut(&'a Expr)) {
    for s in b {
        match &s.kind {
            StmtKind::Let { value, .. } => f(value),
            StmtKind::Assign { target, value, .. } => {
                f(target);
                f(value);
            }
            StmtKind::If { cond, then, els } => {
                f(cond);
                stmts_exprs(then, f);
                if let Some(e) = els {
                    stmts_exprs(e, f);
                }
            }
            StmtKind::While { cond, body } => {
                f(cond);
                stmts_exprs(body, f);
            }
            StmtKind::For { start, end, step, body, .. } => {
                f(start);
                f(end);
                if let Some(k) = step {
                    f(k);
                }
                stmts_exprs(body, f);
            }
            StmtKind::ForEach { iter, body, .. } => {
                f(iter);
                stmts_exprs(body, f);
            }
            StmtKind::Arena(body) => stmts_exprs(body, f),
            StmtKind::Ret(Some(e)) | StmtKind::Expr(e) => f(e),
            StmtKind::Ret(None) | StmtKind::Break | StmtKind::Continue => {}
        }
    }
}

/// The parameters of the lambdas and comprehensions in `e`.
fn lambda_names(e: &Expr, out: &mut HashSet<String>) {
    match &e.kind {
        ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Bool(_) | ExprKind::Str(_) | ExprKind::Char(_) | ExprKind::Var(_) => {}
        ExprKind::Interp(parts) => {
            for p in parts {
                if let InterpPart::Expr(x) = p {
                    lambda_names(x, out);
                }
            }
        }
        ExprKind::Unary(_, x) | ExprKind::Field(x, _) | ExprKind::Labeled(_, x) | ExprKind::Inout(x) | ExprKind::Fmt(x, _) => {
            lambda_names(x, out)
        }
        ExprKind::Binary(_, a, b) | ExprKind::Index(a, b) | ExprKind::In(a, b) | ExprKind::Coalesce(a, b) => {
            lambda_names(a, out);
            lambda_names(b, out);
        }
        ExprKind::Some(x) => lambda_names(x, out),
        ExprKind::None => {}
        ExprKind::Slice(b, lo, hi) => {
            lambda_names(b, out);
            lo.iter().chain(hi.iter()).for_each(|x| lambda_names(x, out));
        }
        ExprKind::Call(_, args) | ExprKind::Array(args) | ExprKind::Tuple(args) => args.iter().for_each(|x| lambda_names(x, out)),
        ExprKind::MapLit(kvs) => {
            for (k, v) in kvs {
                lambda_names(k, out);
                lambda_names(v, out);
            }
        }
        ExprKind::If(c, a, b) => {
            lambda_names(c, out);
            lambda_names(a, out);
            lambda_names(b, out);
        }
        ExprKind::Method(r, _, args) => {
            lambda_names(r, out);
            args.iter().for_each(|x| lambda_names(x, out));
        }
        ExprKind::Lambda(ps, body) => {
            out.extend(ps.iter().map(|(n, _)| n.clone()));
            lambda_names(body, out);
        }
        ExprKind::Comprehension(c) => {
            out.insert(c.var[0].0.clone());
            lambda_names(&c.elem, out);
            if let Some(x) = &c.cond {
                lambda_names(x, out);
            }
            match &c.src {
                CompSrc::Each(x) => lambda_names(x, out),
                CompSrc::Range(a, b, k) => {
                    lambda_names(a, out);
                    lambda_names(b, out);
                    if let Some(k) = k {
                        lambda_names(k, out);
                    }
                }
            }
        }
    }
}
