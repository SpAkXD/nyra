# Speed where agents feel it: `nyra run`, appends, warnings (v0.6)

What an agent waits for is the time from `nyra run prog.nyra` to the first line of output. For a
small program that is almost all C compiler: Nyra itself takes 1 to 7 ms, the program a few
milliseconds. This note records what was measured, what was changed and what was not.

Machine for every number: Windows 11, gcc 15.2.0 (MSYS2 UCRT64), Node 25, Python 3.14, a laptop that
eight other build jobs shared, so single numbers jump by 30% or more. Where it matters the tables
give CPU time (the whole gcc process tree, from a Windows job object, best of 7 to 9), which load
disturbs much less than wall-clock time. tcc is not installed here and was not tried.

## 1. Where the compile time goes

gcc is four to five processes (driver, cc1, as, collect2, ld). An empty `int main(void){return 0;}`
costs about 280 ms CPU on this machine, of which the C headers are about 80 ms and the link about
140 ms. Nyra's runtime and the program add what is left, and only the part gcc keeps: unused runtime
functions are dropped before optimization.

Compile time (CPU ms, best of 7; `-fwrapv -ffp-contract=off -s` in every column):

| program | -O2 | -O1 | -Og | -O0 |
|---|---|---|---|---|
| fib_recursive | 391 | 359 | 469 | 906 |
| big_sieve | 781 | 672 | 609 | 969 |
| word_frequency | 1297 | 875 | 688 | 844 |
| text_adventure | 1875 | 1125 | 1000 | 1016 |
| spreadsheet_eval | 2203 | 1312 | 984 | 1125 |
| perf/structs | 828 | 672 | 609 | 1016 |
| perf/dp | 1000 | 797 | 625 | 1047 |
| **sum** | **8375** | **5812** | **4984** | **6922** |

-O0 is not the fast setting one expects: without optimization gcc keeps every `static` function it is
given, so all of Nyra's runtime (about 60 KB of C) is compiled for every program. (`-ftoplevel-reorder`
does not change that.) Making -O0 cheap would need the code generator to emit only the runtime
functions a program uses, which is a larger change than this one.

Run time of the executables (wall ms, best of 3, same programs; noisy):

| program | -O0 | -Og | -O1 | -O2 |
|---|---|---|---|---|
| perf/fib | 892 | 641 | 447 | 443 |
| perf/dp | 2119 | 405 | 296 | 326 |
| perf/sieve | 4285 | 855 | 428 | 703 |
| perf/sort | 978 | 457 | 366 | 311 |
| perf/strings | 2216 | 1417 | 1331 | 1186 |
| perf/structs | 1954 | 546 | 101 | 160 |
| look_and_say | 127 | 229 | 261 | 184 |
| big_sieve | 353 | 137 | 170 | 125 |

-O0 runs 3 to 10 times slower (the checked arithmetic and index checks are inline functions that are not
inlined), -Og 1.3 to 2 times slower, and -O1 is within noise of -O2.

Decision: `nyra run` compiles with **-O1**: a third less compile time than -O2 on the larger programs,
and the program is as fast. `nyra run --release` and `nyra build` keep -O2. -Og compiles somewhat
faster still, but a program that runs for a second pays for it.

The cache keeps a separate executable per level (`run` and `run --release`/`build` of one file do not
evict each other), and the cache key contains the flags.

## 2. Time to first output

`perf/first_output.py`: the whole `nyra run` command, wall-clock, best of 5. "cold" starts with an
empty cache folder (the C compiler runs), "warm" has the executable cached. Python is `python -I
prog.py`; Node runs the file `nyra build --js` wrote.

| program | cold, -O2 (before) | cold, -O1 (now) | warm, -O2 | warm, -O1 | python | node (--js) |
|---|---|---|---|---|---|---|
| fizzbuzz | 550 | 505 | 36 | 36 | 245 | 100 |
| fib_recursive | 494 | 516 | 38 | 38 | 268 | 103 |
| word_frequency | 1399 | 900 | 42 | 41 | 255 | 100 |
| look_and_say | 991 | 923 | 74 | 248 | 1844 | 780 |
| big_sieve | 1731 | 1126 | 193 | 92 | 1825 | 481 |
| text_adventure | 1668 | 1089 | 42 | 39 | 261 | 108 |
| spreadsheet_eval | 1894 | 1177 | 45 | 48 | 279 | 122 |

