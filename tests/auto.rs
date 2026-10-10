//! `nyra run` in auto mode (src/auto.rs): the interpreter first, the native executable for a program
//! that runs long, and the same output either way.
//!
//! - every example and every runtime-error test prints the same bytes, the same stderr and the same
//!   exit code in auto mode and with `--native`;
//! - a program that goes over the step budget runs natively and its output appears once;
//! - a program that uses `fs` or `os` always runs natively, once (never interpreted first);
//! - standard input works in both paths, also when it is long, and when it stays open;
//! - a cached build is used; `--native` and `--release` never interpret.
//!
//! `NYRA_AUTO_STEPS` sets the budget, so a tiny program can be made to run out of it.

mod common;

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use common::{nyra, stderr};

fn has_cc() -> bool {
    std::env::var("NYRA_CC").is_ok() || ["gcc", "clang", "cc", "tcc"].iter().any(|c| Command::new(c).arg("--version").output().is_ok())
}

/// A new empty folder for one run: `<name>/cache` is its temp folder (the build cache), `<name>/work` its working folder.
fn fresh(name: &str) -> (PathBuf, PathBuf) {
    static RUNS: AtomicUsize = AtomicUsize::new(0);
    let base = std::env::temp_dir().join("nyra-auto-tests").join(format!(
        "{name}-{}-{}",
        std::process::id(),
        RUNS.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&base);
    let (cache, work) = (base.join("cache"), base.join("work"));
    std::fs::create_dir_all(&cache).unwrap();
    std::fs::create_dir_all(&work).unwrap();
    (cache, work)
}

/// `nyra run <flags> <file> [-- args]` with `stdin`, in `work`, with `cache` as the temp folder.
fn run(cache: &Path, work: &Path, file: &Path, flags: &[&str], args: &[String], stdin: &[u8], env: &[(&str, &str)]) -> Output {
    let mut cmd = nyra();
    // (a generous time budget, so that a busy machine does not turn a run in the interpreter into a native one)
    cmd.current_dir(work).env("TEMP", cache).env("TMP", cache).env("TMPDIR", cache).env("NYRA_AUTO_WALL_MS", "600000");
    cmd.envs(env.iter().copied());
    cmd.arg("run").args(flags).arg(file);
    if !args.is_empty() {
        cmd.arg("--").args(args);
    }
    let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let mut pipe = child.stdin.take().unwrap();
    // (a thread: the program may not read it all, or the input may be longer than a pipe holds)
    let input = stdin.to_vec();
    let writer = std::thread::spawn(move || {
        let _ = pipe.write_all(&input);
    });
    let out = child.wait_with_output().unwrap();
    let _ = writer.join();
    out
}

/// A program in its own folder.
fn program(name: &str, src: &str) -> (PathBuf, PathBuf, PathBuf) {
    let (cache, work) = fresh(name);
    let file = work.join("prog.nyra");
    std::fs::write(&file, src).unwrap();
    (cache, work, file)
}

/// The text of bytes, with the Windows line ends of the compiled programs made plain.
fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).replace("\r\n", "\n")
}

/// What `nyra` and the program wrote to stderr, byte for byte, without the messages of the C compiler (a
/// native run that compiles prints its warnings there; a run in the interpreter has none).
fn own_stderr(out: &Output) -> Vec<u8> {
    let mut kept = Vec::new();
    for line in out.stderr.split_inclusive(|&b| b == b'\n') {
        let t = String::from_utf8_lossy(line);
        if ["runtime error", "warning[", "nyra:", "{", "  --> ", "  = "].iter().any(|p| t.starts_with(p)) {
            kept.extend_from_slice(line);
        }
    }
    kept
}

fn files(dir: &str) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> =
        std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).filter(|p| p.extension().is_some_and(|e| e == "nyra")).collect();
    v.sort();
    v
}

/// Runs `check` over `items` on a few threads and collects the failures.
fn in_parallel(items: Vec<PathBuf>, check: impl Fn(&PathBuf) -> Option<String> + Sync) {
    let next = AtomicUsize::new(0);
    let failures = std::sync::Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..4 {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::SeqCst);
                let Some(item) = items.get(i) else { break };
                if let Some(f) = check(item) {
                    failures.lock().unwrap().push(f);
                }
            });
        }
    });
    let failures = failures.into_inner().unwrap();
    assert!(failures.is_empty(), "{} failure(s):\n\n{}", failures.len(), failures.join("\n\n"));
}

