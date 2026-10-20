//! `nyra run site.nyra`: the dev server of a `use web` program. It builds the site in memory,
//! serves it on http://127.0.0.1:8000 (`--port N`; 0 lets the system choose), and builds again
//! when the program, a file it imports, or a file in `assets/` changes. Each page polls
//! `/__nyra/version` and reloads when a new build is there; a build that fails shows its errors
//! in the browser and in the terminal. Plain `std::net`, one thread per connection, no crates.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use super::{build, html, Cli, Error, Site, RELOAD_JS, VERSION_PATH};
use crate::{ast, diag};

/// The script every page of the dev server loads: it polls the build number and reloads.
const RELOAD: &str = "(() => {\n    let v = null;\n    const tick = () => fetch(\"/__nyra/version\", { cache: \"no-store\" })\n        .then((r) => r.text())\n        .then((t) => { if (v !== null && t !== v) location.reload(); v = t; })\n        .catch(() => {})\n        .finally(() => setTimeout(tick, 400));\n    tick();\n})();\n";

/// What the server hands out: the last good site, or the errors of the last build.
#[derive(Default)]
struct Served {
    files: BTreeMap<String, Vec<u8>>,
    csp: String,
    /// The errors of a build that failed, as text.
    error: Option<String>,
    version: u64,
}

pub fn run(c: &Cli, src: &str, prog: ast::Program) -> ExitCode {
    let port = c.port.unwrap_or(8000);
    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("nyra: cannot serve on port {port} ({e}): another server may use it; choose another with --port N");
            return ExitCode::from(2);
        }
    };
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(port);
    let state = Arc::new(Mutex::new(Served::default()));
    let opts = c.options(true, true);
    let mut watched = watch_list(&c.file, &prog, &opts.dir);
    let first = build(prog, src, &opts);
    publish(&state, first, &c.file, src);
    eprintln!("nyra: serving http://127.0.0.1:{port}/ (rebuilds when you save; Ctrl+C stops)");
    {
        let state = Arc::clone(&state);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let state = Arc::clone(&state);
                std::thread::spawn(move || handle(stream, &state));
            }
        });
    }
    let mut seen = snapshot(&watched);
    loop {
        std::thread::sleep(Duration::from_millis(250));
        let now = snapshot(&watched);
        if now == seen {
            continue;
        }
        let start = crate::clock::Instant::now();
        let Ok(text) = std::fs::read_to_string(&c.file) else { continue };
        let compile = |s: &str| crate::compile_granted(s, &c.grant, &crate::cli_flag);
        let compiled = match compile(&text) {
            Ok(p) => Ok((p, text.clone())),
            Err(diags) => match crate::fix::repair_or_report(&text, diags, compile) {
                Ok(r) => Ok((r.value, r.text)),
                Err(left) => Err(Error::Diags(left)),
            },
        };
        let result = match compiled {
            Ok((p, fixed)) => {
                watched = watch_list(&c.file, &p, &opts.dir);
                build(p, &fixed, &opts)
            }
            Err(e) => Err(e),
        };
        let ok = result.is_ok();
        publish(&state, result, &c.file, &text);
        if ok {
            eprintln!("nyra: rebuilt ({})", crate::ms(start.elapsed()));
        }
        seen = snapshot(&watched);
    }
}

/// Puts the result of a build on the server and says what went wrong, if anything.
fn publish(state: &Mutex<Served>, result: Result<Site, Error>, file: &str, src: &str) {
    let mut s = state.lock().unwrap_or_else(|e| e.into_inner());
    s.version += 1;
    match result {
        Ok(site) => {
            eprint!("{}", diag::render_warnings(file, src));
            s.files = site.files;
            s.csp = site.csp;
            s.error = None;
        }
        Err(e) => {
            let text = e.human(file, src);
            eprint!("{text}");
            s.error = Some(text);
        }
    }
}

/// The program, the files it imports, and every file below `assets/`.
fn watch_list(file: &str, prog: &ast::Program, dir: &Path) -> Vec<PathBuf> {
    let mut v = vec![PathBuf::from(file)];
    for f in prog.files.values() {
        let p = PathBuf::from(f);
        if !v.contains(&p) {
            v.push(p);
        }
    }
    fn walk(d: &Path, out: &mut Vec<PathBuf>, depth: usize) {
        if depth > 8 {
            return;
        }
        for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out, depth + 1);
            } else {
                out.push(p);
            }
        }
    }
    // (the folder itself too: a file that is added or removed changes its listing)
    let assets = dir.join("assets");
    v.push(assets.clone());
    walk(&assets, &mut v, 0);
    v
}

/// The modification time and size of each file (`None` for one that is missing).
fn snapshot(files: &[PathBuf]) -> Vec<Option<(SystemTime, u64)>> {
    let mut out: Vec<Option<(SystemTime, u64)>> =
        files.iter().map(|f| std::fs::metadata(f).ok().map(|m| (m.modified().unwrap_or(SystemTime::UNIX_EPOCH), m.len()))).collect();
    // a file added below `assets/`
    if let Some(dir) = files.iter().find(|f| f.ends_with("assets")) {
        let mut n = 0u64;
        count(dir, &mut n, 0);
        out.push(Some((SystemTime::UNIX_EPOCH, n)));
    }
    out
}

