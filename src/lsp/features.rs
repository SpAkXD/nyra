//! The requests: diagnostics, hover, completion, signature help, go to definition, the outline,
//! formatting and quick fixes. Each works on a `View` of one buffer: its text, its last check
//! (and the last one that got through the parser), and the names found in its tokens.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::ast::{Span, Type, OPTION_PREFIX, TUPLE_PREFIX, USER_MAIN};
use crate::diag::Diag;
use crate::edit::{self, Kind as ItemKind};
use crate::json::{obj, Json};
use crate::lexer::{self, StrPart, Tok, Token};
use crate::{explain, fix, fmt, modules, stdlib};

use super::analysis::{self, Analysis, Info};
use super::path_to_uri;
use super::scope::{Decl, Kind, Scopes};
use super::sigs::{self, Sig};
use super::text::{self, Enc, Lines};

/// Where the error codes are documented.
const ERRORS_URL: &str = "https://github.com/SpAkXD/nyra/blob/main/docs/ERRORS.md";

/// The name completion puts where the member being typed is, to learn the receiver's type from
/// the checker (`p.` is checked as `p.NAME`).
const PROBE: &str = "nyra_lsp_probe__";

/// One buffer as the requests see it.
pub struct View<'a> {
    pub uri: &'a str,
    pub text: &'a str,
    pub path: Option<&'a Path>,
    /// The check of `text`.
    pub an: &'a Analysis,
    /// When `an` did not get through the parser: the last check that did, and its text.
    pub good: Option<(&'a Analysis, &'a str)>,
    pub enc: Enc,
    /// The URIs of the open buffers, by `modules::overlay_key` of their paths.
    pub docs: HashMap<String, String>,
    pub lines: Lines<'a>,
    pub scopes: Scopes,
}

/// What a name means: how hover shows it and where its definition is.
struct Target {
    /// Nyra code: a signature, `let x: int`, ...
    label: String,
    doc: String,
    loc: Option<Json>,
}

/// A `use` line.
struct UseLine {
    module: String,
    /// `./shapes` for a file of the project
    path: Option<String>,
    line: usize,
    /// the columns of the module's name or path
    from: usize,
    to: usize,
}

fn before(a: Span, b: Span) -> bool {
    (a.line, a.col) < (b.line, b.col)
}

fn code(label: &str) -> String {
    format!("```nyra\n{label}\n```")
}

