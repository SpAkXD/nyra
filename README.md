<p align="center">
  <img src="assets/logo.svg" alt="Nyra" width="320">
</p>

<p align="center">
  <b>The language agents can run unsupervised.</b><br>
  Nearly as cheap to write as Python, as fast as Rust, and unable to touch anything it didn't declare.<br>
  A small, strict language designed to be written by AI agents. One source file compiles to a native executable (via C) or to JavaScript. (Python, TypeScript, Rust and Go output is experimental.)
</p>

<p align="center">
  <sub>Speed and cost are measured: see <a href="#honest-numbers">Honest numbers</a>. The "touch nothing it didn't declare" part
  (capabilities, deny by default) is <b>coming in v0.6</b>; until then a program has the rights of its process.</sub>
</p>

<p align="center">
  <a href="https://github.com/SpAkXD/nyra/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/SpAkXD/nyra/actions/workflows/ci.yml/badge.svg?branch=main"></a>
  <a href="LICENSE"><img alt="License: MIT" src="https://img.shields.io/badge/license-MIT-blue"></a>
  <a href="Cargo.toml"><img alt="Version" src="https://img.shields.io/badge/dynamic/toml?url=https%3A%2F%2Fraw.githubusercontent.com%2FSpAkXD%2Fnyra%2Fmain%2FCargo.toml&query=%24.package.version&label=version&color=7C5CFF"></a>
</p>

