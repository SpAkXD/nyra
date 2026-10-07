mod ast;
mod check;
mod codegen;
mod diag;
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
  nyra <file.nyra>          same as `nyra run`

options:
  --js         use the JavaScript backend instead of native
  --c          (build) write the generated C source instead of an executable
  -o <path>    output path for `build` (`-o -` prints to stdout)
  --json       print errors as JSON, for AI agents and tools
  --time       show how long each step took

The C compiler is picked automatically (gcc, clang, cc or tcc);
set NYRA_CC to use a specific one.
";

/// Flags nyra passes to the C compiler. Kept deliberately short:
/// -O2      optimize
/// -fwrapv  make int overflow wrap (defined behavior, as the spec says)
const CC_FLAGS: &[&str] = &["-O2", "-fwrapv"];

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

    let code = match opts.target {
        Target::Js => codegen::js::gen(&prog),
        Target::Native | Target::C => codegen::c::gen(&prog),
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

    let mut cc_time = Duration::ZERO;
    if opts.target == Target::Native {
        let c_path = match write_temp(&format!("{stem}.c"), code) {
            Ok(p) => p,
            Err(code) => return code,
        };
        match cc(&c_path, &out) {
            Ok(t) => cc_time = t,
            Err(code) => return code,
        }
    } else if let Err(e) = std::fs::write(&out, code) {
        return fail(format!("cannot write `{}`: {e}", out.display()));
    }

    let cc_part = if opts.target == Target::Native { format!(" + cc {}", ms(cc_time)) } else { String::new() };
    eprintln!("nyra: built {} (nyra {}{cc_part})", out.display(), ms(nyra_time));
    ExitCode::SUCCESS
}

fn run(opts: &Opts, code: &str, stem: &str, nyra_time: Duration) -> ExitCode {
    let mut cc_time = Duration::ZERO;
    let mut cmd = if opts.target == Target::Js {
        let js_path = match write_temp(&format!("{stem}.js"), code) {
            Ok(p) => p,
            Err(code) => return code,
        };
        let mut c = Command::new("node");
        c.arg(js_path);
        c
    } else {
        let c_path = match write_temp(&format!("{stem}.c"), code) {
            Ok(p) => p,
            Err(code) => return code,
        };
        let exe = c_path.with_extension(std::env::consts::EXE_EXTENSION);
        match cc(&c_path, &exe) {
            Ok(t) => cc_time = t,
            Err(code) => return code,
        }
        Command::new(exe)
    };

    let t = Instant::now();
    let status = cmd.status();
    let run_time = t.elapsed();
    if opts.time {
        let cc_part = if opts.target == Target::Js { String::new() } else { format!(" | cc {}", ms(cc_time)) };
        eprintln!("nyra {}{cc_part} | run {}", ms(nyra_time), ms(run_time));
    }
    match status {
        Ok(s) => ExitCode::from(s.code().unwrap_or(1) as u8),
        Err(e) => fail(format!("failed to start the program: {e}")),
    }
}

fn write_temp(name: &str, contents: &str) -> Result<PathBuf, ExitCode> {
    let dir = std::env::temp_dir().join("nyra");
    std::fs::create_dir_all(&dir).map_err(|e| fail(format!("cannot create `{}`: {e}", dir.display())))?;
    let path = dir.join(name);
    std::fs::write(&path, contents).map_err(|e| fail(format!("cannot write `{}`: {e}", path.display())))?;
    Ok(path)
}

/// Compiles generated C into an executable. Returns how long the C compiler took.
fn cc(c_path: &Path, exe: &Path) -> Result<Duration, ExitCode> {
    let Some(cc) = find_cc() else {
        return Err(fail("no C compiler found (tried gcc, clang, cc, tcc); install one, set NYRA_CC, or use --js"));
    };
    let mut cmd = Command::new(&cc);
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
