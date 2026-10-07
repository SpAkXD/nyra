# Nyra v0.1 — language spec

This file is the whole language. It is short on purpose: paste it into an AI agent's
context and the agent can write Nyra.

## Rules
- One way to do each thing. No implicit conversions. No shadowing. No null.
- Every function signature is fully typed. Local variable types are inferred.
- One statement per line. There are no semicolons. Newlines inside `( )` are ignored.
- Comments: `// to end of line`.

## Types
`int` (64-bit signed), `float` (64-bit), `bool`, `str` (immutable text).

## Functions
```
fn add(a: int, b: int) -> int {
    ret a + b
}
fn greet() {            // no `->` means it returns nothing
    print("hi")
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
```

## Control flow
```
if x > 3 { ... } else if x == 3 { ... } else { ... }
while n < 10 { n = n + 1 }
for i in 0..10 { print(i) }   // 0 to 9; `i` is an immutable int
ret value                      // return
```
Conditions must be `bool` (write `x != 0`, not `x`).

## Operators (high to low precedence)
| ops | types |
|---|---|
| `-x` `!x` | `-` on int/float, `!` on bool |
| `*` `/` `%` | int,int or float,float (`%` int only; int `/` truncates) |
| `+` `-` | int,int or float,float |
| `<` `<=` `>` `>=` | int,int or float,float → bool |
| `==` `!=` | same type on both sides → bool (strings compare by value) |
| `&&` | bool |
| `\|\|` | bool |

## Builtins
| call | meaning |
|---|---|
| `print(x)` | print any value and a newline |
| `int(x)` | float → int (truncates toward zero) |
| `float(x)` | int → float |

## Strings
`"text"` with escapes `\n \t \r \\ \"`. Strings can be compared with `==` / `!=`.
Concatenation is not supported yet.

## Errors
`nyra check file.nyra --json` prints:
```json
{"ok":false,"errors":[{"code":"E0201","message":"undefined variable `cout`","file":"a.nyra","line":4,"col":11,"hint":"did you mean `count`?"}]}
```
| code | meaning |
|---|---|
| E0001–E0005 | lexer: bad character, unterminated string, number too large, bad escape, semicolon |
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

## Known differences between backends (v0.1)
- `int` overflow wraps in C; in JS, values above 2^53 lose precision.
