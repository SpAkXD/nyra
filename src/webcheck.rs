//! `use web` (docs/WEB.md): links the web library (`src/std/web.nyra` and its icons) into a
//! program and checks what can be checked before the program runs.
//!
//! - The library's functions are added as `web.name`, like any standard module's, and the
//!   program's calls of their bare names (`hero(...)`) are turned into calls of `web.hero`. A
//!   program function, struct or script variable with the name of a web function or type is
//!   E0372.
//! - The site lives in a script variable, `_web`, declared before the program's first statement.
//! - `on: Msg.X(..)` becomes the JSON text of the value (`web._on(m: Msg)`); a program with `on:`
//!   needs `enum Msg` and `fn update(m: Msg)` (E0373).
//! - Live regions: a function without parameters that returns a `Node` and reads a script
//!   variable that `update` changes is called wrapped in `web._live` (a `data-live` region the
//!   browser renders again). One with parameters is E0376; the code that runs in the browser may
//!   not use `fs`, `os` or `input` (E0384).
//! - Literal arguments are checked: icon names and option words (E0374), icon-only buttons
//!   without a label (E0377), `html(...)` and `css(...)` with a literal only (E0378), links with
//!   a forbidden scheme (E0381). Computed text is checked by the library when it renders, which
//!   prints an error record for the build.

use std::collections::{BTreeSet, HashMap, HashSet};

use crate::ast::*;
use crate::diag::{suggest, Diag, Edit};

/// The library: the elements and kit components, then the generated icon set.
pub const SOURCE: &str = concat!(include_str!("std/web.nyra"), "\n", include_str!("std/web_icons.nyra"));

/// The script variable that holds the site (`WebConfig`).
pub const STATE: &str = "_web";

/// The types of the library a program may name.
pub const TYPES: &[&str] = &["Node", "WebConfig"];

fn library(src: &str) -> Program {
    let (toks, errs) = crate::lexer::lex(src);
    assert!(errs.is_empty(), "the web library does not lex: {errs:?}");
    let (prog, errs) = crate::parser::parse(toks);
    assert!(errs.is_empty(), "the web library does not parse: {errs:?}");
    prog
}

/// The public functions of the library and their parameter names, sorted by name.
#[cfg(test)]
fn functions() -> Vec<(String, Vec<String>)> {
    let mut v: Vec<(String, Vec<String>)> = library(SOURCE)
        .funcs
        .into_iter()
        .filter(|f| !f.name.starts_with('_'))
        .map(|f| (f.name, f.params.into_iter().map(|p| p.name).collect()))
        .collect();
    v.sort();
    v
}

/// The arms `"name" => "value"` of the `match` of the function `func` of the icon set.
fn icon_arms(func: &str) -> Vec<(&'static str, &'static str)> {
    let src: &'static str = include_str!("std/web_icons.nyra");
    let Some(start) = src.find(&format!("fn {func}(")) else { return Vec::new() };
    let body = &src[start..];
    let body = &body[..body.find("\n}").unwrap_or(body.len())];
    body.lines()
        .filter_map(|l| {
            let rest = l.trim_start().strip_prefix('"')?;
            let (name, value) = rest.split_once("\" => ")?;
            Some((name, value.trim().trim_matches('"')))
        })
        .collect()
}

/// The names of the icons (`:name:`, `icon: "name"`), sorted.
pub fn icons() -> Vec<String> {
    let mut v: Vec<String> = icon_arms("_icon_body").into_iter().chain(icon_arms("_brand_body")).map(|(n, _)| n.to_string()).collect();
    v.sort();
    v.dedup();
    v
}

/// The icon of the set that a name of another icon set stands for (`envelope`: `mail`): the
/// `|name=icon|` pairs of `_icon_like` in the icon set.
pub fn icon_like(name: &str) -> Option<&'static str> {
    let src: &'static str = include_str!("std/web_icons.nyra");
    let body = &src[src.find("fn _icon_like(")?..];
    let pairs = body.split_once("let pairs = \"")?.1.split('"').next()?;
    pairs.split('|').filter_map(|p| p.split_once('=')).find(|(n, _)| *n == name).map(|(_, v)| v)
}

/// Warning E0389 for an icon name that is not in the set (`text`: written `:name:` in a text).
/// The build draws the icon the name stands for (`icon_like`); else an icon option gets a plain
/// circle and a text keeps the name as it was written. The same message at compile time (a
/// literal) and while the site is built (a value from data), so it is given once.
pub fn icon_warning(name: &str, text: bool, span: Span) -> Diag {
    let shown = if text { format!(":{name}:") } else { name.to_string() };
    let known = icons();
    match icon_like(name) {
        Some(like) => {
            let what = if text { format!("`:{like}:`") } else { format!("`{like}`") };
            let d = Diag::new("E0389", format!("unknown icon `{shown}`: drawn as `{like}`"), span)
                .hint(format!("`{name}` is not a name of the icon set (Lucide names): write {what}"));
            if text {
                d
            } else {
                d.fix(vec![Edit::replace(span, &format!("\"{name}\""), format!("\"{like}\""))])
            }
        }
        None => {
            let near = known.iter().map(|i| (edits(name, i), i)).filter(|(d, _)| *d <= 2).min();
            // else the icons that share a word with it (`alert-octagon`: `circle-alert`, `triangle-alert`)
            let parts: Vec<&str> = name.split('-').filter(|p| p.len() >= 3).collect();
            let kin: Vec<String> =
                known.iter().filter(|i| i.split('-').any(|p| parts.contains(&p))).take(4).map(|i| format!("`{i}`")).collect();
            let guess = match near {
                Some((_, i)) => format!("did you mean `{i}`? "),
                None if !kin.is_empty() => format!("the set has {}; ", kin.join(", ")),
                None => String::new(),
            };
            let drawn = if text { "shown as it is written" } else { "drawn as a plain circle" };
            Diag::new("E0389", format!("unknown icon `{shown}`: {drawn}"), span).hint(format!(
                "{guess}the icons are a subset of Lucide: `phone`, `mail`, `map-pin`, `clock`, `calendar`, `star`, `heart`, `check`, `circle-alert`, `triangle-alert`, `activity`, `users`, ... (src/std/web_icons.nyra lists them){}",
                if text { "; write `\\\\:` for a colon that starts no icon" } else { "" }
            ))
        }
    }
}

