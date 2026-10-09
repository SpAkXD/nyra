# Changelog

Nyra is pre-1.0. Until 1.0 the language, its syntax and the command line may still change from one
version to the next; each entry says what changed. Error codes are stable: a number is never reused.

## v0.6 (unreleased)

- **Warnings.** A likely mistake that the language allows no longer passes silently: `"cost: ${x}"` (a
  `$` before a `{value}`) prints `warning[E0260]` to stderr and is listed under `"warnings"` in `--json`.
  Warnings never fail the build.
- Compile errors for guesses from other languages: a negative constant index `xs[-1]` or `s.slice(-2, 5)`
  (E0261), optional types `int?` and `Option<int>` (E0262), a `fn` inside a `struct` or an `impl` block
  (E0263) and `class` (E0264); each with a hint, and `class` with a fix.
- Fixed: the standard modules have a namespace of their own. A program with `use math` and a function
  `sign` (or a variable `x`, `lo`, `hi`, ...) failed with E0206 at a line of the bundled library; names
  inside a module never clash with the program's, and no diagnostic points outside the program's file.

## v0.5 (unreleased)

Language
- Lambdas for the array and string methods (`xs.map(x => x * 2)`, `filter`, `count`, `any`, `all`,
  `find_index`, `sort_by`, `fold`, `sum`, `min`, `max`), comprehensions (`[x * x for x in xs if x > 0]`)
  and `for i, x in xs`.
- `ex` examples (`fn sq(x: int) -> int = x * x  ex sq(3) == 9`), checked while compiling; `nyra test`.
- Maps `[K: V]` on every backend.
- Script variables: functions see and change a script's top-level `let`/`var`.
- The standard library: `use input`, `os`, `fs`, `json`, `time`, `random`, `math`, `text`;
  `print(..., end: "")`.
- Forgiving syntax found in real benchmark failures: a lone brace in a string is text (E0006 retired),
  a line may start with an operator, `stmt ret` on one line, nested `fn` definitions.
- **Ints never wrap.** An int overflow is runtime error E0255 on every backend. On JavaScript and
  TypeScript an int beyond 2^53 - 1 stops with E0256 instead of being rounded.
- Code nested more than 256 levels deep is compile error E0103 (it used to crash the compiler).

Tools
- `nyra outline`, `nyra show` and `nyra edit`: change a program by symbol, also over MCP.
- `nyra mcp` runs each tool call on its own thread, so bad input cannot stop the server, and `nyra_run`
  limits a program's memory (1 GiB) and CPU time.
- `nyra explain` lists only the codes the compiler reports; `--planned` adds the planned ones.
- Native speed: inline array fast paths, earlier releases, loop-hoisted checks, compile-time evaluation
  of pure calls; `perf/` compares native Nyra with hand-written C and Rust.
- The benchmark measures runtime, efficiency and self-repair too.

## v0.4.0 (2026-10-08)

- Four more backends: Python, TypeScript, Rust and Go (`--target`), tested on every example.
- `--fix`: errors with exactly one repair carry a machine-applicable fix, and `nyra check --fix`
  applies them.
- `nyra mcp`: a Model Context Protocol server with the spec, the checker, runs, builds and the error
  database as tools.
- Scripts (statements at the top level, no `fn main`), `print(a, b)`, `step` in ranges, `xs.swap`,
  `pad_left`/`pad_right`, char search in strings, the builtins `abs`, `min` and `max`.
- The benchmark's hard tier: 28 rule-dense tasks with Python, TypeScript, Rust and Nyra references.

## v0.3.0 (2026-10-07)

- Real data: strings and chars as values with methods, arrays, structs, `inout` parameters,
  `break` and `continue`.
- Memory without a garbage collector: reference counting with copy on write, and `free`, `keep` and
  `arena` to say when.
- An intermediate representation (IR) between the checker and the backends, with constant folding
  and dead-code removal; C and JavaScript are generated from it.
- The error database `docs/ERRORS.md` and `nyra explain`; every diagnostic says what was expected,
  what was found and how to fix it, with a golden test of every message.

## v0.2.0 (2026-10-07)

- Short code: one-line functions (`fn sq(x: int) -> int = x * x`), `+=` and the other compound
  assignments, string interpolation (`"x = {x}"`), `if` as a value.
- Runtime errors with a code and position, exit code 101 (division by zero, bad `int()`).
- The same output on both backends: numbers print identically, evaluation is left to right.
- A cache of native builds; `llms.txt` and the AI guide; the VS Code extension.

## v0.1 (2026-10-07)

- The first compiler: lexer, parser, type checker, C and JavaScript backends, errors as JSON.