/// A program run in auto mode and with `--native` (each in a folder and a cache of its own, so the
/// second does not find the build of the first): `None` when everything agrees, byte for byte.
fn same_both_ways(path: &PathBuf, flags: &[&str]) -> Option<String> {
    let args: Vec<String> =
        std::fs::read_to_string(path.with_extension("args")).map(|a| a.lines().map(String::from).collect()).unwrap_or_default();
    let stdin = std::fs::read(path.with_extension("in")).unwrap_or_default();
    let file = std::fs::canonicalize(path).unwrap();
    let go = |extra: &[&str]| {
        let (cache, work) = fresh(&path.file_stem().unwrap().to_string_lossy());
        let all: Vec<&str> = flags.iter().chain(extra).copied().collect();
        let out = run(&cache, &work, &file, &all, &args, &stdin, &[]);
        let _ = std::fs::remove_dir_all(work.parent().unwrap());
        out
    };
    let (auto, native) = (go(&[]), go(&["--native"]));
    let label = format!("{} {flags:?}", path.display());
    if auto.status.code() != native.status.code() {
        return Some(format!(
            "{label}: exit code {:?} in auto mode, {:?} natively\n{}",
            auto.status.code(),
            native.status.code(),
            stderr(&auto)
        ));
    }
    if auto.stdout != native.stdout {
        return Some(format!("{label}: stdout differs\n--- auto\n{}--- native\n{}", text(&auto.stdout), text(&native.stdout)));
    }
    if own_stderr(&auto) != own_stderr(&native) {
        return Some(format!("{label}: stderr differs\n--- auto\n{}--- native\n{}", stderr(&auto), stderr(&native)));
    }
    None
}

// ---- the same output in every mode ---------------------------------------------------------------------------

#[test]
fn auto_mode_prints_what_native_prints_on_every_example() {
    if !has_cc() {
        common::missing("no C compiler for the native backend");
        return;
    }
    let all: Vec<PathBuf> = files("examples").into_iter().filter(|p| p.with_extension("out").exists()).collect();
    assert!(all.len() >= 40);
    in_parallel(all, |p| same_both_ways(p, &[]));
}

#[test]
fn auto_mode_reports_runtime_errors_like_native() {
    if !has_cc() {
        common::missing("no C compiler for the native backend");
        return;
    }
    let mut tests = Vec::new();
    for path in files("tests/runtime") {
        let src = std::fs::read_to_string(&path).unwrap();
        // `// only: js ts` tests are about JavaScript; `// only: interp` ones about the limits
        if src.lines().nth(1).is_some_and(|l| l.starts_with("// only:")) {
            continue;
        }
        tests.push(path);
    }
    assert!(tests.len() >= 30);
    // (the message and the position, in human and in JSON form)
    in_parallel(tests.clone(), |p| same_both_ways(p, &[]));
    in_parallel(tests, |p| same_both_ways(p, &["--json"]));
}

#[test]
fn auto_mode_runs_every_example_in_the_interpreter_or_natively_without_the_other_showing() {
    // with a budget of 50 steps most examples run out of it and go native: still the same output
    if !has_cc() {
        common::missing("no C compiler for the native backend");
        return;
    }
    let all: Vec<PathBuf> = files("examples").into_iter().filter(|p| p.with_extension("out").exists()).collect();
    in_parallel(all, |path| {
        let args: Vec<String> =
            std::fs::read_to_string(path.with_extension("args")).map(|a| a.lines().map(String::from).collect()).unwrap_or_default();
        let stdin = std::fs::read(path.with_extension("in")).unwrap_or_default();
        let file = std::fs::canonicalize(path).unwrap();
        let go = |extra: &[&str], env: &[(&str, &str)]| {
            let (cache, work) = fresh(&path.file_stem().unwrap().to_string_lossy());
            let out = run(&cache, &work, &file, extra, &args, &stdin, env);
            let _ = std::fs::remove_dir_all(work.parent().unwrap());
            out
        };
        let (cut, native) = (go(&[], &[("NYRA_AUTO_STEPS", "50"), ("NYRA_AUTO_DELAY_MS", "0")]), go(&["--native"], &[]));
        (cut.status.code() != native.status.code() || cut.stdout != native.stdout || own_stderr(&cut) != own_stderr(&native)).then(
            || {
                format!(
                    "{}: output differs after a fallback\n--- auto, 50 steps\n{}{}--- native\n{}{}",
                    path.display(),
                    text(&cut.stdout),
                    stderr(&cut),
                    text(&native.stdout),
                    stderr(&native)
                )
            },
        )
    });
}