/// The option words the library knows, by parameter (and by function where they differ).
fn words(func: &str, param: &str) -> Option<&'static [&'static str]> {
    Some(match (func, param) {
        ("section", "bg") => &["accent", "alt", "dark", "ink", "primary", "white"],
        ("site", "style") => &["dark", "editorial", "sharp", "soft"],
        ("button", "style") => &["accent", "ghost", "light", "outline", "primary"],
        ("button", "size") => &["lg", "sm"],
        ("p", "size") => &["lg", "sm"],
        ("field", "type") => &["area", "check", "date", "email", "number", "password", "select", "tel", "text", "time", "url"],
        ("section", "align") => &["center", "left"],
        ("section", "width") => &["narrow", "wide"],
        ("stack" | "row", "align") => &["center", "end", "start", "between"],
        ("grid", "align") => &["center", "start"],
        ("hero", "height") => &["full", "half"],
        ("img", "fit") => &["contain", "cover"],
        _ => return None,
    })
}

/// True if `word` is one of the words of option `param:` of the web function `func` (a motion word,
/// an option word, a theme color): written without quotes (`type: tel`, `style: soft`, as on the
/// web card), it is that word, so the quotes are a certain fix.
pub fn option_word(func: &str, param: &str, word: &str) -> bool {
    if let Some(opt) = crate::web::motion::Opt::from_name(param) {
        return opt.words().contains(&word);
    }
    if matches!(param, "color" | "tone") {
        return matches!(word, "primary" | "accent" | "bg" | "alt" | "ink" | "muted" | "white" | "black");
    }
    words(func, param).is_some_and(|ws| ws.contains(&word))
}

/// The hint of E0226 for a named argument a web function does not have, where it is a known
/// guess: what to write instead.
pub fn unknown_param_hint(func: &str, param: &str) -> Option<String> {
    Some(match (func, param) {
        ("hero", "rows" | "hours" | "open") => {
            "`hero` has no `rows:`: `badge:` takes the opening hours rows themselves, `badge: [(\"Mon–Fri\", \"1-5 8:00-16:00\")]`"
                .into()
        }
        (_, "onclick" | "onClick" | "on_click" | "click" | "action") => {
            format!("an event is `on: Msg.X(...)`: `{func}(..., on: Msg.Show(1))`")
        }
        (_, "class" | "className" | "class_name" | "classes") => {
            "a class is `cls: \"name\"`, styled with `css(\".name { ... }\")`".into()
        }
        (_, "style") => {
            format!("`{func}` has no `style:`: give it a class with `cls: \"name\"` and style that with `css(\".name {{ ... }}\")`")
        }
        (_, "children" | "content" | "body") => format!("the content of `{func}` is its child block: `{func}(...) {{ p(\"text\") }}`"),
        _ => return None,
    })
}

/// How the library uses a text argument.
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    /// shown as text: inline Markdown, icons and links
    Text,
    /// a link target
    Url,
    /// an image of `assets/` or an https URL
    Image,
    /// an icon name
    Icon,
    /// used as it is (ids, classes, colors, motion words, CSS sizes)
    Plain,
}

/// The kind of parameter `param` of `func`; for a tuple, of its element `elem`.
fn kind(func: &str, param: &str, elem: Option<usize>) -> Kind {
    match (param, elem) {
        ("href" | "to" | "url", _) => Kind::Url,
        ("links" | "buttons" | "back" | "other" | "cta", Some(1)) => Kind::Url,
        ("image" | "src" | "logo" | "poster", _) => Kind::Image,
        ("photos", Some(0)) => Kind::Image,
        ("photos", Some(1)) | ("photos", Some(2)) | ("filters", Some(0)) => Kind::Plain,
        ("rows" | "badge", Some(1)) => Kind::Plain,
        ("icon", _) => Kind::Icon,
        ("hint" | "alt" | "label" | "subject" | "mailto" | "name", _) if func != "quote" => Kind::Plain,
        (
            "id" | "cls" | "anim" | "hover" | "loop" | "scroll" | "morph" | "bg" | "tone" | "align" | "width" | "widths" | "gap"
            | "ratio" | "fit" | "style" | "size" | "type" | "auto" | "value" | "options" | "color" | "path" | "lang" | "tz"
            | "primary" | "accent" | "alt" | "ink" | "muted" | "palette" | "font" | "display" | "motion" | "transition" | "height"
            | "next" | "say" | "raw" | "body",
            _,
        ) => Kind::Plain,
        _ if func == "css" || func == "html" || func == "keyframes" => Kind::Plain,
        _ => Kind::Text,
    }
}

