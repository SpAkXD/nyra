//! Keeps the docs honest: every Nyra code block in them must type-check, and run without a
//! runtime error (on JavaScript when Node.js is installed, else natively when a C compiler is).
//!
//! - `README.md` and `docs/AI_GUIDE.md` mark Nyra blocks `rust` (GitHub has no Nyra grammar, so
//!   Rust highlighting is borrowed).
//! - `docs/SPEC.md` marks them `nyra`: the spec is pasted into prompts as plain text, where `rust`
//!   would name the wrong language.
//!
//! A block is checked as written (`use` lines included): without `fn main` it is a script, whose top-level statements run in
//! order and whose top-level variables the functions can use. A block of definitions alone gets an
//! empty `fn main()`.

mod common;

use std::process::{Command, Stdio};

/// The contents of every fenced block marked `lang` in a markdown file.
fn blocks(markdown: &str, lang: &str) -> Vec<String> {
    let open = format!("```{lang}");
    let mut blocks = Vec::new();
    let mut current: Option<String> = None;
    for line in markdown.lines() {
        match (current.as_mut(), line.trim_end()) {
            (None, text) if text == open => current = Some(String::new()),
            (Some(_), "```") => blocks.extend(current.take()),
            (Some(code), text) => {
                code.push_str(text);
                code.push('\n');
            }
            (None, _) => {}
        }
    }
    blocks
}

/// A code block as a whole program: a script as written, or definitions only, which get an empty
/// `fn main()`.
fn program(code: &str) -> String {
    if code.contains("fn main") {
        return code.to_string();
    }
    // a statement starts in column 0 and is not a definition, a comment or the end of one
    let statement = |l: &str| {
        !l.is_empty() && !l.starts_with([' ', '}', '/']) && !["fn ", "struct ", "ex ", "use "].iter().any(|k| l.starts_with(k))
    };
    if code.lines().any(statement) {
        code.to_string()
    } else {
        format!(
            "{code}
fn main() {{
}}
"
        )
    }
}

#[test]
fn definitions_alone_get_a_main() {
    assert_eq!(
        program(
            "fn g() = print(1)
"
        ),
        "fn g() = print(1)

fn main() {
}
"
    );
    assert_eq!(
        program(
            "let p = 1
fn g() = print(p)
g()
"
        ),
        "let p = 1
fn g() = print(p)
g()
"
    );
    assert_eq!(
        program(
            "fn main() {
}
"
        ),
        "fn main() {
}
"
    );
}

#[test]
fn nyra_code_blocks_in_the_docs_compile() {
    let dir = std::env::temp_dir().join("nyra-docs-test");
    std::fs::create_dir_all(&dir).unwrap();
    let works = |cmd: &str| Command::new(cmd).arg("--version").output().is_ok();
    let backend: Option<&[&str]> = if works("node") {
        Some(&["--js"])
    } else if std::env::var("NYRA_CC").is_ok() || ["gcc", "clang", "cc", "tcc"].iter().any(|c| works(c)) {
        Some(&[])
    } else {
        common::missing("no Node.js and no C compiler to run the code blocks");
        None
    };

    for (file, lang) in [("README.md", "rust"), ("docs/AI_GUIDE.md", "rust"), ("docs/SPEC.md", "nyra")] {
        let markdown = std::fs::read_to_string(file).unwrap();
        let blocks = blocks(&markdown, lang);
        assert!(!blocks.is_empty(), "{file}: no ```{lang} code blocks found");

        for (i, code) in blocks.iter().enumerate() {
            let source = program(code);
            let path = dir.join(format!("{}-{}.nyra", file.replace(['/', '.'], "_"), i + 1));
            std::fs::write(&path, &source).unwrap();

            let out = Command::new(env!("CARGO_BIN_EXE_nyra")).args(["check", "--json"]).arg(&path).output().unwrap();
            assert!(
                out.status.success(),
                "{file}: code block {} does not compile\n{source}\n{}",
                i + 1,
                String::from_utf8_lossy(&out.stdout)
            );

            // ... and runs, in a folder of its own (it may write files), without arguments or input
            let Some(flags) = backend else { continue };
            let run_dir = dir.join(format!("run-{}-{}", file.replace(['/', '.'], "_"), i + 1));
            let _ = std::fs::remove_dir_all(&run_dir);
            std::fs::create_dir_all(&run_dir).unwrap();
            let out = Command::new(env!("CARGO_BIN_EXE_nyra"))
                .current_dir(&run_dir)
                .arg("run")
                .arg(&path)
                .args(flags)
                .stdin(Stdio::null())
                .output()
                .unwrap();
            let _ = std::fs::remove_dir_all(&run_dir);
            // a program may end itself with `os.exit(n)` (a usage message without arguments)
            let ok = out.status.success() || (source.contains("os.exit(") && out.status.code() != Some(101));
            assert!(ok, "{file}: code block {} fails when it runs\n{source}\n{}", i + 1, String::from_utf8_lossy(&out.stderr));
        }
    }

    // the spec goes into prompts: a `rust` block there would tell a model the code is Rust
    let spec = std::fs::read_to_string("docs/SPEC.md").unwrap();
    assert!(!spec.contains("```rust"), "docs/SPEC.md: mark Nyra code blocks `nyra`, not `rust`");
}

