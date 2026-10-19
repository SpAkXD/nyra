//! A small CSS reader for `nyra clone`: the rules of the page's stylesheets, matched against the
//! page's elements well enough to know each element's background, text color, font and corner
//! radius (the "computed style" of a 1440 px wide desktop window, without layout), and from
//! those the site's theme: page background, text color, brand colors, fonts, radius.
//!
//! It understands selectors with tags, classes, ids, attributes and combinators (ancestors are
//! checked, siblings are not), specificity and order, `!important`, inline styles, custom
//! properties with `var()` and fallbacks, `@media` widths (as a desktop), and the color syntaxes
//! hex, `rgb()`, `hsl()`, `oklch()`, `oklab()` and the common color names.

use std::collections::HashMap;

use super::dom::Dom;

/// One style rule: its selectors and declarations.
pub struct Rule {
    sels: Vec<Sel>,
    decls: Vec<(String, String, bool)>,
    order: usize,
}

/// One complex selector: compounds from left to right (the last one is the element itself).
struct Sel {
    parts: Vec<Compound>,
    spec: u32,
}

#[derive(Default)]
struct Compound {
    tag: Option<String>,
    id: Option<String>,
    classes: Vec<String>,
    attrs: Vec<(String, Option<String>)>,
    /// A state (`:hover`) or pseudo-element (`::before`): the rule is not the element's look.
    state: bool,
}

/// The stylesheets of a page, read.
#[derive(Default)]
pub struct Sheet {
    rules: Vec<Rule>,
    /// `@font-face` families.
    pub faces: Vec<String>,
    /// `@import`ed stylesheets (absolute URLs).
    pub imports: Vec<String>,
}

/// The properties the reader keeps (the others are skipped while matching).
const KEEP: &[&str] = &[
    "background",
    "background-color",
    "background-image",
    "color",
    "font-family",
    "font",
    "border-radius",
    "display",
    "visibility",
    "max-width",
    "text-align",
];

/// Properties that children inherit.
const INHERITED: &[&str] = &["color", "font-family", "visibility", "text-align"];

impl Sheet {
    /// Adds a stylesheet; relative `url(...)`s are made absolute against `base`.
    pub fn add(&mut self, css: &str, base: &str) {
        let css = strip_comments(css);
        let mut p = Parser { s: &css, i: 0, base };
        p.block(self, true);
    }
}

struct Parser<'a> {
    s: &'a str,
    i: usize,
    base: &'a str,
}

impl Parser<'_> {
    /// Reads rules up to the end or to the `}` that closes the current block.
    fn block(&mut self, sheet: &mut Sheet, on: bool) {
        loop {
            self.skip_ws();
            if self.i >= self.s.len() {
                return;
            }
            if self.s[self.i..].starts_with('}') {
                self.i += 1;
                return;
            }
            // the prelude: up to `{` or `;` at depth 0
            let start = self.i;
            let mut depth = 0i32;
            let mut quote: Option<char> = None;
            let mut end = None;
            for (k, c) in self.s[start..].char_indices() {
                if let Some(q) = quote {
                    if c == q {
                        quote = None;
                    }
                    continue;
                }
                match c {
                    '"' | '\'' => quote = Some(c),
                    '(' | '[' => depth += 1,
                    ')' | ']' => depth -= 1,
                    '{' | ';' | '}' if depth <= 0 => {
                        end = Some((start + k, c));
                        break;
                    }
                    _ => {}
                }
            }
            let Some((at, c)) = end else {
                self.i = self.s.len();
                return;
            };
            let prelude = self.s[start..at].trim().to_string();
            self.i = at + 1;
            if c == '}' {
                // a stray declaration before the end of a block
                return;
            }
            if c == ';' {
                if let Some(rest) = prelude.strip_prefix("@import") {
                    if let Some(u) = first_url(rest).or_else(|| quoted(rest)) {
                        sheet.imports.push(resolve(self.base, &u));
                    }
                }
                continue;
            }
            let lower = prelude.to_ascii_lowercase();
            if let Some(query) = lower.strip_prefix("@media") {
                let ok = on && media_matches(query);
                self.block(sheet, ok);
            } else if lower.starts_with("@supports")
                || lower.starts_with("@layer")
                || lower.starts_with("@container")
                || lower.starts_with("@scope")
            {
                self.block(sheet, on);
            } else if lower.starts_with("@font-face") {
                let body = self.body();
                for (k, v, _) in decls(&body, self.base) {
                    if k == "font-family" {
                        let name = v.trim().trim_matches(|c| c == '"' || c == '\'').to_string();
                        if !name.is_empty() && !sheet.faces.contains(&name) {
                            sheet.faces.push(name);
                        }
                    }
                }
            } else if lower.starts_with('@') {
                self.skip_block();
            } else {
                let body = self.body();
                if !on {
                    continue;
                }
                let decls: Vec<(String, String, bool)> = decls(&body, self.base)
                    .into_iter()
                    .filter(|(k, _, _)| k.starts_with("--") || KEEP.contains(&k.as_str()))
                    .collect();
                if decls.is_empty() {
                    continue;
                }
                let sels: Vec<Sel> = split_top(&prelude, ',').iter().filter_map(|s| parse_sel(s)).collect();
                if !sels.is_empty() {
                    let order = sheet.rules.len();
                    sheet.rules.push(Rule { sels, decls, order });
                }
            }
        }
    }

    /// The text of a block whose `{` was just read, up to its matching `}` (nested blocks of CSS
    /// nesting are left out).
    fn body(&mut self) -> String {
        let mut depth = 1;
        let mut out = String::new();
        let mut quote: Option<char> = None;
        for (k, c) in self.s[self.i..].char_indices() {
            if let Some(q) = quote {
                if c == q {
                    quote = None;
                }
                if depth == 1 {
                    out.push(c);
                }
                continue;
            }
            match c {
                '"' | '\'' => quote = Some(c),
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        self.i += k + 1;
                        return out;
                    }
                    continue;
                }
                _ => {}
            }
            if depth == 1 {
                out.push(c);
            }
        }
        self.i = self.s.len();
        out
    }

    fn skip_block(&mut self) {
        let _ = self.body();
    }

    fn skip_ws(&mut self) {
        while self.i < self.s.len() && self.s.as_bytes()[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }
}