// ---- the fallback -------------------------------------------------------------------------------------------

const HEAVY: &str = "fn main() {\n    print(\"start\")\n    var t = 0\n    for i in 0..300000 {\n        t += i % 7\n    }\n    print(\"sum {t}\")\n}\n";

#[test]
fn a_program_over_the_budget_runs_natively_and_prints_once() {
    if !has_cc() {
        common::missing("no C compiler for the native backend");
        return;
    }
    let (cache, work, file) = program("fallback", HEAVY);
    let native = run(&cache, &work, &file, &["--native"], &[], b"", &[]);
    assert_eq!(text(&native.stdout), "start\nsum 899997\n", "{}", stderr(&native));

    // a fresh cache, so that the auto run cannot use the build above
    let (cache, work, file) = program("fallback2", HEAVY);
    let out = run(&cache, &work, &file, &["--time"], &[], b"", &[("NYRA_AUTO_STEPS", "1000")]);
    assert_eq!(out.stdout, native.stdout, "{}", stderr(&out));
    assert_eq!(out.status.code(), Some(0));
    let err = stderr(&out);
    assert!(err.contains("interpreter gave up"), "the interpreter should have run out of steps: {err}");
    assert!(!err.contains("interpreted "), "{err}");

    // the time budget counts too: 1 ms is not enough for 900,000 steps
    let (cache, work, file) = program("fallback-wall", HEAVY);
    let out = run(&cache, &work, &file, &["--time"], &[], b"", &[("NYRA_AUTO_WALL_MS", "1")]);
    assert_eq!(out.stdout, native.stdout, "{}", stderr(&out));
    assert!(stderr(&out).contains("interpreter gave up"), "{}", stderr(&out));

    // within the budget, the same program is answered by the interpreter
    let (cache, work, file) = program("fallback3", HEAVY);
    let out = run(&cache, &work, &file, &["--time"], &[], b"", &[]);
    assert_eq!(out.stdout, native.stdout);
    assert!(stderr(&out).contains("interpreted "), "{}", stderr(&out));
    assert!(!stderr(&out).contains("gave up"), "{}", stderr(&out));
}

#[test]
fn a_fallback_prints_a_runtime_error_once() {
    if !has_cc() {
        common::missing("no C compiler for the native backend");
        return;
    }
    let src = "fn main() {\n    print(\"before\")\n    var t = 0\n    for i in 0..100000 {\n        t += i\n    }\n    let xs = [1, 2]\n    print(xs[t % 5 + 5])\n}\n";
    let (cache, work, file) = program("fallback-error", src);
    let native = run(&cache, &work, &file, &["--native"], &[], b"", &[]);
    assert_eq!(native.status.code(), Some(101), "{}", stderr(&native));
    for steps in ["100", "100000000"] {
        // (the same file, so that the position in the message is the same; a cache without the build)
        let (empty_cache, _) = fresh("fallback-error-cache");
        let out = run(&empty_cache, &work, &file, &[], &[], b"", &[("NYRA_AUTO_STEPS", steps)]);
        assert_eq!(out.status.code(), Some(101), "{steps}: {}", stderr(&out));
        assert_eq!(out.stdout, native.stdout, "{steps}");
        assert_eq!(own_stderr(&out), own_stderr(&native), "{steps}");
        assert_eq!(stderr(&out).matches("runtime error[").count(), 1, "{steps}: {}", stderr(&out));
    }
}

#[test]
fn the_interpreters_limits_are_a_reason_to_run_natively() {
    // 30,000 calls deep is past the interpreter's limit of 20,000 nested calls; natively it is a
    // recursion that the program's own stack can take
    if !has_cc() {
        common::missing("no C compiler for the native backend");
        return;
    }
    let src = "fn depth(n: int) -> int {\n    if n == 0 {\n        return 0\n    }\n    return 1 + depth(n - 1)\n}\nfn main() {\n    print(depth(30000))\n}\n";
    let (cache, work, file) = program("deep", src);
    let native = run(&cache, &work, &file, &["--native"], &[], b"", &[]);
    let (cache, work, file) = program("deep2", src);
    let out = run(&cache, &work, &file, &["--time"], &[], b"", &[]);
    assert_eq!(out.stdout, native.stdout, "{}", stderr(&out));
    assert_eq!(out.status.code(), native.status.code());
    assert!(!stderr(&out).contains("E0358"), "the depth limit of the interpreter is not the program's: {}", stderr(&out));
}

// ---- side effects: run once, natively ------------------------------------------------------------------------

