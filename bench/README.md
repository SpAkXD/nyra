# Nyra benchmark

How often does a model write a correct program in Nyra on the first try, and how many tokens does it spend,
compared with Python, TypeScript and Rust on the same tasks? And how does that differ between models?

Every task is given to each model once per language, with the same task prompt and the same repair budget.
The program the model writes is compiled/run and its standard output is compared with the expected output.
Many models can be compared in one run through [OpenRouter](https://openrouter.ai) (Claude, Gemini, GPT, Grok,
DeepSeek, ...); the raw transcripts stay on your machine and only a summary is published.

## What is measured

| metric | meaning |
|---|---|
| **pass@1** | share of runs whose **first** program printed exactly the expected output |
| **pass within N repairs** | the same, when a failed attempt is followed by feedback (the compiler's JSON errors, the interpreter's or compiler's messages, or the model's own wrong output) and up to N retries (default 3) |
| **code tokens** | tokens of the extracted program alone, counted with the model's own tokenizer. It does not depend on thinking or on prose around the code, so it is the cleanest "how compact is the language" number. **This is the headline token metric** |
| **billed output tokens** | what the API charged for the whole reply, per attempt, for the first attempt, and per run (all attempts). It **includes any thinking** the model did, and is what the headline shows next to the code tokens. When the API says how much of it was thinking, that is shown too |
| **input tokens** | the whole prompt per attempt. For Nyra it contains the language spec, which the other languages do not need |
| **size** | characters and non-blank lines of the extracted program (first attempt) |
| **cost** | what OpenRouter charged (`usage.cost`), per run and in total |

Besides the headline table every model gets: Nyra against each other language on the runs both solved first
try (so neither language is measured on easy tasks while the other is measured on hard ones, with a paired sign
test of the first-try differences task by task), a breakdown per task category, how first attempts failed (no
code block, compile error, runtime error, timeout, wrong output), the Nyra compiler error codes models hit most,
and a per-task table. Pass rates come with 95% Wilson intervals. A run with several models also gets a
model x language table for every headline metric.

## Quick start (no API key, no money)

```
cargo build --release                  # on Windows without MSVC: cargo +stable-x86_64-pc-windows-gnu build --release
python bench/verify.py                 # check every reference solution: Nyra (two backends), Python, TypeScript, Rust
python bench/test_bench.py             # tests of the harness itself (no network, no API)
python bench/run.py --provider mock    # run the whole pipeline with the reference solutions
python bench/run.py --provider mock --models mock,mock-flaky,mock-wrong    # three "models": a comparison table
```

