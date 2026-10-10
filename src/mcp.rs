//! `nyra mcp`: a Model Context Protocol server, so an AI agent (Claude Code, Claude Desktop,
//! Cursor, Gemini CLI, ...) can check, run and learn Nyra with no setup besides the `nyra` binary.
//!
//! Transport: stdio, one JSON-RPC 2.0 message per line. Requests are answered in order. Nothing
//! but protocol messages is ever written to stdout (the C compiler's output is captured).
//!
//!     tools       nyra_spec, nyra_check, nyra_test, nyra_run, nyra_explain, nyra_build,
//!                 nyra_outline, nyra_show, nyra_edit (symbol-addressed editing, see edit.rs)
//!     resources   nyra://spec, nyra://guide, nyra://errors, nyra://errors/{code}
//!
//! Programs are compiled in-process with the same pipeline as the CLI. Generated files and cached
//! executables live in a temp dir of their own per server process, removed when stdin closes.
//!
//! A program is given as `code` (one file) or as `files`, a map from file names to text (`main.nyra`
//! and the files it imports with `use ./name`). Like the CLI, the tools repair an error that has
//! exactly one certain fix in memory and report each repair under `warnings`; `strict: true` turns
//! that off.

use std::io::{BufRead, Read, Write};
use std::path::PathBuf;
use std::process::{Command, ExitCode, Stdio};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime};

use crate::json::{obj, Json};
use crate::{ast, caps, diag, examples, explain, fix, modules, perfwarn, sandbox, Target};

/// The name programs get in diagnostics and runtime errors.
const FILE: &str = "main.nyra";
const SPEC: &str = include_str!("../docs/SPEC.md");
const GUIDE: &str = include_str!("../docs/AI_GUIDE.md");
const CARD_FILE: &str = include_str!("../docs/AGENT_CARD.md");

/// The agent card (docs/AGENT_CARD.md): the compact spec `nyra_spec` returns by default. The file starts with a metadata
/// comment (token count and budget) that a model has no use for, so it is cut off.
pub fn card() -> &'static str {
    match CARD_FILE.strip_prefix("<!--").and_then(|rest| rest.split_once("-->")) {
        Some((_, body)) => body.trim_start_matches(['\r', '\n']),
        None => CARD_FILE,
    }
}

/// Newest first; a client asking for another version gets the newest.
const VERSIONS: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

const STDOUT_CAP: usize = 16 * 1024;
/// The defaults of a sandboxed run: about ten seconds of interpreting.
const SANDBOX_FUEL: u64 = 200_000_000;
const SANDBOX_MEMORY: u64 = 256 << 20;
const STDERR_CAP: usize = 8 * 1024;
const TIMEOUT_MS: u64 = 10_000;
const MAX_TIMEOUT_MS: u64 = 60_000;

const INSTRUCTIONS: &str = "Nyra is a small, strictly typed language that is not in your training data. \
Before writing Nyra, call nyra_spec once: it returns the compact agent card (full: true returns the complete spec, part \"guide\" adds rules, recipes and error fixes); do not guess syntax. \
Loop: nyra_check until ok is true, then nyra_run. nyra_explain gives the full entry for an error code. After each non-trivial function write 1-2 examples (`ex f(3) == 9`): nyra_check runs them and reports a false one as E0250 with the actual value. To change an existing program, do not resend it: nyra_outline it, nyra_show the symbols you need, and nyra_edit them by name. Programs run with no capabilities but standard input: a `use fs` or `use os` needs allow:[\"fs\"] or [\"os\"] (error E0290 names it); nyra_outline lists what each function needs. sandbox:true runs a program in the interpreter, with limits on steps, memory and output.";

