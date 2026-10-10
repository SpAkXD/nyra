//! Self-repair: `nyra check --fix` (and `run`/`build --fix`) apply the fixes that diagnostics carry.
//!
//! - every pair below is a program with typical mistakes and exactly what `--fix` must turn it into;
//!   the result must compile;
//! - mistakes with more than one possible repair get no fix, and leave the file alone;
//! - a program that compiles is never changed;
//! - `--json` carries the same fixes as edits an agent can apply itself.

mod common;

use std::path::Path;

use common::{check_json, nyra, scratch, stderr, stdout, Json};

/// (name, program, what `nyra check --fix` writes back)
const PAIRS: &[(&str, &str, &str)] = &[
    // ---- characters and literals
    ("semicolon at the end", "fn main() {\n    let x = 1;\n    print(x) ;\n}\n", "fn main() {\n    let x = 1\n    print(x)\n}\n"),
    ("semicolon between statements", "fn main() {\n    let a = 1; print(a)\n}\n", "fn main() {\n    let a = 1\n    print(a)\n}\n"),
    ("semicolon alone on a line", "fn main() {\n    print(1)\n    ;\n}\n", "fn main() {\n    print(1)\n}\n"),
    ("hash comment", "fn main() {\n    # say hi, don't shout\n    print(1) # one\n}\n", "fn main() {\n    // say hi, don't shout\n    print(1) // one\n}\n"),
    ("text in single quotes", "fn main() {\n    print('hello world')\n}\n", "fn main() {\n    print(\"hello world\")\n}\n"),
    ("float without digits", "fn main() {\n    print(.5 + 5.)\n}\n", "fn main() {\n    print(0.5 + 5.0)\n}\n"),
    ("math symbols", "fn main() {\n    print(2 × 3 ≤ 7 − 1)\n}\n", "fn main() {\n    print(2 * 3 <= 7 - 1)\n}\n"),
    ("invisible characters", "\u{FEFF}fn main() {\n    let x\u{200B} = 1\n    print(x)\n}\n", "fn main() {\n    let x = 1\n    print(x)\n}\n"),
    ("typographic quotes", "fn main() {\n    print(\u{201C}hi there\u{201D})\n}\n", "fn main() {\n    print(\"hi there\")\n}\n"),
    ("template literal", "fn main() {\n    let n = 2\n    print(`n = ${n}, twice ${n * 2}`)\n}\n", "fn main() {\n    let n = 2\n    print(\"n = {n}, twice {n * 2}\")\n}\n"),
    ("escaped brace", "fn main() {\n    print(\"\\{x\\}\")\n}\n", "fn main() {\n    print(\"{{x}}\")\n}\n"),
    ("hex, separators, exponents", "fn main() {\n    print(0xFF + 1_000)\n    print(2.5e3)\n}\n", "fn main() {\n    print(255 + 1000)\n    print(2500.0)\n}\n"),
    // ---- words and syntax of other languages
    ("return", "fn f(x: int) -> int {\n    return x * 2\n}\nfn main() {\n    print(f(1))\n}\n", "fn f(x: int) -> int {\n    ret x * 2\n}\nfn main() {\n    print(f(1))\n}\n"),
    ("bare return", "fn main() {\n    if true {\n        return\n    }\n}\n", "fn main() {\n    if true {\n        ret\n    }\n}\n"),
    ("elif and, or, not", "fn main() {\n    let x = 3\n    if x > 1 and x < 5 or not true {\n        print(1)\n    } elif x == 0 {\n        print(0)\n    }\n}\n", "fn main() {\n    let x = 3\n    if x > 1 && x < 5 || !true {\n        print(1)\n    } else if x == 0 {\n        print(0)\n    }\n}\n"),
    ("strict equality", "fn main() {\n    let n = 1\n    print(n === 1 || n !== 2)\n}\n", "fn main() {\n    let n = 1\n    print(n == 1 || n != 2)\n}\n"),
    ("assignment in a condition", "fn main() {\n    let x = 1\n    if x = 1 {\n        print(x)\n    }\n}\n", "fn main() {\n    let x = 1\n    if x == 1 {\n        print(x)\n    }\n}\n"),
    ("let mut and const", "fn main() {\n    let mut a = 1\n    const b = 2\n    a += b\n    print(a)\n}\n", "fn main() {\n    var a = 1\n    let b = 2\n    a += b\n    print(a)\n}\n"),
    ("print without parentheses", "fn main() {\n    print \"hi\"\n    print 42\n}\n", "fn main() {\n    print(\"hi\")\n    print(42)\n}\n"),
    ("braces on their own line", "fn main()\n{\n    if true\n    {\n        print(1)\n    } else\n    {\n        print(2)\n    }\n}\n", "fn main() {\n    if true {\n        print(1)\n    } else {\n        print(2)\n    }\n}\n"),
    ("return types", "fn f(): int {\n    ret 1\n}\nfn g() int {\n    ret 2\n}\nfn main() {\n    print(f() + g())\n}\n", "fn f() -> int {\n    ret 1\n}\nfn g() -> int {\n    ret 2\n}\nfn main() {\n    print(f() + g())\n}\n"),
    ("parameters of C and Go", "fn add(int a, b int) -> int = a + b\nfn main() {\n    print(add(1, 2))\n}\n", "fn add(a: int, b: int) -> int = a + b\nfn main() {\n    print(add(1, 2))\n}\n"),
    ("type names", "fn greet(name: string, n: i32) -> String {\n    ret name\n}\nfn main() {\n    print(greet(\"a\", 1))\n}\n", "fn greet(name: str, n: int) -> str {\n    ret name\n}\nfn main() {\n    print(greet(\"a\", 1))\n}\n"),
    ("increment and decrement", "fn main() {\n    var i = 0\n    i++\n    i++\n    i--\n    print(i)\n}\n", "fn main() {\n    var i = 0\n    i += 1\n    i += 1\n    i -= 1\n    print(i)\n}\n"),
    ("range()", "fn main() {\n    let n = 3\n    for i in range(n) {\n        print(i)\n    }\n    for i in range(1, 3) {\n        print(i)\n    }\n}\n", "fn main() {\n    let n = 3\n    for i in 0..n {\n        print(i)\n    }\n    for i in 1..3 {\n        print(i)\n    }\n}\n"),
    ("struct literal in braces", "struct Point { x: int, y: int }\nfn main() {\n    let p = Point { x: 1, y: 2 }\n    let q = Point {\n        x: 3,\n        y: 4,\n    }\n    print(p.x + q.y)\n}\n", "struct Point { x: int, y: int }\nfn main() {\n    let p = Point(x: 1, y: 2)\n    let q = Point(x: 3,\n        y: 4,)\n    print(p.x + q.y)\n}\n"),
    ("function keywords", "def double(x: int) -> int = x * 2\nfunction main() {\n    print(double(2))\n}\n", "fn double(x: int) -> int = x * 2\nfn main() {\n    print(double(2))\n}\n"),
    // ---- names and types
    ("True and False", "fn main() {\n    let t = True\n    print(t && !False)\n}\n", "fn main() {\n    let t = true\n    print(t && !false)\n}\n"),
    ("a name in the wrong case", "struct Point { x: int }\nfn total(p: point) -> int = p.X\nfn main() {\n    let Count = 1\n    print(count + total(point(x: 1)))\n    Print(1)\n}\n", "struct Point { x: int }\nfn total(p: Point) -> int = p.x\nfn main() {\n    let Count = 1\n    print(Count + total(Point(x: 1)))\n    print(1)\n}\n"),
    ("struct name in lowercase", "struct point { x: int }\nfn main() {\n    print(point(x: 1).x)\n}\n", "struct Point { x: int }\nfn main() {\n    print(Point(x: 1).x)\n}\n"),
    ("function without parentheses", "fn limit() -> int = 10\nfn main() {\n    print(limit)\n}\n", "fn limit() -> int = 10\nfn main() {\n    print(limit())\n}\n"),
    ("let that changes", "fn main() {\n    let n = 0\n    n = 1\n    let xs = [1]\n    xs.push(2)\n    print(n + xs.len())\n}\n", "fn main() {\n    var n = 0\n    n = 1\n    var xs = [1]\n    xs.push(2)\n    print(n + xs.len())\n}\n"),
    ("int literals where floats are needed", "fn half(x: float) -> float {\n    ret x / 2\n}\nfn main() {\n    let f: float = 2\n    var t = half(3) * 2\n    t += 1\n    let xs = [1, 2, 2.5]\n    let y = if t > 1.0 { 1 } else { 0.5 }\n    print(t + y + xs[0] + f)\n}\n", "fn half(x: float) -> float {\n    ret x / 2.0\n}\nfn main() {\n    let f: float = 2.0\n    var t = half(3.0) * 2.0\n    t += 1.0\n    let xs = [1.0, 2.0, 2.5]\n    let y = if t > 1.0 { 1.0 } else { 0.5 }\n    print(t + y + xs[0] + f)\n}\n"),
    ("one character in double quotes", "fn main() {\n    let c: char = \"a\"\n    print(c == \"a\")\n}\n", "fn main() {\n    let c: char = 'a'\n    print(c == 'a')\n}\n"),
    ("implicit return", "fn add(a: int, b: int) -> int {\n    a + b\n}\nfn main() {\n    print(add(1, 2))\n}\n", "fn add(a: int, b: int) -> int {\n    ret a + b\n}\nfn main() {\n    print(add(1, 2))\n}\n"),
    ("struct fields by position", "struct Point { x: int, y: int }\nfn main() {\n    let p = Point(1, y: 2)\n    let q = Point(3, 4)\n    print(p.x + q.y)\n}\n", "struct Point { x: int, y: int }\nfn main() {\n    let p = Point(x: 1, y: 2)\n    let q = Point(x: 3, y: 4)\n    print(p.x + q.y)\n}\n"),
    ("named arguments of a function", "fn area(width: int, height: int) -> int = width * height\nfn main() {\n    print(area(width: 3, height: 4))\n}\n", "fn area(width: int, height: int) -> int = width * height\nfn main() {\n    print(area(3, 4))\n}\n"),
    // ---- methods and library functions of other languages
    ("methods of other languages", "fn main() {\n    var xs = [3, 1]\n    xs.append(2)\n    let s = \"Hi\"\n    print(xs.length() + xs.size + s.length)\n    print(s.toUpperCase().startsWith(\"H\") && s[0].isdigit())\n    let n = xs.len\n    print(s.substring(0, 1) + n.to_string())\n}\n", "fn main() {\n    var xs = [3, 1]\n    xs.push(2)\n    let s = \"Hi\"\n    print(xs.len() + xs.len() + s.len())\n    print(s.upper().starts_with(\"H\") && s[0].is_digit())\n    let n = xs.len()\n    print(s.slice(0, 1) + str(n))\n}\n"),
    ("print functions of other languages", "fn main() {\n    console.log(1)\n    println(2)\n    fmt.Println(3)\n    puts(4)\n}\n", "fn main() {\n    print(1)\n    print(2)\n    print(3)\n    print(4)\n}\n"),
    ("len()", "fn main() {\n    let xs = [1, 2]\n    let s = \"abc\"\n    print(len(xs) + len(s))\n}\n", "fn main() {\n    let xs = [1, 2]\n    let s = \"abc\"\n    print(xs.len() + s.len())\n}\n"),
    // ---- several stages at once: characters, then syntax, then types
    ("python and javascript habits together", "fn add(a: int, b: int) -> int {\n    return a + b;\n}\n\nfn main() {\n    let total = add(1, 2);\n    if total > 2 and True {\n        print \"big\"\n    } elif total == 0 {\n        print(total)\n    }\n    let xs = [1, 2, 3]\n    xs.append(4)\n    print(xs.length())\n}\n", "fn add(a: int, b: int) -> int {\n    ret a + b\n}\n\nfn main() {\n    let total = add(1, 2)\n    if total > 2 && true {\n        print(\"big\")\n    } else if total == 0 {\n        print(total)\n    }\n    var xs = [1, 2, 3]\n    xs.push(4)\n    print(xs.len())\n}\n"),
];

