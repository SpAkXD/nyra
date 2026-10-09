//! Bounds of int values, so lowering can use the plain `+ - *` operators where they cannot
//! overflow and the checked operations (`RtOp::AddInt`, ...) everywhere else.
//!
//! A bound holds on every backend at once: it is computed twice, once for the 64-bit backends
//! (any `i64`) and once for JavaScript, where every int is a safe integer (|n| <= 2^53 - 1,
//! because an operation that leaves that range stops the program). An operation is plain only
//! when its result stays in range in both. The analysis is deliberately simple: constants, the
//! counter of `for i in a..b`, immutable `let` variables, lengths, character codes and `%` / `/`
//! by a constant.

use super::{BinOp, Expr, LocalId, PureFn, UnOp};

/// The two worlds a bound must hold in: the 64-bit backends and JavaScript.
pub const MODES: usize = 2;

/// The smallest and largest int of each mode.
const LIMITS: [(i128, i128); MODES] = [(i64::MIN as i128, i64::MAX as i128), (-(1 << 53) + 1, (1 << 53) - 1)];

/// No string, array or map can have more elements than this (no address space holds 2^48 bytes).
const LEN_MAX: i128 = 1 << 48;

/// `lo..=hi` (empty when `lo > hi`: the code that would see it never runs).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Iv {
    pub lo: i128,
    pub hi: i128,
}

impl Iv {
    pub fn full(mode: usize) -> Iv {
        let (lo, hi) = LIMITS[mode];
        Iv { lo, hi }
    }

    fn of(lo: i128, hi: i128) -> Iv {
        Iv { lo, hi }
    }

    fn union(self, o: Iv) -> Iv {
        Iv { lo: self.lo.min(o.lo), hi: self.hi.max(o.hi) }
    }

    /// True if every value is an int of `mode`.
    pub fn fits(self, mode: usize) -> bool {
        let (lo, hi) = LIMITS[mode];
        self.lo > self.hi || (self.lo >= lo && self.hi <= hi)
    }

    /// Cut to the ints of `mode` (a value outside cannot exist there).
    fn clamp(self, mode: usize) -> Iv {
        let (lo, hi) = LIMITS[mode];
        Iv { lo: self.lo.max(lo), hi: self.hi.min(hi) }
    }
}

/// The result bound of an int operator on operands with bounds `a` and `b`.
pub fn arith(op: BinOp, a: Iv, b: Iv) -> Iv {
    match op {
        BinOp::IAdd => Iv::of(a.lo + b.lo, a.hi + b.hi),
        BinOp::ISub => Iv::of(a.lo - b.hi, a.hi - b.lo),
        BinOp::IMul => {
            let c = [a.lo * b.lo, a.lo * b.hi, a.hi * b.lo, a.hi * b.hi];
            Iv::of(*c.iter().min().expect("four"), *c.iter().max().expect("four"))
        }
        _ => Iv::of(i128::MIN / 4, i128::MAX / 4),
    }
}

/// A valid index: `0 <= i < len`.
pub fn index(mode: usize) -> Iv {
    Iv::of(0, LEN_MAX - 1).clamp(mode)
}

pub fn neg(a: Iv) -> Iv {
    Iv::of(-a.hi, -a.lo)
}

/// The bound of `e` in `mode`; `local` gives the bounds known for locals.
pub fn of(e: &Expr, mode: usize, local: &dyn Fn(LocalId) -> Option<[Iv; MODES]>) -> Iv {
    let full = Iv::full(mode);
    let iv = match e {
        Expr::Int(n) => Iv::of(*n as i128, *n as i128),
        Expr::Local(l) => local(*l).map_or(full, |b| b[mode]),
        Expr::Unary(UnOp::INeg, x) => neg(of(x, mode, local)),
        Expr::Binary(op @ (BinOp::IAdd | BinOp::ISub | BinOp::IMul), a, b) => arith(*op, of(a, mode, local), of(b, mode, local)),
        Expr::Binary(BinOp::IDiv, a, b) => match **b {
            Expr::Int(k) if k != 0 => {
                let a = of(a, mode, local);
                let (x, y) = (a.lo / k as i128, a.hi / k as i128);
                Iv::of(x.min(y), x.max(y))
            }
            _ => full,
        },
        Expr::Binary(BinOp::IRem, a, b) => match **b {
            Expr::Int(k) if k != 0 => {
                let m = (k as i128).abs() - 1;
                let a = of(a, mode, local);
                if a.lo >= 0 {
                    Iv::of(0, m.min(a.hi))
                } else if a.hi <= 0 {
                    Iv::of((-m).max(a.lo), 0)
                } else {
                    Iv::of(-m, m)
                }
            }
            _ => full,
        },
        Expr::Select(_, a, b) => of(a, mode, local).union(of(b, mode, local)),
        Expr::Pure(PureFn::StrLen | PureFn::ArrLen | PureFn::MapLen, _) => Iv::of(0, LEN_MAX),
        Expr::Pure(PureFn::StrIndexOf | PureFn::ArrIndexOf, _) => Iv::of(-1, LEN_MAX),
        Expr::Pure(PureFn::CharCode, _) => Iv::of(0, 0x10FFFF),
        _ => full,
    };
    iv.clamp(mode)
}

/// The bounds of `e` in every mode.
pub fn all(e: &Expr, local: &dyn Fn(LocalId) -> Option<[Iv; MODES]>) -> [Iv; MODES] {
    std::array::from_fn(|mode| of(e, mode, local))
}
