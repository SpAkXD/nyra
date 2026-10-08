//! `nyra mcp`: a Model Context Protocol server, so an AI agent (Claude Code, Claude Desktop,
//! Cursor, Gemini CLI, ...) can check, run and learn Nyra with no setup besides the `nyra` binary.
//!
//! Transport: stdio, one JSON-RPC 2.0 message per line. Requests are answered in order. Nothing
//! but protocol messages is ever written to stdout (the C compiler's output is captured).
//!
//!     tools       nyra_spec, nyra_check, nyra_run, nyra_explain, nyra_build
//!     resources   nyra://spec, nyra://guide, nyra://errors, nyra://errors/{code}
//!
//! Programs are compiled in-process with the same pipeline as the CLI. Generated files and cached
//! executables live in a temp dir of their own per server process, removed when stdin closes.

use std::io::{BufRead, Read, Write};
use std::path::PathBuf;
use std::process::{Command, ExitCode, Stdio};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime};

use crate::json::{obj, Json};
use crate::{diag, explain, Target};

/// The name programs get in diagnostics and runtime errors.
const FILE: &str = "main.nyra";
const SPEC: &str = include_str!("../docs/SPEC.md");
const GUIDE: &str = include_str!("../docs/AI_GUIDE.md");

/// Newest first; a client asking for another version gets the newest.
const VERSIONS: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

const STDOUT_CAP: usize = 16 * 1024;
const STDERR_CAP: usize = 8 * 1024;
const TIMEOUT_MS: u64 = 10_000;
const MAX_TIMEOUT_MS: u64 = 60_000;

const INSTRUCTIONS: &str = "Nyra is a small, strictly typed language that is not in your training data. \
Before writing Nyra, call nyra_spec once (part \"guide\" adds rules, recipes and error fixes); do not guess syntax. \
Loop: nyra_check until ok is true, then nyra_run. nyra_explain gives the full entry for an error code.";

/// Tool definitions, as sent by `tools/list`.
const TOOLS: &str = r#"[
{"name":"nyra_spec","title":"Nyra language spec","description":"The complete Nyra language spec (Markdown). Nyra is not in your training data: read it once before writing Nyra. part \"guide\" returns the AI guide instead: workflow, do/don't rules, error codes with fixes, recipes, complete programs.","inputSchema":{"type":"object","properties":{"part":{"type":"string","enum":["spec","guide"],"description":"default spec"}}},"annotations":{"readOnlyHint":true,"openWorldHint":false}},
{"name":"nyra_check","title":"Check Nyra code","description":"Type-check a Nyra program without running it. Returns {\"ok\":bool,\"errors\":[{code,message,file,line,col,hint}]}, the same as `nyra check --json`. Fix every error, then check again.","inputSchema":{"type":"object","properties":{"code":{"type":"string","description":"the whole program"}},"required":["code"]},"annotations":{"readOnlyHint":true,"openWorldHint":false}},
{"name":"nyra_run","title":"Run Nyra code","description":"Compile and run a Nyra program. Returns {ok,exit,stdout,stderr?,errors?,timeout?,truncated?,ms}. Compile errors come back as from nyra_check; a runtime error (exit 101) is in errors. stdout is capped at 16 KiB.","inputSchema":{"type":"object","properties":{"code":{"type":"string","description":"the whole program"},"backend":{"type":"string","enum":["native","js"],"description":"native (via a C compiler, default) or js (Node.js)"},"stdin":{"type":"string","description":"standard input for the program"},"timeout_ms":{"type":"integer","minimum":1,"maximum":60000,"description":"default 10000"}},"required":["code"]},"annotations":{"readOnlyHint":false,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}},
{"name":"nyra_explain","title":"Explain a Nyra error code","description":"The error database entry for a code: what it means, why the rule exists, common causes, a wrong and a fixed program, related codes. Without code: every code with its title.","inputSchema":{"type":"object","properties":{"code":{"type":"string","description":"e.g. E0201"}}},"annotations":{"readOnlyHint":true,"openWorldHint":false}},
{"name":"nyra_build","title":"Build Nyra to C or JavaScript","description":"Compile a Nyra program and return the generated source: {ok,target,source}. Compile errors come back as from nyra_check.","inputSchema":{"type":"object","properties":{"code":{"type":"string","description":"the whole program"},"target":{"type":"string","enum":["c","js"],"description":"default c"}},"required":["code"]},"annotations":{"readOnlyHint":true,"openWorldHint":false}}
]"#;

