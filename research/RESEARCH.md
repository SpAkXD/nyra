# Nyra Research Notes (2026-10-07)

Quick, time-boxed survey (about a dozen searches/fetches). Items marked **[bg]** come from background knowledge and were not re-verified in this pass; check them before relying on them.

## TL;DR

- Agent-first languages already exist (Zero, NanoLang, NERD, MoonBit). The strongest shared idea is that the compiler is the agent's API: stable JSON diagnostics with error codes plus machine-readable fix suggestions.
- Zero (Vercel Labs, May 2026) goes furthest: `check --json`, `graph --json`, `size --json`, `doctor --json`, typed repair metadata, and effects through an explicit `World` capability. Copy the CLI surface and the diagnostic schema, not the "graph is the source of truth" part. That one is a big bet for a solo dev.
- NanoLang transpiles to C, uses unambiguous prefix syntax, requires tests per function, and ships `MEMORY.md` and `spec.json` as in-context references for LLMs. This is the closest existing match to Nyra's architecture.
- MoonBit's design process is the model to follow. Every syntax decision is judged by running LLMs and measuring generation quality. Mandatory type signatures on top-level items and flat, non-nested structure helped.
- The main risk is unfamiliarity. LLMs do worse on low-resource languages, and agents tend to write non-compiling code in unfamiliar languages (arXiv 2607.22807). Mitigate with a very small grammar, a spec that fits in context (a few thousand tokens), many examples, and fast, precise compiler feedback.
- Keep Nyra syntax close to what LLMs already know (Rust/TS/Python-like tokens). Novelty costs more than it saves in tokens.
- Multi-target pain is semantic mismatch: integer widths, overflow, string encoding, concurrency, mutation and aliasing. Decide these once in the Nyra spec and make every backend conform, even when that is slower. Gleam's per-expression target tracking is a good pattern for target-specific features.
- For C output: emit simple, boring C99 with defined behavior (checked or wrapping arithmetic, explicit casts). Compile with tcc in dev and gcc/clang -O2 in release. Perceus-style RC (Koka) and Nim ARC are the proven models for RC without a GC.
- Rust toolchain: `logos` + hand-written recursive descent/Pratt parser + `ariadne` for human output + `serde_json` for machine output. Skip parser generators (hand-written is easier to control and gives better errors).
- Learning path: Crafting Interpreters (parsing, Pratt, scoping) then Nora Sandler's "Writing a C Compiler" (codegen/IR). Both are well known and practical.

---

## 1. Existing AI/LLM-oriented languages

### Zero (vercel-labs/zero), released 2026-05-16
- Experimental systems language "for agents"; `.0` files; native binaries reported under 10 KiB.
- All diagnostics are JSON with stable codes and typed repair metadata (machine-readable repair plans).
- CLI is the agent API: `zero check --json`, `zero graph --json` (call structure), `zero size --json`, `zero doctor --json`, `zero query`, `zero patch --op`, `zero run`.
- Effects: explicit `World` capability passed to `main`; functions that can fail or perform effects are marked `raises` and use `check`. Example from the README: `pub fn main(world: World) -> Void raises { check world.out.write("hello\n") }`.
- The README says the semantic graph (`zero.graph`) is the program database and `.0` text is a projection. Edits are patch ops on node IDs with stale-hash rejection. This is the most radical idea and probably too heavy for Nyra v1.
- Sources: https://github.com/vercel-labs/zero, https://mer.vin/2026/05/zero-language-vercel-labs-agent-first-diagnostics-and-explicit-effects/, https://alexcloudstar.com/blog/vercel-zero-language-ai-agents-2026/, https://techstrong.ai/features/vercel-labs-builds-a-programming-language-designed-for-ai-agents/

### NanoLang (jordanhubbard/nanolang), January 2026
- Tiny language "designed to be targeted by coding LLMs". Transpiles to C by default; also has a bytecode VM.
- Prefix notation `(+ a b)` is supported to remove precedence ambiguity (infix is also allowed). F-strings and pipe `|>` reduce boilerplate.
- Every function requires a "shadow" test block (the README admits enforcement gaps).
- Ships `MEMORY.md` (idioms for the LLM) and `spec.json` (machine-readable spec). Type inference is local only; annotations at function boundaries.
- Simon Willison's test: Claude initially produced non-compiling code, but with the project's examples plus an agent loop it produced a working program. In-context examples plus compile feedback are what make a new language usable.
- Sources: https://github.com/jordanhubbard/nanolang, https://simonwillison.net/2026/Jan/19/nanolang/

