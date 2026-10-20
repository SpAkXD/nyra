//! `nyra serve --mcp tools.nyra`: the `pub fn`s of a Nyra file as the tools of an MCP server.
//!
//! - **Which functions.** Every `pub fn` of the file is a tool, under its own name (`main`, names
//!   that start with `_` and the functions of imported files are not). Its description is the
//!   `//` comment right above it, followed by its Nyra signature. A file that marks no function
//!   `pub` serves no tool (the server says so when it starts). A tools file holds functions,
//!   structs and enums, no top-level statements; it needs no `fn main`.
//! - **Schemas.** The arguments are an object with one property per parameter, its JSON Schema
//!   from the parameter's type (`schema.rs`); a parameter with a default value is optional. The
//!   result, if the function returns one, has an `outputSchema` too: the value is in
//!   `structuredContent.result`, and its JSON text in the text content. An `inout` parameter
//!   cannot come from JSON: such a function is refused when the server starts.
//! - **Running.** Each call runs in the interpreter (`ir/interp.rs`) with fresh limits (`--fuel`,
//!   `--max-memory`, `--max-output`, `--max-depth`, `--timeout`), file paths confined to the
//!   working folder, and only the capabilities of `--allow` (or the mocks of `--mock`). A tool
//!   whose code uses a capability the server does not grant fails with E0290 before it runs.
//! - **Errors.** A wrong argument is E0345 (the JSON does not have the shape of the type), a
//!   runtime error or a limit has its own code; each comes back as a tool error (`isError`) whose
//!   text is `{"ok":false,"errors":[{code,message,file,line,col,hint}]}`, as `nyra run --json`
//!   writes them. What a tool prints is returned as a second text content.
//!
//! `nyra serve --mcp --list tools.nyra` prints the tools (the `tools/list` answer) and exits.

use std::process::ExitCode;

use crate::ast::{self, ExprKind, Span, UnOp};
use crate::caps;
use crate::ir::host::{Host, Out};
use crate::ir::interp::{Interp, Limits, Stop};
use crate::ir::{self, FuncId, Module, RtOp, Stmt, StmtKind, Ty};
use crate::json::{obj, Json};
use crate::mcp::{self, rpc_err, Rpc, RpcError};
use crate::schema::{Gen, Mode};

const USAGE: &str = "\
usage: nyra serve --mcp <tools.nyra> [options]
       nyra serve --mcp --list <tools.nyra>

Serves the `pub fn`s of a Nyra file as the tools of an MCP server on stdin/stdout (newline-delimited
JSON-RPC 2.0). Each tool's description is the `//` comment above it; its arguments and result have
JSON Schemas made from the Nyra types. Each call runs in the sandboxed interpreter with fresh limits.

  --list           print the tools (name, description, inputSchema, outputSchema) and exit
  --allow LIST     the capabilities the tools get: fs, input, os (default: none)
  --mock M         fs=DIR, input=FILE or os.env=KEY=VALUE: a capability served by a mock
  --fuel N         steps per call (default 200,000,000)                    E0355
  --max-memory S   heap per call (default 256M)                            E0356
  --max-output S   bytes a call may print (default 1M)                     E0357
  --max-depth N    nested calls (default 20,000)                           E0358
  --timeout MS     real time per call (default 10000)                      E0359

Claude Code: claude mcp add mytools -- nyra serve --mcp /path/to/tools.nyra
See docs/SERVE.md.
";

/// One tool: a `pub fn`.
struct Tool {
    name: String,
    func: FuncId,
    description: String,
    /// Name, type and default value (JSON text) of each parameter.
    params: Vec<(String, Ty, Option<String>)>,
    ret: Option<Ty>,
    /// The capabilities its code needs, sorted.
    caps: Vec<&'static str>,
    span: Span,
}

/// The tools of a file, ready to be called.
pub struct Tools {
    file: String,
    stem: String,
    m: Module,
    tools: Vec<Tool>,
    grant: caps::Grant,
    limits: Limits,
    mocks: crate::mock::Mocks,
    /// The overlay of `--mock fs=DIR`: it lives as long as the server, so a call sees what an
    /// earlier call wrote.
    mock_fs: Option<crate::mock::MockFs>,
}