const USAGE: &str = "\
usage: nyra mcp

Runs a Model Context Protocol server on stdin/stdout (newline-delimited JSON-RPC 2.0), so AI
agents can check, run and learn Nyra. Add it to Claude Code with:

  claude mcp add nyra -- nyra mcp

Tools: nyra_spec, nyra_check, nyra_run, nyra_explain, nyra_build.
Resources: nyra://spec, nyra://guide, nyra://errors, nyra://errors/{code}.
";

pub fn run(args: Vec<String>) -> ExitCode {
    match args.first().map(String::as_str) {
        None => {}
        Some("-h" | "--help" | "help") => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Some(a) => {
            eprintln!("nyra: `nyra mcp` takes no arguments, found `{a}`\n\n{USAGE}");
            return ExitCode::from(2);
        }
    }
    let mut server = Server::new();
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let stdout = std::io::stdout();
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match input.read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let Some(reply) = server.handle_line(&buf) else { continue };
        let mut out = stdout.lock();
        if writeln!(out, "{reply}").and_then(|_| out.flush()).is_err() {
            break;
        }
    }
    ExitCode::SUCCESS
}

// ---- JSON-RPC ------------------------------------------------------------------------------------

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;
const RESOURCE_NOT_FOUND: i64 = -32002;

struct RpcError {
    code: i64,
    message: String,
    data: Option<Json>,
}

fn rpc_err(code: i64, message: impl Into<String>) -> RpcError {
    RpcError { code, message: message.into(), data: None }
}

fn error_reply(id: Json, e: RpcError) -> Json {
    let mut error = vec![("code".to_string(), Json::from(e.code)), ("message".to_string(), Json::from(e.message))];
    if let Some(data) = e.data {
        error.push(("data".to_string(), data));
    }
    obj([("jsonrpc", "2.0".into()), ("id", id), ("error", Json::Obj(error))])
}

