//! Symbol-addressed editing: `nyra outline`, `nyra show` and `nyra edit`, and the MCP tools
//! `nyra_outline`, `nyra_show` and `nyra_edit`.
//!
//! An agent that changes one function of a long program sends that function by name, not the
//! whole file and not a diff. The symbols are the top-level functions and structs, and struct
//! fields written `Struct.field`.
//!
//! - Every edit replaces an exact range of the source, so the rest of the file stays
//!   byte-identical; inserted text gets the file's line breaks (`\n` or `\r\n`).
//! - The result must check: an edit that adds errors is refused (the file is not written) and the
//!   errors are reported. `--force` applies it anyway, `--fix` first runs the self-repair.
//! - Symbols are found from the tokens, so `outline`, `show` and `replace` also work on a file
//!   with errors (that is how a broken function gets repaired). A rename needs the whole parse:
//!   it changes only the definition and the real references (calls, constructions, type
//!   annotations, field reads and labels), never a string, a comment or another symbol's field.

use std::io::Read;
use std::process::ExitCode;

use crate::ast::{Expr, ExprKind, InterpPart, Program, Span, Stmt, StmtKind, Type};
use crate::diag::{self, Diag};
use crate::json::{obj, Json};
use crate::lexer::{self, Tok, Token};
use crate::{check, fix, parser};

// ---- the source and its lines ---------------------------------------------------------------------

/// Byte offsets of the line starts, to turn `Span`s (lines and character columns) into offsets.
struct Lines<'a> {
    text: &'a str,
    starts: Vec<usize>,
}

impl<'a> Lines<'a> {
    fn new(text: &'a str) -> Lines<'a> {
        let mut starts = vec![0];
        starts.extend(text.bytes().enumerate().filter(|(_, b)| *b == b'\n').map(|(i, _)| i + 1));
        Lines { text, starts }
    }

    /// The byte offset of a position (a column may point at the end of its line).
    fn offset(&self, s: Span) -> usize {
        let Some(&start) = s.line.checked_sub(1).and_then(|l| self.starts.get(l)) else {
            return self.text.len();
        };
        let line_end = self.content_end(start);
        self.text[start..line_end].char_indices().nth(s.col.saturating_sub(1)).map_or(line_end, |(i, _)| start + i)
    }

    /// The 1-based line of a byte offset.
    fn line(&self, at: usize) -> usize {
        self.starts.partition_point(|&s| s <= at).max(1)
    }

    fn line_start(&self, at: usize) -> usize {
        self.starts[self.line(at) - 1]
    }

    /// Where the text of the line holding `at` ends (before `\r\n` or `\n`).
    fn content_end(&self, at: usize) -> usize {
        let end = self.text[at..].find('\n').map_or(self.text.len(), |i| at + i);
        if end > at && self.text.as_bytes()[end - 1] == b'\r' {
            end - 1
        } else {
            end
        }
    }

    /// The start of the next line (after the line break), or the end of the text.
    fn next_line(&self, at: usize) -> usize {
        self.text[at..].find('\n').map_or(self.text.len(), |i| at + i + 1)
    }
}

/// `at` moved back over whitespace.
fn trim_back(text: &str, at: usize) -> usize {
    text[..at].trim_end().len()
}

fn is_blank(s: &str) -> bool {
    s.trim().is_empty()
}

// ---- the outline: symbols and their exact ranges ------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Fn,
    Struct,
}

impl Kind {
    fn word(self) -> &'static str {
        match self {
            Kind::Fn => "fn",
            Kind::Struct => "struct",
        }
    }
}

/// A top-level function or struct. Offsets are bytes of the source.
#[derive(Debug, Clone)]
pub struct Item {
    pub kind: Kind,
    /// Empty when the definition has no name (a syntax error).
    pub name: String,
    /// `fn` or `struct`.
    pub start: usize,
    /// After the last character of the definition, and of a comment on its last line.
    pub end: usize,
    /// Where the comment lines right above the definition start, or `start` without any.
    pub doc: usize,
    /// `fn area(r: Rect) -> int`, or `struct Point { x: int, y: int }`.
    pub sig: String,
    pub fields: Vec<FieldItem>,
    /// The `{` and `}` of a struct (`None` when the struct is cut short).
    pub braces: Option<(usize, usize)>,
}

#[derive(Debug, Clone)]
pub struct FieldItem {
    pub name: String,
    pub ty: String,
    /// The name's first byte; `end` is after the type.
    pub start: usize,
    pub end: usize,
}

/// The symbols of a file, and the statements at its top level (a script).
pub struct Outline {
    pub items: Vec<Item>,
    /// Each top-level statement: its start and end.
    pub script: Vec<(usize, usize)>,
}

struct Scan<'a> {
    text: &'a str,
    lines: Lines<'a>,
    toks: Vec<Token>,
}

