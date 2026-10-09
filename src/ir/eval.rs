//! Compile-time evaluation for the optimizer: runs pure IR on constant inputs.
//!
//! It knows ints, floats, bools, chars and strings, and stops (`Stop`) at anything else: an
//! array or a struct, printing, an operation that would fail at run time (that error must
//! happen when the program runs, with its position), or a computation longer than its step
//! budget. Every operation it does give a result for computes exactly what the runtimes compute:
//! ints wrap, floats are IEEE doubles (no formatting: a float is never turned into text here),
//! string positions count characters.

use std::collections::HashMap;

use super::{BinOp, Expr, FuncId, LocalId, Module, PureFn, RtOp, Stmt, StmtKind, UnOp};

/// A value known at compile time.
#[derive(Clone, Debug, PartialEq)]
pub enum Val {
    Int(i64),
    Float(f64),
    Bool(bool),
    Char(u32),
    Str(String),
}

/// Why evaluation stopped.
#[derive(Debug, PartialEq)]
pub enum Stop {
    /// Out of steps: the same call may still finish with a bigger budget.
    Budget,
    /// Something compile time cannot or must not do; the code stays as it is.
    No,
}

type R<T> = Result<T, Stop>;

/// The longest string compile time builds (a folded string becomes a literal in the program).
const MAX_STR: usize = 4096;
/// Steps for one folded call, and for all calls of a module together.
const CALL_STEPS: u64 = 200_000;
const MODULE_STEPS: u64 = 2_000_000;
/// The deepest recursion compile time follows.
const MAX_DEPTH: usize = 40;

impl Val {
    /// The value as an IR constant, if every backend can write it as a literal that means
    /// exactly this value: no NaN, no infinity, no negative zero (`strs` interns strings).
    pub fn to_expr(&self, strs: &mut dyn FnMut(String) -> super::StrId) -> Option<Expr> {
        Some(match self {
            Val::Int(n) => Expr::Int(*n),
            Val::Float(f) if f.is_finite() && !(*f == 0.0 && f.is_sign_negative()) => Expr::Float(*f),
            Val::Float(_) => return None,
            Val::Bool(b) => Expr::Bool(*b),
            Val::Char(c) => Expr::Char(*c),
            Val::Str(s) if s.len() <= MAX_STR => Expr::Str(strs(s.clone())),
            Val::Str(_) => return None,
        })
    }

    /// The value of a constant expression (a literal).
    pub fn of(e: &Expr, strs: &[String]) -> Option<Val> {
        Some(match e {
            Expr::Int(n) => Val::Int(*n),
            Expr::Float(f) => Val::Float(*f),
            Expr::Bool(b) => Val::Bool(*b),
            Expr::Char(c) => Val::Char(*c),
            Expr::Str(id) => Val::Str(strs[id.0 as usize].clone()),
            _ => return None,
        })
    }
}

fn int(v: Val) -> R<i64> {
    match v {
        Val::Int(n) => Ok(n),
        _ => Err(Stop::No),
    }
}

fn float(v: Val) -> R<f64> {
    match v {
        Val::Float(f) => Ok(f),
        _ => Err(Stop::No),
    }
}

fn boolean(v: Val) -> R<bool> {
    match v {
        Val::Bool(b) => Ok(b),
        _ => Err(Stop::No),
    }
}

fn chr(v: Val) -> R<u32> {
    match v {
        Val::Char(c) => Ok(c),
        _ => Err(Stop::No),
    }
}

fn string(v: Val) -> R<String> {
    match v {
        Val::Str(s) => Ok(s),
        _ => Err(Stop::No),
    }
}

fn short(s: String) -> R<Val> {
    if s.len() > MAX_STR {
        return Err(Stop::No);
    }
    Ok(Val::Str(s))
}

/// The byte offset of character `i` of `s` (`i` <= its length).
fn offset(s: &str, i: usize) -> usize {
    s.char_indices().nth(i).map_or(s.len(), |(b, _)| b)
}

fn is_space(c: u32) -> bool {
    matches!(c, 0x20 | 0x09 | 0x0A | 0x0D)
}

/// A unary or binary operator on constant operands (`None`: not a constant operation).
pub fn unary(op: UnOp, x: Val) -> R<Val> {
    Ok(match op {
        UnOp::INeg => Val::Int(int(x)?.wrapping_neg()),
        UnOp::FNeg => Val::Float(-float(x)?),
        UnOp::Not => Val::Bool(!boolean(x)?),
    })
}

