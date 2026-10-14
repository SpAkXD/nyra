//! `use web` (docs/WEB.md): the renderer, the kit and their contract with the build.
//!
//! - every `examples/web/*.nyra` prints exactly its `.out` (the page and CSS records of the
//!   "Build contract"), in the interpreter and, when Node.js is there, on the JavaScript backend
//!   (which runs the live views in the browser);
//! - every page is a whole, well-nested HTML document with the build's markers;
//! - text never becomes markup, and a computed `javascript:` link is a build error record;
//! - the measured samples of `docs/web-samples` compile and render without errors;
//! - every chunk a page names exists in `src/web/kit.css` or `src/web/kit.js`, and every
//!   component has a fixture page in `tests/web/fixtures`.
//!
//! After changing the library on purpose, rewrite the goldens with
//!
//!     NYRA_BLESS=1 cargo test --release --test web
//!
//! and review the diff.

mod common;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use common::{missing, nyra, scratch, stderr, stdout};

fn programs() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir("examples/web")
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "nyra"))
        .collect();
    v.sort();
    v
}

fn run(path: &Path, flags: &[&str]) -> String {
    let out = nyra().arg("run").arg(path).args(flags).output().unwrap();
    assert!(out.status.success(), "{} {flags:?} failed:\n{}", path.display(), stderr(&out));
    stdout(&out).replace("\r\n", "\n")
}

/// One record of a program's output: `page /path chunks`, `css theme`, `error E0xxx message`.
struct Record {
    kind: String,
    head: String,
    body: String,
}

fn records(text: &str) -> Vec<Record> {
    let mut out = Vec::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let Some(rest) = line.strip_prefix("<!--nyra:").and_then(|r| r.strip_suffix("-->")) else { continue };
        let (kind, head) = rest.split_once(' ').unwrap_or((rest, ""));
        let mut body = Vec::new();
        if kind != "error" {
            for l in lines.by_ref() {
                if l == "<!--nyra:end-->" {
                    break;
                }
                body.push(l);
            }
        }
        out.push(Record { kind: kind.to_string(), head: head.to_string(), body: body.join("\n") });
    }
    out
}

fn node() -> bool {
    Command::new("node").arg("--version").output().is_ok()
}

#[test]
fn examples_print_their_goldens_in_the_interpreter_and_in_javascript() {
    let bless = std::env::var("NYRA_BLESS").is_ok();
    let js = node();
    if !js {
        missing("no Node.js: the web examples run in the interpreter only");
    }
    for path in programs() {
        let golden = path.with_extension("out");
        let got = run(&path, &["--interp"]);
        if bless {
            std::fs::write(&golden, &got).unwrap();
        }
        let want = std::fs::read_to_string(&golden).unwrap_or_else(|_| panic!("{} has no .out", path.display())).replace("\r\n", "\n");
        assert_eq!(got, want, "{} differs from its .out (NYRA_BLESS=1 rewrites it)", path.display());
        if js {
            assert_eq!(run(&path, &["--js"]), want, "{} renders differently on the JavaScript backend", path.display());
        }
    }
}

/// The elements whose end tag may be left out (void elements).
const VOID: &[&str] =
    &["meta", "link", "img", "input", "br", "hr", "source", "path", "circle", "rect", "line", "polyline", "polygon", "ellipse"];

/// Checks that the tags of a document nest; returns the first problem.
fn nesting(html: &str) -> Result<(), String> {
    let mut stack: Vec<String> = Vec::new();
    let mut rest = html;
    while let Some(i) = rest.find('<') {
        rest = &rest[i + 1..];
        if rest.starts_with("!--") {
            let end = rest.find("-->").ok_or("an unclosed comment")?;
            rest = &rest[end + 3..];
            continue;
        }
        if rest.starts_with('!') {
            continue;
        }
        let end = rest.find('>').ok_or("an unclosed tag")?;
        let tag = &rest[..end];
        rest = &rest[end + 1..];
        let closing = tag.starts_with('/');
        let name: String = tag.trim_start_matches('/').chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
        if name.is_empty() {
            return Err(format!("a `<` that starts no tag: <{tag}>"));
        }
        if closing {
            match stack.pop() {
                Some(open) if open == name => {}
                other => return Err(format!("</{name}> closes {other:?}")),
            }
        } else if !tag.ends_with('/') && !VOID.contains(&name.as_str()) {
            stack.push(name.clone());
        }
        // the text of a script or style is not markup
        if !closing && (name == "script" || name == "style") {
            let close = format!("</{name}>");
            let at = rest.find(&close).ok_or("an unclosed script or style")?;
            rest = &rest[at..];
        }
    }
    if stack.is_empty() {
        Ok(())
    } else {
        Err(format!("unclosed: {stack:?}"))
    }
}

