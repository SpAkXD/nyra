//! Symbol-addressed editing: `nyra outline`, `nyra show`, `nyra edit` and the MCP tools
//! `nyra_outline`, `nyra_show`, `nyra_edit`.
//!
//! - every operation changes exactly the range of its symbol: the rest of the file stays
//!   byte-identical (line breaks included);
//! - an edit that adds errors is refused and the file is not written; `--force` and `--fix`;
//! - a rename changes the definition and the real references only;
//! - and what it saves: one changed function instead of the whole file.

mod common;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

use common::{nyra, scratch, stderr, stdout, Json};

const SHAPES: &str = "\
// shapes and their areas
struct Point {
    x: int
    y: int
}

struct Rect { at: Point, w: int, h: int }

// The area of a rectangle.
fn area(r: Rect) -> int = r.w * r.h

fn grow(r: Rect, by: int) -> Rect {
    ret Rect(at: r.at, w: r.w + by, h: r.h + by)
}

fn main() {
    let r = Rect(at: Point(x: 1, y: 2), w: 3, h: 4)
    let rs: [Rect] = [r, grow(r, 1)]
    // area of the first rect
    print(\"area {area(r)} at {r.at.x}\")
    print(area(rs[1]))
    print(\"area is not renamed in a string\")
}
";

/// A file in a fresh directory for one test.
fn file(test: &str, text: &str) -> PathBuf {
    let dir = scratch(&format!("edit-{test}"));
    let path = dir.join("prog.nyra");
    std::fs::write(&path, text).unwrap();
    path
}

fn run(args: &[&str], path: &Path, stdin: Option<&str>) -> Output {
    let mut cmd = nyra();
    cmd.args(&args[..1]).arg(path).args(&args[1..]);
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().unwrap();
    {
        let mut pipe = child.stdin.take().unwrap();
        if let Some(s) = stdin {
            pipe.write_all(s.as_bytes()).unwrap();
        }
    }
    child.wait_with_output().unwrap()
}

/// `nyra edit prog.nyra <args>` on `text`: (success, the file afterwards, stderr).
fn edit(test: &str, text: &str, args: &[&str], stdin: Option<&str>) -> (bool, String, String) {
    let path = file(test, text);
    let mut all = vec!["edit"];
    all.extend_from_slice(args);
    let out = run(&all, &path, stdin);
    (out.status.success(), std::fs::read_to_string(&path).unwrap(), stderr(&out))
}

fn compiles(text: &str) -> bool {
    static N: AtomicUsize = AtomicUsize::new(0);
    let path = file(&format!("compiles-{}", N.fetch_add(1, Ordering::Relaxed)), text);
    nyra().args(["check"]).arg(&path).output().unwrap().status.success()
}

#[test]
fn the_fixture_compiles() {
    assert!(compiles(SHAPES));
}

// ---- outline and show ---------------------------------------------------------------------------------

#[test]
fn outline_lists_symbols_with_lines() {
    let path = file("outline", SHAPES);
    let out = run(&["outline"], &path, None);
    assert!(out.status.success());
    let text = stdout(&out);
    let lines: Vec<&str> = text.lines().skip(1).collect();
    assert_eq!(
        lines,
        [
            "2-5 struct Point { x: int, y: int }",
            "7 struct Rect { at: Point, w: int, h: int }",
            "10 fn area(r: Rect) -> int",
            "12-14 fn grow(r: Rect, by: int) -> Rect",
            "16-23 fn main()",
        ]
    );
    assert!(text.starts_with(&format!("{}: 23 lines\n", path.display())));

    let out = run(&["outline", "--json"], &path, None);
    let json = Json::parse(stdout(&out).trim()).unwrap();
    let syms = json.get("symbols").and_then(Json::as_array).unwrap();
    assert_eq!(syms.len(), 5);
    assert_eq!(syms[0].get("fields").and_then(Json::as_array).unwrap().len(), 2);
    assert_eq!(syms[2].get("sig").and_then(Json::as_str), Some("fn area(r: Rect) -> int"));
    assert_eq!(syms[3].get("end_line").and_then(Json::as_u64), Some(14));
}

#[test]
fn outline_of_a_script_and_of_a_broken_file() {
    let script = "fn sq(x: int) -> int = x * x\nlet n = 3\nfor i in 0..n {\n    print(sq(i))\n}\n";
    let out = run(&["outline"], &file("outline-script", script), None);
    assert!(stdout(&out).ends_with("1 fn sq(x: int) -> int\n2-5 script (2 statements)\n"), "{}", stdout(&out));
    // a function with a missing `}` does not swallow the next one
    let broken = "fn a() {\n    print(1)\nfn b() = print(2)\nfn main() {\n    a()\n}\n";
    let out = run(&["outline"], &file("outline-broken", broken), None);
    assert!(stdout(&out).ends_with("1-2 fn a()\n3 fn b()\n4-6 fn main()\n"), "{}", stdout(&out));
}

#[test]
fn show_prints_symbols_as_they_are() {
    let path = file("show", SHAPES);
    let out = run(&["show", "area", "Rect.w", "grow"], &path, None);
    assert!(out.status.success());
    assert_eq!(
        stdout(&out),
        "// The area of a rectangle.\nfn area(r: Rect) -> int = r.w * r.h\n\nw: int\n\nfn grow(r: Rect, by: int) -> Rect {\n    ret Rect(at: r.at, w: r.w + by, h: r.h + by)\n}\n"
    );
    let out = run(&["show", "--json", "Point"], &path, None);
    let json = Json::parse(stdout(&out).trim()).unwrap();
    let p = &json.get("symbols").and_then(Json::as_array).unwrap()[0];
    assert_eq!(p.get("text").and_then(Json::as_str), Some("// shapes and their areas\nstruct Point {\n    x: int\n    y: int\n}"));
    assert_eq!(p.get("line").and_then(Json::as_u64), Some(1));
    // an unknown name: a suggestion, exit 1
    let out = run(&["show", "aea"], &path, None);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("did you mean `area`?"), "{}", stderr(&out));
    let out = run(&["show", "volume"], &path, None);
    assert!(stderr(&out).contains("the file has: Point, Rect, area, grow, main"), "{}", stderr(&out));
    let out = run(&["show", "Rect.depth"], &path, None);
    assert!(stderr(&out).contains("its fields: at, w, h"), "{}", stderr(&out));
}

// ---- replace, add, delete ------------------------------------------------------------------------------

#[test]
fn replace_changes_only_that_symbol() {
    let new = "fn grow(r: Rect, by: int) -> Rect = Rect(at: r.at, w: r.w * by, h: r.h * by)";
    let (ok, after, err) = edit("replace", SHAPES, &["--set", "grow", new], None);
    assert!(ok, "{err}");
    let expected = SHAPES.replace("fn grow(r: Rect, by: int) -> Rect {\n    ret Rect(at: r.at, w: r.w + by, h: r.h + by)\n}", new);
    assert_eq!(after, expected);
    assert!(err.contains("replaced fn grow (line 12)"), "{err}");
}

#[test]
fn replace_keeps_or_replaces_the_comment_above() {
    // without a comment in the new code the old one stays
    let (ok, after, _) = edit("doc-keep", SHAPES, &["--set", "area", "fn area(r: Rect) -> int = r.h * r.w"], None);
    assert!(ok);
    assert!(after.contains("// The area of a rectangle.\nfn area(r: Rect) -> int = r.h * r.w\n"));
    // with one, it replaces the old one
    let (ok, after, _) = edit("doc-new", SHAPES, &["--set", "area", "// w times h\nfn area(r: Rect) -> int = r.h * r.w"], None);
    assert!(ok);
    assert!(after.contains("\n\n// w times h\nfn area(r: Rect) -> int = r.h * r.w\n\nfn grow"), "{after}");
    assert!(!after.contains("The area of"));
}

#[test]
fn stdin_definitions_replace_or_add() {
    // plain code on stdin: each definition replaces its namesake, a new one goes to the end
    let code = "    fn area(r: Rect) -> int {\n        ret r.w * r.h\n    }\n\n    fn perimeter(r: Rect) -> int = 2 * (r.w + r.h)\n";
    let (ok, after, err) = edit("upsert", SHAPES, &[], Some(code));
    assert!(ok, "{err}");
    assert!(after.contains("// The area of a rectangle.\nfn area(r: Rect) -> int {\n    ret r.w * r.h\n}\n\nfn grow"), "{after}");
    assert!(
        after.ends_with("    print(\"area is not renamed in a string\")\n}\n\nfn perimeter(r: Rect) -> int = 2 * (r.w + r.h)\n"),
        "{after}"
    );
    assert!(err.contains("replaced fn area (lines 10-12), added fn perimeter (line 27)"), "{err}");
}

#[test]
fn add_after_and_before() {
    let script = "@add after area\nfn half(r: Rect) -> int = area(r) / 2\n@add before Point\nstruct Size { w: int, h: int }\n";
    let (ok, after, err) = edit("add", SHAPES, &[], Some(script));
    assert!(ok, "{err}");
    assert!(after.contains("fn area(r: Rect) -> int = r.w * r.h\n\nfn half(r: Rect) -> int = area(r) / 2\n\nfn grow"), "{after}");
    assert!(after.starts_with("struct Size { w: int, h: int }\n\n// shapes and their areas\nstruct Point {"), "{after}");
    // a name that exists is refused: replace it instead
    let (ok, after, err) = edit("add-twice", SHAPES, &["--add", "fn area(r: Rect) -> int = 0"], None);
    assert!(!ok && after == SHAPES);
    assert!(err.contains("`area` already exists (line 10)"), "{err}");
    // in a file without blank lines between definitions, none is added
    let tight = "fn a() -> int = 1\nfn main() {\n    print(a())\n}\n";
    let (ok, after, _) = edit("add-tight", tight, &["--add", "fn b() -> int = 2", "--after", "a"], None);
    assert!(ok);
    assert_eq!(after, "fn a() -> int = 1\nfn b() -> int = 2\nfn main() {\n    print(a())\n}\n");
}

#[test]
fn delete_takes_the_comment_and_one_blank_line() {
    let src = SHAPES.replace("    print(area(rs[1]))\n", "").replace("\"area {area(r)} at {r.at.x}\"", "r.at.x");
    assert!(compiles(&src));
    let (ok, after, err) = edit("delete", &src, &["--delete", "area"], None);
    assert!(ok, "{err}");
    assert_eq!(after, src.replace("// The area of a rectangle.\nfn area(r: Rect) -> int = r.w * r.h\n\n", ""));
    // deleting something still in use is refused
    let (ok, after, err) = edit("delete-used", SHAPES, &["--delete", "area"], None);
    assert!(!ok && after == SHAPES);
    assert!(err.contains("undefined function `area`") && err.contains("edit refused"), "{err}");
}

// ---- rename ------------------------------------------------------------------------------------------

#[test]
fn rename_a_function_changes_calls_only() {
    let (ok, after, err) = edit("rename-fn", SHAPES, &["--rename", "area", "surface"], None);
    assert!(ok, "{err}");
    let expected = SHAPES
        .replace("fn area(", "fn surface(")
        .replace("{area(r)}", "{surface(r)}")
        .replace("print(area(rs[1]))", "print(surface(rs[1]))");
    assert_eq!(after, expected);
    // strings, comments and the word in other names are untouched
    assert!(after.contains("\"area is not renamed in a string\"") && after.contains("// area of the first rect"));
    assert!(err.contains("renamed fn area -> surface, 2 references"), "{err}");
}

#[test]
fn rename_a_struct_changes_types_and_constructions() {
    let (ok, after, err) = edit("rename-struct", SHAPES, &["--rename", "Rect", "Box"], None);
    assert!(ok, "{err}");
    assert_eq!(after.matches("Rect").count(), 0, "{after}");
    assert_eq!(after.matches("Box").count(), 7);
    assert!(after.contains("let rs: [Box] = [r, grow(r, 1)]") && after.contains("fn grow(r: Box, by: int) -> Box {"));
}

#[test]
fn rename_a_field_follows_types() {
    // two structs with a field `w`: only Rect's changes
    let src = format!("{SHAPES}struct Wide {{ w: int }}\nfn wide(v: Wide) -> int = v.w\n");
    assert!(compiles(&src));
    let (ok, after, err) = edit("rename-field", &src, &["--rename", "Rect.w", "width"], None);
    assert!(ok, "{err}");
    let expected = src
        .replace("Rect { at: Point, w: int, h: int }", "Rect { at: Point, width: int, h: int }")
        .replace("= r.w * r.h", "= r.width * r.h")
        .replace("w: r.w + by", "width: r.width + by")
        .replace("w: 3, h: 4", "width: 3, h: 4");
    assert_eq!(after, expected);
    assert!(after.ends_with("struct Wide { w: int }\nfn wide(v: Wide) -> int = v.w\n"));
    // a field of a nested value, read in a string
    let (ok, after, _) = edit("rename-field-nested", SHAPES, &["--rename", "Point.x", "px"], None);
    assert!(ok);
    assert!(after.contains("{r.at.px}") && after.contains("Point(px: 1, y: 2)") && after.contains("    px: int\n"));
}

#[test]
fn rename_refuses_what_it_cannot_do_cleanly() {
    let (ok, _, err) = edit("rename-used", SHAPES, &["--rename", "area", "grow"], None);
    assert!(!ok && err.contains("`grow` is already used on line 12"), "{err}");
    let (ok, _, err) = edit("rename-local", SHAPES, &["--rename", "area", "rs"], None);
    assert!(!ok && err.contains("`rs` is already used"), "{err}");
    let (ok, _, err) = edit("rename-keyword", SHAPES, &["--rename", "area", "while"], None);
    assert!(!ok && err.contains("not a valid name"), "{err}");
    let (ok, _, err) = edit("rename-field-taken", SHAPES, &["--rename", "Rect.w", "h"], None);
    assert!(!ok && err.contains("already has a field `h`"), "{err}");
    let broken = SHAPES.replace("ret Rect(", "ret Rect((");
    let (ok, after, err) = edit("rename-broken", &broken, &["--rename", "area", "surface"], None);
    assert!(!ok && after == broken && err.contains("without syntax errors"), "{err}");
}

// ---- struct fields ----------------------------------------------------------------------------------------

#[test]
fn add_and_delete_fields() {
    let src =
        "struct A {\n    x: int\n    y: int\n}\nstruct B { x: int, y: int }\nstruct C {\n    x: int,\n    y: int,\n}\nstruct D {}\n";
    let (ok, after, err) =
        edit("fields", src, &[], Some("@add-field A z: [str] after x\n@add-field B z: A\n@add-field C z: int\n@add-field D z: int\n"));
    assert!(ok, "{err}");
    assert_eq!(
        after,
        "struct A {\n    x: int\n    z: [str]\n    y: int\n}\nstruct B { x: int, y: int, z: A }\nstruct C {\n    x: int,\n    y: int,\n    z: int,\n}\nstruct D { z: int }\n"
    );
    let (ok, after, err) =
        edit("fields-delete", &after, &[], Some("@delete A.z\n@delete B.x\n@delete B.z\n@delete C.y\n@delete D.z\n"));
    assert!(ok, "{err}");
    assert_eq!(
        after,
        "struct A {\n    x: int\n    y: int\n}\nstruct B { y: int }\nstruct C {\n    x: int,\n    z: int,\n}\nstruct D {}\n"
    );
    // commas between fields but none after the last
    let (ok, after, _) = edit("fields-commas", "struct E {\n    a: int,\n    b: int\n}\n", &["--add-field", "E", "c: int"], None);
    assert!(ok);
    assert_eq!(after, "struct E {\n    a: int,\n    b: int,\n    c: int\n}\n");
    // replace a field's type
    let (ok, after, _) = edit("field-replace", "struct E { a: int, b: int }\n", &["--set", "E.b", "b: float"], None);
    assert!(ok);
    assert_eq!(after, "struct E { a: int, b: float }\n");
}

#[test]
fn a_new_field_and_its_uses_go_in_one_batch() {
    // alone, the new field breaks every construction of Rect: refused
    let (ok, after, err) = edit("field-alone", SHAPES, &["--add-field", "Rect", "depth: int"], None);
    assert!(!ok && after == SHAPES, "{err}");
    // with the functions that build a Rect in the same batch, it goes through
    let script = "@add-field Rect depth: int\n\
                  @replace grow\nfn grow(r: Rect, by: int) -> Rect = Rect(at: r.at, w: r.w + by, h: r.h + by, depth: r.depth)\n\
                  @replace main\nfn main() {\n    let r = Rect(at: Point(x: 1, y: 2), w: 3, h: 4, depth: 5)\n    print(area(grow(r, 1)))\n}\n";
    let (ok, after, err) = edit("field-batch", SHAPES, &[], Some(script));
    assert!(ok, "{err}");
    assert!(after.contains("struct Rect { at: Point, w: int, h: int, depth: int }"));
    assert!(compiles(&after));
}

// ---- checking the result -----------------------------------------------------------------------------------

#[test]
fn an_edit_that_adds_errors_is_refused() {
    let bad = "fn area(r: Rect) -> int = r.w * r.depth";
    let (ok, after, err) = edit("refuse", SHAPES, &["--set", "area", bad], None);
    assert!(!ok);
    assert_eq!(after, SHAPES, "a refused edit writes nothing");
    assert!(err.contains("`Rect` has no field `depth`") && err.contains("edit refused"), "{err}");

    // --json: the errors, each with the symbol it is in
    let path = file("refuse-json", SHAPES);
    let out = run(&["edit", "--json", "--set", "area", bad], &path, None);
    assert_eq!(out.status.code(), Some(1));
    let json = Json::parse(stdout(&out).trim()).unwrap();
    assert_eq!(json.get("ok").and_then(Json::as_bool), Some(false));
    let e = &json.get("errors").and_then(Json::as_array).unwrap()[0];
    assert_eq!(e.get("symbol").and_then(Json::as_str), Some("fn area"));
    assert_eq!(e.get("line").and_then(Json::as_u64), Some(10));

    // --force applies it anyway
    let (ok, after, _) = edit("refuse-force", SHAPES, &["--force", "--set", "area", bad], None);
    assert!(ok);
    assert!(after.contains(bad));
}

#[test]
fn fix_repairs_the_new_code() {
    let sloppy = "fn area(r: Rect) -> int {\n    return r.w * r.h;\n}";
    let (ok, after, err) = edit("fix", SHAPES, &["--fix", "--set", "area", sloppy], None);
    assert!(ok, "{err}");
    assert!(after.contains("fn area(r: Rect) -> int {\n    return r.w * r.h\n}\n"), "{after}");
    assert!(err.contains("--fix repaired"), "{err}");
    let (ok, _, _) = edit("no-fix", SHAPES, &["--set", "area", sloppy], None);
    assert!(!ok);
}

#[test]
fn a_file_with_errors_can_be_repaired_symbol_by_symbol() {
    // two broken functions: repairing one is not refused for the other one's errors
    let broken = SHAPES.replace("= r.w * r.h", "= r.w * r.hh").replace("w: r.w + by", "w: r.ww + by");
    let (ok, after, err) = edit("repair", &broken, &["--set", "area", "fn area(r: Rect) -> int = r.w * r.h"], None);
    assert!(ok, "{err}");
    assert!(err.contains("still has 1 error"), "{err}");
    let (ok, after, _) = edit("repair-2", &after, &["--set", "grow", "fn grow(r: Rect, by: int) -> Rect = r"], None);
    assert!(ok);
    assert!(compiles(&after));
}

#[test]
fn dry_run_writes_nothing() {
    let path = file("dry", SHAPES);
    let out = run(&["edit", "--dry-run", "--rename", "grow", "bigger"], &path, None);
    assert!(out.status.success());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), SHAPES);
    assert!(stdout(&out).contains("+fn bigger(r: Rect, by: int) -> Rect {"), "{}", stdout(&out));
}