/// True if a media query applies to a 1440 x 900 screen in light mode.
fn media_matches(q: &str) -> bool {
    if q.contains("print") && !q.contains("screen") && !q.contains("all") {
        return false;
    }
    if q.contains("prefers-color-scheme: dark") || q.contains("prefers-color-scheme:dark") || q.contains("prefers-reduced-motion") {
        return false;
    }
    if q.contains("hover: none") || q.contains("pointer: coarse") || q.contains("orientation: portrait") {
        return false;
    }
    for (key, min) in [("min-width", true), ("max-width", false)] {
        let mut rest = q;
        while let Some(at) = rest.find(key) {
            rest = &rest[at + key.len()..];
            let v = rest.trim_start_matches([':', ' ']);
            if let Some(px) = length_px(v) {
                if (min && px > 1440.0) || (!min && px < 1440.0) {
                    return false;
                }
            }
        }
    }
    // the range syntax: (width >= 768px), (width < 640px)
    for (op, min) in
        [("width >= ", true), ("width>=", true), ("width > ", true), ("width < ", false), ("width<", false), ("width <= ", false)]
    {
        if let Some(at) = q.find(op) {
            if let Some(px) = length_px(&q[at + op.len()..]) {
                if (min && px > 1440.0) || (!min && px < 1440.0) {
                    return false;
                }
            }
        }
    }
    true
}

/// `768px`, `48rem`, `40em` at the start of `s`, in px.
fn length_px(s: &str) -> Option<f64> {
    let num: String = s.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    let n: f64 = num.parse().ok()?;
    let unit = &s[num.len()..];
    Some(if unit.starts_with("em") || unit.starts_with("rem") { n * 16.0 } else { n })
}

fn strip_comments(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(at) = rest.find("/*") {
        out.push_str(&rest[..at]);
        rest = match rest[at + 2..].find("*/") {
            Some(e) => &rest[at + 2 + e + 2..],
            None => "",
        };
    }
    out.push_str(rest);
    out
}

/// Splits at `sep` outside parentheses, brackets and quotes.
fn split_top(s: &str, sep: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    let mut cur = String::new();
    for c in s.chars() {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            cur.push(c);
            continue;
        }
        match c {
            '"' | '\'' => quote = Some(c),
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            _ => {}
        }
        if c == sep && depth == 0 {
            out.push(std::mem::take(&mut cur));
        } else {
            cur.push(c);
        }
    }
    out.push(cur);
    out
}

/// The declarations of a block: (property, value, important), with `url()`s absolute.
pub fn decls(body: &str, base: &str) -> Vec<(String, String, bool)> {
    let mut out = Vec::new();
    for d in split_top(body, ';') {
        let Some(colon) = d.find(':') else { continue };
        let k = d[..colon].trim();
        let k = if k.starts_with("--") { k.to_string() } else { k.to_ascii_lowercase() };
        let mut v = d[colon + 1..].trim().to_string();
        let mut important = false;
        if let Some(at) = v.to_ascii_lowercase().rfind("!important") {
            v.truncate(at);
            v = v.trim().to_string();
            important = true;
        }
        if k.is_empty() || v.is_empty() {
            continue;
        }
        if v.contains("url(") {
            v = absolute_urls(&v, base);
        }
        out.push((k, v, important));
    }
    out
}

