//! `nyra mcp`: drives a whole session over stdio and checks every reply.

mod common;

use std::io::Write;
use std::process::{Command, Stdio};

use common::{nyra, Json};

/// Sends `requests` (one JSON message per line), closes stdin and returns the replies by id.
fn session(requests: &[String]) -> Vec<Json> {
    session_with(requests, &[])
}

/// `session`, with environment variables set for the server.
fn session_with(requests: &[String], env: &[(&str, &str)]) -> Vec<Json> {
    let mut child = nyra()
        .arg("mcp")
        .env("NYRA_AUTO_WALL_MS", "600000")
        .envs(env.iter().copied())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        let mut stdin = child.stdin.take().unwrap();
        for r in requests {
            writeln!(stdin, "{r}").unwrap();
        }
    }
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "nyra mcp failed:\n{}", common::stderr(&out));
    common::stdout(&out)
        .lines()
        .map(|l| Json::parse(l).unwrap_or_else(|e| panic!("a reply is not one line of JSON ({e}):\n{l}")))
        .collect()
}

fn esc(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out += "\\\"",
            '\\' => out += "\\\\",
            '\n' => out += "\\n",
            c => out.push(c),
        }
    }
    out + "\""
}

fn request(id: u64, method: &str, params: &str) -> String {
    format!(r#"{{"jsonrpc":"2.0","id":{id},"method":"{method}","params":{params}}}"#)
}

fn call(id: u64, tool: &str, args: &str) -> String {
    request(id, "tools/call", &format!(r#"{{"name":"{tool}","arguments":{args}}}"#))
}

fn reply(replies: &[Json], id: u64) -> &Json {
    replies.iter().find(|r| r.get("id").and_then(Json::as_u64) == Some(id)).unwrap_or_else(|| panic!("no reply with id {id}"))
}

fn result(replies: &[Json], id: u64) -> &Json {
    let r = reply(replies, id);
    assert_eq!(r.get("jsonrpc").and_then(Json::as_str), Some("2.0"));
    r.get("result").unwrap_or_else(|| panic!("reply {id} is an error: {r:?}"))
}

fn error_code(replies: &[Json], id: u64) -> f64 {
    match reply(replies, id).get("error").and_then(|e| e.get("code")) {
        Some(Json::Num(n)) => *n,
        other => panic!("reply {id} has no error code: {other:?}"),
    }
}

/// A tool result: (isError, the text content).
fn tool_text(replies: &[Json], id: u64) -> (bool, String) {
    let r = result(replies, id);
    let content = r.get("content").and_then(Json::as_array).unwrap();
    assert_eq!(content.len(), 1);
    assert_eq!(content[0].get("type").and_then(Json::as_str), Some("text"));
    let text = content[0].get("text").and_then(Json::as_str).unwrap().to_string();
    (r.get("isError").and_then(Json::as_bool).unwrap(), text)
}

/// A tool result whose text is JSON.
fn tool_json(replies: &[Json], id: u64) -> (bool, Json) {
    let (is_error, text) = tool_text(replies, id);
    let json = Json::parse(&text).unwrap_or_else(|e| panic!("tool result {id} is not JSON ({e}):\n{text}"));
    (is_error, json)
}

fn node_available() -> bool {
    let ok = Command::new("node").arg("--version").output().is_ok();
    if !ok {
        common::missing("no Node.js for the JavaScript backend");
    }
    ok
}

const HELLO: &str = "fn main() {\n    for i in 0..3 {\n        print(\"hi {i}\")\n    }\n}\n";
const TYPO: &str = "fn main() {\n    let count = 1\n    print(cout)\n}\n";
const OUT_OF_BOUNDS: &str = "fn main() {\n    let xs = [1, 2]\n    print(\"before\")\n    print(xs[5])\n}\n";
const WRONG_SQ: &str = "fn sq(x: int) -> int = x + x   ex sq(2) == 4, sq(3) == 9
fn main() {
    print(sq(5))
}
";
const FOREVER: &str = "fn main() {\n    var i = 0\n    while true {\n        i += 1\n    }\n}\n";

#[test]
fn a_whole_session() {
    let requests = vec![
        request(1, "initialize", r#"{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"1"}}"#),
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#.to_string(),
        request(2, "tools/list", "{}"),
        call(3, "nyra_check", &format!(r#"{{"code":{}}}"#, esc(TYPO))),
        call(4, "nyra_check", &format!(r#"{{"code":{}}}"#, esc(HELLO))),
        call(5, "nyra_run", &format!(r#"{{"code":{}}}"#, esc(HELLO))),
        call(6, "nyra_run", &format!(r#"{{"code":{},"backend":"js"}}"#, esc(HELLO))),
        call(7, "nyra_run", &format!(r#"{{"code":{},"backend":"js"}}"#, esc(OUT_OF_BOUNDS))),
        call(8, "nyra_run", &format!(r#"{{"code":{}}}"#, esc(TYPO))),
        call(9, "nyra_explain", r#"{"code":"E0201"}"#),
        call(10, "nyra_explain", r#"{"code":"E0299"}"#),
        call(11, "nyra_spec", "{}"),
        call(12, "nyra_build", &format!(r#"{{"code":{},"target":"js"}}"#, esc(HELLO))),
        call(13, "nyra_run", &format!(r#"{{"code":{},"backend":"js","timeout_ms":300}}"#, esc(FOREVER))),
        call(14, "nyra_check", "{}"),
        call(15, "no_such_tool", "{}"),
        request(16, "resources/list", "{}"),
        request(17, "resources/read", r#"{"uri":"nyra://spec"}"#),
        request(18, "resources/read", r#"{"uri":"nyra://errors/E0240"}"#),
        request(19, "resources/read", r#"{"uri":"nyra://nothing"}"#),
        request(20, "no/such/method", "{}"),
        "{not json".to_string(),
        request(21, "ping", "{}"),
        call(22, "nyra_run", &format!(r#"{{"code":{},"backend":"native"}}"#, esc(OUT_OF_BOUNDS))),
        call(23, "nyra_test", &format!(r#"{{"code":{}}}"#, esc(WRONG_SQ))),
        call(24, "nyra_check", &format!(r#"{{"code":{}}}"#, esc(WRONG_SQ))),
        call(25, "nyra_test", &format!(r#"{{"code":{}}}"#, esc(TYPO))),
        call(26, "nyra_spec", r#"{"full":true}"#),
        call(27, "nyra_spec", r#"{"part":"guide"}"#),
        call(28, "nyra_spec", r#"{"full":"yes"}"#),
        call(29, "nyra_spec", r#"{"part":"spec","full":false}"#),
        request(30, "resources/read", r#"{"uri":"nyra://card"}"#),
    ];
    let replies = session(&requests);
    // one reply per request: none for the notification, one (id null) for the parse error
    assert_eq!(replies.len(), requests.len() - 1);

    // initialize
    let init = result(&replies, 1);
    assert_eq!(init.get("protocolVersion").and_then(Json::as_str), Some("2025-06-18"));
    let caps = init.get("capabilities").unwrap();
    assert!(caps.get("tools").is_some() && caps.get("resources").is_some());
    assert_eq!(init.get("serverInfo").and_then(|s| s.get("name")).and_then(Json::as_str), Some("nyra"));
    assert!(init.get("instructions").and_then(Json::as_str).unwrap().contains("nyra_spec"));

    // tools/list
    let tools = result(&replies, 2).get("tools").and_then(Json::as_array).unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t.get("name").and_then(Json::as_str).unwrap()).collect();
    for want in ["nyra_check", "nyra_test", "nyra_run", "nyra_explain", "nyra_spec", "nyra_build"] {
        assert!(names.contains(&want), "tools/list lacks {want}: {names:?}");
    }
    for t in tools {
        assert!(t.get("description").and_then(Json::as_str).is_some_and(|d| !d.is_empty()));
        assert_eq!(t.get("inputSchema").and_then(|s| s.get("type")).and_then(Json::as_str), Some("object"));
    }

    // nyra_check: the same JSON as `nyra check --json`
    let (is_error, check) = tool_json(&replies, 3);
    assert!(!is_error);
    assert_eq!(check.get("ok").and_then(Json::as_bool), Some(false));
    let e = &check.get("errors").and_then(Json::as_array).unwrap()[0];
    assert_eq!(e.keys(), ["code", "message", "file", "line", "col", "hint"]);
    assert_eq!(e.get("code").and_then(Json::as_str), Some("E0201"));
    assert_eq!(e.get("line").and_then(Json::as_u64), Some(3));
    assert_eq!(e.get("col").and_then(Json::as_u64), Some(11));
    assert!(e.get("hint").and_then(Json::as_str).unwrap().contains("count"));
    assert_eq!(tool_text(&replies, 4), (false, r#"{"ok":true,"errors":[]}"#.to_string()));

    // nyra_test: every example, with the values of a false one; nyra_check reports the same error
    let (is_error, test) = tool_json(&replies, 23);
    assert!(!is_error);
    assert_eq!(test.get("ok").and_then(Json::as_bool), Some(false));
    assert_eq!(test.get("examples").and_then(Json::as_u64), Some(2));
    assert_eq!(test.get("passed").and_then(Json::as_u64), Some(1));
    assert_eq!(test.get("failed").and_then(Json::as_u64), Some(1));
    let e = &test.get("errors").and_then(Json::as_array).unwrap()[0];
    assert_eq!(e.get("code").and_then(Json::as_str), Some("E0250"));
    assert_eq!(e.get("actual").and_then(Json::as_str), Some("6"));
    assert_eq!(e.get("expected").and_then(Json::as_str), Some("9"));
    assert_eq!(e.get("col").and_then(Json::as_u64), Some(47));
    let (_, checked) = tool_json(&replies, 24);
    assert_eq!(checked.get("errors"), test.get("errors"));
    assert_eq!(tool_json(&replies, 25).1, check);

    // nyra_run, native (unless this machine has no C compiler) and JavaScript
    let (is_error, native) = tool_json(&replies, 5);
    if is_error {
        let msg = native.get("error").and_then(Json::as_str).unwrap();
        assert!(msg.contains("no C compiler"), "native run failed: {msg}");
        eprintln!("note: no C compiler, the native backend was not tested");
    } else {
        assert_eq!(native.get("ok").and_then(Json::as_bool), Some(true));
        assert_eq!(native.get("exit").and_then(Json::as_u64), Some(0));
        assert_eq!(native.get("stdout").and_then(Json::as_str), Some("hi 0\nhi 1\nhi 2\n"));
        assert!(native.get("ms").and_then(|m| m.get("run")).is_some());

        // a runtime error comes back structured
        let (is_error, oob) = tool_json(&replies, 22);
        assert!(!is_error);
        assert_eq!(oob.get("exit").and_then(Json::as_u64), Some(101));
        assert_eq!(oob.get("stdout").and_then(Json::as_str), Some("before\n"));
        let e = &oob.get("errors").and_then(Json::as_array).unwrap()[0];
        assert_eq!(e.get("code").and_then(Json::as_str), Some("E0240"));
    }
    if node_available() {
        let (is_error, js) = tool_json(&replies, 6);
        assert!(!is_error, "{js:?}");
        assert_eq!(js.get("ok").and_then(Json::as_bool), Some(true));
        assert_eq!(js.get("stdout").and_then(Json::as_str), Some("hi 0\nhi 1\nhi 2\n"));

        let (is_error, oob) = tool_json(&replies, 7);
        assert!(!is_error);
        assert_eq!(oob.get("ok").and_then(Json::as_bool), Some(false));
        assert_eq!(oob.get("exit").and_then(Json::as_u64), Some(101));
        assert_eq!(oob.get("stdout").and_then(Json::as_str), Some("before\n"));
        let e = &oob.get("errors").and_then(Json::as_array).unwrap()[0];
        assert_eq!(e.get("code").and_then(Json::as_str), Some("E0240"));
        assert_eq!(e.get("line").and_then(Json::as_u64), Some(4));
        assert!(oob.get("stderr").is_none(), "the runtime error is not repeated in stderr: {oob:?}");

        let (_, forever) = tool_json(&replies, 13);
        assert_eq!(forever.get("timeout").and_then(Json::as_bool), Some(true));
        assert_eq!(forever.get("exit"), Some(&Json::Null));
    } else {
        eprintln!("note: no Node.js, the JS backend was not tested");
    }

    // a program with compile errors is not run: the reply is the check result
    let (is_error, compile_err) = tool_json(&replies, 8);
    assert!(!is_error);
    assert_eq!(compile_err, check);

    // nyra_explain
    let (is_error, entry) = tool_json(&replies, 9);
    assert!(!is_error);
    assert_eq!(entry.get("code").and_then(Json::as_str), Some("E0201"));
    assert_eq!(entry.get("title").and_then(Json::as_str), Some("undefined variable"));
    assert!(entry.get("fixed").and_then(Json::as_str).is_some_and(|f| f.contains("fn main")));
    let (is_error, unknown) = tool_json(&replies, 10);
    assert!(is_error);
    assert_eq!(unknown.get("ok").and_then(Json::as_bool), Some(false));
    assert!(unknown.get("did_you_mean").and_then(Json::as_str).is_some());

    // nyra_spec and nyra_build
    let (is_error, spec) = tool_text(&replies, 11);
    assert!(!is_error);
    // by default the compact agent card (the file without its metadata comment), not the whole spec
    let card_file = std::fs::read_to_string("docs/AGENT_CARD.md").unwrap().replace("\r\n", "\n");
    let card = card_file.split_once("-->\n").expect("the card starts with a metadata comment").1;
    assert!(card_file.starts_with("<!--") && !card.contains("<!--") && card.starts_with("# Nyra v"));
    assert_eq!(spec, card);
    let full_spec = std::fs::read_to_string("docs/SPEC.md").unwrap();
    assert!(spec.len() * 4 < full_spec.len(), "the card is the compact one: {} bytes against {}", spec.len(), full_spec.len());
    let (is_error, full) = tool_text(&replies, 26);
    assert!(!is_error);
    assert_eq!(full, full_spec);
    let (is_error, guide) = tool_text(&replies, 27);
    assert!(!is_error);
    assert_eq!(guide, std::fs::read_to_string("docs/AI_GUIDE.md").unwrap());
    let (is_error, bad) = tool_json(&replies, 28);
    assert!(is_error);
    assert!(bad.get("error").and_then(Json::as_str).unwrap().contains("full"));
    assert_eq!(tool_text(&replies, 29), (false, card.to_string()));
    let (is_error, built) = tool_json(&replies, 12);
    assert!(!is_error);
    assert_eq!(built.get("target").and_then(Json::as_str), Some("js"));
    assert!(built.get("source").and_then(Json::as_str).unwrap().contains("generated by nyra"));

    // bad arguments are tool errors; an unknown tool is a protocol error
    let (is_error, missing) = tool_json(&replies, 14);
    assert!(is_error);
    assert!(missing.get("error").and_then(Json::as_str).unwrap().contains("code"));
    assert_eq!(error_code(&replies, 15), -32602.0);

    // resources
    let resources = result(&replies, 16).get("resources").and_then(Json::as_array).unwrap();
    let uris: Vec<&str> = resources.iter().map(|r| r.get("uri").and_then(Json::as_str).unwrap()).collect();
    assert_eq!(uris, ["nyra://card", "nyra://spec", "nyra://guide", "nyra://errors"]);
    let contents = result(&replies, 17).get("contents").and_then(Json::as_array).unwrap();
    assert_eq!(contents[0].get("mimeType").and_then(Json::as_str), Some("text/markdown"));
    assert_eq!(contents[0].get("text").and_then(Json::as_str), Some(full_spec.as_str()));
    let contents = result(&replies, 30).get("contents").and_then(Json::as_array).unwrap();
    assert_eq!(contents[0].get("text").and_then(Json::as_str), Some(card));
    let contents = result(&replies, 18).get("contents").and_then(Json::as_array).unwrap();
    let entry = Json::parse(contents[0].get("text").and_then(Json::as_str).unwrap()).unwrap();
    assert_eq!(entry.get("code").and_then(Json::as_str), Some("E0240"));
    assert_eq!(error_code(&replies, 19), -32002.0);

    // protocol errors
    assert_eq!(error_code(&replies, 20), -32601.0);
    let parse_error = replies.iter().find(|r| r.get("id") == Some(&Json::Null)).expect("a reply to the bad JSON");
    assert_eq!(parse_error.get("error").and_then(|e| e.get("code")), Some(&Json::Num(-32700.0)));
    assert_eq!(result(&replies, 21), &Json::Obj(Vec::new()));
}

#[test]
fn an_unknown_protocol_version_gets_the_newest() {
    let replies = session(&[request(1, "initialize", r#"{"protocolVersion":"1999-01-01","capabilities":{}}"#)]);
    let version = result(&replies, 1).get("protocolVersion").and_then(Json::as_str).unwrap().to_string();
    assert!(version.as_str() > "2025-01-01", "{version}");
}

#[test]
fn stdout_carries_only_protocol_messages() {
    // `nyra mcp --help` is for people; the server itself never prints anything but replies
    let out = nyra().args(["mcp", "--help"]).output().unwrap();
    assert!(out.status.success());
    assert!(common::stdout(&out).contains("claude mcp add nyra -- nyra mcp"));
    let replies = session(&[]);
    assert!(replies.is_empty());
}

#[test]
fn deeply_nested_code_gets_an_error_and_the_server_keeps_going() {
    let init = request(1, "initialize", r#"{"protocolVersion":"2025-06-18","capabilities":{}}"#);
    let parens = format!("fn main() {{\n    print({}1{})\n}}\n", "(".repeat(6000), ")".repeat(6000));
    let chain = format!("fn main() {{\n    print({})\n}}\n", vec!["1"; 10000].join(" + "));
    let calls = format!("fn main() {{\n    print({}1{})\n}}\n", "abs(".repeat(5000), ")".repeat(5000));
    let methods = format!("fn main() {{\n    print(\"a\"{})\n}}\n", ".trim()".repeat(5000));
    let arrays = format!("fn main() {{\n    print({}1{})\n}}\n", "[".repeat(5000), "]".repeat(5000));
    let unary = format!("fn main() {{\n    print({}1)\n    print({}true)\n}}\n", "-".repeat(5000), "!".repeat(5000));
    let blocks = format!("fn main() {{\n{}print(1)\n{}}}\n", "if true {\n".repeat(3000), "}\n".repeat(3000));
    let deep = [&parens, &chain, &calls, &methods, &arrays, &unary, &blocks];
    let mut requests = vec![init, call(2, "nyra_check", &format!(r#"{{"code":{}}}"#, esc(HELLO)))];
    for (k, code) in deep.iter().enumerate() {
        requests.push(call(10 + k as u64, "nyra_check", &format!(r#"{{"code":{}}}"#, esc(code))));
    }
    requests.push(call(3, "nyra_check", &format!(r#"{{"code":{}}}"#, esc(HELLO))));
    let replies = session(&requests);
    assert_eq!(tool_json(&replies, 2).1.get("ok").and_then(Json::as_bool), Some(true));
    for k in 0..deep.len() as u64 {
        let (_, json) = tool_json(&replies, 10 + k);
        let errors = json.get("errors").and_then(Json::as_array).unwrap_or_else(|| panic!("request {}: {json:?}", 10 + k));
        assert_eq!(errors.len(), 1, "request {}: {json:?}", 10 + k);
        assert_eq!(errors[0].get("code").and_then(Json::as_str), Some("E0103"), "request {}: {json:?}", 10 + k);
    }
    assert_eq!(tool_json(&replies, 3).1.get("ok").and_then(Json::as_bool), Some(true));
}

#[test]
fn a_program_that_eats_memory_stops_and_the_server_keeps_going() {
    // macOS does not enforce memory rlimits: the program would take what the machine has
    if cfg!(target_os = "macos") {
        return;
    }
    let hog = "fn main() {\n    var xs = [1, 2, 3, 4]\n    while true {\n        xs += xs\n    }\n}\n";
    let init = request(1, "initialize", r#"{"protocolVersion":"2025-06-18","capabilities":{}}"#);
    let mut requests = vec![init, call(2, "nyra_run", &format!(r#"{{"code":{}}}"#, esc(hog)))];
    if node_available() {
        requests.push(call(3, "nyra_run", &format!(r#"{{"code":{},"backend":"js"}}"#, esc(hog))));
    }
    requests.push(call(4, "nyra_run", &format!(r#"{{"code":{}}}"#, esc(HELLO))));
    let replies = session(&requests);
    let mut ids = vec![2];
    if node_available() {
        ids.push(3);
    }
    for id in ids {
        let (_, json) = tool_json(&replies, id);
        // without a C compiler there is nothing to run natively
        if json.get("error").is_some() {
            continue;
        }
        assert_eq!(json.get("ok").and_then(Json::as_bool), Some(false), "{json:?}");
        let errors = json.get("errors").and_then(Json::as_array).unwrap_or_else(|| panic!("{json:?}"));
        assert_eq!(errors[0].get("code").and_then(Json::as_str), Some("E0249"), "{json:?}");
    }
    let (_, json) = tool_json(&replies, 4);
    if json.get("error").is_none() {
        assert_eq!(json.get("stdout").and_then(Json::as_str), Some("hi 0\nhi 1\nhi 2\n"));
    }
}

// ---- capabilities and the sandbox ---------------------------------------------------------------------------

const READS_A_FILE: &str = "use fs\n\nfn main() {\n    print(fs.exists(\"Cargo.toml\"))\n}\n";
const READS_STDIN: &str = "use input\n\nfn main() {\n    print(input.line().upper())\n}\n";
const READS_ARGS: &str = "use os\n\nfn main() {\n    print(os.args())\n}\n";
const FILL_MEMORY: &str = "fn main() {\n    var xs: [int] = []\n    while true {\n        xs.push(1)\n    }\n}\n";
const CHATTY: &str = "fn main() {\n    for i in 0..100000 {\n        print(\"line {i}\")\n    }\n}\n";

#[test]
fn capabilities_and_the_sandbox() {
    let requests = vec![
        // 2-3: the default grants standard input only; `allow` names the rest
        call(2, "nyra_run", &format!(r#"{{"code":{}}}"#, esc(READS_A_FILE))),
        call(3, "nyra_check", &format!(r#"{{"code":{}}}"#, esc(READS_A_FILE))),
        call(4, "nyra_check", &format!(r#"{{"code":{},"allow":["fs"]}}"#, esc(READS_A_FILE))),
        call(5, "nyra_test", &format!(r#"{{"code":{}}}"#, esc(READS_A_FILE))),
        call(6, "nyra_run", &format!(r#"{{"code":{},"allow":["files"]}}"#, esc(READS_A_FILE))),
        // 7-13: the sandbox runs the program in the interpreter, with the limits
        call(7, "nyra_run", &format!(r#"{{"code":{},"sandbox":true,"allow":["fs"]}}"#, esc(READS_A_FILE))),
        call(8, "nyra_run", &format!(r#"{{"code":{},"sandbox":true,"stdin":"hello\nworld\n"}}"#, esc(READS_STDIN))),
        call(9, "nyra_run", &format!(r#"{{"code":{},"sandbox":true,"fuel":5000}}"#, esc(FOREVER))),
        call(10, "nyra_run", &format!(r#"{{"code":{},"sandbox":true,"max_memory":4194304}}"#, esc(FILL_MEMORY))),
        call(11, "nyra_run", &format!(r#"{{"code":{},"sandbox":true}}"#, esc(CHATTY))),
        call(12, "nyra_run", &format!(r#"{{"code":{},"sandbox":true,"allow":["os"],"args":["a","b c"]}}"#, esc(READS_ARGS))),
        call(13, "nyra_run", &format!(r#"{{"code":{},"sandbox":true}}"#, esc(OUT_OF_BOUNDS))),
        call(14, "nyra_run", &format!(r#"{{"code":{},"sandbox":true}}"#, esc(TYPO))),
        call(15, "nyra_run", &format!(r#"{{"code":{},"sandbox":true,"fuel":0}}"#, esc(HELLO))),
        call(16, "nyra_run", &format!(r#"{{"code":{},"sandbox":true,"timeout_ms":200,"fuel":100000000000}}"#, esc(FOREVER))),
    ];
    let replies = session(&requests);

    // a compile error, as from nyra_check: E0290 names the module, the capability and what to pass
    let (is_error, denied) = tool_json(&replies, 2);
    assert!(!is_error);
    assert_eq!(denied.get("ok").and_then(Json::as_bool), Some(false));
    let e = &denied.get("errors").and_then(Json::as_array).unwrap()[0];
    assert_eq!(e.get("code").and_then(Json::as_str), Some("E0290"));
    assert!(e.get("message").and_then(Json::as_str).unwrap().contains("module `fs` needs the capability `fs`"));
    assert!(e.get("hint").and_then(Json::as_str).unwrap().contains("allow: [\"fs\"]"), "{e:?}");
    // nyra_check and nyra_test agree with nyra_run
    assert_eq!(tool_json(&replies, 3).1, denied);
    assert_eq!(tool_text(&replies, 4), (false, r#"{"ok":true,"errors":[]}"#.to_string()));
    assert_eq!(tool_json(&replies, 5).1, denied);
    // an unknown capability is a tool error that says what exists
    let (is_error, bad) = tool_json(&replies, 6);
    assert!(is_error);
    assert!(bad.get("error").and_then(Json::as_str).unwrap().contains("unknown capability `files`"), "{bad:?}");

    // the sandbox: no child process, same JSON as nyra_run
    let (is_error, ok) = tool_json(&replies, 7);
    assert!(!is_error, "{ok:?}");
    assert_eq!((ok.get("ok").and_then(Json::as_bool), ok.get("exit").and_then(Json::as_u64)), (Some(true), Some(0)));
    assert_eq!(ok.get("stdout").and_then(Json::as_str), Some("true\n"));
    assert!(ok.get("steps").and_then(Json::as_u64).is_some_and(|s| s > 0));
    let (_, input) = tool_json(&replies, 8);
    assert_eq!(input.get("stdout").and_then(Json::as_str), Some("HELLO\n"), "{input:?}");

    // each limit: its code, its exit code, the position, and the output so far
    let limit = |id: u64, code: &str, exit: u64| -> Json {
        let (is_error, r) = tool_json(&replies, id);
        assert!(!is_error);
        assert_eq!(r.get("ok").and_then(Json::as_bool), Some(false));
        assert_eq!(r.get("exit").and_then(Json::as_u64), Some(exit), "{r:?}");
        let e = &r.get("errors").and_then(Json::as_array).unwrap()[0];
        assert_eq!(e.get("code").and_then(Json::as_str), Some(code), "{r:?}");
        assert_eq!(e.get("runtime").and_then(Json::as_bool), Some(true));
        assert!(e.get("line").and_then(Json::as_u64).is_some_and(|l| l > 0));
        r
    };
    limit(9, "E0355", 120);
    limit(10, "E0356", 121);
    let chatty = limit(11, "E0357", 122);
    assert_eq!(chatty.get("truncated").and_then(Json::as_bool), Some(true));
    assert_eq!(chatty.get("stdout").and_then(Json::as_str).unwrap().len(), 16 * 1024);
    let timed = limit(16, "E0359", 124);
    assert_eq!(timed.get("timeout").and_then(Json::as_bool), Some(true));

    // arguments, runtime errors of the program, compile errors
    assert_eq!(tool_json(&replies, 12).1.get("stdout").and_then(Json::as_str), Some("[\"a\", \"b c\"]\n"));
    let (_, oob) = tool_json(&replies, 13);
    assert_eq!(oob.get("exit").and_then(Json::as_u64), Some(101));
    assert_eq!(oob.get("stdout").and_then(Json::as_str), Some("before\n"));
    assert_eq!(oob.get("errors").and_then(Json::as_array).unwrap()[0].get("code").and_then(Json::as_str), Some("E0240"));
    assert_eq!(
        tool_json(&replies, 14).1.get("errors").and_then(Json::as_array).unwrap()[0].get("code").and_then(Json::as_str),
        Some("E0201")
    );
    let (is_error, zero) = tool_json(&replies, 15);
    assert!(is_error, "{zero:?}");

    // the same sandboxed program stops at the same step every time
    let again = session(&[call(2, "nyra_run", &format!(r#"{{"code":{},"sandbox":true,"fuel":5000}}"#, esc(FOREVER)))]);
    let first = tool_json(&replies, 9).1;
    let second = tool_json(&again, 2).1;
    assert_eq!(first.get("errors"), second.get("errors"), "deterministic");
    assert_eq!(first.get("steps"), second.get("steps"));
}

fn has_cc() -> bool {
    std::env::var("NYRA_CC").is_ok() || ["gcc", "clang", "cc", "tcc"].iter().any(|c| Command::new(c).arg("--version").output().is_ok())
}

const SUM: &str = "fn main() {
    print(\"start\")
    var t = 0
    for i in 0..200000 {
        t += i % 7
    }
    print(\"sum {t}\")
}
";
const SLOW_BOUNDS: &str = "fn main() {
    print(\"before\")
    var t = 0
    for i in 0..100000 {
        t += i
    }
    let xs = [1, 2]
    print(xs[t % 5 + 5])
}
";
const APPEND: &str = "use fs
fn main() {
    fs.write(\"mcp_auto_log.txt\", \"\")
    fs.append(\"mcp_auto_log.txt\", \"x\")
    print(fs.read(\"mcp_auto_log.txt\").len())
}
";
const ECHO: &str = "use input
fn main() {
    for l in input.lines() {
        print(l.upper())
    }
}
";

#[test]
fn nyra_run_answers_from_the_interpreter_and_falls_back_to_a_native_run() {
    if !has_cc() {
        common::missing("no C compiler for the native backend");
        return;
    }
    let init = request(1, "initialize", r#"{"protocolVersion":"2025-06-18","capabilities":{}}"#);
    let run = |id: u64, code: &str, extra: &str| call(id, "nyra_run", &format!(r#"{{"code":{}{extra}}}"#, esc(code)));
    let requests = vec![
        init,
        run(2, SUM, ""),
        run(3, SLOW_BOUNDS, ""),
        run(4, ECHO, r#","stdin":"ab\ncd\n""#),
        run(5, APPEND, r#","allow":["fs"]"#),
    ];
    // within the budget: the interpreter answers (`mode`), with the fields of any run
    let fast = session(&requests);
    // with a budget of 5 steps: the native executable answers, with the same results
    let slow = session_with(&requests, &[("NYRA_AUTO_STEPS", "5")]);
    for id in [2, 3, 4] {
        let (is_error, a) = tool_json(&fast, id);
        let (_, b) = tool_json(&slow, id);
        assert!(!is_error, "{a:?}");
        assert_eq!(a.get("mode").and_then(Json::as_str), Some("interp"), "run {id} should be interpreted: {a:?}");
        assert_eq!(b.get("mode"), None, "run {id} should be native: {b:?}");
        for key in ["ok", "exit", "stdout", "errors"] {
            assert_eq!(a.get(key), b.get(key), "run {id}, `{key}`");
        }
        assert!(b.get("ms").and_then(|m| m.get("run")).is_some() && a.get("ms").and_then(|m| m.get("run")).is_some());
    }
    assert_eq!(
        tool_json(&fast, 2).1.get("stdout").and_then(Json::as_str),
        Some(
            "start
sum 599994
"
        )
    );
    assert_eq!(tool_json(&fast, 3).1.get("exit").and_then(Json::as_u64), Some(101));
    assert_eq!(
        tool_json(&fast, 4).1.get("stdout").and_then(Json::as_str),
        Some(
            "AB
CD
"
        )
    );
    // a program that uses fs is never interpreted first (that would write twice)
    for replies in [&fast, &slow] {
        let (_, fs) = tool_json(replies, 5);
        assert_eq!(fs.get("mode"), None, "{fs:?}");
        assert_eq!(
            fs.get("stdout").and_then(Json::as_str),
            Some(
                "1
"
            ),
            "{fs:?}"
        );
    }
}

const SHAPES_MOD: &str = "pub struct Rect { w: int, h: int }
pub fn area(r: Rect) -> int = r.w * r.h
ex area(Rect(w: 2, h: 5)) == 10
";
const SHAPES_MAIN: &str = "use ./shapes
let r = Rect(w: 3, h: 4)
print(shapes.area(r))
";

fn files(entries: &[(&str, &str)]) -> String {
    let items: Vec<String> = entries.iter().map(|(n, t)| format!("{}:{}", esc(n), esc(t))).collect();
    format!("{{{}}}", items.join(","))
}

#[test]
fn a_program_of_several_files_works_over_mcp() {
    let both = files(&[("main.nyra", SHAPES_MAIN), ("shapes.nyra", SHAPES_MOD)]);
    let bad_mod = files(&[
        ("main.nyra", SHAPES_MAIN),
        (
            "shapes.nyra",
            "pub fn area(r: Rect) -> int = r.w * r.q
pub struct Rect { w: int, h: int }
",
        ),
    ]);
    let requests = vec![
        call(1, "nyra_check", &format!(r#"{{"files":{both}}}"#)),
        call(2, "nyra_run", &format!(r#"{{"files":{both},"sandbox":true}}"#)),
        call(3, "nyra_test", &format!(r#"{{"files":{both}}}"#)),
        call(4, "nyra_check", &format!(r#"{{"files":{bad_mod}}}"#)),
        call(5, "nyra_check", &format!(r#"{{"files":{}}}"#, files(&[("main.nyra", SHAPES_MAIN)]))),
        call(6, "nyra_check", &format!(r#"{{"files":{}}}"#, files(&[("../x.nyra", "fn main() {}")]))),
        call(7, "nyra_check", &format!(r#"{{"files":{both},"code":"print(1)"}}"#)),
        call(8, "nyra_build", &format!(r#"{{"files":{both},"target":"js"}}"#)),
        call(9, "nyra_outline", &format!(r#"{{"files":{both},"file":"shapes.nyra"}}"#)),
        call(10, "nyra_edit", &format!(r#"{{"files":{both},"edits":"fn double(n: int) -> int = n * 2\n"}}"#)),
        call(
            11,
            "nyra_check",
            &format!(r#"{{"files":{},"entry":"app.nyra"}}"#, files(&[("app.nyra", SHAPES_MAIN), ("shapes.nyra", SHAPES_MOD)])),
        ),
    ];
    let replies = session(&requests);
    let (err, j) = tool_json(&replies, 1);
    assert!(!err && j.get("ok").and_then(Json::as_bool) == Some(true), "{j:?}");
    let (_, j) = tool_json(&replies, 2);
    assert_eq!(
        j.get("stdout").and_then(Json::as_str),
        Some(
            "12
"
        ),
        "{j:?}"
    );
    let (_, j) = tool_json(&replies, 3);
    assert_eq!(j.get("examples").and_then(Json::as_u64), Some(1), "{j:?}");
    // an error in the imported file names it, relative to the project, and says nothing of the temp folder
    let (_, j) = tool_json(&replies, 4);
    let e = &j.get("errors").and_then(Json::as_array).unwrap()[0];
    assert_eq!(e.get("file").and_then(Json::as_str), Some("shapes.nyra"), "{j:?}");
    assert_eq!(e.get("code").and_then(Json::as_str), Some("E0224"));
    assert!(!e.get("message").and_then(Json::as_str).unwrap().contains("standard module"));
    // a missing file does not leak the folder the files were written to
    let (_, j) = tool_json(&replies, 5);
    let msg = j.get("errors").and_then(Json::as_array).unwrap()[0].get("message").and_then(Json::as_str).unwrap().to_string();
    assert!(msg.contains("no file `shapes.nyra`") && !msg.contains("proj-"), "{msg}");
    let (is_error, text) = tool_text(&replies, 6);
    assert!(is_error && text.contains("is not a file name Nyra imports"), "{text}");
    let (is_error, text) = tool_text(&replies, 7);
    assert!(is_error && text.contains("either `code` or `files`"), "{text}");
    let (_, j) = tool_json(&replies, 8);
    assert!(j.get("source").and_then(Json::as_str).unwrap().contains("area"));
    let (_, text) = tool_text(&replies, 9);
    assert!(text.contains("1 pub struct Rect") && text.contains("2 pub fn area(r: Rect) -> int"), "{text}");
    let (_, j) = tool_json(&replies, 10);
    assert!(
        j.get("code").and_then(Json::as_str).unwrap().ends_with(
            "fn double(n: int) -> int = n * 2
"
        ),
        "{j:?}"
    );
    assert_eq!(j.get("file").and_then(Json::as_str), Some("main.nyra"));
    let (_, j) = tool_json(&replies, 11);
    assert_eq!(j.get("ok").and_then(Json::as_bool), Some(true), "{j:?}");
}

#[test]
fn errors_with_one_certain_fix_are_repaired_in_memory_unless_strict() {
    let semicolon = "fn main() {
    let x = 1;
    print(x)
}
";
    let requests = vec![
        call(1, "nyra_check", &format!(r#"{{"code":{}}}"#, esc(semicolon))),
        call(2, "nyra_check", &format!(r#"{{"code":{},"strict":true}}"#, esc(semicolon))),
        call(3, "nyra_run", &format!(r#"{{"code":{},"sandbox":true}}"#, esc(semicolon))),
        call(4, "nyra_test", &format!(r#"{{"code":{}}}"#, esc(semicolon))),
        call(
            5,
            "nyra_check",
            &format!(
                r#"{{"code":{}}}"#,
                esc("print(\"cost: ${3}\")
")
            ),
        ),
        call(6, "nyra_build", &format!(r#"{{"code":{},"target":"js"}}"#, esc(semicolon))),
    ];
    let replies = session(&requests);
    let (_, j) = tool_json(&replies, 1);
    assert_eq!(j.get("ok").and_then(Json::as_bool), Some(true), "{j:?}");
    let w = &j.get("warnings").and_then(Json::as_array).unwrap()[0];
    assert_eq!(w.get("code").and_then(Json::as_str), Some("E0005"));
    assert_eq!(w.get("applied").and_then(Json::as_str), Some("`let x = 1`"));
    assert!(w.get("fix").is_some());
    let (_, j) = tool_json(&replies, 2);
    assert_eq!(j.get("ok").and_then(Json::as_bool), Some(false), "{j:?}");
    assert!(j.get("warnings").is_none());
    let (_, j) = tool_json(&replies, 3);
    assert_eq!(
        j.get("stdout").and_then(Json::as_str),
        Some(
            "1
"
        ),
        "{j:?}"
    );
    assert_eq!(j.get("warnings").and_then(Json::as_array).unwrap().len(), 1);
    let (_, j) = tool_json(&replies, 4);
    assert_eq!(j.get("ok").and_then(Json::as_bool), Some(true), "{j:?}");
    assert_eq!(j.get("warnings").and_then(Json::as_array).unwrap().len(), 1);
    // the compiler's own warnings are listed too
    let (_, j) = tool_json(&replies, 5);
    assert_eq!(j.get("warnings").and_then(Json::as_array).unwrap()[0].get("code").and_then(Json::as_str), Some("E0260"), "{j:?}");
    let (_, j) = tool_json(&replies, 6);
    assert_eq!(j.get("warnings").and_then(Json::as_array).unwrap().len(), 1);
}
