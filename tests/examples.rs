//! Runs every `examples/*.nyra` on every available backend and compares stdout
//! with `examples/*.out`. Also checks that `tests/errors/*.nyra` report the
//! error code named in their first line (`// expect: E0203`).

use std::path::PathBuf;
use std::process::Command;

fn nyra() -> Command {
    Command::new(env!("CARGO_BIN_EXE_nyra"))
}

fn available(tool: &str) -> bool {
    Command::new(tool).arg("--version").output().is_ok()
}

fn files(dir: &str) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "nyra"))
        .collect();
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
    }
    if std::env::var("NYRA_CC").is_ok() || ["gcc", "clang", "cc", "tcc"].iter().any(|c| available(c)) {
        targets.push(("native", &[], "1"));
        targets.push(("native, no optimizations", &[], "0"));
    }

    for path in files("examples") {
        let Ok(expected) = std::fs::read_to_string(path.with_extension("out")) else { continue };
        for (target, flags, opt) in &targets {
            // NYRA_LEAKCHECK: native programs exit with 102 on a leak, a double free or a use after free
            let out = nyra()
                .arg("run")
                .arg(&path)
                .args(*flags)
                .env("NYRA_OPT", opt)
                .env("NYRA_LEAKCHECK", "1")
                .output()
                .unwrap();
            assert!(
                out.status.success(),
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
    }
    if std::env::var("NYRA_CC").is_ok() || ["gcc", "clang", "cc", "tcc"].iter().any(|c| available(c)) {
        backends.push(&[]);
    }
    for path in files("tests/runtime") {
        let src = std::fs::read_to_string(&path).unwrap();
        // first line: `// expect: E0241 at 2:35`
        let expect = src.lines().next().and_then(|l| l.strip_prefix("// expect: ")).unwrap_or_else(|| {
            panic!("{} has no `// expect:` line", path.display())
        });
        let (code, at) = expect.trim().split_once(" at ").expect("expected `E0xxx at L:C`");
        let stdout_expected = std::fs::read_to_string(path.with_extension("out")).unwrap_or_default();
        for flags in &backends {
            let out = nyra().arg("run").arg(&path).args(*flags).output().unwrap();
            let (stdout, stderr) = (String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
            let label = format!("{} {:?}", path.display(), flags);
            assert_eq!(out.status.code(), Some(101), "{label}: exit code; stderr:\n{stderr}");
            assert!(stderr.contains(&format!("runtime error[{code}]")), "{label}: stderr was:\n{stderr}");
            assert!(stderr.contains(&format!(":{at}\n")) || stderr.contains(&format!(":{at}\r\n")), "{label}: position {at} missing:\n{stderr}");
            assert_eq!(stdout.replace("\r\n", "\n"), stdout_expected.replace("\r\n", "\n"), "{label}: stdout (must be flushed before the error)");

            let json = nyra().arg("run").arg(&path).args(*flags).arg("--json").output().unwrap();
            let stderr = String::from_utf8_lossy(&json.stderr);
            assert!(stderr.contains(&format!("\"code\":\"{code}\"")) && stderr.contains("\"runtime\":true"), "{label} --json: {stderr}");
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
        assert!(
            stdout.contains(&format!("\"code\":\"{code}\"")),
            "{}: expected {code}, got {stdout}",
            path.display()
        );
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
    let expect = src.lines().next().and_then(|l| l.strip_prefix("// expect: "));
    let out = nyra().arg("run").arg(path).args(flags).output().unwrap();
    let (stdout, stderr) = (String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n"), String::from_utf8_lossy(&out.stderr).replace("\r\n", "\n"));
    let Some(expect) = expect else {
        let expected = std::fs::read_to_string(path.with_extension("out")).ok()?.replace("\r\n", "\n");
        if !out.status.success() {
            return Some(format!("{label} failed:\n{stderr}"));
        }
        return (stdout != expected).then(|| format!("{label} output differs:\n--- got\n{stdout}--- expected\n{expected}"));
    };
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
    let json = nyra().arg("run").arg(path).args(flags).arg("--json").output().unwrap();
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
    let py = std::env::var("NYRA_PYTHON").ok().or_else(|| ["python3", "python"].into_iter().find(|p| works(p, &["--version"])).map(String::from));
    if py.is_none() {
        eprintln!("skipped: no Python (python3, python or NYRA_PYTHON)");
        return;
    }
    check_target(&["--target", "py"]);
}

#[test]
fn every_example_on_typescript() {
    if !node_runs_typescript() {
        eprintln!("skipped: TypeScript needs Node.js 22.6 or later");
        return;
    }
    check_target(&["--target", "ts"]);
}

#[test]
fn every_example_on_rust() {
    let rustc = std::env::var("NYRA_RUSTC").unwrap_or_else(|_| "rustc".into());
    if !works(&rustc, &["--version"]) {
        eprintln!("skipped: no rustc (or NYRA_RUSTC)");
        return;
    }
    check_target(&["--target", "rs"]);
}

#[test]
fn every_example_on_go() {
    let go = std::env::var("NYRA_GO").unwrap_or_else(|_| "go".into());
    if !works(&go, &["version"]) {
        eprintln!("skipped: no go (or NYRA_GO)");
        return;
    }
    check_target(&["--target", "go"]);
}
