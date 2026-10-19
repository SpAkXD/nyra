//! `nyra lsp`: a language server for editors (Language Server Protocol 3.17 over standard input
//! and output). VS Code, Neovim, Helix, Zed and others then show the compiler's errors and
//! warnings while you type, and offer hover, completion, signature help, go to definition, the
//! outline, formatting (`nyra fmt`) and the certain fixes (`nyra check --fix`) as quick fixes.
//!
//! Everything works on the editor's buffers, saved or not: `use ./name` reads an open buffer
//! before the file on disk (`modules::set_overlay`). A buffer is checked again a short moment
//! after the last change (`DEBOUNCE`), and at once when a request needs it.
//!
//! One thread reads messages; the main thread (with the compiler's big stack, see
//! `crate::STACK`) does all the work, so the compiler's per-thread state is never shared. A
//! panic while handling a message becomes an error reply: the server keeps running.

mod analysis;
mod features;
mod rpc;
mod scope;
mod sigs;
mod text;

use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use crate::json::{obj, Json};
use crate::modules;

use analysis::Analysis;
use text::Enc;

/// How long after the last change a buffer is checked again.
const DEBOUNCE: Duration = Duration::from_millis(150);

const USAGE: &str = "\
usage: nyra lsp

Serves editors over the Language Server Protocol (version 3.17) on standard input and output:
errors and warnings as you type, hover, completion, signature help, go to definition, the
outline, formatting and quick fixes. Editors start it themselves; see docs/EDITORS.md.
";

/// An open buffer.
struct Doc {
    /// The URI exactly as the client sent it.
    uri: String,
    path: Option<PathBuf>,
    text: String,
    version: i64,
    /// The last check, and the text it checked.
    analysis: Option<(Analysis, String)>,
    /// `analysis` is of the current text.
    fresh: bool,
    /// The last check that got through the parser, and its text, when `analysis` did not: what
    /// completion and hover fall back on while the line being typed does not parse yet.
    good: Option<(Analysis, String)>,
    /// When the buffer is due to be checked again.
    due: Option<Instant>,
}

struct Server {
    docs: HashMap<String, Doc>,
    enc: Enc,
    initialized: bool,
    shutdown: bool,
    out: std::io::Stdout,
}

