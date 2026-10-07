<p align="center">
  <img src="assets/logo.svg" alt="Nyra" width="320">
</p>

<p align="center">
  <b>A small, strict programming language designed to be written by AI agents.</b><br>
  Compact syntax, no ambiguity, and compiler errors an agent can read and fix by itself.<br>
  One source file compiles to native code (via C) or to JavaScript.
</p>

<p align="center">
  <a href="https://github.com/SpAkXD/nyra/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/SpAkXD/nyra/actions/workflows/ci.yml/badge.svg?branch=main"></a>
  <a href="LICENSE"><img alt="License: MIT" src="https://img.shields.io/badge/license-MIT-blue"></a>
  <a href="Cargo.toml"><img alt="Version" src="https://img.shields.io/badge/dynamic/toml?url=https%3A%2F%2Fraw.githubusercontent.com%2FSpAkXD%2Fnyra%2Fmain%2FCargo.toml&query=%24.package.version&label=version&color=7C5CFF"></a>
</p>

<p align="center">
  <a href="#quick-start">Quick start</a> ·
  <a href="#using-nyra-with-an-ai">Use it with an AI</a> ·
  <a href="#language-tour">Language tour</a> ·
  <a href="docs/SPEC.md">Spec</a> ·
  <a href="docs/AI_GUIDE.md">AI guide</a> ·
  <a href="#roadmap">Roadmap</a>
</p>

<p align="center">
  <sub>AI agent? Read <a href="https://raw.githubusercontent.com/SpAkXD/nyra/main/llms.txt">llms.txt</a> and
  <a href="https://raw.githubusercontent.com/SpAkXD/nyra/main/docs/AI_GUIDE.md">docs/AI_GUIDE.md</a> before you write any Nyra.</sub>
</p>

```rust
fn fib(n: int) -> int {
    if n < 2 { ret n }
    ret fib(n - 1) + fib(n - 2)
}

fn main() {
    for i in 0..6 {
        print("fib({i}) = {fib(i)}")
    }
}
```

```
$ nyra run fib.nyra
fib(0) = 0
fib(1) = 1
fib(2) = 1
fib(3) = 2
fib(4) = 3
fib(5) = 5
```

## Why Nyra

Most languages are designed for people to read. Nyra is designed for an AI to **write**, and for the
compiler to tell the AI exactly what to fix.

- **Familiar tokens, tiny grammar.** It reads like Rust, Go and TypeScript, so a model half-knows it
  already, and the whole language fits in one prompt ([`docs/SPEC.md`](docs/SPEC.md)).
- **One way to do each thing.** No implicit conversions, no shadowing, no null, no semicolons. Fewer
  choices mean fewer wrong ones.
- **Errors are an API.** Every error has a stable code, an exact position and a fix hint, and `--json`
  makes them machine-readable, so an agent can loop *write, check, fix, run* without a human.
- **Compact programs.** One-line functions, string interpolation and `if` as a value keep code short.
  A benchmark of first-try correctness and token count against Python is coming in v0.4.
- **Same output everywhere.** Every example runs on both backends in CI and must match byte for byte.

## Features

- **Two backends from one source:** native code through C99 (`gcc`, `clang` or `tcc`) and JavaScript
  (Node.js or the browser).
- **Strict static types:** `int`, `float`, `bool`, `str`, with local inference and no implicit conversions.
- **Short code (v0.2):** one-line functions, `+=` and friends, string interpolation, `if` as a value.
- **Agent-friendly tooling:** `run`, `build` and `check`, errors as JSON, and a build cache that skips
  the C compiler when the program has not changed.
