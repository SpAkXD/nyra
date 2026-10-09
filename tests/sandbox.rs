//! Safety: capabilities (deny by default) and the sandboxed interpreter.
//!
//! - `nyra run --interp` must print what the compiled programs print: every `examples/*.nyra`
//!   (with its `.in`, `.args` and `.exit` files) and every `tests/runtime` error, differentially;
//! - the limits of a sandboxed run (fuel, memory, output, depth, time) stop it with their code
//!   and exit code (120 to 124), also the programs of `tests/runtime` marked `// only: interp`;
//! - a `use` of a module whose capability is not granted is error E0290, for `--sandbox`,
//!   `--allow`, and in `nyra outline --json`;
//! - property examples (`ex for n in 0..200: f(n) >= 0`) report the first input that fails.

mod common;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

use common::{flags_of, nyra, scratch, stderr, stdout, Json};

/// A new empty folder for one run.
fn fresh(name: &str) -> PathBuf {
    static RUNS: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join("nyra-sandbox-tests").join(format!(
        "{name}-{}-{}",
        std::process::id(),
        RUNS.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// `nyra run <file> <flags> [-- args]` in `dir` with `stdin`.
fn run(dir: &Path, file: &Path, flags: &[&str], args: &[String], stdin: &[u8]) -> Output {
    let mut cmd = nyra();
    cmd.current_dir(dir).arg("run").arg(file).args(flags);
    if !args.is_empty() {
        cmd.arg("--").args(args);
    }
    let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    // (a program may stop reading early: a closed pipe is not an error here)
    let _ = child.stdin.take().unwrap().write_all(stdin);
    child.wait_with_output().unwrap()
}

fn files(dir: &str) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> =
        std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).filter(|p| p.extension().is_some_and(|e| e == "nyra")).collect();
    v.sort();
    v
}

/// The exit code that the limit error `code` stands for.
fn exit_of(code: &str) -> i32 {
    match code {
        "E0355" => 120,
        "E0356" => 121,
        "E0357" => 122,
        "E0358" => 123,
        "E0359" => 124,
        _ => 101,
    }
}

/// Runs `check` over `items` on a few threads and collects the failures.
fn in_parallel(items: Vec<PathBuf>, check: impl Fn(&PathBuf) -> Option<String> + Sync) {
    let next = AtomicUsize::new(0);
    let failures = std::sync::Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..4 {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::SeqCst);
                let Some(item) = items.get(i) else { break };
                if let Some(f) = check(item) {
                    failures.lock().unwrap().push(f);
                }
            });
        }
    });
    let failures = failures.into_inner().unwrap();
    assert!(failures.is_empty(), "{} failure(s):\n\n{}", failures.len(), failures.join("\n\n"));
}

// ---- the interpreter prints what the compiled programs print ---------------------------------------------

/// One example in the interpreter: `None` when it behaves like the compiled programs.
fn example_in(path: &PathBuf, flags: &[&str]) -> Option<String> {
    let expected = std::fs::read_to_string(path.with_extension("out")).ok()?.replace("\r\n", "\n");
    let args: Vec<String> =
        std::fs::read_to_string(path.with_extension("args")).map(|a| a.lines().map(String::from).collect()).unwrap_or_default();
    let stdin = std::fs::read(path.with_extension("in")).unwrap_or_default();
    let want_exit = std::fs::read_to_string(path.with_extension("exit")).map_or(0, |e| e.trim().parse::<i32>().unwrap());
    let dir = fresh(&path.file_stem().unwrap().to_string_lossy());
    let out = run(&dir, &std::fs::canonicalize(path).unwrap(), flags, &args, &stdin);
    let _ = std::fs::remove_dir_all(&dir);
    let label = format!("{} {flags:?}", path.display());
    if out.status.code() != Some(want_exit) {
        return Some(format!("{label}: exit code {:?}, expected {want_exit}:\n{}", out.status.code(), stderr(&out)));
    }
    let got = stdout(&out);
    (got != expected).then(|| format!("{label}: the output differs\n--- got\n{got}--- expected\n{expected}"))
}

