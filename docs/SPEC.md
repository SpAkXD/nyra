# Nyra v0.2 — language spec

This file is the whole language. It is short on purpose: paste it into an AI agent's
context and the agent can write Nyra. For common mistakes and complete examples, see
[AI_GUIDE.md](AI_GUIDE.md).

## Rules
- One way to do each thing. No implicit conversions. No shadowing. No null.
- Every function signature is fully typed. Local variable types are inferred.
- One statement per line. There are no semicolons. Newlines inside `( )` are ignored.
- Everything is evaluated left to right: arguments, operands and the parts of a string.
- Comments: `// to end of line`.

## Types
`int` (64-bit signed), `float` (64-bit), `bool`, `str` (immutable text).

## Functions
```
fn add(a: int, b: int) -> int {
    ret a + b
}
fn square(x: int) -> int = x * x      // one-line function: the expression is returned
fn greet(name: str) = print("hi {name}")
fn log() {                            // no `->` means it returns nothing
    print("done")
}
```
Every program needs `fn main()` with no parameters and no return type.
A function with a return type must `ret` on every path.

## Variables
```
let x = 5               // immutable
let y: float = 2.5      // optional type annotation
var n = 0               // mutable
n = n + 1               // only `var` can be reassigned
n += 1                  // also -= *= /= %=
```

## Control flow
```
if x > 3 { ... } else if x == 3 { ... } else { ... }
while n < 10 { n += 1 }
for i in 0..10 { print(i) }   // 0 to 9; `i` is an immutable int
ret value                      // return
```
Conditions must be `bool` (write `x != 0`, not `x`).

`if` can also be a value. It needs an `else`, and each branch is one expression of the same type:
```
let max = if a > b { a } else { b }
fn sign(x: int) -> str = if x > 0 { "+" } else if x < 0 { "-" } else { "0" }
```

## Operators (high to low precedence)
| ops | types |
|---|---|
| `-x` `!x` | `-` on int/float, `!` on bool |
| `*` `/` `%` | int,int or float,float (`%` int only; int `/` truncates; int `/` or `%` by zero is runtime error E0241) |
| `+` `-` | int,int or float,float |
| `<` `<=` `>` `>=` | int,int or float,float → bool |
| `==` `!=` | same type on both sides → bool (strings compare by value) |
| `&&` | bool |
| `\|\|` | bool |

## Builtins
| call | meaning |
|---|---|
| `print(x)` | print any value and a newline |
| `int(x)` | float → int (truncates toward zero; NaN or a value outside the int range is runtime error E0245) |
| `float(x)` | int → float |

## Strings
`"text"` with escapes `\n \t \r \\ \"`. Compare with `==` / `!=`.

Put any expression inside `{ }` to insert its value (ints, floats, bools, strings):
```
print("{name} is {age} years old, adult: {age >= 18}")
let label = "v{major}.{minor + 1}"
```
Write `{{` and `}}` for literal braces. Quotes are not allowed inside `{ }`:
store the text in a variable first.

## Errors
Every code is explained, with the reason for the rule and a wrong and a fixed program, in
[ERRORS.md](ERRORS.md); `nyra explain E0201` prints an entry (`--json` for JSON, no code lists them all).
`nyra check file.nyra --json` prints:
```json
{"ok":false,"errors":[{"code":"E0201","message":"undefined variable `cout`","file":"a.nyra","line":4,"col":11,"hint":"did you mean `count`?"}]}
```
| code | meaning |
|---|---|
| E0001–E0006 | lexer: bad character, unterminated string, number too large, bad escape, semicolon, bad `{`/`}` in a string |
| E0101 | unexpected token |
| E0102 | unknown type |
| E0201 | undefined variable |
| E0202 | undefined function |
| E0203 | type mismatch |
| E0204 | wrong number of arguments |
| E0205 | assignment to a `let` variable |
| E0206 | name already defined |
| E0207 | bad or missing `ret` |
| E0208 | missing `fn main()` |
| E0209 | condition is not `bool` |
| E0210 | operator used on wrong types |
| E0211 | `main` has parameters or a return type |
| E0212 | `if` used as a value: missing `else`, or branches that aren't one value of the same type |

## Runtime errors
Some mistakes only show up while the program runs. It then stops with exit code 101 and,
after all earlier output, prints:
```
runtime error[E0241]: division by zero
  --> main.nyra:4:13
  = hint: check the divisor first
```
`nyra run --json` prints the same error as JSON (with `"runtime":true`).

| code | meaning |
|---|---|
| E0241 | integer `/` or `%` by zero |
| E0245 | `int(x)` of NaN, infinity or a float outside the int range |

## Numbers
Numbers print the same way everywhere: like JavaScript's `String(x)`
(`3.0` prints `3`, `0.1 + 0.2` prints `0.30000000000000004`, `1.0 / 0.0` prints `Infinity`, never `-0`).

## Known differences between backends
- `int` overflow wraps in C; in JS, values above 2^53 lose precision.
- Strings built with `{ }` are not freed before the program exits yet (the v0.3 memory model fixes this).
