//! `nyra run` in auto mode: the first output of a small program in milliseconds, the speed of the
//! native executable for a program that runs long.
//!
//! A native `run` waits for the C compiler (about half a second, whatever the program). The
//! interpreter (`ir/interp.rs`) starts at once but is slower than native code, so auto mode uses
//! both:
//!
//! 1. If an executable of this exact source is already cached, run that: nothing is faster.
//! 2. A program that must not run twice goes straight to the C compiler: one that uses `fs` or
//!    `os` (it could write a file or stop with a code twice), sleeps (the interpreter does not
//!    wait), or reads a terminal.
//! 3. Otherwise the program starts in the interpreter, with its output kept in memory, a budget of
//!    steps (`BUDGET`), a time budget (`WALL_MS`) and its standard input read to the end first. Meanwhile, after `DELAY`,
//!    the C compiler starts in a background thread, so its work overlaps with the interpreter's.
//!    - If the program ends within the budget, the compiler is stopped and the output is printed:
//!      the whole run took milliseconds.
//!    - If it does not, the output is thrown away (nothing was shown yet), the program waits for
//!      the compiler and runs natively with the same input. The output appears once, from the
//!      native run. A limit of the interpreter that is not the program's fault (memory, output,
//!      call depth) or a gap of the interpreter counts as running out of budget.
//!
//! Output, exit code and runtime errors are the same in every mode (`tests/auto.rs` compares them
//! on all the examples). Without a C compiler the interpreter runs the program to its end.
//!
//! Knobs for tests and measurements: `NYRA_AUTO_STEPS` and `NYRA_AUTO_WALL_MS` (the budgets) and
//! `NYRA_AUTO_DELAY_MS`.

use std::io::{IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::ast::Program;
use crate::ir::{Module, RtOp};
use crate::{limits, sandbox, Opt, Opts, Target};

/// The steps the interpreter may run before the program goes native. See research/SPEED-run.md
/// for how it was chosen.
pub const BUDGET: u64 = 4_000_000;

/// The real time the interpreter may take, in milliseconds: steps are not all alike (an operation
/// on a big string costs many steps and much time), and waiting longer than the C compiler needs
/// would make the fallback slower than a plain native run.
pub const WALL_MS: u64 = 350;

/// How long the interpreter runs alone before the C compiler starts in the background: a program
/// that is done by then never starts one.
pub const DELAY: Duration = Duration::from_millis(15);

/// The most the interpreter keeps of the program's output: a program that prints more runs natively,
/// where its output streams.
const OUTPUT_CAP: u64 = 8 << 20;

/// The heap the interpreter may use before the program runs natively instead.
const MEMORY_CAP: u64 = 256 << 20;

/// How long to wait for the end of standard input before giving up on the interpreter (a program
/// that talks to a pipe, answering as input arrives, must not wait for its end).
const STDIN_WAIT: Duration = Duration::from_millis(40);

fn env_number(name: &str) -> Option<u64> {
    std::env::var(name).ok()?.trim().parse().ok()
}

/// The budget of steps: `NYRA_AUTO_STEPS`, else `BUDGET`.
pub fn budget() -> u64 {
    env_number("NYRA_AUTO_STEPS").unwrap_or(BUDGET)
}

/// The real time budget in milliseconds: `NYRA_AUTO_WALL_MS`, else `WALL_MS`.
pub fn wall_ms() -> u64 {
    env_number("NYRA_AUTO_WALL_MS").unwrap_or(WALL_MS)
}

fn delay() -> Duration {
    env_number("NYRA_AUTO_DELAY_MS").map_or(DELAY, Duration::from_millis)
}

/// What a program does beyond printing, found in its IR (so a function that is never called counts too).
pub struct Effects {
    /// It uses `fs` or `os` (files, the environment, `exit`), or `time.sleep_ms` (the interpreter does
    /// not wait): such a program runs only once, natively.
    pub world: bool,
    /// It reads standard input.
    pub input: bool,
}

pub fn effects(m: &Module) -> Effects {
    let uses = |module: &str| m.uses(&|op| matches!(op, RtOp::Std(f) if f.path().0 == module));
    let sleeps = m.uses(&|op| matches!(op, RtOp::Std(crate::ir::StdFn::TimeSleepMs)));
    Effects { world: uses("fs") || uses("os") || sleeps, input: uses("input") }
}

// ---- running in the interpreter ----------------------------------------------------------------------

/// Runs `m` in the interpreter with the budget and the caps of auto mode, keeping its output.
/// `None`: it did not finish in them (or the interpreter could not run it), so it must run natively.
pub fn attempt(m: &Module, stdin: Vec<u8>, args: Vec<String>, memory: u64, output: u64) -> Option<sandbox::Report> {
    let mut cfg = sandbox::Config::new();
    cfg.limits.steps = budget();
    cfg.limits.wall_ms = wall_ms();
    cfg.limits.memory = memory;
    cfg.limits.output = output;
    cfg.args = args;
    cfg.stdin = Some(stdin);
    cfg.to_stdout = false;
    // a bug of the interpreter (a panic) must not stop a program that the compiler can run
    let quiet = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| sandbox::run(m, cfg)));
    std::panic::set_hook(quiet);
    let Ok(report) = ran else { return None };
    let limit = report.error.as_ref().is_some_and(|e| sandbox::LIMIT_EXITS.iter().any(|(code, _)| *code == e.code));
    (report.internal.is_none() && !limit).then_some(report)
}

