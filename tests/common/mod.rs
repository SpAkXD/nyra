//! Helpers shared by the integration tests: running `nyra`, and a small JSON reader
//! (the compiler prints JSON by hand and has no dependencies, and neither do its tests).
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// A test part that needs a tool this machine lacks: skipped with a note, unless
/// `NYRA_REQUIRE_ALL_TARGETS=1` (CI on Linux, where every toolchain is installed) makes it a failure.
pub fn missing(what: &str) {
    if std::env::var("NYRA_REQUIRE_ALL_TARGETS").is_ok_and(|v| v == "1") {
        panic!("{what}, but NYRA_REQUIRE_ALL_TARGETS=1 requires every target");
    }
    eprintln!("skipped: {what}");
}

pub fn nyra() -> Command {
    Command::new(env!("CARGO_BIN_EXE_nyra"))
}

/// A fresh scratch directory for one test.
pub fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("nyra-test-{name}"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

pub fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n")
}

pub fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).replace("\r\n", "\n")
}

/// The flags a test program asks for with a line `// flags: --sandbox --allow fs` (one of its
/// first three lines): `nyra check`, `run` and `test` get them in front of the file.
pub fn flags_of(src: &str) -> Vec<String> {
    src.lines()
        .take(3)
        .find_map(|l| l.strip_prefix("// flags:"))
        .map(|f| f.split_whitespace().map(String::from).collect())
        .unwrap_or_default()
}

/// `nyra check --json <file>` run in `dir`, parsed.
pub fn check_json(dir: &Path, file: &str) -> (bool, Json) {
    check_json_with(dir, file, &[])
}

/// `nyra check --json <flags> <file>` run in `dir`, parsed.
pub fn check_json_with(dir: &Path, file: &str, flags: &[String]) -> (bool, Json) {
    let out = nyra().current_dir(dir).args(["check", "--json"]).args(flags).arg(file).output().unwrap();
    let text = stdout(&out);
    let json = Json::parse(text.trim()).unwrap_or_else(|e| panic!("`nyra check --json {file}` printed bad JSON ({e}):\n{text}"));
    (out.status.success(), json)
}

#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    pub fn parse(s: &str) -> Result<Json, String> {
        let chars: Vec<char> = s.chars().collect();
        let mut p = Reader { chars, pos: 0 };
        let v = p.value()?;
        p.skip_ws();
        if p.pos != p.chars.len() {
            return Err(format!("trailing text at offset {}", p.pos));
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

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Json::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Json]> {
        match self {
            Json::Arr(a) => Some(a),
            _ => None,
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Json::Num(n) if *n >= 0.0 && n.fract() == 0.0 => Some(*n as u64),
            _ => None,
        }
    }

    pub fn keys(&self) -> Vec<&str> {
        match self {
            Json::Obj(fields) => fields.iter().map(|(k, _)| k.as_str()).collect(),
            _ => Vec::new(),
        }
    }
}

struct Reader {
    chars: Vec<char>,
    pos: usize,
}

impl Reader {
    fn skip_ws(&mut self) {
        while self.pos < self.chars.len() && self.chars[self.pos].is_whitespace() {
            self.pos += 1;
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn eat(&mut self, c: char) -> Result<(), String> {
        if self.peek() == Some(c) {
            self.pos += 1;
            Ok(())
        } else {
            Err(format!("expected `{c}` at offset {}, found {:?}", self.pos, self.peek()))
        }
    }

    fn word(&mut self, w: &str, v: Json) -> Result<Json, String> {
        for c in w.chars() {
            self.eat(c)?;
        }
        Ok(v)
    }

    fn value(&mut self) -> Result<Json, String> {
        self.skip_ws();
        match self.peek() {
            Some('{') => {
                self.pos += 1;
                let mut fields = Vec::new();
                self.skip_ws();
                if self.peek() == Some('}') {
                    self.pos += 1;
                    return Ok(Json::Obj(fields));
                }
                loop {
                    self.skip_ws();
                    let key = self.string()?;
                    self.skip_ws();
                    self.eat(':')?;
                    let v = self.value()?;
                    fields.push((key, v));
                    self.skip_ws();
                    match self.peek() {
                        Some(',') => self.pos += 1,
                        Some('}') => {
                            self.pos += 1;
                            return Ok(Json::Obj(fields));
                        }
                        other => return Err(format!("expected `,` or `}}` at offset {}, found {other:?}", self.pos)),
                    }
                }
            }
            Some('[') => {
                self.pos += 1;
                let mut items = Vec::new();
                self.skip_ws();
                if self.peek() == Some(']') {
                    self.pos += 1;
                    return Ok(Json::Arr(items));
                }
                loop {
                    items.push(self.value()?);
                    self.skip_ws();
                    match self.peek() {
                        Some(',') => self.pos += 1,
                        Some(']') => {
                            self.pos += 1;
                            return Ok(Json::Arr(items));
                        }
                        other => return Err(format!("expected `,` or `]` at offset {}, found {other:?}", self.pos)),
                    }
                }
            }
            Some('"') => Ok(Json::Str(self.string()?)),
            Some('t') => self.word("true", Json::Bool(true)),
            Some('f') => self.word("false", Json::Bool(false)),
            Some('n') => self.word("null", Json::Null),
            Some(c) if c == '-' || c.is_ascii_digit() => {
                let start = self.pos;
                while self.peek().is_some_and(|c| c == '-' || c == '+' || c == '.' || c == 'e' || c == 'E' || c.is_ascii_digit()) {
                    self.pos += 1;
                }
                let text: String = self.chars[start..self.pos].iter().collect();
                text.parse().map(Json::Num).map_err(|_| format!("bad number `{text}`"))
            }
            other => Err(format!("unexpected {other:?} at offset {}", self.pos)),
        }
    }

    fn string(&mut self) -> Result<String, String> {
        self.eat('"')?;
        let mut out = String::new();
        loop {
            let c = self.peek().ok_or("unterminated string")?;
            self.pos += 1;
            match c {
                '"' => return Ok(out),
                '\\' => {
                    let e = self.peek().ok_or("unterminated escape")?;
                    self.pos += 1;
                    match e {
                        '"' => out.push('"'),
                        '\\' => out.push('\\'),
                        '/' => out.push('/'),
                        'b' => out.push('\u{8}'),
                        'f' => out.push('\u{c}'),
                        'n' => out.push('\n'),
                        'r' => out.push('\r'),
                        't' => out.push('\t'),
                        'u' => {
                            let first = self.hex4()?;
                            let code = if (0xD800..0xDC00).contains(&first) {
                                self.eat('\\')?;
                                self.eat('u')?;
                                let second = self.hex4()?;
                                0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00)
                            } else {
                                first
                            };
                            out.push(char::from_u32(code).ok_or("bad \\u escape")?);
                        }
                        other => return Err(format!("bad escape `\\{other}`")),
                    }
                }
                c if (c as u32) < 0x20 => return Err("raw control character in a string".into()),
                c => out.push(c),
            }
        }
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let mut v = 0;
        for _ in 0..4 {
            let c = self.peek().ok_or("short \\u escape")?;
            self.pos += 1;
            v = v * 16 + c.to_digit(16).ok_or("bad \\u escape")?;
        }
        Ok(v)
    }
}
