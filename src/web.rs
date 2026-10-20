//! `use web`: a program that makes a website. `nyra build site.nyra` writes the site to `dist/`,
//! `nyra run site.nyra` serves it with live reload (`web/serve.rs`), and `nyra check site.nyra`
//! runs the build in memory for its checks. The contract with the web library is in docs/WEB.md,
//! section "Build contract".
//!
//! How a site is built:
//!
//! 1. The program runs once in the IR interpreter (deterministic, with the sandbox's limits and no
//!    capabilities unless `--allow` gives them). The web library hands each page to the build with
//!    the intrinsic `_emit(path, html)`, the kit parts it uses with `_uses(chunk)`, the theme and
//!    raw CSS with `_css(text)`, and custom motion words with `_keyframes(name, body)`. For a program
//!    with interactive state, the build appends `_state(name, json.str(var))` to the end of the
//!    script for each script variable the browser needs (`web/client.rs`).
//! 2. Each page is checked: `#anchor` links (E0382), its headings (E0379), its images (E0383), and
//!    the theme's contrast once (E0380). Two pages with one path, or a bad path, is E0387.
//! 3. Images are copied to `img/` with a content hash, get `width`/`height`, `loading="lazy"` and
//!    `decoding="async"`, WebP variants with `srcset` and a blurred preview from the built-in codec (`web/img/`;
//!    E0386 for a file it cannot read).
//! 4. One CSS file and one JS file are assembled from only the used parts (`web/assets.rs`) and the
//!    client code (`web/client.rs`), named by a hash of their content.
//! 5. Each page gets a Content-Security-Policy `<meta>` computed from what it uses, the CSS and JS
//!    links; the site gets `sitemap.xml` (when the pages have canonical URLs), `robots.txt` and
//!    `_headers` (the same policy as an HTTP header, for hosts that read that file).

pub mod assets;
pub mod client;
/// SHA-256 and Base64 (the hash of an inline style in the policy, `data:` URLs).
pub mod digest;
/// Easing presets and springs compiled to CSS `linear()` (WP4).
pub mod ease;
/// The gzipped size of a file, for the size budgets.
pub mod gzip;
pub mod html;
pub mod image;
/// The built-in image codec (not in the WebAssembly playground, which converts no images).
#[cfg(not(target_arch = "wasm32"))]
pub mod img;
/// The motion words: their CSS and JS parts, the `keyframes` checks (WP4).
pub mod motion;
#[cfg(not(target_arch = "wasm32"))]
pub mod serve;

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use crate::ast::{self, Expr, ExprKind, Span, Stmt, StmtKind};
use crate::diag::{self, Diag};
use crate::ir::interp::RuntimeError;
use crate::{caps, sandbox};

/// The path of the live-reload script of the dev server (added to pages only by `nyra run`).
pub const RELOAD_JS: &str = "/__nyra/reload.js";
/// The path the reload script polls: the number of the current build.
pub const VERSION_PATH: &str = "/__nyra/version";
/// The file that marks a folder as written by `nyra build` (the files it wrote, one per line).
pub const MANIFEST: &str = ".nyra-build";

/// True if the source imports the web library (a line `use web`).
pub fn uses_web(src: &str) -> bool {
    src.lines().any(|l| {
        let mut w = l.split_whitespace();
        w.next() == Some("use") && w.next().map(|m| m.trim_end_matches(';')) == Some("web")
    })
}

/// How to build.
pub struct Options {
    /// The program's folder: images are read from `assets/` in it (or from it).
    pub dir: PathBuf,
    /// The program's name in messages and in the browser's runtime errors.
    pub file: String,
    /// `--image-tool`: a tool's name or `none`; `None`: the first one found on the PATH.
    pub image_tool: Option<String>,
    /// Make WebP variants (a real build); `nyra check` and the MCP tool only look.
    pub convert: bool,
    /// A missing image is error E0383 (else the same message as a warning: the dev server, MCP).
    pub strict_images: bool,
    /// Add the dev server's live-reload script to the pages.
    pub dev: bool,
    /// The program's own `print` output goes to stdout (else it is dropped).
    pub to_stdout: bool,
}

/// A built site, in memory.
pub struct Site {
    /// Every file, by its path in the output folder (`about/index.html`), sorted.
    pub files: BTreeMap<String, Vec<u8>>,
    /// The page paths, in the order the program made them.
    pub pages: Vec<String>,
    /// The policy for the `Content-Security-Policy` header (the union of the pages').
    pub csp: String,
}

/// Why a build failed.
pub enum Error {
    /// Compile errors, or the build's own errors (E0382, E0383, E0387).
    Diags(Vec<Diag>),
    /// The program stopped with a runtime error (or a limit) while it ran at build time.
    Runtime(Box<RuntimeError>),
    /// Anything else, said in a sentence.
    Text(String),
}

impl Error {
    /// The error for people (`file` names the program).
    pub fn human(&self, file: &str, src: &str) -> String {
        match self {
            Error::Diags(d) => format!("{}nyra: {} error(s)\n", diag::render_human(d, file, src), d.len()),
            Error::Runtime(e) => {
                format!("{}\nnyra: the program stopped while the site was built\n", sandbox::render_error(e, file, false))
            }
            Error::Text(t) => format!("nyra: {t}\n"),
        }
    }

    /// The error as the JSON of `--json` (`{"ok":false,"errors":[..]}`, warnings included).
    pub fn json(&self, file: &str) -> String {
        match self {
            Error::Diags(d) => diag::render_json(d, file),
            Error::Runtime(e) => sandbox::render_error(e, file, true),
            Error::Text(t) => format!(
                "{{\"ok\":false,\"errors\":[{{\"code\":null,\"message\":{},\"file\":{},\"line\":0,\"col\":0,\"hint\":null}}]{}}}",
                diag::json_str(t),
                diag::json_str(file),
                diag::warnings_json(file)
            ),
        }
    }
}

// ---- running the program -----------------------------------------------------------------------

/// Builds the site of a checked program (`prog`, compiled from `src`). Warnings go to `diag::warn`.
pub fn build(prog: ast::Program, src: &str, o: &Options) -> Result<Site, Error> {
    let at = prog.uses.iter().find(|u| u.module == "web").map_or(Span { line: 1, col: 1 }, |u| u.span);
    let capture = client::state_vars(&prog);
    let prog = if capture.is_empty() { prog } else { with_state(src, &capture, at)? };
    let m = sandbox::module(&prog).map_err(Error::Text)?;
    let mut cfg = sandbox::Config::new();
    cfg.to_stdout = o.to_stdout;
    cfg.stdin = Some(Vec::new());
    let report = sandbox::run(&m, cfg);
    if let Some(e) = report.error {
        return Err(Error::Runtime(Box::new(e)));
    }
    if let Some(what) = report.internal {
        return Err(Error::Text(format!("internal error in the interpreter: {what}; please report this bug")));
    }
    if report.exit != 0 {
        return Err(Error::Text(format!("the program stopped with exit code {} while the site was built", report.exit)));
    }
    Site::assemble(&prog, report.web, at, o, src)
}

