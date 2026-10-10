//! An interpreter for the IR, used inside the compiler. `examples.rs` runs the `ex` examples of a
//! program with it at compile time, and `nyra run --interp` / `--sandbox` (and the MCP tool
//! `nyra_run` with `sandbox: true`) run whole programs with it: no child process, no C compiler,
//! and hard limits on steps, memory, output, call depth and time.
//!
//! Interface: `Interp::new(module, limits)`, then `call(func, args)`, which gives the result or a
//! `Stop` (a runtime error, or a limit that was reached). `display`/`show` format a value as
//! `print` shows it and as Nyra code, `equal` compares two values like `==`. A program that
//! reads input, files or arguments needs a `Host` (`ir/host.rs`) and says where its output goes
//! (`Out`); the standard library and JSON are in `host.rs` and `jsonrt.rs`.
//!
//! It follows the Rust runtime (`rt/rs/runtime.rs`) operation by operation: int overflow is E0255, floats
//! print like JavaScript, string lengths and indexes count characters, and a runtime error has
//! the same code, message, hint and position. Reference counting (`Dup`, `Drop`, `Keep`) does
//! nothing here: values are shared (`Rc`) and a change copies a shared value first. An `inout`
//! argument is copied into the callee and back when it returns, which is the same as passing the
//! place itself: the checker allows no other way to reach that place during the call (E0237).
//!
//! Every statement costs a step, and so does every element or byte an operation makes, so a
//! budget of steps also bounds the time and the memory a run can take. The memory limit is
//! checked against the heap the interpreter's thread really uses (`crate::mem`) every few thousand
//! steps and before an operation that makes a large value; the output cap at every write.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use super::host::{Host, Out};
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
    Arr(Rc<Items>),
    /// A struct: its type id (`Ty::Struct(id)`) and its fields in declaration order.
    Struct(u32, Rc<Items>),
    Map(Rc<MapVal>),
}

impl Value {
    /// A new array.
    pub fn arr(items: Vec<Value>) -> Value {
        Value::Arr(Rc::new(Items(items)))
    }

    /// A new struct value.
    pub fn strukt(id: u32, fields: Vec<Value>) -> Value {
        Value::Struct(id, Rc::new(Items(fields)))
    }

    fn nested(&self) -> bool {
        matches!(self, Value::Arr(_) | Value::Struct(..) | Value::Map(_))
    }
}

/// The elements of an array or the fields of a struct. Dropping values that nest deeply (a tree
/// that a loop built one level at a time) must not recurse once per level, or a program could
/// overflow the stack of the process that interprets it: a value released last is taken apart
/// with a work list instead.
#[derive(Clone, Debug)]
pub struct Items(Vec<Value>);

impl std::ops::Deref for Items {
    type Target = Vec<Value>;
    fn deref(&self) -> &Vec<Value> {
        &self.0
    }
}

impl std::ops::DerefMut for Items {
    fn deref_mut(&mut self) -> &mut Vec<Value> {
        &mut self.0
    }
}

impl Drop for Items {
    fn drop(&mut self) {
        if self.0.iter().any(Value::nested) {
            dismantle(std::mem::take(&mut self.0));
        }
    }
}

/// Drops `work` and everything below it without recursion: a nested value that nobody else
/// holds gives up its parts to the list.
fn dismantle(mut work: Vec<Value>) {
    while let Some(v) = work.pop() {
        match v {
            Value::Arr(rc) | Value::Struct(_, rc) => {
                if let Ok(mut inner) = Rc::try_unwrap(rc) {
                    work.append(&mut inner.0);
                }
            }
            Value::Map(rc) => {
                if let Ok(mut inner) = Rc::try_unwrap(rc) {
                    work.extend(inner.ents.drain(..).flatten().map(|(_, v)| v));
                }
            }
            _ => {}
        }
    }
}

/// The key of a map entry: an int, a string, a char or a bool.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum MapKey {
    Int(i64),
    Str(Rc<String>),
    Char(char),
    Bool(bool),
}

/// A map: entries in insertion order (a removed one is a gap until the next compaction) and an
/// index from key to position, like the maps of the runtimes.
#[derive(Clone, Debug, Default)]
pub struct MapVal {
    ents: Vec<Option<(Value, Value)>>,
    index: HashMap<MapKey, usize>,
    live: usize,
}

impl Drop for MapVal {
    fn drop(&mut self) {
        if self.ents.iter().flatten().any(|(_, v)| v.nested()) {
            dismantle(self.ents.drain(..).flatten().map(|(_, v)| v).collect());
        }
    }
}

impl MapVal {
    fn key(k: &Value) -> Result<MapKey, Stop> {
        Ok(match k {
            Value::Int(n) => MapKey::Int(*n),
            Value::Str(s) => MapKey::Str(s.clone()),
            Value::Char(c) => MapKey::Char(*c),
            Value::Bool(b) => MapKey::Bool(*b),
            _ => return Err(bug("a map key that is not an int, str, char or bool")),
        })
    }

    fn len(&self) -> usize {
        self.live
    }

    fn get_mut(&mut self, k: &Value) -> Result<Option<&mut Value>, Stop> {
        Ok(match self.index.get(&Self::key(k)?) {
            Some(&i) => self.ents[i].as_mut().map(|e| &mut e.1),
            None => None,
        })
    }

    fn get(&self, k: &Value) -> Result<Option<&Value>, Stop> {
        Ok(self.index.get(&Self::key(k)?).and_then(|&i| self.ents[i].as_ref().map(|e| &e.1)))
    }

    fn set(&mut self, k: Value, v: Value) -> Result<(), Stop> {
        let key = Self::key(&k)?;
        if let Some(&i) = self.index.get(&key) {
            self.ents[i] = Some((k, v));
            return Ok(());
        }
        self.index.insert(key, self.ents.len());
        self.ents.push(Some((k, v)));
        self.live += 1;
        Ok(())
    }

