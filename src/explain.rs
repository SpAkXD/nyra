//! `nyra explain`: prints an entry of the error database, `docs/ERRORS.md`.
//!
//!     nyra explain E0201          what the error means, why the rule exists, causes, a wrong and a fixed program
//!     nyra explain E0201 --json   the same entry as JSON
//!     nyra explain                every code the compiler reports, with its title
//!     nyra explain --planned      also the codes that are only planned (designs, not in the compiler)
//!
//! The database is embedded at build time and parsed on demand, so there is one source of truth: the
//! Markdown file people read on GitHub is exactly what the command prints. The parser is strict (every
//! entry has the same fields in the same order) and the unit tests below fail if a malformed entry is
//! committed.

use std::process::ExitCode;

use crate::diag::{json_str, levenshtein};

/// The error database, embedded at build time.
const DB: &str = include_str!("../docs/ERRORS.md");

/// Width of the text printed for humans.
const WIDTH: usize = 96;

const FIELDS: [&str; 7] = ["Kind", "What it means", "Why Nyra has this rule", "Common causes", "Wrong", "Fixed", "Related"];

#[derive(Debug, Clone)]
pub struct Entry {
    pub code: String,
    pub title: String,
    /// `compile error` or `runtime error`
    pub kind: String,
    /// `v0.1`, or `planned for v0.6, not in the compiler yet`
    pub since: String,
    pub planned: bool,
    pub what: String,
    pub why: String,
    pub causes: Vec<String>,
    pub wrong: String,
    pub fixed: String,
    pub related: Vec<String>,
}

// ---- parsing docs/ERRORS.md ---------------------------------------------------------------------

fn is_code(s: &str) -> bool {
    s.len() == 5 && s.starts_with('E') && s[1..].chars().all(|c| c.is_ascii_digit())
}

/// `## E0201: undefined variable` -> (`E0201`, `undefined variable`)
fn heading(line: &str) -> Option<(&str, &str)> {
    let rest = line.strip_prefix("## ")?;
    let (code, title) = rest.split_once(": ")?;
    (is_code(code) && !title.trim().is_empty()).then_some((code, title.trim()))
}

/// Parses the whole database. Errors name the line that is wrong.
pub fn parse(db: &str) -> Result<Vec<Entry>, String> {
    // 1. split into entries: (line number of the heading, code, title, body lines with numbers)
    type Body<'a> = Vec<(usize, &'a str)>;
    let mut sections: Vec<(usize, &str, &str, Body)> = Vec::new();
    let mut in_fence = false;
    let mut open = false;
    for (i, line) in db.lines().enumerate() {
        let no = i + 1;
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
        } else if !in_fence && line.starts_with("## ") {
            match heading(line) {
                Some((code, title)) => {
                    sections.push((no, code, title, Vec::new()));
                    open = true;
                }
                None if sections.is_empty() => open = false,
                None => return Err(format!("line {no}: `{line}` is not an entry heading (`## E0xxx: title`)")),
            }
            continue;
        }
        if open {
            if let Some(s) = sections.last_mut() {
                s.3.push((no, line));
            }
        }
    }
    if in_fence {
        return Err("a code block is never closed".into());
    }

    // 2. parse every entry
    let mut entries: Vec<Entry> = Vec::new();
    for (head_no, code, title, body) in sections {
        entries.push(parse_entry(head_no, code, title, &body)?);
    }

    // 3. cross-checks
    for pair in entries.windows(2) {
        if pair[0].code >= pair[1].code {
            return Err(format!("{} must come before {}: entries are sorted by code, one entry per code", pair[1].code, pair[0].code));
        }
    }
    for e in &entries {
        for r in &e.related {
            if r == &e.code {
                return Err(format!("{}: Related lists the entry itself", e.code));
            }
            if !entries.iter().any(|x| &x.code == r) {
                return Err(format!("{}: Related mentions {r}, which has no entry", e.code));
            }
        }
    }
    Ok(entries)
}

