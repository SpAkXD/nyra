//! `nyra lsp`: drives scripted sessions over stdio (Content-Length framed JSON-RPC) and checks the
//! replies: diagnostics with codes and ranges, hover, completion, signature help, go to
//! definition, the outline, formatting, quick fixes, unsaved buffers of several files, and the
//! time a large file takes.

mod common;

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use common::{nyra, Json};

/// How long a reply may take before the test fails (a debug build on a busy machine is slow).
const WAIT: Duration = Duration::from_secs(60);

struct Lsp {
    child: Child,
    stdin: Option<ChildStdin>,
    rx: Receiver<Json>,
    /// notifications that came while a reply was awaited
    pending: VecDeque<Json>,
    next: u64,
}

fn esc(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out += "\\\"",
            '\\' => out += "\\\\",
            '\n' => out += "\\n",
            '\r' => out += "\\r",
            '\t' => out += "\\t",
            c => out.push(c),
        }
    }
    out + "\""
}

fn uri(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    let s = if s.starts_with('/') { s } else { format!("/{s}") };
    format!("file://{}", s.replace(' ', "%20"))
}

fn pos(line: u64, character: u64) -> String {
    format!(r#""position":{{"line":{line},"character":{character}}}"#)
}

impl Lsp {
    fn start() -> Lsp {
        let mut child = nyra().arg("lsp").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::inherit()).spawn().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut r = BufReader::new(stdout);
            loop {
                let mut len = None;
                loop {
                    let mut line = String::new();
                    if r.read_line(&mut line).unwrap_or(0) == 0 {
                        return;
                    }
                    let line = line.trim_end();
                    if line.is_empty() {
                        break;
                    }
                    let (k, v) = line.split_once(':').expect("a header line");
                    assert_eq!(k, "Content-Length", "unexpected header {line:?}");
                    len = Some(v.trim().parse::<usize>().unwrap());
                }
                let mut body = vec![0; len.expect("Content-Length")];
                r.read_exact(&mut body).unwrap();
                let text = String::from_utf8(body).unwrap();
                let json = Json::parse(&text).unwrap_or_else(|e| panic!("bad JSON from the server ({e}): {text}"));
                if tx.send(json).is_err() {
                    return;
                }
            }
        });
        let stdin = child.stdin.take();
        Lsp { child, stdin, rx, pending: VecDeque::new(), next: 1 }
    }

    fn send(&mut self, body: &str) {
        let stdin = self.stdin.as_mut().unwrap();
        write!(stdin, "Content-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        stdin.flush().unwrap();
    }

    fn notify(&mut self, method: &str, params: &str) {
        self.send(&format!(r#"{{"jsonrpc":"2.0","method":"{method}","params":{params}}}"#));
    }

    /// The whole reply to a request.
    fn reply(&mut self, method: &str, params: &str) -> Json {
        let id = self.next;
        self.next += 1;
        self.send(&format!(r#"{{"jsonrpc":"2.0","id":{id},"method":"{method}","params":{params}}}"#));
        let until = Instant::now() + WAIT;
        loop {
            let msg = self
                .rx
                .recv_timeout(until.saturating_duration_since(Instant::now()))
                .unwrap_or_else(|_| panic!("no reply to {method}"));
            if msg.get("id").and_then(Json::as_u64) == Some(id) {
                return msg;
            }
            self.pending.push_back(msg);
        }
    }

    /// The result of a request (which must not fail).
    fn request(&mut self, method: &str, params: &str) -> Json {
        let r = self.reply(method, params);
        r.get("result").cloned().unwrap_or_else(|| panic!("{method} failed: {r:?}"))
    }

    /// The next diagnostics published for `uri`.
    fn diagnostics(&mut self, uri: &str) -> Vec<Json> {
        let is_ours = |m: &Json| {
            m.get("method").and_then(Json::as_str) == Some("textDocument/publishDiagnostics")
                && m.get("params").and_then(|p| p.get("uri")).and_then(Json::as_str) == Some(uri)
        };
        if let Some(i) = self.pending.iter().position(is_ours) {
            let m = self.pending.remove(i).unwrap();
            return m.get("params").and_then(|p| p.get("diagnostics")).and_then(Json::as_array).unwrap().to_vec();
        }
        let until = Instant::now() + WAIT;
        loop {
            let msg = self
                .rx
                .recv_timeout(until.saturating_duration_since(Instant::now()))
                .unwrap_or_else(|_| panic!("no diagnostics for {uri}"));
            if is_ours(&msg) {
                return msg.get("params").and_then(|p| p.get("diagnostics")).and_then(Json::as_array).unwrap().to_vec();
            }
            self.pending.push_back(msg);
        }
    }

    /// The diagnostics published last for `uri` (waiting for some if none came yet).
    fn last_diagnostics(&mut self, uri: &str) -> Vec<Json> {
        while let Ok(m) = self.rx.try_recv() {
            self.pending.push_back(m);
        }
        let mut last = None;
        while let Some(i) =
            self.pending.iter().position(|m| m.get("params").and_then(|p| p.get("uri")).and_then(Json::as_str) == Some(uri))
        {
            last = self.pending.remove(i);
        }
        match last {
            Some(m) => m.get("params").and_then(|p| p.get("diagnostics")).and_then(Json::as_array).unwrap().to_vec(),
            None => self.diagnostics(uri),
        }
    }

    fn initialize(&mut self) -> Json {
        let r = self.request("initialize", r#"{"processId":null,"rootUri":null,"capabilities":{}}"#);
        self.notify("initialized", "{}");
        r
    }

    fn open(&mut self, uri: &str, text: &str) {
        self.notify(
            "textDocument/didOpen",
            &format!(r#"{{"textDocument":{{"uri":"{uri}","languageId":"nyra","version":1,"text":{}}}}}"#, esc(text)),
        );
    }

    fn change(&mut self, uri: &str, version: u64, text: &str) {
        self.notify(
            "textDocument/didChange",
            &format!(r#"{{"textDocument":{{"uri":"{uri}","version":{version}}},"contentChanges":[{{"text":{}}}]}}"#, esc(text)),
        );
    }

    fn at(&mut self, method: &str, uri: &str, line: u64, character: u64) -> Json {
        self.request(method, &format!(r#"{{"textDocument":{{"uri":"{uri}"}},{}}}"#, pos(line, character)))
    }

    /// shutdown, exit, and the server must end with success
    fn stop(mut self) {
        assert_eq!(self.request("shutdown", "null"), Json::Null);
        self.notify("exit", "null");
        drop(self.stdin.take());
        let status = self.child.wait().unwrap();
        assert!(status.success(), "nyra lsp ended with {status}");
    }
}

fn hover_text(r: &Json) -> String {
    r.get("contents").and_then(|c| c.get("value")).and_then(Json::as_str).unwrap_or("").to_string()
}

fn labels(r: &Json) -> Vec<String> {
    r.get("items")
        .and_then(Json::as_array)
        .unwrap()
        .iter()
        .map(|i| i.get("label").and_then(Json::as_str).unwrap().to_string())
        .collect()
}

fn num(j: &Json, path: &[&str]) -> u64 {
    let mut cur = j;
    for p in path {
        cur = cur.get(p).unwrap_or_else(|| panic!("no `{p}` in {j:?}"));
    }
    cur.as_u64().unwrap()
}

const SHAPES: &str = "// doubles a number\npub fn double(x: int) -> int = x * 2\n";

const MAIN: &str = "use ./shapes
use math

struct Point { x: int, y: int }

// the area of a box
fn area(w: int, h: int) -> int = w * h

let p = Point(x: 1, y: 2)
let total = area(p.x, 3) + shapes.double(2)
let names = [\"a\", \"b\"]
Print(totl, math.sqrt(2.0), names.len())
";

#[test]
fn a_whole_session() {
    let dir = common::scratch("lsp-session");
    std::fs::write(dir.join("shapes.nyra"), SHAPES).unwrap();
    // the buffer is not saved: the file on disk says something else
    std::fs::write(dir.join("main.nyra"), "print(1)\n").unwrap();
    let main = uri(&dir.join("main.nyra"));
    let mut lsp = Lsp::start();

    let init = lsp.initialize();
    let caps = init.get("capabilities").unwrap();
    for c in ["hoverProvider", "definitionProvider", "documentSymbolProvider", "documentFormattingProvider"] {
        assert_eq!(caps.get(c), Some(&Json::Bool(true)), "{c}");
    }
    assert!(caps.get("completionProvider").is_some() && caps.get("signatureHelpProvider").is_some());

    // diagnostics: errors with their codes, ranges, hints and a link to their documentation
    lsp.open(&main, MAIN);
    let diags = lsp.diagnostics(&main);
    assert_eq!(diags.len(), 2, "{diags:?}");
    let by_code =
        |code: &str| diags.iter().find(|d| d.get("code").and_then(Json::as_str) == Some(code)).unwrap_or_else(|| panic!("no {code}"));
    let undefined = by_code("E0201");
    assert_eq!((num(undefined, &["range", "start", "line"]), num(undefined, &["range", "start", "character"])), (11, 6));
    assert_eq!(num(undefined, &["range", "end", "character"]), 10);
    assert_eq!(num(undefined, &["severity"]), 1);
    let msg = undefined.get("message").and_then(Json::as_str).unwrap();
    assert!(msg.contains("undefined variable `totl`") && msg.contains("did you mean `total`?"), "{msg}");
    let href = undefined.get("codeDescription").and_then(|c| c.get("href")).and_then(Json::as_str).unwrap();
    assert!(href.starts_with("https://github.com/SpAkXD/nyra/blob/main/docs/ERRORS.md#e0201-"), "{href}");
    let print = by_code("E0202");
    assert_eq!((num(print, &["range", "start", "character"]), num(print, &["range", "end", "character"])), (0, 5));
    assert_eq!(undefined.get("source").and_then(Json::as_str), Some("nyra"));

    // hover: a function with its comment, a variable's type, a field, a standard and a module function
    let h = hover_text(&lsp.at("textDocument/hover", &main, 9, 13));
    assert!(h.contains("fn area(w: int, h: int) -> int") && h.contains("the area of a box"), "{h}");
    let h = hover_text(&lsp.at("textDocument/hover", &main, 9, 17));
    assert!(h.contains("let p: Point"), "{h}");
    let h = hover_text(&lsp.at("textDocument/hover", &main, 9, 19));
    assert!(h.contains("Point.x: int"), "{h}");
    let h = hover_text(&lsp.at("textDocument/hover", &main, 11, 18));
    assert!(h.contains("fn math.sqrt(x: float) -> float"), "{h}");
    let h = hover_text(&lsp.at("textDocument/hover", &main, 9, 35));
    assert!(h.contains("fn shapes.double(x: int) -> int") && h.contains("doubles a number"), "{h}");
    let h = hover_text(&lsp.at("textDocument/hover", &main, 11, 35));
    assert!(h.contains("fn [str].len() -> int"), "{h}");
    // on an error: what its code means (`nyra explain`)
    let h = hover_text(&lsp.at("textDocument/hover", &main, 11, 2));
    assert!(h.contains("E0202") && h.contains("nyra explain E0202"), "{h}");

    // go to definition: a function, a variable, a module's function, the module of a `use` line
    let d = lsp.at("textDocument/definition", &main, 9, 13);
    assert_eq!(d.get("uri").and_then(Json::as_str), Some(main.as_str()));
    assert_eq!((num(&d, &["range", "start", "line"]), num(&d, &["range", "start", "character"])), (6, 3));
    let d = lsp.at("textDocument/definition", &main, 9, 17);
    assert_eq!((num(&d, &["range", "start", "line"]), num(&d, &["range", "start", "character"])), (8, 4));
    let d = lsp.at("textDocument/definition", &main, 9, 35);
    assert!(d.get("uri").and_then(Json::as_str).unwrap().ends_with("/shapes.nyra"), "{d:?}");
    assert_eq!((num(&d, &["range", "start", "line"]), num(&d, &["range", "start", "character"])), (1, 7));
    let d = lsp.at("textDocument/definition", &main, 0, 8);
    assert!(d.get("uri").and_then(Json::as_str).unwrap().ends_with("/shapes.nyra"), "{d:?}");
    let d = lsp.at("textDocument/definition", &main, 9, 19);
    assert_eq!((num(&d, &["range", "start", "line"]), num(&d, &["range", "start", "character"])), (3, 15));

    // the quick fix of an error with a certain fix (`--fix`)
    let actions = lsp.request(
        "textDocument/codeAction",
        &format!(r#"{{"textDocument":{{"uri":"{main}"}},"range":{{"start":{{"line":11,"character":0}},"end":{{"line":11,"character":0}}}},"context":{{"diagnostics":[]}}}}"#),
    );
    let quick =
        actions.as_array().unwrap().iter().find(|a| a.get("kind").and_then(Json::as_str) == Some("quickfix")).expect("a quick fix");
    assert!(quick.get("title").and_then(Json::as_str).unwrap().starts_with("Fix E0202"), "{quick:?}");
    let edits = quick.get("edit").and_then(|e| e.get("changes")).and_then(|c| c.get(&main)).and_then(Json::as_array).unwrap();
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].get("newText").and_then(Json::as_str), Some("print"));
    assert_eq!((num(&edits[0], &["range", "start", "character"]), num(&edits[0], &["range", "end", "character"])), (0, 5));

    // the outline
    let syms = lsp.request("textDocument/documentSymbol", &format!(r#"{{"textDocument":{{"uri":"{main}"}}}}"#));
    let names: Vec<&str> = syms.as_array().unwrap().iter().map(|s| s.get("name").and_then(Json::as_str).unwrap()).collect();
    assert_eq!(names, ["Point", "area", "p", "total", "names"]);
    let point = &syms.as_array().unwrap()[0];
    assert_eq!(point.get("children").and_then(Json::as_array).unwrap().len(), 2);

    // completion: after `p.` (a line that does not parse yet), after `names.`, after `math.`, and names in scope
    let typing = MAIN.replace("Print(totl, math.sqrt(2.0), names.len())", "print(total, p.)");
    lsp.change(&main, 2, &typing);
    let items = labels(&lsp.at("textDocument/completion", &main, 11, 15));
    assert!(items.contains(&"x".to_string()) && items.contains(&"y".to_string()), "{items:?}");
    let typing = MAIN.replace("Print(totl, math.sqrt(2.0), names.len())", "print(names.)");
    lsp.change(&main, 3, &typing);
    let items = labels(&lsp.at("textDocument/completion", &main, 11, 12));
    for m in ["len", "push", "join", "map"] {
        assert!(items.contains(&m.to_string()), "{m} in {items:?}");
    }
    let typing = MAIN.replace("Print(totl, math.sqrt(2.0), names.len())", "print(math.)");
    lsp.change(&main, 4, &typing);
    let items = labels(&lsp.at("textDocument/completion", &main, 11, 11));
    for m in ["sqrt", "pi", "atan2"] {
        assert!(items.contains(&m.to_string()), "{m} in {items:?}");
    }
    let typing = MAIN.replace("Print(totl, math.sqrt(2.0), names.len())", "print(shapes.)");
    lsp.change(&main, 5, &typing);
    let items = labels(&lsp.at("textDocument/completion", &main, 11, 13));
    assert_eq!(items, ["double"]);
    let typing = MAIN.replace("Print(totl, math.sqrt(2.0), names.len())", "print(to)");
    lsp.change(&main, 6, &typing);
    let items = labels(&lsp.at("textDocument/completion", &main, 11, 8));
    for m in ["total", "p", "names", "area", "Point", "print", "while", "math", "shapes"] {
        assert!(items.contains(&m.to_string()), "{m} in {items:?}");
    }

    // signature help inside a call, on the second argument
    lsp.change(&main, 7, MAIN);
    let s = lsp.at("textDocument/signatureHelp", &main, 9, 22);
    let sig = &s.get("signatures").and_then(Json::as_array).unwrap()[0];
    assert_eq!(sig.get("label").and_then(Json::as_str), Some("area(w: int, h: int) -> int"));
    assert_eq!(num(&s, &["activeParameter"]), 1);

    // formatting: `nyra fmt` (indentation, `return`)
    let messy = "fn f(x: int) -> int {\nret x\n}\nprint(f(2))\n";
    lsp.change(&main, 8, messy);
    let edits = lsp.request(
        "textDocument/formatting",
        &format!(r#"{{"textDocument":{{"uri":"{main}"}},"options":{{"tabSize":4,"insertSpaces":true}}}}"#),
    );
    let edits = edits.as_array().unwrap();
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].get("newText").and_then(Json::as_str), Some("fn f(x: int) -> int {\n    return x\n}\nprint(f(2))\n"));
    // the buffer was checked for the request: its diagnostics are current
    assert_eq!(lsp.last_diagnostics(&main), Vec::<Json>::new());

    lsp.stop();
}

#[test]
fn unsaved_files_import_each_other_and_web_functions_complete() {
    let dir = common::scratch("lsp-unsaved");
    let _ = std::fs::remove_file(dir.join("util.nyra"));
    let util = uri(&dir.join("util.nyra"));
    let app = uri(&dir.join("app.nyra"));
    let mut lsp = Lsp::start();
    lsp.initialize();
    // neither file exists on disk
    lsp.open(&util, "pub fn triple(x: int) -> int = x * 3\n");
    assert!(lsp.diagnostics(&util).is_empty());
    lsp.open(&app, "use ./util\nprint(util.triple(2))\n");
    assert_eq!(lsp.diagnostics(&app), Vec::<Json>::new());
    // a change of the imported buffer checks the importer again
    lsp.change(&util, 2, "pub fn triple(x: str) -> str = x\n");
    let diags = lsp.diagnostics(&app);
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!(diags[0].get("code").and_then(Json::as_str), Some("E0203"), "{diags:?}");

    // `use web`: the library's functions, and their options inside the parentheses
    let site = uri(&dir.join("site.nyra"));
    lsp.open(&site, "use web\n\npage(\"/\", \"Home\") {\n    button(\"Go\", )\n}\n");
    lsp.diagnostics(&site);
    let items = labels(&lsp.at("textDocument/completion", &site, 1, 0));
    assert!(items.contains(&"page".to_string()) && items.contains(&"hero".to_string()), "{items:?}");
    let items = labels(&lsp.at("textDocument/completion", &site, 3, 17));
    assert!(items.contains(&"style:".to_string()) && items.contains(&"href:".to_string()), "{items:?}");
    let h = hover_text(&lsp.at("textDocument/hover", &site, 2, 1));
    assert!(h.contains("fn page(") && h.contains("title: str"), "{h}");
    lsp.stop();
}

#[test]
fn a_large_file_stays_fast() {
    // 2,000 lines: 200 functions of ten lines
    let mut src = String::new();
    for i in 0..200 {
        src += &format!(
            "fn f{i}(a: int, b: int) -> int {{\n    var s = a\n    for k in 0..b {{\n        s += k * {i}\n    }}\n    if s > 100 {{\n        s = s % 100\n    }}\n    return s\n}}\n"
        );
    }
    src += "print(f0(1, 2) + f199(3, 4))\n";
    assert!(src.lines().count() > 2000);
    let dir = common::scratch("lsp-large");
    let big = uri(&dir.join("big.nyra"));
    let mut lsp = Lsp::start();
    lsp.initialize();

    let t = Instant::now();
    lsp.open(&big, &src);
    assert!(lsp.diagnostics(&big).is_empty());
    let open = t.elapsed();

    // a change: the diagnostics come after the pause (150 ms) and the check
    let broken = src.replace("print(f0(1, 2)", "print(f0(1, \"2\")");
    let t = Instant::now();
    lsp.change(&big, 2, &broken);
    let diags = lsp.diagnostics(&big);
    let change = t.elapsed();
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!(num(&diags[0], &["range", "start", "line"]), 2000);

    let t = Instant::now();
    let h = hover_text(&lsp.at("textDocument/hover", &big, 1003, 8));
    let hover = t.elapsed();
    assert!(h.contains("var s: int"), "{h}");

    let t = Instant::now();
    let items = labels(&lsp.at("textDocument/completion", &big, 1003, 9));
    let complete = t.elapsed();
    assert!(items.contains(&"f150".to_string()), "{} items", items.len());

    eprintln!("2,001 lines: open {open:?}, change to diagnostics {change:?}, hover {hover:?}, completion {complete:?}");
    // generous bounds: a debug build on a slow machine; a release build takes a few milliseconds
    let limit = if cfg!(debug_assertions) { Duration::from_secs(20) } else { Duration::from_secs(2) };
    for (what, d) in [("open", open), ("change", change), ("hover", hover), ("completion", complete)] {
        assert!(d < limit, "{what} took {d:?}");
    }
    lsp.stop();
}

#[test]
fn bad_messages_get_errors_and_the_server_keeps_running() {
    let mut lsp = Lsp::start();
    // before `initialize`
    let r = lsp.reply("textDocument/hover", r#"{"textDocument":{"uri":"file:///x.nyra"},"position":{"line":0,"character":0}}"#);
    assert_eq!(r.get("error").and_then(|e| e.get("code")), Some(&Json::Num(-32002.0)));
    lsp.initialize();
    let r = lsp.reply("no/such/method", "{}");
    assert_eq!(r.get("error").and_then(|e| e.get("code")), Some(&Json::Num(-32601.0)));
    let r = lsp.reply("textDocument/hover", r#"{"textDocument":{"uri":"file:///not/open.nyra"},"position":{"line":0,"character":0}}"#);
    assert_eq!(r.get("error").and_then(|e| e.get("code")), Some(&Json::Num(-32602.0)));
    lsp.send("{not json");
    let until = Instant::now() + WAIT;
    let parse_error = loop {
        let m = lsp.rx.recv_timeout(until.saturating_duration_since(Instant::now())).expect("a parse error reply");
        if m.get("error").is_some() {
            break m;
        }
    };
    assert_eq!(parse_error.get("error").and_then(|e| e.get("code")), Some(&Json::Num(-32700.0)));
    // an untitled buffer has no folder: it is checked all the same
    lsp.open("untitled:Untitled-1", "let x = 1\nprint(x + \"a\")\n");
    let d = lsp.diagnostics("untitled:Untitled-1");
    assert_eq!(d.len(), 1, "{d:?}");
    lsp.stop();
}