/// A default value written in the source (a constant) as JSON text; `None` for anything else.
fn literal_json(e: &ast::Expr) -> Option<String> {
    Some(match &e.kind {
        ExprKind::Int(n) => n.to_string(),
        ExprKind::Float(x) if x.is_finite() => crate::ir::interp::num(*x),
        ExprKind::Bool(b) => b.to_string(),
        ExprKind::Str(s) => crate::diag::json_str(s),
        ExprKind::Char(c) => crate::diag::json_str(&char::from_u32(*c)?.to_string()),
        ExprKind::None => "null".to_string(),
        ExprKind::Some(x) => literal_json(x)?,
        ExprKind::Unary(UnOp::Neg, x) => match &x.kind {
            ExprKind::Int(n) => format!("-{n}"),
            ExprKind::Float(f) => format!("-{}", crate::ir::interp::num(*f)),
            _ => return None,
        },
        ExprKind::Array(xs) | ExprKind::Tuple(xs) => {
            format!("[{}]", xs.iter().map(literal_json).collect::<Option<Vec<_>>>()?.join(","))
        }
        _ => return None,
    })
}

/// The capabilities the code that `f` runs needs (its own and that of every function it calls).
fn needs(m: &Module, f: FuncId) -> Vec<&'static str> {
    fn walk(ss: &[Stmt], calls: &mut Vec<FuncId>, mods: &mut Vec<&'static str>) {
        for s in ss {
            match &s.kind {
                StmtKind::Call { func, .. } => calls.push(*func),
                StmtKind::Op { op: RtOp::Std(f), .. } => mods.push(f.path().0),
                StmtKind::If { then, els, .. } => {
                    walk(then, calls, mods);
                    walk(els, calls, mods);
                }
                StmtKind::Loop { head, body, step, .. } => {
                    walk(head, calls, mods);
                    walk(body, calls, mods);
                    walk(step, calls, mods);
                }
                StmtKind::ForEach { body, .. } => walk(body, calls, mods),
                _ => {}
            }
        }
    }
    let (mut seen, mut todo, mut mods) = (Vec::new(), vec![f], Vec::new());
    while let Some(id) = todo.pop() {
        if seen.contains(&id) {
            continue;
        }
        seen.push(id);
        if let Some(func) = m.funcs.get(id.0 as usize) {
            walk(&func.body, &mut todo, &mut mods);
        }
    }
    let mut caps: Vec<&'static str> = mods.into_iter().filter_map(caps::needed).collect();
    caps.sort();
    caps.dedup();
    caps
}

impl Tools {
    /// Compiles a tools file. Errors are printed (exit code in `Err`).
    pub fn load(file: &str) -> Result<Tools, ExitCode> {
        Tools::load_with(file, caps::Grant::none(), crate::mock::Mocks::default())
    }

