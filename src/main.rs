mod ast;
mod auto;
mod caps;
mod check;
// `nyra run --checkpoint` and `nyra resume` (see `checkpoint.rs`)
mod checkpoint;
mod clock;
// `nyra clone` runs curl and browsers: not part of the WebAssembly build
#[cfg(not(target_arch = "wasm32"))]
mod clone;
mod codegen;
mod contracts;
mod diag;
mod edit;
mod examples;
mod explain;
mod fix;
mod fmt;
mod helpers;
mod hints;
mod ir;
mod json;
mod lexer;
mod limits;
// the language server for editors; it needs threads and standard input, so not in the WebAssembly build
#[cfg(not(target_arch = "wasm32"))]
mod lsp;
mod mcp;
mod mem;
// `nyra run --mock fs=DIR ...`: capabilities served by mocks
mod mock;
mod modules;
// `nyra new` and `nyra upgrade` write files and start processes: not in the WebAssembly build
#[cfg(not(target_arch = "wasm32"))]
mod new;
mod parser;
mod perfwarn;
mod sandbox;
// typed output (`fn main() -> T`) and `nyra schema`
mod schema;
// `nyra serve --mcp tools.nyra`: a program's functions as MCP tools (stdio, not in the WebAssembly build)
#[cfg(not(target_arch = "wasm32"))]
mod serve;
mod stdlib;
#[cfg(not(target_arch = "wasm32"))]
mod upgrade;
mod web;
mod webcheck;
// the C-ABI entry points of the WebAssembly build (`src/lib.rs`); natively only for their tests
#[cfg(any(target_arch = "wasm32", test))]
mod wasm;

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

/// Every command, one row each, sorted by name: its arguments, what it does (for `nyra help`), and
/// what an AI agent uses it for (for `nyra help agent`). The help texts are made from this table,
/// so they cannot leave a command out (a test checks the dispatcher against it).
const COMMANDS: &[(&str, &str, &str, &str)] = &[
    ("build", "<file.nyra>", "compile to a native executable (a site: write it to dist/)", "build; `-o PATH`, `--js` for JS source"),
    (
        "check",
        "<file.nyra>",
        "only check for errors (this runs the `ex` examples too)",
        "errors; `--fix` writes certain fixes, `--strict`",
    ),
    ("clone", "<url>", "write site.nyra that recreates a page (`nyra clone --help`)", "a web page as site.nyra"),
    (
        "edit",
        "<file.nyra>",
        "change symbols by name (`nyra edit --help`)",
        "by name: `--set NAME CODE`, `--rename A B`, `--add CODE`, `--delete NAME`",
    ),
    ("explain", "[CODE]", "explain an error code (without CODE: list the codes)", "an error code in full, with examples"),
    ("fmt", "<file.nyra>", "rewrite the file in canonical form (fixes applied, `return`, 4 spaces)", "canonical form, fixes applied"),
    ("help", "[agent]", "this help (`agent`: the short list for AI agents)", "this list"),
    ("lsp", "", "serve editors over the Language Server Protocol (stdio)", "language server (stdio)"),
    ("mcp", "", "serve AI agents over the Model Context Protocol (stdio)", "MCP server (stdio): nyra_check, nyra_run, ..."),
    (
        "new",
        "<kind> <name>",
        "start a project: script, cli, site or lib (`nyra new` lists them)",
        "new project: script, cli, site, lib",
    ),
    ("outline", "<file.nyra>", "list the functions and structs with their lines", "functions and structs with lines"),
    (
        "resume",
        "<state.json>",
        "go on with a run that `--checkpoint` paused (`nyra resume --help`)",
        "continue a run `--checkpoint` paused",
    ),
    (
        "run",
        "<file.nyra>",
        "run it (args after `--` go to the program; a site is served with live reload)",
        "run; `--allow fs,os,net,input`, `--sandbox`, `--native`, `--checkpoint F`, `-- args`",
    ),
    (
        "schema",
        "<file.nyra>",
        "the JSON Schema of what `fn main() -> T` prints (`--tools`: of the MCP tools)",
        "JSON Schema of `main() -> T`; `--tools`",
    ),
    ("serve", "--mcp <file.nyra>", "serve the file's `pub fn`s as MCP tools (`nyra serve --help`)", "`pub fn`s as MCP tools (stdio)"),
    ("show", "<file.nyra> <name>", "print one function, struct or Struct.field", "source of symbols by name"),
    ("test", "<file.nyra>", "run the `ex` examples and report each one that fails", "run the `ex` examples"),
    ("upgrade", "[--check]", "install the newest release over this nyra (`nyra upgrade --help`)", "install the newest release"),
    ("version", "", "print the version of nyra", "the version"),
];

/// The commands that take a program file and the options below (`parse_args`).
const FILE_COMMANDS: &[&str] = &["build", "check", "fmt", "run", "test"];

/// `nyra help`: the commands from `COMMANDS`, then the options.
fn usage() -> String {
    let mut out = String::from("nyra - a language for AI agents\n\nusage:\n");
    for (name, args, what, _) in COMMANDS {
        let call = format!("nyra {name} {args}");
        out += &format!("  {call:<34} {what}\n");
    }
    out += &format!("  {:<34} {}\n\n", "nyra <file.nyra>", "same as `nyra run`");
    out + OPTIONS
}

/// `nyra help agent` (or `nyra --agent-help`): every command on one line, and the flags an agent
/// needs, in about 330 tokens.
fn agent_help() -> String {
    let mut out = String::from("nyra commands (file = program.nyra; JSON output: --json):\n");
    for (name, args, _, agent) in COMMANDS {
        out += &format!("nyra {name}{}{args}: {agent}\n", if args.is_empty() { "" } else { " " });
    }
    out += "--json errors: {ok, errors:[{code,message,line,col,hint,fix?,exact?}], warnings}; \
            `exact` fixes are safe to apply, and are applied in memory unless --strict\n";
    out
}

const OPTIONS: &str = "\
options:
  --target <t> the backend: native (default), c, js; experimental: py, ts, rs, go
  --js --py --ts --rs --go   short for --target js / py / ts / rs / go (py, ts, rs, go: experimental)
  --c          (build) write the generated C source instead of an executable
  -o <path>    output path for `build` (`-o -` prints to stdout)
  --json       print errors as JSON (compile and runtime), for AI agents
  --fix        write every fix that is certain to the file (also when other errors
               remain), then report what is left (or run or build it as usual)
  --strict     errors stay errors: without it, every error that has exactly one certain
               fix is repaired in memory (the file is not written) and reported as a warning,
               and the errors that are left are shown at their places in the file
  --native     (run) compile with the C compiler and start the executable, with no interpreter
               first. Without it `run` is automatic: a build of this exact source that is already
               cached runs at once; otherwise the program starts in the interpreter (milliseconds)
               with a budget of steps and time, while the C compiler starts in the background. A program
               that finishes within the budget prints its output right away; one that does not is
               stopped, its output thrown away, and it runs again natively (same input, so the
               output appears once). Programs that use `fs` or `os` or call `time.sleep_ms` always
               run natively (running them twice could repeat their effects); so do programs that
               read a terminal.
               The output, the exit code and the runtime errors are the same in every mode.
  --release    (run) optimize the generated C like `build` does (-O2), and run it natively (as
               --native); `run` compiles with -O1 by default, so the first output comes sooner
  --time       show how long each step took

