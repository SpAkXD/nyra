//! The agent runtime of v0.9 (docs/SERVE.md), from the command line:
//!
//! - typed output: `fn main() -> T` prints exactly one JSON document on stdout, valid against the
//!   schema `nyra schema` prints, on every backend that supports it (interpreter, native, JS);
//! - checkpoints: `nyra run --checkpoint`, `nyra resume`: a run paused and resumed prints what it
//!   prints at once, does not redo what it did, and a changed program is refused;
//! - mocks: `--mock fs=DIR`, `input=FILE`, `os.env=KEY=VAL`.
//!
//! (`nyra serve --mcp` is tested in tests/serve.rs; the pause and resume of every example, at the
//! level of the interpreter, in src/checkpoint.rs.)

mod common;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use common::{missing, nyra, stderr, stdout, Json};

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("nyra-agent-runtime-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn run_in(dir: &Path, args: &[&str], input: &[u8]) -> Output {
    let mut child =
        nyra().current_dir(dir).args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let _ = child.stdin.take().unwrap().write_all(input);
    child.wait_with_output().unwrap()
}

fn works(tool: &str) -> bool {
    Command::new(tool).arg("--version").output().is_ok()
}

fn has_cc() -> bool {
    std::env::var("NYRA_CC").is_ok()
        || ["gcc", "clang", "cc", "tcc"].iter().any(|c| works(c))
        || (cfg!(windows) && Path::new(r"C:\msys64\ucrt64\bin\gcc.exe").exists())
}

// ---- a small JSON Schema validator: the parts of draft 2020-12 that `nyra schema` writes ----------

fn check(schema: &Json, v: &Json, root: &Json, at: &str) -> Result<(), String> {
    let fail = |what: &str| Err(format!("{at}: {what} (value {v:?})"));
    match schema {
        Json::Bool(true) => return Ok(()),
        Json::Bool(false) => return fail("no value is allowed"),
        _ => {}
    }
    if let Some(r) = schema.get("$ref").and_then(|r| r.as_str()) {
        let name = r.strip_prefix("#/$defs/").expect("a local reference");
        let target = root.get("$defs").and_then(|d| d.get(name)).unwrap_or_else(|| panic!("no $defs/{name}"));
        return check(target, v, root, at);
    }
    if let Some(any) = schema.get("anyOf").and_then(|a| a.as_array()) {
        if !any.iter().any(|s| check(s, v, root, at).is_ok()) {
            return fail("matches nothing of anyOf");
        }
    }
    if let Some(one) = schema.get("oneOf").and_then(|a| a.as_array()) {
        if one.iter().filter(|s| check(s, v, root, at).is_ok()).count() != 1 {
            return fail("does not match exactly one of oneOf");
        }
    }
    if let Some(options) = schema.get("enum").and_then(|e| e.as_array()) {
        if !options.contains(v) {
            return fail("is not in enum");
        }
    }
    if let Some(t) = schema.get("type") {
        let types: Vec<&str> = match t {
            Json::Str(s) => vec![s.as_str()],
            Json::Arr(ts) => ts.iter().filter_map(|t| t.as_str()).collect(),
            _ => panic!("bad type {t:?}"),
        };
        let ok = types.iter().any(|t| match (*t, v) {
            ("integer", Json::Num(n)) => n.fract() == 0.0,
            ("number", Json::Num(_)) | ("string", Json::Str(_)) | ("boolean", Json::Bool(_)) | ("null", Json::Null) => true,
            ("array", Json::Arr(_)) | ("object", Json::Obj(_)) => true,
            _ => false,
        });
        if !ok {
            return fail(&format!("is not of type {types:?}"));
        }
    }
    if let Json::Str(s) = v {
        let n = s.chars().count() as u64;
        if schema.get("minLength").and_then(|m| m.as_u64()).is_some_and(|m| n < m)
            || schema.get("maxLength").and_then(|m| m.as_u64()).is_some_and(|m| n > m)
        {
            return fail("has the wrong length");
        }
    }
    if let Json::Arr(items) = v {
        let n = items.len() as u64;
        if schema.get("minItems").and_then(|m| m.as_u64()).is_some_and(|m| n < m)
            || schema.get("maxItems").and_then(|m| m.as_u64()).is_some_and(|m| n > m)
        {
            return fail("has the wrong number of items");
        }
        let prefix = schema.get("prefixItems").and_then(|p| p.as_array()).unwrap_or(&[]);
        for (i, item) in items.iter().enumerate() {
            let s = match prefix.get(i) {
                Some(s) => s,
                None => match schema.get("items") {
                    Some(s) => s,
                    None => continue,
                },
            };
            check(s, item, root, &format!("{at}[{i}]"))?;
        }
    }
    if let Json::Obj(fields) = v {
        let props = schema.get("properties");
        for r in schema.get("required").and_then(|r| r.as_array()).unwrap_or(&[]) {
            let name = r.as_str().unwrap();
            if !fields.iter().any(|(k, _)| k == name) {
                return fail(&format!("misses `{name}`"));
            }
        }
        for (k, x) in fields {
            match props.and_then(|p| p.get(k)) {
                Some(s) => check(s, x, root, &format!("{at}.{k}"))?,
                None => match schema.get("additionalProperties") {
                    Some(Json::Bool(false)) => return fail(&format!("has `{k}`, which is not allowed")),
                    Some(s @ Json::Obj(_)) => check(s, x, root, &format!("{at}.{k}"))?,
                    _ => {}
                },
            }
        }
    }
    Ok(())
}

