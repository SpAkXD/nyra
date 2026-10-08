//! Lambdas (`x => x * 2`) and the array methods that take them: `map`, `filter`, `count`, `any`,
//! `all`, `find_index`, `sort_by`, `fold`, plus `sum`, `min` and `max`. A lambda is not a value:
//! it is only an argument of these methods, and its parameters get their types from the array's
//! elements. Inside a lambda nothing can be changed (no mutating method, no `inout`), so lowering
//! may run a chain like `xs.filter(..).map(..).sum()` as one loop.

use super::{count, show, start, was_were, Checker, Decl};
use crate::ast::*;
use crate::check_v03 as v3;
use crate::diag::Diag;

/// E0213: a lambda where a value is needed.
pub(super) fn misplaced(span: Span) -> Diag {
    Diag::new("E0213", "a lambda is not a value: it can only be the argument of an array method", span).hint(
        "pass it straight to a method, e.g. `xs.map(x => x * 2)` or `xs.filter(x => x > 0)`; to name a computation, define a function: `fn double(x: int) -> int = x * 2`",
    )
}

/// E0214: something inside a lambda changes a variable.
pub(super) fn changes(e: &Expr, what: &str, span: Span) -> Diag {
    let shown = show(e).unwrap_or_else(|| "this value".into());
    Diag::new("E0214", format!("a lambda cannot {what} `{shown}`: inside a lambda, variables are read-only"), span).hint(
        "let the method build the result (`let ys = xs.map(x => x * 2)`), or change the variable in a `for` loop instead",
    )
}

/// A name for a lambda parameter that `taken` does not reject: `x2`, `x3`, ...
pub(super) fn fresh_name(name: &str, taken: impl Fn(&str) -> bool) -> String {
    (2..).map(|k| format!("{name}{k}")).find(|n| !taken(n)).unwrap_or_default()
}

/// How a method uses its lambda.
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    /// `map`: any value
    Value,
    /// `filter`, `count`, `any`, `all`, `find_index`: a `bool`
    Test,
    /// `sort_by`: an `int`, `float`, `str` or `char` key
    Key,
}

