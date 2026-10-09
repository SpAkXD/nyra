//! The host of the IR interpreter: everything a program can touch outside of itself. Standard
//! input, the arguments, the environment, files, the clocks, random numbers, and the output.
//!
//! It follows the runtimes of the compiled targets function by function (`src/rt/rs/std.rs`
//! is the reference), so a program prints the same here and there, with these differences:
//!
//! - `time.sleep_ms` does not wait. It moves a virtual clock that `now_ms` and `mono_ms` add to
//!   the real one, and costs steps, so a sandboxed program can neither stall nor slow down.
//! - A host can be *confined* (`nyra run --sandbox`): file paths must stay below the folder the
//!   program runs in (no absolute paths, no `..`).
//! - A host can be *pure* (the `ex` examples at compile time): the functions that touch the
//!   world or are not repeatable (files, input, arguments, clocks, random numbers) stop the
//!   interpreter with a `Bug`, which makes an example skip instead of run them.

use std::io::{BufRead, Read, Write};
use std::rc::Rc;
use std::time::Instant;

use super::interp::{bug, fail, num, shown, Stop, Value};
use super::StdFn;
use crate::ast::Span;

/// Where the program writes: nowhere, a string it keeps, or the process's standard output. A
/// cap on the bytes stops the program (`Stop::Output`) at the first byte beyond it.
pub struct Out {
    pub kept: Option<String>,
    sink: Option<std::io::BufWriter<std::io::Stdout>>,
    /// Bytes written so far.
    pub written: u64,
    pub cap: u64,
}

impl Out {
    pub fn discard() -> Out {
        Out { kept: None, sink: None, written: 0, cap: u64::MAX }
    }

    pub fn keep() -> Out {
        Out { kept: Some(String::new()), ..Out::discard() }
    }

    pub fn stdout() -> Out {
        Out { sink: Some(std::io::BufWriter::with_capacity(1 << 16, std::io::stdout())), ..Out::discard() }
    }

    pub fn write(&mut self, text: &str) -> Result<(), Stop> {
        let room = self.cap.saturating_sub(self.written);
        let (part, over) = if text.len() as u64 > room {
            let mut cut = room as usize;
            while !text.is_char_boundary(cut) {
                cut -= 1;
            }
            (&text[..cut], true)
        } else {
            (text, false)
        };
        self.written += part.len() as u64;
        if let Some(k) = &mut self.kept {
            k.push_str(part);
        }
        if let Some(s) = &mut self.sink {
            // (a closed pipe is not an error of the program)
            let _ = s.write_all(part.as_bytes());
        }
        if over {
            return Err(Stop::Output);
        }
        Ok(())
    }

    pub fn flush(&mut self) {
        if let Some(s) = &mut self.sink {
            let _ = s.flush();
        }
    }
}

enum Input {
    /// The process's standard input.
    Process,
    /// Bytes given by the caller (the MCP tool): what is left starts at `pos`.
    Bytes(Vec<u8>, usize),
}

pub struct Host {
    /// See the module comment.
    pub pure: bool,
    pub confined: bool,
    pub args: Vec<String>,
    input: Input,
    /// Set by `random.seed`: the state of the generator.
    seeded: Option<[u32; 4]>,
    os_count: u64,
    /// Milliseconds the program slept: added to both clocks.
    slept_ms: i64,
    start: Instant,
    /// Steps the last call cost on top of the call itself (bytes read, time slept).
    pub cost: u64,
}

impl Host {
    /// A host for examples: pure, no input.
    pub fn pure() -> Host {
        Host { pure: true, ..Host::new() }
    }

    /// A host for a program: standard input is the process's.
    pub fn new() -> Host {
        Host {
            pure: false,
            confined: false,
            args: Vec::new(),
            input: Input::Process,
            seeded: None,
            os_count: 0,
            slept_ms: 0,
            start: Instant::now(),
            cost: 0,
        }
    }

    /// The program's standard input is `bytes` instead of the process's.
    pub fn with_input(mut self, bytes: Vec<u8>) -> Host {
        self.input = Input::Bytes(bytes, 0);
        self
    }