pub fn binary(op: BinOp, a: Val, b: Val) -> R<Val> {
    use BinOp::*;
    Ok(match op {
        IAdd | ISub | IMul | IDiv | IRem | IEq | INe | ILt | ILe | IGt | IGe => {
            let (x, y) = (int(a)?, int(b)?);
            match op {
                IAdd => Val::Int(x.wrapping_add(y)),
                ISub => Val::Int(x.wrapping_sub(y)),
                IMul => Val::Int(x.wrapping_mul(y)),
                IDiv | IRem if y == 0 => return Err(Stop::No),
                IDiv => Val::Int(x.wrapping_div(y)),
                IRem => Val::Int(x.wrapping_rem(y)),
                IEq => Val::Bool(x == y),
                INe => Val::Bool(x != y),
                ILt => Val::Bool(x < y),
                ILe => Val::Bool(x <= y),
                IGt => Val::Bool(x > y),
                _ => Val::Bool(x >= y),
            }
        }
        FAdd | FSub | FMul | FDiv | FEq | FNe | FLt | FLe | FGt | FGe => {
            let (x, y) = (float(a)?, float(b)?);
            match op {
                FAdd => Val::Float(x + y),
                FSub => Val::Float(x - y),
                FMul => Val::Float(x * y),
                FDiv => Val::Float(x / y),
                FEq => Val::Bool(x == y),
                FNe => Val::Bool(x != y),
                FLt => Val::Bool(x < y),
                FLe => Val::Bool(x <= y),
                FGt => Val::Bool(x > y),
                _ => Val::Bool(x >= y),
            }
        }
        BEq => Val::Bool(boolean(a)? == boolean(b)?),
        BNe => Val::Bool(boolean(a)? != boolean(b)?),
        And => Val::Bool(boolean(a)? && boolean(b)?),
        Or => Val::Bool(boolean(a)? || boolean(b)?),
        CEq | CNe | CLt | CLe | CGt | CGe => {
            let (x, y) = (chr(a)?, chr(b)?);
            Val::Bool(match op {
                CEq => x == y,
                CNe => x != y,
                CLt => x < y,
                CLe => x <= y,
                CGt => x > y,
                _ => x >= y,
            })
        }
        // UTF-8 byte order is code point order, as in the runtimes
        SEq | SNe | SLt | SLe | SGt | SGe => {
            let (x, y) = (string(a)?, string(b)?);
            Val::Bool(match op {
                SEq => x == y,
                SNe => x != y,
                SLt => x < y,
                SLe => x <= y,
                SGt => x > y,
                _ => x >= y,
            })
        }
        DeepEq | DeepNe => return Err(Stop::No),
    })
}

/// A pure runtime function on constant arguments.
pub fn pure(p: PureFn, args: Vec<Val>) -> R<Val> {
    let mut it = args.into_iter();
    let mut next = || it.next().ok_or(Stop::No);
    Ok(match p {
        PureFn::StrLen => Val::Int(string(next()?)?.chars().count() as i64),
        PureFn::StrContains => {
            let (s, t) = (string(next()?)?, string(next()?)?);
            Val::Bool(s.contains(t.as_str()))
        }
        PureFn::StrStartsWith => {
            let (s, t) = (string(next()?)?, string(next()?)?);
            Val::Bool(s.starts_with(t.as_str()))
        }
        PureFn::StrEndsWith => {
            let (s, t) = (string(next()?)?, string(next()?)?);
            Val::Bool(s.ends_with(t.as_str()))
        }
        PureFn::StrIndexOf => {
            let (s, t) = (string(next()?)?, string(next()?)?);
            Val::Int(s.find(t.as_str()).map_or(-1, |b| s[..b].chars().count() as i64))
        }
        PureFn::CharCode => Val::Int(i64::from(chr(next()?)?)),
        PureFn::CharUpper => {
            let c = chr(next()?)?;
            Val::Char(if (0x61..=0x7A).contains(&c) { c - 32 } else { c })
        }
        PureFn::CharLower => {
            let c = chr(next()?)?;
            Val::Char(if (0x41..=0x5A).contains(&c) { c + 32 } else { c })
        }
        PureFn::CharIsDigit => Val::Bool((0x30..=0x39).contains(&chr(next()?)?)),
        PureFn::CharIsUpper => Val::Bool((0x41..=0x5A).contains(&chr(next()?)?)),
        PureFn::CharIsLower => Val::Bool((0x61..=0x7A).contains(&chr(next()?)?)),
        PureFn::CharIsLetter => {
            let c = chr(next()?)?;
            Val::Bool((0x41..=0x5A).contains(&c) || (0x61..=0x7A).contains(&c))
        }
        PureFn::CharIsSpace => Val::Bool(is_space(chr(next()?)?)),
        PureFn::ArrLen | PureFn::ArrContains | PureFn::ArrIndexOf | PureFn::MapLen | PureFn::MapHas => return Err(Stop::No),
    })
}

