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

            let out = Command::new(env!("CARGO_BIN_EXE_nyra")).args(["check", "--strict", "--json"]).arg(&path).output().unwrap();
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

/// `nyra check --strict --json` of a program: (passed, the whole JSON text). Strict, so a mistake
/// the compiler would repair in memory (`and` for `&&`) still counts as not compiling.
fn check_source(name: &str, source: &str) -> (bool, String) {
    let dir = std::env::temp_dir().join("nyra-card-test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.nyra"));
    std::fs::write(&path, source).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_nyra")).args(["check", "--strict", "--json"]).arg(&path).output().unwrap();
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
        assert_eq!(
            sorted(card_names(&card, label)),
            sorted(hint_names(&json)),
            "the card's `{label}` methods differ from the compiler's"
        );
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
        ("`null`", "let a = null\n"),
        ("generics", "fn id<T>(x: T) -> T = x\n"),
        ("closures", "let f = x => x + 1\n"),
        ("classes and `impl`", "class P {\n    x: int\n}\n"),
        ("`elif`", "let a = 1\nif a == 1 { print(1) } elif a == 2 { print(2) }\n"),
        ("`and`/`or`/`not`", "let a = true and false\n"),
        ("`i++`", "var i = 0\ni++\n"),
        ("`reduce`", "let xs = [1]\nprint(xs.reduce(0, (a, b) => a + b))\n"),
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
    const MAX_CHARS: usize = 3150; // measured: 3,102 characters = 1,399 tokens on claude-sonnet-5-5
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

// ---- docs/WEB_CARD.md: the web card, `use web` for websites (`nyra_spec` with `topic: "web"`), read with the agent card ----

/// The web card as a model sees it: the file without its metadata comment.
fn web_card() -> String {
    let file = std::fs::read_to_string("docs/WEB_CARD.md").unwrap().replace("\r\n", "\n");
    let (comment, body) = file.split_once("-->\n").expect("docs/WEB_CARD.md starts with a metadata comment");
    assert!(comment.starts_with("<!--"), "docs/WEB_CARD.md must start with its metadata comment");
    body.to_string()
}

/// The quoted names of local images in a program (`"hero.jpg"`), which the build looks up in `assets/`.
fn local_images(source: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for part in source.split('"').skip(1).step_by(2) {
        let image = [".jpg", ".jpeg", ".png", ".webp", ".svg", ".gif", ".avif"].iter().any(|e| part.to_lowercase().ends_with(e));
        if image && !part.contains("://") && !part.contains(' ') && !out.iter().any(|o| o == part) {
            out.push(part.to_string());
        }
    }
    out
}

