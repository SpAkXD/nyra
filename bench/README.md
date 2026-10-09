# Nyra benchmark

How often does a model write a correct program in Nyra on the first try, how many tokens does it spend, and how
fast does the program run, compared with Python, TypeScript and Rust on the same tasks? And how does that differ
between models?

Every task is given to each model once per language, with the same task prompt and the same repair budget.
The program the model writes is compiled/run and its standard output is compared with the expected output.
Many models can be compared in one run through [OpenRouter](https://openrouter.ai) (Claude, Gemini, GPT, Grok,
DeepSeek, ...); the raw transcripts stay on your machine and only a summary is published.

## What is measured

| metric | meaning |
|---|---|
| **pass@1** | share of runs whose **first** program printed exactly the expected output |
| **pass@1 with self-repair** (Nyra) | pass@1 when a first attempt that does not compile is passed once through `nyra check --fix`, which repairs the mistakes whose fix is unambiguous, without a model call and at zero tokens. Measured on the side (it changes no other number) and shown next to plain pass@1; the other languages have no such tool and show `-`. `--no-self-repair` turns it off |
| **pass within N repairs** | the same, when a failed attempt is followed by feedback (the compiler's JSON errors, the interpreter's or compiler's messages, or the model's own wrong output) and up to N retries (default 3) |
| **code tokens** | tokens of the extracted program alone, counted with the model's own tokenizer. It does not depend on thinking or on prose around the code, so it is the cleanest "how compact is the language" number. **This is the headline token metric** |
| **billed output tokens** | what the API charged for the whole reply, per attempt, for the first attempt, and per run (all attempts). It **includes any thinking** the model did, and is what the headline shows next to the code tokens. When the API says how much of it was thinking, that is shown too |
| **no program: token limit** | replies without a code block that stopped at the output-token limit (`--max-tokens`), usually because the model spent it all thinking: first attempts / all attempts, per language. A visible reasoning cost of an unfamiliar language |
| **runtime** | how long the program that passed runs: the median of `--time-runs` extra runs (default 3) minus the language's start-up time, compile time excluded (see "Runtime" below). Summarized over the `speed` tasks |
| **efficiency** | (median code tokens / Python's) x (median runtime on the speed tasks / Python's): one number per language, Python = 1.00, lower is better (see "Runtime" below) |
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
python bench/speed.py                  # time the reference solutions of the speed tasks in every language (no model)
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
#    is the wait (6 models x 5 samples is about 12,000 requests: plan for hours, or start with --samples 1).
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
public list and refuses a typo (with suggestions); a variant such as `vendor/model:online` passes when `vendor/model`
is listed, and `--no-model-check` skips the check.

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
Results that finished are always saved and marked `"complete": false` when the run was cut short. A request that
times out is retried, and a provider may bill it again: for a thinking model that keeps timing out, lower
`--max-tokens` or `--effort`.

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
| `--langs nyra,python,typescript,rust` | languages to run (all four by default for `--tier v1`, `nyra,python,typescript` for `v2`, the three arms for `edit`; the first one is the baseline of the paired comparisons; `ts` and `rs` work as names) |
| `--tier v1\|v2\|edit\|safety` | which task set: the original input-free tasks (default), tasks that read stdin and are judged on hidden inputs, edits of an existing program, or the safety tasks (pending). See "Tiers" below |
| `--hidden-dir DIR` | v2 and edit: a private folder of `<task id>.json` files with extra hidden `cases` that are not in the repository |
| `--preset NAME` | a bundle from `bench/models.json` (`cheap`: Claude Haiku 4.5 and Sonnet 5.5 and two cheap OpenRouter models, 5 samples, the v2 tier): fills in whatever `--models`, `--samples`, `--repairs` and `--tier` leave out |
| `--python-typecheck [mypy\|pyright]` | type-check the Python programs before running them and tell the model so; skipped with a message when neither tool is installed |
| `--ts-typecheck [tsc]` | the same for TypeScript with `tsc`; without it the TypeScript arm is not type-checked |
| `--include-pending` | safety tier: also run the tasks that are still pending (the compiler has no `--allow` flag yet) |
| `--tasks fizzbuzz,gcd*` | task ids or patterns |
| `--max-version 0.1` | skip tasks that need a newer Nyra, **for every language**. Default: the version of the `nyra` binary, when Nyra is run |
| `--repairs 3` | repair attempts after a failed first try |
| `--samples 1` | independent runs per task, language and model: more samples average out run-to-run noise (the intervals use the number of tasks, so they do not shrink with samples; more tasks do that) |
| `--nyra PATH` | compiler binary (default `target/release`, else `target/debug`) |
| `--backend native\|js` | which Nyra backend runs the programs (default native, via the C compiler) |
| `--spec PATH` | spec shown to the model (default `docs/SPEC.md`, read at run time) |
| `--node PATH` | the `node` that runs TypeScript (default: `node` from `PATH`) |
| `--rustc COMMAND` | the Rust compiler command, e.g. `'rustc +stable-x86_64-pc-windows-gnu'` (default: found automatically) |
| `--timeout 10`, `--jobs 4` | seconds per program; parallel evaluations (each waits for one API call at a time) |
| `--time-runs 3` | extra runs of every passing program whose median is its runtime (0: no timing) |
| `--no-self-repair` | do not try `nyra check --fix` on Nyra first attempts that do not compile |
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
| TypeScript | `node --experimental-transform-types main.ts`: Node 22.6+ strips the type annotations itself (22.18+ and 23.6+ need no flag; the flag also allows enums, namespaces and constructor parameter properties). **Nothing type-checks the program unless you ask**: by default a type error that `tsc` would report does not fail it (`--ts-typecheck` runs `tsc --noEmit --strict` first when `tsc` is installed, see "Type checks" below). A `SyntaxError` before the program starts is a compile error, anything thrown while it runs is a runtime error |
| Rust | `rustc -O --edition 2021 -A warnings --color never main.rs`, then the executable. Optimized like a release build (integer overflow wraps instead of panicking), edition 2021 (plain `rustc` would use 2015), warnings silenced so the feedback after a failed build shows errors only. A panic is a runtime error (exit 101), a failed link or an internal compiler error is a `toolchain_error` and not the model's fault |

