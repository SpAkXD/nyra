//! Diagnostics: every error has a stable code, a position and an optional fix hint.
//! Rendered either for humans or as JSON for AI agents (`--json`).
//!
//! A diagnostic whose mistake has exactly one possible repair also carries a `fix`: text edits
//! that turn the program into what the author meant. `nyra check --fix` applies them (see
//! `fix.rs`), and `--json` prints them so an agent can apply them itself. A fix is only added
//! where it is certain; when there are alternatives, there is a hint and no fix.

use crate::ast::Span;

#[derive(Debug, Clone)]
pub struct Diag {
    pub code: &'static str,
    pub msg: String,
    pub span: Span,
    pub hint: Option<String>,
    /// Edits that repair the mistake; empty when there is no certain fix.
    pub fix: Vec<Edit>,
    /// For a failed example (E0250): the value it got and the one it expected, as Nyra code.
    pub actual: Option<String>,
    pub expected: Option<String>,
}

/// One edit of a fix: the text from `start` up to (not including) `end` becomes `text`.
/// Positions are 1-based lines and columns counted in characters, like `Span`.
#[derive(Debug, Clone, PartialEq)]
pub struct Edit {
    pub start: Span,
    pub end: Span,
    pub text: String,
    /// What the replaced text must be, apart from whitespace. A fix whose edits do not find the
    /// text they expect is dropped (`fix::validate`), so a wrong position never edits code.
    pub old: String,
    /// What the code right before `start` must end with, apart from whitespace (for insertions,
    /// which replace nothing that could be checked): `Point(` before the first argument.
    pub after: String,
}

impl Edit {
    /// Replaces `old`, which starts at `start` and holds no line break, with `text`.
    pub fn replace(start: Span, old: &str, text: impl Into<String>) -> Edit {
        let end = after(start, old);
        Edit { start, end, text: text.into(), old: old.to_string(), after: String::new() }
    }

    /// Replaces the range `start..end`, which must hold `old` and otherwise only whitespace.
    pub fn range(start: Span, end: Span, old: &str, text: impl Into<String>) -> Edit {
        Edit { start, end, text: text.into(), old: old.to_string(), after: String::new() }
    }

    /// The same edit, valid only where the code before it ends with `code`.
    pub fn after(mut self, code: impl Into<String>) -> Edit {
        self.after = code.into();
        self
    }

    /// Inserts `text` at `at`.
    pub fn insert(at: Span, text: impl Into<String>) -> Edit {
        Edit { start: at, end: at, text: text.into(), old: String::new(), after: String::new() }
    }
}

/// The position right after `text`, which starts at `s` and holds no line break.
pub fn after(s: Span, text: &str) -> Span {
    Span { line: s.line, col: s.col + text.chars().count() }
}

impl Diag {
    pub fn new(code: &'static str, msg: impl Into<String>, span: Span) -> Self {
        Diag { code, msg: msg.into(), span, hint: None, fix: Vec::new(), actual: None, expected: None }
    }

    /// The value an example got and the one it expected (shown as `actual` and `expected` in JSON).
    pub fn values(mut self, actual: String, expected: String) -> Self {
        self.actual = Some(actual);
        self.expected = Some(expected);
        self
    }

    /// Sets the hint. A fix belongs to the hint it was made with, so a new hint drops it:
    /// add the fix after the hint (`.hint(h).fix(edits)`).
    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self.fix.clear();
        self
    }

    /// Sets the hint only if there is none yet, so a more specific hint is never replaced.
    pub fn or_hint(mut self, hint: impl Into<String>) -> Self {
        if self.hint.is_none() {
            self.hint = Some(hint.into());
        }
        self
    }

    /// The edits that repair this mistake (they go with the current hint).
    pub fn fix(mut self, edits: Vec<Edit>) -> Self {
        self.fix = edits;
        self
    }

    /// A fix of a single edit, if there is one.
    pub fn fix_opt(self, edit: Option<Edit>) -> Self {
        match edit {
            Some(e) => self.fix(vec![e]),
            None => self,
        }
    }

    /// The same diagnostic with its position and its fix moved by `f` (for code inside a string).
    pub fn moved(mut self, f: impl Fn(Span) -> Span) -> Self {
        self.span = f(self.span);
        for e in &mut self.fix {
            e.start = f(e.start);
            e.end = f(e.end);
        }
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
            // keep tabs as tabs, so the caret lines up under the character in any editor
            let mut before = line.chars();
            let caret: String = (1..d.span.col).map(|_| if before.next() == Some('\t') { '\t' } else { ' ' }).collect();
            out += &format!("{pad} |\n{num} | {line}\n{pad} | {caret}^\n");
        }
        if let Some(h) = &d.hint {
            out += &format!("  = hint: {h}\n");
        }
        if let Some(f) = crate::fix::preview(src, &d.fix) {
            out += &format!("  = fix: {f}\n");
        }
        out += &format!("  = explain: nyra explain {}\n", d.code);
        out.push('\n');
    }
    out
}

