# Positioning

What Nyra is for, what we claim, what we do not claim, and where it can lose. Every number here comes
from [`bench/published/`](../bench/published) (83 tasks, run on 2026-10-09 with Claude Opus 5.5,
Sonnet 5.5 and Haiku 4.5) or from [`perf/`](../perf). If a number in another document disagrees with
this one, the published benchmark wins.

## The pitch

> **The language agents can run unsupervised: nearly as cheap to write as Python, as fast as Rust, and
> unable to touch anything it didn't declare.**

Agents now write code and also run it. Today that means either trusting generated code with the
machine it runs on, or starting a VM or container for every call. Nyra is a language for that
code-execution step. It is small enough to sit in a prompt, it compiles to a native executable in
milliseconds of run time, its errors tell the agent exactly what to fix, and (from v0.6) a program can
use only the capabilities it declares.

The order matters. We lead with **safety** and **speed**, because those are what a harness builder
cannot get from Python. **Cost** is a condition, not a selling point: we claim it only as "near Python,
below Rust", and never as "cheaper than Python".

### Status of each claim

| Claim | Status |
|---|---|
| Fast: native code through C, about 5-6 ms vs about 300 ms for Python on the benchmark's speed tasks; within about 1.2x of hand-written C on `perf/` | Measured (v0.5) |
| Near Python in cost: about 1.2-1.3x Python's code tokens, below Rust's | Measured (v0.5) |
| Right on the first try: Opus 98%, Sonnet 96%, Haiku 4.5 66% of 83 tasks (Python: 100%, 99%, 78%) | Measured (v0.5) |
| A compiler that repairs: `--fix` fixes an unambiguous mistake without a model call | Built (v0.4), numbers published |
| Examples as part of the type check: `ex` conditions are evaluated while compiling | Built (v0.5) |
| Unable to touch anything it didn't declare: capabilities (`--allow fs`), deny by default, as compile errors | **Coming in v0.6.** Not built when this was written. Until it ships, a Nyra program has the rights of its process |

## Who it is for

1. **People building agent harnesses** who need a code-execution tool and do not want a VM or
   container per call. The harness hands the model one small spec, runs the result as a native
   process in milliseconds, and (from v0.6) grants it only the capabilities the task needs.
2. **Teams running cheap models at volume.** A small model fails less often when the language has one
   way to do each thing, strict types and errors with a stated fix, and a failure costs less when the
   compiler repairs the mistake itself. This is the group where the evidence is weakest today (see
   Haiku below), so it is the group we most need numbers for.
3. **Evaluation builders** who need programs that are deterministic and replayable: the same output on
   every backend byte for byte, ints that never wrap, no hidden state (a program is deterministic
   unless it uses `random` or `time`), and an `ex` example that pins down what a function should do.

It is not for: people who want to write programs by hand for a large codebase (no modules of your own
yet, no packages, a 0.x syntax that may still change), or anyone who needs a library ecosystem.

## What we claim, and what we do not

### Speed (claimed)

- A compiled Nyra program starts and finishes the benchmark's speed tasks in about 5-6 ms (Opus and
  Sonnet), against about 270-370 ms for Python, about 50x faster.
- On `perf/` (hand-written Nyra, C and Rust versions of the same programs) native Nyra is within about
  1.2x of C.
- **Not claimed:** that Nyra beats Rust. In the benchmark Rust was faster on the speed tasks for every
  model (Opus 2.9 ms vs 5.4, Sonnet 5.0 vs 6.4, Haiku 10 vs 32), and those programs were written by
  the models, so they differ per language. "As fast as Rust" means the same class (native code, no
  interpreter, no garbage collector), not equal.
