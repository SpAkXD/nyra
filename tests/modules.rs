//! Modules of your own: `use ./name` imports `name.nyra` from the folder of the importing file. The
//! same project must print the same on every target, and an error must name the file it is in.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use common::{check_json, missing, nyra, scratch, stderr, stdout};

fn write(dir: &Path, name: &str, text: &str) {
    let path = dir.join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// A fresh folder for one test, holding `files`.
fn project(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = scratch(&format!("modules-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (n, t) in files {
        write(&dir, n, t);
    }
    dir
}

const MAIN: &str = "use ./shapes
use ./util/text_tools
use math

let r = Rect(w: 3, h: 4)
print(shapes.area(r), shapes.describe(r))
print(text_tools.shout(\"hi\"), text_tools.twice(\"ab\"))
print(math.sqrt(16.0))
print(Color.Red, shapes.paint(r, Color.Blue))
for c in Color.all() {
    print(c)
}
ex shapes.area(Rect(w: 2, h: 5)) == 10
";

const SHAPES: &str = "use ./util/text_tools

pub struct Rect {
    w: int
    h: int
}

pub enum Color { Red, Blue }

pub fn area(r: Rect) -> int = r.w * r.h
pub fn describe(r: Rect) -> str = text_tools.shout(\"rect {r.w}x{r.h} area {area(r)}\")
pub fn paint(r: Rect, c: Color) -> str = \"{c} {helper(r)}\"
fn helper(r: Rect) -> int = r.w + r.h
ex area(Rect(w: 1, h: 2)) == 2, helper(Rect(w: 1, h: 2)) == 3
";

const TEXT_TOOLS: &str = "pub fn shout(s: str) -> str = s.upper() + \"!\"
pub fn twice(s: str) -> str = s + s
";

const EXPECTED: &str = "12 RECT 3X4 AREA 12!\nHI! abab\n4\nColor.Red Color.Blue 7\nColor.Red\nColor.Blue\n";

fn available(tool: &str, arg: &str) -> bool {
    Command::new(tool).arg(arg).output().is_ok_and(|o| o.status.success())
}

#[test]
fn a_project_of_several_files_prints_the_same_on_every_target() {
    let dir = project("run", &[("main.nyra", MAIN), ("shapes.nyra", SHAPES), ("util/text_tools.nyra", TEXT_TOOLS)]);
    let mut targets: Vec<(&str, Vec<&str>)> = Vec::new();
    if available("node", "--version") {
        targets.push(("js", vec!["--js"]));
        targets.push(("ts", vec!["--target", "ts"]));
    } else {
        missing("no Node.js for the JavaScript and TypeScript targets");
    }
    if ["gcc", "clang", "cc", "tcc"].iter().any(|c| available(c, "--version")) || std::env::var("NYRA_CC").is_ok() {
        targets.push(("native", vec![]));
    } else {
        missing("no C compiler for the native target");
    }
    if ["python3", "python"].iter().any(|p| available(p, "--version")) {
        targets.push(("py", vec!["--target", "py"]));
    } else {
        missing("no Python");
    }
    if available("rustc", "--version") {
        targets.push(("rs", vec!["--target", "rs"]));
    } else {
        missing("no rustc");
    }
    for (label, flags) in &targets {
        let out = nyra().current_dir(&dir).args(["run", "main.nyra"]).args(flags).output().unwrap();
        assert!(out.status.success(), "[{label}] failed:\n{}", stderr(&out));
        assert_eq!(stdout(&out), EXPECTED, "[{label}]");
    }
    // the examples of every file are checked while compiling
    let out = nyra().current_dir(&dir).args(["test", "main.nyra"]).output().unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stderr(&out).contains("3 examples: 3 passed"), "{}", stderr(&out));
}

/// The codes of `nyra check --json` and the file of the first error.
fn errors(dir: &Path, file: &str) -> (Vec<String>, String, u64) {
    let (ok, json) = check_json(dir, file);
    assert!(!ok);
    let errs = json.get("errors").and_then(|e| e.as_array()).unwrap();
    let codes = errs.iter().map(|e| e.get("code").and_then(|c| c.as_str()).unwrap().to_string()).collect();
    let first = &errs[0];
    let file = first.get("file").and_then(|f| f.as_str()).unwrap().to_string();
    let line = first.get("line").and_then(|l| l.as_u64()).unwrap();
    (codes, file, line)
}

#[test]
fn a_private_function_cannot_be_called_from_another_file() {
    let dir = project(
        "private",
        &[("main.nyra", "use ./lib\nprint(lib.secret())\n"), ("lib.nyra", "fn secret() -> int = 42\npub fn open() -> int = secret()\n")],
    );
    let (codes, file, line) = errors(&dir, "main.nyra");
    assert_eq!(codes, ["E0301"]);
    assert!(file.ends_with("main.nyra") && line == 2, "{file}:{line}");
    // the public one works, and calls the private one inside its own file
    write(&dir, "main.nyra", "use ./lib\nprint(lib.open())\n");
    let out = nyra().current_dir(&dir).args(["run", "main.nyra", "--js"]).output().unwrap();
    assert_eq!(stdout(&out), "42\n", "{}", stderr(&out));
}

#[test]
fn unknown_missing_and_misspelled_things_are_reported_where_they_are() {
    let lib = "pub fn area(w: int) -> int = w * w\n";
    let dir = project("names", &[("main.nyra", "use ./lib\nprint(lib.aera(2))\n"), ("lib.nyra", lib)]);
    let (codes, _, line) = errors(&dir, "main.nyra");
    assert_eq!((codes, line), (vec!["E0306".to_string()], 2));
    // a file that is not there
    write(&dir, "main.nyra", "use ./nothing\nprint(1)\n");
    let (codes, file, line) = errors(&dir, "main.nyra");
    assert_eq!((codes, line), (vec!["E0300".to_string()], 1));
    assert!(file.ends_with("main.nyra"), "{file}");
    // a path that is not allowed
    write(&dir, "main.nyra", "use \"lib.nyra\"\nprint(1)\n");
    assert_eq!(errors(&dir, "main.nyra").0, ["E0305"]);
    // the name of a standard module
    write(&dir, "math.nyra", lib);
    write(&dir, "main.nyra", "use ./math\nprint(1)\n");
    assert_eq!(errors(&dir, "main.nyra").0, ["E0304"]);
    // a module value is not a value
    write(&dir, "main.nyra", "use ./lib\nlet f = lib.area\nprint(1)\n");
    assert_eq!(errors(&dir, "main.nyra").0, ["E0307"]);
}

#[test]
fn files_that_import_each_other_are_an_error() {
    let dir = project(
        "cycle",
        &[
            ("main.nyra", "use ./a\nprint(a.f())\n"),
            ("a.nyra", "use ./b\npub fn f() -> int = b.g()\n"),
            ("b.nyra", "use ./a\npub fn g() -> int = a.f()\n"),
        ],
    );
    let (codes, file, _) = errors(&dir, "main.nyra");
    assert_eq!(codes, ["E0303"]);
    assert!(file.ends_with("b.nyra"), "the import that closes the cycle is in b.nyra, not {file}");
    let out = nyra().current_dir(&dir).args(["check", "main.nyra"]).output().unwrap();
    assert!(stderr(&out).contains("import cycle: a -> b -> a"), "{}", stderr(&out));
}

#[test]
fn an_error_inside_a_module_names_that_file_and_line() {
    let lib = "pub fn double(x: int) -> int = x * 2\npub fn bad() -> int {\n    ret \"no\"\n}\n";
    let dir = project("inside", &[("main.nyra", "use ./lib\nprint(lib.double(2))\n"), ("lib.nyra", lib)]);
    let (codes, file, line) = errors(&dir, "main.nyra");
    assert_eq!(codes, ["E0203"]);
    assert!(file.ends_with("lib.nyra") && line == 3, "{file}:{line}");
    // the human report shows the line of that file
    let out = nyra().current_dir(&dir).args(["check", "main.nyra"]).output().unwrap();
    let err = stderr(&out);
    assert!(err.contains("lib.nyra:3:") && err.contains("ret \"no\""), "{err}");
    // a module has only definitions
    write(&dir, "lib.nyra", "print(\"hello\")\npub fn f() -> int = 1\n");
    let (codes, file, _) = errors(&dir, "main.nyra");
    assert_eq!(codes, ["E0285"]);
    assert!(file.ends_with("lib.nyra"), "{file}");
    // a syntax error inside a module
    write(&dir, "lib.nyra", "pub fn f( -> int = 1\n");
    let (codes, file, _) = errors(&dir, "main.nyra");
    assert_eq!(codes, ["E0101"]);
    assert!(file.ends_with("lib.nyra"), "{file}");
}