// ---- docs/AGENT_CARD.md: the compact spec that `nyra_spec` serves and `bench/run.py --spec card` sends ----

/// The card as a model sees it: the file without its metadata comment.
fn card() -> String {
    let file = std::fs::read_to_string("docs/AGENT_CARD.md").unwrap().replace("\r\n", "\n");
    let (comment, body) = file.split_once("-->\n").expect("docs/AGENT_CARD.md starts with a metadata comment");
    assert!(comment.starts_with("<!--"), "docs/AGENT_CARD.md must start with its metadata comment");
    body.to_string()
}

/// `nyra check --json` of a program: (passed, the whole JSON text).
fn check_source(name: &str, source: &str) -> (bool, String) {
    let dir = std::env::temp_dir().join("nyra-card-test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.nyra"));
    std::fs::write(&path, source).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_nyra")).args(["check", "--json"]).arg(&path).output().unwrap();
    (out.status.success(), String::from_utf8_lossy(&out.stdout).into_owned())
}

#[test]
fn the_agent_card_example_compiles_and_runs() {
    let card = card();
    let blocks = blocks(&card, "nyra");
    assert_eq!(blocks.len(), 1, "the card has exactly one example program");
    let source = &blocks[0];
    assert!(!source.contains("fn main"), "the card's example is a script");
    let (ok, json) = check_source("example", source);
    assert!(ok, "the card's example does not compile\n{json}");
    // its `ex` examples are evaluated: nyra test counts them
    let path = std::env::temp_dir().join("nyra-card-test").join("example.nyra");
    let out = Command::new(env!("CARGO_BIN_EXE_nyra")).arg("test").arg(&path).output().unwrap();
    assert!(out.status.success(), "nyra test fails on the card's example\n{}", String::from_utf8_lossy(&out.stdout));

    let expected = "0: pen\n1: ink\n3 [0, 1] [\"pen\": 2] 0.67\n[0, 4, 16] 3\n";
    let works = |cmd: &str| Command::new(cmd).arg("--version").output().is_ok();
    let mut backends: Vec<&[&str]> = Vec::new();
    if works("node") {
        backends.push(&["--js"]);
    }
    if std::env::var("NYRA_CC").is_ok() || ["gcc", "clang", "cc", "tcc"].iter().any(|c| works(c)) {
        backends.push(&[]);
    }
    if backends.is_empty() {
        common::missing("no Node.js and no C compiler to run the card's example");
    }
    for flags in backends {
        let out = Command::new(env!("CARGO_BIN_EXE_nyra")).arg("run").arg(&path).args(flags).stdin(Stdio::null()).output().unwrap();
        assert!(out.status.success(), "the card's example fails when it runs ({flags:?})\n{}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n"), expected, "output of the card's example ({flags:?})");
    }
}

/// The backticked words of a hint such as "the methods of `str` are `len` `chars`".
fn hint_names(json: &str) -> Vec<String> {
    let hint = json.split("\"hint\":\"").nth(1).and_then(|h| h.split('"').next()).unwrap_or_else(|| panic!("no hint in {json}"));
    let hint = hint.split(" are ").nth(1).unwrap_or_else(|| panic!("no name list in the hint: {hint}"));
    hint.split('`').skip(1).step_by(2).map(String::from).collect()
}

fn sorted(mut names: Vec<String>) -> Vec<String> {
    names.sort();
    names
}

/// The words in backticks of the card's list `- label: ...`.
fn card_names(card: &str, label: &str) -> Vec<String> {
    let line = card
        .lines()
        .find_map(|l| l.strip_prefix(&format!("- {label}: ")))
        .unwrap_or_else(|| panic!("no `- {label}:` list in the card"));
    line.split('`').skip(1).step_by(2).flat_map(|s| s.split(' ').map(String::from).collect::<Vec<_>>()).collect()
}