fn parse_entry(head_no: usize, code: &str, title: &str, body: &[(usize, &str)]) -> Result<Entry, String> {
    // fields: `- **Label:** text`, then continuation lines until the next field
    // (line, label, text, the lines of a code block)
    type Field<'a> = (usize, &'a str, String, Vec<(usize, &'a str)>);
    let mut fields: Vec<Field> = Vec::new();
    let mut in_fence = false;
    for &(no, line) in body {
        let starts_field = !in_fence && line.starts_with("- **") && line.contains(":**");
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
        }
        if starts_field {
            let (label, rest) = line[4..].split_once(":**").unwrap_or((&line[4..], ""));
            fields.push((no, label, rest.trim().to_string(), Vec::new()));
        } else if let Some(f) = fields.last_mut() {
            f.3.push((no, line));
        } else if !line.trim().is_empty() {
            return Err(format!("{code} (line {no}): text before the first field: `{line}`"));
        }
    }
    let labels: Vec<&str> = fields.iter().map(|f| f.1).collect();
    if labels != FIELDS {
        return Err(format!("{code} (line {head_no}): the fields must be {FIELDS:?} in this order, found {labels:?}"));
    }

    // Kind and Since
    let (kind_no, _, kind_line, _) = &fields[0];
    let (kind, since) = kind_line
        .split_once(" · **Since:** ")
        .ok_or_else(|| format!("{code} (line {kind_no}): Kind must read `compile error · **Since:** v0.1`"))?;
    if kind != "compile error" && kind != "runtime error" {
        return Err(format!("{code} (line {kind_no}): the kind is `compile error` or `runtime error`, found `{kind}`"));
    }
    let planned = since.starts_with("planned for ");
    let version_ok = |v: &str| v.strip_prefix("v0.").is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
    let since_ok = if planned {
        since.strip_prefix("planned for ").and_then(|r| r.strip_suffix(", not in the compiler yet")).is_some_and(version_ok)
    } else {
        version_ok(since)
    };
    if !since_ok {
        return Err(format!(
            "{code} (line {kind_no}): Since is `v0.2` or `planned for v0.6, not in the compiler yet`, found `{since}`"
        ));
    }

    let text = |i: usize| -> Result<String, String> {
        let (no, label, rest, cont) = &fields[i];
        let mut t = rest.clone();
        for (_, l) in cont {
            if !l.trim().is_empty() {
                t.push(' ');
                t.push_str(l.trim());
            }
        }
        if t.trim().is_empty() {
            return Err(format!("{code} (line {no}): `{label}` is empty"));
        }
        Ok(t)
    };
    let what = text(1)?;
    let why = text(2)?;

    // Common causes: nested bullets
    let (causes_no, _, causes_rest, causes_body) = &fields[3];
    if !causes_rest.is_empty() {
        return Err(format!("{code} (line {causes_no}): `Common causes` is followed by a list, one `  - cause` per line"));
    }
    let mut causes = Vec::new();
    for (no, l) in causes_body {
        if l.trim().is_empty() {
            continue;
        }
        match l.trim_start().strip_prefix("- ") {
            Some(c) if l.starts_with(' ') => causes.push(c.trim().to_string()),
            _ => return Err(format!("{code} (line {no}): a cause is a line `  - text`, found `{l}`")),
        }
    }
    if causes.is_empty() {
        return Err(format!("{code} (line {causes_no}): `Common causes` needs at least one cause"));
    }

    let code_block = |i: usize| -> Result<String, String> {
        let (no, label, rest, cont) = &fields[i];
        if !rest.is_empty() {
            return Err(format!("{code} (line {no}): `{label}` is followed by a fenced code block, not text"));
        }
        let mut lines: Vec<&str> = Vec::new();
        let (mut open, mut closed, mut indent) = (false, false, 0usize);
        for (n, l) in cont {
            if l.trim_start().starts_with("```") {
                if open {
                    closed = true;
                    break;
                }
                open = true;
                indent = l.len() - l.trim_start().len();
            } else if open {
                lines.push(if l.len() >= indent && l.is_char_boundary(indent) { &l[indent..] } else { l.trim_start() });
            } else if !l.trim().is_empty() {
                return Err(format!("{code} (line {n}): only a code block may follow `{label}`, found `{l}`"));
            }
        }
        if !open || !closed {
            return Err(format!("{code} (line {no}): `{label}` needs a fenced code block"));
        }
        let text = lines.join("\n");
        if text.trim().is_empty() {
            return Err(format!("{code} (line {no}): the `{label}` code block is empty"));
        }
        Ok(text + "\n")
    };
    let wrong = code_block(4)?;
    let fixed = code_block(5)?;

    let (rel_no, _, rel_rest, rel_body) = &fields[6];
    if rel_body.iter().any(|(_, l)| !l.trim().is_empty()) {
        return Err(format!("{code} (line {rel_no}): `Related` is one line: codes separated by commas"));
    }
    let related: Vec<String> = rel_rest.split(',').map(|s| s.trim().to_string()).collect();
    if related.iter().any(|r| !is_code(r)) {
        return Err(format!("{code} (line {rel_no}): `Related` lists codes like `E0206, E0202`, found `{rel_rest}`"));
    }

    Ok(Entry {
        code: code.to_string(),
        title: title.to_string(),
        kind: kind.to_string(),
        since: since.to_string(),
        planned,
        what,
        why,
        causes,
        wrong,
        fixed,
        related,
    })
}