struct Server {
    /// Per-process scratch dir: generated sources and the cache of compiled programs.
    dir: PathBuf,
    /// The C compiler, looked up on first use.
    cc: Option<Option<String>>,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl Server {
    fn new() -> Server {
        let base = crate::temp_dir();
        // dirs of servers that were killed instead of shut down; a day old means abandoned
        let day = Duration::from_secs(24 * 60 * 60);
        for entry in std::fs::read_dir(&base).into_iter().flatten().flatten() {
            let old = entry
                .metadata()
                .and_then(|m| m.modified())
                .is_ok_and(|t| SystemTime::now().duration_since(t).is_ok_and(|age| age > day));
            if old && entry.file_name().to_string_lossy().starts_with("mcp-") {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
        Server { dir: base.join(format!("mcp-{}", std::process::id())), cc: None }
    }

    /// One line of input: a message or a batch. `None` when there is nothing to answer.
    fn handle_line(&mut self, line: &[u8]) -> Option<Json> {
        let Ok(text) = std::str::from_utf8(line) else {
            return Some(error_reply(Json::Null, rpc_err(PARSE_ERROR, "Parse error: the message is not valid UTF-8")));
        };
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        let msg = match Json::parse(text) {
            Ok(m) => m,
            Err(e) => return Some(error_reply(Json::Null, rpc_err(PARSE_ERROR, format!("Parse error: {e}")))),
        };
        match msg {
            Json::Arr(items) if items.is_empty() => {
                Some(error_reply(Json::Null, rpc_err(INVALID_REQUEST, "Invalid Request: empty batch")))
            }
            Json::Arr(items) => {
                let replies: Vec<Json> = items.iter().filter_map(|m| self.handle(m)).collect();
                (!replies.is_empty()).then_some(Json::Arr(replies))
            }
            m => self.handle(&m),
        }
    }

    /// One message. Notifications and responses get no reply.
    fn handle(&mut self, msg: &Json) -> Option<Json> {
        if !msg.is_object() {
            return Some(error_reply(Json::Null, rpc_err(INVALID_REQUEST, "Invalid Request: not an object")));
        }
        let id = msg.get("id").cloned();
        let id_ok = matches!(id, None | Some(Json::Str(_) | Json::Num(_)));
        let reply_id = if id_ok { id.clone().unwrap_or(Json::Null) } else { Json::Null };
        if msg.get("jsonrpc").and_then(Json::as_str) != Some("2.0") {
            return Some(error_reply(reply_id, rpc_err(INVALID_REQUEST, "Invalid Request: jsonrpc must be \"2.0\"")));
        }
        if !id_ok {
            return Some(error_reply(Json::Null, rpc_err(INVALID_REQUEST, "Invalid Request: id must be a string or a number")));
        }
        let Some(method) = msg.get("method").and_then(Json::as_str) else {
            // a response to a request of ours (we send none), or garbage
            if id.is_some() && (msg.get("result").is_some() || msg.get("error").is_some()) {
                return None;
            }
            return Some(error_reply(reply_id, rpc_err(INVALID_REQUEST, "Invalid Request: method must be a string")));
        };
        // notifications (`notifications/initialized`, `notifications/cancelled`, ...) need no action
        let id = id?;
        let empty = Json::Obj(Vec::new());
        let params = match msg.get("params") {
            None => &empty,
            Some(p @ Json::Obj(_)) => p,
            Some(_) => return Some(error_reply(id, rpc_err(INVALID_PARAMS, "Invalid params: params must be an object"))),
        };
        Some(match self.request(method, params) {
            Ok(result) => obj([("jsonrpc", "2.0".into()), ("id", id), ("result", result)]),
            Err(e) => error_reply(id, e),
        })
    }

    fn request(&mut self, method: &str, params: &Json) -> Result<Json, RpcError> {
        match method {
            "initialize" => {
                let Some(asked) = params.get("protocolVersion").and_then(Json::as_str) else {
                    return Err(rpc_err(INVALID_PARAMS, "Invalid params: protocolVersion must be a string"));
                };
                let version = VERSIONS.iter().find(|v| **v == asked).unwrap_or(&VERSIONS[0]);
                let no_changes = || obj([("listChanged", false.into())]);
                Ok(obj([
                    ("protocolVersion", (*version).into()),
                    (
                        "capabilities",
                        obj([
                            ("tools", no_changes()),
                            ("resources", obj([("subscribe", false.into()), ("listChanged", false.into())])),
                        ]),
                    ),
                    (
                        "serverInfo",
                        obj([("name", "nyra".into()), ("title", "Nyra".into()), ("version", env!("CARGO_PKG_VERSION").into())]),
                    ),
                    ("instructions", INSTRUCTIONS.into()),
                ]))
            }
            "ping" => Ok(Json::Obj(Vec::new())),
            "tools/list" => Ok(obj([("tools", tools())])),
            "tools/call" => {
                let Some(name) = params.get("name").and_then(Json::as_str) else {
                    return Err(rpc_err(INVALID_PARAMS, "Invalid params: name must be a string"));
                };
                let empty = Json::Obj(Vec::new());
                let args = match params.get("arguments") {
                    None | Some(Json::Null) => &empty,
                    Some(a @ Json::Obj(_)) => a,
                    Some(_) => return Err(rpc_err(INVALID_PARAMS, "Invalid params: arguments must be an object")),
                };
                let out = match name {
                    "nyra_spec" => spec(args),
                    "nyra_check" => check(args),
                    "nyra_run" => self.run_tool(args),
                    "nyra_explain" => explain_tool(args),
                    "nyra_build" => build(args),
                    _ => return Err(rpc_err(INVALID_PARAMS, format!("Unknown tool: {name}"))),
                };
                let (text, is_error) = match out {
                    Ok(text) => (text, false),
                    Err(text) => (text, true),
                };
                let content = obj([("type", "text".into()), ("text", text.into())]);
                Ok(obj([("content", vec![content].into()), ("isError", is_error.into())]))
            }
            "resources/list" => Ok(obj([("resources", resources())])),
            "resources/templates/list" => Ok(obj([(
                "resourceTemplates",
                vec![obj([
                    ("uriTemplate", "nyra://errors/{code}".into()),
                    ("name", "error".into()),
                    ("title", "Nyra error entry".into()),
                    ("description", "One entry of the error database, e.g. nyra://errors/E0201 (JSON)".into()),
                    ("mimeType", "application/json".into()),
                ])]
                .into(),
            )])),
            "resources/read" => {
                let Some(uri) = params.get("uri").and_then(Json::as_str) else {
                    return Err(rpc_err(INVALID_PARAMS, "Invalid params: uri must be a string"));
                };
                let Some((mime, text)) = read_resource(uri) else {
                    return Err(RpcError {
                        code: RESOURCE_NOT_FOUND,
                        message: format!("Resource not found: {uri}"),
                        data: Some(obj([("uri", uri.into())])),
                    });
                };
                let content = obj([("uri", uri.into()), ("mimeType", mime.into()), ("text", text.into())]);
                Ok(obj([("contents", vec![content].into())]))
            }
            _ => Err(rpc_err(METHOD_NOT_FOUND, format!("Method not found: {method}"))),
        }
    }

    // ---- nyra_run ---------------------------------------------------------------------------------

    fn run_tool(&mut self, args: &Json) -> Result<String, String> {
        let code = required_str(args, "code")?;
        let target = match optional_str(args, "backend")? {
            None | Some("native") => Target::Native,
            Some("js") => Target::Js,
            Some(other) => return Err(tool_error(format!("backend must be \"native\" or \"js\", found {other:?}"))),
        };
        let stdin = optional_str(args, "stdin")?.unwrap_or("");
        let timeout = match args.get("timeout_ms") {
            None | Some(Json::Null) => TIMEOUT_MS,
            Some(t) => match t.as_f64() {
                Some(ms) if ms >= 1.0 && ms <= MAX_TIMEOUT_MS as f64 => ms as u64,
                _ => return Err(tool_error(format!("timeout_ms must be a number from 1 to {MAX_TIMEOUT_MS}"))),
            },
        };

        let start = Instant::now();
        let source = match generate(code, target)? {
            Ok(source) => source,
            Err(diags) => return Ok(diags),
        };
        let compile_ms = ms(start.elapsed());

        let mut cc_ms = None;
        let mut cmd = if target == Target::Js {
            let path = crate::write_temp(&self.dir, "main.js", &source).map_err(tool_error)?;
            let mut c = Command::new("node");
            c.arg(path);
            c
        } else {
            let compiler = self.cc.get_or_insert_with(crate::find_cc).clone().ok_or_else(|| {
                tool_error("no C compiler found (tried gcc, clang, cc, tcc); install one, set NYRA_CC, or use backend \"js\"")
            })?;
            let (exe, t) =
                crate::cc_cached(&compiler, &source, "main", FILE, &self.dir, true).map_err(tool_error)?;
            cc_ms = Some(t);
            Command::new(exe)
        };
        cmd.current_dir(&self.dir).env("NYRA_JSON", "1");
        let ran = execute(cmd, stdin, Duration::from_millis(timeout)).map_err(|e| {
            let hint = if target == Target::Js { " (is Node.js installed? or use backend \"native\")" } else { "" };
            tool_error(format!("failed to start the program: {e}{hint}"))
        })?;

        // runtime errors arrive as JSON lines on stderr (NYRA_JSON=1); report them structured
        let mut errors = Vec::new();
        let mut rest = String::new();
        for line in ran.stderr.lines() {
            let parsed = line.starts_with('{').then(|| Json::parse(line).ok()).flatten();
            match parsed.as_ref().and_then(|j| j.get("errors")).and_then(Json::as_array) {
                Some(errs) => errors.extend(errs.iter().cloned()),
                None => {
                    rest += line;
                    rest.push('\n');
                }
            }
        }

        let mut fields = vec![
            ("ok", Json::from(ran.exit == Some(0))),
            ("exit", ran.exit.map(|c| Json::from(c as i64)).unwrap_or(Json::Null)),
            ("stdout", ran.stdout.into()),
        ];
        if !rest.is_empty() {
            fields.push(("stderr", rest.into()));
        }
        if !errors.is_empty() {
            fields.push(("errors", errors.into()));
        }
        if let Some(sig) = ran.signal {
            fields.push(("signal", Json::from(sig as i64)));
        }
        if ran.timed_out {
            fields.push(("timeout", true.into()));
        }
        if ran.truncated {
            fields.push(("truncated", true.into()));
        }
        let mut times = vec![("compile", compile_ms)];
        match cc_ms {
            Some(Some(t)) => times.push(("cc", ms(t))),
            Some(None) => times.push(("cached", true.into())),
            None => {}
        }
        times.push(("run", ms(ran.elapsed)));
        fields.push(("ms", Json::Obj(times.into_iter().map(|(k, v)| (k.to_string(), v)).collect())));
        Ok(Json::Obj(fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect()).to_string())
    }
}

fn tools() -> Json {
    Json::parse(TOOLS).expect("TOOLS is valid JSON")
}

fn resources() -> Json {
    let res = |uri: &str, name: &str, title: &str, description: &str, mime: &str, size: Option<usize>| {
        let mut fields = vec![
            ("uri", Json::from(uri)),
            ("name", name.into()),
            ("title", title.into()),
            ("description", description.into()),
            ("mimeType", mime.into()),
        ];
        if let Some(n) = size {
            fields.push(("size", Json::from(n as i64)));
        }
        Json::Obj(fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
    };
    vec![
        res("nyra://spec", "spec", "Nyra language spec", "The complete language: types, operators, builtins, methods, structs, memory, runtime errors", "text/markdown", Some(SPEC.len())),
        res("nyra://guide", "guide", "Nyra guide for AI agents", "Workflow, do/don't rules, what does not exist yet, error codes with fixes, recipes, complete programs", "text/markdown", Some(GUIDE.len())),
        res("nyra://errors", "errors", "Nyra error index", "Every error code with its title, kind and version (JSON); nyra://errors/E0201 reads one entry", "application/json", None),
    ]
    .into()
}

fn read_resource(uri: &str) -> Option<(&'static str, String)> {
    match uri {
        "nyra://spec" => Some(("text/markdown", SPEC.to_string())),
        "nyra://guide" => Some(("text/markdown", GUIDE.to_string())),
        "nyra://errors" => Some(("application/json", explain::list_json(&explain::database().ok()?))),
        _ => {
            let code = explain::normalize(uri.strip_prefix("nyra://errors/")?);
            let all = explain::database().ok()?;
            let entry = all.iter().find(|e| e.code == code)?;
            Some(("application/json", explain::entry_json(entry)))
        }
    }
}

// ---- tools -----------------------------------------------------------------------------------------
// Each returns the text of the result: `Ok` for a normal result, `Err` for one with `isError`.

/// `{"ok":false,"error":"..."}`
fn tool_error(msg: impl Into<String>) -> String {
    obj([("ok", false.into()), ("error", msg.into().into())]).to_string()
}

fn optional_str<'a>(args: &'a Json, key: &str) -> Result<Option<&'a str>, String> {
    match args.get(key) {
        None | Some(Json::Null) => Ok(None),
        Some(Json::Str(s)) => Ok(Some(s)),
        Some(_) => Err(tool_error(format!("argument `{key}` must be a string"))),
    }
}

fn required_str<'a>(args: &'a Json, key: &str) -> Result<&'a str, String> {
    optional_str(args, key)?.ok_or_else(|| tool_error(format!("missing argument `{key}`")))
}

