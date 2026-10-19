//! A forgiving HTML reader for `nyra clone`: a tokenizer and a tree builder that accept what
//! browsers accept (void elements, unclosed `p`/`li`/`td`, stray end tags, unquoted attributes),
//! decode character references, and keep the text of `script` and `style` apart (the CSS of
//! `style` is read for the theme; scripts are only counted). It does not run the full HTML5
//! algorithm: it only has to see the structure a person sees.

/// One node: an element (`tag` is its lowercase name), a text (`#text`) or the document (`#document`).
pub struct Node {
    pub tag: String,
    pub attrs: Vec<(String, String)>,
    pub kids: Vec<usize>,
    pub parent: usize,
    /// The text of a `#text` node, or the raw content of `script`, `style`, `textarea`, `title`.
    pub text: String,
}

pub struct Dom {
    pub nodes: Vec<Node>,
}

/// Elements that never have content.
const VOID: &[&str] =
    &["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source", "track", "wbr", "keygen"];

/// Elements whose content is text up to their end tag.
const RAW: &[&str] = &["script", "style", "textarea", "title", "xmp", "iframe", "noembed", "noframes", "noscript", "template"];

/// Start tags that close an open `p`.
const CLOSES_P: &[&str] = &[
    "address",
    "article",
    "aside",
    "blockquote",
    "details",
    "div",
    "dl",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hr",
    "main",
    "menu",
    "nav",
    "ol",
    "p",
    "pre",
    "section",
    "table",
    "ul",
];

impl Dom {
    pub fn parse(html: &str) -> Dom {
        let mut b = Builder { dom: Dom { nodes: vec![new_node("#document", 0)] }, open: vec![0] };
        let s = html;
        let bytes = s.as_bytes();
        let mut i = 0;
        let mut text_start = 0;
        while i < bytes.len() {
            if bytes[i] != b'<' {
                i += 1;
                continue;
            }
            let next = bytes.get(i + 1).copied().unwrap_or(0);
            if next == b'!' || next == b'?' || next == b'/' || next.is_ascii_alphabetic() {
                b.text(&s[text_start..i]);
            } else {
                i += 1;
                continue;
            }
            if s[i..].starts_with("<!--") {
                i = s[i + 4..].find("-->").map_or(bytes.len(), |e| i + 4 + e + 3);
            } else if next == b'!' || next == b'?' {
                i = s[i..].find('>').map_or(bytes.len(), |e| i + e + 1);
            } else if next == b'/' {
                let end = s[i..].find('>').map_or(bytes.len(), |e| i + e);
                let name: String = s[i + 2..end].trim().split(|c: char| c.is_whitespace()).next().unwrap_or("").to_ascii_lowercase();
                if !name.is_empty() {
                    b.end(&name);
                }
                i = (end + 1).min(bytes.len());
            } else {
                let (tag, end) = read_tag(s, i);
                i = end;
                let name = tag.name.clone();
                let raw = RAW.contains(&name.as_str()) && !tag.closed;
                let id = b.start(tag);
                if raw {
                    // the content runs to `</name`, in any case
                    let close = format!("</{name}");
                    let rest = &s[i..];
                    let at = find_ci(rest, &close).unwrap_or(rest.len());
                    let content = &rest[..at];
                    b.dom.nodes[id].text = if name == "title" || name == "textarea" { decode(content) } else { content.to_string() };
                    b.end(&name);
                    i += at;
                    i = s[i..].find('>').map_or(bytes.len(), |e| i + e + 1);
                }
            }
            text_start = i;
        }
        b.text(&s[text_start.min(s.len())..]);
        b.dom
    }

