//! Self-repair: the fixes that diagnostics carry (`Diag::fix`), checked against the source,
//! applied, shown and repeated until the program compiles (`nyra check --fix`).
//!
//! A fix is only kept when every edit finds exactly the text it expects (`Edit::old`), so a
//! position that is off by a character can never damage a program. Every certain fix is applied,
//! also when other errors remain (`repair_all`): the errors that are left are reported at their
//! places in the original text, and the fixes as warnings that carry their edits. `--fix` writes
//! the fixes back, and never touches a program that already compiles.

use std::cell::RefCell;
use std::rc::Rc;

use crate::ast::Span;
use crate::diag::{Diag, Edit};

/// How often `repair_all` applies fixes and checks again: fixing one stage (characters, then
/// syntax, then types) can uncover the mistakes of the next. Each round compiles the whole
/// program again, and a round happens only while it has fixes to apply, so the rounds are few.
const ROUNDS: usize = 6;

/// The source as characters, with the offset where each line starts.
struct Text {
    chars: Vec<char>,
    starts: Vec<usize>,
}

impl Text {
    fn new(src: &str) -> Text {
        let chars: Vec<char> = src.chars().collect();
        let mut starts = vec![0];
        starts.extend(chars.iter().enumerate().filter(|(_, c)| **c == '\n').map(|(i, _)| i + 1));
        Text { chars, starts }
    }

    /// The character offset of a position: a column may point at the line break that ends its line.
    fn offset(&self, s: Span) -> Option<usize> {
        let line_start = *self.starts.get(s.line.checked_sub(1)?)?;
        let line_end = self.starts.get(s.line).map_or(self.chars.len(), |next| next - 1);
        let at = line_start + s.col.checked_sub(1)?;
        (at <= line_end).then_some(at)
    }

    /// The position of a character offset (the inverse of `offset`).
    fn span_of(&self, p: usize) -> Span {
        let line = self.starts.partition_point(|s| *s <= p).max(1);
        Span { line, col: p - self.starts[line - 1] + 1 }
    }

    /// The edits as character ranges, in source order. `None` if one of them is out of range,
    /// does not find the text it expects, or overlaps another.
    fn ranges<'a>(&self, edits: &[&'a Edit]) -> Option<Vec<(usize, usize, &'a Edit)>> {
        let mut out = Vec::new();
        for e in edits {
            let (a, b) = (self.offset(e.start)?, self.offset(e.end)?);
            if a > b {
                return None;
            }
            if !same_code(&self.chars[a..b], &e.old) {
                return None;
            }
            let mut before = self.chars[..a].iter().rev().filter(|c| !c.is_whitespace());
            if !e.after.chars().rev().filter(|c| !c.is_whitespace()).all(|c| before.next() == Some(&c)) {
                return None;
            }
            out.push((a, b, *e));
        }
        out.sort_by_key(|(a, b, _)| (*a, *b));
        out.windows(2).all(|w| w[0].1 <= w[1].0 && (w[0].0, w[0].1) != (w[1].0, w[1].1)).then_some(out)
    }

    fn apply(&self, ranges: &[(usize, usize, &Edit)], newline: &str) -> String {
        let mut out = String::new();
        let mut at = 0;
        for (a, b, e) in ranges {
            out.extend(&self.chars[at..*a]);
            out.push_str(&e.text.replace('\n', newline));
            at = *b;
        }
        out.extend(&self.chars[at..]);
        out
    }
}

/// True if the text holds `old`, apart from whitespace (an edit that spans several tokens does not
/// know how they are spaced; anything else, a comment for one, makes it not match).
fn same_code(text: &[char], old: &str) -> bool {
    text.iter().filter(|c| !c.is_whitespace()).eq(old.chars().filter(|c| !c.is_whitespace()).collect::<Vec<_>>().iter())
}