/// Mistakes with more than one possible repair (or none that is certain): the error has a hint but
/// no fix, and `--fix` leaves the file alone.
const NO_FIX: &[(&str, &str)] = &[
    ("null", "fn main() {\n    let x = null\n}\n"),
    ("a typo (cout or count?)", "fn main() {\n    let count = 1\n    print(cout)\n}\n"),
    ("go's := (let or var?)", "fn main() {\n    x := 5\n}\n"),
    ("a bit operation", "fn main() {\n    print(6 & 3)\n}\n"),
    ("a C for loop", "fn main() {\n    for (i = 0; i < 3; i++) {\n    }\n}\n"),
    ("number (int or float?)", "fn f(x: number) {\n}\nfn main() {\n}\n"),
    ("braces in single quotes", "fn main() {\n    print('{x}')\n}\n"),
    ("an empty character", "fn main() {\n    print('')\n}\n"),
    ("find with a value", "fn main() {\n    let xs = [1]\n    print(xs.find(1))\n}\n"),
    ("a variable that could be a float", "fn main() {\n    let n = 2\n    print(n + 0.5)\n}\n"),
    ("a parameter that changes", "fn f(n: int) {\n    n = 1\n}\nfn main() {\n}\n"),
    ("brace on the next line after a comment", "fn main() // entry\n{\n}\n"),
];