// ---- typed output -----------------------------------------------------------------------------------

const TYPED: &str = r#"struct Point {
    x: int
    y: float
}

enum Shape {
    Circle(float)
    Rect(int, int)
    Empty
}

struct Node {
    v: int
    kids: [Node]
}

struct Out {
    name: str
    letter: char
    pts: [Point]
    shapes: [Shape]
    best: (str, int)?,
    nothing: int?,
    by_name: [str: [int]]
    by_id: [int: bool]
    odd: float
    ok: bool
    tree: Node
}

fn main() -> Out {
    print("log line")
    let zero = 0.0
    let tree = Node(v: 1, kids: [Node(v: 2, kids: []), Node(v: 3, kids: [Node(v: 4, kids: [])])])
    return Out(name: "q\"uote\n", letter: 'é', pts: [Point(x: 1, y: 2.5)], shapes: [Shape.Circle(1.0), Shape.Rect(2, 3), Shape.Empty], best: ("a", 1), nothing: none, by_name: ["a": [1, 2]], by_id: [3: true], odd: zero / zero, ok: true, tree: tree)
}
"#;

#[test]
fn typed_main_prints_one_json_document_that_matches_its_schema_on_every_backend() {
    let d = dir("typed");
    std::fs::write(d.join("typed.nyra"), TYPED).unwrap();
    let out = run_in(&d, &["schema", "typed.nyra"], b"");
    assert!(out.status.success(), "{}", stderr(&out));
    let schema = Json::parse(stdout(&out).trim()).unwrap();
    assert_eq!(schema.get("$schema").and_then(|s| s.as_str()), Some("https://json-schema.org/draft/2020-12/schema"));
    assert!(schema.get("$defs").and_then(|d| d.get("Node")).is_some(), "a struct that contains itself is a $ref");
    let compact = run_in(&d, &["schema", "--compact", "typed.nyra"], b"");
    assert_eq!(stdout(&compact).lines().count(), 1);

    let mut backends: Vec<&[&str]> = vec![&["--interp"], &["--sandbox"], &[]];
    if has_cc() {
        backends.push(&["--native"]);
    } else {
        missing("no C compiler: typed output not checked natively");
    }
    if works("node") {
        backends.push(&["--js"]);
    } else {
        missing("no Node.js: typed output not checked on JavaScript");
    }
    let mut first: Option<String> = None;
    for flags in backends {
        let out = run_in(&d, &[&["run", "typed.nyra"], flags].concat(), b"");
        assert_eq!(out.status.code(), Some(0), "{flags:?}: {}", stderr(&out));
        let text = stdout(&out);
        assert_eq!(text.lines().count(), 1, "{flags:?}: exactly one line on stdout:\n{text}");
        assert!(text.ends_with('\n'));
        assert!(stderr(&out).contains("log line"), "{flags:?}: print goes to stderr: {}", stderr(&out));
        let value = Json::parse(text.trim()).unwrap_or_else(|e| panic!("{flags:?}: not JSON ({e}): {text}"));
        check(&schema, &value, &schema, "$").unwrap_or_else(|e| panic!("{flags:?}: {e}\n{text}"));
        assert_eq!(value.get("odd"), Some(&Json::Null), "NaN is null");
        assert_eq!(value.get("letter").and_then(|l| l.as_str()), Some("é"));
        // every backend writes the same text
        match &first {
            None => first = Some(text),
            Some(f) => assert_eq!(&text, f, "{flags:?}"),
        }
    }
    // the targets without typed output say so
    let out = run_in(&d, &["run", "--py", "typed.nyra"], b"");
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("typed output"), "{}", stderr(&out));
}

