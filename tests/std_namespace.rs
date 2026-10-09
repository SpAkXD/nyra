//! The bundled standard modules (`src/std/*.nyra`, `src/stdlib.rs`) live in a namespace of their own:
//! the names inside them (functions, parameters, variables) never clash with the names of a program,
//! the program reaches them only as `math.name`, and no error ever points outside the program's file.

mod common;

use std::collections::BTreeSet;
use std::path::Path;

use common::{check_json, nyra, scratch, stderr, stdout, Json};

/// Words that cannot be names.
const RESERVED: &[&str] = &[
    "fn", "let", "var", "if", "else", "while", "for", "in", "ret", "true", "false", "struct", "inout", "break", "continue", "arena",
    "int", "float", "bool", "str", "char", "print", "free", "keep", "step", "ex", "use", "main",
];

/// The names of the standard modules, read from `src/stdlib.rs`.
fn modules() -> Vec<String> {
    let src = std::fs::read_to_string("src/stdlib.rs").unwrap();
    let line = src.lines().find(|l| l.starts_with("pub const MODULES")).expect("MODULES in src/stdlib.rs");
    let list = &line[line.find("&[\"").unwrap() + 2..line.rfind(']').unwrap()];
    list.split(',').map(|m| m.trim().trim_matches('"').to_string()).filter(|m| !m.is_empty()).collect()
}

/// Every word of the Nyra sources of the standard modules (parameters, variables, helpers, functions),
/// plus the short names programs like to use.
fn names() -> Vec<String> {
    let mut found: BTreeSet<String> = BTreeSet::new();
    for entry in std::fs::read_dir("src/std").unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "nyra") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        for line in text.lines() {
            let code = line.split("//").next().unwrap_or("");
            let mut word = String::new();
            for c in code.chars().chain(std::iter::once(' ')) {
                if c.is_alphanumeric() || c == '_' {
                    word.push(c);
                } else {
                    if word.chars().next().is_some_and(|f| f.is_alphabetic() || f == '_') {
                        found.insert(std::mem::take(&mut word));
                    }
                    word.clear();
                }
            }
        }
    }
    for n in
        "x y z n s k r t p q hi lo i j a b c d e f m w v u sign abs min max sum len count list data res result val value".split(' ')
    {
        found.insert(n.to_string());
    }
    let mods = modules();
    found.into_iter().filter(|n| !RESERVED.contains(&n.as_str()) && !mods.contains(n)).collect()
}

/// Calls into every Nyra-written function of the `math` module, on one line.
const CALLS: &str = "print(math.sin(1.5), math.cos(0.5), math.tan(0.3), math.exp(1.0), math.log(10.0), math.pow(2.0, 0.5), math.atan2(1.0, 2.0), math.asin(0.5), math.acos(0.5), math.log2(8.0), math.log10(1000.0), math.atan(1.0), math.sqrt(2.0))";

fn uses() -> String {
    modules().iter().map(|m| format!("use {m}\n")).collect()
}

/// A program that declares every name in `names` in the way `kind` says, around calls into `math`.
fn program(kind: &str, names: &[String]) -> String {
    let mut out = uses();
    match kind {
        "functions" => {
            out += CALLS;
            out.push('\n');
            for n in names {
                out += &format!("fn {n}(q9: int) -> int = q9 + 1\n");
            }
            out += CALLS;
        }
        "parameters" => {
            for n in names {
                out += &format!("fn {n}_f({n}: float) -> float = {n} + 1.0\n");
            }
            out += CALLS;
        }
        "script_let" | "script_var" => {
            let kw = if kind == "script_let" { "let" } else { "var" };
            out += CALLS;
            out.push('\n');
            for n in names {
                out += &format!("{kw} {n} = 1.5\n");
            }
            out += CALLS;
        }
        "locals" => {
            out += "fn main() {\n";
            for n in names {
                out += &format!("    let {n} = 1.5\n");
            }
            out += &format!("    {CALLS}\n}}");
        }
        _ => unreachable!(),
    }
    out.push('\n');
    out
}

const KINDS: [&str; 5] = ["functions", "parameters", "script_let", "script_var", "locals"];

