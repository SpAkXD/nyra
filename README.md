<p align="center">
  <img src="assets/logo.svg" alt="Nyra" width="320">
</p>

<p align="center">
  <b>The language agents can run unsupervised.</b><br>
  Nearly as cheap to write as Python, as fast as Rust, and unable to touch anything it didn't declare.<br>
  A small, strict language designed to be written by AI agents. One source file compiles to a native executable
  (via C) or to JavaScript, and from v0.8 a <code>use web</code> program builds a whole website.
</p>

<p align="center">
  <a href="https://github.com/SpAkXD/nyra/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/SpAkXD/nyra/actions/workflows/ci.yml/badge.svg?branch=main"></a>
  <a href="LICENSE"><img alt="License: MIT" src="https://img.shields.io/badge/license-MIT-blue"></a>
  <a href="Cargo.toml"><img alt="Version" src="https://img.shields.io/badge/dynamic/toml?url=https%3A%2F%2Fraw.githubusercontent.com%2FSpAkXD%2Fnyra%2Fmain%2FCargo.toml&query=%24.package.version&label=version&color=7C5CFF"></a>
</p>

<p align="center">
  <a href="https://nyralang.dev">nyralang.dev</a> ·
  <a href="https://nyralang.dev/play/">Playground</a> ·
  <a href="https://nyralang.dev/benchmarks/">Benchmarks</a> ·
  <a href="#install">Install</a> ·
  <a href="#websites-in-nyra">Websites</a> ·
  <a href="#safe-to-run-unsupervised">Safety</a> ·
  <a href="#for-ai-agents">For AI agents</a> ·
  <a href="docs/SPEC.md">Spec</a> ·
  <a href="docs/ERRORS.md">Errors</a>
</p>

<p align="center">
  <sub>AI agent? Read <a href="https://raw.githubusercontent.com/SpAkXD/nyra/main/llms.txt">llms.txt</a> and the
  <a href="https://raw.githubusercontent.com/SpAkXD/nyra/main/docs/AGENT_CARD.md">agent card</a> before you write any
  Nyra. It is not Rust: code blocks here are marked <code>rust</code> only so GitHub colors them.</sub>
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

**Status.** The latest release is **v0.7.0** (capabilities, the sandbox, enums with values, the automatic
`nyra run`, the browser playground). **v0.8 is in progress and not released yet**: websites in Nyra (`use web`,
a component kit, motion, `nyra build` for sites and a dev server in `nyra run`), default parameter values, named
arguments and child blocks. Items marked *v0.8* below are in the source but in no release yet; build from source
to try them. Nyra is 0.x: the syntax may still change before 1.0 ([CHANGELOG.md](CHANGELOG.md)).

## What Nyra is

An agent that writes code can now also run it, and that is where the trouble starts: either you trust generated
code with your machine, or you start a VM or container for every call. Nyra is a language for that code-execution
step, designed for a model to *write* and for the compiler to tell the model exactly what to fix.

| | What you get | Status |
|---|---|---|
| **Safe to run** | A program can use only what it declares. Files, standard input and the OS are capabilities, denied by `--allow` and `--sandbox` before the program starts; `--sandbox` also runs it in an interpreter with limits | Built (v0.6) |
| **Fast** | Native code through C: about 5-6 ms against about 300 ms for Python on the benchmark's speed tasks, within about 1.2x of hand-written C on [`perf/`](perf). A short program is answered by the interpreter in milliseconds | Measured |
| **Near Python in cost** | About 1.2-1.3x Python's code tokens, shorter than Rust. Not cheaper than Python: see [Benchmarks](#benchmarks) | Measured |
| **Right on the first try** | On 83 tasks: Opus 98%, Sonnet 96%, Haiku 4.5 66% (Python: 100%, 99%, 78%). The compiler repairs the simple slips itself | Measured |
| **Websites** | `use web`: a whole site from one short program, prerendered HTML, a small CSS and JS file; about 0.15x the source tokens of the React original on a real site | *v0.8, not released yet* |

What makes a model good at it:

- **Familiar tokens, a small grammar.** It reads like Rust, Go and TypeScript, so a model half-knows it already,
  and the agent card that teaches it is about 1,400 tokens.
