# Nyra token cost: where the extra tokens go and how to cut them (2026-10-08)

Question: Nyra programs use about 1.3–1.6x the code tokens of Python. Which syntax and library
changes would close the gap without making Nyra harder for models to write correctly?

## TL;DR

- **Keywords are not the problem.** In o200k, `fn`, `let`, `var`, `ret`, `struct`, `step`, `arena`
  are each 1 token, the same as `def` and `return`. Only `inout` is 2. Shortening keywords saves 0.
- **The biggest lever already exists in v0.4: top-level scripts.** Every model output still wraps
  the program in `fn main() { }`. That wrapper and its indentation are **28% of the gap** in model
  outputs. Models write it because the docs still say it is required (see "Stale docs" below).
- **Syntax overhead explains about half of the gap; the rest is library and idioms.** Python wins
  with `f"{x:>8}"`, comprehensions with `sum(...)`, `[::-1]`, dicts, and `sorted(key=...)`. Nyra
  programs re-implement these as loops and helper functions.
- **Recommended package** (10 low-risk changes, mostly library): about **-21% code tokens** on the
  benchmark's model outputs (1.41x Python → about 1.11x) and **-17%** on the harder reference solutions
  (1.49x → about 1.24x).
- **Don't do these:** drop `let`, positional struct construction, and parameter type inference. Each
  saves 2–9 tokens per program but adds silent bugs or worse errors. Indentation-based blocks would
  save the most of any single syntax change, but it means redesigning the language. Not for v0.5.
- **Code tokens are the smaller cost.** Models' billed output for Nyra exceeds Python by
  90–300 tokens per task. The code accounts for only 32–42 of those. The rest is reasoning about an
  unfamiliar language. Familiar syntax helps more than terse syntax.

## Method