- **Not claimed:** low latency to the first result. A native build calls a C compiler (0.5-1.5 s with
  gcc on the author's PC, skipped when the program is unchanged; `--js` needs only Node.js). The
  run is fast; the first build is not.

### Cost (claimed carefully)

- Code tokens, first attempt, same tasks: Nyra is about 1.2-1.3x Python (Opus 475 vs 375, Sonnet 408
  vs 338, Haiku 638 vs 519) and shorter than Rust (Opus 475 vs 523, Sonnet 408 vs 523, Haiku 638 vs 665).
  TypeScript is in between.
- **Not claimed:** cheaper than Python. It is not. The billed output tokens of a first attempt
  (thinking included) are higher than Python's for every model (Opus 691 vs 433, Sonnet 661 vs 461,
  Haiku 862 vs 569), and Nyra's cost per run in the benchmark was several times Python's (Opus $0.062
  vs $0.013), because Nyra's prompt carries its spec and the model thinks more about a language it
  has not seen. It is also not below Rust in billed output tokens for Opus (691 vs 605). "Below Rust"
  holds for the program itself, in code tokens.
- The honest summary is: you pay a premium over Python for the spec and the extra thinking, and get
  speed and (from v0.6) containment in return.

### Correctness (claimed with the weak spot named)

| First-try pass, 83 tasks | Nyra | Python | TypeScript | Rust |
|---|---|---|---|---|
| Opus 5.5 | 98% | 100% | 100% | 100% |
| Sonnet 5.5 | 96% | 99% | 98% | 99% |
| Haiku 4.5 | 66% | 78% | 86% | 76% |

- Strong models write Nyra almost as reliably as Python on the first try, and after up to 3 repairs
  with the compiler's feedback both reach 83/83 in every language.
- Haiku 4.5 is clearly worse in Nyra (66% vs 78% for Python; 82% vs 93% within 3 repairs; the
  first-try gap to Python is significant, sign test p = 0.01). Self-repair recovers little of it
  (+1 of 20 tried attempts). This is the largest hole in the "cheap models at volume" story and the
  first thing to work on.
- The tasks are small, input-free programs written by the people who build Nyra, with one sample per
  task. The differences between Nyra and Python for Opus and Sonnet are within noise (p = 0.5-1.0).
- Nyra is given its spec in the prompt; the other languages rely on what the model learned in
  training. That asymmetry favors the other languages, and it is stated in the benchmark itself.

### Safety (claimed as a design, not yet as a fact)

- Capabilities (`--allow fs`, deny by default) are being built for v0.6. Until they ship, **a Nyra
  program is not sandboxed**: `nyra mcp` limits memory and CPU time but a program can still read and
  write files and use the network. For code you do not trust, use a container or VM today.
- When they ship, the claim is narrower than "secure": a program that does not declare a capability
  cannot use it, and trying is a compile error with a code and a fix hint, found before anything runs.
  It is language-level least privilege. It is not a substitute for kernel isolation against a
  determined attacker, and it has to be reviewed like any security boundary before anyone relies on
  it alone.

## Against "Python in a sandbox VM"

The default answer today is a language models write well (Python) inside a microVM, container or
gVisor-style sandbox.

| | Python in a sandbox VM | Nyra |
|---|---|---|
| Isolation strength | Kernel- or hypervisor-level, language-independent, battle-tested | Language-level capabilities (v0.6): weaker until independently reviewed, but nothing to boot |
| Cost per call | A VM or container to start, keep warm or pool, and pay for | A native process; the compiler is about 1 ms, a C build 0.5-1.5 s once per program |
| Run time of the program | Python speed (about 300 ms on the speed tasks) | About 5-6 ms on the same tasks |
| What the model has to know | Python, which it knows very well (100% / 99% first try) | A new language from a spec (98% / 96%; 66% for Haiku) |
| Tokens per task | Fewest (the baseline) | About 1.2-1.3x the code tokens, more billed tokens |
| Least privilege | Whatever the sandbox policy allows, outside the program | Declared in the program, checked by the compiler, visible to the agent as an error it can fix |
| Maturity | Years of production use | 0.x, solo project |

They are not exclusive. The sensible deployment for untrusted code is both: a container for the
boundary you trust, and capabilities inside it so the agent finds out about a missing permission at
compile time instead of from a runtime denial. Nyra's pitch is that for many agent tasks the VM per
call is the expensive part, and a smaller trusted boundary plus a fast native program is enough.

## Against other agent-oriented languages

Several projects share the idea that the compiler is the agent's API (stable error codes, JSON
output, repair hints): Vercel's **Zero**, **NanoLang**, **MoonBit** and others (see
[`research/RESEARCH.md`](../research/RESEARCH.md)).