    pub fn attr(&self, id: usize, name: &str) -> Option<&str> {
        self.nodes[id].attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    pub fn tag(&self, id: usize) -> &str {
        &self.nodes[id].tag
    }

    pub fn is_elem(&self, id: usize) -> bool {
        !self.nodes[id].tag.starts_with('#')
    }

    /// The element children of `id`.
    pub fn elems(&self, id: usize) -> Vec<usize> {
        self.nodes[id].kids.iter().copied().filter(|&k| self.is_elem(k)).collect()
    }

    /// The class names of an element.
    pub fn classes(&self, id: usize) -> Vec<&str> {
        self.attr(id, "class").map(|c| c.split_whitespace().collect()).unwrap_or_default()
    }

    /// Every element below `id` (not `id` itself), in document order.
    pub fn descendants(&self, id: usize) -> Vec<usize> {
        let mut out = Vec::new();
        let mut todo: Vec<usize> = self.nodes[id].kids.iter().rev().copied().collect();
        while let Some(n) = todo.pop() {
            if self.is_elem(n) {
                out.push(n);
                todo.extend(self.nodes[n].kids.iter().rev().copied());
            }
        }
        out
    }

    /// The first element with this tag in the document (or below `from`).
    pub fn find(&self, from: usize, tag: &str) -> Option<usize> {
        self.descendants(from).into_iter().find(|&n| self.nodes[n].tag == tag)
    }

    pub fn find_all(&self, from: usize, tag: &str) -> Vec<usize> {
        self.descendants(from).into_iter().filter(|&n| self.nodes[n].tag == tag).collect()
    }

    /// True if `inner` is `outer` or below it.
    pub fn within(&self, inner: usize, outer: usize) -> bool {
        let mut n = inner;
        loop {
            if n == outer {
                return true;
            }
            if n == 0 {
                return false;
            }
            n = self.nodes[n].parent;
        }
    }
}

struct Tag {
    name: String,
    attrs: Vec<(String, String)>,
    /// `<x/>`
    closed: bool,
}

fn new_node(tag: &str, parent: usize) -> Node {
    Node { tag: tag.to_string(), attrs: Vec::new(), kids: Vec::new(), parent, text: String::new() }
}

/// Reads a start tag at `s[i] == '<'`; returns it and the index after its `>`.
fn read_tag(s: &str, i: usize) -> (Tag, usize) {
    let b = s.as_bytes();
    let mut j = i + 1;
    while j < b.len() && !b[j].is_ascii_whitespace() && b[j] != b'>' && b[j] != b'/' {
        j += 1;
    }
    let name = s[i + 1..j].to_ascii_lowercase();
    let mut attrs = Vec::new();
    let mut closed = false;
    loop {
        while j < b.len() && (b[j].is_ascii_whitespace() || (b[j] == b'/' && b.get(j + 1) != Some(&b'>'))) {
            j += 1;
        }
        if j >= b.len() {
            break;
        }
        if b[j] == b'>' {
            j += 1;
            break;
        }
        if b[j] == b'/' {
            closed = true;
            j += 1;
            continue;
        }
        let start = j;
        while j < b.len()
            && !b[j].is_ascii_whitespace()
            && b[j] != b'='
            && b[j] != b'>'
            && !(b[j] == b'/' && b.get(j + 1) == Some(&b'>'))
        {
            j += 1;
        }
        let key = s[start..j].to_ascii_lowercase();
        while j < b.len() && b[j].is_ascii_whitespace() {
            j += 1;
        }
        let mut value = String::new();
        if j < b.len() && b[j] == b'=' {
            j += 1;
            while j < b.len() && b[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < b.len() && (b[j] == b'"' || b[j] == b'\'') {
                let q = b[j];
                let vstart = j + 1;
                let end = s[vstart..].find(q as char).map_or(b.len(), |e| vstart + e);
                value = decode(&s[vstart..end]);
                j = (end + 1).min(b.len());
            } else {
                let vstart = j;
                while j < b.len() && !b[j].is_ascii_whitespace() && b[j] != b'>' {
                    j += 1;
                }
                value = decode(&s[vstart..j]);
            }
        }
        if !key.is_empty() && !attrs.iter().any(|(k, _): &(String, String)| *k == key) {
            attrs.push((key, value));
        }
    }
    (Tag { name, attrs, closed }, j)
}

fn find_ci(hay: &str, needle: &str) -> Option<usize> {
    let h = hay.as_bytes();
    let n = needle.as_bytes();
    if n.len() > h.len() {
        return None;
    }
    (0..=h.len() - n.len()).find(|&i| h[i..i + n.len()].eq_ignore_ascii_case(n))
}

struct Builder {
    dom: Dom,
    open: Vec<usize>,
}

impl Builder {
    fn cur(&self) -> usize {
        *self.open.last().unwrap_or(&0)
    }

    fn text(&mut self, raw: &str) {
        if raw.is_empty() {
            return;
        }
        let t = decode(raw);
        let parent = self.cur();
        // join with a text just before (a `&` split by nothing, or text around a dropped comment)
        if let Some(&last) = self.dom.nodes[parent].kids.last() {
            if self.dom.nodes[last].tag == "#text" {
                self.dom.nodes[last].text.push_str(&t);
                return;
            }
        }
        let id = self.dom.nodes.len();
        let mut n = new_node("#text", parent);
        n.text = t;
        self.dom.nodes.push(n);
        self.dom.nodes[parent].kids.push(id);
    }

    /// The index in `open` of the nearest open `tag`, not looking past one of `stop`.
    fn open_index(&self, tag: &str, stop: &[&str]) -> Option<usize> {
        for k in (1..self.open.len()).rev() {
            let t = &self.dom.nodes[self.open[k]].tag;
            if t == tag {
                return Some(k);
            }
            if stop.contains(&t.as_str()) {
                return None;
            }
        }
        None
    }

    fn close_to(&mut self, k: usize) {
        self.open.truncate(k);
    }

    fn start(&mut self, tag: Tag) -> usize {
        let name = tag.name.as_str();
        // a second <html> or <body>: its attributes join the first
        if name == "html" || name == "body" || name == "head" {
            if let Some(existing) = self.dom.nodes.iter().position(|n| n.tag == name) {
                for (k, v) in tag.attrs {
                    if !self.dom.nodes[existing].attrs.iter().any(|(a, _)| *a == k) {
                        self.dom.nodes[existing].attrs.push((k, v));
                    }
                }
                if name == "body" && !self.open.contains(&existing) {
                    // content after </body> or a stray <body>: continue inside the body
                    self.open.retain(|&o| o == 0 || self.dom.nodes[o].tag == "html");
                    self.open.push(existing);
                }
                return existing;
            }
            if name == "body" {
                // leave <head>
                if let Some(k) = self.open_index("head", &[]) {
                    self.close_to(k);
                }
            }
        }
        if CLOSES_P.contains(&name) {
            if let Some(k) = self.open_index("p", &["button", "table", "td", "th", "caption", "template", "html"]) {
                self.close_to(k);
            }
        }
        match name {
            "li" => {
                if let Some(k) = self.open_index("li", &["ul", "ol", "menu"]) {
                    self.close_to(k);
                }
            }
            "dt" | "dd" => {
                for t in ["dt", "dd"] {
                    if let Some(k) = self.open_index(t, &["dl"]) {
                        self.close_to(k);
                    }
                }
            }
            "tr" => {
                if let Some(k) = self.open_index("tr", &["table", "thead", "tbody", "tfoot"]) {
                    self.close_to(k);
                }
            }
            "td" | "th" => {
                for t in ["td", "th"] {
                    if let Some(k) = self.open_index(t, &["tr", "table"]) {
                        self.close_to(k);
                    }
                }
            }
            "thead" | "tbody" | "tfoot" => {
                for t in ["thead", "tbody", "tfoot"] {
                    if let Some(k) = self.open_index(t, &["table"]) {
                        self.close_to(k);
                    }
                }
            }
            "option" | "optgroup" => {
                if let Some(k) = self.open_index("option", &["select", "datalist"]) {
                    self.close_to(k);
                }
            }
            "a" => {
                // an <a> inside an open <a> closes it
                if let Some(k) = self.open_index("a", &[]) {
                    self.close_to(k);
                }
            }
            _ => {}
        }
        let parent = self.cur();
        let id = self.dom.nodes.len();
        let mut n = new_node(name, parent);
        n.attrs = tag.attrs;
        self.dom.nodes.push(n);
        self.dom.nodes[parent].kids.push(id);
        if !VOID.contains(&name) && !(tag.closed && is_foreign_or_custom(name)) {
            self.open.push(id);
        }
        id
    }

    fn end(&mut self, name: &str) {
        if name == "br" {
            self.start(Tag { name: "br".into(), attrs: Vec::new(), closed: true });
            return;
        }
        if name == "body" || name == "html" {
            return;
        }
        if let Some(k) = self.open_index(name, &[]) {
            self.close_to(k);
        }
    }
}

/// `<x/>` closes the element only for SVG/MathML parts and custom elements, as in browsers.
fn is_foreign_or_custom(name: &str) -> bool {
    name.contains('-')
        || matches!(
            name,
            "path"
                | "circle"
                | "rect"
                | "line"
                | "polyline"
                | "polygon"
                | "ellipse"
                | "use"
                | "stop"
                | "image"
                | "g"
                | "svg"
                | "symbol"
        )
}

/// Decodes character references (`&amp;`, `&#233;`, `&#x1F600;`); unknown ones stay as written.
pub fn decode(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let semi = rest[1..].find(|c: char| !(c.is_ascii_alphanumeric() || c == '#')).map(|e| e + 1).unwrap_or(rest.len());
        let name = &rest[1..semi];
        let has_semi = rest[semi..].starts_with(';');
        let decoded = if let Some(num) = name.strip_prefix('#') {
            let code = if let Some(hex) = num.strip_prefix('x').or_else(|| num.strip_prefix('X')) {
                u32::from_str_radix(hex, 16).ok()
            } else {
                num.parse::<u32>().ok()
            };
            code.map(|c| match c {
                0 => '\u{FFFD}',
                // the Windows-1252 codes that pages often write as numbers
                0x80..=0x9f => cp1252(c as u8),
                _ => char::from_u32(c).unwrap_or('\u{FFFD}'),
            })
            .map(|c| c.to_string())
        } else {
            entity(name).map(str::to_string)
        };
        match decoded {
            Some(d) if !name.is_empty() => {
                out.push_str(&d);
                rest = &rest[semi + usize::from(has_semi)..];
            }
            _ => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// The Windows-1252 character of a byte (what `iso-8859-1` pages mean by 0x80-0x9F).
pub fn cp1252(b: u8) -> char {
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž', '\u{8f}', '\u{90}', '‘', '’', '“', '”',
        '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}', 'ž', 'Ÿ',
    ];
    if (0x80..0xa0).contains(&b) {
        HIGH[(b - 0x80) as usize]
    } else {
        b as char
    }
}

/// The named character references pages use (the Latin-1 set, typography, arrows, a few symbols).
fn entity(name: &str) -> Option<&'static str> {
    Some(match name {
        "amp" | "AMP" => "&",
        "lt" | "LT" => "<",
        "gt" | "GT" => ">",
        "quot" | "QUOT" => "\"",
        "apos" => "'",
        "nbsp" => "\u{a0}",
        "ensp" | "emsp" | "thinsp" | "hairsp" | "numsp" | "puncsp" | "MediumSpace" => " ",
        "zwj" | "zwnj" | "shy" | "lrm" | "rlm" | "ZeroWidthSpace" => "",
        "copy" | "COPY" => "©",
        "reg" | "REG" => "®",
        "trade" | "TRADE" => "™",
        "hellip" | "mldr" => "…",
        "mdash" => "—",
        "ndash" => "–",
        "minus" => "−",
        "lsquo" => "‘",
        "rsquo" | "rsquor" => "’",
        "sbquo" => "‚",
        "ldquo" => "“",
        "rdquo" | "rdquor" => "”",
        "bdquo" => "„",
        "laquo" => "«",
        "raquo" => "»",
        "lsaquo" => "‹",
        "rsaquo" => "›",
        "bull" | "bullet" => "•",
        "middot" | "centerdot" => "·",
        "times" => "×",
        "divide" => "÷",
        "euro" => "€",
        "pound" => "£",
        "yen" => "¥",
        "cent" => "¢",
        "curren" => "¤",
        "deg" => "°",
        "plusmn" => "±",
        "para" => "¶",
        "sect" => "§",
        "dagger" => "†",
        "Dagger" => "‡",
        "permil" => "‰",
        "prime" => "′",
        "Prime" => "″",
        "larr" | "leftarrow" => "←",
        "rarr" | "rightarrow" => "→",
        "uarr" => "↑",
        "darr" => "↓",
        "harr" => "↔",
        "rArr" | "Rightarrow" => "⇒",
        "lArr" => "⇐",
        "check" | "checkmark" => "✓",
        "cross" => "✗",
        "star" => "☆",
        "starf" | "bigstar" => "★",
        "hearts" => "♥",
        "spades" => "♠",
        "clubs" => "♣",
        "diams" => "♦",
        "infin" => "∞",
        "ne" => "≠",
        "le" => "≤",
        "ge" => "≥",
        "asymp" => "≈",
        "frac12" => "½",
        "frac14" => "¼",
        "frac34" => "¾",
        "sup1" => "¹",
        "sup2" => "²",
        "sup3" => "³",
        "ordf" => "ª",
        "ordm" => "º",
        "micro" => "µ",
        "iexcl" => "¡",
        "iquest" => "¿",
        "brvbar" => "¦",
        "uml" => "¨",
        "macr" => "¯",
        "acute" => "´",
        "cedil" => "¸",
        "not" => "¬",
        "Agrave" => "À",
        "Aacute" => "Á",
        "Acirc" => "Â",
        "Atilde" => "Ã",
        "Auml" => "Ä",
        "Aring" => "Å",
        "AElig" => "Æ",
        "Ccedil" => "Ç",
        "Egrave" => "È",
        "Eacute" => "É",
        "Ecirc" => "Ê",
        "Euml" => "Ë",
        "Igrave" => "Ì",
        "Iacute" => "Í",
        "Icirc" => "Î",
        "Iuml" => "Ï",
        "ETH" => "Ð",
        "Ntilde" => "Ñ",
        "Ograve" => "Ò",
        "Oacute" => "Ó",
        "Ocirc" => "Ô",
        "Otilde" => "Õ",
        "Ouml" => "Ö",
        "Oslash" => "Ø",
        "Ugrave" => "Ù",
        "Uacute" => "Ú",
        "Ucirc" => "Û",
        "Uuml" => "Ü",
        "Yacute" => "Ý",
        "THORN" => "Þ",
        "szlig" => "ß",
        "agrave" => "à",
        "aacute" => "á",
        "acirc" => "â",
        "atilde" => "ã",
        "auml" => "ä",
        "aring" => "å",
        "aelig" => "æ",
        "ccedil" => "ç",
        "egrave" => "è",
        "eacute" => "é",
        "ecirc" => "ê",
        "euml" => "ë",
        "igrave" => "ì",
        "iacute" => "í",
        "icirc" => "î",
        "iuml" => "ï",
        "eth" => "ð",
        "ntilde" => "ñ",
        "ograve" => "ò",
        "oacute" => "ó",
        "ocirc" => "ô",
        "otilde" => "õ",
        "ouml" => "ö",
        "oslash" => "ø",
        "ugrave" => "ù",
        "uacute" => "ú",
        "ucirc" => "û",
        "uuml" => "ü",
        "yacute" => "ý",
        "thorn" => "þ",
        "yuml" => "ÿ",
        "Scaron" => "Š",
        "scaron" => "š",
        "Zcaron" => "Ž",
        "zcaron" => "ž",
        "Ccaron" => "Č",
        "ccaron" => "č",
        "Rcaron" => "Ř",
        "rcaron" => "ř",
        "Ecaron" => "Ě",
        "ecaron" => "ě",
        "Dcaron" => "Ď",
        "dcaron" => "ď",
        "Ncaron" => "Ň",
        "ncaron" => "ň",
        "Tcaron" => "Ť",
        "tcaron" => "ť",
        "Uring" => "Ů",
        "uring" => "ů",
        "OElig" => "Œ",
        "oelig" => "œ",
        "Yuml" => "Ÿ",
        "fnof" => "ƒ",
        "circ" => "ˆ",
        "tilde" => "˜",
        "alpha" => "α",
        "beta" => "β",
        "gamma" => "γ",
        "delta" => "δ",
        "pi" => "π",
        "mu" => "μ",
        "sigma" => "σ",
        "omega" => "ω",
        "Omega" => "Ω",
        "Delta" => "Δ",
        "lambda" => "λ",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(d: &Dom) -> Vec<String> {
        d.nodes.iter().filter(|n| n.tag == "#text" && !n.text.trim().is_empty()).map(|n| n.text.trim().to_string()).collect()
    }

    #[test]
    fn unclosed_paragraphs_and_items() {
        let d = Dom::parse("<body><p>one<p>two<ul><li>a<li>b</ul><div>x</div>");
        let body = d.find(0, "body").unwrap();
        let tags: Vec<&str> = d.elems(body).iter().map(|&k| d.tag(k)).collect();
        assert_eq!(tags, ["p", "p", "ul", "div"]);
        let ul = d.find(0, "ul").unwrap();
        assert_eq!(d.elems(ul).len(), 2);
    }

    #[test]
    fn entities_and_raw_text() {
        let d = Dom::parse("<p title='a &amp; b'>&copy; 2026 &mdash; caf&eacute; &#x1F600; &#8212; &unknown; a&b</p><script>if (a < b) { x = '</p>' }</script><style>p{color:red}</style>");
        let p = d.find(0, "p").unwrap();
        assert_eq!(d.attr(p, "title"), Some("a & b"));
        assert_eq!(texts(&d), ["© 2026 — café 😀 — &unknown; a&b"]);
        let style = d.find(0, "style").unwrap();
        assert_eq!(d.nodes[style].text, "p{color:red}");
    }

    #[test]
    fn stray_end_tags_and_void_elements() {
        let d = Dom::parse("<div><img src=a.jpg alt=\"A\"><br/></span>text</div><input value=x>");
        let div = d.find(0, "div").unwrap();
        assert_eq!(d.elems(div).len(), 2);
        assert_eq!(texts(&d), ["text"]);
        let img = d.find(0, "img").unwrap();
        assert_eq!(d.attr(img, "src"), Some("a.jpg"));
    }
}
