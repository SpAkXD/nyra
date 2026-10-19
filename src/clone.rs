//! `nyra clone <url>`: an existing web page becomes a `use web` program (`site.nyra`) that
//! recreates it with the kit's components, for an agent to refine (docs/CLONE.md).
//!
//! The parts: `fetch` gets the page and its stylesheets (the system's curl, or a headless
//! browser for pages that scripts build), `dom` reads the HTML, `css` the styles and the theme,
//! `read` finds the page's structure (navbar, hero, sections of cards, galleries, quotes, stats,
//! forms, opening hours, maps, footer) with all of its text, images and links, and `write` makes
//! the program. Last, the program is compiled and its site built in memory; a part that fails is
//! written again in a simpler form, so the file always builds.
//!
//! `clone_to_nyra` is the whole conversion of a fetched page, for the command line here and
//! for a later MCP tool.

mod cdp;
mod css;
mod dom;
mod fetch;
mod motion;
mod read;
mod write;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::diag::json_str;

/// How to convert a page.
#[derive(Default)]
pub struct CloneOptions {
    /// The page's stylesheets: (URL as linked, text). Inline `<style>` is read from the HTML.
    pub sheets: Vec<(String, String)>,
    /// The folder of a local page: its relative images are copied into `assets/`.
    pub local_dir: Option<PathBuf>,
    /// Compile the program and build its site in memory, and simplify the parts that fail.
    pub check: bool,
    /// The folder the program will be in (its `assets/` is where the check looks for images).
    pub out_dir: Option<PathBuf>,
    /// Where the page came from, for the comment at the top of the program.
    pub source: String,
}

/// A converted page.
pub struct CloneResult {
    /// The program (`site.nyra`).
    pub source: String,
    /// Each part of the page and the component it became: (part, component, id, title).
    pub parts: Vec<(String, String, String, String)>,
    pub theme: Vec<(String, String)>,
    /// What was left out, with counts.
    pub dropped: BTreeMap<String, usize>,
    /// Words of text on the page, and in the program.
    pub words: (usize, usize),
    /// Content images on the page, and how many of them the program shows.
    pub images: (usize, usize),
    /// Local images to copy into `assets/`: (file, name).
    pub assets: Vec<(PathBuf, String)>,
    /// Parts written in a simpler form because the full form did not compile.
    pub simplified: Vec<String>,
    /// Errors the check could not remove (empty when the program builds).
    pub errors: Vec<String>,
    /// True if the check ran.
    pub checked: bool,
    /// What the page does that needs a server (the program has a `// TODO(server)` note for each).
    pub server: Vec<String>,
    /// The motion found: (what, the word it became, how many).
    pub motion: Vec<(String, String, usize)>,
    /// How many motion options the program writes (`anim:`, `hover:`, `loop:`, `scroll:`).
    pub motion_used: usize,
    /// Videos that play in the program (players, embeds, video thumbnails).
    pub videos: usize,
    /// The label of the consent button `--browser` clicked (the most private choice), or "".
    pub consent: String,
}

