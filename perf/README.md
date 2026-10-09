# perf: native Nyra against C, Rust, Go, Node.js and Python

Six small programs (`dp`, `fib`, `sieve`, `sort`, `strings`, `structs`) written once in Nyra and once
in each of the other languages, the same algorithm and the same output:

| folder | language | built or run with |
|---|---|---|
| `perf/*.nyra` | Nyra | `nyra build` (native, `-O2`) |
| `perf/c/` | C | the C compiler Nyra uses, with Nyra's flags (`-O2 -fwrapv -ffp-contract=off`) |
| `perf/rs/` | Rust | `rustc -C opt-level=3` |
| `perf/go/` | Go | `go build` (skipped when Go is not installed) |
| `perf/js/` | JavaScript | `node` |
| `perf/py/` | Python | `python` (run once: it is slow) |

```
cargo build --release
python perf/run.py                  # every program, best of 5 runs
python perf/run.py sieve fib -n 3   # some of them
python perf/run.py --json out.json  # the timings as JSON too
python perf/first_output.py         # time to the first output of `nyra run`, cold and warm, -O2 and -O1
```

The runner checks that every language prints the same output, then times the whole process (start
included). The hand-written C and Rust do not check array indexes or integer overflow; Nyra does both
(E0240, E0255), so a ratio near 1 means the checks cost little.

## Numbers

Windows 11, gcc 15.2 (UCRT64), rustc stable, Node 25, Python 3.14; best of 5 wall-clock runs
(Python: one run), on a laptop that other jobs shared, so differences under about 15% are noise.
Go is not installed on this machine: `perf/go/*.go` are written and unchecked here.

| program | nyra | c | rust | node | python | nyra / c | nyra / rust | nyra / node | nyra / python |
|---|---|---|---|---|---|---|---|---|---|
| dp | 0.186 s | 0.147 s | 0.127 s | 0.321 s | 12.831 s | 1.27x | 1.47x | 0.58x | 0.01x |
| fib | 0.014 s | 0.138 s | 0.118 s | 0.622 s | 9.664 s | 0.10x | 0.12x | 0.02x | 0.00x |
| sieve | 0.308 s | 0.317 s | 0.260 s | 0.543 s | 1.455 s | 0.97x | 1.19x | 0.57x | 0.21x |
| sort | 0.325 s | 0.353 s | 0.227 s | 0.631 s | 6.716 s | 0.92x | 1.43x | 0.51x | 0.05x |
| strings | 0.780 s | 0.743 s | 0.609 s | 1.643 s | 2.814 s | 1.05x | 1.28x | 0.47x | 0.28x |
| structs | 0.085 s | 0.051 s | 0.054 s | 0.240 s | 22.026 s | 1.67x | 1.57x | 0.35x | 0.00x |

Reading it: native Nyra is within a small factor of C and Rust (the difference is mostly the index and
overflow checks), about twice as fast as Node.js and 5 to 100 times as fast as Python.

`fib` needs a note. Since the overflow check's error path takes three arguments (see below), gcc
optimizes the recursion so much that the program runs in a few milliseconds (what exactly it does to it
was not investigated): that says something about gcc, not about the language. A version of the same
recursion that cannot be folded at compile time (the starting number is a program argument) takes
531 ms in C, 1406 ms with the old five-argument error path and 16 ms with the new one.

## What the checked integers cost

Since v0.6 `int` overflow is a runtime error (E0255), so `a + b` is an add with an overflow test.
The test is `__builtin_add_overflow`, and its error path is a cold function that stops the program.
That function took five arguments (both operands, the operator as text, line and column), which made
the C compiler give up inlining and unrolling around every checked operation: recursion like `fib`
ran 2.65 times slower than the same C without checks. It takes three now: both operands and one
packed number (line, column and operator). The array index check got the same treatment.

Measured on the C that Nyra generates, with all overflow checks compiled out against with them (CPU
time, best of 15):

| program | with checks | checks removed |
|---|---|---|
| dp | 156 ms | 156 ms |
| structs | 62 ms | 47 ms |
| sieve | 344 ms | 328 ms |

So what is left of the overflow checks is about 0 to 15%. Range analysis (`src/ir/lower/bounds.rs`)
already removes the checks it can prove: loop counters, lengths, values below a constant `%`, and
indexes that were just checked. What remains is arithmetic on values that come from arrays and struct
fields, which no analysis of the program text can bound.

## Time to the first output of `nyra run`

`nyra run` compiles with `-O1`, `nyra run --release` and `nyra build` with `-O2`. Wall-clock
milliseconds of the whole command, best of 5, for programs of `bench/solutions/nyra`. Cold: nothing
cached, so the C compiler runs. Warm: the executable is cached.

| program | cold, -O2 (before) | cold, -O1 (now) | warm, -O2 | warm, -O1 | python | node (--js) |
|---|---|---|---|---|---|---|
| fizzbuzz | 550 | 505 | 36 | 36 | 245 | 100 |
| fib_recursive | 494 | 516 | 38 | 38 | 268 | 103 |
| word_frequency | 1399 | 900 | 42 | 41 | 255 | 100 |
| look_and_say | 991 | 923 | 74 | 248 | 1844 | 780 |
| big_sieve | 1731 | 1126 | 193 | 92 | 1825 | 481 |
| text_adventure | 1668 | 1089 | 42 | 39 | 261 | 108 |
| spreadsheet_eval | 1894 | 1177 | 45 | 48 | 279 | 122 |

Small programs are bound by starting gcc (five processes, about 0.5 s on this machine, which other jobs
were loading: Python needed 245 ms to start); larger ones compile 30 to 40% faster at -O1. The method,
the compile and run times at -O0, -Og, -O1 and -O2, and the other changes are in
[research/SPEED-run.md](../research/SPEED-run.md).
