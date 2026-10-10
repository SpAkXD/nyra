# Nyra token cost, round 2: how far can code tokens go down? (2026-10-09)

Goal: Nyra programs at <= 0.6x the tokens of the equivalent Python (Claude tokenizer), a much cheaper spec, and no loss of
first-try correctness. This report replaces the numbers in [TOKENS.md](TOKENS.md) (o200k tokenizer, Nyra 0.3/0.4 outputs) with
measurements on the Claude tokenizer, on the v0.5 benchmark run of 2026-10-09. Scripts, caches and the hand-written sample are in
[`research/tokens/`](tokens/). No compiler, docs or `src/` file was changed, and no model was called (only the free
`count_tokens` endpoint).

## TL;DR

1. **0.6x is not reachable, by any syntax.** A program that had *no syntax at all* (only its identifiers and literals, space
   separated) already costs **0.64-0.66x** of the Python program. Python itself is ~35% syntax; Nyra cannot go below the payload.
   The realistic floor for model-written programs is **about 1.0x Python** (range 0.94-1.10 depending on model and task mix), and
   about **0.95x** if familiarity is thrown away (print without parens, no block colons...).
2. **Today: 1.28x (Opus), 1.22x (Sonnet), 1.49x (reference solutions).** Where the gap is (Opus, 8,354 tokens over 81 programs):
   indentation instead of braces ~27%; Python-style data handling Nyra lacks (tuples, unpacking, multi-assign, nested map update,
   sort keys, `{x:,}`) ~37% (measured on a 24-program sample); `let`/`var` 8%; type annotations (params + returns + locals) 14%;
   `fn main` 6%; keyword spelling 4%.
3. **Best package (0.6-Y, "terse"): 1.28 -> ~0.98x (Opus), 1.22 -> ~0.94x (Sonnet), 1.49 -> ~1.11x (reference)**, mechanical part
   measured on all 243 programs, hand part measured on 24 hand-rewritten ones. The **recommended package (0.6-X, "familiar")**
   reaches ~1.0x / ~0.97x / ~1.16x and keeps every annotation that makes errors local. The hand-written part (10% of tokens) comes
   from a 24-program sample that is harder than average; if only half of it transfers, 0.6-Y is 1.04 / 0.99 / 1.17 and 0.6-X
   1.08 / 1.03 / 1.23 (section 5).
4. **A new finding that o200k hid: on the Claude 5.x tokenizer `fn`, `ret`, `struct`, `inout`, `ex`, `else if` are 2 tokens each**
   (`def`, `return`, `class`, `type` are 1). Respelling `fn->def`, `ret->return`, `struct->class` saves 3.5-6.7 tokens per program
   (3.8-4.9% of the gap, 3.5-6.7 tokens per program) for a lexer alias, and models write `return` by habit anyway. Haiku 4.5's tokenizer does not show it
   (0 saving).
5. **Spec: 8,174 -> 3,493 tokens (-57%) with a drafted agent spec** ([SPEC-agent.md](tokens/SPEC-agent.md), v0.5 semantics, every
   syntax rule and method name kept). **Prompt caching alone cuts Nyra request cost by 55% (Opus/Sonnet), 40% (Haiku)**; both
   together by 63% (Opus/Sonnet). The harness does not cache today: `--extra-json cache_control` would not work (it marks the
   *last* block, which differs per task); the marker has to go on the system block (section 7).
6. **Code tokens are not the main cost.** Per Nyra request on Opus, input (the spec) is 71% of the cost and output 29%; billed
   output is 1.6x Python's but code is only ~69% of it. Parity in code tokens would cut billed output by ~15% (Opus
   691 -> ~590), not by 40%.

## 1. Method and data

