// ---- Nyra runtime (Rust) ------------------------------------------------------------------
// Strings and arrays are reference counted and copied on write (`Rc::make_mut`), so assigning
// one is cheap and two variables never see each other's changes. Ints wrap on overflow (the
// program is built with overflow checks off), floats print like JavaScript's String(x), and
// lengths and indexes of strings count characters.

/// A Nyra `str`: shared, and copied when it is changed while shared (`s += t`).
type Str = Rc<String>;

const NY_FILE: &str = @FILE@;

/// A Nyra runtime error: printed after all earlier output, exit code 101.
fn ny_fail(code: &str, msg: &str, hint: &str, line: u32, col: u32) -> ! {
    use std::io::Write;
    let _ = std::io::stdout().flush();
    if std::env::var_os("NYRA_JSON").is_some() {
        eprintln!(
            "{{\"ok\":false,\"errors\":[{{\"code\":\"{code}\",\"message\":{},\"file\":{},\"line\":{line},\"col\":{col},\"hint\":{},\"runtime\":true}}]}}",
            ny_json(msg),
            ny_json(NY_FILE),
            ny_json(hint)
        );
    } else {
        eprintln!("runtime error[{code}]: {msg}\n  --> {NY_FILE}:{line}:{col}\n  = hint: {hint}\n  = explain: nyra explain {code}");
    }
    std::process::exit(101)
}

