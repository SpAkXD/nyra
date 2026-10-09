//! An interpreter for the IR, used inside the compiler: `examples.rs` runs the `ex` examples of a
//! program with it at compile time. It never runs a program for `nyra run`.
//!
//! Interface: `Interp::new(module, limits)`, then `call(func, args)`, which gives the result or a
//! `Stop` (a runtime error, or a limit that was reached). `display`/`show` format a value as
//! `print` shows it and as Nyra code, `equal` compares two values like `==`.
//!
//! It follows the Rust runtime (`rt/rs/runtime.rs`) operation by operation: ints wrap, floats
//! print like JavaScript, string lengths and indexes count characters, and a runtime error has
//! the same code, message, hint and position. Reference counting (`Dup`, `Drop`, `Keep`) does
//! nothing here: values are shared (`Rc`) and a change copies a shared value first. An `inout`
//! argument is copied into the callee and back when it returns, which is the same as passing the
//! place itself: the checker allows no other way to reach that place during the call (E0237).
//!
//! Every statement costs a step, and so does every element or byte an operation makes, so a
//! budget of steps also bounds the time and the memory a run can take.

use std::cmp::Ordering;
use std::rc::Rc;

use super::{Arg, BinOp, Expr, Func, FuncId, Module, Place, PureFn, RtOp, Step, Stmt, StmtKind, Ty, UnOp};
use crate::ast::Span;

#[derive(Clone, Debug)]
pub enum Value {
    /// A local that holds nothing (not assigned yet, or freed).
    Unset,
    Int(i64),
    Float(f64),
    Bool(bool),
    Char(char),
    Str(Rc<String>),
    Arr(Rc<Vec<Value>>),
    /// A struct: its type id (`Ty::Struct(id)`) and its fields in declaration order.
    Struct(u32, Rc<Vec<Value>>),
}

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Statements run plus elements and bytes made.
    pub steps: u64,
    /// Calls that may be running at the same time (recursion).
    pub depth: usize,
}

/// A runtime error, as the program would report it.
#[derive(Clone, Debug)]
pub struct RuntimeError {
    pub code: &'static str,
    pub msg: String,
    pub hint: &'static str,
    pub span: Span,
    /// The function the error happened in.
    pub func: Option<FuncId>,
}

#[derive(Debug)]
pub enum Stop {
    Error(RuntimeError),
    /// The budget of steps ran out.
    Steps,
    /// Too many nested calls.
    Depth,
    /// Something the IR should never contain (a bug of the compiler, not of the program).
    Bug(#[allow(dead_code)] String),
}

fn fail(code: &'static str, msg: String, hint: &'static str, span: Span) -> Stop {
    Stop::Error(RuntimeError { code, msg, hint, span, func: None })
}

fn oob(i: i64, n: usize, span: Span) -> Stop {
    fail("E0240", format!("index {i} is out of bounds for length {n}"), "valid indexes are 0 to len - 1; compare with `.len()` first", span)
}

fn check_range(a: i64, b: i64, n: usize, span: Span) -> Result<(), Stop> {
    if a < 0 || a > b || b > n as i64 {
        return Err(fail("E0240", format!("range {a}..{b} is out of bounds for length {n}"), "a range a..b needs 0 <= a <= b <= len", span));
    }
    Ok(())
}

fn oom(span: Span) -> Stop {
    fail("E0249", "out of memory".into(), "the program needs more memory than the system gave it", span)
}

fn bug(what: &str) -> Stop {
    Stop::Bug(what.to_string())
}

enum Flow {
    Normal,
    Break,
    Continue,
    Return(Option<Value>),
}

/// A place with its indexes computed: `xs[i].name`.
enum At {
    Index(i64, Span),
    Field(usize),
}

pub struct Interp<'m> {
    m: &'m Module,
    limits: Limits,
    steps: u64,
    depth: usize,
    /// What `print` wrote, when it is kept (`capture`).
    out: Option<String>,
}

impl<'m> Interp<'m> {
    pub fn new(m: &'m Module, limits: Limits) -> Self {
        Interp { m, limits, steps: 0, depth: 0, out: None }
    }

    /// Keeps what `print` writes (otherwise it is dropped).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn capture(&mut self) {
        self.out = Some(String::new());
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn output(&self) -> &str {
        self.out.as_deref().unwrap_or("")
    }

    /// Calls `f` with the values of its parameters. Returns what it returns.
    pub fn call(&mut self, f: FuncId, args: Vec<Value>) -> Result<Option<Value>, Stop> {
        self.invoke(f, args).map(|(v, _)| v)
    }

    fn tick(&mut self, n: u64) -> Result<(), Stop> {
        self.steps = self.steps.saturating_add(n);
        if self.steps > self.limits.steps {
            return Err(Stop::Steps);
        }
        Ok(())
    }

    /// Runs a function; gives its result and its locals (the `inout` parameters go back).
    fn invoke(&mut self, fid: FuncId, args: Vec<Value>) -> Result<(Option<Value>, Vec<Value>), Stop> {
        let m = self.m;
        let f = m.funcs.get(fid.0 as usize).ok_or_else(|| bug("call of a function that does not exist"))?;
        if args.len() != f.params {
            return Err(bug("wrong number of arguments"));
        }
        if self.depth >= self.limits.depth {
            return Err(Stop::Depth);
        }
        self.depth += 1;
        let mut locals = args;
        locals.resize(f.locals.len(), Value::Unset);
        let flow = self.block(f, &mut locals, &f.body);
        self.depth -= 1;
        match flow {
            Ok(Flow::Return(v)) => Ok((v, locals)),
            Ok(_) if f.ret.is_none() => Ok((None, locals)),
            Ok(_) => Err(bug("a function with a result ended without `return`")),
            Err(Stop::Error(mut e)) => {
                e.func.get_or_insert(fid);
                Err(Stop::Error(e))
            }
            Err(e) => Err(e),
        }
    }