fn count(d: &Path, n: &mut u64, depth: usize) {
    if depth > 8 {
        return;
    }
    for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
        *n += 1;
        if e.path().is_dir() {
            count(&e.path(), n, depth + 1);
        }
    }
}

/// One request: `GET` or `HEAD` of a file of the site, the reload script or the build number.
fn handle(mut s: TcpStream, state: &Mutex<Served>) {
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    let mut buf = Vec::new();
    let mut chunk = [0u8; 2048];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") && buf.len() < 16 * 1024 {
        match s.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    let head = String::from_utf8_lossy(&buf);
    let mut first = head.lines().next().unwrap_or("").split_whitespace();
    let (method, target) = (first.next().unwrap_or(""), first.next().unwrap_or("/"));
    if method != "GET" && method != "HEAD" {
        let _ = respond(&mut s, "405 Method Not Allowed", "text/plain; charset=utf-8", b"only GET and HEAD\n", None, true);
        return;
    }
    let path = decode(target.split(['?', '#']).next().unwrap_or("/"));
    let head_only = method == "HEAD";
    if path == VERSION_PATH {
        let v = state.lock().map(|s| s.version).unwrap_or(0).to_string();
        let _ = respond(&mut s, "200 OK", "text/plain; charset=utf-8", v.as_bytes(), None, head_only);
        return;
    }
    if path == RELOAD_JS {
        let _ = respond(&mut s, "200 OK", "text/javascript; charset=utf-8", RELOAD.as_bytes(), None, head_only);
        return;
    }
    let st = state.lock().unwrap_or_else(|e| e.into_inner());
    let rel = path.trim_start_matches('/').to_string();
    let page_like =
        rel.is_empty() || rel.ends_with('/') || rel.ends_with(".html") || !rel.rsplit('/').next().unwrap_or("").contains('.');
    if let Some(err) = st.error.as_ref().filter(|_| page_like) {
        let body = format!(
            "<!doctype html>\n<meta charset=\"utf-8\">\n<title>Build failed</title>\n<script src=\"{RELOAD_JS}\" defer></script>\n<h1>The build failed</h1>\n<pre>{}</pre>\n",
            html::text(err)
        );
        let _ = respond(&mut s, "500 Internal Server Error", "text/html; charset=utf-8", body.as_bytes(), None, head_only);
        return;
    }
    let file = if rel.is_empty() || rel.ends_with('/') { format!("{rel}index.html") } else { rel.clone() };
    if let Some(bytes) = st.files.get(&file) {
        let _ = respond(&mut s, "200 OK", content_type(&file), bytes, Some(&st.csp), head_only);
        return;
    }
    // `/about` is the folder `about/`: the browser must ask for it so (relative links)
    if st.files.contains_key(&format!("{rel}/index.html")) {
        let location = format!("/{rel}/");
        let msg = format!("HTTP/1.1 301 Moved Permanently\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        let _ = s.write_all(msg.as_bytes());
        return;
    }
    let body =
        st.files.get("404.html").cloned().unwrap_or_else(|| b"<!doctype html><title>Not found</title><h1>Not found</h1>\n".to_vec());
    let _ = respond(&mut s, "404 Not Found", "text/html; charset=utf-8", &body, Some(&st.csp), head_only);
}

fn respond(s: &mut TcpStream, status: &str, ty: &str, body: &[u8], csp: Option<&str>, head_only: bool) -> std::io::Result<()> {
    let mut h = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {ty}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n",
        body.len()
    );
    if let Some(p) = csp.filter(|p| !p.is_empty() && ty.starts_with("text/html")) {
        h.push_str(&format!("Content-Security-Policy: {p}\r\n"));
    }
    h.push_str("\r\n");
    s.write_all(h.as_bytes())?;
    if !head_only {
        s.write_all(body)?;
    }
    s.flush()
}

/// `%20` and the like.
fn decode(p: &str) -> String {
    let b = p.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hexd = |c: u8| (c as char).to_digit(16);
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hexd(b[i + 1]), hexd(b[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    let s = String::from_utf8_lossy(&out).into_owned();
    // never above the site's root
    s.split('/').filter(|part| *part != ".." && *part != ".").collect::<Vec<_>>().join("/")
}

pub fn content_type(file: &str) -> &'static str {
    match file.rsplit('.').next().unwrap_or("").to_ascii_lowercase().as_str() {
        "html" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" => "application/json",
        "xml" => "application/xml",
        "txt" => "text/plain; charset=utf-8",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "pdf" => "application/pdf",
        _ if !file.contains('.') => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_paths_and_knows_types() {
        assert_eq!(decode("/a%20b/../c"), "/a b/c");
        assert_eq!(decode("/x%"), "/x%");
        assert_eq!(content_type("a/b.CSS"), "text/css; charset=utf-8");
        assert_eq!(content_type("_headers"), "text/plain; charset=utf-8");
    }
}
