//! Runs every `examples/*.nyra` on every available backend and compares stdout
//! with `examples/*.out`. Also checks that `tests/errors/*.nyra` report the
//! error code named in their first line (`// expect: E0203`). A runtime error test
//! whose second line is `// only: js ts` runs only on those targets (`native`, `js`,
//! `ts`, `py`, `rs`, `go`): E0256 exists only where ints are JavaScript numbers.
//!
//! An example may come with more files next to it:
//! - `X.in`: its standard input (without one, the input is empty);
//! - `X.args`: its arguments, one per line (passed after `--`);
//! - `X.exit`: the exit code it must end with (else 0).
//!
//! Each run starts in an empty folder of its own, so an example may create files.

mod common;

use common::missing;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

fn nyra() -> Command {
    Command::new(env!("CARGO_BIN_EXE_nyra"))
}

fn available(tool: &str) -> bool {
    Command::new(tool).arg("--version").output().is_ok()
}

/// Runs one example (or runtime error test) the way its side files say: `nyra run <example>
/// <flags> -- <args>` in a new empty folder, with its input. Returns the output and the expected
/// exit code.
fn run_example(path: &PathBuf, flags: &[&str], env: &[(&str, &str)]) -> (Output, i32) {
    static RUNS: AtomicUsize = AtomicUsize::new(0);
    let side = |ext: &str| std::fs::read_to_string(path.with_extension(ext)).ok();
    let full = std::fs::canonicalize(path).unwrap();
    let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
    let dir = std::env::temp_dir().join("nyra-examples").join(format!(
        "{stem}-{}-{}",
        std::process::id(),
        RUNS.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut cmd = nyra();
    cmd.current_dir(&dir).arg("run").arg(&full).args(flags).envs(env.iter().copied());
    if let Some(args) = side("args") {
        cmd.arg("--").args(args.lines());
    }
    // (bytes: a test may feed text that is not UTF-8)
    let input = std::fs::read(path.with_extension("in")).ok();
    cmd.stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() }).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().unwrap();
    if let Some(text) = input {
        let mut stdin = child.stdin.take().unwrap();
        // a program may stop reading early: a closed pipe is not an error here
        let _ = stdin.write_all(&text);
    }
    let out = child.wait_with_output().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    let exit = side("exit").map_or(0, |e| e.trim().parse().expect("an .exit file holds a number"));
    (out, exit)
}

/// The targets a test is limited to by a second line `// only: js ts` (`None`: every target).
fn only(src: &str) -> Option<Vec<String>> {
    let line = src.lines().nth(1)?.strip_prefix("// only:")?;
    Some(line.split_whitespace().map(String::from).collect())
}

/// The target that `flags` select.
fn target_of(flags: &[&str]) -> &'static str {
    match flags {
        ["--js", ..] => "js",
        ["--target", t, ..] => match *t {
            "ts" => "ts",
            "py" => "py",
            "rs" => "rs",
            "go" => "go",
            "js" => "js",
            _ => "native",
        },
        _ => "native",
    }
}

/// True if the test `src` runs on the target of `flags`.
fn runs_on(src: &str, flags: &[&str]) -> bool {
    only(src).is_none_or(|ts| ts.iter().any(|t| t == target_of(flags)))
}

fn files(dir: &str) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> =
        std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).filter(|p| p.extension().is_some_and(|e| e == "nyra")).collect();
    v.sort();
    v
}

#[test]
fn examples_produce_expected_output_on_every_backend() {
    // (name, extra flags, NYRA_OPT) for every backend that can run on this machine; the
    // unoptimized run checks that the IR passes never change a program's output
    let mut targets: Vec<(&str, &[&str], &str)> = Vec::new();
    if available("node") {
        targets.push(("js", &["--js"], "1"));
        targets.push(("js, no optimizations", &["--js"], "0"));
    } else {
        missing("no Node.js for the JavaScript backend");
    }
    if std::env::var("NYRA_CC").is_ok() || ["gcc", "clang", "cc", "tcc"].iter().any(|c| available(c)) {
        // (`run` compiles the C with -O1; `--release` is the -O2 of `build`: both must agree)
        targets.push(("native", &[], "1"));
        targets.push(("native, --release", &["--release"], "1"));
        targets.push(("native, no optimizations", &[], "0"));
    } else {
        missing("no C compiler for the native backend");
    }

    for path in files("examples") {
        let Ok(expected) = std::fs::read_to_string(path.with_extension("out")) else { continue };
        for (target, flags, opt) in &targets {
            // NYRA_LEAKCHECK: native programs exit with 102 on a leak, a double free or a use after free
            let (out, exit) = run_example(&path, flags, &[("NYRA_OPT", opt), ("NYRA_LEAKCHECK", "1")]);
            assert_eq!(
                out.status.code(),
                Some(exit),
                "{} [{target}] failed:\n{}",
                path.display(),
                String::from_utf8_lossy(&out.stderr)
            );
            let got = String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n");
            assert_eq!(got, expected.replace("\r\n", "\n"), "{} [{target}] output differs", path.display());
        }
    }
}

