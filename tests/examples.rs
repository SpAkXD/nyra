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
    // (name, extra flags) for every backend that can run on this machine
    let mut targets: Vec<(&str, &[&str])> = Vec::new();
    if available("node") {
        targets.push(("js", &["--js"]));
    }
    if std::env::var("NYRA_CC").is_ok() || ["gcc", "clang", "cc", "tcc"].iter().any(|c| available(c)) {
        targets.push(("native", &[]));
    }

    for path in files("examples") {
        let Ok(expected) = std::fs::read_to_string(path.with_extension("out")) else { continue };
        for (target, flags) in &targets {
            let out = nyra().arg("run").arg(&path).args(*flags).output().unwrap();
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
