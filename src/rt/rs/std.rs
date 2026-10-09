
// ---- standard library: `use input`, `use os`, `use fs`, `use time`, `use random`, `use math`, `use text`

fn ny_fs_fail(what: &str, path: &str, reason: &str, line: u32, col: u32) -> ! {
    ny_fail(
        "E0340",
        &format!("{what} \"{}\" ({reason})", ny_shown(path)),
        "check the path: it is relative to the folder the program runs in (`fs.exists(path)` tests first)",
        line,
        col,
    )
}

fn ny_io_reason(e: &std::io::Error) -> &'static str {
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
fn ny_kind(path: &str) -> u8 {
    if path.is_empty() || path.contains('\0') {
        return 0;
    }
    match std::fs::metadata(path) {
        Ok(m) if m.is_dir() => 2,
        Ok(_) => 1,
        Err(_) => 0,
    }
}

// ---- fs ----

fn ny_std_fs_read(path: &str, line: u32, col: u32) -> Str {
    let what = "fs.read: cannot read";
    match ny_kind(path) {
        0 => ny_fs_fail(what, path, "not found", line, col),
        2 => ny_fs_fail(what, path, "is a directory", line, col),
        _ => {}
    }
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => ny_fs_fail(what, path, ny_io_reason(&e), line, col),
    };
    match String::from_utf8(bytes) {
        Ok(s) => Rc::new(s),
        Err(_) => ny_fs_fail(what, path, "not valid UTF-8", line, col),
    }
}

fn ny_fs_put(path: &str, text: &str, append: bool, what: &str, line: u32, col: u32) {
    use std::io::Write as _;
    if path.is_empty() || path.contains('\0') {
        ny_fs_fail(what, path, "not found", line, col);
    }
    if ny_kind(path) == 2 {
        ny_fs_fail(what, path, "is a directory", line, col);
    }
    let r = if append {
        std::fs::OpenOptions::new().append(true).create(true).open(path).and_then(|mut f| f.write_all(text.as_bytes()))
    } else {
        std::fs::write(path, text)
    };
    if let Err(e) = r {
        ny_fs_fail(what, path, ny_io_reason(&e), line, col);
    }
}

fn ny_std_fs_write(path: &str, text: &str, line: u32, col: u32) {
    ny_fs_put(path, text, false, "fs.write: cannot write", line, col);
}

fn ny_std_fs_append(path: &str, text: &str, line: u32, col: u32) {
    ny_fs_put(path, text, true, "fs.append: cannot append to", line, col);
}

fn ny_std_fs_exists(path: &str, line: u32, col: u32) -> bool {
    ny_kind(path) != 0
}

fn ny_std_fs_list(dir: &str, line: u32, col: u32) -> Rc<Vec<Str>> {
    let what = "fs.list: cannot list";
    match ny_kind(dir) {
        0 => ny_fs_fail(what, dir, "not found", line, col),
        1 => ny_fs_fail(what, dir, "not a directory", line, col),
        _ => {}
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => ny_fs_fail(what, dir, ny_io_reason(&e), line, col),
    };
    let mut names = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => ny_fs_fail(what, dir, ny_io_reason(&e), line, col),
        };
        match entry.file_name().into_string() {
            Ok(n) => names.push(n),
            Err(_) => ny_fs_fail(what, dir, "not valid UTF-8", line, col),
        }
    }
    names.sort();
    Rc::new(names.into_iter().map(Rc::new).collect())
}

fn ny_std_fs_remove(path: &str, line: u32, col: u32) {
    let what = "fs.remove: cannot remove";
    let kind = ny_kind(path);
    if kind == 0 {
        ny_fs_fail(what, path, "not found", line, col);
    }
    if kind == 2 && !ny_std_fs_list(path, line, col).is_empty() {
        ny_fs_fail(what, path, "not empty", line, col);
    }
    let r = if kind == 2 { std::fs::remove_dir(path) } else { std::fs::remove_file(path) };
    if let Err(e) = r {
        ny_fs_fail(what, path, ny_io_reason(&e), line, col);
    }
}

fn ny_std_fs_mkdir(path: &str, line: u32, col: u32) {
    let what = "fs.mkdir: cannot create";
    if path.is_empty() || path.contains('\0') {
        ny_fs_fail(what, path, "not found", line, col);
    }
    if ny_kind(path) != 0 {
        ny_fs_fail(what, path, "already exists", line, col);
    }
    if let Err(e) = std::fs::create_dir(path) {
        ny_fs_fail(what, path, ny_io_reason(&e), line, col);
    }
}