    fn load_with(file: &str, grant: caps::Grant, mocks: crate::mock::Mocks) -> Result<Tools, ExitCode> {
        let src = std::fs::read_to_string(file).map_err(|e| crate::fail(format!("cannot read `{file}`: {e}")))?;
        let (toks, _) = crate::lexer::lex(&src);
        let (shape, _) = crate::parser::parse(toks);
        if shape.script {
            return Err(crate::fail(format!(
                "{file} has statements at the top level: a tools file holds functions (`pub fn` for each tool), structs and enums only"
            )));
        }
        // a tools file needs no `main`: an empty one makes it a program
        let extra = if shape.funcs.iter().any(|f| f.name == "main") { "" } else { "\nfn main() {\n}\n" };
        let (_, prog) = crate::schema::compile_file(file, extra)?;
        // (not optimized: the optimizer drops the functions that `main` does not call)
        let m = ir::lower::lower(&prog).map_err(|what| crate::fail(format!("not supported yet: {what}")))?;
        if let Err(e) = ir::verify::verify(&m) {
            return Err(crate::fail(format!("internal error: the compiler produced invalid IR ({e}); please report this bug")));
        }
        let outline = crate::edit::outline(&src);
        let mut tools = Vec::new();
        for f in prog.funcs.iter().filter(|f| prog.public.contains(&f.name) && f.name != "main" && !f.name.starts_with('_')) {
            if prog.files.contains_key(&f.name) || f.name.contains('.') {
                continue;
            }
            if let Some(p) = f.params.iter().find(|p| p.inout) {
                return Err(crate::fail(format!(
                    "{file}:{}:{}: `pub fn {}` cannot be a tool: its parameter `inout {}` changes the caller's variable, and a tool's arguments come from JSON (make it a plain parameter and return the new value)",
                    f.span.line, f.span.col, f.name, p.name
                )));
            }
            let Some(id) = m.funcs.iter().position(|g| g.name == f.name) else { continue };
            let item = outline.items.iter().find(|i| i.name == f.name && i.kind == crate::edit::Kind::Fn);
            let doc = item
                .map(|i| {
                    src.get(i.doc..i.start)
                        .unwrap_or("")
                        .lines()
                        .map(|l| l.trim().trim_start_matches("//").trim())
                        .filter(|l| !l.is_empty())
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_default();
            let sig = item.map(|i| i.sig.clone()).unwrap_or_default();
            let description = if doc.is_empty() { format!("Nyra: `{sig}`") } else { format!("{doc}\n\nNyra: `{sig}`") };
            let func = &m.funcs[id];
            let params = f
                .params
                .iter()
                .enumerate()
                .map(|(k, p)| (p.name.clone(), func.locals[k].ty, p.default.as_ref().and_then(literal_json)))
                .collect();
            let fid = FuncId(id as u32);
            tools.push(Tool {
                name: f.name.clone(),
                func: fid,
                description,
                params,
                ret: func.ret,
                caps: needs(&m, fid),
                span: f.span,
            });
        }
        let stem = std::path::Path::new(file).file_stem().and_then(|s| s.to_str()).unwrap_or("tools").to_string();
        let limits =
            Limits { steps: 200_000_000, depth: crate::sandbox::DEFAULT_DEPTH, memory: 256 << 20, output: 1 << 20, wall_ms: 10_000 };
        let mock_fs = mocks.fs.clone().map(crate::mock::MockFs::new);
        Ok(Tools { file: file.to_string(), stem, m, tools, grant, limits, mocks, mock_fs })
    }

    fn tool_json(&self, t: &Tool) -> Json {
        let mut input = Gen::new(&self.m, Mode::Input);
        let mut props = Vec::new();
        let mut required = Vec::new();
        for (name, ty, default) in &t.params {
            let mut s = input.schema(*ty);
            match default.as_deref().and_then(|d| Json::parse(d).ok()) {
                Some(d) => {
                    if let Json::Obj(fields) = &mut s {
                        fields.push(("default".to_string(), d));
                    }
                }
                None => required.push(Json::from(name.as_str())),
            }
            props.push((name.clone(), s));
        }
        let mut schema = vec![
            ("type".to_string(), Json::from("object")),
            ("properties".to_string(), Json::Obj(props)),
            ("required".to_string(), Json::Arr(required)),
            ("additionalProperties".to_string(), Json::Bool(false)),
        ];
        if !input.defs().is_empty() {
            schema.push(("$defs".to_string(), Json::Obj(input.defs())));
        }
        let mut description = t.description.clone();
        if !t.caps.is_empty() {
            description.push_str(&format!("\nNeeds the capabilities: {}.", t.caps.join(", ")));
        }
        let mut fields = vec![
            ("name".to_string(), Json::from(t.name.as_str())),
            ("description".to_string(), Json::from(description)),
            ("inputSchema".to_string(), Json::Obj(schema)),
        ];
        if let Some(ret) = t.ret {
            let mut output = Gen::new(&self.m, Mode::Output);
            let result = output.schema(ret);
            let mut out = vec![
                ("type".to_string(), Json::from("object")),
                ("properties".to_string(), Json::Obj(vec![("result".to_string(), result)])),
                ("required".to_string(), Json::Arr(vec!["result".into()])),
            ];
            if !output.defs().is_empty() {
                out.push(("$defs".to_string(), Json::Obj(output.defs())));
            }
            fields.push(("outputSchema".to_string(), Json::Obj(out)));
        }
        Json::Obj(fields)
    }

    /// The tools as `tools/list` answers.
    pub fn manifest(&self) -> Json {
        obj([("tools", Json::Arr(self.tools.iter().map(|t| self.tool_json(t)).collect()))])
    }

    /// An error as `nyra run --json` writes it.
    fn error(&self, code: &str, msg: &str, span: Span, hint: &str) -> String {
        obj([
            ("ok", false.into()),
            (
                "errors",
                Json::Arr(vec![obj([
                    ("code", code.into()),
                    ("message", msg.into()),
                    ("file", self.file.as_str().into()),
                    ("line", Json::from(span.line as i64)),
                    ("col", Json::from(span.col as i64)),
                    ("hint", hint.into()),
                ])]),
            ),
        ])
        .to_string()
    }

    /// One call: the result (`Ok`: content and structured content) or the tool error's text.
    fn call(&mut self, name: &str, args: &Json) -> Result<(Vec<Json>, Option<Json>), String> {
        let Some(k) = self.tools.iter().position(|t| t.name == name) else {
            return Err(self.error(
                "E0345",
                &format!("no tool `{name}`"),
                Span { line: 0, col: 0 },
                "call one of the tools of tools/list",
            ));
        };
        let t = &self.tools[k];
        let (func, ret, span, params) = (t.func, t.ret, t.span, t.params.clone());
        if let Some(cap) = t.caps.iter().find(|c| !self.grant.allows(c)) {
            let granted = self.grant.names();
            let now = if granted.is_empty() { "it grants none".to_string() } else { format!("it grants {}", granted.join(", ")) };
            return Err(self.error(
                "E0290",
                &format!("tool `{name}` needs the capability `{cap}`, which this server does not grant ({now})"),
                t.span,
                &format!(
                    "start the server with `--allow {cap}` (or a `--mock` of it), or change the tool so it does not need `{cap}`"
                ),
            ));
        }
        // the arguments: each one read from its JSON by its type, as `json.parse` reads
        let Json::Obj(given) = args else { return Err(self.error("E0345", "the arguments are not an object", span, "")) };
        if let Some((extra, _)) = given.iter().find(|(k, _)| !params.iter().any(|(p, _, _)| p == k)) {
            let names: Vec<&str> = params.iter().map(|(p, _, _)| p.as_str()).collect();
            return Err(self.error(
                "E0345",
                &format!("tool `{name}` has no parameter `{extra}`"),
                span,
                &format!("its parameters are: {}", if names.is_empty() { "none".to_string() } else { names.join(", ") }),
            ));
        }
        let mut values = Vec::new();
        for (p, ty, default) in &params {
            let text = match (args.get(p), default) {
                (Some(v), _) => v.to_string(),
                (None, Some(d)) => d.clone(),
                (None, None) => {
                    return Err(self.error(
                        "E0345",
                        &format!("missing argument `{p}` ({})", ty.name()),
                        span,
                        "pass every parameter without a default value, as inputSchema says",
                    ))
                }
            };
            match ir::jsonrt::decode(&self.m, *ty, &text, span) {
                Ok(v) => values.push(v),
                Err(Stop::Error(e)) => {
                    // (`expected an int at $.x[0]`: the path is inside the argument)
                    let msg = e.msg.trim_start_matches("json.parse: ");
                    let msg = match msg.strip_suffix(" at $") {
                        Some(m) => m.to_string(),
                        None => msg.replacen(" at $", &format!(" at {p}"), 1),
                    };
                    return Err(self.error(
                        "E0345",
                        &format!("argument `{p}`: {msg}"),
                        span,
                        "the argument must have the shape of the parameter's type",
                    ));
                }
                Err(_) => return Err(self.error("E0345", &format!("argument `{p}` cannot be read"), span, "")),
            }
        }
        // the run: a fresh interpreter, confined, with the limits of the server
        let mut host = Host::new();
        host.confined = true;
        host.mock_fs = self.mock_fs.take();
        host.mock_env = self.mocks.env.clone();
        if let Some(f) = &self.mocks.input {
            host = host.with_input(std::fs::read(f).unwrap_or_default());
        } else {
            host = host.with_input(Vec::new());
        }
        let m = &self.m;
        let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut it = Interp::with_host(m, self.limits, host, Out::keep());
            let r = it.call(func, values);
            let limit = match &r {
                Err(stop) => it.limit_error(stop),
                Ok(_) => None,
            };
            let mock = it.host_mut().mock_fs.take();
            (r, limit, it.output().to_string(), mock)
        }));
        let Ok((result, limit, printed, mock)) = ran else {
            return Err(self.error(
                "E0345",
                "internal error in the interpreter (a bug of nyra)",
                span,
                "please report this tool and its arguments",
            ));
        };
        self.mock_fs = mock;
        let mut content = Vec::new();
        let structured = match result {
            Ok(v) => match (ret, v) {
                (Some(t), Some(v)) => {
                    let mut text = String::new();
                    ir::jsonrt::encode(m, t, &v, &mut text);
                    let structured = Json::parse(&text).ok().map(|j| obj([("result", j)]));
                    content.push(obj([("type", "text".into()), ("text", text.into())]));
                    structured
                }
                _ => {
                    content.push(obj([("type", "text".into()), ("text", if printed.is_empty() { "done" } else { "" }.into())]));
                    None
                }
            },
            Err(Stop::Error(e)) => return Err(crate::sandbox::render_error(&e, &self.file, true)),
            Err(Stop::Exit(code)) => {
                return Err(self.error(
                    "E0345",
                    &format!("the tool stopped the program with os.exit({code})"),
                    span,
                    "a tool returns its result: do not call os.exit in a tool",
                ))
            }
            Err(stop) => match limit {
                Some(e) => return Err(crate::sandbox::render_error(&e, &self.file, true)),
                None => {
                    let what = if let Stop::Bug(w) = stop { w } else { "stopped".to_string() };
                    return Err(self.error(
                        "E0345",
                        &format!("internal error in the interpreter: {what}"),
                        span,
                        "please report this bug",
                    ));
                }
            },
        };
        content.retain(|c| c.get("text").and_then(Json::as_str) != Some(""));
        if !printed.is_empty() {
            content.push(obj([("type", "text".into()), ("text", printed.into())]));
        }
        Ok((content, structured))
    }
}