impl<'a> View<'a> {
    pub fn new(
        uri: &'a str,
        text: &'a str,
        path: Option<&'a Path>,
        an: &'a Analysis,
        good: Option<(&'a Analysis, &'a str)>,
        enc: Enc,
        docs: HashMap<String, String>,
    ) -> View<'a> {
        View { uri, text, path, an, good, enc, docs, lines: Lines::new(text), scopes: Scopes::new(text) }
    }

    /// The check with types: this text's, or the last good one.
    fn typed(&self) -> Option<&Analysis> {
        if self.an.checked {
            Some(self.an)
        } else {
            self.good.map(|(a, _)| a)
        }
    }

    /// The program with its functions' signatures: checked, else parsed.
    fn prog(&self) -> Option<&crate::ast::Program> {
        self.typed().and_then(|a| a.prog.as_ref()).or(self.an.prog.as_ref())
    }

    /// The position of an LSP `position` parameter.
    fn at(&self, params: &Json) -> Span {
        let p = params.get("position");
        let line = p.and_then(|p| p.get("line")).and_then(Json::as_f64).unwrap_or(0.0) as usize;
        let col = p.and_then(|p| p.get("character")).and_then(Json::as_f64).unwrap_or(0.0) as usize;
        self.lines.span(line, col, self.enc)
    }

    fn range(&self, a: Span, b: Span) -> Json {
        self.lines.range(a, b, self.enc)
    }

    fn loc_here(&self, a: Span, b: Span) -> Json {
        obj([("uri", self.uri.into()), ("range", self.range(a, b))])
    }

    /// The URI of a file: an open buffer's own, else a `file:` URI.
    fn uri_of(&self, p: &Path) -> String {
        self.docs.get(&modules::overlay_key(p)).cloned().unwrap_or_else(|| path_to_uri(p))
    }

    /// The text of a file: an open buffer, else the disk.
    fn read(&self, p: &Path) -> Option<String> {
        modules::overlay_text(p).or_else(|| std::fs::read_to_string(p).ok())
    }

    /// The `use` lines of the buffer.
    fn uses(&self) -> Vec<UseLine> {
        let mut out = Vec::new();
        for l in 0..self.lines.count() {
            let line = self.lines.line(l);
            let body = line.trim_start();
            let Some(rest) = body.strip_prefix("use ") else { continue };
            let indent = line.chars().count() - body.chars().count();
            let target = rest.split("//").next().unwrap_or("").trim();
            if target.is_empty() {
                continue;
            }
            let from = indent + 4 + (rest.chars().count() - rest.trim_start().chars().count()) + 1;
            let to = from + target.chars().count();
            let (module, path) = if target.starts_with("./") || target.starts_with("../") {
                (target.rsplit('/').next().unwrap_or(target).to_string(), Some(target.to_string()))
            } else {
                (target.to_string(), None)
            };
            out.push(UseLine { module, path, line: l + 1, from, to });
        }
        out
    }

    fn uses_web(&self) -> bool {
        self.uses().iter().any(|u| u.module == "web" && u.path.is_none())
    }

    /// The file of `use ./rel`.
    fn module_path(&self, rel: &str) -> Option<PathBuf> {
        let p = self.path?.parent()?.join(format!("{rel}.nyra"));
        Some(p.components().filter(|c| !matches!(c, std::path::Component::CurDir)).collect())
    }

    /// The file and text of the imported module `name`.
    fn module_text(&self, name: &str) -> Option<(PathBuf, String)> {
        let u = self.uses().into_iter().find(|u| u.module == name && u.path.is_some())?;
        let p = self.module_path(u.path.as_deref()?)?;
        let t = self.read(&p)?;
        Some((p, t))
    }

    /// The location of a definition in another file.
    fn loc_in(&self, p: &Path, text: &str, item: &edit::Item, part: Option<&edit::FieldItem>) -> Json {
        let lines = Lines::new(text);
        let (a, b) = match part {
            Some(f) => (lines.span_of(f.start), lines.span_of(f.start + f.name.len())),
            None => {
                let at = name_offset(text, item);
                (lines.span_of(at), lines.span_of(at + item.name.len()))
            }
        };
        obj([("uri", self.uri_of(p).into()), ("range", lines.range(a, b, self.enc))])
    }

    /// A definition named `name` of the given kinds: in this buffer, else in an imported file.
    /// Gives its location and the text it is in.
    fn find_item(&self, name: &str, kinds: &[ItemKind]) -> Option<(edit::Item, Json, String)> {
        if let Some(d) = self.scopes.defs.iter().find(|d| d.item.name == name && kinds.contains(&d.item.kind)) {
            let end = Span { line: d.name_at.line, col: d.name_at.col + name.chars().count() };
            return Some((d.item.clone(), self.loc_here(d.name_at, end), self.text.to_string()));
        }
        for u in self.uses().iter().filter(|u| u.path.is_some()) {
            let Some(p) = self.module_path(u.path.as_deref().unwrap_or("")) else { continue };
            let Some(t) = self.read(&p) else { continue };
            let o = edit::outline(&t);
            if let Some(item) = o.items.into_iter().find(|i| i.name == name && kinds.contains(&i.kind)) {
                let loc = self.loc_in(&p, &t, &item, None);
                return Some((item, loc, t));
            }
        }
        None
    }

    // ---- what a name means ------------------------------------------------------------------------

    fn target(&self, at: Span) -> Option<Target> {
        if let Some(t) = self.use_target(at) {
            return Some(t);
        }
        let toks = &self.scopes.toks;
        let i = self.scopes.token_at(at)?;
        let Tok::Ident(name) = &toks[i].tok else { return None };
        let span = toks[i].span;
        if i > 0 && toks[i - 1].tok == Tok::Dot {
            let recv = receiver(toks, i - 1);
            if let Some((r, rspan)) = &recv {
                if self.scopes.resolve(r, *rspan).is_none() {
                    if let Some(t) = self.module_member(r, name).or_else(|| self.variant(r, name)) {
                        return Some(t);
                    }
                }
            }
            if let Some(t) = self.member_at(span) {
                return Some(t);
            }
            let (r, rspan) = recv?;
            let ty = self.var_type(&r, rspan)?;
            return self.member_of(ty, name);
        }
        if let Some(full) = self.call_at(span) {
            if let Some(t) = self.func_target(&full) {
                return Some(t);
            }
        }
        if let Some(d) = self.scopes.resolve(name, span) {
            return Some(self.decl_target(d, span));
        }
        self.type_target(name).or_else(|| self.func_target(name)).or_else(|| self.module_target(name))
    }

    /// The module of a `use` line, when the position is on its name.
    fn use_target(&self, at: Span) -> Option<Target> {
        let u = self.uses().into_iter().find(|u| u.line == at.line && at.col >= u.from && at.col <= u.to)?;
        self.module_target(&u.module)
    }

    fn module_target(&self, name: &str) -> Option<Target> {
        let u = self.uses().into_iter().find(|u| u.module == name)?;
        match &u.path {
            Some(rel) => {
                let p = self.module_path(rel)?;
                let text = self.read(&p).unwrap_or_default();
                let public: Vec<String> = edit::outline(&text)
                    .items
                    .into_iter()
                    .filter(|i| i.sig.starts_with("pub "))
                    .map(|i| format!("- `{}`", i.sig))
                    .collect();
                let doc = if public.is_empty() { "no `pub` definitions".to_string() } else { public.join("\n") };
                let loc = obj([("uri", self.uri_of(&p).into()), ("range", text::range_json((0, 0), (0, 0)))]);
                Some(Target { label: format!("use {rel}  // {}", p.display()), doc, loc: Some(loc) })
            }
            None if stdlib::is_module(name) => {
                let names: Vec<String> = if name == "web" {
                    sigs::web().into_iter().map(|s| s.name).collect()
                } else {
                    stdlib::names(name).into_iter().map(|n| format!("{name}.{n}")).collect()
                };
                let doc = if name == "web" {
                    format!("The web library (docs/WEB.md): its functions are called by their bare names: {}", names.join(", "))
                } else {
                    format!("Standard module: {}", names.join(", "))
                };
                Some(Target { label: format!("use {name}"), doc, loc: None })
            }
            None => None,
        }
    }

    /// `math.sqrt`, `shapes.area`
    fn module_member(&self, m: &str, name: &str) -> Option<Target> {
        let full = format!("{m}.{name}");
        if stdlib::is_module(m) {
            let s = sigs::module_fn(&full)?;
            let label = if s.doc == "constant" { format!("{full}: {}", s.ret) } else { s.label() };
            let doc = if s.doc == "constant" { String::new() } else { s.doc };
            return Some(Target { label, doc, loc: None });
        }
        if self.uses().iter().any(|u| u.module == m && u.path.is_some()) {
            return self.func_target(&full);
        }
        None
    }

    /// `Dir.N`
    fn variant(&self, e: &str, name: &str) -> Option<Target> {
        let (item, _, t) = self.find_item(e, &[ItemKind::Enum])?;
        let v = item.fields.iter().find(|f| f.name == name)?;
        let loc = self.part_loc(&item, v, &t);
        Some(Target { label: format!("{e}.{}{}", v.name, v.ty), doc: format!("a variant of `{}`", item.sig), loc: Some(loc) })
    }

    /// The location of a field or variant of a definition whose text is `t`.
    fn part_loc(&self, item: &edit::Item, part: &edit::FieldItem, t: &str) -> Json {
        if t == self.text {
            let a = self.lines.span_of(part.start);
            let b = self.lines.span_of(part.start + part.name.len());
            return self.loc_here(a, b);
        }
        let (p, _) = self
            .uses()
            .iter()
            .filter_map(|u| u.path.as_deref().and_then(|r| self.module_path(r)))
            .filter_map(|p| self.read(&p).map(|t| (p, t)))
            .find(|(_, text)| text == t)
            .unwrap_or_default();
        self.loc_in(&p, t, item, Some(part))
    }

    /// What the checker found at the name after a `.`.
    fn member_at(&self, span: Span) -> Option<Target> {
        if !self.an.checked {
            return None;
        }
        for info in self.an.exprs.get(&span)? {
            match info {
                Info::Field { recv, name, ty } => return self.field(*recv, name, Some(*ty)),
                Info::Method { recv, name, .. } => {
                    let s = sigs::method(*recv, name)?;
                    return Some(Target { label: s.label(), doc: s.doc, loc: None });
                }
                Info::Call { name, .. } => return self.func_target(name),
                Info::Var(..) => {}
            }
        }
        None
    }

    /// The field `name` of a value of type `recv`.
    fn field(&self, recv: Type, name: &str, ty: Option<Type>) -> Option<Target> {
        let owner = recv.struct_name()?;
        if recv.is_tuple() || recv.is_option() {
            let ty = ty.map(|t| t.name()).unwrap_or_default();
            return Some(Target { label: format!("({}).{name}: {ty}", recv.name()), doc: String::new(), loc: None });
        }
        let ty = ty.filter(|t| !t.is_unknown()).map(|t| t.name()).or_else(|| {
            let s = self.prog()?.structs.iter().find(|s| s.name == owner)?;
            s.fields.iter().find(|f| f.name == name).map(|f| f.ty.name())
        });
        let found = self.find_item(&owner, &[ItemKind::Struct]);
        let loc =
            found.as_ref().and_then(|(item, _, t)| item.fields.iter().find(|f| f.name == name).map(|f| self.part_loc(item, f, t)));
        Some(Target { label: format!("{owner}.{name}: {}", ty.unwrap_or_default()), doc: format!("a field of `{owner}`"), loc })
    }

    /// `.name` on a value of type `ty`: a field, a method, or a function of the program (`r.area()`).
    fn member_of(&self, ty: Type, name: &str) -> Option<Target> {
        if let Some(s) = sigs::method(ty, name) {
            return Some(Target { label: s.label(), doc: s.doc, loc: None });
        }
        if ty.struct_name().is_some() {
            let structure = self.prog().and_then(|p| p.structs.iter().find(|s| Some(&s.name) == ty.struct_name().as_ref()));
            if structure.is_some_and(|s| s.fields.iter().any(|f| f.name == name)) {
                return self.field(ty, name, None);
            }
        }
        self.func_target(name)
    }

    /// The full name of the function called at `span`, as the checker resolved it.
    fn call_at(&self, span: Span) -> Option<String> {
        if !self.an.checked {
            return None;
        }
        self.an.exprs.get(&span)?.iter().find_map(|i| match i {
            Info::Call { name, .. } => Some(name.clone()),
            _ => None,
        })
    }

    /// The type of the variable `name` used at `at`.
    fn var_type(&self, name: &str, at: Span) -> Option<Type> {
        if self.an.checked {
            if let Some(t) = self.an.var_at(at, name) {
                return Some(t);
            }
        }
        let d = self.scopes.resolve(name, at)?;
        self.decl_type(d)
    }

    /// The type of a declared name.
    fn decl_type(&self, d: &Decl) -> Option<Type> {
        if matches!(d.kind, Kind::Param | Kind::Inout) {
            let f = d.func.as_deref()?;
            let func = self.prog()?.funcs.iter().find(|x| x.name == f || (f == "main" && x.name == USER_MAIN))?;
            return func.params.iter().find(|p| p.name == d.name).map(|p| p.ty).filter(|t| !t.is_unknown());
        }
        if self.an.checked {
            if let Some(t) = self.an.var_type_in(&d.name, d.span.line, d.end.line) {
                return Some(t);
            }
        }
        // the last good check: the declaration's line moved by as many lines as the text grew
        let (good, old) = self.good?;
        let shift = self.lines.count() as i64 - Lines::new(old).count() as i64;
        let from = (d.span.line as i64 - shift.max(0)).max(1) as usize;
        let to = (d.end.line as i64 - shift.min(0)).max(1) as usize;
        good.var_type_in(&d.name, from, to)
    }

    fn decl_target(&self, d: &Decl, _at: Span) -> Target {
        let ty = self.decl_type(d).map(|t| format!(": {}", t.name())).unwrap_or_default();
        let label = match d.kind {
            Kind::Let | Kind::Var => format!("{} {}{ty}", d.kind.word(), d.name),
            Kind::Param => format!("{}{ty}", d.name),
            Kind::Inout => format!("inout {}{ty}", d.name),
            Kind::Loop | Kind::Bound => format!("{}{ty}", d.name),
        };
        let doc = match (d.kind, d.global, &d.func) {
            (_, true, _) => "a script variable: every function can use it".to_string(),
            (Kind::Param | Kind::Inout, _, Some(f)) => format!("a parameter of `{f}`"),
            (Kind::Loop, _, _) => "a loop variable".to_string(),
            (Kind::Bound, _, _) => "a lambda parameter or a name bound by a pattern".to_string(),
            _ => String::new(),
        };
        let end = Span { line: d.span.line, col: d.span.col + d.name.chars().count() };
        Target { label, doc, loc: Some(self.loc_here(d.span, end)) }
    }

    /// A struct or enum.
    fn type_target(&self, name: &str) -> Option<Target> {
        if !name.starts_with(|c: char| c.is_uppercase()) {
            return None;
        }
        if let Some((item, loc, t)) = self.find_item(name, &[ItemKind::Struct, ItemKind::Enum]) {
            return Some(Target { label: item.sig.clone(), doc: sigs::doc_of(&t, &item), loc: Some(loc) });
        }
        // a type of the web library
        let s = self.prog()?.structs.iter().find(|s| s.name == name && s.variants.is_empty())?;
        let fields: Vec<String> = s.fields.iter().map(|f| format!("{}: {}", f.name, f.ty.name())).collect();
        Some(Target { label: format!("struct {name} {{ {} }}", fields.join(", ")), doc: String::new(), loc: None })
    }

    /// A function by its full name: the program's own, a module's, a standard one, a builtin.
    fn func_target(&self, full: &str) -> Option<Target> {
        let Some(s) = self.func_sig(full) else {
            // a definition the parser has not seen (a file with a syntax error): as written
            let (item, loc, t) = self.find_item(full, &[ItemKind::Fn])?;
            return Some(Target { label: item.sig.clone(), doc: sigs::doc_of(&t, &item), loc: Some(loc) });
        };
        let loc = match full.split_once('.') {
            Some((m, f)) if !stdlib::is_module(m) => {
                let (p, t) = self.module_text(m)?;
                edit::outline(&t).items.iter().find(|i| i.name == f && i.kind == ItemKind::Fn).map(|i| self.loc_in(&p, &t, i, None))
            }
            Some(_) => None,
            None => self.find_item(full, &[ItemKind::Fn]).map(|(_, loc, _)| loc),
        };
        if s.name.starts_with(|c: char| c.is_uppercase()) {
            return self.type_target(full);
        }
        Some(Target { label: s.label(), doc: s.doc, loc })
    }

    /// The signature of a function by its full name (`area`, `shapes.area`, `math.sqrt`, `web.page`, `page`, `print`).
    fn func_sig(&self, full: &str) -> Option<Sig> {
        let shown = full.strip_prefix("web.").unwrap_or(full);
        if let Some(f) = self.prog().and_then(|p| p.funcs.iter().find(|f| f.name == full || (full == "main" && f.name == USER_MAIN))) {
            if !full.starts_with("web.") && !full.starts_with("math.") {
                let doc = self.func_doc(full);
                return Some(sigs::of_func(full, f, doc));
            }
        }
        if let Some(s) = sigs::module_fn(full) {
            let mut s = s;
            s.name = shown.to_string();
            return Some(s);
        }
        if self.uses_web() {
            if let Some(s) = sigs::web_fn(shown) {
                return Some(s);
            }
        }
        if let Some(s) = sigs::builtins().into_iter().find(|s| s.name == full) {
            return Some(s);
        }
        // a struct: its constructor
        let s = self.prog()?.structs.iter().find(|s| s.name == full && s.variants.is_empty())?;
        Some(Sig {
            name: full.to_string(),
            params: s
                .fields
                .iter()
                .map(|f| sigs::Param { name: f.name.clone(), ty: f.ty.name(), default: None, inout: false })
                .collect(),
            ret: full.to_string(),
            doc: String::new(),
            recv: None,
        })
    }

    /// The comment above a function of the program or of an imported file.
    fn func_doc(&self, full: &str) -> String {
        match full.split_once('.') {
            Some((m, f)) => self
                .module_text(m)
                .and_then(|(_, t)| edit::outline(&t).items.iter().find(|i| i.name == f).map(|i| sigs::doc_of(&t, i)))
                .unwrap_or_default(),
            None => {
                self.scopes.defs.iter().find(|d| d.item.name == full).map(|d| sigs::doc_of(self.text, &d.item)).unwrap_or_default()
            }
        }
    }
}