impl Scan<'_> {
    fn at(&self, i: usize) -> usize {
        self.lines.offset(self.toks[i].span)
    }

    fn tok(&self, i: usize) -> &Tok {
        &self.toks[i.min(self.toks.len() - 1)].tok
    }

    /// True if token `i` is the first one on its line.
    fn first_on_line(&self, i: usize) -> bool {
        i == 0 || matches!(self.toks[i - 1].tok, Tok::Newline)
    }

    /// The end of the code before token `i` (whitespace and line breaks dropped).
    fn end_before(&self, i: usize) -> usize {
        trim_back(self.text, self.at(i).min(self.text.len()))
    }

    /// The end after token `i`.
    fn end_of(&self, i: usize) -> usize {
        let len = match &self.toks[i].tok {
            Tok::Ident(s) => s.len(),
            t => t.text().len().max(1),
        };
        (self.at(i) + len).min(self.text.len())
    }

    /// `end` moved over a `// comment` that follows it on the same line.
    fn with_comment(&self, end: usize) -> usize {
        let line_end = self.lines.content_end(end);
        if self.text[end..line_end].trim_start().starts_with("//") {
            trim_back(self.text, line_end)
        } else {
            end
        }
    }

    /// The start of the `//` lines right above the line of `start` (only when nothing comes
    /// before `start` on its line), else `start`.
    fn doc(&self, start: usize) -> usize {
        let line_start = self.lines.line_start(start);
        if !is_blank(&self.text[line_start..start]) {
            return start;
        }
        let mut doc = start;
        let mut l = self.lines.line(start) - 1;
        while l > 0 {
            let s = self.lines.starts[l - 1];
            if self.text[s..self.lines.content_end(s)].trim_start().starts_with("//") {
                doc = s;
                l -= 1;
            } else {
                break;
            }
        }
        doc
    }

    /// A `fn` at token `i`. Returns the item and the token after it.
    fn func(&self, i: usize) -> (Item, usize) {
        let start = self.at(i);
        let name = match self.tok(i + 1) {
            Tok::Ident(n) => n.clone(),
            _ => String::new(),
        };
        // the body: `{` or `=` outside the parameter list
        let mut j = i + 1;
        let mut depth = 0usize;
        while !matches!(self.tok(j), Tok::Eof | Tok::Newline) || depth > 0 {
            match self.tok(j) {
                Tok::LParen | Tok::LBracket => depth += 1,
                Tok::RParen | Tok::RBracket => depth = depth.saturating_sub(1),
                Tok::LBrace | Tok::Assign if depth == 0 => break,
                Tok::Eof => break,
                _ => {}
            }
            j += 1;
        }
        let sig = squash(&self.text[start..self.end_before(j)]);
        let (end, next) = match self.tok(j) {
            Tok::LBrace => self.block_end(j),
            Tok::Assign => {
                let k = self.line_end_tok(j + 1);
                (self.end_before(k), k)
            }
            _ => (self.end_before(j), j),
        };
        let end = self.with_comment(end);
        (Item { kind: Kind::Fn, name, start, end, doc: self.doc(start), sig, fields: Vec::new(), braces: None }, next)
    }

    /// The `}` that closes the `{` at token `j`: the end after it and the token after it. A `fn`
    /// or `struct` that starts a line inside means the block was cut short: it ends before that line.
    fn block_end(&self, j: usize) -> (usize, usize) {
        let mut depth = 0usize;
        let mut k = j;
        loop {
            match self.tok(k) {
                Tok::LBrace => depth += 1,
                Tok::RBrace => {
                    depth -= 1;
                    if depth == 0 {
                        return (self.end_of(k), k + 1);
                    }
                }
                Tok::Fn | Tok::Struct if self.first_on_line(k) => return (self.end_before(k), k),
                Tok::Eof => return (self.end_before(k), k),
                _ => {}
            }
            k += 1;
        }
    }

    /// The token that ends the line starting at `k` (a line break outside brackets, or the end).
    fn line_end_tok(&self, mut k: usize) -> usize {
        let mut depth = 0usize;
        loop {
            match self.tok(k) {
                Tok::LBrace | Tok::LBracket | Tok::LParen => depth += 1,
                Tok::RBrace | Tok::RBracket | Tok::RParen => depth = depth.saturating_sub(1),
                Tok::Newline if depth == 0 => return k,
                Tok::Fn | Tok::Struct if depth > 0 && self.first_on_line(k) => return k,
                Tok::Eof => return k,
                _ => {}
            }
            k += 1;
        }
    }

    /// A `struct` at token `i`.
    fn structure(&self, i: usize) -> (Item, usize) {
        let start = self.at(i);
        let name = match self.tok(i + 1) {
            Tok::Ident(n) => n.clone(),
            _ => String::new(),
        };
        let doc = self.doc(start);
        let mut item = Item { kind: Kind::Struct, name, start, end: start, doc, sig: String::new(), fields: Vec::new(), braces: None };
        if !matches!(self.tok(i + 2), Tok::LBrace) || item.name.is_empty() {
            let k = self.line_end_tok(i + 1);
            item.end = self.with_comment(self.end_before(k));
            item.sig = squash(&self.text[start..item.end]);
            return (item, k);
        }
        let open = self.at(i + 2);
        let mut k = i + 3;
        let next = loop {
            match self.tok(k) {
                Tok::Newline | Tok::Comma => k += 1,
                Tok::RBrace => {
                    item.braces = Some((open, self.at(k)));
                    item.end = self.end_of(k);
                    break k + 1;
                }
                Tok::Eof | Tok::Fn | Tok::Struct => {
                    item.end = self.end_before(k);
                    break k;
                }
                Tok::Ident(f) if matches!(self.tok(k + 1), Tok::Colon) => {
                    let fstart = self.at(k);
                    let mut t = k + 2;
                    let mut depth = 0usize;
                    let mut last = None;
                    loop {
                        match self.tok(t) {
                            Tok::LBracket => depth += 1,
                            Tok::RBracket if depth > 0 => depth -= 1,
                            Tok::Comma | Tok::Newline | Tok::RBrace | Tok::Eof if depth == 0 => break,
                            Tok::Eof | Tok::Fn | Tok::Struct => break,
                            _ => {}
                        }
                        last = Some(t);
                        t += 1;
                    }
                    let fend = last.map_or(self.end_of(k + 1), |l| self.end_of(l));
                    let ty = squash(&self.text[self.end_of(k + 1)..fend]);
                    item.fields.push(FieldItem { name: f.clone(), ty, start: fstart, end: fend });
                    k = t;
                }
                _ => k += 1,
            }
        };
        item.end = self.with_comment(item.end);
        let fields: Vec<String> = item.fields.iter().map(|f| format!("{}: {}", f.name, f.ty)).collect();
        item.sig = if fields.is_empty() {
            format!("struct {} {{}}", item.name)
        } else {
            format!("struct {} {{ {} }}", item.name, fields.join(", "))
        };
        (item, next)
    }
}

/// Runs of whitespace (line breaks too) as one space.
fn squash(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The symbols of `text`, found from its tokens, so a file with errors has an outline too.
pub fn outline(text: &str) -> Outline {
    let (toks, _) = lexer::lex(text);
    let scan = Scan { text, lines: Lines::new(text), toks };
    let mut out = Outline { items: Vec::new(), script: Vec::new() };
    let mut i = 0;
    while i < scan.toks.len() {
        match scan.tok(i) {
            Tok::Eof => break,
            Tok::Newline => i += 1,
            Tok::Fn => {
                let (item, next) = scan.func(i);
                out.items.push(item);
                i = next.max(i + 1);
            }
            Tok::Struct => {
                let (item, next) = scan.structure(i);
                out.items.push(item);
                i = next.max(i + 1);
            }
            _ => {
                let k = scan.line_end_tok(i);
                let (start, end) = (scan.at(i), scan.with_comment(scan.end_before(k)));
                out.script.push((start, end.max(start)));
                i = k.max(i + 1);
            }
        }
    }
    out
}

/// What a name refers to.
enum Target<'a> {
    Item(&'a Item),
    Field(&'a Item, &'a FieldItem),
}

impl Outline {
    fn named(&self, name: &str) -> Vec<&Item> {
        self.items.iter().filter(|i| i.name == name).collect()
    }

    /// The symbol `name` (`Struct.field` for a field), or why there is none.
    fn find(&self, name: &str, text: &str) -> Result<Target<'_>, String> {
        if let Some((s, f)) = name.split_once('.') {
            let Target::Item(item) = self.find(s, text)? else { unreachable!() };
            if item.kind != Kind::Struct {
                return Err(format!("`{s}` is a function, not a struct: only struct fields are written `Struct.field`"));
            }
            let field = item.fields.iter().find(|x| x.name == f).ok_or_else(|| {
                let names: Vec<&str> = item.fields.iter().map(|x| x.name.as_str()).collect();
                match diag::suggest(f, names.iter().copied()) {
                    Some(h) => format!("struct `{s}` has no field `{f}`: {h}"),
                    None => format!("struct `{s}` has no field `{f}`; its fields: {}", names.join(", ")),
                }
            })?;
            return Ok(Target::Field(item, field));
        }
        match self.named(name).as_slice() {
            [one] => Ok(Target::Item(one)),
            [] => {
                let lines = Lines::new(text);
                if name == "main" && !self.script.is_empty() {
                    let (s, _) = self.script[0];
                    return Err(format!(
                        "this file is a script: its top-level statements (from line {}) are not a symbol; replace them as text, or move them into `fn main() {{ ... }}`",
                        lines.line(s)
                    ));
                }
                let names: Vec<&str> = self.items.iter().map(|i| i.name.as_str()).collect();
                Err(match diag::suggest(name, names.iter().copied()) {
                    Some(h) => format!("no function or struct `{name}`: {h}"),
                    None if names.len() <= 20 => format!("no function or struct `{name}`; the file has: {}", names.join(", ")),
                    None => format!("no function or struct `{name}` in the file (`nyra outline` lists them)"),
                })
            }
            many => {
                let lines = Lines::new(text);
                let at: Vec<String> = many.iter().map(|i| lines.line(i.start).to_string()).collect();
                Err(format!("`{name}` is defined {} times (lines {}): rename or delete one as text first", many.len(), at.join(", ")))
            }
        }
    }

    /// True if the file separates its definitions with blank lines (or has too few to tell).
    fn spaced(&self, text: &str) -> bool {
        let gaps: Vec<&str> = self.items.windows(2).map(|w| &text[w[0].end.min(w[1].doc)..w[1].doc]).collect();
        gaps.is_empty() || gaps.iter().any(|g| g.matches('\n').count() >= 2)
    }
}

// ---- edits ------------------------------------------------------------------------------------------------