#[test]
fn runtime_errors_report_code_position_and_exit_101() {
    let mut backends: Vec<&[&str]> = Vec::new();
    if available("node") {
        backends.push(&["--js"]);
    } else {
        missing("no Node.js for the JavaScript backend");
    }
    if std::env::var("NYRA_CC").is_ok() || ["gcc", "clang", "cc", "tcc"].iter().any(|c| available(c)) {
        backends.push(&[]);
        backends.push(&["--release"]);
    } else {
        missing("no C compiler for the native backend");
    }
    for path in files("tests/runtime") {
        let src = std::fs::read_to_string(&path).unwrap();
        // first line: `// expect: E0241 at 2:35`
        let expect = src
            .lines()
            .next()
            .and_then(|l| l.strip_prefix("// expect: "))
            .unwrap_or_else(|| panic!("{} has no `// expect:` line", path.display()));
        let (code, at) = expect.trim().split_once(" at ").expect("expected `E0xxx at L:C`");
        let stdout_expected = std::fs::read_to_string(path.with_extension("out")).unwrap_or_default();
        for flags in &backends {
            if !runs_on(&src, flags) {
                continue;
            }
            let (out, _) = run_example(&path, flags, &[]);
            let (stdout, stderr) = (String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
            let label = format!("{} {:?}", path.display(), flags);
            assert_eq!(out.status.code(), Some(101), "{label}: exit code; stderr:\n{stderr}");
            assert!(stderr.contains(&format!("runtime error[{code}]")), "{label}: stderr was:\n{stderr}");
            assert!(
                stderr.contains(&format!(":{at}\n")) || stderr.contains(&format!(":{at}\r\n")),
                "{label}: position {at} missing:\n{stderr}"
            );
            assert_eq!(
                stdout.replace("\r\n", "\n"),
                stdout_expected.replace("\r\n", "\n"),
                "{label}: stdout (must be flushed before the error)"
            );

            let (json, _) = run_example(&path, &[*flags, &["--json"][..]].concat(), &[]);
            let stderr = String::from_utf8_lossy(&json.stderr);
            assert!(
                stderr.contains(&format!("\"code\":\"{code}\"")) && stderr.contains("\"runtime\":true"),
                "{label} --json: {stderr}"
            );
        }
    }
}

#[test]
fn bad_programs_report_expected_error_codes() {
    for path in files("tests/errors") {
        let src = std::fs::read_to_string(&path).unwrap();
        let code = src
            .lines()
            .next()
            .and_then(|l| l.strip_prefix("// expect: "))
            .unwrap_or_else(|| panic!("{} has no `// expect:` line", path.display()))
            .trim();
        let out = nyra().args(["check", "--json"]).arg(&path).output().unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(!out.status.success(), "{} should fail to compile", path.display());
        assert!(stdout.starts_with("{\"ok\":false"), "{}: not JSON: {stdout}", path.display());
        assert!(stdout.contains(&format!("\"code\":\"{code}\"")), "{}: expected {code}, got {stdout}", path.display());
    }
}

// ---- the other targets: Python, TypeScript, Rust, Go ----------------------------------------
// Every example and every runtime-error test runs on each target whose tool is installed (a
// missing tool skips its target). The output, the error code, its position, the exit code 101
// and the JSON report must be exactly those of the C and JavaScript backends.

/// True if `cmd args` runs and succeeds (`python3 --version`; a Windows store stub fails).
fn works(cmd: &str, args: &[&str]) -> bool {
    Command::new(cmd).args(args).output().is_ok_and(|o| o.status.success())
}

/// Node 22.6 and later run TypeScript (they strip the types).
fn node_runs_typescript() -> bool {
    let Ok(out) = Command::new("node").arg("--version").output() else { return false };
    let v = String::from_utf8_lossy(&out.stdout);
    let mut parts = v.trim().trim_start_matches('v').split('.').map(|p| p.parse::<u32>().unwrap_or(0));
    let (major, minor) = (parts.next().unwrap_or(0), parts.next().unwrap_or(0));
    major > 22 || (major == 22 && minor >= 6)
}

/// Runs one example or runtime test on a target; `None` when it behaves like the reference.
fn check_on(path: &PathBuf, flags: &[&str]) -> Option<String> {
    let label = format!("{} {flags:?}", path.display());
    let src = std::fs::read_to_string(path).unwrap();
    if !runs_on(&src, flags) {
        return None;
    }
    let expect = src.lines().next().and_then(|l| l.strip_prefix("// expect: "));
    let Some(expect) = expect else {
        let expected = std::fs::read_to_string(path.with_extension("out")).ok()?.replace("\r\n", "\n");
        let (out, exit) = run_example(path, flags, &[]);
        let (stdout, stderr) =
            (String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n"), String::from_utf8_lossy(&out.stderr).replace("\r\n", "\n"));
        if out.status.code() != Some(exit) {
            return Some(format!("{label} failed (exit code {:?}, expected {exit}):\n{stderr}", out.status.code()));
        }
        return (stdout != expected).then(|| format!("{label} output differs:\n--- got\n{stdout}--- expected\n{expected}"));
    };
    let (out, _) = run_example(path, flags, &[]);
    let (stdout, stderr) =
        (String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n"), String::from_utf8_lossy(&out.stderr).replace("\r\n", "\n"));
    let (code, at) = expect.trim().split_once(" at ").expect("expected `E0xxx at L:C`");
    let stdout_expected = std::fs::read_to_string(path.with_extension("out")).unwrap_or_default().replace("\r\n", "\n");
    if out.status.code() != Some(101) {
        return Some(format!("{label}: exit code {:?}; stderr:\n{stderr}", out.status.code()));
    }
    if !stderr.contains(&format!("runtime error[{code}]")) || !stderr.contains(&format!(":{at}\n")) {
        return Some(format!("{label}: expected {code} at {at}, stderr was:\n{stderr}"));
    }
    if stdout != stdout_expected {
        return Some(format!("{label}: stdout (must be flushed before the error) was:\n{stdout}"));
    }
    let (json, _) = run_example(path, &[flags, &["--json"][..]].concat(), &[]);
    let stderr = String::from_utf8_lossy(&json.stderr);
    if !(stderr.contains(&format!("\"code\":\"{code}\"")) && stderr.contains("\"runtime\":true")) {
        return Some(format!("{label} --json: {stderr}"));
    }
    None
}

/// Every example and runtime test on one target, a few at a time (Rust and Go compile each).
fn check_target(flags: &[&str]) {
    let mut all = files("examples");
    all.extend(files("tests/runtime"));
    let next = std::sync::atomic::AtomicUsize::new(0);
    let failures = std::sync::Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..4 {
            s.spawn(|| loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let Some(path) = all.get(i) else { break };
                if let Some(f) = check_on(path, flags) {
                    failures.lock().unwrap().push(f);
                }
            });
        }
    });
    let failures = failures.into_inner().unwrap();
    assert!(failures.is_empty(), "{} failure(s):\n\n{}", failures.len(), failures.join("\n\n"));
}

#[test]
fn every_example_on_python() {
    let py = std::env::var("NYRA_PYTHON")
        .ok()
        .or_else(|| ["python3", "python"].into_iter().find(|p| works(p, &["--version"])).map(String::from));
    if py.is_none() {
        missing("no Python (python3, python or NYRA_PYTHON)");
        return;
    }
    check_target(&["--target", "py"]);
}

#[test]
fn every_example_on_typescript() {
    if !node_runs_typescript() {
        missing("TypeScript needs Node.js 22.6 or later");
        return;
    }
    check_target(&["--target", "ts"]);
}

#[test]
fn every_example_on_rust() {
    let rustc = std::env::var("NYRA_RUSTC").unwrap_or_else(|_| "rustc".into());
    if !works(&rustc, &["--version"]) {
        missing("no rustc (or NYRA_RUSTC)");
        return;
    }
    check_target(&["--target", "rs"]);
}

#[test]
fn every_example_on_go() {
    let go = std::env::var("NYRA_GO").unwrap_or_else(|_| "go".into());
    if !works(&go, &["version"]) {
        missing("no go (or NYRA_GO)");
        return;
    }
    check_target(&["--target", "go"]);
}