/// The receiver of the `.` at token `dot`, when it is a plain name (`p` in `p.x`, not `a.b` in `a.b.c`).
fn receiver(toks: &[Token], dot: usize) -> Option<(String, Span)> {
    let r = dot.checked_sub(1)?;
    let Tok::Ident(name) = &toks[r].tok else { return None };
    if r > 0 && toks[r - 1].tok == Tok::Dot {
        return None;
    }
    Some((name.clone(), toks[r].span))
}

/// Where the name of a definition is (after `fn`, `struct`, `enum`).
fn name_offset(text: &str, item: &edit::Item) -> usize {
    let rest = text.get(item.start..).unwrap_or("");
    let skip = rest.find(|c: char| c.is_whitespace()).unwrap_or(0);
    let skip =
        if rest.starts_with("pub ") { skip + 1 + rest[skip + 1..].find(|c: char| c.is_whitespace()).unwrap_or(0) } else { skip };
    item.start + skip + rest[skip..].find(&item.name).unwrap_or(0)
}

// ---- diagnostics --------------------------------------------------------------------------------------

/// The diagnostics of a check, as LSP diagnostics.
pub fn diagnostics(text: &str, path: Option<&Path>, a: &Analysis, enc: Enc, docs: &HashMap<String, String>) -> Vec<Json> {
    let lines = Lines::new(text);
    let mut out = Vec::new();
    for (d, severity) in a.errors.iter().map(|d| (d, 1)).chain(a.warnings.iter().map(|d| (d, 2))) {
        out.push(diagnostic(d, severity, text, &lines, path, enc, docs));
    }
    out
}