#[test]
fn crlf_files_stay_crlf_and_untouched_bytes_stay_identical() {
    let crlf = SHAPES.replace('\n', "\r\n");
    let path = file("crlf", &crlf);
    let out = run(&["edit"], &path, Some("fn area(r: Rect) -> int {\n    ret r.w * r.h\n}\n\nfn twice(n: int) -> int = n * 2\n"));
    assert!(out.status.success(), "{}", stderr(&out));
    let after = std::fs::read(&path).unwrap();
    let text = String::from_utf8(after).unwrap();
    assert!(!text.replace("\r\n", "").contains('\n'), "a bare \\n in a CRLF file:\n{text:?}");
    let expected = crlf.replace("fn area(r: Rect) -> int = r.w * r.h", "fn area(r: Rect) -> int {\r\n    ret r.w * r.h\r\n}")
        + "\r\nfn twice(n: int) -> int = n * 2\r\n";
    assert_eq!(text, expected);
    // a rename and a field edit too
    let out = run(&["edit", "--rename", "Point.y", "py", "--delete", "twice"], &path, None);
    assert!(out.status.success(), "{}", stderr(&out));
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(!text.replace("\r\n", "").contains('\n'));
    assert!(text.ends_with("    print(\"area is not renamed in a string\")\r\n}\r\n"), "{text:?}");
}