/// Drops every fix that does not match the source exactly. Called on all diagnostics before they
/// are shown, so a printed fix (human or JSON) always applies cleanly.
pub fn validate(diags: &mut [Diag], src: &str) {
    let text = Text::new(src);
    for d in diags.iter_mut().filter(|d| !d.fix.is_empty()) {
        let edits: Vec<&Edit> = d.fix.iter().collect();
        if text.ranges(&edits).is_none() {
            d.fix.clear();
        }
    }
}

/// The source with the edits applied, or `None` if they do not apply.
pub fn apply(src: &str, edits: &[Edit]) -> Option<String> {
    apply_with(src, edits, "\n")
}

/// `apply`, with `newline` for the line breaks the edits write.
fn apply_with(src: &str, edits: &[Edit], newline: &str) -> Option<String> {
    let text = Text::new(src);
    let edits: Vec<&Edit> = edits.iter().collect();
    Some(text.apply(&text.ranges(&edits)?, newline))
}

/// What the lines touched by a fix look like after it, for the `= fix:` line of an error:
/// `` `ret x` ``, or `` `a`, `b` (on separate lines) ``, or `` delete `;` ``.
pub fn preview(src: &str, fix: &[Edit]) -> Option<String> {
    let first = fix.iter().map(|e| e.start.line).min()?;
    // edits within lines, on lines apart (a function renamed with its calls): the lines they
    // change, not the lines between them
    let mut touched: Vec<usize> = fix.iter().map(|e| e.start.line).collect();
    touched.sort_unstable();
    touched.dedup();
    let within = fix.iter().all(|e| e.start.line == e.end.line && !e.text.contains('\n'));
    if within && touched.len() > 1 {
        let fixed = apply(src, fix)?;
        let lines: Vec<&str> = fixed.lines().collect();
        let shown: Vec<String> = touched.iter().take(3).map(|l| format!("`{}`", lines.get(l - 1).unwrap_or(&"").trim())).collect();
        let more = touched.len().saturating_sub(3);
        let nums: Vec<String> = touched.iter().take(3).map(usize::to_string).collect();
        return Some(match more {
            0 => format!("{} (lines {})", shown.join(", "), nums.join(", ")),
            n => format!("{} (lines {}) and {n} more line{}", shown.join(", "), nums.join(", "), if n == 1 { "" } else { "s" }),
        });
    }
    // an edit of whole lines (from the start of one line to the start of another) does not
    // touch the line it ends at
    let whole_lines =
        |e: &Edit| e.start.col == 1 && e.end.col == 1 && e.end.line > e.start.line && (e.text.is_empty() || e.text.ends_with('\n'));
    let last = fix.iter().map(|e| if whole_lines(e) { e.end.line - 1 } else { e.end.line }).max()?;
    let fixed = apply(src, fix)?;
    let added: usize = fix.iter().map(|e| e.text.matches('\n').count()).sum();
    let removed: usize = fix.iter().map(|e| e.end.line - e.start.line).sum();
    let count = (last + 1 - first + added).checked_sub(removed)?;
    let lines: Vec<&str> = fixed.lines().skip(first - 1).take(count).map(str::trim).filter(|l| !l.is_empty()).collect();
    Some(match lines.as_slice() {
        [] => {
            let old: Vec<&str> = src.lines().skip(first - 1).take(last + 1 - first).map(str::trim).collect();
            format!("delete `{}`", old.join(" ").trim())
        }
        [one] => format!("`{one}`"),
        many => format!("{} (on separate lines)", many.iter().map(|l| format!("`{l}`")).collect::<Vec<_>>().join(", ")),
    })
}

/// One fix that was applied: the error it repaired and its edits, at their places in the original
/// text; what the changed lines look like after it, and the line as it was.
#[derive(Debug, Clone)]
pub struct Applied {
    pub diag: Diag,
    pub preview: Option<String>,
    pub line: String,
}

/// A program that `--fix` made compile.
pub struct Repaired<T> {
    pub text: String,
    /// What `compile` returned for it.
    pub value: T,
    /// How many errors were fixed.
    pub fixed: usize,
    /// The fixes, in the order they were applied.
    pub applied: Vec<Applied>,
}

