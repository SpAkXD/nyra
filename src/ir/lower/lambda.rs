//! Lowers the methods that take a lambda (and `sum`, `min`, `max`) to plain loops: no closure
//! exists at run time, so every backend gets them from the IR it already knows.
//!
//! A chain of `map` and `filter` steps and the method that ends it run as **one** loop over the
//! first array: `xs.filter(x => x > 0).map(x => x * x).sum()` becomes
//!
//! ```text
//! acc = 0
//! for x in xs { if x > 0 { y = x * x; acc = acc + y } }
//! ```
//!
//! so each element goes through every step before the next element starts, and no array is
//! built in between. A lambda reads variables but cannot change them (the checker makes sure),
//! so the loop can run where the call is, in the statement's left-to-right order.

use super::{ast, BinOp, Expr, LocalId, Lower, Place, RtOp, Scope, Span, Stmt, StmtKind, Ty, Type, UnOp};
use crate::check::data;

/// True if `recv.name(..)` is lowered here.
pub(super) fn is_chain_method(recv: Ty, name: &str) -> bool {
    match recv {
        Ty::Array(_) => data::LAMBDA_METHODS.contains(&name),
        Ty::Str => data::STR_LAMBDA_METHODS.contains(&name),
        _ => false,
    }
}

/// A `map` or `filter` step of a chain: a lambda's parameter and body (or a comprehension's
/// variable and its element or condition).
struct Step<'e> {
    filter: bool,
    /// `all`: the step keeps the elements that fail the test
    negate: bool,
    params: &'e [(String, Span)],
    body: &'e ast::Expr,
}

impl<'e> Step<'e> {
    fn of(lambda: &'e ast::Expr, filter: bool, negate: bool) -> Self {
        let (params, body) = parts(lambda);
        Step { filter, negate, params, body }
    }
}

/// What the end of the chain does with each element that comes through.
enum Sink<'e> {
    /// the chain's own result: an array of what comes through
    Collect {
        acc: LocalId,
    },
    Sum {
        acc: LocalId,
        float: bool,
    },
    Count {
        acc: LocalId,
    },
    /// `any`: true and stop at the first element; `all` sets false (its step is negated)
    Stop {
        acc: LocalId,
        value: bool,
    },
    FindIndex {
        acc: LocalId,
        pos: LocalId,
        lambda: &'e ast::Expr,
    },
    /// `min` / `max`: `n` counts the elements, `m` is the best so far
    Best {
        m: LocalId,
        n: LocalId,
        max: bool,
        elem: Ty,
    },
    Fold {
        acc: LocalId,
        lambda: &'e ast::Expr,
    },
    /// `find`: the first element that passes, as an optional (`acc` starts as `none`)
    Find {
        acc: LocalId,
    },
}

/// The parameters and the body of a lambda argument (the checker allows nothing else).
fn parts(lambda: &ast::Expr) -> (&[(String, Span)], &ast::Expr) {
    match &lambda.kind {
        ast::ExprKind::Lambda(ps, body) => (ps, body),
        _ => unreachable!("the checker allows only a lambda here"),
    }
}

/// `xs.map(f).filter(g)` as `xs` and its steps, first step first.
fn peel(mut e: &ast::Expr) -> (&ast::Expr, Vec<Step<'_>>) {
    let mut steps = Vec::new();
    while let ast::ExprKind::Method(r, name, args) = &e.kind {
        if !matches!(r.ty, Type::Array(_) | Type::Str) || !matches!(name.as_str(), "map" | "filter") {
            break;
        }
        steps.push(Step::of(&args[0], name == "filter", false));
        e = r;
    }
    steps.reverse();
    (e, steps)
}

fn st(kind: StmtKind, span: Span) -> Stmt {
    Stmt { kind, span }
}

fn local(l: LocalId) -> Box<Expr> {
    Box::new(Expr::Local(l))
}