/// Links the library `src` into `prog` (for its `use web` line at `at`); `intrinsics` are the
/// build's hooks the library calls by their short names (`_emit`).
pub fn link(prog: &mut Program, at: Span, src: &str, intrinsics: &[String]) -> Vec<Diag> {
    let lib = library(src);
    let public: HashSet<String> = lib.funcs.iter().filter(|f| !f.name.starts_with('_')).map(|f| f.name.clone()).collect();
    let params: HashMap<String, Vec<String>> =
        lib.funcs.iter().map(|f| (f.name.clone(), f.params.iter().map(|p| p.name.clone()).collect())).collect();
    let icons: HashSet<String> = icons().into_iter().collect();

    let mut errs = clashes(prog, &public);
    if !errs.is_empty() {
        return errs;
    }
    let msg = MsgInfo::of(prog);
    let user: HashSet<String> = prog.funcs.iter().map(|f| f.name.clone()).collect();
    let mut used_on = false;
    let ctx = Ctx { public: &public, params: &params, icons: &icons, msg: &msg };
    for f in prog.funcs.iter_mut() {
        // only the program's own code: not another standard module's functions
        if f.name.contains('.') && !prog.files.contains_key(&f.name) {
            continue;
        }
        each_stmt_mut(&mut f.body, &mut |_| {}, &mut |e| ctx.call(e, &user, &mut errs, &mut used_on));
    }
    for ex in prog.examples.iter_mut() {
        ex.expr.each_mut(&mut |e| ctx.call(e, &user, &mut errs, &mut used_on));
    }
    if lib.funcs.iter().any(|f| f.name == "_live") {
        errs.extend(live(prog, &public));
    }

    // the library, as `web.name`
    let mut own: Vec<String> = lib.funcs.iter().map(|f| f.name.clone()).collect();
    own.extend(intrinsics.iter().cloned());
    let stateful = own.iter().any(|f| f == "_init");
    for mut f in lib.funcs {
        f.name = format!("web.{}", f.name);
        crate::stdlib::rename_calls(&mut f, "web", &own, at);
        prog.funcs.push(f);
    }
    for mut s in lib.structs {
        s.span = at;
        s.fields.iter_mut().for_each(|x| x.span = at);
        prog.structs.push(s);
    }
    if used_on && msg.update {
        let (toks, _) = crate::lexer::lex("fn _on(m: Msg) -> str = json.str(m)\n");
        let (mut p, _) = crate::parser::parse(toks);
        let mut f = p.funcs.remove(0);
        f.name = "web._on".to_string();
        crate::stdlib::rename_calls(&mut f, "web", &[], at);
        prog.funcs.push(f);
    }
    if stateful {
        declare_state(prog, at);
    }
    errs
}

/// True for an error of `link` that leaves the program whole, so the type checker can still run
/// and report its own errors in the same run: a literal value the library does not take (an
/// unknown word E0374, an icon-only button E0377, raw CSS or HTML that is no literal E0378, a
/// forbidden link E0381) or a mistake of the live views (E0376, E0384). A name clash (E0372) or
/// `on:` without `Msg` (E0373) would only cause more errors.
pub fn independent(d: &Diag) -> bool {
    matches!(d.code, "E0374" | "E0377" | "E0381" | "E0376" | "E0384")
        || (d.code == "E0378" && d.msg.contains("takes only a string literal"))
}

/// The motion options and `keyframes(...)` calls of a program's source whose values are literals
/// (`src/web/motion.rs`, `lint`): an unknown motion word is E0374 and a `keyframes` call that
/// cannot make a word E0388, at the place of the value; a keyframes body that animates a layout
/// property is warning E0375 (the build gives it for computed bodies, and skips the ones given here).
pub fn motion(src: &str) -> Vec<Diag> {
    use crate::web::motion::IssueKind;
    let mut out = Vec::new();
    for issue in crate::web::motion::lint_source(src) {
        let (code, fallback) = match issue.kind {
            IssueKind::UnknownWord => ("E0374", "the motion words are listed under Motion in docs/WEB.md: `nyra explain E0374`"),
            IssueKind::BadKeyframes => {
                ("E0388", "a keyframes word is `keyframes(\"sway\", \"0%, 100% { rotate: -3deg } 50% { rotate: 3deg }\")`")
            }
            IssueKind::LayoutProperty => {
                let hint = issue.hint.unwrap_or_else(|| "animate `translate`, `scale` or `clip-path` instead".to_string());
                crate::diag::warn(Diag::new("E0375", issue.message, issue.span).hint(hint));
                continue;
            }
        };
        out.push(Diag::new(code, issue.message, issue.span).hint(issue.hint.unwrap_or_else(|| fallback.to_string())));
    }
    out
}