#[test]
fn a_script_with_a_typed_main_prints_its_value_after_the_statements() {
    let d = dir("script");
    std::fs::write(
        d.join("s.nyra"),
        "let base = 20\nprint(\"first\")\nfn main() -> [int] {\n    print(\"then\")\n    return [base, base + 1]\n}\n",
    )
    .unwrap();
    let out = run_in(&d, &["run", "--interp", "s.nyra"], b"");
    assert_eq!((stdout(&out).as_str(), out.status.code()), ("[20,21]\n", Some(0)), "{}", stderr(&out));
    assert!(stderr(&out).contains("first\nthen\n"), "{}", stderr(&out));
    // a runtime error prints no value; main without a value has no schema
    std::fs::write(d.join("err.nyra"), "fn main() -> int {\n    let xs = [1]\n    return xs[3]\n}\n").unwrap();
    let out = run_in(&d, &["run", "--interp", "err.nyra"], b"");
    assert_eq!(out.status.code(), Some(101));
    assert_eq!(stdout(&out), "");
    std::fs::write(d.join("plain.nyra"), "print(1)\n").unwrap();
    let out = run_in(&d, &["schema", "plain.nyra"], b"");
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("returns nothing"), "{}", stderr(&out));
    // `main` with a value is not called among the statements (E0211)
    std::fs::write(d.join("called.nyra"), "fn main() -> int {\n    return 1\n}\nmain()\n").unwrap();
    let out = run_in(&d, &["check", "--json", "called.nyra"], b"");
    assert!(stdout(&out).contains("\"code\":\"E0211\""), "{}", stdout(&out));
}

// ---- checkpoints ------------------------------------------------------------------------------------

const LONG: &str = r#"use fs
use input

struct Acc {
    total: int
    seen: [str: int]
}

enum Step {
    Add(int)
    Skip(str)
}

fn fib(n: int) -> int {
    if n < 2 {
        return n
    }
    return fib(n - 1) + fib(n - 2)
}

fn kind(i: int, words: [str]) -> Step = i % 3 == 0 ? Step.Skip(words[i % 2]) : Step.Add(i)

fn main() {
    let words = input.lines()
    var acc = Acc(total: 0, seen: [:])
    for i in 0..2000 {
        match kind(i, words) {
            Step.Add(n) => acc.total += n
            Step.Skip(w) => acc.seen[w] = acc.seen.get(w, 0) + 1
        }
        if i % 250 == 0 {
            print(i, acc.total, fib(10))
            fs.append("log.txt", "{i}\n")
        }
    }
    print(acc)
}
"#;

/// Runs a job with `--checkpoint` in pieces of `fuel` steps until it ends: the output of all the
/// pieces and how many pauses there were.
fn in_pieces(d: &Path, fuel: &str) -> (String, usize) {
    let mut out =
        run_in(d, &["run", "--allow", "fs,input", "--checkpoint", "state.json", "--fuel", fuel, "long.nyra"], b"apple\npear\n");
    let mut text = stdout(&out);
    let mut pauses = 0;
    while out.status.code() == Some(125) {
        pauses += 1;
        assert!(stderr(&out).contains("resume with `nyra resume state.json`"), "{}", stderr(&out));
        assert!(d.join("state.json").exists());
        out = run_in(d, &["resume", "state.json"], b"not read again");
        text.push_str(&stdout(&out));
        assert!(pauses < 1000);
    }
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(!d.join("state.json").exists(), "a run that ended removes its checkpoint");
    (text, pauses)
}