- **Zero** (Vercel Labs) is the closest in spirit: JSON diagnostics with typed repair metadata and
  explicit effects through a `World` capability. As far as we found, it has published no
  agent-success numbers: how often a model gets a program right on the first try, or after
  repairs, against another language. That is the gap Nyra fills: a benchmark with the bad numbers
  published too, so a harness builder can decide with data.
- **NanoLang** ships a spec and idioms for LLMs and requires a test per function; **MoonBit**
  measured LLM output to choose its syntax. Neither, to our knowledge, targets running agent code
  with least privilege as its main use.
- Not agent languages but real competition for the same job: **Deno** and the **Node.js** permission
  model (deny-by-default flags on TypeScript and JavaScript, which models write very well),
  **WebAssembly** runtimes with WASI capabilities, and restricted Python interpreters built for
  running LLM-written code. See the risks below.

## The three features nobody else combines

None of these ideas is new alone (effect systems, doctests and JSON diagnostics all exist). What is
unusual is having all three in a small language whose success rate with models is measured and
published.

1. **Capabilities as compile errors** (v0.6). Files, network, processes and similar powers are
   denied by default. Using one that was not declared is an ordinary compile error: a stable code,
   an exact position and a hint that names the missing capability. The agent learns what it lacks
   in the check step, and the person running it sees what the program can do before it runs.
2. **A compiler that repairs, with published numbers.** An error with exactly one possible repair
   carries a machine-applicable fix and `nyra check --fix` applies them all without a model call.
   Whether this helps is measured, not asserted: Sonnet 96% -> 100% (+3 of 3 tried), Haiku 66% ->
   67% (+1 of 20), Opus no change (0 of 1 tried). It helps when the model's mistake is syntactic, and
   not much when it is a logic mistake.
3. **Examples as part of the type check (`ex`).** `fn sq(x: int) -> int = x * x  ex sq(3) == 9`.
   Every `ex` condition is evaluated while the program compiles and never compiled into it, so a
   function that is wrong for its own examples is a compile error that shows the value it actually
   returned. For an agent the compiler becomes a test runner with no extra step, and for an
   evaluation builder the examples are an executable statement of intent.

## Competitive risks

- **Models get better at everything else.** If a cheap model writes correct Python and a sandbox
  gets cheaper, the gap Nyra fills shrinks. The first-try gap is already near zero for Opus and
  Sonnet; what remains is Haiku-class models and the speed and containment arguments.
- **Existing permission models are good enough.** Deno's deny-by-default permissions and the Node.js
  permission model give least privilege for TypeScript and JavaScript today, with a language models
  know deeply. Restricted interpreters built for LLM-written code and WASM runtimes do the same for
  Python and others. Nyra has to be clearly better at the agent loop (errors, repair, examples, measured
  first-try rate) and the runtime speed, not just equal at permissions.
- **A security claim is a liability.** The capability story is only worth the strength of its
  implementation. The compiler is the trusted part; a hole in it, in the generated C, or in a runtime
  function that bypasses the check is a hole in the boundary. The roadmap's C foreign function
  interface is a direct risk: a call into C can do anything, so it must itself require a capability
  and be reviewed before it ships. Output compiled to other languages (`--target py`, `rs`, `go`)
  is checked when Nyra compiles it, and unrestricted once it is emitted.
- **Training data.** Nyra is not in any model's training, so every call pays for the spec in the
  prompt (caching helps) and the model thinks more. A language that costs more than Python per
  task needs a reason beyond tokens, which is why the pitch leads with safety and speed.
- **Small models.** Haiku 4.5 is at 66%. If the "cheap models at volume" user cannot get that
  closer to Python, the second audience falls away.
- **Ecosystem.** No modules of your own, no packages yet, 0.x syntax that may change, and a solo
  maintainer. A harness builder who needs a library has to write it in Nyra or go elsewhere.
- **Credibility.** The numbers are small-sample, Nyra-authored tasks. A reviewer who runs a
  different task set may get different numbers. The defense is to keep publishing everything,
  including the results that do not flatter the language, and to say "coming in v0.6" until it
  ships.