### MoonBit
- Designed from the start for human+AI collaboration, with an integrated toolchain (compiler, package manager, LSP, formatter).
- Design principles reported: mandatory type signatures on top-level definitions, a clear split between top-level and local definitions, structural interface implementation (avoids nested impl blocks), and syntax choices evaluated by measuring LLM generation quality.
- Semantic sampler: constrains LLM decoding using the language's semantics so output is syntactically and partly semantically valid.
- Sources: https://www.moonbitlang.com/blog/moonbit-ai, https://www.moonbitlang.com/blog/ai-coding, https://conf.researchr.org/details/icse-2024/llm4code-2024-papers/9/MoonBit-Explore-the-Design-of-an-AI-Friendly-Programming-Language, https://deepengineering.substack.com/p/designing-for-ai-and-humans-with

### NERD
- Dense English-keyword language claiming 50-70% fewer tokens than TypeScript; LLM writes NERD, a bootstrap compiler emits LLVM IR, humans audit. The token claims are the project's own and I did not verify them. Source: https://letsdatascience.com/news/llms-redefine-source-code-as-nerd-b5c11ade

### Ideas worth stealing
- JSON diagnostics with stable codes, a span, and a structured `fix` (edit ops) field (Zero).
- A `nyra check --json` / `nyra spec` / `nyra graph --json` / `nyra doctor --json` CLI surface (Zero).
- Explicit effects via a capability value (`World`) rather than ambient IO (Zero). Doubles as a clean way to differ per backend (JS vs C IO).
- Spec delivered as a small machine-readable file plus an idioms cheat sheet written for the model (NanoLang).
- Built-in tests next to the function (NanoLang). Gives the agent a free feedback loop.
- Mandatory signatures at top level, local inference only (MoonBit, NanoLang).
- Evaluate syntax by measuring LLM success rate on a task suite (MoonBit).
- Optional later: constrained decoding/grammar export so an agent harness can enforce the grammar (MoonBit).

---

## 2. What makes code easy or hard for LLMs

- Token consumption varies strongly by language, consistently across models, and agents often write non-compiling code in unfamiliar languages, revise already-passing solutions, distrust provided tests, and prototype in Python first (arXiv 2607.22807, Wu/Anderson/Guha, July 2026; tested Python, Java, Rust, OCaml). Implication: fast compile feedback and trustworthy errors matter as much as terse syntax. https://arxiv.org/abs/2607.22807
- Low-resource languages show a clear gap in generation quality. Larger models benefit more from in-context learning; small models benefit more from fine-tuning ("No Silver Bullet", ICPC 2025). Nyra must rely on in-context spec and examples, since no model has been trained on it. https://arxiv.org/html/2501.19085v1
- Type-constrained decoding reduces type errors (ETH work on type-constrained code generation, arXiv 2504.09246). Supports the choice of explicit static types and a simple type system that could later be exposed for constrained decoding. https://www.arxiv.org/pdf/2504.09246v1
- Token Sugar (ASE 2025): replacing frequent code patterns with token-efficient shorthand reduces tokens. Gains are limited, so do not over-compress the syntax. https://conf.researchr.org/details/ase-2025/ase-2025-papers/92/Token-Sugar-Making-Source-Code-Sweeter-for-LLMs-through-Token-Efficient-Shorthand
- Practical heuristics [bg], consistent with MoonBit/NanoLang design notes above:
  - Use ordinary words and ASCII symbols that tokenize well; avoid rare unicode and clever operators.
  - One canonical form per construct (no optional semicolons/parens variants, no operator overloading, no implicit conversions).
  - Local reasoning: avoid nested scopes with hidden state, no macros, no implicit imports.
  - Keep keywords few; a whole spec under about 3-5k tokens fits in every prompt.
  - Error messages that state the fix beat messages that describe the problem.
- No standard benchmark exists for "LLM-native language" quality. Build a small Nyra task suite (about 30 programs) and measure pass@1 and tokens, as MoonBit does.

---

## 3. Multi-target compilers

### Haxe
- Typed AST is the central representation; macros run in an interpreter at compile time; about 11 targets. Per-target `std` differences exist and users sometimes drop to target-specific code. Reflaxe lets third parties add targets from the typed AST. https://haxe.org/manual/introduction-what-is-haxe.html, https://github.com/SomeRanDev/reflaxe, https://github.com/HaxeFoundation/HaxeManual/blob/master/content/12-target-details.md
- Known pain [bg]: Int overflow/32-bit semantics, null handling and string representation differ per target, and the standard library needs per-target implementations. Lesson: define exact numeric and string semantics in the language spec.

