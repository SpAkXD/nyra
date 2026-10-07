# Contributing to Nyra

Thanks for helping. Nyra is small on purpose: every change should keep the language easy for an AI
to write and the compiler easy to read.

## Build and test

You need [Rust](https://rustup.rs) (stable). The test suite also uses a C compiler (`gcc`, `clang` or
`tcc`) and [Node.js](https://nodejs.org); a backend whose tool is missing is skipped.

```
git clone https://github.com/SpAkXD/nyra
cd nyra
cargo build --release     # the binary is target/release/nyra
cargo test                # must pass before every commit
```

Run a program while you work: `cargo run -- run examples/hello.nyra` (add `--js` for JavaScript).

## Where things live

| Path | What |
|---|---|
| `src/lexer.rs`, `src/parser.rs`, `src/ast.rs` | text to tokens to syntax tree |
| `src/check.rs` | type checker, and most error codes |
| `src/codegen/c.rs`, `src/codegen/js.rs` | the two backends |
| `src/diag.rs` | error rendering, for humans and as JSON |
| `docs/SPEC.md` | the language spec (the source of truth) |
| `docs/AI_GUIDE.md`, `llms.txt` | what AIs read before writing Nyra |
| `examples/` | `name.nyra` plus `name.out`, the exact expected output |
| `tests/errors/` | one small program per error code |
| `editors/vscode/` | VS Code syntax highlighting |

## Adding a language feature

1. Describe it in `docs/SPEC.md` first. Keep the rules few: Nyra prefers one way to do each thing.
2. Implement it through the whole pipeline: lexer, AST, parser, checker, then **both** backends. The
   same program must print the same output on C and on JavaScript.
3. Add an example: `examples/<name>.nyra` and `examples/<name>.out`. `cargo test` runs every example
   on every available backend and compares stdout exactly.
4. Update `docs/AI_GUIDE.md` and `llms.txt` if an AI needs to know about the change.

## Adding an error code

- Codes are stable: never renumber or reuse one. Take the next free number in its group (`E00xx`
  lexer, `E01xx` parser, `E02xx` checker).
- Create it with `Diag::new("E0xxx", message, span).hint("how to fix it")`. Add a hint whenever
  there is a fix; agents rely on it.
- Add `tests/errors/<name>.nyra` whose first line is `// expect: E0xxx`.
- List the code in the table in `docs/SPEC.md` and in `docs/AI_GUIDE.md`.

## Commits and pull requests

- `cargo test` must pass on every commit.
- Keep commits small and focused, with a short imperative subject ("Add string interpolation").
- No new dependencies: the compiler uses only the Rust standard library.
- The pull request template has a short checklist.

By contributing you agree that your work is released under the [MIT license](LICENSE).