fn anchor(code: &str) -> String {
    let title = explain_entry(code).map(|e| e.title).unwrap_or_default();
    let words = format!("{code} {title}").to_lowercase();
    let mut slug = String::new();
    for c in words.chars() {
        if c.is_alphanumeric() || c == '-' || c == '_' {
            slug.push(c);
        } else if c == ' ' {
            slug.push('-');
        }
    }
    slug
}

fn diagnostic(
    d: &Diag,
    severity: i64,
    text: &str,
    lines: &Lines,
    path: Option<&Path>,
    enc: Enc,
    docs: &HashMap<String, String>,
) -> Json {
    let mut message = d.msg.clone();
    if let Some(h) = &d.hint {
        message += &format!("\nhint: {h}");
    }
    if let Some(p) = fix::preview(text, &d.fix) {
        message += &format!("\nfix: {p} (quick fix)");
    }
    if let (Some(a), Some(e)) = (&d.actual, &d.expected) {
        message += &format!("\nactual: {a}\nexpected: {e}");
    }
    let mut range = lines.range(d.span, text::token_end(lines, d.span), enc);
    let mut related = Vec::new();
    // a mistake in an imported file: shown on the `use` line of that file, and linked to it
    if let Some(file) = &d.file {
        let other = PathBuf::from(file);
        let key = modules::overlay_key(&other);
        let base = path.and_then(Path::parent);
        let use_line = (0..lines.count()).find(|&l| {
            let body = lines.line(l).trim();
            let Some(rel) = body.strip_prefix("use ").map(|r| r.split("//").next().unwrap_or("").trim()) else { return false };
            base.is_some_and(|b| modules::overlay_key(&b.join(format!("{rel}.nyra"))) == key)
        });
        if let Some(l) = use_line {
            range = text::range_json((l, 0), (l, lines.line(l).encode_utf16().count()));
        }
        let other_text = modules::overlay_text(&other).or_else(|| std::fs::read_to_string(&other).ok()).unwrap_or_default();
        let ol = Lines::new(&other_text);
        let uri = docs.get(&key).cloned().unwrap_or_else(|| path_to_uri(&other));
        let shown = other.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| file.clone());
        message = format!("in {shown}:{}:{}: {message}", d.span.line, d.span.col);
        related.push(obj([
            ("location", obj([("uri", uri.into()), ("range", ol.range(d.span, text::token_end(&ol, d.span), enc))])),
            ("message", d.msg.clone().into()),
        ]));
    }
    let mut fields = vec![
        ("range".to_string(), range),
        ("severity".to_string(), Json::from(severity)),
        ("code".to_string(), Json::from(d.code)),
        ("codeDescription".to_string(), obj([("href", format!("{ERRORS_URL}#{}", anchor(d.code)).into())])),
        ("source".to_string(), "nyra".into()),
        ("message".to_string(), message.into()),
    ];
    if !related.is_empty() {
        fields.push(("relatedInformation".to_string(), Json::Arr(related)));
    }
    Json::Obj(fields)
}