/// Converts a page: `html` is the page, `base_url` its address (relative links and images are
/// resolved against it; "" for a local file whose links stay relative).
pub fn clone_to_nyra(html: &str, base_url: &str, opts: &CloneOptions) -> CloneResult {
    let doc = dom::Dom::parse(html);
    // <base href> changes the base of the page's links
    let base = doc
        .find(0, "base")
        .and_then(|b| doc.attr(b, "href"))
        .map(|h| css::resolve(base_url, h))
        .filter(|b| fetch::is_url(b))
        .unwrap_or_else(|| base_url.to_string());

    // the stylesheets in document order: linked ones by their URL, then <style> blocks
    let mut sheet = css::Sheet::default();
    let mut used = vec![false; opts.sheets.len()];
    for n in 0..doc.nodes.len() {
        match doc.tag(n) {
            "link" if is_stylesheet(&doc, n) => {
                let href = doc.attr(n, "href").unwrap_or("");
                let abs = css::resolve(&base, href);
                if let Some(k) = opts.sheets.iter().position(|(u, _)| *u == abs || u == href) {
                    if !used[k] {
                        used[k] = true;
                        sheet.add(&opts.sheets[k].1, &abs);
                    }
                }
            }
            "style" => sheet.add(&doc.nodes[n].text, &base),
            _ => {}
        }
    }
    for (k, (u, text)) in opts.sheets.iter().enumerate() {
        if !used[k] {
            sheet.add(text, u);
        }
    }
    let styles = css::Styles::compute(&doc, &sheet, &base);
    let motions = motion::detect(&doc, &styles, &sheet);
    let page = read::Reader::new(&doc, &styles, &base, opts.local_dir.clone(), motions).read();

    let source_label = if opts.source.is_empty() { base.clone() } else { opts.source.clone() };
    let dir = opts.out_dir.clone().unwrap_or_else(|| std::env::temp_dir().join("nyra-clone-check"));
    let (written, levels, simplified, errors) = settle(&page, &source_label, opts.check.then_some(dir.as_path()));
    let part_count = page.sections.len();

    // the summary
    let mut parts = Vec::new();
    for (p, comp) in &written.components {
        let (id, title) = match *p {
            write::NAV => (String::new(), page.nav.as_ref().map(|n| read::md_plain(&n.brand)).unwrap_or_default()),
            write::FOOT => (page.footer.as_ref().map(|f| f.id.clone()).unwrap_or_default(), String::new()),
            write::SIDE => (
                String::new(),
                format!("{} links", page.side.as_ref().map_or(0, |sd| sd.links.iter().filter(|l| !l.1.is_empty()).count())),
            ),
            i => {
                let s = &page.sections[i];
                let title = if s.kind == read::Kind::Split { format!("{} panels", s.panels.len()) } else { read::md_plain(&s.title) };
                (s.id.clone(), title)
            }
        };
        let mut what = comp.clone();
        if *p < part_count && levels.get(p).copied().unwrap_or(0) == 0 {
            let inner: Vec<&str> =
                page.sections[*p].items.iter().map(|i| i.component()).filter(|c| !matches!(*c, "p" | "h3")).collect();
            let mut uniq: Vec<&str> = Vec::new();
            for c in inner {
                if !uniq.contains(&c) {
                    uniq.push(c);
                }
            }
            if !uniq.is_empty() {
                what = format!("{what}: {}", uniq.join(", "));
            }
        }
        parts.push((part_name(&page, *p), what, id, title));
    }
    let t = &page.theme;
    let mut theme = Vec::new();
    for (k, v) in
        [("primary", &t.primary), ("accent", &t.accent), ("bg", &t.bg), ("ink", &t.ink), ("font", &t.font), ("display", &t.display)]
    {
        if let Some(v) = v {
            theme.push((k.to_string(), v.clone()));
        }
    }
    if let Some(r) = t.radius {
        theme.push(("radius".into(), format!("{r}px")));
    }
    let kept_images = page.images.iter().filter(|u| written.src.contains(u.as_str())).count();
    let words = coverage(&page.words, &written.src);
    let motion_used = ["anim: \"", "hover: \"", "loop: \"", "scroll: \""].iter().map(|k| written.src.matches(k).count()).sum();
    CloneResult {
        source: written.src,
        parts,
        theme,
        dropped: page.dropped,
        words,
        images: (page.images.len(), kept_images),
        assets: page.assets,
        simplified,
        errors,
        checked: opts.check,
        server: page.server.clone(),
        motion: page.motion.iter().map(|((what, word), n)| (what.clone(), word.clone(), *n)).collect(),
        motion_used,
        videos: page.videos,
        consent: String::new(),
    }
}

/// (words of the page, how many of them the program has): words are compared as lowercase runs
/// of letters and digits, so Markdown and escapes do not count against the program.
fn coverage(page: &[String], src: &str) -> (usize, usize) {
    let tokens =
        |s: &str| s.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_lowercase).collect::<Vec<_>>();
    let have: std::collections::HashSet<String> = tokens(src).into_iter().collect();
    let all: Vec<String> = page.iter().flat_map(|w| tokens(w)).collect();
    let kept = all.iter().filter(|w| have.contains(*w)).count();
    (all.len(), kept)
}

