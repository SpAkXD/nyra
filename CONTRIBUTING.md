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
cargo fmt                 # the style of rustfmt.toml (CI runs `cargo fmt --check`)
cargo clippy --all-targets -- -D warnings
```

A toolchain that is missing (Python, Rust, Go, Node.js, a C compiler) skips the tests of its target;
with `NYRA_REQUIRE_ALL_TARGETS=1` (set in CI on Linux) a missing one fails them instead.

Run a program while you work: `cargo run -- run examples/hello.nyra` (add `--js` for JavaScript).

The tests run every native example with `NYRA_LEAKCHECK=1`: the program then counts its live strings
and arrays and exits with code 102 on a leak, a double free or a use after free, so a reference-counting
bug fails `cargo test` even when the output looks right. The variable is for nyra's own tests; set it by
hand to check one program the same way.

## Where things live

| Path | What |
|---|---|
| `src/lexer.rs`, `src/parser.rs`, `src/ast.rs` | text to tokens to syntax tree |
| `src/check.rs`, `src/check/` | type checker, and most error codes |
| `src/ir/` | the intermediate representation: lowering (evaluation order, reference counting), checks, optimizations; `interp.rs`, `host.rs`, `jsonrt.rs`: the interpreter that `--sandbox` and the `ex` examples use |
| `src/caps.rs`, `src/sandbox.rs`, `src/mem.rs` | capabilities (`use fs` needs `--allow fs`, E0290), the sandboxed run with its limits, the heap counter |
| `src/codegen/c.rs`, `src/codegen/js.rs` | the two backends |
| `src/rt/c/`, `src/rt/js/` | the runtime code each backend embeds (strings, arrays, printing, runtime errors) |
| `src/diag.rs`, `src/hints.rs` | error rendering, for humans and as JSON, and the hints that name the likely fix |
| `src/fix.rs` | `--fix`: validates, applies and repeats the fixes that errors carry |
| `src/explain.rs`, `docs/ERRORS.md` | `nyra explain` and the error database it prints |
| `docs/SPEC.md` | the language spec (the source of truth) |
| `docs/AI_GUIDE.md`, `llms.txt` | what AIs read before writing Nyra |
| `examples/` | `name.nyra` plus `name.out`, the exact expected output |
| `tests/errors/` | one small program per error code |
| `tests/messages.txt` | every message and hint, word for word (see below) |
| `editors/vscode/` | VS Code syntax highlighting |

## Adding a language feature

1. Describe it in `docs/SPEC.md` first. Keep the rules few: Nyra prefers one way to do each thing.
2. Implement it through the whole pipeline: lexer, AST, parser, checker, IR lowering, then **both**
   backends and their runtimes. The same program must print the same output on C and on JavaScript.
   The interpreter of the IR (`src/ir/interp.rs`, with `host.rs` and `jsonrt.rs`) runs every program for
   `nyra run --interp` and `--sandbox`: a new runtime operation or standard function needs its case there
   too, and `tests/sandbox.rs` checks that every example and runtime test prints the same as the compiled targets.
3. Add an example: `examples/<name>.nyra` and `examples/<name>.out`. `cargo test` runs every example
   on every available backend and compares stdout exactly.
4. Update `docs/AI_GUIDE.md` and `llms.txt` if an AI needs to know about the change. `cargo test`
   compiles the Nyra code blocks of the docs: blocks marked `rust` in `README.md` and the AI guide
   (GitHub has no Nyra highlighting) and blocks marked `nyra` in the spec.

## Adding an error code

- Codes are stable: never renumber or reuse one. Take the next free number in its group (`E00xx`
  lexer, `E01xx` parser, `E02xx` checker).
- Create it with `Diag::new("E0xxx", message, span).hint("how to fix it")`. The message says what is
  wrong with the names and types involved (what was expected, what was found); the hint says how to fix
  it, with corrected code where possible. Every error needs a hint: agents rely on it.
- If the mistake has exactly one possible repair, add it after the hint: `.hint(h).fix(vec![Edit::replace(span,
  "return", "ret")])` (`src/diag.rs`). An edit names the text it replaces, so a wrong position drops the fix
  instead of damaging code. Never guess: with alternatives, give a hint and no fix. Add a pair to
  `tests/fix.rs`.
- Add `tests/errors/<name>.nyra` whose first line is `// expect: E0xxx`. A program that needs command-line
  flags says so with a line `// flags: --sandbox --allow fs` in its first three lines (also in the Wrong and
  Fixed programs of `docs/ERRORS.md`). A run-time error of the interpreter's limits goes to `tests/runtime/`
  with the lines `// only: interp` and `// flags: --interp --fuel 100000`.
- Add the entry to `docs/ERRORS.md` (fields and order are described at the top of that file): what it
  means, why the rule exists, common causes, a **Wrong** program that produces exactly this code and a
  **Fixed** one that runs. `cargo test` checks all of it, and that the compiler and the database list
  the same codes. A code that is only planned has an entry marked "planned".
- List the code in the table in `docs/SPEC.md` and in `docs/AI_GUIDE.md`.
- Add a case to `tests/messages.txt` and run `NYRA_BLESS=1 cargo test --test messages` to record the
  text that `nyra check` prints for it. Review the diff: it is what users will see.

## Commits and pull requests

- `cargo test` must pass on every commit.
- Keep commits small and focused, with a short imperative subject ("Add string interpolation").
- No new dependencies: the compiler uses only the Rust standard library.
- The pull request template has a short checklist.

By contributing you agree that your work is released under the [MIT license](LICENSE).