/// The text as the compiled program's runtime writes it: Windows turns each `\n` in text mode
/// into `\r\n`, and so do the executables, so the interpreter's output must look the same.
pub fn native_text(s: String) -> String {
    if cfg!(windows) {
        s.replace('\n', "\r\n")
    } else {
        s
    }
}

// ---- standard input ----------------------------------------------------------------------------------

/// What a native program gets as its standard input when this process has read some or all of it.
pub enum Feed {
    /// All of it.
    Bytes(Vec<u8>),
    /// What was read, then whatever arrives later (the reader's chunks, until it sees the end).
    Stream { head: Vec<u8>, rest: Receiver<Vec<u8>> },
}

/// Standard input, read by a thread in chunks.
pub struct Input {
    rx: Receiver<Vec<u8>>,
}

impl Input {
    pub fn start() -> Input {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let mut stdin = std::io::stdin().lock();
            let mut chunk = vec![0u8; 64 << 10];
            loop {
                match stdin.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if tx.send(chunk[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                }
            }
        });
        Input { rx }
    }

    /// All of standard input if its end comes within `wait`; else what has come so far and the rest.
    pub fn gather(self, wait: Duration) -> Result<Vec<u8>, Feed> {
        let deadline = Instant::now() + wait;
        let mut all = Vec::new();
        loop {
            match self.rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok(chunk) => all.extend_from_slice(&chunk),
                Err(RecvTimeoutError::Disconnected) => return Ok(all),
                Err(RecvTimeoutError::Timeout) => return Err(Feed::Stream { head: all, rest: self.rx }),
            }
        }
    }
}

/// Runs `cmd` with `feed` as its standard input and waits for it.
pub fn run_fed(mut cmd: Command, feed: Feed) -> std::io::Result<ExitStatus> {
    cmd.stdin(Stdio::piped());
    let mut child = cmd.spawn()?;
    if let Some(mut pipe) = child.stdin.take() {
        // a thread, so a program that never reads cannot block us; dropping the pipe sends EOF
        thread::spawn(move || match feed {
            Feed::Bytes(bytes) => {
                let _ = pipe.write_all(&bytes);
            }
            Feed::Stream { head, rest } => {
                if pipe.write_all(&head).is_ok() {
                    for chunk in rest {
                        if pipe.write_all(&chunk).is_err() {
                            break;
                        }
                    }
                }
            }
        });
    }
    child.wait()
}

// ---- the C compiler in the background ----------------------------------------------------------------

/// Lets one thread stop a C compiler that another one runs.
pub struct Killer {
    cancelled: AtomicBool,
    /// The compiler runs but cannot be killed (the system refused to group it with its children).
    unkillable: AtomicBool,
    tree: Mutex<Option<limits::Tree>>,
    /// The temporary folder of the compiler: a compiler that is killed leaves its files there.
    tmp: PathBuf,
}

impl Killer {
    fn new(dir: &Path) -> Killer {
        static COUNT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = COUNT.fetch_add(1, Ordering::Relaxed);
        Killer {
            cancelled: AtomicBool::new(false),
            unkillable: AtomicBool::new(false),
            tree: Mutex::new(None),
            tmp: dir.join(format!("cctmp-{}-{n}", std::process::id())),
        }
    }