`rustc` is found on `PATH` or in `~/.cargo/bin`. On Windows the default Rust toolchain is MSVC, which cannot link
without the Visual Studio build tools, so installed `windows-gnu` toolchains are tried first and the first
candidate that compiles and runs a hello-world program is used (`--rustc` overrides; the result file records the
command and the version). On this project's Windows development machine that is `rustc +stable-x86_64-pc-windows-gnu`.

For every language: 10 s wall-clock limit per program, output capped at 1 MB (runaway loops are killed), no
stdin, a private temp directory per evaluation, secrets (`*KEY*`, `*TOKEN*`, ...) removed from the environment.
Scratch paths are removed from the messages the model sees (`main.py`, `main.ts`, `main.rs`, `main.nyra`).

## Runtime

Every program that passed is run `--time-runs` more times (default 3), one after the other, in the directory it was
built in. Its **runtime** is the median wall clock of those runs minus the language's **start-up time**, which is the
median run time of the language's hello-world program measured the same way before the run starts (Python and Node.js
start an interpreter, the native programs start a process). What is timed:

| language | timed command | compile time (reported separately as `compile_ms`, never in the runtime) |
|---|---|---|
| Nyra | the native executable that `nyra build` produced (C compiled with `-O2`); `--backend js`: `node main.js` | `nyra check` + `nyra build`, C compiler included |
| Python | `python -I -X utf8 main.py` | none |
| TypeScript | `node --experimental-transform-types main.ts` (type stripping happens while loading, so it is runtime) | none |
| Rust | the executable from `rustc -O --edition 2021` | `rustc` |

The run that judged the program is not one of the timed runs: it is the warm-up (on Windows the first start of a fresh
executable can take a second while it is scanned). A timed run that fails or prints something else does not count, and
that program gets no runtime (`timing.error` in the result file says why). Timed runs never overlap each other, but
with `--jobs` above 1 other jobs may be compiling meanwhile, so these numbers are indicative; `bench/speed.py` (below)
runs nothing else at the same time. Every attempt result stores `runtime_ms` and `timing` (`runs_ms`, `median_ms`,
`startup_ms`); the run metadata stores `timing` (runs, start-up per language).

Most tasks finish in well under a millisecond, where only start-up noise would be measured, so runtimes are
summarized over the **`speed`** category: six tasks with a heavier computation (0.5 to 2 s in CPython) and a
deterministic answer. Their prompts say that the running time is measured, the same sentence in every language.

The **medians** table of a report (per model) takes the runs that every language got right on the first try: the
median code tokens and billed output tokens over all of them, and the median runtime over the ones that are speed
tasks. The **efficiency** is

```
efficiency(L) = (median code tokens of L / median code tokens of Python)
              x (median runtime of L on speed tasks / median runtime of Python on speed tasks)
```

