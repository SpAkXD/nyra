# A/B: the agent card against the full spec, with prompt caching

Date 2026-10-09. Compiler `nyra 0.5.0-dev` (branch `v06-a`, off `v0.6`). Real API calls, hard cap $3, spent **$2.79**.
Raw result files (every prompt, reply and verdict) are in `bench/results/ab-card/<arm>/` on the machine that ran this
(git-ignored); the numbers below come from them with `python research/ab_card.py` (run it on those files to
reproduce the tables).

## Question

Can `docs/AGENT_CARD.md` (about 1,400 tokens) replace `docs/SPEC.md` (8,174 tokens on Claude's tokenizer) as what a model is
shown about Nyra, without writing worse programs, and what does it save once the spec is also prompt-cached?

## Setup

| | |
|---|---|
| Tasks | 38, Nyra only: the 28 `hard` tasks and 10 others (`bank_ledger`, `calendar_month`, `receipt`, `word_wrap`, `matrix_mult`, `int_nbody`, `roman_numerals`, `two_sum`, `happy_numbers`, `josephus`). The 10 were picked, not drawn: four `rules` tasks and two `speed` tasks, where Haiku or Sonnet had failed in the 2026-10-09 run, so that the easy ceiling does not hide a difference; **the "other" column is biased low** |
| Models | `claude-sonnet-5-5` (`--effort medium`), `claude-haiku-4-5-20251001` (its default, no effort) |
| Per run | 1 sample per task, **no repairs** (`--repairs 0`: pass@1 only; repairs would not fit the budget), 3 parallel jobs, `--max-tokens 16000`, `nyra check --fix` measured on the side as usual |
| Caching | `bench/run.py` default: the language text is a system block with `cache_control`, the task rule a second uncached block, the task only in the user message, and one warm-up request per model before the jobs |
| Arms | `full` = `docs/SPEC.md` (`--spec full`). `card1` = the first card (commit 3071d9a). `card2` = the card after one revision (commit d7db445), see below |
| Not run | the arm "card + ask for 1-2 `ex` examples per function" (`--ex-examples`, implemented and tested): the budget was spent (about $0.21 left, an arm costs about $0.55) |

## Results

First try (pass@1), over the 38 tasks. "hard" = 28 hard tasks, "other" = the 10 others.

| arm | pass@1 | hard | other | output tokens / first try | median code tokens | input tokens / attempt | **cost** | cache read / written (share of all input) |
|---|---|---|---|---|---|---|---|---|
| Sonnet full | **30/38 (79%)** | 20/28 | 10/10 | 1,250 | 745 | 9,399 | $0.583 | 330,828 / 8,706 (90%) |
| Sonnet card1 | 25/38 (66%) | 16/28 | 9/10 | 1,475 | 653 | 2,149 | $0.623 | 55,328 / 1,456 (66%) |
| Sonnet card2 | **30/38 (79%)** | 21/28 | 9/10 | 1,281 | 785 | 2,138 | $0.549 | 54,910 / 1,445 (66%) |
| Haiku full | **14/38 (37%)** | 6/28 | 8/10 | 1,586 | 1,284 | 7,890 | $0.360 | 278,806 / 7,337 (91%) |
| Haiku card1 | 6/38 (16%) | 2/28 | 4/10 | 1,435 | 1,252 | 1,729 | $0.340 | 0 (not cacheable) |
| Haiku card2 | 10/38 (26%) | 5/28 | 5/10 | 1,238 | 1,222 | 1,720 | $0.302 | 0 (not cacheable) |

Cost is what the run was charged by `bench/providers.py` prices (verified against the pricing page: Sonnet 5.5 $2/$10,
Haiku 4.5 $1/$5 per million tokens; cache reads 0.05x and 0.1x, writes 1.25x), warm-up request included. No arm had an error or
an unfinished run. Paired first-try comparisons (exact sign test over the tasks on which exactly one arm passed):

| comparison | only the first passed | only the second passed | p |
|---|---|---|---|
| Sonnet full vs card2 | 4 (`calendar_month`, `four_in_row`, `meeting_slots`, `text_adventure`) | 4 (`league_table`, `matrix_report`, `orbit_calendar`, `stack_vm`) | 1.00 |
| Sonnet card1 vs card2 | 4 | 9 | 0.27 |
| Haiku full vs card2 | 6 (`bank_ledger`, `expr_eval`, `int_nbody`, `polynomial_ops`, `snake_game`, `word_wrap`) | 2 (`inventory_ledger`, `vending_machine`) | 0.29 |
| Haiku card1 vs card2 | 1 | 5 | 0.22 |

(The card1 against full comparisons, from the first batch: Sonnet 9 against 4, p = 0.27; Haiku 9 against 1, **p = 0.02**.)

### Cache effect, same requests

What each run would have cost without caching (`input x list price`, same tokens) against what it did:

| arm | uncached | charged | saved | input cost uncached -> cached | output cost |
|---|---|---|---|---|---|
| Sonnet full | $1.207 | $0.583 | **52%** | $0.732 -> $0.108 | $0.475 |
| Sonnet card2 | $0.652 | $0.549 | 16% | $0.166 -> $0.062 | $0.487 |
| Haiku full | $0.609 | $0.360 | **41%** | $0.307 -> $0.058 | $0.301 |
| Haiku card2 | $0.302 | $0.302 | 0% | $0.067 (nothing cacheable) | $0.235 |

## What happened

1. **The first card was broken, and the harness found out.** `card1` told the model what Nyra does not have ("`and`/`or`/`not`")
   but never said what to write instead: it contained no `&&`, `||` or `!` at all. Sonnet then wrote `and`/`or` or invented
   placeholder names (`and_placeholder`, `or_dummy`) in 9 of its 13 failed tasks. Haiku called `len(x)` and
   `text.len(x)` instead of `x.len()` (the card listed method names without ever showing a call), wrote `return`, and
   mixed top-level statements with `fn main`. Sonnet card1 fell from 79% to 66%, Haiku from 37% to 16%.
2. **`card2` fixes exactly those omissions** and nothing tuned to a task: the replacements for `and`/`or`/`not`, one line that
   shows how methods, module functions and bare calls are written, "script or `fn main`, not both", and that `pad_left`'s
   second argument is a char. To stay inside the 1,400 tokens it dropped the list of runtime error codes (1,387 tokens now).
3. **On Sonnet 5.5 the card matches the full spec** at 30/38 each, with 4 tasks lost and 4 won (p = 1.0), at 23% of the input
   tokens per attempt (2,138 against 9,399).
4. **On Haiku 4.5 the card is worse** (10/38 against 14/38, not significant on 38 tasks, p = 0.29; the first card, 6/38, was significantly worse). Haiku has little margin: the lost tasks are the longer programs (`expr_eval`, `snake_game`,
   `polynomial_ops`), where the specifics of the full spec (maps in functions, `inout`, string methods with their
   arguments) matter. E0205 (changing a parameter, which the card says
   in one comment only) is 21 of its card2 compile errors.
5. **The dollar saving of the card is small, because output dominates.** Of Sonnet-full's $0.583, cached input is $0.108 and
   output (programs and thinking) $0.475. The card saved 6% on Sonnet ($0.549 against $0.583) and 16% on Haiku ($0.302
   against $0.360), on Sonnet all of it is cheaper input ($0.108 to $0.062), on Haiku it is output (1,238 tokens per first try
   against 1,586, more likely the failed programs being shorter than an effect of the card).
6. **Prompt caching is the big lever for the full spec**: 52% of the Sonnet run's cost and 41% of Haiku's. With the
   spec cached (90% of all input tokens were cache reads) the full spec costs about 3 cents more than the card on Sonnet.
7. **Haiku 4.5 caches nothing shorter than 4,096 tokens.** The card (1,242 tokens with the rule) is below that, so every
   request pays full input; the harness says so after the warm-up. The full spec (7,337 tokens on Haiku) is cached.
   This is why the card's advantage on Haiku is small and the cached full spec is competitive on input cost.
8. **Cache mechanics worked as documented**: the warm-up wrote the prefix (8,706 tokens on Sonnet for the spec, 1,445 for the
   card), every job then read it (`cache_read_input_tokens` = the whole prefix on each request, `cache_creation` 0), and
   no job raced the write.

## Caveats

- 38 tasks, one sample, no repairs: the intervals are wide (a difference of 4 tasks is noise, and 24/28 against 21/28 on
  the hard tier is not distinguishable). The only significant result is the first-batch Haiku card1 (p = 0.02) and the
  size of the cache saving, which is arithmetic, not statistics.
- `card2` was written after seeing `card1`'s failures **on these same tasks**. The changes are general, and the failures
  were missing language facts rather than task details, but `card2`'s score is not an independent test. A fair
  measure needs unseen tasks or a fresh sample (`--samples 3`).
- The 10 "other" tasks were chosen where models had failed before; do not read the "other" column as a rate over the
  whole 55 easier tasks (on those, in the 2026-10-09 run with the full spec, Sonnet passed 54 of 55 and Haiku 51 of 55 on the first try).
- Thinking is on for Sonnet (`--effort medium`) and the output cost includes it.

## Conclusion and recommendation

- **Sonnet 5.5 and similar: use the card.** Same accuracy, 77% less input, a little cheaper, and a program that does not need the 8k
  spec to be read at all. Put it in a cacheable system block anyway (it is free to do so).
- **Haiku 4.5 and weaker models: keep the full spec, cached.** It is both more accurate and, with the cache, nearly as cheap.
- **Cache whichever you send.** Caching the full spec saves 41% to 52% of a Nyra run's cost; the card saves less than the cache
  does. The README says exactly this.
- The card has a hard budget (1,400 tokens) and `tests/docs.rs` makes sure it still describes the compiler: its example
  runs, its name lists equal the compiler's, and everything in "Not in Nyra" is an error. The next card change should displace
  text, as the README and the header say, and be re-measured with `tools/card_tokens.py` and a run of this A/B.
- Not done: the arm that asks for `ex` examples (budget), repairs (the card's effect on the repair loop is untested; the
  compiler's error messages help there regardless), Opus 5.5.

## Reproduce

```
python tools/card_tokens.py                       # card size on Claude's tokenizer (free)
python bench/run.py --provider anthropic --model claude-sonnet-5-5 --effort medium --langs nyra --spec card \
    --tasks <the 38 ids> --repairs 0 --jobs 3 --time-runs 0 --budget 0.6 --out bench/results/ab-card/sonnet-card2
python bench/run.py ... --spec full ...                 # the other arm
python bench/run.py ... --spec card --ex-examples ...   # the arm that was not run
python research/ab_card.py sonnet-full=bench/results/ab-card/sonnet-full/*.json sonnet-card2=... --pair sonnet-full:sonnet-card2
```

Spent: smoke test $0.032; full/card1 arms $0.583 + $0.623 + $0.360 + $0.340; card2 arms $0.549 + $0.302 = $2.79 (the
tool's own accounting, token counts times list prices; the token-count calls that measure the card are free).
