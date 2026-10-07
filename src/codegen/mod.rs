//! Backends. Each one turns the IR (see `crate::ir`) into source code for a target.

pub mod c;
pub mod js;
mod names;

/// `(a + b)` → `a + b` when one pair of parentheses wraps the whole expression. Only for
/// places where nothing binds tighter: a condition, an assignment's or `return`'s value, a call
/// argument (the backends never emit comma expressions).
pub(crate) fn bare(s: &str) -> &str {
    let b = s.as_bytes();
    if b.len() < 2 || b[0] != b'(' || b[b.len() - 1] != b')' {
        return s;
    }
    let (mut depth, mut quote, mut i) = (0i32, None, 0);
    while i < b.len() {
        let c = b[i];
        if let Some(q) = quote {
            if c == b'\\' {
                i += 2;
                continue;
            }
            if c == q {
                quote = None;
            }
        } else {
            match c {
                b'"' | b'`' => quote = Some(c),
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 && i != b.len() - 1 {
                        return s; // `(a) + (b)`: the first pair closes early
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    &s[1..s.len() - 1]
}