/// E0372: a program function, type or script variable with the name of a web function or type.
fn clashes(prog: &Program, public: &HashSet<String>) -> Vec<Diag> {
    let mut errs = Vec::new();
    for f in &prog.funcs {
        if public.contains(&f.name) && !prog.files.contains_key(&f.name) {
            let new = new_name(&f.name);
            errs.push(
                Diag::new("E0372", format!("`fn {}` has the name of the web function `{}`", f.name, f.name), f.span)
                    .hint(format!(
                        "rename your function, e.g. `fn {new}`, and its calls: in a program with `use web` the web functions are called by their bare names"
                    ))
                    .fix(rename(prog, f, &new, public).unwrap_or_default()),
            );
        }
    }
    let types = prog.structs.iter().map(|s| (&s.name, s.span)).chain(prog.enums.iter().map(|e| (&e.name, e.span)));
    for (name, span) in types {
        if TYPES.contains(&name.as_str()) {
            errs.push(
                Diag::new("E0372", format!("the type `{name}` has the name of the web type `{name}`"), span)
                    .hint(format!("rename your type, e.g. `My{name}`: `use web` defines `{name}`")),
            );
        }
    }
    if let Some(main) = prog.funcs.iter().find(|f| f.name == "main").filter(|_| prog.script) {
        for s in &main.body {
            if let StmtKind::Let { name, mutable, .. } = &s.kind {
                if public.contains(name) || name == STATE {
                    let what = if name == STATE { "the site's state".to_string() } else { format!("the web function `{name}`") };
                    errs.push(
                        Diag::new("E0372", format!("the script variable `{name}` has the name of {what}"), s.span).hint(format!(
                            "rename it, e.g. `{} my_{name} = ...`: in a program with `use web` the web functions are called by their bare names",
                            if *mutable { "var" } else { "let" }
                        )),
                    );
                }
            }
        }
    }
    errs
}

/// The name E0372 suggests for a program function named like a web function.
fn new_name(name: &str) -> String {
    match name {
        "img" | "image" => "photo".to_string(),
        _ => format!("my_{name}"),
    }
}

/// The edits that rename the program's `fn old` (named like a web function) to `new`, and every call
/// of it, or `None` when that is not certain: a call whose argument count does not fit the
/// function (it may mean the web function), the old name used as a variable, `new` already a name
/// of the program, or a program of several files.
fn rename(prog: &Program, f: &Func, new: &str, public: &HashSet<String>) -> Option<Vec<Edit>> {
    if !prog.files.is_empty() || public.contains(new) {
        return None;
    }
    let old = f.name.as_str();
    let required = f.params.iter().filter(|p| p.default.is_none()).count();
    let mut edits = vec![Edit::replace(f.span, old, new)];
    // every name the program uses or declares, apart from the calls of `old`
    let mut names: HashSet<String> = HashSet::new();
    let mut certain = true;
    let mut visit = |e: &mut Expr| match &e.kind {
        ExprKind::Call(n, args) if n == old => {
            let fits = args.len() >= required && args.len() <= f.params.len();
            certain &= fits && !args.iter().any(|a| matches!(a.kind, ExprKind::Inout(_)));
            edits.push(Edit::replace(e.span, old, new));
        }
        ExprKind::Call(n, _) | ExprKind::Var(n) | ExprKind::Bind(n, ..) => {
            names.insert(n.clone());
        }
        ExprKind::Lambda(ps, _) => names.extend(ps.iter().map(|(p, _)| p.clone())),
        _ => {}
    };
    let own = || prog.funcs.iter().filter(|g| !g.name.contains('.'));
    let fns: Vec<&str> = own().map(|g| g.name.as_str()).collect();
    let mut bound: Vec<String> = own().flat_map(|g| g.params.iter().map(|p| p.name.clone())).collect();
    let mut stmt = |s: &mut Stmt| match &s.kind {
        StmtKind::Let { name, .. } | StmtKind::For { var: name, .. } => bound.push(name.clone()),
        StmtKind::ForEach { var, index, .. } => bound.extend(std::iter::once(var.clone()).chain(index.clone())),
        _ => {}
    };
    for g in own() {
        let mut body = g.body.clone();
        each_stmt_mut(&mut body, &mut stmt, &mut visit);
    }
    for ex in &prog.examples {
        ex.expr.clone().each_mut(&mut visit);
    }
    names.extend(bound);
    names.extend(prog.structs.iter().map(|s| s.name.clone()).chain(prog.enums.iter().map(|e| e.name.clone())));
    // `old` as a variable or a parameter is not the function (a local may have a web function's name)
    if !certain || names.contains(old) || names.contains(new) || fns.contains(&new) || fns.iter().filter(|g| **g == old).count() > 1 {
        return None;
    }
    Some(edits)
}

/// What the program declares for events: `enum Msg` and its variants, and `fn update(m: Msg)`.
struct MsgInfo {
    /// the variants of `enum Msg` and whether each carries values (`None`: no `enum Msg`)
    variants: Option<Vec<(String, bool)>>,
    update: bool,
}

impl MsgInfo {
    fn of(prog: &Program) -> MsgInfo {
        let variants = prog
            .enums
            .iter()
            .find(|e| e.name == "Msg")
            .map(|e| e.variants.iter().map(|v| (v.name.clone(), !v.fields.is_empty())).collect());
        let update = variants.is_some()
            && prog
                .funcs
                .iter()
                .any(|f| f.name == "update" && f.params.len() == 1 && f.params[0].ty.name() == "Msg" && !f.params[0].inout);
        MsgInfo { variants, update }
    }
}

struct Ctx<'a> {
    public: &'a HashSet<String>,
    params: &'a HashMap<String, Vec<String>>,
    icons: &'a HashSet<String>,
    msg: &'a MsgInfo,
}

