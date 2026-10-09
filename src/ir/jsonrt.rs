//! `json.str` and `json.parse` for the IR interpreter, by the value's type. The text written and
//! the errors read are those of the runtimes of the compiled targets (`src/rt/rs/json.rs`).

use std::rc::Rc;

use super::interp::{fail, num, Stop, Value};
use super::{Module, Ty};
use crate::ast::Span;

/// `json.str(v)`: the compact JSON text of a value. A map never gets here (the checker refuses it).
pub fn encode(m: &Module, v: &Value, out: &mut String) {
    match v {
        Value::Int(n) => out.push_str(&n.to_string()),
        Value::Float(x) => out.push_str(&if x.is_finite() { num(*x) } else { "null".to_string() }),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Char(c) => string(&c.to_string(), out),
        Value::Str(s) => string(s, out),
        Value::Arr(xs) => {
            out.push('[');
            for (i, x) in xs.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                encode(m, x, out);
            }
            out.push(']');
        }
        Value::Struct(id, fields) => {
            let names = m.structs.get(Ty::Struct(*id)).map(|s| &s.fields[..]).unwrap_or(&[]);
            if fields.is_empty() {
                out.push('{');
            }
            for (k, ((name, _), x)) in names.iter().zip(fields.iter()).enumerate() {
                out.push(if k == 0 { '{' } else { ',' });
                out.push_str(&crate::diag::json_str(name));
                out.push(':');
                encode(m, x, out);
            }
            out.push('}');
        }
        Value::Map(_) | Value::Unset => out.push_str("null"),
    }
}

fn string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// `json.parse(text)` into a value of type `ty`.
pub fn decode(m: &Module, ty: Ty, text: &str, span: Span) -> Result<Value, Stop> {
    let mut p = Parser { m, s: text.as_bytes(), i: 0, depth: 0, path: Vec::new(), span };
    let v = p.value(ty)?;
    p.ws();
    if p.i < p.s.len() {
        return Err(p.syntax("text after the value"));
    }
    Ok(v)
}

struct Parser<'a> {
    m: &'a Module,
    s: &'a [u8],
    i: usize,
    depth: u32,
    /// The path to the value being read: `.items`, `[0]`, ...
    path: Vec<String>,
    span: Span,
}