    pub fn call(&mut self, f: StdFn, a: &[Value], out: &mut Out, span: Span) -> Result<Option<Value>, Stop> {
        self.cost = 0;
        if self.pure
            && !matches!(
                f,
                StdFn::MathSqrt
                    | StdFn::MathFloor
                    | StdFn::MathCeil
                    | StdFn::MathRound
                    | StdFn::MathTrunc
                    | StdFn::TextFixed
                    | StdFn::TextIsInt
                    | StdFn::TextIsFloat
            )
        {
            return Err(bug(&format!("{} in an example", f.full_name())));
        }
        let s = |k: usize| -> Result<&str, Stop> {
            match a.get(k) {
                Some(Value::Str(s)) => Ok(s.as_str()),
                _ => Err(bug("a standard function without its text operand")),
            }
        };
        let i = |k: usize| -> Result<i64, Stop> {
            match a.get(k) {
                Some(Value::Int(n)) => Ok(*n),
                _ => Err(bug("a standard function without its int operand")),
            }
        };
        let x = |k: usize| -> Result<f64, Stop> {
            match a.get(k) {
                Some(Value::Float(n)) => Ok(*n),
                _ => Err(bug("a standard function without its float operand")),
            }
        };
        let text = |t: String| Some(Value::Str(Rc::new(t)));
        let strs = |v: Vec<String>| Some(Value::arr(v.into_iter().map(|t| Value::Str(Rc::new(t))).collect()));
        Ok(match f {
            StdFn::InputLine => {
                out.flush();
                let b = self.read_line();
                self.cost = b.len() as u64;
                text(in_text(b, "input.line", span)?)
            }
            StdFn::InputAll => {
                out.flush();
                let b = self.read_rest();
                self.cost = b.len() as u64;
                text(in_text(b, "input.all", span)?)
            }
            StdFn::InputLines => {
                out.flush();
                let b = self.read_rest();
                self.cost = b.len() as u64;
                strs(lines(&in_text(b, "input.lines", span)?))
            }
            StdFn::InputEof => {
                out.flush();
                Some(Value::Bool(self.at_eof()))
            }
            StdFn::OsArgs => strs(self.args.clone()),
            StdFn::OsEnv => text(getenv(s(0)?, span)?.unwrap_or_default()),
            StdFn::OsHasEnv => Some(Value::Bool(getenv(s(0)?, span)?.is_some())),
            StdFn::OsExit => return Err(Stop::Exit((i(0)? & 255) as i32)),
            StdFn::FsRead => {
                let what = "fs.read: cannot read";
                let path = self.path(s(0)?, what, span)?;
                match kind(path) {
                    0 => return Err(fs_fail(what, path, "not found", span)),
                    2 => return Err(fs_fail(what, path, "is a directory", span)),
                    _ => {}
                }
                let bytes = std::fs::read(path).map_err(|e| fs_fail(what, path, io_reason(&e), span))?;
                self.cost = bytes.len() as u64;
                match String::from_utf8(bytes) {
                    Ok(t) => text(t),
                    Err(_) => return Err(fs_fail(what, path, "not valid UTF-8", span)),
                }
            }
            StdFn::FsWrite => {
                let (what, t) = ("fs.write: cannot write", s(1)?);
                self.put(s(0)?, t, false, what, span)?;
                None
            }
            StdFn::FsAppend => {
                let (what, t) = ("fs.append: cannot append to", s(1)?);
                self.put(s(0)?, t, true, what, span)?;
                None
            }
            StdFn::FsExists => {
                let path = self.path(s(0)?, "fs.exists: cannot test", span)?;
                Some(Value::Bool(kind(path) != 0))
            }
            StdFn::FsList => {
                let path = self.path(s(0)?, "fs.list: cannot list", span)?;
                let names = list_dir(path, span)?;
                self.cost = names.iter().map(|n| n.len() as u64 + 1).sum();
                strs(names)
            }
            StdFn::FsRemove => {
                let what = "fs.remove: cannot remove";
                let path = self.path(s(0)?, what, span)?;
                let k = kind(path);
                if k == 0 {
                    return Err(fs_fail(what, path, "not found", span));
                }
                if k == 2 && !list_dir(path, span)?.is_empty() {
                    return Err(fs_fail(what, path, "not empty", span));
                }
                let r = if k == 2 { std::fs::remove_dir(path) } else { std::fs::remove_file(path) };
                r.map_err(|e| fs_fail(what, path, io_reason(&e), span))?;
                None
            }
            StdFn::FsMkdir => {
                let what = "fs.mkdir: cannot create";
                let path = self.path(s(0)?, what, span)?;
                if path.is_empty() || path.contains('\0') {
                    return Err(fs_fail(what, path, "not found", span));
                }
                if kind(path) != 0 {
                    return Err(fs_fail(what, path, "already exists", span));
                }
                std::fs::create_dir(path).map_err(|e| fs_fail(what, path, io_reason(&e), span))?;
                None
            }
            StdFn::TimeNowMs => {
                let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64);
                Some(Value::Int(now.saturating_add(self.slept_ms)))
            }
            StdFn::TimeMonoMs => Some(Value::Float(self.start.elapsed().as_nanos() as f64 / 1e6 + self.slept_ms as f64)),
            StdFn::TimeSleepMs => {
                let ms = i(0)?;
                if ms > 0 {
                    self.slept_ms = self.slept_ms.saturating_add(ms);
                    // sleeping is never free: a loop of sleeps ends when the steps do
                    self.cost = ms as u64;
                }
                None
            }
            StdFn::RandomSeed => {
                self.seed(i(0)?);
                None
            }
            StdFn::RandomRandom => Some(Value::Float(self.bits53() as f64 / 9007199254740992.0)),
            StdFn::RandomRange => {
                let (lo, hi) = (i(0)?, i(1)?);
                let n = (hi as u64).wrapping_sub(lo as u64);
                if hi <= lo || n > 9007199254740992 {
                    return Err(fail(
                        "E0342",
                        format!("random.range({lo}, {hi}): need lo < hi and hi - lo <= 2^53"),
                        "the upper bound is excluded: `random.range(1, 7)` rolls a die",
                        span,
                    ));
                }
                let limit = 9007199254740992 - (9007199254740992u64 % n) as i64;
                let mut r = self.bits53();
                while r >= limit {
                    r = self.bits53();
                }
                Some(Value::Int((lo as u64).wrapping_add(r as u64 % n) as i64))
            }
            StdFn::MathSqrt => Some(Value::Float(x(0)?.sqrt())),
            StdFn::MathFloor => Some(Value::Float(x(0)?.floor())),
            StdFn::MathCeil => Some(Value::Float(x(0)?.ceil())),
            StdFn::MathTrunc => Some(Value::Float(x(0)?.trunc())),
            StdFn::MathRound => {
                // half away from zero (x - trunc(x) is exact)
                let v = x(0)?;
                let mut t = v.trunc();
                if (v - t).abs() >= 0.5 {
                    t += if v < 0.0 { -1.0 } else { 1.0 };
                }
                Some(Value::Float(t))
            }
            StdFn::TextIsInt => {
                let t = s(0)?;
                let digits = t.strip_prefix('-').unwrap_or(t);
                Some(Value::Bool(!digits.is_empty() && digits.bytes().all(|c| c.is_ascii_digit()) && t.parse::<i64>().is_ok()))
            }
            StdFn::TextIsFloat => Some(Value::Bool(is_float(s(0)?))),
            StdFn::TextFixed => {
                let (v, d) = (x(0)?, i(1)?);
                if !(0..=100).contains(&d) {
                    return Err(fail(
                        "E0342",
                        format!("text.fixed: digits must be 0 to 100, got {d}"),
                        "`text.fixed(x, 2)` shows two decimals",
                        span,
                    ));
                }
                text(fixed(v, d))
            }
        })
    }

    // ---- input ----

    fn read_line(&mut self) -> Vec<u8> {
        let mut b = Vec::new();
        match &mut self.input {
            Input::Process => {
                let _ = std::io::stdin().lock().read_until(b'\n', &mut b);
            }
            Input::Bytes(data, pos) => {
                let rest = &data[*pos..];
                let n = rest.iter().position(|&c| c == b'\n').map_or(rest.len(), |i| i + 1);
                b.extend_from_slice(&rest[..n]);
                *pos += n;
            }
        }
        if b.last() == Some(&b'\n') {
            b.pop();
            if b.last() == Some(&b'\r') {
                b.pop();
            }
        }
        b
    }

    fn read_rest(&mut self) -> Vec<u8> {
        match &mut self.input {
            Input::Process => {
                let mut b = Vec::new();
                let _ = std::io::stdin().lock().read_to_end(&mut b);
                b
            }
            Input::Bytes(data, pos) => {
                let b = data[*pos..].to_vec();
                *pos = data.len();
                b
            }
        }
    }

    fn at_eof(&mut self) -> bool {
        match &mut self.input {
            Input::Process => std::io::stdin().lock().fill_buf().map_or(true, |b| b.is_empty()),
            Input::Bytes(data, pos) => *pos >= data.len(),
        }
    }

    // ---- files ----

    /// The path as it may be used: a confined host refuses a path that leaves the folder.
    fn path<'p>(&self, path: &'p str, what: &str, span: Span) -> Result<&'p str, Stop> {
        if self.confined {
            let outside = path.starts_with(['/', '\\']) || path.contains(':') || path.split(['/', '\\']).any(|c| c == "..");
            if outside {
                return Err(fs_fail(what, path, "outside the working folder (the sandbox allows only paths below it)", span));
            }
        }
        Ok(path)
    }

    fn put(&mut self, path: &str, text: &str, append: bool, what: &str, span: Span) -> Result<(), Stop> {
        let path = self.path(path, what, span)?;
        if path.is_empty() || path.contains('\0') {
            return Err(fs_fail(what, path, "not found", span));
        }
        if kind(path) == 2 {
            return Err(fs_fail(what, path, "is a directory", span));
        }
        self.cost = text.len() as u64;
        let r = if append {
            std::fs::OpenOptions::new().append(true).create(true).open(path).and_then(|mut f| f.write_all(text.as_bytes()))
        } else {
            std::fs::write(path, text)
        };
        r.map_err(|e| fs_fail(what, path, io_reason(&e), span))
    }

    // ---- random numbers: the operating system's generator, or after `random.seed(n)` xoshiro128** ----

    fn seed(&mut self, n: i64) {
        let u = n as u64;
        let half = [u as u32, (u >> 32) as u32];
        let mut s = [0u32; 4];
        for (i, w) in s.iter_mut().enumerate() {
            *w = mix32(half[i & 1].wrapping_add((i as u32 + 1).wrapping_mul(0x9E37_79B9)));
        }
        if s == [0; 4] {
            s[0] = 1;
        }
        self.seeded = Some(s);
    }

    fn bits32(&mut self) -> u32 {
        if let Some(s) = &mut self.seeded {
            let r = s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
            let t = s[1] << 9;
            s[2] ^= s[0];
            s[3] ^= s[1];
            s[1] ^= s[2];
            s[0] ^= s[3];
            s[2] ^= t;
            s[3] = s[3].rotate_left(11);
            return r;
        }
        // the standard library's hasher keys come from the operating system's generator
        use std::hash::{BuildHasher as _, Hasher as _};
        let n = self.os_count;
        self.os_count += 1;
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u64(n);
        (h.finish() >> 16) as u32
    }

    /// 53 random bits: the first draw gives the high 27, the second the low 26.
    fn bits53(&mut self) -> i64 {
        let a = (self.bits32() >> 5) as i64;
        let b = (self.bits32() >> 6) as i64;
        a * 67108864 + b
    }
}