You need Python 3.9 or newer (standard library only), a C compiler for the native Nyra backend, Node.js 22.6 or
newer for TypeScript (it runs `.ts` files itself, see below), and a Rust toolchain (`rustc`; install from
https://rustup.rs). `--langs` leaves languages out: `--langs nyra,python` needs neither Node.js nor Rust.

## A real run through OpenRouter

One API key reaches hundreds of models. The key is read from the environment variable `OPENROUTER_API_KEY` and
nowhere else (not from an argument, not from a file), is sent only to openrouter.ai over HTTPS, and is never
written to a result file or printed.

```
# 1. Pick the models. The list is public (no key): search it, or use the default list in bench/models.json.
python bench/models.py claude opus          # ids, prices, whether the model thinks
python bench/models.py --check              # do all default ids in bench/models.json still exist?

# 2. Set the key (PowerShell: $env:OPENROUTER_API_KEY = "sk-or-v1-...").
export OPENROUTER_API_KEY=sk-or-v1-...

# 3. See the plan and a rough cost, without spending anything.
python bench/run.py --provider openrouter --models default --samples 5 --dry-run

# 4. A smoke test for a few cents: one model, two tasks, one language. Look at the result: replies were
#    found, usage and cost were reported, and "Code tokens" has a number (see "Tokens and cost" below).
python bench/run.py --provider openrouter --models anthropic/claude-sonnet-5.5 --langs python --tasks fizzbuzz,gcd_pairs

# 5. The real run. --budget stops everything once that many dollars have been spent; --jobs 8 because the API
#    is the wait (6 models x 5 samples is about 8,000 requests: plan for hours, or start with --samples 1).
#    A few models by hand work the same way.
python bench/run.py --provider openrouter --models default --samples 5 --jobs 8 --budget 100
python bench/run.py --provider openrouter --models anthropic/claude-opus-5.5,openai/gpt-6-sol,google/gemini-3.8-flash

# 6. Publish the summary (see "Publishing results" below).
python bench/publish.py bench/results/<date>-openrouter-compare.json --name 2026-10-v0.4
```

**Finding model ids.** `python bench/models.py WORDS` searches OpenRouter's public list
(`https://openrouter.ai/api/v1/models`) and prints id, price and whether the model thinks; `--json` prints the raw
entries. Use concrete ids such as `anthropic/claude-opus-5.5`, not the moving `~vendor/...-latest` aliases, so that
a result file says what was measured. `--models default` expands to the list in `bench/models.json`, which you can
edit freely; it only holds ids that were checked against the live list (the `verified` date in the file says when),
and `python bench/models.py --check` re-checks them. Before anything is spent, `run.py` checks every id against the
public list and refuses a typo (with suggestions); `--no-model-check` skips that.

**What is sent.** A chat completion per attempt: the language's system prompt, the task, and for repairs the
conversation so far. Nothing else is set unless you ask: no temperature (many current models reject or ignore
it), no thinking settings, so each model does what its own defaults do. `--effort low|medium|high|...` becomes
`reasoning.effort`; `--extra-json` adds any other top-level request field, for example
`--extra-json '{"reasoning": {"max_tokens": 2000}}'` or, to pin one upstream provider and stop OpenRouter from
silently falling back to another,
`--extra-json '{"provider": {"order": ["anthropic"], "allow_fallbacks": false}}'`. `--max-tokens` (default 16000)
bounds thinking and answer together. The request is recorded in the result file, with the model id OpenRouter
actually served and the upstream provider that ran it (read from the response's `openrouter_metadata`, which the
harness asks for with the `X-OpenRouter-Metadata` header).

**Failures.** Rate limits (429), timeouts and server trouble (5xx, also when OpenRouter reports them inside a 200
response) are retried with exponential backoff that honours `Retry-After`. A request that can never work (400, 404,
422) stops that model and the run goes on with the next one; a bad key or no credits (401, 402) stops everything.
Results that finished are always saved and marked `"complete": false` when the run was cut short.

**Tokens and cost.** OpenRouter reports `usage` on every reply (prompt and completion tokens counted with the
model's own tokenizer, thinking tokens inside the completion tokens, and the dollars charged). OpenRouter has no
token-counting endpoint, so **code tokens** are measured with one tiny extra request per first attempt: the code
alone as a user message with `max_tokens` 16, minus the same measurement of the one-token text `x` (the fixed
per-request overhead). That costs a fraction of a cent per run; `--no-count-tokens` turns it off (the headline then
has billed tokens only). If a model rejects the request, counting is switched off for that model with a warning.
`--dry-run` prints a rough cost estimate (the real prompts, `--assume-output-tokens` per attempt); thinking models
can use several times more, which is what `--budget` is for.

### Without OpenRouter

`--provider anthropic` talks to Claude directly with the official SDK (`pip install -r bench/requirements.txt`,
key in `ANTHROPIC_API_KEY`, `--model claude-opus-5-5`). It counts code tokens with Anthropic's free token-count
endpoint. Everything else is the same.

A run prints one line per finished task, then the Markdown summary (for several models: one line per model, then
the comparison), and writes, for every model:

```
bench/results/<date>-<provider>-<model>.json   everything: metadata, prompts, every reply, every verdict
bench/results/<date>-<provider>-<model>.md     the summary tables of that model
bench/results/<date>-<provider>-compare.md     model x language tables (several models only)
bench/results/<date>-<provider>-compare.json   index of the result files of that run
bench/results/latest.md                        copy of the most recent summary (the comparison, for several models)
```

Existing result files are never overwritten (a `-2`, `-3`... suffix is added). Everything in `bench/results/` is
git-ignored: it holds every prompt and reply.

### Options

`python bench/run.py --help` lists all of them:

| option | |
|---|---|
| `--provider mock\|anthropic\|openrouter` | who answers (default `mock`, which replays the reference solutions) |
| `--models a,b,c` | models to run one after the other; `default` is the list in `bench/models.json`; with `mock`: `mock,mock-flaky,mock-wrong` |
| `--model ID` | one model (the default for `mock` and `anthropic`; `openrouter` has no default: it never guesses what to pay for) |
| `--langs nyra,python,typescript,rust` | languages to run (all four by default; the first one is the baseline of the paired comparisons; `ts` and `rs` work as names) |
| `--tasks fizzbuzz,gcd*` | task ids or patterns |
| `--max-version 0.1` | skip tasks that need a newer Nyra, **for every language**. Default: the version of the `nyra` binary, when Nyra is run |
| `--repairs 3` | repair attempts after a failed first try |
| `--samples 1` | independent runs per task, language and model: more samples give much tighter intervals |
| `--nyra PATH` | compiler binary (default `target/release`, else `target/debug`) |
| `--backend native\|js` | which Nyra backend runs the programs (default native, via the C compiler) |
| `--spec PATH` | spec shown to the model (default `docs/SPEC.md`, read at run time) |
| `--node PATH` | the `node` that runs TypeScript (default: `node` from `PATH`) |
| `--rustc COMMAND` | the Rust compiler command, e.g. `'rustc +stable-x86_64-pc-windows-gnu'` (default: found automatically) |
| `--timeout 10`, `--jobs 4` | seconds per program; parallel evaluations (each waits for one API call at a time) |
| `--out DIR` | where the result files go (default `bench/results`) |
| `--max-tokens 16000` | per reply, thinking included (anthropic, openrouter) |
| `--effort LEVEL`, `--extra-json '{...}'` | extra request settings; recorded in the result file (anthropic, openrouter) |
| `--base-url URL` | another OpenAI-compatible endpoint (https only; for tests and proxies) |
| `--no-model-check` | do not check the model ids against OpenRouter's public list first |
| `--budget USD` | stop the whole run once the calls so far cost this much (openrouter) |
| `--assume-output-tokens 1500` | output tokens per attempt in the `--dry-run` cost estimate |
| `--no-count-tokens` | do not measure code tokens |
| `--mock-flaky` | mock only: break some first attempts to exercise the repair loop |
| `--dry-run` | print the plan, the maximum number of calls and an estimated cost, then stop |
| `-q`, `--quiet` | no per-task progress lines |

If a run is interrupted or a provider error is fatal (bad key, unknown model, rejected parameter), the results
that finished are still saved and marked `"complete": false`; a model where nothing finished writes no files.

## Languages and how their programs are run

All four languages get the same task prompt and the same kind of system prompt (one sentence on the language and
how its programs are run, the same task paragraph, the same reply rule); only Nyra's contains a spec, because the
model cannot know the language. `system_prompts` in the result file hold the exact texts.

| language | how a program is checked |
|---|---|
| Nyra | `nyra check main.nyra --json`; if that passes, `nyra build` and the executable (or Node for `--backend js`). This is what `nyra run` does internally, split in two on purpose: killing a `nyra run` that timed out would leave the program it started running, and the split times compile and run separately |
| Python | `python -I -X utf8 main.py` (isolated mode: standard library only) |
| TypeScript | `node --experimental-transform-types main.ts`: Node 22.6+ strips the type annotations itself (22.18+ and 23.6+ need no flag; the flag also allows enums, namespaces and constructor parameter properties). **Nothing type-checks the program**, so a type error that `tsc` would report does not fail it. A `SyntaxError` before the program starts is a compile error, anything thrown while it runs is a runtime error |
| Rust | `rustc -O --edition 2021 -A warnings --color never main.rs`, then the executable. Optimized like a release build (integer overflow wraps instead of panicking), edition 2021 (plain `rustc` would use 2015), warnings silenced so the feedback after a failed build shows errors only. A panic is a runtime error (exit 101), a failed link or an internal compiler error is a `toolchain_error` and not the model's fault |

`rustc` is found on `PATH` or in `~/.cargo/bin`. On Windows the default Rust toolchain is MSVC, which cannot link
without the Visual Studio build tools, so installed `windows-gnu` toolchains are tried first and the first
candidate that compiles and runs a hello-world program is used (`--rustc` overrides; the result file records the
command and the version). On this project's Windows development machine that is `rustc +stable-x86_64-pc-windows-gnu`.

For every language: 10 s wall-clock limit per program, output capped at 1 MB (runaway loops are killed), no
stdin, a private temp directory per evaluation, secrets (`*KEY*`, `*TOKEN*`, ...) removed from the environment.
Scratch paths are removed from the messages the model sees (`main.py`, `main.ts`, `main.rs`, `main.nyra`).

## How a run works

1. **Tasks.** `bench/tasks/*.json`. Tasks whose `min_version` is above the compiler's version are dropped
   **for every language**, so the languages are always compared on the same tasks (without Nyra in `--langs`
   there is no such limit and all tasks run).
2. **Prompt.** The system prompt is the language's text (above) ending with the same rule: reply with exactly one
   fenced code block and nothing else. The user message is the task's prompt, verbatim.
3. **Extract** the first fenced code block of the reply. No block, or an empty one, is a failed attempt (`no_code`).
4. **Check and run** as in the table above.
5. **Verdict.** Exit code 0 **and** stdout equal to `expected_output` after normalizing CRLF, stripping trailing
   whitespace from every line and dropping trailing blank lines. Leading whitespace matters.
6. **Repair.** If it failed and repairs remain, the model gets its own reply back plus one feedback message and
   answers again; the whole conversation is replayed each time (only the visible reply text, never thinking blocks).
   The feedback (all of it is built in `run.py`, in one place):
   - compile error: Nyra, the compiler's JSON verbatim; Python, the interpreter's `SyntaxError` output; TypeScript,
     Node's `SyntaxError` output; Rust, `rustc`'s error output;
   - runtime error: exit code, the end of stderr (without Node's internal stack frames), and what was printed before
     the crash;
   - timeout / output limit: that, plus the start of the output;
   - wrong output: the program's actual output, the line number of the first difference, and how many lines were
     expected. **It never shows the expected output**, because for a one-line answer that would let the model copy it;
   - no code block: a reminder of the reply format.

## Why the comparison is fair, and where it is not

Symmetric by construction:

- same tasks, written in neutral language (no prompt mentions a language; no task depends on one language's output
  format such as Python's `True` or `3.0`), same task prompt, same reply-format rule;
- same repair budget, same feedback templates, same time/output limits, same normalization;
- the model's settings are identical for every language and recorded in the result file;
- the expected output is produced by the Python reference and **confirmed by independent implementations**: a
  TypeScript and a Rust reference for every task, and a Nyra reference for every task the compiler supports (on two
  backends, native and JavaScript), so a wrong expectation cannot hide. `python bench/verify.py` checks all of them;
- token counts are the provider's own, not estimates; replies, programs and verdicts are all saved, and the file
  records the compiler version, the spec hash, the task-set hash, the toolchain versions and the model id the API
  actually served;
- the metrics are computed on the runs where **every** language produced a valid result, so an API error never
  makes one language look better.

Known asymmetries (they are part of the question, but you should know them):

- Nyra is given its spec (about 1,300 tokens of extra input on every call); the other languages are not, and have
  years of pre-training behind them. That is the situation of any new language; the headline answers "how well does a
  model do with this spec", not "how good is Nyra in the abstract".
- Python and TypeScript have large standard libraries and Nyra has almost none yet. Where a library call would
  trivialize a task (`gcd`, `pow(a, b, m)`, `sorted`), the prompt asks for the algorithm to be written out. Some tasks
  are still shorter in Python for that reason; Nyra's own standard library (v0.6) will change this.
- **TypeScript is not type-checked** (Node only removes the annotations), so it is closer to JavaScript here than
  `tsc` would make it. Rust is the strictest: the compiler rejects what the others would run.
- The error feedback differs because the toolchains differ: Nyra's compiler returns structured diagnostics with fix
  hints, and that is a feature under test. Python's traceback, Node's message and `rustc`'s report are what those
  toolchains offer.
- Through OpenRouter a model may be served by different upstream providers (quantization, limits and speed differ)
  unless you pin one with `--extra-json`. The result file lists the model id and the providers that actually served it.
- Many models think before they answer, and some cannot turn it off. Billed output tokens include that; code tokens do
  not. Quote code tokens for "how compact is the language" and billed tokens for "what did it cost", and say which.
- Tasks are small, input-free programs chosen to be expressible in Nyra at each version. They say nothing about
  large programs, libraries or I/O. They were also written by the people who build Nyra, who know what it can
  express; a task set from a third party would be a stricter test.
- A compiler or backend bug that makes the C step fail on a valid program (`toolchain_error`) counts as a failure,
  because the pipeline did not produce a working program. It is reported separately so it can be fixed.

## Reading the results honestly

- A run has one sample per task by default and a model's output varies from run to run. `--samples 5` (or more)
  averages that noise out of each task's result, so use it before putting a number in a README. The 95% intervals
  deliberately use the number of **tasks** as their sample size, because repeating a task does not add a new task.
  With 29 tasks the interval is roughly 10 to 20 points either way. Quote the count (`25/29`), not only the
  percentage. The paired sign test compares the languages task by task for the same reason.
- Do not compare result files whose `tasks_sha256`, spec hash, compiler version or model differ (`publish.py` refuses
  to).
- Sampling parameters such as temperature are not sent unless you pass them with `--extra-json` (the newest models
  reject them, and some ignore them). Run-to-run variance is handled with `--samples`.
- The harness runs model-written code on your machine without a sandbox (isolated interpreter, time and output limits,
  no secrets in the environment, but the code can still touch your files). Run real benchmarks in a container or VM.

## Publishing results

Raw result files hold every prompt, reply and program, so they stay local (`bench/results/` is git-ignored). What
is meant to be committed is a summary:

```
python bench/publish.py bench/results/<date>-openrouter-compare.json --name 2026-10-v0.4
python bench/publish.py bench/results/*-openrouter-*.json --name 2026-10-v0.4 --stdout   # preview only
```

This writes `bench/published/<name>.md` and `<name>.json`: the headline per model x language (first-try success,
success within the repairs, **code tokens with the billed output tokens in brackets**, cost), Nyra's token use relative
to the other languages, a per-category breakdown for every model, and a few notable failures (runs that never
passed, compiler bugs, the Nyra compiler errors models hit most, the tasks hardest for Nyra on the first try). No
prompt, reply or program is copied; each source file is named with its SHA-256 so the summary can be tied to the
raw files. `publish.py` refuses mock runs, incomplete runs, and result files that were not measured the same way
(different tasks, spec, compiler, samples or repairs), and does not replace an existing summary
(`--allow-mock`, `--allow-incomplete`, `--allow-mismatch` and `--overwrite` override that; the summary then says so).

## Tasks

`bench/tasks/<id>.json`:

```json
{ "id": "gcd_pairs", "title": "Greatest common divisors", "category": "number-theory", "difficulty": "easy",
  "min_version": "0.1", "prompt": "Write Euclid's algorithm yourself ...", "expected_output": "6\n21\n1\n250000\n6\n" }
```

`min_version` is the first Nyra version in which a natural solution can be written (0.1: functions, ints, loops,
one value per `print`; 0.2: string interpolation and compact syntax; 0.3: arrays, structs, string functions).
Categories: math, number-theory, recursion, simulation, patterns, strings, arrays, structs. There are 39 tasks:
19 for 0.1, 10 for 0.2, 10 for 0.3. The 0.3 tasks have no Nyra reference solution yet because that language version
does not exist; they start running as soon as the compiler's version reaches them.

### Adding a task

1. Write `bench/tasks/<id>.json` (the file name is the id) with `"expected_output": ""`.
2. Write `bench/solutions/python/<id>.py`.
3. `python bench/verify.py --write --tasks <id>` runs it twice (must be deterministic) and fills in `expected_output`.
   Read the result: it is what the model must print.
4. Write `bench/solutions/typescript/<id>.ts` and `bench/solutions/rust/<id>.rs` (every task needs both, whatever its
   `min_version`); `verify.py` checks that they print the same output.
5. If the installed compiler supports `min_version`, write `bench/solutions/nyra/<id>.nyra`; `verify.py` then checks that
   it prints the same output on both backends. If a task cannot be solved cleanly in that version, raise its `min_version`.
6. `python bench/test_bench.py`.

Rules for a good task: all data is in the prompt and the prompt says exactly what to print and in what format; no
input; deterministic; runs well under a second in CPython; no language names in the prompt; no floats (Nyra prints
`3.0` as `3`); integers stay below 2^53 (the JS backend and TypeScript); no `%` or `/` on negative numbers (Python
floors, C, JS and Rust truncate); recursion depth below about 900 (Python's limit); booleans are printed as explicit
lowercase words. **Do not edit a task after seeing results**; add a new one (and the task-set hash will show that the
set changed).

### When a new Nyra version lands

Bump the version in `Cargo.toml` (the runner uses it to choose the tasks), write the `.nyra` reference solutions for the
tasks that version unlocks, and run `python bench/verify.py --strict` (a missing reference for a supported version is
then an error). Results from different versions are different experiments.

## Files

```
bench/
  run.py            the runner: CLI, prompts, languages, attempt loop, program checks, result files
  providers.py      model providers: mock, anthropic, openrouter (urllib only) and the interface for adding more
  models.py         find and check OpenRouter model ids (public list, no key)
  models.json       the default models for --models default (ids verified against the live list)
  report.py         metrics, intervals, paired comparison, Markdown tables, model x language comparison
  publish.py        raw results -> the summary that is committed (bench/published/)
  verify.py         checks the reference solutions; --write generates expected outputs
  test_bench.py     tests of the harness itself (no network, no API)
  tasks/            one JSON file per task
  solutions/python/ solutions/typescript/ solutions/rust/   a reference solution for every task
  solutions/nyra/   a reference solution for every task the compiler supports
  results/          raw result files: every prompt and reply (git-ignored)
  published/        summaries of real runs (committed)
```

CI (`.github/workflows/ci.yml`) builds the compiler on Ubuntu, then runs `python bench/test_bench.py` and
`python bench/verify.py` next to `cargo test`.

## Adding a provider or a language

**Provider** (Gemini directly, a local server, ...): subclass `providers.Provider`, implement
`complete(system, messages, meta) -> Reply` (and `count_tokens(text)` if the API can count tokens; set
`count_all_attempts = False` if counting costs money), import any SDK inside `ensure_ready()` (called once before any
work, never for `--dry-run`), and add the class to `PROVIDERS`. `messages` is a list of `{"role", "content"}` dicts;
`meta` is only for the mock provider and logging. Raise `ProviderError(..., fatal=True)` for errors that repeat on every
call of that model (unknown model, parameters) and `stop_all=True` for key or account problems, so the run stops
instead of recording hundreds of fake failures. For an HTTP API, `providers.urllib_transport` and `OpenRouterProvider`
show the retry and error handling; tests inject a fake transport.

**Language**: subclass `run.Language` (`name`, `ext`, `system_prompt`, `evaluate(code, task)`), add reference
solutions under `solutions/<name>/`, register it in `run.make_languages` and `LANG_ORDER` (and in `report.DISPLAY`).
`evaluate` must return an `EvalResult` and should reuse `run_limited` and `judge_run` so the verdict rules stay
identical. `RustLang` and `TypeScriptLang` are the examples.

## Limitations (current)

- No real results yet: the harness is tested end to end with the mock provider, with a fake OpenRouter server on
  this machine, with the real `anthropic` SDK against a local stub server, and with unit tests, but no real model
  has been benchmarked. In particular the OpenRouter request and response shapes follow its documentation and were
  never exercised against the live service, and token counting through OpenRouter (the echo request) is untested
  against real models: check the first real run's `code_tokens` before trusting them.
- Only 29 of the 39 tasks can run today (Nyra 0.2); the rest need Nyra 0.3 (they do run without Nyra in `--langs`).
- Single-turn tasks, small programs; nothing here measures reading or fixing existing code.
- Native Nyra runs need a C compiler (gcc/clang) and `--backend js` needs Node; tasks run in parallel, so timing
  numbers are indicative only.
- TypeScript is not type-checked; there is no `tsc` step.