/// Tool definitions, as sent by `tools/list`.
const TOOLS: &str = r#"[
{"name":"nyra_spec","title":"Nyra language spec","description":"The Nyra agent card (Markdown, about 1,400 tokens): one example program, the rules that differ from other languages, what is not in Nyra, every method and module name. Nyra is not in your training data: read it once before writing Nyra. full: true returns the complete language spec instead (about 8,000 tokens). part \"guide\" returns the AI guide: workflow, do/don't rules, error codes with fixes, recipes, complete programs.","inputSchema":{"type":"object","properties":{"full":{"type":"boolean","description":"return the complete spec instead of the card (default false)"},"part":{"type":"string","enum":["spec","guide"],"description":"default spec (the card, or the complete spec with full: true)"}}},"annotations":{"readOnlyHint":true,"openWorldHint":false}},
{"name":"nyra_check","title":"Check Nyra code","description":"Type-check a Nyra program (code, or files for a program of several files) without running it, and evaluate its `ex` examples. Returns {\"ok\":bool,\"errors\":[{code,message,file,line,col,hint}],\"warnings\":[..]}, the same as `nyra check --json`; a false example is E0250 with actual and expected. An error with exactly one certain fix is repaired in memory and listed under warnings (with the fix edits), unless strict is true. Fix every error, then check again.","inputSchema":{"type":"object","properties":{"code":{"type":"string","description":"the whole program"},"files":{"type":"object","additionalProperties":{"type":"string"},"description":"or a program of several files: file name (relative, ends in .nyra) -> text; main.nyra is the entry unless entry says otherwise, and `use ./shapes` imports shapes.nyra"},"entry":{"type":"string","description":"with files: the file to run (default main.nyra)"},"strict":{"type":"boolean","description":"do not repair errors that have exactly one certain fix (default: repair in memory and report each repair under warnings)"},"allow":{"type":"array","items":{"type":"string","enum":["fs","input","net","os"]},"description":"capabilities nyra_run will grant; default [\"input\"]. A `use` of a module that needs another one is error E0290."}},"required":[]},"annotations":{"readOnlyHint":true,"openWorldHint":false}},
{"name":"nyra_test","title":"Test Nyra examples","description":"Run the `ex` examples of a Nyra program (`fn sq(x: int) -> int = x * x  ex sq(3) == 9`) at compile time, without running main. Returns {ok,examples,passed,failed,errors:[{code,message,line,col,hint,actual?,expected?}]}, the same as `nyra test --json`; compile errors come back as from nyra_check (repairs included).","inputSchema":{"type":"object","properties":{"code":{"type":"string","description":"the whole program"},"files":{"type":"object","additionalProperties":{"type":"string"},"description":"or a program of several files: file name (relative, ends in .nyra) -> text; main.nyra is the entry unless entry says otherwise, and `use ./shapes` imports shapes.nyra"},"entry":{"type":"string","description":"with files: the file to run (default main.nyra)"},"strict":{"type":"boolean","description":"do not repair errors that have exactly one certain fix (default: repair in memory and report each repair under warnings)"},"allow":{"type":"array","items":{"type":"string","enum":["fs","input","net","os"]},"description":"capabilities nyra_run will grant; default [\"input\"]"}},"required":[]},"annotations":{"readOnlyHint":true,"openWorldHint":false}},
{"name":"nyra_run","title":"Run Nyra code","description":"Compile and run a Nyra program (code, or files for several files). Returns {ok,exit,stdout,stderr?,errors?,warnings?,timeout?,truncated?,ms}. A program that ends within a few million steps is answered at once from the interpreter (\"mode\":\"interp\", no wait for the C compiler); one that runs longer, or that uses fs or os, runs as a native executable, with the same output. Compile errors come back as from nyra_check, and an error with one certain fix is repaired in memory first (listed under warnings; strict: true turns that off); a runtime error (exit 101) is in errors. The program gets only the capabilities in allow (default: standard input): a `use fs` or `use os` without them is error E0290. stdout is capped at 16 KiB; a run may use 1 GiB of memory and a CPU-time budget of twice its timeout. With sandbox true the program runs in the interpreter instead (no child process, no C compiler or Node.js): file paths stay below the working folder, and fuel, max_memory and max_output stop it with E0355, E0356 or E0357 (exit 120, 121, 122); the same program and limits always stop at the same place.","inputSchema":{"type":"object","properties":{"code":{"type":"string","description":"the whole program"},"files":{"type":"object","additionalProperties":{"type":"string"},"description":"or a program of several files: file name (relative, ends in .nyra) -> text; main.nyra is the entry unless entry says otherwise, and `use ./shapes` imports shapes.nyra"},"entry":{"type":"string","description":"with files: the file to run (default main.nyra)"},"strict":{"type":"boolean","description":"do not repair errors that have exactly one certain fix (default: repair in memory and report each repair under warnings)"},"backend":{"type":"string","enum":["native","js"],"description":"native (via a C compiler, default) or js (Node.js); ignored with sandbox"},"stdin":{"type":"string","description":"standard input for the program"},"timeout_ms":{"type":"integer","minimum":1,"maximum":60000,"description":"default 10000"},"allow":{"type":"array","items":{"type":"string","enum":["fs","input","net","os"]},"description":"capabilities to grant: fs (files), input (stdin), os (arguments, environment, exit), net. Default [\"input\"]; a program that uses fs and reads stdin needs [\"fs\",\"input\"]"},"sandbox":{"type":"boolean","description":"run in the interpreter with deterministic limits; default false"},"args":{"type":"array","items":{"type":"string"},"description":"sandbox only: the program's arguments (os.args(); needs allow os)"},"fuel":{"type":"integer","minimum":1,"description":"sandbox only: steps the program may run, default 200000000 (E0355)"},"max_memory":{"type":"integer","minimum":1,"description":"sandbox only: bytes of heap, default 268435456 (E0356)"},"max_output":{"type":"integer","minimum":1,"description":"sandbox only: bytes the program may print, default and maximum 16384 (E0357)"}},"required":[]},"annotations":{"readOnlyHint":false,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}},
{"name":"nyra_explain","title":"Explain a Nyra error code","description":"The error database entry for a code: what it means, why the rule exists, common causes, a wrong and a fixed program, related codes. Without code: every code the compiler reports, with its title (planned: true adds the codes of future designs).","inputSchema":{"type":"object","properties":{"code":{"type":"string","description":"e.g. E0201"},"planned":{"type":"boolean","description":"with no code: also list planned codes (not in the compiler yet)"}}},"annotations":{"readOnlyHint":true,"openWorldHint":false}},
{"name":"nyra_build","title":"Build Nyra to C or JavaScript","description":"Compile a Nyra program (code, or files) and return the generated source: {ok,target,source,warnings?}. Compile errors come back as from nyra_check.","inputSchema":{"type":"object","properties":{"code":{"type":"string","description":"the whole program"},"files":{"type":"object","additionalProperties":{"type":"string"},"description":"or a program of several files: file name (relative, ends in .nyra) -> text; main.nyra is the entry unless entry says otherwise, and `use ./shapes` imports shapes.nyra"},"entry":{"type":"string","description":"with files: the file to run (default main.nyra)"},"strict":{"type":"boolean","description":"do not repair errors that have exactly one certain fix (default: repair in memory and report each repair under warnings)"},"target":{"type":"string","enum":["c","js"],"description":"default c"}},"required":[]},"annotations":{"readOnlyHint":true,"openWorldHint":false}}
]"#;

