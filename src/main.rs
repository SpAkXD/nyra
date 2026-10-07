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

usage: nyra <command> <file.nyra> [options]

commands:
  run      compile and run the program (default target: c)
  build    compile to a source file next to the input (or -o <path>)
  emit     print the generated code to stdout
  check    type-check only

options:
  -t, --target <c|js>   backend to use
  -o <path>             output path for `build`
  --json                print errors as JSON (for AI agents and tools)
  --time                print how long each step took

env:
  NYRA_CC               C compiler to use (default: first of gcc, clang, cc, tcc)
";

#[derive(Clone, Copy, PartialEq)]
enum Target {
    C,
    Js,
}

impl Target {
    fn ext(self) -> &'static str {
        match self {
            Target::C => "c",
            Target::Js => "js",
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
    let mut opts = Opts { cmd: String::new(), file: String::new(), target: Target::C, out: None, json: false, time: false };
    while let Some(a) = args.next() {
        match a.as_str() {
            "-h" | "--help" | "help" => return Err(USAGE.to_string()),
            "-V" | "--version" | "version" => return Err(format!("nyra {}", env!("CARGO_PKG_VERSION"))),
            "-t" | "--target" => {
                opts.target = match args.next().as_deref() {
                    Some("c") => Target::C,
                    Some("js") => Target::Js,
                    other => return Err(format!("unknown target {other:?} (expected `c` or `js`)")),
                }
            }
            "-o" => opts.out = Some(args.next().ok_or("-o needs a path")?),
            "--json" => opts.json = true,
            "--time" => opts.time = true,
            _ if a.starts_with('-') => return Err(format!("unknown option `{a}`\n\n{USAGE}")),
            _ => positional.push(a),
        }
    }
    // `nyra file.nyra` is shorthand for `nyra run file.nyra`.
    if positional.len() == 1 && positional[0].ends_with(".nyra") {
        positional.insert(0, "run".into());
    }
    match positional.as_slice() {
        [cmd, file] if ["run", "build", "emit", "check"].contains(&cmd.as_str()) => {
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
        Err(e) => {
            eprintln!("nyra: cannot read `{}`: {e}", opts.file);
            return ExitCode::from(2);
        }
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
            eprintln!("nyra: ok ({})", ms(start.elapsed()));
        }
        return ExitCode::SUCCESS;
    }

    let code = match opts.target {
        Target::C => codegen::c::gen(&prog),
        Target::Js => codegen::js::gen(&prog),
    };
    let compile_time = start.elapsed();

    match opts.cmd.as_str() {
        "emit" => {
            print!("{code}");
            ExitCode::SUCCESS
        }
        "build" => {
            let out = opts
                .out
                .map(PathBuf::from)
                .unwrap_or_else(|| Path::new(&opts.file).with_extension(opts.target.ext()));
            if let Err(e) = std::fs::write(&out, code) {
                eprintln!("nyra: cannot write `{}`: {e}", out.display());
                return ExitCode::from(2);
            }
            eprintln!("nyra: built {} in {}", out.display(), ms(compile_time));
            ExitCode::SUCCESS
        }
        _ => run(&opts, &code, compile_time),
    }
}

fn run(opts: &Opts, code: &str, compile_time: Duration) -> ExitCode {
    let stem = Path::new(&opts.file).file_stem().and_then(|s| s.to_str()).unwrap_or("main");
    let dir = std::env::temp_dir().join("nyra");
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("nyra: cannot create `{}`: {e}", dir.display());
        return ExitCode::from(2);
    }
    let src_path = dir.join(format!("{stem}.{}", opts.target.ext()));
    if let Err(e) = std::fs::write(&src_path, code) {
        eprintln!("nyra: cannot write `{}`: {e}", src_path.display());
        return ExitCode::from(2);
    }

    let mut backend_time = Duration::ZERO;
    let mut cmd = match opts.target {
        Target::Js => {
            let mut c = Command::new("node");
            c.arg(&src_path);
            c
        }
        Target::C => {
            let Some(cc) = find_cc() else {
                eprintln!("nyra: no C compiler found (tried gcc, clang, cc, tcc); install one or set NYRA_CC");
                return ExitCode::from(2);
            };
            let exe = dir.join(format!("{stem}{}", std::env::consts::EXE_SUFFIX));
            let mut cc_cmd = Command::new(&cc);
            // A compiler given by full path needs its own directory on PATH to find its DLLs/tools.
            if let Some(dir) = Path::new(&cc).parent().filter(|d| !d.as_os_str().is_empty()) {
                let path = std::env::var_os("PATH").unwrap_or_default();
                let joined = std::env::join_paths(std::iter::once(dir.to_path_buf()).chain(std::env::split_paths(&path)));
                if let Ok(joined) = joined {
                    cc_cmd.env("PATH", joined);
                }
            }
            let t = Instant::now();
            let status = cc_cmd.args(["-O2", "-fwrapv", "-o"]).arg(&exe).arg(&src_path).status();
            backend_time = t.elapsed();
            match status {
                Ok(s) if s.success() => {}
                _ => {
                    eprintln!("nyra: `{cc}` failed to compile the generated C (this is a nyra bug)");
                    return ExitCode::from(2);
                }
            }
            Command::new(exe)
        }
    };

    let t = Instant::now();
    let status = cmd.status();
    let run_time = t.elapsed();
    if opts.time {
        let backend = match opts.target {
            Target::C => format!(" | cc {}", ms(backend_time)),
            Target::Js => String::new(),
        };
        eprintln!("nyra {}{backend} | run {}", ms(compile_time), ms(run_time));
    }
    match status {
        Ok(s) => ExitCode::from(s.code().unwrap_or(1) as u8),
        Err(e) => {
            eprintln!("nyra: failed to start program: {e}");
            ExitCode::from(2)
        }
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
