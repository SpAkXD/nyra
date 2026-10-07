//! Diagnostics: every error has a stable code, a position and an optional fix hint.
//! Rendered either for humans or as JSON for AI agents (`--json`).

use crate::ast::Span;

#[derive(Debug, Clone)]
pub struct Diag {
    pub code: &'static str,
    pub msg: String,
    pub span: Span,
    pub hint: Option<String>,
}

impl Diag {
    pub fn new(code: &'static str, msg: impl Into<String>, span: Span) -> Self {
        Diag { code, msg: msg.into(), span, hint: None }
    }

    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

pub fn render_human(diags: &[Diag], file: &str, src: &str) -> String {
    let lines: Vec<&str> = src.lines().collect();
    let mut out = String::new();
    for d in diags {
        out += &format!("error[{}]: {}\n", d.code, d.msg);
        out += &format!("  --> {}:{}:{}\n", file, d.span.line, d.span.col);
        if let Some(line) = d.span.line.checked_sub(1).and_then(|i| lines.get(i)) {
            let num = d.span.line.to_string();
            let pad = " ".repeat(num.len());
            let caret = " ".repeat(d.span.col.saturating_sub(1));
            out += &format!("{pad} |\n{num} | {line}\n{pad} | {caret}^\n");
        }
        if let Some(h) = &d.hint {
            out += &format!("  = hint: {h}\n");
        }
        out.push('\n');
    }
    out
}

pub fn render_json(diags: &[Diag], file: &str) -> String {
    let items: Vec<String> = diags
        .iter()
        .map(|d| {
            format!(
                "{{\"code\":\"{}\",\"message\":{},\"file\":{},\"line\":{},\"col\":{},\"hint\":{}}}",
                d.code,
                json_str(&d.msg),
                json_str(file),
                d.span.line,
                d.span.col,
                d.hint.as_deref().map(json_str).unwrap_or_else(|| "null".into())
            )
        })
        .collect();
    format!("{{\"ok\":{},\"errors\":[{}]}}", diags.is_empty(), items.join(","))
}

pub fn json_str(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// "did you mean ...?" — closest candidate within edit distance 2.
pub fn suggest<'a>(name: &str, candidates: impl IntoIterator<Item = &'a str>) -> Option<String> {
    candidates
        .into_iter()
        .map(|c| (levenshtein(name, c), c))
        .filter(|(d, _)| *d <= 2)
        // ties go to the alphabetically first name, so hints never change between runs
        .min_by_key(|&(d, c)| (d, c))
        .map(|(_, c)| format!("did you mean `{c}`?"))
}

fn levenshtein(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let cost = if ca == *cb { 0 } else { 1 };
            cur.push((prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}