websites (a program with `use web`, see docs/WEB.md):
  nyra build site.nyra      write the site to `dist/` next to the program (`-o DIR` for another folder)
  nyra run site.nyra        serve it on http://127.0.0.1:8000 and rebuild and reload when a file changes
  nyra check site.nyra      also run the site's checks (anchors, images, headings, contrast); writes nothing
  nyra clone <url>          write site.nyra that recreates a page (`nyra clone --help`, docs/CLONE.md)
  --port N        (run) the port; 0 lets the system choose
  --image-tool T  builtin (the default), cwebp, magick, vips or none: what makes the WebP image variants
  A web program gets no capability (fs, os, ...) unless --allow names it.

safety (for programs you did not write, or agents that run unsupervised):
  --allow LIST     grant only these capabilities: fs, input, os, net (or all). A `use` of a
                   module whose capability is not granted is error E0290. Without --allow
                   and --sandbox everything is granted.
  --sandbox        run in the interpreter with no capabilities but those of --allow, file
                   paths confined to the working folder, and the limits below
  --interp         run in the interpreter (no C compiler or Node.js needed), with the limits
  --fuel N         steps the program may run (default 2,000,000,000)      E0355, exit 120
  --max-memory S   heap the program may use, e.g. 256M (default 512M)     E0356, exit 121
  --max-output S   bytes the program may print (default 64M)              E0357, exit 122
  --max-depth N    nested calls (default 20,000)                          E0358, exit 123
  --max-time MS    real time (default: none)                              E0359, exit 124
  A limit flag implies --interp. Programs that stop on a limit print the error code on stderr.
  --checkpoint F   when the fuel or the time runs out, pause instead and save the state to F
                   (exit 125); `nyra resume F` goes on from there
  --mock M         serve a capability from a mock, in the interpreter: fs=DIR (a copy-on-write view
                   of DIR: writes stay in memory), input=FILE (standard input), os.env=KEY=VALUE.
                   Like --allow, mocks grant nothing else unless --allow says so.

typed output: a program whose `fn main() -> T` returns a value prints it as one JSON document on
stdout (its `print` output goes to stderr); `nyra schema` gives the JSON Schema of T.

`build` writes an executable natively and source code for every other target.
Supported targets: native and js. py, ts, rs and go are experimental.
`run` needs the target's tool: a C compiler (gcc, clang, cc or tcc; NYRA_CC),
node (js and ts), python3 (NYRA_PYTHON), rustc (NYRA_RUSTC) or go (NYRA_GO).
";

/// Flags nyra passes to the C compiler, besides the optimization level (see `Opt`). Kept deliberately short:
/// -fwrapv  make int overflow wrap (defined behavior, as the spec says)
/// -ffp-contract=off  never fuse float operations (a*b+c), so results match JavaScript
/// -s       strip symbols for a smaller executable (macOS's linker ignores it, so it's left out there)
#[cfg(not(target_os = "macos"))]
const CC_FLAGS: &[&str] = &["-fwrapv", "-ffp-contract=off", "-s"];
#[cfg(target_os = "macos")]
const CC_FLAGS: &[&str] = &["-fwrapv", "-ffp-contract=off"];

/// How hard the C compiler optimizes. `nyra build` and `nyra run --release` use `Release` (-O2). A
/// plain `nyra run` is about getting the first output soon, so it uses `Fast` (-O1): the compile is
/// a third shorter on big programs and the program runs about as fast as at -O2 (-O0 is no
/// faster to compile, as it still emits the whole runtime, and runs 3 to 10 times slower).
#[derive(Clone, Copy, PartialEq)]
pub enum Opt {
    Fast,
    Release,
}

impl Opt {
    fn flag(self) -> &'static str {
        match self {
            Opt::Fast => "-O1",
            Opt::Release => "-O2",
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Target {
    Native,
    C,
    Js,
    Py,
    Ts,
    Rs,
    Go,
}

impl Target {
    fn parse(name: &str) -> Option<Target> {
        Some(match name {
            "native" => Target::Native,
            "c" => Target::C,
            "js" | "javascript" => Target::Js,
            "py" | "python" => Target::Py,
            "ts" | "typescript" => Target::Ts,
            "rs" | "rust" => Target::Rs,
            "go" => Target::Go,
            _ => return None,
        })
    }

    /// The extension of the source file `build` writes (the executable's for native).
    fn ext(self) -> &'static str {
        match self {
            Target::Native => std::env::consts::EXE_EXTENSION,
            Target::C => "c",
            Target::Js => "js",
            Target::Py => "py",
            Target::Ts => "ts",
            Target::Rs => "rs",
            Target::Go => "go",
        }
    }
}

struct Opts {
    cmd: String,
    file: String,
    target: Target,
    out: Option<String>,
    json: bool,
    time: bool,
    fix: bool,
    /// Do not repair errors in memory (see `--strict`).
    strict: bool,
    /// `run --release`: compile the C with -O2 (see `Opt`).
    release: bool,
    /// What follows `--`: the program's own arguments (`nyra run main.nyra -- a b`).
    prog_args: Vec<String>,
    /// `--interp`: run in the interpreter.
    interp: bool,
    /// `--native`: `run` compiles with the C compiler and starts the executable, with no interpreter first.
    native: bool,
    /// `--sandbox`: the interpreter, nothing granted but `--allow`, confined files.
    sandbox: bool,
    /// The values of `--allow`.
    allow: Vec<String>,
    fuel: Option<u64>,
    max_memory: Option<u64>,
    max_output: Option<u64>,
    max_depth: Option<usize>,
    max_time: Option<u64>,
    /// `--port N`: the dev server of a `use web` program.
    port: Option<u16>,
    /// `--image-tool T`: the image tool of a `use web` build.
    image_tool: Option<String>,
    /// `--checkpoint FILE`: pause instead of stopping when the fuel or time runs out.
    checkpoint: Option<String>,
    /// `--mock ...`: capabilities served by mocks.
    mocks: mock::Mocks,
}

impl Opts {
    /// The capabilities this run grants: everything, unless `--sandbox` or `--allow` narrow it.
    fn grant(&self) -> Result<caps::Grant, String> {
        let narrow = self.sandbox || !self.allow.is_empty() || !self.mocks.is_empty();
        let mut g = if narrow { caps::Grant::none() } else { caps::Grant::all() };
        for a in &self.allow {
            g.allow(a)?;
        }
        for c in self.mocks.caps() {
            g.allow(c)?;
        }
        Ok(g)
    }