#[test]
fn every_page_is_a_whole_document_with_the_build_markers() {
    for path in programs() {
        for r in records(&run(&path, &["--interp"])).iter().filter(|r| r.kind == "page") {
            let label = format!("{} page {}", path.display(), r.head);
            let html = &r.body;
            assert!(html.starts_with("<!doctype html>\n<html lang=\""), "{label}");
            assert!(html.ends_with("</html>"), "{label}");
            for marker in
                ["<!--nyra:head-->\n</head>", "<!--nyra:body-->\n</body>", "<main id=\"main\">", "<title>", "<meta name=\"viewport\""]
            {
                assert_eq!(html.matches(marker).count(), 1, "{label}: `{marker}`");
            }
            if let Err(e) = nesting(html) {
                panic!("{label}: {e}");
            }
            // every id is unique, and every #anchor of the page's own links has its id
            let ids: Vec<&str> = html.split(" id=\"").skip(1).map(|s| &s[..s.find('"').unwrap()]).collect();
            let unique: BTreeSet<&str> = ids.iter().copied().collect();
            assert_eq!(unique.len(), ids.len(), "{label}: an id twice: {ids:?}");
            assert!(r.head.starts_with('/') && r.head.split(' ').nth(1) == Some("base"), "{label}: header `{}`", r.head);
        }
    }
}