fn ny_json(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn ny_oom(line: u32, col: u32) -> ! {
    ny_fail("E0249", "out of memory", "the program needs more memory than the system gave it", line, col)
}

// ---- ints ----

/// int `/`: division by zero is an error; `i64::MIN / -1` wraps.
fn ny_div(a: i64, b: i64, line: u32, col: u32) -> i64 {
    if b == 0 {
        ny_fail("E0241", "division by zero", "check the divisor first", line, col);
    }
    a.wrapping_div(b)
}

fn ny_rem(a: i64, b: i64, line: u32, col: u32) -> i64 {
    if b == 0 {
        ny_fail("E0241", "division by zero", "check the divisor first", line, col);
    }
    a.wrapping_rem(b)
}

/// `xs.min()` / `xs.max()` of an empty array (`n` elements seen; `max` says which method).
fn ny_check_non_empty(n: i64, max: i64, line: u32, col: u32) {
    if n == 0 {
        ny_fail("E0247", if max != 0 { "max() of an empty array" } else { "min() of an empty array" }, "an empty array has no smallest or largest element: check `xs.len() > 0` first, or start from a value of your own with `fold`", line, col);
    }
}

/// `for i in a..b step k`: a step of 0 would never end.
fn ny_check_step(k: i64, line: u32, col: u32) {
    if k == 0 {
        ny_fail("E0243", "range step must not be 0", "use a positive step to count up and a negative one to count down", line, col);
    }
}

/// int(x) of a float: truncates toward zero; NaN or a value outside the int range is an error.
fn ny_f2i(x: f64, line: u32, col: u32) -> i64 {
    if x.is_nan() || x >= 9223372036854775807.0 || x < -9223372036854775808.0 {
        ny_fail("E0245", &format!("cannot convert {} to int", ny_num(x)), "int(x) needs a float that is not NaN and fits in an int", line, col);
    }
    x as i64
}

/// A float as JavaScript's String(x) shows it: the shortest digits that read back the same.
fn ny_num(x: f64) -> String {
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
    // `{:e}` gives the shortest digits: "1.2345e-7"
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

// ---- strings: UTF-8, but lengths and indexes count characters ----

fn ny_str(s: &str) -> Str {
    Rc::new(s.to_string())
}

fn ny_len(s: &str) -> i64 {
    if s.is_ascii() { s.len() as i64 } else { s.chars().count() as i64 }
}

fn ny_oob(i: i64, n: i64, line: u32, col: u32) -> ! {
    ny_fail("E0240", &format!("index {i} is out of bounds for length {n}"), "valid indexes are 0 to len - 1; compare with `.len()` first", line, col)
}

fn ny_range(a: i64, b: i64, n: i64, line: u32, col: u32) {
    if a < 0 || a > b || b > n {
        ny_fail("E0240", &format!("range {a}..{b} is out of bounds for length {n}"), "a range a..b needs 0 <= a <= b <= len", line, col);
    }
}

/// s[i]
fn ny_char_at(s: &str, i: i64, line: u32, col: u32) -> char {
    let n = ny_len(s);
    if i < 0 || i >= n {
        ny_oob(i, n, line, col);
    }
    if s.is_ascii() { s.as_bytes()[i as usize] as char } else { s.chars().nth(i as usize).unwrap_or('\0') }
}

fn ny_str_slice(s: &str, a: i64, b: i64, line: u32, col: u32) -> Str {
    ny_range(a, b, ny_len(s), line, col);
    if s.is_ascii() {
        return ny_str(&s[a as usize..b as usize]);
    }
    Rc::new(s.chars().skip(a as usize).take((b - a) as usize).collect())
}

/// The character position of `t` in `s`, or -1.
fn ny_find(s: &str, t: &str) -> i64 {
    match s.find(t) {
        Some(j) => ny_len(&s[..j]),
        None => -1,
    }
}

fn ny_replace(s: &str, old: &str, new: &str, line: u32, col: u32) -> Str {
    if old.is_empty() {
        ny_fail("E0243", "replace() needs a non-empty pattern", "the text to replace can't be \"\"", line, col);
    }
    Rc::new(s.replace(old, new))
}

fn ny_trim(s: &str) -> Str {
    ny_str(s.trim_matches(|c| matches!(c, ' ' | '\t' | '\n' | '\r')))
}

fn ny_str_repeat(s: &str, n: i64, line: u32, col: u32) -> Str {
    if n < 0 {
        ny_fail("E0243", &format!("repeat count must be >= 0, got {n}"), "repeat(n) needs n >= 0", line, col);
    }
    // the longest text every backend can make (in UTF-8 bytes)
    if !s.is_empty() && n > 536870888 / s.len() as i64 {
        ny_oom(line, col);
    }
    Rc::new(s.repeat(n as usize))
}

/// `s.pad_left(n, c)` / `s.pad_right(n, c)`: `c` added until `s` has `n` characters.
fn ny_pad(s: &str, n: i64, c: char, left: bool) -> Str {
    let missing = n - ny_len(s);
    if missing <= 0 {
        return ny_str(s);
    }
    if missing > 536870888 {
        ny_oom(0, 0);
    }
    let fill: String = std::iter::repeat(c).take(missing as usize).collect();
    Rc::new(if left { fill + s } else { format!("{s}{fill}") })
}

fn ny_is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r')
}

fn ny_chr(n: i64, line: u32, col: u32) -> char {
    match char::from_u32(n as u32) {
        Some(c) if (0..=1114111).contains(&n) => c,
        _ => ny_fail("E0246", &format!("char({n}): not a valid character code"), "character codes go from 0 to 1114111, except 55296 to 57343", line, col),
    }
}

/// The text of a string in an error message: control characters as escapes.
fn ny_shown(s: &str) -> String {
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
fn ny_int(s: &str, line: u32, col: u32) -> i64 {
    let digits = s.strip_prefix('-').unwrap_or(s);
    if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
        if let Ok(v) = s.parse::<i64>() {
            return v;
        }
    }
    ny_fail("E0244", &format!("cannot parse \"{}\" as int", ny_shown(s)), "int(s) accepts only digits with an optional `-`, e.g. \"-42\"", line, col)
}

/// float(s): -?[0-9]+(.[0-9]+)?([eE][+-]?[0-9]+)?
fn ny_float(s: &str, line: u32, col: u32) -> f64 {
    let b = s.as_bytes();
    let digits = |mut i: usize| -> Option<usize> {
        let start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i > start { Some(i) } else { None }
    };
    let mut ok = || -> Option<()> {
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
        if i == b.len() { Some(()) } else { None }
    };
    match ok() {
        Some(()) => s.parse().unwrap_or(f64::NAN),
        None => ny_fail(
            "E0244",
            &format!("cannot parse \"{}\" as float", ny_shown(s)),
            "float(s) accepts digits with an optional `-`, `.` part and exponent, e.g. \"-1.5e3\"",
            line,
            col,
        ),
    }
}

fn ny_split(s: &str, sep: &str, line: u32, col: u32) -> Rc<Vec<Str>> {
    if sep.is_empty() {
        ny_fail("E0243", "split() needs a non-empty separator", "for the characters of a string use `s.chars()`", line, col);
    }
    Rc::new(s.split(sep).map(ny_str).collect())
}

fn ny_join(xs: &[Str], sep: &str) -> Str {
    let parts: Vec<&str> = xs.iter().map(|s| s.as_str()).collect();
    Rc::new(parts.join(sep))
}

fn ny_join_chars(xs: &[char], sep: &str) -> Str {
    let parts: Vec<String> = xs.iter().map(|c| c.to_string()).collect();
    Rc::new(parts.join(sep))
}

// ---- arrays: Rc<Vec<T>>, copied on write by Rc::make_mut ----

/// xs[i], checked (E0240).
fn ny_at<T>(xs: &[T], i: i64, line: u32, col: u32) -> &T {
    if i < 0 || i >= xs.len() as i64 {
        ny_oob(i, xs.len() as i64, line, col);
    }
    &xs[i as usize]
}

/// xs[i] to change: the array is copied first when it is shared.
fn ny_at_mut<T: Clone>(xs: &mut Rc<Vec<T>>, i: i64, line: u32, col: u32) -> &mut T {
    if i < 0 || i >= xs.len() as i64 {
        ny_oob(i, xs.len() as i64, line, col);
    }
    &mut Rc::make_mut(xs)[i as usize]
}

fn ny_pop<T: Clone>(xs: &mut Rc<Vec<T>>, line: u32, col: u32) -> T {
    match Rc::make_mut(xs).pop() {
        Some(v) => v,
        None => ny_fail("E0242", "pop() on an empty array", "check `xs.len() > 0` first", line, col),
    }
}

fn ny_insert<T: Clone>(xs: &mut Rc<Vec<T>>, i: i64, v: T, line: u32, col: u32) {
    if i < 0 || i > xs.len() as i64 {
        ny_fail("E0240", &format!("index {i} is out of bounds for length {}", xs.len()), "insert(i, x) needs 0 <= i <= len", line, col);
    }
    Rc::make_mut(xs).insert(i as usize, v);
}

fn ny_remove<T: Clone>(xs: &mut Rc<Vec<T>>, i: i64, line: u32, col: u32) -> T {
    if i < 0 || i >= xs.len() as i64 {
        ny_oob(i, xs.len() as i64, line, col);
    }
    Rc::make_mut(xs).remove(i as usize)
}

fn ny_swap<T: Clone>(xs: &mut Rc<Vec<T>>, i: i64, j: i64, line: u32, col: u32) {
    let n = xs.len() as i64;
    if i < 0 || i >= n {
        ny_oob(i, n, line, col);
    }
    if j < 0 || j >= n {
        ny_oob(j, n, line, col);
    }
    Rc::make_mut(xs).swap(i as usize, j as usize);
}

fn ny_slice<T: Clone>(xs: &[T], a: i64, b: i64, line: u32, col: u32) -> Rc<Vec<T>> {
    ny_range(a, b, xs.len() as i64, line, col);
    Rc::new(xs[a as usize..b as usize].to_vec())
}

fn ny_concat<T: Clone>(a: &[T], b: &[T]) -> Rc<Vec<T>> {
    Rc::new([a, b].concat())
}

fn ny_repeat<T: Clone>(xs: &[T], n: i64, line: u32, col: u32) -> Rc<Vec<T>> {
    if n < 0 {
        ny_fail("E0243", &format!("repeat count must be >= 0, got {n}"), "repeat(n) needs n >= 0", line, col);
    }
    // the longest array every backend makes with repeat
    if !xs.is_empty() && n > 100000000 / xs.len() as i64 {
        ny_oom(line, col);
    }
    let mut out = Vec::with_capacity(xs.len() * n as usize);
    for _ in 0..n {
        out.extend_from_slice(xs);
    }
    Rc::new(out)
}

/// `xs += ys`
fn ny_append<T: Clone>(xs: &mut Rc<Vec<T>>, ys: &[T]) {
    Rc::make_mut(xs).extend_from_slice(ys);
}

fn ny_index_of<T: PartialEq>(xs: &[T], v: &T) -> i64 {
    xs.iter().position(|x| x == v).map_or(-1, |i| i as i64)
}

/// Sorts floats like every backend: stable, NaN after every number.
fn ny_sort_floats(xs: &mut Rc<Vec<f64>>) {
    use std::cmp::Ordering;
    let lt = |x: f64, y: f64| x < y || (y.is_nan() && !x.is_nan());
    Rc::make_mut(xs).sort_by(|a, b| if lt(*a, *b) { Ordering::Less } else if lt(*b, *a) { Ordering::Greater } else { Ordering::Equal });
}

/// `xs.sort_by(x => key)`: a stable sort of the positions by the keys (floats: NaN after every
/// number), then the elements move to their places.
fn ny_sort_by<T: Clone, K>(xs: &mut Rc<Vec<T>>, ks: &[K], lt: fn(&K, &K) -> bool) {
    use std::cmp::Ordering;
    let mut idx: Vec<usize> = (0..ks.len()).collect();
    idx.sort_by(|&i, &j| if lt(&ks[i], &ks[j]) { Ordering::Less } else if lt(&ks[j], &ks[i]) { Ordering::Greater } else { Ordering::Equal });
    let v = Rc::make_mut(xs);
    let mut old: Vec<Option<T>> = std::mem::take(v).into_iter().map(Some).collect();
    *v = idx.iter().map(|&i| old[i].take().expect("each position once")).collect();
}

fn ny_lt_ord<K: PartialOrd>(x: &K, y: &K) -> bool {
    x < y
}

fn ny_lt_float(x: &f64, y: &f64) -> bool {
    x < y || (y.is_nan() && !x.is_nan())
}

// ---- printing: arrays and structs as Nyra code ----

trait NyShow {
    fn show_in(&self, out: &mut String);
}

/// A value as `print` shows it inside an array or a struct (strings and chars quoted).
fn ny_show<T: NyShow>(v: &T) -> String {
    let mut out = String::new();
    v.show_in(&mut out);
    out
}

fn ny_quoted(out: &mut String, text: &str, quote: char) {
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

impl NyShow for i64 {
    fn show_in(&self, out: &mut String) {
        out.push_str(&self.to_string());
    }
}

impl NyShow for f64 {
    fn show_in(&self, out: &mut String) {
        out.push_str(&ny_num(*self));
    }
}

impl NyShow for bool {
    fn show_in(&self, out: &mut String) {
        out.push_str(if *self { "true" } else { "false" });
    }
}

impl NyShow for char {
    fn show_in(&self, out: &mut String) {
        ny_quoted(out, self.encode_utf8(&mut [0; 4]), '\'');
    }
}

impl NyShow for Str {
    fn show_in(&self, out: &mut String) {
        ny_quoted(out, self, '"');
    }
}

impl<T: NyShow> NyShow for Rc<Vec<T>> {
    fn show_in(&self, out: &mut String) {
        out.push('[');
        for (i, v) in self.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            v.show_in(out);
        }
        out.push(']');
    }
}