#[test]
fn programs_that_use_fs_or_os_run_natively_and_once() {
    if !has_cc() {
        common::missing("no C compiler for the native backend");
        return;
    }
    // appends to a file: run twice (interpreter, then native) it would leave `xx`
    let files = "use fs\nfn main() {\n    fs.append(\"log.txt\", \"x\")\n    var t = 0\n    for i in 0..100000 {\n        t += i\n    }\n    print(t)\n}\n";
    let exits = "use os\nfn main() {\n    print(\"bye\")\n    os.exit(3)\n}\n";
    let args = "use os\nfn main() {\n    print(os.args())\n}\n";
    // sleeping must really take the time (the interpreter only moves a clock)
    let sleeps = "use time\nfn main() {\n    time.sleep_ms(50)\n    print(\"slept\")\n}\n";
    let (cache, work, file) = program("sleep", sleeps);
    let out = run(&cache, &work, &file, &["--time"], &[], b"", &[]);
    assert_eq!(text(&out.stdout), "slept\n", "{}", stderr(&out));
    assert!(!stderr(&out).contains("interpreted") && stderr(&out).contains("| cc "), "{}", stderr(&out));
    for steps in ["10", "1000000000"] {
        let (cache, work, file) = program("fs", files);
        let out = run(&cache, &work, &file, &["--time"], &[], b"", &[("NYRA_AUTO_STEPS", steps)]);
        assert_eq!(text(&out.stdout), "4999950000\n", "{}", stderr(&out));
        assert_eq!(std::fs::read_to_string(work.join("log.txt")).unwrap(), "x", "the program ran more than once (budget {steps})");
        let err = stderr(&out);
        assert!(!err.contains("interpreted") && !err.contains("gave up"), "the interpreter should not run it: {err}");
        assert!(err.contains("| cc "), "{err}");

        let (cache, work, file) = program("os", exits);
        let out = run(&cache, &work, &file, &["--time"], &[], b"", &[("NYRA_AUTO_STEPS", steps)]);
        assert_eq!(out.status.code(), Some(3), "{}", stderr(&out));
        assert_eq!(text(&out.stdout), "bye\n");
        assert!(!stderr(&out).contains("interpreted"), "{}", stderr(&out));

        let (cache, work, file) = program("os-args", args);
        let out = run(&cache, &work, &file, &["--time"], &["a".into(), "b c".into()], b"", &[("NYRA_AUTO_STEPS", steps)]);
        assert_eq!(text(&out.stdout), "[\"a\", \"b c\"]\n", "{}", stderr(&out));
    }
}

// ---- standard input ------------------------------------------------------------------------------------------

const SHOUT: &str = "use input\nfn main() {\n    let lines = input.lines()\n    var total = 0\n    for l in lines {\n        total += l.len()\n        print(l.upper())\n    }\n    print(\"{lines.len()} lines, {total} characters\")\n}\n";

fn shout_input(lines: usize) -> (Vec<u8>, String) {
    let mut input = String::new();
    let mut want = String::new();
    let mut total = 0;
    for i in 0..lines {
        let l = format!("line {i} of the input");
        total += l.chars().count();
        want += &format!("{}\n", l.to_uppercase());
        input += &format!("{l}\n");
    }
    want += &format!("{lines} lines, {total} characters\n");
    (input.into_bytes(), want)
}

#[test]
fn standard_input_works_in_the_interpreter_and_natively() {
    if !has_cc() {
        common::missing("no C compiler for the native backend");
        return;
    }
    // (20,000 lines are more than 64 KiB: several chunks of the reader, and more than a pipe holds)
    for lines in [3, 20_000] {
        let (input, want) = shout_input(lines);
        // the interpreter answers; a budget of 5 steps makes the native run answer; --native too
        for (flags, steps, mode) in [
            (&["--time"][..], "100000000", "interpreted"),
            (&["--time"][..], "5", "gave up"),
            (&["--time", "--native"][..], "5", "| cc "),
        ] {
            let (cache, work, file) = program("stdin", SHOUT);
            let out = run(&cache, &work, &file, flags, &[], &input, &[("NYRA_AUTO_STEPS", steps)]);
            assert_eq!(text(&out.stdout), want, "{lines} lines, {mode}: {}", stderr(&out));
            if mode == "gave up" {
                // native answered: either the interpreter gave up, or (on a slow machine, when the input
                // arrives after the pipe check) the run went native from the start; both are correct
                assert!(!stderr(&out).contains("interpreted"), "{lines} lines, expected a native run: {}", stderr(&out));
            } else {
                assert!(stderr(&out).contains(mode), "{lines} lines, expected `{mode}`: {}", stderr(&out));
            }
        }
    }
}