thread_local! {
    static ERRORS: Vec<explain::Entry> = explain::database().unwrap_or_default();
}

fn explain_entry(code: &str) -> Option<explain::Entry> {
    ERRORS.with(|all| all.iter().find(|e| e.code == code).cloned())
}

/// The explanation of an error code, for hover (`nyra explain CODE`, shortened).
fn explain_text(code: &str) -> Option<String> {
    let e = explain_entry(code)?;
    let mut out = format!("**{}: {}** ({})\n\n{}", e.code, e.title, e.kind, e.what);
    if !e.fixed.is_empty() {
        out += &format!("\n\nFixed:\n{}", code_block(&e.fixed));
    }
    out += &format!("\n\n[docs/ERRORS.md]({ERRORS_URL}#{}) · `nyra explain {}`", anchor(code), e.code);
    Some(out)
}

fn code_block(s: &str) -> String {
    format!("```nyra\n{}\n```", s.trim_end())
}

// ---- hover ----------------------------------------------------------------------------------------------

pub fn hover(v: &View, params: &Json) -> Json {
    let at = v.at(params);
    let mut parts: Vec<String> = Vec::new();
    if let Some(t) = v.target(at) {
        let mut s = code(&t.label);
        if !t.doc.is_empty() {
            s += &format!("\n\n{}", t.doc);
        }
        parts.push(s);
    }
    // the errors and warnings at this place, with what their code means
    let mut codes: Vec<&str> = Vec::new();
    for d in v.an.errors.iter().chain(&v.an.warnings).filter(|d| d.file.is_none()) {
        let end = text::token_end(&v.lines, d.span);
        let on = at.line == d.span.line && at.col >= d.span.col && (at.col <= end.col || end == d.span);
        if on && !codes.contains(&d.code) {
            codes.push(d.code);
            if let Some(x) = explain_text(d.code) {
                parts.push(x);
            }
        }
    }
    // an error code written in the text (a comment): `E0201`
    if let Some((w, _)) = text::word_at(v.lines.line(at.line - 1), at.col) {
        let is_code = w.len() == 5 && w.starts_with('E') && w[1..].chars().all(|c| c.is_ascii_digit());
        if is_code && !codes.contains(&w.as_str()) {
            if let Some(x) = explain_text(&w) {
                parts.push(x);
            }
        }
    }
    if parts.is_empty() {
        return Json::Null;
    }
    obj([("contents", obj([("kind", "markdown".into()), ("value", parts.join("\n\n---\n\n").into())]))])
}

// ---- go to definition -------------------------------------------------------------------------------------

pub fn definition(v: &View, params: &Json) -> Json {
    v.target(v.at(params)).and_then(|t| t.loc).unwrap_or(Json::Null)
}

// ---- completion -----------------------------------------------------------------------------------------

/// LSP completion item kinds.
mod ck {
    pub const METHOD: i64 = 2;
    pub const FUNCTION: i64 = 3;
    pub const FIELD: i64 = 5;
    pub const VARIABLE: i64 = 6;
    pub const MODULE: i64 = 9;
    pub const PROPERTY: i64 = 10;
    pub const KEYWORD: i64 = 14;
    pub const FILE: i64 = 17;
    pub const ENUM_MEMBER: i64 = 20;
    pub const CONSTANT: i64 = 21;
    pub const STRUCT: i64 = 22;
    pub const ENUM: i64 = 13;
}

struct Items {
    list: Vec<Json>,
    seen: Vec<String>,
}

impl Items {
    fn add(&mut self, label: &str, kind: i64, detail: &str, doc: &str) {
        self.add_full(label, kind, detail, doc, None, None);
    }

    fn add_full(&mut self, label: &str, kind: i64, detail: &str, doc: &str, insert: Option<&str>, sort: Option<&str>) {
        if label.is_empty() || self.seen.iter().any(|s| s == label) {
            return;
        }
        self.seen.push(label.to_string());
        let mut f = vec![("label".to_string(), Json::from(label)), ("kind".to_string(), Json::from(kind))];
        if !detail.is_empty() {
            f.push(("detail".to_string(), detail.into()));
        }
        if !doc.is_empty() {
            f.push(("documentation".to_string(), obj([("kind", "markdown".into()), ("value", doc.into())])));
        }
        if let Some(i) = insert {
            f.push(("insertText".to_string(), i.into()));
        }
        if let Some(s) = sort {
            f.push(("sortText".to_string(), s.into()));
        }
        self.list.push(Json::Obj(f));
    }

    fn sig(&mut self, label: &str, s: &Sig, kind: i64) {
        self.add(label, kind, &s.label(), &s.doc);
    }
}

pub fn completion(v: &View, params: &Json) -> Json {
    let at = v.at(params);
    let mut items = Items { list: Vec::new(), seen: Vec::new() };
    let line: Vec<char> = v.lines.line(at.line - 1).chars().collect();
    let cur = (at.col - 1).min(line.len());
    let mut start = cur;
    while start > 0 && text::is_word(line[start - 1]) {
        start -= 1;
    }
    let before: String = line[..start].iter().collect();
    let trimmed = before.trim_start();
    if trimmed == "use " || trimmed == "use" {
        use_items(v, &mut items);
    } else if start > 0 && line[start - 1] == '.' && !before.ends_with("..") {
        let dot = Span { line: at.line, col: start };
        member_items(v, dot, Span { line: at.line, col: start + 1 }, &mut items);
    } else {
        named_option_items(v, at, &mut items);
        general_items(v, at, &mut items);
    }
    obj([("isIncomplete", false.into()), ("items", Json::Arr(items.list))])
}

