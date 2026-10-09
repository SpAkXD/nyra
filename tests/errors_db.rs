//! Keeps the error database (`docs/ERRORS.md`, printed by `nyra explain`) honest:
//!
//! - every error code the compiler can emit has an entry, and every entry that is not marked
//!   "planned" is a code the compiler really emits;
//! - the "Wrong" program of every such entry produces exactly that code and the "Fixed" program
//!   compiles and runs (a run-time error entry fails with exit code 101 instead);
//! - `nyra explain` works, human errors point to it, and the other docs point to the database.

mod common;

use std::collections::BTreeSet;
use std::path::Path;
use std::io::Write;
use std::process::{Command, Stdio};

use common::{check_json, nyra, scratch, stderr, stdout, Json};

fn available(tool: &str) -> bool {
    Command::new(tool).arg("--version").output().is_ok()
}

/// The bytes `\xNN` and `\n` stand for in a `// stdin:` line.
fn unescape(text: &str) -> Vec<u8> {
    let (b, mut out, mut i) = (text.as_bytes(), Vec::new(), 0);
    while i < b.len() {
        if b[i] == b'\\' && b.get(i + 1) == Some(&b'n') {
            out.push(b'\n');
            i += 2;
        } else if b[i] == b'\\' && b.get(i + 1) == Some(&b'x') && i + 4 <= b.len() {
            out.push(u8::from_str_radix(&text[i + 2..i + 4], 16).expect("\\xNN"));
            i += 4;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out
}

/// Every `E` followed by four digits in the text.
fn codes_in(text: &str) -> BTreeSet<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut found = BTreeSet::new();
    for i in 0..chars.len() {
        let starts_word = i == 0 || !(chars[i - 1].is_alphanumeric() || chars[i - 1] == '_');
        if chars[i] == 'E'
            && starts_word
            && chars.len() >= i + 5
            && chars[i + 1..i + 5].iter().all(|c| c.is_ascii_digit())
            && chars.get(i + 5).is_none_or(|c| !c.is_ascii_digit())
        {
            found.insert(chars[i..i + 5].iter().collect());
        }
    }
    found
}

/// Every file below `dir`: the runtime that the backends embed (C, JavaScript) may emit codes too.
fn source_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            source_files(&path, out);
        } else {
            out.push(path);
        }
    }
}

struct Listed {
    code: String,
    kind: String,
    planned: bool,
}

/// The list printed by `nyra explain --json`.
fn listed() -> Vec<Listed> {
    let out = nyra().args(["explain", "--json"]).output().unwrap();
    assert!(out.status.success(), "`nyra explain --json` failed: {}", stderr(&out));
    let json = Json::parse(stdout(&out).trim()).expect("`nyra explain --json` must print JSON");
    json.get("codes")
        .and_then(|c| c.as_array())
        .expect("a `codes` array")
        .iter()
        .map(|c| Listed {
            code: c.get("code").and_then(|v| v.as_str()).unwrap().to_string(),
            kind: c.get("kind").and_then(|v| v.as_str()).unwrap().to_string(),
            planned: c.get("planned").and_then(|v| v.as_bool()).unwrap(),
        })
        .collect()
}

fn entry(code: &str) -> Json {
    let out = nyra().args(["explain", code, "--json"]).output().unwrap();
    assert!(out.status.success(), "`nyra explain {code} --json` failed: {}", stderr(&out));
    Json::parse(stdout(&out).trim()).unwrap_or_else(|e| panic!("`nyra explain {code} --json`: {e}"))
}

