//! `nyra clone` (docs/CLONE.md) on the saved pages in `tests/clone/` (no network: each page is a
//! local file given a `--base` address, and links only stylesheets next to it):
//!
//! - the program it writes compiles and builds with `nyra build`;
//! - the built page has every heading, paragraph, list item and quote of the original (text the
//!   original hides, marked `data-dropped` in the fixture, is left out on purpose), and every
//!   content image URL;
//! - each page maps to the kit components it should, and repeated data becomes a struct, an
//!   array and a loop.

mod common;

use std::path::{Path, PathBuf};

use common::{nyra, scratch, stderr, stdout, Json};

struct Case {
    name: &'static str,
    /// Components that must appear in the summary.
    components: &'static [&'static str],
    /// Text that must appear in the program.
    source: &'static [&'static str],
    /// Image URLs that come from CSS (backgrounds), which the HTML scan cannot see.
    css_images: &'static [&'static str],
}

const CASES: &[Case] = &[
    Case {
        name: "bakery",
        components: &["navbar", "hero", "card", "stat", "quote", "hours", "map", "form", "footer"],
        source: &["struct Bread {", "for c in breads { card(", "primary: \"#7a3e1d\"", "cta: (\"Order a cake\", \"#order\")"],
        css_images: &["https://bakery.example/img/counter.jpg"],
    },
    Case {
        name: "saas",
        components: &["navbar", "hero", "gallery", "card", "steps", "stat", "footer"],
        source: &["struct Feature {", "primary: \"#155dfc\"", "font: \"Inter\"", "display: \"Space Grotesk\"", "bg: \"dark\""],
        css_images: &[],
    },
    Case {
        name: "portfolio",
        components: &["navbar", "hero", "gallery", "list", "footer"],
        source: &[
            "lang: \"cs\"",
            "style: \"dark\"",
            "https://portfolio.example/foto/7-1200.jpg",
            "https://portfolio.example/foto/6.jpg",
        ],
        css_images: &["https://portfolio.example/foto/uvod.jpg"],
    },
    Case {
        name: "nested",
        components: &["hero", "list", "table", "img"],
        source: &["{{name}}", "${{price}}", "\\\\*\\\\*like this\\\\*\\\\*", "\\\\:phone:", "C:\\\\\\\\Users", "](#syntax)"],
        css_images: &[],
    },
    Case {
        name: "wordpress",
        components: &["navbar", "hero", "gallery", "embed", "hours", "footer"],
        source: &["(\"Mon–Fri\", \"1-5 8:00-18:00\")", "wp-content/uploads/2026/04/tulips.jpg"],
        css_images: &["https://greenvalley.example/wp-content/uploads/2026/03/greenhouse-1600x900.jpg"],
    },
];

fn fixture(name: &str) -> PathBuf {
    Path::new("tests/clone").join(format!("{name}.html"))
}