    fn block(&mut self, f: &Func, locals: &mut Vec<Value>, ss: &[Stmt]) -> Result<Flow, Stop> {
        for s in ss {
            match self.stmt(f, locals, s)? {
                Flow::Normal => {}
                other => return Ok(other),
            }
        }
        Ok(Flow::Normal)
    }

    fn stmt(&mut self, f: &Func, locals: &mut Vec<Value>, s: &Stmt) -> Result<Flow, Stop> {
        self.tick(1)?;
        let m = self.m;
        match &s.kind {
            StmtKind::Set(l, e) => {
                let v = eval(m, locals, e)?;
                locals[l.0 as usize] = v;
            }
            StmtKind::Call { dst, func, args } => {
                let mut vals = Vec::with_capacity(args.len());
                let mut places = Vec::new();
                for (k, a) in args.iter().enumerate() {
                    match a {
                        Arg::Val(e) => vals.push(eval(m, locals, e)?),
                        Arg::InOut(p) => {
                            let path = resolve(m, locals, p)?;
                            vals.push(self.place(locals, p.root.0 as usize, &path)?.clone());
                            places.push((k, p.root.0 as usize, path));
                        }
                    }
                }
                let (ret, mut callee) = self.invoke(*func, vals)?;
                for (k, root, path) in places {
                    let v = std::mem::replace(&mut callee[k], Value::Unset);
                    *self.place(locals, root, &path)? = v;
                }
                if let Some(d) = dst {
                    locals[d.0 as usize] = ret.ok_or_else(|| bug("the result of a function that returns nothing"))?;
                }
            }
            StmtKind::Op { dst, op, args } => {
                let vals = args.iter().map(|a| eval(m, locals, a)).collect::<Result<Vec<_>, _>>()?;
                let ty = dst.map(|d| f.local(d).ty);
                if let Some(v) = self.op(*op, vals, ty, s.span)? {
                    if let Some(d) = dst {
                        locals[d.0 as usize] = v;
                    }
                }
            }
            StmtKind::Store { place, value } => {
                let v = eval(m, locals, value)?;
                let path = resolve(m, locals, place)?;
                *self.place(locals, place.root.0 as usize, &path)? = v;
            }
            StmtKind::Mutate { dst, op, place, args } => {
                let vals = args.iter().map(|a| eval(m, locals, a)).collect::<Result<Vec<_>, _>>()?;
                let path = resolve(m, locals, place)?;
                let r = self.mutate(locals, place.root.0 as usize, &path, *op, vals, s.span)?;
                if let (Some(d), Some(v)) = (dst, r) {
                    locals[d.0 as usize] = v;
                }
            }
            StmtKind::If { cond, then, els } => {
                let branch = if truth(&eval(m, locals, cond)?)? { then } else { els };
                return self.block(f, locals, branch);
            }
            StmtKind::Loop { head, cond, body, step } => loop {
                self.tick(1)?;
                match self.block(f, locals, head)? {
                    Flow::Normal => {}
                    Flow::Break => break,
                    Flow::Return(v) => return Ok(Flow::Return(v)),
                    Flow::Continue => return Err(bug("`continue` in the head of a loop")),
                }
                if !truth(&eval(m, locals, cond)?)? {
                    break;
                }
                match self.block(f, locals, body)? {
                    Flow::Normal | Flow::Continue => {}
                    Flow::Break => break,
                    Flow::Return(v) => return Ok(Flow::Return(v)),
                }
                if let Flow::Return(v) = self.block(f, locals, step)? {
                    return Ok(Flow::Return(v));
                }
            },
            StmtKind::ForEach { var, iter, body } => {
                // the loop goes over the value as it was when it started
                let items: Vec<Value> = match eval(m, locals, iter)? {
                    Value::Str(s) => s.chars().map(Value::Char).collect(),
                    Value::Arr(xs) => xs.iter().cloned().collect(),
                    _ => return Err(bug("`for` over a value that is not a string or an array")),
                };
                for v in items {
                    self.tick(1)?;
                    locals[var.0 as usize] = v;
                    match self.block(f, locals, body)? {
                        Flow::Normal | Flow::Continue => {}
                        Flow::Break => break,
                        Flow::Return(v) => return Ok(Flow::Return(v)),
                    }
                }
            }
            StmtKind::Break => return Ok(Flow::Break),
            StmtKind::Continue => return Ok(Flow::Continue),
            StmtKind::Return(e) => {
                let v = match e {
                    Some(e) => Some(eval(m, locals, e)?),
                    None => None,
                };
                return Ok(Flow::Return(v));
            }
            StmtKind::Free(l) => locals[l.0 as usize] = Value::Unset,
            StmtKind::Dup(_) | StmtKind::Drop(_) | StmtKind::Keep(_) => {}
        }
        Ok(Flow::Normal)
    }

