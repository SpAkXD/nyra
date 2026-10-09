//! Warnings: a mistake the language allows, so the build goes on. They are printed to stderr as
//! `warning[E0260]: ...` and listed under `"warnings"` in `--json`; they never change the exit code.

mod common;

use common::{check_json, nyra, scratch, stderr, stdout};

/// Every program in `tests/warnings` compiles and gives the warning named in its first line.
#[test]
fn the_warning_programs_warn() {
    let mut seen = 0;
    for entry in std::fs::read_dir("tests/warnings").unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "nyra") {
            continue;
        }
        let src = std::fs::read_to_string(&path).unwrap();
        let code = src.lines().next().and_then(|l| l.strip_prefix("// expect: ")).expect("a `// expect:` line").trim().to_string();
        let (ok, json) = check_json(std::path::Path::new("."), path.to_str().unwrap());
        assert!(ok, "{}: a warning program must compile", path.display());
        let warnings = json.get("warnings").and_then(|w| w.as_array()).unwrap_or(&[]);
        assert!(
            !warnings.is_empty() && warnings.iter().all(|w| w.get("code").and_then(|c| c.as_str()) == Some(code.as_str())),
            "{}: expected only {code}, got {warnings:?}",
            path.display()
        );
        seen += 1;
    }
    assert!(seen >= 1);
}

const DOLLAR: &str = "let x = 3\nprint(\"cost: ${x}\")\n";

#[test]
fn dollar_brace_warns_and_still_runs() {
    let dir = scratch("warnings-run");
    std::fs::write(dir.join("a.nyra"), DOLLAR).unwrap();
    let out = nyra().current_dir(&dir).args(["run", "a.nyra", "--js"]).output().unwrap();
    // the program is built and runs as written: the `$` is printed
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stdout(&out), "cost: $3\n");
    let err = stderr(&out);
    assert!(err.contains("warning[E0260]: `${x}` in a string prints a `$` and then the value of `x`"), "{err}");
    assert!(err.contains("  --> a.nyra:2:14\n"), "{err}");
    assert!(err.contains("  = hint: ") && err.contains("drop the `$`"), "{err}");
    assert!(err.contains("  = explain: nyra explain E0260"), "{err}");
    assert!(!err.contains("error"), "{err}");

    // `check` too, with the exit code 0
    let out = nyra().current_dir(&dir).args(["check", "a.nyra"]).output().unwrap();
    assert!(out.status.success());
    assert!(stderr(&out).contains("warning[E0260]") && stderr(&out).contains("nyra: no errors"), "{}", stderr(&out));
}

#[test]
fn dollar_brace_is_listed_in_json() {
    let dir = scratch("warnings-json");
    std::fs::write(dir.join("a.nyra"), DOLLAR).unwrap();
    let (ok, json) = check_json(&dir, "a.nyra");
    assert!(ok, "a warning never fails the build");
    assert_eq!(json.get("errors").and_then(|e| e.as_array()).map(|e| e.len()), Some(0));
    let warnings = json.get("warnings").and_then(|w| w.as_array()).expect("a `warnings` array");
    assert_eq!(warnings.len(), 1);
    let w = &warnings[0];
    assert_eq!(w.get("code").and_then(|c| c.as_str()), Some("E0260"));
    assert_eq!(w.get("line").and_then(|c| c.as_u64()), Some(2));
    assert_eq!(w.get("col").and_then(|c| c.as_u64()), Some(14));
    assert!(w.get("hint").and_then(|h| h.as_str()).is_some_and(|h| h.contains("drop the `$`")));

    // nothing to warn about: no `warnings` key at all
    std::fs::write(dir.join("b.nyra"), "let x = 3\nprint(\"cost: {x}\")\n").unwrap();
    let (ok, json) = check_json(&dir, "b.nyra");
    assert!(ok && json.get("warnings").is_none());

    // `nyra test --json` lists them too
    let out = nyra().current_dir(&dir).args(["test", "--json", "a.nyra"]).output().unwrap();
    assert!(out.status.success());
    assert!(stdout(&out).contains("\"warnings\":[{\"code\":\"E0260\""), "{}", stdout(&out));
}

#[test]
fn a_run_with_json_keeps_stdout_for_the_program() {
    let dir = scratch("warnings-run-json");
    std::fs::write(dir.join("a.nyra"), DOLLAR).unwrap();
    let out = nyra().current_dir(&dir).args(["run", "--json", "a.nyra", "--js"]).output().unwrap();
    assert!(out.status.success());
    assert_eq!(stdout(&out), "cost: $3\n");
    assert!(stderr(&out).contains("warning[E0260]"), "{}", stderr(&out));
}

#[test]
fn dollar_brace_does_not_hide_an_error() {
    let dir = scratch("warnings-error");
    std::fs::write(dir.join("a.nyra"), "print(\"${root}/logs\")\n").unwrap();
    let (ok, json) = check_json(&dir, "a.nyra");
    assert!(!ok);
    // the existing error for an undefined name inside `${name}` (with its fix) stays
    let errors = json.get("errors").and_then(|e| e.as_array()).unwrap();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].get("code").and_then(|c| c.as_str()), Some("E0201"));
    assert!(errors[0].get("fix").is_some());
    // and the warning is listed next to it
    let warnings = json.get("warnings").and_then(|w| w.as_array()).unwrap();
    assert_eq!(warnings[0].get("code").and_then(|c| c.as_str()), Some("E0260"));

    let out = nyra().current_dir(&dir).args(["check", "--strict", "a.nyra"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = stderr(&out);
    assert!(err.contains("warning[E0260]") && err.contains("error[E0201]"), "{err}");

    // without --strict the one possible fix (`${{root}}`) is applied in memory and reported
    let out = nyra().current_dir(&dir).args(["check", "a.nyra"]).output().unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(stderr(&out).contains("E0201"), "{}", stderr(&out));
}

#[test]
fn text_that_is_not_dollar_brace_does_not_warn() {
    let dir = scratch("warnings-none");
    for line in [
        "print(\"cost: {x}\")",
        "print(\"literal ${{x}}\")",
        "print(\"$5 and US$\")",
        "print(\"a $ {x}\")",
        "print(\"{x}$\")",
        "print(\"${\")",
        "print(\"${}\")",
    ] {
        std::fs::write(dir.join("a.nyra"), format!("let x = 3\n{line}\n")).unwrap();
        let (ok, json) = check_json(&dir, "a.nyra");
        assert!(ok, "{line}");
        assert!(json.get("warnings").is_none(), "{line} should not warn: {json:?}");
    }
}
