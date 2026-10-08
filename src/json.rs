//! A small JSON reader and writer for `nyra mcp` (the compiler has no dependencies).
//!
//! Numbers keep their source text, so a JSON-RPC request id such as `12345678901234567890` is
//! echoed back exactly. Objects keep their key order, so output is stable.

use std::fmt;

use crate::diag::json_str;

#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    /// The number as written (already validated against the JSON grammar).
    Num(String),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

/// Nesting deeper than this is rejected instead of overflowing the stack.
const MAX_DEPTH: usize = 128;

impl Json {
    pub fn parse(text: &str) -> Result<Json, String> {
        let mut r = Reader { s: text.as_bytes(), pos: 0 };
        let v = r.value(0)?;
        r.skip_ws();
        if r.pos != r.s.len() {
            return Err(format!("unexpected text after the value at byte {}", r.pos));
        }
        Ok(v)
    }

    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Json::Num(n) => n.parse().ok(),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Json]> {
        match self {
            Json::Arr(a) => Some(a),
            _ => None,
        }
    }

    pub fn is_object(&self) -> bool {
        matches!(self, Json::Obj(_))
    }

    /// A number with a fixed number of decimals, e.g. a time in milliseconds.
    pub fn fixed(x: f64, decimals: usize) -> Json {
        if x.is_finite() {
            Json::Num(format!("{x:.decimals$}"))
        } else {
            Json::Null
        }
    }
}

/// `obj([("a", 1.into()), ...])`
pub fn obj<const N: usize>(fields: [(&str, Json); N]) -> Json {
    Json::Obj(fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

impl From<&str> for Json {
    fn from(s: &str) -> Json {
        Json::Str(s.to_string())
    }
}

impl From<String> for Json {
    fn from(s: String) -> Json {
        Json::Str(s)
    }
}

impl From<bool> for Json {
    fn from(b: bool) -> Json {
        Json::Bool(b)
    }
}

impl From<i64> for Json {
    fn from(n: i64) -> Json {
        Json::Num(n.to_string())
    }
}

impl From<Vec<Json>> for Json {
    fn from(v: Vec<Json>) -> Json {
        Json::Arr(v)
    }
}

/// Compact output: no spaces, no newlines (one message per line on the wire).
impl fmt::Display for Json {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Json::Null => f.write_str("null"),
            Json::Bool(b) => write!(f, "{b}"),
            Json::Num(n) => f.write_str(n),
            Json::Str(s) => f.write_str(&json_str(s)),
            Json::Arr(items) => {
                f.write_str("[")?;
                for (i, v) in items.iter().enumerate() {
                    if i > 0 {
                        f.write_str(",")?;
                    }
                    write!(f, "{v}")?;
                }
                f.write_str("]")
            }
            Json::Obj(fields) => {
                f.write_str("{")?;
                for (i, (k, v)) in fields.iter().enumerate() {
                    if i > 0 {
                        f.write_str(",")?;
                    }
                    write!(f, "{}:{v}", json_str(k))?;
                }
                f.write_str("}")
            }
        }
    }
}