fn spec(args: &Json) -> Result<String, String> {
    match optional_str(args, "part")? {
        None | Some("spec") => Ok(SPEC.to_string()),
        Some("guide") => Ok(GUIDE.to_string()),
        Some(other) => Err(tool_error(format!("part must be \"spec\" or \"guide\", found {other:?}"))),
    }
}

fn check(args: &Json) -> Result<String, String> {
    let code = required_str(args, "code")?;
    Ok(match crate::compile(code) {
        Ok(_) => diag::render_json(&[], FILE),
        Err(diags) => diag::render_json(&diags, FILE),
    })
}

/// Compiles to C or JS. The inner `Err` is the diagnostics JSON (a normal tool result);
/// the outer one is a failure of the compiler itself.
fn generate(code: &str, target: Target) -> Result<Result<String, String>, String> {
    let prog = match crate::compile(code) {
        Ok(p) => p,
        Err(diags) => return Ok(Err(diag::render_json(&diags, FILE))),
    };
    crate::generate(&prog, target, FILE).map(Ok).map_err(tool_error)
}

fn build(args: &Json) -> Result<String, String> {
    let code = required_str(args, "code")?;
    let (target, name) = match optional_str(args, "target")? {
        None | Some("c") => (Target::C, "c"),
        Some("js") => (Target::Js, "js"),
        Some(other) => return Err(tool_error(format!("target must be \"c\" or \"js\", found {other:?}"))),
    };
    Ok(match generate(code, target)? {
        Ok(source) => obj([("ok", true.into()), ("target", name.into()), ("source", source.into())]).to_string(),
        Err(diags) => diags,
    })
}