impl Ctx<'_> {
    /// A call of a web function by its bare name: renamed, its `on:` made JSON, its literals checked.
    fn call(&self, e: &mut Expr, user: &HashSet<String>, errs: &mut Vec<Diag>, used_on: &mut bool) {
        let span = e.span;
        let ExprKind::Call(name, args) = &mut e.kind else { return };
        if name == "Node" || name == "WebConfig" {
            errs.push(
                Diag::new("E0378", format!("a `{name}` is made by the web functions, not written out"), span).hint(
                    "use the elements and components (`p(...)`, `card(...)`); for markup of your own, `html(\"...\")` with a string literal",
                ),
            );
            return;
        }
        if !self.public.contains(name.as_str()) || user.contains(name.as_str()) {
            return;
        }
        let func = name.clone();
        *name = format!("web.{func}");
        let names = &self.params[&func];
        let given: Vec<String> =
            args.iter().map(|a| if let ExprKind::Labeled(l, _) = &a.kind { l.clone() } else { String::new() }).collect();
        for (i, a) in args.iter_mut().enumerate() {
            let (param, value) = match &mut a.kind {
                ExprKind::Labeled(l, v) => (l.clone(), &mut **v),
                ExprKind::Block(..) => continue,
                _ => (names.get(i).cloned().unwrap_or_default(), a),
            };
            if param == "on" {
                *used_on = true;
                self.on(&func, value, errs);
                continue;
            }
            if func == "gallery" && param == "photos" {
                tag_pairs(value);
            }
            self.literal(&func, &param, value, None, errs);
        }
        // an icon-only button or link needs a label for screen readers
        if matches!(func.as_str(), "button" | "link") && !given.iter().any(|g| g == "label") {
            let text = args.iter().enumerate().find_map(|(i, a)| match &a.kind {
                ExprKind::Labeled(l, v) if l == "text" => Some(&**v),
                ExprKind::Labeled(..) | ExprKind::Block(..) => None,
                _ if i == 0 => Some(a),
                _ => None,
            });
            let icon = args.iter().any(|a| matches!(&a.kind, ExprKind::Labeled(l, _) if l == "icon"));
            let empty = match text.map(|t| &t.kind) {
                Some(ExprKind::Str(s)) => strip_icons(s).trim().is_empty() && (icon || s.contains(':')),
                None => icon,
                _ => false,
            };
            if empty {
                errs.push(
                    Diag::new("E0377", format!("this `{func}` shows only an icon, so a screen reader has nothing to read"), span)
                        .hint(format!("say what it does with `label:`, e.g. `{func}(..., label: \"Call us\")`")),
                );
            }
        }
    }

    /// `on: Msg.X(..)`: the JSON of the value, made by `web._on`; a form's `on: Msg.Send` (a variant
    /// that carries the fields) is written without its value, as the JSON string `"Send"`.
    fn on(&self, func: &str, value: &mut Expr, errs: &mut Vec<Diag>) {
        if !self.msg.update {
            let missing = if self.msg.variants.is_none() { "no `enum Msg`" } else { "no `fn update(m: Msg)`" };
            errs.push(
                Diag::new("E0373", format!("`on:` sends a `Msg` to `update`, but the program has {missing}"), value.span).hint(
                    "declare the events and what they do: `enum Msg { Show(str) }` and `fn update(m: Msg) { match m { Msg.Show(k) => kind = k } }`",
                ),
            );
            return;
        }
        if matches!(func, "form" | "field") {
            if let ExprKind::Field(base, variant) = &value.kind {
                let carries = self.msg.variants.as_ref().and_then(|vs| vs.iter().find(|(n, _)| n == variant)).is_some_and(|(_, c)| *c);
                if matches!(&base.kind, ExprKind::Var(m) if m == "Msg") && carries {
                    value.kind = ExprKind::Str(format!("\"{variant}\""));
                    return;
                }
            }
        }
        let inner = std::mem::replace(value, Expr::new(ExprKind::None, value.span));
        *value = Expr::new(ExprKind::Call("web._on".to_string(), vec![inner]), value.span);
    }

    /// Checks a literal argument (or the literal parts of one) of parameter `param` of `func`.
    fn literal(&self, func: &str, param: &str, value: &Expr, elem: Option<usize>, errs: &mut Vec<Diag>) {
        let k = kind(func, param, elem);
        match &value.kind {
            ExprKind::Str(s) => self.text(func, param, k, s, value.span, true, errs),
            ExprKind::Interp(parts) => {
                if matches!(func, "html" | "css") {
                    raw_needs_literal(func, value.span, errs);
                    return;
                }
                for (i, p) in parts.iter().enumerate() {
                    if let InterpPart::Lit(s) = p {
                        self.text(func, param, k, s, value.span, i == 0, errs);
                    }
                }
            }
            ExprKind::Array(xs) => xs.iter().for_each(|x| self.literal(func, param, x, None, errs)),
            ExprKind::Tuple(xs) => xs.iter().enumerate().for_each(|(i, x)| self.literal(func, param, x, Some(i), errs)),
            _ if matches!(func, "html" | "css") && matches!(param, "raw" | "text") => raw_needs_literal(func, value.span, errs),
            _ => {}
        }
    }

    /// One piece of literal text. `start`: it is the start of the value (where a scheme would be).
    #[allow(clippy::too_many_arguments)]
    fn text(&self, func: &str, param: &str, k: Kind, s: &str, span: Span, start: bool, errs: &mut Vec<Diag>) {
        match k {
            Kind::Url | Kind::Image if start => {
                if let Some(bad) = forbidden_scheme(s, k == Kind::Image) {
                    errs.push(scheme_error(&bad, s, span));
                }
            }
            Kind::Icon if start && !s.is_empty() && !self.icons.contains(s) => {
                // `icon: ":clock:"`: the colons that mark an icon in text, around a known name
                let bare = s.strip_prefix(':').and_then(|x| x.strip_suffix(':')).filter(|x| self.icons.contains(*x));
                match bare {
                    Some(name) => errs.push(
                        Diag::new("E0374", format!("unknown icon `{s}`"), span)
                            .hint(format!(
                                "`{param}:` takes the icon's name without colons: `{param}: \"{name}\"` (colons mark an icon inside text)"
                            ))
                            .fix(vec![Edit::replace(span, &format!("\"{s}\""), format!("\"{name}\""))]),
                    ),
                    // an icon is decoration: the site still builds (and draws a stand-in)
                    None => crate::diag::warn(icon_warning(s, false, span)),
                }
            }
            Kind::Text => {
                for name in icon_names(s) {
                    if !self.icons.contains(&name) {
                        crate::diag::warn(icon_warning(&name, true, span));
                    }
                }
                for url in md_links(s) {
                    if let Some(bad) = forbidden_scheme(&url, false) {
                        errs.push(scheme_error(&bad, &url, span));
                    }
                }
            }
            Kind::Plain if start => {
                if let Some(ws) = words(func, param) {
                    if !s.is_empty() && !ws.contains(&s) {
                        let hint = match suggest(s, ws.iter().copied()) {
                            Some(h) => format!("{h} The words of `{param}:` are {}", list(ws)),
                            None => format!("the words of `{param}:` are {}", list(ws)),
                        };
                        errs.push(Diag::new("E0374", format!("unknown word `{s}` for `{param}:` of `{func}`"), span).hint(hint));
                    }
                }
            }
            _ => {}
        }
    }
}

