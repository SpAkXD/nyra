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
- **The compiler repairs simple mistakes itself.** An error with exactly one possible repair (`return` for
  `ret`, a `;`, `elif`, `'text'`, `xs.length()`, `string`, `Point { x: 1 }`, ...) carries a machine-applicable
  fix, and `--fix` applies them all: a slip costs no extra model call.
- **Compact programs.** One-line functions, string interpolation, `if` as a value and built-in methods
  on strings and arrays keep code short. A benchmark of first-try correctness and token count against
  Python is coming in v0.4.
- **Values, not references.** Arrays, strings and structs are copied on assignment (cheaply, copy on
  write), so nothing changes behind your back; a function changes a caller's variable only through an
  `inout` parameter that the call names too.
- **Same output everywhere.** Every example runs on every backend in CI and must match byte for byte.

## Features

- **Six targets from one source:** native code through C99 (`gcc`, `clang` or `tcc`), JavaScript
  (Node.js or the browser), and readable [Python, TypeScript, Rust and Go](#targets).
- **Strict static types:** `int`, `float`, `bool`, `str`, `char`, arrays and structs, with local inference
  and no implicit conversions.
- **Real data (v0.3):** structs, arrays, strings and chars with methods (`split`, `replace`, `slice`,
  `sort`, `join`, ...), `inout` parameters, `break` and `continue`. Memory is freed by reference counting,
  with no garbage collector, and `free`, `arena` and `keep` say when if you want to.
- **Short code (v0.2):** one-line functions, `+=` and friends, string interpolation, `if` as a value.
- **Agent-friendly tooling:** `run`, `build` and `check`, errors as JSON, `explain` for every error code,
  runtime errors with the exact position, and a build cache that skips the C compiler when the program
  has not changed.
- **Fast and small:** the compiler takes about a millisecond per file and the C compiler 0.5 to 1.5 s
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
nyra check prog.nyra --json --fix   # repairs what has a certain fix, then -> {"ok":true,...} or the errors left
# fix each remaining error using its code, position and hint; repeat until "ok":true
nyra explain E0210 --json           # if the hint is not enough: what the code means, why, causes, wrong and fixed program
nyra run prog.nyra
```

### Self-repair with `--fix`

Many mistakes have exactly one possible repair: `return x` is `ret x`, a `;` goes, `elif` is `else if`,
`and` is `&&`, `True` is `true`, `'hello'` is `"hello"`, `xs.length()` is `xs.len()`, `string` is `str`,
`Point { x: 1 }` is `Point(x: 1)`, `5.` is `5.0`, `print "hi"` is `print("hi")`. Such an error carries a
**fix**, and `nyra check --fix` (or `run --fix`, `build --fix`) applies every fix, checks again (a few
rounds, since fixing the syntax can reveal a type error with its own fix) and, if the program then
compiles, writes it back, prints the edits to stderr as a diff and goes on. If an error without a fix
remains, the file is left unchanged and the errors are reported as usual. A program that compiles is
never touched.

**Agents should run `--fix` (or apply the JSON `fix` themselves) before asking a model to repair a
program**: a mistake with a fix then costs no model call and no tokens. In the JSON form the fix is a
list of edits, each replacing the text from `line`:`col` up to (not including) `end_line`:`end_col`
with `text` (columns count characters, from 1):

```
{"code":"E0101","message":"`return` is not part of Nyra: ...","line":2,"col":12,"hint":"Nyra spells it `ret`: ...",
 "fix":[{"line":2,"col":5,"end_line":2,"end_col":11,"text":"ret"}]}
```

A fix is only given where it is certain. When there are alternatives there is a hint and no fix: a typo
(`cout`: `count`?), `null`, Go's `:=` (`let` or `var`?), `number` (`int` or `float`?), `n + 0.5` with an
`int` `n` (convert `n`, or declare it a float?).

An AI that cannot run code (a plain chat) can still write correct programs by following the guide;
you then run `nyra run prog.nyra` yourself. If your AI cannot open links, paste the contents of
[`docs/AI_GUIDE.md`](docs/AI_GUIDE.md) into the chat first.

### MCP server

`nyra mcp` is a [Model Context Protocol](https://modelcontextprotocol.io) server built into the
compiler, so any MCP client can use Nyra with no other setup: the agent gets the spec, the checker,
both backends and the error database as tools, and needs no files or shell access.

| Tool | What it does |
|---|---|
| `nyra_spec` | the language spec (`part: "guide"`: the AI guide), so the agent learns Nyra in one call |
| `nyra_check` | `{code}` → the same JSON as `nyra check --json` |
| `nyra_run` | `{code, backend?: "native"\|"js", stdin?, timeout_ms?}` → `{ok, exit, stdout, errors?, ms}` (10 s timeout, output capped) |
| `nyra_explain` | `{code: "E0201"}` → the error database entry (without `code`: every code) |
| `nyra_build` | `{code, target?: "c"\|"js"}` → the generated C or JavaScript |
| `nyra_outline` | `{path}` or `{code}` → one line per function and struct with its line range |
| `nyra_show` | `{path or code, name: "find Item.tags"}` → the source of those symbols |
| `nyra_edit` | `{path or code, edits, force?, fix?}` → change symbols by name ([below](#editing-by-symbol)); a path is written in place and only a summary returns |

Resources: `nyra://spec`, `nyra://guide`, `nyra://errors` (the error index) and `nyra://errors/{code}`.

**Claude Code:**

```
claude mcp add nyra -- nyra mcp
```

**Claude Desktop:** Settings → Developer → Edit Config, then add to `claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "nyra": { "command": "nyra", "args": ["mcp"] }
  }
}
```

**Cursor:** the same `mcpServers` entry in `.cursor/mcp.json` (one project) or `~/.cursor/mcp.json`
(every project). Gemini CLI reads it from `~/.gemini/settings.json`, and other clients take the
command `nyra mcp` the same way. If `nyra` is not on the `PATH` the client sees, give the full path
to the binary as `command`.

### Editing by symbol

An agent that changes one function of a long program should not send the program again. `nyra edit`
(and the `nyra_edit` tool) changes functions, structs and struct fields **by name**:

```
$ nyra outline shop.nyra
shop.nyra: 209 lines
4-10 struct Item { sku: str, name: str, price: int, stock: int, tags: [str] }
...
71-79 fn discount(total: int) -> int
...
$ nyra show shop.nyra discount            # just that function
$ nyra edit shop.nyra < new_discount.txt  # the new `fn discount ...`: replaces the old one
nyra: edited shop.nyra: replaced fn discount (lines 71-82)
$ nyra edit shop.nyra --rename Item.stock in_stock
nyra: edited shop.nyra: renamed field Item.stock -> in_stock, 7 references (line 8)
```

- **Operations:** replace a function or struct (send its new definition), add one (at the end, or
  `after`/`before` another), delete one, rename one with every reference, add or remove a struct field.
  Several go in one script (`@replace NAME`, `@add after NAME`, `@delete NAME`, `@rename NAME NEW`,
  `@add-field Struct name: type`), or plain definitions replace the symbols of their names.
- **Exact:** each edit replaces the source range of its symbol, so the rest of the file stays
  byte-identical, including its line breaks (`\r\n` files stay `\r\n`). A rename uses the parser and the
  checker: it changes calls, constructions, type annotations, field reads and labels, never strings,
  comments or another struct's field of the same name.
- **Checked:** the result must compile. An edit that adds errors is refused with those errors (each
  names the symbol it is in) and the file is not written; `--force` applies it anyway and `--fix` runs
  the [self-repair](#self-repair-with---fix) first.
- **Cheap:** on the 209-line `tests/edit/inventory.nyra` (5072 bytes, about 1450 tokens), changing one
  function costs 208 bytes of edit and a 58-byte reply, about 75 tokens: **95% less** than resending
  the file. The outline of the whole file is 1195 bytes.

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

## Targets

One Nyra file compiles to six targets. All of them come from the same intermediate representation, so
the output, the order things happen in and the runtime errors (code, message, position, exit code 101)
are the same on each; the tests run every example on every target that is installed.

| Target | `--target` | `nyra build` writes | `nyra run` needs |
|---|---|---|---|
| native | `native` (default) | an executable | a C compiler (`gcc`, `clang`, `cc`, `tcc`; `NYRA_CC`) |
| C | `c` | `file.c` | (same as native) |
| JavaScript | `js` (`--js`) | `file.js` | Node.js |
| Python | `py` (`--py`) | `file.py` | Python 3.8+ (`python3` or `python`; `NYRA_PYTHON`) |
| TypeScript | `ts` (`--ts`) | `file.ts` | Node.js 22.6+ (it strips the types) |
| Rust | `rs` (`--rs`) | `file.rs` | `rustc` (`NYRA_RUSTC`) |
| Go | `go` (`--go`) | `file.go` | Go 1.23+ (`go`; `NYRA_GO`) |

```sh
nyra build --target py scores.nyra        # writes scores.py
nyra build --target rs scores.nyra -o -   # prints the Rust code
nyra run --go scores.nyra                 # builds with `go build` (cached) and runs it
```

The generated code reads like code a person would write in that language: typed signatures, variables
declared where they are first needed, counted loops as `for` loops, structs as classes or structs, and a
small runtime at the end of the file for what the language does differently (64-bit wrapping ints in
Python, character-based string indexes, JavaScript's number format, checked indexes). Values keep Nyra's
semantics: arrays and structs are copied on write (a shared mark in Python, TypeScript and Go,
`Rc::make_mut` in Rust), and an `inout` parameter becomes a returned value in Python
(`x, y = swap(x, y)`), a `{ v }` box in TypeScript, `&mut T` in Rust and a pointer in Go. Build the Rust
file with overflow checks off (`rustc -O -C overflow-checks=off`, as `nyra run` does): Nyra ints wrap.
See [known differences](docs/SPEC.md#known-differences-between-backends) for the few edge cases.

## CLI

| Command | What it does |
|---|---|
| `nyra run <file>` | compile and run (`nyra <file>` is the same) |
| `nyra build <file>` | compile to a native executable |
| `nyra check <file>` | type-check only; exit code 0 means no errors |
| `nyra explain [CODE]` | explain an error code (what it means, why, causes, a wrong and a fixed program); without a code, list all codes |
| `nyra mcp` | run the [MCP server](#mcp-server) on stdin/stdout, for AI agents |
| `nyra outline <file>` | the functions and structs with signatures, fields and line ranges (`--json` too) |
| `nyra show <file> <name>...` | the source of functions, structs or fields (`Struct.field`) |
| `nyra edit <file> [edits]` | [change symbols by name](#editing-by-symbol): `--set`, `--add`, `--delete`, `--rename`, `--add-field`, or an edit script on stdin; `--force`, `--fix`, `--dry-run`, `--json` |

| Option | Meaning |
|---|---|
| `--target <t>` | the [target](#targets): `native` (default), `c`, `js`, `py`, `ts`, `rs` or `go` |
| `--js` `--py` `--ts` `--rs` `--go` | short for `--target js` and so on |
| `--c` | with `build`: write the generated C instead of an executable |
| `-o <path>` | output path for `build` (`-o -` prints to stdout) |
| `--json` | print errors as JSON (compile and runtime errors), for AI agents and tools; with `explain`, print the entry as JSON |
| `--fix` | with `check`, `run` and `build`: apply the fixes that errors carry, check again, and write the file back if it then compiles |
| `--time` | show how long each step took |

Exit codes: `0` success, `1` compile errors, `2` usage or tool problem, `101` runtime error (for example
an index out of bounds, which prints `runtime error[E0240]` with the file and position, identically on
every target). `NYRA_CC`, `NYRA_PYTHON`, `NYRA_RUSTC` and `NYRA_GO` select the tools.

## How it works

```
                                                         ┌─► C99 ──► gcc / clang ──► native executable
                                                         ├─► JavaScript ──► Node.js / browser
source.nyra ─► lexer ─► parser ─► type checker ─► IR ────┼─► Python, TypeScript
                                                         └─► Rust, Go ──► rustc / go build
```

| File | Role |
|---|---|
| `src/lexer.rs` | text to tokens |
| `src/parser.rs` | tokens to syntax tree (recursive descent, recovers after errors) |
| `src/check.rs`, `src/check_v03.rs` | type checking, collects every error in one pass |
| `src/ir/` | the intermediate representation: evaluation order, runtime checks, reference counting, optimizations |
| `src/codegen/` | the backends: `c.rs`, `js.rs`, `py.rs`, `ts.rs`, `rs.rs`, `go.rs`; `scope.rs` places declarations and finds counted loops for the last four |
| `src/rt/*/` | the runtimes they embed: strings, arrays, printing, runtime errors |
| `src/diag.rs`, `src/hints.rs` | errors for humans and JSON for agents, and the "what did you probably mean" hints |
| `src/fix.rs` | `--fix`: checks, applies and repeats the fixes that errors carry |
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