#[test]
fn std_names_never_clash_with_program_names() {
    let names = names();
    assert!(names.len() > 100, "found only {} names", names.len());
    let dir = scratch("std-namespace");
    for kind in KINDS {
        let file = format!("{kind}.nyra");
        std::fs::write(dir.join(&file), program(kind, &names)).unwrap();
        let (ok, json) = check_json(&dir, &file);
        assert!(ok, "{kind}: a program that only uses names of the standard modules does not compile:\n{json:?}");
    }
}

#[test]
fn programs_with_std_names_run_the_same_on_a_backend() {
    let names = names();
    let dir = scratch("std-namespace-run");
    // the same calls with no user names at all: the output to compare with
    std::fs::write(dir.join("plain.nyra"), format!("use math\n{CALLS}\n{CALLS}\n")).unwrap();
    let flags: &[&str] = if has_tool("node") { &["--js"] } else { &[] };
    let plain = nyra().current_dir(&dir).args(["run", "plain.nyra"]).args(flags).output().unwrap();
    assert!(plain.status.success(), "{}", stderr(&plain));
    for kind in ["functions", "script_let", "script_var"] {
        let file = format!("{kind}.nyra");
        std::fs::write(dir.join(&file), program(kind, &names)).unwrap();
        let out = nyra().current_dir(&dir).args(["run", &file]).args(flags).output().unwrap();
        assert!(out.status.success(), "{kind}: {}", stderr(&out));
        assert_eq!(stdout(&out), stdout(&plain), "{kind}: the standard functions give other results next to the program's names");
    }
}

fn has_tool(tool: &str) -> bool {
    std::process::Command::new(tool).arg("--version").output().is_ok()
}

#[test]
fn a_program_name_that_is_a_module_name_is_one_clear_error() {
    // a script variable or a function named like the module: the only error is the name clash, at the
    // program's own line, and the module keeps working for the rest of the program
    let dir = scratch("std-namespace-clash");
    for (src, line) in [
        ("use math\nlet math = 3\nprint(1)\n", 2),
        ("use math\nvar math = 3\nprint(1)\n", 2),
        ("use math\nfn main() {\n    let math = 3\n    print(math)\n}\n", 3),
    ] {
        std::fs::write(dir.join("a.nyra"), src).unwrap();
        let (ok, json) = check_json(&dir, "a.nyra");
        assert!(!ok, "{src}");
        let errors = json.get("errors").and_then(|e| e.as_array()).unwrap();
        assert!(errors.iter().all(|e| e.get("code").and_then(|c| c.as_str()) == Some("E0206")), "{src}\n{errors:?}");
        for e in errors {
            let msg = e.get("message").and_then(|m| m.as_str()).unwrap();
            assert!(!msg.contains("standard module"), "an error inside the library leaked out: {msg}");
            assert_eq!(e.get("line").and_then(|l| l.as_u64()), Some(line), "{src}");
        }
    }
}

#[test]
fn the_private_helpers_of_a_module_are_not_reachable() {
    let dir = scratch("std-namespace-private");
    for (src, code) in [
        ("use math\nprint(math._fabs(1.0))\n", "E0306"),
        ("use math\nprint(_fabs(1.0))\n", "E0202"),
        ("use math\nprint(exp(1.0))\n", "E0202"),
        ("use math\nprint(math.floor(1.5), floor(1.5))\n", "E0202"),
    ] {
        std::fs::write(dir.join("a.nyra"), src).unwrap();
        let (ok, json) = check_json(&dir, "a.nyra");
        assert!(!ok, "{src}");
        let errors = json.get("errors").and_then(|e| e.as_array()).unwrap();
        assert!(errors.iter().any(|e| e.get("code").and_then(|c| c.as_str()) == Some(code)), "{src}\n{errors:?}");
        assert!(errors.iter().all(|e| e.get("line").and_then(|l| l.as_u64()) == Some(2)), "{src}\n{errors:?}");
    }
}