- Tokenizer: `tiktoken` `o200k_base`. Its Nyra/Python ratios match the benchmark's measured
  `code_tokens` (each model's own counter) to within 0.05. For example, Haiku: 1.32 o200k vs 1.37
  measured; DeepSeek: 1.59 vs 1.59; Gemini: 1.31 vs 1.31.
- **Corpus A, model outputs:** first attempts of DeepSeek V4.1 Flash, Gemini 3.8 Flash and Claude
  Haiku 4.5, from `bench/results/2026-10-07-openrouter-*.json` (Nyra 0.3, 49 tasks). That's 145
  programs where all 4 languages produced code. Means: Nyra 117 tokens, Python 83 (comments stripped).
  `bank_ledger` is excluded from the idiom estimates because DeepSeek's Python answer hardcodes the output.
- **Corpus B, reference solutions:** `bench/solutions/{nyra,python,typescript,rust}`, 77 tasks,
  including the harder `rules` tasks. Means: Nyra 409 tokens, Python 274 (comments stripped).
- **Syntax causes** are measured by mechanical source rewrites over the whole corpus. Examples:
  strip param types, unwrap `fn main`, delete `}` lines. Each count is the change applied alone.
- **Library and idiom causes** are measured by rewriting real excerpts by hand (24 before/after pairs).
  The saving per use is multiplied by how often the Python solutions use the matching feature.
- **Validation:** 11 whole programs were rewritten in the proposed syntax (table at the end).

| | Nyra | Python | TypeScript | Rust | Nyra / Python |
|---|---|---|---|---|---|
| A: model outputs (o200k, with comments) | 17,051 | 12,275 | — | — | 1.39 |
| A: Haiku / DeepSeek / Gemini separately | | | | | 1.32 / 1.59 / 1.31 |
| B: reference solutions (o200k, with comments) | 32,450 | 21,354 | 26,081 | 30,021 | 1.52 |

## 1. Diagnosis: what Nyra's extra tokens are

Gap = Nyra tokens − Python tokens, comments stripped. Corpus A gap: 4,941 tokens (34 per program).
Corpus B gap: 10,352 (134 per program). Comments are excluded because the Nyra references carry
970 comment tokens to Python's 226. That's 7% of B's raw gap, and it's style, not syntax.

| Cause | A: tokens | A: % of gap | B: tokens | B: % of gap |
|---|---|---|---|---|
| `fn main() { }` wrapper and the extra indentation level | 1,372 | **28%** | 908 | 9% |
| Closing-brace lines (`}` alone on a line costs 1–2 tokens) | 995 | 20% | 1,256 | 12% |
| `let` / `var` keywords | 353 | 7% | 630 | 6% |
| Parameter type annotations | 324 | 7% | 691 | 7% |
| Return type annotations (`-> int`) | 160 | 3% | 399 | 4% |
| Local annotations (`var xs: [int] = []`) | 84 | 2% | 208 | 2% |
| Struct construction labels (`Item(name: …, price: …)`) | 72 | 1% | 539 | 5% |
| `ret` on the last line (vs. implicit return) | 69 | 1% | 125 | 1% |
| `.len()` method vs `len()` | ~0 | 0% | ~0 | 0% |
| String interpolation `"{x}"` (already shorter than `f"{x}"`) | <0 | <0 | <0 | <0 |
| **All syntax causes above, applied together** | 3,086 | **62%** | 4,440 | **43%** |
| **Residual: library and idioms** | 1,855 | **38%** | 5,912 | **57%** |

The residual breaks down as follows. The figures are measured on excerpts; see section 2 for how often each occurs.

| Residual cause | Typical excerpt (Nyra → Python-equivalent) | Saving per use |
|---|---|---|
| No format specs: pad helpers, zero-padding via `if c < 10 { "0" }` | receipt, calendar, league table | 12–129 |
| No comprehension: push loops, counting loops, digit-sum loops | vowel_count, digit_sums, number_rows, caesar | 15–43 |
| No value-returning helpers: string reverse, `sum`, `gcd` | palindromes (58), reverse_digits (54), lcm (57) | 50–60 |
| No map type: linear `find(xs, key)` helpers plus `-1` checks | bank_ledger, league_table | 57–113 |
| No sort with a comparator: hand-written insertion sort | league_table | 63 |
| Immutable parameters force a copy (`var m = n`) | 28 of them in A, 24 in B | ~6 |
| Index loops `for i in 0..xs.len()` instead of element plus index | 50 in B | ~5 |
| Model verbosity: existing features not used (`"aeiou".contains(c)`, `print(if …)`) | vowel_count, leap_years | varies |

Two tokenizer facts:
- `}` costs about 1 token per block, the same as Python's `:`, plus 1 more token per closing line.
- `0..n` costs 4 tokens (`' '`, `0`, `..`, `n`), one more than `range(n)`. Not worth changing.

## 2. Proposals, ranked by tokens saved per typical program

How to read the tables:
- "Per program" is the mean of the A and B savings.
- **M** = measured over the whole corpus by a mechanical rewrite.
- **E** = estimated: the saving per use from hand rewrites, times the share of Python solutions that use the feature.
  - Format specs: Python uses them in 4% of A and 13% of B.
  - Comprehensions: 12% / 32%.
  - `sum`/`min`/`max`/`any`/`all`: 11% / 18%.
  - Reversal: 6% / 0%.
  - Dict or set: 8% / 32%.
  - Sort: 1% / 12%.

### Top 10 by tokens saved

| # | Change | A tokens/prog | B tokens/prog | Per program | Basis | Accuracy risk | Verdict |
|---|---|---|---|---|---|---|---|
| 1 | Indentation blocks: drop `}` lines | 6.9 | 16.3 | **11.6** | M | **Medium-high.** Means redesigning one-line `if c { ret x }` and `if`-expressions. Indentation errors appear when models patch or repair code partially. Every doc and example changes. | ⚠ Not for v0.5 |
| 2 | Scripts by default (top-level statements, no `fn main`). **Already legal in v0.4.** | 9.5 | 11.8 | **10.7** | M | None | ✅ Fix docs and examples |
| 3 | Map type `{str: int}`: `m[k]`, `m.has(k)`, `m.keys()`/`values()` in insertion order | ~2 | ~15 | **~8.5** | E (57–113 per use) | Low. Familiar from every mainstream language. | ✅ |
| 4 | Comprehension `[e for x in xs if c]` | ~4 | ~10 | **~7** | E (15–43 per use) | Low. Python syntax is heavily trained. | ✅ |
| 5 | Parameter type inference | 2.2 | 9.0 | **5.6** | M | **High.** Signatures stop documenting code. Errors move to call sites. Needs whole-program inference. | ❌ Hurts |
| 6 | Drop `let` (first assignment declares) | 2.4 | 8.2 | **5.3** | M | **High.** A typo silently creates a new variable. The no-shadowing errors go away. | ❌ Hurts |
| 7 | Format specs in interpolation: `{x:>8}` `{x:<8}` `{n:02}` `{f:.2}` | ~2 | ~8 | **~5** | E (12–129 per use) | Low if limited to the Python subset of align, width, zero-pad and precision | ✅ |
| 8 | Array and string value helpers: `.sum()` `.min()` `.max()` `.rev()` (returns new), `c.digit()` | ~4 | ~3 | **~3.5** | E (50–60 per use) | Low. Discoverable names that return a value. | ✅ |
| 9 | `xs.sort_by(before)`, where `before` is a named `fn(a: T, b: T) -> bool`. No lambdas. | ~0.3 | ~7.6 | **~4** | E (63 per use) | Low | ✅ |
| 10 | Positional struct construction `Item("apple", 125, 3)` | 0.5 | 7.0 | **3.8** | M | **High.** Same-typed fields swap silently, e.g. price and quantity. | ❌ Hurts |

### Next five (smaller, mostly low risk)

| # | Change | A | B | Per program | Basis | Risk | Verdict |
|---|---|---|---|---|---|---|---|
| 11 | Return type inference for block functions. One-line `fn f(x: int) = e` alone saves only 0.1 / 1.4. | 1.1 | 5.2 | 3.2 | M | Medium. Recursion still needs `->`, so there would be two rules. | ⚖ Optional |
| 12 | `for i, x in xs` (index plus element) | 0.2 | 3.1 | 1.7 | M | Low | ✅ |
| 13 | Infer an empty array's type from its first use (`var xs = []`) | 0.5 | 2.7 | 1.6 | M | Low to medium. Errors become less local. | ✅ |
| 14 | `var` parameters (`fn f(var n: int)`), so no copy into a local | 1.2 | 1.9 | 1.6 | M | Low. Value semantics already make it safe. | ✅ |
| 15 | Implicit return of the last expression | 0.5 | 1.6 | 1.1 | M | Low to medium. A Rust-style footgun when a statement ends a function. | ❌ Not worth it |

Changes measured and rejected:

| Change | Saving | Why not |
|---|---|---|
| Shorter keywords (`f`, `r`, `l`) | **0**: every keyword is already 1 token | Cryptic, and no gain |
| Destructuring `let [kind, amount] = tx.split(" ")` | 0 (−1 to +1 per use) | A clarity win, not a token win. Add it for readability if wanted. |
| Struct field defaults plus field-name punning `Point(x, y)` | ~0.5 | Too rare in the corpus |
| Closing several blocks with `}}}` | 1–2 per nest | Cryptic. Models miscount. |
| `..n` for `0..n` | 1 per loop | Unfamiliar, and saves almost nothing |

## 3. Expected total

The recommended package is items 2, 3, 4, 7, 8, 9, 11, 12, 13 and 14:

| | A: model outputs | B: reference solutions |
|---|---|---|
| Nyra today, tokens per program | 117 | 409 |
| Saved by the package | ~25 (**−21%**) | ~68 (**−17%**) |
| Nyra after | ~92 | ~341 |
| Python | 83 | 274 |
| Ratio vs Python: today → after | **1.41 → ~1.11** | **1.49 → ~1.24** |
| Scripts by default only (docs, no compiler work) | 1.41 → 1.30 | 1.49 → 1.45 |

Whole-program check: 11 real programs rewritten in the proposed syntax using the package only. The
sample is biased toward the tasks with the largest gap, so it overstates the corpus-wide saving.

| Program | v0.4 as written | Proposed | Python | Change |
|---|---|---|---|---|
| ref/receipt | 277 | 209 | 158 | −25% |
| ref/fizzbuzz | 71 | 64 | 63 | −10% |
| ref/league_table | 1,150 | 828 | 708 | −28% |
| gemini/palindromes | 118 | 47 | 47 | −60% |
| gemini/digit_sums | 92 | 37 | 36 | −60% |
| gemini/happy_numbers | 159 | 110 | 89 | −31% |
| gemini/vowel_count | 79 | 31 | 37 | −61% |
| gemini/receipt | 322 | 155 | 186 | −52% |
| gemini/reverse_digits | 92 | 28 | 39 | −70% |
| gemini/caesar_cipher | 96 | 59 | 57 | −39% |
| gemini/number_rows | 51 | 29 | 22 | −43% |
| **Total** | **2,507** | **1,597** | **1,442** | **−36% (1.74x → 1.11x Python)** |

## 4. The larger cost: reasoning tokens

From `2026-10-07-openrouter-compare.md`, first attempt, mean tokens per task:

| Model | Code: Nyra − Python | Billed output: Nyra − Python | Not code (reasoning and prose) |
|---|---|---|---|
| DeepSeek V4.1 Flash | 112 − 70 = **42** | 719 − 420 = **299** | ~257 |
| Gemini 3.8 Flash | 137 − 105 = **32** | 546 − 274 = **272** | ~240 |
| Claude Haiku 4.5 | 159 − 118 = **41** | 1,120 − 1,033 = **87** | ~46 |

For two of the three models, the extra reasoning is 6–8x the extra code. So every proposal is judged
first by "does a model already know this shape?" and only then by token count. That is why the
recommended items copy Python's comprehension and format-spec syntax and common method names. It is
also why shorter-than-Python inventions are rejected.

## 5. Stale docs (cheap fix, biggest single win)

v0.4 allows top-level statements (SPEC.md "Rules"), but the docs models learn from still say otherwise:

- `llms.txt` line 9: "must contain `fn main() { ... }` ... Nothing else may be at the top level".
- `docs/AI_GUIDE.md` line 94: `fn main() {  // required`. Line 280: the E0208 row "add `fn main() { ... }`".
- 29 of 31 `examples/*.nyra` and 77 of 77 `bench/solutions/nyra/*.nyra` use `fn main`.

Switching these to script style should give most of item 2's 9.5–11.8 tokens per program. It needs
no compiler change. Re-run the benchmark afterwards to confirm that models follow it.

## Suggested order

1. Docs and examples to script style (item 2). Re-measure.
2. Library only, no grammar change: `.sum() .min() .max() .rev()`, `c.digit()`, `sort_by(fn)`, `for i, x in xs`.
3. Format specs in interpolation (a contained lexer change).
4. Comprehensions, then maps. These are the largest features. Maps matter most for the harder tasks.
5. Measure again before considering return-type inference. Leave indentation blocks out of scope.