    /// The value at a place, to change it. A shared array or struct on the way is copied first
    /// (copy on write) and every index is checked (E0240 at the `[`).
    fn place<'v>(&mut self, locals: &'v mut [Value], root: usize, path: &[At]) -> Result<&'v mut Value, Stop> {
        let mut cur = locals.get_mut(root).ok_or_else(|| bug("a place in a local that does not exist"))?;
        for at in path {
            cur = match (at, cur) {
                (At::Index(i, span), Value::Arr(xs)) => {
                    if *i < 0 || *i >= xs.len() as i64 {
                        return Err(oob(*i, xs.len(), *span));
                    }
                    if Rc::strong_count(xs) > 1 {
                        self.tick(xs.len() as u64)?;
                    }
                    &mut Rc::make_mut(xs)[*i as usize]
                }
                (At::Field(k), Value::Struct(_, fields)) => {
                    Rc::make_mut(fields).get_mut(*k).ok_or_else(|| bug("a field that does not exist"))?
                }
                _ => return Err(bug("a place that does not match its value")),
            };
        }
        Ok(cur)
    }

    fn mutate(
        &mut self,
        locals: &mut [Value],
        root: usize,
        path: &[At],
        op: RtOp,
        args: Vec<Value>,
        span: Span,
    ) -> Result<Option<Value>, Stop> {
        let mut args = args.into_iter();
        let mut arg = || args.next().ok_or_else(|| bug("an operation without its operand"));
        // (the operands are computed before the place, like the backends do)
        let (a0, a1) = match op {
            RtOp::StrAppend | RtOp::ArrPush | RtOp::ArrRemove | RtOp::ArrAppend | RtOp::ArrSortBy => (Some(arg()?), None),
            RtOp::ArrInsert | RtOp::ArrSwap => (Some(arg()?), Some(arg()?)),
            _ => (None, None),
        };
        let size = |v: &Option<Value>| match v {
            Some(Value::Str(s)) => s.len() as u64,
            Some(Value::Arr(xs)) => xs.len() as u64,
            _ => 1,
        };
        self.tick(size(&a0))?;
        let shared = |v: &Value| match v {
            Value::Str(s) => Rc::strong_count(s) > 1,
            Value::Arr(xs) => Rc::strong_count(xs) > 1,
            _ => false,
        };
        let target = self.place(locals, root, path)?;
        let copy = match &*target {
            Value::Str(s) if shared(target) => s.len() as u64,
            Value::Arr(xs) if shared(target) => xs.len() as u64,
            _ => 0,
        };
        // (the copy of a shared value is counted after the change: `self` is borrowed by `target`)
        let result = match (op, target) {
            (RtOp::StrAppend, Value::Str(s)) => {
                match a0 {
                    Some(Value::Str(t)) => Rc::make_mut(s).push_str(&t),
                    Some(Value::Char(c)) => Rc::make_mut(s).push(c),
                    _ => return Err(bug("`+=` on a string with a value that is not text")),
                }
                None
            }
            (RtOp::ArrPush, Value::Arr(xs)) => {
                Rc::make_mut(xs).push(a0.unwrap_or(Value::Unset));
                None
            }
            (RtOp::ArrPop, Value::Arr(xs)) => match Rc::make_mut(xs).pop() {
                Some(v) => Some(v),
                None => return Err(fail("E0242", "pop() on an empty array".into(), "check `xs.len() > 0` first", span)),
            },
            (RtOp::ArrInsert, Value::Arr(xs)) => {
                let i = int(&a0)?;
                if i < 0 || i > xs.len() as i64 {
                    return Err(fail(
                        "E0240",
                        format!("index {i} is out of bounds for length {}", xs.len()),
                        "insert(i, x) needs 0 <= i <= len",
                        span,
                    ));
                }
                Rc::make_mut(xs).insert(i as usize, a1.unwrap_or(Value::Unset));
                None
            }
            (RtOp::ArrRemove, Value::Arr(xs)) => {
                let i = int(&a0)?;
                if i < 0 || i >= xs.len() as i64 {
                    return Err(oob(i, xs.len(), span));
                }
                Some(Rc::make_mut(xs).remove(i as usize))
            }
            (RtOp::ArrSwap, Value::Arr(xs)) => {
                let (i, j) = (int(&a0)?, int(&a1)?);
                for k in [i, j] {
                    if k < 0 || k >= xs.len() as i64 {
                        return Err(oob(k, xs.len(), span));
                    }
                }
                Rc::make_mut(xs).swap(i as usize, j as usize);
                None
            }
            (RtOp::ArrSort, Value::Arr(xs)) => {
                let v = Rc::make_mut(xs);
                if v.iter().any(|x| matches!(x, Value::Float(_))) {
                    // NaN after every number, like every backend
                    let lt = |x: f64, y: f64| x < y || (y.is_nan() && !x.is_nan());
                    v.sort_by(|a, b| match (a, b) {
                        (Value::Float(x), Value::Float(y)) if lt(*x, *y) => Ordering::Less,
                        (Value::Float(x), Value::Float(y)) if lt(*y, *x) => Ordering::Greater,
                        _ => Ordering::Equal,
                    });
                } else {
                    v.sort_by(|a, b| compare(a, b).unwrap_or(Ordering::Equal));
                }
                None
            }
            (RtOp::ArrSortBy, Value::Arr(xs)) => {
                // stable, by the parallel keys; NaN keys after every number, like every backend
                let Some(Value::Arr(ks)) = a0 else { return Err(bug("sort_by without its keys")) };
                let lt = |x: &Value, y: &Value| match (x, y) {
                    (Value::Float(a), Value::Float(b)) => a < b || (b.is_nan() && !a.is_nan()),
                    _ => compare(x, y) == Some(Ordering::Less),
                };
                let mut idx: Vec<usize> = (0..ks.len()).collect();
                idx.sort_by(|&p, &q| {
                    if lt(&ks[p], &ks[q]) {
                        Ordering::Less
                    } else if lt(&ks[q], &ks[p]) {
                        Ordering::Greater
                    } else {
                        Ordering::Equal
                    }
                });
                let old = xs.as_ref().clone();
                let v = Rc::make_mut(xs);
                for (k, &from) in idx.iter().enumerate() {
                    v[k] = old[from].clone();
                }
                None
            }
            (RtOp::ArrReverse, Value::Arr(xs)) => {
                Rc::make_mut(xs).reverse();
                None
            }
            (RtOp::ArrAppend, Value::Arr(xs)) => {
                let Some(Value::Arr(ys)) = a0 else { return Err(bug("`+=` on an array with a value that is not one")) };
                Rc::make_mut(xs).extend(ys.iter().cloned());
                None
            }
            (op, _) => return Err(bug(&format!("{} on a place of the wrong type", op.name()))),
        };
        let sort_cost = if op == RtOp::ArrSort { copy.max(1) } else { 0 };
        self.tick(copy + sort_cost)?;
        Ok(result)
    }

    /// An operation that makes a value (or prints). `ty` is the type of the destination.
    fn op(&mut self, op: RtOp, args: Vec<Value>, ty: Option<Ty>, span: Span) -> Result<Option<Value>, Stop> {
        let m = self.m;
        let s = |k: usize| -> Result<&str, Stop> {
            match args.get(k) {
                Some(Value::Str(s)) => Ok(s.as_str()),
                _ => Err(bug("an operand that should be a string")),
            }
        };
        let i = |k: usize| -> Result<i64, Stop> {
            match args.get(k) {
                Some(Value::Int(n)) => Ok(*n),
                _ => Err(bug("an operand that should be an int")),
            }
        };
        let arr = |k: usize| -> Result<&Rc<Vec<Value>>, Stop> {
            match args.get(k) {
                Some(Value::Arr(xs)) => Ok(xs),
                _ => Err(bug("an operand that should be an array")),
            }
        };
        let text = |v: String| Some(Value::Str(Rc::new(v)));
        let v = match op {
            RtOp::Print | RtOp::PrintNoLine | RtOp::Format => {
                let mut line = String::new();
                for a in &args {
                    line += &display(m, a);
                }
                self.tick(line.len() as u64)?;
                if op == RtOp::Format {
                    return Ok(text(line));
                }
                if let Some(out) = &mut self.out {
                    out.push_str(&line);
                    if op == RtOp::Print {
                        out.push('\n');
                    }
                }
                None
            }
            RtOp::DivInt | RtOp::RemInt => {
                let (a, b) = (i(0)?, i(1)?);
                if b == 0 {
                    return Err(fail("E0241", "division by zero".into(), "check the divisor first", span));
                }
                Some(Value::Int(if op == RtOp::DivInt { a.wrapping_div(b) } else { a.wrapping_rem(b) }))
            }
            RtOp::FloatToInt => {
                let Some(Value::Float(x)) = args.first() else { return Err(bug("int() of a value that is not a float")) };
                let x = *x;
                if x.is_nan() || x >= 9223372036854775807.0 || x < -9223372036854775808.0 {
                    return Err(fail(
                        "E0245",
                        format!("cannot convert {} to int", num(x)),
                        "int(x) needs a float that is not NaN and fits in an int",
                        span,
                    ));
                }
                Some(Value::Int(x as i64))
            }
            RtOp::StrConcat => {
                let (a, b) = (s(0)?, s(1)?);
                self.tick((a.len() + b.len()) as u64)?;
                text(format!("{a}{b}"))
            }
            RtOp::StrAt => {
                let (t, k) = (s(0)?, i(1)?);
                let n = t.chars().count();
                if k < 0 || k >= n as i64 {
                    return Err(oob(k, n, span));
                }
                Some(Value::Char(t.chars().nth(k as usize).unwrap_or('\0')))
            }
            RtOp::StrSlice => {
                let (t, a, b) = (s(0)?, i(1)?, i(2)?);
                check_range(a, b, t.chars().count(), span)?;
                self.tick((b - a) as u64)?;
                text(t.chars().skip(a as usize).take((b - a) as usize).collect())
            }
            RtOp::StrReplace => {
                let (t, old, new) = (s(0)?, s(1)?, s(2)?);
                if old.is_empty() {
                    return Err(fail("E0243", "replace() needs a non-empty pattern".into(), "the text to replace can't be \"\"", span));
                }
                let r = t.replace(old, new);
                self.tick(r.len() as u64)?;
                text(r)
            }
            RtOp::StrTrim => text(s(0)?.trim_matches(|c| matches!(c, ' ' | '\t' | '\n' | '\r')).to_string()),
            RtOp::StrUpper => text(s(0)?.to_ascii_uppercase()),
            RtOp::StrLower => text(s(0)?.to_ascii_lowercase()),
            RtOp::StrRepeat => {
                let (t, n) = (s(0)?, i(1)?);
                if n < 0 {
                    return Err(fail("E0243", format!("repeat count must be >= 0, got {n}"), "repeat(n) needs n >= 0", span));
                }
                if !t.is_empty() && n > 536870888 / t.len() as i64 {
                    return Err(oom(span));
                }
                self.tick(t.len() as u64 * n as u64)?;
                text(t.repeat(n as usize))
            }
            RtOp::StrToInt => Some(Value::Int(parse_int(s(0)?, span)?)),
            RtOp::StrToFloat => Some(Value::Float(parse_float(s(0)?, span)?)),
            RtOp::CharFrom => {
                let n = i(0)?;
                match char::from_u32(n as u32) {
                    Some(c) if (0..=1114111).contains(&n) => Some(Value::Char(c)),
                    _ => {
                        return Err(fail(
                            "E0246",
                            format!("char({n}): not a valid character code"),
                            "character codes go from 0 to 1114111, except 55296 to 57343",
                            span,
                        ))
                    }
                }
            }
            RtOp::StrChars | RtOp::StrCodes => {
                let t = s(0)?;
                self.tick(t.len() as u64)?;
                let items = if op == RtOp::StrChars {
                    t.chars().map(Value::Char).collect()
                } else {
                    t.chars().map(|c| Value::Int(c as i64)).collect()
                };
                Some(Value::Arr(Rc::new(items)))
            }
            RtOp::StrSplit => {
                let (t, sep) = (s(0)?, s(1)?);
                if sep.is_empty() {
                    return Err(fail("E0243", "split() needs a non-empty separator".into(), "for the characters of a string use `s.chars()`", span));
                }
                self.tick(t.len() as u64)?;
                Some(Value::Arr(Rc::new(t.split(sep).map(|p| Value::Str(Rc::new(p.to_string()))).collect())))
            }
            RtOp::CheckStep => {
                if i(0)? == 0 {
                    return Err(fail(
                        "E0243",
                        "range step must not be 0".into(),
                        "use a positive step to count up and a negative one to count down",
                        span,
                    ));
                }
                None
            }
            RtOp::StrPadLeft | RtOp::StrPadRight => {
                let (t, n) = (s(0)?, i(1)?);
                let Some(Value::Char(c)) = args.get(2) else { return Err(bug("pad without a char")) };
                let missing = n - t.chars().count() as i64;
                if missing <= 0 {
                    return Ok(text(t.to_string()));
                }
                if missing > 536870888 {
                    return Err(oom(Span { line: 0, col: 0 }));
                }
                self.tick(missing as u64)?;
                let fill: String = std::iter::repeat(*c).take(missing as usize).collect();
                text(if op == RtOp::StrPadLeft { fill + t } else { format!("{t}{fill}") })
            }
            RtOp::ArrNew => {
                self.tick(args.len() as u64)?;
                Some(Value::Arr(Rc::new(args)))
            }
            RtOp::StructNew => match ty {
                Some(Ty::Struct(id)) => Some(Value::Struct(id, Rc::new(args))),
                _ => return Err(bug("a struct made for a destination that is not a struct")),
            },
            RtOp::ArrGet => {
                let (xs, k) = (arr(0)?, i(1)?);
                if k < 0 || k >= xs.len() as i64 {
                    return Err(oob(k, xs.len(), span));
                }
                Some(xs[k as usize].clone())
            }
            RtOp::ArrSlice => {
                let (xs, a, b) = (arr(0)?, i(1)?, i(2)?);
                check_range(a, b, xs.len(), span)?;
                self.tick((b - a) as u64)?;
                Some(Value::Arr(Rc::new(xs[a as usize..b as usize].to_vec())))
            }
            RtOp::ArrRepeat => {
                let (xs, n) = (arr(0)?, i(1)?);
                if n < 0 {
                    return Err(fail("E0243", format!("repeat count must be >= 0, got {n}"), "repeat(n) needs n >= 0", span));
                }
                if !xs.is_empty() && n > 100000000 / xs.len() as i64 {
                    return Err(oom(span));
                }
                self.tick(xs.len() as u64 * n as u64)?;
                let mut out = Vec::with_capacity(xs.len() * n as usize);
                for _ in 0..n {
                    out.extend(xs.iter().cloned());
                }
                Some(Value::Arr(Rc::new(out)))
            }
            RtOp::ArrConcat => {
                let (a, b) = (arr(0)?, arr(1)?);
                self.tick((a.len() + b.len()) as u64)?;
                Some(Value::Arr(Rc::new(a.iter().chain(b.iter()).cloned().collect())))
            }
            RtOp::ArrJoin => {
                let (xs, sep) = (arr(0)?, s(1)?);
                let parts: Vec<String> = xs.iter().map(|v| display(m, v)).collect();
                let r = parts.join(sep);
                self.tick(r.len() as u64)?;
                text(r)
            }
            RtOp::CheckNonEmpty => {
                if i(0)? == 0 {
                    let msg = if i(1)? != 0 { "max() of an empty array" } else { "min() of an empty array" };
                    return Err(fail(
                        "E0247",
                        msg.to_string(),
                        "an empty array has no smallest or largest element: check `xs.len() > 0` first, or start from a value of your own with `fold`",
                        span,
                    ));
                }
                None
            }
            other => return Err(bug(&format!("{} outside of a `Mutate`", other.name()))),
        };
        Ok(v)
    }
}