| | |
|---|---|
| Tokenizer | `client.messages.count_tokens(model="claude-sonnet-5-5")`, per-message overhead (8 tokens) subtracted; identical results on `claude-opus-5-5`. Re-run on `claude-haiku-4-5` for the headline tables: ratios move by <= 0.03 (section 2). Counts cached in `tokens/cache_*.json`. |
| Corpus O / S | First attempts of **Opus 5.5** (81 tasks) and **Sonnet 5.5** (79 tasks) where Nyra *and* Python passed first try, from `bench/results/2026-10-09-anthropic-*.json`. Comments stripped from both languages (models write none in Nyra; the Python twins have few). |
| Corpus R | `bench/solutions/{nyra,python}`: 83 hand-written reference solutions, comments stripped (Nyra references are written with `fn main` and 970 comment tokens, which are removed). |
| Ratio | Sum of Nyra tokens / sum of Python tokens (the harness's "mean code tokens"). The median per-task ratio is lower (Opus: 1.21 today, 1.02 after the mechanical 0.6-Y rewrite) because small tasks are nearly equal. |
| Mechanical rewrites | `tokens/rewrites.py`, `rewrites2.py`: token-level source rewrites (lexer in `nyralex.py`) that turn a Nyra 0.5 program into what a model would write under a proposal. Applied to all programs. Not valid Nyra: simulations. |
| Hand rewrites | 24 Opus programs (`tokens/sample/`) rewritten by hand in the proposed syntax. `orig/` = Nyra 0.5 as the model wrote it, `X/` = 0.6-X, `Y/` = 0.6-Y, `py/` = its Python twin. Chosen to span sizes (23 to 1,734 tokens); skewed to larger programs (orig/Python = 1.35 vs 1.28 corpus-wide). Where Python used tuples I used tuples; where Python used a library Nyra lacks (`Fraction`, `re`, `Counter`) I kept the model's own Nyra algorithm. **They were not compiled** (the syntax does not exist); semantics were checked by reading. |

## 2. The gap today

Mean per program: Opus Nyra 468 vs Python 364 tokens (+103); Sonnet 398 vs 327 (+71); reference 538 vs 361 (+178).

### 2.1 Waterfall (rewrites applied cumulatively; each row is the marginal saving; tokens, ratio to Python)

| step | Opus (81) | | Sonnet (79) | | Reference (83) | |
|---|---:|---:|---:|---:|---:|---:|
| Nyra 0.5 as written | 37,868 | 1.283 | 31,442 | 1.217 | 44,694 | 1.493 |
| drop `fn main() { }` (scripts) | -488 | 1.267 | -224 | 1.209 | -1,303 | 1.449 |
| library idioms (`in`, `xs[-1]`, `a if c else b`, `{x:>3}`, `.map(int)`, comprehension for push-loops) | -529 | 1.249 | -390 | 1.193 | -427 | 1.435 |
| implicit return of the last expression | -172 | 1.243 | -150 | 1.188 | -258 | 1.427 |
| infer empty-collection types (`var xs = []`) | -321 | 1.232 | -281 | 1.177 | -208 | 1.420 |
| infer return types | -388 | 1.219 | -303 | 1.165 | -634 | 1.398 |
| **indentation instead of braces** (`:`; no `}` lines; `elif`; `if c: ret x`) | **-2,174** | 1.145 | **-1,848** | 1.093 | **-2,497** | 1.315 |
| keyword respelling `fn/ret/struct` -> `def/return/class` | -205 | **1.138** | -190 | **1.086** | -348 | **1.303** |
| *= 0.6-X mechanical* | | | | | | |
| no `let`/`var` keyword (first assignment declares) | -661 | | -533 | | -672 | |
| infer parameter types | -443 | | -361 | | -816 | |
| positional struct constructors | -336 | | -159 | | -690 | |
| *= 0.6-Y mechanical (with keywords)* | | **1.089** | | **1.045** | | **1.231** |

Haiku 4.5's own tokenizer gives the same picture (Python and Nyra both count ~12% fewer tokens): orig 1.305 / 1.239 / 1.520,
0.6-X mechanical 1.153 / 1.101 / 1.329, 0.6-Y mechanical 1.100 / 1.057 / 1.253 (Opus / Sonnet / reference).

### 2.2 Each cause alone (rewrite applied to the original programs only; shares of the Nyra - Python gap)

| # | Cause | Opus | Sonnet | Ref | tokens/prog (Opus / ref) |
|---|---|---:|---:|---:|---:|
| 1 | **Braces -> indentation** (closing `}` lines, `{` -> `:`, `else if` -> `elif`, one-line forms) | 27.1% | 33.6% | 18.8% | 27.9 / 33.5 |
| 2 | **Python data idioms Nyra lacks** (see 2.3; measured on the 24-program sample only) | ~37% | n/a | n/a | ~55 / n/a |
| 3 | `let`/`var` keyword | 7.9% | 9.5% | 4.6% | 8.2 / 8.1 |
| 4 | Type annotations: params 5.3% + returns 4.6% + locals 4.2% | 14.1% | 17.7% | 11.5% | 14.6 / 20.4 |
| 5 | `fn main() { }` wrapper (27 of 81 Opus programs, 13 of 79 Sonnet programs still write it) | 5.8% | 4.0% | 8.8% | 6.0 / 15.7 |
| 6 | Keyword spelling (`fn`, `ret`, `struct`: 2 tokens each on this tokenizer) | 3.8% | 4.9% | 3.8% | 3.9 / 6.7 |
| 7 | Labelled struct constructors | 4.0% | 2.8% | 4.7% | 4.1 / 8.3 |
| 8 | `ret` on the last line | 2.1% | 2.7% | 1.7% | 2.1 / 3.1 |
| 9 | library idioms listed above (`in`, `[-1]`, ternary, format specs, comprehension) | 4.3% | 5.5% | 3.1% | 4.4 / 5.5 |

Shares overlap a little (alone vs cumulative), so they do not add to 100%. The mechanical rewrites together explain **68% of the
Opus gap, 79% of Sonnet's, 53% of the reference gap**; the rest is cause 2 (library/idiom/structure) and model verbosity.

Tokens that do **not** matter, measured: `&&`/`and`, `||`/`or`, `=>`/`:` lambdas, `0..n`/`range`, `use`/`import`, `true`/`True`
are all equal. `"a" + str(x)` -> `"a{x}"` interpolation is **token-neutral** (0.2 tokens per program): the `{ }` and the quotes cost as
much as `" + str(`...`) + "`. Blank lines are free (merged into the newline token). Indentation as such costs 5.6% of a Python
program; deleting *all* layout (statements joined by `;`) saves 8%.

### 2.3 What the hand-written sample says about cause 2

24 Opus programs, tokens: original 13,655 (1.352x Python 10,100); mechanical 0.6-X 12,157; hand-written 0.6-X **10,838** (1.073x);
mechanical 0.6-Y 11,629; hand-written 0.6-Y **10,476** (1.037x). Hand-written is **10.9% (X) / 9.9% (Y) below mechanical**: that
10% is what tuples, unpacking, sort keys and the other structure-level features buy, on top of the syntax. Pricing each use
with the micro-benchmark (section 4) gives:

| Feature used in the hand-written programs | uses | programs | est. tokens saved |
|---|---:|---:|---:|
| in-place update through a map element (`data[a][b] = v`, `acc[n][m] += x`) | 17 | 5 | ~340 |
| `sorted_by` / `sorted` with a tuple key (replaces insertion sorts and two-key comparators) | 5 | 3 | ~300 |
| format spec `{x:,}` (replaces a 90-token grouping function) | 3 | 1 | ~255 |
| list of tuples + unpacking (`for v, s in [(1000, "M"), ...]`) | 5 | 5 | ~150 |
| destructuring (`let a, b = s.split(" ")`, `let ok, text = f()`) | 13 | 9 | ~90 |
| `.trim(chars)` | 1 | 1 | ~80 |
| `var` parameters (`fn gcd(var a: int, var b: int)`) | 10 | 5 | ~50 |
| tuple swap `a, b = b, a % b` | 7 | 5 | ~35 |
| tuple returns `(false, "")` | 3 | 2 | ~30 |
| **sum of estimates / measured difference** | | | **~1,330 / 1,319** |

The agreement is partly luck (per-use values are the micro-benchmark values discounted where the replaced code was smaller); read
it as +-40%. Tuples as a family (unpack + tables + returns + swap) are ~300 tokens of it; the nested-update, sort-key and
format-spec items are bigger but each hits few programs.

## 3. Why 0.6x is out of reach (the floor)

Python programs, Claude tokenizer, comments stripped (`tokens/floor.py`, `structure_share.py`):

| variant of the Python text | Opus (81) | Sonnet (79) | Ref (83) |
|---|---:|---:|---:|
| full program | 100% | 100% | 100% |
| layout deleted (no indentation, statements joined with `;`) | 91.8% | 91.9% | 92.2% |
| brackets, commas, colons deleted | 94.8% | 93.8% | 94.4% |
| **only identifiers + numbers + string literals, one space between** (a language with *zero* syntax) | **65.0%** | **65.8%** | **63.7%** |
| keywords only | 6.0% | 6.0% | 5.8% |

Identifiers and literals are what the program says (task data, names the model picks); no language can drop them. Even zero
syntax is above 0.6x, and a language with no operators or brackets cannot exist. Short names do not help either: renaming every
user identifier to 1-2 letters saves 0.6% of the 0.6-Y sample (Opus already names things `ts`, `ms`, `idx`); the lever belongs to
the prompt, equally for both languages.

Realistic floor: Python is already the cheapest mainstream syntax. Nyra can reach parity where its structure is Python's, and beat
it only where Python is verbose (dataclass boilerplate, `%`-formatting, regex helper functions): in the sample 4 of 24 programs
are below 0.9x (`struct_rectangles` 0.60x, `line_diff` 0.78x, `matrix_mult` 0.86x, `receipt` 0.87x) and
5 stay above 1.25x (`digit_sums` 1.9x: Python's `sum(int(d) for d in str(n))`).

Stretch options that cost familiarity, applied on top of 0.6-Y (sample, ratio to Python): keyword respell 1.043 -> 1.037; plus
`print x` without parentheses 1.034; no `:` after block headers alone 1.023; both 1.020; 1-2 letter names alone 1.031. **Even all of it
is ~0.99-1.02x.**

## 4. Proposals, ranked

Per-use cost from `tokens/micro.py` (snippet pairs counted with the tokenizer); frequency from the corpora. "Basis" M =
mechanical rewrite over all programs, H = hand-written sample, E = per-use cost x frequency. Risk is to **first-try correctness**
(section 6 has the evidence).

| # | Change | tokens/prog (Opus) | share of gap | per-use saving (micro) | Basis | Risk | Compiler effort | Verdict |
|---|---|---:|---:|---|---|---|---|---|
| 1 | **Indentation blocks** (`:`, no braces, `elif`, `if c: ret x`) | 26.8 | 26% | if/else block 6, while 2, fn 5 | M | Low-medium | L (lexer INDENT/DEDENT, parser, `fix`/`hints`/`edit`, docs, tests) | Do |
| 2 | **Tuples**: literals, `let a, b = f()`, `a, b = b, a`, `for k, v in`, tuple types, tuple sort keys | ~12 (H) | ~12% | unpack 14, table 30, return 20, swap 5 | H | **Low** (removes failures) | L (type system, 6 backends) | Do |
| 3 | Nested in-place update `m[k][j] += n`, `m[k].push(v)` | ~14 (H) | ~14% | 25 | H | Low (removes E0229) | M (checker + codegen) | Do |
| 4 | `xs.sorted_by(x => key)` returning a new array + tuple keys; `sort_by` accepts tuple keys | ~12 (H) | ~12% | up to 104 | H | Low | M | Do |
| 5 | Format specs in interpolation `{x:>8} {x:<8} {n:02} {f:.2f} {n:,}` | ~11 (H); 0.4 on Opus corpus | 0.4-10% | pad 20, zero-pad 9, fixed 8, thousands 85 | E | Low (Python syntax) | M | Do |
| 6 | **Respell `fn/ret/struct` -> `def/return/class`** (accept both, document one) | 3.9 | 3.8% | 1 per keyword use | M | **None, helps** | S (lexer aliases) | Do |
| 7 | Infer return types (recursion keeps `-> T`) | 4.8 | 4.6% | 3-5 | M | Low-medium | M | Do |
| 8 | Infer empty-collection types from first use | 4.3 | 4.2% | 4-6 | M | Low | M | Do |
| 9 | `var` parameters | 2.1 (H) | ~2% | 5 each | H | Low | S | Do |
| 10 | Implicit return of the last expression | 2.1 | 2.1% | 2 | M | Low-medium (a call at the end of a fn silently returns) | S | Do, keep `ret` |
| 11 | `x in xs` / `x not in m` for arrays, strings, maps | 0.2 | 0.2% | 3-4 | M | Low | S | Do (habit) |
| 12 | `a if c else b` (replaces the `if c { a } else { b }` expression, which looks wrong without braces) | 0.8 | 0.8% | 4 | M | Low; also catches `?:` slips | S | Do |
| 13 | `xs[-1]` literal negative index | 0.8 | 0.7% | 5 | M | Medium if computed (`xs[i-1]` at i=0 returns the last element silently) -> allow literals only | S | Do, literals only |
| 14 | `s.trim(chars)` | 3.3 (H) | ~3% | 80 | H | Low | S | Do |
| 15 | **No `let`/`var` keyword** (assignment declares) | 8.2 | 7.9% | 1 per declaration | M | **High**: typo creates a variable; block scoping vs Python's function scope; loses immutability | M | No (or later, measure) |
| 16 | **Infer parameter types** | 5.5 | 5.3% | 7-8 per fn | M | **High**: whole-program inference, errors move to call sites, signatures stop documenting | L | No |
| 17 | **Positional struct constructors** | 4.1 | 4.0% | 8 | M | **High**: same-typed fields swap silently | S | No (allow only for single-field or when types differ?) |
| 18 | `.slice(a, b)` -> `xs[a:b]` | 1.4 (E) | ~1.4% | 5 | E | Low | S | Optional |
| 19 | `int(c)` digit value, `m[k] += 1` on a missing key | ~1 (E) | ~1% | 8 / 11 | E | Medium (hides E0248) | S | Optional |
| 20 | `print x` without parentheses; no `:` after block headers | 1.2 + 6 (H) | 0.3% + 1.4% of tokens | 2 per print, 1 per block | M | **High** (unfamiliar) | S | No |
| 21 | Regex module (`re.findall` ...) | <1 (5 of 81 Python programs use `re`) | ~1% | 80-150 per use | E | Medium | L | Not now |
| 22 | Shorter method names (`pad_left` 8 tokens, `index_of` 7, `starts_with` 8 vs `find` 5, `startswith` 7) | <1 | <1% | 1-2 | E | Low but churn | S | No |

Measured and rejected for tokens: interpolation vs `+ str()` (0 saved), tabs vs spaces (0), `&&`->`and`, `=>`->`:`, `0..n`->`range(n)`,
`use`->`import` (all 0), blank lines (0), shorter identifiers in user code (0.6%).

Micro-benchmark highlights (tokens, before -> after): `fn main` wrapper around 4 statements 37 -> 25; `let p = s.split(" ")` + 2
index lets 26 -> `let a, b = s.split(" ")` 12; parallel arrays + index loop 87 -> list of tuples 57; struct for two return values
72 -> tuple 52; push loop 41 -> comprehension 19; `m[k] = m.get(k, 0) + 1` 19 -> `m[k] += 1` 8; get/modify/set of a map row 34 ->
`acc[a][m] += n` 9; `str(i).pad_left(3) + " " + ...` 41 -> `"{i:>3} ..."` 21; `.trim("'")` vs two `while` loops 91 -> 11.

## 5. Packages and projected ratio to Python

"Mechanical" = measured on every program of the corpus. "Projected" multiplies it by the hand-written/mechanical factor measured on
the sample (X 0.8915, Y 0.9009). "Half" uses half of that effect, for the case that the sample (larger, tuple-heavier programs than
average) overstates it.

| Package | contents | Opus | Sonnet | Ref |
|---|---|---:|---:|---:|
| **today** | Nyra 0.5 | 1.283 | 1.217 | 1.493 |
| P0 docs only | scripts, no `fn main` (docs, examples, llms.txt, AI_GUIDE still say it is required) | 1.267 | 1.209 | 1.449 |
| P1 "lite" | P0 + keyword respell + library idioms (`in`, ternary, `[-1]`, format specs) | 1.238 | 1.183 | 1.417 |
| **P2 0.6-X "familiar"** (recommended) | P1 + indentation blocks + inferred locals/returns + implicit return + tuples/unpacking + `var` params + nested update + `sorted_by` + `trim(chars)`; keeps `let/var`, param types, labelled constructors | mech 1.138, **proj 1.015**, half 1.076 | mech 1.086, **proj 0.968**, half 1.027 | mech 1.303, **proj 1.162**, half 1.233 |
| **P3 0.6-Y "terse"** | P2 + no `let/var` keyword + inferred parameter types + positional constructors | mech 1.089, **proj 0.981**, half 1.035 | mech 1.045, **proj 0.942**, half 0.994 | mech 1.231, **proj 1.109**, half 1.170 |
| P4 stretch | P3 + `print x` + no block colons | ~0.96 | ~0.92 | ~1.09 | 
| (floor, zero syntax) | identifiers + literals only | 0.65 | 0.66 | 0.64 |

On the sample itself (24 programs, Opus; hand-written): today 1.352 -> P2 1.073 -> P3 1.037 -> P4 1.020.

What the numbers say honestly: syntax work (P2 mechanical) gets from 1.28 to 1.14. The structure features that Python gets "for free"
(tuples, nested containers, sort keys, format specs) are worth another ~10 points. **P2 lands at about parity with Python
(1.0-1.08 on model outputs, ~1.2 on the harder reference programs); P3 is 3-5 points better and not worth its risk.** Nothing gets
close to 0.6.

## 6. Risk to first-try correctness

Evidence base: `tokens/failmine.py` over all 321 Nyra attempts of the run (Opus 85, Sonnet 86, Haiku 150 attempts; 1 / 3 / 46
of them returned errors), plus `brain/benchmarks/2026-10-09-anthropic-v05.md`. Compile errors that are Python/TS habits:

| compile error in the run | count | habit | fixed by |
|---|---:|---|---|
| `expected a type, found (` | 15 | tuple types in signatures | tuples (#2) |
| `expected a field or method name after ., found number 0` | 13 | `pair.0` / `x.1` | tuples |
| `expected ) to close (, found ,` | ~14 | tuple literal `(a, b)` | tuples |
| `expected a variable name, found (` | 6 | `let (a, b) = ...` | tuples |
| `array elements must all have one type` (E0231) | 11 | tuple-like `[name, 3]` rows | tuples |
| `a value inside a map cannot change in place` (E0229) | 9 | `m[a][b] = v` | nested update (#3) |
| `unexpected character` (E0001) | 16 | `?:`, `^`, `**` | #12 for `?:` |
| function declared `-> T` but not every path ends with `ret` (E0207) | 8 | Python-style last-expression value | implicit return (#10) |
| `cannot assign ... declared with let` (E0205) | 2 | | not a syntax issue |
| undefined variable (E0201) | 89 | largely `"${root}"` read as interpolation and a `main` variable used in another fn (a few tasks, repeated through repairs) | docs: `{{`; or an `f""`-style opt-in |

About 60 of the ~330 error occurrences (almost all on Haiku) are tuple-family: **tuples are a correctness fix first and
a token saving second.** Haiku 4.5 (no thinking) is where the language shows (Nyra 66% first-try vs Python 78%; hard tier 14% vs 46%).

| proposal | risk and why |
|---|---|
| Indentation blocks | Low-medium. Models write indentation languages reliably, and every brace-matching error (`expected } ...`) disappears. Risks: partial edits/repairs that mis-indent (the harness repairs by resending whole files, fine; `nyra edit` replaces whole symbols, fine), one-line forms (`if c: ret x` must be defined), and rewriting hints, `--fix`, ERRORS.md. Test: re-run Haiku, the sensitive model. |
| Tuples | Low; fixes ~60 errors/run. Define: `(a, b)` literal, `(int, str)` type, destructuring in `let`/`var`/`for`/assignment, `a, b = b, a`, tuple comparison for sort keys. Do not add `x.0` (unpack instead) or the Haiku slips remain. |
| Nested in-place update, `sorted_by`, `trim(chars)`, format specs, `in`, ternary, var params, implicit return | Low. Each is a Python/Rust habit; two (nested update, implicit return) remove observed failures. |
| Keyword respell | None; `return` is currently a compile error that models make (`--fix` rewrites it). Accept `fn`/`ret` as aliases for a release. |
| Inferred return types | Low-medium: errors in the body surface at the call site; recursive functions need `-> T` (two rules). Keep param types: they stay the documentation of the function. |
| Inferred locals | Low. The first `push`/assignment decides the type; the error must name the line of the first use. |
| `xs[-1]` | Medium if computed; literals only. |
| No `let`/`var` | **High.** Typos create variables silently, E0201/E0205 detection (the checks that fire most) disappears, block scoping differs from Python's function scoping, immutability is lost. |
| Inferred parameter types | **High.** Needs global inference; errors move to call sites; `x` could be int or float. |
| Positional constructors | **High.** `Item("pen", 3, 125)` vs `(price, qty)` swapped silently; the checker catches nothing for same-typed fields. |

## 7. The spec: size, a compressed draft, prompt caching

### 7.1 Sizes (Claude tokenizer; `tokens/spec_size.py`)

| text | chars | Opus / Sonnet | Haiku 4.5 |
|---|---:|---:|---:|
| `docs/SPEC.md` (what the harness sends) | 19,217 | **8,174** | 6,903 |
| `llms.txt` | 9,665 | 4,008 | 3,070 |
| `docs/AI_GUIDE.md` | 39,570 | 16,373 | 13,550 |
| `docs/ERRORS.md` | 107,975 | 42,154 | 33,927 |
| Nyra request in the harness (system prompt + task), measured = the `usage.input_tokens` of the run | | 8,471 | 7,162 |
| Python request | | 251 | 222 |
| **[SPEC-agent.md](tokens/SPEC-agent.md) (draft)** | 7,721 | **3,493 (-57%)** | **2,930 (-58%)** |
| Nyra request with the draft | | 3,790 | 3,189 |

`llms.txt` is stale (it says there are no tuples, that `fn main` is the norm, "v0.4"); `SPEC.md` still says "Nyra v0.4" in its title
and its prose describes scripts as the exception. The spec is 96% of a Nyra request's input tokens and about 70% of its cost (Opus: input $0.0435 of $0.061).

### 7.2 The compressed draft

`research/tokens/SPEC-agent.md`: same language (v0.5), every syntax rule, every method name and every runtime error that a program
can hit, written as dense lists instead of prose and tables. Dropped or cut: the memory section (`free`/`arena`/`keep` become one line,
"never required"), "Known differences between backends", the 14-row error table (6 kept), `ex` details, repeated examples, edge
sentences. Added: "no tuples, no `?:`, no `x.1`, no `and`/`or`/`not`, no negative index" (the observed slips), an explicit list of
what is *not* in the language. It is **untested against models** (no generation was run). Test plan: `python bench/run.py --provider
anthropic --models claude-haiku-4-5-20251001,claude-sonnet-5-5 --langs nyra --spec research/tokens/SPEC-agent.md`; the full 83-task
Nyra-only run costs about $1.9 (Sonnet) + $1.6 (Haiku) uncached, and compares directly with the 2026-10-09 results (Sonnet 80/83
first try, Haiku 55/83). Accept if first-try does not fall by more than the 95% interval (Haiku +-10 points). For 0.6, ship one
agent spec per language version (~3.5-4k tokens) and let `nyra explain` / the MCP `nyra_spec` serve the long text.

### 7.3 Prompt caching in `bench/run.py` terms

- Nyra's system prompt (`_NYRA_SYSTEM` + spec, 8,324 tokens) is **identical for every Nyra request** of a model; the user turn (task,
  ~150 tokens) and, on repairs, the later turns differ. `AnthropicProvider._request` sends `system` as a plain string, so nothing is cached.
- Mechanism: `system=[{"type": "text", "text": system, "cache_control": {"type": "ephemeral"}}]`, one explicit breakpoint at the end of the
  shared block. **Do not** use `--extra-json '{"cache_control": ...}'`: `_ANTHROPIC_FIRST_CLASS` would pass it as the top-level
  automatic marker, which lands on the last block (the task), a different prefix every request: it pays the 1.25x write on every request
  and never reads (the API docs call this a pure surcharge). Optionally also the top-level marker, so repair turns reuse the conversation.
- Prices (API docs): reads 0.1x input (0.05x on Opus 5.5), 5-minute writes 1.25x, 1-hour writes 2x; entries refresh on each read; an
  entry is readable once the first response *starts streaming*, so the first ~`--jobs` requests (6 here) all write. **Minimum cacheable
  prefix: 512 tokens on Opus 5.5 / Sonnet 5.5 (fine), 4,096 on Haiku 4.5**: the compressed spec (3.2k) is *below* that on Haiku and
  silently would not cache.
- Needed bookkeeping change: `usage.cost = (input_tokens * price + ...)` sums `input_tokens + cache_creation + cache_read` at list
  price; it must bill reads at 0.1x and writes at 1.25x, or `--budget` will overstate the spend. Also `bench/providers.py` prices
  Opus 5.5 at $5/$25 and Sonnet 5.5 at $3/$15 per M, while the model table in the current API docs says $4/$20 and $2/$10: check
  before quoting dollars (every ratio here is unaffected).

Nyra cost of the 2026-10-09 run, same requests, same outputs, `bench/providers.py` prices (`tokens/caching_cost.py`; 7 cache writes
assumed, the rest reads):

| model | now | cached | compressed spec | both | per request now -> both |
|---|---:|---:|---:|---:|---|
| Opus 5.5 (85 Nyra requests) | $5.18 | $2.33 (-55%) | $3.19 (-38%) | **$1.94 (-63%)** | $0.061 -> $0.023 (Python $0.013) |
| Sonnet 5.5 (86) | $3.10 | $1.37 (-56%) | $1.90 (-39%) | **$1.14 (-63%)** | $0.036 -> $0.013 (Python $0.008) |
| Haiku 4.5 (150, with repairs) | $2.23 | $1.33 (-40%) | $1.63 (-27%) | $1.63 (-27%, not cacheable) | $0.015 -> $0.011 (Python $0.005); caching the full spec is better ($1.33) |

With both, a Nyra request on Opus costs 1.75x a Python request instead of 4.7x. Caching makes spec length a smaller cost and a
slower-changing one; an 8k spec cached ($2.33 on Opus) is cheaper than a 3.5k spec uncached ($3.19), so the compression is worth doing for latency, for the
long tail of repairs, for context budget in agent use, and for Haiku, not primarily for dollars.

## 8. Recommended plan

| step | what | effort | expected effect | gate |
|---|---|---|---|---|
| 0 (now, no language change) | explicit `cache_control` on the system block + cost bookkeeping in `bench/providers.py`; update `llms.txt`, `AI_GUIDE.md`, `SPEC.md` (v0.5, scripts as the norm, no `fn main` in examples/solutions) | S | -55% bench cost; models stop writing `fn main` (-488..-1,303 tokens per 81-83 programs, 1.28 -> 1.27 Opus) | `cache_read_input_tokens > 0` on 2nd request |
| 1 | A/B the drafted agent spec on Sonnet and Haiku (Nyra only, ~$3.5) | S | -57% spec tokens; learn the real first-try cost of a short spec | first-try within noise |
| 2 | Lexer aliases `def`/`return`/`class` (+ `elif` kept), document them as canonical; `nyra fmt`/`--fix` normalizes | S | -4 tokens/program on the 5.x tokenizers, fewer E0101-style slips | none |
| 3 | Tuples + destructuring + multi-assign + tuple sort keys; nested in-place update; `sorted_by`; format specs; `in`; ternary; `trim(chars)`; `var` params | L | about 10% of Nyra tokens (~10 points of ratio) and ~60 fewer compile errors per run | Haiku first-try on hard tier |
| 4 | Indentation blocks, inferred locals/returns, implicit return | L | the largest single syntax gain: 2,174 of 37,868 Opus tokens (~7 points of ratio); with the other mechanical items the Opus ratio is 1.14 (before step 3's structure gain) | docs/hints/fix rewritten; re-run full benchmark |
| 5 | Re-measure with the full benchmark; only then consider `let`/`var`-less, param inference, positional constructors | | P3 is at most 3-5 points further and carries the three high risks | pass rate must not move |

Cumulative projection (Opus ratio, token-weighted): 1.28 -> 1.27 (step 0) -> 1.24 (steps 2 + library part of 3) -> **~1.0-1.08 (steps 3 + 4)**. Do not
promise 0.6x; promise "Python parity or better in tokens, at fewer compile errors, with an 8k-token spec cached to ~0.8k-equivalent".

## 9. Reproduce

```
W=C:/Users/vondr/AppData/Local/Temp/claude/C--Users-vondr-Documents-Projects-Nyra/cac4ea4e-167b-4d7f-bbda-2f40ab246bc8/scratchpad/ant_env.py   # loads ANTHROPIC_API_KEY from the user env, never prints it
cd research/tokens
python $W baseline_counts.py   # raw/stripped tokens of all 972 corpus programs
python $W ablate.py            # each syntax rewrite alone       -> ablate.json
python $W ablate2.py           # idiom rewrites + packages       -> ablate2.json
python $W packages.py          # cumulative waterfall + sample   -> packages.json (TOK_MODEL=claude-haiku-4-5-20251001 for Haiku)
python projection.py           # projected ratios (no API)       -> projection.json
python $W micro.py             # per-use costs                   -> micro.json
python $W keywords.py; python $W keywords2.py   # keyword / method-name spellings
python $W floor.py; python $W structure_share.py; python $W stretch.py
python mksample.py             # (re)creates sample/orig, sample/py and the X starting points; hand-edited sample/X is kept
python $W sample_count.py      # counts sample X/Y (writes sample/Y from X)
python featurecount.py; python failmine.py
python $W spec_size.py; python $W spec_agent_size.py; python caching_cost.py
```

`tokcount.py` caches every count in `cache_<model>.json`; a rerun costs no API calls for texts already counted. Limits of this study: (1) tokens
only, no pass-rate data for any proposal; (2) the hand-written sample is Opus-only and 24 programs; (3) mechanical rewrites are heuristics
(regex/token level) and may mis-handle rare shapes; (4) `fn`/`ret` effect is specific to the Claude 5.x tokenizer; (5) costs use the
harness's list prices and assume 7 cache writes per model run.