// ---- output -------------------------------------------------------------------------------------

/// Greedy word wrap. Text between backticks is never split.
fn wrap(text: &str, first: &str, rest: &str) -> String {
    let mut words: Vec<String> = Vec::new();
    let (mut cur, mut in_code) = (String::new(), false);
    for c in text.chars() {
        if c == '`' {
            in_code = !in_code;
        }
        if c.is_whitespace() && !in_code {
            if !cur.is_empty() {
                words.push(std::mem::take(&mut cur));
            }
        } else {
            cur.push(c);
        }
    }
    if !cur.is_empty() {
        words.push(cur);
    }
    let mut out = String::new();
    let mut line = first.to_string();
    let mut fresh = true;
    for w in words {
        if !fresh && line.chars().count() + 1 + w.chars().count() > WIDTH {
            out += line.trim_end();
            out.push('\n');
            line = rest.to_string();
            fresh = true;
        }
        if !fresh {
            line.push(' ');
        }
        line += &w;
        fresh = false;
    }
    out += line.trim_end();
    out.push('\n');
    out
}

fn indented(code: &str) -> String {
    code.lines().map(|l| if l.is_empty() { "\n".to_string() } else { format!("    {l}\n") }).collect()
}

fn render_entry(e: &Entry, all: &[Entry]) -> String {
    let mut out = format!("{}: {}\n", e.code, e.title);
    if e.planned {
        out += &format!(
            "PLANNED, NOT IN THE COMPILER: no version of nyra reports this code yet.\n{}, {} (this entry describes the design)\n",
            e.kind, e.since
        );
    } else {
        out += &format!("{}, since {}\n", e.kind, e.since);
    }
    out += "\nWhat it means\n";
    out += &wrap(&e.what, "  ", "  ");
    out += "\nWhy Nyra has this rule\n";
    out += &wrap(&e.why, "  ", "  ");
    out += "\nCommon causes\n";
    for c in &e.causes {
        out += &wrap(c, "  - ", "    ");
    }
    out += "\nWrong\n";
    out += &indented(&e.wrong);
    out += "\nFixed\n";
    out += &indented(&e.fixed);
    let related: Vec<String> = e
        .related
        .iter()
        .map(|r| match all.iter().find(|x| &x.code == r) {
            Some(x) => format!("{r} ({})", x.title),
            None => r.clone(),
        })
        .collect();
    out.push('\n');
    out += &wrap(&format!("Related: {}", related.join(", ")), "", "         ");
    out
}