/// After `use `: the standard modules and the `.nyra` files next to this one.
fn use_items(v: &View, items: &mut Items) {
    for m in stdlib::MODULES {
        items.add(m, ck::MODULE, "standard module", "");
    }
    let Some(dir) = v.path.and_then(Path::parent) else { return };
    let own = v.path.and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().into_owned());
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().to_str().and_then(|n| n.strip_suffix(".nyra")).map(str::to_string))
        .filter(|n| Some(n) != own.as_ref())
        .collect();
    names.sort();
    for n in names {
        items.add(&format!("./{n}"), ck::FILE, "a file of the project", "");
    }
}

/// After `x.`: the fields and methods of `x`'s type, a module's functions, an enum's variants.
fn member_items(v: &View, dot: Span, word: Span, items: &mut Items) {
    let toks = &v.scopes.toks;
    let dot_i = toks.iter().position(|t| t.span == dot && t.tok == Tok::Dot);
    let recv = dot_i.and_then(|i| receiver(toks, i));
    if let Some((r, rspan)) = &recv {
        if v.scopes.resolve(r, *rspan).is_none() {
            if stdlib::is_module(r) {
                for s in sigs::module(r) {
                    let short = s.name.rsplit('.').next().unwrap_or(&s.name).to_string();
                    if s.doc == "constant" {
                        items.add(&short, ck::CONSTANT, &format!("{}: float", s.name), "");
                    } else {
                        items.sig(&short, &s, ck::FUNCTION);
                    }
                }
                return;
            }
            if let Some((_, t)) = v.module_text(r) {
                for i in edit::outline(&t).items.iter().filter(|i| i.sig.starts_with("pub fn")) {
                    let full = format!("{r}.{}", i.name);
                    match v.func_sig(&full) {
                        Some(s) => items.add(&i.name, ck::FUNCTION, &s.label(), &sigs::doc_of(&t, i)),
                        None => items.add(&i.name, ck::FUNCTION, &i.sig, &sigs::doc_of(&t, i)),
                    }
                }
                return;
            }
            if let Some((item, _, _)) = v.find_item(r, &[ItemKind::Enum]) {
                for f in &item.fields {
                    items.add(&f.name, ck::ENUM_MEMBER, &format!("{r}.{}{}", f.name, f.ty), "");
                }
                return;
            }
        }
    }
    // the receiver's type: from the checker, with a name in place of the member being typed
    let ty = probe_type(v, word).or_else(|| {
        let (r, rspan) = recv.as_ref()?;
        v.var_type(r, *rspan)
    });
    let Some(ty) = ty else { return };
    if let Some(name) = ty.struct_name() {
        if let Some(elems) = ty.tuple_elems() {
            for (i, t) in elems.iter().enumerate() {
                items.add(&i.to_string(), ck::FIELD, &t.name(), "");
            }
        } else if !ty.is_option() {
            if let Some(s) = v.prog().and_then(|p| p.structs.iter().find(|s| s.name == name)) {
                for f in &s.fields {
                    items.add(&f.name, ck::FIELD, &format!("{}: {}", f.name, f.ty.name()), &format!("a field of `{name}`"));
                }
            }
        }
    }
    for s in sigs::methods(ty) {
        items.sig(&s.name.clone(), &s, ck::METHOD);
    }
    // `r.area()` calls `area(r)`
    if let Some(p) = v.prog() {
        for f in p.funcs.iter().filter(|f| !f.name.contains('.') && f.params.first().is_some_and(|x| x.ty == ty)) {
            let s = sigs::of_func(&f.name, f, v.func_doc(&f.name));
            items.sig(&f.name, &s, ck::METHOD);
        }
    }
}

/// The type of the receiver of the member being typed at `word`, from a check of the text with a
/// name put there.
fn probe_type(v: &View, word: Span) -> Option<Type> {
    let lines = &v.lines;
    let start = lines.offset(word);
    let rest = &v.text[start..];
    let len = rest.find(|c: char| !text::is_word(c)).unwrap_or(rest.len());
    let probed = format!("{}{PROBE}{}", &v.text[..start], &rest[len..]);
    let a = analysis::analyze(&probed, v.path);
    if !a.checked {
        return None;
    }
    let infos = a.exprs.get(&word)?;
    infos.iter().find_map(|i| match i {
        Info::Field { recv, name, .. } | Info::Method { recv, name, .. } if name == PROBE && !recv.is_unknown() => Some(*recv),
        _ => None,
    })
}

/// Everything a name can be at `at`.
fn general_items(v: &View, at: Span, items: &mut Items) {
    for d in v.scopes.visible(at) {
        let ty = v.decl_type(d).map(|t| t.name()).unwrap_or_default();
        items.add(&d.name, ck::VARIABLE, &ty, if d.global { "script variable" } else { "" });
    }
    for d in &v.scopes.defs {
        let name = &d.item.name;
        match d.item.kind {
            ItemKind::Fn if name != "main" => match v.func_sig(name) {
                Some(s) => items.sig(name, &s, ck::FUNCTION),
                None => items.add(name, ck::FUNCTION, &d.item.sig, ""),
            },
            ItemKind::Struct => items.add(name, ck::STRUCT, &d.item.sig, ""),
            ItemKind::Enum => items.add(name, ck::ENUM, &d.item.sig, ""),
            _ => {}
        }
    }
    // the structs and enums of imported files
    if let Some(p) = v.prog() {
        for s in p.structs.iter().filter(|s| !s.name.starts_with(TUPLE_PREFIX) && !s.name.starts_with(OPTION_PREFIX)) {
            let kind = if s.variants.is_empty() { ck::STRUCT } else { ck::ENUM };
            items.add(&s.name, kind, "", "");
        }
    }
    let web = v.uses_web();
    for u in v.uses() {
        if u.path.is_some() || !(web && u.module == "web") {
            items.add(&u.module, ck::MODULE, &format!("use {}", u.path.as_deref().unwrap_or(&u.module)), "");
        }
    }
    if web {
        for s in sigs::web() {
            items.sig(&s.name.clone(), &s, ck::FUNCTION);
        }
    }
    for s in sigs::builtins() {
        items.sig(&s.name.clone(), &s, ck::FUNCTION);
    }
    for k in sigs::KEYWORDS {
        items.add(k, ck::KEYWORD, "", "");
    }
}