/// Where `add` puts new definitions.
#[derive(Debug, Clone, PartialEq)]
pub enum Place {
    End,
    After(String),
    Before(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// Definitions: each replaces the symbol of its name, or is added at the end.
    Upsert(String),
    /// `name` (or `Struct.field`) becomes the new definition.
    Replace(String, String),
    Add(String, Place),
    Delete(String),
    Rename(String, String),
    /// `struct`, `name: type`, after which field (default: the last).
    AddField(String, String, Option<String>),
}

/// `text` with `a..b` replaced by `with`.
fn splice(text: &str, a: usize, b: usize, with: &str) -> String {
    let mut out = String::with_capacity(text.len() + with.len());
    out.push_str(&text[..a]);
    out.push_str(with);
    out.push_str(&text[b..]);
    out
}

/// New code as sent: `\n` line breaks, the common indentation removed, no blank lines around it.
fn clean(code: &str) -> String {
    let code = code.replace("\r\n", "\n");
    let indent = code.lines().filter(|l| !is_blank(l)).map(|l| l.len() - l.trim_start().len()).min().unwrap_or(0);
    let lines: Vec<&str> = code.lines().map(|l| if l.len() >= indent { &l[indent..] } else { l.trim_start() }).collect();
    let text = lines.join("\n");
    let text = text.trim_end();
    // drop the blank lines in front, keep the indentation of the first line
    let first = text.find(|c: char| !c.is_whitespace()).unwrap_or(0);
    let from = text[..first].rfind('\n').map_or(0, |i| i + 1);
    text[from..].to_string()
}

/// A definition of new code: its name, kind, text (with its comment lines) and whether it has some.
struct Def {
    name: String,
    kind: Kind,
    text: String,
    has_doc: bool,
}

/// The definitions in new code. Anything else in it is a mistake: it would be lost.
fn defs(code: &str) -> Result<Vec<Def>, String> {
    let code = clean(code);
    if code.is_empty() {
        return Err("the new code is empty: to remove a symbol use `@delete name`".into());
    }
    let o = outline(&code);
    if let Some(&(s, e)) = o.script.first() {
        return Err(format!(
            "the new code may only hold `fn` and `struct` definitions, but it has `{}` on line {}",
            squash(&code[s..e]).chars().take(40).collect::<String>(),
            Lines::new(&code).line(s)
        ));
    }
    let mut out = Vec::new();
    for item in &o.items {
        if item.name.is_empty() {
            return Err(format!("a definition without a name: `{}`", item.sig));
        }
        out.push(Def {
            name: item.name.clone(),
            kind: item.kind,
            text: code[item.doc..item.end].to_string(),
            has_doc: item.doc < item.start,
        });
    }
    Ok(out)
}

/// A name, as one identifier token.
fn valid_name(name: &str) -> bool {
    let (toks, errs) = lexer::lex(name);
    errs.is_empty() && matches!(toks.first().map(|t| &t.tok), Some(Tok::Ident(n)) if n == name) && toks.len() <= 3
}

/// One applied edit: the new text, and what changed (`replaced fn area`) with the symbol it touched.
struct Applied {
    text: String,
    notes: Vec<(String, Option<String>)>,
}

fn apply(text: &str, op: &Op, nl: &str) -> Result<Applied, String> {
    let note = |n: String, sym: &str| (n, Some(sym.to_string()));
    match op {
        Op::Upsert(code) => {
            let mut text = text.to_string();
            let mut notes = Vec::new();
            for d in defs(code)? {
                let exists = !outline(&text).named(&d.name).is_empty();
                let op = if exists { Op::Replace(d.name.clone(), d.text) } else { Op::Add(d.text, Place::End) };
                let a = apply(&text, &op, nl)?;
                text = a.text;
                notes.extend(a.notes);
            }
            Ok(Applied { text, notes })
        }
        Op::Replace(name, code) => {
            let o = outline(text);
            match o.find(name, text)? {
                Target::Field(s, f) => {
                    let new = clean(code);
                    if new.contains('\n') || !new.contains(':') {
                        return Err(format!("a field is replaced by one line `name: type`, e.g. `{}: int`", f.name));
                    }
                    let fname = new.split(':').next().unwrap_or("").trim().to_string();
                    let text = splice(text, f.start, f.end, &new);
                    Ok(Applied {
                        text,
                        notes: vec![note(format!("replaced field {}.{}", s.name, f.name), &format!("{}.{fname}", s.name))],
                    })
                }
                Target::Item(item) => {
                    let mut ds = defs(code)?;
                    if ds.len() != 1 {
                        return Err(format!("`@replace {name}` takes one definition, found {}", ds.len()));
                    }
                    let d = ds.remove(0);
                    if d.name != *name {
                        return Err(format!(
                            "the new code defines `{}`, not `{name}`: to rename `{name}` everywhere use `@rename {name} {}`",
                            d.name, d.name
                        ));
                    }
                    let from = if d.has_doc { item.doc } else { item.start };
                    let text = splice(text, from, item.end, &d.text.replace('\n', nl));
                    Ok(Applied { text, notes: vec![note(format!("replaced {} {name}", d.kind.word()), name)] })
                }
            }
        }
        Op::Add(code, place) => {
            let ds = defs(code)?;
            let o = outline(text);
            for d in &ds {
                if let Some(old) = o.named(&d.name).first() {
                    return Err(format!(
                        "`{}` already exists (line {}): use `@replace {}`, or send the code without a command to replace it",
                        d.name,
                        Lines::new(text).line(old.start),
                        d.name
                    ));
                }
            }
            let spaced = o.spaced(text);
            let gap = if spaced { nl } else { "" };
            let block = ds.iter().map(|d| d.text.replace('\n', nl)).collect::<Vec<_>>().join(&format!("{nl}{gap}"));
            let lines = Lines::new(text);
            let (at, insert) = match place {
                Place::End => {
                    let mut pre = String::new();
                    if !text.is_empty() && !text.ends_with('\n') {
                        pre += nl;
                    }
                    if spaced && !text.trim().is_empty() {
                        // one blank line before, unless the file already ends with one
                        let tail = &text[trim_back(text, text.len())..];
                        if tail.matches('\n').count() < 2 {
                            pre += nl;
                        }
                    }
                    (text.len(), format!("{pre}{block}{nl}"))
                }
                Place::After(anchor) => {
                    let Target::Item(a) = o.find(anchor, text)? else {
                        return Err(
                            "`@add after` takes a function or struct; for a field use `@add-field Struct name: type after field`"
                                .into(),
                        );
                    };
                    let at = lines.next_line(a.end);
                    let pre = if at == text.len() && !text.ends_with('\n') { format!("{nl}{gap}") } else { gap.to_string() };
                    (at, format!("{pre}{block}{nl}"))
                }
                Place::Before(anchor) => {
                    let Target::Item(a) = o.find(anchor, text)? else {
                        return Err("`@add before` takes a function or struct".into());
                    };
                    let at = lines.line_start(a.doc);
                    (at, format!("{block}{nl}{gap}"))
                }
            };
            let text = splice(text, at, at, &insert);
            let notes = ds.iter().map(|d| note(format!("added {} {}", d.kind.word(), d.name), &d.name)).collect();
            Ok(Applied { text, notes })
        }
        Op::Delete(name) => {
            let o = outline(text);
            let lines = Lines::new(text);
            match o.find(name, text)? {
                Target::Field(s, f) => {
                    let (a, b) = field_range(text, &lines, s, f);
                    let text = splice(text, a, b, "");
                    Ok(Applied { text, notes: vec![(format!("deleted field {name}"), None)] })
                }
                Target::Item(item) => {
                    let (mut a, mut b) = (item.doc, item.end);
                    let alone = is_blank(&text[lines.line_start(a)..a]) && is_blank(&text[b..lines.content_end(b)]);
                    if alone {
                        a = lines.line_start(a);
                        b = lines.next_line(b);
                        // do not leave two blank lines where there was one on each side
                        let blank_before = text[..a].ends_with("\n\n") || text[..a].ends_with("\n\r\n");
                        let rest = &text[b..];
                        if blank_before && (rest.starts_with('\n') || rest.starts_with("\r\n")) {
                            b = lines.next_line(b);
                        } else if blank_before && b == text.len() {
                            a = lines.line_start(a - 1);
                        }
                    }
                    let text = splice(text, a, b, "");
                    Ok(Applied { text, notes: vec![(format!("deleted {} {name}", item.kind.word()), None)] })
                }
            }
        }
        Op::Rename(name, new) => rename(text, name, new),
        Op::AddField(s, field, after) => add_field(text, s, field, after.as_deref(), nl),
    }
}

/// The text a deleted field takes with it: its line when it is alone there, else the field and
/// one comma next to it.
fn field_range(text: &str, lines: &Lines, s: &Item, f: &FieldItem) -> (usize, usize) {
    let line_start = lines.line_start(f.start);
    let after = &text[f.end..lines.content_end(f.end)];
    let rest = after.trim_start().strip_prefix(',').unwrap_or(after.trim_start()).trim_start();
    if is_blank(&text[line_start..f.start]) && (rest.is_empty() || rest.starts_with("//")) {
        return (line_start, lines.next_line(f.end));
    }
    // a following comma (and the spaces after it)
    let tail = &text[f.end..];
    let ws = tail.len() - tail.trim_start_matches([' ', '\t']).len();
    if tail[ws..].starts_with(',') {
        let after_comma = &tail[ws + 1..];
        let sp = after_comma.len() - after_comma.trim_start_matches([' ', '\t']).len();
        return (f.start, f.end + ws + 1 + sp);
    }
    // the last field: the comma before it
    let head = &text[..f.start];
    let before = head.trim_end_matches([' ', '\t']);
    if before.ends_with(',') && s.fields.len() > 1 {
        return (before.len() - 1, f.end);
    }
    // the only field of a one-line struct: `struct P { x: int }` -> `struct P {}`
    if let (Some((open, close)), 1) = (s.braces, s.fields.len()) {
        if lines.line(open) == lines.line(close) {
            return (open + 1, close);
        }
    }
    (f.start, f.end)
}

fn add_field(text: &str, s: &str, field: &str, after: Option<&str>, nl: &str) -> Result<Applied, String> {
    let field = squash(field);
    let fname = field.split(':').next().unwrap_or("").trim().to_string();
    if !field.contains(':') || !valid_name(&fname) {
        return Err(format!("a new field is written `name: type`, e.g. `@add-field {s} count: int`, found `{field}`"));
    }
    let o = outline(text);
    let Target::Item(item) = o.find(s, text)? else {
        return Err(format!("`@add-field` takes a struct name, found `{s}`"));
    };
    if item.kind != Kind::Struct {
        return Err(format!("`{s}` is a function, not a struct"));
    }
    if item.fields.iter().any(|f| f.name == fname) {
        return Err(format!("struct `{s}` already has a field `{fname}`: to change its type use `@replace {s}.{fname}`"));
    }
    let Some((open, close)) = item.braces else {
        return Err(format!("struct `{s}` has no closing `}}`: replace the whole struct instead"));
    };
    let lines = Lines::new(text);
    let anchor = match after {
        Some(a) => Some(item.fields.iter().find(|f| f.name == a).ok_or_else(|| format!("struct `{s}` has no field `{a}`"))?),
        None => item.fields.last(),
    };
    let note = vec![(format!("added field {s}.{fname}"), Some(format!("{s}.{fname}")))];
    let multi = lines.line(open) != lines.line(close);
    let text = match (anchor, multi) {
        (None, false) => splice(text, open + 1, close, &format!(" {field} ")),
        (None, true) => {
            let at = lines.line_start(close);
            let indent = "    ";
            splice(text, at, at, &format!("{indent}{field}{nl}"))
        }
        (Some(a), false) => splice(text, a.end, a.end, &format!(", {field}")),
        (Some(a), true) => {
            let tail = &text[a.end..lines.content_end(a.end)];
            let has_comma = tail.trim_start().starts_with(',');
            let line_start = lines.line_start(a.start);
            let starts_line = is_blank(&text[line_start..a.start]);
            let indent = if starts_line { &text[line_start..a.start] } else { "    " };
            // comma style: every field but the last is followed by a comma
            let commas = item.fields.iter().any(|f| text[f.end..].trim_start_matches([' ', '\t']).starts_with(','));
            let at = lines.next_line(a.end);
            let line = format!("{indent}{field}{}{nl}", if has_comma { "," } else { "" });
            let text = splice(text, at, at, &line);
            if commas && !has_comma {
                splice(&text, a.end, a.end, ",")
            } else {
                text
            }
        }
    };
    Ok(Applied { text, notes: note })
}

// ---- rename -----------------------------------------------------------------------------------------------

/// Every expression of the statements, outer ones first.
fn walk_stmts(stmts: &[Stmt], f: &mut dyn FnMut(&Expr)) {
    for s in stmts {
        match &s.kind {
            StmtKind::Let { value, .. } => walk(value, f),
            StmtKind::Assign { target, value, .. } => {
                walk(target, f);
                walk(value, f);
            }
            StmtKind::If { cond, then, els } => {
                walk(cond, f);
                walk_stmts(then, f);
                if let Some(e) = els {
                    walk_stmts(e, f);
                }
            }
            StmtKind::While { cond, body } => {
                walk(cond, f);
                walk_stmts(body, f);
            }
            StmtKind::For { start, end, step, body, .. } => {
                walk(start, f);
                walk(end, f);
                if let Some(s) = step {
                    walk(s, f);
                }
                walk_stmts(body, f);
            }
            StmtKind::ForEach { iter, body, .. } => {
                walk(iter, f);
                walk_stmts(body, f);
            }
            StmtKind::Arena(body) => walk_stmts(body, f),
            StmtKind::Match { scrut, arms } => {
                walk(scrut, f);
                for arm in arms {
                    arm.pats.iter().for_each(|p| walk(p, f));
                    walk_stmts(&arm.body, f);
                }
            }
            StmtKind::Ret(Some(e)) | StmtKind::Expr(e) => walk(e, f),
            _ => {}
        }
    }
}

fn walk(e: &Expr, f: &mut dyn FnMut(&Expr)) {
    f(e);
    match &e.kind {
        ExprKind::Interp(parts) => {
            for p in parts {
                if let InterpPart::Expr(x) = p {
                    walk(x, f);
                }
            }
        }
        ExprKind::Unary(_, x) | ExprKind::Field(x, _) | ExprKind::Labeled(_, x) | ExprKind::Inout(x) => walk(x, f),
        ExprKind::Binary(_, a, b) | ExprKind::Index(a, b) => {
            walk(a, f);
            walk(b, f);
        }
        ExprKind::Call(_, args) | ExprKind::Array(args) => args.iter().for_each(|a| walk(a, f)),
        ExprKind::Method(r, _, args) => {
            walk(r, f);
            args.iter().for_each(|a| walk(a, f));
        }
        ExprKind::If(c, a, b) => {
            walk(c, f);
            walk(a, f);
            walk(b, f);
        }
        _ => {}
    }
}

/// The program parsed, or the first syntax error.
fn parse(text: &str) -> Result<(Program, Vec<Token>), String> {
    let first = |ds: Vec<Diag>| {
        let d = &ds[0];
        format!(
            "a rename needs a file without syntax errors; line {}: {} (fix it first, e.g. by replacing that symbol)",
            d.span.line, d.msg
        )
    };
    let (toks, errs) = lexer::lex(text);
    if !errs.is_empty() {
        return Err(first(errs));
    }
    let (prog, errs) = parser::parse(toks.clone());
    if !errs.is_empty() {
        return Err(first(errs));
    }
    Ok((prog, toks))
}

fn rename(text: &str, name: &str, new: &str) -> Result<Applied, String> {
    if !valid_name(new) {
        return Err(format!("`{new}` is not a valid name: use letters, digits and `_`, not a keyword"));
    }
    let o = outline(text);
    let target = o.find(name, text)?;
    let (mut prog, toks) = parse(text)?;
    let lines = Lines::new(text);
    let mut sites: Vec<Span> = Vec::new();
    let (old, what) = match target {
        Target::Field(s, f) => {
            if s.fields.iter().any(|x| x.name == new) {
                return Err(format!("struct `{}` already has a field `{new}`", s.name));
            }
            // field reads need the types of their bases: errors elsewhere do not matter here
            let _ = check::check(&mut prog);
            let st = Type::structure(&s.name);
            for sd in prog.structs.iter().filter(|sd| sd.name == s.name) {
                sites.extend(sd.fields.iter().filter(|x| x.name == f.name).map(|x| x.span));
            }
            for func in &prog.funcs {
                walk_stmts(&func.body, &mut |e| match &e.kind {
                    ExprKind::Field(base, n) if *n == f.name && base.ty == st => sites.push(e.span),
                    ExprKind::Call(c, args) if *c == s.name => {
                        for a in args {
                            if matches!(&a.kind, ExprKind::Labeled(l, _) if *l == f.name) {
                                sites.push(a.span);
                            }
                        }
                    }
                    _ => {}
                });
            }
            (f.name.clone(), format!("field {}.{}", s.name, f.name))
        }
        Target::Item(item) => {
            if let Some(t) = toks.iter().find(|t| t.tok == Tok::Ident(new.to_string())) {
                return Err(format!("`{new}` is already used on line {}: pick a name the file does not use", t.span.line));
            }
            if item.kind == Kind::Fn {
                sites.extend(prog.funcs.iter().filter(|f| f.name == name).map(|f| f.span));
            } else {
                sites.extend(prog.structs.iter().filter(|s| s.name == name).map(|s| s.span));
                // type annotations: `p: Point`, `-> Point`, `[Point]` (the AST keeps no positions for types)
                for (k, t) in toks.iter().enumerate() {
                    let typed = k > 0 && matches!(toks[k - 1].tok, Tok::Colon | Tok::Arrow | Tok::LBracket);
                    let call = matches!(toks.get(k + 1).map(|t| &t.tok), Some(Tok::LParen));
                    if t.tok == Tok::Ident(name.to_string()) && typed && !call {
                        sites.push(t.span);
                    }
                }
            }
            // calls and constructions, also inside `{ }` of strings
            for func in &prog.funcs {
                walk_stmts(&func.body, &mut |e| {
                    if matches!(&e.kind, ExprKind::Call(c, _) if c == name) {
                        sites.push(e.span);
                    }
                });
            }
            (name.to_string(), format!("{} {name}", item.kind.word()))
        }
    };
    let mut at: Vec<usize> = sites.iter().map(|s| lines.offset(*s)).collect();
    at.sort_unstable();
    at.dedup();
    for &a in &at {
        if text.get(a..a + old.len()) != Some(old.as_str()) {
            return Err(format!("internal error: line {} does not hold `{old}`; please report this bug", lines.line(a)));
        }
    }
    let mut out = text.to_string();
    for &a in at.iter().rev() {
        out = splice(&out, a, a + old.len(), new);
    }
    let refs = at.len().saturating_sub(1);
    let sym = match name.split_once('.') {
        Some((s, _)) => format!("{s}.{new}"),
        None => new.to_string(),
    };
    let note = format!("renamed {what} -> {new}, {refs} reference{}", if refs == 1 { "" } else { "s" });
    Ok(Applied { text: out, notes: vec![(note, Some(sym))] })
}

// ---- the edit script ------------------------------------------------------------------------------------

/// Reads an edit script. Code without a command is an upsert: each definition replaces the symbol
/// of its name, or is added at the end.
///
/// ```text
/// @replace NAME          the next lines: the new definition (NAME may be Struct.field)
/// @add [after|before NAME]   the next lines: new definitions (default: at the end)
/// @delete NAME           a function, a struct or Struct.field
/// @rename NAME NEW       the definition and every reference (NAME may be Struct.field)
/// @add-field STRUCT name: type [after FIELD]
/// ```
pub fn parse_script(script: &str) -> Result<Vec<Op>, String> {
    let script = script.replace("\r\n", "\n");
    if !script.trim_start().starts_with('@') {
        return Ok(vec![Op::Upsert(script)]);
    }
    let mut ops = Vec::new();
    let mut lines = script.lines().peekable();
    while let Some(line) = lines.next() {
        if is_blank(line) {
            continue;
        }
        let Some(cmd) = line.strip_prefix('@') else {
            return Err(format!("expected a command starting with `@`, found `{}`", line.trim()));
        };
        let mut body = String::new();
        while let Some(next) = lines.peek() {
            if next.starts_with('@') {
                break;
            }
            body.push_str(next);
            body.push('\n');
            lines.next();
        }
        let words: Vec<&str> = cmd.split_whitespace().collect();
        let no_body = |op: Op| if is_blank(&body) { Ok(op) } else { Err(format!("`@{}` takes no code after it", words[0])) };
        let op = match words.as_slice() {
            ["replace" | "set", name, rest @ ..] => {
                let code = if rest.is_empty() { body.clone() } else { format!("{}\n{body}", rest.join(" ")) };
                Op::Replace(name.to_string(), code)
            }
            ["add"] => Op::Add(body.clone(), Place::End),
            ["add", "after", name] => Op::Add(body.clone(), Place::After(name.to_string())),
            ["add", "before", name] => Op::Add(body.clone(), Place::Before(name.to_string())),
            ["upsert"] => Op::Upsert(body.clone()),
            ["delete" | "remove", name] => no_body(Op::Delete(name.to_string()))?,
            ["rename", name, new] | ["rename", name, "->", new] => no_body(Op::Rename(name.to_string(), new.to_string()))?,
            ["add-field", s, rest @ ..] if !rest.is_empty() => {
                let rest = rest.join(" ");
                let (field, after) = match rest.rsplit_once(" after ") {
                    Some((f, a)) => (f.to_string(), Some(a.trim().to_string())),
                    None => (rest, None),
                };
                no_body(Op::AddField(s.to_string(), field, after))?
            }
            _ => {
                return Err(format!(
                    "unknown command `@{cmd}`: use @replace NAME, @add [after|before NAME], @delete NAME, @rename NAME NEW or @add-field STRUCT name: type"
                ))
            }
        };
        ops.push(op);
    }
    if ops.is_empty() {
        return Err("no edits given".into());
    }
    Ok(ops)
}

// ---- running a batch --------------------------------------------------------------------------------------

/// What a batch of edits did.
pub struct Outcome {
    pub text: String,
    /// `replaced fn area (lines 12-15)`, ...
    pub notes: Vec<String>,
    /// Errors the edits added: the batch is refused unless forced.
    pub added: Vec<Diag>,
    /// All errors of the result.
    pub errors: Vec<Diag>,
    /// How many errors `--fix` repaired.
    pub fixed: usize,
}

fn errors_of(src: &str) -> Vec<Diag> {
    crate::compile(src).err().unwrap_or_default()
}

/// Applies the edits in order (each sees the result of the one before), then checks the result.
/// With `fix`, errors that have a certain fix are repaired first. The errors that were not in the
/// original (matched by code and message, since positions move) are `added`.
pub fn run_edits(src: &str, ops: &[Op], fix_errors: bool) -> Result<Outcome, String> {
    let nl = if src.contains("\r\n") { "\r\n" } else { "\n" };
    let mut text = src.to_string();
    let mut notes = Vec::new();
    for op in ops {
        let a = apply(&text, op, nl)?;
        text = a.text;
        notes.extend(a.notes);
    }
    let mut errors = errors_of(&text);
    let mut fixed = 0;
    if fix_errors && !errors.is_empty() {
        if let Some(r) = fix::repair(&text, errors.clone(), crate::compile) {
            text = r.text;
            fixed = r.fixed;
            errors.clear();
        }
    }
    let mut before: Vec<(&str, String)> = errors_of(src).into_iter().map(|d| (d.code, d.msg)).collect();
    let added = errors
        .iter()
        .filter(|d| match before.iter().position(|(c, m)| *c == d.code && *m == d.msg) {
            Some(i) => {
                before.remove(i);
                false
            }
            None => true,
        })
        .cloned()
        .collect();
    // where each touched symbol is now
    let o = outline(&text);
    let lines = Lines::new(&text);
    let notes = notes
        .into_iter()
        .map(|(n, sym)| {
            let at = sym.and_then(|s| match o.find(&s, &text).ok()? {
                Target::Item(i) => Some((lines.line(i.start), lines.line(i.end))),
                Target::Field(_, f) => Some((lines.line(f.start), lines.line(f.start))),
            });
            match at {
                Some((a, b)) if a == b => format!("{n} (line {a})"),
                Some((a, b)) => format!("{n} (lines {a}-{b})"),
                None => n,
            }
        })
        .collect();
    Ok(Outcome { text, notes, added, errors, fixed })
}

/// The symbol whose definition holds `line` (for errors in the result).
fn symbol_at(o: &Outline, lines: &Lines, line: usize) -> Option<String> {
    o.items.iter().find(|i| lines.line(i.doc) <= line && line <= lines.line(i.end)).map(|i| format!("{} {}", i.kind.word(), i.name))
}

fn errors_json(diags: &[Diag], text: &str) -> Json {
    let o = outline(text);
    let lines = Lines::new(text);
    diags
        .iter()
        .map(|d| {
            let mut f = vec![
                ("code", Json::from(d.code)),
                ("message", d.msg.clone().into()),
                ("line", (d.span.line as i64).into()),
                ("col", (d.span.col as i64).into()),
            ];
            if let Some(h) = &d.hint {
                f.push(("hint", h.clone().into()));
            }
            if let Some(s) = symbol_at(&o, &lines, d.span.line) {
                f.push(("symbol", s.into()));
            }
            Json::Obj(f.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
        })
        .collect::<Vec<_>>()
        .into()
}

/// The JSON answer for an outcome: `{"ok":true,"edits":[...]}` or the refusal with the errors.
fn outcome_json(out: &Outcome, applied: bool, code: bool) -> Json {
    let mut f: Vec<(&str, Json)> = vec![("ok", applied.into())];
    if !applied {
        f.push((
            "refused",
            format!("the edit adds {} error(s); nothing was changed (force applies it anyway)", out.added.len()).into(),
        ));
    }
    f.push(("edits", out.notes.iter().map(|n| Json::from(n.as_str())).collect::<Vec<_>>().into()));
    if out.fixed > 0 {
        f.push(("fixed", (out.fixed as i64).into()));
    }
    let shown = if applied { &out.errors } else { &out.added };
    if !shown.is_empty() {
        f.push(("errors", errors_json(shown, &out.text)));
    }
    if applied && code {
        f.push(("code", out.text.clone().into()));
    }
    Json::Obj(f.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

// ---- outline and show as text and JSON ----------------------------------------------------------------------

fn range(a: usize, b: usize) -> String {
    if a == b {
        a.to_string()
    } else {
        format!("{a}-{b}")
    }
}

/// One line per symbol: `6-9 fn area(r: Rect) -> int`.
pub fn outline_text(text: &str, file: &str) -> String {
    let o = outline(text);
    let lines = Lines::new(text);
    let n = text.lines().count();
    let mut out = format!("{file}: {n} lines\n");
    for i in o.items.iter().filter(|i| !i.name.is_empty()) {
        out += &format!("{} {}\n", range(lines.line(i.start), lines.line(i.end)), i.sig);
    }
    if !o.script.is_empty() {
        out += &format!("{} script ({} statements)\n", script_ranges(&o, &lines).join(","), o.script.len());
    }
    out
}

/// The line ranges of the top-level statements, consecutive ones merged.
fn script_ranges(o: &Outline, lines: &Lines) -> Vec<String> {
    let mut runs: Vec<(usize, usize)> = Vec::new();
    for &(s, e) in &o.script {
        let (a, b) = (lines.line(s), lines.line(e));
        match runs.last_mut() {
            Some(r) if a <= r.1 + 1 && !o.items.iter().any(|i| lines.line(i.start) > r.1 && lines.line(i.start) < a) => r.1 = b,
            _ => runs.push((a, b)),
        }
    }
    runs.into_iter().map(|(a, b)| range(a, b)).collect()
}

pub fn outline_json(text: &str, file: &str) -> Json {
    let o = outline(text);
    let lines = Lines::new(text);
    let mut syms: Vec<Json> = Vec::new();
    for i in o.items.iter().filter(|i| !i.name.is_empty()) {
        let mut f: Vec<(&str, Json)> = vec![
            ("kind", i.kind.word().into()),
            ("name", i.name.clone().into()),
            ("line", (lines.line(i.start) as i64).into()),
            ("end_line", (lines.line(i.end) as i64).into()),
        ];
        match i.kind {
            Kind::Fn => f.push(("sig", i.sig.clone().into())),
            Kind::Struct => f.push((
                "fields",
                i.fields
                    .iter()
                    .map(|x| {
                        obj([
                            ("name", x.name.clone().into()),
                            ("type", x.ty.clone().into()),
                            ("line", (lines.line(x.start) as i64).into()),
                        ])
                    })
                    .collect::<Vec<_>>()
                    .into(),
            )),
        }
        syms.push(Json::Obj(f.into_iter().map(|(k, v)| (k.to_string(), v)).collect()));
    }
    if !o.script.is_empty() {
        syms.push(obj([
            ("kind", "script".into()),
            ("lines", script_ranges(&o, &lines).into_iter().map(Json::from).collect::<Vec<_>>().into()),
            ("statements", (o.script.len() as i64).into()),
        ]));
    }
    obj([("file", file.into()), ("lines", (text.lines().count() as i64).into()), ("symbols", syms.into())])
}

/// A symbol's source as it is in the file: (name, kind, first line, last line, text).
fn show_one(text: &str, name: &str) -> Result<(String, &'static str, usize, usize, String), String> {
    let o = outline(text);
    let lines = Lines::new(text);
    if name == "main" && o.named("main").is_empty() && !o.script.is_empty() {
        let parts: Vec<&str> = o.script.iter().map(|&(s, e)| &text[lines.line_start(s)..e]).collect();
        let (first, last) = (o.script[0].0, o.script[o.script.len() - 1].1);
        return Ok((name.into(), "script", lines.line(first), lines.line(last), parts.join("\n")));
    }
    Ok(match o.find(name, text)? {
        Target::Item(i) => (i.name.clone(), i.kind.word(), lines.line(i.doc), lines.line(i.end), text[i.doc..i.end].to_string()),
        Target::Field(_, f) => (name.to_string(), "field", lines.line(f.start), lines.line(f.start), text[f.start..f.end].to_string()),
    })
}

/// The source of each named symbol, separated by blank lines.
pub fn show_text(text: &str, names: &[String]) -> Result<String, String> {
    let mut out = Vec::new();
    for n in names {
        out.push(show_one(text, n)?.4.replace("\r\n", "\n"));
    }
    Ok(out.join("\n\n") + "\n")
}

pub fn show_json(text: &str, names: &[String]) -> Result<Json, String> {
    let mut out = Vec::new();
    for n in names {
        let (name, kind, a, b, t) = show_one(text, n)?;
        out.push(obj([
            ("name", name.into()),
            ("kind", kind.into()),
            ("line", (a as i64).into()),
            ("end_line", (b as i64).into()),
            ("text", t.replace("\r\n", "\n").into()),
        ]));
    }
    Ok(obj([("symbols", out.into())]))
}

// ---- the command line -------------------------------------------------------------------------------------

const USAGE: &str = "\
usage:
  nyra outline <file.nyra> [--json]          the functions, structs and fields, with line ranges
  nyra show <file.nyra> <name>... [--json]   the source of symbols (`Struct.field` for a field)
  nyra edit <file.nyra> [edits] [options]    change symbols by name

edits (several may be given; without any, the edit script is read from stdin):
  --set NAME [CODE]       replace NAME with a new definition (CODE `-` or left out: stdin)
  --add CODE              add definitions (at the end, or --after NAME / --before NAME)
  --delete NAME           delete a function, a struct or a field (Struct.field)
  --rename NAME NEW       rename it and every reference (NAME may be Struct.field)
  --add-field STRUCT 'name: type'   add a field (last, or after --after FIELD)

the edit script on stdin: plain definitions replace the symbols of their names or are added;
or commands, each followed by its code on the next lines:
  @replace NAME | @add [after NAME | before NAME] | @delete NAME | @rename NAME NEW
  @add-field STRUCT name: type [after FIELD]

options:
  --json      print the result as JSON
  --force     apply edits even if they add errors (by default they are refused)
  --fix       repair errors that have a certain fix first (as `nyra check --fix`)
  --dry-run   print the change as a diff and do not write the file
";

fn usage_error(msg: impl std::fmt::Display) -> ExitCode {
    eprintln!("nyra: {msg}\n\n{USAGE}");
    ExitCode::from(2)
}

/// `nyra outline`, `nyra show` and `nyra edit`.
pub fn run(cmd: &str, args: Vec<String>) -> ExitCode {
    if args.iter().any(|a| matches!(a.as_str(), "-h" | "--help")) {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let json = args.iter().any(|a| a == "--json");
    let Some(file) = args.iter().find(|a| !a.starts_with("--")).cloned() else {
        return usage_error(format!("`nyra {cmd}` needs a file"));
    };
    let src = match std::fs::read_to_string(&file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("nyra: cannot read `{file}`: {e}");
            return ExitCode::from(2);
        }
    };
    let rest: Vec<String> = {
        let at = args.iter().position(|a| *a == file).unwrap_or(0);
        args[..at].iter().chain(&args[at + 1..]).cloned().collect()
    };
    match cmd {
        "outline" => {
            if let Some(a) = rest.iter().find(|a| *a != "--json") {
                return usage_error(format!("unexpected `{a}`"));
            }
            if json {
                println!("{}", outline_json(&src, &file));
            } else {
                print!("{}", outline_text(&src, &file));
            }
            ExitCode::SUCCESS
        }
        "show" => {
            let names: Vec<String> = rest.into_iter().filter(|a| a != "--json").collect();
            if names.is_empty() {
                return usage_error("`nyra show` needs a symbol name");
            }
            let shown = if json { show_json(&src, &names).map(|j| format!("{j}\n")) } else { show_text(&src, &names) };
            match shown {
                Ok(t) => {
                    print!("{t}");
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    if json {
                        println!("{}", obj([("ok", false.into()), ("error", e.into())]));
                    } else {
                        eprintln!("nyra: {e}");
                    }
                    ExitCode::from(1)
                }
            }
        }
        _ => edit_cli(&file, &src, rest, json),
    }
}

fn read_stdin() -> Result<String, String> {
    let mut s = String::new();
    std::io::stdin().read_to_string(&mut s).map_err(|e| format!("cannot read stdin: {e}"))?;
    Ok(s)
}

fn edit_cli(file: &str, src: &str, args: Vec<String>, json: bool) -> ExitCode {
    let (mut force, mut fix_errors, mut dry) = (false, false, false);
    let mut ops: Vec<Op> = Vec::new();
    let mut place = Place::End;
    let mut field_after: Option<String> = None;
    let mut it = args.into_iter().peekable();
    let fail = |e: String| {
        if json {
            println!("{}", obj([("ok", false.into()), ("error", e.into())]));
        } else {
            eprintln!("nyra: {e}");
        }
        ExitCode::from(1)
    };
    // the value of a flag: the next argument (stdin for `-` or when there is none)
    let mut stdin_used = false;
    let mut code_arg = |it: &mut std::iter::Peekable<std::vec::IntoIter<String>>| -> Result<String, String> {
        match it.peek() {
            Some(v) if v != "-" && !v.starts_with("--") => Ok(it.next().unwrap_or_default()),
            other => {
                if other.is_some() {
                    it.next();
                }
                if stdin_used {
                    return Err("stdin can be read only once".into());
                }
                stdin_used = true;
                read_stdin()
            }
        }
    };
    while let Some(a) = it.next() {
        let mut word = |what: &str| it.next().filter(|v| !v.starts_with("--")).ok_or(format!("{a} needs {what}"));
        let op = match a.as_str() {
            "--json" => continue,
            "--force" => {
                force = true;
                continue;
            }
            "--fix" => {
                fix_errors = true;
                continue;
            }
            "--dry-run" => {
                dry = true;
                continue;
            }
            "--after" | "--before" => {
                let name = match word("a name") {
                    Ok(n) => n,
                    Err(e) => return usage_error(e),
                };
                if a == "--after" {
                    field_after = Some(name.clone());
                    place = Place::After(name);
                } else {
                    place = Place::Before(name);
                }
                continue;
            }
            "--set" => {
                let name = match word("a name") {
                    Ok(n) => n,
                    Err(e) => return usage_error(e),
                };
                match code_arg(&mut it) {
                    Ok(code) => Op::Replace(name, code),
                    Err(e) => return fail(e),
                }
            }
            "--add" => match code_arg(&mut it) {
                Ok(code) => Op::Add(code, Place::End),
                Err(e) => return fail(e),
            },
            "--delete" => match word("a name") {
                Ok(n) => Op::Delete(n),
                Err(e) => return usage_error(e),
            },
            "--rename" => match (word("a name and a new name"), it.next()) {
                (Ok(n), Some(new)) => Op::Rename(n, new),
                _ => return usage_error("--rename needs a name and a new name"),
            },
            "--add-field" => match (word("a struct and a field"), it.next()) {
                (Ok(s), Some(f)) => Op::AddField(s, f, None),
                _ => return usage_error("--add-field needs a struct and a field, e.g. --add-field Point 'z: int'"),
            },
            _ => return usage_error(format!("unexpected `{a}`")),
        };
        ops.push(op);
    }
    // --after / --before apply to --add and --add-field
    for op in &mut ops {
        match op {
            Op::Add(_, p) => *p = place.clone(),
            Op::AddField(_, _, after) if after.is_none() => after.clone_from(&field_after),
            _ => {}
        }
    }
    if ops.is_empty() {
        let script = match read_stdin() {
            Ok(s) => s,
            Err(e) => return fail(e),
        };
        ops = match parse_script(&script) {
            Ok(ops) => ops,
            Err(e) => return fail(e),
        };
    }
    let out = match run_edits(src, &ops, fix_errors) {
        Ok(o) => o,
        Err(e) => return fail(e),
    };
    let applied = out.added.is_empty() || force;
    if dry {
        print!("{}", fix::diff(src, &out.text));
    } else if applied && out.text != src {
        if let Err(e) = std::fs::write(file, &out.text) {
            eprintln!("nyra: cannot write `{file}`: {e}");
            return ExitCode::from(2);
        }
    }
    if json {
        println!("{}", outcome_json(&out, applied, false));
    } else if applied {
        let verb = if dry { "would edit" } else { "edited" };
        eprintln!("nyra: {verb} {file}: {}", out.notes.join(", "));
        if out.fixed > 0 {
            eprintln!("nyra: --fix repaired {} error(s)", out.fixed);
        }
        if !out.errors.is_empty() {
            eprint!("{}", diag::render_human(&out.errors, file, &out.text));
            eprintln!("nyra: the file still has {} error(s)", out.errors.len());
        }
    } else {
        eprint!("{}", diag::render_human(&out.added, file, &out.text));
        eprintln!(
            "nyra: edit refused: it adds {} error(s) (shown in the edited text); {file} is unchanged (--force applies it anyway)",
            out.added.len()
        );
    }
    if applied {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

// ---- MCP tools ------------------------------------------------------------------------------------------

/// The tool definitions `nyra mcp` adds to its list.
pub const TOOLS: &str = r#"[
{"name":"nyra_outline","title":"Outline a Nyra file","description":"The cheapest view of a Nyra program: one line per function (signature) and struct (fields) with its line range, e.g. `6-9 fn area(r: Rect) -> int`. Read this instead of the whole file, then nyra_show the symbols you need.","inputSchema":{"type":"object","properties":{"path":{"type":"string","description":"a .nyra file"},"code":{"type":"string","description":"or the program itself"}}},"annotations":{"readOnlyHint":true,"openWorldHint":false}},
{"name":"nyra_show","title":"Show Nyra symbols","description":"The source of functions, structs or fields (Struct.field) by name, exactly as in the file.","inputSchema":{"type":"object","properties":{"path":{"type":"string","description":"a .nyra file"},"code":{"type":"string","description":"or the program itself"},"name":{"type":"string","description":"one or more names separated by spaces"}},"required":["name"]},"annotations":{"readOnlyHint":true,"openWorldHint":false}},
{"name":"nyra_edit","title":"Edit Nyra symbols by name","description":"Change a program by symbol instead of resending it. edits: plain definitions replace the symbols of their names (or are added), or commands: `@replace NAME` + code, `@add [after|before NAME]` + code, `@delete NAME`, `@rename NAME NEW` (updates every reference), `@add-field Struct name: type`. Fields are Struct.field. The rest of the file stays byte-identical. The result is checked: an edit that adds errors is refused with the errors (force applies it). With path the file is written and only a summary returns; with code the new code returns.","inputSchema":{"type":"object","properties":{"path":{"type":"string","description":"a .nyra file to edit in place"},"code":{"type":"string","description":"or the program itself"},"edits":{"type":"string","description":"the edit script"},"force":{"type":"boolean","description":"apply even if errors are added"},"fix":{"type":"boolean","description":"repair errors with a certain fix first"}},"required":["edits"]},"annotations":{"readOnlyHint":false,"destructiveHint":false,"idempotentHint":false,"openWorldHint":false}}
]"#;

fn tool_error(msg: impl Into<String>) -> String {
    obj([("ok", false.into()), ("error", msg.into().into())]).to_string()
}

fn arg_str<'a>(args: &'a Json, key: &str) -> Result<Option<&'a str>, String> {
    match args.get(key) {
        None | Some(Json::Null) => Ok(None),
        Some(Json::Str(s)) => Ok(Some(s)),
        Some(_) => Err(tool_error(format!("argument `{key}` must be a string"))),
    }
}

fn arg_bool(args: &Json, key: &str) -> Result<bool, String> {
    match args.get(key) {
        None | Some(Json::Null) => Ok(false),
        Some(Json::Bool(b)) => Ok(*b),
        Some(_) => Err(tool_error(format!("argument `{key}` must be true or false"))),
    }
}

/// The program of a tool call: (source, file name, path to write back).
fn source(args: &Json) -> Result<(String, String, Option<String>), String> {
    match (arg_str(args, "path")?, arg_str(args, "code")?) {
        (Some(_), Some(_)) => Err(tool_error("give either path or code, not both")),
        (None, None) => Err(tool_error("missing argument `path` (a .nyra file) or `code`")),
        (None, Some(code)) => Ok((code.to_string(), "main.nyra".into(), None)),
        (Some(path), None) => {
            if !path.ends_with(".nyra") {
                return Err(tool_error(format!("path must be a .nyra file, found `{path}`")));
            }
            let text = std::fs::read_to_string(path).map_err(|e| tool_error(format!("cannot read `{path}`: {e}")))?;
            Ok((text, path.to_string(), Some(path.to_string())))
        }
    }
}

/// `nyra_outline`, `nyra_show` and `nyra_edit`: `Ok` is a normal result, `Err` one with `isError`.
pub fn tool(name: &str, args: &Json) -> Result<String, String> {
    let (src, file, path) = source(args)?;
    match name {
        "nyra_outline" => Ok(outline_text(&src, &file)),
        "nyra_show" => {
            let names: Vec<String> = arg_str(args, "name")?
                .ok_or_else(|| tool_error("missing argument `name`"))?
                .split([' ', ',', '\n'])
                .filter(|n| !n.is_empty())
                .map(String::from)
                .collect();
            show_text(&src, &names).map_err(tool_error)
        }
        _ => {
            let script = arg_str(args, "edits")?.ok_or_else(|| tool_error("missing argument `edits`"))?;
            let ops = parse_script(script).map_err(tool_error)?;
            let out = run_edits(&src, &ops, arg_bool(args, "fix")?).map_err(tool_error)?;
            let applied = out.added.is_empty() || arg_bool(args, "force")?;
            if applied && out.text != src {
                if let Some(p) = &path {
                    std::fs::write(p, &out.text).map_err(|e| tool_error(format!("cannot write `{p}`: {e}")))?;
                }
            }
            Ok(outcome_json(&out, applied, path.is_none()).to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_become_byte_offsets() {
        let l = Lines::new("ab\r\nçd\n");
        assert_eq!(l.offset(Span { line: 2, col: 1 }), 4);
        assert_eq!(l.offset(Span { line: 2, col: 2 }), 6);
        assert_eq!(l.offset(Span { line: 2, col: 3 }), 7);
        assert_eq!(l.content_end(0), 2);
        assert_eq!(l.next_line(0), 4);
        assert_eq!(l.line(5), 2);
    }

    #[test]
    fn outline_finds_items_fields_and_script() {
        let src = "// adds\nfn add(a: int,\n    b: int) -> int = a + b // sum\nstruct P { x: int, ys: [int] }\nprint(add(1, 2))\nfn main2() {\n    if true {\n        print(1)\n    }\n}\n";
        let o = outline(src);
        assert_eq!(o.items.len(), 3);
        let add = &o.items[0];
        assert_eq!(
            (add.sig.as_str(), &src[add.doc..add.end]),
            ("fn add(a: int, b: int) -> int", "// adds\nfn add(a: int,\n    b: int) -> int = a + b // sum")
        );
        assert_eq!(o.items[1].sig, "struct P { x: int, ys: [int] }");
        assert_eq!(o.items[1].fields[1].ty, "[int]");
        assert_eq!(o.script.len(), 1);
        assert!(src[o.items[2].start..o.items[2].end].ends_with("    }\n}"));
    }

    #[test]
    fn scripts_parse() {
        let ops = parse_script("@rename a b\n@delete P.x\n@add after f\nfn g() = print(1)\n@add-field P z: [int] after y\n").unwrap();
        assert_eq!(ops[0], Op::Rename("a".into(), "b".into()));
        assert_eq!(ops[1], Op::Delete("P.x".into()));
        assert_eq!(ops[2], Op::Add("fn g() = print(1)\n".into(), Place::After("f".into())));
        assert_eq!(ops[3], Op::AddField("P".into(), "z: [int]".into(), Some("y".into())));
        assert_eq!(parse_script("fn f() = 1\n").unwrap(), vec![Op::Upsert("fn f() = 1\n".into())]);
        assert!(parse_script("@frobnicate x").is_err());
        assert!(parse_script("@delete x\nfn f() = 1").is_err());
    }
}