fn mix32(mut z: u32) -> u32 {
    z = (z ^ (z >> 16)).wrapping_mul(0x85EB_CA6B);
    z = (z ^ (z >> 13)).wrapping_mul(0xC2B2_AE35);
    z ^ (z >> 16)
}

fn fs_fail(what: &str, path: &str, reason: &str, span: Span) -> Stop {
    fail(
        "E0340",
        format!("{what} \"{}\" ({reason})", shown(path)),
        "check the path: it is relative to the folder the program runs in (`fs.exists(path)` tests first)",
        span,
    )
}

fn io_reason(e: &std::io::Error) -> &'static str {
    use std::io::ErrorKind as K;
    match e.kind() {
        K::NotFound | K::NotADirectory => "not found",
        K::PermissionDenied => "permission denied",
        K::AlreadyExists => "already exists",
        K::DirectoryNotEmpty => "not empty",
        _ => "io error",
    }
}

/// 0: nothing there (also for "" and a path with a NUL), 1: a file, 2: a directory.
fn kind(path: &str) -> u8 {
    if path.is_empty() || path.contains('\0') {
        return 0;
    }
    match std::fs::metadata(path) {
        Ok(m) if m.is_dir() => 2,
        Ok(_) => 1,
        Err(_) => 0,
    }
}