pub fn run(args: Vec<String>) -> ExitCode {
    if !args.is_empty() {
        let help = args.iter().any(|a| a == "-h" || a == "--help");
        // editors may pass `--stdio`; it is the only transport there is
        if help || args.iter().any(|a| a != "--stdio") {
            eprint!("{USAGE}");
            return if help { ExitCode::SUCCESS } else { ExitCode::from(2) };
        }
    }
    let (tx, rx) = mpsc::channel::<String>();
    std::thread::Builder::new()
        .name("nyra-lsp-read".into())
        .spawn(move || {
            let stdin = std::io::stdin();
            let mut input = stdin.lock();
            loop {
                match rpc::read(&mut input) {
                    Ok(Some(msg)) => {
                        if tx.send(msg).is_err() {
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(e) => {
                        eprintln!("nyra lsp: cannot read a message: {e}");
                        break;
                    }
                }
            }
        })
        .expect("cannot start the reader thread");
    let mut server = Server { docs: HashMap::new(), enc: Enc::Utf16, initialized: false, shutdown: false, out: std::io::stdout() };
    loop {
        let next = server.docs.values().filter_map(|d| d.due).min();
        let msg = match next {
            Some(t) => rx.recv_timeout(t.saturating_duration_since(Instant::now())),
            None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        match msg {
            Ok(text) => {
                if let Some(code) = server.message(&text) {
                    return code;
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            // the client went away without `exit`
            Err(RecvTimeoutError::Disconnected) => return ExitCode::from(if server.shutdown { 0 } else { 1 }),
        }
        server.check_due();
    }
}

impl Server {
    fn send(&mut self, msg: &Json) {
        let mut out = self.out.lock();
        if rpc::write(&mut out, msg).is_err() {
            // nobody listens any more
            std::process::exit(1);
        }
    }

    /// One message from the client. `Some` when the server is to exit.
    fn message(&mut self, text: &str) -> Option<ExitCode> {
        let msg = match Json::parse(text) {
            Ok(m) => m,
            Err(e) => {
                self.send(&rpc::error(Json::Null, rpc::PARSE_ERROR, format!("Parse error: {e}")));
                return None;
            }
        };
        let method = msg.get("method").and_then(Json::as_str).map(str::to_string);
        let id = msg.get("id").cloned();
        let params = msg.get("params").cloned().unwrap_or(Json::Null);
        let Some(method) = method else {
            // a response to a request of ours (we send none)
            return None;
        };
        match id {
            Some(id) => {
                let reply = if !self.initialized && method != "initialize" {
                    rpc::error(id, rpc::SERVER_NOT_INITIALIZED, "the server is not initialized yet")
                } else if self.shutdown {
                    rpc::error(id, rpc::INVALID_REQUEST, "the server is shutting down")
                } else {
                    match catch_unwind(AssertUnwindSafe(|| self.request(&method, &params))) {
                        Ok(Ok(result)) => rpc::reply(id, result),
                        Ok(Err((code, msg))) => rpc::error(id, code, msg),
                        Err(panic) => {
                            let what = panic_text(&panic);
                            eprintln!("nyra lsp: internal error in {method}: {what}");
                            rpc::error(id, rpc::INTERNAL_ERROR, format!("internal error in nyra ({what}); please report it"))
                        }
                    }
                };
                self.send(&reply);
            }
            None => {
                if method == "exit" {
                    return Some(ExitCode::from(if self.shutdown { 0 } else { 1 }));
                }
                if self.initialized {
                    if let Err(panic) = catch_unwind(AssertUnwindSafe(|| self.notification(&method, &params))) {
                        eprintln!("nyra lsp: internal error in {method}: {}", panic_text(&panic));
                    }
                }
            }
        }
        None
    }

    fn request(&mut self, method: &str, params: &Json) -> Result<Json, (i64, String)> {
        match method {
            "initialize" => Ok(self.initialize(params)),
            "shutdown" => {
                self.shutdown = true;
                Ok(Json::Null)
            }
            "textDocument/hover" => self.with_doc(params, features::hover),
            "textDocument/completion" => self.with_doc(params, features::completion),
            "textDocument/signatureHelp" => self.with_doc(params, features::signature_help),
            "textDocument/definition" => self.with_doc(params, features::definition),
            "textDocument/documentSymbol" => self.with_doc(params, features::symbols),
            "textDocument/formatting" => self.with_doc(params, features::formatting),
            "textDocument/codeAction" => self.with_doc(params, features::code_actions),
            _ => Err((rpc::METHOD_NOT_FOUND, format!("method not found: {method}"))),
        }
    }

    fn initialize(&mut self, params: &Json) -> Json {
        self.initialized = true;
        let offered = params
            .get("capabilities")
            .and_then(|c| c.get("general"))
            .and_then(|g| g.get("positionEncodings"))
            .and_then(Json::as_array)
            .unwrap_or(&[]);
        let utf32 = offered.iter().any(|e| e.as_str() == Some("utf-32"));
        self.enc = if utf32 { Enc::Utf32 } else { Enc::Utf16 };
        let strs = |v: &[&str]| Json::Arr(v.iter().map(|s| Json::from(*s)).collect());
        obj([
            (
                "capabilities",
                obj([
                    ("positionEncoding", if utf32 { "utf-32" } else { "utf-16" }.into()),
                    (
                        "textDocumentSync",
                        obj([("openClose", true.into()), ("change", 1i64.into()), ("save", obj([("includeText", false.into())]))]),
                    ),
                    ("hoverProvider", true.into()),
                    ("completionProvider", obj([("triggerCharacters", strs(&["."])), ("resolveProvider", false.into())])),
                    ("signatureHelpProvider", obj([("triggerCharacters", strs(&["(", ","])), ("retriggerCharacters", strs(&[","]))])),
                    ("definitionProvider", true.into()),
                    ("documentSymbolProvider", true.into()),
                    ("documentFormattingProvider", true.into()),
                    ("codeActionProvider", obj([("codeActionKinds", strs(&["quickfix", "source.fixAll"]))])),
                ]),
            ),
            ("serverInfo", obj([("name", "nyra".into()), ("version", env!("CARGO_PKG_VERSION").into())])),
        ])
    }

    fn notification(&mut self, method: &str, params: &Json) {
        let td = params.get("textDocument");
        let uri = td.and_then(|t| t.get("uri")).and_then(Json::as_str).unwrap_or("").to_string();
        let version = td.and_then(|t| t.get("version")).and_then(Json::as_f64).unwrap_or(0.0) as i64;
        match method {
            "textDocument/didOpen" => {
                let text = td.and_then(|t| t.get("text")).and_then(Json::as_str).unwrap_or("").to_string();
                let path = uri_to_path(&uri);
                if let Some(p) = &path {
                    modules::set_overlay(p, Some(text.clone()));
                }
                self.docs.insert(
                    uri.clone(),
                    Doc { uri: uri.clone(), path, text, version, analysis: None, fresh: false, good: None, due: None },
                );
                self.touch_importers(&uri);
                self.check_now(&uri);
            }
            "textDocument/didChange" => {
                let Some(doc) = self.docs.get_mut(&uri) else { return };
                for change in params.get("contentChanges").and_then(Json::as_array).unwrap_or(&[]) {
                    let new = change.get("text").and_then(Json::as_str).unwrap_or("");
                    match change.get("range") {
                        // a client that sends parts anyway
                        Some(r) => {
                            let lines = text::Lines::new(&doc.text);
                            let at = |p: Option<&Json>| {
                                let line = p.and_then(|p| p.get("line")).and_then(Json::as_f64).unwrap_or(0.0) as usize;
                                let col = p.and_then(|p| p.get("character")).and_then(Json::as_f64).unwrap_or(0.0) as usize;
                                lines.offset(lines.span(line, col, self.enc))
                            };
                            let (a, b) = (at(r.get("start")), at(r.get("end")));
                            let (a, b) = (a.min(b), a.max(b));
                            doc.text.replace_range(a..b, new);
                        }
                        None => doc.text = new.to_string(),
                    }
                }
                doc.version = version;
                doc.fresh = false;
                doc.due = Some(Instant::now() + DEBOUNCE);
                if let Some(p) = &doc.path {
                    modules::set_overlay(p, Some(doc.text.clone()));
                }
                self.touch_importers(&uri);
            }
            "textDocument/didClose" => {
                if let Some(doc) = self.docs.remove(&uri) {
                    if let Some(p) = &doc.path {
                        modules::set_overlay(p, None);
                    }
                    let clear = obj([("uri", uri.clone().into()), ("diagnostics", Json::Arr(Vec::new()))]);
                    self.send(&rpc::notification("textDocument/publishDiagnostics", clear));
                    self.touch_importers(&uri);
                }
            }
            "textDocument/didSave" => self.touch_importers(&uri),
            _ => {}
        }
    }

    /// The other buffers that import files are checked again soon: one of their files changed.
    fn touch_importers(&mut self, changed: &str) {
        let due = Instant::now() + DEBOUNCE;
        for d in self.docs.values_mut() {
            if d.uri != changed && (d.text.contains("use ./") || d.text.contains("use ../")) {
                d.fresh = false;
                d.due = Some(d.due.map_or(due, |t| t.min(due)));
            }
        }
    }

    /// Checks the buffers whose time has come.
    fn check_due(&mut self) {
        let now = Instant::now();
        let due: Vec<String> = self.docs.values().filter(|d| d.due.is_some_and(|t| t <= now)).map(|d| d.uri.clone()).collect();
        for uri in due {
            self.check_now(&uri);
        }
    }

    /// Checks a buffer (if its check is not current) and publishes its diagnostics.
    fn check_now(&mut self, uri: &str) {
        let Some(doc) = self.docs.get_mut(uri) else { return };
        doc.due = None;
        if doc.fresh {
            return;
        }
        let a = check(&doc.text, doc.path.as_deref());
        let index = self.docs_index();
        let doc = self.docs.get_mut(uri).expect("the buffer is open");
        let diags = features::diagnostics(&doc.text, doc.path.as_deref(), &a, self.enc, &index);
        if a.checked {
            doc.good = None;
        } else if let Some(prev) = doc.analysis.take().filter(|(p, _)| p.checked) {
            doc.good = Some(prev);
        }
        doc.analysis = Some((a, doc.text.clone()));
        doc.fresh = true;
        let mut params = vec![("uri".to_string(), Json::from(uri)), ("diagnostics".to_string(), Json::Arr(diags))];
        params.push(("version".to_string(), Json::from(doc.version)));
        self.send(&rpc::notification("textDocument/publishDiagnostics", Json::Obj(params)));
    }

    /// The URIs of the open buffers by path (`modules::overlay_key`), to name other files.
    fn docs_index(&self) -> HashMap<String, String> {
        self.docs.values().filter_map(|d| d.path.as_ref().map(|p| (modules::overlay_key(p), d.uri.clone()))).collect()
    }

    /// Runs a request on the buffer it names, checked first if it changed.
    fn with_doc(&mut self, params: &Json, f: fn(&features::View, &Json) -> Json) -> Result<Json, (i64, String)> {
        let uri = params
            .get("textDocument")
            .and_then(|t| t.get("uri"))
            .and_then(Json::as_str)
            .ok_or((rpc::INVALID_PARAMS, "missing textDocument.uri".to_string()))?
            .to_string();
        if !self.docs.contains_key(&uri) {
            return Err((rpc::INVALID_PARAMS, format!("the document {uri} is not open")));
        }
        self.check_now(&uri);
        let docs = self.docs_index();
        let doc = &self.docs[&uri];
        let view = features::View::new(
            &doc.uri,
            &doc.text,
            doc.path.as_deref(),
            doc.analysis.as_ref().map(|(a, _)| a).expect("checked above"),
            doc.good.as_ref().map(|(a, t)| (a, t.as_str())),
            self.enc,
            docs,
        );
        Ok(f(&view, params))
    }
}

/// Checks a text, catching a panic of the compiler (reported, and the buffer then has no
/// diagnostics instead of stopping the server).
fn check(text: &str, path: Option<&Path>) -> Analysis {
    match catch_unwind(AssertUnwindSafe(|| analysis::analyze(text, path))) {
        Ok(a) => a,
        Err(panic) => {
            eprintln!(
                "nyra lsp: internal error while checking {}: {}",
                path.map_or("a buffer".into(), |p| p.display().to_string()),
                panic_text(&panic)
            );
            Analysis {
                errors: Vec::new(),
                warnings: Vec::new(),
                prog: None,
                checked: false,
                exprs: HashMap::new(),
                lets: HashMap::new(),
            }
        }
    }
}

fn panic_text(panic: &Box<dyn std::any::Any + Send>) -> String {
    panic
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| panic.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown".into())
}

/// The path of a `file:` URI (`None` for other schemes, such as an unsaved `untitled:` buffer).
pub fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let bytes = rest.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("zz"), 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    let s = String::from_utf8(out).ok()?;
    // `file:///c:/x` is `c:/x` on Windows; `file://host/share/x` is a network path
    let s = if !s.starts_with('/') {
        format!("//{s}")
    } else if cfg!(windows) && s.len() > 2 && s.as_bytes()[2] == b':' {
        s[1..].to_string()
    } else {
        s
    };
    Some(PathBuf::from(s))
}

/// The `file:` URI of a path.
pub fn path_to_uri(p: &Path) -> String {
    let mut s = p.to_string_lossy().replace('\\', "/");
    if let Some(rest) = s.strip_prefix("//?/") {
        s = rest.to_string();
    }
    if !s.starts_with('/') {
        s = format!("/{s}");
    }
    let mut out = String::from("file://");
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"/-_.~:".contains(&b) {
            out.push(b as char);
        } else {
            out += &format!("%{b:02X}");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_uris_and_paths() {
        if cfg!(windows) {
            assert_eq!(uri_to_path("file:///c%3A/My%20Code/a.nyra"), Some(PathBuf::from("c:/My Code/a.nyra")));
            assert_eq!(path_to_uri(Path::new(r"C:\My Code\a.nyra")), "file:///C:/My%20Code/a.nyra");
        } else {
            assert_eq!(uri_to_path("file:///home/x/My%20Code/a.nyra"), Some(PathBuf::from("/home/x/My Code/a.nyra")));
            assert_eq!(path_to_uri(Path::new("/home/x/My Code/a.nyra")), "file:///home/x/My%20Code/a.nyra");
        }
        assert_eq!(uri_to_path("untitled:Untitled-1"), None);
    }
}