impl Checker {
    /// `recv.name(args)` for the methods of this module; `None` for every other method.
    pub(super) fn lambda_method(&mut self, recv: &mut Expr, rt: Type, name: &str, args: &mut [Expr], span: Span) -> Option<Type> {
        let elem = match rt {
            Type::Array(_) if v3::LAMBDA_METHODS.contains(&name) => rt.elem()?,
            Type::Str if v3::STR_LAMBDA_METHODS.contains(&name) => Type::Char,
            _ => return None,
        };
        let r = show(recv).unwrap_or_else(|| if rt == Type::Str { "s".into() } else { "xs".into() });
        let p = if rt == Type::Str { "c" } else { "x" };
        let example = match name {
            "sum" | "min" | "max" => format!("{r}.{name}()"),
            "fold" => format!("{r}.fold(0, (acc, {p}) => acc + {p})"),
            "map" => format!("{r}.map({p} => {p} * 2)"),
            "sort_by" => format!("{r}.sort_by({p} => {p})"),
            _ if rt == Type::Str => format!("{r}.{name}({p} => {p}.is_digit())"),
            _ => format!("{r}.{name}({p} => {p} > 0)"),
        };
        let want = match name {
            "sum" | "min" | "max" => 0,
            "fold" => 2,
            _ => 1,
        };
        if args.len() != want {
            for a in args.iter_mut() {
                if !matches!(a.kind, ExprKind::Lambda(..)) {
                    self.expr(a);
                }
            }
            let hint = format!("call it as `{example}`");
            self.errs.push(
                Diag::new(
                    "E0204",
                    format!("`.{name}()` takes {} but {} {} given", count(want, "argument"), args.len(), was_were(args.len())),
                    span,
                )
                .hint(hint),
            );
            return Some(Type::Unknown);
        }
        let sortable = |t: Type| matches!(t, Type::Int | Type::Float | Type::Str | Type::Char);
        let t = match name {
            "sum" | "min" | "max" => {
                let ok = if name == "sum" { matches!(elem, Type::Int | Type::Float) } else { sortable(elem) };
                if ok || elem.is_unknown() {
                    elem
                } else {
                    let (needs, hint) = if name == "sum" {
                        ("`[int]` or `[float]`", format!("add up a number of each element: `{r}.fold(0, (acc, x) => acc + ...)`"))
                    } else {
                        (
                            "`[int]`, `[float]`, `[str]` or `[char]`",
                            format!("compare a key of each element: `{r}.fold({r}[0], (best, x) => if key(x) {} key(best) {{ x }} else {{ best }})`", if name == "min" { "<" } else { ">" }),
                        )
                    };
                    self.errs.push(Diag::new("E0228", format!("`{name}` needs {needs}, found `{}`", rt.name()), span).hint(hint));
                    Type::Unknown
                }
            }
            "fold" => {
                let (init, rest) = args.split_first_mut()?;
                let acc = self.expr(init);
                if acc == Type::Void {
                    self.errs.push(
                        Diag::new("E0203", "the first argument of `.fold()` is the start value, but it returns nothing", init.span)
                            .hint(format!("start from a value: `{example}`")),
                    );
                }
                let acc = if acc == Type::Void { Type::Unknown } else { acc };
                let body = self.lambda_arg(&mut rest[0], &[acc, elem], name, &example, Kind::Value);
                if !body.is_unknown() && !acc.is_unknown() && body != acc && body != Type::Void {
                    let hint = if acc == Type::Int && body == Type::Float {
                        format!("start from a float, e.g. `{r}.fold(0.0, ...)`: the start value decides the type")
                    } else {
                        format!("the lambda must give the same type as the start value, `{}`", acc.name())
                    };
                    self.errs.push(
                        Diag::new(
                            "E0203",
                            format!("type mismatch in the lambda of `.fold()`: expected `{}`, found `{}`", acc.name(), body.name()),
                            lambda_body_span(&rest[0]),
                        )
                        .hint(hint),
                    );
                }
                acc
            }
            _ => {
                let kind = match name {
                    "map" => Kind::Value,
                    "sort_by" => Kind::Key,
                    _ => Kind::Test,
                };
                let body = self.lambda_arg(&mut args[0], &[elem], name, &example, kind);
                if body.is_unknown() {
                    return Some(Type::Unknown);
                }
                match name {
                    "map" => Type::array(body),
                    "filter" => rt,
                    "count" | "find_index" => Type::Int,
                    "any" | "all" => Type::Bool,
                    _ => {
                        // `sort_by` sorts its receiver in place, like `sort`
                        if v3::place_root(recv).is_some() {
                            self.check_place(recv, "call `.sort_by()` on", span);
                        } else {
                            self.errs.push(
                                Diag::new("E0229", "cannot call `.sort_by()` on a temporary value: it changes its receiver", span)
                                    .hint("store the value in a `var` first, then call the method on the variable"),
                            );
                        }
                        Type::Void
                    }
                }
            }
        };
        Some(t)
    }

    /// `[elem for x in src if cond]`: like `src.filter(x => cond).map(x => elem)`.
    pub(super) fn comprehension(&mut self, c: &mut Comp) -> Type {
        let (var, vspan) = c.var[0].clone();
        let elem = match &mut c.src {
            CompSrc::Range(a, b, k) => {
                for (e, which) in [(Some(a), "start"), (Some(b), "end"), (k.as_mut(), "step")] {
                    if let Some(e) = e {
                        let t = self.expr(e);
                        self.range_bound(t, e, which);
                    }
                }
                Type::Int
            }
            CompSrc::Each(src) => match self.expr(src) {
                t if t.is_unknown() => Type::Unknown,
                Type::Str => Type::Char,
                t @ Type::Array(_) => t.elem().unwrap_or(Type::Unknown),
                t => {
                    let hint = match (t, show(src)) {
                        (Type::Int, Some(s)) => format!("to count, use a range: `[... for {var} in 0..{s}]`"),
                        _ => "a comprehension goes over a range `a..b`, an array or a string".to_string(),
                    };
                    self.errs.push(
                        Diag::new("E0234", format!("cannot loop over `{}` in a comprehension", t.name()), src.span).hint(hint),
                    );
                    Type::Unknown
                }
            },
        };
        self.scopes.push(Default::default());
        self.declare(&var, elem, Decl::Lambda, vspan);
        self.lambda_depth += 1;
        if let Some(cond) = &mut c.cond {
            let t = self.expr(cond);
            self.cond(t, cond, "a comprehension's `if`");
        }
        let t = self.expr(&mut c.elem);
        self.lambda_depth -= 1;
        self.scopes.pop();
        if t == Type::Void {
            self.errs.push(
                Diag::new("E0203", "the element of a comprehension has no value: it returns nothing", start(&c.elem))
                    .hint("write the value each element becomes, e.g. `[x * 2 for x in xs]`; to do something for each element write a `for` loop"),
            );
            return Type::Unknown;
        }
        Type::array(t)
    }