#[test]
fn the_interpreter_prints_what_the_compiled_programs_print() {
    let all: Vec<PathBuf> = files("examples").into_iter().filter(|p| p.with_extension("out").exists()).collect();
    assert!(all.len() >= 40);
    in_parallel(all.clone(), |p| example_in(p, &["--interp"]));
    // the sandbox runs them too, with everything granted (and the files confined to the folder)
    in_parallel(all, |p| example_in(p, &["--sandbox", "--allow", "all"]));
}

#[test]
fn the_interpreter_reports_runtime_errors_like_the_compiled_programs() {
    let mut checked = 0;
    let mut tests = Vec::new();
    for path in files("tests/runtime") {
        let src = std::fs::read_to_string(&path).unwrap();
        // `// only: js ts` tests are about JavaScript; `// only: interp` ones about the limits (below)
        if src.lines().nth(1).is_some_and(|l| l.starts_with("// only:")) {
            continue;
        }
        tests.push(path);
        checked += 1;
    }
    assert!(checked >= 30);
    in_parallel(tests, |path| {
        let src = std::fs::read_to_string(path).unwrap();
        let expect = src.lines().next().and_then(|l| l.strip_prefix("// expect: ")).unwrap();
        let (code, at) = expect.trim().split_once(" at ").unwrap();
        let stdin = std::fs::read(path.with_extension("in")).unwrap_or_default();
        let want_stdout = std::fs::read_to_string(path.with_extension("out")).unwrap_or_default().replace("\r\n", "\n");
        let dir = fresh(&path.file_stem().unwrap().to_string_lossy());
        let file = std::fs::canonicalize(path).unwrap();
        let out = run(&dir, &file, &["--interp"], &[], &stdin);
        let _ = std::fs::remove_dir_all(&dir);
        // (a folder of its own: the program may create files)
        let dir = fresh(&path.file_stem().unwrap().to_string_lossy());
        let json = run(&dir, &file, &["--interp", "--json"], &[], &stdin);
        let _ = std::fs::remove_dir_all(&dir);
        let (err, label) = (stderr(&out), path.display());
        if out.status.code() != Some(101) {
            return Some(format!("{label}: exit code {:?}; stderr:\n{err}", out.status.code()));
        }
        if !err.contains(&format!("runtime error[{code}]")) || !err.contains(&format!(":{at}\n")) {
            return Some(format!("{label}: expected {code} at {at}, stderr was:\n{err}"));
        }
        if stdout(&out) != want_stdout {
            return Some(format!("{label}: stdout was\n{}", stdout(&out)));
        }
        let j = stderr(&json);
        if !(j.contains(&format!("\"code\":\"{code}\""))
            && j.contains("\"runtime\":true")
            && j.contains(&format!("\"line\":{}", at.split(':').next().unwrap())))
        {
            return Some(format!("{label} --json: {j}"));
        }
        None
    });
}

// ---- limits ---------------------------------------------------------------------------------------------------

#[test]
fn every_limit_stops_the_program_with_its_code_and_exit_code() {
    let mut seen = Vec::new();
    for path in files("tests/runtime") {
        let src = std::fs::read_to_string(&path).unwrap();
        if !src.lines().nth(1).is_some_and(|l| l.trim() == "// only: interp") {
            continue;
        }
        let expect = src.lines().next().and_then(|l| l.strip_prefix("// expect: ")).unwrap();
        let (code, at) = expect.trim().split_once(" at ").unwrap();
        let flags = flags_of(&src);
        let flags: Vec<&str> = flags.iter().map(String::as_str).collect();
        let dir = fresh("limit");
        let file = std::fs::canonicalize(&path).unwrap();
        let out = run(&dir, &file, &flags, &[], b"");
        let json = run(&dir, &file, &[&flags[..], &["--json"][..]].concat(), &[], b"");
        let _ = std::fs::remove_dir_all(&dir);
        let label = path.display();
        assert_eq!(out.status.code(), Some(exit_of(code)), "{label}: {}", stderr(&out));
        assert!(stderr(&out).contains(&format!("runtime error[{code}]")), "{label}: {}", stderr(&out));
        // (the position of the time limit depends on the moment the clock ran out)
        if code != "E0359" {
            assert!(stderr(&out).contains(&format!(":{at}\n")), "{label}: expected position {at}:\n{}", stderr(&out));
        }
        let want = std::fs::read_to_string(path.with_extension("out")).unwrap_or_default();
        assert_eq!(stdout(&out), want, "{label}: what the program printed before the limit");
        let j = stderr(&json);
        assert!(j.contains(&format!("\"code\":\"{code}\"")) && j.contains("\"runtime\":true"), "{label} --json: {j}");
        seen.push(code.to_string());
    }
    seen.sort();
    assert_eq!(seen, ["E0355", "E0356", "E0357", "E0358", "E0359"], "a test program for each limit");
}