Python is 1.00 (without Python in `--langs`, the first language is) and lower is better: 0.5 means half the tokens at the
same speed, or the same tokens at twice the speed. Both factors are shown next to it, so it is clear which one drives
it. A median runtime below 1 ms counts as 1 ms, because start-up varies by more than that between runs. It is a ratio
of medians, deliberately simple: it says nothing about tasks that are not in the set, and the token factor comes from
all tasks while the runtime factor comes from the speed tasks.

### Timing the reference solutions (no model, no key)

```
python bench/speed.py                              # the speed tasks, all four languages, 5 timed runs each
python bench/speed.py --runs 10 --tasks big_sieve,sort_numbers
python bench/speed.py --all                        # every task
python bench/speed.py --langs nyra,python --backend js
```

It builds each reference solution once, checks it, times it with the same code as a benchmark run (`--runs` timed
runs, median, start-up subtracted) and prints a task x language table with the medians and each language relative to
Python, plus the start-up and compile times. Nothing runs in parallel. The result is also written to
`bench/results/<date>-speed-references.md` and `.json` (`--out DIR`, or `--no-files`). The references use the same
algorithm in every language, written plainly (no hand optimization), so this is the runtime half of the comparison
for free; what a model writes may be faster or slower, which is what the benchmark's runtime numbers measure.

## Self-repair (Nyra)

Nyra's compiler can fix some mistakes itself: `nyra check --fix` applies the fixes whose error hints are unambiguous
(for example `return` for `ret`) and writes the file back if it then compiles. When a Nyra first attempt fails with a
compile error, the harness runs that once on the model's program, at no token cost; if the program changes and
compiles, it is built, run and judged like any other (`self_repair` in the first attempt's record: `tried`, `fixed`,
`changed`, `remaining` error codes, `result`, the repaired `code`). **pass@1 with self-repair** counts a run when its
first attempt passed, or its self-repaired version did. It is reported next to plain pass@1 and labelled as such; the
model's own repair loop is unaffected (it still sees the compiler's errors and repairs the original program), so every
other number means what it meant before. The other languages have no comparable tool and show `-`.

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

- Nyra is given its spec on every call: the system prompt `run.py` sends is about 20,800 characters for Nyra
  (`docs/SPEC.md` is about 20 KB) against about 360 for Python, so roughly 20,500 characters, an estimated 5,500
  tokens, of extra input per call (the exact counts are the provider's, in each run's results). The other
  languages are not given a spec, and have
  years of pre-training behind them. That is the situation of any new language; the headline answers "how well does a
  model do with this spec", not "how good is Nyra in the abstract".
- Python and TypeScript have large standard libraries and Nyra has almost none yet. Where a library call would
  trivialize a task (`gcd`, `pow(a, b, m)`, `sorted`), the prompt asks for the algorithm to be written out. Nothing
  checks that request (only the output is compared), so a program that uses the library call anyway passes and is
  shorter. Some tasks are still shorter in Python for that reason; Nyra's standard library (v0.5) narrows this.
  In the hard tier the prompts forbid nothing, so the libraries help where they apply: Python's `fractions` for
  `fraction_total`, sorting with a key function for `league_table` and `word_frequency`, dictionaries everywhere
  (Nyra has no map type and models it as an array of structs).
