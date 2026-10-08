//! `ex` examples: `nyra test` in both forms, `nyra check` and `run` refusing a program with a
//! false example, and examples never reaching the generated program.

mod common;

use common::{nyra, scratch, stderr, stdout, Json};

fn write(name: &str, code: &str) -> std::path::PathBuf {
    let path = scratch("inline-examples").join(name);
    std::fs::write(&path, code).unwrap();
    path
}

const WRONG: &str = "\
fn dist(a: int, b: int) -> int {
    if a > b { ret a - b }
    ret a - b
}
ex dist(7, 2) == 5, dist(2, 7) == 5
fn sq(x: int) -> int = x * x   ex sq(3) == 9, sq(4) != 16

fn main() {
    print(dist(1, 2))
}
";

#[test]
fn test_reports_every_failed_example() {
    let path = write("wrong.nyra", WRONG);
    let out = nyra().arg("test").arg(&path).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = stderr(&out);
    assert!(err.contains("error[E0250]: example `dist(2, 7) == 5` is false: `dist(2, 7)` is -5, not 5"), "{err}");
    assert!(err.contains("wrong.nyra:5:21"), "{err}");
    assert!(err.contains("`dist` is wrong for a = 2, b = 7"), "{err}");
    assert!(err.contains("example `sq(4) != 16` is false: `sq(4)` is 16"), "{err}");
    assert!(err.contains("4 examples: 2 passed, 2 failed"), "{err}");

    let out = nyra().args(["test", "--json"]).arg(&path).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let json = Json::parse(stdout(&out).trim()).unwrap();
    assert_eq!(json.keys(), ["ok", "file", "examples", "passed", "failed", "errors"]);
    assert_eq!(json.get("ok").and_then(Json::as_bool), Some(false));
    assert_eq!(json.get("examples").and_then(Json::as_u64), Some(4));
    assert_eq!(json.get("passed").and_then(Json::as_u64), Some(2));
    let errors = json.get("errors").and_then(Json::as_array).unwrap();
    assert_eq!(errors.len(), 2);
    let e = &errors[0];
    assert_eq!(e.keys(), ["code", "message", "file", "line", "col", "hint", "actual", "expected"]);
    assert_eq!(e.get("actual").and_then(Json::as_str), Some("-5"));
    assert_eq!(e.get("expected").and_then(Json::as_str), Some("5"));
    assert_eq!(errors[1].get("expected").and_then(Json::as_str), Some("anything but 16"));

    // `check`, `run` and `build` refuse the program with the same errors
    let out = nyra().args(["check", "--json"]).arg(&path).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let check = Json::parse(stdout(&out).trim()).unwrap();
    assert_eq!(check.get("errors"), json.get("errors"));
    let out = nyra().args(["run", "--js"]).arg(&path).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(stdout(&out).is_empty());
}

#[test]
fn test_passes_and_examples_are_not_compiled() {
    let out = nyra().args(["test", "examples/inline_examples.nyra"]).output().unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stderr(&out).contains("19 examples: 19 passed"), "{}", stderr(&out));

    // the generated program has no trace of them
    for target in ["js", "c", "py"] {
        let out = nyra().args(["build", "examples/inline_examples.nyra", "--target", target, "-o", "-"]).output().unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        let code = stdout(&out);
        assert!(!code.contains("ex#") && !code.contains("Hello Nyra World"), "{target}:\n{code}");
    }

    // no examples: nothing to do, and that is fine
    let path = write("none.nyra", "fn main() {\n    print(1)\n}\n");
    let out = nyra().arg("test").arg(&path).output().unwrap();
    assert!(out.status.success());
    assert!(stderr(&out).contains("no examples"), "{}", stderr(&out));
    let out = nyra().args(["test", "--json"]).arg(&path).output().unwrap();
    assert_eq!(stdout(&out).trim(), format!("{{\"ok\":true,\"file\":{:?},\"examples\":0,\"passed\":0,\"failed\":0,\"errors\":[]}}", path.display().to_string()));
}

#[test]
fn compile_errors_come_first() {
    let path = write("typo.nyra", "fn sq(x: int) -> int = x * y   ex sq(2) == 4\nfn main() {\n    print(sq(2))\n}\n");
    let out = nyra().args(["test", "--json"]).arg(&path).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let json = Json::parse(stdout(&out).trim()).unwrap();
    assert_eq!(json.keys(), ["ok", "errors"]);
    let e = &json.get("errors").and_then(Json::as_array).unwrap()[0];
    assert_eq!(e.get("code").and_then(Json::as_str), Some("E0201"));
}

#[test]
fn ex_is_still_a_name() {
    // `ex` starts examples only when a condition follows it on the same line
    let path = write("name.nyra", "var ex = [1]\nex.push(2)\nex[0] = 5\nex += [3]\nprint(ex, ex.len())\nfn ex2() -> int = 2  ex ex2() == 2\n");
    let out = nyra().args(["run", "--js"]).arg(&path).output().unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stdout(&out).trim(), "[5, 2, 3] 3");
}
