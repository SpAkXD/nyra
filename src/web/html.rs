//! Just enough HTML for the build: the tags of a page with their attributes (for the checks and
//! for rewriting image and asset references), escaping, and edits at byte positions. The pages are
//! made by the web library, so this is a reader of well-formed HTML, not a browser's parser: it
//! knows comments, the raw text of `script`, `style`, `textarea` and `title`, and quoted or bare
//! attribute values.

/// One start or end tag.
#[derive(Debug, Clone)]
pub struct Tag {
    /// Byte range of the whole tag, `<` to `>` included.
    pub start: usize,
    pub end: usize,
    /// Lower case.
    pub name: String,
    /// `</name>`
    pub close: bool,
    /// `<br/>`
    pub self_closing: bool,
    /// In order; names are lower case, values are decoded (`&amp;` is `&`).
    pub attrs: Vec<(String, Option<String>)>,
}

impl Tag {
    pub fn get(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_deref().unwrap_or(""))
    }

    pub fn has(&self, name: &str) -> bool {
        self.attrs.iter().any(|(n, _)| n == name)
    }

    /// Sets an attribute (added at the end when it is not there).
    pub fn set(&mut self, name: &str, value: &str) {
        match self.attrs.iter_mut().find(|(n, _)| n == name) {
            Some((_, v)) => *v = Some(value.to_string()),
            None => self.attrs.push((name.to_string(), Some(value.to_string()))),
        }
    }

    /// The tag written out again.
    pub fn render(&self) -> String {
        let mut s = format!("<{}{}", if self.close { "/" } else { "" }, self.name);
        for (n, v) in &self.attrs {
            s.push(' ');
            s.push_str(n);
            if let Some(v) = v {
                s.push_str("=\"");
                s.push_str(&attr(v));
                s.push('"');
            }
        }
        s.push_str(if self.self_closing { "/>" } else { ">" });
        s
    }
}

/// Every tag of `html`, in order.
pub fn tags(html: &str) -> Vec<Tag> {
    let b = html.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(off) = html[i..].find('<') {
        let at = i + off;
        let rest = &html[at..];
        if rest.starts_with("<!--") {
            i = rest.find("-->").map_or(b.len(), |e| at + e + 3);
            continue;
        }
        if rest.starts_with("<!") || rest.starts_with("<?") {
            i = rest.find('>').map_or(b.len(), |e| at + e + 1);
            continue;
        }
        let close = rest.starts_with("</");
        let name_at = at + if close { 2 } else { 1 };
        if !b.get(name_at).is_some_and(|c| c.is_ascii_alphabetic()) {
            i = at + 1;
            continue;
        }
        let mut j = name_at;
        while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'-' || b[j] == b':') {
            j += 1;
        }
        let name = html[name_at..j].to_ascii_lowercase();
        let mut attrs = Vec::new();
        let mut self_closing = false;
        loop {
            while j < b.len() && b[j].is_ascii_whitespace() {
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
                j += 1;
                if b.get(j) == Some(&b'>') {
                    self_closing = true;
                    j += 1;
                    break;
                }
                continue;
            }
            let n0 = j;
            while j < b.len() && !b[j].is_ascii_whitespace() && !matches!(b[j], b'=' | b'>' | b'/') {
                j += 1;
            }
            let aname = html[n0..j].to_ascii_lowercase();
            let mut k = j;
            while k < b.len() && b[k].is_ascii_whitespace() {
                k += 1;
            }
            if b.get(k) != Some(&b'=') {
                attrs.push((aname, None));
                continue;
            }
            k += 1;
            while k < b.len() && b[k].is_ascii_whitespace() {
                k += 1;
            }
            let value = match b.get(k) {
                Some(&q) if q == b'"' || q == b'\'' => {
                    let e = html[k + 1..].find(q as char).map_or(b.len(), |e| k + 1 + e);
                    let v = &html[k + 1..e];
                    j = (e + 1).min(b.len());
                    v
                }
                _ => {
                    let v0 = k;
                    while k < b.len() && !b[k].is_ascii_whitespace() && b[k] != b'>' {
                        k += 1;
                    }
                    j = k;
                    &html[v0..k]
                }
            };
            attrs.push((aname, Some(decode(value))));
        }
        out.push(Tag { start: at, end: j, name: name.clone(), close, self_closing, attrs });
        i = j;
        // the text of these is not HTML: skip to their end tag
        if !close && matches!(name.as_str(), "script" | "style" | "textarea" | "title") {
            let lower = html[i..].to_ascii_lowercase();
            if let Some(e) = lower.find(&format!("</{name}")) {
                i += e;
            } else {
                i = b.len();
            }
        }
    }
    out
}

/// `&amp;`, `&lt;`, `&gt;`, `&quot;`, `&#39;` and numeric references decoded.
pub fn decode(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(semi) = rest.find(';').filter(|&p| p <= 12) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let ent = &rest[1..semi];
        let ch = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some('\u{a0}'),
            _ if ent.starts_with("#x") || ent.starts_with("#X") => u32::from_str_radix(&ent[2..], 16).ok().and_then(char::from_u32),
            _ if ent.starts_with('#') => ent[1..].parse::<u32>().ok().and_then(char::from_u32),
            _ => None,
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &rest[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Text for an attribute value in double quotes.
pub fn attr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            c => out.push(c),
        }
    }
    out
}

/// Text for the content of an element.
pub fn text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            c => out.push(c),
        }
    }
    out
}

/// Replaces byte ranges of `html` (the ranges do not overlap).
pub fn apply(html: &str, mut edits: Vec<(usize, usize, String)>) -> String {
    edits.sort_by_key(|e| (e.0, e.1));
    let mut out = String::with_capacity(html.len() + 256);
    let mut at = 0;
    for (s, e, text) in edits {
        if s < at {
            continue;
        }
        out.push_str(&html[at..s]);
        out.push_str(&text);
        at = e;
    }
    out.push_str(&html[at..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_tags_attributes_and_skips_raw_text() {
        let h = "<!doctype html><!-- <a href=x> --><p class=\"a b\" id=top data-x='1 &amp; 2' hidden>t</p>\
                 <script>if (a < b) { x = \"<div>\" }</script><IMG SRC=a.png/><textarea><b></textarea>";
        let t = tags(h);
        let names: Vec<String> = t.iter().map(|t| format!("{}{}", if t.close { "/" } else { "" }, t.name)).collect();
        assert_eq!(names, ["p", "/p", "script", "/script", "img", "textarea", "/textarea"]);
        assert_eq!(t[0].get("class"), Some("a b"));
        assert_eq!(t[0].get("id"), Some("top"));
        assert_eq!(t[0].get("data-x"), Some("1 & 2"));
        assert!(t[0].has("hidden"));
        assert_eq!(t[4].get("src"), Some("a.png/"));
        assert_eq!(&h[t[0].start..t[0].end], "<p class=\"a b\" id=top data-x='1 &amp; 2' hidden>");
    }

    #[test]
    fn renders_and_edits() {
        let h = "<img src=\"a.png\" alt=\"x &quot;y&quot;\">";
        let mut t = tags(h).remove(0);
        t.set("width", "10");
        assert_eq!(t.render(), "<img src=\"a.png\" alt=\"x &quot;y&quot;\" width=\"10\">");
        assert_eq!(apply("abcdef", vec![(3, 4, "X".into()), (0, 1, "Y".into())]), "YbcXef");
        assert_eq!(decode("&#x41;&#66;&lt;&unknown;"), "AB<&unknown;");
    }
}
