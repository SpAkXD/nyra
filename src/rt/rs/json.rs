
// ---- json: `json.str(v)` and `json.parse(text)` by the value's type ----

trait NyJson: Sized {
    /// Only `str`: a map with this key type is a JSON object.
    const STR_KEY: bool = false;
    fn ny_jenc(&self, out: &mut String);
    fn ny_jdec(p: &mut NyJP) -> Self;
    /// Only `str`: a key of a JSON object.
    fn ny_jfrom_key(_: String) -> Self {
        unreachable!("only a str is a key of a JSON object")
    }
}

fn ny_jstr<T: NyJson>(v: &T) -> Str {
    let mut out = String::new();
    v.ny_jenc(&mut out);
    Rc::new(out)
}

fn ny_jparse<T: NyJson>(text: &str, line: u32, col: u32) -> T {
    let mut p = NyJP { s: text.as_bytes(), i: 0, depth: 0, path: Vec::new(), line, col };
    let v = T::ny_jdec(&mut p);
    p.ws();
    if p.i < p.s.len() {
        p.syntax("text after the value");
    }
    v
}

fn ny_jenc_str(s: &str, out: &mut String) {
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

/// The parser: the text, the position, the nesting and the path to the value being read.
struct NyJP<'a> {
    s: &'a [u8],
    i: usize,
    depth: u32,
    path: Vec<String>,
    line: u32,
    col: u32,
}

