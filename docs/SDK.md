# SDKs: Nyra in Python and JavaScript agents

Two packages let a program run code that a model wrote in the Nyra sandbox, instead of `eval()` or a
shell: `nyra` for Python (`sdk/python/`) and `@nyralang/nyra` for JavaScript (`sdk/js/`). Both have the same
API: a `Sandbox` with limits, and `run`, `check`, `fix`, `explain` and `run_json` / `runJson`. Every
error, at compile time or at run time, comes back as data: its code, message, line, column, hint and,
when there is one, its certain fix. The packages are not on PyPI or npm yet; use them from the repository.

## What runs where

| | Python `nyra` | JS, native backend | JS, wasm backend |
|---|---|---|---|
| needs | the `nyra` executable | the `nyra` executable | nothing (bundled `nyra.wasm`) |
| runs on | CPython 3.9+ | Node.js 18+, Deno, Bun | Node.js, Deno, Bun, browsers, Cloudflare Workers |
| how | `nyra run --sandbox` in a child process | the same | the compiler's C ABI (`src/wasm.rs`) in-process |
| capabilities | `input`, plus `allow`: fs, os, net | the same | `input` only |
| `cwd`, `args`, `files` | yes | yes | no |

The executable is found in `NYRA_PATH` (the file or its folder), then on `PATH`; Python also looks in the
cache of `nyra.install()`, which downloads the release archive for this computer from GitHub, checks its
SHA-256 against the release's `SHA256SUMS.txt` and keeps only the executable (Windows x86_64, Linux x86_64,
macOS arm64). Nothing is downloaded unless `install()` is called. In JavaScript, `backend: "auto"` (the
default) uses the executable when one is found and WebAssembly otherwise.

## The sandbox

A run is `nyra run main.nyra --sandbox --json --time --allow input[,...] --fuel N --max-memory NM
--max-output N --max-time MS [--max-depth N]` in a fresh temporary folder that is removed afterwards (or
`run_interp` with the same limits in WebAssembly). Standard input comes from the caller, so `input` is
always granted; nothing else is unless `allow` names it. With `allow=["fs"]`, file paths are confined to
`cwd`. `native=True` / `native: true` compiles with the C compiler instead: that is **not** a sandbox (only
the timeout applies) and is never used unless asked for.

| limit | Python | JavaScript | default | error, exit code |
|---|---|---|---|---|
| steps | `fuel` | `fuel` | 100,000,000 | E0355, 120 |
| heap | `memory_mb` | `memoryMb` | 256 MiB | E0356, 121 |
| output | `max_output` | `maxOutput` | 1 MiB | E0357, 122 |
| call depth | `max_depth` | `maxDepth` | the interpreter's | E0358, 123 |
| real time | `timeout` (s) | `timeoutMs` | 10 s | E0359, 124 |

The time limit is enforced by the interpreter; the child process is also killed 5 s after it as a last
resort. In Cloudflare Workers the clock does not advance during a call, so there only `fuel` stops a run.

Without `strict`, an error that has exactly one certain fix is repaired in memory, as `nyra run` does, and
listed in `warnings` with `applied` (and in `feedback()`), so the model learns the right form.

## API

Python (`from nyra import Sandbox`) and JavaScript (`import { Sandbox } from "@nyralang/nyra"`, every
method returns a promise):

| | |
|---|---|
| `run(code, stdin="", args=, files=)` | a `Result`: `ok`, `stdout`, `stderr`, `exit_code` / `exitCode`, `errors`, `warnings`, `steps`, `ms`, `stage` ("compile", "runtime" or none), `limit` (which limit stopped it), `backend`; `feedback()` is the text for a model |
| `check(code, strict=True)` | `ok`, `errors`, `warnings`; runs the `ex` examples too. The wasm backend is always strict |
| `fix(code)` | the code with its certain fixes applied. Native: only when they make it compile (`nyra check --fix`); wasm: every certain fix, also when other errors remain |
| `explain(code)` | the entry of `nyra explain CODE --json` plus `text`; `{"ok": false, ...}` for an unknown code |
| `run_json(code)` / `runJson(code)` | runs it and parses the printed output (all of it, else its last line) as JSON; raises `NyraError` (with `.result`) when it fails |
| `version()` | the compiler's version |

Module-level shortcuts: `nyra.run`, `nyra.check`, `nyra.fix`, `nyra.explain`, `nyra.run_json` (and the
same in JS).

## Tools for agents

The tool is called `run_nyra` and takes `{"code": ..., "stdin": ...}`; its result is `feedback()`.

- Raw APIs: `anthropic_tool_spec()` / `anthropicToolSpec()` (`name`, `description`, `input_schema`),
  `openai_tool_spec()` / `openaiToolSpec()`, and `run_tool(input, sandbox)` / `runTool(input, sandbox)` for
  a call.
- Python frameworks, imported only when the helper is called: `langchain_tool()` (a `StructuredTool`),
  `llamaindex_tool()` (`FunctionTool`), `crewai_tool()` (`BaseTool`), `autogen_function()` (autogen-core
  `FunctionTool`). Without the framework each returns a `NyraTool`: a callable with `name`, `description`
  and `parameters` (JSON schema). The adapters are tested against stand-ins of those classes, not the real
  frameworks.

Examples in `sdk/examples/`: `python/replace_eval.py` and `js/replace_eval.mjs` (eval() replaced),
`python/anthropic_tool_loop.py` (a manual tool-use loop with Claude) and `python/langchain_tool.py`.

## Overhead per call

Measured on Windows 11 (the owner's PC), median of 50 calls of `print("hi")`, and of 10 calls of a loop
of 1,000,000 iterations, release build of the v0.9 branch (2026-10-20):

| | `print("hi")` | 1M-iteration loop |
|---|---|---|
| Python, native | 26 ms | 374 ms |
| Node.js, native | 63 ms | 408 ms |
| Node.js, wasm (warm instance) | 0.2 ms | 528 ms |

A native call is a process start (`nyra --version` alone takes 19 ms from Python; Node.js spawns a process
on Windows in about 55 ms) plus a temporary folder. WebAssembly costs about 50 ms once per process to
compile `nyra.wasm`, then nothing per call; its interpreter is about 1.3x slower than the native one. For
many short runs WebAssembly is the fast choice; for long ones, either.

## Tests

```
CARGO_INCREMENTAL=0 cargo build --release           # target/release/nyra, used by both test suites
python -m unittest discover -s sdk/python/tests      # or python -m pytest sdk/python/tests
python tools/build_wasm.py --no-copy && node sdk/js/scripts/build-wasm.mjs
cd sdk/js && node --test                             # native and wasm backends
```

`sdk/js/wasm/nyra.wasm` is not committed (like `site/play/nyra.wasm` lives only on the `site` branch); the
npm package's `prepack` refuses to pack without it.
