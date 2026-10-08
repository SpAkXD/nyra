mod ast;
mod check;
mod check_v03;
mod codegen;
mod diag;
mod explain;
mod fix;
mod hints;
mod ir;
mod json;
mod lexer;
mod mcp;
mod parser;

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

const USAGE: &str = "\
nyra - a language for AI agents

usage:
  nyra run   <file.nyra>    compile and run
  nyra build <file.nyra>    compile to a native executable
  nyra check <file.nyra>    only check for errors
  nyra explain [CODE]       explain an error code (without CODE: list all codes)
  nyra mcp                  serve AI agents over the Model Context Protocol (stdio)
  nyra <file.nyra>          same as `nyra run`

options:
  --js         use the JavaScript backend instead of native
  --c          (build) write the generated C source instead of an executable
  -o <path>    output path for `build` (`-o -` prints to stdout)
  --json       print errors as JSON (compile and runtime), for AI agents
  --fix        apply the fixes the errors suggest, check again, and write the
               file back if it then compiles (then run or build it as usual)
  --time       show how long each step took

The C compiler is picked automatically (gcc, clang, cc or tcc);
set NYRA_CC to use a specific one.
";

/// Flags nyra passes to the C compiler. Kept deliberately short:
/// -O2      optimize
/// -fwrapv  make int overflow wrap (defined behavior, as the spec says)
/// -ffp-contract=off  never fuse float operations (a*b+c), so results match JavaScript
/// -s       strip symbols for a smaller executable (macOS's linker ignores it, so it's left out there)
#[cfg(not(target_os = "macos"))]
const CC_FLAGS: &[&str] = &["-O2", "-fwrapv", "-ffp-contract=off", "-s"];
#[cfg(target_os = "macos")]
const CC_FLAGS: &[&str] = &["-O2", "-fwrapv", "-ffp-contract=off"];

#[derive(Clone, Copy, PartialEq)]
enum Target {
    Native,
    C,
    Js,
}

struct Opts {
    cmd: String,
    file: String,
    target: Target,
    out: Option<String>,
    json: bool,
    time: bool,
    fix: bool,
}

fn parse_args() -> Result<Opts, String> {
    let mut args = std::env::args().skip(1);
    let mut positional = Vec::new();
    let mut opts =
        Opts { cmd: String::new(), file: String::new(), target: Target::Native, out: None, json: false, time: false, fix: false };
    while let Some(a) = args.next() {
        match a.as_str() {
            "-h" | "--help" | "help" => return Err(USAGE.to_string()),
            "-V" | "--version" | "version" => return Err(format!("nyra {}", env!("CARGO_PKG_VERSION"))),
            "--js" => opts.target = Target::Js,
            "--c" => opts.target = Target::C,
            "-o" => opts.out = Some(args.next().ok_or("-o needs a path")?),
            "--json" => opts.json = true,
            "--time" => opts.time = true,
            "--fix" => opts.fix = true,
            _ if a.starts_with('-') => return Err(format!("unknown option `{a}`\n\n{USAGE}")),
            _ => positional.push(a),
        }
    }
    if positional.len() == 1 && positional[0].ends_with(".nyra") {
        positional.insert(0, "run".into());
    }
    match positional.as_slice() {
        [cmd, file] if ["run", "build", "check"].contains(&cmd.as_str()) => {
            opts.cmd = cmd.clone();
            opts.file = file.clone();
            Ok(opts)
        }
        _ => Err(USAGE.to_string()),
    }
}

