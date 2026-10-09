//! Keeps the docs honest: every Nyra code block in them must type-check.
//!
//! - `README.md` and `docs/AI_GUIDE.md` mark Nyra blocks `rust` (GitHub has no Nyra grammar, so
//!   Rust highlighting is borrowed).
//! - `docs/SPEC.md` marks them `nyra`: the spec is pasted into prompts as plain text, where `rust`
//!   would name the wrong language.
//!
//! A block is checked as written (`use` lines included): without `fn main` it is a script, whose top-level statements run in
//! order and whose top-level variables the functions can use. A block of definitions alone gets an
//! empty `fn main()`.

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
        format!("{code}
fn main() {{
}}
")
    }
}

#[test]
fn definitions_alone_get_a_main() {
    assert_eq!(program("fn g() = print(1)
"), "fn g() = print(1)

fn main() {
}
");
    assert_eq!(program("let p = 1
fn g() = print(p)
g()
"), "let p = 1
fn g() = print(p)
g()
");
    assert_eq!(program("fn main() {
}
"), "fn main() {
}
");
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