/// The program compiled again with `web._state("name", json.str(name))` at the end of the script
/// for each script variable in `vars`: the build records the values the browser starts from.
fn with_state(src: &str, vars: &[String], at: Span) -> Result<ast::Program, Error> {
    let saved = diag::warnings();
    let fail = |errs: Vec<Diag>| Error::Diags(errs);
    let (toks, errs) = crate::lexer::lex(src);
    if !errs.is_empty() {
        return Err(fail(errs));
    }
    let (mut prog, errs) = crate::parser::parse(toks);
    if !errs.is_empty() {
        return Err(fail(errs));
    }
    let errs = crate::modules::load(&mut prog);
    if !errs.is_empty() {
        return Err(fail(errs));
    }
    if !prog.uses.iter().any(|u| u.module == "json") {
        prog.uses.push(ast::Use { module: "json".into(), span: at, path: None });
    }
    let errs = crate::stdlib::link(&mut prog);
    if !errs.is_empty() {
        return Err(fail(errs));
    }
    let e = |kind: ExprKind| Expr::new(kind, at);
    if let Some(main) = prog.funcs.iter_mut().find(|f| f.name == "main") {
        for v in vars {
            let json = e(ExprKind::Method(Box::new(e(ExprKind::Var("json".into()))), "str".into(), vec![e(ExprKind::Var(v.clone()))]));
            let call = e(ExprKind::Call("web._state".into(), vec![e(ExprKind::Str(v.clone())), json]));
            main.body.push(Stmt { kind: StmtKind::Expr(call), span: at });
        }
    }
    let errs = crate::check::check(&mut prog);
    diag::clear_warnings();
    saved.into_iter().for_each(diag::warn);
    if !errs.is_empty() {
        return Err(fail(errs));
    }
    Ok(prog)
}

// ---- pages ---------------------------------------------------------------------------------------

struct Page {
    path: String,
    /// The file in the output folder.
    file: String,
    /// `../` times the folders above the file, for links to the site's own files.
    up: String,
    html: String,
    span: Span,
    tags: Vec<html::Tag>,
}

/// The file of a page path: `/` is `index.html`, `/about` and `/about/` are `about/index.html`,
/// `/404` is `404.html`, and a path ending in `.html` is that file. `Err`: why the path is not one.
pub fn page_file(path: &str) -> Result<String, String> {
    if !path.starts_with('/') {
        return Err("a page path starts with `/`".into());
    }
    if let Some(c) = path.chars().find(|c| !(c.is_ascii_alphanumeric() || "-_./~%".contains(*c))) {
        return Err(format!("a page path has only letters, digits and `-_./~%` (found `{c}`)"));
    }
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    if path.contains("//") || parts.iter().any(|p| *p == "." || *p == "..") {
        return Err("a page path has no empty part, `.` or `..`".into());
    }
    Ok(match parts.as_slice() {
        [] => "index.html".into(),
        ["404"] => "404.html".into(),
        [.., last] if last.ends_with(".html") => parts.join("/"),
        _ => format!("{}/index.html", parts.join("/")),
    })
}

/// The path a link points to, as the pages are keyed: no trailing `/` (except `/`), no `.html` of
/// an index file.
fn norm_path(p: &str) -> String {
    let p = p.strip_suffix("index.html").unwrap_or(p);
    let p = p.trim_end_matches('/');
    if p.is_empty() {
        "/".into()
    } else {
        p.to_string()
    }
}

// ---- images --------------------------------------------------------------------------------------

/// An image of the site as the pages refer to it.
struct Img {
    /// Its file in the output folder.
    file: String,
    size: Option<(u32, u32)>,
    /// `srcset` entries (file, width), when there are WebP variants.
    variants: Vec<(String, u32)>,
    /// A JPEG copy for `og:image`, when the file itself does not suit.
    og: Option<String>,
    /// A `data:` URL of a tiny preview, shown blurred behind the image while it loads.
    preview: Option<String>,
}

/// The file of a relative reference: in `assets/` of the program's folder, else in the folder.
fn find_asset(dir: &Path, r: &str) -> Option<PathBuf> {
    let r = r.trim_start_matches("./").trim_start_matches('/');
    let r = r.split(['?', '#']).next().unwrap_or(r);
    if r.is_empty() || r.split(['/', '\\']).any(|p| p == "..") || r.contains(':') {
        return None;
    }
    [dir.join("assets").join(r), dir.join(r)].into_iter().find(|p| p.is_file())
}

/// A reference to a file of the site itself (not `https://...`, `data:`, `mailto:`, `#x`).
fn local_ref(v: &str) -> bool {
    !v.is_empty() && !v.starts_with('#') && !v.starts_with("//") && !v.contains(':')
}

/// `name-0123abcd.ext` for the bytes of `path`.
fn hashed(path: &Path, bytes: &[u8], extra: &str) -> String {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
    let stem: String = stem.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect();
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("bin").to_ascii_lowercase();
    format!("{stem}{extra}-{}.{ext}", &hash(bytes)[..8])
}

/// The first hex digits of the FNV-1a hash of `bytes`.
pub fn hash(bytes: &[u8]) -> String {
    format!("{:016x}", crate::fnv1a(&[bytes]))
}

// ---- the policy ------------------------------------------------------------------------------------

/// A Content-Security-Policy as directives with their sources, in a fixed order.
#[derive(Default, Clone)]
struct Policy(BTreeMap<usize, (String, Vec<String>)>);

const DIRECTIVES: [&str; 11] = [
    "default-src",
    "script-src",
    "style-src",
    "img-src",
    "font-src",
    "frame-src",
    "media-src",
    "connect-src",
    "form-action",
    "base-uri",
    "object-src",
];

impl Policy {
    fn add(&mut self, directive: &str, source: &str) {
        let k = DIRECTIVES.iter().position(|d| *d == directive).unwrap_or(DIRECTIVES.len());
        let e = self.0.entry(k).or_insert_with(|| (directive.to_string(), Vec::new()));
        if !e.1.iter().any(|s| s == source) {
            e.1.push(source.to_string());
        }
    }

    fn has(&self, directive: &str, source: &str) -> bool {
        self.0.values().any(|(d, s)| d == directive && s.iter().any(|x| x == source))
    }

    fn remove_hashes(&mut self, directive: &str) {
        for (d, s) in self.0.values_mut() {
            if d == directive {
                s.retain(|x| !x.starts_with("'sha256-"));
            }
        }
    }

    fn merge(&mut self, other: &Policy) {
        for (d, sources) in other.0.values() {
            for s in sources {
                self.add(d, s);
            }
        }
    }

    fn text(&self) -> String {
        self.0.values().map(|(d, s)| format!("{d} {}", s.join(" "))).collect::<Vec<_>>().join("; ")
    }
}

// ---- style attributes --------------------------------------------------------------------------------