fn compile(src: &str) -> Result<ast::Program, Vec<diag::Diag>> {
    // only fixes that match the source exactly are kept
    let checked = |mut errs: Vec<diag::Diag>| {
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
    let errs = check::check(&mut prog);
    if !errs.is_empty() {
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

fn main() -> ExitCode {
    // `nyra explain [CODE] [--json]` needs no source file
    if std::env::args().nth(1).as_deref() == Some("explain") {
        return explain::run(std::env::args().skip(2).collect());
    }
    if std::env::args().nth(1).as_deref() == Some("mcp") {
        return mcp::run(std::env::args().skip(2).collect());
    }
    let opts = match parse_args() {
        Ok(o) => o,
        Err(msg) => {
            eprintln!("{msg}");
            return ExitCode::from(2);
        }
    };
    let src = match std::fs::read_to_string(&opts.file) {
        Ok(s) => s,
        Err(e) => return fail(format!("cannot read `{}`: {e}", opts.file)),
    };

    let start = Instant::now();
    let mut fixed = 0;
    let prog = match compile(&src) {
        Ok(p) => p,
        Err(diags) => {
            // --fix: apply the fixes and check again; the file changes only if it then compiles
            let repaired = if opts.fix { fix::repair(&src, diags.clone(), compile) } else { None };
            match repaired {
                Some(r) => {
                    if let Err(e) = std::fs::write(&opts.file, &r.text) {
                        return fail(format!("cannot write `{}`: {e}", opts.file));
                    }
                    eprint!("nyra: fixed {} error(s) in {}:
{}", r.fixed, opts.file, fix::diff(&src, &r.text));
                    fixed = r.fixed;
                    r.value
                }
                None => {
                    if opts.json {
                        println!("{}", diag::render_json(&diags, &opts.file));
                    } else {
                        eprint!("{}", diag::render_human(&diags, &opts.file, &src));
                        eprintln!("nyra: {} error(s)", diags.len());
                    }
                    if opts.fix && diags.iter().any(|d| !d.fix.is_empty()) {
                        eprintln!("nyra: --fix did not change {}: errors without a fix remain", opts.file);
                    }
                    return ExitCode::from(1);
                }
            }
        }
    };

    if opts.cmd == "check" {
        if opts.json {
            let json = diag::render_json(&[], &opts.file);
            match fixed {
                0 => println!("{json}"),
                n => println!("{},\"fixed\":{n}}}", &json[..json.len() - 1]),
            }
        } else {
            eprintln!("nyra: no errors ({})", ms(start.elapsed()));
        }
        return ExitCode::SUCCESS;
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

/// The back half of the compiler: a checked program to C or JavaScript source.
/// `file` is the name the generated code reports in runtime errors.
fn generate(prog: &ast::Program, target: Target, file: &str) -> Result<String, String> {
    let mut module = ir::lower::lower(prog).map_err(|what| format!("not supported yet: {what} (coming later in v0.3)"))?;
    // NYRA_OPT=0 (for tests and debugging) skips the optimization passes
    if std::env::var_os("NYRA_OPT").is_none_or(|v| v != "0") {
        ir::opt::optimize(&mut module);
    }
    if let Err(e) = ir::verify::verify(&module) {
        return Err(format!("internal error: the compiler produced invalid IR ({e}); please report this bug"));
    }
    if std::env::var_os("NYRA_DUMP").is_some_and(|v| v == "ir") {
        eprint!("{}", ir::print::print(&module));
    }
    Ok(match target {
        Target::Js => codegen::js::gen(&module, file),
        Target::Native | Target::C => codegen::c::gen(&module, file),
    })
}

const NO_CC: &str = "no C compiler found (tried gcc, clang, cc, tcc); install one, set NYRA_CC, or use --js";

/// The executable for generated C, built in the shared temp dir (see `cc_cached`).
fn native(code: &str, stem: &str, source: &str) -> Result<(PathBuf, Option<Duration>), ExitCode> {
    let Some(compiler) = find_cc() else {
        return Err(fail(NO_CC));
    };
    cc_cached(&compiler, code, stem, source, &temp_dir(), false).map_err(fail)
}

fn build(opts: &Opts, code: &str, stem: &str, nyra_time: Duration) -> ExitCode {
    let ext = match opts.target {
        Target::Native => std::env::consts::EXE_EXTENSION,
        Target::C => "c",
        Target::Js => "js",
    };
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
        let (exe, cc_time) = match native(code, stem, &opts.file) {
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

fn run(opts: &Opts, code: &str, stem: &str, nyra_time: Duration) -> ExitCode {
    let mut cc_time = None;
    let mut cmd = if opts.target == Target::Js {
        let js_path = match write_temp(&temp_dir(), &format!("{stem}.js"), code) {
            Ok(p) => p,
            Err(msg) => return fail(msg),
        };
        let mut c = Command::new("node");
        c.arg(js_path);
        c
    } else {
        let exe = match native(code, stem, &opts.file) {
            Ok((exe, t)) => {
                cc_time = t;
                exe
            }
            Err(code) => return code,
        };
        Command::new(exe)
    };

    if opts.json {
        // the program's runtime errors are then printed as JSON too
        cmd.env("NYRA_JSON", "1");
    }
    let t = Instant::now();
    let status = cmd.status();
    let run_time = t.elapsed();
    if opts.time {
        let cc_part = if opts.target == Target::Js { String::new() } else { format!(" | {}", cc_label(cc_time)) };
        eprintln!("nyra {}{cc_part} | run {}", ms(nyra_time), ms(run_time));
    }
    match status {
        Ok(s) => ExitCode::from(s.code().unwrap_or(1) as u8),
        Err(e) => fail(format!("failed to start the program: {e}")),
    }
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
    capture: bool,
) -> Result<(PathBuf, Option<Duration>), String> {
    let key = format!("{:016x}", fnv1a(&[code.as_bytes(), compiler.as_bytes(), CC_FLAGS.join(" ").as_bytes()]));
    // Builds are grouped per source file, so two projects that both have a
    // `main.nyra` never evict each other's cached executables.
    let full = std::fs::canonicalize(source).unwrap_or_else(|_| PathBuf::from(source));
    let group = format!("{stem}-{:08x}-", fnv1a(&[full.to_string_lossy().as_bytes()]) as u32);
    let exe_suffix = std::env::consts::EXE_SUFFIX;
    let c_path = write_temp(dir, &format!("{group}{key}.c"), code)?;
    let exe = c_path.with_file_name(format!("{group}{key}{exe_suffix}"));
    if exe.exists() {
        return Ok((exe, None));
    }

    // Build to a temporary, per-process name first so an interrupted or
    // concurrent build never looks cached.
    let partial = c_path.with_file_name(format!("{group}{key}.{}.partial{exe_suffix}", std::process::id()));
    let t = cc(compiler, &c_path, &partial, capture)?;
    if let Err(e) = std::fs::rename(&partial, &exe) {
        let _ = std::fs::remove_file(&partial);
        if !exe.exists() {
            return Err(format!("cannot write `{}`: {e}", exe.display()));
        }
    }

    // Drop older builds of this program so the cache doesn't grow forever.
    if let Some(dir) = exe.parent() {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(rest) = name.strip_prefix(&group) else { continue };
            let (hash, ext) = rest.split_at(rest.len().min(16));
            let ours = hash.len() == 16 && hash.chars().all(|c| c.is_ascii_hexdigit());
            if ours && hash != key && (ext == ".c" || ext == exe_suffix) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    Ok((exe, Some(t)))
}

/// Runs the C compiler. Returns how long it took.
fn cc(cc: &str, c_path: &Path, exe: &Path, capture: bool) -> Result<Duration, String> {
    let mut cmd = Command::new(cc);
    // A compiler given by full path needs its own directory on PATH to find its DLLs/tools.
    if let Some(dir) = Path::new(&cc).parent().filter(|d| !d.as_os_str().is_empty()) {
        let path = std::env::var_os("PATH").unwrap_or_default();
        if let Ok(joined) = std::env::join_paths(std::iter::once(dir.to_path_buf()).chain(std::env::split_paths(&path))) {
            cmd.env("PATH", joined);
        }
    }
    let t = Instant::now();
    cmd.args(CC_FLAGS).arg("-o").arg(exe).arg(c_path);
    let failed = format!("`{cc}` failed to compile the generated C (this is a nyra bug)");
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
            Err(if short.is_empty() { failed } else { format!("{failed}:\n{short}") })
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
    ["gcc", "clang", "cc", "tcc"]
        .into_iter()
        .find(|cc| Command::new(cc).arg("--version").output().is_ok())
        .map(String::from)
}