/// A runtime operation on constant arguments. `Ok(None)`: it writes nothing (`check_step`).
/// An operation that would fail at run time stops evaluation: the program must report it.
pub fn op(op: RtOp, args: Vec<Val>) -> R<Option<Val>> {
    let mut it = args.into_iter();
    let mut next = || it.next().ok_or(Stop::No);
    Ok(Some(match op {
        RtOp::DivInt | RtOp::RemInt => {
            let (a, b) = (int(next()?)?, int(next()?)?);
            match (op, b) {
                (_, 0) => return Err(Stop::No),
                (RtOp::DivInt, -1) => Val::Int(a.wrapping_neg()),
                (_, -1) => Val::Int(0),
                (RtOp::DivInt, _) => Val::Int(a / b),
                _ => Val::Int(a % b),
            }
        }
        RtOp::FloatToInt => {
            let x = float(next()?)?;
            if x.is_nan() || x >= 9223372036854775807.0 || x < -9223372036854775808.0 {
                return Err(Stop::No);
            }
            Val::Int(x as i64)
        }
        RtOp::Format => {
            let mut s = String::new();
            for v in std::iter::from_fn(|| next().ok()) {
                match v {
                    Val::Int(n) => s.push_str(&n.to_string()),
                    Val::Bool(b) => s.push_str(if b { "true" } else { "false" }),
                    Val::Char(c) => s.push(char::from_u32(c).ok_or(Stop::No)?),
                    Val::Str(t) => s.push_str(&t),
                    // only the runtimes turn floats into text
                    Val::Float(_) => return Err(Stop::No),
                }
                if s.len() > MAX_STR {
                    return Err(Stop::No);
                }
            }
            Val::Str(s)
        }
        RtOp::StrConcat => {
            let (a, b) = (string(next()?)?, string(next()?)?);
            short(a + &b)?
        }
        RtOp::StrAt => {
            let (s, i) = (string(next()?)?, int(next()?)?);
            let c = usize::try_from(i).ok().and_then(|i| s.chars().nth(i)).ok_or(Stop::No)?;
            Val::Char(c as u32)
        }
        RtOp::StrSlice => {
            let (s, a, b) = (string(next()?)?, int(next()?)?, int(next()?)?);
            let n = s.chars().count() as i64;
            if a < 0 || a > b || b > n {
                return Err(Stop::No);
            }
            let (from, to) = (offset(&s, a as usize), offset(&s, b as usize));
            Val::Str(s[from..to].to_string())
        }
        RtOp::StrReplace => {
            let (s, old, new) = (string(next()?)?, string(next()?)?, string(next()?)?);
            if old.is_empty() {
                return Err(Stop::No);
            }
            short(s.replace(old.as_str(), &new))?
        }
        RtOp::StrTrim => {
            let s = string(next()?)?;
            Val::Str(s.trim_matches(|c: char| is_space(c as u32)).to_string())
        }
        RtOp::StrUpper => Val::Str(string(next()?)?.to_ascii_uppercase()),
        RtOp::StrLower => Val::Str(string(next()?)?.to_ascii_lowercase()),
        RtOp::StrRepeat => {
            let (s, n) = (string(next()?)?, int(next()?)?);
            if n < 0 || (n as u128) * (s.len() as u128) > MAX_STR as u128 {
                return Err(Stop::No);
            }
            Val::Str(s.repeat(n as usize))
        }
        RtOp::StrToInt => {
            // -?[0-9]+ that fits, nothing else (like the runtimes)
            let s = string(next()?)?;
            let digits = s.strip_prefix('-').unwrap_or(&s);
            if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
                return Err(Stop::No);
            }
            Val::Int(s.parse::<i64>().map_err(|_| Stop::No)?)
        }
        RtOp::CharFrom => {
            let n = int(next()?)?;
            if !(0..=1114111).contains(&n) || (55296..=57343).contains(&n) {
                return Err(Stop::No);
            }
            Val::Char(n as u32)
        }
        RtOp::CheckStep => {
            if int(next()?)? == 0 {
                return Err(Stop::No);
            }
            return Ok(None);
        }
        RtOp::StrPadLeft | RtOp::StrPadRight => {
            let (s, n, c) = (string(next()?)?, int(next()?)?, chr(next()?)?);
            let c = char::from_u32(c).ok_or(Stop::No)?;
            let missing = n - s.chars().count() as i64;
            if missing <= 0 {
                return Ok(Some(Val::Str(s)));
            }
            if missing as u128 * 4 > MAX_STR as u128 {
                return Err(Stop::No);
            }
            let pad: String = std::iter::repeat_n(c, missing as usize).collect();
            short(if op == RtOp::StrPadLeft { pad + &s } else { s + &pad })?
        }
        _ => return Err(Stop::No),
    }))
}