/// The class that stands for the declarations of a `style` attribute, and its rule for the site's
/// CSS, or `None` when the declarations cannot go into a CSS file as they are (a brace, a comment, an
/// at-sign or a backslash could end the rule). The declarations are `!important`, so they beat the
/// kit's rules as the attribute did.
fn style_rule(style: &str) -> Option<(String, String)> {
    let style = style.trim().trim_end_matches(';').trim();
    if style.is_empty() || style.contains(['{', '}', '<', '@', '\\']) || style.contains("/*") {
        return None;
    }
    let mut decls = Vec::new();
    let (mut depth, mut quote, mut from) = (0i32, None::<char>, 0);
    let mut parts = Vec::new();
    for (i, c) in style.char_indices() {
        match (c, quote) {
            ('"' | '\'', None) => quote = Some(c),
            (q, Some(open)) if q == open => quote = None,
            (_, Some(_)) => {}
            ('(', None) => depth += 1,
            (')', None) => depth -= 1,
            (';', None) if depth == 0 => {
                parts.push(&style[from..i]);
                from = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&style[from..]);
    for d in parts.iter().map(|d| d.trim()).filter(|d| !d.is_empty()) {
        let (name, value) = d.split_once(':')?;
        let value = value.trim();
        let value = value.strip_suffix("!important").map_or(value, str::trim_end);
        decls.push(format!("{}:{value} !important", name.trim()));
    }
    let body = decls.join(";");
    let class = format!("s-{}", &hash(body.as_bytes())[..8]);
    Some((class.clone(), format!(".{class}{{{body}}}")))
}

/// The page with each `style` attribute replaced by its class (`style_rule`), whose rule goes into
/// `rules`: a page's policy then needs no `'unsafe-inline'` for its styles. An attribute that cannot
/// be a rule stays (and the page's policy allows it).
fn style_classes(page: &str, rules: &mut BTreeMap<String, String>) -> String {
    let mut edits = Vec::new();
    for t in html::tags(page).into_iter().filter(|t| !t.close && t.has("style")) {
        let Some((class, rule)) = style_rule(t.get("style").unwrap_or("")) else { continue };
        let mut changed = t.clone();
        changed.attrs.retain(|(n, _)| n != "style");
        let classes = t.get("class").map_or(class.clone(), |c| format!("{c} {class}"));
        changed.set("class", &classes);
        rules.insert(class, rule);
        edits.push((t.start, t.end, changed.render()));
    }
    html::apply(page, edits)
}

/// The place of the first `needle` in the program's source (a value the library found wrong while
/// the site was built, as the string literal that holds it).
fn locate(src: &str, needle: &str) -> Option<Span> {
    if needle.is_empty() || needle == "\"\"" {
        return None;
    }
    let at = src.find(needle)?;
    let before = &src[..at];
    let line = before.matches('\n').count() + 1;
    let col = before[before.rfind('\n').map_or(0, |i| i + 1)..].chars().count() + 1;
    Some(Span { line, col })
}

/// The hint of E0390 (opening hours the badge cannot read).
const HOURS_HINT: &str = "a row is (label, \"days times\"): `(\"Mon–Fri\", \"1-5 8:00-16:00\")`, days 1 (Monday) to 7 (Sunday), one or more times `8:00-12:00 13:00-17:00`, \"\" for closed; the days may also be in the label (`(\"Monday\", \"8:00-18:00\")`, `(\"Mon–Fri\", \"8–16\")`)";

/// `https://host[:port]` of an absolute http(s) URL.
fn origin(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://").map(|r| ("https", r)).or_else(|| url.strip_prefix("http://").map(|r| ("http", r)))?;
    let host = rest.1.split(['/', '?', '#']).next()?;
    (!host.is_empty()).then(|| format!("{}://{host}", rest.0))
}

// ---- assembling the site ---------------------------------------------------------------------------

impl Site {
    fn assemble(prog: &ast::Program, web: crate::ir::host::Web, at: Span, o: &Options, src: &str) -> Result<Site, Error> {
        let mut errs: Vec<Diag> = Vec::new();
        let mut warn = |d: Diag| diag::warn(d);

        // the pages and their paths (E0387)
        let mut pages: Vec<Page> = Vec::new();
        for (path, text, span) in web.pages {
            match page_file(&path) {
                Err(why) => {
                    errs.push(Diag::new("E0387", format!("page path \"{path}\" is not a path of the site: {why}"), span).hint(
                        "write the path from the site's root, as in the address bar: `page(\"/\", ...)`, `page(\"/about\", ...)`",
                    ))
                }
                Ok(file) => {
                    if let Some(first) = pages.iter().find(|p| p.file == file) {
                        let same = if first.path == path {
                            String::new()
                        } else {
                            format!(" (\"{}\" and \"{path}\" are one file)", first.path)
                        };
                        errs.push(
                            Diag::new("E0387", format!("two pages have the path \"{path}\"{same}"), span)
                                .hint("each page needs its own path: rename one of them, or make one page of the two"),
                        );
                        continue;
                    }
                    let up = "../".repeat(file.matches('/').count());
                    let tags = html::tags(&text);
                    pages.push(Page { path: norm_path(&path), file, up, html: text, span, tags });
                }
            }
        }
        // errors and warnings the library found while rendering (a computed `javascript:` link, an
        // unknown color or icon): `_css("/*! nyra:error E0381 message */")`, at the string literal
        // of the program that holds the value (a value from data, `icon: s.0`, has no place of its own)
        for text in &web.css {
            for part in text.split("/*! nyra:warn ").skip(1) {
                let what = part.split(" */").next().unwrap_or(part);
                let mut words = what.splitn(3, ' ');
                let (code, kind, name) = (words.next().unwrap_or(""), words.next().unwrap_or(""), words.next().unwrap_or(""));
                if code == "E0389" {
                    let in_text = kind == "text";
                    let needle = if in_text { format!(":{name}:") } else { format!("\"{name}\"") };
                    let d = crate::webcheck::icon_warning(name, in_text, locate(src, &needle).unwrap_or(at));
                    // (a literal was warned about when the program compiled)
                    if !diag::warnings().iter().any(|w| w.code == d.code && w.msg == d.msg) {
                        warn(d);
                    }
                }
            }
            for part in text.split("/*! nyra:error ").skip(1) {
                let msg = part.split(" */").next().unwrap_or(part);
                let (code, msg) = msg.split_once(' ').unwrap_or((msg, ""));
                let (code, hint): (&'static str, &str) = match code {
                    "E0381" => ("E0381", "link to `https://...`, `mailto:...`, `tel:...`, a path or an #anchor; the value was computed while the site was built"),
                    "E0390" => ("E0390", HOURS_HINT),
                    _ => ("E0374", "use a hex color (`#0F766E`), a theme color (`primary`, `accent`, `bg`, `alt`, `ink`, `muted`) or a name of `site(palette:)`"),
                };
                let value = msg.split('`').nth(1).unwrap_or("");
                let place = locate(src, &format!("\"{value}\"")).or_else(|| locate(src, value).filter(|_| !value.is_empty()));
                let d = Diag::new(code, msg.to_string(), place.unwrap_or(at)).hint(hint);
                if !errs.iter().any(|e| e.code == d.code && e.msg == d.msg) {
                    errs.push(d);
                }
            }
        }
        if pages.is_empty() && errs.is_empty() {
            return Err(Error::Text(
                "the program made no page: a `use web` program calls `page(\"/\", title) { ... }` at least once".into(),
            ));
        }

        // anchors (E0382) and headings (E0379)
        let ids: Vec<Vec<String>> = pages
            .iter()
            .map(|p| {
                p.tags
                    .iter()
                    .filter(|t| !t.close)
                    .flat_map(|t| t.get("id").into_iter().chain(t.get("name").filter(|_| t.name == "a")))
                    .map(str::to_string)
                    .collect()
            })
            .collect();
        for (pi, p) in pages.iter().enumerate() {
            let mut reported: Vec<String> = Vec::new();
            for t in p.tags.iter().filter(|t| !t.close) {
                let Some(href) = t.get("href") else { continue };
                let Some((target, id)) = href.split_once('#') else { continue };
                let id = html::decode(id);
                if id.is_empty() || id == "top" || id.contains(['/', '?']) {
                    continue;
                }
                let target_page = if target.is_empty() {
                    Some(pi)
                } else if target.starts_with('/') && !target.starts_with("//") {
                    let t = norm_path(target);
                    pages.iter().position(|q| q.path == t)
                } else {
                    None
                };
                let Some(tp) = target_page else { continue };
                if ids[tp].contains(&id) || reported.contains(&href.to_string()) {
                    continue;
                }
                reported.push(href.to_string());
                let on = if tp == pi { "on this page".to_string() } else { format!("on the page \"{}\"", pages[tp].path) };
                let mut known: Vec<&str> = ids[tp].iter().map(String::as_str).collect();
                let hint = match diag::suggest(&id, known.iter().copied()) {
                    Some(s) => format!("{s} An anchor link needs an element with that id {on}"),
                    None => {
                        known.truncate(12);
                        let list = if known.is_empty() {
                            "none".to_string()
                        } else {
                            known.iter().map(|k| format!("`{k}`")).collect::<Vec<_>>().join(", ")
                        };
                        format!("give the element you mean that id (a section's first argument is its id: `section(\"{id}\", ...)`), or fix the link; the ids {on}: {list}")
                    }
                };
                errs.push(
                    Diag::new(
                        "E0382",
                        format!("page \"{}\" links to \"{href}\", but no element {on} has id=\"{id}\"", p.path),
                        p.span,
                    )
                    .hint(hint),
                );
            }
            let levels: Vec<u32> = p
                .tags
                .iter()
                .filter(|t| !t.close && t.name.len() == 2 && t.name.starts_with('h'))
                .filter_map(|t| t.name[1..].parse().ok())
                .filter(|l| (1..=6).contains(l))
                .collect();
            let h1 = levels.iter().filter(|l| **l == 1).count();
            if h1 != 1 {
                warn(Diag::new("E0379", format!("page \"{}\" has {h1} `h1` headings: a page has exactly one", p.path), p.span).hint(
                    "the `h1` is the page's title for search engines and screen readers: a hero's title is one; make the other headings `h2` or `h3`",
                ));
            }
            if let Some(w) = levels.windows(2).find(|w| w[1] > w[0] + 1) {
                warn(
                    Diag::new(
                        "E0379",
                        format!("page \"{}\" skips a heading level: an `h{}` follows an `h{}`", p.path, w[1], w[0]),
                        p.span,
                    )
                    .hint(format!(
                        "go down one level at a time (`h{}` after `h{}`), so screen readers can follow the outline",
                        w[0] + 1,
                        w[0]
                    )),
                );
            }
        }

        // the theme's contrast (E0380)
        contrast(&web.css, at, &mut warn);

        // images: every one the pages show is loaded (and its variants made) first, in parallel
        let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        let tool = image::find_tool(o.image_tool.as_deref());
        // (each with: shown by a page, used by a `<meta>`)
        let mut wanted: Vec<(String, bool, bool)> = Vec::new();
        for p in &pages {
            for t in p.tags.iter().filter(|t| !t.close) {
                for (name, value) in &t.attrs {
                    let Some(v) = value else { continue };
                    if image_ref(t, name, v) {
                        let meta = name == "content";
                        match wanted.iter_mut().find(|(w, _, _)| w == v) {
                            Some(w) => {
                                w.1 |= !meta;
                                w.2 |= meta;
                            }
                            None => wanted.push((v.clone(), !meta, meta)),
                        }
                    }
                }
            }
        }
        let (images, unscaled, failed) = load_images(&o.dir, &wanted, tool.as_ref(), o.convert, &mut files);
        let mut reported: Vec<&str> = Vec::new();
        let mut edits: Vec<Vec<(usize, usize, String)>> = vec![Vec::new(); pages.len()];
        // per page: the preview classes it uses (class, `data:` URL)
        let mut previews: Vec<Vec<(String, String)>> = vec![Vec::new(); pages.len()];
        for (pi, p) in pages.iter().enumerate() {
            for t in p.tags.iter().filter(|t| !t.close) {
                let mut changed = t.clone();
                let mut any = false;
                for (name, value) in &t.attrs {
                    let Some(v) = value else { continue };
                    if !image_ref(t, name, v) {
                        continue;
                    }
                    if !reported.contains(&v.as_str()) {
                        reported.push(v);
                        if images.get(v).is_none_or(Option::is_none) {
                            let msg =
                                format!("page \"{}\" shows the image \"{v}\", which is not in `assets/` next to the program", p.path);
                            let d = Diag::new("E0383", msg, p.span).hint(format!(
                                "put the file at `assets/{}` (images are read from the folder `assets` next to the program), or fix the name",
                                v.trim_start_matches("./").trim_start_matches('/')
                            ));
                            if o.strict_images {
                                errs.push(d);
                            } else {
                                warn(d);
                            }
                        }
                    }
                    let Some(Some(img)) = images.get(v) else { continue };
                    let url = if name == "content" {
                        let file = img.og.as_ref().unwrap_or(&img.file);
                        site_url(&pages, file).unwrap_or(format!("/{file}"))
                    } else {
                        format!("{}{}", p.up, img.file)
                    };
                    changed.set(name, &url);
                    any = true;
                    if t.name == "img" && name == "src" {
                        if let Some((w, h)) = img.size {
                            if !t.has("width") && !t.has("height") {
                                changed.set("width", &w.to_string());
                                changed.set("height", &h.to_string());
                            }
                        }
                        if !img.variants.is_empty() && !t.has("srcset") {
                            let set: Vec<String> = img.variants.iter().map(|(f, w)| format!("{}{f} {w}w", p.up)).collect();
                            changed.set("srcset", &set.join(", "));
                            if !t.has("sizes") {
                                changed.set("sizes", "100vw");
                            }
                        }
                        // a lazy image shows its blurred preview until it loads (the first image is
                        // preloaded and needs none)
                        if let Some(uri) = img.preview.as_ref().filter(|_| t.get("fetchpriority") != Some("high")) {
                            let cls = format!("ph-{}", &hash(uri.as_bytes())[..8]);
                            let classes = match changed.get("class") {
                                Some(c) if !c.is_empty() => format!("{c} {cls}"),
                                _ => cls.clone(),
                            };
                            changed.set("class", &classes);
                            if !previews[pi].iter().any(|(c, _)| *c == cls) {
                                previews[pi].push((cls, uri.clone()));
                            }
                        }
                    }
                    // a preloaded image must name the same variants as its `<img>`, or the browser fetches the
                    // full-size file first and the WebP it then picks second
                    if t.name == "link" && name == "href" && t.get("as") == Some("image") && !img.variants.is_empty() {
                        if !t.has("imagesrcset") {
                            let set: Vec<String> = img.variants.iter().map(|(f, w)| format!("{}{f} {w}w", p.up)).collect();
                            changed.set("imagesrcset", &set.join(", "));
                        }
                        if !t.has("imagesizes") {
                            changed.set("imagesizes", "100vw");
                        }
                    }
                }
                if t.name == "img" {
                    for (k, v) in [("loading", "lazy"), ("decoding", "async")] {
                        if !t.has(k) && !(k == "loading" && t.get("fetchpriority") == Some("high")) {
                            changed.set(k, v);
                            any = true;
                        }
                    }
                }
                if any {
                    edits[pi].push((t.start, t.end, changed.render()));
                }
            }
        }
        if unscaled > 0 {
            warn(Diag::new("E0386", format!("{unscaled} image(s) copied without WebP variants or `srcset` (--image-tool none)"), at).hint(
                "build without `--image-tool none` to make WebP variants with the built-in codec; the site works without them, with larger images on phones",
            ));
        }
        if !failed.is_empty() {
            let list: Vec<String> = failed.iter().take(5).map(|(f, why)| format!("`{f}` ({why})")).collect();
            let more = if failed.len() > 5 { format!(" and {} more", failed.len() - 5) } else { String::new() };
            warn(
                Diag::new(
                    "E0386",
                    format!("{} image(s) copied without WebP variants: the built-in codec cannot read {}{more}", failed.len(), list.join(", ")),
                    at,
                )
                .hint("save the file again as a plain JPEG, PNG or WebP (an image editor's \"export\"); the site still shows the original"),
            );
        }

        // the pages with their images, and their `style` attributes as classes of the CSS
        let mut style_rules: BTreeMap<String, String> = BTreeMap::new();
        let bodies: Vec<String> = pages
            .iter()
            .enumerate()
            .map(|(pi, p)| style_classes(&html::apply(&p.html, std::mem::take(&mut edits[pi])), &mut style_rules))
            .collect();

        // the CSS: the kit's base and used parts, the motion words, custom keyframes, the program's own
        let mut used: Vec<(String, String)> = Vec::new();
        let mut events = false;
        let mut views: Vec<String> = Vec::new();
        for p in &pages {
            assets::motion_attrs(&p.tags, &mut used);
            for t in p.tags.iter().filter(|t| !t.close) {
                events |= t.has("data-on");
                if let Some(v) = t.get("data-live") {
                    if !views.iter().any(|x| x == v) {
                        views.push(v.to_string());
                    }
                }
            }
        }
        // `keyframes(name, body)`: a wrong one stops the build (E0388), layout properties warn (E0375)
        let mut custom: Vec<motion::Keyframes> = Vec::new();
        for (name, body, span) in &web.keyframes {
            match motion::check_keyframes(name, body) {
                Ok(k) => {
                    // (a literal body was warned about at its place when the program compiled)
                    if let Some((msg, hint)) = k.layout_warning() {
                        if !diag::warnings().iter().any(|w| w.code == "E0375" && w.msg == msg) {
                            warn(Diag::new("E0375", msg, *span).hint(hint));
                        }
                    }
                    custom.push(k);
                }
                Err(e) => {
                    let hint = e.hint.unwrap_or_else(|| {
                        "a keyframes word is `keyframes(\"sway\", \"0%, 100% { rotate: -3deg } 50% { rotate: 3deg }\")`".to_string()
                    });
                    errs.push(Diag::new("E0388", e.message, *span).hint(hint));
                }
            }
        }
        // the motion values of the pages (E0374); a keyframes name counts as a word even when its
        // body was refused above, so one mistake gives one error
        let names: Vec<&str> = web.keyframes.iter().map(|(n, _, _)| n.as_str()).collect();
        for (attr, value) in &used {
            let Some(opt) = motion::Opt::from_name(attr) else { continue };
            if let Err(e) = motion::parse(opt, value, &names) {
                let hint = e
                    .hint()
                    .unwrap_or_else(|| "the motion words are listed under Motion in docs/WEB.md: `nyra explain E0374`".to_string());
                let place = locate(src, &format!("\"{value}\"")).unwrap_or(at);
                errs.push(Diag::new("E0374", format!("{} (in data-{attr}=\"{value}\")", e.message), place).hint(hint));
            }
        }
        let pairs: Vec<(&str, &str)> = used.iter().map(|(a, v)| (a.as_str(), v.as_str())).collect();
        let mut css = assets::pick(assets::KIT_CSS, "chunk", &web.chunks, true);
        css.push_str(&motion::motion_css(&pairs, &custom));
        // (without the records of errors and warnings, which were read above)
        for c in web.css.iter().filter(|c| !c.starts_with("/*! nyra:")) {
            css.push('\n');
            css.push_str(c);
        }
        for rule in style_rules.values() {
            css.push('\n');
            css.push_str(rule);
        }
        let css = css_files(&assets::min_css(&css), &o.dir, &mut files);

        // the JS: kit behaviors, motion, the program's client code
        let mut js = String::new();
        let kit_js = assets::pick(assets::KIT_JS, "chunk", &web.chunks, false);
        if !kit_js.trim().is_empty() {
            js.push_str(&format!("(() => {{\n{}}})();\n", assets::min_js(&kit_js)));
        }
        // the motion script is its own closure already
        js.push_str(&motion::motion_js(&pairs));
        let client = client::build(prog, &views, events, &web.state, &o.file).map_err(Error::Text)?;
        if let Some(c) = client {
            js.push_str(&assets::min_js(&c));
        }
        let css_file = (!css.trim().is_empty()).then(|| format!("site.{}.css", &hash(css.as_bytes())[..10]));
        let js_file = (!js.trim().is_empty()).then(|| format!("site.{}.js", &hash(js.as_bytes())[..10]));
        if let Some(f) = &css_file {
            files.insert(f.clone(), css.into_bytes());
        }
        if let Some(f) = &js_file {
            files.insert(f.clone(), js.into_bytes());
        }

        if !errs.is_empty() {
            return Err(Error::Diags(errs));
        }

        // each page: its policy, the links to the CSS and JS, the reload script of the dev server
        let mut all = Policy::default();
        let base = base_url(&pages);
        let mut sitemap = Vec::new();
        for (pi, (p, body)) in pages.iter().zip(bodies).enumerate() {
            let styles = html::tags(&body).iter().any(|t| !t.close && t.has("style"));
            let mut policy = page_policy(p, js_file.is_some() || o.dev, &web.chunks, styles);
            // the previews: one inline style, allowed by its hash (unless inline styles are allowed anyway)
            let mut style = String::new();
            if !previews[pi].is_empty() {
                let css: String =
                    previews[pi].iter().map(|(c, u)| format!(".{c}{{background:url({u}) 50%/cover no-repeat}}")).collect();
                if !policy.has("style-src", "'unsafe-inline'") {
                    policy.add("style-src", &format!("'sha256-{}'", digest::base64(&digest::sha256(css.as_bytes()))));
                }
                style = format!("<style>{css}</style>");
            }
            all.merge(&policy);
            let html = inject(&body, &policy.text(), &p.up, css_file.as_deref(), js_file.as_deref(), o.dev, &style);
            files.insert(p.file.clone(), html.into_bytes());
            let noindex = p.tags.iter().any(|t| {
                t.name == "meta" && t.get("name") == Some("robots") && t.get("content").is_some_and(|c| c.contains("noindex"))
            });
            if p.file != "404.html" && !noindex {
                let canonical =
                    p.tags.iter().find(|t| t.name == "link" && t.get("rel") == Some("canonical")).and_then(|t| t.get("href"));
                match (canonical, &base) {
                    (Some(c), _) if origin(c).is_some() => sitemap.push(c.to_string()),
                    (_, Some(b)) => {
                        sitemap.push(format!("{b}{}", if p.path == "/" { "/".to_string() } else { format!("{}/", p.path) }))
                    }
                    _ => {}
                }
            }
        }
        all.add("frame-ancestors", "'self'");
        // a hash in the site-wide policy would turn off `'unsafe-inline'` that another page needs
        if all.has("style-src", "'unsafe-inline'") {
            all.remove_hashes("style-src");
        }
        let mut robots = String::from("User-agent: *\nAllow: /\n");
        if let Some(b) = &base {
            let mut xml = String::from(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n",
            );
            for u in &sitemap {
                xml.push_str(&format!("  <url><loc>{}</loc></url>\n", html::text(u)));
            }
            xml.push_str("</urlset>\n");
            files.insert("sitemap.xml".into(), xml.into_bytes());
            robots.push_str(&format!("Sitemap: {b}/sitemap.xml\n"));
        }
        files.insert("robots.txt".into(), robots.into_bytes());
        let csp = all.text();
        let headers = format!(
            "/*\n  Content-Security-Policy: {csp}\n  X-Content-Type-Options: nosniff\n  Referrer-Policy: strict-origin-when-cross-origin\n"
        );
        files.insert("_headers".into(), headers.into_bytes());
        Ok(Site { files, pages: pages.iter().map(|p| p.path.clone()).collect(), csp })
    }
}

/// True if attribute `name` of tag `t` with value `v` refers to an image of the site (see
/// docs/WEB.md, "Build contract").
fn image_ref(t: &html::Tag, name: &str, v: &str) -> bool {
    let is_ref = match name {
        "src" | "poster" | "data-src" | "data-full" | "data-bg" | "data-image" => true,
        "href" => t.name == "a" || t.name == "link",
        "content" => t.name == "meta" && matches!(t.get("property").or(t.get("name")), Some("og:image" | "twitter:image")),
        _ => false,
    };
    is_ref && local_ref(v) && image::is_image(v.split(['?', '#']).next().unwrap_or(v))
}

/// Loads the images `wanted` (each with whether a page shows it and whether a `<meta>` uses it),
/// copies them into `files` and
/// makes their variants. Returns each one's `Img` (`None`: there is no such file), how many were
/// copied without variants because of `--image-tool none`, and the files the built-in codec could
/// not read (with the reason).
#[allow(clippy::type_complexity)]
fn load_images(
    dir: &Path,
    wanted: &[(String, bool, bool)],
    tool: Option<&image::Tool>,
    convert: bool,
    files: &mut BTreeMap<String, Vec<u8>>,
) -> (HashMap<String, Option<Img>>, usize, Vec<(String, String)>) {
    let mut out = HashMap::new();
    let mut unscaled = 0;
    let mut failed = Vec::new();
    // read the files, and with the built-in codec make the variants on several threads
    let found: Vec<Option<(PathBuf, Vec<u8>)>> =
        wanted.iter().map(|(r, _, _)| find_asset(dir, r).and_then(|p| std::fs::read(&p).ok().map(|b| (p, b)))).collect();
    let builtin = convert && tool == Some(&image::Tool::Builtin);
    let mut made: Vec<Option<Result<image::Made, String>>> = (0..wanted.len()).map(|_| None).collect();
    if builtin {
        let jobs: Vec<usize> = (0..wanted.len())
            .filter(|&i| found[i].is_some() && image::decodable(wanted[i].0.split(['?', '#']).next().unwrap_or("")))
            .collect();
        let next = std::sync::atomic::AtomicUsize::new(0);
        let threads = std::thread::available_parallelism().map_or(2, |n| n.get()).clamp(1, 4).min(jobs.len().max(1));
        let results: Vec<(usize, Result<image::Made, String>)> = std::thread::scope(|s| {
            let workers: Vec<_> = (0..threads)
                .map(|_| {
                    s.spawn(|| {
                        let mut done = Vec::new();
                        loop {
                            let k = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            let Some(&i) = jobs.get(k) else { break };
                            let (_, bytes) = found[i].as_ref().expect("a found file");
                            // a panic in the codec is a bug; it must not take the build down
                            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                image::builtin(bytes, wanted[i].1, wanted[i].2)
                            }))
                            .unwrap_or_else(|_| Err("the image codec failed on it".into()));
                            done.push((i, r));
                        }
                        done
                    })
                })
                .collect();
            workers.into_iter().flat_map(|w| w.join().unwrap_or_default()).collect()
        });
        for (i, r) in results {
            made[i] = Some(r);
        }
    }
    for (i, ((r, _, _), f)) in wanted.iter().zip(found).enumerate() {
        let Some((path, bytes)) = f else {
            out.insert(r.clone(), None);
            continue;
        };
        let size = image::size(&bytes);
        let mut file = format!("img/{}", hashed(&path, &bytes, ""));
        let mut variants = Vec::new();
        let mut og = None;
        let mut preview = None;
        match made[i].take() {
            Some(Ok(m)) => {
                preview = m.preview.map(|p| format!("data:image/webp;base64,{}", digest::base64(&p)));
                let webp = path.with_extension("webp");
                for (vw, data) in m.variants {
                    let name = format!("img/{}", hashed(&webp, &bytes, &format!("-{vw}")));
                    files.insert(name.clone(), data);
                    variants.push((name, vw));
                }
                if let Some(data) = m.og {
                    let name = format!("img/{}", hashed(&path.with_extension("jpg"), &bytes, "-og"));
                    files.insert(name.clone(), data);
                    og = Some(name);
                }
                if m.replace && (!variants.is_empty() || og.is_some()) {
                    // the widest variant (or, for a `<meta>` only, the JPEG copy) stands in for the original
                    file = variants.last().map(|v| &v.0).or(og.as_ref()).cloned().unwrap_or(file);
                } else {
                    if m.original_top {
                        if let Some((w, _)) = size {
                            variants.push((file.clone(), w));
                        }
                    }
                    files.insert(file.clone(), bytes);
                }
                if variants.len() == 1 && variants[0].0 == file {
                    // the file alone needs no `srcset`
                    variants.clear();
                }
            }
            Some(Err(why)) => {
                failed.push((format!("assets/{}", r.trim_start_matches("./").trim_start_matches('/')), why));
                files.insert(file.clone(), bytes);
            }
            None => {
                if image::resizable(r) {
                    match (tool, size) {
                        (Some(image::Tool::External(tool)), Some((w, _))) if convert => {
                            for vw in image::WIDTHS.iter().copied().filter(|vw| *vw < w) {
                                let name = format!("img/{}", hashed(&path.with_extension("webp"), &bytes, &format!("-{vw}")));
                                if let Some(data) = variant(tool, &path, &bytes, vw) {
                                    files.insert(name.clone(), data);
                                    variants.push((name, vw));
                                }
                            }
                            if !variants.is_empty() {
                                variants.push((file.clone(), w));
                            }
                        }
                        (None, _) => unscaled += 1,
                        _ => {}
                    }
                }
                files.insert(file.clone(), bytes);
            }
        }
        out.insert(r.clone(), Some(Img { file, size, variants, og, preview }));
    }
    (out, unscaled, failed)
}

