//! `use web` (docs/WEB.md): the library (`src/std/web.nyra`), the kit (`src/web/kit.css`,
//! `src/web/kit.js`) and how they meet the build.
//!
//! - every `examples/web/*.nyra` builds (`nyra build`) to exactly the site in
//!   `tests/web/fixtures/<name>/`: those folders are the golden output and, opened in a browser,
//!   the fixture pages of every element and component;
//! - every page is a whole, well-nested HTML document;
//! - text never becomes markup, and a computed `javascript:` link fails the build (E0381);
//! - literal mistakes are compile errors; the measured samples of `docs/web-samples` build;
//! - every kit chunk a component names exists in the kit files, and the behaviors are valid
//!   JavaScript.
//!
//! After changing the library or the kit on purpose, rewrite the fixtures with
//!
//!     NYRA_BLESS=1 cargo test --release --test web
//!
//! and look at the pages (and the diff) before committing them.

mod common;

use std::collections::{BTreeMap, BTreeSet};
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

/// Every file below `dir`, by its path relative to `dir` (with `/`).
fn files(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for e in entries {
            let p = e.unwrap().path();
            if p.is_dir() {
                todo.push(p);
            } else {
                let rel = p.strip_prefix(dir).unwrap().to_string_lossy().replace('\\', "/");
                out.insert(rel, std::fs::read(&p).unwrap());
            }
        }
    }
    out
}

/// Builds an example into a fresh folder and gives its files.
fn build(path: &Path) -> BTreeMap<String, Vec<u8>> {
    let name = path.file_stem().unwrap().to_string_lossy().to_string();
    let out = scratch(&format!("web-build-{name}"));
    let _ = std::fs::remove_dir_all(&out);
    let run = nyra().arg("build").arg(path).arg("-o").arg(&out).args(["--image-tool", "none"]).output().unwrap();
    assert!(run.status.success(), "nyra build {} failed:\n{}", path.display(), stderr(&run));
    files(&out)
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).replace("\r\n", "\n")
}

#[test]
fn examples_build_to_their_fixtures() {
    let bless = std::env::var("NYRA_BLESS").is_ok();
    for path in programs() {
        let name = path.file_stem().unwrap().to_string_lossy().to_string();
        let got = build(&path);
        let dir = Path::new("tests/web/fixtures").join(&name);
        if bless {
            let _ = std::fs::remove_dir_all(&dir);
            for (f, bytes) in &got {
                let p = dir.join(f);
                std::fs::create_dir_all(p.parent().unwrap()).unwrap();
                std::fs::write(p, bytes).unwrap();
            }
        }
        let want = files(&dir);
        let (g, w): (BTreeSet<&String>, BTreeSet<&String>) = (got.keys().collect(), want.keys().collect());
        assert_eq!(g, w, "{}: the built files differ from {} (NYRA_BLESS=1 rewrites them)", path.display(), dir.display());
        for (f, bytes) in &got {
            assert!(
                text(bytes) == text(&want[f]),
                "{}: {f} differs from {} (NYRA_BLESS=1 rewrites it)",
                path.display(),
                dir.display()
            );
        }
    }
}

/// The pages of the fixtures: (label, html).
fn pages() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for p in programs() {
        let name = p.file_stem().unwrap().to_string_lossy().to_string();
        for (f, bytes) in files(&Path::new("tests/web/fixtures").join(&name)) {
            if f.ends_with(".html") {
                out.push((format!("{name}/{f}"), text(&bytes)));
            }
        }
    }
    assert!(out.len() >= 8, "only {} fixture pages", out.len());
    out
}