#[test]
fn edit_script_errors() {
    let (ok, after, err) = edit("script-errors", SHAPES, &[], Some("@replace area\nfn surface(r: Rect) -> int = 1\n"));
    assert!(!ok && after == SHAPES && err.contains("@rename area surface"), "{err}");
    let (_, _, err) = edit("script-unknown", SHAPES, &[], Some("@move area\n"));
    assert!(err.contains("unknown command `@move area`"), "{err}");
    let (_, _, err) = edit("script-statement", SHAPES, &[], Some("fn f() = print(1)\nprint(2)\n"));
    assert!(err.contains("only hold `fn` and `struct` definitions"), "{err}");
    let (_, _, err) = edit("script-main", "print(1)\n", &["--set", "main", "fn main() {\n}"], None);
    assert!(err.contains("this file is a script"), "{err}");
}

// ---- MCP ----------------------------------------------------------------------------------------------------

fn esc(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out += "\\\"",
            '\\' => out += "\\\\",
            '\n' => out += "\\n",
            '\r' => out += "\\r",
            c => out.push(c),
        }
    }
    out + "\""
}

fn call(id: u64, tool: &str, args: &str) -> String {
    format!(r#"{{"jsonrpc":"2.0","id":{id},"method":"tools/call","params":{{"name":"{tool}","arguments":{args}}}}}"#)
}

/// Runs an MCP session; returns each reply's (isError, text) by id order.
fn mcp(requests: &[String]) -> Vec<(bool, String)> {
    let mut child = nyra().arg("mcp").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    {
        let mut stdin = child.stdin.take().unwrap();
        for r in requests {
            writeln!(stdin, "{r}").unwrap();
        }
    }
    let out = child.wait_with_output().unwrap();
    stdout(&out)
        .lines()
        .map(|l| {
            let j = Json::parse(l).unwrap();
            let r = j.get("result").unwrap_or_else(|| panic!("an error reply: {l}"));
            let text = r.get("content").and_then(Json::as_array).unwrap()[0].get("text").and_then(Json::as_str).unwrap().to_string();
            (r.get("isError").and_then(Json::as_bool).unwrap(), text)
        })
        .collect()
}

#[test]
fn mcp_tools() {
    let path = file("mcp", SHAPES);
    let p = esc(&path.display().to_string());
    let replies = mcp(&[
        call(1, "nyra_outline", &format!(r#"{{"path":{p}}}"#)),
        call(2, "nyra_show", &format!(r#"{{"path":{p},"name":"area grow"}}"#)),
        call(3, "nyra_edit", &format!(r#"{{"path":{p},"edits":"@rename area surface"}}"#)),
        call(4, "nyra_edit", &format!(r#"{{"path":{p},"edits":{}}}"#, esc("fn surface(r: Rect) -> int = r.depth"))),
        call(
            5,
            "nyra_edit",
            &format!(
                r#"{{"code":{},"edits":"@delete Point.y","force":true}}"#,
                esc("struct Point { x: int, y: int }\nfn main() {\n}\n")
            ),
        ),
        call(6, "nyra_show", &format!(r#"{{"path":{p},"name":"nothing"}}"#)),
        call(7, "nyra_outline", &format!(r#"{{"code":{}}}"#, esc("fn main() {\n}\n"))),
        call(8, "nyra_edit", r#"{"path":"notes.txt","edits":"@delete a"}"#),
    ]);
    assert_eq!(replies.len(), 8);
    assert_eq!(replies[0], (false, format!("{}: 23 lines\n2-5 struct Point {{ x: int, y: int }}\n7 struct Rect {{ at: Point, w: int, h: int }}\n10 fn area(r: Rect) -> int\n12-14 fn grow(r: Rect, by: int) -> Rect\n16-23 fn main()\n", path.display())));
    assert!(replies[1].1.starts_with("// The area of a rectangle.\nfn area(r: Rect) -> int = r.w * r.h\n\nfn grow("));
    // with a path the file is written, and only a summary comes back
    assert_eq!(replies[2], (false, r#"{"ok":true,"edits":["renamed fn area -> surface, 2 references (line 10)"]}"#.to_string()));
    assert!(std::fs::read_to_string(&path).unwrap().contains("print(surface(rs[1]))"));
    // a refused edit: not an MCP error, the errors with their symbols
    let (is_error, text) = &replies[3];
    assert!(!is_error);
    let j = Json::parse(text).unwrap();
    assert_eq!(j.get("ok").and_then(Json::as_bool), Some(false));
    assert_eq!(j.get("errors").and_then(Json::as_array).unwrap()[0].get("symbol").and_then(Json::as_str), Some("fn surface"));
    assert!(!std::fs::read_to_string(&path).unwrap().contains("depth"));
    // with code the new code comes back
    let j = Json::parse(&replies[4].1).unwrap();
    assert_eq!(j.get("code").and_then(Json::as_str), Some("struct Point { x: int }\nfn main() {\n}\n"));
    assert!(replies[5].0 && replies[5].1.contains("no function or struct `nothing`"));
    assert_eq!(replies[6], (false, "main.nyra: 2 lines\n1-2 fn main()\n".to_string()));
    assert!(replies[7].0 && replies[7].1.contains("must be a .nyra file"));
}

// ---- what it saves ------------------------------------------------------------------------------------------

/// A 200-line program; one function changes. The whole file versus the edit, in bytes (about
/// 3.5 bytes make a token in code), for both directions of an agent's turn.
#[test]
fn one_edit_costs_a_fraction_of_the_whole_file() {
    let big = std::fs::read_to_string("tests/edit/inventory.nyra").unwrap().replace("\r\n", "\n");
    assert!(big.lines().count() >= 200);
    let new_discount = "fn discount(total: int) -> int {\n    if total > 100000 {\n        ret total * 15 / 100\n    }\n    if total > 50000 {\n        ret total / 10\n    }\n    if total > 10000 {\n        ret total / 20\n    }\n    ret 0\n}\n";
    let path = file("savings", &big);

    // the agent's side: what it sends (the edit) and what it reads back (the summary)
    let sent = new_discount.len();
    let out = run(&["edit", "--json"], &path, Some(new_discount));
    assert!(out.status.success(), "{}", stderr(&out));
    let reply = stdout(&out);
    let after = std::fs::read_to_string(&path).unwrap();
    let start = big.find("fn discount(").unwrap();
    let end = start + big[start..].find("\n}\n").unwrap() + 2;
    assert_eq!(after, format!("{}{}{}", &big[..start], new_discount.trim_end(), &big[end..]), "only discount changed");
    // the cheap way to look at a big file first
    let outline = stdout(&run(&["outline"], &path, None));
    let shown = stdout(&run(&["show", "discount"], &path, None));

    let whole = big.len();
    let edit_turn = sent + reply.trim().len();
    let saved = 100.0 - 100.0 * edit_turn as f64 / whole as f64;
    eprintln!(
        "inventory.nyra: {} lines, {whole} bytes (~{} tokens); outline {} bytes; show discount {} bytes; \
         edit sent {sent} bytes + reply {} bytes = {edit_turn} (~{} tokens): {saved:.1}% saved",
        big.lines().count(),
        whole * 10 / 35,
        outline.len(),
        shown.len(),
        reply.trim().len(),
        edit_turn * 10 / 35
    );
    assert!(edit_turn * 10 < whole, "an edit of one function must cost under a tenth of the file");
    assert!(outline.len() * 3 < whole, "the outline must cost under a third of the file");
    assert!(compiles(&after));
}