/// A WebP of `src` at `width`, made by `tool`, kept in a cache in the temp folder (by the content).
fn variant(tool: &str, src: &Path, bytes: &[u8], width: u32) -> Option<Vec<u8>> {
    let dir = std::env::temp_dir().join("nyra").join("web-img");
    std::fs::create_dir_all(&dir).ok()?;
    let out = dir.join(format!("{}-{width}.webp", hash(bytes)));
    if !out.is_file() {
        let part = dir.join(format!("{}-{width}.{}.partial.webp", hash(bytes), std::process::id()));
        if !image::webp(tool, src, width, &part) {
            let _ = std::fs::remove_file(&part);
            return None;
        }
        std::fs::rename(&part, &out).ok()?;
    }
    std::fs::read(out).ok()
}
/// The CSS with each `url(...)` of a file in `assets/` copied to the site and pointed at the copy.
fn css_files(css: &str, dir: &Path, files: &mut BTreeMap<String, Vec<u8>>) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(i) = rest.find("url(") {
        out.push_str(&rest[..i + 4]);
        rest = &rest[i + 4..];
        let Some(end) = rest.find(')') else { break };
        let raw = &rest[..end];
        let v = raw.trim().trim_matches(['"', '\'']);
        let copied = local_ref(v).then(|| find_asset(dir, v)).flatten().and_then(|p| {
            let bytes = std::fs::read(&p).ok()?;
            let folder = if image::is_image(v) { "img" } else { "files" };
            let name = format!("{folder}/{}", hashed(&p, &bytes, ""));
            files.insert(name.clone(), bytes);
            Some(name)
        });
        out.push_str(copied.as_deref().unwrap_or(raw));
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// The site's address (`https://example.com`, no `/` at the end), from the canonical links.
fn base_url(pages: &[Page]) -> Option<String> {
    let canon = |p: &Page| {
        p.tags.iter().find(|t| t.name == "link" && t.get("rel") == Some("canonical")).and_then(|t| t.get("href")).map(str::to_string)
    };
    if let Some(home) = pages.iter().find(|p| p.path == "/").and_then(canon) {
        if origin(&home).is_some() {
            return Some(home.trim_end_matches('/').to_string());
        }
    }
    pages.iter().find_map(|p| canon(p).and_then(|c| origin(&c)))
}

/// The address of a file of the site (for `og:image`, which must be absolute).
fn site_url(pages: &[Page], file: &str) -> Option<String> {
    base_url(pages).map(|b| format!("{b}/{file}"))
}

/// The Content-Security-Policy of a page, from what it uses. `styles`: the page still has `style`
/// attributes (the build makes the others classes, `style_classes`).
fn page_policy(p: &Page, scripts: bool, chunks: &[String], styles: bool) -> Policy {
    let mut pol = Policy::default();
    pol.add("default-src", "'self'");
    pol.add("script-src", if scripts { "'self'" } else { "'none'" });
    pol.add("style-src", "'self'");
    pol.add("img-src", "'self'");
    pol.add("img-src", "data:");
    pol.add("font-src", "'self'");
    pol.add("form-action", "'self'");
    pol.add("base-uri", "'self'");
    pol.add("object-src", "'none'");
    if chunks.iter().any(|c| c == "map") {
        pol.add("frame-src", "https://www.google.com");
        pol.add("frame-src", "https://maps.google.com");
    }
    if styles {
        pol.add("style-src", "'unsafe-inline'");
    }
    for t in p.tags.iter().filter(|t| !t.close) {
        if t.name == "style" {
            pol.add("style-src", "'unsafe-inline'");
        }
        let o = |a: &str| t.get(a).and_then(origin);
        match t.name.as_str() {
            "img" | "source" | "video" => {
                for a in ["src", "srcset", "poster"] {
                    if let Some(x) = o(a) {
                        if t.name == "video" && a == "src" {
                            pol.add("media-src", &x);
                        } else if x.starts_with("https://") {
                            // image hosts and CDNs redirect (picsum.photos -> fastly.picsum.photos), so a remote
                            // image allows any https image; an image cannot run code
                            pol.add("img-src", "https:");
                        } else {
                            pol.add("img-src", &x);
                        }
                    }
                }
            }
            "audio" => {
                if let Some(x) = o("src") {
                    pol.add("media-src", &x);
                }
            }
            "iframe" => {
                if let Some(x) = o("src") {
                    pol.add("frame-src", &x);
                }
            }
            "link" if t.get("rel").is_some_and(|r| r.split_whitespace().any(|w| w == "stylesheet")) => {
                if let Some(x) = o("href") {
                    if x == "https://fonts.googleapis.com" {
                        pol.add("font-src", "https://fonts.gstatic.com");
                    }
                    pol.add("style-src", &x);
                }
            }
            "form" => match t.get("action") {
                Some(a) if a.starts_with("mailto:") => pol.add("form-action", "mailto:"),
                Some(a) => {
                    if let Some(x) = origin(a) {
                        pol.add("form-action", &x);
                    }
                }
                None => {}
            },
            _ => {}
        }
        // a frame the kit loads later (a map or a video after a click)
        if t.name != "img" {
            for a in ["data-src", "data-embed", "data-frame"] {
                if let Some(x) = o(a) {
                    pol.add("frame-src", &x);
                }
            }
        }
    }
    pol
}

/// The page with its policy, the CSS and JS links and (dev) the reload script in its `<head>`.
/// A page without `<head>` is wrapped in a whole document.
fn inject(page: &str, policy: &str, up: &str, css: Option<&str>, js: Option<&str>, dev: bool, style: &str) -> String {
    let meta = format!("<meta http-equiv=\"Content-Security-Policy\" content=\"{}\">", html::attr(policy));
    let mut tail = String::new();
    if let Some(c) = css {
        tail.push_str(&format!("<link rel=\"stylesheet\" href=\"{up}{c}\">"));
    }
    if let Some(j) = js {
        tail.push_str(&format!("<script src=\"{up}{j}\" defer></script>"));
    }
    if dev {
        tail.push_str(&format!("<script src=\"{RELOAD_JS}\" defer></script>"));
    }
    tail.push_str(style);
    let tags = html::tags(page);
    let Some(head) = tags.iter().find(|t| t.name == "head" && !t.close) else {
        return format!(
            "<!doctype html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n{meta}\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n{tail}\n</head>\n<body>\n{page}\n</body>\n</html>\n"
        );
    };
    // the policy goes first (it covers only what comes after it), after the charset
    let charset = tags.iter().find(|t| t.name == "meta" && t.has("charset") && t.start > head.start);
    let meta_at = charset.map_or(head.end, |t| t.end);
    let tail_at = tags.iter().find(|t| t.name == "head" && t.close && t.start > meta_at).map_or(meta_at, |t| t.start);
    html::apply(page, vec![(meta_at, meta_at, meta), (tail_at, tail_at, tail)])
}

/// E0380 for theme colors that do not reach WCAG AA contrast (4.5:1 for text): `--ink` and
/// `--muted` on `--bg` and `--alt`, as the theme CSS (`_css`) defines them with hex values.
fn contrast(css: &[String], at: Span, warn: &mut impl FnMut(Diag)) {
    let mut vars: HashMap<String, (f64, f64, f64)> = HashMap::new();
    let mut shown: HashMap<String, String> = HashMap::new();
    for c in css {
        let mut rest = c.as_str();
        while let Some(i) = rest.find("--") {
            rest = &rest[i + 2..];
            let Some(colon) = rest.find(':') else { break };
            let name = rest[..colon].trim();
            if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
                continue;
            }
            let value = rest[colon + 1..].split([';', '}']).next().unwrap_or("").trim();
            if let Some(rgb) = hex(value) {
                vars.insert(name.to_string(), rgb);
                shown.insert(name.to_string(), value.to_string());
            }
        }
    }
    for (fg, bg) in [("ink", "bg"), ("muted", "bg"), ("ink", "alt"), ("muted", "alt")] {
        let (Some(a), Some(b)) = (vars.get(fg), vars.get(bg)) else { continue };
        let ratio = contrast_ratio(*a, *b);
        if ratio < 4.5 {
            let r = (ratio * 100.0).floor() / 100.0;
            warn(
                Diag::new(
                    "E0380",
                    format!(
                        "theme colors `{fg}` {} on `{bg}` {} have a contrast of {r:.2}:1, below the 4.5:1 that WCAG AA needs for text",
                        shown[fg], shown[bg]
                    ),
                    at,
                )
                .hint(format!(
                    "make `{fg}` darker or `{bg}` lighter (or the reverse for a dark theme) in `site(...)` until they reach 4.5:1"
                )),
            );
        }
    }
}

