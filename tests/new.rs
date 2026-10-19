//! `nyra new`: every starter compiles, passes its examples, runs or builds, and every command in its
//! README works.

mod common;

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use common::{missing, nyra, scratch, stderr, stdout};

/// A fresh folder in which `nyra new <kind> <name>` has run.
fn starter(kind: &str, name: &str) -> PathBuf {
    let dir = scratch(&format!("new-{kind}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let out = nyra().args(["new", kind, name]).current_dir(&dir).output().unwrap();
    assert!(out.status.success(), "nyra new {kind} {name}: {}", stderr(&out));
    assert!(stdout(&out).contains(&format!("cd {name}")), "{}", stdout(&out));
    let project = dir.join(name);
    for f in ["main.nyra", "README.md", ".gitignore", "AGENTS.md"] {
        assert!(project.join(f).is_file(), "{kind}: no {f}");
    }
    let agents = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/AGENTS.md")).unwrap();
    assert_eq!(std::fs::read_to_string(project.join("AGENTS.md")).unwrap(), agents);
    let gitignore = std::fs::read_to_string(project.join(".gitignore")).unwrap();
    assert!(gitignore.contains("dist/") && gitignore.contains("/main.exe"), "{gitignore}");
    let main = std::fs::read_to_string(project.join("main.nyra")).unwrap();
    assert!(!main.contains("__NAME__") && !main.contains("__TITLE__"), "{main}");
    project
}

/// The lines of the README's ```sh block.
fn readme_commands(project: &Path) -> Vec<String> {
    let readme = std::fs::read_to_string(project.join("README.md")).unwrap();
    let block = readme.split("```sh\n").nth(1).expect("README has a ```sh block");
    let block = block.split("```").next().unwrap();
    let cmds: Vec<String> = block.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect();
    assert!(!cmds.is_empty());
    cmds
}

/// True when `out` failed only because this machine has no C compiler (then the step is skipped).
fn no_cc(out: &std::process::Output) -> bool {
    !out.status.success() && stderr(out).contains("no C compiler found")
}

/// Runs one README line (`nyra ...`, or `echo "text" | nyra ...`) in `project` and returns its output.
fn run_line(project: &Path, line: &str) -> Option<String> {
    let (input, cmd) = match line.strip_prefix("echo \"") {
        Some(rest) => {
            let (text, cmd) = rest.split_once("\" | ").expect("echo \"...\" | nyra ...");
            (Some(format!("{text}\n")), cmd)
        }
        None => (None, line),
    };
    let args: Vec<&str> = cmd.split_whitespace().collect();
    assert_eq!(args[0], "nyra", "README line `{line}` is not a nyra command");
    let mut child = nyra()
        .args(&args[1..])
        .current_dir(project)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    if let Some(text) = input {
        stdin.write_all(text.as_bytes()).unwrap();
    }
    drop(stdin);
    let out = child.wait_with_output().unwrap();
    if no_cc(&out) {
        missing(&format!("a C compiler for `{line}`"));
        return None;
    }
    assert!(out.status.success(), "`{line}` failed:\n{}{}", stdout(&out), stderr(&out));
    Some(stdout(&out) + &stderr(&out))
}

#[test]
fn nyra_new_lists_the_kinds() {
    let out = nyra().arg("new").output().unwrap();
    assert!(out.status.success());
    for k in ["script", "cli", "site", "lib"] {
        assert!(stdout(&out).contains(&format!("  {k} ")), "{}", stdout(&out));
    }
}

#[test]
fn nyra_new_refuses_bad_requests() {
    let dir = scratch("new-refuses");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("taken")).unwrap();
    std::fs::write(dir.join("taken/keep.txt"), "mine").unwrap();
    let cases: [(&[&str], &str); 4] = [
        (&["new", "game", "x"], "unknown kind `game`"),
        (&["new", "script"], "needs a name"),
        (&["new", "script", "taken"], "not empty"),
        (&["new", "script", "bad name!"], "not a good folder name"),
    ];
    for (args, want) in cases {
        let out = nyra().args(args).current_dir(&dir).output().unwrap();
        assert!(!out.status.success(), "{args:?}");
        assert!(stderr(&out).contains(want), "{args:?}: {}", stderr(&out));
    }
    // nothing was overwritten
    assert_eq!(std::fs::read_to_string(dir.join("taken/keep.txt")).unwrap(), "mine");
    assert!(!dir.join("taken/main.nyra").exists());
}

#[test]
fn the_script_starter_runs_and_its_readme_commands_work() {
    let p = starter("script", "hello");
    let mut ran = String::new();
    for line in readme_commands(&p) {
        if let Some(out) = run_line(&p, &line) {
            ran += &out;
        }
    }
    assert!(ran.contains("Hello, world!") && ran.contains("squares: [1, 4, 9, 16, 25]"), "{ran}");
    assert!(ran.contains("passed"), "{ran}");
    assert!(ran.contains("\"ok\":true"), "{ran}");
}

#[test]
fn the_cli_starter_reads_arguments_and_standard_input() {
    let p = starter("cli", "words");
    let cmds = readme_commands(&p);
    assert!(cmds.iter().any(|c| c.starts_with("echo ")), "the README shows standard input: {cmds:?}");
    let mut outs = Vec::new();
    for line in &cmds {
        outs.push(run_line(&p, line).unwrap_or_default());
    }
    assert!(outs[0].contains("    2  the") && outs[0].contains("5 words, 4 different"), "{}", outs[0]);
    assert!(outs[1].contains("    2  a") && !outs[1].contains("  b"), "{}", outs[1]);
    assert!(outs[2].contains("    2  two") && outs[2].contains("3 words"), "{}", outs[2]);
    let help = nyra().args(["run", "main.nyra", "--", "--help"]).current_dir(&p).output().unwrap();
    assert!(stdout(&help).contains("usage: words"), "{}", stdout(&help));
}

#[test]
fn the_lib_starter_uses_its_module() {
    let p = starter("lib", "stats");
    assert!(p.join("lib.nyra").is_file());
    let mut ran = String::new();
    for line in readme_commands(&p) {
        ran += &run_line(&p, &line).unwrap_or_default();
    }
    assert!(ran.contains("7 values from 9 to 21, mean 14.71, spread 12") && ran.contains("a warm week"), "{ran}");
    // what is not `pub` stays in lib.nyra
    std::fs::write(p.join("bad.nyra"), "use ./lib\nprint(lib.spread(lib.summarize([1])))\n").unwrap();
    let out = nyra().args(["check", "bad.nyra"]).current_dir(&p).output().unwrap();
    assert!(!out.status.success());
}

#[test]
fn the_site_starter_builds_and_serves() {
    let p = starter("site", "my-site");
    for line in readme_commands(&p) {
        if line == "nyra run main.nyra" {
            serve(&p);
        } else {
            run_line(&p, &line);
        }
    }
    let html = std::fs::read_to_string(p.join("dist/index.html")).unwrap();
    assert!(html.contains("<title>My Site</title>") || html.contains("My Site"), "{html}");
    assert!(html.contains("What you get") && html.contains("Made with"), "{html}");
}

/// `nyra run main.nyra` of a site serves it: start it on a free port, fetch the page, stop it.
fn serve(p: &Path) {
    let mut child =
        nyra().args(["run", "main.nyra", "--port", "0"]).current_dir(p).stdout(Stdio::null()).stderr(Stdio::piped()).spawn().unwrap();
    let mut err = BufReader::new(child.stderr.take().unwrap());
    let mut line = String::new();
    let mut url = None;
    while err.read_line(&mut line).unwrap_or(0) > 0 {
        if let Some(rest) = line.split("serving http://").nth(1) {
            url = Some(rest.split('/').next().unwrap().to_string());
            break;
        }
        line.clear();
    }
    let addr = url.unwrap_or_else(|| {
        let _ = child.kill();
        panic!("`nyra run main.nyra` did not say where it serves: {line}")
    });
    let mut s = std::net::TcpStream::connect(&addr).unwrap();
    s.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").unwrap();
    let mut page = String::new();
    let _ = s.read_to_string(&mut page);
    let _ = child.kill();
    let _ = child.wait();
    assert!(page.starts_with("HTTP/1.1 200") && page.contains("My Site"), "{page}");
}

#[test]
fn a_starter_can_be_made_in_an_empty_folder_and_a_nested_path() {
    let dir = scratch("new-nested");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("empty")).unwrap();
    let out = nyra().args(["new", "script", "empty"]).current_dir(&dir).output().unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    let out = nyra().args(["new", "lib", "apps/tools"]).current_dir(&dir).output().unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    let readme = std::fs::read_to_string(dir.join("apps/tools/README.md")).unwrap();
    assert!(readme.starts_with("# Tools\n"), "{readme}");
    let out =
        Command::new(env!("CARGO_BIN_EXE_nyra")).args(["check", "main.nyra"]).current_dir(dir.join("apps/tools")).output().unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
}