fn int(v: &Option<Value>) -> Result<i64, Stop> {
    match v {
        Some(Value::Int(n)) => Ok(*n),
        _ => Err(bug("an operand that should be an int")),
    }
}

fn truth(v: &Value) -> Result<bool, Stop> {
    match v {
        Value::Bool(b) => Ok(*b),
        _ => Err(bug("a condition that is not a bool")),
    }
}

/// The indexes of a place, computed (they are pure expressions).
fn resolve(m: &Module, locals: &[Value], p: &Place) -> Result<Vec<At>, Stop> {
    p.path
        .iter()
        .map(|s| match s {
            Step::Index(e, span) => match eval(m, locals, e)? {
                Value::Int(i) => Ok(At::Index(i, *span)),
                _ => Err(bug("an index that is not an int")),
            },
            Step::Field(k) => Ok(At::Field(*k as usize)),
        })
        .collect()
}

/// A pure expression. It cannot fail: only a malformed IR can make this an error.
fn eval(m: &Module, locals: &[Value], e: &Expr) -> Result<Value, Stop> {
    Ok(match e {
        Expr::Int(n) => Value::Int(*n),
        Expr::Float(x) => Value::Float(*x),
        Expr::Bool(b) => Value::Bool(*b),
        Expr::Char(c) => Value::Char(char::from_u32(*c).ok_or_else(|| bug("a char literal that is not a character"))?),
        Expr::Str(id) => Value::Str(Rc::new(m.str(*id).to_string())),
        Expr::Local(l) => match locals.get(l.0 as usize) {
            Some(Value::Unset) | None => return Err(bug("a local read before it has a value")),
            Some(v) => v.clone(),
        },
        Expr::Unary(op, x) => match (op, eval(m, locals, x)?) {
            (UnOp::INeg, Value::Int(n)) => Value::Int(n.wrapping_neg()),
            (UnOp::FNeg, Value::Float(x)) => Value::Float(-x),
            (UnOp::Not, Value::Bool(b)) => Value::Bool(!b),
            _ => return Err(bug("a unary operator on the wrong type")),
        },
        Expr::Binary(op, a, b) => binary(*op, eval(m, locals, a)?, eval(m, locals, b)?)?,
        Expr::Select(c, a, b) => {
            if truth(&eval(m, locals, c)?)? {
                eval(m, locals, a)?
            } else {
                eval(m, locals, b)?
            }
        }
        Expr::IntToFloat(x) => match eval(m, locals, x)? {
            Value::Int(n) => Value::Float(n as f64),
            _ => return Err(bug("float() of a value that is not an int")),
        },
        Expr::Field(x, k, _) => match eval(m, locals, x)? {
            Value::Struct(_, fields) => fields.get(*k as usize).cloned().ok_or_else(|| bug("a field that does not exist"))?,
            _ => return Err(bug("a field of a value that is not a struct")),
        },
        Expr::Pure(p, args) => {
            let vals = args.iter().map(|a| eval(m, locals, a)).collect::<Result<Vec<_>, _>>()?;
            pure(*p, &vals)?
        }
    })
}