/// How a block ended.
enum Flow {
    Next,
    Break,
    Continue,
    Return(Option<Val>),
}

/// Runs calls of user functions with constant arguments.
pub struct Calls<'m> {
    m: &'m Module,
    steps: u64,
    /// Left for the whole module.
    total: u64,
    /// Results already known (pure functions always give the same result); `None`: it cannot
    /// be evaluated (not a lack of steps).
    memo: HashMap<(FuncId, Vec<Key>), Option<Option<Val>>>,
}

/// A hashable argument (floats by their bits).
#[derive(Clone, PartialEq, Eq, Hash)]
enum Key {
    Int(i64),
    Float(u64),
    Bool(bool),
    Char(u32),
    Str(String),
}

fn key(v: &Val) -> Key {
    match v {
        Val::Int(n) => Key::Int(*n),
        Val::Float(f) => Key::Float(f.to_bits()),
        Val::Bool(b) => Key::Bool(*b),
        Val::Char(c) => Key::Char(*c),
        Val::Str(s) => Key::Str(s.clone()),
    }
}

impl<'m> Calls<'m> {
    pub fn new(m: &'m Module) -> Self {
        Calls { m, steps: 0, total: MODULE_STEPS, memo: HashMap::new() }
    }

    /// The result of `func(args)` (`Some(None)`: a call that returns nothing finished), or
    /// `None` when compile time cannot know it.
    pub fn call(&mut self, func: FuncId, args: Vec<Val>) -> Option<Option<Val>> {
        if self.total == 0 {
            return None;
        }
        self.steps = CALL_STEPS.min(self.total);
        let budget = self.steps;
        let r = self.run(func, args, 0);
        self.total -= budget - self.steps;
        r.ok()
    }

    fn run(&mut self, func: FuncId, args: Vec<Val>, depth: usize) -> R<Option<Val>> {
        if depth > MAX_DEPTH {
            return Err(Stop::No);
        }
        let k = (func, args.iter().map(key).collect::<Vec<_>>());
        if let Some(known) = self.memo.get(&k) {
            return known.clone().ok_or(Stop::No);
        }
        let f = self.m.func(func);
        let mut frame: Vec<Option<Val>> = vec![None; f.locals.len()];
        for (slot, a) in frame.iter_mut().zip(args) {
            *slot = Some(a);
        }
        let r = match self.block(&f.body, &mut frame, depth) {
            Ok(Flow::Return(v)) => Ok(v),
            Ok(Flow::Next) if f.ret.is_none() => Ok(None),
            Ok(_) => Err(Stop::No),
            Err(e) => Err(e),
        };
        match &r {
            Ok(v) => {
                self.memo.insert(k, Some(v.clone()));
            }
            Err(Stop::No) => {
                self.memo.insert(k, None);
            }
            Err(Stop::Budget) => {}
        }
        r
    }

    fn tick(&mut self) -> R<()> {
        if self.steps == 0 {
            return Err(Stop::Budget);
        }
        self.steps -= 1;
        Ok(())
    }

    fn block(&mut self, ss: &[Stmt], frame: &mut [Option<Val>], depth: usize) -> R<Flow> {
        for s in ss {
            match self.stmt(s, frame, depth)? {
                Flow::Next => {}
                other => return Ok(other),
            }
        }
        Ok(Flow::Next)
    }

    fn expr(&self, e: &Expr, frame: &[Option<Val>]) -> R<Val> {
        expr(e, &mut |l| frame[l.0 as usize].clone(), &self.m.strs)
    }