const USAGE: &str = "\
usage: nyra mcp

Runs a Model Context Protocol server on stdin/stdout (newline-delimited JSON-RPC 2.0), so AI
agents can check, run and learn Nyra. Add it to Claude Code with:

  claude mcp add nyra -- nyra mcp

Tools: nyra_spec, nyra_check, nyra_test, nyra_run, nyra_explain, nyra_build, nyra_outline, nyra_show, nyra_edit.
Resources: nyra://card, nyra://spec, nyra://guide, nyra://errors, nyra://errors/{code}.
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
                if !TOOL_NAMES.contains(&name) {
                    return Err(rpc_err(INVALID_PARAMS, format!("Unknown tool: {name}")));
                }
                let out = self.isolated(|server| match name {
                    "nyra_spec" => spec(args),
                    "nyra_check" => check(server, args),
                    "nyra_test" => test(server, args),
                    "nyra_run" => server.run_tool(args),
                    "nyra_explain" => explain_tool(args),
                    "nyra_build" => build(server, args),
                    _ => crate::edit::tool(name, args, &server.dir),
                });
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

    /// Runs one tool call on a worker thread with a big stack (see `crate::STACK`). A panic (a bug
    /// in nyra, or input that finds one) becomes the call's error: one bad request never stops the
    /// server.
    fn isolated(&mut self, call: impl FnOnce(&mut Server) -> Result<String, String> + Send) -> Result<String, String> {
        std::thread::scope(|s| {
            let worker = std::thread::Builder::new().name("nyra-tool".into()).stack_size(crate::STACK).spawn_scoped(s, || call(self));
            match worker {
                Ok(h) => h.join().unwrap_or_else(|panic| {
                    let what = panic
                        .downcast_ref::<&str>()
                        .map(|s| s.to_string())
                        .or_else(|| panic.downcast_ref::<String>().cloned())
                        .unwrap_or_else(|| "unknown".to_string());
                    Err(tool_error(format!("internal error in nyra ({what}); the server is still running: please report this input")))
                }),
                Err(e) => Err(tool_error(format!("cannot start a worker thread: {e}"))),
            }
        })
    }

    // ---- nyra_run ---------------------------------------------------------------------------------

    fn run_tool(&mut self, args: &Json) -> Result<String, String> {
        let program = self.program(args)?;
        let strict = arg_bool(args, "strict")?;
        let target = match optional_str(args, "backend")? {
            None | Some("native") => Target::Native,
            Some("js") => Target::Js,
            Some(other) => return Err(tool_error(format!("backend must be \"native\" or \"js\", found {other:?}"))),
        };
        let stdin = optional_str(args, "stdin")?.unwrap_or("");
        let grant = grant_of(args)?;
        if matches!(args.get("sandbox"), Some(Json::Bool(true))) {
            return sandboxed(args, &program, stdin, &grant, strict);
        }
        let timeout = match args.get("timeout_ms") {
            None | Some(Json::Null) => TIMEOUT_MS,
            Some(t) => match t.as_f64() {
                Some(ms) if ms >= 1.0 && ms <= MAX_TIMEOUT_MS as f64 => ms as u64,
                _ => return Err(tool_error(format!("timeout_ms must be a number from 1 to {MAX_TIMEOUT_MS}"))),
            },
        };

        let start = Instant::now();
        let c = match compile_tool(&program, &grant, strict, true) {
            Ok(c) => c,
            Err(diags) => return Ok(diag::render_json(&diags, &program.name)),
        };
        perfwarn::check(&c.prog, false);
        let applied = c.applied;
        let module = crate::lower(&c.prog).map_err(tool_error)?;
        let source = crate::emit(&module, target, &program.name);
        let compile_ms = ms(start.elapsed());
        let file = program.name.clone();

        // Auto mode, as in `nyra run` (see auto.rs): a program that has no build yet starts in the
        // interpreter while the C compiler works in the background; one that finishes within the
        // budget is answered at once, the others wait for the compiler and run natively.
        let mut pending = None;
        if target == Target::Native {
            if let Some(compiler) = self.cc.get_or_insert_with(crate::find_cc).clone() {
                let cached = crate::cc_lookup(&compiler, &source, "main", &file, &self.dir, crate::Opt::Fast).is_some();
                if !cached && !crate::auto::effects(&module).world {
                    let job = crate::auto::Job {
                        compiler,
                        code: source.clone(),
                        stem: "main".into(),
                        source: file.clone(),
                        dir: self.dir.clone(),
                        opt: crate::Opt::Fast,
                        capture: true,
                    };
                    let build = crate::auto::Build::spawn(job, crate::auto::DELAY);
                    match crate::auto::attempt(&module, stdin.as_bytes().to_vec(), Vec::new(), SANDBOX_MEMORY, STDOUT_CAP as u64) {
                        Some(report) => {
                            build.cancel();
                            return Ok(interpreted_json(
                                &report,
                                compile_ms,
                                Some("interp"),
                                &file,
                                warnings_value(&program, &applied),
                            ));
                        }
                        None => pending = Some(build),
                    }
                }
            }
        }

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
            let (exe, t) = match pending.take() {
                Some(build) => build.finish().map_err(tool_error)?,
                None => crate::cc_cached(&compiler, &source, "main", &file, &self.dir, crate::Opt::Fast, true).map_err(tool_error)?,
            };
            cc_ms = Some(t);
            Command::new(exe)
        };
        cmd.current_dir(&self.dir).env("NYRA_JSON", "1");
        let ran = execute(cmd, stdin, Duration::from_millis(timeout), target == Target::Js).map_err(|e| {
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

        // Node.js ends a program that hits the memory limit with a dump of its heap: report the
        // out-of-memory error that the native runtime reports
        if errors.is_empty() && ran.exit != Some(0) && rest.to_ascii_lowercase().contains("out of memory") {
            let msg = format!("out of memory: the program needs more than the {} MiB nyra_run gives it", crate::limits::MEMORY >> 20);
            errors.push(obj([
                ("code", "E0249".into()),
                ("message", msg.into()),
                ("file", file.as_str().into()),
                ("line", Json::from(0)),
                ("col", Json::from(0)),
                ("hint", "the program needs more memory than the system gave it".into()),
                ("runtime", true.into()),
            ]));
            rest.clear();
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
        if let Some(w) = warnings_value(&program, &applied) {
            fields.push(("warnings", w));
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

/// The names of the tools (as in `TOOLS`).
const TOOL_NAMES: &[&str] =
    &["nyra_spec", "nyra_check", "nyra_test", "nyra_run", "nyra_explain", "nyra_build", "nyra_outline", "nyra_show", "nyra_edit"];

fn tools() -> Json {
    let mut tools = Json::parse(TOOLS).expect("TOOLS is valid JSON");
    if let (Json::Arr(all), Ok(Json::Arr(more))) = (&mut tools, Json::parse(crate::edit::TOOLS)) {
        all.extend(more);
    }
    tools
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
        res(
            "nyra://card",
            "card",
            "Nyra agent card",
            "The compact spec for agents (about 1,400 tokens): an example program, the rules that differ from other languages, what is not in Nyra, every method and module name",
            "text/markdown",
            Some(card().len()),
        ),
        res(
            "nyra://spec",
            "spec",
            "Nyra language spec",
            "The complete language: types, operators, builtins, methods, structs, memory, runtime errors",
            "text/markdown",
            Some(SPEC.len()),
        ),
        res(
            "nyra://guide",
            "guide",
            "Nyra guide for AI agents",
            "Workflow, do/don't rules, what does not exist yet, error codes with fixes, recipes, complete programs",
            "text/markdown",
            Some(GUIDE.len()),
        ),
        res(
            "nyra://errors",
            "errors",
            "Nyra error index",
            "Every error code with its title, kind and version (JSON); nyra://errors/E0201 reads one entry",
            "application/json",
            None,
        ),
    ]
    .into()
}

fn read_resource(uri: &str) -> Option<(&'static str, String)> {
    match uri {
        "nyra://card" => Some(("text/markdown", card().to_string())),
        "nyra://spec" => Some(("text/markdown", SPEC.to_string())),
        "nyra://guide" => Some(("text/markdown", GUIDE.to_string())),
        "nyra://errors" => Some(("application/json", explain::list_json(&explain::database().ok()?, false))),
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

fn spec(args: &Json) -> Result<String, String> {
    let full = match args.get("full") {
        None | Some(Json::Null) => false,
        Some(Json::Bool(b)) => *b,
        Some(_) => return Err(tool_error("argument `full` must be a boolean")),
    };
    match optional_str(args, "part")? {
        None | Some("spec") if full => Ok(SPEC.to_string()),
        None | Some("spec") => Ok(card().to_string()),
        Some("guide") => Ok(GUIDE.to_string()),
        Some(other) => Err(tool_error(format!("part must be \"spec\" or \"guide\", found {other:?}"))),
    }
}

/// The capabilities a call grants: its `allow` list, else only standard input.
fn grant_of(args: &Json) -> Result<caps::Grant, String> {
    let mut grant = caps::Grant::none();
    match args.get("allow") {
        None | Some(Json::Null) => grant.allow("input").map_err(tool_error)?,
        Some(Json::Arr(items)) => {
            for item in items {
                let name = item.as_str().ok_or_else(|| tool_error("argument `allow` must be a list of capability names"))?;
                grant.allow(name).map_err(tool_error)?;
            }
        }
        Some(_) => return Err(tool_error("argument `allow` must be a list of capability names, e.g. [\"fs\"]")),
    }
    Ok(grant)
}

/// How to grant a capability, for the hint of E0290.
fn allow_flag(cap: &str) -> String {
    format!("grant it by passing allow: [\"{cap}\"] to the tool")
}

fn arg_bool(args: &Json, key: &str) -> Result<bool, String> {
    match args.get(key) {
        None | Some(Json::Null) => Ok(false),
        Some(Json::Bool(b)) => Ok(*b),
        Some(_) => Err(tool_error(format!("argument `{key}` must be true or false"))),
    }
}

// ---- the program of a call -------------------------------------------------------------------------

/// The most files, and the most text, a call may send.
const MAX_FILES: usize = 100;
const MAX_FILES_BYTES: usize = 4 << 20;

/// The program a tool call works on: the text of the file that is compiled (`src`), the name that
/// messages call it, and, for a program of several files, the folder they were written to (removed
/// when this value is dropped).
pub(crate) struct Program {
    pub(crate) src: String,
    pub(crate) name: String,
    root: Option<Root>,
}

struct Root {
    dir: PathBuf,
    entry: PathBuf,
}

impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl Program {
    /// Tells the loader where the imports of the file are (call it on the thread that compiles).
    pub(crate) fn enter(&self) {
        if let Some(r) = &self.root {
            modules::set_main(Some(&r.entry));
        }
    }

    /// The files named in `diags` are shown relative to the program's folder.
    fn localize(&self, diags: &mut [diag::Diag]) {
        let Some(r) = &self.root else { return };
        let prefix = format!("{}/", r.dir.display().to_string().replace('\\', "/"));
        for d in diags {
            if let Some(f) = d.file.as_mut() {
                let shown = f.replace('\\', "/");
                *f = shown.strip_prefix(&prefix).unwrap_or(&shown).to_string();
            }
            d.msg = d.msg.replace(&prefix, "");
            if let Some(h) = d.hint.as_mut() {
                *h = h.replace(&prefix, "");
            }
        }
    }
}

/// A file name of a program given as `files`: relative, with `/`, ending in `.nyra`.
fn valid_file_name(name: &str) -> bool {
    name.ends_with(".nyra")
        && !name.starts_with('/')
        && !name.contains(['\\', ':', '\0'])
        && name.split('/').all(|part| !part.is_empty() && part != "." && part != "..")
}

impl Server {
    /// The program of a call: `code`, or `files` (written to a folder of their own, so that
    /// `use ./name` finds them).
    fn program(&self, args: &Json) -> Result<Program, String> {
        program_in(&self.dir, args, optional_str(args, "entry")?)
    }
}

/// The program of a call, written (for `files`) to a new folder below `dir`. `entry` names the file
/// of `files` that is compiled; without it that is `main.nyra`, or the only file.
pub(crate) fn program_in(dir: &std::path::Path, args: &Json, entry: Option<&str>) -> Result<Program, String> {
    {
        let code = optional_str(args, "code")?;
        let files = match args.get("files") {
            None | Some(Json::Null) => None,
            Some(Json::Obj(items)) => Some(items),
            Some(_) => return Err(tool_error("argument `files` must be an object: file name -> text")),
        };
        let items = match (code, files) {
            (Some(_), Some(_)) => return Err(tool_error("give either `code` or `files`, not both")),
            (None, None) => return Err(tool_error("missing argument `code` (the program) or `files` (a program of several files)")),
            (Some(code), None) => return Ok(Program { src: code.to_string(), name: FILE.to_string(), root: None }),
            (None, Some(items)) => items,
        };
        if items.is_empty() {
            return Err(tool_error("`files` is empty"));
        }
        if items.len() > MAX_FILES {
            return Err(tool_error(format!("`files` has {} files: at most {MAX_FILES}", items.len())));
        }
        let mut files: Vec<(&str, &str)> = Vec::new();
        for (name, text) in items {
            let Json::Str(text) = text else {
                return Err(tool_error(format!("the text of `{name}` in `files` must be a string")));
            };
            if !valid_file_name(name) {
                return Err(tool_error(format!(
                    "`{name}` is not a file name Nyra imports: write a relative name with `/` that ends in .nyra, e.g. \"shapes.nyra\" or \"util/text.nyra\""
                )));
            }
            files.push((name, text));
        }
        if files.iter().map(|(_, t)| t.len()).sum::<usize>() > MAX_FILES_BYTES {
            return Err(tool_error(format!("`files` holds more than {} MiB of text", MAX_FILES_BYTES >> 20)));
        }
        let names: Vec<&str> = files.iter().map(|(n, _)| *n).collect();
        let entry = match entry {
            Some(e) => e,
            None if names.contains(&"main.nyra") => "main.nyra",
            None if names.len() == 1 => names[0],
            None => {
                return Err(tool_error(format!(
                    "`files` has no main.nyra: name the file to run with `entry` (the files are {})",
                    names.join(", ")
                )))
            }
        };
        let Some((_, src)) = files.iter().find(|(n, _)| *n == entry) else {
            return Err(tool_error(format!("`entry` is `{entry}`, which is not one of the files ({})", names.join(", "))));
        };
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let dir = dir.join(format!("proj-{}", NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&dir);
        // the folder is removed when `root` is dropped, also when writing a file fails
        let root = Root { entry: dir.join(entry), dir };
        for (name, text) in &files {
            let path = root.dir.join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| tool_error(format!("cannot write the files: {e}")))?;
            }
            std::fs::write(&path, text).map_err(|e| tool_error(format!("cannot write `{name}`: {e}")))?;
        }
        Ok(Program { src: src.to_string(), name: entry.to_string(), root: Some(root) })
    }
}

// ---- compiling with repairs -----------------------------------------------------------------------------

/// A program that compiled, and the repairs it needed.
struct Compiled {
    prog: ast::Program,
    applied: Vec<fix::Applied>,
}

/// Compiles `program` like the CLI: an error with exactly one certain fix is repaired in memory, and
/// each repair is reported (unless `strict`). With `full` the examples run too; without it only the
/// front half runs (`nyra_test` runs the examples itself). The diagnostics name files relative to
/// the program's folder.
fn compile_tool(program: &Program, grant: &caps::Grant, strict: bool, full: bool) -> Result<Compiled, Vec<diag::Diag>> {
    program.enter();
    let build = |s: &str| -> Result<ast::Program, Vec<diag::Diag>> {
        if full {
            return crate::compile_granted(s, grant, &allow_flag);
        }
        let prog = crate::front(s)?;
        let errs = caps::enforce(&prog, grant, allow_flag);
        if errs.is_empty() {
            Ok(prog)
        } else {
            Err(errs)
        }
    };
    let result = match build(&program.src) {
        Ok(prog) => Ok(Compiled { prog, applied: Vec::new() }),
        Err(diags) if strict => Err(diags),
        Err(diags) => match fix::repair(&program.src, diags.clone(), build) {
            Some(r) => Ok(Compiled { prog: r.value, applied: r.applied }),
            None => Err(diags),
        },
    };
    result.map_err(|mut diags| {
        program.localize(&mut diags);
        diags
    })
}

/// The `warnings` of a call: the warnings of the compiler (`${x}` in a string, slow patterns) and the
/// repairs that were applied. `None` when there are none.
fn warnings_value(program: &Program, applied: &[fix::Applied]) -> Option<Json> {
    let mut w = diag::warnings();
    program.localize(&mut w);
    let mut items: Vec<Json> = Vec::new();
    if !w.is_empty() {
        items.extend(
            Json::parse(&diag::render_json_errors(&w, &program.name))
                .ok()
                .and_then(|j| j.as_array().map(<[Json]>::to_vec))
                .unwrap_or_default(),
        );
    }
    if !applied.is_empty() {
        items.extend(
            Json::parse(&diag::render_json_warnings(applied, &program.name))
                .ok()
                .and_then(|j| j.as_array().map(<[Json]>::to_vec))
                .unwrap_or_default(),
        );
    }
    (!items.is_empty()).then_some(Json::Arr(items))
}

/// `{"ok":true,"errors":[],"warnings":[..]}` for a program that compiled.
fn ok_json(program: &Program, applied: &[fix::Applied]) -> String {
    let mut fields = vec![("ok", Json::from(true)), ("errors", Json::Arr(Vec::new()))];
    if let Some(w) = warnings_value(program, applied) {
        fields.push(("warnings", w));
    }
    Json::Obj(fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect()).to_string()
}

fn check(server: &Server, args: &Json) -> Result<String, String> {
    let program = server.program(args)?;
    let grant = grant_of(args)?;
    Ok(match compile_tool(&program, &grant, arg_bool(args, "strict")?, true) {
        Ok(c) => {
            perfwarn::check(&c.prog, false);
            ok_json(&program, &c.applied)
        }
        Err(diags) => diag::render_json(&diags, &program.name),
    })
}

/// The examples of a program: the JSON of `nyra test --json`.
fn test(server: &Server, args: &Json) -> Result<String, String> {
    let program = server.program(args)?;
    let grant = grant_of(args)?;
    Ok(match compile_tool(&program, &grant, arg_bool(args, "strict")?, false) {
        Ok(mut c) => {
            perfwarn::check(&c.prog, false);
            let mut json = examples::json(&examples::run(&mut c.prog), &program.name);
            if let Some(w) = warnings_value(&program, &c.applied) {
                if json.ends_with('}') {
                    json.pop();
                    json.push_str(&format!(",\"warnings\":{w}}}"));
                }
            }
            json
        }
        Err(diags) => diag::render_json(&diags, &program.name),
    })
}

/// Compiles to C or JS. The inner `Err` is the diagnostics JSON (a normal tool result);
/// the outer one is a failure of the compiler itself. The inner `Ok` has the source and the repairs.
fn generate(program: &Program, target: Target, strict: bool) -> Result<Result<(String, Vec<fix::Applied>), String>, String> {
    generate_granted(program, target, &caps::Grant::all(), strict)
}

/// `generate`, for a program that may use only what `grant` gives (else E0290).
fn generate_granted(
    program: &Program,
    target: Target,
    grant: &caps::Grant,
    strict: bool,
) -> Result<Result<(String, Vec<fix::Applied>), String>, String> {
    let c = match compile_tool(program, grant, strict, true) {
        Ok(c) => c,
        Err(diags) => return Ok(Err(diag::render_json(&diags, &program.name))),
    };
    perfwarn::check(&c.prog, false);
    crate::generate(&c.prog, target, &program.name).map(|source| Ok((source, c.applied))).map_err(tool_error)
}

/// `nyra_run` with `sandbox: true`: the program runs in the interpreter, in this process, under
/// limits of steps, memory and output that stop it at the same place on every machine.
fn sandboxed(args: &Json, program: &Program, stdin: &str, grant: &caps::Grant, strict: bool) -> Result<String, String> {
    let number = |key: &str, default: u64| -> Result<u64, String> {
        match args.get(key) {
            None | Some(Json::Null) => Ok(default),
            Some(v) => match v.as_f64() {
                Some(n) if n >= 1.0 && n.fract() == 0.0 && n < 1.8e19 => Ok(n as u64),
                _ => Err(tool_error(format!("`{key}` must be a whole number from 1"))),
            },
        }
    };
    let mut cfg = sandbox::Config::new();
    cfg.limits.steps = number("fuel", SANDBOX_FUEL)?;
    cfg.limits.memory = number("max_memory", SANDBOX_MEMORY)?;
    cfg.limits.output = number("max_output", STDOUT_CAP as u64)?.min(STDOUT_CAP as u64);
    cfg.limits.wall_ms = number("timeout_ms", TIMEOUT_MS)?.min(MAX_TIMEOUT_MS);
    cfg.confined = true;
    cfg.to_stdout = false;
    cfg.stdin = Some(stdin.as_bytes().to_vec());
    if let Some(list) = args.get("args").filter(|a| !matches!(a, Json::Null)) {
        let items = list.as_array().ok_or_else(|| tool_error("argument `args` must be a list of strings"))?;
        for item in items {
            cfg.args.push(item.as_str().ok_or_else(|| tool_error("argument `args` must be a list of strings"))?.to_string());
        }
    }
    let start = Instant::now();
    let c = match compile_tool(program, grant, strict, true) {
        Ok(c) => c,
        Err(diags) => return Ok(diag::render_json(&diags, &program.name)),
    };
    perfwarn::check(&c.prog, false);
    let module = sandbox::module(&c.prog).map_err(tool_error)?;
    let compile_ms = ms(start.elapsed());
    let report = sandbox::run(&module, cfg);
    Ok(interpreted_json(&report, compile_ms, None, &program.name, warnings_value(program, &c.applied)))
}

/// The result of a program that ran in the interpreter: `ok`, `exit`, `stdout`, its runtime error,
/// the steps and the times. `mode` says how a run that was not asked to be interpreted got here.
fn interpreted_json(report: &sandbox::Report, compile_ms: Json, mode: Option<&str>, file: &str, warnings: Option<Json>) -> String {
    let mut fields =
        vec![("ok", Json::from(report.exit == 0)), ("exit", Json::from(report.exit as i64)), ("stdout", report.stdout.clone().into())];
    if let Some(what) = &report.internal {
        fields.push(("stderr", format!("internal error in the interpreter: {what}").into()));
    }
    if let Some(e) = &report.error {
        let err = obj([
            ("code", e.code.into()),
            ("message", e.msg.clone().into()),
            ("file", file.into()),
            ("line", Json::from(e.span.line as i64)),
            ("col", Json::from(e.span.col as i64)),
            ("hint", e.hint.into()),
            ("runtime", true.into()),
        ]);
        fields.push(("errors", vec![err].into()));
        match e.code {
            "E0357" => fields.push(("truncated", true.into())),
            "E0359" => fields.push(("timeout", true.into())),
            _ => {}
        }
    }
    if let Some(w) = warnings {
        fields.push(("warnings", w));
    }
    fields.push(("steps", Json::from(report.steps as i64)));
    if let Some(mode) = mode {
        fields.push(("mode", mode.into()));
    }
    let times = vec![("compile", compile_ms), ("run", Json::fixed(report.run_ms, 1))];
    fields.push(("ms", Json::Obj(times.into_iter().map(|(k, v)| (k.to_string(), v)).collect())));
    Json::Obj(fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect()).to_string()
}

fn build(server: &Server, args: &Json) -> Result<String, String> {
    let program = server.program(args)?;
    let (target, name) = match optional_str(args, "target")? {
        None | Some("c") => (Target::C, "c"),
        Some("js") => (Target::Js, "js"),
        Some(other) => return Err(tool_error(format!("target must be \"c\" or \"js\", found {other:?}"))),
    };
    Ok(match generate(&program, target, arg_bool(args, "strict")?)? {
        Ok((source, applied)) => {
            let mut fields = vec![("ok", Json::from(true)), ("target", name.into()), ("source", source.into())];
            if let Some(w) = warnings_value(&program, &applied) {
                fields.push(("warnings", w));
            }
            Json::Obj(fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect()).to_string()
        }
        Err(diags) => diags,
    })
}

fn explain_tool(args: &Json) -> Result<String, String> {
    let all = explain::database().map_err(tool_error)?;
    let Some(arg) = optional_str(args, "code")? else {
        let planned = matches!(args.get("planned"), Some(Json::Bool(true)));
        return Ok(explain::list_json(&all, planned));
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
/// The program gets at most `limits::MEMORY` of memory and a CPU-time budget (`node`: it runs
/// on Node.js, see `limits`).
fn execute(mut cmd: Command, stdin: &str, timeout: Duration, node: bool) -> std::io::Result<Ran> {
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let cpu = crate::limits::cpu_seconds(timeout.as_millis() as u64);
    crate::limits::before_spawn(&mut cmd, cpu, node);
    let start = Instant::now();
    let mut child = cmd.spawn()?;
    let limited = crate::limits::after_spawn(&child, cpu);
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
    // Windows: closing the job here kills anything the program left running. Elsewhere `Limited`
    // holds nothing, so the drop does nothing there.
    #[allow(clippy::drop_non_drop)]
    drop(limited);
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
        let names: Vec<&str> = tools.as_array().unwrap().iter().map(|t| t.get("name").and_then(Json::as_str).unwrap()).collect();
        assert_eq!(
            names,
            [
                "nyra_spec",
                "nyra_check",
                "nyra_test",
                "nyra_run",
                "nyra_explain",
                "nyra_build",
                "nyra_outline",
                "nyra_show",
                "nyra_edit"
            ]
        );
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
        for uri in ["nyra://card", "nyra://spec", "nyra://guide", "nyra://errors", "nyra://errors/E0201", "nyra://errors/e201"] {
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