/// The text of a string operand, also for a char (`s.contains('a')`).
fn as_text(v: &Value) -> Result<String, Stop> {
    match v {
        Value::Str(s) => Ok(s.to_string()),
        Value::Char(c) => Ok(c.to_string()),
        _ => Err(bug("an operand that should be text")),
    }
}

fn pure(p: PureFn, a: &[Value]) -> Result<Value, Stop> {
    let ch = |k: usize| match a.get(k) {
        Some(Value::Char(c)) => Ok(*c),
        _ => Err(bug("an operand that should be a char")),
    };
    let text = |k: usize| a.get(k).ok_or_else(|| bug("a missing operand")).and_then(as_text);
    let arr = |k: usize| match a.get(k) {
        Some(Value::Arr(xs)) => Ok(xs.clone()),
        _ => Err(bug("an operand that should be an array")),
    };
    Ok(match p {
        PureFn::StrLen => Value::Int(text(0)?.chars().count() as i64),
        PureFn::StrContains => Value::Bool(text(0)?.contains(text(1)?.as_str())),
        PureFn::StrStartsWith => Value::Bool(text(0)?.starts_with(text(1)?.as_str())),
        PureFn::StrEndsWith => Value::Bool(text(0)?.ends_with(text(1)?.as_str())),
        PureFn::StrIndexOf => {
            let (s, t) = (text(0)?, text(1)?);
            Value::Int(match s.find(t.as_str()) {
                Some(j) => s[..j].chars().count() as i64,
                None => -1,
            })
        }
        PureFn::CharCode => Value::Int(ch(0)? as i64),
        PureFn::CharUpper => Value::Char(ch(0)?.to_ascii_uppercase()),
        PureFn::CharLower => Value::Char(ch(0)?.to_ascii_lowercase()),
        PureFn::CharIsDigit => Value::Bool(ch(0)?.is_ascii_digit()),
        PureFn::CharIsLetter => Value::Bool(ch(0)?.is_ascii_alphabetic()),
        PureFn::CharIsUpper => Value::Bool(ch(0)?.is_ascii_uppercase()),
        PureFn::CharIsLower => Value::Bool(ch(0)?.is_ascii_lowercase()),
        PureFn::CharIsSpace => Value::Bool(matches!(ch(0)?, ' ' | '\t' | '\n' | '\r')),
        PureFn::ArrLen => Value::Int(arr(0)?.len() as i64),
        PureFn::ArrContains => {
            let v = a.get(1).ok_or_else(|| bug("a missing operand"))?;
            Value::Bool(arr(0)?.iter().any(|x| equal(x, v)))
        }
        PureFn::ArrIndexOf => {
            let v = a.get(1).ok_or_else(|| bug("a missing operand"))?;
            Value::Int(arr(0)?.iter().position(|x| equal(x, v)).map_or(-1, |i| i as i64))
        }
        // maps are not evaluated in examples yet: such an example is skipped
        PureFn::MapLen | PureFn::MapHas => return Err(bug("a map in an example")),
    })
}