struct Reader<'a> {
    s: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.s.get(self.pos).copied()
    }

    fn found(&self) -> String {
        match self.peek() {
            None => "the end of the input".to_string(),
            Some(_) => {
                let rest = String::from_utf8_lossy(&self.s[self.pos..]);
                format!("`{}` at byte {}", rest.chars().next().unwrap_or('?'), self.pos)
            }
        }
    }

    fn eat(&mut self, c: u8) -> Result<(), String> {
        if self.peek() == Some(c) {
            self.pos += 1;
            Ok(())
        } else {
            Err(format!("expected `{}`, found {}", c as char, self.found()))
        }
    }

    fn word(&mut self, w: &str, v: Json) -> Result<Json, String> {
        if self.s[self.pos..].starts_with(w.as_bytes()) {
            self.pos += w.len();
            Ok(v)
        } else {
            Err(format!("unexpected {}", self.found()))
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, String> {
        if depth > MAX_DEPTH {
            return Err(format!("nested deeper than {MAX_DEPTH} levels"));
        }
        self.skip_ws();
        match self.peek() {
            Some(b'{') => {
                self.pos += 1;
                let mut fields = Vec::new();
                self.skip_ws();
                if self.peek() == Some(b'}') {
                    self.pos += 1;
                    return Ok(Json::Obj(fields));
                }
                loop {
                    self.skip_ws();
                    let key = self.string()?;
                    self.skip_ws();
                    self.eat(b':')?;
                    let v = self.value(depth + 1)?;
                    fields.push((key, v));
                    self.skip_ws();
                    match self.peek() {
                        Some(b',') => self.pos += 1,
                        Some(b'}') => {
                            self.pos += 1;
                            return Ok(Json::Obj(fields));
                        }
                        _ => return Err(format!("expected `,` or `}}`, found {}", self.found())),
                    }
                }
            }
            Some(b'[') => {
                self.pos += 1;
                let mut items = Vec::new();
                self.skip_ws();
                if self.peek() == Some(b']') {
                    self.pos += 1;
                    return Ok(Json::Arr(items));
                }
                loop {
                    items.push(self.value(depth + 1)?);
                    self.skip_ws();
                    match self.peek() {
                        Some(b',') => self.pos += 1,
                        Some(b']') => {
                            self.pos += 1;
                            return Ok(Json::Arr(items));
                        }
                        _ => return Err(format!("expected `,` or `]`, found {}", self.found())),
                    }
                }
            }
            Some(b'"') => Ok(Json::Str(self.string()?)),
            Some(b't') => self.word("true", Json::Bool(true)),
            Some(b'f') => self.word("false", Json::Bool(false)),
            Some(b'n') => self.word("null", Json::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err(format!("unexpected {}", self.found())),
        }
    }

    /// `-? (0 | [1-9][0-9]*) (. [0-9]+)? ([eE] [+-]? [0-9]+)?`
    fn number(&mut self) -> Result<Json, String> {
        let start = self.pos;
        let digits = |r: &mut Self| {
            let from = r.pos;
            while r.peek().is_some_and(|c| c.is_ascii_digit()) {
                r.pos += 1;
            }
            r.pos > from
        };
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        let int_start = self.pos;
        if !digits(self) {
            return Err(format!("bad number at byte {start}"));
        }
        if self.s[int_start] == b'0' && self.pos - int_start > 1 {
            return Err(format!("bad number at byte {start}: leading zero"));
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            if !digits(self) {
                return Err(format!("bad number at byte {start}"));
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if !digits(self) {
                return Err(format!("bad number at byte {start}"));
            }
        }
        // the slice is ASCII, so this cannot fail
        Ok(Json::Num(String::from_utf8_lossy(&self.s[start..self.pos]).into_owned()))
    }

    fn string(&mut self) -> Result<String, String> {
        self.eat(b'"')?;
        let mut out: Vec<u8> = Vec::new();
        loop {
            let Some(c) = self.peek() else { return Err("unterminated string".into()) };
            self.pos += 1;
            match c {
                b'"' => return String::from_utf8(out).map_err(|_| "a string is not valid UTF-8".to_string()),
                b'\\' => {
                    let Some(e) = self.peek() else { return Err("unterminated string".into()) };
                    self.pos += 1;
                    let ch = match e {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let first = self.hex4()?;
                            let code = if (0xD800..0xDC00).contains(&first) {
                                // a surrogate pair: `\ud83d\ude00`
                                let second = if self.s[self.pos..].starts_with(b"\\u") {
                                    self.pos += 2;
                                    self.hex4()?
                                } else {
                                    0
                                };
                                if !(0xDC00..0xE000).contains(&second) {
                                    return Err("a \\u escape is half of a surrogate pair".into());
                                }
                                0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00)
                            } else {
                                first
                            };
                            char::from_u32(code).ok_or("a \\u escape is half of a surrogate pair")?
                        }
                        _ => return Err(format!("bad escape `\\{}` at byte {}", e as char, self.pos - 2)),
                    };
                    let mut buf = [0; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                }
                c if c < 0x20 => return Err(format!("a raw control character in a string at byte {}", self.pos - 1)),
                c => out.push(c),
            }
        }
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let mut v = 0;
        for _ in 0..4 {
            let d = self.peek().and_then(|c| (c as char).to_digit(16)).ok_or("bad \\u escape")?;
            self.pos += 1;
            v = v * 16 + d;
        }
        Ok(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_compactly() {
        let text = r#"{"a":[1,-2.5,3e10,true,false,null],"b":"x\"y\\z\n\u0001","c":{},"d":[]}"#;
        let v = Json::parse(text).unwrap();
        assert_eq!(v.to_string(), text);
        let spaced = "{ \"a\" : [ 1 , 2 ] ,\r\n\t\"b\" : { } }";
        assert_eq!(Json::parse(spaced).unwrap().to_string(), r#"{"a":[1,2],"b":{}}"#);
    }

    #[test]
    fn keeps_numbers_exactly() {
        let v = Json::parse("12345678901234567890").unwrap();
        assert_eq!(v.to_string(), "12345678901234567890");
        assert_eq!(Json::parse("0.5").unwrap().as_f64(), Some(0.5));
    }

    #[test]
    fn decodes_escapes_and_utf8() {
        let v = Json::parse(r#""\u00e9\ud83d\ude00 é 😀 \/""#).unwrap();
        assert_eq!(v.as_str(), Some("é😀 é 😀 /"));
        assert_eq!(Json::Str("é\u{7}".into()).to_string(), "\"é\\u0007\"");
    }

    #[test]
    fn rejects_bad_input() {
        for bad in [
            "", "{", "[1,]", "{\"a\" 1}", "01", "1.", "-", "1e", "tru", "\"abc", "\"\\x\"", "\"a\nb\"",
            "\"\\ud800\"", "{} x", "{1:2}", "nul",
        ] {
            assert!(Json::parse(bad).is_err(), "accepted {bad:?}");
        }
        let deep = "[".repeat(200) + &"]".repeat(200);
        assert!(Json::parse(&deep).is_err());
        let ok = "[".repeat(100) + &"]".repeat(100);
        assert!(Json::parse(&ok).is_ok());
    }

    #[test]
    fn builds_objects() {
        let v = obj([("ok", true.into()), ("n", 3i64.into()), ("s", "hi".into()), ("t", Json::fixed(1.234, 1))]);
        assert_eq!(v.to_string(), r#"{"ok":true,"n":3,"s":"hi","t":1.2}"#);
        assert_eq!(v.get("s").and_then(Json::as_str), Some("hi"));
    }
}
