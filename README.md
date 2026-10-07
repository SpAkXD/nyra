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
  <a href="docs/ERRORS.md">Errors</a> ·
  <a href="#roadmap">Roadmap</a>
</p>

<p align="center">
  <sub>AI agent? Read <a href="https://raw.githubusercontent.com/SpAkXD/nyra/main/llms.txt">llms.txt</a> and
  <a href="https://raw.githubusercontent.com/SpAkXD/nyra/main/docs/AI_GUIDE.md">docs/AI_GUIDE.md</a> before you write any Nyra.</sub>
</p>

```rust
struct Player {
    name: str
    score: int
}

fn main() {
    var players: [Player] = []
    for entry in "ada:31 grace:47 alan:28".split(" ") {
        let parts = entry.split(":")
        players.push(Player(name: parts[0], score: int(parts[1])))
    }
    var best = players[0]
    for p in players {
        if p.score > best.score { best = p }
    }
    print("{best.name.upper()} wins with {best.score} points")
    print(best)
}
```

```
$ nyra run scores.nyra
GRACE wins with 47 points
Player(name: "grace", score: 47)
```

## Why Nyra

Most languages are designed for people to read. Nyra is designed for an AI to **write**, and for the
compiler to tell the AI exactly what to fix.

- **Familiar tokens, tiny grammar.** It reads like Rust, Go and TypeScript, so a model half-knows it
  already, and the whole language fits in one prompt ([`docs/SPEC.md`](docs/SPEC.md)).
- **One way to do each thing.** No implicit conversions, no shadowing, no null, no semicolons. Fewer
  choices mean fewer wrong ones.
- **Errors are an API.** Every error has a stable code, an exact position and a fix hint, and `--json`
  makes them machine-readable, so an agent can loop *write, check, fix, run* without a human. Each message says
  what was expected and what was found, and `nyra explain E0201` explains any code with a wrong and a fixed
  program ([`docs/ERRORS.md`](docs/ERRORS.md)).
- **Compact programs.** One-line functions, string interpolation, `if` as a value and built-in methods
  on strings and arrays keep code short. A benchmark of first-try correctness and token count against
  Python is coming in v0.4.
- **Values, not references.** Arrays, strings and structs are copied on assignment (cheaply, copy on
  write), so nothing changes behind your back; a function changes a caller's variable only through an
  `inout` parameter that the call names too.
- **Same output everywhere.** Every example runs on both backends in CI and must match byte for byte.

## Features

- **Two backends from one source:** native code through C99 (`gcc`, `clang` or `tcc`) and JavaScript
  (Node.js or the browser).
- **Strict static types:** `int`, `float`, `bool`, `str`, `char`, arrays and structs, with local inference
  and no implicit conversions.
- **Real data (v0.3):** structs, arrays, strings and chars with methods (`split`, `replace`, `slice`,
  `sort`, `join`, ...), `inout` parameters, `break` and `continue`. Memory is freed by reference counting,
  with no garbage collector, and `free`, `arena` and `keep` say when if you want to.
- **Short code (v0.2):** one-line functions, `+=` and friends, string interpolation, `if` as a value.
- **Agent-friendly tooling:** `run`, `build` and `check`, errors as JSON, `explain` for every error code,
  runtime errors with the exact position, and a build cache that skips the C compiler when the program
  has not changed.