fn write(dir: &Path, name: &str, text: &str) {
    std::fs::write(dir.join(name), text).unwrap();
}

fn read(dir: &Path, name: &str) -> String {
    std::fs::read_to_string(dir.join(name)).unwrap()
}

#[test]
fn fix_turns_each_mistake_into_the_intended_program() {
    let dir = scratch("fix-pairs");
    let mut failures = Vec::new();
    for (name, bad, fixed) in PAIRS {
        write(&dir, "prog.nyra", bad);
        let out = nyra().current_dir(&dir).args(["check", "--fix", "prog.nyra"]).output().unwrap();
        let got = read(&dir, "prog.nyra");
        if !out.status.success() || got != *fixed {
            failures.push(format!("--- {name}\nexpected:\n{fixed}got:\n{got}stderr:\n{}", stderr(&out)));
            continue;
        }
        assert!(stderr(&out).contains("nyra: fixed "), "{name}: no summary of the edits:\n{}", stderr(&out));
        // the guarantee: what --fix writes compiles
        let (ok, json) = check_json(&dir, "prog.nyra");
        assert!(ok, "{name}: the fixed program does not compile:\n{got}\n{json:?}");
        // and it is stable: there is nothing left to fix
        let again = nyra().current_dir(&dir).args(["check", "--fix", "prog.nyra"]).output().unwrap();
        assert!(again.status.success() && read(&dir, "prog.nyra") == *fixed, "{name}: a second --fix changed the program");
    }
    assert!(failures.is_empty(), "{} of {} pairs failed:\n{}", failures.len(), PAIRS.len(), failures.join("\n"));
}

