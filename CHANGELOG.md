# Changelog

Nyra is pre-1.0. Until 1.0 the language, its syntax and the command line may still change from one
version to the next; each entry says what changed. Error codes are stable: a number is never reused.

## v0.7.0 (2026-10-10)

Speed
- **`nyra run` is automatic.** It used to wait about half a second for the C compiler before the first line of
  output, even for a ten-line program. Now a cached build of the exact source runs as it is; any other program
  starts in the interpreter with its output kept and a budget of 4,000,000 steps and 350 ms, while the C compiler starts in
  the background after 15 ms. A program that ends within the budget prints at once (milliseconds); one that does
  not is dropped and run natively with the same input, so its output appears once. Programs that use `fs` or
  `os`, call `time.sleep_ms` or read a terminal always run natively; a program that answers a pipe as it goes is not made to wait
  for the end of its input. Output, exit codes and runtime errors are the same in every mode. `--native` forces
  the old way; `--release` implies it. Without a C compiler `run` interprets. The MCP tool `nyra_run` does the same
  (`"mode":"interp"` in its reply when the interpreter answered). See research/SPEED-run.md for the numbers.
- **A browser playground** (https://nyralang.dev/play/): the compiler builds to WebAssembly with no crates (`python tools/build_wasm.py`), and the page runs programs in the sandboxed interpreter in a Web Worker.

Language
- Enums that carry values (sum types): `enum Shape { Circle(float), Rect(float, float), Empty }`, built `Shape.Circle(2.0)`, taken apart with `match s { Shape.Circle(r) => ..., Shape.Rect(w, h) => ..., Shape.Empty => ... }` (every case must be covered, `_` skips a value). Values compare with `==` and print as `Shape.Circle(2)`. New codes E0286 (a variant written without its values), E0287 (a variant pattern that does not name its values), E0288 (`all()` of an enum with values); E0222 also covers an enum that contains itself.
- `match` and `if let` are values: `let area = match s { Shape.Circle(r) => 3.14 * r * r, _ => 0.0 }`, `return match d { ... }`, `let n = if let v = m.get(k) { v + 1 } else { 0 }`. Each arm is one expression (arms on lines of their own or separated by commas) and all arms have one type (E0212); the statement forms are unchanged.

Tooling
- `nyra outline`, `show` and `edit` (and the MCP tools `nyra_outline`, `nyra_show`, `nyra_edit`) know enums: the outline lists `enum Shape { Circle(float), Empty }` with its variants (`--json` has `variants`), `Enum.Variant` addresses one variant, and an edit can replace, add (`@add-variant Shape Tri(float)` / `--add-variant`), delete or rename an enum or a variant, with every reference. A rename now also follows the uses inside `ex` examples.
- Tuples as map keys: `var grid: [(int, int): char] = [:]`, `grid[(2, 1)] = 'b'`, `(2, 1) in grid`; the parts must be `int`, `str`, `char`, `bool` or such tuples (E0218 otherwise, and for a float part). They hash and compare by their parts on every target.
- `json.str` and `json.parse` work for every value type on every target: a tuple is an array (`[1,"a"]`), an optional the value or `null`, an enum variant its name (`"Empty"`) or `{"Rect":[1,2]}` with values, a map with `str` keys an object, any other map an array of `[key,value]` pairs. A shape that does not fit is E0345 with the path (`expected an array of 2 elements at $.a`). E0309 now only means that `json.parse` has no type to read.
- MCP: the tools `nyra_check`, `nyra_test`, `nyra_run` and `nyra_build` accept `files` (a map of file names to text) instead of `code`, so `use ./shapes` works over MCP (`entry` names the main file; `nyra_outline`, `nyra_show` and `nyra_edit` take `files` and `file`). They repair an error that has exactly one certain fix in memory, like the CLI, and return the repairs and the compiler's warnings under `warnings`; `strict: true` turns the repairs off.
- Fixed: an error inside an imported file of your own was reworded as a bug of a bundled standard module.

## v0.6.0 (2026-10-10)

Language
- Tuples: `(1, "a")`, `t.0`, `fn f() -> (int, bool)`, `let (a, b) = f()`, `(a, b) = (b, a)`, `for (k, v) in pairs`. They compare and sort part by part and print as `(1, "a")`. New codes E0272, E0273.
- Format specifiers in strings, Python's subset: `{x:>8}`, `{n:05}`, `{f:.2}` (rounded like `text.fixed`), `{n:,}`, `{f:>10.2}`. New codes E0270, E0271.
- Denser helpers: `xs.sorted()`, `sorted_by`, `min_by`, `max_by` (keys may be tuples), `x in xs`, `zip`, `chunks`, slices `xs[a..b]` / `s[a..b]`, `s.trim(chars)`, `m.items()`, `fn f(var n: int)`, and `r.area()` for `fn area(r: Rect)`. New code E0275.
- Optional values: `T?`, `none`, `x ?? default`, `if let v = x { }`, `unwrap()`; `m.get(k)` now gives a `V?` (`get(k, default)` is unchanged), plus `s.to_int()`, `s.to_float()` and `xs.find(x => test)`. New codes E0276, E0277, E0350.
- Enums and `match`: `enum Dir { N, E, S, W }`, values `Dir.N`, `Dir.all()`, and `match` on an enum (every case must be covered, `_` takes the rest), `bool`, `int`, `str` or `char`. New codes E0278, E0279, E0281, E0283, E0284.
- Modules of your own: `use ./name` imports `name.nyra` from the importing file's folder; its `pub fn`s are called `name.f(x)`, its `pub struct`s and `enum`s need no prefix; errors inside an imported file name that file. New codes E0285 (and E0301, E0303, E0304, E0305, E0332, planned until now, are emitted).

- **Warnings.** A likely mistake that the language allows no longer passes silently: `"cost: ${x}"` (a
  `$` before a `{value}`) prints `warning[E0260]` to stderr and is listed under `"warnings"` in `--json`.
  Warnings never fail the build.
- Compile errors for guesses from other languages: a negative constant index `xs[-1]` or `s.slice(-2, 5)`
  (E0261), optional types `int?` and `Option<int>` (E0262), a `fn` inside a `struct` or an `impl` block
  (E0263) and `class` (E0264); each with a hint, and `class` with a fix.
- Fixed: the standard modules have a namespace of their own. A program with `use math` and a function
  `sign` (or a variable `x`, `lo`, `hi`, ...) failed with E0206 at a line of the bundled library; names
  inside a module never clash with the program's, and no diagnostic points outside the program's file.

Safety (so that agents can run Nyra unsupervised)
- **Capabilities.** The `use` lines are the program's permissions: `json`, `math`, `text`, `time` and
  `random` are always allowed, `fs`, `input` and `os` (and later `net`) need a capability. A `use` of a
  module the run does not grant is error E0290, which names the module, the capability and the flag to
  add. `--allow fs,os` grants only those, `--sandbox` grants nothing but what `--allow` names, and plain
  `nyra run` grants everything as before. The MCP tool `nyra_run` grants only standard input unless its
  `allow` argument says more; `nyra_check` and `nyra_test` take `allow` too.
- `nyra outline --json` lists the capabilities each function needs, also through the functions it calls
  (`"effects"`, and `"capabilities"` for the whole program); the text outline adds `[needs fs]`.
- **A sandboxed interpreter.** `nyra run --interp` / `--sandbox` runs the program in the IR interpreter,
  which now runs every program (maps, the whole standard library, JSON, input, arguments, `os.exit`).
  Limits end a run with their own error and exit code: `--fuel` E0355 (120), `--max-memory` E0356 (121),
  `--max-output` E0357 (122), `--max-depth` E0358 (123), `--max-time` E0359 (124). Steps are counted, so
  the same program stops at the same statement everywhere. `time.sleep_ms` does not wait in the
  interpreter. In `--sandbox` files stay below the working folder. `nyra_run` takes `sandbox: true`,
  `fuel`, `max_memory`, `max_output` and `args`.
- **Property examples.** `ex for n in 0..200: f(n) >= 0` runs its conditions for every `n` while the
  program compiles and reports the first failing input (E0250, E0251, E0253 say `for n = 7`); E0292 and
  E0293 are the errors of a malformed or too large range.
- Examples now also run functions that use the standard library's pure functions (`math`, `text`, `json`).

Language
- `return` is the keyword (`ret` is still accepted, so old programs run); the docs write `return`.
- The conditional value `c ? a : b`: the same as `if c { a } else { b }`, lower than `||`, right-associative.
- `fn main` next to top-level statements is valid (E0101 no longer): the statements run first, in order,
  then `main()` is called; a bare `main()` line among them calls it there.
- A value inside a map changes in place on every backend: `m[k].push(x)`, `m[k].field = v`, `m[k][i] = v`,
  `m[k].sort()` (E0229 now only forbids `inout m[k]`); a missing key is E0248, as for `m[k]`.

Tools
- An error with exactly one certain fix is repaired in memory by `check`, `run`, `build` and `test`, and
  reported as a warning (`warnings` in `--json`); `--fix` writes the file, `--strict` keeps errors as errors.
- `nyra fmt file.nyra`: canonical form (fixes applied, `return`, four-space indentation).
Speed
- `nyra run` compiles the generated C with `-O1` (about a third less compile time on big programs, the
  program runs as fast as at `-O2`); `nyra build` and `nyra run --release` keep `-O2`.
- `s = s + x` and `xs = xs + ys` append in place, like `s += x`, while nothing else holds the string or
  array: a loop that builds a string this way was quadratic (4.4 s for a look-and-say of 40 steps written
  with `out = out + ...`, now 0.1 s). The pass is on the IR, so every backend gets it.
- The overflow check of `int` costs less: its error path takes three arguments instead of five, so the C
  compiler optimizes around it again (a recursive `fib` that took 2.65 times as long as C now takes a
  fraction of it); the index check got the same change. What the checks still cost in loops is at most
  about 15% (`perf/README.md`).
- Performance warnings E0360-E0362 (never errors): a search (`contains`, `index_of`) in a long loop over
  an array that is built with `push`, text put in front of a string in a loop, and appending to a string
  in a long loop on the Go target. They are in the `"warnings"` list of `--json`.
- `perf/` also times Go, Node.js and Python versions of each program; `perf/first_output.py` measures the
  time to the first output of `nyra run`.

## v0.5 (released as part of v0.6.0)

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
- `docs/AGENT_CARD.md`: the language in 1,400 tokens or less (a hard budget, measured with
  `tools/card_tokens.py`): one example program, the rules that differ from other languages, what is not in
  Nyra, every method and module name. `tests/docs.rs` runs its example and checks its names against the compiler.
  `nyra_spec` returns the card by default and the whole spec with `full: true`; the new resource is `nyra://card`.
- The benchmark caches the prompt with the Anthropic provider (the Nyra text is a system block with
  `cache_control`, one warm-up request per model, cache reads and writes priced and recorded), has
  `--spec full|card`, `--ex-examples` and `--budget` for `--provider anthropic`, and the verified prices
  of Opus 5.5, Sonnet 5.5 and Haiku 4.5. The card against the full spec: `research/AB-card.md`.

Project
- The Python, TypeScript, Rust and Go targets are marked experimental (README, SPEC, `nyra --help`, llms.txt);
  native (C) and JavaScript are the supported targets. No code or tests were removed.
- The benchmark's Nyra reference solutions and the `examples/` are written as scripts in current style
  (no `fn main`, `print(a, b)`, lambdas, comprehensions, maps, `for i, x in xs`); `examples/explicit_main.nyra`
  keeps the `fn main` form.
- `docs/DESIGN.md` records what was decided not to do yet, and why.

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