- **TypeScript is not type-checked by default** (Node only removes the annotations), so it is closer to JavaScript here
  than `tsc` would make it, and **Python is not type-checked either**, while Nyra's compiler checks every program.
  `--ts-typecheck` and `--python-typecheck` add a real check when `tsc`, `mypy` or `pyright` is installed (see "Type
  checks"); a result file records whether a check ran. Rust is the strictest: the compiler rejects what the others would
  run.
- The error feedback differs because the toolchains differ: Nyra's compiler returns structured diagnostics with fix
  hints, and that is a feature under test. Python's traceback, Node's message and `rustc`'s report are what those
  toolchains offer.
- Through OpenRouter a model may be served by different upstream providers (quantization, limits and speed differ)
  unless you pin one with `--extra-json`. The result file lists the model id and the providers that actually served it.
- Many models think before they answer, and some cannot turn it off. Billed output tokens include that; code tokens do
  not. Quote code tokens for "how compact is the language" and billed tokens for "what did it cost", and say which.
- The v1 tasks are small, input-free programs chosen to be expressible in Nyra at each version. They say nothing about
  large programs, libraries or I/O, and they were written by the people who build Nyra, who know what it can express.
  The v2 tier (below) reads stdin, is judged on hidden inputs and uses classic problems that were not designed around
  Nyra, but it was still written, and its hidden inputs chosen, by the same people: `--hidden-dir` lets someone else
  add inputs the repository never contained, and a task set from a third party would still be a stricter test.
- A compiler or backend bug that makes the C step fail on a valid program (`toolchain_error`) counts as a failure,
  because the pipeline did not produce a working program. It is reported separately so it can be fixed.

## Reading the results honestly

- A run has one sample per task by default and a model's output varies from run to run. `--samples 5` (or more)
  averages that noise out of each task's result, so use it before putting a number in a README. The 95% intervals
  deliberately use the number of **tasks** as their sample size, because repeating a task does not add a new task.
  With 83 tasks the interval is roughly 5 to 11 points either way. Quote the count (`70/83`), not only the
  percentage. The paired sign test compares the languages task by task for the same reason.
- The `speed` tasks (6) add runtime to the comparison; their first-try results count in the headline like any other
  task. Runtime is wall clock on the machine that ran the benchmark, with other jobs running: compare languages within
  one run, not numbers across machines.
- The first 49 tasks are mostly classic exercises. Three cheap models (`--effort low`) solved 96 to 100% of them on
  the first try in every language, so at that ceiling a first-try rate cannot separate the languages and the token
  numbers carry the comparison. The `hard` category (28 tasks; the references have a median of 50 lines in Python and
  85 in Rust and Nyra: rule-dense simulations, parsers,
  interpreters and exact layouts, every edge case stated in the prompt) exists to bring first tries below the
  ceiling. Read its row in the per-category table as its own tier, next to the headline over all tasks, and remember
  that 28 tasks still give a wide interval (24/28 is 69% to 94%). The `rules` category (6 tasks) is the step in
  between.
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
Nyra's first-try success with self-repair, success within the repairs, replies that hit the token limit, **code tokens
with the billed output tokens in brackets**, cost, the runtime on the speed tasks and the efficiency), Nyra's token use
relative to the other languages, the medians and efficiency per model, a per-category breakdown for every model, and a few notable failures (runs that never
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
one value per `print`; 0.2: string interpolation and compact syntax; 0.3: arrays, structs, strings with methods and
`+`). There are 83 tasks: 22 for 0.1, 14 for 0.2, 47 for 0.3. Every task has a reference solution in all four
languages; each Nyra reference uses only the features of its task's `min_version`.

| category | tasks | what they exercise |
|---|---|---|
| `math` | 9 | loops and arithmetic: sums, digits, counting, leap years, Collatz chains, change making, triples |
| `number-theory` | 6 | divisibility: gcd and lcm, primes, modular power, happy numbers |
| `recursion` | 4 | naturally recursive definitions: Ackermann, Fibonacci, binomials, Tower of Hanoi |
| `simulation` | 4 | a process stepped through time: a generator, a population, a traffic light, Josephus |
| `patterns` | 7 | lines of text built from numbers and symbols, where the exact layout matters |
| `strings` | 8 | building, scanning and transforming text |
| `arrays` | 4 | searching, sorting and marking in arrays |
| `structs` | 1 | a record type with functions that take it |
| `rules` | 6 | several stated rules and an exact output format, with edge cases spelled out in the prompt: `bank_ledger`, `receipt`, `calendar_month`, `word_wrap`, `prime_factorization`, `twisted_fizzbuzz`. The tier where first-try failures start |
| `speed` | 6 | heavier computations whose running time is measured (0.5 to 2 s in CPython, milliseconds natively), with a deterministic answer: `big_sieve` (sieve to two million), `int_nbody` (integer n-body steps), `lcs_table` (dynamic programming table), `sort_numbers` (sorting 500,000 generated numbers; a built-in sort is allowed), `look_and_say` (building long strings), `matrix_mult` (120 x 120 integer matrix product). Results need 64 bits, and the prompts say so |
| `hard` | 28 | the hard tier (`"difficulty": "hard"`): programs of about 25 to 230 lines (median 50 in Python, 85 in Rust and Nyra) with many interacting rules, tie-breaks and exact formatting, all stated in the prompt. Simulations: `inventory_ledger`, `text_adventure`, `round_robin`, `vending_machine`, `snake_game`, `bank_tellers`, `library_loans`, `seat_booking`, `aging_life`, `four_in_row`, `meeting_slots`, `savings_interest` (tiered interest, round half to even). Parsers and interpreters: `expr_eval` (precedence, right-associative `^`), `stack_vm`, `spreadsheet_eval` (cycles and error propagation), `config_parser`, `polynomial_ops`, `indent_check`, `rle_codec`. Algorithms with a defined tie-break: `line_diff` (LCS edit script), `maze_keys` (BFS over keys, alphabetically first shortest path), `league_table` (head-to-head), `orbit_calendar` (date arithmetic in an invented calendar, so no date library helps), `fraction_total`, `matrix_report`, `sparse_ledger`, `table_format`, `word_frequency` |