fn binary(op: BinOp, a: Value, b: Value) -> Result<Value, Stop> {
    use BinOp::*;
    Ok(match (op, &a, &b) {
        (IAdd, Value::Int(x), Value::Int(y)) => Value::Int(x.wrapping_add(*y)),
        (ISub, Value::Int(x), Value::Int(y)) => Value::Int(x.wrapping_sub(*y)),
        (IMul, Value::Int(x), Value::Int(y)) => Value::Int(x.wrapping_mul(*y)),
        (IDiv, Value::Int(x), Value::Int(y)) if *y != 0 => Value::Int(x.wrapping_div(*y)),
        (IRem, Value::Int(x), Value::Int(y)) if *y != 0 => Value::Int(x.wrapping_rem(*y)),
        (FAdd, Value::Float(x), Value::Float(y)) => Value::Float(x + y),
        (FSub, Value::Float(x), Value::Float(y)) => Value::Float(x - y),
        (FMul, Value::Float(x), Value::Float(y)) => Value::Float(x * y),
        (FDiv, Value::Float(x), Value::Float(y)) => Value::Float(x / y),
        (And, Value::Bool(x), Value::Bool(y)) => Value::Bool(*x && *y),
        (Or, Value::Bool(x), Value::Bool(y)) => Value::Bool(*x || *y),
        (IEq | FEq | BEq | CEq | SEq | DeepEq, _, _) => Value::Bool(equal(&a, &b)),
        (INe | FNe | BNe | CNe | SNe | DeepNe, _, _) => Value::Bool(!equal(&a, &b)),
        (ILt | FLt | CLt | SLt, _, _) => Value::Bool(compare(&a, &b) == Some(Ordering::Less)),
        (ILe | FLe | CLe | SLe, _, _) => Value::Bool(matches!(compare(&a, &b), Some(Ordering::Less | Ordering::Equal))),
        (IGt | FGt | CGt | SGt, _, _) => Value::Bool(compare(&a, &b) == Some(Ordering::Greater)),
        (IGe | FGe | CGe | SGe, _, _) => Value::Bool(matches!(compare(&a, &b), Some(Ordering::Greater | Ordering::Equal))),
        _ => return Err(bug(&format!("`{}` on operands of the wrong type", op.symbol()))),
    })
}