/// Inside the parentheses of a call: its parameters as named arguments (`title: `), first.
fn named_option_items(v: &View, at: Span, items: &mut Items) {
    let Some(call) = call_context(v, at) else { return };
    let Some(sig) = call_sig(v, &call) else { return };
    for (i, p) in sig.params.iter().enumerate() {
        if call.given.contains(&p.name) || p.ty == "..." {
            continue;
        }
        let detail = match &p.default {
            Some(d) => format!("{}: {} = {d}", p.name, p.ty),
            None => format!("{}: {}", p.name, p.ty),
        };
        items.add_full(&format!("{}:", p.name), ck::PROPERTY, &detail, "", Some(&format!("{}: ", p.name)), Some(&format!("0{i:03}")));
    }
}

// ---- signature help -------------------------------------------------------------------------------------

/// The call around a position.
struct CallCtx {
    /// the function's name token
    name: usize,
    /// the argument the position is in
    active: usize,
    /// the argument being written by name (`title: ...`)
    named: Option<String>,
    /// the names of the arguments already given by name
    given: Vec<String>,
}

fn call_context(v: &View, at: Span) -> Option<CallCtx> {
    let toks = &v.scopes.toks;
    let end = toks.partition_point(|t| before(t.span, at));
    let mut depth = 0usize;
    let mut commas = 0;
    let mut named = None;
    let mut given = Vec::new();
    let mut open = None;
    for k in (0..end).rev() {
        match &toks[k].tok {
            Tok::RParen | Tok::RBracket | Tok::RBrace => depth += 1,
            Tok::LParen if depth == 0 => {
                open = Some(k);
                break;
            }
            Tok::LBracket | Tok::LBrace if depth == 0 => return None,
            Tok::LParen | Tok::LBracket | Tok::LBrace => depth -= 1,
            Tok::Comma if depth == 0 => commas += 1,
            Tok::Colon if depth == 0 && k >= 2 && matches!(toks[k - 2].tok, Tok::LParen | Tok::Comma) => {
                if let Tok::Ident(n) = &toks[k - 1].tok {
                    given.push(n.clone());
                    if commas == 0 {
                        named = Some(n.clone());
                    }
                }
            }
            _ => {}
        }
    }
    let open = open?;
    let name = open.checked_sub(1)?;
    if !matches!(toks[name].tok, Tok::Ident(_)) {
        return None;
    }
    Some(CallCtx { name, active: commas, named, given })
}

/// The signature of the function a call calls.
fn call_sig(v: &View, call: &CallCtx) -> Option<Sig> {
    let toks = &v.scopes.toks;
    let Tok::Ident(name) = &toks[call.name].tok else { return None };
    let span = toks[call.name].span;
    if let Some(full) = v.call_at(span) {
        if let Some(s) = v.func_sig(&full) {
            return Some(s);
        }
    }
    if call.name > 0 && toks[call.name - 1].tok == Tok::Dot {
        let recv = receiver(toks, call.name - 1);
        if let Some((r, rspan)) = &recv {
            if v.scopes.resolve(r, *rspan).is_none() && (stdlib::is_module(r) || v.module_text(r).is_some()) {
                return v.func_sig(&format!("{r}.{name}"));
            }
        }
        let ty =
            v.an.exprs
                .get(&span)
                .and_then(|infos| {
                    infos.iter().find_map(|i| match i {
                        Info::Method { recv, .. } if v.an.checked => Some(*recv),
                        _ => None,
                    })
                })
                .or_else(|| recv.as_ref().and_then(|(r, rs)| v.var_type(r, *rs)))?;
        if let Some(s) = sigs::method(ty, name) {
            return Some(s);
        }
        // `r.area()`: the function without its first parameter
        let mut s = v.func_sig(name)?;
        if !s.params.is_empty() {
            s.params.remove(0);
        }
        return Some(s);
    }
    v.func_sig(name)
}

pub fn signature_help(v: &View, params: &Json) -> Json {
    let at = v.at(params);
    let Some(call) = call_context(v, at) else { return Json::Null };
    let Some(sig) = call_sig(v, &call) else { return Json::Null };
    let (label, ranges) = sig.line();
    let active = match &call.named {
        Some(n) => sig.params.iter().position(|p| &p.name == n).unwrap_or(call.active),
        None if sig.params.first().is_some_and(|p| p.ty == "...") => 0,
        None => call.active,
    };
    let params_json: Vec<Json> =
        ranges.iter().map(|(a, b)| obj([("label", Json::Arr(vec![(*a as i64).into(), (*b as i64).into()]))])).collect();
    let mut sig_json = vec![("label".to_string(), Json::from(label)), ("parameters".to_string(), Json::Arr(params_json))];
    if !sig.doc.is_empty() {
        sig_json.push(("documentation".to_string(), obj([("kind", "markdown".into()), ("value", sig.doc.clone().into())])));
    }
    obj([
        ("signatures", Json::Arr(vec![Json::Obj(sig_json)])),
        ("activeSignature", 0i64.into()),
        ("activeParameter", (active.min(sig.params.len().saturating_sub(1)) as i64).into()),
    ])
}

// ---- document symbols -------------------------------------------------------------------------------------

