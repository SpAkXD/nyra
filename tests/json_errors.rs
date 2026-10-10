//! The text of a `json.parse` runtime error is the same on every target. `tests/examples.rs` already checks
//! the code, the position and the exit code of every `tests/runtime/*.nyra`; this checks the message of
//! the `json_*.nyra` ones, which carry it in a line `// message: ...`, on the interpreter and on every
//! target whose tools are installed.

mod common;

use common::{missing, nyra, stderr};
use std::process::Command;

fn works(cmd: &str, args: &[&str]) -> bool {
    Command::new(cmd).args(args).output().is_ok_and(|o| o.status.success())
}

fn node_runs_typescript() -> bool {
    let Ok(out) = Command::new("node").arg("--version").output() else { return false };
    let v = String::from_utf8_lossy(&out.stdout);
    let mut parts = v.trim().trim_start_matches('v').split('.').map(|p| p.parse::<u32>().unwrap_or(0));
    let (major, minor) = (parts.next().unwrap_or(0), parts.next().unwrap_or(0));
    major > 22 || (major == 22 && minor >= 6)
}

#[test]
fn json_parse_errors_read_the_same_on_every_target() {
    let mut targets: Vec<(&str, Vec<&str>)> = vec![("interpreter", vec!["--interp"])];
    if works("node", &["--version"]) {
        targets.push(("js", vec!["--js"]));
    } else {
        missing("no Node.js for the JavaScript target");
    }
    if std::env::var("NYRA_CC").is_ok() || ["gcc", "clang", "cc", "tcc"].iter().any(|c| works(c, &["--version"])) {
        targets.push(("native", vec![]));
    } else {
        missing("no C compiler for the native target");
    }
    let py = std::env::var("NYRA_PYTHON")
        .ok()
        .or_else(|| ["python3", "python"].into_iter().find(|p| works(p, &["--version"])).map(String::from));
    if py.is_some() {
        targets.push(("py", vec!["--target", "py"]));
    } else {
        missing("no Python");
    }
    if node_runs_typescript() {
        targets.push(("ts", vec!["--target", "ts"]));
    } else {
        missing("TypeScript needs Node.js 22.6 or later");
    }
    if works(&std::env::var("NYRA_RUSTC").unwrap_or_else(|_| "rustc".into()), &["--version"]) {
        targets.push(("rs", vec!["--target", "rs"]));
    } else {
        missing("no rustc");
    }
    if works(&std::env::var("NYRA_GO").unwrap_or_else(|_| "go".into()), &["version"]) {
        targets.push(("go", vec!["--target", "go"]));
    } else {
        missing("no go");
    }
    let mut files: Vec<_> = std::fs::read_dir("tests/runtime")
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "nyra") && p.file_name().unwrap().to_string_lossy().starts_with("json_"))
        .collect();
    files.sort();
    let mut failures = Vec::new();
    let mut checked = 0;
    for path in &files {
        let src = std::fs::read_to_string(path).unwrap();
        let Some(message) = src.lines().find_map(|l| l.strip_prefix("// message: ")) else { continue };
        for (name, flags) in &targets {
            let out = nyra().arg("run").arg(path).args(flags).output().unwrap();
            let err = stderr(&out);
            checked += 1;
            if out.status.code() != Some(101) || !err.contains(&format!("runtime error[E0345]: {message}\n")) {
                failures.push(format!(
                    "{} [{name}]: exit {:?}, stderr:\n{err}\nexpected the message: {message}",
                    path.display(),
                    out.status.code()
                ));
            }
        }
    }
    assert!(checked > 0, "no json error test with a message found");
    assert!(failures.is_empty(), "{} failure(s):\n\n{}", failures.len(), failures.join("\n\n"));
}