/// `gallery([(image, alt), ...])`: a literal list of pairs, each completed with the empty tag (a
/// list of pairs in a variable is mapped by the checker, `photo_pairs`).
fn tag_pairs(value: &mut Expr) {
    let ExprKind::Array(items) = &mut value.kind else { return };
    if items.is_empty() || !items.iter().all(|x| matches!(&x.kind, ExprKind::Tuple(parts) if parts.len() == 2)) {
        return;
    }
    for x in items.iter_mut() {
        let at = x.span;
        if let ExprKind::Tuple(parts) = &mut x.kind {
            parts.push(Expr::new(ExprKind::Str(String::new()), at));
        }
    }
}

fn raw_needs_literal(func: &str, span: Span, errs: &mut Vec<Diag>) {
    errs.push(
        Diag::new("E0378", format!("`{func}(...)` takes only a string literal"), span).hint(format!(
            "write the {} out as one literal without `{{ }}`: data never becomes markup; for text from data use the elements (`p(text)`), which escape it",
            if func == "css" { "CSS" } else { "markup" }
        )),
    );
}

/// The edit distance of two names (a swap of neighbours counts as one edit).
fn edits(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut d = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in d[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            d[i][j] = (d[i - 1][j] + 1).min(d[i][j - 1] + 1).min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1);
            }
        }
    }
    d[a.len()][b.len()]
}

fn scheme_error(scheme: &str, url: &str, span: Span) -> Diag {
    Diag::new("E0381", format!("the link `{url}` uses the scheme `{scheme}:`, which a page may not link to"), span).hint(
        "link to `https://...`, `mailto:...`, `tel:...`, a path (`/about`) or an anchor (`#contact`); `javascript:` and `data:` run or carry code",
    )
}

fn list(ws: &[&str]) -> String {
    ws.iter().map(|w| format!("`{w}`")).collect::<Vec<_>>().join(", ")
}

/// The scheme of `url` if a page may not use it (images: only http and https).
fn forbidden_scheme(url: &str, image: bool) -> Option<String> {
    let u = url.trim();
    let mut scheme = String::new();
    for (i, c) in u.chars().enumerate() {
        if c == ':' {
            break;
        }
        if !(c.is_ascii_alphabetic() || (i > 0 && (c.is_ascii_digit() || matches!(c, '+' | '-' | '.')))) {
            return None;
        }
        scheme.push(c.to_ascii_lowercase());
    }
    if scheme.is_empty() || scheme.len() == u.len() {
        return None;
    }
    let ok: &[&str] = if image { &["http", "https"] } else { &["http", "https", "mailto", "tel"] };
    (!ok.contains(&scheme.as_str())).then_some(scheme)
}

