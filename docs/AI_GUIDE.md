# Nyra: guide for AI agents

How to write correct **Nyra v0.2** programs. Read it once, top to bottom; it is short on purpose.

Nyra is **not in your training data** and it is **not** Rust, Go, TypeScript or Python, even though
the tokens look familiar (code blocks here are marked `rust` only so GitHub highlights them). Use only
what is shown here or in the [language spec](https://raw.githubusercontent.com/SpAkXD/nyra/main/docs/SPEC.md).
If a feature is not described, it does not exist yet (section 4).

## 1. Workflow

```
nyra check prog.nyra --json    # compile only; prints {"ok":true,"errors":[]} when clean
nyra run prog.nyra             # compile and run natively (needs gcc, clang or tcc)
nyra run prog.nyra --js        # or run on Node.js
```

1. Write the program to `prog.nyra`.
2. Run `nyra check prog.nyra --json`. If `"ok"` is `false`, fix **every** entry of `errors`, then check
   again. Each error has a stable `code`, a `message`, `line`, `col` and usually a `hint`:
   ```json
   {"ok":false,"errors":[{"code":"E0201","message":"undefined variable `cout`","file":"a.nyra","line":4,"col":11,"hint":"did you mean `count`?"}]}
   ```
3. Run it and compare the output with what you expect.

Exit codes: `0` ok, `1` compile errors, `2` usage or tool problem (for example no C compiler: use `--js`),
`101` runtime error (see the bottom of section 5). `nyra run prog.nyra --json` reports runtime errors as JSON too.

**If you cannot run commands** (you are answering in a chat): follow the rules below, go through the
checklist in section 8, and give the user the code, the output you expect, and the command to run it
(`nyra run prog.nyra`). Binaries: <https://github.com/SpAkXD/nyra/releases/latest>. `nyra --version`
must be 0.2 or newer, or the `=` functions, `+=`, interpolation and `if` values below will not compile.

## 2. The language at a glance

```rust
// A comment. Files end in .nyra. No semicolons anywhere.

fn add(a: int, b: int) -> int = a + b         // one-line function: the expression is the result
fn shout(msg: str) = print("{msg}!")          // no `->`: returns nothing
fn limit() -> int = 100                       // constants are functions: there are no globals

fn gcd(a: int, b: int) -> int {               // block body: return with `ret`
    if b == 0 { ret a }
    ret gcd(b, a % b)
}

fn main() {                                   // required; no parameters, no return type
    let x = 7                                 // immutable, type inferred (int)
    var total = 0                             // mutable
    let ratio: float = 2.5                    // type annotation is optional
    total += x                                // also -= *= /= %=  (var only)
    total = total * 2                         // plain reassignment (var only)

    if total > 20 && x != 0 {                 // the condition must be a bool
        print("big")
    } else if total == 20 {
        print("exact")
    } else {
        print("small")
    }
    while total > 0 { total -= 5 }
    for i in 0..3 { print(i) }                // 0, 1, 2: the end is exclusive
    let bigger = if x > 3 { x } else { 3 }    // `if` as a value (needs `else`)

    print("x = {x}, sum = {add(x, 2)}")       // {expression} interpolation
    print("big: {x > 3}, {{braces}}")         // bool, float, str work too; {{ }} = literal braces
    print(float(x) / ratio)                   // no implicit conversions: float(...) and int(...)
    print(7 / 2)                              // 3: int division truncates
    shout("done")
}
```

Types: `int` (64-bit), `float` (64-bit), `bool`, `str`. Function signatures are always fully typed;
local types are inferred. The only builtins are `print(x)`, `int(x)` and `float(x)`.

## 3. Rules: do and don't

| Rule | Don't (error) | Do |
|---|---|---|
| Only `fn` at top level | `let max = 10` at top level (E0101) | `fn max() -> int = 10` |
| No semicolons | `let x = 1;` (E0005) | `let x = 1` |
| One statement per line | `let a = 1 let b = 2` (E0101) | two lines |
| `{` on the same line | `fn main()` newline `{` (E0101) | `fn main() {` |
| Braces are required | `if x > 0 print(x)` (E0101) | `if x > 0 { print(x) }` |
| Typed signatures | `fn f(a) -> int` (E0101), `a: string` (E0102) | `fn f(a: int) -> int` |
| No implicit conversion | `1 + 2.0` (E0210), `let f: float = 2` (E0203) | `float(1) + 2.0`, `let f = 2.0` |
| Only `var` changes | `let n = 0` then `n = 1` (E0205) | `var n = 0` then `n = 1` |
| Parameters are immutable | `n = n / 2` on a parameter (E0205) | `var m = n`, then change `m` |
| No shadowing | `let x = 1` ... `let x = 2` in one function (E0206) | a new name, or `var` and reassign |
| Unique names | `let print = 1`, a variable named like a function (E0206) | another name |
| Bool conditions | `if n {`, `while n {` (E0209) | `if n != 0 {` |
| Return with `ret` | `return x` (E0101) | `ret x` |
| Every path returns | `if x > 0 { ret 1 }` as the last statement (E0207) | add `ret 0` after it, or `else { ret 0 }` |
| `main` takes nothing | `fn main() -> int` (E0211), no `main` (E0208) | `fn main() { ... }` |
| Ranges are exclusive | `0..=9` (E0101), `0.0..1.0` (E0203) | `0..10` with int bounds |
| No `break` / `continue` | `break` (E0201) | `ret` from a helper, or a `bool` flag in the `while` condition |
| `print` takes one value | `print(a, b)` (E0204) | `print("{a} {b}")` |
| No string `+` | `"a" + b` (E0210) | `"a{b}"` |
| Braces in strings | `print("{")` (E0006) | `print("{{")` |
| Comments | `# note`, `/* note */` (E0001, E0101) | `// note` |
| Literals | `.5`, `5.`, `1e5`, `1_000`, `0xFF`, `'a'` (E0001, E0101) | `0.5`, `5.0`, `100000.0`, `1000`, `255`, `"a"` |
| Operators | `and`, `or`, `i++`, `2 ** 3`, `a < b < c` (E0101, E0210) | `&&`, `i += 1`, `2 * 2 * 2`, `a < b && b < c` |

Good to know:

- Operators, high to low: `-x` `!x` · `*` `/` `%` · `+` `-` · `<` `<=` `>` `>=` · `==` `!=` · `&&` · `||`.
  `%` is for ints only. `==` and `!=` need the same type on both sides and compare `str` by value.
  `<` and `>` work on `int` and `float` only.
- Int division truncates toward zero: `7 / 2` is `3`, `-7 / 2` is `-3`, `-7 % 3` is `-1`.
- `print` accepts an `int`, `float`, `bool` or `str`. Floats print in the shortest form that reads back
  exactly, and whole floats have no `.0`: `print(2.0)` prints `2`, `print(0.1 + 0.2)` prints
  `0.30000000000000004`.
- Float math shows its rounding noise: multiplying 1000.0 by 1.05 four times prints
  `1215.5062500000001`. When you state the expected output of float math, call it approximate.
- Ints are 64-bit and wrap on overflow natively; with `--js` they are exact only up to 2^53
  (9007199254740991). Stay below that and both backends print the same.
- A name declared inside `{ }` is gone after the closing brace (you may reuse it then). Functions can
  be defined in any order and call each other. `ret` with no value leaves a function that returns nothing.
- An `if` used as a value needs an `else`, one expression per branch, and the same type in both
  branches. `else if` chains work and the branches may span lines. A one-line function's expression
  starts on the same line as its `=`.
- Text built with `{ }` and stored in a variable (`row = "{row}*"`) is not freed yet, so do not build
  millions of strings in a loop. `print("... {x}")` allocates nothing and is always fine.
- An `int` `/` or `%` by zero stops the program with runtime error E0241, and `int(x)` of NaN or
  infinity stops it with E0245 (exit code 101, same on both backends). Float division by zero is
  fine and prints the same everywhere: `1.0 / 0.0` is `Infinity`, `0.0 / 0.0` is `NaN`.
- Newlines inside `( )` are ignored and trailing commas are fine. You may break a line after a
  binary operator, never before it.
- Style: 4 spaces, `snake_case`, short functions, `//` comments that say why.

## 4. What does not exist (yet)

Do not use any of these. If the task needs one, say so in a sentence and write the closest program
that works (for example, sort three numbers with `min`/`max` instead of sorting a list).

- **Input of any kind**: no stdin, arguments, files, network, clock or random numbers. Programs are
  closed, so put the test values in the program (`let n = 12`).
- **Collections and types**: no arrays, lists, maps, tuples, structs, enums, methods or `.len()`.
- **String operations**: no `+`, indexing, slicing, length, split, case changes or characters. You can
  compare with `==` / `!=` and build text with interpolation.
- **Control flow**: no `break`, `continue`, `loop`, `do-while`, `match`, `switch`, `?:` or `elif`.
- **Declarations**: no global variables, nested functions, closures, generics, overloading, default
  arguments, imports or modules. One file is one program.
- **Library**: no `abs`, `min`, `max`, `pow`, `sqrt`, `floor`, `len`... Write them yourself (section 6).
- **Errors**: no exceptions, `null`, `Option`, `Result`, `assert`, `panic` or `exit`.
- **Output**: `print` always ends the line. There is no `printf` and no format specifier (`{x:.2f}` is
  an error): a float always prints in its shortest form, so `12.5` is never shown as `12.50`.

## 5. Error codes

| Code | Meaning | Usual cause and fix |
|---|---|---|
| E0001 | unexpected character | `#`, `'`, `?`, `[`, `.5`, `5.`, single `&`: use `//`, `"`, `if` / `else`, `0.5` |
| E0002 | unterminated string | strings end on the same line: close with `"`, use `\n` for line breaks |
| E0003 | number too large | `int` max is 9223372036854775807 |
| E0004 | unknown escape | only `\n` `\t` `\r` `\\` `\"` exist |
| E0005 | semicolon | delete it |
| E0006 | bad brace in a string | `{` starts an interpolation: a lone `{` or `}`, an empty `{}` or quotes inside `{ }` are errors. Write `{{` and `}}` for literal braces |
| E0101 | syntax error | `return`, `elif`, `i++`, `0..=n`, `{` on a new line, two statements on a line, a missing `=` or type: compare with section 2 |
| E0102 | unknown type | only `int`, `float`, `bool`, `str` (not `string`, `i32`, `double`, `char`) |
| E0201 | undefined variable | typo (see `hint`), used before its `let`, declared in another block, or `break` / `continue` |
| E0202 | undefined function | not a builtin (`len`, `abs`, `sqrt`, `max`...): define it yourself |
| E0203 | type mismatch | wrong argument, return or assigned type: `float(x)` / `int(x)`, write `2.0` not `2` |
| E0204 | wrong argument count | `print` takes exactly one argument |
| E0205 | assignment to `let` | `let` variables, parameters and loop variables are immutable: use `var` |
| E0206 | name already defined | no shadowing, no duplicate functions, no variable named like a function or `print` / `int` / `float` |
| E0207 | bad or missing `ret` | end every path of a `->` function with `ret value`; no `ret value` without `->` |
| E0208 | no `fn main()` | add `fn main() { ... }` |
| E0209 | condition is not `bool` | compare: `if n != 0` |
| E0210 | operator on wrong types | `int + float`, `str + str`, `!int`, `5.0 % 2.0`, `"a" < "b"`, `a < b < c` |
| E0211 | bad `main` | no parameters and no return type |
| E0212 | bad `if` value | an `if` used as a value needs an `else`, exactly one expression per branch, and the same type in both |

**Runtime errors** stop a running program with exit code 101, after everything it printed so far:

```
runtime error[E0241]: division by zero
  --> prog.nyra:4:13
  = hint: check the divisor first
```

| Code | Meaning | Fix |
|---|---|---|
| E0241 | integer `/` or `%` by zero | check the divisor first (`if d != 0 { ... }`) |
| E0245 | `int(x)` of NaN, infinity or a float too big for an `int` | check the value before converting |

## 6. Recipes

Nyra has no standard library yet, so write small helpers yourself:

```rust
fn abs(x: int) -> int = if x < 0 { -x } else { x }
fn min(a: int, b: int) -> int = if a < b { a } else { b }
fn max(a: int, b: int) -> int = if a > b { a } else { b }
fn is_even(n: int) -> bool = n % 2 == 0

fn pow(base: int, exp: int) -> int {
    var result = 1
    for i in 0..exp { result *= base }
    ret result
}

fn sqrt(x: float) -> float {                   // Newton's method; x must be > 0.0
    var g = x / 2.0
    for i in 0..20 { g = (g + x / g) / 2.0 }
    ret g
}
```

Leave a loop early (there is no `break`): `ret` leaves the loop and the function.

```rust
fn first_divisor(n: int) -> int {              // smallest divisor above 1
    for d in 2..n {
        if n % d == 0 { ret d }
    }
    ret n
}
```

Count down, and build a line of text with interpolation (strings cannot be added):

```rust
fn main() {
    var i = 3
    while i > 0 {
        print(i)                                // 3, 2, 1
        i -= 1
    }

    var row = ""
    for j in 0..5 { row = "{row}*" }
    print(row)                                  // *****
}
```

## 7. Complete programs

FizzBuzz (`for`, `else if`, `%`). Prints `1`, `2`, `Fizz`, `4`, `Buzz`, `Fizz`, `7`, `8`, `Fizz`, `Buzz`,
`11`, `Fizz`, `13`, `14`, `FizzBuzz`, one per line:

```rust
fn main() {
    for i in 1..16 {
        if i % 15 == 0 { print("FizzBuzz") }
        else if i % 3 == 0 { print("Fizz") }
        else if i % 5 == 0 { print("Buzz") }
        else { print(i) }
    }
}
```

Primes (`while`, early `ret`, `+=`, interpolation). Prints `2 is prime` ... `29 is prime`, then
`10 primes below 30`:

```rust
fn is_prime(n: int) -> bool {
    if n < 2 { ret false }
    var d = 2
    while d * d <= n {
        if n % d == 0 { ret false }
        d += 1
    }
    ret true
}

fn main() {
    var count = 0
    for n in 2..30 {
        if is_prime(n) {
            print("{n} is prime")
            count += 1
        }
    }
    print("{count} primes below 30")
}
```

Leap years (one-line functions, `if` as a value). Prints `1999: 365 days`, `2000: 366 days`,
`2001: 365 days`, `2002: 365 days`, `2003: 365 days`, `2004: 366 days`:

```rust
fn is_leap(y: int) -> bool = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)
fn days_in(y: int) -> int = if is_leap(y) { 366 } else { 365 }

fn main() {
    for y in 1999..2005 {
        print("{y}: {days_in(y)} days")
    }
}
```

Averages (explicit `int` to `float` conversion). Prints `sum of squares: 385`, then `average square: 38.5`:

```rust
fn average(total: int, count: int) -> float = float(total) / float(count)

fn main() {
    var total = 0
    for n in 1..11 {
        total += n * n
    }
    print("sum of squares: {total}")
    print("average square: {average(total, 10)}")
}
```

More programs with expected output live in
[`examples/`](https://github.com/SpAkXD/nyra/tree/main/examples): `gcd`, `collatz`, `perfect`, `digits`,
`primes`, `fib`, `math`, `short_fns`, `interpolation`, `if_value`.

## 8. Checklist before you answer

1. `fn main()` exists, and only `fn` definitions are at the top level.
2. No `;`, `return`, `break`, `continue`, `elif`, `++`, `0..=n`, string `+`, or Allman-style `{` on its own line.
3. Every name is unique inside its function (parameters, loop variables and locals), and no variable
   shares a name with a function or with `print` / `int` / `float`.
4. Both sides of every operator have the same type; floats are written with a dot (`2.0`);
   conversions are explicit (`float(n)`, `int(x)`).
5. Every `if` / `while` condition is a comparison or a `bool`.
6. Every `->` function ends with `ret` on all paths (a one-line `=` function needs none).
7. Only `print`, `int`, `float` and your own functions are called; there is no input.
8. Literal braces in strings are doubled (`{{` `}}`), and nothing inside `{ }` contains quotes.
9. You stated the output the program should print, and how to run it: `nyra run prog.nyra`.