// ---- input: standard input as bytes (the same on every system) ----

fn ny_in_text(b: Vec<u8>, what: &str, line: u32, col: u32) -> Str {
    match String::from_utf8(b) {
        Ok(s) => Rc::new(s),
        Err(_) => ny_fail("E0341", &format!("{what}: the input is not valid UTF-8"), "standard input must be UTF-8 text", line, col),
    }
}

fn ny_std_input_line(line: u32, col: u32) -> Str {
    use std::io::{BufRead as _, Write as _};
    let _ = std::io::stdout().flush();
    let mut b = Vec::new();
    let _ = std::io::stdin().lock().read_until(b'\n', &mut b);
    if b.last() == Some(&b'\n') {
        b.pop();
        if b.last() == Some(&b'\r') {
            b.pop();
        }
    }
    ny_in_text(b, "input.line", line, col)
}

fn ny_std_input_eof(line: u32, col: u32) -> bool {
    use std::io::{BufRead as _, Write as _};
    let _ = std::io::stdout().flush();
    std::io::stdin().lock().fill_buf().map_or(true, |b| b.is_empty())
}

fn ny_read_rest() -> Vec<u8> {
    use std::io::{Read as _, Write as _};
    let _ = std::io::stdout().flush();
    let mut b = Vec::new();
    let _ = std::io::stdin().lock().read_to_end(&mut b);
    b
}

fn ny_std_input_all(line: u32, col: u32) -> Str {
    ny_in_text(ny_read_rest(), "input.all", line, col)
}

/// The lines of a text: each without its `\n` (and a `\r` before it); no last empty line.
fn ny_lines(s: &str) -> Rc<Vec<Str>> {
    let mut out = Vec::new();
    let mut rest = s;
    while !rest.is_empty() {
        let (l, next) = match rest.find('\n') {
            Some(i) => (rest[..i].strip_suffix('\r').unwrap_or(&rest[..i]), &rest[i + 1..]),
            None => (rest, ""),
        };
        out.push(Rc::new(l.to_string()));
        rest = next;
    }
    Rc::new(out)
}

fn ny_std_input_lines(line: u32, col: u32) -> Rc<Vec<Str>> {
    let text = ny_in_text(ny_read_rest(), "input.lines", line, col);
    ny_lines(&text)
}

// ---- os ----

fn ny_std_os_args(line: u32, col: u32) -> Rc<Vec<Str>> {
    let mut out = Vec::new();
    for a in std::env::args_os().skip(1) {
        match a.into_string() {
            Ok(s) => out.push(Rc::new(s)),
            Err(_) => ny_fail("E0341", "os.args: an argument is not valid UTF-8", "program arguments must be UTF-8 text", line, col),
        }
    }
    Rc::new(out)
}

fn ny_getenv(name: &str, line: u32, col: u32) -> Option<String> {
    if name.is_empty() || name.contains('=') || name.contains('\0') {
        return None;
    }
    match std::env::var_os(name)?.into_string() {
        Ok(v) => Some(v),
        Err(_) => ny_fail("E0341", "os.env: the value is not valid UTF-8", "environment variables must be UTF-8 text", line, col),
    }
}

fn ny_std_os_env(name: &str, line: u32, col: u32) -> Str {
    Rc::new(ny_getenv(name, line, col).unwrap_or_default())
}

fn ny_std_os_has_env(name: &str, line: u32, col: u32) -> bool {
    ny_getenv(name, line, col).is_some()
}

fn ny_std_os_exit(code: i64, line: u32, col: u32) {
    use std::io::Write as _;
    let _ = std::io::stdout().flush();
    std::process::exit((code & 255) as i32)
}

// ---- time ----

fn ny_std_time_now_ms(line: u32, col: u32) -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

fn ny_std_time_mono_ms(line: u32, col: u32) -> f64 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START.get_or_init(std::time::Instant::now).elapsed().as_nanos() as f64 / 1e6
}

fn ny_std_time_sleep_ms(ms: i64, line: u32, col: u32) {
    use std::io::Write as _;
    let _ = std::io::stdout().flush();
    if ms > 0 {
        std::thread::sleep(std::time::Duration::from_millis(ms as u64));
    }
}

// ---- random: the operating system's generator, or after `random.seed(n)` xoshiro128** ----