fn explain_tool(args: &Json) -> Result<String, String> {
    let all = explain::database().map_err(tool_error)?;
    let Some(arg) = optional_str(args, "code")? else {
        return Ok(explain::list_json(&all));
    };
    let code = explain::normalize(arg);
    match all.iter().find(|e| e.code == code) {
        Some(e) => Ok(explain::entry_json(e)),
        None => Err(explain::unknown_json(arg, explain::closest(&code, &all))),
    }
}

fn ms(d: Duration) -> Json {
    Json::fixed(d.as_secs_f64() * 1000.0, 1)
}

// ---- running a program ---------------------------------------------------------------------------

struct Ran {
    exit: Option<i32>,
    signal: Option<i32>,
    stdout: String,
    stderr: String,
    truncated: bool,
    timed_out: bool,
    elapsed: Duration,
}

/// Runs `cmd` with `stdin` as its input, captures its output (capped) and kills it after `timeout`.
fn execute(mut cmd: Command, stdin: &str, timeout: Duration) -> std::io::Result<Ran> {
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let start = Instant::now();
    let mut child = cmd.spawn()?;
    if let Some(mut pipe) = child.stdin.take() {
        let input = stdin.as_bytes().to_vec();
        // a thread, so a program that never reads cannot block us; dropping the pipe sends EOF
        thread::spawn(move || {
            let _ = pipe.write_all(&input);
        });
    }
    let out = child.stdout.take().map(|p| drain(p, STDOUT_CAP));
    let err = child.stderr.take().map(|p| drain(p, STDERR_CAP));

    let mut timed_out = false;
    let mut pause = Duration::from_micros(200);
    let status = loop {
        if let Some(s) = child.try_wait()? {
            break Some(s);
        }
        if start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            timed_out = true;
            break None;
        }
        thread::sleep(pause);
        pause = (pause * 2).min(Duration::from_millis(5));
    };
    let elapsed = start.elapsed();
    let collect = |h: Option<JoinHandle<(Vec<u8>, bool)>>| h.and_then(|h| h.join().ok()).unwrap_or_default();
    let (out, out_cut) = collect(out);
    let (err, err_cut) = collect(err);

    #[cfg(unix)]
    let signal = {
        use std::os::unix::process::ExitStatusExt;
        status.and_then(|s| s.signal())
    };
    #[cfg(not(unix))]
    let signal = None;
    Ok(Ran {
        exit: if timed_out { None } else { status.and_then(|s| s.code()) },
        signal: if timed_out { None } else { signal },
        stdout: text(&out, out_cut),
        stderr: text(&err, err_cut),
        truncated: out_cut || err_cut,
        timed_out,
        elapsed,
    })
}