    fn stmt(&mut self, s: &Stmt, frame: &mut [Option<Val>], depth: usize) -> R<Flow> {
        self.tick()?;
        let set = |frame: &mut [Option<Val>], l: LocalId, v: Val| frame[l.0 as usize] = Some(v);
        match &s.kind {
            StmtKind::Set(l, e) => {
                let v = self.expr(e, frame)?;
                set(frame, *l, v);
            }
            StmtKind::Call { dst, func, args } => {
                let mut vals = Vec::with_capacity(args.len());
                for a in args {
                    match a {
                        super::Arg::Val(e) => vals.push(self.expr(e, frame)?),
                        super::Arg::InOut(_) => return Err(Stop::No),
                    }
                }
                let r = self.run(*func, vals, depth + 1)?;
                if let (Some(d), Some(v)) = (dst, r) {
                    set(frame, *d, v);
                }
            }
            StmtKind::Op { dst, op: o, args } => {
                let vals = args.iter().map(|a| self.expr(a, frame)).collect::<R<Vec<_>>>()?;
                if let (Some(d), Some(v)) = (dst, op(*o, vals)?) {
                    set(frame, *d, v);
                }
            }
            StmtKind::Mutate { dst: None, op: RtOp::StrAppend, place, args } if place.path.is_empty() => {
                let s = string(frame[place.root.0 as usize].clone().ok_or(Stop::No)?)?;
                let t = string(self.expr(&args[0], frame)?)?;
                set(frame, place.root, short(s + &t)?);
            }
            StmtKind::If { cond, then, els } => {
                let c = boolean(self.expr(cond, frame)?)?;
                return self.block(if c { then } else { els }, frame, depth);
            }
            StmtKind::Loop { head, cond, body, step } => loop {
                self.tick()?;
                match self.block(head, frame, depth)? {
                    Flow::Next => {}
                    Flow::Return(v) => return Ok(Flow::Return(v)),
                    _ => return Err(Stop::No),
                }
                if !boolean(self.expr(cond, frame)?)? {
                    break;
                }
                match self.block(body, frame, depth)? {
                    Flow::Break => break,
                    Flow::Return(v) => return Ok(Flow::Return(v)),
                    Flow::Next | Flow::Continue => {}
                }
                self.block(step, frame, depth)?;
            },
            StmtKind::ForEach { var, iter, body } => {
                let s = string(self.expr(iter, frame)?)?;
                for c in s.chars() {
                    self.tick()?;
                    set(frame, *var, Val::Char(c as u32));
                    match self.block(body, frame, depth)? {
                        Flow::Break => break,
                        Flow::Return(v) => return Ok(Flow::Return(v)),
                        Flow::Next | Flow::Continue => {}
                    }
                }
            }
            StmtKind::Break => return Ok(Flow::Break),
            StmtKind::Continue => return Ok(Flow::Continue),
            StmtKind::Return(v) => {
                let v = match v {
                    Some(e) => Some(self.expr(e, frame)?),
                    None => None,
                };
                return Ok(Flow::Return(v));
            }
            // reference counting changes nothing a program can see
            StmtKind::Dup(_) | StmtKind::Drop(_) | StmtKind::Keep(_) => {}
            StmtKind::Free(l) => frame[l.0 as usize] = None,
            StmtKind::Store { .. } | StmtKind::Mutate { .. } => return Err(Stop::No),
        }
        Ok(Flow::Next)
    }
}

/// The value of a pure expression; `local` gives the values of the locals it reads.
pub fn expr(e: &Expr, local: &mut dyn FnMut(LocalId) -> Option<Val>, strs: &[String]) -> R<Val> {
    Ok(match e {
        Expr::Int(_) | Expr::Float(_) | Expr::Bool(_) | Expr::Char(_) | Expr::Str(_) => Val::of(e, strs).ok_or(Stop::No)?,
        Expr::Local(l) => local(*l).ok_or(Stop::No)?,
        Expr::Unary(op, x) => unary(*op, expr(x, local, strs)?)?,
        Expr::Binary(op, a, b) => {
            let a = expr(a, local, strs)?;
            binary(*op, a, expr(b, local, strs)?)?
        }
        Expr::Select(c, a, b) => {
            if boolean(expr(c, local, strs)?)? {
                expr(a, local, strs)?
            } else {
                expr(b, local, strs)?
            }
        }
        Expr::IntToFloat(x) => Val::Float(int(expr(x, local, strs)?)? as f64),
        Expr::Pure(p, args) => {
            let vals = args.iter().map(|a| expr(a, local, strs)).collect::<R<Vec<_>>>()?;
            pure(*p, vals)?
        }
        Expr::Field(..) => return Err(Stop::No),
    })
}
