# Nyra benchmark

How often does a model write a correct program in Nyra on the first try, and how many tokens does it
spend, compared with Python on the same tasks?

Every task is given to the model once per language, with the same prompt and the same repair budget.
The program it writes is compiled/run and its standard output is compared with the expected output.

## What is measured

| metric | meaning |
|---|---|
| **pass@1** | share of runs whose **first** program printed exactly the expected output |
| **pass within N repairs** | the same, when a failed attempt is followed by feedback (the compiler's JSON errors, the Python traceback, or the model's own wrong output) and up to N retries (default 3) |
| **output tokens** | tokens the provider reports as generated, per attempt, for the first attempt, and per run (all attempts). This is the API's `usage.output_tokens`, so it **includes any thinking** the model did |
| **code tokens** | tokens of the extracted program alone, measured with the provider's token counter (`count_tokens`). It does not depend on thinking or on prose around the code, so it is the cleanest "how compact is the language" number |
| **input tokens** | the whole prompt per attempt. For Nyra it contains the language spec, which Python does not need |
| **size** | characters and non-blank lines of the extracted program (first attempt) |

Besides the headline table the summary has: a paired comparison on the runs both languages solved first
try (so neither language is measured on easy tasks while the other is measured on hard ones), a paired sign
test of the first-try differences task by task (with one sample per task it is exactly McNemar's exact test),
how first attempts failed (no code block, compile error, runtime error, timeout, wrong output), the Nyra
compiler error codes models hit most, and a per-task table. Pass rates come with 95% Wilson intervals.

## Quick start

```
cargo build --release                  # on Windows without MSVC: cargo +stable-x86_64-pc-windows-gnu build --release
python bench/verify.py                 # check every reference solution on both Nyra backends
python bench/test_bench.py             # tests of the harness itself (no API calls)
python bench/run.py --provider mock    # run the whole pipeline with the reference solutions, no API key
```

Everything above uses the Python standard library only (Python 3.9 or newer). For a real run:

```
pip install -r bench/requirements.txt  # the anthropic SDK, only for --provider anthropic
export ANTHROPIC_API_KEY=...           # (PowerShell: $env:ANTHROPIC_API_KEY = "...")  the only place the key is read from
python bench/run.py --provider anthropic --dry-run      # what would run, and the maximum number of API calls
python bench/run.py --provider anthropic --model claude-opus-5-5
python bench/run.py --provider anthropic --samples 5    # repeat every task 5 times: much tighter intervals
```

A run prints one line per finished task, then the Markdown summary, and writes:

```
bench/results/<date>-<provider>-<model>.json   everything: metadata, prompts, every reply, every verdict
bench/results/<date>-<provider>-<model>.md     the summary table
bench/results/latest.md                        copy of the most recent summary (git-ignored)
```

Existing result files are never overwritten (a `-2`, `-3`... suffix is added).

Useful options (`python bench/run.py --help` lists all):

| option | |
|---|---|
| `--langs nyra,python` | languages to run (both by default) |
| `--tasks fizzbuzz,gcd*` | task ids or patterns |
| `--max-version 0.1` | skip tasks that need a newer Nyra, **for every language**. Default: the version of the `nyra` binary |
| `--repairs 3` | repair attempts after a failed first try |
| `--samples 1` | independent runs per task and language |
| `--nyra PATH` | compiler binary (default `target/release`, else `target/debug`) |
| `--backend native\|js` | which Nyra backend runs the programs (default native, via the C compiler) |
| `--spec PATH` | spec shown to the model (default `docs/SPEC.md`, read at run time) |
| `--effort low\|...` , `--extra-json '{"thinking": ...}'` | extra Anthropic request settings; recorded in the result file |
| `--jobs 4`, `--timeout 10` | parallel evaluations, seconds per program |
| `--mock-flaky` | mock provider only: break some first attempts to exercise the repair loop |

If a run is interrupted or a provider error is fatal (bad key, unknown model, rejected parameter), the
results that finished are still saved and marked `"complete": false`; a run where nothing finished writes
no files.

## How a run works

1. **Tasks.** `bench/tasks/*.json`. Tasks whose `min_version` is above the compiler's version are dropped
   **for every language**, so the languages are always compared on the same tasks.
2. **Prompt.** The system prompt is, for Nyra, "you write Nyra, here is the complete spec" followed by
   `docs/SPEC.md`; for Python just "Python 3, standard library". Both end with the same rule: reply with exactly one
   fenced code block and nothing else. The user message is the task's prompt, verbatim. (`system_prompts` in the
   result file hold the exact texts.)
