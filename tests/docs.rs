//! Keeps the docs honest: every Nyra code block in them must type-check.
//!
//! - `README.md` and `docs/AI_GUIDE.md` mark Nyra blocks `rust` (GitHub has no Nyra grammar, so
//!   Rust highlighting is borrowed).
//! - `docs/SPEC.md` marks them `nyra`: the spec is pasted into prompts as plain text, where `rust`
//!   would name the wrong language.
//!
//! A block without `fn main` becomes a program: its top-level `fn` and `struct` definitions and its
//! `ex` examples stay at the top level and every other line goes into an appended `fn main()`.

use std::process::Command;

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

/// A code block as a whole program: the lines of top-level `fn` and `struct` definitions stay at
/// the top level, all other lines go into `fn main()`.
fn program(code: &str) -> String {
    if code.contains("fn main") {
        return code.to_string();
    }
    let (mut items, mut body) = (String::new(), String::new());
    let mut in_item = false;
    for line in code.lines() {
        let starts_item = line.starts_with("fn ") || line.starts_with("struct ") || line.starts_with("ex ");
        if starts_item || in_item {
            items.push_str(line);
            items.push('\n');
            // a definition whose first line ends with `{` goes on until the `}` in column 0
            let code_part = line.split("//").next().unwrap_or("").trim_end();
            in_item = if starts_item { code_part.ends_with('{') } else { !line.starts_with('}') };
        } else {
            if !line.is_empty() {
                body.push_str("    ");
                body.push_str(line);
            }
            body.push('\n');
        }
    }
    format!("{items}\nfn main() {{\n{body}}}\n")
}

#[test]
fn statements_of_a_block_go_into_main() {
    let code = "struct P {\n    x: int\n}\nfn f(p: P) -> int { // f\n    ret p.x\n}\nfn g() = print(1)\nlet p = P(x: 1)\nif true {\n    g()\n}\n";
    let expected = "struct P {\n    x: int\n}\nfn f(p: P) -> int { // f\n    ret p.x\n}\nfn g() = print(1)\n\nfn main() {\n    let p = P(x: 1)\n    if true {\n        g()\n    }\n}\n";
    assert_eq!(program(code), expected);
    assert_eq!(program("fn main() {\n}\n"), "fn main() {\n}\n");
}

#[test]
fn nyra_code_blocks_in_the_docs_compile() {
    let dir = std::env::temp_dir().join("nyra-docs-test");
    std::fs::create_dir_all(&dir).unwrap();

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
        }
    }

    // the spec goes into prompts: a `rust` block there would tell a model the code is Rust
    let spec = std::fs::read_to_string("docs/SPEC.md").unwrap();
    assert!(!spec.contains("```rust"), "docs/SPEC.md: mark Nyra code blocks `nyra`, not `rust`");
}
