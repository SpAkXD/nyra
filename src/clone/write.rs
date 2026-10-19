//! Writing the `use web` program of a cloned page: the model of `read.rs` as `site(...)`, one
//! `page("/", ...)` with the kit's components, and, for repeated data, a `struct` with an array
//! and a `for` loop, as a person would write it.
//!
//! Each part of the program (the navbar, a section, the footer) remembers its lines, so the
//! self-check can rewrite one part in a simpler form when the compiler rejects it.

use std::collections::{HashMap, HashSet};

use super::read::{md_plain, slug, Card, Field, Form, Item, Kind, Page, Quote, Section};

/// Ids of the parts that are not sections.
pub const NAV: usize = usize::MAX - 1;
pub const FOOT: usize = usize::MAX;

/// How simply a part is written: 0 with the kit's components, 1 with plain elements (headings,
/// paragraphs, images, lists), 2 with paragraphs only. A navbar or footer at level 2 is left out.
pub type Levels = HashMap<usize, u8>;

pub struct Written {
    pub src: String,
    /// (part, first line, last line), 1-based.
    pub parts: Vec<(usize, usize, usize)>,
    /// (part, component) for the summary: what each part became.
    pub components: Vec<(usize, String)>,
}

/// A Nyra string literal: `\` and `"` escaped, `{` and `}` doubled (Nyra strings interpolate).
pub fn lit(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '{' => out.push_str("{{"),
            '}' => out.push_str("}}"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push(' '),
            '\r' | '\0' => {}
            c if (c as u32) < 0x20 => {}
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Names a program may not use for its own types and script variables.
const TAKEN: &[&str] = &[
    "fn",
    "let",
    "var",
    "if",
    "else",
    "while",
    "for",
    "in",
    "return",
    "ret",
    "true",
    "false",
    "struct",
    "inout",
    "break",
    "continue",
    "arena",
    "enum",
    "match",
    "use",
    "none",
    "step",
    "ex",
    "free",
    "keep",
    "and",
    "or",
    "not",
    "print",
    "abs",
    "min",
    "max",
    "str",
    "int",
    "float",
    "char",
    "bool",
    "zip",
    "len",
    "site",
    "page",
    "css",
    "keyframes",
    "html",
    "section",
    "grid",
    "stack",
    "row",
    "card",
    "h1",
    "h2",
    "h3",
    "p",
    "small",
    "note",
    "img",
    "link",
    "button",
    "list",
    "table",
    "address",
    "chip",
    "embed",
    "quote",
    "navbar",
    "hero",
    "split",
    "marquee",
    "gallery",
    "hours",
    "map",
    "form",
    "field",
    "steps",
    "stat",
    "footer",
    "maps",
    "year",
    "web",
    "json",
    "time",
    "math",
    "text",
    "fs",
    "os",
    "io",
    "random",
    "input",
    "node",
    "msg",
    "update",
    "main",
    "self",
    "type",
    "impl",
    "class",
    "null",
];

struct W<'a> {
    page: &'a Page,
    levels: &'a Levels,
    names: HashSet<String>,
    /// (part, lines) before `page(...)`: structs and arrays.
    decls: Vec<(usize, Vec<String>)>,
    components: Vec<(usize, String)>,
    has_h1: bool,
    code: bool,
}

pub fn write(page: &Page, levels: &Levels, source: &str) -> Written {
    let mut w = W { page, levels, names: HashSet::new(), decls: Vec::new(), components: Vec::new(), has_h1: false, code: false };
    // the body of the page, part by part
    let mut body: Vec<(usize, Vec<String>)> = Vec::new();
    if let Some(nav) = &page.nav {
        if w.level(NAV) < 2 {
            body.push((NAV, vec![w.navbar(nav)]));
            w.components.push((NAV, "navbar".into()));
        }
    }
    w.has_h1 = page.sections.iter().enumerate().any(|(i, s)| s.kind == Kind::Hero && w.level(i) < 2);
    for (i, s) in page.sections.iter().enumerate() {
        let lines = w.section(i, s);
        if !lines.is_empty() {
            body.push((i, lines));
        }
    }
    if let Some(f) = &page.footer {
        if w.level(FOOT) < 2 {
            body.push((FOOT, w.footer(f)));
            w.components.push((FOOT, "footer".into()));
        }
    }

    // the program
    let mut lines: Vec<String> = Vec::new();
    let mut parts = Vec::new();
    lines.push(format!("// Cloned from {} by `nyra clone`: check it, refine it, then `nyra build` it.", source.replace('\n', " ")));
    lines.push("use web".into());
    lines.push(String::new());
    lines.push(w.site());
    if w.code {
        lines.push("css(\"pre {{ overflow-x: auto; padding: 1rem 1.25rem; border-radius: 12px; background: color-mix(in srgb, var(--ink) 7%, transparent); font-size: .875rem; line-height: 1.55 }} pre code {{ padding: 0; background: none }}\")".into());
    }
    let split = |v: Vec<String>| v.into_iter().flat_map(|l| l.split('\n').map(String::from).collect::<Vec<_>>()).collect::<Vec<_>>();
    for (part, d) in std::mem::take(&mut w.decls) {
        lines.push(String::new());
        let start = lines.len() + 1;
        lines.extend(split(d));
        parts.push((part, start, lines.len()));
    }
    lines.push(String::new());
    let title = if page.title.is_empty() { page.name.clone() } else { page.title.clone() };
    lines.push(format!("page(\"/\", {}) {{", lit(&title)));
    for (part, b) in body {
        let start = lines.len() + 1;
        lines.extend(split(b));
        parts.push((part, start, lines.len()));
    }
    lines.push("}".into());
    let mut src = lines.join("\n");
    src.push('\n');
    Written { src: fix_anchors(&src), parts, components: w.components }
}

/// A link to an `#id` that no element of the program has goes to `#top` instead (a missing
/// anchor is build error E0382). The reader keeps the ids links point to; this is the safety net
/// for an element whose id had no place in the kit (a heading inside a card).
fn fix_anchors(src: &str) -> String {
    let mut ids: HashSet<String> = ["top", "main"].iter().map(|s| s.to_string()).collect();
    for pat in ["section(\"", "id: \""] {
        let mut rest = src;
        while let Some(at) = rest.find(pat) {
            rest = &rest[at + pat.len()..];
            if let Some(end) = rest.find('"') {
                ids.insert(rest[..end].to_string());
            }
        }
    }
    let mut out = String::with_capacity(src.len());
    let mut rest = src;
    loop {
        let a = rest.find("\"#");
        let b = rest.find("](#");
        let (at, skip, close) = match (a, b) {
            (Some(x), Some(y)) if y < x => (y, 3, ')'),
            (Some(x), _) => (x, 2, '"'),
            (None, Some(y)) => (y, 3, ')'),
            (None, None) => break,
        };
        out.push_str(&rest[..at + skip]);
        rest = &rest[at + skip..];
        let end = rest.find(close).unwrap_or(rest.len());
        let id = &rest[..end];
        // a link is a quoted `#id` after `, ` (in a tuple or a call) or `href: `, or in `[..](#id)`
        // (colors are `#hex` too, after `primary: ` and friends)
        let link = close == ')' || out.ends_with(", \"#") || out.ends_with("href: \"#");
        let ok = !link || ids.contains(id) || !id.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') || id.is_empty();
        out.push_str(if ok { id } else { "top" });
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

impl W<'_> {
    fn level(&self, part: usize) -> u8 {
        self.levels.get(&part).copied().unwrap_or(0)
    }

    fn site(&self) -> String {
        let p = self.page;
        let t = &p.theme;
        let mut args = vec![lit(&p.name)];
        if !p.lang.is_empty() && p.lang != "en" {
            args.push(format!("lang: {}", lit(&p.lang)));
        }
        if !p.desc.is_empty() {
            args.push(format!("desc: {}", lit(&p.desc)));
        }
        for (k, v) in [("primary", &t.primary), ("accent", &t.accent), ("bg", &t.bg), ("ink", &t.ink)] {
            if let Some(v) = v {
                args.push(format!("{k}: {}", lit(v)));
            }
        }
        if let Some(f) = &t.font {
            args.push(format!("font: {}", lit(f)));
        }
        if let Some(f) = &t.display {
            args.push(format!("display: {}", lit(f)));
        }
        if let Some(r) = t.radius {
            args.push(format!("radius: {r}"));
        }
        if t.dark {
            args.push("style: \"dark\"".into());
        }
        format!("site({})", args.join(", "))
    }

    fn navbar(&self, n: &super::read::Nav) -> String {
        let links: Vec<String> = n.links.iter().map(|(l, h)| format!("({}, {})", lit(l), lit(h))).collect();
        let mut s = format!("    navbar({}, [{}]", lit(if n.brand.is_empty() { &self.page.name } else { &n.brand }), links.join(", "));
        if self.level(NAV) == 0 {
            if let Some((l, h)) = &n.cta {
                s.push_str(&format!(", cta: ({}, {})", lit(l), lit(h)));
            }
            if !n.logo.is_empty() {
                s.push_str(&format!(", logo: {}", lit(&n.logo)));
            }
        }
        s.push(')');
        s
    }

    fn footer(&self, f: &super::read::Foot) -> Vec<String> {
        let mut args = Vec::new();
        let simple = self.level(FOOT) > 0;
        if !f.brand.is_empty() {
            args.push(format!("brand: {}", lit(&f.brand)));
        }
        if !f.text.is_empty() {
            args.push(format!("text: {}", lit(&f.text)));
        }
        if !f.links.is_empty() && !simple {
            let l: Vec<String> = f.links.iter().map(|(a, b)| format!("({}, {})", lit(a), lit(b))).collect();
            args.push(format!("links: [{}]", l.join(", ")));
            if !f.links_title.is_empty() {
                args.push(format!("links_title: {}", lit(&md_plain(&f.links_title))));
            }
        }
        if !f.contact.is_empty() {
            let l: Vec<String> = f.contact.iter().map(|c| lit(c)).collect();
            args.push(format!("contact: [{}]", l.join(", ")));
        }
        if !f.small.is_empty() {
            let l: Vec<String> = f.small.iter().map(|c| lit(c)).collect();
            args.push(format!("small: [{}]", l.join(", ")));
        }
        if !f.id.is_empty() {
            args.push(format!("id: {}", lit(&f.id)));
        }
        wrap_call("    ", "footer", &args, "")
    }

    fn section(&mut self, i: usize, s: &Section) -> Vec<String> {
        let level = self.level(i);
        let mut out = Vec::new();
        match (s.kind, level) {
            (Kind::Split, 0) => {
                self.components.push((i, "split".into()));
                out.push("    split {".into());
                for c in &s.panels {
                    out.push(format!("        {}", self.card_call(c, "", false)));
                }
                out.push("    }".into());
                return out;
            }
            (Kind::Split, _) => {
                // the panels as a grid of cards, or their text
                self.components.push((i, "section".into()));
                out.push(format!("    section({}) {{", lit(&s.id)));
                for c in &s.panels {
                    self.card_simple(c, "        ", level, &mut out);
                }
                out.push("    }".into());
                return out;
            }
            // the kit's hero is a photo or a colored band; a hero on the page's own background is
            // closer as a section that starts with the h1
            (Kind::Hero, 0) if !s.image.is_empty() || !s.bg.is_empty() => {
                self.components.push((i, "hero".into()));
                let mut args = vec![lit(&s.title)];
                if !s.kicker.is_empty() {
                    args.push(format!("kicker: {}", lit(&s.kicker)));
                }
                if !s.text.is_empty() {
                    args.push(format!("text: {}", lit(&s.text)));
                }
                if !s.image.is_empty() {
                    args.push(format!("image: {}", lit(&s.image)));
                    args.push(format!("alt: {}", lit(&s.alt)));
                }
                if !s.buttons.is_empty() {
                    let b: Vec<String> = s.buttons.iter().map(|(l, h)| format!("({}, {})", lit(l), lit(h))).collect();
                    args.push(format!("buttons: [{}]", b.join(", ")));
                }
                if !s.id.is_empty() {
                    args.push(format!("id: {}", lit(&s.id)));
                }
                return wrap_call("    ", "hero", &args, "");
            }
            _ => {}
        }
        // a plain section (or a simpler hero)
        let mut args = Vec::new();
        let title = s.title.clone();
        let hero_simple = s.kind == Kind::Hero;
        if !s.id.is_empty() || !title.is_empty() {
            args.push(lit(&s.id));
        }
        if !title.is_empty() && !hero_simple {
            args.push(lit(&title));
        }
        if !s.kicker.is_empty() && !hero_simple {
            args.push(format!("kicker: {}", lit(&s.kicker)));
        }
        if !s.text.is_empty() && level < 2 && !hero_simple {
            args.push(format!("text: {}", lit(&s.text)));
        }
        if !s.bg.is_empty() && level == 0 {
            args.push(format!("bg: {}", lit(&s.bg)));
        }
        if self.page.theme.left && !title.is_empty() && !hero_simple {
            args.push("align: \"left\"".into());
        }
        if self.page.theme.narrow {
            args.push("width: \"narrow\"".into());
        }
        if args.first().is_some_and(|a| a == "\"\"") && args.len() == 1 {
            args.clear();
        }
        let mut kids = Vec::new();
        if hero_simple {
            if !s.kicker.is_empty() {
                kids.push(format!("        small({})", lit(&s.kicker)));
            }
            kids.push(format!("        {}({})", if level < 2 { "h1" } else { "p" }, lit(&title)));
            if !s.text.is_empty() {
                kids.push(format!("        p({}, size: \"lg\")", lit(&s.text)));
            }
            if !s.image.is_empty() && level < 2 {
                kids.push(format!("        img({}, {})", lit(&s.image), lit(&s.alt)));
            }
            if !s.buttons.is_empty() && level < 2 {
                kids.push(self.buttons(&s.buttons, "        "));
            }
        } else if !s.text.is_empty() && level >= 2 {
            kids.push(format!("        p({})", lit(&s.text)));
        }
        for it in &s.items {
            self.item(i, it, "        ", level, &mut kids, s);
        }
        self.components.push((i, if hero_simple { "section (hero)".into() } else { "section".into() }));
        let head = if args.is_empty() { "    section".to_string() } else { format!("    section({})", args.join(", ")) };
        if kids.is_empty() {
            out.push(format!("{head} {{"));
            out.push("    }".into());
            return out;
        }
        out.push(format!("{head} {{"));
        out.extend(kids);
        out.push("    }".into());
        out
    }

    fn item(&mut self, part: usize, it: &Item, ind: &str, level: u8, out: &mut Vec<String>, s: &Section) {
        if level >= 2 {
            for t in text_of(it) {
                out.push(format!("{ind}p({})", lit(&t)));
            }
            return;
        }
        match it {
            Item::P(t) => out.push(format!("{ind}p({})", lit(t))),
            Item::H(l, t, hid) => {
                let id = if hid.is_empty() { String::new() } else { format!(", id: {}", lit(hid)) };
                let tag = if *l == 1 && !self.has_h1 {
                    self.has_h1 = true;
                    "h1"
                } else if *l <= 2 && s.title.is_empty() {
                    "h2"
                } else {
                    "h3"
                };
                out.push(format!("{ind}{tag}({}{id})", lit(t)));
            }
            Item::Img { src, alt, caption } => {
                let cap = if caption.is_empty() { String::new() } else { format!(", caption: {}", lit(caption)) };
                out.push(format!("{ind}img({}, {}{cap})", lit(src), lit(alt)));
            }
            Item::Buttons(b) => out.push(self.buttons(b, ind)),
            Item::List { items, ordered } => {
                let l: Vec<String> = items.iter().map(|x| lit(x)).collect();
                let ord = if *ordered { ", ordered: true" } else { "" };
                out.extend(wrap_list(ind, "list(", &l, &format!("]{ord})")));
            }
            Item::Table { head, rows } => {
                let r: Vec<String> =
                    rows.iter().map(|row| format!("[{}]", row.iter().map(|c| lit(c)).collect::<Vec<_>>().join(", "))).collect();
                let h = if head.is_empty() {
                    String::new()
                } else {
                    format!(", head: [{}]", head.iter().map(|c| lit(c)).collect::<Vec<_>>().join(", "))
                };
                out.extend(wrap_list(ind, "table(", &r, &format!("]{h})")));
            }
            Item::Cards { cols, cards } => {
                if level > 0 {
                    for c in cards {
                        self.card_simple(c, ind, level, out);
                    }
                    return;
                }
                self.cards(part, *cols, cards, ind, out, s);
            }
            Item::Gallery(photos) => {
                if level > 0 {
                    for (src, alt) in photos {
                        out.push(format!("{ind}img({}, {})", lit(src), lit(alt)));
                    }
                    return;
                }
                let l: Vec<String> = photos.iter().map(|(s, a)| format!("({}, {}, \"\")", lit(s), lit(a))).collect();
                out.extend(wrap_list(ind, "gallery(", &l, "])"));
            }
            Item::Quotes(qs) => {
                if level > 0 {
                    for q in qs {
                        out.push(format!("{ind}p({})", lit(&q.text)));
                        if !q.name.is_empty() {
                            out.push(format!("{ind}small({})", lit(&join_name(q))));
                        }
                    }
                    return;
                }
                if qs.len() == 1 && qs[0].name.is_empty() {
                    // a quotation without a person is a lead paragraph, not a testimonial card
                    out.push(format!("{ind}p({}, size: \"lg\")", lit(&qs[0].text)));
                } else if qs.len() == 1 {
                    out.push(format!("{ind}{}", quote_call(&qs[0])));
                } else {
                    out.push(format!("{ind}grid({}) {{", qs.len().min(3)));
                    for q in qs {
                        out.push(format!("{ind}    {}", quote_call(q)));
                    }
                    out.push(format!("{ind}}}"));
                }
            }
            Item::Stats(st) => {
                if level > 0 {
                    for (v, l) in st {
                        out.push(format!("{ind}p({})", lit(&format!("**{}** {l}", md_plain(v)))));
                    }
                    return;
                }
                out.push(format!("{ind}grid({}) {{", st.len().min(4)));
                for (v, l) in st {
                    out.push(format!("{ind}    stat({}, {})", lit(v), lit(l)));
                }
                out.push(format!("{ind}}}"));
            }
            Item::Steps(st) => {
                if level > 0 {
                    let l: Vec<String> = st.iter().map(|(a, b)| lit(&format!("**{}** {b}", md_plain(a)))).collect();
                    out.extend(wrap_list(ind, "list(", &l, "], ordered: true)"));
                    return;
                }
                let l: Vec<String> = st.iter().map(|(a, b)| format!("({}, {})", lit(a), lit(b))).collect();
                out.extend(wrap_list(ind, "steps(", &l, "])"));
            }
            Item::Form(f) => {
                if level > 0 {
                    if !f.title.is_empty() {
                        out.push(format!("{ind}h3({})", lit(&f.title)));
                    }
                    let l: Vec<String> = f.fields.iter().map(|x| lit(&x.label)).collect();
                    out.extend(wrap_list(ind, "list(", &l, "])"));
                    return;
                }
                self.form(f, ind, out);
            }
            Item::Hours(rows) => {
                if level > 0 {
                    let r: Vec<String> = rows.iter().map(|(a, b)| format!("[{}, {}]", lit(a), lit(&hours_plain(b)))).collect();
                    out.extend(wrap_list(ind, "table(", &r, "])"));
                    return;
                }
                let r: Vec<String> = rows.iter().map(|(a, b)| format!("({}, {})", lit(a), lit(b))).collect();
                out.extend(wrap_list(ind, "hours(", &r, "])"));
            }
            Item::Map(a) => {
                if level > 0 {
                    out.push(format!("{ind}p({})", lit(&super::read::md_escape(a))));
                } else {
                    out.push(format!("{ind}map({})", lit(a)));
                }
            }
            Item::Embed { url, title } => {
                if level > 0 {
                    out.push(format!("{ind}p({})", lit(&format!("[{}]({url})", super::read::md_escape(title)))));
                } else {
                    out.push(format!("{ind}embed({}, {})", lit(url), lit(title)));
                }
            }
            Item::Address(lines) => {
                if level > 0 {
                    for l in lines {
                        out.push(format!("{ind}p({})", lit(l)));
                    }
                    return;
                }
                out.push(format!("{ind}address {{"));
                for l in lines {
                    out.push(format!("{ind}    p({})", lit(l)));
                }
                out.push(format!("{ind}}}"));
            }
            Item::Code(code) => {
                if level > 0 {
                    out.push(format!("{ind}p({})", lit(&format!("`{}`", code.replace('`', "'").replace('\n', " ")))));
                    return;
                }
                self.code = true;
                let esc = code.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
                out.push(format!("{ind}html({})", lit(&format!("<pre><code>{esc}</code></pre>"))));
            }
        }
    }

    fn buttons(&self, b: &[(String, String)], ind: &str) -> String {
        if b.len() == 1 {
            return format!("{ind}button({}, {})", lit(&b[0].0), lit(&b[0].1));
        }
        let mut lines = vec![format!("{ind}row {{")];
        for (k, (l, h)) in b.iter().enumerate() {
            let style = if k == 0 { String::new() } else { ", style: \"outline\"".into() };
            lines.push(format!("{ind}    button({}, {}{style})", lit(l), lit(h)));
        }
        lines.push(format!("{ind}}}"));
        lines.join("\n")
    }

    fn form(&self, f: &Form, ind: &str, out: &mut Vec<String>) {
        let mut args = Vec::new();
        if !f.title.is_empty() {
            args.push(lit(&f.title));
        }
        if !f.mailto.is_empty() {
            args.push(format!("mailto: {}", lit(&f.mailto)));
        } else if !f.to.is_empty() {
            args.push(format!("to: {}", lit(&f.to)));
        }
        if !f.send.is_empty() {
            args.push(format!("send: {}", lit(&f.send)));
        }
        let head = if args.is_empty() { "form".to_string() } else { format!("form({})", args.join(", ")) };
        out.push(format!("{ind}{head} {{"));
        for x in &f.fields {
            out.push(format!("{ind}    {}", field_call(x)));
        }
        out.push(format!("{ind}}}"));
    }

    /// A grid of cards: with data repeated four or more times, a struct, an array and a loop.
    fn cards(&mut self, part: usize, cols: usize, cards: &[Card], ind: &str, out: &mut Vec<String>, s: &Section) {
        let simple = cards.iter().all(|c| c.buttons.is_empty() && c.extra.is_empty() && c.items.is_empty());
        let fields: Vec<&str> = ["title", "text", "image", "alt", "href", "tag"]
            .into_iter()
            .filter(|f| cards.iter().any(|c| !card_field(c, f).is_empty()))
            .filter(|f| *f != "alt" || cards.iter().any(|c| !c.alt.is_empty() && c.alt != md_plain(&c.title)))
            .collect();
        if simple && cards.len() >= 4 && fields.len() >= 2 {
            let (ty, var) = self.data_names(s);
            let mut decl =
                vec![format!("struct {ty} {{ {} }}", fields.iter().map(|f| format!("{f}: str")).collect::<Vec<_>>().join(", "))];
            decl.push(format!("let {var} = ["));
            for (k, c) in cards.iter().enumerate() {
                let vals: Vec<String> = fields.iter().map(|f| format!("{f}: {}", lit(&card_field(c, f)))).collect();
                decl.push(format!("    {ty}({}){}", vals.join(", "), if k + 1 < cards.len() { "," } else { "" }));
            }
            decl.push("]".into());
            self.decls.push((part, decl));
            let x = "c";
            let mut call = Vec::new();
            let title = if fields.contains(&"title") { format!("{x}.title") } else { "\"\"".into() };
            call.push(title);
            if fields.contains(&"text") {
                call.push(format!("{x}.text"));
            }
            for f in ["image", "alt", "href", "tag"] {
                if fields.contains(&f) {
                    call.push(format!("{f}: {x}.{f}"));
                }
            }
            out.push(format!("{ind}grid({cols}) {{"));
            out.push(format!("{ind}    for {x} in {var} {{ card({}) }}", call.join(", ")));
            out.push(format!("{ind}}}"));
            return;
        }
        out.push(format!("{ind}grid({cols}) {{"));
        for c in cards {
            let inner = format!("{ind}    ");
            let call = self.card_call(c, &inner, true);
            out.push(format!("{inner}{call}"));
        }
        out.push(format!("{ind}}}"));
    }

    /// `card(...)`, with a child block for its buttons and other content.
    fn card_call(&mut self, c: &Card, ind: &str, kids: bool) -> String {
        let mut args = Vec::new();
        if !c.title.is_empty() || !c.text.is_empty() {
            args.push(lit(&c.title));
        }
        if !c.text.is_empty() {
            args.push(lit(&c.text));
        }
        if !c.image.is_empty() {
            args.push(format!("image: {}", lit(&c.image)));
            if !c.alt.is_empty() && c.alt != md_plain(&c.title) {
                args.push(format!("alt: {}", lit(&c.alt)));
            }
        }
        if !c.href.is_empty() {
            args.push(format!("href: {}", lit(&c.href)));
        }
        if !c.tag.is_empty() {
            args.push(format!("tag: {}", lit(&c.tag)));
        }
        if !c.color.is_empty() {
            args.push(format!("color: {}", lit(&c.color)));
        }
        if !c.items.is_empty() {
            args.push(format!("items: [{}]", c.items.iter().map(|x| lit(x)).collect::<Vec<_>>().join(", ")));
        }
        let mut s = format!("card({})", args.join(", "));
        if kids && (!c.buttons.is_empty() || !c.extra.is_empty()) {
            let inner = format!("{ind}    ");
            let mut lines = Vec::new();
            if !c.buttons.is_empty() {
                lines.push(self.buttons(&c.buttons, &inner));
            }
            let dummy = super::read::Section {
                kind: Kind::Plain,
                id: String::new(),
                title: "x".into(),
                kicker: String::new(),
                text: String::new(),
                bg: String::new(),
                image: String::new(),
                alt: String::new(),
                buttons: Vec::new(),
                items: Vec::new(),
                panels: Vec::new(),
                level: 0,
            };
            for e in &c.extra {
                // a nested grid inside a card stays simple
                let lvl = if matches!(e, Item::Cards { .. }) { 1 } else { 0 };
                self.item(usize::MAX - 2, e, &inner, lvl, &mut lines, &dummy);
            }
            s.push_str(" {\n");
            s.push_str(&lines.join("\n"));
            s.push('\n');
            s.push_str(ind);
            s.push('}');
        }
        s
    }

    fn card_simple(&mut self, c: &Card, ind: &str, level: u8, out: &mut Vec<String>) {
        if !c.title.is_empty() {
            out.push(format!("{ind}h3({})", lit(&c.title)));
        }
        if !c.tag.is_empty() {
            out.push(format!("{ind}p({})", lit(&c.tag)));
        }
        if !c.text.is_empty() {
            out.push(format!("{ind}p({})", lit(&c.text)));
        }
        if !c.image.is_empty() && level < 2 {
            out.push(format!("{ind}img({}, {})", lit(&c.image), lit(&c.alt)));
        }
        if !c.items.is_empty() {
            let l: Vec<String> = c.items.iter().map(|x| lit(x)).collect();
            out.extend(wrap_list(ind, "list(", &l, "])"));
        }
        for (l, h) in &c.buttons {
            out.push(format!("{ind}p({})", lit(&format!("[{l}]({h})"))));
        }
        for e in &c.extra {
            for t in text_of(e) {
                out.push(format!("{ind}p({})", lit(&t)));
            }
        }
    }

    /// A struct name and an array name for a section's data, from its title or id.
    fn data_names(&mut self, s: &Section) -> (String, String) {
        let from = if !s.id.is_empty() { s.id.clone() } else { md_plain(&s.title) };
        let word =
            slug(&from).split('-').rfind(|w| w.len() > 2 && w.chars().all(|c| c.is_ascii_alphanumeric())).unwrap_or("").to_string();
        let word = if word.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) { word } else { String::new() };
        let singular = if word.ends_with("ies") && word.len() > 4 {
            format!("{}y", &word[..word.len() - 3])
        } else if word.ends_with('s') && !word.ends_with("ss") && word.len() > 3 {
            word[..word.len() - 1].to_string()
        } else {
            word.clone()
        };
        let (mut ty, mut var) = if singular.is_empty() {
            ("Item".to_string(), "items".to_string())
        } else {
            let mut t = singular.clone();
            t[..1].make_ascii_uppercase();
            let v = if word != singular { word.clone() } else { format!("{word}_items") };
            (t, v)
        };
        let base_ty = ty.clone();
        let base_var = var.clone();
        let mut k = 2;
        while TAKEN.contains(&var.as_str())
            || TAKEN.contains(&ty.to_ascii_lowercase().as_str())
            || self.names.contains(&var)
            || self.names.contains(&ty)
            || ty == "Node"
            || ty == "WebConfig"
            || ty == "Msg"
        {
            ty = format!("{base_ty}{k}");
            var = format!("{base_var}{k}");
            k += 1;
        }
        self.names.insert(ty.clone());
        self.names.insert(var.clone());
        (ty, var)
    }
}

fn card_field(c: &Card, f: &str) -> String {
    match f {
        "title" => c.title.clone(),
        "text" => c.text.clone(),
        "image" => c.image.clone(),
        "alt" => {
            if c.alt != md_plain(&c.title) {
                c.alt.clone()
            } else {
                String::new()
            }
        }
        "href" => c.href.clone(),
        "tag" => c.tag.clone(),
        _ => String::new(),
    }
}

fn quote_call(q: &Quote) -> String {
    let mut s = format!("quote({}, {}", lit(&q.text), lit(&q.name));
    if !q.place.is_empty() {
        s.push_str(&format!(", place: {}", lit(&q.place)));
    }
    s.push(')');
    s
}

fn join_name(q: &Quote) -> String {
    if q.place.is_empty() {
        q.name.clone()
    } else {
        format!("{}, {}", q.name, q.place)
    }
}

fn field_call(x: &Field) -> String {
    let mut s = format!("field({}, {}", lit(&x.name), lit(&x.label));
    if x.kind != "text" {
        s.push_str(&format!(", type: {}", lit(&x.kind)));
    }
    if x.req {
        s.push_str(", req: true");
    }
    if !x.hint.is_empty() {
        s.push_str(&format!(", hint: {}", lit(&x.hint)));
    }
    if !x.options.is_empty() {
        s.push_str(&format!(", options: [{}]", x.options.iter().map(|o| lit(o)).collect::<Vec<_>>().join(", ")));
    }
    s.push(')');
    s
}

/// "1-5 8:00-16:00" -> "8:00 – 16:00" for a plain table.
fn hours_plain(spec: &str) -> String {
    if spec.is_empty() {
        return "–".into();
    }
    spec.split(' ').skip(1).map(|t| t.replace('-', " – ")).collect::<Vec<_>>().join(", ")
}

/// The text an item shows, as paragraphs (the simplest form of any item).
fn text_of(it: &Item) -> Vec<String> {
    match it {
        Item::P(t) | Item::H(_, t, _) | Item::Map(t) => vec![t.clone()],
        Item::Img { caption, .. } => {
            if caption.is_empty() {
                vec![]
            } else {
                vec![caption.clone()]
            }
        }
        Item::Buttons(b) => b.iter().map(|(l, _)| l.clone()).collect(),
        Item::List { items, .. } => items.clone(),
        Item::Table { head, rows } => {
            std::iter::once(head).chain(rows).map(|r| r.join(" · ")).filter(|r| !r.trim().is_empty()).collect()
        }
        Item::Cards { cards, .. } => cards
            .iter()
            .flat_map(|c| {
                let mut v = vec![c.title.clone(), c.tag.clone(), c.text.clone()];
                v.extend(c.items.clone());
                v.extend(c.buttons.iter().map(|b| b.0.clone()));
                v.extend(c.extra.iter().flat_map(text_of));
                v
            })
            .filter(|t| !t.is_empty())
            .collect(),
        Item::Gallery(p) => p.iter().map(|x| super::read::md_escape(&x.1)).filter(|t| !t.is_empty()).collect(),
        Item::Quotes(q) => q.iter().flat_map(|q| [q.text.clone(), join_name(q)]).filter(|t| !t.is_empty()).collect(),
        Item::Stats(s) | Item::Steps(s) => s.iter().map(|(a, b)| format!("{a} {b}")).collect(),
        Item::Form(f) => {
            std::iter::once(f.title.clone()).chain(f.fields.iter().map(|x| x.label.clone())).filter(|t| !t.is_empty()).collect()
        }
        Item::Hours(h) => h.iter().map(|(a, b)| format!("{a} {}", hours_plain(b))).collect(),
        Item::Embed { title, .. } => vec![super::read::md_escape(title)],
        Item::Address(l) => l.clone(),
        Item::Code(c) => vec![format!("`{}`", c.replace('`', "'").replace('\n', " "))],
    }
}

/// A call with its arguments on one line, or one per line when it is long.
fn wrap_call(ind: &str, name: &str, args: &[String], tail: &str) -> Vec<String> {
    let one = format!("{ind}{name}({}){tail}", args.join(", "));
    if one.chars().count() <= 140 || args.len() <= 1 {
        return vec![one];
    }
    let mut lines = vec![format!("{ind}{name}({},", args[0])];
    for (k, a) in args.iter().enumerate().skip(1) {
        let end = if k + 1 < args.len() { "," } else { "" };
        lines.push(format!("{ind}    {a}{end}"));
    }
    if let Some(last) = lines.last_mut() {
        last.push(')');
        last.push_str(tail);
    }
    lines
}

/// `open` + the elements + `close` (`list([`, `])`), one element per line when long.
fn wrap_list(ind: &str, open: &str, elems: &[String], close: &str) -> Vec<String> {
    let one = format!("{ind}{open}[{}{close}", elems.join(", "));
    if one.chars().count() <= 140 || elems.len() <= 1 {
        return vec![one];
    }
    let mut lines = vec![format!("{ind}{open}[")];
    for (k, e) in elems.iter().enumerate() {
        let end = if k + 1 < elems.len() { "," } else { "" };
        lines.push(format!("{ind}    {e}{end}"));
    }
    lines.push(format!("{ind}{close}"));
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literals() {
        assert_eq!(lit("a \"b\" {c} \\ d"), "\"a \\\"b\\\" {{c}} \\\\ d\"");
    }
}