#[test]
fn every_emitted_code_has_an_entry_and_every_entry_is_emitted() {
    let mut files = Vec::new();
    source_files(Path::new("src"), &mut files);
    let mut emitted = BTreeSet::new();
    for f in &files {
        // explain.rs only mentions codes in its own documentation and tests
        if f.file_name().is_some_and(|n| n == "explain.rs") {
            continue;
        }
        let text = String::from_utf8_lossy(&std::fs::read(f).unwrap()).into_owned();
        emitted.extend(codes_in(&text));
    }
    assert!(emitted.len() >= 22, "found only {} codes in src/: {emitted:?}", emitted.len());

    let all = listed();
    let current: BTreeSet<String> = all.iter().filter(|e| !e.planned).map(|e| e.code.clone()).collect();
    let planned: BTreeSet<String> = all.iter().filter(|e| e.planned).map(|e| e.code.clone()).collect();

    let undocumented: Vec<_> = emitted.difference(&current).collect();
    assert!(
        undocumented.is_empty(),
        "the compiler emits {undocumented:?}, which has no entry in docs/ERRORS.md (or whose entry still says \"planned\"): \
         add or update the entry, with a Wrong and a Fixed example"
    );
    let dead: Vec<_> = current.difference(&emitted).collect();
    assert!(
        dead.is_empty(),
        "docs/ERRORS.md describes {dead:?} as current, but no source file in src/ emits it: mark the entry \"planned\" or remove it"
    );
    assert!(current.is_disjoint(&planned));
}

#[test]
fn wrong_examples_produce_their_code_and_fixed_examples_run() {
    // run the examples on one backend: JavaScript when Node is there (it starts faster), else native
    let backend: Option<&[&str]> = if available("node") {
        Some(&["--js"])
    } else if std::env::var("NYRA_CC").is_ok() || ["gcc", "clang", "cc", "tcc"].iter().any(|c| available(c)) {
        Some(&[])
    } else {
        None
    };
    let dir = scratch("errors-db");
    let mut checked = 0;
    for item in listed().iter().filter(|e| !e.planned) {
        let code = &item.code;
        let e = entry(code);
        let wrong = e.get("wrong").and_then(|v| v.as_str()).unwrap();
        let fixed = e.get("fixed").and_then(|v| v.as_str()).unwrap();
        assert!(wrong.contains("fn ") && fixed.contains("fn "), "{code}: the examples must be whole programs");

        let wrong_file = format!("{code}_wrong.nyra");
        let fixed_file = format!("{code}_fixed.nyra");
        std::fs::write(dir.join(&wrong_file), wrong).unwrap();
        std::fs::write(dir.join(&fixed_file), fixed).unwrap();

        // the fixed program compiles cleanly...
        let (ok, json) = check_json(&dir, &fixed_file);
        assert!(ok, "{code}: the Fixed example does not compile:\n{fixed}\n{json:?}");

        if item.kind == "runtime error" {
            // ...and the wrong one compiles but stops at run time with that code
            let (ok, json) = check_json(&dir, &wrong_file);
            assert!(ok, "{code}: a run-time error example must compile:\n{wrong}\n{json:?}");
            // a first line `// target: js`: the error happens only on JavaScript (and TypeScript)
            let js_only = wrong.starts_with("// target: js");
            if let Some(flags) = backend.filter(|f| !js_only || f.contains(&"--js")) {
                // a first line `// stdin: ...` is the program's input (`\xff`, `\n` escapes)
                let input = wrong.lines().next().and_then(|l| l.strip_prefix("// stdin: ")).map(unescape).unwrap_or_default();
                let mut child = nyra()
                    .current_dir(&dir)
                    .arg("run")
                    .arg(&wrong_file)
                    .args(flags)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap();
                let _ = child.stdin.take().unwrap().write_all(&input);
                let out = child.wait_with_output().unwrap();
                assert_eq!(out.status.code(), Some(101), "{code}: the Wrong example should stop with exit code 101\n{}", stderr(&out));
                assert!(stderr(&out).contains(&format!("runtime error[{code}]")), "{code}: stderr was\n{}", stderr(&out));
            }
        } else {
            // ...and the wrong one reports exactly this code (possibly several times)
            let (ok, json) = check_json(&dir, &wrong_file);
            assert!(!ok, "{code}: the Wrong example compiles:\n{wrong}");
            let errors = json.get("errors").and_then(|e| e.as_array()).unwrap();
            assert!(!errors.is_empty(), "{code}: no errors reported");
            for err in errors {
                assert_eq!(
                    err.get("code").and_then(|c| c.as_str()),
                    Some(code.as_str()),
                    "{code}: the Wrong example must produce only this code, but got {}",
                    err.get("message").and_then(|m| m.as_str()).unwrap_or("?")
                );
                let hint = err.get("hint").and_then(|h| h.as_str()).unwrap_or("");
                assert!(!hint.is_empty(), "{code}: the error has no hint");
            }
        }
        if let Some(flags) = backend {
            let out = nyra().current_dir(&dir).arg("run").arg(&fixed_file).args(flags).output().unwrap();
            assert!(out.status.success(), "{code}: the Fixed example fails to run:\n{fixed}\n{}", stderr(&out));
        }
        checked += 1;
    }
    assert!(checked >= 22, "only {checked} entries were checked");
}