#[test]
fn a_paused_and_resumed_run_prints_what_a_whole_run_prints_and_does_nothing_twice() {
    let d = dir("checkpoint");
    std::fs::write(d.join("long.nyra"), LONG).unwrap();
    let whole = run_in(&d, &["run", "--interp", "--allow", "fs,input", "long.nyra"], b"apple\npear\n");
    assert!(whole.status.success(), "{}", stderr(&whole));
    let log = std::fs::read_to_string(d.join("log.txt")).unwrap();
    assert_eq!(log.lines().count(), 8);
    std::fs::remove_file(d.join("log.txt")).unwrap();
    for fuel in ["3000", "777"] {
        let (text, pauses) = in_pieces(&d, fuel);
        assert!(pauses >= 5, "{pauses} pauses with fuel {fuel}");
        assert_eq!(text, stdout(&whole), "fuel {fuel}");
        assert_eq!(std::fs::read_to_string(d.join("log.txt")).unwrap(), log, "each line of the log is written once");
        std::fs::remove_file(d.join("log.txt")).unwrap();
    }
}

#[test]
fn the_checkpoint_says_where_it_stopped_and_a_changed_program_is_refused() {
    let d = dir("refused");
    std::fs::write(d.join("long.nyra"), LONG).unwrap();
    let out =
        run_in(&d, &["run", "--json", "--allow", "fs,input", "--checkpoint", "state.json", "--fuel", "5000", "long.nyra"], b"a\nb\n");
    assert_eq!(out.status.code(), Some(125));
    let notice = Json::parse(stderr(&out).trim()).unwrap();
    assert_eq!(notice.get("paused").and_then(|p| p.as_bool()), Some(true));
    assert_eq!(notice.get("steps").and_then(|s| s.as_u64()), Some(5000));
    assert_eq!(notice.get("reason").and_then(|r| r.as_str()), Some("fuel"));
    let state = Json::parse(&std::fs::read_to_string(d.join("state.json")).unwrap()).unwrap();
    for key in ["source_hash", "program_hash", "stack", "heap", "host", "run"] {
        assert!(state.get(key).is_some(), "the checkpoint has `{key}`");
    }
    let stack = state.get("stack").and_then(|s| s.as_array()).unwrap();
    assert_eq!(stack[0].get("fn").and_then(|f| f.as_str()), Some("main"));
    assert_eq!(state.get("host").and_then(|h| h.get("input")).and_then(|i| i.as_str()), Some(""), "the input was read");

    // the source changed: no resume
    std::fs::write(d.join("long.nyra"), LONG.replace("fib(10)", "fib(11)")).unwrap();
    let out = run_in(&d, &["resume", "state.json"], b"");
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("changed since the run paused"), "{}", stderr(&out));
    assert!(d.join("state.json").exists(), "a refused checkpoint stays");
    // not a checkpoint
    std::fs::write(d.join("other.json"), "{\"a\":1}").unwrap();
    let out = run_in(&d, &["resume", "other.json"], b"");
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("not a checkpoint"), "{}", stderr(&out));
}

#[test]
fn time_pauses_too_and_a_typed_main_prints_its_value_when_it_ends() {
    let d = dir("time");
    let src = "fn main() -> [int] {\n    var xs: [int] = []\n    var i = 0\n    while i < 3000000 {\n        if i % 1000000 == 0 {\n            xs.push(i)\n        }\n        i += 1\n    }\n    return xs\n}\n";
    std::fs::write(d.join("t.nyra"), src).unwrap();
    let out = run_in(&d, &["run", "--checkpoint", "state.json", "--max-time", "20", "--json", "t.nyra"], b"");
    assert_eq!(out.status.code(), Some(125), "{}", stderr(&out));
    assert!(stderr(&out).contains("\"reason\":\"time\""), "{}", stderr(&out));
    assert_eq!(stdout(&out), "", "no value before the end");
    let out = run_in(&d, &["resume", "state.json", "--max-time", "0", "--fuel", "1000000000"], b"");
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(stdout(&out), "[0,1000000,2000000]\n");
}