3. **Extract** the first fenced code block of the reply. No block, or an empty one, is a failed attempt (`no_code`).
4. **Check and run.**
   - Nyra: `nyra check main.nyra --json`; if that passes, `nyra build` and run the executable (or Node for `--backend js`).
     This is what `nyra run` does internally, split in two on purpose: killing a `nyra run` that timed out would leave the
     program it started running (an endless loop would live on), and the split also times compile and run separately.
   - Python: `python -I -X utf8 main.py` (isolated mode: standard library only).
   - Both: 10 s wall-clock limit, output capped at 1 MB (runaway loops are killed), no stdin, a private temp
     directory, secrets (`*KEY*`, `*TOKEN*`, ...) removed from the environment.
5. **Verdict.** Exit code 0 **and** stdout equal to `expected_output` after normalizing CRLF, stripping trailing
   whitespace from every line and dropping trailing blank lines. Leading whitespace matters.
6. **Repair.** If it failed and repairs remain, the model gets its own reply back plus one feedback message and
   answers again; the whole conversation is replayed each time (only the visible reply text, never thinking blocks).
   The feedback (all of it is built in `run.py`, in one place):
   - compile error: Nyra, the compiler's JSON verbatim; Python, the interpreter's `SyntaxError` output;
   - runtime error: exit code, the end of stderr, and what was printed before the crash;
   - timeout / output limit: that, plus the start of the output;
   - wrong output: the program's actual output, the line number of the first difference, and how many lines were
     expected. **It never shows the expected output**, because for a one-line answer that would let the model copy it;
   - no code block: a reminder of the reply format.

## Why the comparison is fair, and where it is not

Symmetric by construction:

- same tasks, written in neutral language (no prompt mentions Nyra or Python; no task depends on Python-only output
  such as `True` or `3.0`), same prompt text, same reply-format rule;
- same repair budget, same feedback templates, same time/output limits, same normalization;
- the model's settings are identical for both languages and recorded in the result file;
- the expected output is produced by the Python reference and, for every task the compiler supports, **confirmed by an
  independent Nyra implementation on two backends** (native and JavaScript), so a wrong expectation cannot hide (the
  0.2 and 0.3 tasks are confirmed by the Python reference only until their Nyra references exist; they were also checked by hand);
- token counts are the provider's own, not estimates; replies, programs and verdicts are all saved, and the file
  records the compiler version, the spec hash, the task-set hash and the model id the API actually served;
- the metrics are computed on the runs where **every** language produced a valid result, so an API error never
  makes one language look better.

Known asymmetries (they are part of the question, but you should know them):

- Nyra is given its spec (about 1,000 tokens of extra input on every call); Python is not, and has years of
  pre-training behind it. That is the situation of any new language; the headline answers "how well does a model
  do with this spec", not "how good is Nyra in the abstract".
- Python has a large standard library and Nyra has almost none yet. Where a library call would trivialize a task
  (`gcd`, `pow(a, b, m)`, `sorted`), the prompt asks for the algorithm to be written out. Some tasks are still shorter
  in Python for that reason; Nyra's own standard library (v0.6) will change this.
- The error feedback differs because the toolchains differ: Nyra's compiler returns structured diagnostics with
  fix hints, and that is a feature under test. Python's traceback is what Python offers.
- Tasks are small, input-free programs chosen to be expressible in Nyra at each version. They say nothing about
  large programs, libraries or I/O. They were also written by the people who build Nyra, who know what it can
  express; a task set from a third party would be a stricter test.
- A compiler or backend bug that makes the C step fail on a valid program (`toolchain_error`) counts as a failure,
  because the pipeline did not produce a working program. It is reported separately so it can be fixed.

## Reading the results honestly

- A run has one sample per task by default and a model's output varies from run to run. `--samples 5` (or more) averages
  that noise out of each task's result, so use it before putting a number in a README. The 95% intervals deliberately use
  the number of **tasks** as their sample size, because repeating a task does not add a new task. With 19 tasks the interval
  is roughly 15 to 20 points either way (15/19 = 79% is really "somewhere between 57% and 92%"). Quote the count
  (`17/19`), not only the percentage. The paired sign test compares the languages task by task for the same reason.
- Do not compare result files whose `tasks_sha256`, spec hash, compiler version or model differ.
- Output tokens include thinking when the model thinks, and some models cannot turn it off. Quote **code tokens** for
  "how compact is the language" and **output tokens** for "what did it cost", and say which.
- Sampling parameters such as temperature are not sent: the newest Claude models reject them and the SDK no longer
  types them. Only `--effort` and `--extra-json` change the request (for an older model,
  `--extra-json '{"temperature": 0}'` is passed through unchanged). The request is recorded in the result file.