/// What `repair_all` made of a program: the text with every certain fix applied, what `compile`
/// says about that text (its errors are at their places in `text`; rendering moves them back to
/// the original, see `Origin`), and the fixes.
pub struct Outcome<T> {
    pub text: String,
    pub result: Result<T, Vec<Diag>>,
    /// How many errors were fixed.
    pub fixed: usize,
    /// The fixes, at their places in the original text.
    pub applied: Vec<Applied>,
}

/// Applies the fixes of `diags` (the errors of `src`), compiles again and repeats, a few rounds
/// at most. Returns the repaired program only if it compiles in the end. The repaired text is
/// the caller's new text, so no `Origin` is kept.
pub fn repair<T>(src: &str, diags: Vec<Diag>, compile: impl Fn(&str) -> Result<T, Vec<Diag>>) -> Option<Repaired<T>> {
    let out = repair_all(src, diags, compile);
    set_origin(None);
    let value = out.result.ok()?;
    Some(Repaired { text: out.text, value, fixed: out.fixed, applied: out.applied })
}

/// Applies every certain fix of `diags` (the errors of `src`), compiles again, and repeats while
/// that uncovers more certain fixes (a comma after a match arm hides every type error behind it),
/// `ROUNDS` at most. A fix that overlaps another waits for the next round. The errors without a
/// certain fix do not stop the others: the result has the errors that are left, of the repaired
/// text. When something was fixed, the `Origin` is recorded, so diagnostics are shown at their
/// places in `src`, with the fixes as warnings.
pub fn repair_all<T>(src: &str, diags: Vec<Diag>, compile: impl Fn(&str) -> Result<T, Vec<Diag>>) -> Outcome<T> {
    // keep the file's line breaks when a fix adds a line
    let newline = if src.contains("\r\n") { "\r\n" } else { "\n" };
    let mut current = src.to_string();
    let mut result: Result<T, Vec<Diag>> = Err(diags);
    let mut fixed = 0;
    // the text before each round, and the edits of each round: character ranges of that text,
    // with the length of what each wrote (to move a later round's positions back into `src`)
    let mut texts: Vec<String> = Vec::new();
    let mut rounds: Vec<Vec<(usize, usize, usize)>> = Vec::new();
    // the errors whose fixes were applied, with their round
    let mut done: Vec<(Diag, usize, Option<String>)> = Vec::new();
    for _ in 0..ROUNDS {
        let Err(diags) = &result else { break };
        let text = Text::new(&current);
        // the fixes of all errors, in order; one that overlaps an earlier fix waits for the next round
        let mut chosen: Vec<&Edit> = Vec::new();
        let mut round: Vec<&Diag> = Vec::new();
        for d in diags.iter().filter(|d| !d.fix.is_empty() && d.file.is_none()) {
            let fresh: Vec<&Edit> = d.fix.iter().filter(|e| !chosen.contains(e)).collect();
            let mut with: Vec<&Edit> = chosen.clone();
            with.extend(&fresh);
            if fresh.is_empty() || text.ranges(&with).is_some() {
                chosen = with;
                round.push(d);
            }
        }
        let Some(ranges) = (!chosen.is_empty()).then(|| text.ranges(&chosen)).flatten() else { break };
        for d in round {
            fixed += 1;
            done.push((d.clone(), rounds.len(), preview(&current, &d.fix)));
        }
        let next = text.apply(&ranges, newline);
        rounds.push(ranges.iter().map(|(a, b, e)| (*a, *b, e.text.replace('\n', newline).chars().count())).collect());
        texts.push(std::mem::replace(&mut current, next));
        result = compile(&current);
    }
    if rounds.is_empty() {
        set_origin(None);
        return Outcome { text: current, result, fixed, applied: Vec::new() };
    }
    let orig = Text::new(src);
    let applied = originals(src, &orig, &texts, &rounds, done, &current, newline);
    set_origin(Some(Origin { src: src.to_string(), orig, now: Text::new(&current), rounds, applied: applied.clone() }));
    Outcome { text: current, result, fixed, applied }
}