<p align="center">
  <a href="#why-nyra">Why Nyra</a> ·
  <a href="#honest-numbers">Honest numbers</a> ·
  <a href="#quick-start">Quick start</a> ·
  <a href="#using-nyra-with-an-ai">Use it with an AI</a> ·
  <a href="#safe-to-run-unsupervised">Safety</a> ·
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
```

```
$ nyra run scores.nyra
GRACE wins with 47 points
Player(name: "grace", score: 47)
```

## Why Nyra

An agent that writes code can now also run it, and that is where the trouble starts: either you trust
generated code with your machine, or you start a VM or container for every call. Nyra is a language for
that code-execution step.

| | What you get | Status |
|---|---|---|
| **Safe to run** | A program can use only what it declares. Files, network and the like are capabilities, denied by default and checked by the compiler: `nyra run prog.nyra --allow fs`. No VM per call. | **Coming in v0.6** |
| **Fast** | Native code through C. About 5-6 ms against about 300 ms for Python on the benchmark's speed tasks, and within about 1.2x of hand-written C on [`perf/`](perf). | Measured |
| **Near Python in cost** | About 1.2-1.3x Python's code tokens, and shorter than Rust (Opus 475 vs 523, Sonnet 408 vs 523). Not cheaper than Python: see [Honest numbers](#honest-numbers). | Measured |
| **Right on the first try** | On 83 tasks: Opus 98%, Sonnet 96%, Haiku 4.5 66% (Python: 78%). The compiler repairs the simple slips itself. | Measured |

**Who it is for**

- People building **agent harnesses** who need a code-execution tool without a VM per call.
- Teams running **cheap models at volume**, where one-way-to-do-it syntax, strict types and
  self-repairing errors turn a failed attempt into a cheap one.
- **Evaluation builders** who need deterministic, replayable programs: the same output on every
  backend, byte for byte.

**What sets it apart.** None of these ideas is new alone; together, in a small language with published
agent-success numbers, they are:

- **Capabilities as compile errors** (v0.6). Using a power you did not declare is an ordinary
  compile error with a code, a position and a hint, so the agent learns what it lacks in the check
  step, and you see what a program can do before it runs. Until it ships, run code you do not trust
  in a container or VM (see [MCP server](#mcp-server)).
- **A compiler that repairs, with published numbers.** An error with exactly one possible repair
  (`return` for `ret`, a `;`, `elif`, `'text'`, `xs.length()`, `string`, `Point { x: 1 }`, ...) carries
  a machine-applicable fix, and `--fix` applies them all: a slip costs no extra model call. How much
  that helps is in the [benchmark](bench/published): Sonnet 96% to 100%, Haiku 66% to 67%.
- **Examples as part of the type check.** `fn sq(x: int) -> int = x * x  ex sq(3) == 9`: every `ex`
  condition is evaluated while the program compiles and never compiled into it, so a function that is
  wrong for its own examples is a compile error that shows the value it really gave.

More on who Nyra is for, what it claims and what it does not: [`docs/POSITIONING.md`](docs/POSITIONING.md).

**A language a model can write.** Most languages are designed for people to read. Nyra is designed for
an AI to *write*, and for the compiler to tell the AI exactly what to fix.

- **Familiar tokens, tiny grammar.** It reads like Rust, Go and TypeScript, so a model half-knows it
  already, and the whole language fits in one prompt ([`docs/SPEC.md`](docs/SPEC.md)).
- **One way to do each thing.** No implicit conversions, no shadowing, no null, no semicolons. Fewer
  choices mean fewer wrong ones.
- **Errors are an API.** Every error has a stable code, an exact position and a fix hint, and `--json`
  makes them machine-readable, so an agent can loop *write, check, fix, run* without a human. Each message says
  what was expected and what was found, and `nyra explain E0201` explains any code with a wrong and a fixed
  program ([`docs/ERRORS.md`](docs/ERRORS.md)).
- **Examples catch logic mistakes before anything runs.** `fn sq(x: int) -> int = x * x  ex sq(3) == 9`:
  every `ex` condition is evaluated while the program compiles and never compiled into it, so a function
  that is wrong for its own examples is a compile error that shows the value it really gave.
  `ex for n in 0..200: f(n) >= 0` checks a property for many inputs and names the first one that fails.
- **Safe to run unsupervised.** Capabilities are decided before a program runs: `fs`, `os` and `input`
  must be granted (`--allow fs`), or the program does not compile (E0290). `--sandbox` runs it in an
  interpreter with limits on steps, memory, output and recursion, each with its own error code
  ([below](#safe-to-run-unsupervised)).
- **The compiler repairs simple mistakes itself.** An error with exactly one possible repair (`return` for
  `ret`, a `;`, `elif`, `'text'`, `xs.length()`, `string`, `Point { x: 1 }`, ...) carries a machine-applicable
  fix, and `--fix` applies them all: a slip costs no extra model call.

- **The compiler repairs simple mistakes itself.** An error with exactly one possible repair (a `;`, `elif`,
  `'text'`, `xs.length()`, `string`, `Point { x: 1 }`, ...) carries a machine-applicable fix. `check`, `run`
  and `build` apply those fixes in memory and go on, reporting each as a warning, and `--fix` writes them to
  the file: a slip costs no extra model call.
- **It accepts what models write anyway.** `return` (and `ret`), `c ? a : b`, a `fn main` next to top-level
  statements, `groups[k].push(x)` on a map value.
- **Compact programs.** One-line functions, string interpolation, `if` as a value, lambdas and built-in
  methods on strings and arrays keep code short. [`bench/`](bench) measures first-try correctness and
  token count against Python, TypeScript and Rust.
- **Values, not references.** Arrays, strings and structs are copied on assignment (cheaply, copy on
  write), so nothing changes behind your back; a function changes a caller's variable only through an
  `inout` parameter that the call names too.
- **Same output everywhere.** Every example runs on every backend in CI and must match byte for byte.
  The one exception is ints beyond 2^53: JavaScript and TypeScript cannot hold them exactly, so there a
  program stops with a runtime error (E0255 for a 64-bit overflow, which every backend reports;
  E0256 only on JavaScript) instead of printing a rounded number. Use the native, Rust, Go or Python
  target for such programs.

## Honest numbers

Measured on 2026-10-09 with Claude Opus 5.5, Sonnet 5.5 and Haiku 4.5: 83 deterministic tasks, four
languages, one sample per task. Full tables, per-category results and the failures are in
[`bench/published/`](bench/published/2026-10-v0.5-anthropic.md).

| First-try pass | Nyra | Python | TypeScript | Rust |
|---|---|---|---|---|
| Opus 5.5 | 98% | 100% | 100% | 100% |
| Sonnet 5.5 | 96% | 99% | 98% | 99% |
| Haiku 4.5 | 66% | 78% | 86% | 76% |

| Code tokens of the first attempt | Nyra | Python | TypeScript | Rust |
|---|---|---|---|---|
| Opus 5.5 | 475 | 375 | 492 | 523 |
| Sonnet 5.5 | 408 | 338 | 439 | 523 |
| Haiku 4.5 | 638 | 519 | 638 | 665 |

| Runtime on the speed tasks (median ms) | Nyra | Python | TypeScript | Rust |
|---|---|---|---|---|
| Opus 5.5 | 5.4 | 272 | 63 | 2.9 |
| Sonnet 5.5 | 6.4 | 373 | 69 | 5.0 |
| Haiku 4.5 | 32 | 573 | 73 | 10 |

What the numbers do not say:

- **Cost is near Python, not below it.** Nyra uses about 1.2-1.3x Python's code tokens, and more billed
  output tokens (Opus 691 vs 433, thinking included). Its prompt carries the spec, and a run costs several
  times Python's in this benchmark (Opus $0.062 vs $0.013). It is shorter than Rust in code tokens only.
- **Speed is Rust's class, not Rust.** Rust was faster on the speed tasks for every model, and the programs
  were written by the models, so they differ per language.
- **Haiku 4.5 is not there.** 66% against Python's 78% on the first try, 82% against 93% within 3 repairs.
- **The tasks are small** and were written by the people who build Nyra. Nyra gets its spec in the
  prompt, and the other languages rely on what the model learned in training.
- **Safety is not in this table.** Capabilities are not built yet, so nothing here measures them.

## Features

- **Two supported targets:** native executables through C99 (`gcc`, `clang` or `tcc`) and JavaScript (Node.js or the
  browser); readable C source too. [TypeScript, Python, Rust and Go](#targets) output is experimental.
- **Strict static types:** `int`, `float`, `bool`, `str`, `char`, arrays, maps and structs, with local
  inference and no implicit conversions. Ints are 64-bit and never wrap: an overflow is a runtime error.
- **Real data:** structs, arrays, maps, strings and chars with methods (`split`, `replace`, `slice`,
  `sort`, `join`, ...), lambdas (`xs.map(x => x * 2)`) and comprehensions, `inout` parameters, `break`
  and `continue`. Memory is freed by reference counting, with no garbage collector, and `free`, `arena`
  and `keep` say when if you want to.
- **Short code:** scripts without `fn main` (or with one: the statements run first), one-line functions,
  `c ? a : b`, `+=` and friends, string interpolation,
  `if` as a value, `print(a, b)`.
- **Capabilities and a sandbox:** the `use` lines are the program's permissions; `--allow fs,os` and
  `--sandbox` deny what a run does not grant at compile time, and the sandboxed interpreter stops runaway
  programs with a code and an exit code (`--fuel`, `--max-memory`, `--max-output`, `--max-depth`).
- **Agent-friendly tooling:** `run`, `build` and `check`, errors as JSON, `explain` for every error code,
  runtime errors with the exact position, and a build cache that skips the C compiler when the program
  has not changed.
- **Fast and small:** the compiler takes about a millisecond per file and the C compiler 0.5 to 1.5 s
  (measured on the author's PC). It is Rust with zero dependencies and writes plain, readable code.
- **Standard library:** `use input`, `os`, `fs`, `json`, `time`, `random`, `math`, `text`, the same on every
  backend (`math` gives identical digits everywhere). Maps `[K: V]` are values like arrays. **Not yet:** modules of your own (see the [roadmap](#roadmap)). Nyra is 0.x, so the
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
nyra test examples/inline_examples.nyra   # run the `ex` examples and count what passed
```