#[test]
fn mistakes_with_alternatives_get_no_fix() {
    let dir = scratch("fix-none");
    for (name, bad) in NO_FIX {
        write(&dir, "prog.nyra", bad);
        let (ok, json) = check_json(&dir, "prog.nyra");
        assert!(!ok, "{name}: should not compile");
        for e in json.get("errors").and_then(|e| e.as_array()).unwrap() {
            assert!(e.get("fix").is_none(), "{name}: unexpected fix {e:?}");
            assert!(e.get("hint").and_then(|h| h.as_str()).is_some_and(|h| !h.is_empty()), "{name}: no hint");
        }
        let out = nyra().current_dir(&dir).args(["check", "--fix", "prog.nyra"]).output().unwrap();
        assert_eq!(out.status.code(), Some(1), "{name}");
        assert_eq!(read(&dir, "prog.nyra"), *bad, "{name}: --fix changed the file");
    }
}

#[test]
fn errors_without_a_fix_leave_the_file_unchanged() {
    // the `;` has a fix, `null` does not: nothing is written, and the errors are reported as usual
    let dir = scratch("fix-partial");
    let bad = "fn main() {\n    let x = 1;\n    let y = null\n}\n";
    write(&dir, "prog.nyra", bad);
    let out = nyra().current_dir(&dir).args(["check", "--fix", "prog.nyra"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(read(&dir, "prog.nyra"), bad);
    let err = stderr(&out);
    assert!(err.contains("error[E0005]") && err.contains("  = fix: `let x = 1`"), "{err}");
    assert!(err.contains("nyra: --fix did not change prog.nyra: errors without a fix remain"), "{err}");
}

#[test]
fn programs_that_compile_are_never_changed() {
    let dir = scratch("fix-examples");
    let mut seen = 0;
    for entry in std::fs::read_dir("examples").unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "nyra") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        write(&dir, "prog.nyra", &text);
        let out = nyra().current_dir(&dir).args(["check", "--fix", "prog.nyra"]).output().unwrap();
        assert!(out.status.success(), "{}: {}", path.display(), stderr(&out));
        assert!(!stderr(&out).contains("fixed"), "{}: {}", path.display(), stderr(&out));
        assert_eq!(read(&dir, "prog.nyra"), text, "{}: --fix changed a program that compiles", path.display());
        seen += 1;
    }
    assert!(seen >= 10, "only {seen} examples");
}

