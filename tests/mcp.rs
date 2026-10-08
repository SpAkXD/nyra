//! `nyra mcp`: drives a whole session over stdio and checks every reply.

mod common;

use std::io::Write;
use std::process::{Command, Stdio};

use common::{nyra, Json};

/// Sends `requests` (one JSON message per line), closes stdin and returns the replies by id.
fn session(requests: &[String]) -> Vec<Json> {
    let mut child = nyra().arg("mcp").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
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
    replies
        .iter()
        .find(|r| r.get("id").and_then(Json::as_u64) == Some(id))
        .unwrap_or_else(|| panic!("no reply with id {id}"))
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
    Command::new("node").arg("--version").output().is_ok()
}

const HELLO: &str = "fn main() {\n    for i in 0..3 {\n        print(\"hi {i}\")\n    }\n}\n";
const TYPO: &str = "fn main() {\n    let count = 1\n    print(cout)\n}\n";
const OUT_OF_BOUNDS: &str = "fn main() {\n    let xs = [1, 2]\n    print(\"before\")\n    print(xs[5])\n}\n";
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
    for want in ["nyra_check", "nyra_run", "nyra_explain", "nyra_spec", "nyra_build"] {
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
    assert_eq!(spec, std::fs::read_to_string("docs/SPEC.md").unwrap());
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
    assert_eq!(uris, ["nyra://spec", "nyra://guide", "nyra://errors"]);
    let contents = result(&replies, 17).get("contents").and_then(Json::as_array).unwrap();
    assert_eq!(contents[0].get("mimeType").and_then(Json::as_str), Some("text/markdown"));
    assert_eq!(contents[0].get("text").and_then(Json::as_str), Some(spec.as_str()));
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