/// `repair_all` for callers that report the errors that are left at once: the repaired program,
/// or those errors (rendering puts them at their places in `src`).
pub fn repair_or_report<T>(
    src: &str,
    diags: Vec<Diag>,
    compile: impl Fn(&str) -> Result<T, Vec<Diag>>,
) -> Result<Repaired<T>, Vec<Diag>> {
    let out = repair_all(src, diags, compile);
    match out.result {
        Ok(value) => Ok(Repaired { text: out.text, value, fixed: out.fixed, applied: out.applied }),
        Err(left) => Err(left),
    }
}

/// The fixes that were applied, with their errors and edits moved to their places in `src`. The
/// edits of all of them together must turn `src` into `now`; when a later round edited text that
/// an earlier fix wrote, that is not so, and the first fix carries one edit for all of them.
fn originals(
    src: &str,
    orig: &Text,
    texts: &[String],
    rounds: &[Vec<(usize, usize, usize)>],
    done: Vec<(Diag, usize, Option<String>)>,
    now: &str,
    newline: &str,
) -> Vec<Applied> {
    let lines: Vec<&str> = src.lines().collect();
    let mut out: Vec<Applied> = done
        .into_iter()
        .map(|(d, k, preview)| {
            let d = back(d, orig, &rounds[..k], &Text::new(&texts[k]));
            let line = d.span.line.checked_sub(1).and_then(|i| lines.get(i)).unwrap_or(&"").to_string();
            Applied { diag: d, preview, line }
        })
        .collect();
    let all: Vec<Edit> = {
        let mut v: Vec<Edit> = Vec::new();
        for e in out.iter().flat_map(|a| a.diag.fix.iter()) {
            if !v.contains(e) {
                v.push(e.clone());
            }
        }
        v
    };
    if apply_with(src, &all, newline).as_deref() == Some(now) {
        return out;
    }
    // one edit from the first character that changed to the last
    let (a, b): (Vec<char>, Vec<char>) = (src.chars().collect(), now.chars().collect());
    let pre = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let max = a.len().min(b.len()) - pre;
    let suf = a.iter().rev().zip(b.iter().rev()).take(max).take_while(|(x, y)| x == y).count();
    let old: String = a[pre..a.len() - suf].iter().collect();
    let new: String = b[pre..b.len() - suf].iter().collect();
    let edit = Edit::range(orig.span_of(pre), orig.span_of(a.len() - suf), &old, new.replace(newline, "\n"));
    for (i, x) in out.iter_mut().enumerate() {
        x.diag.fix = if i == 0 { vec![edit.clone()] } else { Vec::new() };
    }
    out
}

/// The original text of a program that `repair_all` changed, and how its places moved: the
/// errors and warnings of the repaired text are shown at their places in the original, which is
/// the text the author (or agent) has.
pub struct Origin {
    pub src: String,
    orig: Text,
    now: Text,
    rounds: Vec<Vec<(usize, usize, usize)>>,
    /// The fixes that were applied, at their places in `src`.
    pub applied: Vec<Applied>,
}

thread_local! {
    /// The origin of the program being compiled (`None`: it was not changed).
    static ORIGIN: RefCell<Option<Rc<Origin>>> = const { RefCell::new(None) };
}

/// Records the origin of the program being compiled (`None` forgets it: the text compiled is the
/// author's, or the repaired text became the author's, as with `--fix`).
pub fn set_origin(o: Option<Origin>) {
    ORIGIN.with(|x| *x.borrow_mut() = o.map(Rc::new));
}

/// The origin of the program being compiled, if `repair_all` changed it.
pub fn origin() -> Option<Rc<Origin>> {
    ORIGIN.with(|x| x.borrow().clone())
}

/// A diagnostic of the program being compiled, at its place in the original text.
pub fn to_origin(d: Diag) -> Diag {
    match origin() {
        Some(o) => {
            let mut d = back(d, &o.orig, &o.rounds, &o.now);
            if !d.fix.is_empty() && d.file.is_none() {
                validate(std::slice::from_mut(&mut d), &o.src);
            }
            d
        }
        None => d,
    }
}