#[test]
fn the_agent_card_lists_exactly_the_names_the_compiler_has() {
    let card = card();
    // the compiler lists the names of a type or module when a program asks for one that does not exist
    let probes = [
        ("str", "let s = \"a\"\ns.nosuch()\n"),
        ("char", "let c = 'a'\nc.nosuch()\n"),
        ("array", "let xs = [1]\nxs.nosuch()\n"),
        ("map", "let m = [\"a\": 1]\nm.nosuch()\n"),
    ];
    for (label, source) in probes {
        let (ok, json) = check_source("names", source);
        assert!(!ok, "{label}: the probe should fail");
        assert_eq!(sorted(card_names(&card, label)), sorted(hint_names(&json)), "the card's `{label}` methods differ from the compiler's");
    }
    // modules: "- modules (`use math`): input `line lines all eof`; os `args env ...`; ..."
    let line = card.lines().find_map(|l| l.strip_prefix("- modules (`use math`): ")).expect("no modules list in the card");
    let mut seen = Vec::new();
    for part in line.split("; ") {
        let (module, names) = part.split_once(" `").unwrap_or_else(|| panic!("bad modules entry {part:?}"));
        let names: Vec<String> = names.trim_end_matches('`').split(' ').map(String::from).collect();
        let (ok, json) = check_source("names", &format!("use {module}\nlet a = {module}.nosuch()\n"));
        assert!(!ok, "{module}: the probe should fail");
        assert_eq!(sorted(names), sorted(hint_names(&json)), "the card's `{module}` names differ from the compiler's");
        seen.push(module.to_string());
    }
    // every standard module is on the card
    for module in ["input", "os", "fs", "json", "time", "random", "math", "text"] {
        assert!(seen.iter().any(|m| m == module), "module {module} is missing from the card");
    }
}

#[test]
fn what_the_agent_card_says_is_not_in_nyra_is_an_error() {
    let card = card();
    let not_in = card.split("## Not in Nyra\n").nth(1).and_then(|s| s.split("\n##").next()).expect("no `## Not in Nyra` section");
    let probes = [
        ("Tuples", "let a = (1, 2)\n"),
        ("enums", "enum Color { Red }\n"),
        ("`Option`", "let a: Option = 1\n"),
        ("`null`", "let a = null\n"),
        ("generics", "fn id<T>(x: T) -> T = x\n"),
        ("closures", "let f = x => x + 1\n"),
        ("methods on structs", "struct P { x: int }\nfn P.f(p: P) -> int = p.x\n"),
        ("`match`", "let a = 1\nmatch a { 1 => print(1) }\n"),
        ("`?:`", "let a = true ? 1 : 2\n"),
        ("`elif`", "let a = 1\nif a == 1 { print(1) } elif a == 2 { print(2) }\n"),
        ("`and`/`or`/`not`", "let a = true and false\n"),
        ("`i++`", "var i = 0\ni++\n"),
        ("`xs[a..b]`", "let xs = [1, 2, 3]\nprint(xs[0..2])\n"),
        ("sets", "let s = set()\n"),
        ("`reduce`", "let xs = [1]\nprint(xs.reduce(0, (a, b) => a + b))\n"),
        ("`find`", "let xs = [1]\nprint(xs.find(x => x > 0))\n"),
    ];
    for (word, source) in probes {
        assert!(not_in.contains(word), "the probe for {word} is not in the card's \"Not in Nyra\" list any more: update the probes");
        let (ok, json) = check_source("not", source);
        assert!(!ok, "the card says {word} is not in Nyra, but this compiles:\n{source}{json}");
    }
}

#[test]
fn the_agent_card_keeps_its_token_budget() {
    // The budget is in tokens on Claude's tokenizer (python tools/card_tokens.py measures it and writes the result into
    // the header). This test cannot call the API, so it checks the header and that the card did not grow past the size
    // that was measured: re-measure after any change, then move MAX_CHARS.
    const BUDGET: usize = 1400;
    const MAX_CHARS: usize = 3300; // measured: 3,287 characters = 1,398 tokens on claude-sonnet-5-5
    let file = std::fs::read_to_string("docs/AGENT_CARD.md").unwrap().replace("\r\n", "\n");
    let header: &str = file.split_once("-->\n").unwrap().0;
    assert!(header.contains("The card has a hard budget: a feature that needs card text must displace something."));
    let tokens = |model: &str| -> usize {
        let tail = header.split_once("TOKENS: ").expect("no TOKENS in the header").1;
        let at = tail.find(&format!(" on {model}")).unwrap_or_else(|| panic!("no count for {model} in the header"));
        let start = tail[..at].rfind(|c: char| !c.is_ascii_digit()).map_or(0, |i| i + 1);
        tail[start..at].parse().unwrap()
    };
    let sonnet = tokens("claude-sonnet-5-5");
    assert!(sonnet > 0 && sonnet <= BUDGET, "the header says {sonnet} tokens: the budget is {BUDGET}");
    assert!(tokens("claude-haiku-4-5") > 0);
    let chars = card().chars().count();
    assert!(
        chars <= MAX_CHARS,
        "the card has {chars} characters, more than the {MAX_CHARS} that fit the {BUDGET}-token budget: cut something (or re-measure with python tools/card_tokens.py and move MAX_CHARS)"
    );
}