struct Server {
    tools: Tools,
}

impl Rpc for Server {
    fn answer(&mut self, method: &str, params: &Json) -> Result<Json, RpcError> {
        match method {
            "initialize" => {
                let version = mcp::version(params)?;
                Ok(obj([
                    ("protocolVersion", version.into()),
                    ("capabilities", obj([("tools", obj([("listChanged", false.into())]))])),
                    ("serverInfo", obj([("name", self.tools.stem.as_str().into()), ("version", env!("CARGO_PKG_VERSION").into())])),
                    (
                        "instructions",
                        format!(
                            "Tools written in Nyra ({}). Each call runs in a sandboxed interpreter with limits; errors carry a Nyra error code (`nyra explain CODE`).",
                            self.tools.file
                        )
                        .into(),
                    ),
                ]))
            }
            "ping" => Ok(Json::Obj(Vec::new())),
            "tools/list" => Ok(self.tools.manifest()),
            "tools/call" => {
                let Some(name) = params.get("name").and_then(Json::as_str) else {
                    return Err(rpc_err(mcp::INVALID_PARAMS, "Invalid params: name must be a string"));
                };
                if !self.tools.tools.iter().any(|t| t.name == name) {
                    return Err(rpc_err(mcp::INVALID_PARAMS, format!("Unknown tool: {name}")));
                }
                let empty = Json::Obj(Vec::new());
                let args = match params.get("arguments") {
                    None | Some(Json::Null) => &empty,
                    Some(a) => a,
                };
                Ok(match self.tools.call(name, args) {
                    Ok((content, structured)) => {
                        let mut fields = vec![("content".to_string(), Json::Arr(content))];
                        if let Some(s) = structured {
                            fields.push(("structuredContent".to_string(), s));
                        }
                        fields.push(("isError".to_string(), false.into()));
                        Json::Obj(fields)
                    }
                    Err(text) => obj([
                        ("content", Json::Arr(vec![obj([("type", "text".into()), ("text", text.into())])])),
                        ("isError", true.into()),
                    ]),
                })
            }
            _ => Err(rpc_err(mcp::METHOD_NOT_FOUND, format!("Method not found: {method}"))),
        }
    }
}