- **One way to do each thing.** No implicit conversions, no shadowing, no null, no semicolons.
- **Errors are an API.** Every error has a stable code, an exact position and a fix hint; `--json` makes them
  machine-readable and `nyra explain E0201` explains any code with a wrong and a fixed program
  ([`docs/ERRORS.md`](docs/ERRORS.md)).
- **The compiler repairs.** An error with exactly one possible repair (`elif`, a `;`, `'text'`, `xs.length()`,
  `Point { x: 1 }`, ...) is fixed in memory and reported as a warning; `--fix` writes the fix to the file. A slip
  costs no model call.
- **Examples are part of the type check.** `fn sq(x: int) -> int = x * x  ex sq(3) == 9`: every `ex` condition
  runs while the program compiles, so a function that is wrong for its own examples is a compile error that shows
  the value it really gave. `ex for n in 0..200: f(n) >= 0` checks a property.
- **Values, not references.** Arrays, strings, maps and structs are copied on assignment (cheaply, copy on
  write); a function changes a caller's variable only through an `inout` parameter that the call names too.
- **Same output everywhere.** Every example runs on every backend in CI and must match byte for byte.

Who it is for, what it claims and what it does not: [`docs/POSITIONING.md`](docs/POSITIONING.md).

## Install

**Prebuilt binary** (Linux x86_64, Windows x86_64, macOS Apple Silicon): download the archive for your platform
from the [latest release](https://github.com/SpAkXD/nyra/releases/latest), unpack it and put `nyra` on your
`PATH`. On macOS run `xattr -d com.apple.quarantine nyra` once (the binary is not signed). The release is v0.7.0;
for the v0.8 web features build from source.

**From source** (any platform with [Rust](https://rustup.rs); no crates are downloaded, the compiler uses only
Rust's standard library):

```
git clone https://github.com/SpAkXD/nyra
cd nyra
cargo build --release          # the binary is target/release/nyra
```

or `cargo install --git https://github.com/SpAkXD/nyra`. Check it with `nyra --version`.

To run programs natively Nyra needs a C compiler: `gcc`, `clang` or `tcc`, found automatically (`NYRA_CC` picks
one; on Windows, [MSYS2](https://www.msys2.org)'s gcc works out of the box). Without one, `nyra run` uses its
interpreter. The JavaScript backend (`--js`) needs [Node.js](https://nodejs.org). Websites need neither.

**Editors.** VS Code: syntax highlighting and the file icon are in [`editors/vscode`](editors/vscode). Windows:
give `.nyra` files the Nyra icon in Explorer (per user, no admin rights, your editor stays the default):

```
powershell -ExecutionPolicy Bypass -File tools\windows\install-file-icon.ps1
```

(`uninstall-file-icon.ps1` next to it undoes it.)

## Quick start

```
nyra run examples/hello.nyra          # run it: at once in the interpreter, natively if it runs long
nyra run examples/hello.nyra --js     # the same program on Node.js
nyra build examples/hello.nyra        # a native executable next to the source
nyra check examples/hello.nyra        # only report errors (and evaluate the `ex` examples)
nyra test examples/inline_examples.nyra   # run the `ex` examples and count what passed
nyra explain E0210                    # what an error code means, with a wrong and a fixed program
```

Save the program above as `scores.nyra` and run `nyra scores.nyra`. You never pass flags to the C compiler; a
build is cached, so running an unchanged program again skips it.

**No install: the [browser playground](https://nyralang.dev/play/).** The compiler is built to WebAssembly (with
no crates, `python tools/build_wasm.py`) and runs programs in its sandboxed interpreter in a Web Worker, with the
same errors, codes and hints as the command line.

## The language in one example

```rust
struct Point { x: int, y: int }                 // a struct: named fields, uppercase name

fn add(a: int, b: int) -> int = a + b          // one-line function: the expression is the result
fn greet(name: str) = print("hi {name}")       // no `->`: returns nothing
fn dist2(p: Point) -> int = p.x * p.x + p.y * p.y
ex dist2(Point(x: 3, y: 4)) == 25              // an example, checked while the program compiles

fn gcd(a: int, b: int) -> int {                // block body: `return` returns
    if b == 0 { return a }
    return gcd(b, a % b)
}

fn bump(inout n: int) { n += 1 }               // `inout`: may change the caller's variable
fn price(n: int, currency: str = "CZK") -> str = "{n} {currency}"   // a default value (v0.8)

enum Shape { Circle(float), Rect(float, float) }
fn area(s: Shape) -> float = match s {
    Shape.Circle(r) => 3.14 * r * r
    Shape.Rect(w, h) => w * h
}

let x = 5                                      // immutable, type inferred
var total = 0                                  // mutable
total += x
let size = if x > 3 { "big" } else { "small" } // `if` as a value: `else` is required
for i in 0..3 { print(i) }                     // 0 1 2: the end is exclusive

var xs = [3, 1, 2]                             // an array: [int]
xs.push(4)
xs.sort()                                      // [1, 2, 3, 4]
print(xs.map(v => v * v).sum(), [v for v in xs if v % 2 == 0])   // 30 [2, 4]

var ages: [str: int] = ["ada": 36]             // a map
ages["alan"] = 41
print(ages.get("grace") ?? 0, ages.len())      // 0 2: `get` gives an optional `int?`

let p = Point(x: 3, y: 4)                      // every field is named
print("{p} has dist2 {dist2(p)}")              // Point(x: 3, y: 4) has dist2 25
bump(inout total)                              // the call says `inout` too
print("sum: {add(x, 2)}, size: {size}, total: {total}, {float(x) / 2.0:.2}")
print(gcd(48, 18), price(5), price(7, currency: "EUR"), area(Shape.Rect(2.0, 3.0)))
greet("nyra")
```

- **Types:** `int` (64-bit, never wraps: an overflow is runtime error E0255), `float`, `bool`, `str`, `char`,
  arrays `[T]`, maps `[K: V]`, tuples `(int, str)`, optionals `int?`, structs and enums. Signatures are typed;
  locals are inferred. No implicit conversions: `1 + 2.0` is error E0210.
- **Scripts:** a program is its top-level statements, and their variables are visible in every function.
- **Standard library:** `use input`, `os`, `fs`, `json`, `time`, `random`, `math`, `text`, the same on every
  backend; files of your own with `use ./shapes` and `pub fn`.
- The complete reference is [`docs/SPEC.md`](docs/SPEC.md); [`docs/AI_GUIDE.md`](docs/AI_GUIDE.md) adds every
  common mistake with its error code and fix.

## Websites in Nyra

*In v0.8, not released yet.* A program with `use web` describes a site: `site(...)` sets the theme once, each
`page(...)` is one HTML page, and its child blocks hold sections, grids, cards and the kit components (`navbar`,
`hero`, `gallery`, `hours`, `map`, `form`, `footer`, ...). Motion is one word per element (`anim: "rise"`,
`hover: "lift"`, `scroll: "parallax"`). No HTML, CSS or JavaScript is written:

```nyra
use web

site("Bakery Hana", primary: "#7A3E1D", display: "Fraunces")

struct Bread { name: str, price: int }
let breads = [Bread(name: "Sourdough", price: 89), Bread(name: "Croissant", price: 35),
    Bread(name: "Baguette", price: 29)]

page("/", "Bakery Hana: fresh bread every morning") {
    navbar("Bakery Hana", [("Menu", "#menu"), ("Visit", "#visit")])
    hero("Fresh bread every morning", text: "Baked in Brno since 1991.", buttons: [("See the menu", "#menu")])
    section("menu", "Today's bread", bg: "alt") {
        grid(3, anim: "stagger", hover: "lift") {
            for b in breads { card(b.name, "{b.price} CZK", icon: "heart") }
        }
    }
    section("visit", "Visit us") {
        hours([("Mon-Fri", "1-5 7:00-18:00"), ("Saturday", "6 8:00-12:00")])
        map("Hlavni 1, Brno")
    }
}
```

```
nyra run bakery.nyra       # dev server on http://127.0.0.1:8000: rebuilds and reloads the page on save
nyra build bakery.nyra     # the site in dist/: index.html, site.<hash>.css, site.<hash>.js, robots.txt, _headers
nyra check bakery.nyra     # types plus the site's checks (broken #anchors, missing images, headings, contrast)
```

What you get: every page prerendered (it shows without JavaScript), one CSS file with the theme and only the
components used, a small JS file only when something is interactive (the navbar menu, the open/closed badge, the
map that loads on click), a computed Content-Security-Policy, and `prefers-reduced-motion` respected. Text is
always escaped, links are checked (`javascript:` is E0381, a broken `#anchor` E0382), and an unknown icon or motion
word is a compile error with the nearest name (E0374). Interactivity is the Elm pattern: state in script `var`s,
events as values of an `enum Msg`, one `fn update(m: Msg)`; the compiler finds the parts of the page to re-render.
The kit, motion and the build are described in [`docs/WEB.md`](docs/WEB.md); for models there is a web card,
`docs/WEB_CARD.md`, read together with the agent card.

**Measured on a real site.** [Flora](https://flora.ageofwords.site), a React + Tailwind site, rebuilt in Nyra with
the same features (the 3D showroom, the opening screen and the testimonials are left out of both):

| | React + Tailwind | Nyra |
|---|---:|---:|
| Source tokens | 34,928 | 5,209 (0.15x) |
| First contentful paint, throttled phone | 4.9 s | 2.4 s |
| JavaScript | 120 KB | 4.6 KB |
| Requests | 29 | 12 |

**A/B test: the same site brief, two answers.** Gemini 3.8 Flash (high effort) wrote the site once as plain
one-file HTML and once in Nyra. The HTML answer was 27,167 output tokens, the Nyra one 3,032 (0.11x; Claude's
tokenizer), and the Nyra file compiled on the first try. The Nyra prompt is larger, 6,233 input tokens against
2,684, because it carries the two reference cards.

Four more models on OpenRouter, three runs each at medium reasoning effort (code tokens on Claude's tokenizer; time
and cost as billed, averages):

| Model | Code tokens, HTML → Nyra | Time, HTML → Nyra | Cost, HTML → Nyra | Nyra built first try |
|---|---|---|---|:-:|
| DeepSeek v4.1 Flash | 20,685 → 2,927 (0.14x) | 126 s → 33 s | $0.0031 → $0.0091 | 1/3 |
| GPT-6 Luna | 20,224 → 3,037 (0.15x) | 109 s → 44 s | $0.0074 → $0.0025 | 1/3 |
| GLM 5.3 | 15,252 → 3,043 (0.20x) | 45 s → 14 s | $0.0154 → $0.0051 | 2/3 |
| GLM 5.3 Flash | 10,796 → 2,968 (0.27x) | 31 s → 18 s | $0.0015 → $0.0008 | 1/3 |

The honest part: every HTML answer worked (13 of 13), but only 7 of 13 Nyra files built on the first try, each
failure a gap between the first web card and the kit, and the menu filter did not work in the Nyra builds (a kit
bug). DeepSeek's Nyra cost is higher because one run spent 11,500 tokens reasoning. These are being fixed, and the
test will be repeated with one repair round from the compiler's messages, the way agents use Nyra.

One site and a few model runs are not a benchmark: treat these as a first measurement, not a general ratio. The
design document's own samples give 0.19x against plain HTML/CSS/JS and 0.48x against markup that uses a
ready-made component kit ([Token measurement](docs/WEB.md#token-measurement)).

## Safe to run unsupervised

A run decides what a program may touch *before* it starts, and a program that runs away is stopped by a limit
with an error code.

**Capabilities.** The `use` lines are the program's permissions. `json`, `math`, `text`, `time` and `random` are
always available; `fs` (files), `input` (standard input) and `os` (arguments, environment, `exit`) need the
capability of the same name. A `use` that the run does not grant is a compile error that names the module, the
capability and the flag to add. Nothing has run yet:

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
| `nyra run f.nyra` | everything, as any program of yours |
| `nyra run f.nyra --allow fs,os` | only those (`all` and `none` work too) |
| `nyra run f.nyra --sandbox` | nothing but what `--allow` names, in the interpreter, with the limits below |
| MCP `nyra_run` | standard input, plus its `allow: ["fs", ...]` argument |
| `nyra build site.nyra` (`use web`) | nothing unless `--allow` names it; only the build writes, and only into its output folder |

`nyra outline --json` lists the capabilities each function needs, also through the functions it calls, so an
agent sees what a file may do before it runs it.

**A sandboxed interpreter.** `--sandbox` (and `--interp`, which keeps all capabilities) runs the program in the
compiler's own interpreter: no child process, no C compiler, no Node.js, the same output as the compiled targets.
In the sandbox, file paths stay below the working folder (E0340). Each limit ends the program with its own code
and exit code:

| Limit | Flag (default) | Error | Exit |
|---|---|---|---|
| steps run | `--fuel N` (2,000,000,000) | E0355 | 120 |
| heap memory | `--max-memory 256M` (512M) | E0356 | 121 |
| bytes printed | `--max-output 1M` (64M) | E0357 | 122 |
| nested calls | `--max-depth N` (20,000) | E0358 | 123 |
| real time | `--max-time MS` (none) | E0359 | 124 |

Steps are counted rather than timed, so the same program stops at the same statement on every machine. What
this is not: kernel isolation. A compiled program (no `--sandbox`) is an ordinary process once it has its
capabilities; for code you do not trust, use `--sandbox` (or `sandbox: true` over MCP), or run Nyra inside a
container or VM as well.

## Speed

- **Native code.** Nyra compiles through C (`-O2` for `build`, `-O1` for `run` so the first output comes sooner,
  `run --release` for `-O2`), with no garbage collector (reference counting, copy on write). Within about 1.2x
  of hand-written C on [`perf/`](perf).
- **No wait for small programs.** `nyra run` is automatic: a program that ends within a few million steps is
  answered by the interpreter in milliseconds while the C compiler starts in the background; a longer one runs
  natively, with the same output. A cached build runs at once. `--native` always compiles.
- **A fast compiler.** About a millisecond per file for Nyra itself; the C compiler takes 0.5 to 1.5 s once per
  program (measured on the author's PC; [`research/SPEED-run.md`](research/SPEED-run.md)).
- **Fast sites.** A `use web` site ships prerendered HTML and only the CSS and JS its pages use (Flora: 4.6 KB of
  JS, see [above](#websites-in-nyra)).

## Benchmarks

Measured on 2026-10-09 with Claude Opus 5.5, Sonnet 5.5 and Haiku 4.5: 83 deterministic tasks, four languages,
one sample per task. Charts: [nyralang.dev/benchmarks](https://nyralang.dev/benchmarks/); tables, per-category
results and the failures: [`bench/published/`](bench/published/2026-10-v0.5-anthropic.md).

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

- **Cost is near Python, not below it.** Nyra uses about 1.2-1.3x Python's code tokens and more billed output
  tokens (Opus 691 vs 433, thinking included). Its prompt carries the spec, and a run cost several times Python's
  in this benchmark (Opus $0.062 vs $0.013). It is shorter than Rust in code tokens only.
- **Speed is Rust's class, not Rust.** Rust was faster on the speed tasks for every model, and the programs were
  written by the models, so they differ per language.
- **Haiku 4.5 is not there.** 66% against Python's 78% on the first try, 82% against 93% within 3 repairs.
- **The tasks are small** and were written by the people who build Nyra. Nyra gets its spec in the prompt; the
  other languages rely on what the model learned in training.
- **This benchmark is from v0.5.** Capabilities, the sandbox and the web are not measured in it. The website
  numbers are [above](#websites-in-nyra).

## For AI agents

Nyra is new, so no model has seen it in training. It does not need to: a model learns it from a short card, and
the compiler corrects what is left. **Paste this repo's URL into any AI that can read links** and ask for a
program:

> Read https://github.com/SpAkXD/nyra, starting with `llms.txt` and `docs/AGENT_CARD.md`, then write me a Nyra
> program that prints the first 20 prime numbers.

| The AI reads | What it gets | Size |
|---|---|---|
| [`llms.txt`](llms.txt) | what Nyra is, the most important rules, links ([llmstxt.org](https://llmstxt.org) convention) | about 2,500 tokens |
| [`docs/AGENT_CARD.md`](docs/AGENT_CARD.md) | the **agent card**: one dense example, the rules that differ from other languages, what is not in Nyra, every method and module name | 1,400 tokens or less (a hard budget, measured on Claude's tokenizer) |
| `docs/WEB_CARD.md` | the **web card** (v0.8): one complete page, the elements, kit components and motion words; read with the agent card | 1,400 tokens or less |
| [`docs/SPEC.md`](docs/SPEC.md) | the complete language spec | about 8,200 tokens |
| [`docs/AI_GUIDE.md`](docs/AI_GUIDE.md) | workflow, do/don't rules, what does not exist, error codes with fixes, complete programs | about 10,000 tokens |
| [`docs/ERRORS.md`](docs/ERRORS.md) | every error code: what it means, why, the usual causes, a wrong and a fixed program | look codes up one at a time with `nyra explain` |
| [`examples/`](examples) | runnable programs, each with its expected output in a `.out` file | |

### AGENTS.md for your project

[`docs/AGENTS.md`](docs/AGENTS.md) is a short instruction file for coding agents that build things **with** Nyra:
how to check and run, the edit-compile loop with error codes and `--fix`, capabilities, websites, the MCP setup,
the mistakes models make most and how to keep token use low. Copy it into the root of your own project as
`AGENTS.md`, the file Codex, Cursor, GitHub Copilot and other coding agents read on their own:

```
curl -o AGENTS.md https://raw.githubusercontent.com/SpAkXD/nyra/main/docs/AGENTS.md
```

Claude Code reads `CLAUDE.md` (and Gemini CLI `GEMINI.md`): one line there is enough, `Read AGENTS.md`.

### The loop: write, check, fix, run

```
nyra check prog.nyra --json --fix   # repairs what has a certain fix, then {"ok":true,...} or the errors left
# fix each remaining error using its code, position and hint; repeat until "ok":true
nyra explain E0210 --json           # if the hint is not enough: what the code means, causes, wrong and fixed program
nyra run prog.nyra
```

Many mistakes have exactly one possible repair: a `;` goes, `elif` is `else if`, `and` is `&&`, `True` is `true`,
`'hello'` is `"hello"`, `xs.length()` is `xs.len()`, `string` is `str`, `Point { x: 1 }` is `Point(x: 1)`. Such an
error carries a **fix**. `nyra check`, `run`, `build` and `test` apply every fix in memory, check again and go on,
reporting each repair as a warning (in `--json`: a `warnings` array); `--fix` writes the file back, `--strict`
keeps every error an error. In JSON a fix is a list of edits, each replacing the text from `line`:`col` up to (not
including) `end_line`:`end_col`:

```
{"code":"E0101","message":"`elif` is not part of Nyra: ...","line":4,"col":7,"hint":"write `else if` (two words) ...",
 "fix":[{"line":4,"col":7,"end_line":4,"end_col":11,"text":"else if"}]}
```

A fix is only given where it is certain; with alternatives (a typo, `null`, Go's `:=`) there is a hint and no fix.
`nyra fmt prog.nyra` rewrites a file into canonical form without changing what it does.

**In your own prompts:** send the card as a cached system block, the same bytes in every request (the task goes
in the user message). On Sonnet 5.5 the card wrote as many correct programs as the full spec from 23% of the
input; a small model (Haiku 4.5) is better served by the full spec, cached
([`research/AB-card.md`](research/AB-card.md)).

### MCP server

`nyra mcp` is a [Model Context Protocol](https://modelcontextprotocol.io) server built into the compiler: the agent
gets the card, the checker, the runner, the builder and the error database as tools, with no files or shell
access needed.

| Tool | What it does |
|---|---|
| `nyra_spec` | the agent card (`full: true`: the complete spec; `part: "guide"`: the AI guide) |
| `nyra_check` | `{code, allow?, strict?}` → the same JSON as `nyra check --json`; `files: {"main.nyra": ..., "shapes.nyra": ...}` replaces `code` for a program of several files (the other tools take it too) |
| `nyra_test` | `{code}` → every `ex` example, with the values of a false one |
| `nyra_run` | `{code, backend?, stdin?, timeout_ms?, allow?, sandbox?, fuel?, max_memory?, max_output?, args?}` → `{ok, exit, stdout, errors?, ms}`; grants only standard input unless `allow` says more; `sandbox: true` runs it in the interpreter with limits, no child process |
| `nyra_build` | `{code, target?: "c"\|"js"}` → the generated C or JavaScript; a `use web` program builds its site in memory (`{pages, files, warnings}`) |
| `nyra_explain` | `{code: "E0201"}` → the error database entry |
| `nyra_outline`, `nyra_show`, `nyra_edit` | the outline of a file, the source of named symbols, and [edits by symbol name](#editing-by-symbol) |

Resources: `nyra://card`, `nyra://spec`, `nyra://guide`, `nyra://errors` and `nyra://errors/{code}`.

**Claude Code:**

```
claude mcp add nyra -- nyra mcp
```

**Claude Desktop, Cursor, Gemini CLI and others** take the same `mcpServers` entry (Claude Desktop:
`claude_desktop_config.json`; Cursor: `.cursor/mcp.json` or `~/.cursor/mcp.json`; Gemini CLI:
`~/.gemini/settings.json`):

```json
{
  "mcpServers": {
    "nyra": { "command": "nyra", "args": ["mcp"] }
  }
}
```

If `nyra` is not on the `PATH` the client sees, give the full path to the binary as `command`. Each tool call
runs on its own thread, and a program started by `nyra_run` gets a timeout, at most 1 GiB of memory and a CPU-time
budget. For code you do not trust, use `sandbox: true`, or run `nyra mcp` inside a container or VM.

### Editing by symbol

An agent that changes one function of a long program should not send the program again. `nyra edit` (and the
`nyra_edit` tool) changes functions, structs, enums, fields and variants **by name**:

```
$ nyra outline shop.nyra
shop.nyra: 209 lines
4-10 struct Item { sku: str, name: str, price: int, stock: int, tags: [str] }
71-79 fn discount(total: int) -> int
$ nyra show shop.nyra discount            # just that function
$ nyra edit shop.nyra < new_discount.txt  # the new `fn discount ...`: replaces the old one
nyra: edited shop.nyra: replaced fn discount (lines 71-82)
$ nyra edit shop.nyra --rename Item.stock in_stock
nyra: edited shop.nyra: renamed field Item.stock -> in_stock, 7 references (line 8)
```

The rest of the file stays byte-identical, a rename follows every reference (never strings or comments), and an
edit that adds errors is refused. On a 209-line file, changing one function costs about 75 tokens instead of
about 1,450 for resending it.

## Targets

One Nyra file compiles to a native executable (`native`, through C) or to JavaScript (`js`); these are the
**supported** targets. `c` writes the generated C. Python, TypeScript, Rust and Go output (`--py`, `--ts`, `--rs`,
`--go`) is **experimental**: tested on every example, but the docs and the benchmark are written for native and
JavaScript. Every target comes from the same intermediate representation, so the output, the order things happen
in and the runtime errors are the same; the one exception is ints beyond 2^53, which JavaScript stops with E0256
instead of rounding. Details: [known differences](docs/SPEC.md#known-differences-between-backends).

## CLI

| Command | What it does |
|---|---|
| `nyra run <file> [-- args]` | run it (`nyra <file>` is the same): automatic interpreter or native, see [Speed](#speed). For a `use web` program: the dev server |
| `nyra build <file>` | a native executable (or the target's source); for a `use web` program, the site in `dist/` |
| `nyra check <file>` | type-check and evaluate the `ex` examples (and run a site's checks); exit code 0 means no errors |
| `nyra test <file>` | run the `ex` examples and report each one that fails |
| `nyra fmt <file>` | rewrite the file in canonical form (`-o -` prints instead) |
| `nyra explain [CODE]` | explain an error code; without a code, list them |
| `nyra mcp` | the [MCP server](#mcp-server) on stdin/stdout |
| `nyra outline <file>`, `nyra show <file> <name>`, `nyra edit <file>` | [work by symbol](#editing-by-symbol) (`nyra edit --help`) |

| Option | Meaning |
|---|---|
| `--target <t>`, `--js` | the target: `native` (default), `c`, `js`; experimental: `py`, `ts`, `rs`, `go` (`--py` and so on) |
| `--c`, `-o <path>` | with `build`: write the generated C; the output path (`-o -` prints to stdout; for a site, the folder) |
| `--json` | errors (and `explain`) as JSON |
| `--fix`, `--strict` | write the certain fixes back to the file; or keep every error an error |
| `--native`, `--release`, `--time` | with `run`: always compile; compile with `-O2`; show how long each step took |
| `--allow LIST`, `--sandbox`, `--interp` | [capabilities and the sandbox](#safe-to-run-unsupervised) |
| `--fuel` `--max-memory` `--max-output` `--max-depth` `--max-time` | the limits of an interpreted run (any of them implies `--interp`) |
| `--port N`, `--image-tool T` | websites: the dev server's port (`0`: any free one); `cwebp`, `magick`, `vips` or `none` for WebP image variants |

Exit codes: `0` success, `1` compile errors, `2` usage or tool problem, `101` runtime error (for example an index
out of bounds, E0240, with the file and position), `120` to `124` a limit of the interpreter. `NYRA_CC`,
`NYRA_PYTHON`, `NYRA_RUSTC` and `NYRA_GO` select the tools.

## Project layout

```
                                                         ┌─► C99 ──► gcc / clang ──► native executable
                                                         ├─► JavaScript ──► Node.js / browser
source.nyra ─► lexer ─► parser ─► type checker ─► IR ────┼─► interpreter (run, --sandbox, ex, web build)
                                                         └─► Python, TypeScript, Rust, Go (experimental)
```

| Path | What |
|---|---|
| `src/lexer.rs`, `src/parser.rs`, `src/ast.rs` | text to tokens to syntax tree (recursive descent, recovers after errors) |
| `src/check.rs`, `src/check/` | type checking; collects every error in one pass |
| `src/ir/` | the intermediate representation: evaluation order, runtime checks, reference counting, optimizations; `interp.rs` interprets it |
| `src/codegen/`, `src/rt/` | the backends and the runtimes they embed |
| `src/caps.rs`, `src/sandbox.rs`, `src/mem.rs` | capabilities (E0290), the sandboxed run and its limits |
| `src/diag.rs`, `src/hints.rs`, `src/fix.rs`, `src/explain.rs` | errors for people and JSON for agents, hints, `--fix`, `nyra explain` |
| `src/mcp.rs`, `src/edit.rs` | the MCP server; outline, show and edit by symbol |
| `src/web.rs`, `src/web/`, `src/webcheck.rs`, `src/std/web.nyra` | websites (v0.8): the build, the dev server, the kit and motion CSS/JS, the checks, the `web` library |
| `src/wasm.rs` | the compiler as WebAssembly, for the playground (`python tools/build_wasm.py`) |
| `docs/` | the spec, the agent and web cards, the AI guide, the error database, [`DESIGN.md`](docs/DESIGN.md), [`WEB.md`](docs/WEB.md), [`AGENTS.md`](docs/AGENTS.md) for your projects |
| `examples/`, `examples/web/` | runnable programs with their expected output; the web examples |
| `tests/` | `cargo test`: every example on every backend, every error code, the docs' code blocks, the web build |
| `bench/`, `perf/`, `research/` | the agent benchmark, the speed programs, measurements and notes |
| `editors/vscode/`, `tools/` | the VS Code extension; scripts (the Windows file icon, the WASM build, token counts) |

## Roadmap

| Version | Theme | Status |
|---|---|---|
| v0.1-v0.4 | the core language, C and JavaScript backends, JSON errors, `--fix`, the MCP server, the experimental backends | done |
| v0.5 | lambdas, comprehensions, `ex` examples, `outline`/`show`/`edit`, the standard library, maps, checked ints | done (released in v0.6.0) |
| v0.6 | capabilities and the sandbox, tuples, optionals, enums and `match`, format specifiers, modules of your own | done |
| v0.7 | enums with values, `match` and `if let` as values, JSON for every type, the automatic `nyra run`, the browser playground | done |
| v0.8 | websites: `use web`, the kit, motion, `nyra build` and the dev server; default values, named arguments, child blocks | in progress, not released yet |
| later | packages, a published VS Code extension, browser effects for sites, 1.0 | planned |

## Contributing

Issues and pull requests are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md) first; the short version is that
`cargo test` must stay green and the compiler takes no dependencies. If an AI could not write a program you
expected it to, open an issue with the program and the `nyra check --json` output: that is the most useful bug
report.

## License

[MIT](LICENSE)