    pub fn tmp(&self) -> &Path {
        &self.tmp
    }

    pub fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// The compiler has started: from now on `cancel` stops it (at once if it was called already).
    pub fn attach(&self, child: &Child) {
        let tree = limits::Tree::of(child);
        if tree.is_none() {
            self.unkillable.store(true, Ordering::SeqCst);
        }
        *self.tree.lock().unwrap_or_else(|e| e.into_inner()) = tree;
        if self.cancelled() {
            self.kill();
        }
    }

    fn kill(&self) {
        if let Some(tree) = self.tree.lock().unwrap_or_else(|e| e.into_inner()).take() {
            tree.kill();
        }
    }

    /// Stops the compiler; false if it is running and could not be stopped.
    fn cancel(&self) -> bool {
        self.cancelled.store(true, Ordering::SeqCst);
        self.kill();
        !self.unkillable.load(Ordering::SeqCst)
    }
}

/// What a background build compiles.
pub struct Job {
    pub compiler: String,
    pub code: String,
    pub stem: String,
    pub source: String,
    pub dir: PathBuf,
    pub opt: Opt,
    /// The compiler's messages go into the error instead of the terminal.
    pub capture: bool,
}

type Built = Result<(PathBuf, Option<Duration>), String>;

const CANCELLED: &str = "the build was cancelled";

/// A C compile that runs in a thread, starting after a delay, and can be cancelled or waited for.
pub struct Build {
    go: Sender<()>,
    killer: Arc<Killer>,
    thread: JoinHandle<Built>,
}

impl Build {
    pub fn spawn(job: Job, delay: Duration) -> Build {
        let (go, wake) = mpsc::channel::<()>();
        let killer = Arc::new(Killer::new(&job.dir));
        let k = killer.clone();
        let thread = thread::spawn(move || {
            // (`go` is dropped, not sent, when the build is cancelled before it started)
            if let Err(RecvTimeoutError::Disconnected) = wake.recv_timeout(delay) {
                return Err(CANCELLED.to_string());
            }
            if k.cancelled() {
                return Err(CANCELLED.to_string());
            }
            sweep(&job.dir);
            let built =
                crate::cc_cached_kill(&job.compiler, &job.code, &job.stem, &job.source, &job.dir, job.opt, job.capture, Some(&k));
            let _ = std::fs::remove_dir_all(k.tmp());
            built
        });
        Build { go, killer, thread }
    }

    /// Starts the compile now if it has not started, and waits for it.
    pub fn finish(self) -> Built {
        let _ = self.go.send(());
        self.thread.join().unwrap_or_else(|_| Err("the C compiler thread stopped unexpectedly".to_string()))
    }

    /// Stops the compile; waits for the compiler to be gone so that it leaves nothing behind.
    pub fn cancel(self) {
        let stopped = self.killer.cancel();
        drop(self.go);
        if stopped {
            let _ = self.thread.join();
        }
    }
}

