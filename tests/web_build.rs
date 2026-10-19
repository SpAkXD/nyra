//! `use web`: `nyra build` writes a site, `nyra check` runs its checks, `nyra run` serves it with
//! live reload, and the client code runs `update` and re-renders in Node.js. The site is
//! `tests/web_build/site.nyra`, written against the stub library `tests/web_build/minimal_web.nyra`
//! (made the `web` module with `NYRA_WEB_LIB`), so these tests do not depend on the real kit.

mod common;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use common::{missing, scratch, stderr, stdout, Json};

/// `nyra` with the stub web library.
fn nyra() -> Command {
    let mut c = common::nyra();
    c.env("NYRA_WEB_LIB", std::fs::canonicalize("tests/web_build/minimal_web.nyra").unwrap());
    c
}

/// A PNG header of 800 x 600 (the build reads only the header).
fn png() -> Vec<u8> {
    let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    b.extend_from_slice(&800u32.to_be_bytes());
    b.extend_from_slice(&600u32.to_be_bytes());
    b.extend_from_slice(&[8, 2, 0, 0, 0]);
    b
}

/// A fresh folder with the test site and its images.
fn site(name: &str) -> PathBuf {
    let dir = scratch(&format!("web-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    std::fs::copy("tests/web_build/site.nyra", dir.join("site.nyra")).unwrap();
    std::fs::write(dir.join("assets/photo.png"), png()).unwrap();
    std::fs::write(dir.join("assets/logo.svg"), "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"48\" height=\"24\"></svg>\n")
        .unwrap();
    dir
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// The one file of `dir` whose name starts with `prefix` and ends with `ext`.
fn one(dir: &Path, prefix: &str, ext: &str) -> PathBuf {
    let found: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.file_name().unwrap().to_string_lossy().starts_with(prefix) && p.to_string_lossy().ends_with(ext))
        .collect();
    assert_eq!(found.len(), 1, "{prefix}*{ext} in {}: {found:?}", dir.display());
    found[0].clone()
}

#[test]
fn build_writes_the_pages_the_assets_and_the_site_files() {
    let dir = site("build");
    let out = nyra().current_dir(&dir).args(["build", "site.nyra", "--image-tool", "none"]).output().unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    let err = stderr(&out);
    assert!(err.contains("warning[E0386]") && err.contains("nyra: built 2 page(s) into"), "{err}");
    let dist = dir.join("dist");

    // the pages, with the policy first, then the files at the end of the head (relative: the site works from a folder)
    let home = read(&dist.join("index.html"));
    let css = one(&dist, "site.", ".css");
    let js = one(&dist, "site.", ".js");
    let (css_name, js_name) =
        (css.file_name().unwrap().to_string_lossy().into_owned(), js.file_name().unwrap().to_string_lossy().into_owned());
    assert!(home.starts_with("<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:;"), "{home}");
    assert!(
        home.contains(&format!("<link rel=\"stylesheet\" href=\"{css_name}\"><script src=\"{js_name}\" defer></script></head>")),
        "{home}"
    );
    let about = read(&dist.join("about/index.html"));
    assert!(about.contains(&format!("href=\"../{css_name}\"")) && about.contains(&format!("src=\"../{js_name}\"")), "{about}");
    assert!(!home.contains("__nyra"), "the reload script is only for the dev server");

    // images: copied with a hash, with their size, lazy
    let img = std::fs::read_dir(dist.join("img"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert_eq!(img.len(), 2, "{img:?}");
    let photo = img.iter().find(|n| n.starts_with("photo-") && n.ends_with(".png")).unwrap();
    assert!(
        home.contains(&format!(
            "<img src=\"img/{photo}\" alt=\"The counter\" width=\"800\" height=\"600\" loading=\"lazy\" decoding=\"async\">"
        )),
        "{home}"
    );
    assert!(about.contains("width=\"48\" height=\"24\""), "{about}");
    assert_eq!(std::fs::read(dist.join("img").join(photo)).unwrap(), png());

    // the CSS: custom keyframes and the theme; the JS: the client code
    let css_text = read(&css);
    assert!(css_text.contains("@keyframes sway{50%{rotate:3deg}}") && css_text.contains("--ink:#1a1a1a"), "{css_text}");
    assert!(read(&js).contains("const ny_web = {"));

    // sitemap, robots, headers
    assert_eq!(
        read(&dist.join("sitemap.xml")),
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n  <url><loc>https://example.com/</loc></url>\n  <url><loc>https://example.com/about</loc></url>\n</urlset>\n"
    );
    assert_eq!(read(&dist.join("robots.txt")), "User-agent: *\nAllow: /\nSitemap: https://example.com/sitemap.xml\n");
    let headers = read(&dist.join("_headers"));
    assert!(
        headers.starts_with("/*\n  Content-Security-Policy: default-src 'self'; script-src 'self';")
            && headers.contains("frame-ancestors 'self'"),
        "{headers}"
    );

    // a second build replaces the files of the first one
    std::fs::write(dir.join("site.nyra"), read(&dir.join("site.nyra")).replace("Bread every morning", "Fresh bread")).unwrap();
    std::fs::write(dist.join("img").join("stale-0000.png"), b"x").unwrap();
    let out = nyra().current_dir(&dir).args(["build", "site.nyra", "--image-tool", "none"]).output().unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(read(&dist.join("index.html")).contains("Fresh bread"));
    assert!(dist.join("img").join("stale-0000.png").exists(), "a file the build did not write is left alone");
    assert!(!dist.join(&js_name).exists() || read(&dist.join("index.html")).contains(&js_name));
}

#[test]
fn build_writes_only_into_a_folder_of_its_own() {
    let dir = site("out");
    std::fs::create_dir_all(dir.join("mine")).unwrap();
    std::fs::write(dir.join("mine/notes.txt"), "keep me").unwrap();
    let out = nyra().current_dir(&dir).args(["build", "site.nyra", "-o", "mine", "--image-tool", "none"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert!(stderr(&out).contains("has files that an earlier `nyra build` did not write"), "{}", stderr(&out));
    assert_eq!(read(&dir.join("mine/notes.txt")), "keep me");
    let out = nyra().current_dir(&dir).args(["build", "site.nyra", "-o", "public", "--image-tool", "none"]).output().unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(dir.join("public/index.html").is_file() && dir.join("public/.nyra-build").is_file());
}

#[test]
fn json_build_lists_the_files() {
    let dir = site("json");
    let out = nyra().current_dir(&dir).args(["build", "--json", "site.nyra", "--image-tool", "none"]).output().unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    let json = Json::parse(stdout(&out).trim()).unwrap();
    assert_eq!(json.get("ok").and_then(Json::as_bool), Some(true));
    let pages: Vec<&str> = json.get("pages").and_then(Json::as_array).unwrap().iter().filter_map(Json::as_str).collect();
    assert_eq!(pages, ["/", "/about"]);
    let files = json.get("files").and_then(Json::as_array).unwrap();
    assert!(files.iter().any(|f| f.get("path").and_then(Json::as_str) == Some("about/index.html")
        && f.get("bytes").and_then(Json::as_u64).is_some_and(|b| b > 100)));
    let warnings = json.get("warnings").and_then(Json::as_array).unwrap();
    assert_eq!(warnings[0].get("code").and_then(Json::as_str), Some("E0386"));
}

#[test]
fn the_site_files_stay_within_the_size_budget() {
    let dir = site("budget");
    let out = nyra().current_dir(&dir).args(["build", "site.nyra", "--image-tool", "none"]).output().unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    let dist = dir.join("dist");
    let (css, js) = (one(&dist, "site.", ".css"), one(&dist, "site.", ".js"));
    let raw = |p: &Path| std::fs::metadata(p).unwrap().len();
    // the targets of docs/WEB.md are gzipped sizes (CSS 15 KB, JS 8 KB); without Node the raw
    // sizes stand in, with room for what gzip saves
    assert!(raw(&css) < 48 * 1024 && raw(&js) < 24 * 1024, "css {} js {}", raw(&css), raw(&js));
    if Command::new("node").arg("--version").output().is_err() {
        missing("no Node.js to measure the gzipped sizes");
        return;
    }
    let gz = |p: &Path| -> u64 {
        let out = Command::new("node")
            .args(["-e", "process.stdout.write(String(require('zlib').gzipSync(require('fs').readFileSync(process.argv[1]), {level: 9}).length))"])
            .arg(p)
            .output()
            .unwrap();
        stdout(&out).trim().parse().unwrap()
    };
    let (gcss, gjs) = (gz(&css), gz(&js));
    assert!(gcss < 15 * 1024 && gjs < 8 * 1024, "gzipped: css {gcss} js {gjs}");
    // the client code is small: the runtime parts it calls, the program's functions, runtime.js
    assert!(gjs < 6 * 1024, "the client code alone is {gjs} bytes gzipped");
}

#[test]
fn the_client_code_runs_update_and_rerenders_in_node() {
    if Command::new("node").arg("--version").output().is_err() {
        missing("no Node.js to run the client code");
        return;
    }
    let dir = site("client");
    let out = nyra().current_dir(&dir).args(["build", "site.nyra", "--image-tool", "none"]).output().unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    let js = one(&dir.join("dist"), "site.", ".js");
    let out = Command::new("node").arg("tests/web_build/client_test.js").arg(&js).output().unwrap();
    assert!(out.status.success() && stdout(&out).trim() == "ok", "{}\n{}", stdout(&out), stderr(&out));
}

#[test]
fn a_view_that_returns_a_node_is_rendered_by_the_program_s_render() {
    let dir = scratch("web-render");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let src = r#"use web
use json

struct Item { tag: str, text: str }
enum Msg { Add }
var n = 0

fn update(m: Msg) {
    n += 1
}

fn render(x: Item) -> str = "<" + x.tag + ">" + x.text + "</" + x.tag + ">"

fn counter() -> Item = Item(tag: "p", text: "{n} items")

let msg = json.str(Msg.Add)
_emit("/", "<!doctype html><html><head><title>Cart</title></head><body><h1>Cart</h1><button data-on='" + msg + "'>Add</button><div data-live=\"counter\">" + render(counter()) + "</div></body></html>")
"#;
    std::fs::write(dir.join("cart.nyra"), src).unwrap();
    let out = nyra().current_dir(&dir).args(["build", "cart.nyra"]).output().unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    let js_path = one(&dir.join("dist"), "site.", ".js");
    let js = read(&js_path);
    assert!(js.contains("\"counter\": () => {") && js.contains("render(counter(ny_st[\"n\"]))"), "{js}");
    if Command::new("node").arg("--version").output().is_err() {
        missing("no Node.js to run the client code");
        return;
    }
    let test = format!(
        "require({:?}); const w = globalThis.nyra_web.web; w.update('\"Add\"'); w.update('\"Add\"'); process.stdout.write(w.views.counter());",
        js_path.display().to_string()
    );
    let out = Command::new("node").args(["-e", &test]).output().unwrap();
    assert_eq!(stdout(&out), "<p>2 items</p>", "{}", stderr(&out));
}

#[test]
fn check_reports_the_site_s_errors_and_writes_nothing() {
    let dir = scratch("web-check");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let src = r##"use web

fn doc(path: str, body: str) {
    _emit(path, "<!doctype html><html><head><title>T</title></head><body>" + body + "</body></html>")
}

doc("/", "<h1>Home</h1><a href=\"#contact\">Contact</a><a href=\"/about#team\">Team</a><img src=\"gone.png\" alt=\"\">")
doc("/about", "<h2>About</h2><section id=\"team\"></section>")
doc("/about/", "<h1>Again</h1>")
"##;
    std::fs::write(dir.join("a.nyra"), src).unwrap();
    let out = nyra().current_dir(&dir).args(["check", "--json", "a.nyra"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let json = Json::parse(stdout(&out).trim()).unwrap();
    let codes: Vec<&str> =
        json.get("errors").and_then(Json::as_array).unwrap().iter().filter_map(|e| e.get("code").and_then(Json::as_str)).collect();
    assert_eq!(codes, ["E0387", "E0382", "E0383"], "{json:?}");
    let warnings: Vec<&str> =
        json.get("warnings").and_then(Json::as_array).unwrap().iter().filter_map(|e| e.get("code").and_then(Json::as_str)).collect();
    assert_eq!(warnings, ["E0379"]);
    assert!(!dir.join("dist").exists(), "check writes nothing");

    // the anchor to another page is found there
    let ok = src
        .replace("<h2>About</h2>", "<h1>About</h1>")
        .replace("doc(\"/about/\", \"<h1>Again</h1>\")\n", "")
        .replace("<img src=\\\"gone.png\\\" alt=\\\"\\\">", "");
    let ok = ok.replace("<a href=\\\"#contact\\\">Contact</a>", "");
    std::fs::write(dir.join("b.nyra"), &ok).unwrap();
    let out = nyra().current_dir(&dir).args(["check", "b.nyra"]).output().unwrap();
    assert!(out.status.success(), "{}\n{ok}", stderr(&out));
    assert!(stderr(&out).contains("nyra: no errors (2 page(s)"), "{}", stderr(&out));
}

#[test]
fn a_runtime_error_while_building_is_reported() {
    let dir = scratch("web-runtime");
    std::fs::write(dir.join("a.nyra"), "use web\nlet xs = [1, 2]\nlet i = 5\n_emit(\"/\", \"<h1>{xs[i]}</h1>\")\n").unwrap();
    let out = nyra().current_dir(&dir).args(["build", "a.nyra", "-o", "out"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = stderr(&out);
    assert!(err.contains("runtime error[E0240]") && err.contains("the program stopped while the site was built"), "{err}");
}

#[test]
fn a_web_program_gets_no_capability_unless_allowed() {
    let dir = scratch("web-caps");
    std::fs::write(dir.join("a.nyra"), "use web\nuse fs\n_emit(\"/\", \"<h1>\" + fs.read(\"a.nyra\").slice(0, 3) + \"</h1>\")\n")
        .unwrap();
    let out = nyra().current_dir(&dir).args(["build", "a.nyra", "-o", "out1"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("error[E0290]") && stderr(&out).contains("--allow fs"), "{}", stderr(&out));
    let out = nyra().current_dir(&dir).args(["build", "a.nyra", "-o", "out2", "--allow", "fs"]).output().unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(read(&dir.join("out2/index.html")).contains("<h1>use</h1>"));
}

#[test]
fn images_get_webp_variants_with_an_image_tool() {
    // a stand-in for cwebp that copies its input: `cwebp -quiet -q 80 -resize W 0 in -o out`
    let dir = site("tool");
    let tool = if cfg!(windows) {
        let p = dir.join("cwebp.cmd");
        std::fs::write(&p, "@copy /y %7 %9 >nul\r\n").unwrap();
        p
    } else {
        let p = dir.join("cwebp");
        std::fs::write(&p, "#!/bin/sh\ncp \"$7\" \"$9\"\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        p
    };
    let out = nyra().current_dir(&dir).args(["build", "site.nyra", "--image-tool"]).arg(&tool).output().unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(!stderr(&out).contains("E0386"), "{}", stderr(&out));
    let home = read(&dir.join("dist/index.html"));
    // the photo is 800 px wide: one variant (480), then the original in the `srcset`
    let names: Vec<String> =
        std::fs::read_dir(dir.join("dist/img")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert!(names.iter().any(|n| n.starts_with("photo-480-") && n.ends_with(".webp")), "{names:?}");
    assert!(!names.iter().any(|n| n.starts_with("photo-800-")), "{names:?}");
    assert!(
        home.contains("srcset=\"img/photo-480-")
            && home.contains(".webp 480w, img/photo-")
            && home.contains(".png 800w\" sizes=\"100vw\""),
        "{home}"
    );
}

/// A real PNG (stored deflate blocks, so the test needs no compressor): `w` x `h` pixels of
/// `px(x, y)` as RGBA.
fn real_png(w: u32, h: u32, mut px: impl FnMut(u32, u32) -> [u8; 4]) -> Vec<u8> {
    fn crc(data: &[u8]) -> u32 {
        let mut c = 0xFFFF_FFFFu32;
        for &b in data {
            c ^= b as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
        }
        !c
    }
    fn chunk(out: &mut Vec<u8>, id: &[u8], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut body = id.to_vec();
        body.extend_from_slice(data);
        out.extend_from_slice(&body);
        out.extend_from_slice(&crc(&body).to_be_bytes());
    }
    let mut raw = Vec::new();
    for y in 0..h {
        raw.push(0);
        for x in 0..w {
            raw.extend_from_slice(&px(x, y));
        }
    }
    let mut z = vec![0x78, 0x01];
    for (i, part) in raw.chunks(65535).enumerate() {
        let last = (i + 1) * 65535 >= raw.len();
        z.push(last as u8);
        z.extend_from_slice(&(part.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(part.len() as u16)).to_le_bytes());
        z.extend_from_slice(part);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &v in &raw {
        a = (a + v as u32) % 65521;
        b = (b + a) % 65521;
    }
    z.extend_from_slice(&(b << 16 | a).to_be_bytes());
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = w.to_be_bytes().to_vec();
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

#[test]
fn the_built_in_codec_makes_variants_and_previews() {
    let dir = scratch("web-codec");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    // a photo-like picture (gradients and noise), a small logo with transparency, a broken file
    let mut seed = 7u32;
    let photo = real_png(1000, 700, |x, y| {
        seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
        let n = seed >> 27;
        [(x / 4 + n) as u8, (y / 3 + n) as u8, ((x + y) / 7) as u8, 255]
    });
    std::fs::write(dir.join("assets/photo.png"), &photo).unwrap();
    std::fs::write(dir.join("assets/logo.png"), real_png(40, 20, |x, _| if x < 20 { [200, 30, 30, 255] } else { [0, 0, 0, 0] }))
        .unwrap();
    std::fs::write(dir.join("assets/broken.jpg"), b"\xFF\xD8\xFF\xE0 not really a JPEG").unwrap();
    std::fs::write(
        dir.join("site.nyra"),
        "use web\nweb.site()\nweb.page(\"/\", \"Shop\", [web.h1(\"Shop\"), web.img(\"photo.png\", \"Counter\"), \
         web.img(\"logo.png\", \"Logo\"), web.img(\"broken.jpg\", \"Broken\")])\n",
    )
    .unwrap();
    let out = nyra().current_dir(&dir).args(["build", "site.nyra"]).output().unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    // only the broken file is reported
    let err = stderr(&out);
    assert!(err.contains("warning[E0386]") && err.contains("assets/broken.jpg") && !err.contains("photo.png"), "{err}");
    let names: Vec<String> =
        std::fs::read_dir(dir.join("dist/img")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    for w in ["480", "800", "1000"] {
        let f = names.iter().find(|n| n.starts_with(&format!("photo-{w}-")) && n.ends_with(".webp"));
        let f = f.unwrap_or_else(|| panic!("no {w} px variant in {names:?}"));
        let data = std::fs::read(dir.join("dist/img").join(f)).unwrap();
        assert!(data.starts_with(b"RIFF") && &data[8..12] == b"WEBP", "{f}");
    }
    let home = read(&dir.join("dist/index.html"));
    assert!(home.contains("srcset=\"img/photo-480-") && home.contains(".webp 1000w\""), "{home}");
    // the photo gets a preview class, and its inline style is allowed by its hash
    let style = home.split("<style>").nth(1).and_then(|s| s.split("</style>").next()).expect("a <style> with the previews");
    assert!(style.starts_with(".ph-") && style.contains("url(data:image/webp;base64,"), "{style}");
    assert!(home.contains("class=\"ph-"), "{home}");
    assert!(home.contains("style-src 'self' 'sha256-") && !home.contains("unsafe-inline"), "{home}");
    // a rebuild reads the variants from the cache and writes the same files
    let again = nyra().current_dir(&dir).args(["build", "site.nyra"]).output().unwrap();
    assert!(again.status.success());
    assert_eq!(read(&dir.join("dist/index.html")), home);
}

/// Reads one HTTP response (the server closes the connection after it).
fn get(port: u16, path: &str) -> (u16, String) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    write!(s, "GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();
    let mut text = String::new();
    s.read_to_string(&mut text).unwrap();
    let status = text.split_whitespace().nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
    (status, text)
}

#[test]
fn the_dev_server_serves_the_site_and_rebuilds_on_save() {
    let dir = site("serve");
    let mut child = nyra()
        .current_dir(&dir)
        .args(["run", "site.nyra", "--port", "0", "--image-tool", "none"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(child.stderr.take().unwrap()).lines();
    let mut port = 0;
    let start = Instant::now();
    while port == 0 && start.elapsed() < Duration::from_secs(60) {
        let Some(Ok(line)) = lines.next() else { break };
        if let Some(rest) = line.strip_prefix("nyra: serving http://127.0.0.1:") {
            port = rest.split('/').next().unwrap().parse().unwrap();
        }
    }
    // (keep reading stderr, so the server never blocks on a full pipe)
    std::thread::spawn(move || for _ in lines {});
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        assert!(port > 0, "the server did not say its port");
        let (status, home) = get(port, "/");
        assert_eq!(status, 200, "{home}");
        assert!(home.contains("Content-Security-Policy: ") && home.contains("Bread every morning"), "{home}");
        assert!(home.contains("<script src=\"/__nyra/reload.js\" defer></script>"), "{home}");
        let (status, about) = get(port, "/about");
        assert!(status == 301 && about.contains("Location: /about/"), "{about}");
        assert_eq!(get(port, "/about/").0, 200);
        assert!(get(port, "/__nyra/reload.js").1.contains("/__nyra/version"));
        assert_eq!(get(port, "/nothing-here.png").0, 404);
        let version = |p: u16| get(p, "/__nyra/version").1.rsplit("\r\n\r\n").next().unwrap().to_string();
        let v1 = version(port);

        // a change of the program: a new build, a new version, the new text
        std::thread::sleep(Duration::from_millis(1100));
        std::fs::write(dir.join("site.nyra"), read(&dir.join("site.nyra")).replace("Bread every morning", "Fresh bread")).unwrap();
        let start = Instant::now();
        while version(port) == v1 && start.elapsed() < Duration::from_secs(30) {
            std::thread::sleep(Duration::from_millis(100));
        }
        assert_ne!(version(port), v1, "no rebuild after the change");
        assert!(get(port, "/").1.contains("Fresh bread"));

        // a program that does not compile: the errors in the browser, until it is fixed
        let v2 = version(port);
        std::fs::write(dir.join("site.nyra"), read(&dir.join("site.nyra")).replace("var clicks = 0", "var clicks = ")).unwrap();
        let start = Instant::now();
        while version(port) == v2 && start.elapsed() < Duration::from_secs(30) {
            std::thread::sleep(Duration::from_millis(100));
        }
        let (status, page) = get(port, "/");
        assert!(status == 500 && page.contains("The build failed") && page.contains("error["), "{page}");
    }));
    let _ = child.kill();
    let _ = child.wait();
    if let Err(e) = result {
        std::panic::resume_unwind(e);
    }
}

#[test]
fn the_mcp_build_tool_describes_a_site() {
    let code = read(Path::new("tests/web_build/site.nyra"));
    let mut quoted = String::from("\"");
    for c in code.chars() {
        match c {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            '\n' => quoted.push_str("\\n"),
            '\r' => {}
            c => quoted.push(c),
        }
    }
    quoted.push('"');
    let call = format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{{\"name\":\"nyra_build\",\"arguments\":{{\"code\":{quoted}}}}}}}"
    );
    let mut child = nyra().arg("mcp").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    writeln!(child.stdin.take().unwrap(), "{call}").unwrap();
    let out = child.wait_with_output().unwrap();
    let reply = Json::parse(stdout(&out).lines().next().unwrap()).unwrap();
    let text = reply.get("result").and_then(|r| r.get("content")).and_then(Json::as_array).unwrap()[0]
        .get("text")
        .and_then(Json::as_str)
        .unwrap()
        .to_string();
    let site = Json::parse(&text).unwrap();
    assert_eq!(site.get("ok").and_then(Json::as_bool), Some(true), "{text}");
    assert_eq!(site.get("target").and_then(Json::as_str), Some("web"));
    let files: Vec<&str> =
        site.get("files").and_then(Json::as_array).unwrap().iter().filter_map(|f| f.get("path").and_then(Json::as_str)).collect();
    assert!(files.contains(&"index.html") && files.contains(&"about/index.html") && files.contains(&"sitemap.xml"), "{files:?}");
    // the images are not there (a program given as code has no folder): a warning, not an error
    let warnings = site.get("warnings").and_then(Json::as_array).unwrap();
    assert!(warnings.iter().any(|w| w.get("code").and_then(Json::as_str) == Some("E0383")), "{text}");
}