/// A place in the program being compiled (a runtime error's), at its place in the original text.
pub fn span_to_origin(s: Span) -> Span {
    match origin() {
        Some(o) if !o.rounds.is_empty() => back_span(s, &o.orig, &o.rounds, &o.now),
        _ => s,
    }
}

/// A diagnostic of the text `now` (which the edits of `rounds` made from the text `orig`), moved to
/// its place in `orig`. A place inside text that a fix wrote moves to the start of that fix.
fn back(d: Diag, orig: &Text, rounds: &[Vec<(usize, usize, usize)>], now: &Text) -> Diag {
    if rounds.is_empty() || d.file.is_some() {
        return d;
    }
    d.moved(|s: Span| back_span(s, orig, rounds, now))
}

/// A place of the text `now` at its place in `orig` (see `back`).
fn back_span(s: Span, orig: &Text, rounds: &[Vec<(usize, usize, usize)>], now: &Text) -> Span {
    let Some(mut p) = now.offset(s) else { return s };
    for edits in rounds.iter().rev() {
        p = before_edits(p, edits);
    }
    orig.span_of(p)
}

/// A character offset of the text after `edits` (character ranges of the text before them, with the
/// length of what each wrote), in the text before them.
fn before_edits(p: usize, edits: &[(usize, usize, usize)]) -> usize {
    let mut shift: isize = 0;
    for &(a, b, n) in edits {
        let start = (a as isize + shift) as usize;
        if p < start {
            break;
        }
        if p < start + n {
            return a;
        }
        shift += n as isize - (b - a) as isize;
    }
    (p as isize - shift).max(0) as usize
}