- **Fast and small:** the compiler takes about a millisecond per file and the C compiler 0.5 to 1 s
  (measured on the author's PC). It is Rust with zero dependencies and writes plain, readable C and JavaScript.
- **Not yet:** maps, modules, a standard library, input (see the [roadmap](#roadmap)). Nyra is 0.x, so the
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

Save the program above as `scores.nyra` and run `nyra scores.nyra`. You never pass flags to the C
compiler: Nyra calls it with `-O2 -fwrapv -ffp-contract=off` (plus `-s` to strip the executable) and
caches the result, so running an unchanged program again skips the C compiler. Editor support: syntax
highlighting for VS Code is in [`editors/vscode`](editors/vscode).

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
| [`docs/ERRORS.md`](docs/ERRORS.md) | the error database: every code, what it means, why, the usual causes, a wrong and a fixed program |
| [`examples/`](examples) | runnable programs, each with its expected output in a `.out` file |

An agent that can run commands follows one loop: **write, check, fix, run**.

```
nyra check prog.nyra --json   # -> {"ok":false,"errors":[{"code":"E0210","line":3,"col":13,...}]}
# fix each error using its code, position and hint; repeat until "ok":true
nyra explain E0210 --json     # if the hint is not enough: what the code means, why, causes, wrong and fixed program
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
struct Point { x: int, y: int }                 // a struct: named fields, uppercase name

fn add(a: int, b: int) -> int = a + b          // one-line function: the expression is the result
fn greet(name: str) = print("hi {name}")       // no `->`: returns nothing
fn dist2(p: Point) -> int = p.x * p.x + p.y * p.y

fn gcd(a: int, b: int) -> int {                // block body: `ret` returns
    if b == 0 { ret a }
    ret gcd(b, a % b)
}

fn bump(inout n: int) { n += 1 }               // `inout`: may change the caller's variable

fn main() {                                    // every program starts here
    let x = 5                                  // immutable, type inferred
    var total = 0                              // mutable
    total += x                                 // also -= *= /= %=  (var only)
    let size = if x > 3 { "big" } else { "small" }   // `if` as a value: `else` is required

    for i in 0..3 { print(i) }                 // 0 1 2: the end is exclusive
    while total < 20 { total += 7 }
    if total > 20 && !(x == 0) { print("over") } else { print("under") }

    var xs = [3, 1, 2]                         // an array: [int]
    xs.push(4)
    xs.sort()                                  // [1, 2, 3, 4]
    for v in xs {
        if v == 3 { break }                    // `break` and `continue` work in every loop
        print(v)                               // 1 2
    }
    let p = Point(x: 3, y: 4)                  // every field is named
    print("{p} has dist2 {dist2(p)}")          // Point(x: 3, y: 4) has dist2 25
    let word = "nyra"
    print(word.upper() + "!")                  // NYRA!: `+` joins two strings
    print(word[0] == 'n')                      // true: s[i] is a char
    bump(inout total)                          // the call says `inout` too
    print("sum: {add(x, 2)}, size: {size}, total: {total}")   // {{ and }} are literal braces
    print(float(x) / 2.0)                      // 2.5: conversions are always explicit
    print(gcd(48, 18))                         // 6
    greet("nyra")
}
```

- **Types:** `int` (64-bit), `float` (64-bit), `bool`, `str`, `char`, arrays `[T]` and structs.
  Signatures are fully typed; locals are inferred.
- **No implicit conversions:** `1 + 2.0` is error `E0210`; write `float(1) + 2.0`. `7 / 2` is `3`, and a
  float prints in its shortest form: `print(2.0)` shows `2`.
- **No shadowing, no null, no semicolons.** `let` is immutable, `var` can be reassigned, and a name
  cannot be declared again while it is visible.
- **Values, not references:** assigning or passing an array or a struct copies it; only an `inout`
  parameter changes the caller's variable.
- **Strings** are UTF-8 and count characters: `s[i]` is a `char` (`'a'`), `+` joins two strings, and
  `==` and `<` compare them by content. Arrays and structs print as Nyra code.
- **Conditions must be `bool`:** `if n != 0`, not `if n`.
- **Builtins:** `print(x)` (one argument), `str(x)`, `int(x)`, `float(x)`, `char(n)`; everything else is
  a method, like `s.split(",")` or `xs.len()`. No input yet.
- **Operators**, high to low: calls, fields, indexes, methods · `-` `!` · `*` `/` `%` · `+` `-` ·
  `<` `<=` `>` `>=` · `==` `!=` · `&&` · `||`.

The complete reference is [`docs/SPEC.md`](docs/SPEC.md); [`docs/AI_GUIDE.md`](docs/AI_GUIDE.md) adds
every common mistake with its error code and fix.

## CLI

| Command | What it does |
|---|---|
| `nyra run <file>` | compile and run (`nyra <file>` is the same) |
| `nyra build <file>` | compile to a native executable |
| `nyra check <file>` | type-check only; exit code 0 means no errors |
| `nyra explain [CODE]` | explain an error code (what it means, why, causes, a wrong and a fixed program); without a code, list all codes |

| Option | Meaning |
|---|---|
| `--js` | use the JavaScript backend instead of native |
| `--c` | with `build`: write the generated C instead of an executable |
| `-o <path>` | output path for `build` (`-o -` prints to stdout) |
| `--json` | print errors as JSON (compile and runtime errors), for AI agents and tools; with `explain`, print the entry as JSON |
| `--time` | show how long each step took |

Exit codes: `0` success, `1` compile errors, `2` usage or tool problem, `101` runtime error (for example
an index out of bounds, which prints `runtime error[E0240]` with the file and position, identically on
both backends). `NYRA_CC` selects the C compiler.

## How it works

```
                                                         ┌─► C99 ──► gcc / clang ──► native executable
source.nyra ─► lexer ─► parser ─► type checker ─► IR ────┤
                                                         └─► JavaScript ──► Node.js / browser
```

| File | Role |
|---|---|
| `src/lexer.rs` | text to tokens |
| `src/parser.rs` | tokens to syntax tree (recursive descent, recovers after errors) |
| `src/check.rs`, `src/check_v03.rs` | type checking, collects every error in one pass |
| `src/ir/` | the intermediate representation: evaluation order, runtime checks, reference counting, optimizations |
| `src/codegen/c.rs`, `src/codegen/js.rs` | the two backends |
| `src/rt/c/`, `src/rt/js/` | the runtimes they embed: strings, arrays, printing, runtime errors |
| `src/diag.rs`, `src/hints.rs` | errors for humans and JSON for agents, and the "what did you probably mean" hints |
| `src/explain.rs`, `docs/ERRORS.md` | `nyra explain` and the error database it prints |

## Roadmap

| Version | Theme | Status |
|---|---|---|
| v0.1 | core language, C and JavaScript backends, JSON errors | done |
| v0.2 | short code: one-line functions, `+=`, string interpolation, `if` as a value, build cache | done |
| v0.3 | real data: structs, arrays, strings and chars with methods, `inout`, `break`/`continue`, memory model (no GC) | done |
| v0.4 | benchmark: first-try correctness and token count against Python | harness ready in [`bench/`](bench), results coming |
| v0.5 | WASM backend and browser playground (the intermediate representation is done) | planned |
| v0.6 | modules, standard library, C FFI | planned |
| v1.0 | packages and addons, published VS Code extension, docs site | planned |

## Contributing

Issues and pull requests are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md) first; the short version
is that `cargo test` must stay green. If an AI could not write a program you expected it to, open an
issue with the program and the `nyra check --json` output: that is the most useful bug report.

## License

[MIT](LICENSE)