#[test]
fn the_web_card_example_builds() {
    let card = web_card();
    let blocks = blocks(&card, "nyra");
    assert_eq!(blocks.len(), 1, "the web card has exactly one example program");
    let source = &blocks[0];
    assert!(source.starts_with("use web\n"), "the web card's example is a site");
    // the site's folder: the program and, in `assets/`, the images it names
    let dir = std::env::temp_dir().join("nyra-web-card-test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    let images = local_images(source);
    assert!(!images.is_empty(), "the example shows a local image");
    let jpeg = std::fs::read("tests/warnings/assets/counter.jpg").unwrap();
    for img in &images {
        std::fs::write(dir.join("assets").join(img), &jpeg).unwrap();
    }
    std::fs::write(dir.join("site.nyra"), source).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_nyra"))
        .args(["build", "--strict", "--json", "--image-tool", "none"])
        .arg(dir.join("site.nyra"))
        .arg("-o")
        .arg(dir.join("dist"))
        .output()
        .unwrap();
    let json = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(out.status.success(), "the web card's example does not build\n{source}\n{json}{}", String::from_utf8_lossy(&out.stderr));
    // cleanly: the only warning allowed is the missing image tool (E0386), which this test turns off
    let codes: Vec<&str> = json.split("\"code\":\"").skip(1).map(|s| &s[..5]).collect();
    assert!(codes.iter().all(|c| *c == "E0386"), "the web card's example builds with warnings {codes:?}\n{json}");
    // and it is a live site: its buttons send messages and its view is redrawn
    let page = std::fs::read_to_string(dir.join("dist").join("index.html")).unwrap();
    for part in ["data-on=", "data-live=", "class=\"nav\"", "class=\"hero hero-in\"", "data-anim=\"rise\""] {
        assert!(page.contains(part), "the example's page has no `{part}`");
    }
}

/// The parameters of each public function of the web library (`src/std/web.nyra`), in order.
fn web_functions() -> Vec<(String, Vec<String>)> {
    let src = std::fs::read_to_string("src/std/web.nyra").unwrap().replace("\r\n", "\n");
    let mut out = Vec::new();
    for part in src.split("\nfn ").skip(1) {
        let name: String = part.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
        if name.starts_with('_') {
            continue;
        }
        // the parameter list, up to the `)` that closes it (types have parentheses and brackets of their own)
        let mut depth = 0;
        let mut params = vec![String::new()];
        for c in part[name.len() + 1..].chars() {
            match c {
                '(' | '[' => depth += 1,
                ')' | ']' if depth == 0 => break,
                ')' | ']' => depth -= 1,
                ',' if depth == 0 => {
                    params.push(String::new());
                    continue;
                }
                _ => {}
            }
            params.last_mut().unwrap().push(c);
        }
        let names: Vec<String> =
            params.iter().map(|p| p.trim().split(':').next().unwrap().trim().to_string()).filter(|p| !p.is_empty()).collect();
        out.push((name, names));
    }
    assert!(out.len() > 30, "only {} web functions found", out.len());
    out
}

/// The calls of a line of the card's element list, `name(args)`, with the names before the `(`: `h1 h2 h3(text)`
/// shows three functions.
fn shown_calls(line: &str) -> Vec<(Vec<String>, String)> {
    let mut out = Vec::new();
    let mut rest = line;
    while let Some(open) = rest.find('(') {
        let mut names: Vec<String> = rest[..open]
            .split_whitespace()
            .rev()
            .take_while(|w| !w.is_empty() && w.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
            .map(String::from)
            .collect();
        names.reverse();
        let mut depth = 0;
        let mut end = rest.len();
        for (i, c) in rest[open + 1..].char_indices() {
            match c {
                '(' | '[' => depth += 1,
                ')' | ']' if depth == 0 => {
                    end = open + 1 + i;
                    break;
                }
                ')' | ']' => depth -= 1,
                _ => {}
            }
        }
        if !names.is_empty() {
            out.push((names, rest[open + 1..end].to_string()));
        }
        rest = &rest[(end + 1).min(rest.len())..];
    }
    out
}

#[test]
fn the_web_card_lists_the_web_functions_as_they_are() {
    let card = web_card();
    let functions = web_functions();
    let list = card.split("## Elements\n").nth(1).and_then(|s| s.split("\n## ").next()).expect("no `## Elements` section");
    let mut shown: Vec<String> = Vec::new();
    for line in list.lines().filter(|l| !l.starts_with("```")) {
        for (names, args) in shown_calls(line) {
            // what is inside quotes is a value (`css(".x { color: red }")`), not parameters
            let args = args.split('"').step_by(2).collect::<Vec<_>>().join("\"\"");
            // `h1 h2 h3(...)`: every name that is a web function; else the last word is the call (`days 1-7`)
            let fs: Vec<&String> = names.iter().filter(|n| functions.iter().any(|(f, _)| f == *n)).collect();
            let fs = if fs.is_empty() { vec![names.last().unwrap()] } else { fs };
            for f in fs {
                let have = &functions
                    .iter()
                    .find(|(n, _)| n == f)
                    .unwrap_or_else(|| panic!("the web card shows `{f}(...)`, which the library does not have"))
                    .1;
                // named: each `word:` is a parameter of that name
                for w in args.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == ':')) {
                    if let Some(p) = w.strip_suffix(':').filter(|p| !p.is_empty() && !p.contains(':')) {
                        assert!(
                            have.iter().any(|h| h == p),
                            "the web card gives `{f}` the option `{p}:`, which it does not have: {have:?}"
                        );
                    }
                }
                // positional: the arguments before the first `name:`, in order
                let mut positional: Vec<String> = Vec::new();
                let mut depth = 0;
                let mut cur = String::new();
                for c in args.chars() {
                    match c {
                        '(' | '[' => depth += 1,
                        ')' | ']' => depth -= 1,
                        _ => {}
                    }
                    if c == ',' && depth == 0 {
                        positional.push(std::mem::take(&mut cur).trim().to_string());
                    } else {
                        cur.push(c);
                    }
                }
                positional.push(cur.trim().to_string());
                positional.retain(|p| !p.is_empty() && !p.contains(':') && !p.starts_with('"'));
                assert!(
                    positional.len() <= have.len(),
                    "the web card gives `{f}` {} positional parameters: {have:?}",
                    positional.len()
                );
                for (i, p) in positional.iter().enumerate() {
                    // a placeholder may name the value (`rows`, `[(label, href)]`), but not another parameter
                    let plain = p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
                    assert!(
                        !plain || have[i] == *p || !have.contains(p),
                        "the web card shows `{p}` as parameter {} of `{f}`, which is `{}`: {have:?}",
                        i + 1,
                        have[i]
                    );
                }
                shown.push(f.clone());
            }
        }
    }
    // every public web function is on the card, but the helpers left out on purpose
    let left_out = ["html", "maps", "year"];
    for (f, _) in &functions {
        if !left_out.contains(&f.as_str()) {
            // a call shown with only a block (`stack {}`) counts too
            let block = list.contains(&format!("{f} {{")) && !list.contains(&format!("_{f} {{"));
            assert!(shown.contains(f) || block || card.contains(&format!("`{f}(")), "the web function `{f}` is not on the web card");
        }
    }
}

#[test]
fn the_web_card_keeps_its_token_budget() {
    // As for the agent card: `python tools/card_tokens.py --card web --write` measures it on Claude's tokenizer and
    // writes the counts into the header; this test checks the header and that the card did not grow since.
    const BUDGET: usize = 1400;
    const MAX_CHARS: usize = 3050; // measured: 2,967 characters = 1,400 tokens on claude-sonnet-5-5
    let file = std::fs::read_to_string("docs/WEB_CARD.md").unwrap().replace("\r\n", "\n");
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
    let chars = web_card().chars().count();
    assert!(
        chars <= MAX_CHARS,
        "the web card has {chars} characters, more than the {MAX_CHARS} that fit the {BUDGET}-token budget: cut something (or re-measure with python tools/card_tokens.py --card web and move MAX_CHARS)"
    );
}