#[test]
fn a_limit_flag_runs_the_program_in_the_interpreter() {
    let dir = fresh("limit-flag");
    let file = dir.join("spin.nyra");
    std::fs::write(&file, "fn main() {\n    var n = 0\n    while true {\n        n += 1\n    }\n}\n").unwrap();
    // no --interp: --fuel implies it
    let out = run(&dir, &file, &["--fuel", "5000"], &[], b"");
    assert_eq!(out.status.code(), Some(120), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("runtime error[E0355]: step limit reached: the program ran more than 5000 steps"),
        "{}",
        stderr(&out)
    );
    // sizes: 4k, 1M ... ; bad values are usage errors
    let out = run(&dir, &file, &["--max-memory", "banana"], &[], b"");
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("is not a size"), "{}", stderr(&out));
    let out = run(&dir, &file, &["--max-depth", "5000000"], &[], b"");
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("--max-depth is at most"), "{}", stderr(&out));
    // the interpreter is not a target
    let out = run(&dir, &file, &["--interp", "--js"], &[], b"");
    assert_eq!(out.status.code(), Some(2));
    let out = nyra().current_dir(&dir).args(["build", "--sandbox"]).arg(&file).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("for `nyra run`"), "{}", stderr(&out));
}

#[test]
fn sleeping_in_the_interpreter_does_not_wait() {
    let dir = fresh("sleep");
    let file = dir.join("sleep.nyra");
    std::fs::write(
        &file,
        "use time\nfn main() {\n    let t = time.mono_ms()\n    time.sleep_ms(60000)\n    print(time.mono_ms() - t >= 60000.0)\n}\n",
    )
    .unwrap();
    let started = std::time::Instant::now();
    let out = run(&dir, &file, &["--interp"], &[], b"");
    assert_eq!(stdout(&out), "true\n", "{}", stderr(&out));
    assert!(started.elapsed().as_secs() < 20, "a sleep of a minute must not take a minute");
}

#[test]
fn the_interpreter_runs_programs_that_the_limits_allow() {
    // lots of work, no limit hit: 3 million loop rounds in the default fuel
    let dir = fresh("work");
    let file = dir.join("work.nyra");
    std::fs::write(
        &file,
        "fn main() {\n    var total = 0\n    for i in 0..3000000 {\n        total += i % 7\n    }\n    print(total)\n    let big = \"x\".repeat(1000000)\n    print(big.len())\n}\n",
    )
    .unwrap();
    let out = run(&dir, &file, &["--sandbox"], &[], b"");
    assert_eq!(stdout(&out), "8999994\n1000000\n", "{}", stderr(&out));
    assert_eq!(out.status.code(), Some(0));
}

// ---- capabilities ---------------------------------------------------------------------------------------------

const FS_AND_OS: &str = "use fs
use os

fn load(path: str) -> str = fs.read(path)
fn count(path: str) -> int = load(path).len()
fn plain(x: int) -> int = x + 1

