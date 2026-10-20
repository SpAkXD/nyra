//! `nyra serve --mcp tools.nyra`: the `pub fn`s of a file as MCP tools, tested with scripted
//! stdio sessions (one JSON-RPC message per line in, one answer per line out).

mod common;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use common::{nyra, stderr, stdout, Json};

const TOOLS: &str = r#"use fs

struct Stats {
    count: int
    mean: float
    best: str?
}

enum Unit {
    C
    F
}

// Adds two whole numbers.
// b is 1 unless it is given.
pub fn add(a: int, b: int = 1) -> int = a + b

// Count, mean and best name of a list of scores.
pub fn stats(names: [str], scores: [float]) -> Stats {
    var best = -1.0
    var top: str? = none
    var sum = 0.0
    for i, s in scores {
        sum += s
        if s > best {
            best = s
            top = names[i]
        }
    }
    let n = scores.len()
    return Stats(count: n, mean: n > 0 ? sum / float(n) : 0.0, best: top)
}

// Converts a temperature to another unit.
pub fn convert(t: float, to: Unit) -> float {
    return match to {
        Unit.C => (t - 32.0) * 5.0 / 9.0
        Unit.F => t * 9.0 / 5.0 + 32.0
    }
}

// Reads a file of the working folder.
pub fn read(path: str) -> str = fs.read(path)

// Appends a line to a file.
pub fn note(path: str, line: str) {
    fs.append(path, line + "\n")
}

pub fn pick(xs: [int], i: int) -> int {
    print("picking", i)
    return xs[i]
}

pub fn spin() -> int {
    var n = 0
    while true {
        n += 1
    }
    return n
}

pub fn pair(p: (int, str), m: [str: [int]]) -> [(str, int)] {
    var out: [(str, int)] = []
    for k in m {
        out.push((k + p.1, m[k].len() + p.0))
    }
    return out
}

fn helper() -> int = 3
"#;

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("nyra-serve-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("tools.nyra"), TOOLS).unwrap();
    d
}

/// Starts the server in `dir` with `flags`, sends `requests` (one per line), closes its input and
/// gives the answers by id, and what it wrote to stderr.
fn session(dir: &Path, flags: &[&str], requests: &[&str]) -> (Vec<Json>, String, Option<i32>) {
    let mut child = nyra()
        .current_dir(dir)
        .args(["serve", "--mcp", "tools.nyra"])
        .args(flags)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        let mut input = child.stdin.take().unwrap();
        let init = r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}"#;
        let _ = writeln!(input, "{init}");
        let _ = writeln!(input, r#"{{"jsonrpc":"2.0","method":"notifications/initialized"}}"#);
        for r in requests {
            let _ = writeln!(input, "{r}");
        }
    }
    let out = child.wait_with_output().unwrap();
    let answers =
        stdout(&out).lines().filter(|l| !l.trim().is_empty()).map(|l| Json::parse(l).unwrap_or_else(|e| panic!("{e}: {l}"))).collect();
    (answers, stderr(&out), out.status.code())
}