/// Writes the program, and with `check` (the folder it will be in) compiles and builds it in
/// memory: a part with an error is written again more simply (kit components, then plain
/// elements, then text only) until the program builds. Returns the program, each part's level,
/// what was simplified and the errors that remain.
fn settle(page: &read::Page, source: &str, check_in: Option<&Path>) -> (write::Written, write::Levels, Vec<String>, Vec<String>) {
    let mut levels = write::Levels::new();
    let mut simplified = Vec::new();
    let mut errors = Vec::new();
    let part_count = page.sections.len();
    let all_parts: Vec<usize> = (0..part_count).chain([write::NAV, write::FOOT, write::SIDE]).collect();
    let mut written = write::write(page, &levels, source);
    let Some(dir) = check_in else { return (written, levels, simplified, errors) };
    for _round in 0..(part_count * 2 + 8) {
        let errs = match check(&written.src, dir) {
            Ok(()) => {
                errors.clear();
                break;
            }
            Err(e) => e,
        };
        errors = errs.iter().map(|(line, m)| if *line > 0 { format!("line {line}: {m}") } else { m.clone() }).collect();
        // the parts the errors are in, or every part when an error has no line in the program
        let mut bump: Vec<usize> = Vec::new();
        for (line, _) in &errs {
            match written.parts.iter().find(|(_, a, b)| line >= a && line <= b) {
                Some((p, _, _)) => bump.push(*p),
                None => bump.extend(all_parts.iter().copied().filter(|p| levels.get(p).copied().unwrap_or(0) < 2)),
            }
        }
        bump.sort_unstable();
        bump.dedup();
        let mut changed = false;
        for p in bump {
            let l = levels.entry(p).or_insert(0);
            if *l < 2 {
                *l += 1;
                changed = true;
                simplified.push(format!("{} -> {}", part_name(page, p), if *l == 1 { "plain elements" } else { "text only" }));
            }
        }
        if !changed {
            break;
        }
        written = write::write(page, &levels, source);
    }
    (written, levels, simplified, errors)
}

fn is_stylesheet(doc: &dom::Dom, n: usize) -> bool {
    let rel = doc.attr(n, "rel").unwrap_or("").to_ascii_lowercase();
    let media = doc.attr(n, "media").unwrap_or("all").to_ascii_lowercase();
    rel.split_whitespace().any(|r| r == "stylesheet") && !rel.contains("alternate") && !media.contains("print")
        || (rel.contains("preload") && doc.attr(n, "as") == Some("style"))
}

fn part_name(page: &read::Page, p: usize) -> String {
    match p {
        write::NAV => "navbar".into(),
        write::FOOT => "footer".into(),
        write::SIDE => "sidebar".into(),
        i => {
            let s = &page.sections[i];
            if !s.id.is_empty() {
                format!("section #{}", s.id)
            } else {
                format!("section {}", i + 1)
            }
        }
    }
}

/// Compiles the program and builds its site in memory: `Err` with (line, message) for each
/// error (line 0: not at a line of the program).
fn check(src: &str, dir: &Path) -> Result<(), Vec<(usize, String)>> {
    let result = check_inner(src, dir);
    crate::diag::clear_warnings();
    result
}

fn check_inner(src: &str, dir: &Path) -> Result<(), Vec<(usize, String)>> {
    let lines = |d: &[crate::diag::Diag]| -> Vec<(usize, String)> {
        d.iter().map(|x| (if x.file.is_none() { x.span.line } else { 0 }, format!("error[{}]: {}", x.code, x.msg))).collect()
    };
    let prog = crate::compile_granted(src, &crate::caps::Grant::none(), &crate::cli_flag).map_err(|d| lines(&d))?;
    let opts = crate::web::Options {
        dir: dir.to_path_buf(),
        file: "site.nyra".into(),
        image_tool: Some("none".into()),
        convert: false,
        strict_images: false,
        dev: false,
        to_stdout: false,
    };
    match crate::web::build(prog, src, &opts) {
        Ok(_) => Ok(()),
        Err(crate::web::Error::Diags(d)) => Err(lines(&d)),
        Err(crate::web::Error::Runtime(e)) => Err(vec![(0, format!("runtime error[{}] while building: {}", e.code, e.msg))]),
        Err(crate::web::Error::Text(t)) => Err(vec![(0, t)]),
    }
}