- The harness runs model-written code on your machine without a sandbox (isolated interpreter, time and output limits,
  no secrets in the environment, but the code can still touch your files). Run real benchmarks in a container or VM.

## Tasks

`bench/tasks/<id>.json`:

```json
{ "id": "gcd_pairs", "title": "Greatest common divisors", "category": "number-theory", "difficulty": "easy",
  "min_version": "0.1", "prompt": "Write Euclid's algorithm yourself ...", "expected_output": "6\n21\n1\n250000\n6\n" }
```

`min_version` is the first Nyra version in which a natural solution can be written (0.1: functions, ints, loops,
one value per `print`; 0.2: string interpolation and compact syntax; 0.3: arrays, structs, string functions).
Categories: math, number-theory, recursion, simulation, patterns, strings, arrays, structs. There are 39 tasks:
19 for 0.1, 10 for 0.2, 10 for 0.3. The 0.2 and 0.3 tasks have no Nyra reference solution yet because those
language versions do not exist; they start running as soon as the compiler's version reaches them.

### Adding a task

1. Write `bench/tasks/<id>.json` (the file name is the id) with `"expected_output": ""`.
2. Write `bench/solutions/python/<id>.py`.
3. `python bench/verify.py --write --tasks <id>` runs it twice (must be deterministic) and fills in `expected_output`.
   Read the result: it is what the model must print.
4. If the installed compiler supports `min_version`, write `bench/solutions/nyra/<id>.nyra`; `verify.py` then checks that it
   prints the same output on both backends. If a task cannot be solved cleanly in that version, raise its `min_version`.
5. `python bench/test_bench.py`.

Rules for a good task: all data is in the prompt and the prompt says exactly what to print and in what format; no
input; deterministic; runs well under a second in CPython; no language names in the prompt; no floats (Nyra prints
`3.0` as `3`); integers stay below 2^53 (the JS backend); no `%` or `/` on negative numbers (Python floors, C and
JS truncate); recursion depth below about 900 (Python's limit); booleans are printed as explicit lowercase words.
**Do not edit a task after seeing results**; add a new one (and the task-set hash will show that the set changed).

### When a new Nyra version lands

Bump the version in `Cargo.toml` (the runner uses it to choose the tasks), write the `.nyra` reference solutions for the
tasks that version unlocks, and run `python bench/verify.py --strict` (a missing reference for a supported version is
then an error). Results from different versions are different experiments.

## Files

```
bench/
  run.py            the runner: CLI, prompts, attempt loop, program checks, result files
  providers.py      model providers (mock, anthropic) and the interface for adding more
  report.py         metrics, intervals, paired comparison, Markdown summary
  verify.py         checks the reference solutions; --write generates expected outputs
  test_bench.py     tests of the harness itself
  tasks/            one JSON file per task
  solutions/python/ a reference solution for every task
  solutions/nyra/   a reference solution for every task the compiler supports
  results/          result files (mock runs and latest.md are git-ignored)
```

## Adding a provider or a language

**Provider** (an OpenAI-compatible endpoint, Gemini, ...): subclass `providers.Provider`, implement
`complete(system, messages, meta) -> Reply` (and `count_tokens(text)` if the API can count tokens), import the SDK inside
`__init__`, and add the class to `PROVIDERS`. `messages` is a list of `{"role", "content"}` dicts; `meta` is only for
the mock provider and logging. Raise `ProviderError(..., fatal=True)` for errors that repeat on every call (key, model,
parameters) so the run stops instead of recording hundreds of fake failures.

**Language** (TypeScript, Rust, ...): subclass `run.Language` (`name`, `ext`, `system_prompt`, `evaluate(code, task)`), add
reference solutions under `solutions/<name>/`, and register it in `run.make_languages`. `evaluate` must return an
`EvalResult` and should reuse `run_limited` and `judge_run` so the verdict rules stay identical.

## Limitations (current)

- No real results yet: the harness is tested end to end with the mock provider, with the real `anthropic` SDK against a
  local stub server, and with unit tests, but no real model has been benchmarked.
- Only Nyra and Python. The roadmap also mentions TypeScript and Rust.
- Only 19 of the 39 tasks can run today (Nyra 0.1); the rest need Nyra 0.2 and 0.3.
- One model per run, single-turn tasks, small programs; nothing here measures reading or fixing existing code.
- Native runs need a C compiler (gcc/clang) and `--backend js` needs Node; tasks run in parallel, so timing numbers are
  indicative only.