fn main() {
    print(os.args())
    fs.write(\"note.txt\", \"hello\")
    print(count(\"note.txt\"), plain(1))
}
";

fn check(dir: &Path, file: &Path, flags: &[&str]) -> (i32, Json) {
    let out = nyra().current_dir(dir).args(["check", "--json"]).args(flags).arg(file).output().unwrap();
    let text = stdout(&out);
    (out.status.code().unwrap(), Json::parse(text.trim()).unwrap_or_else(|e| panic!("not JSON ({e}): {text}")))
}

fn codes(json: &Json) -> Vec<String> {
    json.get("errors")
        .and_then(|e| e.as_array())
        .unwrap()
        .iter()
        .map(|e| e.get("code").and_then(Json::as_str).unwrap().to_string())
        .collect()
}

#[test]
fn a_run_without_the_capability_cannot_use_the_module() {
    let dir = scratch("caps");
    let file = dir.join("caps.nyra");
    std::fs::write(&file, FS_AND_OS).unwrap();

    // everything is granted by default: plain `nyra run` keeps working
    let (code, json) = check(&dir, &file, &[]);
    assert_eq!((code, codes(&json)), (0, Vec::<String>::new()));

    // --sandbox grants nothing: one error per module, naming the module, the capability and the flag
    let (code, json) = check(&dir, &file, &["--sandbox"]);
    assert_eq!(code, 1);
    assert_eq!(codes(&json), ["E0290", "E0290"]);
    let errors = json.get("errors").and_then(|e| e.as_array()).unwrap();
    let first = errors[0].get("message").and_then(Json::as_str).unwrap();
    assert!(first.contains("module `fs` needs the capability `fs`") && first.contains("not granted"), "{first}");
    assert_eq!((errors[0].get("line").and_then(Json::as_u64), errors[0].get("col").and_then(Json::as_u64)), (Some(1), Some(5)));
    let hint = errors[0].get("hint").and_then(Json::as_str).unwrap();
    assert!(hint.contains("--allow fs"), "{hint}");
    assert!(errors[1].get("message").and_then(Json::as_str).unwrap().contains("module `os` needs the capability `os`"));

    // --allow narrows the default too (and works without --sandbox)
    let (_, json) = check(&dir, &file, &["--allow", "fs"]);
    assert_eq!(codes(&json), ["E0290"]);
    let message = json.get("errors").and_then(|e| e.as_array()).unwrap()[0].get("message").and_then(Json::as_str).unwrap().to_string();
    assert!(message.contains("module `os`") && message.contains("this run grants fs"), "{message}");
    for flags in [
        &["--allow", "fs,os"][..],
        &["--allow", "fs", "--allow", "os"],
        &["--allow=fs,os"],
        &["--sandbox", "--allow", "all"],
        &["--allow", "all"],
    ] {
        let (code, json) = check(&dir, &file, flags);
        assert_eq!((code, codes(&json)), (0, Vec::<String>::new()), "{flags:?}");
    }

    // a typo in a capability is a usage error that says what exists
    let out = nyra().current_dir(&dir).args(["check", "--allow", "inputs"]).arg(&file).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("unknown capability `inputs`") && stderr(&out).contains("did you mean `input`"), "{}", stderr(&out));

    // pure modules need nothing
    let pure = dir.join("pure.nyra");
    std::fs::write(
        &pure,
        "use math\nuse text\nuse json\nuse time\nuse random\nfn main() {\n    print(text.fixed(math.sqrt(2.0), 2))\n}\n",
    )
    .unwrap();
    let (code, _) = check(&dir, &pure, &["--sandbox"]);
    assert_eq!(code, 0);

    // `run` and `test` refuse it as well, before anything runs
    let out = run(&dir, &file, &["--sandbox"], &[], b"");
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("error[E0290]"), "{}", stderr(&out));
    assert!(!dir.join("note.txt").exists(), "nothing ran");
    let out = nyra().current_dir(&dir).args(["test", "--sandbox"]).arg(&file).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("error[E0290]"), "{}", stderr(&out));
}