/// Clones a fixture into a fresh folder: (the program's path, the summary JSON).
fn clone(name: &str) -> (PathBuf, Json) {
    let dir = scratch(&format!("clone-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("site.nyra");
    let run = nyra()
        .arg("clone")
        .arg(fixture(name))
        .args(["--base", &format!("https://{name}.example/"), "--json", "-o"])
        .arg(&out)
        .output()
        .unwrap();
    let text = stdout(&run);
    let json = Json::parse(text.trim()).unwrap_or_else(|e| panic!("nyra clone {name}: bad JSON ({e}):\n{text}\n{}", stderr(&run)));
    assert!(run.status.success(), "nyra clone {name} failed:\n{text}\n{}", stderr(&run));
    (out, json)
}

fn build(program: &Path) -> String {
    let dist = program.parent().unwrap().join("dist");
    let run = nyra().arg("build").arg(program).arg("-o").arg(&dist).args(["--image-tool", "none"]).output().unwrap();
    assert!(run.status.success(), "nyra build {} failed:\n{}", program.display(), stderr(&run));
    std::fs::read_to_string(dist.join("index.html")).unwrap()
}

/// Character references the fixtures and the kit write.
fn decode(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let Some(end) = rest.find(';').filter(|e| *e < 10) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let name = &rest[1..end];
        let c = if let Some(n) = name.strip_prefix("#x") {
            u32::from_str_radix(n, 16).ok().and_then(char::from_u32)
        } else if let Some(n) = name.strip_prefix('#') {
            n.parse().ok().and_then(char::from_u32)
        } else {
            Some(match name {
                "amp" => '&',
                "lt" => '<',
                "gt" => '>',
                "quot" => '"',
                "apos" => '\'',
                "nbsp" => ' ',
                "middot" => '·',
                "mdash" => '—',
                "ndash" => '–',
                "laquo" => '«',
                "raquo" => '»',
                "euro" => '€',
                "copy" => '©',
                _ => '?',
            })
        };
        out.push(c.unwrap_or('?'));
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out
}

/// The text of HTML without tags, spaces and punctuation (so line breaks and inline boxes do not
/// matter).
fn bare(html: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    // letters and digits in order: separators between links (`a · b`) may become separate lines,
    // and "Jana Novak, Brno" a name and a place
    decode(&out).chars().filter(|c| c.is_alphanumeric()).collect()
}

/// The contents of every `<tag ...>...</tag>` of these tags that is not marked `data-dropped`.
fn texts(html: &str, tags: &[&str]) -> Vec<String> {
    let lower = html.to_ascii_lowercase();
    let mut out = Vec::new();
    for tag in tags {
        let open = format!("<{tag}");
        let mut from = 0;
        while let Some(at) = lower[from..].find(&open).map(|a| a + from) {
            from = at + open.len();
            let next = lower.as_bytes().get(from).copied().unwrap_or(b' ');
            if next != b'>' && next != b' ' {
                continue;
            }
            let Some(gt) = lower[from..].find('>').map(|g| g + from) else { break };
            if lower[at..gt].contains("data-dropped") {
                continue;
            }
            // up to the end tag, or the next block that closes it (the fixtures leave some open)
            let body = &lower[gt + 1..];
            let mut end = body.len();
            for stop in [
                format!("</{tag}>"),
                "<li".into(),
                "<p>".into(),
                "<p ".into(),
                "</ul".into(),
                "<div".into(),
                "</div".into(),
                "<table".into(),
                "<ul".into(),
            ] {
                if let Some(e) = body.find(&stop) {
                    end = end.min(e);
                }
            }
            let t = bare(&html[gt + 1..gt + 1 + end]);
            if !t.is_empty() {
                out.push(t);
            }
        }
    }
    out
}

fn images(html: &str, base: &str) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(at) = rest.find("<img") {
        rest = &rest[at + 4..];
        let tag = &rest[..rest.find('>').unwrap_or(rest.len())];
        if tag.contains("data-dropped") {
            continue;
        }
        let attr = |name: &str| -> Option<String> {
            let key = format!(" {name}=\"");
            tag.find(&key).map(|a| {
                let v = &tag[a + key.len()..];
                decode(&v[..v.find('"').unwrap_or(v.len())])
            })
        };
        let mut cands = Vec::new();
        for k in ["data-src", "data-lazy-src", "src"] {
            if let Some(v) = attr(k).filter(|v| !v.starts_with("data:")) {
                cands.push(v);
            }
        }
        if let Some(set) = attr("srcset") {
            cands.extend(set.split(',').filter_map(|p| p.split_whitespace().next().map(str::to_string)));
        }
        let abs: Vec<String> = cands
            .into_iter()
            .map(|u| {
                if u.starts_with("http") {
                    u
                } else {
                    format!("{}{}", base.trim_end_matches('/'), if u.starts_with('/') { u } else { format!("/{u}") })
                }
            })
            .collect();
        out.push(abs);
    }
    out
}

#[test]
fn cloned_pages_build_and_keep_their_content() {
    for case in CASES {
        let html = std::fs::read_to_string(fixture(case.name)).unwrap();
        let (program, json) = clone(case.name);
        let src = std::fs::read_to_string(&program).unwrap();
        assert_eq!(json.get("ok").and_then(Json::as_bool), Some(true), "{}: {json:?}", case.name);
        let errors = json.get("errors").and_then(Json::as_array).unwrap();
        assert!(errors.is_empty(), "{}: the self-check reports {errors:?}", case.name);

        // the components
        let parts: Vec<String> = json
            .get("parts")
            .and_then(Json::as_array)
            .unwrap()
            .iter()
            .map(|p| p.get("component").and_then(Json::as_str).unwrap_or("").to_string())
            .collect();
        let all = parts.join(" | ");
        for c in case.components {
            assert!(all.contains(c), "{}: no `{c}` in the components: {all}\n{src}", case.name);
        }
        for s in case.source {
            assert!(src.contains(s), "{}: the program has no `{s}`:\n{src}", case.name);
        }

        // it builds, and the page has the original's text and images
        let page = build(&program);
        let text = bare(&page);
        for t in texts(&html, &["h1", "h2", "h3", "h4", "p", "li", "blockquote", "figcaption", "summary"]) {
            assert!(text.contains(&t), "{}: the built page lacks the text `{t}`\nprogram:\n{src}", case.name);
        }
        let base = format!("https://{}.example/", case.name);
        for cands in images(&html, &base) {
            assert!(
                cands.iter().any(|u| page.contains(u.as_str())),
                "{}: the built page has none of {cands:?}\nprogram:\n{src}",
                case.name
            );
        }
        for u in case.css_images {
            assert!(page.contains(u), "{}: the built page lacks the background image {u}\nprogram:\n{src}", case.name);
        }
    }
}

#[test]
fn what_is_dropped_is_counted() {
    let (_, json) = clone("wordpress");
    let dropped = json.get("dropped").unwrap();
    assert!(dropped.get("scripts").and_then(Json::as_u64).is_some_and(|n| n >= 2), "{dropped:?}");
    assert!(dropped.get("cookie banners and dialogs").is_some(), "{dropped:?}");
    assert!(dropped.get("iframes").is_some(), "{dropped:?}");
    let kept = json.get("kept").unwrap();
    let (w, of) = (kept.get("words").and_then(Json::as_u64).unwrap(), kept.get("of_words").and_then(Json::as_u64).unwrap());
    assert!(w * 100 >= of * 90, "kept {w} of {of} words");
}

#[test]
fn a_file_not_written_by_clone_is_not_overwritten() {
    let dir = scratch("clone-keep");
    let mine = dir.join("mine.nyra");
    std::fs::write(&mine, "print(1)\n").unwrap();
    let run = nyra().arg("clone").arg(fixture("nested")).arg("-o").arg(&mine).output().unwrap();
    assert!(!run.status.success());
    assert!(stderr(&run).contains("was not written by `nyra clone`"), "{}", stderr(&run));
    assert_eq!(std::fs::read_to_string(&mine).unwrap(), "print(1)\n");
}

#[test]
fn local_images_are_copied_into_assets() {
    let dir = scratch("clone-local");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("page")).unwrap();
    // a tiny PNG next to a saved page
    let png: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13, 0x49, 0x48, 0x44, 0x52, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0,
        0x1F, 0x15, 0xC4, 0x89, 0, 0, 0, 13, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0xF8, 0xCF, 0xC0, 0xF0, 0x1F, 0, 0x05, 0x00,
        0x01, 0xFF, 0x89, 0x99, 0x3D, 0x1D, 0, 0, 0, 0, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];
    std::fs::write(dir.join("page").join("photo one.png"), png).unwrap();
    std::fs::write(
        dir.join("page").join("index.html"),
        "<html><body><h1>Local page</h1><p>With a photo.</p><img src=\"photo%20one.png\" alt=\"A photo\" width=\"800\"></body></html>",
    )
    .unwrap();
    let out = dir.join("site").join("site.nyra");
    std::fs::create_dir_all(out.parent().unwrap()).unwrap();
    let run = nyra().arg("clone").arg(dir.join("page").join("index.html")).arg("-o").arg(&out).output().unwrap();
    assert!(run.status.success(), "{}", stderr(&run));
    assert!(out.parent().unwrap().join("assets").join("photo-one.png").is_file());
    let src = std::fs::read_to_string(&out).unwrap();
    assert!(src.contains("\"photo-one.png\""), "{src}");
    let page = build(&out);
    assert!(page.contains("img/photo-one-"), "{page}");
}