/// The fixes that were applied automatically, for people: one warning each.
pub fn render_warnings_human(applied: &[crate::fix::Applied], file: &str) -> String {
    let mut out = String::new();
    for a in applied {
        let d = &a.diag;
        out += &format!("warning[{}]: fixed automatically: {}\n", d.code, d.msg);
        out += &format!("  --> {}:{}:{}\n", file, d.span.line, d.span.col);
        let num = d.span.line.to_string();
        let pad = " ".repeat(num.len());
        out += &format!("{pad} |\n{num} | {}\n", a.line);
        if let Some(p) = &a.preview {
            out += &format!("{pad} = applied: {p}\n");
        }
        out.push('\n');
    }
    out
}

/// The `warnings` array: one object per applied fix, with the `code`, `message`, position, the
/// `applied` text (what the changed lines look like now) and the `fix` edits.
pub fn render_json_warnings(applied: &[crate::fix::Applied], file: &str) -> String {
    let items: Vec<String> = applied
        .iter()
        .map(|a| {
            let d = &a.diag;
            format!(
                "{{\"code\":\"{}\",\"message\":{},\"file\":{},\"line\":{},\"col\":{},\"applied\":{}{}}}",
                d.code,
                json_str(&d.msg),
                json_str(file),
                d.span.line,
                d.span.col,
                a.preview.as_deref().map(json_str).unwrap_or_else(|| "null".into()),
                fix_json(&d.fix)
            )
        })
        .collect();
    format!("[{}]", items.join(","))
}

pub fn render_json(diags: &[Diag], file: &str) -> String {
    format!("{{\"ok\":{},\"errors\":{}}}", diags.is_empty(), render_json_errors(diags, file))
}

/// The `errors` array of `render_json`.
pub fn render_json_errors(diags: &[Diag], file: &str) -> String {
    let items: Vec<String> = diags
        .iter()
        .map(|d| {
            let values = match (&d.actual, &d.expected) {
                (Some(a), Some(e)) => format!(",\"actual\":{},\"expected\":{}", json_str(a), json_str(e)),
                _ => String::new(),
            };
            format!(
                "{{\"code\":\"{}\",\"message\":{},\"file\":{},\"line\":{},\"col\":{},\"hint\":{}{}{values}}}",
                d.code,
                json_str(&d.msg),
                json_str(file),
                d.span.line,
                d.span.col,
                d.hint.as_deref().map(json_str).unwrap_or_else(|| "null".into()),
                fix_json(&d.fix)
            )
        })
        .collect();
    format!("[{}]", items.join(","))
}

/// `,"fix":[{"line":..,"col":..,"end_line":..,"end_col":..,"text":".."}]`, or nothing without a fix.
/// Columns count characters; the end is exclusive.
fn fix_json(fix: &[Edit]) -> String {
    if fix.is_empty() {
        return String::new();
    }
    let edits: Vec<String> = fix
        .iter()
        .map(|e| {
            format!(
                "{{\"line\":{},\"col\":{},\"end_line\":{},\"end_col\":{},\"text\":{}}}",
                e.start.line,
                e.start.col,
                e.end.line,
                e.end.col,
                json_str(&e.text)
            )
        })
        .collect();
    format!(",\"fix\":[{}]", edits.join(","))
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

/// "did you mean ...?": the closest candidate. A name must be close in proportion to its length
/// (one edit for up to five characters, two for longer names) and have at least three characters,
/// otherwise `max` would be "corrected" to `main` and `x` to `y`. A different capital letter always counts.
pub fn suggest<'a>(name: &str, candidates: impl IntoIterator<Item = &'a str>) -> Option<String> {
    let allowed = (name.chars().count() / 3).clamp(1, 2);
    let long_enough = name.chars().count() >= 3;
    candidates
        .into_iter()
        .map(|c| (levenshtein(name, c), c))
        .filter(|(d, c)| *d > 0 && (name.eq_ignore_ascii_case(c) || (long_enough && *d <= allowed)))
        // a name that differs only in case comes first (`Print` is `print`, not `Point`); ties go to
        // the alphabetically first name, so hints never change between runs
        .min_by_key(|&(d, c)| (!name.eq_ignore_ascii_case(c), d, c))
        .map(|(_, c)| format!("did you mean `{c}`?"))
}

/// `suggest`, and with it the suggested name as a fix when it differs from `name` only in case
/// and no other candidate does: `point` for `Point` is certain, `cout` for `count` is a guess.
pub fn suggest_fix<'a>(name: &str, candidates: impl IntoIterator<Item = &'a str>) -> Option<(String, Option<&'a str>)> {
    let all: Vec<&'a str> = candidates.into_iter().collect();
    let hint = suggest(name, all.iter().copied())?;
    let mut same = all.iter().copied().filter(|c| *c != name && c.eq_ignore_ascii_case(name));
    let first = same.next();
    let fix = first.filter(|c| same.all(|o| o == *c) && hint == format!("did you mean `{c}`?"));
    Some((hint, fix))
}

pub fn levenshtein(a: &str, b: &str) -> usize {
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