/// Removes what compiles that were stopped long ago left behind: temporary executables and the
/// compilers' temporary folders.
fn sweep(dir: &Path) {
    let old = |meta: &std::fs::Metadata| {
        meta.modified().ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age > Duration::from_secs(600))
    };
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(meta) = entry.metadata() else { continue };
        if !old(&meta) {
            continue;
        }
        if meta.is_dir() && name.starts_with("cctmp-") {
            let _ = std::fs::remove_dir_all(entry.path());
        } else if meta.is_file() && name.contains(".partial") {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

// ---- nyra run -----------------------------------------------------------------------------------------

/// `nyra run` of a checked program, in auto mode (see the module comment).
pub fn run(opts: &Opts, prog: &Program, start: Instant) -> ExitCode {
    let module = match crate::lower(prog) {
        Ok(m) => m,
        Err(msg) => return crate::fail(msg),
    };
    let code = crate::emit(&module, Target::Native, &opts.file);
    let nyra_time = start.elapsed();
    let stem = Path::new(&opts.file).file_stem().and_then(|s| s.to_str()).unwrap_or("main").to_string();
    let dir = crate::temp_dir();
    let compiler = crate::find_cc();

    // 1. a build of this source is cached: run it
    if let Some(cc) = &compiler {
        for opt in [Opt::Fast, Opt::Release] {
            if let Some(exe) = crate::cc_lookup(cc, &code, &stem, &opts.file, &dir, opt) {
                return crate::exec(opts, Command::new(exe), Some(None), nyra_time, None);
            }
        }
    }
    let Some(compiler) = compiler else {
        return interpret_to_the_end(opts, &module, nyra_time);
    };

    // 2. a program that must not run twice, or that talks to a terminal
    let fx = effects(&module);
    if fx.world || (fx.input && std::io::stdin().is_terminal()) {
        return natively(opts, &code, &stem, nyra_time, None);
    }

    // 3. standard input, all of it (a program that does not read it leaves it alone)
    let mut stdin = Vec::new();
    if fx.input {
        match Input::start().gather(STDIN_WAIT) {
            Ok(bytes) => stdin = bytes,
            // the end does not come soon: a program that answers a pipe as it goes must not wait for it
            Err(feed) => return natively(opts, &code, &stem, nyra_time, Some(feed)),
        }
    }

    // 4. the interpreter, with the compiler working in the background
    let build = Build::spawn(Job { compiler, code, stem, source: opts.file.clone(), dir, opt: Opt::Fast, capture: false }, delay());
    let t = Instant::now();
    let Some(report) = attempt(&module, stdin.clone(), opts.prog_args.clone(), MEMORY_CAP, OUTPUT_CAP) else {
        // over the budget: what the interpreter printed is dropped, the native run prints it all
        let wasted = t.elapsed();
        let (exe, built) = match build.finish() {
            Ok(r) => r,
            Err(msg) => return crate::fail(msg),
        };
        if opts.time {
            eprintln!("nyra {} | interpreter gave up after {} | cc {}", crate::ms(nyra_time), crate::ms(wasted), cc_part(built));
        }
        let feed = fx.input.then_some(Feed::Bytes(stdin));
        return crate::exec(opts, Command::new(exe), Some(built), nyra_time, feed);
    };
    build.cancel();

    // within the budget: print what the program printed, as the native one would have
    {
        let mut out = std::io::stdout().lock();
        let _ = out.write_all(native_text(report.stdout).as_bytes());
        let _ = out.flush();
    }
    if let Some(e) = &report.error {
        let text = format!("{}\n", sandbox::render_error(e, &opts.file, opts.json));
        eprint!("{}", native_text(text));
    }
    if opts.time {
        eprintln!("nyra {} | interpreted {} ({} steps)", crate::ms(nyra_time), crate::ms(t.elapsed()), report.steps);
    }
    ExitCode::from(report.exit as u8)
}

fn cc_part(built: Option<Duration>) -> String {
    match built {
        Some(t) => crate::ms(t),
        None => "cached".to_string(),
    }
}

/// The old way: compile, then start the executable.
fn natively(opts: &Opts, code: &str, stem: &str, nyra_time: Duration, feed: Option<Feed>) -> ExitCode {
    match crate::native(code, stem, &opts.file, Opt::Fast) {
        Ok((exe, built)) => crate::exec(opts, Command::new(exe), Some(built), nyra_time, feed),
        Err(code) => code,
    }
}

/// No C compiler: the interpreter runs the program to its end, with no limit but the depth.
fn interpret_to_the_end(opts: &Opts, module: &Module, nyra_time: Duration) -> ExitCode {
    eprintln!("nyra: no C compiler found (tried gcc, clang, cc, tcc; NYRA_CC): running in the interpreter, which is slower");
    let mut cfg = sandbox::Config::new();
    cfg.limits.steps = u64::MAX;
    cfg.limits.memory = u64::MAX;
    cfg.limits.output = u64::MAX;
    cfg.limits.depth = sandbox::MAX_DEPTH;
    cfg.args = opts.prog_args.clone();
    let report = sandbox::run(module, cfg);
    if let Some(e) = &report.error {
        eprintln!("{}", sandbox::render_error(e, &opts.file, opts.json));
    }
    if let Some(what) = &report.internal {
        eprintln!("nyra: internal error in the interpreter: {what}");
    }
    if opts.time {
        eprintln!("nyra {} | interpreted {:.2} ms ({} steps)", crate::ms(nyra_time), report.run_ms, report.steps);
    }
    ExitCode::from(report.exit as u8)
}
