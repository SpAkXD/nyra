//! Keeps the docs honest. In `README.md` and `docs/AI_GUIDE.md` every fenced code block marked
//! `rust` is Nyra code (GitHub has no Nyra grammar, so Rust highlighting is borrowed). Each of those
//! blocks must type-check; a block without `fn main` gets an empty one appended first.

use std::process::Command;

/// The contents of every ```rust fenced block in a markdown file.
fn rust_blocks(markdown: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current: Option<String> = None;
    for line in markdown.lines() {
        match (current.as_mut(), line.trim_end()) {
            (None, "```rust") => current = Some(String::new()),
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

#[test]
fn nyra_code_blocks_in_the_docs_compile() {
    let dir = std::env::temp_dir().join("nyra-docs-test");
    std::fs::create_dir_all(&dir).unwrap();

    for file in ["README.md", "docs/AI_GUIDE.md"] {
        let markdown = std::fs::read_to_string(file).unwrap();
        let blocks = rust_blocks(&markdown);
        assert!(!blocks.is_empty(), "{file}: no ```rust code blocks found");

        for (i, code) in blocks.iter().enumerate() {
            let source = if code.contains("fn main") { code.clone() } else { format!("{code}\nfn main() {{\n}}\n") };
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
}