// ---- the command line ---------------------------------------------------------------------------------

const USAGE: &str = "\
usage: nyra clone <url | page.html> [-o site.nyra] [--browser] [--base URL] [--json] [--no-check] [--force]

Writes a `use web` program that recreates the page with the kit's components (docs/CLONE.md):
the theme, the navbar, the hero, sections of cards, galleries, quotes, stats, steps, forms,
opening hours, maps and the footer, with all of the page's text, images and links.

  -o FILE      the program to write (default: site.nyra); images of a local page go to assets/ next to it
  --browser    load the page in a headless Chrome, Edge, Brave or Chromium and read the DOM it renders
               (for sites built by JavaScript): cookie dialogs are turned down (never accepted), the page
               is scrolled so lazy content loads, its motion is observed; NYRA_BROWSER names the browser
  --base URL   the address of a local page, so its relative links and images point to the site
  --json       print the summary as JSON
  --no-check   do not compile and build the program in memory (the check simplifies parts that fail)
  --force      overwrite a file that `nyra clone` did not write

The page is fetched with the system's curl (20 s, 5 MB limit; stylesheets 2 MB each). Clone only
sites you own or have the right to copy.";

pub fn cli(args: Vec<String>) -> ExitCode {
    let mut target: Option<String> = None;
    let mut out = "site.nyra".to_string();
    let mut browser = false;
    let mut json = false;
    let mut check = true;
    let mut force = false;
    let mut base: Option<String> = None;
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            "-o" => match it.next() {
                Some(o) => out = o,
                None => return usage_error("-o needs a file name"),
            },
            "--browser" => browser = true,
            "--json" => json = true,
            "--no-check" => check = false,
            "--force" => force = true,
            "--base" => match it.next() {
                Some(b) if fetch::is_url(&b) => base = Some(b),
                _ => return usage_error("--base needs an http(s) URL"),
            },
            _ if a.starts_with('-') => return usage_error(&format!("unknown option `{a}`")),
            _ if target.is_none() => target = Some(a),
            _ => return usage_error(&format!("one page at a time (also got `{a}`)")),
        }
    }
    let Some(target) = target else { return usage_error("give a URL or a saved .html file") };
    let out_path = PathBuf::from(&out);
    if out_path.exists() && !force {
        let first = std::fs::read_to_string(&out_path).ok().and_then(|s| s.lines().next().map(str::to_string)).unwrap_or_default();
        if !first.starts_with("// Cloned from") {
            return fail(
                json,
                &format!("`{out}` exists and was not written by `nyra clone`: choose another name with -o, or pass --force"),
            );
        }
    }

    // the page
    let is_url = fetch::is_url(&target);
    let page = if is_url {
        if browser {
            fetch::rendered(&target, 30)
        } else {
            fetch::get(&target, fetch::MAX_PAGE, 20)
        }
    } else {
        if browser {
            return usage_error("--browser loads a URL; for a local file, open it in the browser and save the page");
        }
        fetch::read_file(Path::new(&target), fetch::MAX_PAGE)
    };
    let page = match page {
        Ok(p) => p,
        Err(e) => return fail(json, &e),
    };
    // the page as it was read (with --browser: the rendered DOM and its hints), for debugging
    if let Some(save) = std::env::var_os("NYRA_CLONE_SAVE") {
        let _ = std::fs::write(save, &page.text);
    }
    let base_url = match (&base, is_url) {
        (Some(b), _) => b.clone(),
        (None, true) => page.url.clone(),
        (None, false) => String::new(),
    };
    let local_dir = if is_url {
        None
    } else {
        Path::new(&target).parent().map(|p| if p.as_os_str().is_empty() { PathBuf::from(".") } else { p.to_path_buf() })
    };

    // its stylesheets, fetched in parallel
    let sheets = stylesheets(&page.text, &base_url, local_dir.as_deref());
    let css_bytes: usize = sheets.iter().map(|(_, t)| t.len()).sum();

    let out_dir = out_path
        .parent()
        .map(|p| if p.as_os_str().is_empty() { PathBuf::from(".") } else { p.to_path_buf() })
        .unwrap_or_else(|| PathBuf::from("."));
    let opts = CloneOptions {
        sheets,
        local_dir,
        check,
        out_dir: Some(out_dir.clone()),
        source: if is_url { target.clone() } else { target.replace('\\', "/") },
    };
    let mut result = clone_to_nyra(&page.text, &base_url, &opts);
    result.consent = page.consent.clone();
    if !result.assets.is_empty() {
        let dir = out_dir.join("assets");
        if let Err(e) = std::fs::create_dir_all(&dir) {
            return fail(json, &format!("cannot make `{}`: {e}", dir.display()));
        }
        for (src, name) in &result.assets {
            if let Err(e) = std::fs::copy(src, dir.join(name)) {
                return fail(json, &format!("cannot copy `{}` into assets/: {e}", src.display()));
            }
        }
    }
    let _ = std::fs::create_dir_all(&out_dir);
    if let Err(e) = std::fs::write(&out_path, &result.source) {
        return fail(json, &format!("cannot write `{out}`: {e}"));
    }
    let html_bytes = page.text.len();
    if json {
        println!("{}", summary_json(&result, &out, &target, html_bytes, css_bytes));
    } else {
        eprint!("{}", summary(&result, &out, &target, html_bytes, css_bytes));
    }
    if result.errors.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn usage_error(msg: &str) -> ExitCode {
    eprintln!("nyra clone: {msg}\n\n{USAGE}");
    ExitCode::from(2)
}