fn call(id: u32, name: &str, args: &str) -> String {
    format!(r#"{{"jsonrpc":"2.0","id":{id},"method":"tools/call","params":{{"name":"{name}","arguments":{args}}}}}"#)
}

fn answer(answers: &[Json], id: u32) -> &Json {
    answers
        .iter()
        .find(|a| a.get("id").and_then(|i| i.as_u64()) == Some(id as u64))
        .unwrap_or_else(|| panic!("no answer to {id}: {answers:?}"))
}

/// The text of the first content of a tool result, and whether it is an error.
fn text(answers: &[Json], id: u32) -> (String, bool) {
    let r = answer(answers, id).get("result").unwrap_or_else(|| panic!("{id} is no result: {:?}", answer(answers, id)));
    let content = r.get("content").and_then(|c| c.as_array()).unwrap();
    (content[0].get("text").and_then(|t| t.as_str()).unwrap().to_string(), r.get("isError").and_then(|e| e.as_bool()).unwrap())
}

/// The error code of a tool error's text (`{"ok":false,"errors":[{"code":...}]}`).
fn code(text: &str) -> String {
    let j = Json::parse(text).unwrap_or_else(|e| panic!("{e}: {text}"));
    let e = &j.get("errors").and_then(|e| e.as_array()).unwrap()[0];
    assert!(e.get("message").and_then(|m| m.as_str()).is_some_and(|m| !m.is_empty()), "{text}");
    e.get("code").and_then(|c| c.as_str()).unwrap().to_string()
}

#[test]
fn a_session_lists_and_calls_the_pub_functions() {
    let d = dir("session");
    let (answers, err, exit) = session(
        &d,
        &[],
        &[
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
            &call(2, "add", r#"{"a":2}"#),
            &call(3, "add", r#"{"a":2,"b":40}"#),
            &call(4, "stats", r#"{"names":["x","y"],"scores":[1.5,3.0]}"#),
            &call(5, "convert", r#"{"t":212,"to":"C"}"#),
            &call(6, "pair", r#"{"p":[1,"!"],"m":{"a":[1,2],"b":[]}}"#),
            &call(7, "pick", r#"{"xs":[10,20],"i":1}"#),
            r#"{"jsonrpc":"2.0","id":8,"method":"ping"}"#,
        ],
    );
    assert_eq!(exit, Some(0), "{err}");
    let init = answer(&answers, 0).get("result").unwrap();
    assert_eq!(init.get("protocolVersion").and_then(|v| v.as_str()), Some("2025-06-18"));
    assert!(init.get("capabilities").and_then(|c| c.get("tools")).is_some());

    // the tools: every `pub fn`, nothing else, with schemas from the types
    let tools = answer(&answers, 1).get("result").and_then(|r| r.get("tools")).and_then(|t| t.as_array()).unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t.get("name").and_then(|n| n.as_str()).unwrap()).collect();
    assert_eq!(names, ["add", "stats", "convert", "read", "note", "pick", "spin", "pair"]);
    let add = &tools[0];
    let desc = add.get("description").and_then(|d| d.as_str()).unwrap();
    assert!(desc.starts_with("Adds two whole numbers.\nb is 1 unless it is given."), "{desc}");
    assert!(desc.contains("`pub fn add(a: int, b: int = 1) -> int`"), "{desc}");
    let input = add.get("inputSchema").unwrap();
    assert_eq!(input.get("type").and_then(|t| t.as_str()), Some("object"));
    let props = input.get("properties").unwrap();
    assert_eq!(props.get("a").and_then(|a| a.get("type")).and_then(|t| t.as_str()), Some("integer"));
    assert_eq!(props.get("b").and_then(|b| b.get("default")).and_then(|d| d.as_u64()), Some(1));
    assert_eq!(input.get("required").and_then(|r| r.as_array()).map(|r| r.len()), Some(1), "b has a default");
    let out = add.get("outputSchema").unwrap();
    assert_eq!(
        out.get("properties").and_then(|p| p.get("result")).and_then(|r| r.get("type")).and_then(|t| t.as_str()),
        Some("integer")
    );
    let stats = tools[1].get("outputSchema").and_then(|o| o.get("properties")).and_then(|p| p.get("result")).unwrap();
    assert_eq!(stats.get("required").and_then(|r| r.as_array()).map(|r| r.len()), Some(3));
    let unit = tools[2].get("inputSchema").and_then(|i| i.get("properties")).and_then(|p| p.get("to")).unwrap();
    assert_eq!(unit.get("enum").and_then(|e| e.as_array()).map(|e| e.len()), Some(2));
    assert!(tools[3].get("description").and_then(|d| d.as_str()).unwrap().contains("Needs the capabilities: fs."));
    assert!(tools[4].get("outputSchema").is_none(), "`note` returns nothing");

    // calls: the result as JSON text and as structured content
    assert_eq!(text(&answers, 2), ("3".to_string(), false));
    assert_eq!(text(&answers, 3), ("42".to_string(), false));
    let (s, _) = text(&answers, 4);
    assert_eq!(s, r#"{"count":2,"mean":2.25,"best":"y"}"#);
    let structured = answer(&answers, 4).get("result").and_then(|r| r.get("structuredContent")).unwrap();
    assert_eq!(structured.get("result").and_then(|r| r.get("best")).and_then(|b| b.as_str()), Some("y"));
    assert_eq!(text(&answers, 5).0, "100");
    assert_eq!(text(&answers, 6).0, r#"[["a!",3],["b!",1]]"#);
    // what a tool prints is a second content
    let content = answer(&answers, 7).get("result").and_then(|r| r.get("content")).and_then(|c| c.as_array()).unwrap();
    assert_eq!(content[0].get("text").and_then(|t| t.as_str()), Some("20"));
    assert_eq!(content[1].get("text").and_then(|t| t.as_str()), Some("picking 1\n"));
    assert!(answer(&answers, 8).get("result").is_some());
}

#[test]
fn errors_are_tool_errors_with_nyra_codes_and_the_server_goes_on() {
    let d = dir("errors");
    std::fs::write(d.join("data.txt"), "from disk").unwrap();
    let (answers, err, _) = session(
        &d,
        &["--fuel", "100000"],
        &[
            &call(1, "pick", r#"{"xs":[1,2],"i":5}"#),
            &call(2, "add", r#"{"a":"x"}"#),
            &call(3, "add", r#"{"b":1}"#),
            &call(4, "add", r#"{"a":1,"c":2}"#),
            &call(5, "stats", r#"{"names":["x"],"scores":[1,"y"]}"#),
            &call(6, "read", r#"{"path":"data.txt"}"#),
            &call(7, "spin", "{}"),
            &call(8, "add", r#"{"a":1}"#),
            &call(9, "helper", "{}"),
            &call(10, "main", "{}"),
        ],
    );
    let (t, is_err) = text(&answers, 1);
    assert!(is_err);
    assert_eq!(code(&t), "E0240", "{t}");
    assert!(t.contains("\"line\":") && t.contains("tools.nyra"), "{t}");
    let (t, _) = text(&answers, 2);
    assert_eq!(code(&t), "E0345");
    assert!(t.contains("argument `a`: expected an int"), "{t}");
    let (t, _) = text(&answers, 3);
    assert!(code(&t) == "E0345" && t.contains("missing argument `a`"), "{t}");
    let (t, _) = text(&answers, 4);
    assert!(code(&t) == "E0345" && t.contains("no parameter `c`"), "{t}");
    let (t, _) = text(&answers, 5);
    assert!(code(&t) == "E0345" && t.contains("argument `scores`: expected a number at scores[1]"), "{t}");
    // no capability: the tool does not run
    let (t, is_err) = text(&answers, 6);
    assert!(is_err && code(&t) == "E0290" && t.contains("--allow fs"), "{t}");
    // a limit stops one call, not the server
    let (t, _) = text(&answers, 7);
    assert_eq!(code(&t), "E0355", "{t}");
    assert_eq!(text(&answers, 8), ("2".to_string(), false));
    // only `pub fn`s are tools
    for id in [9, 10] {
        let e = answer(&answers, id).get("error").unwrap_or_else(|| panic!("{err}"));
        assert!(e.get("message").and_then(|m| m.as_str()).unwrap().contains("Unknown tool"));
    }
}

#[test]
fn capabilities_come_from_allow_or_a_mock() {
    let d = dir("caps");
    std::fs::write(d.join("data.txt"), "from disk").unwrap();
    let (answers, _, _) =
        session(&d, &["--allow", "fs"], &[&call(1, "read", r#"{"path":"data.txt"}"#), &call(2, "read", r#"{"path":"../x"}"#)]);
    assert_eq!(text(&answers, 1), ("\"from disk\"".to_string(), false));
    let (t, _) = text(&answers, 2);
    assert_eq!(code(&t), "E0340", "paths stay below the working folder: {t}");

    // a mock: the tools see the folder, their writes stay in memory and later calls see them
    std::fs::create_dir_all(d.join("fixtures")).unwrap();
    std::fs::write(d.join("fixtures/log.txt"), "one\n").unwrap();
    let (answers, err, _) = session(
        &d,
        &["--mock", "fs=fixtures"],
        &[&call(1, "note", r#"{"path":"log.txt","line":"two"}"#), &call(2, "read", r#"{"path":"log.txt"}"#)],
    );
    assert_eq!(text(&answers, 1), ("done".to_string(), false), "{err}");
    assert_eq!(text(&answers, 2).0, "\"one\\ntwo\\n\"");
    assert_eq!(std::fs::read_to_string(d.join("fixtures/log.txt")).unwrap(), "one\n", "the folder never changes");
}

#[test]
fn list_prints_the_manifest_and_bad_files_are_refused() {
    let d = dir("list");
    let out = nyra().current_dir(&d).args(["serve", "--mcp", "--list", "tools.nyra"]).output().unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    let manifest = Json::parse(stdout(&out).trim()).unwrap();
    assert_eq!(manifest.get("tools").and_then(|t| t.as_array()).map(|t| t.len()), Some(8));
    // `nyra schema --tools` prints the same
    let again = nyra().current_dir(&d).args(["schema", "--tools", "tools.nyra"]).output().unwrap();
    assert_eq!(stdout(&again), stdout(&out));

    // a tool cannot take an `inout` parameter
    std::fs::write(d.join("inout.nyra"), "pub fn bump(inout xs: [int]) {\n    xs.push(1)\n}\n").unwrap();
    let out = nyra().current_dir(&d).args(["serve", "--mcp", "inout.nyra"]).stdin(Stdio::null()).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("cannot be a tool") && stderr(&out).contains("inout xs"), "{}", stderr(&out));
    // a tools file has no statements at the top level
    std::fs::write(d.join("script.nyra"), "print(1)\npub fn f() -> int = 1\n").unwrap();
    let out = nyra().current_dir(&d).args(["serve", "--mcp", "script.nyra"]).stdin(Stdio::null()).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("statements at the top level"), "{}", stderr(&out));
    // a file that does not compile shows its errors
    std::fs::write(d.join("bad.nyra"), "pub fn f() -> int = \"x\"\n").unwrap();
    let out = nyra().current_dir(&d).args(["serve", "--mcp", "--list", "bad.nyra"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("error[E0"), "{}", stderr(&out));
    // usage
    let out = nyra().args(["serve", "tools.nyra"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(nyra().args(["serve", "--help"]).output().unwrap().status.success());
}