/// The elements whose end tag is left out (void elements).
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
            continue;
        }
        if !tag.ends_with('/') && !VOID.contains(&name.as_str()) {
            stack.push(name.clone());
        }
        // the text of a script or style is not markup
        if name == "script" || name == "style" {
            let at = rest.find(&format!("</{name}>")).ok_or("an unclosed script or style")?;
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
fn every_page_is_a_whole_document() {
    for (label, html) in pages() {
        assert!(html.starts_with("<!doctype html>\n<html lang=\""), "{label}");
        assert!(html.trim_end().ends_with("</html>"), "{label}");
        for one in [
            "<main id=\"main\">",
            "<title>",
            "<meta name=\"viewport\"",
            "<a class=\"skip\" href=\"#main\">",
            "Content-Security-Policy",
        ] {
            assert_eq!(html.matches(one).count(), 1, "{label}: `{one}`");
        }
        assert!(!html.contains("<script>"), "{label}: an inline script");
        if let Err(e) = nesting(&html) {
            panic!("{label}: {e}");
        }
        let ids: Vec<&str> = html.split(" id=\"").skip(1).map(|s| &s[..s.find('"').unwrap()]).collect();
        let unique: BTreeSet<&str> = ids.iter().copied().collect();
        assert_eq!(unique.len(), ids.len(), "{label}: an id twice: {ids:?}");
    }
}

/// A page's `style` attributes become classes of the site's CSS, so no page's policy needs
/// `'unsafe-inline'` for them; and the hero's title is painted from the first frame (no entrance
/// hides it), while its photo settles.
#[test]
fn style_attributes_become_classes_and_the_hero_title_is_not_hidden() {
    let mut with_class = 0;
    for (label, html) in pages() {
        let body = &html[html.find("<body").unwrap()..];
        assert!(!body.contains(" style=\""), "{label}: a style attribute");
        let policy = &html[html.find("Content-Security-Policy\" content=\"").unwrap()..];
        let policy = &policy[..policy.find("\">").unwrap()];
        assert!(!policy.contains("unsafe-inline"), "{label}: {policy}");
        with_class += usize::from(body.contains(" s-"));
        if let Some(i) = body.find("class=\"hero-title\"") {
            let tag = &body[body[..i].rfind('<').unwrap()..i + body[i..].find('>').unwrap()];
            assert!(!tag.contains("data-anim"), "{label}: the hero title has an entrance: {tag}");
        }
    }
    assert!(with_class > 0, "no fixture page had a style attribute to turn into a class");
    let css: String =
        files(Path::new("tests/web/fixtures/kit")).into_iter().filter(|(f, _)| f.ends_with(".css")).map(|(_, b)| text(&b)).collect();
    assert!(css.contains(".s-") && css.contains("!important"), "the rules of the style attributes are in the CSS");
}

/// `gallery` takes (image, alt) pairs too, written out or from a variable or a function: the tag
/// is `""`.
#[test]
fn a_gallery_takes_pairs_too() {
    let dir = scratch("web-gallery-pairs");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let src = "use web\n\nlet shots = [(\"https://example.com/a.jpg\", \"A\"), (\"https://example.com/b.jpg\", \"B\")]\nfn more() -> [(str, str)] = shots\n\npage(\"/\", \"Home\") {\n    h1(\"Photos\")\n    gallery([(\"https://example.com/c.jpg\", \"C\")])\n    gallery(shots)\n    gallery(more())\n}\n";
    std::fs::write(dir.join("g.nyra"), src).unwrap();
    let out = nyra().current_dir(&dir).args(["build", "g.nyra", "-o", "out", "--image-tool", "none"]).output().unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    let page = text(&std::fs::read(dir.join("out/index.html")).unwrap());
    assert_eq!(page.matches("class=\"gallery-item\"").count(), 5, "{page}");
}

#[test]
fn text_never_becomes_markup() {
    let page = text(&std::fs::read("tests/web/fixtures/escaping/index.html").unwrap());
    let body = &page[page.find("<body").unwrap()..];
    assert!(!body.contains("<script>alert"), "a script tag got through");
    assert!(!body.contains("onmouseover=\""), "an attribute got through");
    assert!(!body.contains("<i>") && !body.contains("<b>"), "inline HTML in text got through");
    assert!(body.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
    assert!(page.contains("<title>Title &lt;/title&gt;&lt;script&gt;x&lt;/script&gt;</title>"));
    assert!(body.contains("Literal *stars* and :phone: and 8:00 – 16:00 and a lone * star"), "escapes and lone stars");
    assert!(body.contains("href=\"https://example.com/?a=1&amp;b=&quot;2&quot;\""), "a URL's quotes and ampersands");
    assert!(body.contains("<a href=\"/x?a=(1)\">link</a>"), "parentheses inside a Markdown link");
}

#[test]
fn a_computed_forbidden_link_fails_the_build() {
    let dir = scratch("web-forbidden");
    let file = dir.join("site.nyra");
    std::fs::write(
        &file,
        "use web\nlet scheme = \"javascript\"\nlet data = \"DATA\"\npage(\"/\", \"T\") {\n    h1(\"T\")\n    p(\"[a]({scheme}:alert(1))\")\n    button(\"B\", href: \"{scheme}:void(0)\")\n    list([\"[x]({data}:text/html,hi)\"])\n}\n",
    )
    .unwrap();
    let out = nyra().args(["build", "--json"]).arg(&file).arg("-o").arg(dir.join("dist")).output().unwrap();
    let json = stdout(&out);
    assert!(!out.status.success(), "the build succeeded: {json}");
    assert_eq!(json.matches("\"code\":\"E0381\"").count(), 3, "{json}");
    assert!(!dir.join("dist").join("index.html").exists(), "a failed build wrote a page");
}

#[test]
fn literal_mistakes_are_compile_errors() {
    let dir = scratch("web-errors");
    let cases = [
        ("p(\"<b>[x](javascript:alert(1))</b>\")", "E0381"),
        ("img(\"data:image/png;base64,AA\", \"x\")", "E0381"),
        ("navbar(\"A\", [(\"B\", \"JAVASCRIPT:x\")])", "E0381"),
        ("section(\"a\", \"A\", bg: \"gray\")", "E0374"),
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
        "use web\nlet n = 3\ncss(\"a {{ color: red }}\")\npage(\"/\", \"T\") {\n    h1(\"T\")\n    link(\":mail:\", \"mailto:a@b.cz\", label: \"Mail\")\n    p(\"8:00 \\\\:x: [a](tel:+420 1) {n}\")\n}\n",
    )
    .unwrap();
    let out = nyra().arg("check").arg(&ok).output().unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
}

/// What models write that the kit reads as meant: an icon outside the set (a warning at the
/// literal that holds it, even from data, and a stand-in), a clickable card, a table of tuples,
/// `css(...)` in a page's block, opening hours written as people write them, a `mailto:` with
/// its own `?subject=` and the form's own submit button.
#[test]
fn the_kit_reads_what_models_write() {
    let dir = scratch("web-model-forms");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let src = r#"use web
site("Dentist", tz: "Europe/Prague")
let services = [("siren", "Emergency care"), ("envelope", "Write to us")]
let prices = [("Check-up", 900), ("Hygiene", 1600)]
let opening = [("Monday", "8:00–18:00"), ("Tue–Fri", "8 - 14"), ("Saturday", "Closed"), ("Sunday", "")]
enum Msg { Open(int) }
var open = -1
fn update(m: Msg) {
    match m {
        Msg.Open(i) => open = if open == i { -1 } else { i }
    }
}
fn faq() -> Node = stack {
    for i in 0..2 {
        card("Question {i}", if open == i { "Answer {i}" } else { "" }, on: Msg.Open(i))
    }
}
page("/", "Dentist") {
    h1("Dentist")
    section("services", "What we do") {
        for s in services {
            card(s.1, "Call us", icon: s.0)
        }
        faq()
        table(prices, head: ["Treatment", "Price"])
        hours(opening)
        form("Book", mailto: "desk@example.com?subject=Booking request") {
            field("name", "Name", req: true)
            button("Send request")
        }
    }
    css(".x { color: red }")
}
"#;
    std::fs::write(dir.join("s.nyra"), src).unwrap();
    let out = nyra().current_dir(&dir).args(["build", "s.nyra", "-o", "out", "--image-tool", "none"]).output().unwrap();
    let err = stderr(&out);
    assert!(out.status.success(), "{err}");
    // the icons: a warning each, at the literal in the data, drawn as a stand-in
    assert!(err.contains("warning[E0389]: unknown icon `siren`: drawn as a plain circle\n  --> s.nyra:3:18"), "{err}");
    assert!(err.contains("warning[E0389]: unknown icon `envelope`: drawn as `mail`\n  --> s.nyra:3:47"), "{err}");
    let page = text(&std::fs::read(dir.join("out/index.html")).unwrap());
    assert!(page.contains("<use href=\"#i-circle\"/>") && page.contains("<use href=\"#i-mail\"/>"), "{page}");
    // the card is a button
    assert!(page.contains("<h3 class=\"card-title\"><button type=\"button\" class=\"card-hit\" data-on="), "{page}");
    // the table shows the numbers as written
    assert!(page.contains("<td>Check-up</td><td>900</td>"), "{page}");
    // the hours: days from the labels, any dash, words for closed
    assert!(page.contains("data-hours=\"1 8:00-18:00|2-5 8:00-14:00\""), "{page}");
    assert!(page.contains("<tr class=\"is-shut\"><th scope=\"row\">Saturday</th><td>Closed</td></tr>"), "{page}");
    // the form: the address and the subject apart, one submit button: the program's own
    assert!(page.contains("data-mailto=\"desk@example.com\" data-query=\"subject=Booking%20request\""), "{page}");
    assert_eq!(page.matches("type=\"submit\"").count(), 1, "{page}");
    assert!(page.contains("<button type=\"submit\" class=\"btn btn-primary\">Send request</button>"), "{page}");
    let css: String = std::fs::read_dir(dir.join("out"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "css"))
        .map(|p| text(&std::fs::read(p).unwrap()))
        .collect();
    assert!(css.contains(".x{color:red}") && !css.contains("nyra:"), "the page's css(...) is in the CSS, the build's records are not");
}

/// The measured samples of the design, put together into one program as a site would be.
#[test]
fn the_design_samples_build() {
    let part = |f: &str| {
        let text = std::fs::read_to_string(Path::new("docs/web-samples").join(f)).unwrap().replace("\r\n", "\n");
        text.lines().skip(1).filter(|l| *l != "use web").collect::<Vec<_>>().join("\n")
    };
    // the sections the Flora page calls but the samples leave out
    let rest = r#"
fn store() -> Node = section("prodejna", "Vše pro dům a zahradu", kicker: "Sortiment") {
    grid(3) {
        card("Zahrada", "Osivo a substráty.", image: "zahrada.svg", items: ["Osivo", "Hnojiva"])
    }
}
fn gallery_section(tab: str) -> Node = section("galerie", "Galerie", bg: "alt") {
    gallery([("g1.svg", "Krb", "krby"), ("g2.svg", "Záhon", "flora")], filters: [("krby", "Krby"), ("flora", "Flora")])
}
fn footer_section(l: [(str, str)]) -> Node = footer(links: l)
page("/krby", "Kamnářství") {
    h1("Kamnářství Kuběna")
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
    )
    .replace(".jpg\"", ".svg\"");
    let dir = scratch("web-samples");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    let svg = std::fs::read("examples/web/assets/photo-1.svg").unwrap();
    for img in ["flora-hero", "about-namesti", "zahrada", "g1", "g2"] {
        std::fs::write(dir.join("assets").join(format!("{img}.svg")), &svg).unwrap();
    }
    std::fs::write(dir.join("site.nyra"), src).unwrap();
    let out = nyra().arg("build").arg(dir.join("site.nyra")).args(["--image-tool", "none"]).output().unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    let flora = std::fs::read_to_string(dir.join("dist/flora/index.html")).unwrap();
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
    assert!(dir.join("dist/krby/index.html").is_file());
}

/// The chunk markers of a kit file, in order.
fn chunks(file: &str) -> Vec<String> {
    let text = std::fs::read_to_string(file).unwrap();
    text.lines().filter_map(|l| l.strip_prefix("/*! chunk: ").and_then(|r| r.strip_suffix(" */"))).map(str::to_string).collect()
}

/// The chunk names the library hands to the build (`kit: ["gallery"]`, `_kits(kids, "hero")`, ...).
fn named_chunks() -> BTreeSet<String> {
    let src = std::fs::read_to_string("src/std/web.nyra").unwrap();
    let mut out = BTreeSet::new();
    for pat in ["kit: [\"", "_kits(kids, \"", "_add(kit, \""] {
        for part in src.split(pat).skip(1) {
            let name = &part[..part.find('"').unwrap()];
            if !name.is_empty() {
                out.insert(name.to_string());
            }
        }
    }
    out
}

#[test]
fn every_chunk_a_component_names_is_in_the_kit() {
    let css = chunks("src/web/kit.css");
    let js = chunks("src/web/kit.js");
    for list in [&css, &js] {
        let unique: BTreeSet<&String> = list.iter().collect();
        assert_eq!(unique.len(), list.len(), "a chunk twice: {list:?}");
    }
    let named = named_chunks();
    assert!(named.len() >= 12, "{named:?}");
    for c in &named {
        assert!(css.contains(c) || js.contains(c), "the library names the chunk `{c}`, which neither kit file has");
    }
    for c in css.iter().chain(&js) {
        assert!(named.contains(c), "the kit has the chunk `{c}`, which no component names");
    }
    // the stylesheet's braces balance (a missing one would swallow the rest of the kit)
    let text = std::fs::read_to_string("src/web/kit.css").unwrap();
    assert_eq!(text.matches('{').count(), text.matches('}').count());
    assert!(text.len() < 64 * 1024, "kit.css grew to {} bytes", text.len());
    // (a page gets only the chunks it uses; the file holds them all)
    assert!(std::fs::metadata("src/web/kit.js").unwrap().len() < 20 * 1024);
}

#[test]
fn the_behaviors_are_valid_javascript() {
    if Command::new("node").arg("--version").output().is_err() {
        missing("no Node.js to check src/web/kit.js");
        return;
    }
    let mut scripts = vec![PathBuf::from("src/web/kit.js")];
    for (f, _) in files(Path::new("tests/web/fixtures")) {
        if f.ends_with(".js") {
            scripts.push(Path::new("tests/web/fixtures").join(f));
        }
    }
    assert!(scripts.len() >= 3, "{scripts:?}");
    for s in scripts {
        let out = Command::new("node").arg("--check").arg(&s).output().unwrap();
        assert!(out.status.success(), "{}: {}", s.display(), stderr(&out));
    }
}
