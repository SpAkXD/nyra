# Nyra: design decisions

What was deliberately **not** done, and why. Each line is a decision, not a gap: do not add the item without a new
reason (a measurement, not a taste). The measurements are in `research/TOKENS-v2.md` and the benchmark results under
`bench/results/`.

## Principle: familiar beats terse

A model writes the code it has seen. Nyra's tokens (`fn`, `let`, `if x { }`, `for x in xs`, `print`) look like Rust,
Go, TypeScript and Python on purpose, so a model half-knows the language before it reads the spec. When a shorter form
is less familiar, the shorter form loses.

Evidence: Token Sugar (ASE 2025) replaces frequent code patterns with shorthand. It cuts tokens by 13 to 15%, but the
pass rate of a model that was not fine-tuned on the shorthand falls from 94.5% to 51.2%. Nyra has no fine-tuned model
and will not have one soon, so every saved token has to be paid for in first-try correctness. Our own numbers agree:
code tokens are only about 70% of the billed output of a Nyra request and the spec is most of its input, so shaving
syntax moves the cost less than it moves the error rate. Judge a change by pass@1 first, tokens second.

## Not done now

| What | Why not |
|---|---|
| Packages, a registry, a published VS Code extension | Nothing uses them yet: a program is one file or a few with `use ./name`, and the first job is a language models get right on the first try. Revisit when there are users who ask. (The site nyralang.dev and the unpublished extension in `editors/vscode` exist.) |
| `nyra make` as a built-in command | It is a build runner. A script in `examples/` that calls `nyra build`/`run` does the same job and costs the language nothing. |
| Indentation blocks (Python style) | It is the largest single syntax saving we measured (about 7 points of the Nyra/Python token ratio), but it removes the braces that make a truncated or mis-indented reply a compile error, and braces are what models also write in Rust, Go and TypeScript. It breaks "familiar beats terse" and would rewrite the docs, hints, `--fix` and every example. |
| Dropping `let`, signature types and struct field labels | `let`/`var` mark what can change, signature types and `Point(x: 1, y: 2)` labels make an error local and its fix certain. They cost tokens but are what lets `nyra check --fix` and the error hints work without a model call. Measure with the full benchmark before removing any of them. |
| New backends (WASM, LLVM, JVM, ...) | The IR keeps one more backend cheap to add, but each one adds a column to every test and every "known differences" list. Native C and JavaScript are the supported targets; Python, TypeScript, Rust and Go stay but are experimental. (The playground runs the *compiler* as WebAssembly, with programs in its interpreter; that is not a WASM backend.) |
| C FFI | Capabilities exist since v0.6, but a call into C can do anything, so it would be a hole in every rule they state. FFI waits until it can require a capability of its own and has been reviewed as part of the boundary. |
| Writing sites in raw HTML and CSS | `use web` saves tokens mostly through its kit (components, theme, motion words), not its syntax ([`WEB.md`](WEB.md#where-the-savings-come-from)). `css(...)`, `html(...)` and `cls:` stay as escape hatches, literals only; a look the kit cannot make is a reason to improve the kit. |
| Cloning a site from a URL (`nyra web clone`) | Designed in [`WEB.md`](WEB.md#clone-from-a-url-later), not in v0.8: it needs the kit first, and its quality depends on how many real sites the kit covers. |
| Tuning the language to our own 83 tasks | The tasks are a thermometer, not a spec. A feature that helps only these 83 programs (a builtin for exactly `word_frequency`, say) overfits the benchmark and the next set of tasks will not care. Add a feature when it removes a failure pattern that shows up across models and across task sets. |

## Rule: the agent card budget

The agent card ([`AGENT_CARD.md`](AGENT_CARD.md)) is the short spec an agent gets in its context: what `nyra_spec`
serves and `bench/run.py --spec card` sends. The spec is most of the input tokens of a Nyra request, so its length is
a cost paid on every call.

- The card has a hard budget of **1,400 tokens** on Claude's tokenizer (`python tools/card_tokens.py` measures it;
  `tests/docs.rs` keeps the header and the size honest). The web card ([`WEB_CARD.md`](WEB_CARD.md)) has the same
  budget and is sent only for `use web` work. The long text is served on demand (`docs/SPEC.md`, `nyra explain`,
  `docs/ERRORS.md`).
- A feature that cannot be explained inside that budget is not ready: it is cut, simplified, or something older is
  shortened to make room.
- A new rule goes on the card only if models were seen breaking it; a feature the card does not mention does not exist
  (the card says so: "if a feature is not described, it does not exist yet").

## Other decisions in force

- Supported targets: native (through C) and JavaScript. `--py`, `--ts`, `--rs` and `--go` are experimental; their code
  and tests stay, but docs and the benchmark are written for the two supported ones.
- Scripts are the norm: a program is its top-level statements, and `fn main` is allowed but costs tokens. Examples and
  benchmark reference solutions are scripts, because models copy the style of the examples they are shown (the old
  references were 1.5 times the Python tokens, with `fn main` everywhere and no maps or lambdas).
- One way to do each thing; no implicit conversions, no shadowing, no null.