/// The cases of `tests/messages.txt`: small bad programs.
fn corpus() -> Vec<(String, String)> {
    let text = std::fs::read_to_string("tests/messages.txt").unwrap().replace("\r\n", "\n");
    let mut cases: Vec<(String, String)> = Vec::new();
    let mut in_expected = false;
    for line in text.lines() {
        if let Some(name) = line.strip_prefix("==== ") {
            cases.push((name.trim().to_string(), String::new()));
            in_expected = false;
        } else if line == "----" && !in_expected && !cases.is_empty() {
            in_expected = true;
        } else if let (false, Some(c)) = (in_expected, cases.last_mut()) {
            c.1.push_str(line);
            c.1.push('\n');
        }
    }
    cases
}

/// `\u{XXXX}` in the corpus stands for the character with that code.
fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find("\\u{") {
        out.push_str(&rest[..i]);
        let tail = &rest[i + 3..];
        match tail.find('}').and_then(|j| u32::from_str_radix(&tail[..j], 16).ok().map(|c| (j, c))) {
            Some((j, c)) if char::from_u32(c).is_some() => {
                out.push(char::from_u32(c).unwrap());
                rest = &tail[j + 1..];
            }
            _ => {
                out.push_str("\\u{");
                rest = tail;
            }
        }
    }
    out.push_str(rest);
    out
}

fn assert_positions_in_file(src: &str, json: &Json, what: &str) {
    // the line after the last one is where "end of file" errors point
    let lines = src.matches('\n').count() + 1;
    for key in ["errors", "warnings"] {
        for e in json.get(key).and_then(|e| e.as_array()).unwrap_or(&[]) {
            let line = e.get("line").and_then(|l| l.as_u64()).unwrap_or(0) as usize;
            let col = e.get("col").and_then(|c| c.as_u64()).unwrap_or(0);
            assert!(
                (1..=lines).contains(&line) && col >= 1,
                "{what}: {key} at {line}:{col} is outside the {lines} lines of the file:\n{e:?}"
            );
            for edit in e.get("fix").and_then(|f| f.as_array()).unwrap_or(&[]) {
                let (l, c) = (
                    edit.get("line").and_then(|l| l.as_u64()).unwrap_or(0) as usize,
                    edit.get("col").and_then(|c| c.as_u64()).unwrap_or(0),
                );
                assert!((1..=lines).contains(&l) && c >= 1, "{what}: a fix at {l}:{c} is outside the file");
            }
        }
    }
}

#[test]
fn no_diagnostic_points_outside_the_file() {
    let dir = scratch("std-namespace-positions");
    // every bad program of the test corpus, with all the standard modules imported on top of it
    // (a script's own `use` lines may stand anywhere)
    for (name, src) in corpus() {
        let src = unescape(&src);
        std::fs::write(dir.join("case.nyra"), &src).unwrap();
        let (_, json) = check_json(&dir, "case.nyra");
        assert_positions_in_file(&src, &json, &name);
        let with_uses = format!("{}{src}", uses());
        std::fs::write(dir.join("case.nyra"), &with_uses).unwrap();
        let (_, json) = check_json(&dir, "case.nyra");
        assert_positions_in_file(&with_uses, &json, &format!("{name} (with the standard modules imported)"));
    }
    for entry in std::fs::read_dir("tests/errors").unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "nyra") {
            continue;
        }
        let src = std::fs::read_to_string(&path).unwrap();
        let (_, json) = check_json(Path::new("."), path.to_str().unwrap());
        assert_positions_in_file(&src, &json, &path.display().to_string());
    }
}

#[test]
fn an_example_that_fails_inside_a_standard_function_names_no_line_of_the_library() {
    // an `ex` that runs library code reports the example's own line
    let dir = scratch("std-namespace-example");
    let src = "use math\nfn half(x: float) -> float = math.exp(x) / 2.0\nex half(0.0) == 3.0\nprint(half(4.0))\n";
    std::fs::write(dir.join("a.nyra"), src).unwrap();
    let (ok, json) = check_json(&dir, "a.nyra");
    assert!(!ok);
    assert_positions_in_file(src, &json, "example");
    let first = &json.get("errors").and_then(|e| e.as_array()).unwrap()[0];
    assert_eq!(first.get("code").and_then(|c| c.as_str()), Some("E0250"));
    assert_eq!(first.get("line").and_then(|l| l.as_u64()), Some(3));
}