/// A unified diff of two texts without context lines: `@@ -3 +3 @@`, `-old line`, `+new line`.
pub fn diff(old: &str, new: &str) -> String {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    let pre = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let suf = a[pre..].iter().rev().zip(b[pre..].iter().rev()).take_while(|(x, y)| x == y).count();
    let (am, bm) = (&a[pre..a.len() - suf], &b[pre..b.len() - suf]);

    // longest common subsequence of the middle part (small: only the lines between the first and
    // the last change); a very large middle is shown as replaced as a whole
    let mut ops: Vec<(char, &str)> = Vec::new();
    if am.len() * bm.len() <= 4_000_000 {
        let w = bm.len() + 1;
        let mut lcs = vec![0u32; (am.len() + 1) * w];
        for i in (0..am.len()).rev() {
            for j in (0..bm.len()).rev() {
                lcs[i * w + j] =
                    if am[i] == bm[j] { lcs[(i + 1) * w + j + 1] + 1 } else { lcs[(i + 1) * w + j].max(lcs[i * w + j + 1]) };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < am.len() || j < bm.len() {
            if i < am.len() && j < bm.len() && am[i] == bm[j] {
                ops.push(('=', am[i]));
                (i, j) = (i + 1, j + 1);
            } else if j < bm.len() && (i == am.len() || lcs[i * w + j + 1] >= lcs[(i + 1) * w + j]) {
                ops.push(('+', bm[j]));
                j += 1;
            } else {
                ops.push(('-', am[i]));
                i += 1;
            }
        }
    } else {
        ops.extend(am.iter().map(|l| ('-', *l)));
        ops.extend(bm.iter().map(|l| ('+', *l)));
    }

    let mut out = String::new();
    let (mut la, mut lb) = (pre + 1, pre + 1);
    let mut k = 0;
    while k < ops.len() {
        if ops[k].0 == '=' {
            (la, lb, k) = (la + 1, lb + 1, k + 1);
            continue;
        }
        let end = ops[k..].iter().position(|(op, _)| *op == '=').map_or(ops.len(), |p| k + p);
        let hunk = &ops[k..end];
        let (dels, adds) = (hunk.iter().filter(|(op, _)| *op == '-').count(), hunk.iter().filter(|(op, _)| *op == '+').count());
        let range = |start: usize, n: usize| {
            if n == 1 {
                start.to_string()
            } else {
                format!("{},{n}", if n == 0 { start - 1 } else { start })
            }
        };
        out += &format!("@@ -{} +{} @@\n", range(la, dels), range(lb, adds));
        for (op, line) in hunk.iter().filter(|(op, _)| *op == '-').chain(hunk.iter().filter(|(op, _)| *op == '+')) {
            out += &format!("{op}{line}\n");
        }
        (la, lb, k) = (la + dels, lb + adds, end);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(line: usize, col: usize) -> Span {
        Span { line, col }
    }

    #[test]
    fn edits_apply_only_where_they_find_their_text() {
        let src = "fn main() {\n    return 1\n}\n";
        let ok = Edit::replace(at(2, 5), "return", "ret");
        assert_eq!(apply(src, std::slice::from_ref(&ok)).as_deref(), Some("fn main() {\n    ret 1\n}\n"));
        // one column off: the text there is not `return`, so nothing happens
        assert_eq!(apply(src, &[Edit::replace(at(2, 4), "return", "ret")]), None);
        // out of range
        assert_eq!(apply(src, &[Edit::replace(at(9, 1), "x", "y")]), None);
        // overlapping edits
        assert_eq!(apply(src, &[ok.clone(), Edit::replace(at(2, 6), "eturn", "x")]), None);
        assert_eq!(preview(src, &[ok]).as_deref(), Some("`ret 1`"));
    }

    #[test]
    fn a_range_may_hold_whitespace_around_the_expected_text() {
        let src = "fn main()\n{\n}\n";
        let join = Edit::range(at(1, 9), at(2, 1), ")", ") ");
        assert_eq!(apply(src, std::slice::from_ref(&join)).as_deref(), Some("fn main() {\n}\n"));
        assert_eq!(preview(src, &[join]).as_deref(), Some("`fn main() {`"));
        // a comment in the range is not whitespace
        assert_eq!(apply("fn main() // c\n{\n}\n", &[Edit::range(at(1, 9), at(2, 1), ")", ") ")]), None);
    }

    #[test]
    fn previews_of_split_and_deleted_lines() {
        let src = "fn main() {\n    let a = 1; let b = 2\n    ;\n}\n";
        let split = Edit::range(at(2, 14), at(2, 16), ";", "\n    ");
        assert_eq!(preview(src, &[split]).as_deref(), Some("`let a = 1`, `let b = 2` (on separate lines)"));
        assert_eq!(preview(src, &[Edit::replace(at(3, 5), ";", "")]).as_deref(), Some("delete `;`"));
    }

    #[test]
    fn repair_repeats_until_the_program_compiles() {
        // a toy compiler: every `x` must become `y`, one per round
        let compile = |s: &str| -> Result<(), Vec<Diag>> {
            match s.find('x') {
                None => Ok(()),
                Some(i) => {
                    let span = at(1, s[..i].chars().count() + 1);
                    Err(vec![Diag::new("E0001", "x", span).hint("y").fix(vec![Edit::replace(span, "x", "y")])])
                }
            }
        };
        let first = compile("axbx").unwrap_err();
        let r = repair("axbx", first, compile).unwrap();
        assert_eq!((r.text.as_str(), r.fixed), ("ayby", 2));
        // an error without a fix: nothing is repaired
        let stuck = |_: &str| -> Result<(), Vec<Diag>> { Err(vec![Diag::new("E0001", "no", at(1, 1))]) };
        assert!(repair("a", stuck("a").unwrap_err(), stuck).is_none());
    }

    #[test]
    fn diff_shows_the_changed_lines() {
        assert_eq!(diff("a\nb\nc\n", "a\nB\nc\n"), "@@ -2 +2 @@\n-b\n+B\n");
        assert_eq!(diff("a\nb;c\nd\n", "a\nb\nc\nd\n"), "@@ -2 +2,2 @@\n-b;c\n+b\n+c\n");
        assert_eq!(diff("f()\n{\n}\n", "f() {\n}\n"), "@@ -1,2 +1 @@\n-f()\n-{\n+f() {\n");
        assert_eq!(diff("same\n", "same\n"), "");
    }
}