// ---- mocks ------------------------------------------------------------------------------------------

#[test]
fn mocks_serve_files_input_and_environment_without_touching_the_machine() {
    let d = dir("mock");
    std::fs::create_dir_all(d.join("fixtures/sub")).unwrap();
    std::fs::write(d.join("fixtures/data.txt"), "fixture").unwrap();
    std::fs::write(d.join("prompt.txt"), "line one\nline two\n").unwrap();
    let src = "use fs\nuse input\nuse os\n\nfn main() {\n    print(fs.read(\"data.txt\"))\n    fs.write(\"out.txt\", \"written\")\n    fs.append(\"data.txt\", \" +more\")\n    fs.mkdir(\"new\")\n    fs.remove(\"sub\")\n    print(fs.read(\"data.txt\"), fs.read(\"out.txt\"), fs.list(\".\"))\n    print(input.lines())\n    print(os.env(\"TOKEN\"), os.has_env(\"PATH\"))\n    print(fs.read(\"/etc/passwd\"))\n}\n";
    std::fs::write(d.join("m.nyra"), src).unwrap();
    let out = run_in(
        &d,
        &["run", "m.nyra", "--mock", "fs=fixtures", "--mock", "input=prompt.txt", "--mock", "os.env=TOKEN=abc"],
        b"ignored",
    );
    assert_eq!(
        stdout(&out),
        "fixture\nfixture +more written [\"data.txt\", \"new\", \"out.txt\"]\n[\"line one\", \"line two\"]\nabc false\n",
        "{}",
        stderr(&out)
    );
    assert_eq!(out.status.code(), Some(101));
    assert!(stderr(&out).contains("runtime error[E0340]") && stderr(&out).contains("outside the mock folder"), "{}", stderr(&out));
    // nothing changed on the disk
    assert_eq!(std::fs::read_to_string(d.join("fixtures/data.txt")).unwrap(), "fixture");
    assert!(!d.join("fixtures/out.txt").exists() && !d.join("out.txt").exists() && !d.join("fixtures/new").exists());
    assert!(d.join("fixtures/sub").is_dir());

    // a mock grants its capability only: the others need --allow (E0290)
    let out = run_in(&d, &["check", "--json", "m.nyra", "--mock", "fs=fixtures"], b"");
    assert!(!out.status.success());
    let text = stdout(&out);
    assert!(text.contains("\"code\":\"E0290\"") && text.contains("module `input`") && !text.contains("module `fs`"), "{text}");
    // a bad mock is a usage error
    let out = run_in(&d, &["run", "m.nyra", "--mock", "net=fixtures"], b"");
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("no network module"), "{}", stderr(&out));
}

#[test]
fn a_checkpoint_keeps_the_writes_of_a_mock() {
    let d = dir("mock-checkpoint");
    std::fs::create_dir_all(d.join("fx")).unwrap();
    let src = "use fs\n\nfn main() {\n    for i in 0..200 {\n        fs.append(\"log.txt\", \"{i},\")\n    }\n    print(fs.read(\"log.txt\").split(\",\").len())\n}\n";
    std::fs::write(d.join("w.nyra"), src).unwrap();
    let mut out = run_in(&d, &["run", "w.nyra", "--mock", "fs=fx", "--checkpoint", "state.json", "--fuel", "300"], b"");
    let mut pauses = 0;
    while out.status.code() == Some(125) {
        pauses += 1;
        out = run_in(&d, &["resume", "state.json"], b"");
    }
    assert!(pauses >= 2, "{pauses}");
    assert_eq!(stdout(&out), "201\n", "{}", stderr(&out));
    assert!(!d.join("fx/log.txt").exists());
}