(milliseconds; `python perf/first_output.py -n 5 --markdown`.)

Reading it:

- A cold `nyra run` of a program of 10 to 50 lines is 0.5 s on this (loaded) machine whatever the flag:
  gcc starts five processes and links, and `fizzbuzz` or `fib_recursive` have nothing to optimize. Python
  needs 245 to 280 ms for the same programs here, because the machine is slow right now (it starts in
  about 20 ms when idle), so the ratio matters more than the numbers.
- Programs with more code (`word_frequency`, `text_adventure`, `spreadsheet_eval`, `big_sieve`) are 30 to
  40% faster to first output with -O1: 1.4 s to 0.9 s, 1.7 s to 1.1 s, 1.9 s to 1.2 s, 1.7 s to 1.1 s.
- A warm run (cached executable) is 40 ms, process start included.
- -O1 can run a program slower: `look_and_say` takes 248 ms against 74 ms at -O2 (`term.chars()` and the
  strings are what -O2 inlines better). `run --release` is there for a program that runs for long.
- What would bring a cold run near Python's is not a flag: the floor is gcc's process startup. Options
  not taken here: an interpreter for the first run (another part of v0.6), `tcc` (`NYRA_CC=tcc` works
  in principle; it is not installed here, so it was not measured and nothing selects it by default), or
  emitting only the runtime functions a program uses so that -O0 gets cheap (section 1).

## 3. `s = s + x`

`s += x` already appended in place (amortized doubling) while the string has one owner. `s = s + x`,
which is what a model writes more often, lowered to `t = concat(s, x); drop s; s = t`: a new string
and a full copy in every round, so a loop was quadratic. The IR pass `append_in_place`
(`src/ir/opt.rs`) turns the pattern into the append when nothing between the concat and the
assignment touches `s` or the partial results (a chain like `s = s + a + b` becomes two appends; an
operand that reads `s`, as in `s = s + s` or `s = s + f(s)`, is left alone). The same for
`xs = xs + ys`. Because it is an IR pass, every backend gets it:

| program (see below) | before | after |
|---|---|---|
| look-and-say written with `out = out + str(n) + str(c)`, native, run time | 4431 ms | 109 ms |
| 200,000 x `s = s + "ab"`, 100,000 x `t = t + str(i) + ","`, 100,000 x `u = u + "{i}-"`, native, run time | 8504 ms | 125 ms |
| the same, `--js` (whole command) | 250 ms | 579 ms (node start and noise: V8 ropes were never slow) |
| the same, `--py` | 9.3 s | 1.4 s |
| the same, `--rs` (whole command, rustc 2 s) | 49 s | 2.3 s |
| look-and-say, `--py` | 3.9 s | 2.2 s |

Output is identical (`examples/append_in_place.nyra` runs on node, native and with `NYRA_OPT=0`;
`NYRA_LEAKCHECK=1` passes). Go strings are immutable and `s += x` there copies the string each time;
this cannot be changed without a Go toolchain to test with, so the Go target gets a warning (E0362).

## 4. Warnings

`src/perfwarn.rs`: E0360 (search in a long loop over an array the function builds), E0361 (text put in
front of a string in a loop), E0362 (appending to a string in a long loop, Go only). They are
`diag::warn`ings: stderr lines, `"warnings"` in `--json`, never an error. Each rule fires only when the
pattern is almost certainly slow: a counted loop of 1000 rounds or more (or `while i < 100000`), an
array that is a local `push`ed variable and whose length is not compared with a small number, never
in loops of fewer than 100 rounds. On the 83 benchmark solutions, the 100 examples and the six perf
programs, the default target gives exactly one warning (E0361 on `reverse_string`, the canonical
case); the Go target gives E0362 on three programs.

## 5. Checked integers

See `perf/README.md`. The cold error path of an overflow (and of an index check) took five arguments;
now three, one of them the packed position. Recursion that gcc had stopped optimizing is
optimized again (a `fib` that takes its argument from the command line takes 16 ms against 531 ms in C
and 1406 ms before; what gcc does to it was not investigated); loops that do not recurse (`dp`, `sieve`, `structs`) lose at most 15% to the remaining checks.
