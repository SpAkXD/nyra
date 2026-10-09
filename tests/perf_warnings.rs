//! The performance warnings (E0360-E0362): every program in `tests/warnings` gives the warning named
//! in its first line (`// expect: E0360`, with `at 6:13` for its position), every program in
//! `tests/no_warnings` gives none. A line `// target: go` makes the check for the Go target.
//! Warnings never stop a build, and `--json` lists them.

mod common;

use common::{check_json_with, nyra, stderr, stdout, Json};
use std::path::{Path, PathBuf};

fn programs(dir: &str) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> =
        std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).filter(|p| p.extension().is_some_and(|e| e == "nyra")).collect();
    v.sort();
    v
}

/// The flags the program asks for: `--go` after `// target: go`.
fn flags(text: &str) -> Vec<&'static str> {
    if text.lines().take(3).any(|l| l.trim() == "// target: go") {
        vec!["--go"]
    } else {
        vec![]
    }
}

fn warnings_of(path: &Path, flags: &[&str]) -> Vec<Json> {
    let (ok, json) = check_json_with(Path::new("."), flags, path.to_str().unwrap());
    assert!(ok, "{}: a warning must not stop the build: {json:?}", path.display());
    json.get("warnings").and_then(|w| w.as_array()).map(|w| w.to_vec()).unwrap_or_default()
}

#[test]
fn slow_patterns_give_their_warning() {
    let found = programs("tests/warnings");
    assert!(found.len() >= 6);
    for path in found {
        let text = std::fs::read_to_string(&path).unwrap();
        let first = text.lines().next().unwrap();
        let mut expect =
            first.strip_prefix("// expect: ").unwrap_or_else(|| panic!("{}: no `// expect:` line", path.display())).split_whitespace();
        let code = expect.next().unwrap();
        let at = match (expect.next(), expect.next()) {
            (Some("at"), Some(pos)) => Some(pos.to_string()),
            _ => None,
        };
        let ws = warnings_of(&path, &flags(&text));
        assert!(!ws.is_empty(), "{}: no warning", path.display());
        for w in &ws {
            assert_eq!(w.get("code").and_then(|c| c.as_str()), Some(code), "{}: {w:?}", path.display());
            assert!(
                w.get("hint").and_then(|h| h.as_str()).is_some_and(|h| h.len() > 20),
                "{}: the warning needs a hint",
                path.display()
            );
        }
        if let Some(pos) = at {
            let w = &ws[0];
            let got =
                format!("{}:{}", w.get("line").and_then(|v| v.as_u64()).unwrap(), w.get("col").and_then(|v| v.as_u64()).unwrap());
            assert_eq!(got, pos, "{}: position of the warning", path.display());
        }
    }
}

#[test]
fn fast_code_gives_no_warning() {
    let found = programs("tests/no_warnings");
    assert!(found.len() >= 6);
    for path in found {
        let text = std::fs::read_to_string(&path).unwrap();
        let ws = warnings_of(&path, &flags(&text));
        assert!(ws.is_empty(), "{}: a program that is fine gave a warning: {ws:?}", path.display());
    }
}

#[test]
fn the_go_warning_is_only_for_go() {
    let path = Path::new("tests/warnings/go_append_loop.nyra");
    assert!(warnings_of(path, &[]).is_empty(), "appending is fast on the native target");
    assert!(warnings_of(path, &["--js"]).is_empty());
    assert_eq!(warnings_of(path, &["--go"]).len(), 1);
}

#[test]
fn a_warning_is_printed_and_the_program_still_runs() {
    let path = "tests/warnings/search_seen_list.nyra";
    let check = nyra().args(["check", path]).output().unwrap();
    assert!(check.status.success());
    let err = stderr(&check);
    assert!(err.contains("warning[E0360]") && err.contains("hint:") && err.contains("nyra explain E0360"), "{err}");
    assert!(err.contains("no errors"), "{err}");

    // `run` warns on stderr and prints the program's output on stdout; the exit code is the program's
    let backend: &[&str] = if has_tool("node") { &["--js"] } else { &[] };
    let run = nyra().arg("run").arg(path).args(backend).output().unwrap();
    assert!(run.status.success(), "{}", stderr(&run));
    assert_eq!(stdout(&run), "1000\n");
    assert!(stderr(&run).contains("warning[E0360]"), "{}", stderr(&run));
}

#[test]
fn the_json_of_a_check_lists_the_warnings() {
    let path = "tests/warnings/prepend_in_loop.nyra";
    let out = nyra().args(["check", "--json", path]).output().unwrap();
    let json = Json::parse(stdout(&out).trim()).unwrap();
    assert_eq!(json.get("ok").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(json.get("errors").and_then(|v| v.as_array()).map(|e| e.len()), Some(0));
    let ws = json.get("warnings").and_then(|v| v.as_array()).expect("a `warnings` list");
    assert_eq!(ws.len(), 1);
    assert_eq!(ws[0].get("code").and_then(|v| v.as_str()), Some("E0361"));
    assert!(ws[0].get("message").and_then(|v| v.as_str()).is_some_and(|m| m.contains("in a loop")));
    // a program without a slow pattern has no `warnings` key at all
    let quiet = nyra().args(["check", "--json", "tests/no_warnings/append_loop.nyra"]).output().unwrap();
    assert!(Json::parse(stdout(&quiet).trim()).unwrap().get("warnings").is_none());
}

fn has_tool(tool: &str) -> bool {
    std::process::Command::new(tool).arg("--version").output().is_ok()
}