fn list_dir(dir: &str, span: Span) -> Result<Vec<String>, Stop> {
    let what = "fs.list: cannot list";
    match kind(dir) {
        0 => return Err(fs_fail(what, dir, "not found", span)),
        1 => return Err(fs_fail(what, dir, "not a directory", span)),
        _ => {}
    }
    let entries = std::fs::read_dir(dir).map_err(|e| fs_fail(what, dir, io_reason(&e), span))?;
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| fs_fail(what, dir, io_reason(&e), span))?;
        match entry.file_name().into_string() {
            Ok(n) => names.push(n),
            Err(_) => return Err(fs_fail(what, dir, "not valid UTF-8", span)),
        }
    }
    names.sort();
    Ok(names)
}

fn in_text(b: Vec<u8>, what: &str, span: Span) -> Result<String, Stop> {
    String::from_utf8(b)
        .map_err(|_| fail("E0341", format!("{what}: the input is not valid UTF-8"), "standard input must be UTF-8 text", span))
}

/// The lines of a text: each without its `\n` (and a `\r` before it); no last empty line.
fn lines(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = s;
    while !rest.is_empty() {
        let (l, next) = match rest.find('\n') {
            Some(i) => (rest[..i].strip_suffix('\r').unwrap_or(&rest[..i]), &rest[i + 1..]),
            None => (rest, ""),
        };
        out.push(l.to_string());
        rest = next;
    }
    out
}