/// The list of codes; the planned ones (designs, not in the compiler) only with `planned`.
fn render_list(all: &[Entry], planned: bool) -> String {
    let mut out = String::from("Nyra error codes. `nyra explain CODE` shows an entry; add --json for JSON.\n\n");
    let width = all.iter().map(|e| e.title.chars().count()).max().unwrap_or(0).min(56);
    type Section = (&'static str, fn(&Entry) -> bool);
    let sections: [Section; 3] = [
        ("compile errors", |e| !e.planned && e.kind == "compile error"),
        ("run-time errors (the program stops with exit code 101)", |e| !e.planned && e.kind == "runtime error"),
        ("planned, not in the compiler yet", |e| e.planned),
    ];
    for (heading, pick) in sections {
        if heading.starts_with("planned") && !planned {
            let n = all.iter().filter(|e| e.planned).count();
            out += &format!("({n} planned codes of future designs are not shown: `nyra explain --planned`)\n");
            continue;
        }
        out += &format!("{heading}\n");
        for e in all.iter().filter(|e| pick(e)) {
            let version = e.since.strip_prefix("planned for ").and_then(|s| s.split(',').next());
            let note = version.map(|v| format!("  [{v}]")).unwrap_or_default();
            out += format!("  {}  {:<width$}{note}", e.code, e.title).trim_end();
            out.push('\n');
        }
        out.push('\n');
    }
    out.trim_end().to_string() + "\n"
}

/// The list of codes as JSON; the planned ones only with `planned`.
pub fn list_json(all: &[Entry], planned: bool) -> String {
    let items: Vec<String> = all
        .iter()
        .filter(|e| planned || !e.planned)
        .map(|e| {
            format!(
                "{{\"code\":{},\"title\":{},\"kind\":{},\"since\":{},\"planned\":{}}}",
                json_str(&e.code),
                json_str(&e.title),
                json_str(&e.kind),
                json_str(&e.since),
                e.planned
            )
        })
        .collect();
    format!("{{\"codes\":[{}]}}", items.join(","))
}

pub fn entry_json(e: &Entry) -> String {
    let strs = |v: &[String]| v.iter().map(|s| json_str(s)).collect::<Vec<_>>().join(",");
    let note = if e.planned { ",\"note\":\"planned, not in the compiler: no version of nyra reports this code yet\"" } else { "" };
    format!(
        "{{\"code\":{},\"title\":{},\"kind\":{},\"since\":{},\"planned\":{}{note},\"what\":{},\"why\":{},\"causes\":[{}],\"wrong\":{},\"fixed\":{},\"related\":[{}]}}",
        json_str(&e.code),
        json_str(&e.title),
        json_str(&e.kind),
        json_str(&e.since),
        e.planned,
        json_str(&e.what),
        json_str(&e.why),
        strs(&e.causes),
        json_str(&e.wrong),
        json_str(&e.fixed),
        strs(&e.related)
    )
}

// ---- the command --------------------------------------------------------------------------------

const USAGE: &str = "\
usage: nyra explain [CODE] [--json]

  nyra explain E0201         what an error code means, why the rule exists, the usual causes,
                             and a wrong and a fixed program
  nyra explain               list every code the compiler reports, with its title
  nyra explain --planned     the list with the planned codes too (designs, not in the compiler)
  --json                     print JSON instead of text (for tools and AI agents)

The same text is in docs/ERRORS.md.
";

/// `E0201`, `e0201`, `0201` and `201` all name the same code.
pub fn normalize(arg: &str) -> String {
    let up = arg.trim().to_ascii_uppercase();
    let digits = up.strip_prefix('E').unwrap_or(&up);
    if !digits.is_empty() && digits.len() <= 4 && digits.chars().all(|c| c.is_ascii_digit()) {
        format!("E{digits:0>4}")
    } else {
        up
    }
}

/// The parsed error database (`docs/ERRORS.md`, embedded at build time).
pub fn database() -> Result<Vec<Entry>, String> {
    parse(DB).map_err(|e| format!("the error database (docs/ERRORS.md) is malformed: {e}"))
}

/// The nearest known code to a mistyped one (at most two edits away).
pub fn closest<'a>(code: &str, all: &'a [Entry]) -> Option<&'a Entry> {
    all.iter()
        .map(|e| (levenshtein(code, &e.code), e))
        .filter(|(d, _)| *d <= 2)
        .min_by_key(|(d, e)| (*d, e.code.clone()))
        .map(|(_, e)| e)
}