impl Parser<'_> {
    fn syntax(&self, what: &str) -> Stop {
        let line = self.s[..self.i.min(self.s.len())].iter().filter(|&&c| c == b'\n').count() + 1;
        fail(
            "E0345",
            format!("json.parse: invalid JSON at line {line}: {what}"),
            "check the JSON text: it must be one value, with keys and strings in double quotes",
            self.span,
        )
    }

    fn type_err(&self, what: &str) -> Stop {
        fail(
            "E0345",
            format!("json.parse: expected {what} at ${}", self.path.concat()),
            "the JSON text must have the shape of the type it is read into",
            self.span,
        )
    }

    fn missing(&self, field: &str) -> Stop {
        fail(
            "E0345",
            format!("json.parse: missing field \"{field}\" at ${}", self.path.concat()),
            "the JSON object must have every field of the struct",
            self.span,
        )
    }

    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    fn ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.i += 1;
        }
    }

    /// Skips white space to the start of a value: a syntax error unless one can start here.
    fn start(&mut self) -> Result<u8, Stop> {
        self.ws();
        match self.peek() {
            None => Err(self.syntax("unexpected end of the text")),
            Some(c) if b"{[\"tfn-0123456789".contains(&c) => Ok(c),
            Some(_) => Err(self.syntax("expected a value")),
        }
    }

    /// `[` or `{`: true if a first element follows, false for an empty one (already closed).
    fn open(&mut self, open: u8, what: &str) -> Result<bool, Stop> {
        if self.start()? != open {
            return Err(self.type_err(what));
        }
        self.depth += 1;
        if self.depth > 500 {
            return Err(self.syntax("nested too deeply"));
        }
        self.i += 1;
        self.ws();
        let close = if open == b'[' { b']' } else { b'}' };
        if self.peek() == Some(close) {
            self.i += 1;
            self.depth -= 1;
            return Ok(false);
        }
        Ok(true)
    }

    /// After an element: true at `,` (another one follows), false at the closing bracket.
    fn next(&mut self, close: u8) -> Result<bool, Stop> {
        self.ws();
        match self.peek() {
            Some(b',') => {
                self.i += 1;
                Ok(true)
            }
            Some(c) if c == close => {
                self.i += 1;
                self.depth -= 1;
                Ok(false)
            }
            None => Err(self.syntax("unexpected end of the text")),
            Some(_) => Err(self.syntax(&format!("expected `,` or `{}`", close as char))),
        }
    }

    fn hex(&self, at: usize) -> Option<u32> {
        let h = self.s.get(at..at + 4)?;
        if !h.iter().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        u32::from_str_radix(std::str::from_utf8(h).ok()?, 16).ok()
    }

    /// A string; the position is at its `"`.
    fn string(&mut self) -> Result<String, Stop> {
        let mut out: Vec<u8> = Vec::new();
        self.i += 1;
        loop {
            let Some(c) = self.peek() else { return Err(self.syntax("unterminated string")) };
            if c == b'"' {
                self.i += 1;
                return Ok(String::from_utf8(out).unwrap_or_default());
            }
            if c < 0x20 {
                return Err(self.syntax("control character in a string"));
            }
            if c != b'\\' {
                out.push(c);
                self.i += 1;
                continue;
            }
            self.i += 1;
            let Some(e) = self.peek() else { return Err(self.syntax("unterminated string")) };
            let simple = match e {
                b'"' => Some('"'),
                b'\\' => Some('\\'),
                b'/' => Some('/'),
                b'b' => Some('\u{8}'),
                b'f' => Some('\u{c}'),
                b'n' => Some('\n'),
                b'r' => Some('\r'),
                b't' => Some('\t'),
                _ => None,
            };
            let ch = if let Some(ch) = simple {
                self.i += 1;
                ch
            } else {
                if e != b'u' {
                    return Err(self.syntax("invalid escape"));
                }
                let mut cp = match self.hex(self.i + 1) {
                    Some(v) if !(0xDC00..=0xDFFF).contains(&v) => v,
                    _ => return Err(self.syntax("invalid escape")),
                };
                self.i += 5;
                if (0xD800..=0xDBFF).contains(&cp) {
                    let lo = if self.s.get(self.i..self.i + 2) == Some(b"\\u") { self.hex(self.i + 2) } else { None };
                    match lo {
                        Some(lo) if (0xDC00..=0xDFFF).contains(&lo) => cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00),
                        _ => return Err(self.syntax("invalid escape")),
                    }
                    self.i += 6;
                }
                char::from_u32(cp).unwrap_or('\u{fffd}')
            };
            let mut buf = [0u8; 4];
            out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
        }
    }

    /// A number: where its text starts and ends, and whether it has no fraction and no exponent.
    fn number(&mut self) -> Result<(usize, usize, bool), Stop> {
        let start = self.i;
        let digit = |p: &Self| p.peek().is_some_and(|c| c.is_ascii_digit());
        let mut whole = true;
        if self.peek() == Some(b'-') {
            self.i += 1;
        }
        if self.peek() == Some(b'0') {
            self.i += 1;
        } else if digit(self) {
            while digit(self) {
                self.i += 1;
            }
        } else {
            return Err(self.syntax("invalid number"));
        }
        if self.peek() == Some(b'.') {
            self.i += 1;
            if !digit(self) {
                return Err(self.syntax("invalid number"));
            }
            while digit(self) {
                self.i += 1;
            }
            whole = false;
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.i += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.i += 1;
            }
            if !digit(self) {
                return Err(self.syntax("invalid number"));
            }
            while digit(self) {
                self.i += 1;
            }
            whole = false;
        }
        Ok((start, self.i, whole))
    }

    fn literal(&mut self) -> Result<&'static str, Stop> {
        for w in ["true", "false", "null"] {
            if self.s[self.i..].starts_with(w.as_bytes()) {
                self.i += w.len();
                return Ok(w);
            }
        }
        Err(self.syntax("expected a value"))
    }

    /// An object's key and the `:` after it.
    fn key(&mut self) -> Result<String, Stop> {
        self.ws();
        match self.peek() {
            None => return Err(self.syntax("unexpected end of the text")),
            Some(b'"') => {}
            Some(_) => return Err(self.syntax("expected a string key")),
        }
        let k = self.string()?;
        self.ws();
        match self.peek() {
            None => return Err(self.syntax("unexpected end of the text")),
            Some(b':') => self.i += 1,
            Some(_) => return Err(self.syntax("expected `:`")),
        }
        Ok(k)
    }

    /// Any value, only checked (an object's fields that the type does not have).
    fn skip(&mut self) -> Result<(), Stop> {
        let c = self.start()?;
        match c {
            b'{' | b'[' => {
                if self.open(c, "")? {
                    loop {
                        if c == b'{' {
                            self.key()?;
                        }
                        self.skip()?;
                        if !self.next(if c == b'{' { b'}' } else { b']' })? {
                            break;
                        }
                    }
                }
            }
            b'"' => {
                self.string()?;
            }
            b'-' | b'0'..=b'9' => {
                self.number()?;
            }
            _ => {
                self.literal()?;
            }
        }
        Ok(())
    }

    /// A value of type `ty`.
    fn value(&mut self, ty: Ty) -> Result<Value, Stop> {
        match ty {
            Ty::Int => {
                let c = self.start()?;
                if c != b'-' && !c.is_ascii_digit() {
                    return Err(self.type_err("an int"));
                }
                let (a, b, whole) = self.number()?;
                match std::str::from_utf8(&self.s[a..b]).ok().and_then(|t| t.parse::<i64>().ok()) {
                    Some(v) if whole => Ok(Value::Int(v)),
                    _ => Err(self.type_err("an int")),
                }
            }
            Ty::Float => {
                let c = self.start()?;
                if c != b'-' && !c.is_ascii_digit() {
                    return Err(self.type_err("a number"));
                }
                let (a, b, _) = self.number()?;
                Ok(Value::Float(std::str::from_utf8(&self.s[a..b]).ok().and_then(|t| t.parse::<f64>().ok()).unwrap_or(0.0)))
            }
            Ty::Bool => {
                let c = self.start()?;
                if c != b't' && c != b'f' {
                    return Err(self.type_err("true or false"));
                }
                Ok(Value::Bool(self.literal()? == "true"))
            }
            Ty::Char => {
                if self.start()? != b'"' {
                    return Err(self.type_err("a one-character string"));
                }
                let t = self.string()?;
                let mut cs = t.chars();
                match (cs.next(), cs.next()) {
                    (Some(c), None) => Ok(Value::Char(c)),
                    _ => Err(self.type_err("a one-character string")),
                }
            }
            Ty::Str => {
                if self.start()? != b'"' {
                    return Err(self.type_err("a string"));
                }
                Ok(Value::Str(Rc::new(self.string()?)))
            }
            Ty::Array(_) => {
                let elem = ty.elem().unwrap_or(Ty::Unknown);
                let mut v = Vec::new();
                if self.open(b'[', "an array")? {
                    loop {
                        self.path.push(format!("[{}]", v.len()));
                        v.push(self.value(elem)?);
                        self.path.pop();
                        if !self.next(b']')? {
                            break;
                        }
                    }
                }
                Ok(Value::Arr(Rc::new(v)))
            }
            Ty::Struct(id) => {
                let Some(info) = self.m.structs.get(ty) else { return Err(self.type_err("a known struct")) };
                let mut got: Vec<Option<Value>> = vec![None; info.fields.len()];
                if self.open(b'{', "an object")? {
                    loop {
                        let k = self.key()?;
                        match info.fields.iter().position(|(f, _)| *f == k) {
                            Some(at) => {
                                self.path.push(format!(".{k}"));
                                got[at] = Some(self.value(info.fields[at].1)?);
                                self.path.pop();
                            }
                            None => self.skip()?,
                        }
                        if !self.next(b'}')? {
                            break;
                        }
                    }
                }
                let mut fields = Vec::with_capacity(got.len());
                for (v, (name, _)) in got.into_iter().zip(&info.fields) {
                    match v {
                        Some(v) => fields.push(v),
                        None => return Err(self.missing(name)),
                    }
                }
                Ok(Value::Struct(id, Rc::new(fields)))
            }
            _ => Err(self.type_err("a type json.parse can read")),
        }
    }
}