### Gleam
- Two targets (Erlang, JavaScript) from one typed AST. The compiler tracks target support per expression/function, so code using only portable features compiles for both and the rest produces a clear error. JS uses the plain promise model rather than emulating actors. https://gleam.run/news/v0.34-multi-target-projects, https://gleam.run/news/v0.16-gleam-compiles-to-javascript/
- Lesson: add a `@target` annotation/capability check in the type checker instead of pretending all targets are equal.

### Nim
- Frontend produces AST, then an IR, then C (primary), C++, ObjC, or JS. Generated C goes to `nimcache`. Some language features (for example guaranteed tail calls) are limited by mapping Nim functions to C functions. https://nim-lang.org/faq.html, https://nim-lang.org/1.4.0/nimc.html

### V
- Compiles to C (and has a JS backend); goals of no UB/no null/immutable by default; its design has drawn criticism for gaps between claims and implementation [bg]. Lesson for a portfolio project: under-promise in the README.

### Takeaways
- One typed, lowered IR shared by all backends; keep it small, explicit, and close to C (SSA not required). Backends should be dumb printers; all semantics (overflow checks, bounds checks, string ops, RC insertion) are decided before or in the IR.
- Run one conformance suite (the same `.nyra` programs plus expected stdout) against every backend from day one. That is how semantic drift is caught.

---

## 4. Compiling to C and reference counting

- Koka compiles straight to C with Perceus: precise RC insertion that makes programs "garbage free", plus reuse analysis (in-place update when a value is uniquely referenced). No GC or runtime system needed. https://www.microsoft.com/en-us/research/publication/perceus-garbage-free-reference-counting-with-reuse/, https://github.com/koka-lang/koka/
  - For Nyra: start with simple RC (dup on copy, drop at last use/scope end) and add "move on last use" and reuse later. Perceus needs immutable, acyclic-ish data to shine.
- Nim ARC/ORC: ARC is deterministic RC with move semantics and destructors; ORC adds a cycle collector (trial deletion). ORC is the default in modern Nim. https://nim-lang.org/docs/mm.html, https://nim-lang.org/blog/2020/10/15/introduction-to-arc-orc-in-nim.html
  - For Nyra: ARC without a cycle collector is simplest. Either forbid cyclic types by construction (immutable values, or an explicit weak/arena type), or accept leaks for cycles and document it.
- Lobster: compile-time lifetime analysis removes about 95% of RC ops; cycles checked at exit. https://strlen.com/lobster
- Arenas: good for compiler-style or request-scoped workloads; could be an explicit `region` construct later [bg].
- Dev compile speed: `tcc` compiles C very fast (about 10x faster than gcc -O2 [bg]) but optimizes little, so use `tcc` for `nyra run`/check loops and `gcc/clang -O2` for release. Keep generated C within plain C99 so tcc accepts it (avoid GNU extensions like statement expressions and computed goto in the default path). Add a `--cc` flag and auto-detect.
- Generated-C hygiene [bg]:
  - Prefix all emitted symbols (`ny_`) and mangle module paths.
  - Define arithmetic explicitly: signed overflow is UB in C. Use unsigned wraparound or `__builtin_*_overflow` with a tcc-safe fallback.
  - Emit explicit bounds checks, null checks, and `#line` directives so C compiler errors can map back to `.nyra` positions (the user rarely sees C errors if the Nyra checker is complete).
  - Don't rely on C evaluation order; lower to temporaries in the IR.
  - Runtime in a single small `nyra_rt.h` that is embedded in the compiler binary.
  - Guarantee deep recursion behavior and stack limits explicitly if the spec promises anything.
- Windows note: the dev machine is Windows 11. Ship tcc via `zig cc`, or bundle tcc; check that `gcc`/`clang`/`tcc` resolution works without WSL.

---

## 5. Rust crates for compilers

| Need | Crate | Notes |
|---|---|---|
| Lexer | `logos` | Derive-based, very fast, simple. |
| Parser | hand-written recursive descent + Pratt | Best error messages and recovery control; most tutorials use this. |
| Parser (alt) | `chumsky` | Combinators with error recovery; sister project of ariadne. Slower compile times, bigger learning curve. |
| Parser (alt) | `lalrpop` | LR(1) generator; error messages are harder to customize. |
| Diagnostics (human) | `ariadne` | Pretty multi-label output; maintained; fits chumsky. |
| Diagnostics (human, alt) | `miette` | Popular, heavier, derive macros. |
| Diagnostics (old) | `codespan-reporting` | Reported as unmaintained since 2021 per one comparison, so I would not start with it. |
| JSON output | `serde` + `serde_json` | Define diagnostic structs once and render to both ariadne and JSON. |
| CLI | `clap` | [bg] |
| Snapshot tests | `insta` | [bg] Excellent for golden-testing diagnostics and generated C. |
| Interning/arenas | `string-interner`/`lasso`, `bumpalo`, `la-arena` | [bg] Use IDs (`u32`) rather than `Rc<Node>` for the AST/IR. |
| Optional backends later | `cranelift`, `inkwell` | Not needed while targeting C. |

