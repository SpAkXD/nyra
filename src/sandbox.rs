//! Running a whole program in the IR interpreter: `nyra run --interp`, `nyra run --sandbox`, and
//! the MCP tool `nyra_run` with `sandbox: true`.
//!
//! No child process and no C compiler: the program is lowered to the IR and interpreted in this
//! process, under limits that do not depend on the machine:
//!
//! | limit | flag | default | stops with |
//! |---|---|---|---|
//! | steps (statements run, elements and bytes made) | `--fuel N` | 2,000,000,000 | E0355, exit 120 |
//! | heap memory | `--max-memory SIZE` | 512 MiB | E0356, exit 121 |
//! | bytes printed | `--max-output SIZE` | 64 MiB | E0357, exit 122 |
//! | nested calls | `--max-depth N` | 20,000 | E0358, exit 123 |
//! | real time (off unless given) | `--max-time MS` | none | E0359, exit 124 |
//!
//! `--sandbox` also grants no capability but the ones named with `--allow`, and confines file paths
//! to the folder the program runs in. `time.sleep_ms` does not wait: it moves a virtual clock and
//! costs steps (see `ir/host.rs`).

use crate::ast::Program;
use crate::clock::Instant;
use crate::ir::host::{Host, Out};
use crate::ir::interp::{Interp, Limits, RuntimeError, Stop};
use crate::ir::{self};

pub const DEFAULT_FUEL: u64 = 2_000_000_000;
pub const DEFAULT_MEMORY: u64 = 512 << 20;
pub const DEFAULT_OUTPUT: u64 = 64 << 20;
pub const DEFAULT_DEPTH: usize = 20_000;
/// The most nested calls a run may be given: an interpreted call uses about a kilobyte of stack.
pub const MAX_DEPTH: usize = 100_000;

/// The exit codes of a run that a limit stopped (`E0355`..`E0359` in this order).
pub const LIMIT_EXITS: [(&str, i32); 5] = [("E0355", 120), ("E0356", 121), ("E0357", 122), ("E0358", 123), ("E0359", 124)];

/// `64M`, `512k`, `1G`, `4096`: bytes with an optional binary suffix.
pub fn parse_size(text: &str) -> Result<u64, String> {
    let t = text.trim();
    let (digits, mult) = match t.chars().last() {
        Some('k' | 'K') => (&t[..t.len() - 1], 1u64 << 10),
        Some('m' | 'M') => (&t[..t.len() - 1], 1 << 20),
        Some('g' | 'G') => (&t[..t.len() - 1], 1 << 30),
        _ => (t, 1),
    };
    digits
        .parse::<u64>()
        .ok()
        .and_then(|n| n.checked_mul(mult))
        .ok_or_else(|| format!("`{text}` is not a size: write bytes, or a number with k, M or G (`64M`)"))
}

pub struct Config {
    pub limits: Limits,
    /// The program's own arguments (`os.args()`).
    pub args: Vec<String>,
    /// Standard input given by the caller; `None`: the process's own.
    pub stdin: Option<Vec<u8>>,
    /// File paths must stay below the working folder.
    pub confined: bool,
    /// Write straight to the process's standard output; else keep the output in `Report::stdout`.
    pub to_stdout: bool,
}

impl Config {
    pub fn new() -> Config {
        Config {
            limits: Limits { steps: DEFAULT_FUEL, depth: DEFAULT_DEPTH, memory: DEFAULT_MEMORY, output: DEFAULT_OUTPUT, wall_ms: 0 },
            args: Vec::new(),
            stdin: None,
            confined: false,
            to_stdout: true,
        }
    }
}

/// How a run ended.
pub struct Report {
    /// The process exit code: 0, `os.exit`'s, 101 for a runtime error, 120 to 124 for a limit.
    pub exit: i32,
    /// What the program printed, when it was kept (`to_stdout` false).
    pub stdout: String,
    /// The runtime error or the limit that stopped the program.
    pub error: Option<RuntimeError>,
    /// The interpreter met something it cannot do (a bug of nyra, not of the program): exit 70.
    pub internal: Option<String>,
    pub steps: u64,
    /// Allocations the run made (the interpreter's own included).
    pub allocs: u64,
    /// Milliseconds spent running (without compiling).
    pub run_ms: f64,
}

/// Lowers a checked program, as every backend does (`NYRA_OPT=0` skips the optimizer).
pub fn module(prog: &Program) -> Result<ir::Module, String> {
    let mut m = ir::lower::lower(prog).map_err(|what| format!("not supported yet: {what} (coming later in v0.3)"))?;
    if std::env::var_os("NYRA_OPT").is_none_or(|v| v != "0") {
        ir::opt::optimize(&mut m);
    }
    if let Err(e) = ir::verify::verify(&m) {
        return Err(format!("internal error: the compiler produced invalid IR ({e}); please report this bug"));
    }
    Ok(m)
}

/// Runs `main` of a lowered program.
pub fn run(m: &ir::Module, cfg: Config) -> Report {
    let mut host = Host::new();
    host.args = cfg.args;
    host.confined = cfg.confined;
    if let Some(bytes) = cfg.stdin {
        host = host.with_input(bytes);
    }
    let out = if cfg.to_stdout { Out::stdout() } else { Out::keep() };
    let mut it = Interp::with_host(m, cfg.limits, host, out);
    let start = Instant::now();
    let allocs0 = crate::mem::allocs();
    let result = it.call(m.main, Vec::new());
    let run_ms = start.elapsed().as_secs_f64() * 1000.0;
    it.flush();
    let mut internal = None;
    let (exit, error) = match result {
        Ok(_) => (0, None),
        Err(Stop::Exit(code)) => (code, None),
        Err(Stop::Error(e)) => (101, Some(*e)),
        Err(Stop::Bug(what)) => {
            internal = Some(what);
            (70, None)
        }
        Err(limit) => match it.limit_error(&limit) {
            Some(e) => {
                let exit = LIMIT_EXITS.iter().find(|(c, _)| *c == e.code).map_or(101, |(_, x)| *x);
                (exit, Some(e))
            }
            None => (101, None),
        },
    };
    let stdout = it.output().to_string();
    Report { exit, stdout, error, internal, steps: it.steps(), allocs: crate::mem::allocs() - allocs0, run_ms }
}

/// A runtime error as the compiled programs print it: human or JSON (one line).
pub fn render_error(e: &RuntimeError, file: &str, json: bool) -> String {
    if json {
        format!(
            "{{\"ok\":false,\"errors\":[{{\"code\":\"{}\",\"message\":{},\"file\":{},\"line\":{},\"col\":{},\"hint\":{},\"runtime\":true}}]}}",
            e.code,
            crate::diag::json_str(&e.msg),
            crate::diag::json_str(file),
            e.span.line,
            e.span.col,
            crate::diag::json_str(e.hint)
        )
    } else {
        format!(
            "runtime error[{}]: {}\n  --> {file}:{}:{}\n  = hint: {}\n  = explain: nyra explain {}",
            e.code, e.msg, e.span.line, e.span.col, e.hint, e.code
        )
    }
}