- **Fast and small:** the compiler takes about 0.1 to 0.3 ms per file and the C compiler about 0.4 s
  (measured on the author's PC). It is Rust with zero dependencies and writes plain, readable C and JavaScript.
- **Not yet:** arrays, structs, string functions, input (see the [roadmap](#roadmap)). Nyra is 0.x, so the
  syntax may still change before 1.0.

## Install

**Prebuilt binary** (Linux x86_64, Windows x86_64, macOS Apple Silicon): download the archive for your
platform from the [latest release](https://github.com/SpAkXD/nyra/releases/latest), unpack it and put
`nyra` on your `PATH`. On macOS run `xattr -d com.apple.quarantine nyra` once (the binary is not signed).

**From source** (any platform with [Rust](https://rustup.rs)):

```
git clone https://github.com/SpAkXD/nyra
cd nyra
cargo build --release          # the binary is target/release/nyra
```

or `cargo install --git https://github.com/SpAkXD/nyra`.

To run programs natively Nyra needs a C compiler: `gcc`, `clang` or `tcc`, found automatically (set
`NYRA_CC` to pick one; on Windows, [MSYS2](https://www.msys2.org)'s gcc works out of the box). For the
JavaScript backend (`--js`) you need [Node.js](https://nodejs.org) instead.

## Quick start

```
nyra run examples/hello.nyra          # compile and run natively
nyra run examples/hello.nyra --js     # the same program on Node.js
nyra build examples/hello.nyra        # a native executable next to the source
nyra check examples/hello.nyra        # only report errors
```

Save the program above as `fib.nyra` and run `nyra fib.nyra`. You never pass flags to the C compiler:
Nyra calls it with `-O2 -fwrapv` (plus `-s` to strip the executable) and caches the result, so running
an unchanged program again skips the C compiler. Editor support: syntax highlighting for VS Code is in
[`editors/vscode`](editors/vscode).

## Using Nyra with an AI

Nyra is brand new, so no model has seen it in training. It does not need to: the docs are short enough
to read in one go, and the compiler corrects what is left. **Paste this repo's URL into any AI that can
read links** (Gemini, ChatGPT, Claude, ...) and ask for a program:

> Read https://github.com/SpAkXD/nyra, starting with `llms.txt` and `docs/AI_GUIDE.md`, then write me
> a Nyra program that prints the first 20 prime numbers.

| The AI reads | What it gets |
|---|---|
| [`llms.txt`](llms.txt) | what Nyra is, the ten most important rules, links ([llmstxt.org](https://llmstxt.org) convention) |
| [`docs/AI_GUIDE.md`](docs/AI_GUIDE.md) | workflow, do/don't rules, what does not exist, error codes with fixes, complete programs |
| [`docs/SPEC.md`](docs/SPEC.md) | the complete language spec |
| [`examples/`](examples) | runnable programs, each with its expected output in a `.out` file |

An agent that can run commands follows one loop: **write, check, fix, run**.

```
nyra check prog.nyra --json   # -> {"ok":false,"errors":[{"code":"E0210","line":3,"col":13,...}]}
# fix each error using its code, position and hint; repeat until "ok":true
nyra run prog.nyra
```

An AI that cannot run code (a plain chat) can still write correct programs by following the guide;
you then run `nyra run prog.nyra` yourself. If your AI cannot open links, paste the contents of
[`docs/AI_GUIDE.md`](docs/AI_GUIDE.md) into the chat first.

> [!NOTE]
> **For AI agents reading this page:** Nyra is not Rust (code blocks here say `rust` only so GitHub
> highlights them). Do not guess syntax. Read
> [AI_GUIDE.md](https://raw.githubusercontent.com/SpAkXD/nyra/main/docs/AI_GUIDE.md) and
> [llms.txt](https://raw.githubusercontent.com/SpAkXD/nyra/main/llms.txt) before you write code.

## Language tour

```rust
fn add(a: int, b: int) -> int = a + b          // one-line function: the expression is the result
fn greet(name: str) = print("hi {name}")       // no `->`: returns nothing

fn gcd(a: int, b: int) -> int {                // block body: `ret` returns
    if b == 0 { ret a }
    ret gcd(b, a % b)
}

fn main() {                                    // every program starts here
    let x = 5                                  // immutable, type inferred
    var total = 0                              // mutable
    total += x                                 // also -= *= /= %=  (var only)
    let size = if x > 3 { "big" } else { "small" }   // `if` as a value: `else` is required

    for i in 0..3 { print(i) }                 // 0 1 2: the end is exclusive
    while total < 20 { total += 7 }
    if total > 20 && !(x == 0) { print("over") } else { print("under") }

    print("sum: {add(x, 2)}, size: {size}")    // interpolation; {{ and }} are literal braces
    print(float(x) / 2.0)                      // 2.5: conversions are always explicit
    print(gcd(48, 18))                         // 6
    greet("nyra")
}
```

- **Types:** `int` (64-bit), `float` (64-bit), `bool`, `str`. Signatures are fully typed; locals are inferred.
- **No implicit conversions:** `1 + 2.0` is error `E0210`; write `float(1) + 2.0`. `7 / 2` is `3`, and a
  float prints in its shortest form: `print(2.0)` shows `2`.
- **No shadowing, no null, no semicolons.** `let` is immutable, `var` can be reassigned, and a name is
  declared once per function.
- **Conditions must be `bool`:** `if n != 0`, not `if n`.
- **Strings** compare by value with `==`. There is no string `+`: join text with interpolation, `"{a}{b}"`.
- **Builtins:** `print(x)` (one argument), `int(x)`, `float(x)`. No `break`, no input, no arrays yet.
- **Operators**, high to low: `-` `!` · `*` `/` `%` · `+` `-` · `<` `<=` `>` `>=` · `==` `!=` · `&&` · `||`.

The complete reference is [`docs/SPEC.md`](docs/SPEC.md); [`docs/AI_GUIDE.md`](docs/AI_GUIDE.md) adds
every common mistake with its error code and fix.

## CLI

| Command | What it does |
|---|---|
| `nyra run <file>` | compile and run (`nyra <file>` is the same) |
| `nyra build <file>` | compile to a native executable |
| `nyra check <file>` | type-check only; exit code 0 means no errors |

| Option | Meaning |
|---|---|
| `--js` | use the JavaScript backend instead of native |
| `--c` | with `build`: write the generated C instead of an executable |
| `-o <path>` | output path for `build` (`-o -` prints to stdout) |
| `--json` | print errors as JSON, for AI agents and tools |
| `--time` | show how long each step took |

Exit codes: `0` success, `1` compile errors, `2` usage or tool problem. `NYRA_CC` selects the C compiler.

## How it works

```
                                                   ┌─► C99 ──► gcc / clang ──► native executable
source.nyra ─► lexer ─► parser ─► type checker ────┤
                                                   └─► JavaScript ──► Node.js / browser
```

| File | Role |
|---|---|
| `src/lexer.rs` | text to tokens |
| `src/parser.rs` | tokens to syntax tree (recursive descent, recovers after errors) |
| `src/check.rs` | type checking, collects every error in one pass |
| `src/codegen/c.rs`, `src/codegen/js.rs` | the two backends |
| `src/diag.rs` | errors for humans and JSON for agents |

## Roadmap

| Version | Theme | Status |
|---|---|---|
| v0.1 | core language, C and JavaScript backends, JSON errors | done |
| v0.2 | short code: one-line functions, `+=`, string interpolation, `if` as a value, build cache | done |
| v0.3 | structs, arrays, string functions, memory model (no GC) | next |
| v0.4 | benchmark: first-try correctness and token count against Python | coming |
| v0.5 | intermediate representation, WASM backend, browser playground | planned |
| v0.6 | modules, standard library, C FFI | planned |
| v1.0 | packages and addons, published VS Code extension, docs site | planned |

## Contributing

Issues and pull requests are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md) first; the short version
is that `cargo test` must stay green. If an AI could not write a program you expected it to, open an
issue with the program and the `nyra check --json` output: that is the most useful bug report.

## License

[MIT](LICENSE)