#[test]
fn a_sandboxed_program_gets_what_it_is_allowed() {
    let dir = fresh("granted");
    let file = dir.join("caps.nyra");
    std::fs::write(&file, FS_AND_OS).unwrap();
    let out = run(&dir, &file, &["--sandbox", "--allow", "fs,os"], &["a".to_string(), "b c".to_string()], b"");
    assert_eq!(stdout(&out), "[\"a\", \"b c\"]\n5 2\n", "{}", stderr(&out));
    assert_eq!(std::fs::read_to_string(dir.join("note.txt")).unwrap(), "hello");

    // standard input is a capability too
    let reader = dir.join("reader.nyra");
    std::fs::write(&reader, "use input\nfn main() {\n    print(input.line().upper())\n}\n").unwrap();
    let out = run(&dir, &reader, &["--sandbox"], &[], b"hi\n");
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("module `input` needs the capability `input`"), "{}", stderr(&out));
    let out = run(&dir, &reader, &["--sandbox", "--allow", "input"], &[], b"hi\n");
    assert_eq!(stdout(&out), "HI\n", "{}", stderr(&out));
}

#[test]
fn the_sandbox_keeps_files_in_the_working_folder() {
    let dir = fresh("confined");
    let outside = dir.join("outside");
    std::fs::create_dir_all(dir.join("work")).unwrap();
    let file = dir.join("escape.nyra");
    std::fs::write(
        &file,
        "use fs\nfn main() {\n    fs.mkdir(\"inner\")\n    fs.write(\"inner/ok.txt\", \"fine\")\n    print(fs.read(\"inner/ok.txt\"))\n    fs.write(\"../escaped.txt\", \"no\")\n}\n",
    )
    .unwrap();
    let work = dir.join("work");
    let out = run(&work, &file, &["--sandbox", "--allow", "fs"], &[], b"");
    assert_eq!(out.status.code(), Some(101), "{}", stderr(&out));
    assert_eq!(stdout(&out), "fine\n");
    assert!(stderr(&out).contains("runtime error[E0340]") && stderr(&out).contains("outside the working folder"), "{}", stderr(&out));
    assert!(!dir.join("escaped.txt").exists() && !outside.exists());
    // an absolute path is outside too; without --sandbox the interpreter is as free as a compiled program
    let abs = dir.join("abs.nyra");
    let target = dir.join("abs.txt").to_string_lossy().replace('\\', "/");
    std::fs::write(&abs, format!("use fs\nfn main() {{\n    fs.write(\"{target}\", \"x\")\n}}\n")).unwrap();
    let out = run(&work, &abs, &["--sandbox", "--allow", "fs"], &[], b"");
    assert_eq!(out.status.code(), Some(101));
    assert!(!dir.join("abs.txt").exists());
    let out = run(&work, &abs, &["--interp"], &[], b"");
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(dir.join("abs.txt").exists());
}