fn fail(json: bool, msg: &str) -> ExitCode {
    if json {
        println!("{{\"ok\":false,\"error\":{}}}", json_str(msg));
    } else {
        eprintln!("nyra clone: {msg}");
    }
    ExitCode::from(2)
}

/// The page's linked stylesheets and their `@import`s: (URL as linked, text).
fn stylesheets(html: &str, base: &str, local: Option<&Path>) -> Vec<(String, String)> {
    let doc = dom::Dom::parse(html);
    // (the URL the page links, made absolute; where to read it)
    let mut jobs: Vec<(String, String)> = Vec::new();
    let add = |href: &str, jobs: &mut Vec<(String, String)>| {
        let abs = css::resolve(base, href);
        // Google Fonts: the family names are in the URL, the page's CSS names them anyway
        if abs.contains("fonts.googleapis.com") || jobs.iter().any(|(k, _)| *k == abs) || jobs.len() >= fetch::MAX_SHEETS {
            return;
        }
        // a local page's own stylesheets are read next to it, even with --base
        let from = if local.is_some() && !css::has_scheme(href) && !href.starts_with("//") { href.to_string() } else { abs.clone() };
        jobs.push((abs, from));
    };
    for n in doc.find_all(0, "link") {
        if is_stylesheet(&doc, n) {
            if let Some(href) = doc.attr(n, "href") {
                add(href, &mut jobs);
            }
        }
    }
    let mut out = fetch_all(&jobs, local);
    // one level of @import
    let mut imports = Vec::new();
    for (u, text) in &out {
        let mut s = css::Sheet::default();
        s.add(text, u);
        for i in s.imports {
            if !jobs.iter().any(|(k, _)| *k == i) {
                add(&i, &mut imports);
            }
        }
    }
    let more = fetch_all(&imports, local);
    // imported sheets come before the sheet that imports them; at the start is close enough
    let mut all = more;
    all.append(&mut out);
    all
}