/// `==` of two values: strings, arrays and structs by content; a NaN equals nothing.
pub fn equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => x == y,
        (Value::Float(x), Value::Float(y)) => x == y,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Char(x), Value::Char(y)) => x == y,
        (Value::Str(x), Value::Str(y)) => x == y,
        (Value::Arr(x), Value::Arr(y)) => x.len() == y.len() && x.iter().zip(y.iter()).all(|(p, q)| equal(p, q)),
        (Value::Struct(s, x), Value::Struct(t, y)) => s == t && x.iter().zip(y.iter()).all(|(p, q)| equal(p, q)),
        _ => false,
    }
}

/// The order of two ints, floats, chars or strings (by code points); `None` for a NaN.
pub fn compare(a: &Value, b: &Value) -> Option<Ordering> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Some(x.cmp(y)),
        (Value::Float(x), Value::Float(y)) => x.partial_cmp(y),
        (Value::Char(x), Value::Char(y)) => Some(x.cmp(y)),
        (Value::Str(x), Value::Str(y)) => Some(x.as_str().cmp(y.as_str())),
        _ => None,
    }
}

/// A value as `print` shows it: text and chars as they are, arrays and structs as Nyra code.
pub fn display(m: &Module, v: &Value) -> String {
    match v {
        Value::Str(s) => s.to_string(),
        Value::Char(c) => c.to_string(),
        _ => show(m, v),
    }
}

/// A value as Nyra code: `"text"`, `'c'`, `[1, 2]`, `Point(x: 1, y: 2)`.
pub fn show(m: &Module, v: &Value) -> String {
    let mut out = String::new();
    show_in(m, v, &mut out);
    out
}

fn show_in(m: &Module, v: &Value, out: &mut String) {
    match v {
        Value::Unset => out.push('?'),
        Value::Int(n) => out.push_str(&n.to_string()),
        Value::Float(x) => out.push_str(&num(*x)),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Char(c) => quoted(out, &c.to_string(), '\''),
        Value::Str(s) => quoted(out, s, '"'),
        Value::Arr(xs) => {
            out.push('[');
            for (i, x) in xs.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                show_in(m, x, out);
            }
            out.push(']');
        }
        Value::Struct(id, fields) => {
            let Some(info) = m.structs.get(Ty::Struct(*id)) else {
                out.push('?');
                return;
            };
            out.push_str(&info.name);
            out.push('(');
            for (k, ((name, _), x)) in info.fields.iter().zip(fields.iter()).enumerate() {
                if k > 0 {
                    out.push_str(", ");
                }
                out.push_str(name);
                out.push_str(": ");
                show_in(m, x, out);
            }
            out.push(')');
        }
    }
}

fn quoted(out: &mut String, text: &str, quote: char) {
    out.push(quote);
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c => out.push(c),
        }
    }
    out.push(quote);
}

/// A float as JavaScript's String(x) shows it: the shortest digits that read back the same.
pub fn num(x: f64) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x == 0.0 {
        return "0".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "Infinity" } else { "-Infinity" }.into();
    }
    let sign = if x < 0.0 { "-" } else { "" };
    let e = format!("{:e}", x.abs());
    let (mant, exp) = e.split_once('e').unwrap_or((&e, "0"));
    let digits: String = mant.chars().filter(|c| *c != '.').collect();
    let k = digits.len() as i32;
    let n = exp.parse::<i32>().unwrap_or(0) + 1; // the value is 0.DIGITS * 10^n
    if k <= n && n <= 21 {
        format!("{sign}{digits}{}", "0".repeat((n - k) as usize))
    } else if 0 < n && n <= 21 {
        format!("{sign}{}.{}", &digits[..n as usize], &digits[n as usize..])
    } else if -6 < n && n <= 0 {
        format!("{sign}0.{}{digits}", "0".repeat((-n) as usize))
    } else {
        let rest = if k > 1 { format!(".{}", &digits[1..]) } else { String::new() };
        format!("{sign}{}{rest}e{}{}", &digits[..1], if n > 0 { "+" } else { "-" }, (n - 1).abs())
    }
}

