//! The site's one CSS file and one JS file, made of only the parts its pages use.
//!
//! The kit (`src/web/kit.css`, `kit.js`) and the motion words (`src/web/motion.css`,
//! `motion.js`) are split into chunks by marker comments:
//!
//! ```text
//! /*! chunk: navbar */      a kit component or behavior, chosen by `_uses("navbar")`
//! /*! motion: rise */       a motion word, chosen when a page has it in a motion attribute
//! ```
//!
//! A chunk runs to the next marker. What comes before the first marker of a file is its base: the
//! kit's base CSS is always included; the base of the other files only when one of their chunks is.
//! The files are embedded when they exist (`build.rs`), so a missing one is simply empty.

include!(concat!(env!("OUT_DIR"), "/web_assets.rs"));

/// A file split at its markers of `kind` (`chunk` or `motion`): the base, then (name, text) in order.
pub fn split(text: &str, kind: &str) -> (String, Vec<(String, String)>) {
    let mut base = String::new();
    let mut parts: Vec<(String, String)> = Vec::new();
    let mut rest = text;
    let open = format!("/*! {kind}:");
    loop {
        let Some(at) = rest.find(&open) else {
            push(&mut base, &mut parts, rest);
            break;
        };
        push(&mut base, &mut parts, &rest[..at]);
        let after = &rest[at + open.len()..];
        let Some(end) = after.find("*/") else {
            push(&mut base, &mut parts, &rest[at..]);
            break;
        };
        parts.push((after[..end].trim().to_string(), String::new()));
        rest = &after[end + 2..];
    }
    (base, parts)
}

fn push(base: &mut String, parts: &mut [(String, String)], text: &str) {
    match parts.last_mut() {
        Some((_, t)) => t.push_str(text),
        None => base.push_str(text),
    }
}

/// The chunks of `text` named in `used` (in the file's order), after its base. `always`: the base
/// is included even when no chunk is. Empty when nothing is used.
pub fn pick(text: &str, kind: &str, used: &[String], always: bool) -> String {
    let (base, parts) = split(text, kind);
    let chosen: Vec<&(String, String)> = parts.iter().filter(|(n, _)| used.contains(n)).collect();
    if chosen.is_empty() && !always {
        return String::new();
    }
    let mut out = base;
    for (_, t) in chosen {
        out.push_str(t);
    }
    out
}

/// The motion words of a page: every word in its motion attributes, plus the attribute's own name
/// (`at` for `data-at`), so a chunk can serve a whole option.
pub const MOTION_ATTRS: [&str; 6] = ["data-anim", "data-hover", "data-loop", "data-scroll", "data-at", "data-motion"];

pub fn motion_words(tags: &[super::html::Tag], out: &mut Vec<String>) {
    for t in tags {
        for (n, v) in &t.attrs {
            if !MOTION_ATTRS.contains(&n.as_str()) {
                continue;
            }
            let mut add = |w: &str| {
                if !w.is_empty() && !out.iter().any(|o| o == w) {
                    out.push(w.to_string());
                }
            };
            add(&n["data-".len()..]);
            for w in v.as_deref().unwrap_or("").split_whitespace() {
                add(w);
            }
        }
    }
}

/// CSS without comments and with the whitespace that does not matter removed. Strings and
/// `url(...)` are copied as they are.
pub fn min_css(css: &str) -> String {
    let b: Vec<char> = css.chars().collect();
    let mut out = String::with_capacity(css.len());
    let mut i = 0;
    let mut space = false;
    while i < b.len() {
        let c = b[i];
        if c == '/' && b.get(i + 1) == Some(&'*') {
            let mut j = i + 2;
            while j + 1 < b.len() && !(b[j] == '*' && b[j + 1] == '/') {
                j += 1;
            }
            i = j + 2;
            continue;
        }
        if c == '"' || c == '\'' {
            if space {
                push_space(&mut out);
                space = false;
            }
            out.push(c);
            i += 1;
            while i < b.len() && b[i] != c {
                if b[i] == '\\' && i + 1 < b.len() {
                    out.push(b[i]);
                    i += 1;
                }
                out.push(b[i]);
                i += 1;
            }
            if i < b.len() {
                out.push(c);
            }
            i += 1;
            continue;
        }
        if c.is_whitespace() {
            space = true;
            i += 1;
            continue;
        }
        if space {
            // a space matters only between two parts of a value or a selector
            if !matches!(c, '{' | '}' | ';' | ',' | '>') && !out.ends_with(['{', '}', ';', ',', ':', '>']) && !out.is_empty() {
                out.push(' ');
            }
            space = false;
        }
        if c == '}' && out.ends_with(';') {
            out.pop();
        }
        out.push(c);
        i += 1;
    }
    out
}

fn push_space(out: &mut String) {
    if !out.is_empty() && !out.ends_with(['{', '}', ';', ',', ':', '>', '(']) {
        out.push(' ');
    }
}

/// JavaScript without its comment lines, blank lines, chunk markers and leading indentation
/// outside template literals. (A light pass: names and statements stay as written.)
pub fn min_js(js: &str) -> String {
    let mut out = String::with_capacity(js.len());
    let mut in_template = false;
    let mut in_block_comment = false;
    for line in js.lines() {
        let t = line.trim();
        if !in_template {
            if in_block_comment {
                if let Some(e) = t.find("*/") {
                    in_block_comment = false;
                    let rest = t[e + 2..].trim();
                    if !rest.is_empty() {
                        out.push_str(rest);
                        out.push('\n');
                    }
                }
                continue;
            }
            if t.is_empty() || t.starts_with("//") {
                continue;
            }
            if t.starts_with("/*") {
                if !t.contains("*/") {
                    in_block_comment = true;
                }
                if t.ends_with("*/") || !t.contains("*/") {
                    continue;
                }
            }
            out.push_str(t);
        } else {
            out.push_str(line);
        }
        out.push('\n');
        // a line with an odd number of backticks opens or closes a template literal
        if line.chars().filter(|&c| c == '`').count() % 2 == 1 {
            in_template = !in_template;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_and_picks_chunks() {
        let css = "body{x:1}\n/*! chunk: hero */.hero{a:b}\n/*! chunk: card */.card{c:d}\n";
        assert_eq!(pick(css, "chunk", &["card".into()], true), "body{x:1}\n.card{c:d}\n");
        assert_eq!(pick(css, "chunk", &[], true), "body{x:1}\n");
        assert_eq!(pick(css, "chunk", &[], false), "");
        let js = "const core = 1;\n/*! motion: rise */rise();\n";
        assert_eq!(pick(js, "motion", &["rise".into()], false), "const core = 1;\nrise();\n");
        assert_eq!(pick("", "chunk", &["x".into()], true), "");
    }

    #[test]
    fn minifies_css_and_js() {
        let css = "/* c */\n.a  .b > p ,\n  .c {\n  color : red ;\n  font: 12px \"A  B\";\n  margin: calc(1px + 2px);\n}\n";
        assert_eq!(min_css(css), ".a .b>p,.c{color :red;font:12px \"A  B\";margin:calc(1px + 2px)}");
        let js = "// x\nfunction f() {\n    /* y */\n    return `a\n    b`;\n}\n";
        assert_eq!(min_js(js), "function f() {\nreturn `a\n    b`;\n}\n");
    }
}