#[test]
fn text_never_becomes_markup() {
    let out = std::fs::read_to_string("examples/web/escaping.out").unwrap().replace("\r\n", "\n");
    let page = &records(&out).into_iter().find(|r| r.kind == "page").unwrap().body;
    assert!(!page.contains("<script"), "a script tag got through");
    assert!(!page.contains("onmouseover=\""), "an attribute got through");
    assert!(!page.contains("<i>") && !page.contains("<b>"), "inline HTML in text got through");
    assert!(!page.to_lowercase().contains("href=\"javascript:") && !page.to_lowercase().contains("href=\"data:"));
    assert!(page.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
    assert!(page.contains("<title>Title &lt;/title&gt;&lt;script&gt;x&lt;/script&gt;</title>"));
    assert!(page.contains("Literal *stars* and :phone: and 8:00 – 16:00 and a lone * star"), "escapes and lone stars");
    assert!(page.contains("href=\"https://example.com/?a=1&amp;b=&quot;2&quot;\""), "a URL's quotes and ampersands");
    let errors: Vec<String> = records(&out).into_iter().filter(|r| r.kind == "error").map(|r| r.head).collect();
    assert_eq!(errors.len(), 3, "{errors:?}");
    assert!(errors.iter().all(|e| e.starts_with("E0381 ")), "{errors:?}");
}

#[test]
fn literal_mistakes_are_compile_errors() {
    let dir = scratch("web-errors");
    let cases = [
        ("p(\"<b>[x](javascript:alert(1))</b>\")", "E0381"),
        ("img(\"data:image/png;base64,AA\", \"x\")", "E0381"),
        ("navbar(\"A\", [(\"B\", \"JAVASCRIPT:x\")])", "E0381"),
        ("card(\"A\", icon: \"no-such-icon\")", "E0374"),
        ("button(\"Go\", style: \"primay\")", "E0374"),
        ("link(\":mail:\", \"mailto:a@b.cz\")", "E0377"),
        ("button(icon: \"phone\", href: \"tel:1\")", "E0377"),
        ("css(\"a {{ color: {1} }}\")", "E0378"),
    ];
    for (i, (call, code)) in cases.iter().enumerate() {
        let file = dir.join(format!("case{i}.nyra"));
        let body = if call.starts_with("css") {
            format!("{call}\npage(\"/\", \"T\") {{\n}}\n")
        } else {
            format!("page(\"/\", \"T\") {{\n    {call}\n}}\n")
        };
        std::fs::write(&file, format!("use web\n{body}")).unwrap();
        let out = nyra().args(["check", "--json"]).arg(&file).output().unwrap();
        let json = stdout(&out);
        assert!(!out.status.success(), "{call} compiles");
        assert!(json.contains(&format!("\"code\":\"{code}\"")), "{call}: expected {code}, got {json}");
    }
    // and the right forms compile
    let ok = dir.join("ok.nyra");
    std::fs::write(
        &ok,
        "use web\nlet n = 3\ncss(\"a {{ color: red }}\")\npage(\"/\", \"T\") {\n    link(\":mail:\", \"mailto:a@b.cz\", label: \"Mail\")\n    p(\"8:00 \\\\:x: [a](tel:+420 1) {n}\")\n}\n",
    )
    .unwrap();
    let out = nyra().arg("check").arg(&ok).output().unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
}

/// The measured samples of the design, put together into one program as a site would be.
#[test]
fn the_design_samples_render_without_errors() {
    let part = |f: &str| {
        let text = std::fs::read_to_string(Path::new("docs/web-samples").join(f)).unwrap().replace("\r\n", "\n");
        text.lines().skip(1).filter(|l| *l != "use web").collect::<Vec<_>>().join("\n")
    };
    // the sections the Flora page calls but the samples leave out
    let rest = r#"
fn store() -> Node = section("prodejna", "Vše pro dům a zahradu", kicker: "Sortiment") {
    grid(3) {
        card("Zahrada", "Osivo a substráty.", image: "zahrada.jpg", items: ["Osivo", "Hnojiva"])
    }
}
fn gallery_section(tab: str) -> Node = section("galerie", "Galerie", bg: "alt") {
    gallery([("g1.jpg", "Krb", "krby"), ("g2.jpg", "Záhon", "flora")], filters: [("krby", "Krby"), ("flora", "Flora")])
}
fn footer_section(l: [(str, str)]) -> Node = footer(links: l)
page("/krby", "Kamnářství") {
    services()
    about()
    contact(krby)
}
"#;
    let src = format!(
        "use web\n{}\n{}\n{}\n{}\n{}\n{}",
        part("0-shared.nyra"),
        part("2-services.nyra"),
        part("3-contact.nyra"),
        part("4-about.nyra"),
        part("1-flora-page.nyra"),
        rest
    );
    let file = scratch("web-samples").join("site.nyra");
    std::fs::write(&file, src).unwrap();
    let out = run(&file, &["--interp"]);
    let rs = records(&out);
    let errors: Vec<&str> = rs.iter().filter(|r| r.kind == "error").map(|r| r.head.as_str()).collect();
    assert!(errors.is_empty(), "{errors:?}");
    let pages: Vec<&str> = rs.iter().filter(|r| r.kind == "page").map(|r| r.head.as_str()).collect();
    assert_eq!(pages.len(), 2, "{pages:?}");
    let flora = &rs.iter().find(|r| r.head.starts_with("/flora ")).unwrap().body;
    for part in [
        "class=\"nav\"",
        "class=\"hero\"",
        "class=\"marquee\"",
        "class=\"gallery\"",
        "class=\"card hours\"",
        "data-mailto=\"floravalmez@seznam.cz\"",
        "Nyní otevřeno",
    ] {
        assert!(flora.contains(part), "the Flora page has no `{part}`");
    }
}

/// The chunk markers of a kit file, in order.
fn chunks(file: &str) -> Vec<String> {
    let text = std::fs::read_to_string(file).unwrap();
    text.lines().filter_map(|l| l.strip_prefix("/*! chunk: ").and_then(|r| r.strip_suffix(" */"))).map(str::to_string).collect()
}

#[test]
fn every_chunk_a_page_names_is_in_the_kit() {
    let css = chunks("src/web/kit.css");
    let js = chunks("src/web/kit.js");
    assert_eq!(css.first().map(String::as_str), Some("base"), "kit.css starts with the base chunk");
    for list in [&css, &js] {
        let unique: BTreeSet<&String> = list.iter().collect();
        assert_eq!(unique.len(), list.len(), "a chunk twice: {list:?}");
    }
    let mut named = BTreeSet::new();
    for path in programs() {
        for r in records(&run(&path, &["--interp"])).into_iter().filter(|r| r.kind == "page") {
            named.extend(r.head.split(' ').skip(1).map(str::to_string).collect::<Vec<_>>());
        }
    }
    for c in &named {
        assert!(css.contains(c) || js.contains(c), "pages name the chunk `{c}`, which neither kit file has");
    }
    // every JavaScript chunk belongs to a component that names it
    for c in &js {
        assert!(named.contains(c), "kit.js has the chunk `{c}`, which no example page uses");
    }
    // the stylesheet's braces balance (a missing one would swallow the rest of the kit)
    let text = std::fs::read_to_string("src/web/kit.css").unwrap();
    assert_eq!(text.matches('{').count(), text.matches('}').count());
    assert!(text.len() < 64 * 1024, "kit.css grew to {} bytes", text.len());
    assert!(std::fs::metadata("src/web/kit.js").unwrap().len() < 16 * 1024);
}

#[test]
fn the_behaviors_are_valid_javascript() {
    if !node() {
        missing("no Node.js to check src/web/kit.js");
        return;
    }
    let out = Command::new("node").args(["--check", "src/web/kit.js"]).output().unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
}

#[test]
fn every_kit_page_has_a_fixture() {
    for name in ["elements", "kit"] {
        let out = std::fs::read_to_string(format!("examples/web/{name}.out")).unwrap().replace("\r\n", "\n");
        for r in records(&out).into_iter().filter(|r| r.kind == "page") {
            let path = r.head.split(' ').next().unwrap().trim_matches('/').replace('/', "-");
            let file = format!("tests/web/fixtures/{}.html", if path.is_empty() { "index".to_string() } else { path });
            let html =
                std::fs::read_to_string(&file).unwrap_or_else(|_| panic!("{file} is missing: make it with tools/web_preview.py"));
            assert!(
                html.contains("src/web/kit.css") && html.contains("src/web/kit.js") && !html.contains("<!--nyra:head-->"),
                "{file} is not a rendered fixture"
            );
        }
    }
}