    fn remove(&mut self, k: &Value) -> Result<(), Stop> {
        let Some(i) = self.index.remove(&Self::key(k)?) else { return Ok(()) };
        self.ents[i] = None;
        self.live -= 1;
        if self.ents.len() > 8 && self.live < self.ents.len() / 2 {
            let ents: Vec<(Value, Value)> = self.ents.drain(..).flatten().collect();
            self.index.clear();
            self.live = 0;
            for (k, v) in ents {
                self.set(k, v)?;
            }
        }
        Ok(())
    }

    fn iter(&self) -> impl Iterator<Item = &(Value, Value)> {
        self.ents.iter().flatten()
    }
}

/// The limits of a run. A limit that is reached stops the program with a runtime error
/// (E0355 to E0359) that says which one.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Statements run plus elements and bytes made.
    pub steps: u64,
    /// Calls that may be running at the same time (recursion).
    pub depth: usize,
    /// Bytes of heap memory the run may grow by.
    pub memory: u64,
    /// Bytes the program may print.
    pub output: u64,
    /// Milliseconds of real time (0: no limit).
    pub wall_ms: u64,
}

impl Limits {
    /// Steps and depth only (examples): the other limits do not apply.
    pub const fn new(steps: u64, depth: usize) -> Limits {
        Limits { steps, depth, memory: u64::MAX, output: u64::MAX, wall_ms: 0 }
    }

    pub const fn memory(mut self, bytes: u64) -> Limits {
        self.memory = bytes;
        self
    }
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
    /// (boxed: a `Result` that carries a `Stop` is returned for every expression evaluated)
    Error(Box<RuntimeError>),
    /// The budget of steps ran out.
    Steps,
    /// Too many nested calls.
    Depth,
    /// The program grew the heap beyond its limit.
    Memory,
    /// The program printed more than its limit.
    Output,
    /// The real-time limit passed.
    Time,
    /// `os.exit(code)`.
    Exit(i32),
    /// Something the IR should never contain (a bug of the compiler, not of the program), or an
    /// operation this host does not do (a file operation in an example).
    Bug(#[allow(dead_code)] String),
}

/// int + - * / and negation outside the 64-bit range (E0255), worded like the runtimes.
fn overflow(a: i64, op: &str, b: i64, span: Span) -> Stop {
    let msg = if op == "~" {
        format!("int overflow: -({a}) does not fit in 64 bits")
    } else {
        format!("int overflow: {a} {op} {b} does not fit in 64 bits")
    };
    fail(
        "E0255",
        msg,
        "an int holds -9223372036854775808 to 9223372036854775807: use smaller values, or keep a running value small with `%` (e.g. `h = (h * 31 + x) % 1000000007`)",
        span,
    )
}

pub(super) fn fail(code: &'static str, msg: String, hint: &'static str, span: Span) -> Stop {
    Stop::Error(Box::new(RuntimeError { code, msg, hint, span, func: None }))
}

fn oob(i: i64, n: usize, span: Span) -> Stop {
    fail(
        "E0240",
        format!("index {i} is out of bounds for length {n}"),
        "valid indexes are 0 to len - 1; compare with `.len()` first",
        span,
    )
}

fn check_range(a: i64, b: i64, n: usize, span: Span) -> Result<(), Stop> {
    if a < 0 || a > b || b > n as i64 {
        return Err(fail(
            "E0240",
            format!("range {a}..{b} is out of bounds for length {n}"),
            "a range a..b needs 0 <= a <= b <= len",
            span,
        ));
    }
    Ok(())
}

fn oom(span: Span) -> Stop {
    fail("E0249", "out of memory".into(), "the program needs more memory than the system gave it", span)
}

pub(super) fn bug(what: &str) -> Stop {
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
    /// `m[k]` changed in place (E0248 at the `[` when the key is missing)
    Key(Value, Span),
}

pub struct Interp<'m> {
    m: &'m Module,
    limits: Limits,
    steps: u64,
    depth: usize,
    /// The steps at which memory and time are checked next.
    next_check: u64,
    /// The heap in use when the run started (`crate::mem::live`).
    mem_base: i64,
    started: Instant,
    /// The statement that ran last: where a limit that stops the program is reported.
    span: Span,
    host: Host,
    out: Out,
    /// The string literals of the module, made once (a literal is a shared `Rc`).
    strs: Vec<Rc<String>>,
}

impl<'m> Interp<'m> {
    /// An interpreter for examples: a pure host (no files, input or clocks) and no output.
    pub fn new(m: &'m Module, limits: Limits) -> Self {
        Interp::with_host(m, limits, Host::pure(), Out::discard())
    }