/// `nyra serve ...`
pub fn cli(args: Vec<String>) -> ExitCode {
    let (mut is_mcp, mut list, mut file) = (false, false, None);
    let mut grant = caps::Grant::none();
    let mut mocks = crate::mock::Mocks::default();
    let mut limits: Vec<(String, u64)> = Vec::new();
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-h" | "--help" => {
                print!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            "--mcp" => is_mcp = true,
            "--list" => list = true,
            "--allow" => {
                let Some(v) = it.next() else { return crate::fail("--allow needs capabilities: `--allow fs,os`") };
                if let Err(e) = grant.allow(&v) {
                    return crate::fail(e);
                }
            }
            "--mock" => {
                let Some(v) = it.next() else { return crate::fail("--mock needs a mock: fs=DIR, input=FILE or os.env=KEY=VALUE") };
                if let Err(e) = mocks.add(&v) {
                    return crate::fail(e);
                }
            }
            "--fuel" | "--max-depth" | "--timeout" | "--max-time" | "--max-memory" | "--max-output" => {
                let Some(v) = it.next() else { return crate::fail(format!("{a} needs a value")) };
                let n = if a.starts_with("--max-m") || a == "--max-output" {
                    crate::sandbox::parse_size(&v)
                } else {
                    v.replace('_', "").parse::<u64>().map_err(|_| format!("{a} needs a whole number, found `{v}`"))
                };
                match n {
                    Ok(n) => limits.push((a.clone(), n)),
                    Err(e) => return crate::fail(e),
                }
            }
            _ if a.starts_with('-') => return crate::fail(format!("unknown option `{a}`\n\n{USAGE}")),
            _ if file.is_none() => file = Some(a),
            _ => return crate::fail(format!("`nyra serve` takes one file\n\n{USAGE}")),
        }
    }
    let Some(file) = file.filter(|_| is_mcp) else {
        eprint!("{USAGE}");
        return ExitCode::from(2);
    };
    for c in mocks.caps() {
        let _ = grant.allow(c);
    }
    let mut tools = match Tools::load_with(&file, grant, mocks) {
        Ok(t) => t,
        Err(code) => return code,
    };
    for (flag, n) in limits {
        match flag.as_str() {
            "--fuel" => tools.limits.steps = n,
            "--max-depth" => tools.limits.depth = (n as usize).min(crate::sandbox::MAX_DEPTH),
            "--max-memory" => tools.limits.memory = n,
            "--max-output" => tools.limits.output = n,
            _ => tools.limits.wall_ms = n,
        }
    }
    if list {
        print!("{}", crate::schema::pretty(&tools.manifest()));
        return ExitCode::SUCCESS;
    }
    if tools.tools.is_empty() {
        eprintln!("nyra: {file} has no `pub fn`: the server has no tools (mark the functions to serve with `pub fn`)");
    }
    let mut server = Server { tools };
    mcp::serve_stdio(&mut server);
    ExitCode::SUCCESS
}