/// `{"ok":false,"error":"unknown error code `E9`","did_you_mean":"E0009"}`
pub fn unknown_json(arg: &str, close: Option<&Entry>) -> String {
    let close = close.map(|e| json_str(&e.code)).unwrap_or_else(|| "null".into());
    format!("{{\"ok\":false,\"error\":{},\"did_you_mean\":{close}}}", json_str(&format!("unknown error code `{arg}`")))
}

pub fn run(args: Vec<String>) -> ExitCode {
    let mut json = false;
    let mut planned = false;
    let mut codes: Vec<String> = Vec::new();
    for a in args {
        match a.as_str() {
            "-h" | "--help" | "help" => {
                print!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            "--json" => json = true,
            "--planned" => planned = true,
            _ if a.starts_with('-') => {
                eprintln!("nyra: unknown option `{a}`\n\n{USAGE}");
                return ExitCode::from(2);
            }
            _ => codes.push(a),
        }
    }
    if codes.len() > 1 {
        eprintln!("nyra: `nyra explain` takes one error code at a time\n\n{USAGE}");
        return ExitCode::from(2);
    }
    let all = match database() {
        Ok(all) => all,
        Err(e) => {
            eprintln!("nyra: {e}");
            return ExitCode::from(2);
        }
    };

    let Some(arg) = codes.first() else {
        print!("{}", if json { list_json(&all, planned) + "\n" } else { render_list(&all, planned) });
        return ExitCode::SUCCESS;
    };
    let code = normalize(arg);
    match all.iter().find(|e| e.code == code) {
        Some(e) => {
            print!("{}", if json { entry_json(e) + "\n" } else { render_entry(e, &all) });
            ExitCode::SUCCESS
        }
        None => {
            let close = closest(&code, &all);
            if json {
                println!("{}", unknown_json(arg, close));
            } else {
                eprintln!("nyra: unknown error code `{arg}`");
                if let Some(e) = close {
                    eprintln!("  did you mean `{}` ({})?", e.code, e.title);
                }
                eprintln!("  run `nyra explain` to list every code");
            }
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embedded_database_is_well_formed() {
        let all = parse(DB).unwrap_or_else(|e| panic!("docs/ERRORS.md: {e}"));
        assert!(all.len() >= 60, "found only {} entries", all.len());
        for code in ["E0001", "E0201", "E0241", "E0245", "E0220", "E0300"] {
            assert!(all.iter().any(|e| e.code == code), "{code} is missing");
        }
        let e = all.iter().find(|e| e.code == "E0201").unwrap();
        assert_eq!(e.title, "undefined variable");
        assert!(!e.planned && e.kind == "compile error" && e.since == "v0.1");
        assert!(e.wrong.contains("cout") && e.fixed.contains("count"));
        assert_eq!(e.related, ["E0202", "E0206", "E0205"]);
        assert!(!all.iter().find(|e| e.code == "E0220").unwrap().planned);
        assert!(all.iter().find(|e| e.code == "E0310").unwrap().planned);
        assert_eq!(all.iter().find(|e| e.code == "E0241").unwrap().kind, "runtime error");
    }

    #[test]
    fn a_malformed_entry_is_reported_with_its_code() {
        let bad = "## E0001: x\n- **Kind:** compile error · **Since:** v0.1\n- **What it means:** a\n";
        let err = parse(bad).unwrap_err();
        assert!(err.contains("E0001"), "{err}");
        let unsorted = DB.replacen("## E0002: unterminated string", "## E0000: unterminated string", 1);
        assert!(parse(&unsorted).is_err());
    }

    #[test]
    fn codes_are_normalized() {
        for s in ["E0201", "e0201", "0201", "201", " E201 "] {
            assert_eq!(normalize(s), "E0201", "{s}");
        }
        assert_eq!(normalize("hello"), "HELLO");
        assert_eq!(normalize("E12345"), "E12345");
    }

    #[test]
    fn long_text_wraps_without_splitting_code() {
        let t = "word ".repeat(40) + "`a b c` end";
        let w = wrap(&t, "  ", "  ");
        assert!(w.lines().all(|l| l.chars().count() <= WIDTH), "{w}");
        assert!(w.contains("`a b c`"));
    }
}