/// The text of a string in an error message: control characters as escapes.
fn shown(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// int(s): digits with an optional `-` that fit in an int, nothing else.
fn parse_int(s: &str, span: Span) -> Result<i64, Stop> {
    let digits = s.strip_prefix('-').unwrap_or(s);
    if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
        if let Ok(v) = s.parse::<i64>() {
            return Ok(v);
        }
    }
    Err(fail(
        "E0244",
        format!("cannot parse \"{}\" as int", shown(s)),
        "int(s) accepts only digits with an optional `-`, e.g. \"-42\"",
        span,
    ))
}

/// float(s): -?[0-9]+(.[0-9]+)?([eE][+-]?[0-9]+)?
fn parse_float(s: &str, span: Span) -> Result<f64, Stop> {
    let b = s.as_bytes();
    let digits = |mut i: usize| -> Option<usize> {
        let start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        (i > start).then_some(i)
    };
    let ok = || -> Option<()> {
        let mut i = if b.first() == Some(&b'-') { 1 } else { 0 };
        i = digits(i)?;
        if b.get(i) == Some(&b'.') {
            i = digits(i + 1)?;
        }
        if matches!(b.get(i), Some(b'e' | b'E')) {
            i += 1;
            if matches!(b.get(i), Some(b'+' | b'-')) {
                i += 1;
            }
            i = digits(i)?;
        }
        (i == b.len()).then_some(())
    };
    match ok() {
        Some(()) => Ok(s.parse().unwrap_or(f64::NAN)),
        None => Err(fail(
            "E0244",
            format!("cannot parse \"{}\" as float", shown(s)),
            "float(s) accepts digits with an optional `-`, `.` part and exponent, e.g. \"-1.5e3\"",
            span,
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs `main` of a program in the interpreter: its output, and the runtime error if one stops it.
    /// None for a program that uses what the interpreter does not have yet: maps and the standard
    /// modules (examples that use them are skipped).
    fn run_main(src: &str) -> Option<(String, Option<RuntimeError>)> {
        let modules = src.lines().any(|l| l.starts_with("use "));
        let prog = crate::front(src).unwrap_or_else(|d| panic!("{d:?}"));
        let m = crate::ir::lower::lower(&prog).expect("lowers");
        std::thread::scope(|s| {
            std::thread::Builder::new()
                .stack_size(512 << 20)
                .spawn_scoped(s, || {
                    let mut it = Interp::new(&m, Limits { steps: 2_000_000_000, depth: 100_000 });
                    it.capture();
                    let r = it.call(m.main, Vec::new());
                    let err = match r {
                        Ok(_) => None,
                        Err(Stop::Error(e)) => Some(e),
                        Err(Stop::Bug(b)) if modules || b.contains("map") => return None,
                        Err(other) => panic!("stopped: {other:?}"),
                    };
                    Some((it.output().to_string(), err))
                })
                .unwrap()
                .join()
                .unwrap()
        })
    }

    fn files(dir: &str) -> Vec<std::path::PathBuf> {
        let mut v: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|e| e == "nyra"))
            .collect();
        v.sort();
        v
    }

    /// The interpreter must compute what the backends compute: every example program prints its
    /// `.out` file, the same check `tests/examples.rs` makes for C, JavaScript, Python, ...
    #[test]
    fn every_example_prints_its_expected_output() {
        let mut checked = 0;
        for path in files("examples") {
            let Ok(expected) = std::fs::read_to_string(path.with_extension("out")) else { continue };
            let src = std::fs::read_to_string(&path).unwrap();
            let Some((out, err)) = run_main(&src) else { continue };
            assert!(err.is_none(), "{}: {err:?}", path.display());
            assert_eq!(out, expected.replace("\r\n", "\n"), "{}", path.display());
            checked += 1;
        }
        assert!(checked >= 20);
    }

    /// ... and every runtime error test stops with its code, at its position, after its output.
    #[test]
    fn every_runtime_error_has_its_code_and_position() {
        for path in files("tests/runtime") {
            let src = std::fs::read_to_string(&path).unwrap();
            let expect = src.lines().next().and_then(|l| l.strip_prefix("// expect: ")).unwrap();
            let (code, at) = expect.trim().split_once(" at ").unwrap();
            let Some((out, err)) = run_main(&src) else { continue };
            let err = err.unwrap_or_else(|| panic!("{}: no runtime error", path.display()));
            assert_eq!(err.code, code, "{}: {}", path.display(), err.msg);
            assert_eq!(format!("{}:{}", err.span.line, err.span.col), at, "{}", path.display());
            let stdout = std::fs::read_to_string(path.with_extension("out")).unwrap_or_default();
            assert_eq!(out, stdout.replace("\r\n", "\n"), "{}", path.display());
        }
    }

    #[test]
    fn floats_print_like_javascript() {
        for (x, s) in [(0.1 + 0.2, "0.30000000000000004"), (1e21, "1e+21"), (1e-7, "1e-7"), (123.0, "123"), (-0.0, "0"), (2.5e-6, "0.0000025")] {
            assert_eq!(num(x), s);
        }
    }

    #[test]
    fn limits_stop_endless_loops_and_recursion() {
        let src = "fn spin() -> int {\n    while true { }\n    ret 0\n}\nfn down(n: int) -> int = down(n + 1)\nfn main() {\n    print(1)\n}\n";
        let prog = crate::front(src).unwrap();
        let m = crate::ir::lower::lower(&prog).unwrap();
        let id = |name: &str| FuncId(m.funcs.iter().position(|f| f.name == name).unwrap() as u32);
        let limits = Limits { steps: 10_000, depth: 100 };
        assert!(matches!(Interp::new(&m, limits).call(id("spin"), vec![]), Err(Stop::Steps)));
        assert!(matches!(Interp::new(&m, limits).call(id("down"), vec![Value::Int(0)]), Err(Stop::Depth)));
    }
}