/// The source with the `fix` edits of a JSON error applied, the way an agent would apply them.
fn apply_json(src: &str, edits: &[Json]) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut starts = vec![0];
    starts.extend(chars.iter().enumerate().filter(|(_, c)| **c == '\n').map(|(i, _)| i + 1));
    let at = |e: &Json, line: &str, col: &str| {
        let l = e.get(line).and_then(|v| v.as_u64()).unwrap() as usize;
        let c = e.get(col).and_then(|v| v.as_u64()).unwrap() as usize;
        starts[l - 1] + c - 1
    };
    let mut ranges: Vec<(usize, usize, String)> = edits
        .iter()
        .map(|e| (at(e, "line", "col"), at(e, "end_line", "end_col"), e.get("text").and_then(|t| t.as_str()).unwrap().to_string()))
        .collect();
    // from the last edit to the first, so the earlier positions stay valid
    ranges.sort_by_key(|a| std::cmp::Reverse(a.0));
    let mut out = chars;
    for (a, b, text) in ranges {
        out.splice(a..b, text.chars());
    }
    out.into_iter().collect()
}

#[test]
fn json_errors_carry_the_fix_as_edits() {
    let dir = scratch("fix-json");
    let bad = "struct Point { x: int, y: int }\nfn main() {\n    let p = Point { x: 1, y: 2 }\n    print(p.x)\n}\n";
    write(&dir, "prog.nyra", bad);
    let (ok, json) = check_json(&dir, "prog.nyra");
    assert!(!ok);
    let errors = json.get("errors").and_then(|e| e.as_array()).unwrap();
    assert_eq!(errors.len(), 1);
    let fix = errors[0].get("fix").and_then(|f| f.as_array()).expect("a `fix` array");
    for edit in fix {
        assert_eq!(edit.keys(), ["line", "col", "end_line", "end_col", "text"]);
    }
    let fixed = apply_json(bad, fix);
    assert_eq!(fixed, "struct Point { x: int, y: int }\nfn main() {\n    let p = Point(x: 1, y: 2)\n    print(p.x)\n}\n");

    // every error of a program gets its own fix, and they can all be applied at once
    let bad = "fn main() {\n    let s = 'hi';\n    print(s × 2)\n}\n";
    write(&dir, "prog.nyra", bad);
    let (_, json) = check_json(&dir, "prog.nyra");
    let edits: Vec<Json> = json
        .get("errors")
        .and_then(|e| e.as_array())
        .unwrap()
        .iter()
        .flat_map(|e| e.get("fix").and_then(|f| f.as_array()).unwrap().to_vec())
        .collect();
    assert_eq!(apply_json(bad, &edits), "fn main() {\n    let s = \"hi\"\n    print(s * 2)\n}\n");
    // an error without a fix has no `fix` key
    write(&dir, "prog.nyra", "fn main() {\n    let x = null\n}\n");
    let (_, json) = check_json(&dir, "prog.nyra");
    assert!(json.get("errors").and_then(|e| e.as_array()).unwrap()[0].get("fix").is_none());
}