    /// True if the program runs in the interpreter: asked for, or needed by a limit.
    fn interpreted(&self) -> bool {
        self.interp
            || self.sandbox
            || self.fuel.is_some()
            || self.max_memory.is_some()
            || self.max_output.is_some()
            || self.max_depth.is_some()
            || self.max_time.is_some()
            || self.checkpoint.is_some()
            || !self.mocks.is_empty()
    }
}

/// What to add to the command line to grant a capability.
fn cli_flag(cap: &str) -> String {
    format!("grant it by running with `--allow {cap}` (`nyra run main.nyra --allow {cap}`)")
}

fn parse_args() -> Result<Opts, String> {
    let mut args = std::env::args().skip(1);
    let mut positional = Vec::new();
    let mut opts = Opts {
        cmd: String::new(),
        file: String::new(),
        target: Target::Native,
        out: None,
        json: false,
        time: false,
        fix: false,
        strict: false,
        release: false,
        prog_args: Vec::new(),
        interp: false,
        native: false,
        sandbox: false,
        allow: Vec::new(),
        fuel: None,
        max_memory: None,
        max_output: None,
        max_depth: None,
        max_time: None,
        port: None,
        image_tool: None,
        checkpoint: None,
        mocks: mock::Mocks::default(),
    };
    while let Some(a) = args.next() {
        match a.as_str() {
            "--" => {
                opts.prog_args = args.by_ref().collect();
                break;
            }
            "-h" | "--help" | "help" => return Err(usage()),
            "-V" | "--version" | "version" => return Err(format!("nyra {}", env!("CARGO_PKG_VERSION"))),
            "--js" => opts.target = Target::Js,
            "--c" => opts.target = Target::C,
            "--py" => opts.target = Target::Py,
            "--ts" => opts.target = Target::Ts,
            "--rs" => opts.target = Target::Rs,
            "--go" => opts.target = Target::Go,
            "--target" => {
                let t = args.next().ok_or("--target needs a name: native, c, js, py, ts, rs or go")?;
                opts.target = Target::parse(&t).ok_or_else(|| format!("unknown target `{t}`: use native, c, js, py, ts, rs or go"))?;
            }
            "-o" => opts.out = Some(args.next().ok_or("-o needs a path")?),
            "--json" => opts.json = true,
            "--time" => opts.time = true,
            "--fix" => opts.fix = true,
            "--interp" => opts.interp = true,
            "--native" => opts.native = true,
            "--sandbox" => opts.sandbox = true,
            "--allow" => opts.allow.push(args.next().ok_or("--allow needs capabilities: `--allow fs,os` (or `all`)")?),
            _ if a.starts_with("--allow=") => opts.allow.push(a["--allow=".len()..].to_string()),
            "--fuel" | "--max-depth" | "--max-time" => {
                let v = args.next().ok_or_else(|| format!("{a} needs a number"))?;
                let n: u64 = v.replace('_', "").parse().map_err(|_| format!("{a} needs a whole number, found `{v}`"))?;
                match a.as_str() {
                    "--fuel" => opts.fuel = Some(n),
                    "--max-depth" if n > sandbox::MAX_DEPTH as u64 => {
                        return Err(format!(
                            "--max-depth is at most {} (deeper calls would overflow the stack of the interpreter)",
                            sandbox::MAX_DEPTH
                        ));
                    }
                    "--max-depth" => opts.max_depth = Some(n as usize),
                    _ => opts.max_time = Some(n),
                }
            }
            "--max-memory" | "--max-output" => {
                let v = args.next().ok_or_else(|| format!("{a} needs a size such as 256M"))?;
                let n = sandbox::parse_size(&v)?;
                if a == "--max-memory" {
                    opts.max_memory = Some(n);
                } else {
                    opts.max_output = Some(n);
                }
            }
            "--strict" => opts.strict = true,
            "--checkpoint" => opts.checkpoint = Some(args.next().ok_or("--checkpoint needs a file: `--checkpoint state.json`")?),
            "--mock" => opts.mocks.add(&args.next().ok_or("--mock needs a mock: fs=DIR, input=FILE or os.env=KEY=VALUE")?)?,
            _ if a.starts_with("--mock=") => opts.mocks.add(&a["--mock=".len()..])?,
            "--release" => opts.release = true,
            "--port" => {
                let v = args.next().ok_or("--port needs a number")?;
                opts.port = Some(v.parse().map_err(|_| format!("--port needs a port number (0 to 65535), found `{v}`"))?);
            }
            "--image-tool" => {
                let t = args.next().ok_or("--image-tool needs a name: builtin, cwebp, magick, vips or none")?;
                // a name, or the path of one (`C:\tools\cwebp.exe`)
                let stem = Path::new(&t).file_stem().and_then(|s| s.to_str()).unwrap_or("");
                if !web::image::TOOLS.contains(&stem) && t != "none" && t != "builtin" {
                    return Err(format!("unknown image tool `{t}`: use builtin, cwebp, magick, vips or none (or the path of one)"));
                }
                opts.image_tool = Some(t);
            }
            _ if a.starts_with('-') => return Err(format!("unknown option `{a}`\n\n{}", usage())),
            _ => positional.push(a),
        }
    }
    if positional.len() == 1 && positional[0].ends_with(".nyra") {
        positional.insert(0, "run".into());
    }
    match positional.as_slice() {
        [cmd, file] if FILE_COMMANDS.contains(&cmd.as_str()) => {
            opts.cmd = cmd.clone();
            opts.file = file.clone();
            Ok(opts)
        }
        _ => Err(usage()),
    }
}

/// Lexes, parses, type-checks and runs the examples: a program that passes can be generated.
fn compile(src: &str) -> Result<ast::Program, Vec<diag::Diag>> {
    compile_granted(src, &caps::Grant::all(), &cli_flag)
}

/// `compile`, and a `use` of a module whose capability `grant` does not give is error E0290
/// (`flag` says how to grant it). The capabilities are checked before the examples run.
fn compile_granted(src: &str, grant: &caps::Grant, flag: &dyn Fn(&str) -> String) -> Result<ast::Program, Vec<diag::Diag>> {
    let mut prog = front(src)?;
    let errs = caps::enforce(&prog, grant, flag);
    if !errs.is_empty() {
        return Err(errs);
    }
    let errs = examples::run(&mut prog).errors;
    if !errs.is_empty() {
        return Err(errs);
    }
    Ok(prog)
}

/// Lexes, parses and type-checks, without running the examples.
fn front(src: &str) -> Result<ast::Program, Vec<diag::Diag>> {
    diag::clear_warnings();
    // a new text: the places of its diagnostics are its own (`fix::repair_all` sets them after)
    fix::set_origin(None);
    // only fixes that match the source exactly are kept
    let checked = |mut errs: Vec<diag::Diag>| {
        // a fix is a text edit of this file: one for a mistake in an imported file would edit the wrong text
        errs.iter_mut().filter(|d| d.file.is_some()).for_each(|d| d.fix.clear());
        fix::validate(&mut errs, src);
        errs
    };
    let (toks, errs) = lexer::lex(src);
    if !errs.is_empty() {
        return Err(checked(errs));
    }
    let (mut prog, errs) = parser::parse(toks);
    if !errs.is_empty() {
        return Err(checked(errs));
    }
    // the files the program imports (`use ./name`)
    let errs = modules::load(&mut prog);
    if !errs.is_empty() {
        return Err(checked(errs));
    }
    let linked = stdlib::link(&mut prog);
    // the web library's checks of literal values (an unknown word, a forbidden link, ...) leave the
    // program whole: its type errors are found in the same run, so one report lists them all
    if linked.iter().any(|d| !webcheck::independent(d)) {
        return Err(checked(linked));
    }
    // the motion words and `keyframes(...)` of a site, where they are literals (E0374, E0388)
    let motion = if prog.uses.iter().any(|u| u.module == "web") { webcheck::motion(src) } else { Vec::new() };
    let mut errs = linked;
    errs.extend(check::check(&mut prog));
    errs.extend(motion);
    if !errs.is_empty() {
        errs.sort_by_key(|d| (d.file.is_some(), d.span.line, d.span.col));
        return Err(checked(errs));
    }
    Ok(prog)
}

fn ms(d: Duration) -> String {
    format!("{:.2} ms", d.as_secs_f64() * 1000.0)
}

fn fail(msg: impl std::fmt::Display) -> ExitCode {
    eprintln!("nyra: {msg}");
    ExitCode::from(2)
}

/// The stack of the threads that compile. The parser limits nesting (E0103), so the stages that
/// recurse over a program (parser, checker, lowering, backends) stay far below this; the size is a
/// second safety net. Only address space is reserved: memory is used as the stack grows.
pub const STACK: usize = 256 << 20;

fn main() -> ExitCode {
    // everything runs on a thread with a big stack (see `STACK`)
    match std::thread::Builder::new().name("nyra".into()).stack_size(STACK).spawn(real_main) {
        Ok(t) => t.join().unwrap_or(ExitCode::from(101)),
        Err(_) => real_main(),
    }
}

fn real_main() -> ExitCode {
    // the commands that need no program file (the others are `FILE_COMMANDS`, see `parse_args`)
    let rest = || std::env::args().skip(2).collect::<Vec<String>>();
    match std::env::args().nth(1).as_deref() {
        Some("explain") => return explain::run(rest()),
        Some("mcp") => return mcp::run(rest()),
        Some(cmd @ ("outline" | "show" | "edit")) => return edit::run(cmd, rest()),
        Some("help") if std::env::args().nth(2).as_deref() == Some("agent") => {
            print!("{}", agent_help());
            return ExitCode::SUCCESS;
        }
        Some("--agent-help") => {
            print!("{}", agent_help());
            return ExitCode::SUCCESS;
        }
        _ => {}
    }
    #[cfg(not(target_arch = "wasm32"))]
    match std::env::args().nth(1).as_deref() {
        Some("clone") => return clone::cli(rest()),
        Some("lsp") => return lsp::run(rest()),
        Some("new") => return new::run(rest()),
        Some("resume") => return checkpoint::cli(rest()),
        Some("schema") => return schema::cli(rest()),
        Some("serve") => return serve::cli(rest()),
        Some("upgrade") => return upgrade::run(rest()),
        // on stdout with success, so scripts and installers can read it
        Some("-V" | "--version" | "version") if std::env::args().len() == 2 => {
            println!("nyra {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        _ => {}
    }
    let opts = match parse_args() {
        Ok(o) => o,
        Err(msg) => {
            eprintln!("{msg}");
            return ExitCode::from(2);
        }
    };
    let mut src = match std::fs::read_to_string(&opts.file) {
        Ok(s) => s,
        Err(e) => return fail(format!("cannot read `{}`: {e}", opts.file)),
    };
    // `use ./name` lines are relative to this file
    modules::set_main(Some(Path::new(&opts.file)));

    if opts.cmd == "fmt" {
        return fmt::run(&opts.file, &src, opts.out.as_deref(), opts.json);
    }

    let grant = match opts.grant() {
        Ok(g) => g,
        Err(msg) => return fail(msg),
    };
    // a `use web` program builds or serves a site (src/web.rs), with no capability unless `--allow` gives it
    let is_web = web::uses_web(&src) && ["build", "run", "check"].contains(&opts.cmd.as_str());
    let grant = if is_web && !opts.sandbox && opts.allow.is_empty() { caps::Grant::none() } else { grant };
    if is_web && (opts.interpreted() || opts.target != Target::Native) {
        return fail("a `use web` program builds a site in the interpreter: --target, --interp, --sandbox and the limit flags do not apply (see `nyra help`)");
    }
    let compile = |s: &str| compile_granted(s, &grant, &cli_flag);
    // `test` checks the program and its capabilities; the examples are what it runs
    let check_only = |s: &str| {
        let prog = front(s)?;
        let errs = caps::enforce(&prog, &grant, cli_flag);
        if errs.is_empty() {
            Ok(prog)
        } else {
            Err(errs)
        }
    };
    if opts.interpreted() && opts.cmd == "build" {
        return fail("--interp, --sandbox and the limit flags are for `nyra run`: `build` makes a native executable or source code");
    }
    if opts.interpreted() && opts.target != Target::Native {
        return fail("--interp and --sandbox run the program in the interpreter: do not combine them with --target, --js, --py, ...");
    }

    let start = Instant::now();
    let mut fixed = 0;
    let mut applied = 0;
    let builder: &dyn Fn(&str) -> Result<ast::Program, Vec<diag::Diag>> = if opts.cmd == "test" { &check_only } else { &compile };
    let prog = match builder(&src) {
        Ok(p) => p,
        Err(diags) => {
            // Every error with a certain fix is repaired (`fix::repair_all`), also when others
            // remain: in memory (each fix is reported as a warning, with its edits, and the errors
            // that are left at their places in the file), or with --fix in the file itself.
            // --strict repairs nothing.
            let out = if opts.fix || !opts.strict {
                fix::repair_all(&src, diags, builder)
            } else {
                fix::Outcome { text: src.clone(), result: Err(diags), fixed: 0, applied: Vec::new() }
            };
            if opts.fix && out.fixed > 0 {
                if let Err(e) = std::fs::write(&opts.file, &out.text) {
                    return fail(format!("cannot write `{}`: {e}", opts.file));
                }
                eprint!("nyra: fixed {} error(s) in {}:\n{}", out.fixed, opts.file, fix::diff(&src, &out.text));
                // the file is the repaired text now: what is left is shown at its places in it
                fix::set_origin(None);
                fixed = out.fixed;
                src = out.text.clone();
            } else if out.fixed > 0 {
                applied = out.fixed;
                src = out.text.clone();
            }
            match out.result {
                Ok(p) => p,
                Err(left) => {
                    if opts.json {
                        let mut json = diag::render_json(&left, &opts.file);
                        if fixed > 0 {
                            json = format!("{},\"fixed\":{fixed}}}", &json[..json.len() - 1]);
                        }
                        println!("{json}");
                    } else {
                        eprint!("{}", diag::report(&left, &opts.file, &src));
                    }
                    return ExitCode::from(1);
                }
            }
        }
    };
    // (the fixes applied in memory are shown with the warnings; `run --json` and `build --json`
    // print them on stderr, as the program's output is on stdout)
    if applied > 0 && opts.json && opts.cmd != "check" {
        eprintln!("{{\"warnings\":{}}}", diag::render_json_warnings(&diag::applied(), &opts.file));
    }

    // slow patterns: warnings too (E0360-E0362)
    perfwarn::check(&prog, opts.target == Target::Go);

    if is_web {
        let cli = web::Cli {
            cmd: opts.cmd.clone(),
            file: opts.file.clone(),
            out: opts.out.clone(),
            json: opts.json,
            port: opts.port,
            image_tool: opts.image_tool.clone(),
            grant: grant.clone(),
        };
        return web::cli(&cli, &src, prog);
    }

    if opts.cmd == "test" {
        return test(&opts, &src, prog);
    }

    // warnings do not stop the build; `check --json` lists them in its JSON instead
    if !(opts.json && opts.cmd == "check") {
        eprint!("{}", diag::render_warnings(&opts.file, &src));
    }
    if opts.cmd == "check" {
        if opts.json {
            let mut json = diag::render_json(&[], &opts.file);
            if fixed > 0 {
                json = format!("{},\"fixed\":{fixed}}}", &json[..json.len() - 1]);
            }
            println!("{json}");
        } else {
            eprintln!("nyra: no errors ({})", ms(start.elapsed()));
        }
        return ExitCode::SUCCESS;
    }

    if opts.interpreted() {
        return run_interpreted(&opts, &prog, &src);
    }
    // `fn main() -> T` is written as JSON by the interpreter, C and JavaScript
    if schema::typed_main(&prog).is_some() && matches!(opts.target, Target::Py | Target::Ts | Target::Rs | Target::Go) {
        return fail("`fn main() -> T` (typed output) runs on the native, c and js targets and in the interpreter (--interp); the py, ts, rs and go targets do not support it yet");
    }

    // `nyra run` is automatic (see auto.rs) unless it was asked to compile: --native, --release
    if opts.cmd == "run" && opts.target == Target::Native && !opts.native && !opts.release {
        return auto::run(&opts, &prog, start);
    }

    let code = match generate(&prog, opts.target, &opts.file) {
        Ok(code) => code,
        Err(msg) => return fail(msg),
    };
    let nyra_time = start.elapsed();
    let stem = Path::new(&opts.file).file_stem().and_then(|s| s.to_str()).unwrap_or("main").to_string();

    if opts.cmd == "build" {
        build(&opts, &code, &stem, nyra_time)
    } else {
        run(&opts, &code, &stem, nyra_time)
    }
}

/// `nyra test`: runs every example and reports each one that fails; exit code 1 if any does.
fn test(opts: &Opts, src: &str, mut prog: ast::Program) -> ExitCode {
    let start = Instant::now();
    let out = examples::run(&mut prog);
    if opts.json {
        println!("{}", examples::json(&out, &opts.file));
    } else {
        eprint!("{}", diag::render_human(&out.errors, &opts.file, src));
        eprint!("{}", diag::render_warnings(&opts.file, src));
        eprintln!("nyra: {} ({})", examples::summary(&out, &opts.file), ms(start.elapsed()));
    }
    if out.errors.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

/// The back half of the compiler: a checked program to the source code of a target.
/// `file` is the name the generated code reports in runtime errors.
fn generate(prog: &ast::Program, target: Target, file: &str) -> Result<String, String> {
    Ok(emit(&lower(prog)?, target, file))
}

/// A checked program to IR: lowered, optimized (`NYRA_OPT=0` skips that) and verified.
fn lower(prog: &ast::Program) -> Result<ir::Module, String> {
    let module = sandbox::module(prog)?;
    if std::env::var_os("NYRA_DUMP").is_some_and(|v| v == "ir") {
        eprint!("{}", ir::print::print(&module));
    }
    Ok(module)
}

/// IR to the source code of a target.
fn emit(module: &ir::Module, target: Target, file: &str) -> String {
    match target {
        Target::Js => codegen::js::gen(module, file),
        Target::Py => codegen::py::gen(module, file),
        Target::Ts => codegen::ts::gen(module, file),
        Target::Rs => codegen::rs::gen(module, file),
        Target::Go => codegen::go::gen(module, file),
        Target::Native | Target::C => codegen::c::gen(module, file),
    }
}

const NO_CC: &str = "no C compiler found (tried gcc, clang, cc, tcc); install one, set NYRA_CC, or use --js";

/// The executable for generated C, built in the shared temp dir (see `cc_cached`).
fn native(code: &str, stem: &str, source: &str, opt: Opt) -> Result<(PathBuf, Option<Duration>), ExitCode> {
    let Some(compiler) = find_cc() else {
        return Err(fail(NO_CC));
    };
    cc_cached(&compiler, code, stem, source, &temp_dir(), opt, false).map_err(fail)
}

fn build(opts: &Opts, code: &str, stem: &str, nyra_time: Duration) -> ExitCode {
    let ext = opts.target.ext();
    let out = match &opts.out {
        Some(o) if o == "-" && opts.target != Target::Native => {
            print!("{code}");
            return ExitCode::SUCCESS;
        }
        Some(o) => PathBuf::from(o),
        None => Path::new(&opts.file).with_extension(ext),
    };

    let mut cc_part = String::new();
    if opts.target == Target::Native {
        let (exe, cc_time) = match native(code, stem, &opts.file, Opt::Release) {
            Ok(r) => r,
            Err(code) => return code,
        };
        if let Err(e) = std::fs::copy(&exe, &out) {
            return fail(format!("cannot write `{}`: {e}", out.display()));
        }
        cc_part = format!(" + {}", cc_label(cc_time));
    } else if let Err(e) = std::fs::write(&out, code) {
        return fail(format!("cannot write `{}`: {e}", out.display()));
    }

    eprintln!("nyra: built {} (nyra {}{cc_part})", out.display(), ms(nyra_time));
    ExitCode::SUCCESS
}

/// `nyra run --interp` / `--sandbox` / `--checkpoint` / `--mock`: the program runs in the IR
/// interpreter, in this process, under the limits of `sandbox.rs`. Its runtime errors are printed
/// like those of a compiled program.
fn run_interpreted(opts: &Opts, prog: &ast::Program, src: &str) -> ExitCode {
    let start = Instant::now();
    let m = match sandbox::module(prog) {
        Ok(m) => m,
        Err(msg) => return fail(msg),
    };
    let mut cfg = sandbox::Config::new();
    cfg.args = opts.prog_args.clone();
    cfg.confined = opts.sandbox;
    cfg.limits.steps = opts.fuel.unwrap_or(cfg.limits.steps);
    cfg.limits.memory = opts.max_memory.unwrap_or(cfg.limits.memory);
    cfg.limits.output = opts.max_output.unwrap_or(cfg.limits.output);
    cfg.limits.depth = opts.max_depth.unwrap_or(cfg.limits.depth);
    cfg.limits.wall_ms = opts.max_time.unwrap_or(0);
    cfg.mock_fs = opts.mocks.fs.clone().map(mock::MockFs::new);
    cfg.mock_env = opts.mocks.env.clone();
    if let Some(f) = &opts.mocks.input {
        match std::fs::read(f) {
            Ok(bytes) => cfg.stdin = Some(bytes),
            Err(e) => return fail(format!("--mock input: cannot read `{}`: {e}", f.display())),
        }
    }
    cfg.pausable = opts.checkpoint.is_some();
    // a run that may pause reads its standard input first: what is left of it goes into the checkpoint
    if cfg.pausable && cfg.stdin.is_none() && auto::effects(&m).input {
        use std::io::Read;
        let mut bytes = Vec::new();
        let _ = std::io::stdin().lock().read_to_end(&mut bytes);
        cfg.stdin = Some(bytes);
    }
    let run = checkpoint::Run {
        file: opts.file.clone(),
        fuel: cfg.limits.steps,
        max_memory: cfg.limits.memory,
        max_output: cfg.limits.output,
        max_depth: cfg.limits.depth,
        max_time: cfg.limits.wall_ms,
        allow: opts.grant().map(|g| g.names()).unwrap_or_default(),
        sandbox: opts.sandbox,
    };
    let nyra_time = start.elapsed();
    let report = sandbox::run(&m, cfg);
    let code = report_interpreted(&report, &opts.file, opts.json, opts.time, nyra_time);
    match &opts.checkpoint {
        Some(path) if report.paused.is_some() => checkpoint::paused(&m, &run, src, 0, &report, path, opts.json),
        _ => code,
    }
}

/// What an interpreted run reports on stderr after it ends: its runtime error, a bug of the
/// interpreter, the time it took (`--time`). Gives its exit code.
fn report_interpreted(report: &sandbox::Report, file: &str, json: bool, time: bool, nyra_time: Duration) -> ExitCode {
    if let Some(e) = &report.error {
        let text = sandbox::render_error(e, file, json);
        eprintln!("{text}");
    }
    if let Some(what) = &report.internal {
        eprintln!("nyra: internal error in the interpreter: {what}");
        eprintln!("nyra: run the program without --interp and report this bug");
    }
    if time {
        eprintln!(
            "nyra {} | interpreted {:.2} ms ({} steps, {} allocations)",
            ms(nyra_time),
            report.run_ms,
            report.steps,
            report.allocs
        );
    }
    ExitCode::from(report.exit as u8)
}

fn run(opts: &Opts, code: &str, stem: &str, nyra_time: Duration) -> ExitCode {
    // the tool's own compile step (C, Rust, Go), when there is one: `None` when it was cached
    let mut build_time: Option<Option<Duration>> = None;
    let cmd = match opts.target {
        Target::Js | Target::Ts | Target::Py => {
            // named by the code, so two programs with the same name can run at the same time
            let name = format!("{stem}-{:016x}.{}", fnv1a(&[code.as_bytes()]), opts.target.ext());
            let path = match write_temp(&temp_dir(), &name, code) {
                Ok(p) => p,
                Err(msg) => return fail(msg),
            };
            let mut c = match opts.target {
                Target::Py => match find_python() {
                    Some(py) => Command::new(py),
                    None => return fail("no Python found (tried python3 and python); install Python 3 or set NYRA_PYTHON"),
                },
                Target::Ts => {
                    // Node runs TypeScript by stripping the types (22.6 and later)
                    let mut c = Command::new("node");
                    if node_needs_strip_flag() {
                        c.arg("--experimental-strip-types");
                    }
                    c.arg("--disable-warning=ExperimentalWarning");
                    c
                }
                _ => Command::new("node"),
            };
            c.arg(path);
            c
        }
        Target::Rs | Target::Go | Target::Native | Target::C => {
            let built = match opts.target {
                Target::Rs => rust_cached(code, stem, &opts.file).map_err(fail),
                Target::Go => go_cached(code, stem, &opts.file).map_err(fail),
                _ => native(code, stem, &opts.file, if opts.release { Opt::Release } else { Opt::Fast }),
            };
            match built {
                Ok((exe, t)) => {
                    build_time = Some(t);
                    Command::new(exe)
                }
                Err(code) => return code,
            }
        }
    };

    exec(opts, cmd, build_time, nyra_time, None)
}

/// Starts the program `cmd` with the options' arguments and waits for it. `feed` is the standard
/// input to give it instead of this process's own (`nyra run` in auto mode has read it already).
fn exec(
    opts: &Opts,
    mut cmd: Command,
    build_time: Option<Option<Duration>>,
    nyra_time: Duration,
    feed: Option<auto::Feed>,
) -> ExitCode {
    if opts.json {
        // the program's runtime errors are then printed as JSON too
        cmd.env("NYRA_JSON", "1");
    }
    cmd.args(&opts.prog_args);
    let t = Instant::now();
    let status = match feed {
        None => cmd.status(),
        Some(feed) => auto::run_fed(cmd, feed),
    };
    let run_time = t.elapsed();
    if opts.time {
        let tool = match opts.target {
            Target::Rs => "rustc",
            Target::Go => "go",
            _ => "cc",
        };
        let build_part = match build_time {
            Some(Some(t)) => format!(" | {tool} {}", ms(t)),
            Some(None) => format!(" | {tool} cached"),
            None => String::new(),
        };
        eprintln!("nyra {}{build_part} | run {}", ms(nyra_time), ms(run_time));
    }
    match status {
        Ok(s) => ExitCode::from(s.code().unwrap_or(1) as u8),
        Err(e) => fail(format!("failed to start the program: {e}")),
    }
}

/// True if a command runs and exits with success (`python3 --version`).
fn works(cmd: &str, args: &[&str]) -> bool {
    Command::new(cmd).args(args).output().is_ok_and(|o| o.status.success())
}

/// The Python 3 interpreter: NYRA_PYTHON, else `python3`, else `python` (Windows).
fn find_python() -> Option<String> {
    if let Ok(py) = std::env::var("NYRA_PYTHON") {
        return Some(py);
    }
    ["python3", "python"].into_iter().find(|p| works(p, &["--version"])).map(String::from)
}

/// Node before 23.6 (and 22.18) runs TypeScript only with `--experimental-strip-types`.
fn node_needs_strip_flag() -> bool {
    let Ok(out) = Command::new("node").arg("--version").output() else { return false };
    let v = String::from_utf8_lossy(&out.stdout);
    let mut parts = v.trim().trim_start_matches('v').split('.').map(|p| p.parse::<u32>().unwrap_or(0));
    let (major, minor) = (parts.next().unwrap_or(0), parts.next().unwrap_or(0));
    major < 22 || (major == 22 && minor < 18) || (major == 23 && minor < 6)
}

/// Where `nyra run` puts generated code and cached executables.
fn temp_dir() -> PathBuf {
    std::env::temp_dir().join("nyra")
}

fn write_temp(dir: &Path, name: &str, contents: &str) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create `{}`: {e}", dir.display()))?;
    let path = dir.join(name);
    std::fs::write(&path, contents).map_err(|e| format!("cannot write `{}`: {e}", path.display()))?;
    Ok(path)
}

fn cc_label(cc_time: Option<Duration>) -> String {
    match cc_time {
        Some(t) => format!("cc {}", ms(t)),
        None => "cc cached".to_string(),
    }
}

/// 64-bit FNV-1a over several byte strings (with a separator between them).
fn fnv1a(parts: &[&[u8]]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for p in parts {
        for &b in p.iter().chain(&[0xff]) {
            h ^= b as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    }
    h
}

/// Compiles generated C into an executable in `dir`. If the same C code was
/// already compiled with the same compiler and flags, the old executable is reused.
/// Returns the executable and the C compiler's time (`None` when cached).
/// With `capture`, the C compiler's messages go into the error instead of the terminal.
fn cc_cached(
    compiler: &str,
    code: &str,
    stem: &str,
    source: &str,
    dir: &Path,
    opt: Opt,
    capture: bool,
) -> Result<(PathBuf, Option<Duration>), String> {
    cc_cached_kill(compiler, code, stem, source, dir, opt, capture, None)
}

/// `cc_cached`, and the C compiler can be cancelled through `killer` (see `auto::Build`).
#[allow(clippy::too_many_arguments)]
fn cc_cached_kill(
    compiler: &str,
    code: &str,
    stem: &str,
    source: &str,
    dir: &Path,
    opt: Opt,
    capture: bool,
    killer: Option<&auto::Killer>,
) -> Result<(PathBuf, Option<Duration>), String> {
    let slot = cc_slot(compiler, code, stem, source, dir, opt);
    cached(code, "c", slot, dir, |src, exe| cc(compiler, src, exe, opt, capture, killer))
}

/// Where the executable for `code` is (or will be) kept. The builds of each optimization level
/// are kept apart, so `run` and `build` of one file do not evict each other.
fn cc_slot(compiler: &str, code: &str, stem: &str, source: &str, dir: &Path, opt: Opt) -> Slot {
    let key = format!("{compiler} | {} {}", opt.flag(), CC_FLAGS.join(" "));
    let variant = if opt == Opt::Fast { "fast" } else { "" };
    slot(code, stem, source, "c", variant, &key, dir)
}

/// The cached executable for `code`, if a build of it exists (nothing is compiled).
fn cc_lookup(compiler: &str, code: &str, stem: &str, source: &str, dir: &Path, opt: Opt) -> Option<PathBuf> {
    let slot = cc_slot(compiler, code, stem, source, dir, opt);
    slot.exe.exists().then_some(slot.exe)
}

/// Flags for rustc: optimized, and int overflow wraps (Nyra's `int`), as in a release build.
const RUSTC_FLAGS: &[&str] =
    &["--edition", "2021", "-C", "opt-level=2", "-C", "overflow-checks=off", "-C", "debuginfo=0", "--cap-lints", "allow"];

/// Compiles generated Rust with rustc (NYRA_RUSTC, else `rustc`), cached like C.
fn rust_cached(code: &str, stem: &str, source: &str) -> Result<(PathBuf, Option<Duration>), String> {
    let rustc = std::env::var("NYRA_RUSTC").unwrap_or_else(|_| "rustc".to_string());
    if !works(&rustc, &["--version"]) {
        return Err("no Rust compiler found (tried rustc); install Rust or set NYRA_RUSTC".into());
    }
    let key = format!("{rustc} | {}", RUSTC_FLAGS.join(" "));
    let slot = slot(code, stem, source, "rs", "", &key, &temp_dir());
    cached(code, "rs", slot, &temp_dir(), |src, exe| {
        let t = Instant::now();
        match Command::new(&rustc).args(RUSTC_FLAGS).arg("-o").arg(exe).arg(src).status() {
            Ok(s) if s.success() => Ok(t.elapsed()),
            _ => Err(format!("`{rustc}` failed to compile the generated Rust (this is a nyra bug)")),
        }
    })
}

/// Compiles generated Go with `go build` (NYRA_GO, else `go`), cached like C.
fn go_cached(code: &str, stem: &str, source: &str) -> Result<(PathBuf, Option<Duration>), String> {
    let go = std::env::var("NYRA_GO").unwrap_or_else(|_| "go".to_string());
    if !works(&go, &["version"]) {
        return Err("no Go toolchain found (tried go); install Go or set NYRA_GO".into());
    }
    let slot = slot(code, stem, source, "go", "", &go, &temp_dir());
    cached(code, "go", slot, &temp_dir(), |src, exe| {
        let t = Instant::now();
        // one file builds without a module (it must end in `.go`)
        match Command::new(&go).args(["build", "-o"]).arg(exe).arg(src).status() {
            Ok(s) if s.success() => Ok(t.elapsed()),
            _ => Err(format!("`{go} build` failed to compile the generated Go (this is a nyra bug)")),
        }
    })
}

/// The files of one cached build.
struct Slot {
    /// The builds of one source file share a group.
    group: String,
    /// The hash of the code and of the tool that compiles it.
    key: String,
    src_path: PathBuf,
    exe: PathBuf,
}

/// The names of the build of `code` with the tool described by `key`; nothing is read or written.
fn slot(code: &str, stem: &str, source: &str, ext: &str, variant: &str, key: &str, dir: &Path) -> Slot {
    let key = format!("{:016x}", fnv1a(&[code.as_bytes(), key.as_bytes()]));
    // Builds are grouped per source file, so two projects that both have a
    // `main.nyra` never evict each other's cached executables.
    let full = std::fs::canonicalize(source).unwrap_or_else(|_| PathBuf::from(source));
    let full_text = full.to_string_lossy().into_owned();
    let mut parts: Vec<&[u8]> = vec![full_text.as_bytes()];
    // (C builds keep the group names they had before the other compiled targets came)
    if ext != "c" {
        parts.push(ext.as_bytes());
    }
    if !variant.is_empty() {
        parts.push(variant.as_bytes());
    }
    let group = format!("{stem}-{:08x}-", fnv1a(&parts) as u32);
    let src_path = dir.join(format!("{group}{key}.{ext}"));
    let exe = src_path.with_file_name(format!("{group}{key}{}", std::env::consts::EXE_SUFFIX));
    Slot { group, key, src_path, exe }
}

/// Compiles generated code into the executable of `slot` with `build(source, exe)`. If it already
/// exists (the same code, compiled with the same tool), it is reused.
/// Returns the executable and the build time (`None` when cached).
fn cached(
    code: &str,
    ext: &str,
    slot: Slot,
    dir: &Path,
    build: impl FnOnce(&Path, &Path) -> Result<Duration, String>,
) -> Result<(PathBuf, Option<Duration>), String> {
    let Slot { group, key, src_path, exe } = slot;
    let exe_suffix = std::env::consts::EXE_SUFFIX;
    if exe.exists() {
        return Ok((exe, None));
    }
    let file_name = src_path.file_name().and_then(|n| n.to_str()).unwrap_or("main").to_string();
    write_temp(dir, &file_name, code)?;

    // Build to a temporary, per-process name first so an interrupted or
    // concurrent build never looks cached.
    let partial = src_path.with_file_name(format!("{group}{key}.{}.partial{exe_suffix}", std::process::id()));
    let t = match build(&src_path, &partial) {
        Ok(t) => t,
        Err(e) => {
            let _ = std::fs::remove_file(&partial);
            return Err(e);
        }
    };
    if let Err(e) = std::fs::rename(&partial, &exe) {
        let _ = std::fs::remove_file(&partial);
        if !exe.exists() {
            return Err(format!("cannot write `{}`: {e}", exe.display()));
        }
    }

    // Drop older builds of this program so the cache doesn't grow forever.
    let src_ext = format!(".{ext}");
    if let Some(dir) = exe.parent() {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(rest) = name.strip_prefix(&group) else { continue };
            let (hash, rest_ext) = rest.split_at(rest.len().min(16));
            let ours = hash.len() == 16 && hash.chars().all(|c| c.is_ascii_hexdigit());
            if ours && hash != key && (rest_ext == src_ext || rest_ext == exe_suffix) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    Ok((exe, Some(t)))
}

/// Runs the C compiler. Returns how long it took.
fn cc(cc: &str, c_path: &Path, exe: &Path, opt: Opt, capture: bool, killer: Option<&auto::Killer>) -> Result<Duration, String> {
    let mut cmd = Command::new(cc);
    // A compiler given by full path needs its own directory on PATH to find its DLLs/tools.
    if let Some(dir) = Path::new(&cc).parent().filter(|d| !d.as_os_str().is_empty()) {
        let path = std::env::var_os("PATH").unwrap_or_default();
        if let Ok(joined) = std::env::join_paths(std::iter::once(dir.to_path_buf()).chain(std::env::split_paths(&path))) {
            cmd.env("PATH", joined);
        }
    }
    let t = Instant::now();
    cmd.arg(opt.flag()).args(CC_FLAGS).arg("-o").arg(exe).arg(c_path);
    // the math library (`math.sqrt`, `math.floor`) is separate outside Windows; it goes after the source
    #[cfg(not(windows))]
    cmd.arg("-lm");
    let failed = format!("`{cc}` failed to compile the generated C (this is a nyra bug)");
    if let Some(killer) = killer {
        return cc_killable(cmd, killer, capture, t, failed);
    }
    if !capture {
        return match cmd.status() {
            Ok(s) if s.success() => Ok(t.elapsed()),
            _ => Err(failed),
        };
    }
    match cmd.stdin(Stdio::null()).output() {
        Ok(out) if out.status.success() => Ok(t.elapsed()),
        Ok(out) => {
            let text = String::from_utf8_lossy(&out.stderr);
            let short: String = text.trim().chars().take(2000).collect();
            Err(if short.is_empty() {
                failed
            } else {
                format!(
                    "{failed}:
{short}"
                )
            })
        }
        Err(_) => Err(failed),
    }
}

/// Runs a C compiler that `killer` can stop: the compiler with everything it starts is one tree
/// (see `limits::Tree`), and it gets a temporary folder of its own that the caller removes.
fn cc_killable(mut cmd: Command, killer: &auto::Killer, capture: bool, t: Instant, failed: String) -> Result<Duration, String> {
    if killer.cancelled() {
        return Err(failed);
    }
    let _ = std::fs::create_dir_all(killer.tmp());
    cmd.env("TMPDIR", killer.tmp()).env("TMP", killer.tmp()).env("TEMP", killer.tmp()).stdin(Stdio::null());
    if capture {
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    }
    limits::Tree::prepare(&mut cmd);
    let Ok(child) = cmd.spawn() else { return Err(failed) };
    killer.attach(&child);
    match child.wait_with_output() {
        Ok(out) if out.status.success() => Ok(t.elapsed()),
        Ok(out) => {
            let text = String::from_utf8_lossy(&out.stderr);
            let short: String = text.trim().chars().take(2000).collect();
            Err(if short.is_empty() {
                failed
            } else {
                format!(
                    "{failed}:
{short}"
                )
            })
        }
        Err(_) => Err(failed),
    }
}

fn find_cc() -> Option<String> {
    if let Ok(cc) = std::env::var("NYRA_CC") {
        return Some(cc);
    }
    // On Windows, MSYS2's native toolchains work out of the box while the
    // `gcc` on PATH is often the POSIX-emulation one without headers.
    #[cfg(windows)]
    for cc in [r"C:\msys64\ucrt64\bin\gcc.exe", r"C:\msys64\mingw64\bin\gcc.exe", r"C:\msys64\clang64\bin\clang.exe"] {
        if Path::new(cc).exists() {
            return Some(cc.to_string());
        }
    }
    ["gcc", "clang", "cc", "tcc"].into_iter().find(|cc| Command::new(cc).arg("--version").output().is_ok()).map(String::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The commands the dispatcher knows: `FILE_COMMANDS` and the names `real_main` matches.
    fn dispatched() -> Vec<String> {
        let src = include_str!("main.rs");
        let body = &src[src.find("fn real_main()").expect("real_main")..];
        let body = &body[..body.find("\n}\n").unwrap_or(body.len())];
        let mut names: Vec<String> = FILE_COMMANDS.iter().map(|s| s.to_string()).collect();
        for line in body.lines().filter(|l| l.trim_start().starts_with("Some(")) {
            // the names matched, not the conditions after them
            let pattern = line.split(" if ").next().unwrap_or(line).split(" =>").next().unwrap_or(line);
            for part in pattern.split('"').skip(1).step_by(2) {
                let name = part.trim_start_matches('-');
                if !name.is_empty() && name.chars().all(|c| c.is_ascii_lowercase() || c == '-') && !names.iter().any(|n| n == name) {
                    names.push(name.to_string());
                }
            }
        }
        names
    }

    #[test]
    fn every_command_is_in_both_helps() {
        let agent = agent_help();
        let human = usage();
        let names = dispatched();
        assert!(names.len() >= 11, "{names:?}");
        for name in names.iter().filter(|n| *n != "agent-help") {
            assert!(COMMANDS.iter().any(|c| c.0 == name), "`{name}` is dispatched but not in COMMANDS");
            assert!(agent.contains(&format!("nyra {name}")), "`nyra help agent` misses `{name}`");
            assert!(human.contains(&format!("nyra {name}")), "`nyra help` misses `{name}`");
        }
        // one row per command, sorted, so another branch adds its rows without conflicts
        assert!(COMMANDS.windows(2).all(|w| w[0].0 < w[1].0), "COMMANDS must be sorted by name");
        // compact: one short line a command, about 330 tokens for the 19 commands (4 characters a token)
        assert!(agent.len() < 1350, "`nyra help agent` is {} characters", agent.len());
    }
}