Save the program above as `scores.nyra` and run `nyra scores.nyra`. You never pass flags to the C
compiler: Nyra calls it with `-fwrapv -ffp-contract=off` (plus `-s` to strip the executable) and `-O2` for
`build`; `run` uses `-O1` so the first output comes sooner (`run --release` is `-O2`). It
caches the result, so running an unchanged program again skips the C compiler. Editor support: syntax
highlighting for VS Code is in [`editors/vscode`](editors/vscode).

## Using Nyra with an AI

Nyra is brand new, so no model has seen it in training. It does not need to: a model learns it from the
docs, and the compiler corrects what is left. **Paste this repo's URL into any AI that can read links**
(Gemini, ChatGPT, Claude, ...) and ask for a program:

> Read https://github.com/SpAkXD/nyra, starting with `llms.txt` and `docs/AI_GUIDE.md`, then write me
> a Nyra program that prints the first 20 prime numbers.

| The AI reads | What it gets | Size |
|---|---|---|
| [`llms.txt`](llms.txt) | what Nyra is, the ten most important rules, links ([llmstxt.org](https://llmstxt.org) convention) | 10 KB, about 2,500 tokens |
| [`docs/AGENT_CARD.md`](docs/AGENT_CARD.md) | the **agent card**: one dense example program, the rules that differ from other languages, what is not in Nyra, every method and module name. It has a hard budget (the header says how many tokens it has) | 3 KB, 1,400 tokens or less on Claude's tokenizer |
| [`docs/SPEC.md`](docs/SPEC.md) | the complete language spec | 20 KB, about 8,200 tokens on Claude's tokenizer |
| [`docs/AI_GUIDE.md`](docs/AI_GUIDE.md) | workflow, do/don't rules, what does not exist, error codes with fixes, complete programs | 40 KB, about 10,000 tokens |
| [`docs/ERRORS.md`](docs/ERRORS.md) | the error database: every code, what it means, why, the usual causes, a wrong and a fixed program | 114 KB: look codes up one at a time with `nyra explain` |
| [`examples/`](examples) | runnable programs, each with its expected output in a `.out` file | |
| [`docs/DESIGN.md`](docs/DESIGN.md) | what was deliberately not done yet, and why (for people changing the language) | 4 KB |

The token counts of the card and the spec are measured with Anthropic's `count_tokens` endpoint (the
others are estimates, about 3.7 characters per token); the spec alone is enough to write programs, and
the guide pays off when a model writes a lot of Nyra.

**Putting Nyra into your own prompts or agents: send the card (or the spec) as a cached system block.**
The spec is most of the input of every request that writes Nyra, so put the **card**
([`docs/AGENT_CARD.md`](docs/AGENT_CARD.md), about 1,400 tokens) in a system block of its own, the same
bytes in every request (the task goes in the user message), and mark that block for prompt caching.
With the Anthropic API:

```python
system = [{"type": "text", "text": card, "cache_control": {"type": "ephemeral"}}]
client.messages.create(model=..., system=system, messages=[{"role": "user", "content": task}], ...)
```

A cache read costs 0.05x to 0.1x of the input price, a write 1.25x, and an entry is readable only once
the request that wrote it has started to answer: send one request first when you run many in parallel.
The minimum cacheable prefix is 512 tokens on Opus 5.5 and Sonnet 5.5 but 4,096 on Haiku 4.5, so the
card is not cached there (it is cheap enough uncached). [`bench/`](bench/README.md) does exactly this
(`--spec card`). Measured on 38 tasks ([`research/AB-card.md`](research/AB-card.md)): on Sonnet 5.5 the card
wrote as many correct programs as the full spec (30 of 38 each) from 23% of the input; on Haiku 4.5 it was
worse (10 against 14), so a small model is better served by the **full spec, cached**, which cut the cost of
a run by 41% (Haiku) to 52% (Sonnet). The MCP server's `nyra_spec` returns the card by default and the
complete spec with `full: true`.

An agent that can run commands follows one loop: **write, check, fix, run**.

```
nyra check prog.nyra --json --fix   # repairs what has a certain fix, then -> {"ok":true,...} or the errors left
# fix each remaining error using its code, position and hint; repeat until "ok":true
nyra explain E0210 --json           # if the hint is not enough: what the code means, why, causes, wrong and fixed program
nyra run prog.nyra
```

### Self-repair with `--fix`

Many mistakes have exactly one possible repair: a `;` goes, `elif` is `else if`,
`and` is `&&`, `True` is `true`, `'hello'` is `"hello"`, `xs.length()` is `xs.len()`, `string` is `str`,
`Point { x: 1 }` is `Point(x: 1)`, `5.` is `5.0`, `print "hi"` is `print("hi")`. Such an error carries a
**fix**. `nyra check`, `run`, `build` and `test` apply every fix **in memory**, check again (a few rounds,
since fixing the syntax can reveal a type error with its own fix) and go on with the repaired program;
each repair is a warning on stderr (with `--json`, `check` prints a `warnings` array with the `code`,
position and applied text of each one), and the file is not written. `--fix` writes the file back when it
then compiles (and prints the edits to stderr as a diff); `--strict` keeps every error an error. If an
error without a fix remains, the errors are reported as usual. A program that compiles is never touched.
`nyra fmt prog.nyra` rewrites a file into canonical form: the fixes applied, `return` for `ret`, four-space
indentation. It never changes what the program does.

**Agents should run `--fix` (or apply the JSON `fix` themselves) before asking a model to repair a
program**: a mistake with a fix then costs no model call and no tokens. In the JSON form the fix is a
list of edits, each replacing the text from `line`:`col` up to (not including) `end_line`:`end_col`
with `text` (columns count characters, from 1):

```
{"code":"E0101","message":"`elif` is not part of Nyra: ...","line":4,"col":7,"hint":"write `else if` (two words) ...",
 "fix":[{"line":4,"col":7,"end_line":4,"end_col":11,"text":"else if"}]}
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
| `nyra_spec` | the agent card, about 1,400 tokens (`full: true`: the complete spec; `part: "guide"`: the AI guide), so the agent learns Nyra in one call |
| `nyra_check` | `{code}` → the same JSON as `nyra check --json` |
| `nyra_test` | `{code}` → the same JSON as `nyra test --json`: every `ex` example, with the values of a false one |
| `nyra_run` | `{code, backend?: "native"\|"js", stdin?, timeout_ms?}` → `{ok, exit, stdout, errors?, ms}` (10 s timeout, output capped) |
| `nyra_spec` | the language spec (`part: "guide"`: the AI guide), so the agent learns Nyra in one call |
| `nyra_check` | `{code, allow?}` → the same JSON as `nyra check --json` (with the capabilities `nyra_run` would grant) |
| `nyra_test` | `{code, allow?}` → the same JSON as `nyra test --json`: every `ex` example, with the values of a false one |
| `nyra_run` | `{code, backend?: "native"\|"js", stdin?, timeout_ms?, allow?: ["fs", "os", "input", "net"], sandbox?, fuel?, max_memory?, max_output?, args?}` → `{ok, exit, stdout, errors?, ms}` (10 s timeout, output capped; grants only `input` unless `allow` says more; `sandbox: true` runs it in the interpreter with limits, no child process) |
| `nyra_explain` | `{code: "E0201"}` → the error database entry (without `code`: every code) |
| `nyra_build` | `{code, target?: "c"\|"js"}` → the generated C or JavaScript |
| `nyra_outline` | `{path}` or `{code}` → one line per function and struct with its line range |
| `nyra_show` | `{path or code, name: "find Item.tags"}` → the source of those symbols |
| `nyra_edit` | `{path or code, edits, force?, fix?}` → change symbols by name ([below](#editing-by-symbol)); a path is written in place and only a summary returns |

Resources: `nyra://card`, `nyra://spec`, `nyra://guide`, `nyra://errors` (the error index) and `nyra://errors/{code}`.

**Limits and safety.** Each tool call runs on its own thread, so input that crashes the compiler gets
an error reply and the server keeps going. A program started by `nyra_run` is stopped after its
timeout and gets at most 1 GiB of memory and a CPU-time budget (a Job Object on Windows, `setrlimit`
on Linux and macOS; macOS does not enforce the memory limit). These limits protect the machine from
a runaway program, but they are no sandbox: the program can read and write files and use the network
like any process of yours. For code you do not trust, run `nyra mcp` inside a container or VM. Capabilities that deny a program everything it did not declare (`--allow fs`) are planned for v0.6; until they ship, a container or VM is the boundary.
a runaway program. A program that `nyra_run` starts only gets the capabilities in its `allow` list
(standard input by default): a `use fs` or `use os` without them is a compile error (E0290). With
`sandbox: true` the program does not start a process at all: it runs in the interpreter, with limits on
steps (`fuel`), memory (`max_memory`) and output that stop it with E0355 to E0357 at the same place on
every machine, and with its files confined to the working folder. A compiled program (`sandbox` false)
is still an ordinary process of yours once it has its capabilities: for code you do not trust, use
`sandbox: true`, or run `nyra mcp` inside a container or VM.

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

## Safe to run unsupervised

Nyra is meant for agents that run the code they write, so a run decides what a program may touch
*before* it starts, and a program that runs away is stopped by a limit with an error code.

**Capabilities, deny by default where it matters.** The `use` lines already say which standard modules a
program touches. `json`, `math`, `text`, `time` and `random` are always available. `fs` (files), `input`
(standard input), `os` (arguments, environment variables, `exit`) and later `net` are *effectful*: they need
the capability of the same name, and a `use` of one that the run does not grant is a compile error that
names the module, the capability and the flag to add. Nothing has run yet.

```
$ nyra run scan.nyra --sandbox
error[E0290]: module `fs` needs the capability `fs` (read, write, list and remove files and folders), which is not granted: this run grants none
  --> scan.nyra:1:5
  |
1 | use fs
  |     ^
  = hint: grant it by running with `--allow fs` (`nyra run main.nyra --allow fs`), or remove `use fs` and the code that calls it; the capabilities are `fs`, `input`, `net` and `os`
$ nyra run scan.nyra --sandbox --allow fs
```

| You run | It grants |
|---|---|
| `nyra run f.nyra` | everything, as before |
| `nyra run f.nyra --allow fs,os` | only those (`all` and `none` work too) |
| `nyra run f.nyra --sandbox` | nothing but what `--allow` names, in the interpreter, with the limits below |
| MCP `nyra_run` | standard input, plus its `allow: ["fs", ...]` argument |

`nyra outline --json` lists the capabilities each function needs, also through the functions it calls
(`"effects": ["fs"]`; the top-level `"capabilities"` is the `--allow` list that runs the whole program), so an
agent sees what a file may do before it runs it:

```
$ nyra outline scan.nyra
scan.nyra: 18 lines
3-3 fn read_all(path: str) -> str  [needs fs]
4-6 fn count_lines(path: str) -> int  [needs fs]
7-9 fn plain(x: int) -> int
10-18 fn main()  [needs fs, os]
```

**A sandboxed interpreter.** `--sandbox` (and `--interp`, which keeps all capabilities) runs the program
in the compiler's own interpreter of the intermediate representation: no child process, no C compiler, no
Node.js. It runs every program, maps and the whole standard library included, and prints exactly what the
compiled targets print (CI compares every example in both). Each limit that is reached ends the program with
a runtime error that has a code and its own exit code, after the output printed so far:

| Limit | Flag (default) | Error | Exit |
|---|---|---|---|
| steps run (statements, plus the elements and characters operations make) | `--fuel N` (2,000,000,000) | E0355 | 120 |
| heap memory | `--max-memory 256M` (512M) | E0356 | 121 |
| bytes printed | `--max-output 1M` (64M) | E0357 | 122 |
| nested calls | `--max-depth N` (20,000) | E0358 | 123 |
| real time | `--max-time MS` (none) | E0359 | 124 |

Steps are counted rather than timed, so the same program with the same fuel stops at the same statement on
every machine. In the sandbox file paths stay below the working folder (an absolute path or `..` is E0340),
and `time.sleep_ms` does not wait: it moves a virtual clock and costs steps. The MCP tool `nyra_run` takes
`sandbox: true`, `fuel`, `max_memory` and `max_output` for the same thing. A limit flag alone (`--fuel 5000`)
selects the interpreter.

## Language tour

```rust
struct Point { x: int, y: int }                 // a struct: named fields, uppercase name

fn add(a: int, b: int) -> int = a + b          // one-line function: the expression is the result
fn greet(name: str) = print("hi {name}")       // no `->`: returns nothing
fn dist2(p: Point) -> int = p.x * p.x + p.y * p.y

fn gcd(a: int, b: int) -> int {                // block body: `return` returns
    if b == 0 { return a }
    return gcd(b, a % b)
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
- **Scripts:** without `fn main`, the top-level statements are the program, and their `let`/`var`
  variables are visible in every function: `var pos = 0` then `fn advance() { pos += 1 }`. With a `fn main`
  too, the statements run first and then `main()` is called.
- **Conditional value:** `c ? a : b` is `if c { a } else { b }` (a `bool` condition, one type for both
  branches); it nests to the right.
- **Strings** are UTF-8 and count characters: `s[i]` is a `char` (`'a'`), `+` joins two strings, and
  `==` and `<` compare them by content. Arrays and structs print as Nyra code.
- **Conditions must be `bool`:** `if n != 0`, not `if n`.
- **Builtins:** `print(a, b)` (a last `end: ""` replaces the newline), `str(x)`, `int(x)`, `float(x)`,
  `char(n)`; everything else is a method, like `s.split(",")`, or a module function after `use`, like
  `math.sqrt(x)`, `input.line()`, `fs.read(path)` or `json.parse(text)`.
- **Operators**, high to low: calls, fields, indexes, methods · `-` `!` · `*` `/` `%` · `+` `-` ·
  `<` `<=` `>` `>=` · `==` `!=` · `&&` · `||`.

The complete reference is [`docs/SPEC.md`](docs/SPEC.md); [`docs/AI_GUIDE.md`](docs/AI_GUIDE.md) adds
every common mistake with its error code and fix.

## Targets

One Nyra file compiles to a native executable (`native`, through C) or to JavaScript (`js`); `c` writes the generated C.
These are the **supported** targets. The Python, TypeScript, Rust and Go targets are **experimental**: they are
tested on every example and kept in step with the language, but they are not where the effort goes, they may change
or lose features, and the benchmark, the docs and the error messages are written for native and JavaScript. All of them come from the same intermediate representation, so
the output, the order things happen in and the runtime errors (code, message, position, exit code 101)
are the same on each; the tests run every example on every target that is installed.

| Target | `--target` | `nyra build` writes | `nyra run` needs |
|---|---|---|---|
| native | `native` (default) | an executable | a C compiler (`gcc`, `clang`, `cc`, `tcc`; `NYRA_CC`) |
| C | `c` | `file.c` | (same as native) |
| JavaScript | `js` (`--js`) | `file.js` | Node.js |
| Python (experimental) | `py` (`--py`) | `file.py` | Python 3.8+ (`python3` or `python`; `NYRA_PYTHON`) |
| TypeScript (experimental) | `ts` (`--ts`) | `file.ts` | Node.js 22.6+ (it strips the types) |
| Rust (experimental) | `rs` (`--rs`) | `file.rs` | `rustc` (`NYRA_RUSTC`) |
| Go (experimental) | `go` (`--go`) | `file.go` | Go 1.23+ (`go`; `NYRA_GO`) |

```sh
nyra build --target py scores.nyra        # writes scores.py
nyra build --target rs scores.nyra -o -   # prints the Rust code
nyra run --go scores.nyra                 # builds with `go build` (cached) and runs it
```

The generated code reads like code a person would write in that language: typed signatures, variables
declared where they are first needed, counted loops as `for` loops, structs as classes or structs, and a
small runtime at the end of the file for what the language does differently (checked 64-bit ints,
character-based string indexes, JavaScript's number format, checked indexes). Values keep Nyra's
semantics: arrays and structs are copied on write (a shared mark in Python, TypeScript and Go,
`Rc::make_mut` in Rust), and an `inout` parameter becomes a returned value in Python
(`x, y = swap(x, y)`), a `{ v }` box in TypeScript, `&mut T` in Rust and a pointer in Go. Build the Rust
file with overflow checks off (`rustc -O -C overflow-checks=off`, as `nyra run` does): the generated code
checks the int operations that can overflow itself, with Nyra's error (E0255).
See [known differences](docs/SPEC.md#known-differences-between-backends) for the few edge cases.

## CLI

| Command | What it does |
|---|---|
| `nyra run <file>` | compile and run (`nyra <file>` is the same) |
| `nyra build <file>` | compile to a native executable |
| `nyra check <file>` | type-check only (and evaluate the `ex` examples); exit code 0 means no errors |
| `nyra test <file>` | run the `ex` examples and report each one that fails; exit code 0 means all passed |
| `nyra fmt <file>` | rewrite the file in canonical form: the fixes errors carry applied, `return` for `ret`, four-space indentation; the program does the same afterwards (`-o -` prints instead) |
| `nyra explain [CODE]` | explain an error code (what it means, why, causes, a wrong and a fixed program); without a code, list the codes it reports (`--planned` adds those of future designs) |
| `nyra mcp` | run the [MCP server](#mcp-server) on stdin/stdout, for AI agents |
| `nyra outline <file>` | the functions and structs with signatures, fields and line ranges (`--json` too) |
| `nyra show <file> <name>...` | the source of functions, structs or fields (`Struct.field`) |
| `nyra edit <file> [edits]` | [change symbols by name](#editing-by-symbol): `--set`, `--add`, `--delete`, `--rename`, `--add-field`, or an edit script on stdin; `--force`, `--fix`, `--dry-run`, `--json` |

| Option | Meaning |
|---|---|
| `--target <t>` | the [target](#targets): `native` (default), `c`, `js`; experimental: `py`, `ts`, `rs`, `go` |
| `--js` `--py` `--ts` `--rs` `--go` | short for `--target js` and so on (`--py`, `--ts`, `--rs` and `--go` are experimental) |
| `--c` | with `build`: write the generated C instead of an executable |
| `-o <path>` | output path for `build` (`-o -` prints to stdout) |
| `--json` | print errors as JSON (compile and runtime errors), for AI agents and tools; with `explain`, print the entry as JSON |
| `--fix` | with `check`, `run` and `build`: apply the fixes that errors carry, check again, and write the file back if it then compiles |
| `--strict` | errors stay errors: without it `check`, `run`, `build` and `test` repair, in memory, an error that has exactly one certain fix and report it as a warning |
| `--release` | with `run`: compile the generated C with `-O2` like `build` does (`run` is `-O1` by default: a shorter compile, about the same speed) |
| `--time` | show how long each step took |
| `--allow fs,os` | grant only these [capabilities](#safe-to-run-unsupervised) (`fs`, `input`, `os`, `net`; `all`, `none`) |
| `--sandbox` | run in the interpreter, granting nothing but `--allow`, with files confined to the working folder and the limits below |
| `--interp` | run in the interpreter (no C compiler or Node.js needed), with the limits |
| `--fuel N` `--max-memory S` `--max-output S` `--max-depth N` `--max-time MS` | the limits of an interpreted run; any of them implies `--interp` |

Exit codes: `0` success, `1` compile errors, `2` usage or tool problem, `101` runtime error (for example
an index out of bounds, which prints `runtime error[E0240]` with the file and position, identically on
every target), `120` to `124` a limit of the interpreter was reached (steps, memory, output, call depth, time). `NYRA_CC`, `NYRA_PYTHON`, `NYRA_RUSTC` and `NYRA_GO` select the tools.

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
| `src/check.rs`, `src/check/` | type checking, collects every error in one pass |
| `src/ir/` | the intermediate representation: evaluation order, runtime checks, reference counting, optimizations |
| `src/codegen/` | the backends: `c.rs`, `js.rs`, and the experimental `py.rs`, `ts.rs`, `rs.rs`, `go.rs`; `scope.rs` places declarations and finds counted loops for the last four |
| `src/ir/` | the intermediate representation: evaluation order, runtime checks, reference counting, optimizations; `interp.rs` (with `host.rs`, `jsonrt.rs`) interprets it |
| `src/caps.rs`, `src/sandbox.rs`, `src/mem.rs` | capabilities (E0290), the sandboxed run and its limits, the heap counter behind the memory limit |
| `src/codegen/` | the backends: `c.rs`, `js.rs`, `py.rs`, `ts.rs`, `rs.rs`, `go.rs`; `scope.rs` places declarations and finds counted loops for the last four |
| `src/rt/*/` | the runtimes they embed: strings, arrays, printing, runtime errors |
| `src/diag.rs`, `src/hints.rs` | errors for humans and JSON for agents, and the "what did you probably mean" hints |
| `src/fix.rs` | `--fix`: checks, applies and repeats the fixes that errors carry |
| `src/explain.rs`, `docs/ERRORS.md` | `nyra explain` and the error database it prints |

## Roadmap

| Version | Theme | Status |
|---|---|---|
| v0.1 | core language, C and JavaScript backends, JSON errors | done |
| v0.2 | short code: one-line functions, `+=`, string interpolation, `if` as a value, build cache, runtime errors | done |
| v0.3 | real data: structs, arrays, strings and chars with methods, `inout`, `break`/`continue`, memory model (no GC); the intermediate representation, the error database and `nyra explain` | done |
| v0.4 | `--fix` self-repair, the `nyra mcp` server, Python, TypeScript, Rust and Go backends, scripts, `print(a, b)`, the benchmark harness and its hard tier | done |
| v0.5 | lambdas and comprehensions, `ex` examples, `nyra outline`/`show`/`edit`, the standard library (`use math`, `fs`, `json`, ...), maps, script variables, native speed and compile-time evaluation, checked ints | in progress |
| v0.6 | capabilities (`--allow fs`, deny by default), modules of your own, packages, C FFI | planned |
| later | WASM backend and browser playground, published VS Code extension, docs site, 1.0 | planned |

Nyra is pre-1.0: the syntax may still change between versions (see [CHANGELOG.md](CHANGELOG.md)).

## Contributing

Issues and pull requests are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md) first; the short version
is that `cargo test` must stay green. If an AI could not write a program you expected it to, open an
issue with the program and the `nyra check --json` output: that is the most useful bug report.

## License

[MIT](LICENSE)