#[test]
fn a_program_that_answers_a_pipe_as_it_goes_does_not_wait_for_its_end() {
    if !has_cc() {
        common::missing("no C compiler for the native backend");
        return;
    }
    let src =
        "use input\nfn main() {\n    while !input.eof() {\n        print(\"got {input.line()}\")\n    }\n    print(\"done\")\n}\n";
    let (cache, work, file) = program("conversation", src);
    let mut child = nyra()
        .current_dir(&work)
        .env("TEMP", &cache)
        .env("TMP", &cache)
        .env("TMPDIR", &cache)
        .arg("run")
        .arg(&file)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let stdout = child.stdout.take().unwrap();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let _ = tx.send(line.unwrap());
        }
    });
    let started = Instant::now();
    // the input stays open: each answer must come before the next line is sent
    for word in ["one", "two", "three"] {
        writeln!(stdin, "{word}").unwrap();
        stdin.flush().unwrap();
        let line =
            rx.recv_timeout(Duration::from_secs(30)).unwrap_or_else(|_| panic!("no answer to `{word}` after {:?}", started.elapsed()));
        assert_eq!(line.trim_end(), format!("got {word}"));
    }
    drop(stdin);
    assert_eq!(rx.recv_timeout(Duration::from_secs(30)).unwrap().trim_end(), "done");
    assert!(child.wait().unwrap().success());
}

// ---- the build cache and the flags ----------------------------------------------------------------------------

#[test]
fn a_cached_build_is_run_and_the_flags_never_interpret() {
    if !has_cc() {
        common::missing("no C compiler for the native backend");
        return;
    }
    let (cache, work, file) = program("cached", HEAVY);
    let first = run(&cache, &work, &file, &["--time"], &[], b"", &[]);
    assert!(stderr(&first).contains("interpreted "), "{}", stderr(&first));
    // nothing was compiled (or kept): the next run is the interpreter again
    let again = run(&cache, &work, &file, &["--time"], &[], b"", &[]);
    assert!(stderr(&again).contains("interpreted "), "{}", stderr(&again));

    // --native compiles; after it, `run` finds the build
    let native = run(&cache, &work, &file, &["--time", "--native"], &[], b"", &[]);
    assert!(stderr(&native).contains("| cc ") && !stderr(&native).contains("interpreted"), "{}", stderr(&native));
    assert_eq!(native.stdout, first.stdout);
    let cached = run(&cache, &work, &file, &["--time"], &[], b"", &[]);
    assert!(stderr(&cached).contains("| cc cached |") && !stderr(&cached).contains("interpreted"), "{}", stderr(&cached));
    assert_eq!(cached.stdout, first.stdout);

    // another source is not that build
    std::fs::write(&file, HEAVY.replace("300000", "300001")).unwrap();
    let other = run(&cache, &work, &file, &["--time"], &[], b"", &[]);
    assert!(stderr(&other).contains("interpreted "), "{}", stderr(&other));

    // --release is native as well (and -O2 is another build)
    let release = run(&cache, &work, &file, &["--time", "--release"], &[], b"", &[]);
    assert!(stderr(&release).contains("| cc ") && !stderr(&release).contains("interpreted"), "{}", stderr(&release));
    assert!(!stderr(&release).contains("cached"), "{}", stderr(&release));
    let after = run(&cache, &work, &file, &["--time"], &[], b"", &[]);
    assert!(stderr(&after).contains("| cc cached |"), "an -O2 build is as good: {}", stderr(&after));
}

#[test]
fn a_program_that_ends_early_leaves_no_compiler_files_behind() {
    if !has_cc() {
        common::missing("no C compiler for the native backend");
        return;
    }
    let (cache, work, file) = program("cleanup", HEAVY);
    // the compiler starts at once, and is stopped when the interpreter is done
    let out = run(&cache, &work, &file, &["--time"], &[], b"", &[("NYRA_AUTO_DELAY_MS", "0")]);
    assert_eq!(text(&out.stdout), "start\nsum 899997\n", "{}", stderr(&out));
    assert!(stderr(&out).contains("interpreted "), "{}", stderr(&out));
    let left: Vec<String> = std::fs::read_dir(cache.join("nyra"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains(".partial") || n.starts_with("cctmp-") || n.ends_with(".exe"))
        .collect();
    assert!(left.is_empty(), "left behind: {left:?}");
}