#[test]
fn check_json_fix_reports_how_many_errors_were_fixed() {
    let dir = scratch("fix-json-ok");
    write(&dir, "prog.nyra", "fn main() {\n    let x = 1;\n    print(x);\n}\n");
    let out = nyra().current_dir(&dir).args(["check", "--json", "--fix", "prog.nyra"]).output().unwrap();
    assert!(out.status.success());
    let json = Json::parse(stdout(&out).trim()).unwrap();
    assert_eq!(json.get("ok").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(json.get("fixed").and_then(|v| v.as_u64()), Some(2));
    // the summary of the edits goes to stderr, a unified diff without context lines
    assert!(
        stderr(&out).contains("@@ -2,2 +2,2 @@\n-    let x = 1;\n-    print(x);\n+    let x = 1\n+    print(x)\n"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn run_and_build_continue_with_the_fixed_program() {
    let dir = scratch("fix-run");
    let bad = "fn main() {\n    let s = 'fixed';\n    print(s)\n}\n";
    // build --c needs no C compiler: it writes the generated C
    write(&dir, "prog.nyra", bad);
    let out = nyra().current_dir(&dir).args(["build", "--fix", "--c", "-o", "prog.c", "prog.nyra"]).output().unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(read(&dir, "prog.c").contains("fixed"));
    assert_eq!(read(&dir, "prog.nyra"), "fn main() {\n    let s = \"fixed\"\n    print(s)\n}\n");

    let node = std::process::Command::new("node").arg("--version").output().is_ok();
    if node {
        write(&dir, "prog.nyra", bad);
        let out = nyra().current_dir(&dir).args(["run", "--fix", "--js", "prog.nyra"]).output().unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert_eq!(stdout(&out), "fixed\n");
        assert!(stderr(&out).contains("nyra: fixed 2 error(s) in prog.nyra:"), "{}", stderr(&out));
    }
    // without --fix nothing is written
    write(&dir, "prog.nyra", bad);
    let out = nyra().current_dir(&dir).args(["build", "--c", "-o", "prog.c", "prog.nyra"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(read(&dir, "prog.nyra"), bad);
}

#[test]
fn fix_keeps_windows_line_breaks() {
    let dir = scratch("fix-crlf");
    write(&dir, "prog.nyra", "fn main() {\r\n    let a = 1; print(a);\r\n}\r\n");
    let out = nyra().current_dir(&dir).args(["check", "--fix", "prog.nyra"]).output().unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(read(&dir, "prog.nyra"), "fn main() {\r\n    let a = 1\r\n    print(a)\r\n}\r\n");
}

/// The golden corpus of the diagnostics test, through `--fix`: a program is either repaired into one
/// that compiles, or left exactly as it was. Never anything in between.
#[test]
fn every_fix_of_the_corpus_compiles_or_changes_nothing() {
    let text = std::fs::read_to_string("tests/messages.txt").unwrap().replace("\r\n", "\n");
    let mut programs: Vec<String> = Vec::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        if line.starts_with("==== ") {
            programs.extend(current.take());
            current = Some(String::new());
        } else if line == "----" {
            programs.extend(current.take());
        } else if let Some(p) = current.as_mut() {
            p.push_str(line);
            p.push('\n');
        }
    }
    assert!(programs.len() > 100);
    let dir = scratch("fix-corpus");
    let mut repaired = 0;
    for src in &programs {
        // `\u{XXXX}` stands for an invisible character in the corpus
        let src = src.replace("\\u{FEFF}", "\u{FEFF}").replace("\\u{200B}", "\u{200B}");
        write(&dir, "prog.nyra", &src);
        let out = nyra().current_dir(&dir).args(["check", "--fix", "prog.nyra"]).output().unwrap();
        let after = read(&dir, "prog.nyra");
        if out.status.success() {
            assert_ne!(after, src);
            let (ok, json) = check_json(&dir, "prog.nyra");
            assert!(ok, "--fix wrote a program that does not compile:\n{src}\n->\n{after}\n{json:?}");
            repaired += 1;
        } else {
            assert_eq!(after, src, "--fix failed but changed the file");
        }
    }
    assert!(repaired >= 60, "only {repaired} of {} corpus programs were repaired", programs.len());
}