/// `#rgb` or `#rrggbb` as 0..1 channels.
fn hex(v: &str) -> Option<(f64, f64, f64)> {
    let h = v.strip_prefix('#')?;
    let full: String = match h.len() {
        3 => h.chars().flat_map(|c| [c, c]).collect(),
        6 => h.to_string(),
        _ => return None,
    };
    let n = u32::from_str_radix(&full, 16).ok()?;
    let ch = |s: u32| ((n >> s) & 0xFF) as f64 / 255.0;
    Some((ch(16), ch(8), ch(0)))
}

/// The WCAG contrast ratio of two colors (1 to 21).
pub fn contrast_ratio(a: (f64, f64, f64), b: (f64, f64, f64)) -> f64 {
    let lum = |(r, g, b): (f64, f64, f64)| {
        let f = |c: f64| if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
        0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b)
    };
    let (la, lb) = (lum(a), lum(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

// ---- the command line ------------------------------------------------------------------------------

/// What `nyra build`, `run` and `check` need from the command line for a `use web` program.
pub struct Cli {
    pub cmd: String,
    pub file: String,
    /// `-o DIR` (build): the output folder, else `dist` next to the program.
    pub out: Option<String>,
    pub json: bool,
    /// `--port N` (run): default 8000; 0 lets the system choose.
    pub port: Option<u16>,
    pub image_tool: Option<String>,
    /// What the program may use (`--allow`): nothing unless given.
    pub grant: caps::Grant,
}

impl Cli {
    fn options(&self, convert: bool, dev: bool) -> Options {
        let dir = Path::new(&self.file)
            .parent()
            .map(Path::to_path_buf)
            .filter(|d| !d.as_os_str().is_empty())
            .unwrap_or_else(|| PathBuf::from("."));
        Options {
            dir,
            file: self.file.clone(),
            image_tool: self.image_tool.clone(),
            convert,
            strict_images: !dev,
            dev,
            to_stdout: self.cmd != "check" && !self.json,
        }
    }
}

/// `nyra build`, `nyra run` or `nyra check` of a checked `use web` program.
pub fn cli(c: &Cli, src: &str, prog: ast::Program) -> std::process::ExitCode {
    use std::process::ExitCode;
    let start = crate::clock::Instant::now();
    if c.cmd == "run" {
        #[cfg(not(target_arch = "wasm32"))]
        return serve::run(c, src, prog);
    }
    let check = c.cmd == "check";
    let result = build(prog, src, &c.options(!check, false));
    let site = match result {
        Ok(s) => s,
        Err(e) => {
            if c.json {
                println!("{}", e.json(&c.file));
            } else {
                eprint!("{}", diag::render_warnings(&c.file, src));
                eprint!("{}", e.human(&c.file, src));
            }
            return ExitCode::from(1);
        }
    };
    if check {
        if c.json {
            println!("{}", diag::render_json(&[], &c.file));
        } else {
            eprint!("{}", diag::render_warnings(&c.file, src));
            eprintln!("nyra: no errors ({} page(s), {})", site.pages.len(), crate::ms(start.elapsed()));
        }
        return ExitCode::SUCCESS;
    }
    if !c.json {
        eprint!("{}", diag::render_warnings(&c.file, src));
    }
    let out = match &c.out {
        Some(o) => PathBuf::from(o),
        None => c.options(false, false).dir.join("dist"),
    };
    if let Err(e) = write(&site, &out) {
        eprintln!("nyra: {e}");
        return ExitCode::from(2);
    }
    let total: usize = site.files.values().map(Vec::len).sum();
    let part = |ext: &str| {
        site.files
            .iter()
            .find(|(n, _)| n.starts_with("site.") && n.ends_with(ext))
            .map(|(n, b)| format!(", {n} {}", kb(b.len())))
            .unwrap_or_default()
    };
    if c.json {
        println!("{}", site_json(&site, &c.file, Some(&out)));
    } else {
        eprintln!(
            "nyra: built {} page(s) into {} ({} files, {}{}{}) ({})",
            site.pages.len(),
            out.display(),
            site.files.len(),
            kb(total),
            part(".css"),
            part(".js"),
            crate::ms(start.elapsed())
        );
    }
    ExitCode::SUCCESS
}

/// `12.3 KB`
pub fn kb(n: usize) -> String {
    format!("{:.1} KB", n as f64 / 1024.0)
}

/// `{"ok":true,"pages":[..],"files":[{"path":..,"bytes":..}],"warnings":[..]}`
pub fn site_json(site: &Site, file: &str, out: Option<&Path>) -> String {
    let pages: Vec<String> = site.pages.iter().map(|p| diag::json_str(p)).collect();
    let files: Vec<String> =
        site.files.iter().map(|(n, b)| format!("{{\"path\":{},\"bytes\":{}}}", diag::json_str(n), b.len())).collect();
    let dir = out.map(|o| format!(",\"out\":{}", diag::json_str(&o.display().to_string()))).unwrap_or_default();
    format!(
        "{{\"ok\":true,\"errors\":[]{dir},\"pages\":[{}],\"files\":[{}]{}}}",
        pages.join(","),
        files.join(","),
        diag::warnings_json(file)
    )
}

/// Writes the site to `out`. Only a folder that is empty, missing, or written by an earlier build
/// (it has `.nyra-build`) is used; files of the earlier build that are gone now are removed.
pub fn write(site: &Site, out: &Path) -> Result<(), String> {
    let manifest = out.join(MANIFEST);
    let old: Vec<String> = match std::fs::read_to_string(&manifest) {
        Ok(t) => t.lines().map(str::to_string).collect(),
        Err(_) => {
            let busy = std::fs::read_dir(out).map(|mut d| d.next().is_some()).unwrap_or(false);
            if busy {
                return Err(format!(
                    "`{}` has files that an earlier `nyra build` did not write: choose an empty folder with `-o DIR`",
                    out.display()
                ));
            }
            Vec::new()
        }
    };
    for (name, bytes) in &site.files {
        let path = out.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("cannot create `{}`: {e}", parent.display()))?;
        }
        std::fs::write(&path, bytes).map_err(|e| format!("cannot write `{}`: {e}", path.display()))?;
    }
    for name in old.iter().filter(|n| !n.is_empty() && !site.files.contains_key(*n)) {
        if name.split('/').any(|p| p == "..") {
            continue;
        }
        let path = out.join(name);
        let _ = std::fs::remove_file(&path);
        // and the folders it leaves empty
        let mut dir = path.parent();
        while let Some(d) = dir.filter(|d| d.starts_with(out) && *d != out) {
            if std::fs::remove_dir(d).is_err() {
                break;
            }
            dir = d.parent();
        }
    }
    let list: Vec<&str> = site.files.keys().map(String::as_str).collect();
    std::fs::write(&manifest, list.join("\n") + "\n").map_err(|e| format!("cannot write `{}`: {e}", manifest.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_paths_map_to_files() {
        assert_eq!(page_file("/").unwrap(), "index.html");
        assert_eq!(page_file("/about").unwrap(), "about/index.html");
        assert_eq!(page_file("/about/").unwrap(), "about/index.html");
        assert_eq!(page_file("/a/b").unwrap(), "a/b/index.html");
        assert_eq!(page_file("/404").unwrap(), "404.html");
        assert_eq!(page_file("/x.html").unwrap(), "x.html");
        assert!(page_file("about").is_err());
        assert!(page_file("/a/../b").is_err());
        assert!(page_file("/a b").is_err());
        assert!(page_file("//x").is_err());
        assert_eq!(norm_path("/about/"), "/about");
        assert_eq!(norm_path("/"), "/");
        assert_eq!(norm_path("/index.html"), "/");
    }

    #[test]
    fn contrast_follows_wcag() {
        let black = (0.0, 0.0, 0.0);
        let white = (1.0, 1.0, 1.0);
        assert!((contrast_ratio(black, white) - 21.0).abs() < 1e-9);
        let grey = hex("#777").unwrap();
        let r = contrast_ratio(grey, white);
        assert!(r > 4.47 && r < 4.49, "{r}");
        assert_eq!(hex("#fff"), Some(white));
        assert_eq!(hex("red"), None);
    }

    #[test]
    fn the_head_gets_the_policy_first_and_the_files_last() {
        let page = "<!doctype html><html><head><meta charset=\"utf-8\"><title>x</title></head><body></body></html>";
        let out = inject(page, "default-src 'self'", "../", Some("site.a.css"), Some("site.b.js"), false, "");
        assert_eq!(
            out,
            "<!doctype html><html><head><meta charset=\"utf-8\"><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'self'\"><title>x</title><link rel=\"stylesheet\" href=\"../site.a.css\"><script src=\"../site.b.js\" defer></script></head><body></body></html>"
        );
        let bare = inject("<h1>Hi</h1>", "p", "", None, None, true, "");
        assert!(bare.starts_with("<!doctype html>") && bare.contains("<h1>Hi</h1>") && bare.contains(RELOAD_JS));
    }

    #[test]
    fn origins_and_uses() {
        assert_eq!(origin("https://www.google.com/maps?q=x").as_deref(), Some("https://www.google.com"));
        assert_eq!(origin("/a"), None);
        assert!(uses_web("// x\nuse web\n") && uses_web("use  web;") && !uses_web("use webby\n"));
        assert!(local_ref("a.png") && local_ref("/img/a.png") && !local_ref("https://x/a.png") && !local_ref("data:x"));
    }

    #[test]
    fn style_attributes_become_classes() {
        let (class, rule) = style_rule("--tint:#c00; --ratio: 4/3;").unwrap();
        assert!(class.starts_with("s-") && class.len() == 10, "{class}");
        assert_eq!(rule, format!(".{class}{{--tint:#c00 !important;--ratio:4/3 !important}}"));
        // the same declarations, the same class; a `;` inside quotes or parentheses is not the end of one
        assert_eq!(style_rule("--tint:#c00;--ratio:4/3").unwrap().0, class);
        assert!(style_rule("background:url(\"a;b.png\")").unwrap().1.contains("url(\"a;b.png\") !important"));
        // what could end the rule stays an attribute
        for bad in ["color:red}body{color:blue", "--x:1/*", "--x:\\7d", "", "@import x"] {
            assert!(style_rule(bad).is_none(), "{bad}");
        }
        let mut rules = BTreeMap::new();
        let page = style_classes("<p class=\"a\" style=\"--tint:#c00\">x</p><i style=\"--x:1\"></i><b style=\"a}\"></b>", &mut rules);
        let (tint, x) = (style_rule("--tint:#c00").unwrap().0, style_rule("--x:1").unwrap().0);
        assert_eq!(page, format!("<p class=\"a {tint}\">x</p><i class=\"{x}\"></i><b style=\"a}}\"></b>"));
        assert_eq!(rules.len(), 2);
    }
}