impl NyJP<'_> {
    fn syntax(&self, what: &str) -> ! {
        let line = self.s[..self.i.min(self.s.len())].iter().filter(|&&c| c == b'\n').count() + 1;
        ny_fail(
            "E0345",
            &format!("json.parse: invalid JSON at line {line}: {what}"),
            "check the JSON text: it must be one value, with keys and strings in double quotes",
            self.line,
            self.col,
        )
    }

    fn type_err(&self, what: &str) -> ! {
        ny_fail(
            "E0345",
            &format!("json.parse: expected {what} at ${}", self.path.concat()),
            "the JSON text must have the shape of the type it is read into",
            self.line,
            self.col,
        )
    }

    /// A tuple, the values of a variant or a map pair: an array of exactly `n` elements; this opens it.
    fn seq_open(&mut self, n: usize) {
        if !self.open(b'[', &format!("an array of {n} elements")) {
            self.type_err(&format!("an array of {n} elements"));
        }
    }

    /// After the element `k` of `n`: the array must go on, or end, as the length says.
    fn seq_after(&mut self, k: usize, n: usize) {
        if self.next(b']') != (k + 1 < n) {
            self.type_err(&format!("an array of {n} elements"));
        }
    }

    fn variant_err(&self, en: &str) -> ! {
        self.type_err(&format!("a variant of {en}: a name, or {{\"Name\": [values]}}"))
    }

    /// An enum: a variant's name, and true if it was the key of an object (the position is after its `:`).
    fn variant(&mut self, en: &str) -> (String, bool) {
        match self.start() {
            b'"' => (self.string(), false),
            b'{' => {
                if !self.open(b'{', "") {
                    self.variant_err(en);
                }
                (self.key(), true)
            }
            _ => self.variant_err(en),
        }
    }

    /// After the values of a variant that was an object's key: nothing else may follow in the object.
    fn variant_end(&mut self, en: &str) {
        if self.next(b'}') {
            self.variant_err(en);
        }
    }

    fn missing(&self, field: &str) -> ! {
        ny_fail(
            "E0345",
            &format!("json.parse: missing field \"{field}\" at ${}", self.path.concat()),
            "the JSON object must have every field of the struct",
            self.line,
            self.col,
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
    fn start(&mut self) -> u8 {
        self.ws();
        match self.peek() {
            None => self.syntax("unexpected end of the text"),
            Some(c) if b"{[\"tfn-0123456789".contains(&c) => c,
            Some(_) => self.syntax("expected a value"),
        }
    }

    /// `[` or `{`: true if a first element follows, false for an empty one (already closed).
    fn open(&mut self, open: u8, what: &str) -> bool {
        if self.start() != open {
            self.type_err(what);
        }
        self.depth += 1;
        if self.depth > 500 {
            self.syntax("nested too deeply");
        }
        self.i += 1;
        self.ws();
        let close = if open == b'[' { b']' } else { b'}' };
        if self.peek() == Some(close) {
            self.i += 1;
            self.depth -= 1;
            return false;
        }
        true
    }

    /// After an element: true at `,` (another one follows), false at the closing bracket.
    fn next(&mut self, close: u8) -> bool {
        self.ws();
        match self.peek() {
            Some(b',') => {
                self.i += 1;
                true
            }
            Some(c) if c == close => {
                self.i += 1;
                self.depth -= 1;
                false
            }
            None => self.syntax("unexpected end of the text"),
            Some(_) => self.syntax(&format!("expected `,` or `{}`", close as char)),
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
    fn string(&mut self) -> String {
        let mut out: Vec<u8> = Vec::new();
        self.i += 1;
        loop {
            let Some(c) = self.peek() else { self.syntax("unterminated string") };
            if c == b'"' {
                self.i += 1;
                return String::from_utf8(out).unwrap_or_default();
            }
            if c < 0x20 {
                self.syntax("control character in a string");
            }
            if c != b'\\' {
                out.push(c);
                self.i += 1;
                continue;
            }
            self.i += 1;
            let Some(e) = self.peek() else { self.syntax("unterminated string") };
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
                    self.syntax("invalid escape");
                }
                let mut cp = match self.hex(self.i + 1) {
                    Some(v) if !(0xDC00..=0xDFFF).contains(&v) => v,
                    _ => self.syntax("invalid escape"),
                };
                self.i += 5;
                if (0xD800..=0xDBFF).contains(&cp) {
                    let lo = if self.s.get(self.i..self.i + 2) == Some(b"\\u") { self.hex(self.i + 2) } else { None };
                    match lo {
                        Some(lo) if (0xDC00..=0xDFFF).contains(&lo) => cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00),
                        _ => self.syntax("invalid escape"),
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
    fn number(&mut self) -> (usize, usize, bool) {
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
            self.syntax("invalid number");
        }
        if self.peek() == Some(b'.') {
            self.i += 1;
            if !digit(self) {
                self.syntax("invalid number");
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
                self.syntax("invalid number");
            }
            while digit(self) {
                self.i += 1;
            }
            whole = false;
        }
        (start, self.i, whole)
    }

    fn literal(&mut self) -> &'static str {
        for w in ["true", "false", "null"] {
            if self.s[self.i..].starts_with(w.as_bytes()) {
                self.i += w.len();
                return w;
            }
        }
        self.syntax("expected a value")
    }

    /// An object's key and the `:` after it.
    fn key(&mut self) -> String {
        self.ws();
        match self.peek() {
            None => self.syntax("unexpected end of the text"),
            Some(b'"') => {}
            Some(_) => self.syntax("expected a string key"),
        }
        let k = self.string();
        self.ws();
        match self.peek() {
            None => self.syntax("unexpected end of the text"),
            Some(b':') => self.i += 1,
            Some(_) => self.syntax("expected `:`"),
        }
        k
    }

    /// Any value, only checked (an object's fields that the type does not have).
    fn skip(&mut self) {
        let c = self.start();
        match c {
            b'{' | b'[' => {
                if self.open(c, "") {
                    loop {
                        if c == b'{' {
                            self.key();
                        }
                        self.skip();
                        if !self.next(if c == b'{' { b'}' } else { b']' }) {
                            break;
                        }
                    }
                }
            }
            b'"' => {
                self.string();
            }
            b'-' | b'0'..=b'9' => {
                self.number();
            }
            _ => {
                self.literal();
            }
        }
    }
}

impl NyJson for i64 {
    fn ny_jenc(&self, out: &mut String) {
        out.push_str(&self.to_string());
    }
    fn ny_jdec(p: &mut NyJP) -> Self {
        let c = p.start();
        if c != b'-' && !c.is_ascii_digit() {
            p.type_err("an int");
        }
        let (a, b, whole) = p.number();
        let s = p.s;
        match std::str::from_utf8(&s[a..b]).ok().and_then(|t| t.parse::<i64>().ok()) {
            Some(v) if whole => v,
            _ => p.type_err("an int"),
        }
    }
}

impl NyJson for f64 {
    fn ny_jenc(&self, out: &mut String) {
        out.push_str(&if self.is_finite() { ny_num(*self) } else { "null".to_string() });
    }
    fn ny_jdec(p: &mut NyJP) -> Self {
        let c = p.start();
        if c != b'-' && !c.is_ascii_digit() {
            p.type_err("a number");
        }
        let (a, b, _) = p.number();
        let s = p.s;
        std::str::from_utf8(&s[a..b]).ok().and_then(|t| t.parse::<f64>().ok()).unwrap_or(0.0)
    }
}

impl NyJson for bool {
    fn ny_jenc(&self, out: &mut String) {
        out.push_str(if *self { "true" } else { "false" });
    }
    fn ny_jdec(p: &mut NyJP) -> Self {
        let c = p.start();
        if c != b't' && c != b'f' {
            p.type_err("true or false");
        }
        p.literal() == "true"
    }
}

impl NyJson for char {
    fn ny_jenc(&self, out: &mut String) {
        ny_jenc_str(&self.to_string(), out);
    }
    fn ny_jdec(p: &mut NyJP) -> Self {
        if p.start() != b'"' {
            p.type_err("a one-character string");
        }
        let s = p.string();
        let mut cs = s.chars();
        match (cs.next(), cs.next()) {
            (Some(c), None) => c,
            _ => p.type_err("a one-character string"),
        }
    }
}

impl NyJson for Str {
    const STR_KEY: bool = true;
    fn ny_jfrom_key(k: String) -> Self {
        Rc::new(k)
    }
    fn ny_jenc(&self, out: &mut String) {
        ny_jenc_str(self, out);
    }
    fn ny_jdec(p: &mut NyJP) -> Self {
        if p.start() != b'"' {
            p.type_err("a string");
        }
        Rc::new(p.string())
    }
}

impl<T: NyJson + Clone> NyJson for Rc<Vec<T>> {
    fn ny_jenc(&self, out: &mut String) {
        out.push('[');
        for (i, x) in self.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            x.ny_jenc(out);
        }
        out.push(']');
    }
    fn ny_jdec(p: &mut NyJP) -> Self {
        let mut v = Vec::new();
        if p.open(b'[', "an array") {
            loop {
                p.path.push(format!("[{}]", v.len()));
                v.push(T::ny_jdec(p));
                p.path.pop();
                if !p.next(b']') {
                    break;
                }
            }
        }
        Rc::new(v)
    }
}

impl<K: NyJson + Clone + Eq + std::hash::Hash, V: NyJson + Clone> NyJson for Rc<NyMap<K, V>> {
    fn ny_jenc(&self, out: &mut String) {
        out.push(if K::STR_KEY { '{' } else { '[' });
        for (i, (k, v)) in self.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            if K::STR_KEY {
                k.ny_jenc(out);
                out.push(':');
            } else {
                out.push('[');
                k.ny_jenc(out);
                out.push(',');
            }
            v.ny_jenc(out);
            if !K::STR_KEY {
                out.push(']');
            }
        }
        out.push(if K::STR_KEY { '}' } else { ']' });
    }
    fn ny_jdec(p: &mut NyJP) -> Self {
        let mut m: NyMap<K, V> = NyMap::default();
        if K::STR_KEY {
            if p.open(b'{', "an object") {
                loop {
                    let k = p.key();
                    p.path.push(format!(".{k}"));
                    let v = V::ny_jdec(p);
                    p.path.pop();
                    m.set(K::ny_jfrom_key(k), v);
                    if !p.next(b'}') {
                        break;
                    }
                }
            }
        } else if p.open(b'[', "an array") {
            let mut i = 0;
            loop {
                p.path.push(format!("[{i}]"));
                p.seq_open(2);
                p.path.push("[0]".to_string());
                let k = K::ny_jdec(p);
                p.path.pop();
                p.seq_after(0, 2);
                p.path.push("[1]".to_string());
                let v = V::ny_jdec(p);
                p.path.pop();
                p.seq_after(1, 2);
                p.path.pop();
                m.set(k, v);
                i += 1;
                if !p.next(b']') {
                    break;
                }
            }
        }
        Rc::new(m)
    }
}
