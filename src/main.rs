mod ast;
mod check;
mod check_v03;
mod codegen;
mod diag;
mod explain;
mod hints;
mod ir;
mod lexer;
mod parser;

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::{Duration, Instant};

const USAGE: &str = "\
nyra - a language for AI agents

usage:
  nyra run   <file.nyra>    compile and run
  nyra build <file.nyra>    compile to a native executable
  nyra check <file.nyra>    only check for errors
  nyra explain [CODE]       explain an error code (without CODE: list all codes)
  nyra <file.nyra>          same as `nyra run`

options:
  --target <t> the backend: native (default), c, js, py, ts, rs or go
  --js --py --ts --rs --go   short for --target js / py / ts / rs / go
  --c          (build) write the generated C source instead of an executable
  -o <path>    output path for `build` (`-o -` prints to stdout)
  --json       print errors as JSON (compile and runtime), for AI agents
  --time       show how long each step took

`build` writes an executable natively and source code for every other target.
`run` needs the target's tool: a C compiler (gcc, clang, cc or tcc; NYRA_CC),
node (js and ts), python3 (NYRA_PYTHON), rustc (NYRA_RUSTC) or go (NYRA_GO).
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
}

fn parse_args() -> Result<Opts, String> {
    let mut args = std::env::args().skip(1);
    let mut positional = Vec::new();
    let mut opts =
        Opts { cmd: String::new(), file: String::new(), target: Target::Native, out: None, json: false, time: false };
    while let Some(a) = args.next() {
        match a.as_str() {
            "-h" | "--help" | "help" => return Err(USAGE.to_string()),
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
    let (toks, errs) = lexer::lex(src);
    if !errs.is_empty() {
        return Err(errs);
    }
    let (mut prog, errs) = parser::parse(toks);
    if !errs.is_empty() {
        return Err(errs);
    }
    let errs = check::check(&mut prog);
    if !errs.is_empty() {
        return Err(errs);
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
    let prog = match compile(&src) {
        Ok(p) => p,
        Err(diags) => {
            if opts.json {
                println!("{}", diag::render_json(&diags, &opts.file));
            } else {
                eprint!("{}", diag::render_human(&diags, &opts.file, &src));
                eprintln!("nyra: {} error(s)", diags.len());
            }
            return ExitCode::from(1);
        }
    };

    if opts.cmd == "check" {
        if opts.json {
            println!("{}", diag::render_json(&[], &opts.file));
        } else {
            eprintln!("nyra: no errors ({})", ms(start.elapsed()));
        }
        return ExitCode::SUCCESS;
    }

    let mut module = match ir::lower::lower(&prog) {
        Ok(m) => m,
        Err(what) => return fail(format!("not supported yet: {what} (coming later in v0.3)")),
    };
    // NYRA_OPT=0 (for tests and debugging) skips the optimization passes
    if std::env::var_os("NYRA_OPT").is_none_or(|v| v != "0") {
        ir::opt::optimize(&mut module);
    }
    if let Err(e) = ir::verify::verify(&module) {
        return fail(format!("internal error: the compiler produced invalid IR ({e}); please report this bug"));
    }
    if std::env::var_os("NYRA_DUMP").is_some_and(|v| v == "ir") {
        eprint!("{}", ir::print::print(&module));
    }
    let code = match opts.target {
        Target::Js => codegen::js::gen(&module, &opts.file),
        Target::Py => codegen::py::gen(&module, &opts.file),
        Target::Ts => codegen::ts::gen(&module, &opts.file),
        Target::Rs => codegen::rs::gen(&module, &opts.file),
        Target::Go => codegen::go::gen(&module, &opts.file),
        Target::Native | Target::C => codegen::c::gen(&module, &opts.file),
    };
    let nyra_time = start.elapsed();
    let stem = Path::new(&opts.file).file_stem().and_then(|s| s.to_str()).unwrap_or("main").to_string();

    if opts.cmd == "build" {
        build(&opts, &code, &stem, nyra_time)
    } else {
        run(&opts, &code, &stem, nyra_time)
    }
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
        let (exe, cc_time) = match cc_cached(code, stem, &opts.file) {
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
    // the tool's own compile step (C, Rust, Go), when there is one: `None` when it was cached
    let mut build_time: Option<Option<Duration>> = None;
    let mut cmd = match opts.target {
        Target::Js | Target::Ts | Target::Py => {
            let path = match write_temp(&format!("{stem}.{}", opts.target.ext()), code) {
                Ok(p) => p,
                Err(code) => return code,
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
                Target::Rs => rust_cached(code, stem, &opts.file),
                Target::Go => go_cached(code, stem, &opts.file),
                _ => cc_cached(code, stem, &opts.file),
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

    if opts.json {
        // the program's runtime errors are then printed as JSON too
        cmd.env("NYRA_JSON", "1");
    }
    let t = Instant::now();
    let status = cmd.status();
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

fn write_temp(name: &str, contents: &str) -> Result<PathBuf, ExitCode> {
    let dir = std::env::temp_dir().join("nyra");
    std::fs::create_dir_all(&dir).map_err(|e| fail(format!("cannot create `{}`: {e}", dir.display())))?;
    let path = dir.join(name);
    std::fs::write(&path, contents).map_err(|e| fail(format!("cannot write `{}`: {e}", path.display())))?;
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

/// Compiles generated C into an executable in the temp dir. If the same C code was
/// already compiled with the same compiler and flags, the old executable is reused.
/// Returns the executable and the C compiler's time (`None` when cached).
fn cc_cached(code: &str, stem: &str, source: &str) -> Result<(PathBuf, Option<Duration>), ExitCode> {
    let Some(compiler) = find_cc() else {
        return Err(fail("no C compiler found (tried gcc, clang, cc, tcc); install one, set NYRA_CC, or use --js"));
    };
    let key = [compiler.as_str(), &CC_FLAGS.join(" ")].join(" | ");
    cached(code, stem, source, "c", &key, |src, exe| cc(&compiler, src, exe))
}

/// Flags for rustc: optimized, and int overflow wraps (Nyra's `int`), as in a release build.
const RUSTC_FLAGS: &[&str] = &["--edition", "2021", "-C", "opt-level=2", "-C", "overflow-checks=off", "-C", "debuginfo=0", "--cap-lints", "allow"];

/// Compiles generated Rust with rustc (NYRA_RUSTC, else `rustc`), cached like C.
fn rust_cached(code: &str, stem: &str, source: &str) -> Result<(PathBuf, Option<Duration>), ExitCode> {
    let rustc = std::env::var("NYRA_RUSTC").unwrap_or_else(|_| "rustc".to_string());
    if !works(&rustc, &["--version"]) {
        return Err(fail("no Rust compiler found (tried rustc); install Rust or set NYRA_RUSTC"));
    }
    let key = [rustc.as_str(), &RUSTC_FLAGS.join(" ")].join(" | ");
    cached(code, stem, source, "rs", &key, |src, exe| {
        let t = Instant::now();
        let status = Command::new(&rustc).args(RUSTC_FLAGS).arg("-o").arg(exe).arg(src).status();
        match status {
            Ok(s) if s.success() => Ok(t.elapsed()),
            _ => Err(fail(format!("`{rustc}` failed to compile the generated Rust (this is a nyra bug)"))),
        }
    })
}

/// Compiles generated Go with `go build` (NYRA_GO, else `go`), cached like C.
fn go_cached(code: &str, stem: &str, source: &str) -> Result<(PathBuf, Option<Duration>), ExitCode> {
    let go = std::env::var("NYRA_GO").unwrap_or_else(|_| "go".to_string());
    if !works(&go, &["version"]) {
        return Err(fail("no Go toolchain found (tried go); install Go or set NYRA_GO"));
    }
    cached(code, stem, source, "go", &go, |src, exe| {
        let t = Instant::now();
        // a single file builds without a module; the source file must end in `.go`
        let status = Command::new(&go).args(["build", "-o"]).arg(exe).arg(src).env("GO111MODULE", "on").status();
        match status {
            Ok(s) if s.success() => Ok(t.elapsed()),
            _ => Err(fail(format!("`{go} build` failed to compile the generated Go (this is a nyra bug)"))),
        }
    })
}

/// Compiles generated code into an executable in the temp dir with `build(source, exe)`. If the
/// same code was already compiled with the same tool (`key`), the old executable is reused.
/// Returns the executable and the build time (`None` when cached).
fn cached(
    code: &str,
    stem: &str,
    source: &str,
    ext: &str,
    key: &str,
    build: impl FnOnce(&Path, &Path) -> Result<Duration, ExitCode>,
) -> Result<(PathBuf, Option<Duration>), ExitCode> {
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
    let group = format!("{stem}-{:08x}-", fnv1a(&parts) as u32);
    let exe_suffix = std::env::consts::EXE_SUFFIX;
    let src_path = write_temp(&format!("{group}{key}.{ext}"), code)?;
    let exe = src_path.with_file_name(format!("{group}{key}{exe_suffix}"));
    if exe.exists() {
        return Ok((exe, None));
    }

    // Build to a temporary, per-process name first so an interrupted or
    // concurrent build never looks cached.
    let partial = src_path.with_file_name(format!("{group}{key}.{}.partial{exe_suffix}", std::process::id()));
    let t = build(&src_path, &partial)?;
    if let Err(e) = std::fs::rename(&partial, &exe) {
        let _ = std::fs::remove_file(&partial);
        if !exe.exists() {
            return Err(fail(format!("cannot write `{}`: {e}", exe.display())));
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
fn cc(cc: &str, c_path: &Path, exe: &Path) -> Result<Duration, ExitCode> {
    let mut cmd = Command::new(cc);
    // A compiler given by full path needs its own directory on PATH to find its DLLs/tools.
    if let Some(dir) = Path::new(&cc).parent().filter(|d| !d.as_os_str().is_empty()) {
        let path = std::env::var_os("PATH").unwrap_or_default();
        if let Ok(joined) = std::env::join_paths(std::iter::once(dir.to_path_buf()).chain(std::env::split_paths(&path))) {
            cmd.env("PATH", joined);
        }
    }
    let t = Instant::now();
    let status = cmd.args(CC_FLAGS).arg("-o").arg(exe).arg(c_path).status();
    match status {
        Ok(s) if s.success() => Ok(t.elapsed()),
        _ => Err(fail(format!("`{cc}` failed to compile the generated C (this is a nyra bug)"))),
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