/// Reads stylesheets in parallel: each job is (its key, an http(s) URL or a path of a local page).
fn fetch_all(jobs: &[(String, String)], local: Option<&Path>) -> Vec<(String, String)> {
    let handles: Vec<_> = jobs
        .iter()
        .cloned()
        .map(|(key, from)| {
            let local = local.map(Path::to_path_buf);
            std::thread::spawn(move || {
                let got = if fetch::is_url(&from) {
                    fetch::get(&from, fetch::MAX_CSS, 15).map(|f| f.text)
                } else if let Some(dir) = local {
                    let rel = from.split(['?', '#']).next().unwrap_or("").trim_start_matches('/');
                    fetch::read_file(&dir.join(rel), fetch::MAX_CSS).map(|f| f.text)
                } else {
                    Err(String::new())
                };
                (key, got.ok())
            })
        })
        .collect();
    handles.into_iter().filter_map(|h| h.join().ok()).filter_map(|(u, t)| t.map(|t| (u, t))).collect()
}

/// About how many tokens a text costs a model (4 characters per token: a rough, common measure,
/// the same for the page and for the program).
fn tokens(bytes: usize) -> usize {
    bytes.div_ceil(4)
}

fn kb(n: usize) -> String {
    if n < 1024 {
        format!("{n} B")
    } else {
        format!("{:.1} KB", n as f64 / 1024.0)
    }
}

fn summary(r: &CloneResult, out: &str, target: &str, html: usize, css: usize) -> String {
    let mut s = String::new();
    let nyra = r.source.len();
    s.push_str(&format!(
        "nyra clone: {target} -> {out}: {} (about {} tokens); the page was {} of HTML and {} of CSS (about {} tokens)\n",
        kb(nyra),
        tokens(nyra),
        kb(html),
        kb(css),
        tokens(html + css)
    ));
    if !r.theme.is_empty() {
        s.push_str(&format!("  theme     {}\n", r.theme.iter().map(|(k, v)| format!("{k} {v}")).collect::<Vec<_>>().join(", ")));
    }
    for (name, what, _, title) in &r.parts {
        let t = if title.is_empty() { String::new() } else { format!("  \"{}\"", shorten(title, 60)) };
        s.push_str(&format!("  {name:<22} {what}{t}\n"));
    }
    if !r.consent.is_empty() {
        s.push_str(&format!("  consent   turned the cookie dialog down (clicked \"{}\")\n", shorten(&r.consent, 40)));
    }
    let (w, kw) = r.words;
    let pct = (kw.min(w) * 100).checked_div(w).unwrap_or(100);
    s.push_str(&format!("  kept      {} of {} words of text ({pct}%), {} of {} images\n", kw.min(w), w, r.images.1, r.images.0));
    if r.videos > 0 {
        s.push_str(&format!("  videos    {} that play here (a player loads from the video site only when clicked)\n", r.videos));
    }
    if !r.motion.is_empty() {
        let mut found: Vec<&(String, String, usize)> = r.motion.iter().collect();
        found.sort_by_key(|a| std::cmp::Reverse(a.2));
        let list: Vec<String> = found.iter().take(8).map(|(what, word, n)| format!("{what} x{n} -> {word}")).collect();
        let more = if found.len() > 8 { format!(", and {} more", found.len() - 8) } else { String::new() };
        s.push_str(&format!("  motion    {}{more}; the program has {} motion options\n", list.join(", "), r.motion_used));
    }
    if !r.server.is_empty() {
        s.push_str(&format!("  server    needs a server: {} (see the `// TODO(server)` notes)\n", r.server.join(", ")));
    }
    if !r.dropped.is_empty() {
        s.push_str(&format!("  dropped   {}\n", r.dropped.iter().map(|(k, n)| format!("{n} {k}")).collect::<Vec<_>>().join(", ")));
    }
    if !r.assets.is_empty() {
        s.push_str(&format!("  assets    {} local images copied into assets/\n", r.assets.len()));
    }
    if r.checked {
        if r.errors.is_empty() {
            if r.simplified.is_empty() {
                s.push_str("  check     compiles and builds\n");
            } else {
                s.push_str(&format!("  check     compiles and builds; written simpler: {}\n", r.simplified.join(", ")));
            }
        } else {
            s.push_str("  check     the program does not build yet:\n");
            for e in &r.errors {
                s.push_str(&format!("            {e}\n"));
            }
        }
    }
    s.push_str(&format!("  next      nyra run {out}   (images stay on the original site; save them into assets/ to host them)\n"));
    s
}