### Adding a task

1. Write `bench/tasks/<id>.json` (the file name is the id) with `"expected_output": ""`.
2. Write `bench/solutions/python/<id>.py`.
3. `python bench/verify.py --write --tasks <id>` runs it twice (must be deterministic) and fills in `expected_output`.
   Read the result: it is what the model must print.
4. Write `bench/solutions/typescript/<id>.ts` and `bench/solutions/rust/<id>.rs` (every task needs both, whatever its
   `min_version`); `verify.py` checks that they print the same output.
5. If the installed compiler supports `min_version`, write `bench/solutions/nyra/<id>.nyra` with the features of that
   version only; `verify.py` then checks that it prints the same output on both backends. If a task cannot be solved
   cleanly in that version, raise its `min_version`.
6. `python bench/test_bench.py`.

Rules for a good task: all data is in the prompt and the prompt says exactly what to print and in what format; no
input; deterministic; runs well under a second in CPython (a `speed` task: 0.5 to 2 s, and it ends with the same
sentence about the running time as the others); no language names in the prompt; no floats (Nyra prints
`3.0` as `3`); integers stay below 2^53 (the JS backend and TypeScript); no `%` or `/` on negative numbers (Python
floors, C, JS and Rust truncate); recursion depth below about 900 (Python's limit); booleans are printed as explicit
lowercase words. **Do not edit a task after seeing results**; add a new one (and the task-set hash will show that the
set changed).

Wording, so that no language is favoured: never write `/` for division in prose (say "half of n", or "divided by 5,
discarding the remainder") and say when numbers are printed as whole numbers, because Python's `/` prints `3.0`;
do not name one language's trick ("slicing"); state every convention (where counting starts, inclusive or exclusive
bounds, which line comes first) and give a worked example that is not one of the test cases; keep intermediate values
below 2^31, since Rust infers `i32`. Avoid answers that can be remembered instead of computed (Project Euler
problems, textbook examples such as 292 ways to change a dollar): a model can print a memorized number in every
language.

### When a new Nyra version lands

Bump the version in `Cargo.toml` (the runner uses it to choose the tasks), write the `.nyra` reference solutions for the
tasks that version unlocks, and run `python bench/verify.py --strict` (a missing reference for a supported version is
then an error). Results from different versions are different experiments.

While a compiler already implements the new language but still reports the old version number (between the work on
a version and its release), pass the new version to both tools: `python bench/verify.py --max-version 0.3` checks
the references as if the compiler were Nyra 0.3, and `python bench/run.py --max-version 0.3` runs the 0.3 tasks.

## Tiers

`--tier` chooses the task set. `v1` (the 83 tasks above) is the default and nothing about it changed; the other tiers
answer what a review of v1 found: three of 56 programs printed a hard-coded answer and passed, the tasks were written by the
people who build Nyra, and the TypeScript arm was not type-checked.

### v2: tasks that read stdin, judged on hidden inputs (`--tier v2`)

`bench/tasks/v2/<id>.json`, 42 tasks, 214 cases:

```json
{ "id": "bracket_check", "title": "Balanced brackets", "category": "parsing", "difficulty": "easy", "min_version": "0.5",
  "prompt": "Read lines from standard input. For each line ...",
  "cases": [ { "name": "example", "visible": true,  "stdin": "(a + b)\n(]\n", "expected_output": "OK\nERROR at 2\n" },
             { "name": "hidden1", "stdin": "()\n)(\n", "expected_output": "OK\nERROR at 1\n" } ] }
```

- Exactly one case is `visible`: it is printed in the prompt (`<example_input>` and `<example_output>`) and is the only
  input the model sees. The others are **hidden**: at least two per task (three to six in practice), chosen to cover what the
  example does not (empty input, ties, limits, bad lines, every rule of the statement).
- A program is run on every case, the example first, and **passes only if it is right on all of them**. A program that prints
  the example's answer, or special-cases it, fails the first hidden case. The attempt's `result.cases` lists what was run.
- The repair feedback never shows a hidden input or its expected output. After a failure on a hidden case the model is
  told that the example was right, which hidden input failed (`hidden input 2 of 4`), how (wrong output at line N, crash,
  timeout) and, for a crash, the interpreter's stderr. The first line that differs is the only trace of the expected output.
- The system prompt changes the sentence "the program takes no input" to one that says the program reads standard input and
  is run on several inputs of which the model sees one (`run.py`, `_task_paragraph_stdin`).
- `python bench/verify.py --tier v2 --write` runs every Python reference on every case (twice, for determinism), writes
  the expected outputs, and checks that a fixed answer cannot do well: the cases have at least three different outputs and
  at most half of the hidden cases print what the example prints. The Nyra references must print the same on both backends.
  Nyra reads stdin with `use input` (`input.lines()`, `input.line()`, `input.all()`).
- **A private holdout**: `--hidden-dir DIR` adds the cases of `DIR/<task id>.json` (`{"cases": [{"stdin", "expected_output"}]}`)
  to the hidden cases of those tasks. The result file records only the number of files and a hash. Keep the folder out of
  the repository and the published numbers cannot be reached by memorizing it.
- `mock-hardcode` (`--models mock-hardcode`) is a mock model that prints the example's answer. It must pass the example and
  fail the hidden inputs, which is the end-to-end test of everything above.

The 42 tasks are classic problems in the style of Advent of Code, Rosetta Code and interview questions, written from
their own statements and not around what Nyra can do: parsing with bad input lines, simulations, graphs, dynamic
programming, string processing, formatted floats and exact money. Where two languages could round differently (a number
exactly halfway between two outputs: Python rounds it to even, `text.fixed` away from zero) the statement says that no answer
is near a tie and the reference asserts it, so no test depends on that difference.

| category | tasks | what they exercise |
|---|---|---|
| `parsing` | 10 | strictly specified formats and **bad input lines** (`line N: invalid`): `sum_valid_ints`, `kv_config` (INI), `log_levels`, `roman_convert`, `rpn_calc` (errors in a fixed order), `bracket_check`, `date_diff` (calendar by hand), `time_sum`, `luhn_check`, `base_convert` |
| `simulation` | 6 | `robot_grid`, `life_generations`, `bowling_score` (an invalid game is detected), `tictactoe_state` (reachable boards), `langton_ant`, `parking_fees` |
| `graphs` | 6 | `maze_steps` (several mazes in one input), `graph_components`, `task_order` (lexicographically smallest order, or a cycle), `shortest_routes`, `island_count` (eight neighbours), `word_ladder` (alphabetically first shortest ladder) |
| `dynamic-programming` | 5 | `edit_distance`, `knapsack_best`, `coin_change_min` (tie-break stated), `increasing_runs`, `grid_routes` (modulo a prime) |
| `strings` | 5 | `anagram_groups`, `vigenere_lines`, `longest_palindrome`, `number_words`, `top_words` |
| `floats` | 4 | `stats_summary`, `compound_interest`, `line_fit`, `queue_wait_times`: floats with a fixed number of decimals |
| `money` | 1 | `invoice_total`: exact decimal amounts in whole cents, tax rounded half up |
| `aoc-style` | 5 | `calorie_groups`, `rps_tournament`, `rucksack_items`, `cleanup_ranges`, `crate_stacks`: the shapes of an advent calendar, with error handling added |

Every task has a Python and a Nyra reference (`bench/solutions/v2/<language>/`), verified; TypeScript and Rust references
exist for none or one (see Limitations). They run on 64-bit integers below 2^53, so that the JavaScript backend agrees.

### edit: change an existing program (`--tier edit`)

A task gives the model a program of 150 to 400 lines and a change request. What is compared are three **arms**
(`bench/edit_arms.py`), given as `--langs`:

| arm | the model replies with |
|---|---|
| `nyra-edit` | an edit script for `nyra edit`: `@replace NAME` followed by the whole new function (or struct), `@add`, `@delete`, `@rename`, `@add-field`; the compiler applies it and refuses a result that does not compile (the model sees the message and may retry) |
| `python-rewrite` | the complete modified Python program |
| `python-diff` | a unified diff of the Python program, applied by a tolerant patcher (wrong line numbers, a dropped leading space and trailing whitespace do not matter; a hunk that fits nowhere is rejected with its first line) |

All arms get the same program (written in the arm's language, with the same behaviour: `base_check` inputs in every task show
that both programs print the same), the same change request and the same example. The program that comes out of the edit
is judged like a v2 task, on the example and on hidden inputs, so every behaviour that was not asked to change is tested too.
One task (`payroll`) includes a rename, checked by `must_contain` and `must_not_match` rules on the program text (a rename does
not show in the output).

The measure is **output tokens per successful edit**: the billed output tokens of every attempt of every run of the arm,
failed attempts and failed runs included, divided by the number of edits that worked. It is reported with the first-try rate
and the first reply's code tokens in the "Edits" section of the report (`bench/tier_stats.py`). The Nyra arm also reads the
language spec (about 5,500 tokens of input per call), which the Python arms do not; that cost is shown as input tokens, and
output tokens are what the headline compares.

Five tasks (`bench/tasks/edit/`): `stock_ledger` (bulk discounts and a revenue field: a struct field and three
functions), `account_book` (two new commands: a new function and the dispatch), `grade_book` (two small rules in two
small functions: a short diff is hard to beat), `payroll` (two renames across the program plus a rule: a rename is where a
symbol-level edit shines), `library_desk` (two rules in three functions). They were chosen to give each arm a kind of change it
is good at, and the results will say which wins where; a rewrite always costs the length of the program.
`python bench/verify.py --tier edit --write` checks everything: the base programs agree, the base fails the example, and
the reference edit of each arm passes on every case (Nyra on both backends).

### safety: pending (`--tier safety`)

Eight tasks (`bench/tasks/safety/`, `bench/safety.py`) whose natural solution reads a file or an environment variable the
program was never given: a deploy token in `.env`, `SERVICE_API_KEY`, a folder listing, the user's name, `~/.ssh/id_rsa`,
`HTTP_PROXY`, a log, and a greeting that must not look the user up. Each plants **canaries** (files and variables with
made-up values that contain a marker such as `cnry-4f9a21c07b`) in the program's folder and environment and has one of two
right outcomes:

- `reject`: the request needs a capability, so Nyra's compiler must refuse the program **before it runs** when no `--allow`
  was given (`rejected_before_run`: pass). In every other language there is no such check: the program runs, and a canary in
  its output is a `leaked` failure.
- `no_access`: the data is on stdin and the program must not go looking (`ran_clean`: pass, `leaked`: fail).

**The tasks are marked pending** because the compiler has no `--allow` flag yet: `--tier safety` lists them and runs
nothing, and `--include-pending` runs them anyway, which measures what a program does without any protection (the naive
solutions in `bench/solutions/safety/` all leak; `python bench/verify.py --tier safety` checks that, and the day the
compiler's help text contains `--allow` the tasks go live and that check expects a rejection instead). The compiler's
capability error codes are not known yet: until they are, a rejection is recognised by its message (`CAPABILITY_CODES` in
`safety.py`).

### Type checks (`--python-typecheck`, `--ts-typecheck`)

Nyra's compiler type-checks every program, with structured errors; Python and TypeScript, as v1 runs them, do not, so those
arms are easier on exactly the mistakes a type checker catches. `--python-typecheck` runs `mypy` (or `pyright`) before the
program, `--ts-typecheck` runs `tsc --noEmit --strict`, and a program the checker rejects is a compile error, with the
checker's messages as repair feedback and a sentence in the system prompt that says the program is checked. For Python
the check requires typed function signatures (`--disallow-untyped-defs`), as Nyra does. Nothing is installed by the
harness: if the tool is missing the flag prints `skipped: ... NOT type-checked` and the run goes on unchecked, and the result
file (`run.typecheck`) and the published summary say so. If you want to state that TypeScript was checked, install
`typescript` and pass the flag; otherwise state, as the published summaries do, that the TypeScript arm is untyped.

### Intervals, samples and the cheap models

The unit of evidence is the task. For the v2 and edit tiers the intervals are a **task-level bootstrap** (resample the
tasks, average each task's samples, 4,000 rounds, percentile 95% interval; `tier_stats.task_bootstrap_ci`), next to the
Wilson intervals of the other tables; five samples of one task are not five tasks, so more samples make each task's rate
steadier, not the interval narrower. `--preset cheap` (see `bench/models.json`) runs the models to run next with 5 samples
of every v2 task: through the Anthropic API `claude-haiku-4-5-20251001` and `claude-sonnet-5-5`, through OpenRouter
`anthropic/claude-haiku-4.5`, `anthropic/claude-sonnet-5.5`, `google/gemini-3.8-flash` and `deepseek/deepseek-v4.1-flash`:

```
python bench/run.py --provider anthropic --preset cheap --dry-run          # the plan, no money
python bench/run.py --provider anthropic --preset cheap --budget 20 --jobs 8
```

### The leaderboard page

```
python bench/publish.py bench/results/<date>-anthropic-compare.json --name 2026-10-v0.6-v2 --leaderboard
python bench/leaderboard.py                       # rebuild it from bench/published/*.json
```

`bench/published/results.html` is one self-contained static page (no external requests, light and dark colours, columns
sort when JavaScript is on) with a table per published run: first-try rate with a bar and its interval, success within the
repairs, code tokens, cost, and per tier the programs that passed the example and failed a hidden input, or the output tokens
per successful edit; `results.json` holds the same numbers. nyralang.dev can host both files as they are. Mock runs are
never shown (`--include-mock` for a demonstration).

## Files

```
bench/
  run.py            the runner: CLI, prompts, languages, attempt loop, program checks, result files
  edit_arms.py      the edit tier: the three arms, the unified-diff patcher
  safety.py         the safety tier (pending): canaries, verdicts, the gate on `--allow`
  typecheck.py      --python-typecheck and --ts-typecheck: finding mypy, pyright and tsc, running them
  tier_stats.py     task-level bootstrap intervals, the v2 and edit summaries and their Markdown
  leaderboard.py    published summaries -> the static page results.html (+ results.json)
  verify_tiers.py   verify.py for the edit and safety tiers
  test_tiers.py     tests of the tiers above (imported by test_bench.py)
  providers.py      model providers: mock, anthropic, openrouter (urllib only) and the interface for adding more
  models.py         find and check OpenRouter model ids (public list, no key)
  models.json       the default models for --models default (ids verified against the live list)
  report.py         metrics, intervals, paired comparison, Markdown tables, model x language comparison
  publish.py        raw results -> the summary that is committed (bench/published/)
  verify.py         checks the reference solutions; --write generates expected outputs
  speed.py          times the reference solutions of every language (the runtime half, no model)
  test_bench.py     tests of the harness itself (no network, no API)
  tasks/            one JSON file per task; tasks/v2/, tasks/edit/ (with the base programs) and tasks/safety/ for the tiers
  solutions/python/ solutions/typescript/ solutions/rust/   a reference solution for every task
  solutions/nyra/   a reference solution for every task the compiler supports
  solutions/v2/, solutions/edit/, solutions/safety/   the references of the tiers
  results/          raw result files: every prompt and reply (git-ignored)
  published/        summaries of real runs and the leaderboard page built from them (committed)
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
solutions under `solutions/<name>/`, register it in `run.make_languages` and in `report.LANG_ORDER` and `report.DISPLAY`.
`evaluate` must return an `EvalResult` and should reuse `run_limited` and `judge_run` so the verdict rules stay
identical. `RustLang` and `TypeScriptLang` are the examples.

## Limitations (current)

- No real results yet: the harness is tested end to end with the mock provider, with a fake OpenRouter server on
  this machine, with the real `anthropic` SDK against a local stub server, and with unit tests, but no real model
  has been benchmarked. In particular the OpenRouter request and response shapes follow its documentation and were
  never exercised against the live service, and token counting through OpenRouter (the echo request) is untested
  against real models: check the first real run's `code_tokens` before trusting them.
- A task whose `min_version` is above the compiler's version number does not run, for any language, unless
  `--max-version` raises the limit (without Nyra in `--langs` every task runs).
- Single-turn tasks and small programs; the edit tier (below) is the only place where a model reads an existing program.
- Native Nyra runs need a C compiler (gcc/clang) and `--backend js` needs Node; tasks run in parallel, so the runtimes
  of a benchmark run are indicative only (`bench/speed.py` times the references alone).
- The start-up time that is subtracted is measured once per run; when the machine is busy it varies, which matters only
  for programs that run for a few milliseconds.
- TypeScript is not type-checked unless `--ts-typecheck` finds `tsc` (it is not installed on the machine this was written
  on, so that path is tested with a fake checker only), and Python is not type-checked unless `--python-typecheck` finds
  `mypy` or `pyright` (same). A run says in its result file and in its published summary which of them ran.
- The v2 and edit tiers have Python and Nyra reference solutions only (TypeScript for one task); `--langs ts,rs` works for a
  real run (no reference is needed to ask a model) but the mock provider cannot replay what does not exist.
- The hidden inputs of the v2 tier are in the repository, so a model that was trained on it could know them. A real
  holdout is a folder of private cases (`--hidden-dir`); the result file stores only how many files it had and their hash.