Sources: https://github.com/sdiehl/compiler-crates, https://docs.rs/ariadne

Small Rust compilers to read: Tristan Hume's "Writing a compiler in Rust" https://thume.ca/2019/04/18/writing-a-compiler-in-rust/ ; lolcode-to-C with Pratt parser https://lib.rs/crates/lolcode ; https://github.com/benmkw/lang ; https://github.com/Aleman778/First-Compiler ; https://github.com/Jakersnell/micro-c . I only saw search snippets for these; skim before relying on them. For bigger real-world references [bg]: `gleam-lang/gleam` (Rust, multi-target, JSON-ish error handling) and `rust-analyzer`'s `rowan`-free hand parsers.

---

## 6. Learning resources

- Crafting Interpreters (Robert Nystrom), free online at https://craftinginterpreters.com : scanner, Pratt parser, scopes, closures, VM. Written in Java and C, easy to port to Rust.
- Writing a C Compiler (Nora Sandler, No Starch, 2024): staged, test-suite-driven build of a compiler with IR and codegen; implementation language is your choice. https://nostarch.com/writing-c-compiler
- Tristan Hume's blog post above for Rust-specific lessons.
- Gleam's compiler source for a production Rust multi-target compiler (see GitHub) [bg].
- "Types and Programming Languages" (Pierce) and "Engineering a Compiler" are optional depth, not needed for v1 [bg].
- Perceus paper (linked in section 4) when you reach RC optimization; Koka's C backend source is readable [bg].

---

## Recommendations for Nyra

1. **Milestone 0: spec and eval suite before the compiler.** Write a 2-4k-token spec (`nyra.spec.md` + a JSON form) and about 30 tasks. Test Claude on them with only the spec in context. Revise syntax based on measured failure modes (MoonBit method).
2. **Syntax:** familiar tokens (Rust/TS-like), `fn name(a: T) -> R { ... }`, explicit types on all function signatures, local inference only, one loop form, one way to declare variables, no macros, no operator overloading, no implicit conversions, no exceptions (use `Result`-like values). Consider NanoLang-style inline `test` blocks.
3. **Diagnostics schema (design this first, it is the product):** `{code, severity, message, file, span:{start,end,line,col}, labels[], fix:[{edit ops}], docs}`. Stable codes like `E0101`. Emit one JSON array per run on stdout with `--json`; `ariadne` rendering for humans. Include `nyra explain E0101`.
4. **Effects:** pass an explicit capability (like Zero's `World`) for IO, with a plain `io`/`pure` marker in signatures. Gives backend portability and lets the checker tell agents precisely what a function can do.
5. **Pipeline:** `logos` lexer, hand-written Pratt/recursive-descent parser with error recovery that always returns an AST plus diagnostics, name resolution, type checker, lowering to a small typed IR (monomorphized, explicit RC ops), then backends as printers. Use ID-based arenas.
6. **Backends order:** C first (plain C99, tcc for dev, gcc/clang for release), JS second. Write one conformance suite (`.nyra` + expected output) run on both from the start. Pin down integer widths (e.g. `i64` with defined overflow behavior), UTF-8 strings, and map/hash iteration order in the spec now. JS needs `BigInt` or restricted ints for 64-bit; decide up front.
7. **Memory:** non-atomic RC at first (single-threaded), immutable-by-default values, no cycles by construction (or a documented weak ref). Add Perceus-style drop/reuse later. Skip arenas and ownership checking in v1.
8. **Target gating:** a `@target(...)` or capability check in the type checker (Gleam style) for non-portable features, with a clear JSON error.
9. **Agent CLI surface:** `nyra check --json`, `nyra build --target c|js`, `nyra run`, `nyra test --json`, `nyra fmt` (a canonical formatter makes the "one way" rule enforceable), `nyra spec`, `nyra explain`. A formatter and `fmt --check` should come early.
10. **Do not do in v1:** graph-as-source-of-truth (Zero), formal proofs (NanoLang's Coq), constrained decoding integration, SQL/WASM backends. Keep them as documented extension points; the IR boundary is the extension point.
11. **Portfolio angle:** publish the eval results (tokens and pass rate versus Python/Rust/TS for the same tasks), the diagnostic schema, and the spec. Quantified claims about LLM-friendliness are rare and would stand out.
12. **Testing:** golden tests with `insta` for diagnostics, IR, and generated C; run the conformance suite in CI on Linux and Windows.