fn shorten(t: &str, n: usize) -> String {
    if t.chars().count() <= n {
        t.to_string()
    } else {
        format!("{}…", t.chars().take(n).collect::<String>())
    }
}

fn summary_json(r: &CloneResult, out: &str, target: &str, html: usize, css: usize) -> String {
    let parts: Vec<String> = r
        .parts
        .iter()
        .map(|(n, w, id, t)| {
            format!("{{\"part\":{},\"component\":{},\"id\":{},\"title\":{}}}", json_str(n), json_str(w), json_str(id), json_str(t))
        })
        .collect();
    let theme: Vec<String> = r.theme.iter().map(|(k, v)| format!("{}:{}", json_str(k), json_str(v))).collect();
    let dropped: Vec<String> = r.dropped.iter().map(|(k, n)| format!("{}:{n}", json_str(k))).collect();
    let list = |v: &[String]| v.iter().map(|x| json_str(x)).collect::<Vec<_>>().join(",");
    let assets: Vec<String> = r.assets.iter().map(|(_, n)| n.clone()).collect();
    format!(
        "{{\"ok\":{},\"url\":{},\"out\":{},\"parts\":[{}],\"theme\":{{{}}},\"kept\":{{\"words\":{},\"of_words\":{},\"images\":{},\"of_images\":{}}},\"dropped\":{{{}}},\"assets\":[{}],\"simplified\":[{}],\"errors\":[{}],\"server\":[{}],\"motion\":[{}],\"videos\":{},\"consent\":{},\"bytes\":{{\"html\":{html},\"css\":{css},\"nyra\":{}}},\"tokens\":{{\"original\":{},\"nyra\":{}}}}}",
        r.errors.is_empty(),
        json_str(target),
        json_str(out),
        parts.join(","),
        theme.join(","),
        r.words.1.min(r.words.0),
        r.words.0,
        r.images.1,
        r.images.0,
        dropped.join(","),
        list(&assets),
        list(&r.simplified),
        list(&r.errors),
        list(&r.server),
        r.motion.iter().map(|(w, m, n)| format!("{{\"found\":{},\"word\":{},\"count\":{n}}}", json_str(w), json_str(m))).collect::<Vec<_>>().join(","),
        r.videos,
        json_str(&r.consent),
        r.source.len(),
        tokens(html + css),
        tokens(r.source.len()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_part_that_does_not_build_is_written_more_simply() {
        // the compiler runs on a thread with its big stack, as `nyra` does
        std::thread::Builder::new().stack_size(crate::STACK).spawn(simpler).unwrap().join().unwrap();
    }

    fn simpler() {
        let mut page = read::Page { name: "Test".into(), title: "Test".into(), ..Default::default() };
        // an image the kit refuses (a `javascript:` URL is build error E0381, found while building)
        let bad = read::Item::Gallery(vec![("javascript:alert(1)".into(), "A photo".into())]);
        page.sections.push(read::plain(String::new(), vec![read::Item::P("Fine text".into()), bad]));
        let dir = std::env::temp_dir().join("nyra-clone-unit");
        let (written, levels, simplified, errors) = settle(&page, "test", Some(&dir));
        assert!(errors.is_empty(), "{errors:?}\n{}", written.src);
        assert!(!simplified.is_empty());
        assert_eq!(levels.get(&0), Some(&2), "{}", written.src);
        assert!(written.src.contains("p(\"Fine text\")") && written.src.contains("p(\"A photo\")"), "{}", written.src);
        assert!(!written.src.contains("javascript:"), "{}", written.src);
    }

    #[test]
    fn coverage_counts_words_not_markup() {
        let words: Vec<String> = ["Hello,", "world", "of", "bread"].iter().map(|s| s.to_string()).collect();
        assert_eq!(coverage(&words, "p(\"**Hello** world\")\np(\"of\")"), (4, 3));
    }
}