fn getenv(name: &str, span: Span) -> Result<Option<String>, Stop> {
    if name.is_empty() || name.contains('=') || name.contains('\0') {
        return Ok(None);
    }
    let Some(v) = std::env::var_os(name) else { return Ok(None) };
    v.into_string()
        .map(Some)
        .map_err(|_| fail("E0341", "os.env: the value is not valid UTF-8".into(), "environment variables must be UTF-8 text", span))
}

fn is_float(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0;
    let digits = |i: &mut usize| {
        let start = *i;
        while *i < b.len() && b[*i].is_ascii_digit() {
            *i += 1;
        }
        *i > start
    };
    if i < b.len() && b[i] == b'-' {
        i += 1;
    }
    if !digits(&mut i) {
        return false;
    }
    if i < b.len() && b[i] == b'.' {
        i += 1;
        if !digits(&mut i) {
            return false;
        }
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        if !digits(&mut i) {
            return false;
        }
    }
    i == b.len()
}

/// `text.fixed(x, d)`: the exact value of x rounded to d decimals, ties away from zero. The double
/// is m * 2^e; x * 10^d = m * 5^d * 2^(e+d) is computed exactly with 32-bit limbs.
fn fixed(x: f64, d: i64) -> String {
    if !x.is_finite() {
        return num(x);
    }
    let bits = x.to_bits();
    let neg = bits >> 63 == 1;
    let mut ex = ((bits >> 52) & 0x7FF) as i64;
    let mut m = bits & 0xF_FFFF_FFFF_FFFF;
    if ex == 0 {
        ex = 1;
    } else {
        m |= 1 << 52;
    }
    let mut a: Vec<u32> = vec![m as u32, (m >> 32) as u32];
    for _ in 0..d {
        let mut carry = 0u64;
        for limb in a.iter_mut() {
            let v = *limb as u64 * 5 + carry;
            *limb = v as u32;
            carry = v >> 32;
        }
        if carry > 0 {
            a.push(carry as u32);
        }
    }
    let s = ex - 1075 + d;
    if s > 0 {
        let (words, bitsh) = ((s / 32) as usize, (s % 32) as u32);
        let mut shifted = vec![0u32; words];
        shifted.extend_from_slice(&a);
        if bitsh > 0 {
            let mut carry = 0u32;
            for limb in shifted.iter_mut() {
                let v = *limb;
                *limb = (v << bitsh) | carry;
                carry = v >> (32 - bitsh);
            }
            if carry > 0 {
                shifted.push(carry);
            }
        }
        a = shifted;
    } else if s < 0 {
        let k = (-s) as usize;
        let bit = |a: &[u32], i: usize| i / 32 < a.len() && (a[i / 32] >> (i % 32)) & 1 == 1;
        let up = bit(&a, k - 1);
        let (words, bitsh) = (k / 32, (k % 32) as u32);
        let mut q: Vec<u32> = if words >= a.len() { vec![0] } else { a[words..].to_vec() };
        if bitsh > 0 {
            for i in 0..q.len() {
                let next = if i + 1 < q.len() { q[i + 1] << (32 - bitsh) } else { 0 };
                q[i] = (q[i] >> bitsh) | next;
            }
        }
        if up {
            let mut i = 0;
            loop {
                if i == q.len() {
                    q.push(1);
                    break;
                }
                q[i] = q[i].wrapping_add(1);
                if q[i] != 0 {
                    break;
                }
                i += 1;
            }
        }
        a = q;
    }
    // the decimal digits, least significant first
    let mut digits: Vec<u8> = Vec::new();
    while a.iter().any(|&l| l != 0) {
        let mut rem = 0u64;
        for limb in a.iter_mut().rev() {
            let v = (rem << 32) | *limb as u64;
            *limb = (v / 1_000_000_000) as u32;
            rem = v % 1_000_000_000;
        }
        for _ in 0..9 {
            digits.push(b'0' + (rem % 10) as u8);
            rem /= 10;
        }
    }
    while digits.last() == Some(&b'0') {
        digits.pop();
    }
    let zero = digits.is_empty();
    while digits.len() <= d as usize {
        digits.push(b'0');
    }
    let mut out = String::new();
    if neg && !zero {
        out.push('-');
    }
    for i in (0..digits.len()).rev() {
        if i + 1 == d as usize {
            out.push('.');
        }
        out.push(digits[i] as char);
    }
    out
}