pub fn symbols(v: &View, _params: &Json) -> Json {
    // LSP symbol kinds
    const ENUM: i64 = 10;
    const FUNCTION: i64 = 12;
    const VARIABLE: i64 = 13;
    const FIELD: i64 = 8;
    const ENUM_MEMBER: i64 = 22;
    const STRUCT: i64 = 23;
    let mut out: Vec<(Span, Json)> = Vec::new();
    for d in &v.scopes.defs {
        if d.item.name.is_empty() {
            continue;
        }
        let kind = match d.item.kind {
            ItemKind::Fn => FUNCTION,
            ItemKind::Struct => STRUCT,
            ItemKind::Enum => ENUM,
        };
        let name_end = Span { line: d.name_at.line, col: d.name_at.col + d.item.name.chars().count() };
        let children: Vec<Json> = d
            .item
            .fields
            .iter()
            .map(|f| {
                let a = v.lines.span_of(f.start);
                let b = v.lines.span_of(f.end.max(f.start + f.name.len()));
                let n = v.lines.span_of(f.start + f.name.len());
                obj([
                    ("name", f.name.clone().into()),
                    ("detail", f.ty.clone().into()),
                    ("kind", (if d.item.kind == ItemKind::Enum { ENUM_MEMBER } else { FIELD }).into()),
                    ("range", v.range(a, b)),
                    ("selectionRange", v.range(a, n)),
                ])
            })
            .collect();
        out.push((
            d.start,
            obj([
                ("name", d.item.name.clone().into()),
                ("detail", d.item.sig.clone().into()),
                ("kind", kind.into()),
                ("range", v.range(d.start, d.end)),
                ("selectionRange", v.range(d.name_at, name_end)),
                ("children", Json::Arr(children)),
            ]),
        ));
    }
    for d in v.scopes.decls.iter().filter(|d| d.global) {
        let end = Span { line: d.span.line, col: d.span.col + d.name.chars().count() };
        let ty = v.decl_type(d).map(|t| t.name()).unwrap_or_default();
        out.push((
            d.span,
            obj([
                ("name", d.name.clone().into()),
                (
                    "detail",
                    format!("{} {}{}", d.kind.word(), d.name, if ty.is_empty() { String::new() } else { format!(": {ty}") }).into(),
                ),
                ("kind", VARIABLE.into()),
                ("range", v.range(d.span, end)),
                ("selectionRange", v.range(d.span, end)),
            ]),
        ));
    }
    out.sort_by_key(|(s, _)| (s.line, s.col));
    Json::Arr(out.into_iter().map(|(_, j)| j).collect())
}

// ---- formatting -------------------------------------------------------------------------------------------

/// The text `nyra fmt` makes of the buffer: the certain fixes applied when the program then
/// compiles, and the indentation made canonical. `None` when it does not lex or nothing changes.
fn formatted(v: &View) -> Option<String> {
    modules::set_main(v.path);
    let base = match crate::compile(v.text) {
        Ok(_) => v.text.to_string(),
        Err(diags) => match fix::repair(v.text, diags, crate::compile) {
            Some(r) => r.text,
            None => v.text.to_string(),
        },
    };
    let (_, errs) = lexer::lex(&base);
    if !errs.is_empty() {
        return None;
    }
    let out = fmt::format(&base);
    // only white space and the spelling of `return` may change
    if !same_tokens(&base, &out) {
        return None;
    }
    (out != v.text).then_some(out)
}

pub fn formatting(v: &View, _params: &Json) -> Json {
    match formatted(v) {
        Some(out) => Json::Arr(vec![obj([("range", v.lines.whole(v.enc)), ("newText", out.into())])]),
        None => Json::Arr(Vec::new()),
    }
}

/// True if two texts have the same tokens (`fmt::verify` also compiles, which a buffer with errors does not).
fn same_tokens(a: &str, b: &str) -> bool {
    let (x, _) = lexer::lex(a);
    let (y, _) = lexer::lex(b);
    x.len() == y.len()
        && x.iter().zip(&y).all(|(p, q)| match (&p.tok, &q.tok) {
            (Tok::Interp(m), Tok::Interp(n)) => {
                m.len() == n.len()
                    && m.iter().zip(n).all(|(s, t)| match (s, t) {
                        (StrPart::Lit(s), StrPart::Lit(t)) => s == t,
                        (StrPart::Code(s, _), StrPart::Code(t, _)) => s == t,
                        _ => false,
                    })
            }
            (p, q) => p == q,
        })
}

// ---- code actions -----------------------------------------------------------------------------------------

fn edits_json(v: &View, edits: &[crate::diag::Edit]) -> Json {
    Json::Arr(edits.iter().map(|e| obj([("range", v.range(e.start, e.end)), ("newText", e.text.clone().into())])).collect())
}

pub fn code_actions(v: &View, params: &Json) -> Json {
    let range = params.get("range");
    let pos = |key: &str| {
        let p = range.and_then(|r| r.get(key));
        let line = p.and_then(|p| p.get("line")).and_then(Json::as_f64).unwrap_or(0.0) as usize;
        let col = p.and_then(|p| p.get("character")).and_then(Json::as_f64).unwrap_or(0.0) as usize;
        v.lines.span(line, col, v.enc)
    };
    let (from, to) = (pos("start"), pos("end"));
    let only: Vec<String> = params
        .get("context")
        .and_then(|c| c.get("only"))
        .and_then(Json::as_array)
        .map(|a| a.iter().filter_map(Json::as_str).map(str::to_string).collect())
        .unwrap_or_default();
    let wants = |kind: &str| only.is_empty() || only.iter().any(|o| kind == o || kind.starts_with(&format!("{o}.")));
    let mut actions = Vec::new();
    let fixable: Vec<&Diag> = v.an.errors.iter().chain(&v.an.warnings).filter(|d| d.file.is_none() && !d.fix.is_empty()).collect();
    let docs = HashMap::new();
    if wants("quickfix") {
        for d in &fixable {
            let end = text::token_end(&v.lines, d.span);
            let touches = !before(end, from) && !before(to, d.span) || d.span.line >= from.line && d.span.line <= to.line;
            if !touches || fix::apply(v.text, &d.fix).is_none() {
                continue;
            }
            let what = fix::preview(v.text, &d.fix).unwrap_or_else(|| d.hint.clone().unwrap_or_default());
            let severity = if v.an.errors.iter().any(|e| std::ptr::eq(e, *d)) { 1 } else { 2 };
            actions.push(obj([
                ("title", format!("Fix {}: {what}", d.code).into()),
                ("kind", "quickfix".into()),
                ("diagnostics", Json::Arr(vec![diagnostic(d, severity, v.text, &v.lines, v.path, v.enc, &docs)])),
                ("isPreferred", true.into()),
                ("edit", obj([("changes", Json::Obj(vec![(v.uri.to_string(), edits_json(v, &d.fix))]))])),
            ]));
        }
    }
    if wants("source.fixAll") && !fixable.is_empty() {
        modules::set_main(v.path);
        if let Some(r) = fix::repair(v.text, v.an.errors.clone(), crate::compile) {
            let edit = obj([("range", v.lines.whole(v.enc)), ("newText", r.text.into())]);
            actions.push(obj([
                ("title", format!("Apply every certain fix ({}), as `nyra check --fix`", r.fixed).into()),
                ("kind", "source.fixAll".into()),
                ("edit", obj([("changes", Json::Obj(vec![(v.uri.to_string(), Json::Arr(vec![edit]))]))])),
            ]));
        }
    }
    Json::Arr(actions)
}