fn absolute_urls(v: &str, base: &str) -> String {
    let mut out = String::new();
    let mut rest = v;
    while let Some(at) = rest.find("url(") {
        out.push_str(&rest[..at + 4]);
        rest = &rest[at + 4..];
        let end = rest.find(')').unwrap_or(rest.len());
        let raw = rest[..end].trim().trim_matches(|c| c == '"' || c == '\'');
        let abs = if raw.starts_with("data:") { raw.to_string() } else { resolve(base, raw) };
        out.push('"');
        out.push_str(&abs);
        out.push('"');
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// The first `url(...)` of a value, without quotes.
pub fn first_url(v: &str) -> Option<String> {
    let at = v.find("url(")?;
    let rest = &v[at + 4..];
    let end = rest.find(')')?;
    let u = rest[..end].trim().trim_matches(|c| c == '"' || c == '\'').to_string();
    (!u.is_empty()).then_some(u)
}

fn quoted(v: &str) -> Option<String> {
    let v = v.trim();
    let q = v.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let rest = &v[1..];
    rest.find(q).map(|e| rest[..e].to_string())
}

/// `href` made absolute against `base` (an absolute URL, or "" to leave it as it is).
pub fn resolve(base: &str, href: &str) -> String {
    let h = href.trim();
    if h.is_empty() || base.is_empty() {
        return h.to_string();
    }
    if has_scheme(h) {
        return h.to_string();
    }
    let (scheme, rest) = match base.find("://") {
        Some(at) => (&base[..at], &base[at + 3..]),
        None => return h.to_string(),
    };
    let host_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let origin = format!("{scheme}://{}", &rest[..host_end]);
    let path = &rest[host_end..];
    let path = &path[..path.find(['?', '#']).unwrap_or(path.len())];
    if let Some(r) = h.strip_prefix("//") {
        return format!("{scheme}://{r}");
    }
    if h.starts_with('/') {
        return format!("{origin}{}", normalize(h));
    }
    if h.starts_with('?') {
        return format!("{origin}{path}{h}");
    }
    if h.starts_with('#') {
        return format!("{origin}{path}{h}");
    }
    let dir = match path.rfind('/') {
        Some(at) => &path[..=at],
        None => "/",
    };
    format!("{origin}{}", normalize(&format!("{dir}{h}")))
}

/// `/a/./b/../c` -> `/a/c` (the query and fragment are left as they are).
fn normalize(p: &str) -> String {
    let cut = p.find(['?', '#']).unwrap_or(p.len());
    let (path, tail) = p.split_at(cut);
    let mut out: Vec<&str> = Vec::new();
    let segs: Vec<&str> = path.split('/').collect();
    for (k, seg) in segs.iter().enumerate() {
        match *seg {
            "." => {
                if k == segs.len() - 1 {
                    out.push("");
                }
            }
            ".." => {
                if out.len() > 1 {
                    out.pop();
                }
                if k == segs.len() - 1 {
                    out.push("");
                }
            }
            s => out.push(s),
        }
    }
    let mut joined = out.join("/");
    if !joined.starts_with('/') {
        joined.insert(0, '/');
    }
    joined + tail
}

pub fn has_scheme(u: &str) -> bool {
    let mut chars = u.char_indices();
    match chars.next() {
        Some((_, c)) if c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    for (_, c) in chars {
        if c == ':' {
            return true;
        }
        if !(c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.') {
            return false;
        }
    }
    false
}

// ---- selectors -------------------------------------------------------------------------------------

fn parse_sel(s: &str) -> Option<Sel> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    // split into compounds at combinators outside brackets and parentheses
    let mut parts = Vec::new();
    let mut cur = String::new();
    let mut depth = 0i32;
    let mut esc = false;
    for c in s.chars() {
        if esc {
            cur.push(c);
            esc = false;
            continue;
        }
        match c {
            '\\' => {
                esc = true;
                cur.push(c);
                continue;
            }
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            _ => {}
        }
        if depth == 0 && (c.is_whitespace() || c == '>' || c == '+' || c == '~') {
            if !cur.is_empty() {
                parts.push(std::mem::take(&mut cur));
            }
        } else {
            cur.push(c);
        }
    }
    if !cur.is_empty() {
        parts.push(cur);
    }
    let mut spec = 0;
    let mut comps = Vec::new();
    for p in &parts {
        let c = parse_compound(p)?;
        spec += c.id.is_some() as u32 * 10000 + (c.classes.len() + c.attrs.len()) as u32 * 100 + c.tag.is_some() as u32;
        comps.push(c);
    }
    if comps.last().is_some_and(|c| c.state) {
        return None;
    }
    Some(Sel { parts: comps, spec })
}

/// One compound selector (`a.btn#x[href]:hover`); `None` for one this reader cannot use.
fn parse_compound(p: &str) -> Option<Compound> {
    let mut c = Compound::default();
    let chars: Vec<char> = p.chars().collect();
    let mut i = 0;
    let ident = |i: &mut usize| -> String {
        let mut out = String::new();
        while *i < chars.len() {
            let ch = chars[*i];
            if ch == '\\' && *i + 1 < chars.len() {
                // an escape: one character, or up to six hex digits and a space
                let mut hex = String::new();
                let mut j = *i + 1;
                while j < chars.len() && hex.len() < 6 && chars[j].is_ascii_hexdigit() {
                    hex.push(chars[j]);
                    j += 1;
                }
                if !hex.is_empty() {
                    if let Some(ch) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                        out.push(ch);
                    }
                    if j < chars.len() && chars[j] == ' ' {
                        j += 1;
                    }
                    *i = j;
                } else {
                    out.push(chars[*i + 1]);
                    *i += 2;
                }
                continue;
            }
            if ch == '.' || ch == '#' || ch == '[' || ch == ':' || ch == '(' || ch == ')' {
                break;
            }
            out.push(ch);
            *i += 1;
        }
        out
    };
    while i < chars.len() {
        match chars[i] {
            '.' => {
                i += 1;
                c.classes.push(ident(&mut i));
            }
            '#' => {
                i += 1;
                c.id = Some(ident(&mut i));
            }
            '[' => {
                let end = chars[i..].iter().position(|&x| x == ']').map_or(chars.len(), |e| i + e);
                let inner: String = chars[i + 1..end].iter().collect();
                let (k, v) = match inner.find('=') {
                    Some(at) => {
                        let key = inner[..at].trim_end_matches(['~', '|', '^', '$', '*']).trim().to_ascii_lowercase();
                        let op = inner[..at].chars().last().unwrap_or('=');
                        let val = inner[at + 1..]
                            .trim()
                            .trim_end_matches([' ', 'i', 's'])
                            .trim()
                            .trim_matches(|q| q == '"' || q == '\'')
                            .to_string();
                        // only exact values are checked; `^=` and friends match any value
                        (key, if op == '=' || op == '~' { Some(val) } else { None })
                    }
                    None => (inner.trim().to_ascii_lowercase(), None),
                };
                c.attrs.push((k, v));
                i = end + 1;
            }
            ':' => {
                let double = chars.get(i + 1) == Some(&':');
                i += if double { 2 } else { 1 };
                let name = ident(&mut i).to_ascii_lowercase();
                let mut arg = String::new();
                if i < chars.len() && chars[i] == '(' {
                    let mut depth = 0;
                    while i < chars.len() {
                        let ch = chars[i];
                        if ch == '(' {
                            depth += 1;
                        } else if ch == ')' {
                            depth -= 1;
                            if depth == 0 {
                                i += 1;
                                break;
                            }
                        }
                        arg.push(ch);
                        i += 1;
                    }
                }
                match name.as_str() {
                    "root" => c.tag = Some("html".into()),
                    // structural pseudo-classes and :not() are taken as matching
                    "not" | "first-child" | "last-child" | "nth-child" | "nth-of-type" | "first-of-type" | "last-of-type"
                    | "only-child" | "where" | "is" | "lang" | "dir" | "link" | "any-link" | "defined" => {}
                    _ => c.state = true,
                }
            }
            '*' => i += 1,
            _ => {
                let t = ident(&mut i).to_ascii_lowercase();
                if t.is_empty() {
                    return None;
                }
                c.tag = Some(t);
            }
        }
    }
    Some(c)
}

fn compound_matches(dom: &Dom, id: usize, c: &Compound) -> bool {
    if let Some(t) = &c.tag {
        if dom.tag(id) != t {
            return false;
        }
    }
    if let Some(want) = &c.id {
        if dom.attr(id, "id") != Some(want.as_str()) {
            return false;
        }
    }
    if !c.classes.is_empty() {
        let have = dom.classes(id);
        if !c.classes.iter().all(|k| have.contains(&k.as_str())) {
            return false;
        }
    }
    for (k, v) in &c.attrs {
        match (dom.attr(id, k), v) {
            (None, _) => return false,
            (Some(have), Some(want)) if k != "class" && have != want && !have.split_whitespace().any(|w| w == want) => return false,
            _ => {}
        }
    }
    true
}

fn sel_matches(dom: &Dom, id: usize, s: &Sel) -> bool {
    let n = s.parts.len();
    if !compound_matches(dom, id, &s.parts[n - 1]) {
        return false;
    }
    // the ancestors' compounds, from right to left, each on some ancestor further up
    let mut at = dom.nodes[id].parent;
    for c in s.parts[..n - 1].iter().rev() {
        if c.state {
            // `.group:hover .x`: a state of an ancestor: not the element's look at rest
            return false;
        }
        loop {
            if at == 0 {
                return false;
            }
            if compound_matches(dom, at, c) {
                at = dom.nodes[at].parent;
                break;
            }
            at = dom.nodes[at].parent;
        }
    }
    true
}

// ---- computed styles ----------------------------------------------------------------------------

/// The declared values of each element (by node id) after the cascade, for the properties in `KEEP`
/// and the custom properties.
pub struct Styles {
    own: Vec<HashMap<String, String>>,
}

impl Styles {
    /// The styles of every element; `base` is the page's address (for `url()` in `style=""`).
    pub fn compute(dom: &Dom, sheet: &Sheet, base: &str) -> Styles {
        // rules by the key of their last compound: an id, a class, a tag, or none
        let mut by_id: HashMap<&str, Vec<(usize, usize)>> = HashMap::new();
        let mut by_class: HashMap<&str, Vec<(usize, usize)>> = HashMap::new();
        let mut by_tag: HashMap<&str, Vec<(usize, usize)>> = HashMap::new();
        let mut any: Vec<(usize, usize)> = Vec::new();
        for (r, rule) in sheet.rules.iter().enumerate() {
            for (s, sel) in rule.sels.iter().enumerate() {
                let last = sel.parts.last().unwrap();
                if let Some(i) = &last.id {
                    by_id.entry(i.as_str()).or_default().push((r, s));
                } else if let Some(c) = last.classes.first() {
                    by_class.entry(c.as_str()).or_default().push((r, s));
                } else if let Some(t) = &last.tag {
                    by_tag.entry(t.as_str()).or_default().push((r, s));
                } else {
                    any.push((r, s));
                }
            }
        }
        let mut own = Vec::with_capacity(dom.nodes.len());
        for id in 0..dom.nodes.len() {
            let mut map: HashMap<String, String> = HashMap::new();
            if !dom.is_elem(id) {
                own.push(map);
                continue;
            }
            let mut cands: Vec<(usize, usize)> = Vec::new();
            if let Some(v) = dom.attr(id, "id").and_then(|i| by_id.get(i)) {
                cands.extend(v);
            }
            for c in dom.classes(id) {
                if let Some(v) = by_class.get(c) {
                    cands.extend(v);
                }
            }
            if let Some(v) = by_tag.get(dom.tag(id)) {
                cands.extend(v);
            }
            cands.extend(&any);
            // (important, specificity, order) of the winner of each property
            let mut rank: HashMap<String, (bool, u32, usize)> = HashMap::new();
            for (r, s) in cands {
                let rule = &sheet.rules[r];
                let sel = &rule.sels[s];
                if !sel_matches(dom, id, sel) {
                    continue;
                }
                for (k, v, imp) in &rule.decls {
                    let key = (*imp, sel.spec, rule.order);
                    let wins = rank.get(k).is_none_or(|old| key >= *old);
                    if wins {
                        rank.insert(k.clone(), key);
                        map.insert(k.clone(), v.clone());
                    }
                }
            }
            if let Some(style) = dom.attr(id, "style") {
                for (k, v, imp) in decls(style, base) {
                    if !(k.starts_with("--") || KEEP.contains(&k.as_str())) {
                        continue;
                    }
                    let key = (imp, 1_000_000, usize::MAX);
                    if rank.get(&k).is_none_or(|old| key >= *old) {
                        rank.insert(k.clone(), key);
                        map.insert(k, v);
                    }
                }
            }
            own.push(map);
        }
        Styles { own }
    }

    /// The value of a property for an element, with `var()` resolved; inherited properties and
    /// custom properties come from the nearest ancestor that sets them.
    pub fn get(&self, dom: &Dom, id: usize, prop: &str) -> Option<String> {
        let inherit = INHERITED.contains(&prop) || prop.starts_with("--");
        let mut n = id;
        loop {
            if let Some(v) = self.own.get(n).and_then(|m| m.get(prop)) {
                let v = v.clone();
                if v == "inherit" && n != 0 {
                    n = dom.nodes[n].parent;
                    continue;
                }
                return Some(self.vars(dom, id, &v, 0));
            }
            if !inherit || n == 0 {
                return None;
            }
            n = dom.nodes[n].parent;
        }
    }

    fn vars(&self, dom: &Dom, id: usize, v: &str, depth: u32) -> String {
        if !v.contains("var(") || depth > 6 {
            return v.to_string();
        }
        let mut out = String::new();
        let mut rest = v;
        while let Some(at) = rest.find("var(") {
            out.push_str(&rest[..at]);
            let inner_start = at + 4;
            let mut d = 1;
            let mut end = rest.len();
            for (k, c) in rest[inner_start..].char_indices() {
                match c {
                    '(' => d += 1,
                    ')' => {
                        d -= 1;
                        if d == 0 {
                            end = inner_start + k;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            let inner = &rest[inner_start..end];
            let (name, fallback) = match inner.find(',') {
                Some(c) => (inner[..c].trim(), Some(inner[c + 1..].trim())),
                None => (inner.trim(), None),
            };
            let value = self.get_raw(dom, id, name).or_else(|| fallback.map(str::to_string)).unwrap_or_default();
            out.push_str(&self.vars(dom, id, &value, depth + 1));
            rest = if end < rest.len() { &rest[end + 1..] } else { "" };
        }
        out.push_str(rest);
        out
    }

    fn get_raw(&self, dom: &Dom, id: usize, name: &str) -> Option<String> {
        let mut n = id;
        loop {
            if let Some(v) = self.own.get(n).and_then(|m| m.get(name)) {
                return Some(v.clone());
            }
            if n == 0 {
                return None;
            }
            n = dom.nodes[n].parent;
        }
    }

    /// The element's own background color (not inherited), if it sets an opaque enough one.
    pub fn bg(&self, dom: &Dom, id: usize) -> Option<Rgb> {
        let v = self.get(dom, id, "background-color").or_else(|| self.get(dom, id, "background"))?;
        let c = parse_color_in(&v)?;
        (c.a >= 0.5).then_some(c)
    }

    /// The color of the element's background or of its gradient, when it is a brand color.
    pub fn tint(&self, dom: &Dom, id: usize) -> Option<Rgb> {
        let from_image = self.get(dom, id, "background-image").filter(|v| v.contains("gradient(")).and_then(|v| parse_color_in(&v));
        self.bg(dom, id).or(from_image).filter(Rgb::colorful)
    }

    /// The element's background image URL (absolute), if it has one.
    pub fn bg_image(&self, dom: &Dom, id: usize) -> Option<String> {
        let v = self.get(dom, id, "background-image").or_else(|| self.get(dom, id, "background"))?;
        first_url(&v).filter(|u| !u.starts_with("data:"))
    }

    pub fn color(&self, dom: &Dom, id: usize) -> Option<Rgb> {
        parse_color_in(&self.get(dom, id, "color")?)
    }

    /// The first family of the element's font that is not a generic or system name.
    pub fn font(&self, dom: &Dom, id: usize) -> Option<String> {
        let v = self.get(dom, id, "font-family").or_else(|| self.get(dom, id, "font").and_then(|f| font_shorthand_family(&f)))?;
        first_family(&v)
    }

    /// True if the element is not displayed (`display: none`, `visibility: hidden`).
    pub fn hidden(&self, dom: &Dom, id: usize) -> bool {
        self.own.get(id).and_then(|m| m.get("display")).is_some_and(|d| d.trim() == "none")
            || self.get(dom, id, "visibility").is_some_and(|v| v.trim() == "hidden" || v.trim() == "collapse")
    }

    /// The element's `max-width` in px, when it is given in px, rem, em or ch.
    pub fn max_width(&self, dom: &Dom, id: usize) -> Option<f64> {
        let v = self.get(dom, id, "max-width")?;
        let v = v.trim();
        let num: String = v.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
        let n: f64 = num.parse().ok()?;
        match &v[num.len()..] {
            "px" => Some(n),
            "rem" | "em" => Some(n * 16.0),
            "ch" => Some(n * 8.0),
            _ => None,
        }
    }

    /// The corner radius in px, if the element sets one in px or rem.
    pub fn radius(&self, dom: &Dom, id: usize) -> Option<f64> {
        let v = self.get(dom, id, "border-radius")?;
        let first = v.split_whitespace().next()?;
        let num: String = first.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
        let n: f64 = num.parse().ok()?;
        let unit = &first[num.len()..];
        match unit {
            "px" => Some(n),
            "rem" | "em" => Some(n * 16.0),
            _ if n == 0.0 => Some(0.0),
            _ => None,
        }
    }
}

fn font_shorthand_family(f: &str) -> Option<String> {
    // `400 1rem/1.6 "Inter", sans-serif`: the family list follows the size
    let at = f.find(['"', '\'']).or_else(|| {
        let words: Vec<&str> = f.split_whitespace().collect();
        let k = words
            .iter()
            .position(|w| w.chars().next().is_some_and(|c| c.is_ascii_digit()) && w.contains(|c: char| c.is_alphabetic()))?;
        let prefix: usize = words[..=k].iter().map(|w| w.len() + 1).sum();
        Some(prefix.min(f.len()))
    })?;
    Some(f[at..].to_string())
}

/// System and generic font names: a site that uses only these has no font to load.
const SYSTEM_FONTS: &[&str] = &[
    "system-ui",
    "-apple-system",
    "blinkmacsystemfont",
    "segoe ui",
    "helvetica",
    "helvetica neue",
    "arial",
    "sans-serif",
    "serif",
    "monospace",
    "cursive",
    "fantasy",
    "ui-sans-serif",
    "ui-serif",
    "ui-monospace",
    "ui-rounded",
    "inherit",
    "initial",
    "unset",
    "georgia",
    "times",
    "times new roman",
    "verdana",
    "tahoma",
    "trebuchet ms",
    "courier",
    "courier new",
    "apple color emoji",
    "segoe ui emoji",
    "segoe ui symbol",
    "noto color emoji",
    "emoji",
    "math",
    "consolas",
    "menlo",
    "monaco",
    "sfmono-regular",
    "liberation sans",
    "liberation mono",
    "oxygen",
    "ubuntu",
    "cantarell",
    "fira sans",
    "droid sans",
    "lucida grande",
    "lucida sans unicode",
    "sf pro",
    "sf pro text",
    "sf pro display",
    "san francisco",
    "avenir",
    "avenir next",
    "optima",
    "gill sans",
    "calibri",
    "cambria",
    "candara",
    "corbel",
    "franklin gothic medium",
    "palatino",
    "palatino linotype",
    "book antiqua",
    "garamond",
    "baskerville",
    "hoefler text",
    "charter",
    "iowan old style",
    "noto sans",
    "dejavu sans",
    "sans",
];

pub fn first_family(v: &str) -> Option<String> {
    for f in split_top(v, ',') {
        let name = f.trim().trim_matches(|c| c == '"' || c == '\'').trim().to_string();
        if name.is_empty() || name.starts_with("var(") {
            continue;
        }
        let lower = name.to_ascii_lowercase();
        if SYSTEM_FONTS.contains(&lower.as_str())
            || lower.starts_with("segoe ")
            || lower.starts_with("sf ")
            || lower.starts_with(".sf")
            || lower.starts_with("helvetica")
        {
            continue;
        }
        // "Inter var", "Inter Variable" -> "Inter"
        let clean = name.trim_end_matches(" var").trim_end_matches(" Variable").trim_end_matches(" VF").to_string();
        // a build's font names ("__Inter_d65c78", "__Inter_Fallback_d65c78")
        if clean.starts_with("__") {
            let core = clean.trim_start_matches('_');
            let first = core.split('_').next().unwrap_or("");
            if !first.is_empty() && !clean.contains("Fallback") {
                return Some(first.replace('-', " "));
            }
            continue;
        }
        return Some(clean);
    }
    None
}

// ---- colors ---------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgb {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
}

impl Rgb {
    pub fn hex(&self) -> String {
        let c = |v: f64| v.round().clamp(0.0, 255.0) as u8;
        format!("#{:02x}{:02x}{:02x}", c(self.r), c(self.g), c(self.b))
    }

    /// Relative luminance (WCAG).
    pub fn luma(&self) -> f64 {
        let ch = |v: f64| {
            let s = v / 255.0;
            if s <= 0.03928 {
                s / 12.92
            } else {
                ((s + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * ch(self.r) + 0.7152 * ch(self.g) + 0.0722 * ch(self.b)
    }

    /// (hue in degrees, saturation 0..1, lightness 0..1)
    pub fn hsl(&self) -> (f64, f64, f64) {
        let (r, g, b) = (self.r / 255.0, self.g / 255.0, self.b / 255.0);
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let l = (max + min) / 2.0;
        if max - min < 1e-9 {
            return (0.0, 0.0, l);
        }
        let d = max - min;
        let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
        let h = if max == r {
            (g - b) / d + if g < b { 6.0 } else { 0.0 }
        } else if max == g {
            (b - r) / d + 2.0
        } else {
            (r - g) / d + 4.0
        };
        (h * 60.0, s, l)
    }

    /// A brand color: clearly colored, neither near white nor near black.
    pub fn colorful(&self) -> bool {
        let (_, s, l) = self.hsl();
        s > 0.28 && l > 0.12 && l < 0.88
    }

    pub fn distance(&self, o: &Rgb) -> f64 {
        ((self.r - o.r).powi(2) + (self.g - o.g).powi(2) + (self.b - o.b).powi(2)).sqrt()
    }
}

pub fn contrast(a: &Rgb, b: &Rgb) -> f64 {
    let (x, y) = (a.luma(), b.luma());
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// The first color in a value (`#fff`, `rgb(...)`, `oklch(...)`, `red`, ... inside a shorthand).
pub fn parse_color_in(v: &str) -> Option<Rgb> {
    let v = v.trim();
    if let Some(c) = parse_color(v) {
        return Some(c);
    }
    // a gradient or a shorthand: the first part that is a color
    for word in split_top(&v.replace(',', " , "), ' ') {
        let w = word.trim().trim_end_matches(',');
        if w.is_empty() || w.starts_with("url(") {
            continue;
        }
        if w.contains("gradient(") {
            let inner_start = w.find('(').unwrap_or(0) + 1;
            let inner = &w[inner_start..w.len().saturating_sub(1)];
            for part in split_top(inner, ',') {
                let first = part.split_whitespace().next().unwrap_or("");
                let candidate = if first.contains('(') {
                    part.trim().rsplit_once(')').map(|(a, _)| format!("{a})")).unwrap_or_default()
                } else {
                    first.to_string()
                };
                if let Some(c) = parse_color(&candidate) {
                    return Some(c);
                }
            }
            continue;
        }
        if let Some(c) = parse_color(w) {
            return Some(c);
        }
    }
    None
}

pub fn parse_color(v: &str) -> Option<Rgb> {
    let v = v.trim().to_ascii_lowercase();
    if let Some(h) = v.strip_prefix('#') {
        let h: String = h.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
        let n = |s: &str| u8::from_str_radix(s, 16).ok().map(f64::from);
        let d = |c: char| n(&format!("{c}{c}"));
        let cs: Vec<char> = h.chars().collect();
        return match cs.len() {
            3 | 4 => Some(Rgb { r: d(cs[0])?, g: d(cs[1])?, b: d(cs[2])?, a: if cs.len() == 4 { d(cs[3])? / 255.0 } else { 1.0 } }),
            6 | 8 => Some(Rgb {
                r: n(&h[0..2])?,
                g: n(&h[2..4])?,
                b: n(&h[4..6])?,
                a: if cs.len() == 8 { n(&h[6..8])? / 255.0 } else { 1.0 },
            }),
            _ => None,
        };
    }
    let open = v.find('(');
    if let Some(open) = open {
        let name = v[..open].trim();
        let inner = v[open + 1..].trim_end_matches(')');
        if name == "light-dark" {
            // the light value, as a page in light mode shows it
            return split_top(inner, ',').first().and_then(|l| parse_color(l));
        }
        let parts: Vec<&str> = inner.split(|c: char| c == ',' || c == '/' || c.is_whitespace()).filter(|p| !p.is_empty()).collect();
        let num = |s: &str, scale: f64| -> Option<f64> {
            if let Some(p) = s.strip_suffix('%') {
                p.parse::<f64>().ok().map(|x| x / 100.0 * scale)
            } else if s == "none" {
                Some(0.0)
            } else {
                s.trim_end_matches("deg").parse::<f64>().ok()
            }
        };
        let alpha = |k: usize| -> f64 {
            parts
                .get(k)
                .and_then(
                    |a| if let Some(p) = a.strip_suffix('%') { p.parse::<f64>().ok().map(|x| x / 100.0) } else { a.parse().ok() },
                )
                .unwrap_or(1.0)
        };
        return match name {
            "rgb" | "rgba" if parts.len() >= 3 => {
                Some(Rgb { r: num(parts[0], 255.0)?, g: num(parts[1], 255.0)?, b: num(parts[2], 255.0)?, a: alpha(3) })
            }
            "hsl" | "hsla" if parts.len() >= 3 => {
                let (h, s, l) = (num(parts[0], 360.0)?, num(parts[1], 1.0)?, num(parts[2], 1.0)?);
                let s = if s > 1.0 { s / 100.0 } else { s };
                let l = if l > 1.0 { l / 100.0 } else { l };
                let (r, g, b) = hsl_to_rgb(h, s, l);
                Some(Rgb { r, g, b, a: alpha(3) })
            }
            "oklch" if parts.len() >= 3 => {
                let l = num(parts[0], 1.0)?;
                let l = if l > 1.0 { l / 100.0 } else { l };
                let c = num(parts[1], 0.4)?;
                let h = num(parts[2], 360.0)?.to_radians();
                let (r, g, b) = oklab_to_rgb(l, c * h.cos(), c * h.sin());
                Some(Rgb { r, g, b, a: alpha(3) })
            }
            "oklab" if parts.len() >= 3 => {
                let l = num(parts[0], 1.0)?;
                let l = if l > 1.0 { l / 100.0 } else { l };
                let (r, g, b) = oklab_to_rgb(l, num(parts[1], 0.4)?, num(parts[2], 0.4)?);
                Some(Rgb { r, g, b, a: alpha(3) })
            }
            _ => None,
        };
    }
    let named = match v.as_str() {
        "white" => (255, 255, 255),
        "black" => (0, 0, 0),
        "red" => (255, 0, 0),
        "green" => (0, 128, 0),
        "blue" => (0, 0, 255),
        "navy" => (0, 0, 128),
        "teal" => (0, 128, 128),
        "orange" => (255, 165, 0),
        "purple" => (128, 0, 128),
        "gray" | "grey" => (128, 128, 128),
        "silver" => (192, 192, 192),
        "maroon" => (128, 0, 0),
        "olive" => (128, 128, 0),
        "darkgreen" => (0, 100, 0),
        "darkblue" => (0, 0, 139),
        "darkred" => (139, 0, 0),
        "gold" => (255, 215, 0),
        "crimson" => (220, 20, 60),
        "tomato" => (255, 99, 71),
        "coral" => (255, 127, 80),
        "whitesmoke" => (245, 245, 245),
        "ivory" => (255, 255, 240),
        "beige" => (245, 245, 220),
        "linen" => (250, 240, 230),
        "snow" => (255, 250, 250),
        "aliceblue" => (240, 248, 255),
        "ghostwhite" => (248, 248, 255),
        "lightgray" | "lightgrey" => (211, 211, 211),
        "darkgray" | "darkgrey" => (169, 169, 169),
        "dimgray" | "dimgrey" => (105, 105, 105),
        "royalblue" => (65, 105, 225),
        "steelblue" => (70, 130, 180),
        "dodgerblue" => (30, 144, 255),
        "rebeccapurple" => (102, 51, 153),
        "seagreen" => (46, 139, 87),
        "forestgreen" => (34, 139, 34),
        "firebrick" => (178, 34, 34),
        "chocolate" => (210, 105, 30),
        "saddlebrown" => (139, 69, 19),
        "sienna" => (160, 82, 45),
        "goldenrod" => (218, 165, 32),
        "darkorange" => (255, 140, 0),
        "slategray" | "slategrey" => (112, 128, 144),
        _ => return None,
    };
    Some(Rgb { r: named.0 as f64, g: named.1 as f64, b: named.2 as f64, a: 1.0 })
}

fn hsl_to_rgb(h: f64, s: f64, l: f64) -> (f64, f64, f64) {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = (h.rem_euclid(360.0)) / 60.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    ((r + m) * 255.0, (g + m) * 255.0, (b + m) * 255.0)
}

fn oklab_to_rgb(l: f64, a: f64, b: f64) -> (f64, f64, f64) {
    let l_ = l + 0.396_337_777_4 * a + 0.215_803_757_3 * b;
    let m_ = l - 0.105_561_345_8 * a - 0.063_854_172_8 * b;
    let s_ = l - 0.089_484_177_5 * a - 1.291_485_548 * b;
    let (l3, m3, s3) = (l_ * l_ * l_, m_ * m_ * m_, s_ * s_ * s_);
    let r = 4.076_741_662_1 * l3 - 3.307_711_591_3 * m3 + 0.230_969_929_2 * s3;
    let g = -1.268_438_004_6 * l3 + 2.609_757_401_1 * m3 - 0.341_319_396_5 * s3;
    let bl = -0.004_196_086_3 * l3 - 0.703_418_614_7 * m3 + 1.707_614_701 * s3;
    let gamma = |x: f64| {
        let x = x.clamp(0.0, 1.0);
        let v = if x <= 0.003_130_8 { 12.92 * x } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 };
        v * 255.0
    };
    (gamma(r), gamma(g), gamma(bl))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors() {
        assert_eq!(parse_color("#FFF").unwrap().hex(), "#ffffff");
        assert_eq!(parse_color("rgb(37 99 235 / 1)").unwrap().hex(), "#2563eb");
        assert_eq!(parse_color("rgba(0,0,0,.5)").unwrap().a, 0.5);
        assert_eq!(parse_color("hsl(0, 100%, 50%)").unwrap().hex(), "#ff0000");
        // Tailwind v4 blue-600
        let c = parse_color("oklch(54.6% 0.245 262.881)").unwrap();
        assert!(c.distance(&parse_color("#155dfc").unwrap()) < 6.0, "{}", c.hex());
        assert_eq!(parse_color_in("linear-gradient(90deg, #1E3D12 50%, #5C1800 50%)").unwrap().hex(), "#1e3d12");
        assert_eq!(parse_color_in("#2d5a1b url(a.png) no-repeat").unwrap().hex(), "#2d5a1b");
    }

    #[test]
    fn urls() {
        assert_eq!(resolve("https://a.dev/x/y.html", "../img/p.jpg"), "https://a.dev/img/p.jpg");
        assert_eq!(resolve("https://a.dev/x/", "p.jpg"), "https://a.dev/x/p.jpg");
        assert_eq!(resolve("https://a.dev", "/p.jpg?v=2"), "https://a.dev/p.jpg?v=2");
        assert_eq!(resolve("https://a.dev/x", "//cdn.b.com/p.jpg"), "https://cdn.b.com/p.jpg");
        assert_eq!(resolve("https://a.dev/x", "mailto:a@b.c"), "mailto:a@b.c");
        assert_eq!(resolve("https://a.dev/x/y", "#top"), "https://a.dev/x/y#top");
    }

    #[test]
    fn cascade_and_vars() {
        let dom = Dom::parse(
            "<html><body class='page'><a class='btn md:flex' href='#'>Go</a><p id='x' style='color: var(--ink)'>t</p></body></html>",
        );
        let mut sheet = Sheet::default();
        sheet.add(
            ":root { --brand: #2563eb; --ink: #111 } body { background: #faf8f5; font-family: 'Inter var', sans-serif }
             .btn { background-color: var(--brand, red) } .btn:hover { background: black }
             @media (min-width: 768px) { .md\\:flex { display: flex } } @media (max-width: 600px) { .btn { background: green } }
             @media (prefers-color-scheme: dark) { body { background: #000 } }",
            "https://a.dev/",
        );
        let st = Styles::compute(&dom, &sheet, "https://a.dev/");
        let a = dom.find(0, "a").unwrap();
        let body = dom.find(0, "body").unwrap();
        let p = dom.find(0, "p").unwrap();
        assert_eq!(st.bg(&dom, a).unwrap().hex(), "#2563eb");
        assert_eq!(st.bg(&dom, body).unwrap().hex(), "#faf8f5");
        assert_eq!(st.color(&dom, p).unwrap().hex(), "#111111");
        assert_eq!(st.font(&dom, p).as_deref(), Some("Inter"));
        assert_eq!(st.get(&dom, a, "display").as_deref(), Some("flex"));
    }
}