/// Reads a pipe to its end, keeping the first `cap` bytes. Returns them and whether more came.
fn drain(mut pipe: impl Read + Send + 'static, cap: usize) -> JoinHandle<(Vec<u8>, bool)> {
    thread::spawn(move || {
        let mut kept = Vec::new();
        let mut cut = false;
        let mut chunk = [0u8; 8192];
        loop {
            match pipe.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let room = cap.saturating_sub(kept.len());
                    kept.extend_from_slice(&chunk[..n.min(room)]);
                    cut |= n > room;
                }
            }
        }
        (kept, cut)
    })
}

/// Program output as text with `\n` line ends (native Windows programs write `\r\n`).
fn text(bytes: &[u8], cut: bool) -> String {
    let mut s = String::from_utf8_lossy(bytes).replace("\r\n", "\n");
    if cut {
        // the cap may have split a character
        while s.ends_with('\u{FFFD}') {
            s.pop();
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(server: &mut Server, line: &str) -> Json {
        server.handle_line(line.as_bytes()).expect("a reply")
    }

    #[test]
    fn tool_definitions_are_valid() {
        let tools = tools();
        let names: Vec<&str> =
            tools.as_array().unwrap().iter().map(|t| t.get("name").and_then(Json::as_str).unwrap()).collect();
        assert_eq!(names, ["nyra_spec", "nyra_check", "nyra_run", "nyra_explain", "nyra_build"]);
        for t in tools.as_array().unwrap() {
            assert_eq!(t.get("inputSchema").and_then(|s| s.get("type")).and_then(Json::as_str), Some("object"));
        }
    }

    #[test]
    fn negotiates_the_protocol_version() {
        let mut s = Server::new();
        let r = call(&mut s, r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05"}}"#);
        assert_eq!(r.get("result").unwrap().get("protocolVersion").unwrap().as_str(), Some("2024-11-05"));
        let r = call(&mut s, r#"{"jsonrpc":"2.0","id":"a","method":"initialize","params":{"protocolVersion":"1999-01-01"}}"#);
        assert_eq!(r.get("result").unwrap().get("protocolVersion").unwrap().as_str(), Some(VERSIONS[0]));
        assert_eq!(r.get("id").unwrap().as_str(), Some("a"));
    }

    #[test]
    fn protocol_errors() {
        let mut s = Server::new();
        let code = |r: &Json| r.get("error").unwrap().get("code").unwrap().to_string();
        assert_eq!(code(&call(&mut s, "{nope")), "-32700");
        assert_eq!(code(&call(&mut s, "[]")), "-32600");
        assert_eq!(code(&call(&mut s, "3")), "-32600");
        assert_eq!(code(&call(&mut s, r#"{"jsonrpc":"2.0","id":2,"method":"nope"}"#)), "-32601");
        assert_eq!(code(&call(&mut s, r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"x"}}"#)), "-32602");
        assert_eq!(code(&call(&mut s, r#"{"jsonrpc":"2.0","id":2,"method":"resources/read","params":{"uri":"nyra://x"}}"#)), "-32002");
        assert_eq!(code(&call(&mut s, r#"{"jsonrpc":"1.0","id":2,"method":"ping"}"#)), "-32600");
        assert_eq!(code(&call(&mut s, r#"{"jsonrpc":"2.0","id":null,"method":"ping"}"#)), "-32600");
        // notifications and responses get no reply, even unknown ones
        assert!(s.handle_line(br#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).is_none());
        assert!(s.handle_line(br#"{"jsonrpc":"2.0","method":"whatever"}"#).is_none());
        assert!(s.handle_line(br#"{"jsonrpc":"2.0","id":5,"result":{}}"#).is_none());
        assert!(s.handle_line(b"  \r\n").is_none());
        // a batch answers its requests only
        let r = call(&mut s, r#"[{"jsonrpc":"2.0","id":1,"method":"ping"},{"jsonrpc":"2.0","method":"notifications/initialized"}]"#);
        assert_eq!(r.to_string(), r#"[{"jsonrpc":"2.0","id":1,"result":{}}]"#);
    }

    #[test]
    fn resources_resolve() {
        for uri in ["nyra://spec", "nyra://guide", "nyra://errors", "nyra://errors/E0201", "nyra://errors/e201"] {
            assert!(read_resource(uri).is_some(), "{uri}");
        }
        // (tests/errors_db.rs counts every code written in src/, so no made-up code here)
        assert!(read_resource("nyra://errors/nope").is_none());
        assert!(read_resource("nyra://other").is_none());
    }

    #[test]
    fn output_is_capped_at_a_character_boundary() {
        assert_eq!(text("ab\u{e9}".as_bytes()[..3].as_ref(), true), "ab");
        assert_eq!(text(b"a\r\nb\r\n", false), "a\nb\n");
    }
}