#[test]
fn explain_prints_an_entry() {
    let out = nyra().args(["explain", "E0201"]).output().unwrap();
    assert!(out.status.success());
    let text = stdout(&out);
    for part in ["E0201: undefined variable", "What it means", "Why Nyra has this rule", "Common causes", "Wrong", "Fixed", "Related:"] {
        assert!(text.contains(part), "missing `{part}` in:\n{text}");
    }
    assert!(text.contains("print(cout)") && text.contains("print(count)"));
    // lower case, without the E and without the zeros all name the same code
    for alias in ["e0201", "0201", "201"] {
        let again = nyra().args(["explain", alias]).output().unwrap();
        assert_eq!(stdout(&again), text, "`nyra explain {alias}`");
    }
    // a planned code says so
    let planned = stdout(&nyra().args(["explain", "E0310"]).output().unwrap());
    assert!(planned.contains("planned for v0.6, not in the compiler yet"), "{planned}");
    // a run-time code
    let runtime = stdout(&nyra().args(["explain", "E0241"]).output().unwrap());
    assert!(runtime.contains("division by zero") && runtime.contains("runtime error"), "{runtime}");
}

#[test]
fn explain_prints_json() {
    let e = entry("E0203");
    assert_eq!(e.keys(), ["code", "title", "kind", "since", "planned", "what", "why", "causes", "wrong", "fixed", "related"]);
    assert_eq!(e.get("code").and_then(|v| v.as_str()), Some("E0203"));
    assert_eq!(e.get("title").and_then(|v| v.as_str()), Some("type mismatch"));
    assert_eq!(e.get("kind").and_then(|v| v.as_str()), Some("compile error"));
    assert_eq!(e.get("since").and_then(|v| v.as_str()), Some("v0.1"));
    assert_eq!(e.get("planned").and_then(|v| v.as_bool()), Some(false));
    assert!(e.get("causes").and_then(|c| c.as_array()).is_some_and(|c| c.len() >= 3));
    assert!(e.get("related").and_then(|c| c.as_array()).is_some_and(|c| !c.is_empty()));
    assert!(e.get("wrong").and_then(|v| v.as_str()).is_some_and(|w| w.contains("half(n)")));

    let planned = entry("E0310");
    assert_eq!(planned.get("planned").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(planned.get("since").and_then(|v| v.as_str()), Some("planned for v0.6, not in the compiler yet"));
}

#[test]
fn explain_lists_every_code() {
    let text = stdout(&nyra().arg("explain").output().unwrap());
    let all = listed();
    assert!(all.len() >= 60);
    for item in &all {
        assert!(text.contains(&item.code), "{} is missing from the list", item.code);
    }
    assert!(text.contains("compile errors") && text.contains("run-time errors") && text.contains("planned, not in the compiler yet"));
    assert!(all.iter().any(|e| e.code == "E0245" && e.kind == "runtime error" && !e.planned));
    let codes: Vec<&str> = all.iter().map(|e| e.code.as_str()).collect();
    let mut sorted = codes.clone();
    sorted.sort();
    assert_eq!(codes, sorted, "the list is sorted by code");
}

#[test]
fn explain_suggests_a_code_for_an_unknown_one() {
    let out = nyra().args(["explain", "E0299"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let err = stderr(&out);
    assert!(err.contains("unknown error code `E0299`") && err.contains("did you mean `E0209`"), "{err}");
    assert!(err.contains("nyra explain"), "{err}");

    let out = nyra().args(["explain", "E0299", "--json"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let json = Json::parse(stdout(&out).trim()).unwrap();
    assert_eq!(json.get("ok").and_then(|v| v.as_bool()), Some(false));
    assert_eq!(json.get("did_you_mean").and_then(|v| v.as_str()), Some("E0209"));

    // nothing close: still helpful, still exit code 2
    let out = nyra().args(["explain", "hello"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("run `nyra explain` to list every code"));

    // usage problems are tool errors too
    assert_eq!(nyra().args(["explain", "E0201", "E0202"]).output().unwrap().status.code(), Some(2));
    assert_eq!(nyra().args(["explain", "--nope"]).output().unwrap().status.code(), Some(2));
    assert!(nyra().args(["explain", "--help"]).output().unwrap().status.success());
}

#[test]
fn compile_errors_point_to_explain() {
    let dir = scratch("errors-db-explain");
    std::fs::write(dir.join("a.nyra"), "fn main() {\n    let count = 1\n    print(cout)\n}\n").unwrap();
    let out = nyra().current_dir(&dir).args(["check", "a.nyra"]).output().unwrap();
    let err = stderr(&out);
    assert!(err.contains("error[E0201]: undefined variable `cout`"), "{err}");
    assert!(err.contains("  = hint: did you mean `count`?\n  = explain: nyra explain E0201\n"), "{err}");
}

#[test]
fn every_error_program_in_the_tests_has_a_hint_and_a_documented_code() {
    let documented: BTreeSet<String> = listed().into_iter().map(|e| e.code).collect();
    let mut seen = 0;
    for entry in std::fs::read_dir("tests/errors").unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "nyra") {
            continue;
        }
        let (ok, json) = check_json(Path::new("."), path.to_str().unwrap());
        assert!(!ok, "{} should not compile", path.display());
        for err in json.get("errors").and_then(|e| e.as_array()).unwrap() {
            let code = err.get("code").and_then(|c| c.as_str()).unwrap();
            assert!(documented.contains(code), "{}: {code} has no entry in docs/ERRORS.md", path.display());
            let hint = err.get("hint").and_then(|h| h.as_str()).unwrap_or("");
            assert!(!hint.is_empty(), "{}: {code} has no hint", path.display());
        }
        seen += 1;
    }
    assert!(seen >= 25);
}

/// The codes named in the first line (`// expect: E0203 ...`) of the programs in a folder.
fn expected_codes(dir: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "nyra") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let first = text.lines().next().unwrap_or("");
        let code = first.strip_prefix("// expect: ").and_then(|r| r.split_whitespace().next());
        found.insert(code.unwrap_or_else(|| panic!("{} has no `// expect:` line", path.display())).to_string());
    }
    found
}

#[test]
fn every_code_of_the_compiler_has_a_test_program() {
    let compile = expected_codes("tests/errors");
    let runtime = expected_codes("tests/runtime");
    for item in listed().iter().filter(|e| !e.planned) {
        let folder = if item.kind == "runtime error" { ("tests/runtime", &runtime) } else { ("tests/errors", &compile) };
        assert!(
            folder.1.contains(&item.code),
            "{} has no program in {}: add one whose first line is `// expect: {}`",
            item.code,
            folder.0,
            item.code
        );
    }
}

#[test]
fn the_docs_point_to_the_database_and_mention_only_documented_codes() {
    let documented: BTreeSet<String> = listed().into_iter().map(|e| e.code).collect();
    for (file, needles) in [
        ("README.md", &["nyra explain", "docs/ERRORS.md"][..]),
        ("docs/AI_GUIDE.md", &["nyra explain", "ERRORS.md"][..]),
        ("docs/SPEC.md", &["ERRORS.md"][..]),
        ("llms.txt", &["ERRORS.md", "nyra explain"][..]),
        ("CONTRIBUTING.md", &["ERRORS.md"][..]),
    ] {
        let text = std::fs::read_to_string(file).unwrap();
        for needle in needles {
            assert!(text.contains(needle), "{file} should mention `{needle}`");
        }
        for code in codes_in(&text) {
            assert!(documented.contains(&code), "{file} mentions {code}, which has no entry in docs/ERRORS.md");
        }
    }
}