    pub fn with_host(m: &'m Module, limits: Limits, host: Host, mut out: Out) -> Self {
        out.cap = limits.output;
        Interp {
            m,
            limits,
            steps: 0,
            depth: 0,
            next_check: 0,
            mem_base: crate::mem::live(),
            started: Instant::now(),
            span: Span { line: 0, col: 0 },
            host,
            out,
            strs: m.strs.iter().map(|t| Rc::new(t.clone())).collect(),
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn output(&self) -> &str {
        self.out.kept.as_deref().unwrap_or("")
    }

    /// Writes what the program printed that is still buffered.
    pub fn flush(&mut self) {
        self.out.flush();
    }

    /// Steps run so far.
    pub fn steps(&self) -> u64 {
        self.steps
    }

    /// The runtime error a limit that stopped the program stands for (`None` for a `Stop` that is
    /// not one: an error of the program has its own, `os.exit` is no error).
    pub fn limit_error(&self, stop: &Stop) -> Option<RuntimeError> {
        let (code, msg, hint): (&'static str, String, &'static str) = match stop {
            Stop::Steps => (
                "E0355",
                format!("step limit reached: the program ran more than {} steps", self.limits.steps),
                "an endless loop or recursion? Otherwise raise the limit (nyra run --sandbox --fuel N; nyra_run: fuel)",
            ),
            Stop::Memory => (
                "E0356",
                format!("memory limit reached: the program used more than {} bytes of memory", self.limits.memory),
                "free values you no longer need, or build less at once; raise the limit with --max-memory (nyra_run: max_memory)",
            ),
            Stop::Output => (
                "E0357",
                format!("output limit reached: the program printed more than {} bytes", self.limits.output),
                "print less, or raise the limit with --max-output (nyra_run: max_output)",
            ),
            Stop::Depth => (
                "E0358",
                format!("call depth limit reached: more than {} calls were nested", self.limits.depth),
                "a recursive function that never reaches its base case? Otherwise raise the limit with --max-depth",
            ),
            Stop::Time => (
                "E0359",
                format!("time limit reached: the program ran longer than {} ms", self.limits.wall_ms),
                "raise the limit with --max-time MS (nyra_run: timeout_ms), or look for an endless loop",
            ),
            _ => return None,
        };
        Some(RuntimeError { code, msg, hint, span: self.span, func: None })
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
        if self.steps >= self.next_check {
            self.check()?;
        }
        Ok(())
    }

    /// The memory and the time, every few thousand steps (or after a big operation).
    fn check(&mut self) -> Result<(), Stop> {
        self.next_check = self.steps.saturating_add(4096);
        if self.used() > self.limits.memory {
            return Err(Stop::Memory);
        }
        if self.limits.wall_ms > 0 && self.started.elapsed() > Duration::from_millis(self.limits.wall_ms) {
            return Err(Stop::Time);
        }
        Ok(())
    }

    /// Bytes of heap the run has added so far.
    fn used(&self) -> u64 {
        (crate::mem::live() - self.mem_base).max(0) as u64
    }

    /// Before an operation makes a value of about `bytes` bytes: it must fit under the limit.
    fn reserve(&self, bytes: u64) -> Result<(), Stop> {
        if self.limits.memory != u64::MAX && self.used().saturating_add(bytes) > self.limits.memory {
            return Err(Stop::Memory);
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
        self.span = s.span;
        self.tick(1)?;
        let m = self.m;
        match &s.kind {
            StmtKind::Set(l, e) => {
                let v = eval(&self.strs, locals, e)?;
                locals[l.0 as usize] = v;
            }
            StmtKind::Call { dst, func, args } => {
                // (the callee's locals follow its parameters in this vector: one allocation per call)
                let mut vals = Vec::with_capacity(m.funcs.get(func.0 as usize).map_or(args.len(), |c| c.locals.len().max(args.len())));
                let mut places = Vec::new();
                for (k, a) in args.iter().enumerate() {
                    match a {
                        Arg::Val(e) => vals.push(eval(&self.strs, locals, e)?),
                        Arg::InOut(p) => {
                            let path = resolve(&self.strs, locals, p)?;
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
                // the arithmetic that loops are made of, without the general machinery; a result
                // that is an error (overflow, a zero divisor) is made by the general path below
                if let (RtOp::AddInt | RtOp::SubInt | RtOp::MulInt | RtOp::DivInt | RtOp::RemInt, [a, b], Some(d)) =
                    (*op, args.as_slice(), dst)
                {
                    if let (Value::Int(x), Value::Int(y)) = (eval(&self.strs, locals, a)?, eval(&self.strs, locals, b)?) {
                        let r = match op {
                            RtOp::AddInt => x.checked_add(y),
                            RtOp::SubInt => x.checked_sub(y),
                            RtOp::MulInt => x.checked_mul(y),
                            RtOp::DivInt if y != 0 => x.checked_div(y),
                            RtOp::RemInt if y != 0 => Some(x.wrapping_rem(y)),
                            _ => None,
                        };
                        if let Some(r) = r {
                            locals[d.0 as usize] = Value::Int(r);
                            return Ok(Flow::Normal);
                        }
                    }
                }
                let ty = dst.map(|d| f.local(d).ty);
                // (most operations have few operands: they go in a buffer on the stack)
                let r = if args.len() <= 4 {
                    let mut buf = [Value::Unset, Value::Unset, Value::Unset, Value::Unset];
                    for (k, a) in args.iter().enumerate() {
                        buf[k] = eval(&self.strs, locals, a)?;
                    }
                    self.op(*op, &buf[..args.len()], ty, s.span)?
                } else {
                    let vals = args.iter().map(|a| eval(&self.strs, locals, a)).collect::<Result<Vec<_>, _>>()?;
                    self.op(*op, &vals, ty, s.span)?
                };
                if let (Some(v), Some(d)) = (r, dst) {
                    locals[d.0 as usize] = v;
                }
            }
            StmtKind::Store { place, value } => {
                let v = eval(&self.strs, locals, value)?;
                let path = resolve(&self.strs, locals, place)?;
                *self.place(locals, place.root.0 as usize, &path)? = v;
            }
            StmtKind::Mutate { dst, op, place, args } => {
                let vals = args.iter().map(|a| eval(&self.strs, locals, a)).collect::<Result<Vec<_>, _>>()?;
                let path = resolve(&self.strs, locals, place)?;
                let r = self.mutate(locals, place.root.0 as usize, &path, *op, vals, s.span)?;
                if let (Some(d), Some(v)) = (dst, r) {
                    locals[d.0 as usize] = v;
                }
            }
            StmtKind::If { cond, then, els } => {
                let branch = if truth(&eval(&self.strs, locals, cond)?)? { then } else { els };
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
                if !truth(&eval(&self.strs, locals, cond)?)? {
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
                let items: Vec<Value> = match eval(&self.strs, locals, iter)? {
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
                    Some(e) => Some(eval(&self.strs, locals, e)?),
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
                (At::Key(k, span), Value::Map(mv)) => {
                    if mv.get(k)?.is_none() {
                        return Err(fail(
                            "E0248",
                            format!("key {} is not in the map", show(self.m, k)),
                            "check with `m.has(k)` first, or read it with `m.get(k, default)`",
                            *span,
                        ));
                    }
                    if Rc::strong_count(mv) > 1 {
                        self.tick(mv.len() as u64)?;
                    }
                    Rc::make_mut(mv).get_mut(k)?.ok_or_else(|| bug("a map entry that vanished"))?
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
            RtOp::StrAppend | RtOp::ArrPush | RtOp::ArrRemove | RtOp::ArrAppend | RtOp::ArrSortBy | RtOp::MapRemove => {
                (Some(arg()?), None)
            }
            RtOp::ArrInsert | RtOp::ArrSwap | RtOp::MapSet => (Some(arg()?), Some(arg()?)),
            _ => (None, None),
        };
        let size = |v: &Option<Value>| match v {
            Some(Value::Str(s)) => s.len() as u64,
            Some(Value::Arr(xs)) => xs.len() as u64,
            Some(Value::Map(mv)) => mv.len() as u64,
            _ => 1,
        };
        self.tick(size(&a0))?;
        let shared = |v: &Value| match v {
            Value::Str(s) => Rc::strong_count(s) > 1,
            Value::Arr(xs) => Rc::strong_count(xs) > 1,
            Value::Map(mv) => Rc::strong_count(mv) > 1,
            _ => false,
        };
        let target = self.place(locals, root, path)?;
        let copy = match &*target {
            Value::Str(s) if shared(target) => s.len() as u64,
            Value::Arr(xs) if shared(target) => xs.len() as u64,
            Value::Map(mv) if shared(target) => mv.len() as u64,
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
            (RtOp::MapSet, Value::Map(mv)) => {
                let (k, v) = (a0.unwrap_or(Value::Unset), a1.unwrap_or(Value::Unset));
                Rc::make_mut(mv).set(k, v)?;
                None
            }
            (RtOp::MapRemove, Value::Map(mv)) => {
                Rc::make_mut(mv).remove(&a0.unwrap_or(Value::Unset))?;
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
    fn op(&mut self, op: RtOp, args: &[Value], ty: Option<Ty>, span: Span) -> Result<Option<Value>, Stop> {
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
        let arr = |k: usize| -> Result<&Rc<Items>, Stop> {
            match args.get(k) {
                Some(Value::Arr(xs)) => Ok(xs),
                _ => Err(bug("an operand that should be an array")),
            }
        };
        let text = |v: String| Some(Value::Str(Rc::new(v)));
        let v = match op {
            RtOp::Print | RtOp::PrintNoLine | RtOp::Format => {
                let mut line = String::new();
                for a in args {
                    line += &display(m, a);
                }
                self.tick(line.len() as u64)?;
                if op == RtOp::Format {
                    return Ok(text(line));
                }
                if op == RtOp::Print {
                    line.push('\n');
                }
                self.out.write(&line)?;
                None
            }
            RtOp::DivInt | RtOp::RemInt => {
                let (a, b) = (i(0)?, i(1)?);
                if b == 0 {
                    return Err(fail("E0241", "division by zero".into(), "check the divisor first", span));
                }
                if op == RtOp::DivInt {
                    Some(Value::Int(a.checked_div(b).ok_or_else(|| overflow(a, "/", b, span))?))
                } else {
                    Some(Value::Int(a.wrapping_rem(b)))
                }
            }
            RtOp::AddInt => {
                Some(Value::Int(i(0)?.checked_add(i(1)?).ok_or_else(|| overflow(i(0).unwrap_or(0), "+", i(1).unwrap_or(0), span))?))
            }
            RtOp::SubInt => {
                Some(Value::Int(i(0)?.checked_sub(i(1)?).ok_or_else(|| overflow(i(0).unwrap_or(0), "-", i(1).unwrap_or(0), span))?))
            }
            RtOp::MulInt => {
                Some(Value::Int(i(0)?.checked_mul(i(1)?).ok_or_else(|| overflow(i(0).unwrap_or(0), "*", i(1).unwrap_or(0), span))?))
            }
            RtOp::NegInt => Some(Value::Int(i(0)?.checked_neg().ok_or_else(|| overflow(i(0).unwrap_or(0), "~", 0, span))?)),
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
                self.reserve((a.len() + b.len()) as u64)?;
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
                    return Err(fail(
                        "E0243",
                        "replace() needs a non-empty pattern".into(),
                        "the text to replace can't be \"\"",
                        span,
                    ));
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
                self.reserve(t.len() as u64 * n as u64)?;
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
                self.reserve(t.len() as u64 * 16)?;
                self.tick(t.len() as u64)?;
                let items = if op == RtOp::StrChars {
                    t.chars().map(Value::Char).collect()
                } else {
                    t.chars().map(|c| Value::Int(c as i64)).collect()
                };
                Some(Value::arr(items))
            }
            RtOp::StrSplit => {
                let (t, sep) = (s(0)?, s(1)?);
                if sep.is_empty() {
                    return Err(fail(
                        "E0243",
                        "split() needs a non-empty separator".into(),
                        "for the characters of a string use `s.chars()`",
                        span,
                    ));
                }
                self.reserve(t.len() as u64 * 16)?;
                self.tick(t.len() as u64)?;
                Some(Value::arr(t.split(sep).map(|p| Value::Str(Rc::new(p.to_string()))).collect()))
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
                    return Err(oom(span));
                }
                self.reserve(missing as u64)?;
                self.tick(missing as u64)?;
                let fill: String = std::iter::repeat_n(*c, missing as usize).collect();
                text(if op == RtOp::StrPadLeft { fill + t } else { format!("{t}{fill}") })
            }
            RtOp::ArrNew => {
                self.tick(args.len() as u64)?;
                Some(Value::arr(args.to_vec()))
            }
            RtOp::StructNew => match ty {
                Some(Ty::Struct(id)) => Some(Value::strukt(id, args.to_vec())),
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
                self.reserve((b - a) as u64 * 16)?;
                self.tick((b - a) as u64)?;
                Some(Value::arr(xs[a as usize..b as usize].to_vec()))
            }
            RtOp::ArrRepeat => {
                let (xs, n) = (arr(0)?, i(1)?);
                if n < 0 {
                    return Err(fail("E0243", format!("repeat count must be >= 0, got {n}"), "repeat(n) needs n >= 0", span));
                }
                if !xs.is_empty() && n > 100000000 / xs.len() as i64 {
                    return Err(oom(span));
                }
                self.reserve(xs.len() as u64 * n as u64 * 16)?;
                self.tick(xs.len() as u64 * n as u64)?;
                let mut out = Vec::with_capacity(xs.len() * n as usize);
                for _ in 0..n {
                    out.extend(xs.iter().cloned());
                }
                Some(Value::arr(out))
            }
            RtOp::ArrConcat => {
                let (a, b) = (arr(0)?, arr(1)?);
                self.reserve((a.len() + b.len()) as u64 * 16)?;
                self.tick((a.len() + b.len()) as u64)?;
                Some(Value::arr(a.iter().chain(b.iter()).cloned().collect()))
            }
            RtOp::ArrJoin => {
                let (xs, sep) = (arr(0)?, s(1)?);
                let parts: Vec<String> = xs.iter().map(|v| display(m, v)).collect();
                let r = parts.join(sep);
                self.tick(r.len() as u64)?;
                text(r)
            }
            RtOp::CheckSome => {
                let Some(Value::Bool(has)) = args.first() else { return Err(bug("unwrap without a flag")) };
                if !*has {
                    return Err(fail(
                        "E0350",
                        "unwrap() of none".to_string(),
                        "check `x != none` first, or give a default with `x ?? value`",
                        span,
                    ));
                }
                None
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
            RtOp::MapNew => {
                self.tick(args.len() as u64 / 2)?;
                let mut mv = MapVal::default();
                for pair in args.chunks(2) {
                    if let [k, v] = pair {
                        mv.set(k.clone(), v.clone())?;
                    }
                }
                Some(Value::Map(Rc::new(mv)))
            }
            RtOp::MapGet | RtOp::MapGetOr => {
                let Some(Value::Map(mv)) = args.first() else { return Err(bug("an operand that should be a map")) };
                let key = args.get(1).ok_or_else(|| bug("a map access without its key"))?;
                match (mv.get(key)?, args.get(2)) {
                    (Some(v), _) => Some(v.clone()),
                    (None, Some(default)) if op == RtOp::MapGetOr => Some(default.clone()),
                    _ => {
                        return Err(fail(
                            "E0248",
                            format!("key {} is not in the map", show(m, key)),
                            "check with `m.has(k)` first, or read it with `m.get(k, default)`",
                            span,
                        ))
                    }
                }
            }
            RtOp::MapKeys | RtOp::MapValues => {
                let Some(Value::Map(mv)) = args.first() else { return Err(bug("an operand that should be a map")) };
                self.tick(mv.len() as u64)?;
                let items = mv.iter().map(|(k, v)| if op == RtOp::MapKeys { k.clone() } else { v.clone() }).collect();
                Some(Value::arr(items))
            }
            RtOp::JsonStr => {
                let mut out = String::new();
                super::jsonrt::encode(m, args.first().ok_or_else(|| bug("json.str without a value"))?, &mut out);
                self.tick(out.len() as u64)?;
                text(out)
            }
            RtOp::JsonParse => {
                let t = ty.ok_or_else(|| bug("json.parse without a destination"))?;
                let src = s(0)?;
                self.tick(src.len() as u64)?;
                Some(super::jsonrt::decode(m, t, src, span)?)
            }
            RtOp::Std(f) => {
                let r = self.host.call(f, args, &mut self.out, span)?;
                if self.host.cost > 0 {
                    self.tick(self.host.cost)?;
                }
                r
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
fn resolve(strs: &[Rc<String>], locals: &[Value], p: &Place) -> Result<Vec<At>, Stop> {
    p.path
        .iter()
        .map(|s| match s {
            Step::Index(e, span) => match eval(strs, locals, e)? {
                Value::Int(i) => Ok(At::Index(i, *span)),
                _ => Err(bug("an index that is not an int")),
            },
            Step::Field(k) => Ok(At::Field(*k as usize)),
            Step::Key(e, span) => Ok(At::Key(eval(strs, locals, e)?, *span)),
        })
        .collect()
}

/// A pure expression. It cannot fail: only a malformed IR can make this an error.
fn eval(strs: &[Rc<String>], locals: &[Value], e: &Expr) -> Result<Value, Stop> {
    Ok(match e {
        Expr::Int(n) => Value::Int(*n),
        Expr::Float(x) => Value::Float(*x),
        Expr::Bool(b) => Value::Bool(*b),
        Expr::Char(c) => Value::Char(char::from_u32(*c).ok_or_else(|| bug("a char literal that is not a character"))?),
        Expr::Str(id) => Value::Str(strs.get(id.0 as usize).cloned().ok_or_else(|| bug("a string literal that does not exist"))?),
        Expr::Local(l) => match locals.get(l.0 as usize) {
            Some(Value::Unset) | None => return Err(bug("a local read before it has a value")),
            Some(v) => v.clone(),
        },
        Expr::Unary(op, x) => match (op, eval(strs, locals, x)?) {
            (UnOp::INeg, Value::Int(n)) => Value::Int(n.wrapping_neg()),
            (UnOp::FNeg, Value::Float(x)) => Value::Float(-x),
            (UnOp::Not, Value::Bool(b)) => Value::Bool(!b),
            _ => return Err(bug("a unary operator on the wrong type")),
        },
        Expr::Binary(op, a, b) => binary(*op, eval(strs, locals, a)?, eval(strs, locals, b)?)?,
        Expr::Select(c, a, b) => {
            if truth(&eval(strs, locals, c)?)? {
                eval(strs, locals, a)?
            } else {
                eval(strs, locals, b)?
            }
        }
        Expr::IntToFloat(x) => match eval(strs, locals, x)? {
            Value::Int(n) => Value::Float(n as f64),
            _ => return Err(bug("float() of a value that is not an int")),
        },
        Expr::Field(x, k, _) => match eval(strs, locals, x)? {
            Value::Struct(_, fields) => fields.get(*k as usize).cloned().ok_or_else(|| bug("a field that does not exist"))?,
            _ => return Err(bug("a field of a value that is not a struct")),
        },
        Expr::Pure(p, args) => {
            let vals = args.iter().map(|a| eval(strs, locals, a)).collect::<Result<Vec<_>, _>>()?;
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
        PureFn::MapLen | PureFn::MapHas => {
            let Some(Value::Map(mv)) = a.first() else { return Err(bug("an operand that should be a map")) };
            if p == PureFn::MapLen {
                Value::Int(mv.len() as i64)
            } else {
                Value::Bool(mv.get(a.get(1).ok_or_else(|| bug("a missing operand"))?)?.is_some())
            }
        }
    })
}

fn binary(op: BinOp, a: Value, b: Value) -> Result<Value, Stop> {
    use BinOp::*;
    Ok(match (op, &a, &b) {
        (IAdd, Value::Int(x), Value::Int(y)) => Value::Int(x.wrapping_add(*y)),
        (ISub, Value::Int(x), Value::Int(y)) => Value::Int(x.wrapping_sub(*y)),
        (IMul, Value::Int(x), Value::Int(y)) => Value::Int(x.wrapping_mul(*y)),
        (ILt, Value::Int(x), Value::Int(y)) => Value::Bool(x < y),
        (ILe, Value::Int(x), Value::Int(y)) => Value::Bool(x <= y),
        (IGt, Value::Int(x), Value::Int(y)) => Value::Bool(x > y),
        (IGe, Value::Int(x), Value::Int(y)) => Value::Bool(x >= y),
        (IEq, Value::Int(x), Value::Int(y)) => Value::Bool(x == y),
        (INe, Value::Int(x), Value::Int(y)) => Value::Bool(x != y),
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
    // (a work list instead of recursion: values can nest as deep as a loop builds them)
    let mut pairs = vec![(a, b)];
    while let Some((a, b)) = pairs.pop() {
        let same = match (a, b) {
            (Value::Int(x), Value::Int(y)) => x == y,
            (Value::Float(x), Value::Float(y)) => x == y,
            (Value::Bool(x), Value::Bool(y)) => x == y,
            (Value::Char(x), Value::Char(y)) => x == y,
            (Value::Str(x), Value::Str(y)) => x == y,
            (Value::Arr(x), Value::Arr(y)) => {
                pairs.extend(x.iter().zip(y.iter()));
                x.len() == y.len()
            }
            (Value::Struct(s, x), Value::Struct(t, y)) => {
                pairs.extend(x.iter().zip(y.iter()));
                s == t
            }
            // by content, in any order
            (Value::Map(x), Value::Map(y)) => {
                let mut same = x.len() == y.len();
                for (k, v) in x.iter() {
                    match y.get(k) {
                        Ok(Some(w)) => pairs.push((v, w)),
                        _ => same = false,
                    }
                }
                same
            }
            _ => false,
        };
        if !same {
            return false;
        }
    }
    true
}

/// The order of two ints, floats, chars or strings (by code points); `None` for a NaN.
pub fn compare(a: &Value, b: &Value) -> Option<Ordering> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Some(x.cmp(y)),
        (Value::Float(x), Value::Float(y)) => x.partial_cmp(y),
        (Value::Char(x), Value::Char(y)) => Some(x.cmp(y)),
        (Value::Str(x), Value::Str(y)) => Some(x.as_str().cmp(y.as_str())),
        (Value::Bool(x), Value::Bool(y)) => Some(x.cmp(y)),
        // tuples: element by element, the first difference decides
        (Value::Struct(_, xs), Value::Struct(_, ys)) => {
            for (x, y) in xs.iter().zip(ys.iter()) {
                if !equal(x, y) {
                    return compare(x, y);
                }
            }
            Some(Ordering::Equal)
        }
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
    show_in(m, v, &mut out, 0);
    out
}

/// Values nested deeper than this (a tree built by a loop) are shown as `...` below it.
const SHOW_DEPTH: usize = 20_000;

fn show_in(m: &Module, v: &Value, out: &mut String, depth: usize) {
    if depth > SHOW_DEPTH {
        out.push_str("...");
        return;
    }
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
                show_in(m, x, out, depth + 1);
            }
            out.push(']');
        }
        Value::Map(mv) => {
            if mv.len() == 0 {
                out.push_str("[:]");
                return;
            }
            out.push('[');
            for (i, (k, x)) in mv.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                show_in(m, k, out, depth + 1);
                out.push_str(": ");
                show_in(m, x, out, depth + 1);
            }
            out.push(']');
        }
        Value::Struct(id, fields) => {
            let Some(info) = m.structs.get(Ty::Struct(*id)) else {
                out.push('?');
                return;
            };
            // an enum value: its variant
            if !info.variants.is_empty() {
                if let Value::Int(k) = &fields[0] {
                    if let Some(v) = info.variants.get(*k as usize) {
                        out.push_str(&format!("{}.{v}", info.name));
                        // `Shape.Circle(2)`: the values of the variant
                        if info.payloads.get(*k as usize).is_some_and(|n| *n > 0) {
                            out.push('(');
                            for (j, slot) in info.slots(*k as usize).enumerate() {
                                if j > 0 {
                                    out.push_str(", ");
                                }
                                show_in(m, &fields[slot], out, depth + 1);
                            }
                            out.push(')');
                        }
                        return;
                    }
                }
            }
            // an optional: `none`, or `Some(value)`
            if info.option {
                match (&fields[0], &fields[1]) {
                    (Value::Bool(true), v) => {
                        out.push_str("Some(");
                        show_in(m, v, out, depth + 1);
                        out.push(')');
                    }
                    _ => out.push_str("none"),
                }
                return;
            }
            if !info.tuple {
                out.push_str(&info.name);
            }
            out.push('(');
            for (k, ((name, _), x)) in info.fields.iter().zip(fields.iter()).enumerate() {
                if k > 0 {
                    out.push_str(", ");
                }
                if !info.tuple {
                    out.push_str(name);
                    out.push_str(": ");
                }
                show_in(m, x, out, depth + 1);
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
pub(super) fn shown(s: &str) -> String {
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

    /// How a program ended in `run_with`.
    struct Ran {
        out: String,
        err: Option<RuntimeError>,
        /// `os.exit(code)`
        exit: Option<i32>,
    }

    /// Runs `main` of a program in the interpreter, with standard input `stdin` and the arguments `args`.
    fn run_with(src: &str, stdin: &[u8], args: &[String], limits: Limits) -> Ran {
        let prog = crate::front(src).unwrap_or_else(|d| panic!("{d:?}"));
        let m = crate::ir::lower::lower(&prog).expect("lowers");
        let types = crate::ast::type_tables();
        std::thread::scope(|s| {
            std::thread::Builder::new()
                .stack_size(512 << 20)
                .spawn_scoped(s, || {
                    crate::ast::install_type_tables(types);
                    let mut host = Host::new().with_input(stdin.to_vec());
                    host.args = args.to_vec();
                    let mut it = Interp::with_host(&m, limits, host, Out::keep());
                    let (mut err, mut exit) = (None, None);
                    match it.call(m.main, Vec::new()) {
                        Ok(_) => {}
                        Err(Stop::Error(e)) => err = Some(*e),
                        Err(Stop::Exit(code)) => exit = Some(code),
                        Err(other) => match it.limit_error(&other) {
                            Some(e) => err = Some(e),
                            None => panic!("stopped: {other:?}"),
                        },
                    };
                    Ran { out: it.output().to_string(), err, exit }
                })
                .unwrap()
                .join()
                .unwrap()
        })
    }

    fn run_main(src: &str) -> Ran {
        run_with(src, b"", &[], Limits::new(2_000_000_000, 100_000))
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

    /// A program that touches the files of the folder the tests run in: tests/sandbox.rs runs it
    /// in a folder of its own.
    fn uses_files(src: &str) -> bool {
        src.lines().any(|l| l.trim() == "use fs")
    }

    /// The interpreter must compute what the backends compute: every example program prints its
    /// `.out` file, the same check `tests/examples.rs` makes for C, JavaScript, Python, ...
    #[test]
    fn every_example_prints_its_expected_output() {
        let mut checked = 0;
        for path in files("examples") {
            let Ok(expected) = std::fs::read_to_string(path.with_extension("out")) else { continue };
            let src = std::fs::read_to_string(&path).unwrap();
            if uses_files(&src) {
                continue;
            }
            let stdin = std::fs::read(path.with_extension("in")).unwrap_or_default();
            let args: Vec<String> = std::fs::read_to_string(path.with_extension("args"))
                .map(|a| a.lines().map(String::from).collect())
                .unwrap_or_default();
            let want_exit = std::fs::read_to_string(path.with_extension("exit")).ok().map(|e| e.trim().parse::<i32>().unwrap());
            let ran = run_with(&src, &stdin, &args, Limits::new(2_000_000_000, 100_000));
            assert!(ran.err.is_none(), "{}: {:?}", path.display(), ran.err);
            assert_eq!(ran.out, expected.replace("\r\n", "\n"), "{}", path.display());
            assert_eq!(ran.exit, want_exit, "{}", path.display());
            checked += 1;
        }
        assert!(checked >= 40, "only {checked} examples were run");
    }

    /// ... and every runtime error test stops with its code, at its position, after its output.
    #[test]
    fn every_runtime_error_has_its_code_and_position() {
        let mut checked = 0;
        for path in files("tests/runtime") {
            let src = std::fs::read_to_string(&path).unwrap();
            let expect = src.lines().next().and_then(|l| l.strip_prefix("// expect: ")).unwrap();
            let (code, at) = expect.trim().split_once(" at ").unwrap();
            // a test limited to some targets (`// only: js ts`) runs here only if Rust is one; one
            // for the sandbox (`// only: interp`) is run by tests/sandbox.rs, with its flags
            let only = src.lines().nth(1).and_then(|l| l.strip_prefix("// only:"));
            if only.is_some_and(|ts| !ts.split_whitespace().any(|t| t == "rs")) || uses_files(&src) {
                continue;
            }
            let stdin = std::fs::read(path.with_extension("in")).unwrap_or_default();
            let ran = run_with(&src, &stdin, &[], Limits::new(2_000_000_000, 100_000));
            let err = ran.err.unwrap_or_else(|| panic!("{}: no runtime error", path.display()));
            assert_eq!(err.code, code, "{}: {}", path.display(), err.msg);
            assert_eq!(format!("{}:{}", err.span.line, err.span.col), at, "{}", path.display());
            let stdout = std::fs::read_to_string(path.with_extension("out")).unwrap_or_default();
            assert_eq!(ran.out, stdout.replace("\r\n", "\n"), "{}", path.display());
            checked += 1;
        }
        assert!(checked >= 30, "only {checked} runtime tests were run");
    }

    #[test]
    fn floats_print_like_javascript() {
        for (x, s) in
            [(0.1 + 0.2, "0.30000000000000004"), (1e21, "1e+21"), (1e-7, "1e-7"), (123.0, "123"), (-0.0, "0"), (2.5e-6, "0.0000025")]
        {
            assert_eq!(num(x), s);
        }
    }

    #[test]
    fn limits_stop_endless_loops_and_recursion() {
        let src = "fn spin() -> int {\n    while true { }\n    ret 0\n}\nfn down(n: int) -> int = down(n + 1)\nfn main() {\n    print(1)\n}\n";
        let prog = crate::front(src).unwrap();
        let m = crate::ir::lower::lower(&prog).unwrap();
        let id = |name: &str| FuncId(m.funcs.iter().position(|f| f.name == name).unwrap() as u32);
        let limits = Limits::new(10_000, 100);
        assert!(matches!(Interp::new(&m, limits).call(id("spin"), vec![]), Err(Stop::Steps)));
        assert!(matches!(Interp::new(&m, limits).call(id("down"), vec![Value::Int(0)]), Err(Stop::Depth)));
    }

    /// The limits of a run and the errors they stand for.
    #[test]
    fn every_limit_has_its_error_code() {
        let limit = |src: &str, limits: Limits| run_with(src, b"", &[], limits).err.map(|e| e.code);
        let spin = "fn main() {\n    var x = 0\n    while true { x += 1 }\n}\n";
        assert_eq!(limit(spin, Limits::new(10_000, 100)), Some("E0355"));
        let grow = "fn main() {\n    var xs: [int] = []\n    while true { xs.push(1) }\n}\n";
        assert_eq!(limit(grow, Limits::new(u64::MAX, 100).memory(8 << 20)), Some("E0356"));
        let big = "fn main() {\n    let s = \"ab\".repeat(100000000)\n    print(s.len())\n}\n";
        assert_eq!(limit(big, Limits::new(u64::MAX, 100).memory(8 << 20)), Some("E0356"));
        let chatty = "fn main() {\n    while true { print(\"hello\") }\n}\n";
        let mut limits = Limits::new(u64::MAX, 100);
        limits.output = 100;
        let ran = run_with(chatty, b"", &[], limits);
        assert_eq!(ran.err.map(|e| e.code), Some("E0357"));
        assert_eq!(ran.out.len(), 100, "the output is cut at the cap");
        let deep = "fn down(n: int) -> int = down(n + 1) + 1\nfn main() {\n    print(down(0))\n}\n";
        assert_eq!(limit(deep, Limits::new(u64::MAX, 50)), Some("E0358"));
        let mut limits = Limits::new(u64::MAX, 100);
        limits.wall_ms = 50;
        assert_eq!(limit(spin, limits), Some("E0359"));
    }

    #[test]
    fn maps_run_in_insertion_order() {
        let src = "fn main() {\n    var m = [\"b\": 1, \"a\": 2]\n    m[\"c\"] = 3\n    m.remove(\"b\")\n    m[\"b\"] = 9\n    print(m, m.len(), m.has(\"c\"), m.get(\"z\", -1), m == [\"c\": 3, \"a\": 2, \"b\": 9])\n    print(m.keys(), m.values())\n    for k in m { print(k, m[k]) }\n    let e: [int: str] = [:]\n    print(e, e.len())\n    print(m[\"nope\"])\n}\n";
        let ran = run_main(src);
        assert_eq!(ran.out, "[\"a\": 2, \"c\": 3, \"b\": 9] 3 true -1 true\n[\"a\", \"c\", \"b\"] [2, 3, 9]\na 2\nc 3\nb 9\n[:] 0\n");
        let e = ran.err.expect("a missing key stops the program");
        assert_eq!((e.code, e.msg.as_str()), ("E0248", "key \"nope\" is not in the map"));
    }

    #[test]
    fn json_round_trips_structs_and_arrays() {
        let src = "use json\nstruct P {\n    name: str\n    tags: [str]\n    x: float\n}\nfn main() {\n    let p = P(name: \"ab\", tags: [\"x\", \"y\"], x: 1.5)\n    let t = json.str(p)\n    print(t)\n    let q: P = json.parse(t)\n    print(q == p, q)\n    let bad: P = json.parse(\"{{\\\"name\\\": 1}}\")\n    print(bad)\n}\n";
        let ran = run_main(src);
        assert!(ran.out.starts_with("{\"name\":\"ab\",\"tags\":[\"x\",\"y\"],\"x\":1.5}\ntrue P(name: \"ab\""), "{}", ran.out);
        let e = ran.err.expect("a wrong shape stops the program");
        assert_eq!((e.code, e.msg.as_str()), ("E0345", "json.parse: expected a string at $.name"));
    }

    #[test]
    fn standard_input_arguments_and_exit() {
        let src = "use input\nuse os\nfn main() {\n    let first = input.line()\n    let rest = input.lines()\n    print(first, rest, input.eof(), os.args())\n    os.exit(7)\n}\n";
        let ran = run_with(src, b"one\ntwo\nthree\n", &["a".to_string(), "b c".to_string()], Limits::new(1_000_000, 100));
        assert_eq!(ran.out, "one [\"two\", \"three\"] true [\"a\", \"b c\"]\n");
        assert_eq!(ran.exit, Some(7));
    }

    #[test]
    fn sleeping_moves_a_virtual_clock_and_costs_steps() {
        let src =
            "use time\nfn main() {\n    let t = time.mono_ms()\n    time.sleep_ms(5000)\n    print(time.mono_ms() - t >= 5000.0)\n}\n";
        let started = std::time::Instant::now();
        let ran = run_main(src);
        assert_eq!(ran.out, "true\n");
        assert!(started.elapsed().as_secs() < 4, "the sleep must not wait");
        let spent = run_with(src, b"", &[], Limits::new(1000, 100));
        assert_eq!(spent.err.map(|e| e.code), Some("E0355"), "5000 ms of sleep cost more than 1000 steps");
    }
}