/// The icon names `:name:` in a text (the same rule as the library's `_md`).
fn icon_names(s: &str) -> Vec<String> {
    let cs: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < cs.len() {
        if cs[i] == '\\' {
            i += 2;
            continue;
        }
        if cs[i] == ':' && i + 1 < cs.len() && cs[i + 1].is_ascii_lowercase() {
            let mut j = i + 1;
            while j < cs.len() && (cs[j].is_ascii_lowercase() || cs[j].is_ascii_digit() || cs[j] == '-') {
                j += 1;
            }
            if j < cs.len() && cs[j] == ':' {
                out.push(cs[i + 1..j].iter().collect());
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// A text without its icons.
fn strip_icons(s: &str) -> String {
    let mut out = s.to_string();
    for name in icon_names(s) {
        out = out.replace(&format!(":{name}:"), "");
    }
    out
}

/// The targets of the Markdown links `[label](url)` in a text.
fn md_links(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(i) = rest.find("](") {
        let after = &rest[i + 2..];
        match after.find(')') {
            Some(j) if !after[..j].contains('\n') => {
                out.push(after[..j].to_string());
                rest = &after[j..];
            }
            _ => rest = after,
        }
    }
    out
}

/// Declares the site's state, `var _web = web._init()`, before the program's first statement. A
/// program that has only `fn main` gets a script `main` that declares it and calls the program's.
fn declare_state(prog: &mut Program, at: Span) {
    let decl = Stmt {
        kind: StmtKind::Let {
            name: STATE.to_string(),
            mutable: true,
            ty: None,
            value: Expr::new(ExprKind::Call("web._init".to_string(), Vec::new()), at),
        },
        span: at,
    };
    if prog.script {
        if let Some(main) = prog.funcs.iter_mut().find(|f| f.name == "main") {
            main.body.insert(0, decl);
        }
        return;
    }
    let Some(user) = prog.funcs.iter_mut().find(|f| f.name == "main") else { return };
    user.name = USER_MAIN.to_string();
    let call = Stmt { kind: StmtKind::Expr(Expr::new(ExprKind::Call(USER_MAIN.to_string(), Vec::new()), at)), span: at };
    prog.funcs.push(Func { name: "main".to_string(), params: Vec::new(), ret: Type::Void, body: vec![decl, call], span: at });
    prog.script = true;
}

// ---------------------------------------------------------------------------------------------
// Live regions

/// What one function does with the script variables, directly.
#[derive(Default)]
struct Facts {
    reads: BTreeSet<String>,
    writes: BTreeSet<String>,
    calls: BTreeSet<String>,
    /// `fs`, `os` or `input` calls: (module, span)
    effects: Vec<(String, Span)>,
}

/// Finds the live views and wraps their calls (see the module comment); E0376 and E0384.
fn live(prog: &mut Program, public: &HashSet<String>) -> Vec<Diag> {
    let mut errs = Vec::new();
    if !prog.funcs.iter().any(|f| f.name == "update") || !prog.script {
        return errs;
    }
    let globals: HashSet<String> = match prog.funcs.iter().find(|f| f.name == "main") {
        Some(m) => {
            m.body.iter().filter_map(|s| if let StmtKind::Let { name, .. } = &s.kind { Some(name.clone()) } else { None }).collect()
        }
        None => return errs,
    };
    let names: HashSet<String> = prog.funcs.iter().map(|f| f.name.clone()).collect();
    let mut facts: HashMap<String, Facts> = HashMap::new();
    for f in &prog.funcs {
        if f.name == "main" || (f.name.contains('.') && !prog.files.contains_key(&f.name)) {
            continue;
        }
        facts.insert(f.name.clone(), facts_of(f, &globals, &names, public));
    }
    let closure = |start: &str, pick: &dyn Fn(&Facts) -> &BTreeSet<String>| -> BTreeSet<String> {
        let mut seen: HashSet<String> = HashSet::new();
        let mut out = BTreeSet::new();
        let mut todo = vec![start.to_string()];
        while let Some(n) = todo.pop() {
            if !seen.insert(n.clone()) {
                continue;
            }
            if let Some(fa) = facts.get(&n) {
                out.extend(pick(fa).iter().cloned());
                todo.extend(fa.calls.iter().cloned());
            }
        }
        out
    };
    let changed = closure("update", &|f| &f.writes);
    if changed.is_empty() {
        return errs;
    }
    let mut views: Vec<String> = Vec::new();
    let mut with_params: Vec<String> = Vec::new();
    for f in &prog.funcs {
        if !facts.contains_key(&f.name) || f.name == "update" || f.ret.name() != "Node" {
            continue;
        }
        if closure(&f.name, &|x| &x.reads).iter().any(|v| changed.contains(v)) {
            if f.params.is_empty() {
                views.push(f.name.clone());
            } else {
                with_params.push(f.name.clone());
            }
        }
    }
    // the code that runs in the browser: `update`, the views and what they call
    let mut client: Vec<String> = vec!["update".to_string()];
    client.extend(views.iter().cloned());
    let mut seen: HashSet<String> = HashSet::new();
    while let Some(n) = client.pop() {
        if !seen.insert(n.clone()) {
            continue;
        }
        if let Some(fa) = facts.get(&n) {
            for (m, span) in &fa.effects {
                errs.push(
                    Diag::new("E0384", format!("`{n}` runs in the browser (as `update`, a live view or a function they call), but it uses `{m}`"), *span).hint(
                        format!("the browser has no files, arguments or standard input for `{m}`: read them while the site is built, into a script variable, and use that"),
                    ),
                );
            }
            client.extend(fa.calls.iter().cloned());
        }
    }
    for f in prog.funcs.iter_mut() {
        if views.contains(&f.name) || f.name == "update" || (f.name.contains('.') && !prog.files.contains_key(&f.name)) {
            continue;
        }
        each_stmt_mut(&mut f.body, &mut |_| {}, &mut |e| {
            let ExprKind::Call(name, args) = &e.kind else { return };
            if args.is_empty() && views.contains(name) {
                let name = name.clone();
                let span = e.span;
                let inner = std::mem::replace(e, Expr::new(ExprKind::None, span));
                // (the inner call is renamed back below: this walk goes on into the new call)
                let inner = match inner.kind {
                    ExprKind::Call(n, a) => Expr::new(ExprKind::Call(format!("{HELD}{n}"), a), span),
                    k => Expr::new(k, span),
                };
                *e = Expr::new(ExprKind::Call("web._live".to_string(), vec![Expr::new(ExprKind::Str(name), span), inner]), span);
            } else if with_params.contains(name) {
                errs.push(
                    Diag::new("E0376", format!("`{name}` reads state that `update` changes, so it must render again in the browser, but it has parameters"), e.span)
                        .hint(format!("give `fn {name}` no parameters and let it read the script variables it needs: the browser calls a live view again with no arguments")),
                );
            }
        });
    }
    for f in prog.funcs.iter_mut() {
        each_stmt_mut(&mut f.body, &mut |_| {}, &mut |e| {
            if let ExprKind::Call(n, _) = &mut e.kind {
                if let Some(plain) = n.strip_prefix(HELD) {
                    *n = plain.to_string();
                }
            }
        });
    }
    errs
}

/// Marks a call of a live view while its calls are being wrapped.
const HELD: &str = "\u{1}";

fn facts_of(f: &Func, globals: &HashSet<String>, funcs: &HashSet<String>, public: &HashSet<String>) -> Facts {
    let mut own: HashSet<String> = f.params.iter().map(|p| p.name.clone()).collect();
    let mut decls = Vec::new();
    crate::check::collect_decls(&f.body, &mut decls);
    own.extend(decls.into_iter().map(|(n, _)| n));
    let mut fa = Facts::default();
    let global = |n: &str| globals.contains(n) && !own.contains(n);
    let mut body = f.body.clone();
    let mut assigned = Vec::new();
    each_stmt_mut(
        &mut body,
        &mut |s| {
            if let StmtKind::Assign { target, .. } = &s.kind {
                if let Some(r) = root(target).filter(|r| global(r)) {
                    assigned.push(r);
                }
            }
        },
        &mut |e| match &e.kind {
            ExprKind::Var(n) if global(n) => {
                fa.reads.insert(n.clone());
            }
            ExprKind::Call(n, args) => {
                if funcs.contains(n) && !public.contains(n) {
                    fa.calls.insert(n.clone());
                }
                for a in args {
                    if let ExprKind::Inout(p) = &a.kind {
                        if let Some(r) = root(p).filter(|r| global(r)) {
                            fa.writes.insert(r);
                        }
                    }
                }
            }
            ExprKind::Method(recv, m, _) => {
                if let ExprKind::Var(module) = &recv.kind {
                    if matches!(module.as_str(), "fs" | "os" | "input") && !own.contains(module) && !globals.contains(module) {
                        fa.effects.push((format!("{module}.{m}"), e.span));
                    }
                }
                if MUTATORS.contains(&m.as_str()) {
                    if let Some(r) = root(recv).filter(|r| global(r)) {
                        fa.writes.insert(r);
                    }
                }
            }
            _ => {}
        },
    );
    fa.writes.extend(assigned);
    fa
}

/// The methods that change the array or map they are called on.
const MUTATORS: &[&str] = &["push", "pop", "insert", "remove", "swap", "sort", "reverse", "set", "sort_by", "clear"];

/// The variable at the root of a place: `a` of `a.b[3].c`.
fn root(e: &Expr) -> Option<String> {
    match &e.kind {
        ExprKind::Var(n) => Some(n.clone()),
        ExprKind::Field(b, _) | ExprKind::Index(b, _) => root(b),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_library_parses_and_has_its_icons() {
        let fs = functions();
        for f in ["site", "page", "section", "hero", "navbar", "gallery", "hours", "map", "form", "field", "footer"] {
            assert!(fs.iter().any(|(n, _)| n == f), "{f}");
        }
        let icons = icons();
        for i in
            ["phone", "mail", "map-pin", "clock", "leaf", "flame", "home", "facebook", "send", "navigation", "alert-circle", "circle"]
        {
            assert!(icons.iter().any(|n| n == i), "{i}");
        }
        // a name of another set is no icon of the set, but stands for one
        assert!(!icons.iter().any(|n| n == "envelope"));
        assert_eq!(icon_like("envelope"), Some("mail"));
        assert_eq!(icon_like("map-marker"), Some("map-pin"));
        assert_eq!(icon_like("siren"), None);
        for (other, icon) in [("warning", "triangle-alert"), ("cog", "settings")] {
            assert_eq!(icon_like(other), Some(icon));
            assert!(icons.iter().any(|n| n == icon), "{icon}");
        }
    }

    #[test]
    fn schemes_and_markdown_links() {
        assert_eq!(forbidden_scheme("javascript:alert(1)", false).as_deref(), Some("javascript"));
        assert_eq!(forbidden_scheme(" JavaScript:x", false).as_deref(), Some("javascript"));
        assert_eq!(forbidden_scheme("data:text/html,x", false).as_deref(), Some("data"));
        assert_eq!(forbidden_scheme("mailto:a@b.cz", false), None);
        assert_eq!(forbidden_scheme("mailto:a@b.cz", true).as_deref(), Some("mailto"));
        assert_eq!(forbidden_scheme("/a:b", false), None);
        assert_eq!(forbidden_scheme("#kontakt", false), None);
        assert_eq!(md_links("[a](https://x.cz) and [b](javascript:x)"), vec!["https://x.cz", "javascript:x"]);
        assert_eq!(icon_names(":phone: 8:00 \\:x: :map-pin:"), vec!["phone", "map-pin"]);
    }
}