    /// The lambda argument of a method: its parameters have the types `params`. Returns the type
    /// of its body, or `Unknown` after an error.
    fn lambda_arg(&mut self, a: &mut Expr, params: &[Type], m: &str, example: &str, kind: Kind) -> Type {
        let span = a.span;
        let ExprKind::Lambda(ps, body) = &mut a.kind else {
            let shown = show(a);
            // a function's name: call it inside a lambda
            let fname = match &a.kind {
                ExprKind::Var(n) if self.lookup(n).is_none() && self.fns.contains_key(n.as_str()) => Some(n.clone()),
                _ => None,
            };
            let t = if fname.is_some() { Type::Unknown } else { self.expr(a) };
            let p = if params.last() == Some(&Type::Char) { "c" } else { "x" };
            let hint = match (fname, shown) {
                (Some(f), _) if params.len() == 2 => format!("call the function in a lambda: `(acc, {p}) => {f}(acc, {p})`"),
                (Some(f), _) => format!("call the function in a lambda: `{p} => {f}({p})`"),
                (None, Some(v)) if m == "count" && t == params[0] => {
                    format!("`.count()` takes a test: to count the elements equal to `{v}` write `.count({p} => {p} == {v})`")
                }
                (None, Some(v)) if m == "find_index" && t == params[0] => {
                    format!("`.find_index()` takes a test; to find the value `{v}` itself write `.index_of({v})`")
                }
                _ => format!("write a lambda: `{example}`"),
            };
            self.errs.push(Diag::new("E0215", format!("`.{m}()` needs a lambda here, e.g. `{example}`"), span).hint(hint));
            return Type::Unknown;
        };
        if ps.len() != params.len() {
            let names: Vec<&str> = if params.len() == 1 { vec!["x"] } else { vec!["acc", "x"] };
            self.errs.push(
                Diag::new(
                    "E0215",
                    format!("the lambda of `.{m}()` takes {}, found {}", count(params.len(), "parameter"), ps.len()),
                    span,
                )
                .hint(if params.len() == 1 {
                    format!("one parameter, the element: `{example}`")
                } else {
                    format!("two parameters, the value so far and the element: `({}) => ...`", names.join(", "))
                }),
            );
        }
        self.scopes.push(Default::default());
        for (k, (n, s)) in ps.iter().enumerate() {
            let t = params.get(k).copied().unwrap_or(Type::Unknown);
            self.declare(n, t, Decl::Lambda, *s);
        }
        self.lambda_depth += 1;
        let t = self.expr(body);
        self.lambda_depth -= 1;
        self.scopes.pop();
        let bspan = start(body);
        a.ty = t;
        if ps.len() != params.len() || t.is_unknown() {
            return Type::Unknown;
        }
        if t == Type::Void {
            self.errs.push(
                Diag::new("E0215", format!("the lambda of `.{m}()` gives no value: its body returns nothing"), bspan)
                    .hint(format!("the body is the value the method uses, e.g. `{example}`; to do something for each element write a `for` loop")),
            );
            return Type::Unknown;
        }
        match kind {
            Kind::Test if t != Type::Bool => {
                let p = &ps[0].0;
                let hint = match t {
                    Type::Int | Type::Float => format!("compare, e.g. `{p} => {p} > 0`"),
                    Type::Str => format!("compare, e.g. `{p} => {p} != \"\"`"),
                    _ => format!("give a `bool`, e.g. `{example}`"),
                };
                self.errs.push(
                    Diag::new("E0203", format!("the lambda of `.{m}()` must give a `bool`, found `{}`", t.name()), bspan).hint(hint),
                );
                Type::Unknown
            }
            Kind::Key if !matches!(t, Type::Int | Type::Float | Type::Str | Type::Char) => {
                self.errs.push(
                    Diag::new("E0228", format!("`sort_by` needs a key of type `int`, `float`, `str` or `char`, found `{}`", t.name()), bspan)
                        .hint("sort by one field or a number made from the element, e.g. `ps.sort_by(p => p.age)`"),
                );
                Type::Unknown
            }
            _ => t,
        }
    }
}

/// Where the body of a lambda argument starts (the argument itself when it is not a lambda).
fn lambda_body_span(a: &Expr) -> Span {
    match &a.kind {
        ExprKind::Lambda(_, body) => start(body),
        _ => a.span,
    }
}
