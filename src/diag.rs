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
    /// The imported file the mistake is in (`None`: the file being compiled).
    pub file: Option<String>,
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
        Diag { code, msg: msg.into(), span, hint: None, fix: Vec::new(), actual: None, expected: None, file: None }
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

thread_local! {
    /// The warnings of the program being compiled (see `warn`).
    static WARNINGS: std::cell::RefCell<Vec<Diag>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Records a warning: a mistake that is probably there but that the language allows, so the build
/// goes on (`"${x}"` prints a `$`). Warnings are printed to stderr and listed in `--json`.
pub fn warn(d: Diag) {
    WARNINGS.with(|w| {
        let mut w = w.borrow_mut();
        // (the web build reports one warning per page at the same call: those differ in the message)
        if !w.iter().any(|o| o.code == d.code && o.span == d.span && o.msg == d.msg) {
            w.push(d);
        }
    });
}

/// Forgets the warnings of the last compilation.
pub fn clear_warnings() {
    WARNINGS.with(|w| w.borrow_mut().clear());
}

/// The warnings of the last compilation, in the order of the source.
pub fn warnings() -> Vec<Diag> {
    let mut v = WARNINGS.with(|w| w.borrow().clone());
    v.sort_by_key(|d| (d.span.line, d.span.col));
    v
}

// ---- what is shown ----------------------------------------------------------------------------------
//
// Every report shows the errors first (the first line an agent reads is what blocks the build), then
// the fixes that were applied in memory, then the warnings. Diagnostics are moved to their places in
// the text the author has (`fix::to_origin`), sorted by place, and one root mistake that shows up in
// several places is one diagnostic that lists the other lines (`condense`).

/// The diagnostics as they are shown: at their places in the original text, one per root mistake,
/// in the order of the source (the imported files after the program's own).
pub fn prepare(diags: &[Diag]) -> Vec<Diag> {
    let mut v: Vec<Diag> = diags.iter().cloned().map(|d| tidy(crate::fix::to_origin(d))).collect();
    v.sort_by_key(|d| (d.file.is_some(), d.file.clone(), d.span.line, d.span.col));
    condense(v)
}

/// The text of a diagnostic as it is shown: the web functions by their bare names (`hero`, not
/// `web.hero`: that is how a program calls them), and with a fix, only the why of the hint (the
/// fix itself says what to write).
fn tidy(mut d: Diag) -> Diag {
    d.msg = bare_web(&d.msg);
    d.hint = d.hint.map(|h| bare_web(&h));
    if !d.fix.is_empty() {
        d.hint = d.hint.map(|h| why_only(&h));
    }
    d
}

/// `web.hero` as `hero`, where `web.` starts a name.
fn bare_web(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find("web.") {
        let before = rest[..i].chars().next_back();
        let after = rest[i + 4..].chars().next();
        out.push_str(&rest[..i]);
        let starts_name = !before.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.' || c == '/');
        if !(starts_name && after.is_some_and(|c| c.is_ascii_lowercase())) {
            out.push_str("web.");
        }
        rest = &rest[i + 4..];
    }
    out.push_str(rest);
    out
}

/// The first part of a hint, up to its first `: ` or `; ` outside backticks and parentheses: the
/// reason, without the code that follows it (which a fix already writes). A hint without such a
/// break is kept.
fn why_only(h: &str) -> String {
    let (mut depth, mut code) = (0i32, false);
    let cs: Vec<char> = h.chars().collect();
    for (i, c) in cs.iter().enumerate() {
        match c {
            '`' => code = !code,
            '(' if !code => depth += 1,
            ')' if !code => depth -= 1,
            ':' | ';' if !code && depth == 0 && cs.get(i + 1) == Some(&' ') && i >= 8 => {
                return cs[..i].iter().collect();
            }
            _ => {}
        }
    }
    h.to_string()
}

/// The name a cascade hangs on: an unknown struct `Rect` is an unknown type in a signature and an
/// undefined function where a value is built, and both are the one missing `struct Rect`.
fn root_name(d: &Diag) -> Option<&str> {
    if !matches!(d.code, "E0102" | "E0202") {
        return None;
    }
    let name = d.msg.split('`').nth(1)?;
    name.starts_with(|c: char| c.is_ascii_uppercase()).then_some(name)
}

/// True if `b` is the same mistake as `a`, made again in another place.
fn same_mistake(a: &Diag, b: &Diag) -> bool {
    a.file == b.file
        && a.fix.is_empty() == b.fix.is_empty()
        && a.actual.is_none()
        && b.actual.is_none()
        && ((a.code == b.code && a.msg == b.msg) || root_name(a).is_some_and(|n| root_name(b) == Some(n)))
}

/// One diagnostic per mistake: the others of the same mistake are counted in its message (`and 2
/// more: lines 7, 9`), and their fixes join its fix (all of them or none are certain).
pub fn condense(diags: Vec<Diag>) -> Vec<Diag> {
    let mut out: Vec<(Diag, Vec<usize>)> = Vec::new();
    for d in diags {
        match out.iter_mut().find(|(o, _)| same_mistake(o, &d)) {
            Some((o, lines)) => {
                lines.push(d.span.line);
                for e in d.fix {
                    if !o.fix.contains(&e) {
                        o.fix.push(e);
                    }
                }
                // an undefined struct: the hint that names its fields
                if o.code != d.code && d.code == "E0202" && d.hint.is_some() {
                    o.hint = d.hint;
                }
            }
            None => out.push((d, Vec::new())),
        }
    }
    out.into_iter()
        .map(|(mut d, lines)| {
            if !lines.is_empty() {
                let mut shown: Vec<usize> = lines.clone();
                shown.dedup();
                let list: Vec<String> = shown.iter().take(6).map(usize::to_string).collect();
                let more = if shown.len() > 6 { ", ..." } else { "" };
                let s = if shown.len() == 1 { "" } else { "s" };
                d.msg = format!("{} (and {} more: line{s} {}{more})", d.msg, lines.len(), list.join(", "));
            }
            d
        })
        .collect()
}

/// The text the snippets come from: the original, when the program was repaired in memory.
fn shown_src(src: &str) -> String {
    crate::fix::origin().map_or_else(|| src.to_string(), |o| o.src.clone())
}

/// The errors for people (each mistake once, at its place in the original text).
pub fn render_human(diags: &[Diag], file: &str, src: &str) -> String {
    render(&prepare(diags), file, &shown_src(src), "error", &mut Vec::new())
}

/// The whole report of a program that does not compile, for people: the errors, the fixes applied
/// in memory, the warnings, and the count.
pub fn report(diags: &[Diag], file: &str, src: &str) -> String {
    let errs = prepare(diags);
    let text = shown_src(src);
    let mut seen = Vec::new();
    let mut out = render(&errs, file, &text, "error", &mut seen);
    out += &render_applied(&applied(), file);
    out += &render(&prepare(&warnings()), file, &text, "warning", &mut seen);
    out += &format!("nyra: {} error(s)\n", errs.len());
    out
}

/// The fixes applied in memory and the warnings of the last compilation, for people (empty when
/// there are none).
pub fn render_warnings(file: &str, src: &str) -> String {
    let mut seen = Vec::new();
    let fixes = applied();
    let mut out = render_applied(&fixes, file);
    out += &render(&prepare(&warnings()), file, &shown_src(src), "warning", &mut seen);
    if !fixes.is_empty() {
        out += &format!("nyra: {} fix(es) applied in memory; `--fix` writes them to {file}\n", fixes.len());
    }
    out
}

/// The fixes `repair_all` applied to the program being compiled, one per mistake.
pub fn applied() -> Vec<crate::fix::Applied> {
    crate::fix::origin().map(|o| condense_applied(&o.applied, &o.src)).unwrap_or_default()
}

/// One applied fix per mistake (see `condense`), with what its lines look like now.
fn condense_applied(applied: &[crate::fix::Applied], src: &str) -> Vec<crate::fix::Applied> {
    let mut diags: Vec<Diag> = applied.iter().map(|a| tidy(a.diag.clone())).collect();
    diags.sort_by_key(|d| (d.span.line, d.span.col));
    condense(diags)
        .into_iter()
        .map(|d| {
            let first = applied.iter().find(|a| a.diag.span == d.span && a.diag.code == d.code);
            let preview = crate::fix::preview(src, &d.fix).or_else(|| first.and_then(|a| a.preview.clone()));
            let line = first.map(|a| a.line.clone()).unwrap_or_default();
            crate::fix::Applied { diag: d, preview, line }
        })
        .collect()
}

fn render(diags: &[Diag], file: &str, src: &str, label: &str, seen: &mut Vec<&'static str>) -> String {
    let mut out = String::new();
    for d in diags {
        // a mistake in an imported file shows that file's line
        let other = d.file.as_ref().and_then(|f| std::fs::read_to_string(f).ok());
        let text = other.as_deref().unwrap_or(src);
        let lines: Vec<&str> = text.lines().collect();
        let file = d.file.as_deref().unwrap_or(file);
        out += &format!("{label}[{}]: {}\n", d.code, d.msg);
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
        if let Some(f) = crate::fix::preview(text, &d.fix) {
            out += &format!("  = fix: {f}\n");
        }
        // the explanation of a code once per report
        if !seen.contains(&d.code) {
            seen.push(d.code);
            out += &format!("  = explain: nyra explain {}\n", d.code);
        }
        out.push('\n');
    }
    out
}

/// The fixes that were applied automatically, for people: one warning each.
pub fn render_warnings_human(applied: &[crate::fix::Applied], file: &str) -> String {
    render_applied(applied, file)
}

fn render_applied(applied: &[crate::fix::Applied], file: &str) -> String {
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
/// `applied` text (what the changed lines look like now) and the `fix` edits (at their places in
/// the original text, so an agent can write them back).
pub fn render_json_warnings(applied: &[crate::fix::Applied], file: &str) -> String {
    format!("[{}]", applied_items(applied, file).join(","))
}

fn applied_items(applied: &[crate::fix::Applied], file: &str) -> Vec<String> {
    applied
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
        .collect()
}

pub fn render_json(diags: &[Diag], file: &str) -> String {
    let errs = prepare(diags);
    format!("{{\"ok\":{},\"errors\":{}{}}}", errs.is_empty(), json_errors(&errs, file), warnings_json(file))
}

/// `,"warnings":[..]` with the fixes applied in memory and the warnings of the last compilation, or
/// nothing when there are none.
pub fn warnings_json(file: &str) -> String {
    let mut items = applied_items(&applied(), file);
    let w = prepare(&warnings());
    let rest = json_errors(&w, file);
    if !w.is_empty() {
        items.push(rest[1..rest.len() - 1].to_string());
    }
    if items.is_empty() {
        return String::new();
    }
    format!(",\"warnings\":[{}]", items.join(","))
}

/// `,"warnings":[..]` with the compiler's warnings only (for results that list the applied fixes
/// on their own), or nothing.
#[cfg_attr(not(any(target_arch = "wasm32", test)), allow(dead_code))]
pub fn compiler_warnings_json(file: &str) -> String {
    let w = prepare(&warnings());
    if w.is_empty() {
        return String::new();
    }
    format!(",\"warnings\":{}", json_errors(&w, file))
}

/// The `errors` array of `render_json`.
pub fn render_json_errors(diags: &[Diag], file: &str) -> String {
    json_errors(&prepare(diags), file)
}

fn json_errors(diags: &[Diag], file: &str) -> String {
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
                json_str(d.file.as_deref().unwrap_or(file)),
                d.span.line,
                d.span.col,
                d.hint.as_deref().map(json_str).unwrap_or_else(|| "null".into()),
                fix_json(&d.fix)
            )
        })
        .collect();
    format!("[{}]", items.join(","))
}

/// `,"fix":[{"line":..,"col":..,"end_line":..,"end_col":..,"text":".."}],"exact":true`, or nothing
/// without a fix. Columns count characters; the end is exclusive. A fix is only given when it is
/// certain, so `exact` is always true: an agent may apply it without looking.
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
    format!(",\"fix\":[{}],\"exact\":true", edits.join(","))
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