thread_local! {
    static NY_SEEDED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static NY_RS: std::cell::Cell<[u32; 4]> = const { std::cell::Cell::new([0; 4]) };
    static NY_OS_COUNT: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

fn ny_mix32(mut z: u32) -> u32 {
    z = (z ^ (z >> 16)).wrapping_mul(0x85EB_CA6B);
    z = (z ^ (z >> 13)).wrapping_mul(0xC2B2_AE35);
    z ^ (z >> 16)
}

fn ny_bits32() -> u32 {
    if NY_SEEDED.with(|s| s.get()) {
        return NY_RS.with(|c| {
            let mut s = c.get();
            let r = s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
            let t = s[1] << 9;
            s[2] ^= s[0];
            s[3] ^= s[1];
            s[1] ^= s[2];
            s[0] ^= s[3];
            s[2] ^= t;
            s[3] = s[3].rotate_left(11);
            c.set(s);
            r
        });
    }
    // the standard library's hasher keys come from the operating system's generator
    use std::hash::{BuildHasher as _, Hasher as _};
    let n = NY_OS_COUNT.with(|c| {
        let v = c.get();
        c.set(v + 1);
        v
    });
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u64(n);
    (h.finish() >> 16) as u32
}

fn ny_std_random_seed(n: i64, line: u32, col: u32) {
    let u = n as u64;
    let half = [u as u32, (u >> 32) as u32];
    let mut s = [0u32; 4];
    for (i, w) in s.iter_mut().enumerate() {
        *w = ny_mix32(half[i & 1].wrapping_add((i as u32 + 1).wrapping_mul(0x9E37_79B9)));
    }
    if s == [0; 4] {
        s[0] = 1;
    }
    NY_RS.with(|c| c.set(s));
    NY_SEEDED.with(|c| c.set(true));
}

/// 53 random bits: the first draw gives the high 27, the second the low 26.
fn ny_bits53() -> i64 {
    let a = (ny_bits32() >> 5) as i64;
    let b = (ny_bits32() >> 6) as i64;
    a * 67108864 + b
}

fn ny_std_random_random(line: u32, col: u32) -> f64 {
    ny_bits53() as f64 / 9007199254740992.0
}

fn ny_std_random_range(lo: i64, hi: i64, line: u32, col: u32) -> i64 {
    let n = (hi as u64).wrapping_sub(lo as u64);
    if hi <= lo || n > 9007199254740992 {
        ny_fail(
            "E0342",
            &format!("random.range({lo}, {hi}): need lo < hi and hi - lo <= 2^53"),
            "the upper bound is excluded: `random.range(1, 7)` rolls a die",
            line,
            col,
        );
    }
    let limit = 9007199254740992 - (9007199254740992u64 % n) as i64;
    let mut r = ny_bits53();
    while r >= limit {
        r = ny_bits53();
    }
    (lo as u64).wrapping_add(r as u64 % n) as i64
}

// ---- math: operations that are exact on every host ----

fn ny_std_math_sqrt(x: f64, line: u32, col: u32) -> f64 {
    x.sqrt()
}

fn ny_std_math_floor(x: f64, line: u32, col: u32) -> f64 {
    x.floor()
}

fn ny_std_math_ceil(x: f64, line: u32, col: u32) -> f64 {
    x.ceil()
}

fn ny_std_math_trunc(x: f64, line: u32, col: u32) -> f64 {
    x.trunc()
}

/// Half away from zero (x - trunc(x) is exact).
fn ny_std_math_round(x: f64, line: u32, col: u32) -> f64 {
    let mut t = x.trunc();
    if (x - t).abs() >= 0.5 {
        t += if x < 0.0 { -1.0 } else { 1.0 };
    }
    t
}

// ---- text ----

fn ny_std_text_is_int(s: &str, line: u32, col: u32) -> bool {
    let digits = s.strip_prefix('-').unwrap_or(s);
    !digits.is_empty() && digits.bytes().all(|c| c.is_ascii_digit()) && s.parse::<i64>().is_ok()
}

fn ny_std_text_is_float(s: &str, line: u32, col: u32) -> bool {
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
fn ny_std_text_fixed(x: f64, d: i64, line: u32, col: u32) -> Str {
    if !(0..=100).contains(&d) {
        ny_fail("E0342", &format!("text.fixed: digits must be 0 to 100, got {d}"), "`text.fixed(x, 2)` shows two decimals", line, col);
    }
    if !x.is_finite() {
        return Rc::new(ny_num(x));
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
    Rc::new(out)
}