impl Lower<'_> {
    /// `recv.name(args)` for a method of `is_chain_method`.
    pub(super) fn chain_method(
        &mut self,
        recv: &ast::Expr,
        name: &str,
        args: &[ast::Expr],
        e: &ast::Expr,
        out: &mut Vec<Stmt>,
    ) -> Expr {
        if name == "sort_by" {
            return self.sort_by(recv, &args[0], e.span, out);
        }
        if matches!(name, "sorted_by" | "min_by" | "max_by") {
            return self.keyed(recv, name, &args[0], e, out);
        }
        let (src, mut steps) = peel(recv);
        let span = e.span;
        let lambda = args.last();
        match name {
            "map" | "filter" | "count" | "any" | "find" => steps.push(Step::of(&args[0], name != "map", false)),
            "all" => steps.push(Step::of(&args[0], true, true)),
            _ => {}
        }
        // the receiver first, then the start value of `fold` (left to right)
        let mut it = self.expr(src, None, out);
        if name == "fold" && super::mutates(&args[0]) {
            it = self.snapshot(it, src.ty, src.span, out);
        }
        let it = match it {
            Expr::Local(_) => it,
            v => {
                // a field or a literal: the loop reads it from a local (borrowed, nothing changes it)
                let t = self.temp(src.ty);
                out.push(st(StmtKind::Set(t, v), span));
                Expr::Local(t)
            }
        };
        let int = |n: i64| Expr::Int(n);
        let sink = match name {
            "map" | "filter" => {
                let Expr::Local(acc) = self.op(RtOp::ArrNew, Vec::new(), e.ty, None, span, out) else { unreachable!() };
                Sink::Collect { acc }
            }
            "sum" | "count" | "find_index" => {
                let acc = self.temp(e.ty);
                let float = e.ty == Type::Float;
                let start = match name {
                    "find_index" => int(-1),
                    _ if float => Expr::Float(0.0),
                    _ => int(0),
                };
                out.push(st(StmtKind::Set(acc, start), span));
                match name {
                    "sum" => Sink::Sum { acc, float },
                    "count" => Sink::Count { acc },
                    _ => {
                        let pos = self.temp(Ty::Int);
                        out.push(st(StmtKind::Set(pos, int(0)), span));
                        Sink::FindIndex { acc, pos, lambda: &args[0] }
                    }
                }
            }
            "find" => {
                let inner = e.ty.option_inner().expect("`find` gives an optional");
                let none = self.default_value(inner, span, out);
                let Expr::Local(acc) = self.op(RtOp::StructNew, vec![Expr::Bool(false), none], e.ty, None, span, out) else {
                    unreachable!()
                };
                Sink::Find { acc }
            }
            "any" | "all" => {
                let acc = self.temp(Ty::Bool);
                out.push(st(StmtKind::Set(acc, Expr::Bool(name == "all")), span));
                Sink::Stop { acc, value: name == "any" }
            }
            "min" | "max" => {
                let (m, n) = (self.temp(e.ty), self.temp(Ty::Int));
                let none = match e.ty {
                    Type::Float => Expr::Float(0.0),
                    Type::Char => Expr::Char(0),
                    Type::Str => Expr::Str(self.strs.intern("")),
                    _ => int(0),
                };
                out.push(st(StmtKind::Set(m, none), span));
                out.push(st(StmtKind::Set(n, int(0)), span));
                Sink::Best { m, n, max: name == "max", elem: e.ty }
            }
            _ => {
                // `fold`: the accumulator owns its value
                let v = self.expr(&args[0], None, out);
                let acc = self.temp(e.ty);
                // (the receiver's temporaries stay pending: the loop still reads them)
                self.init(acc, v, span, out);
                Sink::Fold { acc, lambda: lambda.expect("two arguments") }
            }
        };

        // the loop: its body releases what each round makes
        let saved = std::mem::take(&mut self.pending);
        let live = std::mem::take(&mut self.chain_live);
        let elem = match src.ty {
            Type::Str => Ty::Char,
            t => t.elem().expect("the checker allows strings and arrays"),
        };
        let first = steps.first().map(|s| s.params).or(match &sink {
            Sink::FindIndex { lambda, .. } | Sink::Fold { lambda, .. } => Some(parts(lambda).0),
            _ => None,
        });
        // the loop variable takes the name of the first lambda's element parameter
        let var_name = first.and_then(|ps| ps.last()).map(|p| p.0.clone());
        let x = self.new_local(var_name, elem);
        let mut body = Vec::new();
        self.steps(&steps, Expr::Local(x), elem, &sink, span, &mut body);
        out.push(st(StmtKind::ForEach { var: x, iter: it, body }, span));
        self.chain_live = live;
        self.pending = saved;

        match sink {
            Sink::Collect { acc }
            | Sink::Sum { acc, .. }
            | Sink::Count { acc }
            | Sink::Stop { acc, .. }
            | Sink::Find { acc }
            | Sink::FindIndex { acc, .. } => Expr::Local(acc),
            Sink::Best { m, n, max, elem } => {
                let which = int(i64::from(max));
                out.push(st(StmtKind::Op { dst: None, op: RtOp::CheckNonEmpty, args: vec![Expr::Local(n), which] }, span));
                if self.managed(elem) {
                    self.pending.push(m);
                }
                Expr::Local(m)
            }
            Sink::Fold { acc, .. } => {
                if self.managed(e.ty) {
                    self.pending.push(acc);
                }
                Expr::Local(acc)
            }
        }
    }

    /// `[elem for x in src if cond]`: one loop, like `src.filter(x => cond).map(x => elem)`.
    pub(super) fn comprehension(&mut self, c: &ast::Comp, e: &ast::Expr, out: &mut Vec<Stmt>) -> Expr {
        let span = e.span;
        let mut steps = Vec::new();
        if let Some(cond) = &c.cond {
            steps.push(Step { filter: true, negate: false, params: &c.var, body: cond });
        }
        steps.push(Step { filter: false, negate: false, params: &c.var, body: &c.elem });
        let Expr::Local(acc) = self.op(RtOp::ArrNew, Vec::new(), e.ty, None, span, out) else { unreachable!() };
        // then the source: a range's bounds, or the array
        let mut range = None;
        let mut each = None;
        match &c.src {
            ast::CompSrc::Range(a, b, k) => {
                let i = self.new_local(Some(c.var[0].0.clone()), Ty::Int);
                let (cond, next) = self.range(i, a, b, k.as_ref(), span, out);
                range = Some((i, cond, next));
            }
            ast::CompSrc::Each(src) => {
                let v = self.expr(src, None, out);
                let it = match v {
                    Expr::Local(_) => v,
                    v => {
                        let t = self.temp(src.ty);
                        out.push(st(StmtKind::Set(t, v), span));
                        Expr::Local(t)
                    }
                };
                let elem = match src.ty {
                    Type::Str => Ty::Char,
                    t => t.elem().expect("the checker allows strings and arrays"),
                };
                each = Some((it, elem));
            }
        }
        let sink = Sink::Collect { acc };
        let saved = std::mem::take(&mut self.pending);
        let live = std::mem::take(&mut self.chain_live);
        let mut body = Vec::new();
        if let Some((i, cond, next)) = range {
            self.steps(&steps, Expr::Local(i), Ty::Int, &sink, span, &mut body);
            let next = st(StmtKind::Set(i, next), span);
            out.push(st(StmtKind::Loop { head: Vec::new(), cond, body, step: vec![next] }, span));
        } else if let Some((it, elem)) = each {
            let x = self.new_local(Some(c.var[0].0.clone()), elem);
            self.steps(&steps, Expr::Local(x), elem, &sink, span, &mut body);
            out.push(st(StmtKind::ForEach { var: x, iter: it, body }, span));
        }
        self.chain_live = live;
        self.pending = saved;
        Expr::Local(acc)
    }

    /// Makes `name` (a lambda parameter) mean the value `v` of type `t`.
    fn bind(&mut self, name: &str, v: Expr, t: Ty, span: Span, out: &mut Vec<Stmt>) {
        let id = match v {
            Expr::Local(l) => l,
            v => {
                let l = self.new_local(Some(name.to_string()), t);
                out.push(st(StmtKind::Set(l, v), span));
                l
            }
        };
        self.scopes.last_mut().expect("pushed by the caller").names.insert(name.to_string(), id);
    }

    /// The value of a lambda's body with its parameters bound to `args`.
    fn apply(&mut self, lambda: &ast::Expr, args: &[(Expr, Ty)], out: &mut Vec<Stmt>) -> Expr {
        let (ps, body) = parts(lambda);
        self.apply_to(ps, body, args, out)
    }

    /// The value of `body` with `ps` bound to `args`.
    fn apply_to(&mut self, ps: &[(String, Span)], body: &ast::Expr, args: &[(Expr, Ty)], out: &mut Vec<Stmt>) -> Expr {
        self.scopes.push(Scope::default());
        for ((p, at), (v, t)) in ps.iter().zip(args) {
            self.bind(p, v.clone(), *t, *at, out);
        }
        let v = self.expr(body, None, out);
        self.scopes.pop();
        v
    }

    /// A test: its temporaries are released before the branch that uses it.
    fn test(&mut self, ps: &[(String, Span)], body: &ast::Expr, arg: (Expr, Ty), out: &mut Vec<Stmt>) -> Expr {
        let c = self.apply_to(ps, body, &[arg], out);
        if self.pending.is_empty() {
            return c;
        }
        let span = body.span;
        let b = self.temp(Ty::Bool);
        out.push(st(StmtKind::Set(b, c), span));
        self.end_statement(span, out);
        Expr::Local(b)
    }

    /// The steps from `steps[0]` on, for the element value `cur` of type `t`.
    fn steps(&mut self, steps: &[Step], cur: Expr, t: Ty, sink: &Sink, span: Span, out: &mut Vec<Stmt>) {
        let Some((step, rest)) = steps.split_first() else {
            return self.sink(sink, cur, t, span, out);
        };
        if step.filter {
            let c = self.test(step.params, step.body, (cur.clone(), t), out);
            let c = if step.negate { Expr::Unary(UnOp::Not, Box::new(c)) } else { c };
            let mut then = Vec::new();
            self.steps(rest, cur, t, sink, span, &mut then);
            out.push(st(StmtKind::If { cond: c, then, els: Vec::new() }, span));
            return;
        }
        // `map`: the new value (and what it is made from) lives until the rest of the chain used it
        let v = self.apply_to(step.params, step.body, &[(cur, t)], out);
        let mine = std::mem::take(&mut self.pending);
        let mark = self.chain_live.len();
        self.chain_live.extend(&mine);
        self.steps(rest, v, step.body.ty, sink, span, out);
        self.chain_live.truncate(mark);
        if !out.last().is_some_and(|s| matches!(s.kind, StmtKind::Break)) {
            for l in mine.into_iter().rev() {
                out.push(st(StmtKind::Drop(l), span));
            }
        }
    }

    /// Leaves the chain's loop: the values of its `map` steps are released first.
    fn stop(&mut self, span: Span, out: &mut Vec<Stmt>) {
        for l in self.chain_live.iter().rev() {
            out.push(st(StmtKind::Drop(*l), span));
        }
        out.push(st(StmtKind::Break, span));
    }

    /// What the end of the chain does with the element value `cur` of type `t`.
    fn sink(&mut self, sink: &Sink, cur: Expr, t: Ty, span: Span, out: &mut Vec<Stmt>) {
        let add = |acc: LocalId, v: Expr, op: BinOp| StmtKind::Set(acc, Expr::Binary(op, local(acc), Box::new(v)));
        match *sink {
            Sink::Collect { acc } => {
                out.push(st(StmtKind::Mutate { dst: None, op: RtOp::ArrPush, place: Place::local(acc), args: vec![cur] }, span));
            }
            Sink::Sum { acc, float: true } => out.push(st(add(acc, cur, BinOp::FAdd), span)),
            // an int sum may overflow: the checked operation
            Sink::Sum { acc, float: false } => {
                out.push(st(StmtKind::Op { dst: Some(acc), op: RtOp::AddInt, args: vec![Expr::Local(acc), cur] }, span));
            }
            Sink::Count { acc } => out.push(st(add(acc, Expr::Int(1), BinOp::IAdd), span)),
            Sink::Stop { acc, value } => {
                out.push(st(StmtKind::Set(acc, Expr::Bool(value)), span));
                self.stop(span, out);
            }
            Sink::Find { acc } => {
                // the first element that passed: the optional now holds it (`none` held a default to release)
                if self.managed(self.ty_of(acc)) {
                    out.push(st(StmtKind::Drop(acc), span));
                }
                out.push(st(StmtKind::Op { dst: Some(acc), op: RtOp::StructNew, args: vec![Expr::Bool(true), cur] }, span));
                self.stop(span, out);
            }
            Sink::FindIndex { acc, pos, lambda } => {
                let (ps, body) = parts(lambda);
                let c = self.test(ps, body, (cur, t), out);
                let mut then = vec![st(StmtKind::Set(acc, Expr::Local(pos)), span)];
                self.stop(span, &mut then);
                out.push(st(StmtKind::If { cond: c, then, els: Vec::new() }, span));
                out.push(st(add(pos, Expr::Int(1), BinOp::IAdd), span));
            }
            Sink::Best { m, n, max, elem } => {
                // like `min(a, b)` / `max(a, b)` from left to right: the first one wins a tie
                let lt = match elem {
                    Type::Float => BinOp::FLt,
                    Type::Char => BinOp::CLt,
                    Type::Str => BinOp::SLt,
                    _ => BinOp::ILt,
                };
                let better = if max {
                    Expr::Binary(lt, local(m), Box::new(cur.clone()))
                } else {
                    Expr::Binary(lt, Box::new(cur.clone()), local(m))
                };
                let first = Expr::Binary(BinOp::IEq, local(n), Box::new(Expr::Int(0)));
                let cond = Expr::Binary(BinOp::Or, Box::new(first), Box::new(better));
                if self.managed(elem) {
                    // the best value so far is owned: it may come from a `map` step whose value goes
                    let tmp = self.temp(elem);
                    let then = vec![
                        st(StmtKind::Set(tmp, cur), span),
                        st(StmtKind::Dup(tmp), span),
                        st(StmtKind::Drop(m), span),
                        st(StmtKind::Set(m, Expr::Local(tmp)), span),
                    ];
                    out.push(st(StmtKind::If { cond, then, els: Vec::new() }, span));
                } else {
                    out.push(st(StmtKind::Set(m, Expr::Select(Box::new(cond), Box::new(cur), local(m))), span));
                }
                out.push(st(add(n, Expr::Int(1), BinOp::IAdd), span));
            }
            Sink::Fold { acc, lambda } => {
                let at = self.ty_of(acc);
                let v = self.apply(lambda, &[(Expr::Local(acc), at), (cur, t)], out);
                if self.managed(at) {
                    // like `acc = v` for a variable: the new value gets its owner before the old one goes
                    if self.take(&v) || matches!(v, Expr::Str(_)) {
                        out.push(st(StmtKind::Drop(acc), span));
                        out.push(st(StmtKind::Set(acc, v), span));
                    } else {
                        let tmp = self.temp(at);
                        out.push(st(StmtKind::Set(tmp, v), span));
                        out.push(st(StmtKind::Dup(tmp), span));
                        out.push(st(StmtKind::Drop(acc), span));
                        out.push(st(StmtKind::Set(acc, Expr::Local(tmp)), span));
                    }
                } else {
                    // the value may read temporaries of this round: store it before they go
                    out.push(st(StmtKind::Set(acc, v), span));
                }
                self.end_statement(span, out);
            }
        }
    }

    /// `xs.sorted_by(x => key)`, `xs.min_by(..)`, `xs.max_by(..)`: the keys are computed first, once
    /// per element and in order; then a sorted copy is made, or a generated helper function picks
    /// the element with the smallest or largest key.
    fn keyed(&mut self, recv: &ast::Expr, name: &str, lambda: &ast::Expr, e: &ast::Expr, out: &mut Vec<Stmt>) -> Expr {
        let span = e.span;
        let key = parts(lambda).1.ty;
        let v = self.expr(recv, None, out);
        if name == "sorted_by" {
            let len = Expr::Pure(super::PureFn::ArrLen, vec![v.clone()]);
            let Expr::Local(copy) = self.op(RtOp::ArrSlice, vec![v, Expr::Int(0), len], recv.ty, None, span, out) else { unreachable!() };
            let ks = self.keys_of(Expr::Local(copy), recv.ty, lambda, span, out);
            self.sort_with_keys(Place::local(copy), recv.ty, key, ks, span, out);
            return Expr::Local(copy);
        }
        let it = match v {
            Expr::Local(_) => v,
            v => {
                let t = self.temp(recv.ty);
                out.push(st(StmtKind::Set(t, v), span));
                Expr::Local(t)
            }
        };
        let ks = self.keys_of(it.clone(), recv.ty, lambda, span, out);
        let h = super::H::Best(name == "max_by", recv.ty, key);
        self.call_helper(h, vec![super::Arg::Val(it), super::Arg::Val(Expr::Local(ks))], e.ty, None, span, out)
    }

    /// The array of the keys `lambda` gives for the elements of the array `it`.
    fn keys_of(&mut self, it: Expr, arr: Ty, lambda: &ast::Expr, span: Span, out: &mut Vec<Stmt>) -> LocalId {
        let elem = arr.elem().expect("the checker allows arrays");
        let key = parts(lambda).1.ty;
        let Expr::Local(ks) = self.op(RtOp::ArrNew, Vec::new(), Type::array(key), None, span, out) else { unreachable!() };
        let saved = std::mem::take(&mut self.pending);
        let live = std::mem::take(&mut self.chain_live);
        let ps = parts(lambda).0;
        let x = self.new_local(Some(ps[0].0.clone()), elem);
        let mut body = Vec::new();
        let v = self.apply(lambda, &[(Expr::Local(x), elem)], &mut body);
        body.push(st(StmtKind::Mutate { dst: None, op: RtOp::ArrPush, place: Place::local(ks), args: vec![v] }, span));
        self.end_statement(span, &mut body);
        out.push(st(StmtKind::ForEach { var: x, iter: it, body }, span));
        self.chain_live = live;
        self.pending = saved;
        ks
    }

    /// Sorts the array in `place` by `ks`, the keys of its elements.
    fn sort_with_keys(&mut self, place: Place, arr: Ty, key: Ty, ks: LocalId, span: Span, out: &mut Vec<Stmt>) {
        if key.is_tuple() {
            self.forget(place.root);
            self.call_helper(super::H::SortKeyed(arr, key), vec![super::Arg::InOut(place), super::Arg::Val(Expr::Local(ks))], Ty::Void, None, span, out);
        } else {
            out.push(st(StmtKind::Mutate { dst: None, op: RtOp::ArrSortBy, place, args: vec![Expr::Local(ks)] }, span));
        }
    }

    /// `xs.sort_by(x => key)`: the keys are computed first, once per element and in order, then
    /// the runtime sorts the array by them (stable, the same merge sort as `sort`).
    fn sort_by(&mut self, recv: &ast::Expr, lambda: &ast::Expr, span: Span, out: &mut Vec<Stmt>) -> Expr {
        let place = self.place(recv, true, out);
        let cur = self.read(&place, recv.ty, out);
        let key = parts(lambda).1.ty;
        let it = match cur {
            Expr::Local(_) => cur,
            v => {
                let t = self.temp(recv.ty);
                out.push(st(StmtKind::Set(t, v), span));
                Expr::Local(t)
            }
        };
        let ks = self.keys_of(it, recv.ty, lambda, span, out);
        self.sort_with_keys(place, recv.ty, key, ks, span, out);
        Expr::Bool(false)
    }
}