#[test]
fn outline_lists_the_effects_of_each_function() {
    let dir = scratch("effects");
    let file = dir.join("effects.nyra");
    std::fs::write(
        &file,
        "use fs
use os
use input
use math

fn read_all(path: str) -> str = fs.read(path)
fn shout(path: str) -> str = read_all(path).upper()
fn root(x: float) -> float = math.sqrt(x)
fn pick() -> str = \"{os.args()}\"
fn first() -> str = input.line()
fn plain(x: int) -> int = x + 1
fn mixed(path: str) -> str {
    let a = shout(path)
    ret a + first()
}
fn main() {
    print(mixed(\"a\"), plain(1), root(2.0), pick())
}
",
    )
    .unwrap();
    let out = nyra().current_dir(&dir).args(["outline", "--json"]).arg(&file).output().unwrap();
    let json = Json::parse(stdout(&out).trim()).unwrap();
    let list = |j: &Json| -> Vec<String> { j.as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect() };
    assert_eq!(list(json.get("capabilities").unwrap()), ["fs", "input", "os"]);
    let effects = |name: &str| -> Vec<String> {
        let sym = json
            .get("symbols")
            .and_then(Json::as_array)
            .unwrap()
            .iter()
            .find(|s| s.get("name").and_then(Json::as_str) == Some(name))
            .unwrap_or_else(|| panic!("no symbol {name}"));
        list(sym.get("effects").unwrap_or_else(|| panic!("{name} has no effects")))
    };
    assert_eq!(effects("read_all"), ["fs"]);
    // through calls
    assert_eq!(effects("shout"), ["fs"]);
    assert_eq!(effects("mixed"), ["fs", "input"]);
    // inside a string, and a pure module
    assert_eq!(effects("pick"), ["os"]);
    assert_eq!(effects("root"), Vec::<String>::new());
    assert_eq!(effects("plain"), Vec::<String>::new());
    assert_eq!(effects("main"), ["fs", "input", "os"]);
    // the text form says it too
    let out = nyra().current_dir(&dir).arg("outline").arg(&file).output().unwrap();
    let text = stdout(&out);
    assert!(text.contains("fn shout(path: str) -> str  [needs fs]"), "{text}");
    assert!(text.contains("fn mixed(path: str) -> str  [needs fs, input]"), "{text}");
    assert!(!text.contains("fn plain(x: int) -> int  ["), "{text}");
}

// ---- property examples ---------------------------------------------------------------------------------------

#[test]
fn a_property_example_reports_the_first_input_that_fails() {
    let dir = scratch("properties");
    let file = dir.join("prop.nyra");
    std::fs::write(
        &file,
        "fn half(n: int) -> int = n / 2
ex for n in 0..200: half(n) >= 0
ex for n in 0..200: half(n) * 2 <= n, half(n) * 2 >= n - 1
ex for n in 10..0 step -3: half(n) > 0
ex for n in -5..6 step 5: half(n) != 0, half(n) < 100

fn main() {
    print(half(9))
}
",
    )
    .unwrap();
    let out = nyra().current_dir(&dir).args(["test", "--json"]).arg(&file).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let json = Json::parse(stdout(&out).trim()).unwrap();
    // one example per condition: 1 + 2 + 1 + 2
    assert_eq!(json.get("examples").and_then(Json::as_u64), Some(6));
    let errors = json.get("errors").and_then(Json::as_array).unwrap();
    let messages: Vec<&str> = errors.iter().map(|e| e.get("message").and_then(Json::as_str).unwrap()).collect();
    // `half(-5)` is -2 (division truncates toward zero) ... the properties that are false fail on the first input
    assert!(messages.iter().any(|m| m.contains("example `half(n) != 0` is false for n = 0")), "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("example `half(n) * 2 >= n - 1`") || m.contains("half(n) > 0")), "{messages:?}");
    let e = errors.iter().find(|e| e.get("message").and_then(Json::as_str).unwrap().contains("half(n) != 0")).unwrap();
    assert_eq!(e.get("code").and_then(Json::as_str), Some("E0250"));
    assert_eq!(e.get("actual").and_then(Json::as_str), Some("0"));
    assert_eq!(e.get("expected").and_then(Json::as_str), Some("anything but 0"));
    assert!(e.get("hint").and_then(Json::as_str).unwrap().contains("n = 0"), "{e:?}");

    // a runtime error for one input is E0251 and says which
    let boom = dir.join("boom.nyra");
    std::fs::write(&boom, "fn inv(n: int) -> int = 100 / n\nex for n in -3..4: inv(n) != 0\n\nfn main() {\n    print(inv(4))\n}\n")
        .unwrap();
    let out = nyra().current_dir(&dir).args(["check", "--json"]).arg(&boom).output().unwrap();
    let json = Json::parse(stdout(&out).trim()).unwrap();
    let e = &json.get("errors").and_then(Json::as_array).unwrap()[0];
    assert_eq!(e.get("code").and_then(Json::as_str), Some("E0251"));
    assert!(e.get("message").and_then(Json::as_str).unwrap().contains("runtime error E0241 for n = 0"), "{e:?}");

    // a property that holds passes, and never reaches the generated program
    let good = dir.join("good.nyra");
    std::fs::write(&good, "fn sq(x: int) -> int = x * x\nex for n in -100..101: sq(n) >= 0, sq(n) == sq(-n)\nex sq(3) == 9\n\nfn main() {\n    print(sq(5))\n}\n").unwrap();
    let out = nyra().current_dir(&dir).arg("test").arg(&good).output().unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stderr(&out).contains("3 examples: 3 passed"), "{}", stderr(&out));
    let out = run(&dir, &good, &["--interp"], &[], b"");
    assert_eq!(stdout(&out), "25\n");
}
