# Nyra error database

Every error the Nyra compiler can report, one entry per code: what it means, why the rule exists, the usual
causes, a wrong program that produces the code and a fixed one. Read an entry with

```
nyra explain E0201            # the entry, for humans
nyra explain E0201 --json     # the same entry as JSON, for tools and AI agents
nyra explain                  # every code with its title
```

Errors from `nyra check file.nyra --json` carry the code (`"code":"E0201"`) and a `hint` that usually contains the
fix already; this file explains the rule behind it. The programs under **Wrong** and **Fixed** are tested: for every
code the compiler can emit, the wrong program produces exactly that code and the fixed program compiles and runs.
Codes marked *planned* are described in the design for a future version; the compiler does not emit them yet and the
design may still change.

## Code ranges

| Range | Stage | What goes wrong |
|---|---|---|
| E0001-E0006 | lexer | characters, numbers and strings |
| E0007 | lexer | character literals |
| E0101-E0102 | parser | grammar and type names |
| E0201-E0212 | type checker | names, types, `ret`, conditions |
| E0220-E0239 | type checker (planned, v0.3) | structs, arrays, strings, methods, `inout`, `free` / `keep` / `arena` |
| E0240-E0249 | run time | the program stops with exit code 101 (E0241 and E0245 exist; E0240, E0242-E0244, E0246 and E0249 are planned) |
| E0300-E0316 | modules and FFI (planned, v0.6) | `use`, `pub`, `extern`, targets |
| E0320-E0325 | packages (planned, v0.6) | `nyra.toml`, dependencies, `nyra.lock` |
| E0330-E0332 | declarations (planned, v0.6) | `never`, `const`, `pub` |
| E0340-E0344 | run time (planned, v0.6) | standard library and foreign function failures |

Codes are stable: a number is never reused for another error. Numbers that are not listed (E0247-E0248, E0309,
E0317-E0319, E0326-E0329, E0333-E0339, E0345-E0349) are kept free for future errors of the same kind. E0900-E0919 are set aside
for the intermediate representation and the WebAssembly backend (v0.5), which needs no codes of its own so far.

## Entry format

Each entry is a heading `## E0xxx: title` followed by these fields, in this order. The test suite checks the format, and
`nyra explain` reads it, so keep it exactly like this when you add a code (see CONTRIBUTING.md).

```text
## E0201: undefined variable
- **Kind:** compile error · **Since:** v0.1            (runtime error · compile error; or "planned for v0.6, not in the compiler yet")
- **What it means:** one or two precise sentences.
- **Why Nyra has this rule:** the design reason.
- **Common causes:**
  - one cause per line
- **Wrong:**
(a fenced rust block: for a code in the compiler it really produces this code)
- **Fixed:**
(a fenced rust block: it compiles and runs)
- **Related:** E0206, E0202                              (other codes that are easily confused with this one)
```

## E0001: unexpected character
- **Kind:** compile error · **Since:** v0.1
- **What it means:** The source contains a character that cannot start any Nyra token. Nyra's alphabet is small: letters, digits, `_`, strings in double quotes, `//` comments and the symbols `( ) { } , : + - * / % = < > !` plus the two-character forms `-> .. == != <= >= && || += -= *= /= %=`. A `.` is only valid inside a float (`2.5`) or in a range (`0..10`), and `&` and `|` only in pairs.
- **Why Nyra has this rule:** A closed alphabet keeps every program unambiguous. Characters that mean something in other languages (`#`, `?`, `[`, `'`) are not silently ignored or reinterpreted: you get a precise error at the exact position.
- **Common causes:**
  - a `#` comment: Nyra comments start with `//`
  - single quotes (`'a'`), backticks or typographic quotes (“ ” ‘ ’): text uses straight double quotes
  - `?` and `:` as a ternary: write `if cond { a } else { b }` as a value
  - `[` and `]`: arrays and indexing do not exist yet
  - a single `&` or `|`: write `&&` or `||`
  - `.5` or `5.`: a float needs digits on both sides of the dot (`0.5`, `5.0`)
  - a `.` after a name (`s.len()`, `console.log(x)`): there are no methods or fields
  - `$`, `@`, `^`, `~` or a backslash outside a string
  - characters pasted from documents: `×`, `÷`, `≤`, `≥`, `≠`, invisible characters and a byte order mark at the start of the file
- **Wrong:**
```rust
fn main() {
    # say hello
    print("hello")
}
```
- **Fixed:**
```rust
fn main() {
    // say hello
    print("hello")
}
```
- **Related:** E0005, E0002, E0101

## E0002: unterminated string
- **Kind:** compile error · **Since:** v0.1
- **What it means:** A string that starts with `"` is not closed with `"` before the end of the line. A Nyra string always sits on one line.
- **Why Nyra has this rule:** If strings could continue on the next line, one forgotten quote would swallow the rest of the file and the error would be reported far from the mistake. With single-line strings the position of the error is the position of the opening quote.
- **Common causes:**
  - the closing `"` is missing
  - the text is meant to span several lines: write `\n` for a line break inside one string
  - the line ends with a backslash, which escapes the line break instead of closing the string
- **Wrong:**
```rust
fn main() {
    print("hello)
}
```
- **Fixed:**
```rust
fn main() {
    print("hello")
}
```
- **Related:** E0004, E0006

## E0003: number too large for `int`
- **Kind:** compile error · **Since:** v0.1
- **What it means:** An integer literal does not fit in `int`, a 64-bit signed integer whose largest value is 9223372036854775807. The smallest `int` cannot be written as a literal at all, because the minus sign is a separate operator: write `-9223372036854775807 - 1`.
- **Why Nyra has this rule:** `int` is exactly 64 bits and wraps on overflow. A literal that silently wrapped or lost digits would be a wrong program with no error.
- **Common causes:**
  - a very large constant such as a factorial, `2^64` or an ID number pasted from elsewhere
  - a value that should be a float: write it with a dot (`99999999999999999999.0`)
  - `-9223372036854775808`, the smallest `int`, which has no literal form
- **Wrong:**
```rust
fn main() {
    let big = 99999999999999999999
    print(big)
}
```
- **Fixed:**
```rust
fn main() {
    let big = 99999999999999999999.0
    print(big > 1.0)
}
```
- **Related:** E0203, E0245

## E0004: unknown escape
- **Kind:** compile error · **Since:** v0.1
- **What it means:** A backslash inside a string is followed by a character that is not an escape. The escapes are `\n` (new line), `\t` (tab), `\r` (carriage return), `\\` (a backslash) and `\"` (a quote).
- **Why Nyra has this rule:** A closed set of escapes means a backslash never silently disappears: `"C:\Users"` would otherwise print `C:Users`. Strings are UTF-8, so any character can be typed directly instead of using `\u` or `\x` codes.
- **Common causes:**
  - a Windows path: write every backslash twice (`"C:\\Users"`)
  - `\u00e9` or `\x41`: type the character itself, as in `"é"`
  - `\{` or `\}`: braces are doubled instead (`{{`, `}}`), see E0006
  - `\'`: a single quote needs no escape
- **Wrong:**
```rust
fn main() {
    print("C:\Users\nyra")
}
```
- **Fixed:**
```rust
fn main() {
    print("C:\\Users\\nyra")
}
```
- **Related:** E0006, E0002

## E0005: semicolon
- **Kind:** compile error · **Since:** v0.1
- **What it means:** The source contains a `;`. Nyra has no semicolons: the end of a line ends a statement.
- **Why Nyra has this rule:** There is exactly one way to end a statement. Nothing can be forgotten or added by accident, and the grammar has one token less.
- **Common causes:**
  - the habit from C, Java, JavaScript or Rust: delete the `;`
  - a C-style `for (init; cond; step)` loop: write `for i in 0..10 { }` or a `while`
  - two statements on one line separated by `;`: put them on two lines
- **Wrong:**
```rust
fn main() {
    let x = 1;
    print(x)
}
```
- **Fixed:**
```rust
fn main() {
    let x = 1
    print(x)
}
```
- **Related:** E0001, E0101

## E0006: bad `{` or `}` in a string
- **Kind:** compile error · **Since:** v0.2
- **What it means:** Inside a string, `{` starts an inserted value (`"{x}"`) and `}` ends it. The error is reported for a `}` with no `{`, a `{` that is never closed, an empty `{}`, or a quote inside the braces.
- **Why Nyra has this rule:** Interpolation is the only way to build text (there is no string `+`), so braces are reserved. To print a literal brace, write it twice: `{{` prints `{` and `}}` prints `}`. Quotes are not allowed inside `{ }`: put the text in a variable first.
- **Common causes:**
  - printing a literal brace (JSON, code, a set) without doubling it
  - a placeholder `{}` copied from a Rust or Python format string: name the variable, `"{x}"`
  - a string literal inside the braces, as in `"{n == "x"}"`: store the text in a variable and use `{name}`
  - a `{` that is never closed, as in `"total: {x"`
- **Wrong:**
```rust
fn main() {
    print("{")
}
```
- **Fixed:**
```rust
fn main() {
    print("{{")
}
```
- **Related:** E0004, E0101

## E0007: bad character literal
- **Kind:** compile error · **Since:** v0.3
- **What it means:** A character literal, text between single quotes, does not hold exactly one character. `'a'`, `'é'`, `'\n'` and `'\''` are fine. These are not: `'hello'` (message: "a character literal holds exactly one character, but `'hello'` has 5"), `''` ("empty character") and `'a` with no closing quote ("unterminated character"). The error is reported at the opening quote, and the hint shows the text in double quotes. A character is one Unicode code point, so a letter written as a letter plus a combining accent counts as two.
- **Why Nyra has this rule:** A `char` is exactly one code point, and its number is `c.code()`. A literal with no character or with several would have no type. Text of any length is written in double quotes, so the quotes tell a `char` (`'a'`) from a `str` (`"a"`) at a glance.
- **Common causes:**
  - text in single quotes, as in Python or JavaScript: `'hello'` is written `"hello"`
  - an empty `''` for an empty string: write `""`
  - a missing closing quote: `'a`
  - an apostrophe inside single quotes, as in `'it's'`: use double quotes, `"it's"`
  - an accented letter typed as a letter plus a combining accent (two code points that look like one)
- **Wrong:**
```rust
fn main() {
    let greeting = 'hello'
    if greeting[0] == 'h' {
        print(greeting)
    }
}
```
- **Fixed:**
```rust
fn main() {
    let greeting = "hello"
    if greeting[0] == 'h' {
        print(greeting)
    }
}
```
- **Related:** E0001, E0002, E0004, E0203

## E0101: unexpected token
- **Kind:** compile error · **Since:** v0.1
- **What it means:** The parser found a token that cannot appear at this point. The message names what it expected and what it found (for example: expected end of line, found `x`); the hint usually names the construct you meant.
- **Why Nyra has this rule:** The grammar is small and strict on purpose: `ret` is the only way to return, braces are always required and `{` stays on the line of its `fn`, `if`, `else`, `while` or `for`, and there is one statement per line. So every program has exactly one spelling, and a model that knows another language is corrected at the first deviation.
- **Common causes:**
  - `return`, `elif`, `elseif`, `and`, `or`, `not`, `function`, `def`: Nyra spells them `ret`, `else if`, `&&`, `||`, `!`, `fn`
  - `i++`, `i--`, `2 ** 3`, `0..=9`, `x => x * 2`, `a === b`: these operators do not exist
  - `{` on a line of its own, or a missing `{` or `}`: put `{` on the same line, and close every block
  - two statements on one line (`let a = 1 let b = 2`) or a line that starts with an operator
  - a missing piece: `let x` without `= value`, `fn f(a)` without a type, `for i 0..3` without `in`
  - code outside a function: only `fn` definitions may be at the top level (no globals, no `struct`, no `import`)
  - `0xFF`, `1_000` and `1e5` number forms: write `255`, `1000`, `100000.0`
  - `=` where `==` was meant, as in `if x = 1 {`
  - a format specifier inside a string, as in `"{x:.2f}"`
- **Wrong:**
```rust
fn double(x: int) -> int {
    return x * 2
}

fn main() {
    print(double(4))
}
```
- **Fixed:**
```rust
fn double(x: int) -> int {
    ret x * 2
}

fn main() {
    print(double(4))
}
```
- **Related:** E0102, E0212, E0006, E0001

## E0102: unknown type
- **Kind:** compile error · **Since:** v0.1
- **What it means:** A type annotation names a type that does not exist. The types are `int` (64-bit integer), `float` (64-bit float), `bool` and `str` (text).
- **Why Nyra has this rule:** Four types, each with one name, and no aliases: a model never has to guess whether the text type is `str`, `string` or `String`.
- **Common causes:**
  - another language's name: `string`, `String`, `i32`, `i64`, `double`, `number`, `boolean`, `char`
  - `void` for "returns nothing": leave out the `->` part of the signature
  - arrays, structs and other collection types, which do not exist yet
  - a typo (`flot`) or a capital letter (`Int`): all type names are lowercase
- **Wrong:**
```rust
fn greet(name: string) {
    print("hello {name}")
}

fn main() {
    greet("nyra")
}
```
- **Fixed:**
```rust
fn greet(name: str) {
    print("hello {name}")
}

fn main() {
    greet("nyra")
}
```
- **Related:** E0101, E0203

## E0201: undefined variable
- **Kind:** compile error · **Since:** v0.1
- **What it means:** A name is used as a value, but no variable, parameter or loop variable with that name is visible at that point.
- **Why Nyra has this rule:** Every variable is declared explicitly with `let` or `var`, and names are never created by assignment. A typo can therefore never introduce a new variable silently; it is reported with a "did you mean" hint.
- **Common causes:**
  - a typo in the name (the hint suggests the closest one)
  - the variable is declared later in the function: move its `let` above the use
  - the variable was declared inside an inner `{ }` block and is used after the block ended: declare it before the block
  - a function used without call parentheses: write `limit()`, not `limit`
  - assigning to a variable that was never declared (`count = 1`): declare it first with `var count = 0`
  - words from other languages: `null`, `None`, `break`, `continue`, `return`, `self`, `True`
- **Wrong:**
```rust
fn main() {
    let count = 1
    print(cout)
}
```
- **Fixed:**
```rust
fn main() {
    let count = 1
    print(count)
}
```
- **Related:** E0202, E0206, E0205

## E0202: undefined function
- **Kind:** compile error · **Since:** v0.1
- **What it means:** A call `name(...)` refers to a function that is not defined in the file and is not one of the three builtins `print`, `int` and `float`.
- **Why Nyra has this rule:** There is no standard library yet, so every helper is written in the program itself. A missing function is reported, with the recipe for common ones, instead of guessing what `abs` or `len` should do.
- **Common causes:**
  - a library function from another language: `len`, `abs`, `min`, `max`, `pow`, `sqrt`, `floor`, `str`, `input`
  - `println`, `printf` or `echo`: the output function is `print(x)`
  - a typo in a function name (the hint suggests the closest one)
  - calling a variable as if it were a function
- **Wrong:**
```rust
fn main() {
    print(abs(-3))
}
```
- **Fixed:**
```rust
fn abs(x: int) -> int = if x < 0 { -x } else { x }

fn main() {
    print(abs(-3))
}
```
- **Related:** E0201, E0204

## E0203: type mismatch
- **Kind:** compile error · **Since:** v0.1
- **What it means:** A value has one type where another is required: the initial value of a `let` with a type annotation, an assignment, an argument of a function call, a `ret` value, the bounds of a `for` range, or the argument of `int()`/`float()`. A call to a function that returns nothing cannot be used as a value either.
- **Why Nyra has this rule:** Nothing converts implicitly. Turning an `int` into a `float` (or back) changes the result, so you write it: `float(n)` and `int(x)` (which truncates toward zero). That makes every numeric conversion visible in the code.
- **Common causes:**
  - an `int` where a `float` is needed: write `2.0` for a literal, `float(n)` for a variable
  - a `float` where an `int` is needed, such as a range bound: `int(x)`
  - text where a number is needed (`int("3")`): there is no conversion from text to numbers
  - the wrong type returned from a function, or passed as an argument
  - using the result of a function without a return type (`let x = print(1)`)
  - a number used as a `bool`: write a comparison
- **Wrong:**
```rust
fn half(x: float) -> float = x / 2.0

fn main() {
    let n = 7
    print(half(n))
}
```
- **Fixed:**
```rust
fn half(x: float) -> float = x / 2.0

fn main() {
    let n = 7
    print(half(float(n)))
}
```
- **Related:** E0210, E0204, E0207, E0209

## E0204: wrong number of arguments
- **Kind:** compile error · **Since:** v0.1
- **What it means:** A call passes a different number of arguments than the function declares. `print`, `int` and `float` take exactly one argument.
- **Why Nyra has this rule:** A function has one signature: no default arguments, no variable argument lists, no overloading. The message shows the signature and the hint says which argument is missing or extra.
- **Common causes:**
  - an argument was forgotten, or one too many was passed
  - `print(a, b)`: `print` takes one value, so build the text with interpolation: `print("{a} {b}")`
  - `print()` with nothing to print
  - the function's parameter list changed but a call was not updated
- **Wrong:**
```rust
fn add(a: int, b: int) -> int = a + b

fn main() {
    print(add(1))
}
```
- **Fixed:**
```rust
fn add(a: int, b: int) -> int = a + b

fn main() {
    print(add(1, 2))
}
```
- **Related:** E0203, E0202

## E0205: assignment to a variable that is not `var`
- **Kind:** compile error · **Since:** v0.1
- **What it means:** Something that cannot change is assigned to (`=`, `+=`, `-=`, `*=`, `/=` or `%=`): a variable declared with `let`, a function parameter, or the variable of a `for` loop.
- **Why Nyra has this rule:** Values are immutable unless declared with `var`, so reading a function shows exactly where something can change. Parameters and loop variables never change, which keeps loops and calls easy to reason about.
- **Common causes:**
  - `let` was used where `var` was needed: change the declaration
  - a compound assignment such as `count += 1` on a `let` variable
  - modifying a parameter (`n = n / 2`): copy it first with `var m = n`
  - changing the loop variable of a `for`: use a `while` loop with a `var` counter instead
- **Wrong:**
```rust
fn main() {
    let count = 0
    count += 1
    print(count)
}
```
- **Fixed:**
```rust
fn main() {
    var count = 0
    count += 1
    print(count)
}
```
- **Related:** E0206, E0201

## E0206: name already defined
- **Kind:** compile error · **Since:** v0.1
- **What it means:** A name is declared where Nyra does not allow it: it hides a variable or parameter that is still visible (shadowing), a function is defined twice, a variable has the name of a function, or a function or variable is named `print`, `int` or `float`.
- **Why Nyra has this rule:** One name means one thing. Without shadowing, a model never has to work out which of two `x` is meant, and a rename can never change the meaning of a program. There is no overloading either: every function name is used once.
- **Common causes:**
  - `let x = ...` twice in one function (also in an inner `{ }` block while the outer `x` is visible)
  - a local variable with the name of a parameter
  - a nested `for i in ...` inside another `for i in ...`: name the inner variable `j`
  - two functions with the same name, for example two `max` with different parameter types
  - a variable named like a function that exists (`let add = 1` while `fn add` exists)
  - a parameter or variable named `print`, `int` or `float`
- **Wrong:**
```rust
fn main() {
    let x = 1
    let x = 2
    print(x)
}
```
- **Fixed:**
```rust
fn main() {
    let x = 1
    let y = 2
    print(x + y)
}
```
- **Related:** E0205, E0201

## E0207: bad or missing `ret`
- **Kind:** compile error · **Since:** v0.1
- **What it means:** The use of `ret` does not match the function's signature. A function declared `-> T` must end every path with `ret value`; a function without `->` cannot return a value; and `ret` without a value is only valid in a function that returns nothing.
- **Why Nyra has this rule:** Returning is always explicit, so the end of every path is visible and there is no implicit "last expression is the result" in block functions. The one-line form `fn f(x: int) -> int = x * 2` needs no `ret`.
- **Common causes:**
  - the last `if` has no `else` and no `ret` follows it: add a final `ret` or an `else { ret ... }`
  - the last line is a value (`a + b`) written Rust-style without `ret`
  - a loop that may run zero times is the last statement: add a `ret` after it
  - `ret 1` in a function whose signature has no `-> int`
  - `ret` with no value in a function that returns a value
- **Wrong:**
```rust
fn sign(x: int) -> int {
    if x > 0 { ret 1 }
    if x < 0 { ret -1 }
}

fn main() {
    print(sign(5))
}
```
- **Fixed:**
```rust
fn sign(x: int) -> int {
    if x > 0 { ret 1 }
    if x < 0 { ret -1 }
    ret 0
}

fn main() {
    print(sign(5))
}
```
- **Related:** E0203, E0208

## E0208: missing `fn main()`
- **Kind:** compile error · **Since:** v0.1
- **What it means:** The file defines no function called `main`. A program starts running at `fn main()`.
- **Why Nyra has this rule:** One entry point with one shape means every program starts the same way on every backend. Top-level statements and global variables do not exist.
- **Common causes:**
  - an empty file, or a file with only helper functions
  - the entry function has another name, or a different capitalisation (`Main`)
  - the code is written at the top level instead of inside a function
- **Wrong:**
```rust
fn greet() {
    print("hello")
}
```
- **Fixed:**
```rust
fn greet() {
    print("hello")
}

fn main() {
    greet()
}
```
- **Related:** E0211, E0101

## E0209: condition is not `bool`
- **Kind:** compile error · **Since:** v0.1
- **What it means:** The condition of an `if` or a `while` (or of an `if` used as a value) is not a `bool`.
- **Why Nyra has this rule:** There is no "truthiness": `0`, an empty string or a missing value never count as false. A condition is a comparison or a `bool`, so what it tests is written down.
- **Common causes:**
  - `if n {` or `while n {` with an `int`: write `n != 0`
  - a `str` or `float` used as a condition: compare it (`s != ""`, `x != 0.0`)
  - a call that returns nothing used as a condition
  - a flag stored as `0` and `1`: store a `bool` (`true` / `false`) instead
- **Wrong:**
```rust
fn main() {
    let n = 3
    if n {
        print("not zero")
    }
}
```
- **Fixed:**
```rust
fn main() {
    let n = 3
    if n != 0 {
        print("not zero")
    }
}
```
- **Related:** E0210, E0212

## E0210: operator used on wrong types
- **Kind:** compile error · **Since:** v0.1
- **What it means:** An operator received operands it does not accept. `+ - * /` and `< <= > >=` need two `int`s or two `float`s; `%` needs two `int`s; `==` and `!=` need two values of the same type; `&&`, `||` and `!` need `bool`s; unary `-` needs a number.
- **Why Nyra has this rule:** Operators never convert their operands and cannot be overloaded, so `1 + 2.0` is an error rather than a guess, and `"a" + b` is not text concatenation (text is built with interpolation: `"a{b}"`).
- **Common causes:**
  - an `int` and a `float` in one expression (`1 + 2.0`, `n * 0.5`): convert with `float(n)`
  - compound assignment with the wrong type, such as `x += 1.5` when `x` is an `int`
  - `"total: " + n`: use interpolation, `"total: {n}"`
  - a chained comparison `a < b < c`: write `a < b && b < c`
  - `%` on floats, `<` on strings, `!` on an `int`, `-` on a `bool`
  - `a && b` where `a` or `b` is an `int`: compare each side first
  - comparing different types with `==`, such as a `bool` with `1`
- **Wrong:**
```rust
fn main() {
    print(1 + 2.0)
}
```
- **Fixed:**
```rust
fn main() {
    print(float(1) + 2.0)
}
```
- **Related:** E0203, E0209

## E0211: bad `main`
- **Kind:** compile error · **Since:** v0.1
- **What it means:** `fn main` declares parameters or a return type. `main` takes nothing and returns nothing.
- **Why Nyra has this rule:** A program has no command-line arguments, input or exit code yet, and `main` behaves identically on every backend. To stop early, write `ret` without a value.
- **Common causes:**
  - `fn main() -> int` copied from C or Rust
  - `fn main(args: ...)`: programs are closed, so put the values in the program (`let n = 12`)
  - wanting to return an exit code: `main` always ends normally
- **Wrong:**
```rust
fn main() -> int {
    ret 0
}
```
- **Fixed:**
```rust
fn main() {
    print("done")
}
```
- **Related:** E0208, E0207

## E0212: bad `if` used as a value
- **Kind:** compile error · **Since:** v0.2
- **What it means:** An `if` that is used as a value (`let x = if c { a } else { b }`) is incomplete or inconsistent: it has no `else`, a branch is not exactly one expression (an empty branch, a statement, several lines), a branch produces no value, or the two branches have different types.
- **Why Nyra has this rule:** A value must exist on every path and have one type, so the compiler can give it that type. Branches that do things belong in an `if` statement, which has no value.
- **Common causes:**
  - `let x = if c { 1 }` without `else`
  - a branch that holds two lines or a statement such as `print(...)` or an assignment
  - an empty branch `{ }`
  - branches of different types, such as `{ 1 } else { 2.5 }`: convert one (`float(1)`, or write `1.0`)
  - a branch that calls a function that returns nothing
- **Wrong:**
```rust
fn main() {
    let n = 5
    let sign = if n > 0 { 1 }
    print(sign)
}
```
- **Fixed:**
```rust
fn main() {
    let n = 5
    let sign = if n > 0 { 1 } else { 0 }
    print(sign)
}
```
- **Related:** E0101, E0203, E0209

## E0220: duplicate field in a struct
- **Kind:** compile error · **Since:** v0.3
- **What it means:** A `struct` declares two fields with the same name. The message names the field, the struct and the line of the first one: "field `score` is defined twice in struct `Player` (first on line 3)". The error is reported at the second declaration, which is then ignored, so the rest of the program is checked against the first one.
- **Why Nyra has this rule:** A field name must identify exactly one value, so `p.score` and `Player(name: "ann", score: 3)` mean one thing.
- **Common causes:**
  - a field line copied and not renamed
  - two fields that were meant to differ (`x` and `y`, `width` and `height`)
  - the same field added twice by two edits
- **Wrong:**
```rust
struct Player {
    name: str
    score: int
    score: int
}

fn main() {
    let p = Player(name: "ann", score: 3)
    print("{p.name} {p.score}")
}
```
- **Fixed:**
```rust
struct Player {
    name: str
    score: int
}

fn main() {
    let p = Player(name: "ann", score: 3)
    print("{p.name} {p.score}")
}
```
- **Related:** E0206, E0221, E0224

## E0221: struct name must start with an uppercase letter
- **Kind:** compile error · **Since:** v0.3
- **What it means:** A struct name does not start with an uppercase letter. The message is "struct name `point` must start with an uppercase letter" and the hint gives the name to write, `struct Point`. The lowercase name still works everywhere in the file until it is renamed, so this is the only error you get for it. After the rename, any place that still uses the old spelling is reported there: an unknown type (E0102) or an undefined function (E0202) for a construction, each with "did you mean `Point`?".
- **Why Nyra has this rule:** `Point(x: 1, y: 2)` builds a struct and `point(1, 2)` calls a function, and the two are told apart at a glance. A type name starts uppercase, a variable or function name starts lowercase.
- **Common causes:**
  - a lowercase or snake_case name chosen out of habit from C (`struct point`) or Python
  - a name that starts with `_`
- **Wrong:**
```rust
struct point {
    x: int
    y: int
}

fn main() {
    let p = point(x: 1, y: 2)
    print(p.x + p.y)
}
```
- **Fixed:**
```rust
struct Point {
    x: int
    y: int
}

fn main() {
    let p = Point(x: 1, y: 2)
    print(p.x + p.y)
}
```
- **Related:** E0102, E0202, E0206, E0220

## E0222: struct contains itself
- **Kind:** compile error · **Since:** v0.3
- **What it means:** A struct has a field whose type is the struct itself, or a struct that contains it in turn, so a value would have to contain itself. The message is "struct `Node` contains itself, so its size would be infinite" and it is reported at the struct's name (for two structs that contain each other, at both). A field of type `[Node]` is fine: an array is not stored inside the struct, and it can be empty.
- **Why Nyra has this rule:** A struct value holds its fields directly, so a struct that contained itself would never end. Anything recursive, such as a list or a tree, keeps its children in an array, which can be empty, and that ends the recursion.
- **Common causes:**
  - a linked list written as `next: Node`
  - a tree written with direct children (`left: Tree`, `right: Tree`)
  - two structs that contain each other (`struct A { b: B }` and `struct B { a: A }`)
- **Wrong:**
```rust
struct Node {
    value: int
    next: Node
}

fn main() {
    print("linked list")
}
```
- **Fixed:**
```rust
struct Node {
    value: int
    next: [Node]
}

fn main() {
    let last = Node(value: 2, next: [])
    let first = Node(value: 1, next: [last])
    print(first.next.len())
}
```
- **Related:** E0220, E0221

## E0223: missing field in a struct construction
- **Kind:** compile error · **Since:** v0.3
- **What it means:** A construction `Point(...)` does not give a value for every field of the struct. The message lists what is missing: "missing field in `Point(...)`: `y`" (or "missing fields ... `x`, `y`") and the hint shows the whole call: `Point(x: ..., y: ...)`.
- **Why Nyra has this rule:** There are no default values and no null. Every field of a struct always holds a value, so a field can never be read before it was set.
- **Common causes:**
  - a field was added to the struct and a construction was not updated
  - a field was forgotten
  - `Point()` with no arguments, hoping for default values
  - a field name with a typo in the construction: the unknown name is reported (E0224) and so is the field that got no value
- **Wrong:**
```rust
struct Point {
    x: int
    y: int
}

fn main() {
    let p = Point(x: 1)
    print(p.x)
}
```
- **Fixed:**
```rust
struct Point {
    x: int
    y: int
}

fn main() {
    let p = Point(x: 1, y: 2)
    print(p.x)
}
```
- **Related:** E0224, E0225

## E0224: unknown field
- **Kind:** compile error · **Since:** v0.3
- **What it means:** A field name is used that the type does not have. In a construction (`User(name: "ann", years: 31)`) or a read or write (`u.username`) the message is "`User` has no field `username`", and the hint suggests the closest field or lists all of them. The same code is used for `xs.length` on an array, a string or a character: they have methods and no fields, and the hint names the method that does the job ("the length is `xs.len()`").
- **Why Nyra has this rule:** A typo in a field name must be an error, not a new value or an empty one.
- **Common causes:**
  - a typo in a field name
  - a field that exists in another struct
  - a field that was renamed in the struct but not where it is used
  - `xs.length` or `s.length`: the length is a method, `xs.len()`
- **Wrong:**
```rust
struct User {
    name: str
    age: int
}

fn main() {
    let u = User(name: "ann", age: 31)
    print(u.username)
}
```
- **Fixed:**
```rust
struct User {
    name: str
    age: int
}

fn main() {
    let u = User(name: "ann", age: 31)
    print(u.name)
}
```
- **Related:** E0223, E0227, E0236

## E0225: struct fields must be named, in declaration order
- **Kind:** compile error · **Since:** v0.3
- **What it means:** A construction passes values without field names (`Point(1, 2)`) or in a different order than the struct declares them (`Point(y: 2, x: 1)`). It is reported once per construction, at the first value that is wrong: "the fields of `Point(...)` must be named, in declaration order", and the hint shows the call to write: `Point(x: ..., y: ...)`.
- **Why Nyra has this rule:** Naming every field makes a construction readable and unambiguous. Writing the fields in the same order everywhere means that a printed struct, `Point(x: 1, y: 2)`, is also valid source code.
- **Common causes:**
  - positional values, as for a tuple struct in Rust or a constructor in C, Java or Python
  - the fields written in another order than the declaration
  - braces instead of parentheses, `Point { x: 1, y: 2 }` (that is E0101)
- **Wrong:**
```rust
struct Point {
    x: int
    y: int
}

fn main() {
    let p = Point(1, 2)
    print(p.x)
}
```
- **Fixed:**
```rust
struct Point {
    x: int
    y: int
}

fn main() {
    let p = Point(x: 1, y: 2)
    print(p.x)
}
```
- **Related:** E0101, E0223, E0226

## E0226: named argument in a function call
- **Kind:** compile error · **Since:** v0.3
- **What it means:** A call to a function, a builtin or a method uses `name: value` arguments. Names belong only to building a struct, `Point(x: 1, y: 2)`. Each named argument is reported: "named argument `width:` in a call to function `area`", and when the values are short the hint shows the call to write, with the values in the order of the parameters: `area(3, 4)`.
- **Why Nyra has this rule:** Arguments are passed in one way only: by position, in the order of the parameters. There are no keyword arguments, no default values and no reordering, so a call reads the same way everywhere. Struct construction is the one place where names appear.
- **Common causes:**
  - keyword arguments from Python, Swift or Kotlin
  - calling a function as if it were a struct
  - `print(value: x)` or `str(value: x)`
- **Wrong:**
```rust
fn area(width: int, height: int) -> int = width * height

fn main() {
    print(area(width: 3, height: 4))
}
```
- **Fixed:**
```rust
fn area(width: int, height: int) -> int = width * height

fn main() {
    print(area(3, 4))
}
```
- **Related:** E0204, E0225

## E0227: unknown method
- **Kind:** compile error · **Since:** v0.3
- **What it means:** A method call `value.name(...)` names a method that the type of `value` does not have. Arrays, strings and characters have a fixed list of methods; numbers, bools and structs have none. The message is "`[int]` has no method `length`". The hint gives the Nyra spelling when the name is one that other languages use (`length` is `len`, `append` is `push`, `toUpperCase` is `upper`, `substring` is `slice`), suggests the closest name for a typo, or lists the methods. For a struct it says to write a function instead: `area(r)`, not `r.area()`.
- **Why Nyra has this rule:** The built-in operations are methods so that they add no global names, and the list is small and fixed, so there is one name for each operation. Structs have no methods at all: a function that takes the struct as a parameter does the same.
- **Common causes:**
  - a method name from another language: `length()`, `size()`, `append(x)`, `toUpperCase()`, `substring(a, b)`, `isEmpty()`, `map(...)`
  - a method on a number: `n.abs()`, `x.sqrt()`, `n.to_string()` (write `str(n)`)
  - calling a method on a struct: write `area(r)`
  - `"A".code()`: `code()` belongs to `char`, so write `'A'.code()` or `s[0].code()` (all codes of a string: `s.codes()`)
  - a typo in a method name
- **Wrong:**
```rust
fn main() {
    let xs = [4, 8, 15]
    print(xs.length())
}
```
- **Fixed:**
```rust
fn main() {
    let xs = [4, 8, 15]
    print(xs.len())
}
```
- **Related:** E0224, E0228, E0236

## E0228: method not available for this element type
- **Kind:** compile error · **Since:** v0.3
- **What it means:** A method exists, but not for the element type of this array. `join` needs `[str]` or `[char]`, and `sort` needs `[int]`, `[float]`, `[str]` or `[char]`. The message is "`join` needs `[str]` or `[char]`, found `[int]`". For `join` the hint shows the loop that turns the elements into text first.
- **Why Nyra has this rule:** Joining and ordering are only defined for element types where the result is the same on every backend. Characters and strings join as text and numbers, strings and characters have one order. There is no order for `bool` or for structs, and `join` never converts numbers to text behind your back: `str(x)` says what text you want.
- **Common causes:**
  - `join` on an array of numbers: turn the elements into text first with `str(x)`
  - `sort` on an array of `bool` or of structs
  - `sort` on an array of arrays
- **Wrong:**
```rust
fn main() {
    let nums = [3, 1, 2]
    print(nums.join(", "))
}
```
- **Fixed:**
```rust
fn main() {
    let nums = [3, 1, 2]
    var parts: [str] = []
    for n in nums {
        parts.push(str(n))
    }
    print(parts.join(", "))
}
```
- **Related:** E0227

## E0229: cannot assign to this expression
- **Kind:** compile error · **Since:** v0.3
- **What it means:** The left side of an assignment, the receiver of a method that changes its receiver (`push`, `pop`, `insert`, `remove`, `sort`, `reverse`) or an `inout` argument is not something that can change. Only a variable, a field or an element of one can. The messages name the case: "cannot assign to a character of a string: strings are immutable" (`name[0] = 'A'`), "cannot call `.push()` on a temporary value: it changes its receiver" (`items().push(3)`), "cannot assign to this expression" (`a + b = 3`) and "`inout` needs a variable, a field or an element" (`bump(inout 5)`). A variable that is not a `var` is a different error, E0205.
- **Why Nyra has this rule:** Only variables, and the fields and elements inside them, can change. A string is a value that never changes in place: to change a character, build a new string and assign it. A temporary value has no name, so a change to it would be lost.
- **Common causes:**
  - `s[0] = 'A'` on a string: build the new string with `slice` and `+`
  - `push`, `pop` or `sort` on the result of a function call: store the result in a `var` first
  - an assignment to an expression such as `a + b = 3` or `f() = 1`
  - `inout` with a value that is not a variable, such as `inout 5` or `inout f()`
- **Wrong:**
```rust
fn main() {
    var name = "ann"
    name[0] = 'A'
    print(name)
}
```
- **Fixed:**
```rust
fn main() {
    var name = "ann"
    name = str(name[0].upper()) + name.slice(1, name.len())
    print(name)
}
```
- **Related:** E0205, E0233, E0237

## E0230: cannot infer the type of `[]`
- **Kind:** compile error · **Since:** v0.3
- **What it means:** An empty array literal `[]` appears where nothing says what its element type is. The message is "cannot infer the type of the empty array `[]`" and the hint shows `var xs: [int] = []`. The type is known, and `[]` is fine, where it is declared (`var xs: [int] = []`), assigned to a variable, passed as an argument, put in a struct field, returned with `ret`, pushed or inserted into an array of arrays, or compared with or added to an array of known type (`xs == []`, `xs + []`).
- **Why Nyra has this rule:** Every array has one element type, and an empty literal contains no element to read it from. The compiler never guesses a type, so a program is the same on every backend.
- **Common causes:**
  - `var xs = []` or `let xs = []` without a type, to be filled later with `push`
  - `print([])`, or `[] == []`, where there is no known type on either side
  - `[[], [1]]`: the first `[]` is read before the element that would give its type (write `[[1], []]`, or declare the type of the whole array)
- **Wrong:**
```rust
fn main() {
    var names = []
    names.push("ann")
    print(names.len())
}
```
- **Fixed:**
```rust
fn main() {
    var names: [str] = []
    names.push("ann")
    print(names.len())
}
```
- **Related:** E0231

## E0231: array elements must have one type
- **Kind:** compile error · **Since:** v0.3
- **What it means:** The elements of an array literal do not all have the same type, as in `["ann", 31]`. The message is "array elements must all have one type: the first is `str`, this one is `int`" and it is reported at every element that does not match the first one. The hint depends on the case: `str(31)` to write a number as text, `1.0` to make whole numbers floats, or a struct to group values of different types.
- **Why Nyra has this rule:** An array `[T]` holds values of exactly one type, so reading an element always gives a known type. Values of different types that belong together, such as a name and an age, are a struct with named fields.
- **Common causes:**
  - a "row" of mixed values, `["ann", 31]`: declare a `struct User { name: str, age: int }`
  - mixing `int` and `float` elements, `[1, 2.5]`: write them all as floats, `[1.0, 2.5]`
  - an element that is a call or a variable of another type than the first
- **Wrong:**
```rust
fn main() {
    let user = ["ann", 31]
    print(user.len())
}
```
- **Fixed:**
```rust
struct User {
    name: str
    age: int
}

fn main() {
    let user = User(name: "ann", age: 31)
    print(user.age)
}
```
- **Related:** E0203, E0230

## E0232: index must be an `int`
- **Kind:** compile error · **Since:** v0.3
- **What it means:** An array or a string is indexed with a value that is not an `int`: a `float`, a `str` or a `char`. The message is "an index must be an `int`, found `float`" and the hint shows the conversion to write: `[int(mid)]`.
- **Why Nyra has this rule:** A position is a whole number, and Nyra never converts a float to an int behind your back. `int(x)` truncates toward zero and is written where you want it. An array is not a map: a `str` is not a key, and a `char` is not a position (use `c.code()` for its number).
- **Common causes:**
  - an index computed with floats, such as half the length: `float(xs.len()) / 2.0`
  - a float literal such as `xs[1.0]`
  - a `str` used as a key (`xs["a"]`), which arrays do not have
  - a `char` used as an index into a table of counts (`counts[c]`): write `counts[c.code()]`
- **Wrong:**
```rust
fn main() {
    let xs = [10, 20, 30, 40, 50]
    let mid = float(xs.len()) / 2.0
    print(xs[mid])
}
```
- **Fixed:**
```rust
fn main() {
    let xs = [10, 20, 30, 40, 50]
    let mid = xs.len() / 2
    print(xs[mid])
}
```
- **Related:** E0233, E0240

## E0233: cannot index this type
- **Kind:** compile error · **Since:** v0.3
- **What it means:** The index operator `[...]` is applied to a value that is not an array or a string: an `int`, a `float`, a `bool`, a `char` or a struct. The message is "cannot index a value of type `int`". For a struct the hint names a field to read, `p.n`; for an `int` it shows how to get at the digits, `str(n)`.
- **Why Nyra has this rule:** Only arrays and strings have positions. A struct has fields, read with a dot, and a number is one value.
- **Common causes:**
  - indexing a number to get one of its digits: turn it into text first with `str(n)`, or use `n % 10` and `n / 10`
  - indexing a struct instead of reading a field (`p[0]` instead of `p.x`)
  - a variable that was declared as one value (`let xs = 5`) but is used as an array: look at its declaration
  - indexing a `char` (`c[0]`)
- **Wrong:**
```rust
fn main() {
    let n = 4821
    print(n[0])
}
```
- **Fixed:**
```rust
fn main() {
    let n = 4821
    let digits = str(n)
    print(digits[0])
}
```
- **Related:** E0229, E0232, E0234

## E0234: cannot loop over this type
- **Kind:** compile error · **Since:** v0.3
- **What it means:** `for x in value` is used with a value that is not an array or a string, and it is not a range `a..b`: an `int`, a `float`, a `bool`, a `char` or a struct. The message is "cannot loop over `int`: `for i in ...` needs an array, a string or a range". For an `int` the hint shows the range to write, `for i in 0..n`.
- **Why Nyra has this rule:** A loop needs a clear sequence of values: the numbers of a range (the end is not included), the elements of an array, or the characters of a string. A single number is none of them.
- **Common causes:**
  - `for i in n` to count to `n`, as in Python's `for i in range(n)`: write `for i in 0..n`
  - looping over the digits of a number: turn it into text first, `for c in str(n)`
  - looping over a struct: loop over an array field instead (`for x in p.items`)
- **Wrong:**
```rust
fn main() {
    let n = 5
    for i in n {
        print(i)
    }
}
```
- **Fixed:**
```rust
fn main() {
    let n = 5
    for i in 0..n {
        print(i)
    }
}
```
- **Related:** E0203, E0233

## E0235: type used as a value
- **Kind:** compile error · **Since:** v0.3
- **What it means:** The name of a struct is used where a value is needed, as in `let origin = Point` or `Point.new(1, 2)`. The message is "`Point` is a type, not a value" and the hint shows the construction with every field: `Point(x: ..., y: ...)`.
- **Why Nyra has this rule:** Types and values are different things. A struct name appears in declarations (`p: Point`) and in the construction `Point(x: 1, y: 2)`. There are no constructors such as `new`, no default instance and no static methods.
- **Common causes:**
  - forgetting the construction parentheses and fields
  - `Point.new(...)`, `Point.default()` or `Point.zero()` from another language
  - passing the type where an instance was meant
- **Wrong:**
```rust
struct Point {
    x: int
    y: int
}

fn main() {
    let origin = Point.new(1, 2)
    print(origin.x)
}
```
- **Fixed:**
```rust
struct Point {
    x: int
    y: int
}

fn main() {
    let origin = Point(x: 1, y: 2)
    print(origin.x)
}
```
- **Related:** E0206, E0223

## E0236: method must be called
- **Kind:** compile error · **Since:** v0.3
- **What it means:** A method name is written without call parentheses, as in `xs.len`. The message is "method `len` must be called" and the hint is "add the parentheses: `.len()`".
- **Why Nyra has this rule:** A method is always called. There are no function values, so a method name alone has nothing to stand for, and `xs.len` can never be a number.
- **Common causes:**
  - `xs.len` or `s.len` written like a property, as `length` is in JavaScript
  - `xs.pop` or `s.trim` on a line of its own, without `()`
- **Wrong:**
```rust
fn main() {
    let xs = [4, 8, 15]
    if xs.len > 2 {
        print("long")
    }
}
```
- **Fixed:**
```rust
fn main() {
    let xs = [4, 8, 15]
    if xs.len() > 2 {
        print("long")
    }
}
```
- **Related:** E0224, E0227

## E0237: bad `inout` argument
- **Kind:** compile error · **Since:** planned for v0.3, not in the compiler yet
- **What it means:** `inout` is used inconsistently. A parameter declared `inout` is called without `inout` at the call (or `inout` is written for a plain parameter), or two `inout` arguments of one call are the same variable, as in `swap(inout xs[0], inout xs[1])`.
- **Why Nyra has this rule:** An `inout` parameter lets a function change the caller's variable. The call says `inout` too, so you can see what can change, and two `inout` arguments are never the same variable, so nothing can observe the variable half-way: no aliasing surprises.
- **Common causes:**
  - `inout` forgotten at the call: `bump(x)` instead of `bump(inout x)`
  - `inout` written for a parameter that is not `inout`
  - two elements of one array passed to one call: swap through a temporary variable instead
- **Wrong:**
```rust
fn bump(inout n: int) {
    n += 1
}

fn main() {
    var x = 1
    bump(x)
    print(x)
}
```
- **Fixed:**
```rust
fn bump(inout n: int) {
    n += 1
}

fn main() {
    var x = 1
    bump(inout x)
    print(x)
}
```
- **Related:** E0205, E0229, E0238

## E0238: bad target for `free`, `keep` or `arena`
- **Kind:** compile error · **Since:** planned for v0.3, not in the compiler yet
- **What it means:** `free(x)` or `keep(x)` is used on something that cannot be freed or kept (an `int`, a parameter, an `inout` parameter, an expression), or a string or array declared outside an `arena { }` block is changed inside the block.
- **Why Nyra has this rule:** Memory is automatic unless a program asks: `free(x)` returns a local variable's memory now, `arena { }` frees everything created inside it at its `}`, and `keep(x)` never frees. Every use is checked at compile time, so a program that compiles can never use freed memory. Values created in an arena are freed at its `}`, so nothing declared outside may be changed to point into it.
- **Common causes:**
  - `free(n)` on an `int`: there is nothing to free
  - freeing or keeping a parameter: the caller owns it
  - `names.push(...)` inside an `arena` while `names` was declared before it: change it after the block, or return the result from a function that uses `arena`
- **Wrong:**
```rust
fn main() {
    let n = 1
    free(n)
}
```
- **Fixed:**
```rust
fn main() {
    var xs = [1, 2, 3]
    print(xs.len())
    free(xs)
}
```
- **Related:** E0239, E0237, E0205

## E0239: use after free
- **Kind:** compile error · **Since:** planned for v0.3, not in the compiler yet
- **What it means:** A variable is used after `free(x)`, or on a path where it may have been freed (freed in only one branch of an `if`, or in an earlier round of a loop). Every use, including a second `free`, is an error until a `var` is given a new value. The message says where the variable was freed: "`xs` was freed at line 7".
- **Why Nyra has this rule:** Freed memory must never be used. The compiler follows every local variable through the function and rejects the program, instead of letting it crash or print garbage.
- **Common causes:**
  - reading the variable after `free(xs)`
  - `free` inside an `if`, with a use after the `if`
  - `free` inside a loop, so the second round uses a freed variable
- **Wrong:**
```rust
fn main() {
    var xs = [1]
    free(xs)
    print(xs)
}
```
- **Fixed:**
```rust
fn main() {
    var xs = [1]
    print(xs)
    free(xs)
}
```
- **Related:** E0238, E0201

## E0240: index out of bounds
- **Kind:** runtime error · **Since:** planned for v0.3, not in the compiler yet
- **What it means:** At run time, an index or a range is outside the array or string: `index 3 is out of bounds for length 3`. Valid indexes are 0 up to the length minus 1; positions for `insert`, `remove` and `slice` are checked the same way.
- **Why Nyra has this rule:** Reading past the end would be undefined behaviour in C and `undefined` in JavaScript. Nyra stops the program with the same error and exit code 101 on every backend.
- **Common causes:**
  - an off-by-one: the last valid index is `xs.len() - 1`
  - an index computed from data without checking it against `xs.len()`
  - indexing an empty array
- **Wrong:**
```rust
fn main() {
    let xs = [1, 2, 3]
    print("before")
    print(xs[3])
}
```
- **Fixed:**
```rust
fn main() {
    let xs = [1, 2, 3]
    print("before")
    print(xs[xs.len() - 1])
}
```
- **Related:** E0242, E0232

## E0241: division by zero
- **Kind:** runtime error · **Since:** v0.2
- **What it means:** An integer `/` or `%` was executed with a divisor of zero. The program flushes everything it printed so far, prints `runtime error[E0241]` with the file and position of the operator, and exits with code 101. Float division by zero is not an error: `1.0 / 0.0` is `Infinity` and `0.0 / 0.0` is `NaN`.
- **Why Nyra has this rule:** Natively the CPU would crash with a signal and lose the output, while JavaScript would quietly produce `Infinity`. Nyra makes every backend stop the same way instead of hiding the bug.
- **Common causes:**
  - a divisor that is computed and happens to reach 0 (a count, a difference of two equal values)
  - a loop that starts its divisor at 0
  - an average over zero items
- **Wrong:**
```rust
fn div(a: int, b: int) -> int = a / b

fn main() {
    print(div(6, 3))
    print(div(1, 0))
}
```
- **Fixed:**
```rust
fn div(a: int, b: int) -> int {
    if b == 0 { ret 0 }
    ret a / b
}

fn main() {
    print(div(6, 3))
    print(div(1, 0))
}
```
- **Related:** E0245

## E0242: pop on an empty array
- **Kind:** runtime error · **Since:** planned for v0.3, not in the compiler yet
- **What it means:** `xs.pop()` was called on an array with no elements, so there is no last element to remove and return.
- **Why Nyra has this rule:** There is no null to return instead, so the program stops with exit code 101 on every backend.
- **Common causes:**
  - popping in a loop that runs more often than the array was filled
  - an array that is empty because of an earlier branch
- **Wrong:**
```rust
fn main() {
    var xs: [int] = []
    xs.pop()
}
```
- **Fixed:**
```rust
fn main() {
    var xs: [int] = []
    if xs.len() > 0 {
        xs.pop()
    }
}
```
- **Related:** E0240

## E0243: bad argument value
- **Kind:** runtime error · **Since:** planned for v0.3, not in the compiler yet
- **What it means:** A method received an argument that is valid in type but not in value: `repeat(n)` with a negative `n`, `replace("", x)` with an empty pattern, or `split("")` with an empty separator (for the characters of a string use `s.chars()`).
- **Why Nyra has this rule:** These calls have no sensible result, and the host languages disagree about them. A clear error beats an answer that differs by backend.
- **Common causes:**
  - a repeat count computed as a negative number
  - an empty search text passed to `replace`, or an empty separator passed to `split`
- **Wrong:**
```rust
fn main() {
    print("ab".repeat(0 - 1))
}
```
- **Fixed:**
```rust
fn main() {
    print("ab".repeat(2))
}
```
- **Related:** E0240, E0244

## E0244: cannot parse text as a number
- **Kind:** runtime error · **Since:** planned for v0.3, not in the compiler yet
- **What it means:** `int(s)` or `float(s)` was given text that is not a number: `int("12x")`. `int` accepts only digits with an optional leading `-` and must fit in 64 bits; the text may not contain spaces, a `+` or an `_`.
- **Why Nyra has this rule:** Turning letters into numbers silently would hide bugs. A failing conversion stops the program with a precise message; check the text first when it may be invalid.
- **Common causes:**
  - text with spaces or a unit (`"12 "`, `"3px"`)
  - an empty string
  - a number too large for `int`
- **Wrong:**
```rust
fn main() {
    let n = int("12x")
    print(n)
}
```
- **Fixed:**
```rust
fn main() {
    let n = int("12")
    print(n)
}
```
- **Related:** E0245, E0243

## E0245: `int()` of a value that is not a whole number in range
- **Kind:** runtime error · **Since:** v0.2
- **What it means:** `int(x)` was called with NaN, plus or minus Infinity, or a float outside the range of a 64-bit `int`. The program stops with `runtime error[E0245]` and exit code 101. A finite float inside the range is truncated toward zero: `int(2.9)` is `2`.
- **Why Nyra has this rule:** C leaves the result undefined and JavaScript produces other values. Saturating or returning 0 would hide a bug, so every backend stops with the same error.
- **Common causes:**
  - the result of a division by `0.0` (`1.0 / 0.0` is `Infinity`, `0.0 / 0.0` is NaN)
  - a very large float, such as a repeated multiplication that overflowed
  - a NaN produced by earlier float arithmetic, such as `0.0 / 0.0`
- **Wrong:**
```rust
fn main() {
    print(int(1.0 / 0.0))
}
```
- **Fixed:**
```rust
fn main() {
    let ratio = 1.0 / 0.0
    if ratio < 1000000.0 {
        print(int(ratio))
    } else {
        print("too big")
    }
}
```
- **Related:** E0241, E0244

## E0246: not a valid character code
- **Kind:** runtime error · **Since:** planned for v0.3, not in the compiler yet
- **What it means:** `char(n)` was called with a number that is not a character: negative, above 1114111, or in the surrogate range 55296 to 57343. The program stops with exit code 101.
- **Why Nyra has this rule:** A `char` is always one real Unicode character, so a string can never contain invalid text. Turning a number into a character is explicit, and an impossible number is an error rather than a replacement character.
- **Common causes:**
  - arithmetic on codes that left the valid range, such as `char('a'.code() - 100)`
  - using a byte or a random number as a code without checking it
- **Wrong:**
```rust
fn main() {
    print(char(-1))
}
```
- **Fixed:**
```rust
fn main() {
    print(char(65))
}
```
- **Related:** E0244, E0245

## E0249: out of memory
- **Kind:** runtime error · **Since:** planned for v0.3, not in the compiler yet
- **What it means:** The native backend could not allocate memory for a string, array or struct. The program stops with exit code 101. The JavaScript backend has no such error: the engine reports its own failure.
- **Why Nyra has this rule:** With memory managed by the runtime, running out of it must end the program cleanly with a message rather than a crash.
- **Common causes:**
  - building a huge array or string in a loop
  - an endless loop that keeps growing a value
- **Wrong:**
```rust
fn main() {
    var xs: [int] = []
    while true {
        xs.push(1)
    }
}
```
- **Fixed:**
```rust
fn main() {
    var xs: [int] = []
    for i in 0..1000 {
        xs.push(i)
    }
}
```
- **Related:** E0240

## E0300: module not found
- **Kind:** compile error · **Since:** planned for v0.6, not in the compiler yet
- **What it means:** A `use` line names a module that cannot be found: a library name that is not in the standard library or the dependencies, or a quoted path to a file that does not exist (the file name must match exactly, including capital letters).
- **Why Nyra has this rule:** Imports must be explicit and checkable before anything runs. The message suggests the closest module name and lists the standard modules.
- **Common causes:**
  - a typo in a module name (`mth` for `math`)
  - a file path without `./`, or with the wrong capitalisation
  - a dependency that is not listed in `nyra.toml`
  - `use str`: the string helpers are in the `text` module (and `str(x)` is a builtin)
- **Wrong:**
```rust
use mth

fn main() {
    print(mth.sqrt(2.0))
}
```
- **Fixed:**
```rust
use math

fn main() {
    print(math.sqrt(2.0))
}
```
- **Related:** E0302, E0305, E0306

## E0301: item is private to its module
- **Kind:** compile error · **Since:** planned for v0.6, not in the compiler yet
- **What it means:** A function or constant of another module is used, but that module did not mark it `pub`.
- **Why Nyra has this rule:** A module decides what it exposes. Private by default means changing a helper never breaks code in other files.
- **Common causes:**
  - the `pub` keyword is missing on the definition
  - calling an internal helper that was not meant to be shared
- **Wrong:**
```rust
// shapes.nyra
fn secret() -> int = 42

// main.nyra
use "./shapes"

fn main() {
    print(shapes.secret())
}
```
- **Fixed:**
```rust
// shapes.nyra
pub fn secret() -> int = 42

// main.nyra
use "./shapes"

fn main() {
    print(shapes.secret())
}
```
- **Related:** E0306, E0332

## E0302: bad `use` line
- **Kind:** compile error · **Since:** planned for v0.6, not in the compiler yet
- **What it means:** A `use` line is not in one of the allowed forms or is in the wrong place. The forms are `use name`, `use "./path"` and `... as alias`; all `use` lines come first in the file, one per line.
- **Why Nyra has this rule:** One import syntax in one place, so the dependencies of a file are visible at the top. Other languages' spellings (`import`, `from ... import`, `use a.{b}`) are not accepted.
- **Common causes:**
  - `import math` instead of `use math`
  - a `use` line after a function
  - an alias that is not an identifier
- **Wrong:**
```rust
import math

fn main() {
    print(math.sqrt(2.0))
}
```
- **Fixed:**
```rust
use math

fn main() {
    print(math.sqrt(2.0))
}
```
- **Related:** E0300, E0304

## E0303: import cycle
- **Kind:** compile error · **Since:** planned for v0.6, not in the compiler yet
- **What it means:** Two or more files import each other, directly or through a chain: `a.nyra -> b.nyra -> a.nyra`.
- **Why Nyra has this rule:** Modules are checked in dependency order, which needs a cycle-free graph. The message prints the chain.
- **Common causes:**
  - two files that call each other's functions
  - shared helpers placed in one of the two files
- **Wrong:**
```rust
// a.nyra
use "./b"
pub fn f() -> int = b.g()

// b.nyra
use "./a"
pub fn g() -> int = a.f()
```
- **Fixed:**
```rust
// shared.nyra
pub fn base() -> int = 1

// a.nyra
use "./shared"
pub fn f() -> int = shared.base()

// b.nyra
use "./shared"
pub fn g() -> int = shared.base() + 1
```
- **Related:** E0304, E0305

## E0304: two imports with the same name
- **Kind:** compile error · **Since:** planned for v0.6, not in the compiler yet
- **What it means:** Two `use` lines give the same module name, for example `./a/util` and `./b/util` which are both called `util`.
- **Why Nyra has this rule:** A module is always used by its name (`util.f(x)`), so each name must mean one module in a file. `as` renames one of them.
- **Common causes:**
  - files with the same name in different folders
  - importing one module twice
- **Wrong:**
```rust
use "./a/util"
use "./b/util"

fn main() {
    print(util.f())
}
```
- **Fixed:**
```rust
use "./a/util"
use "./b/util" as butil

fn main() {
    print(util.f() + butil.f())
}
```
- **Related:** E0302, E0206

## E0305: import path not allowed
- **Kind:** compile error · **Since:** planned for v0.6, not in the compiler yet
- **What it means:** The quoted path of a `use "..."` is not valid. A path must start with `./` or `../`, use `/` as the only separator, leave out `.nyra` and stay inside the project folder.
- **Why Nyra has this rule:** One portable path syntax that means the same on Windows and Linux, and a project cannot reach outside its own folder by accident.
- **Common causes:**
  - `use "shapes"` without `./`
  - a Windows path with backslashes
  - `use "./shapes.nyra"` with the extension
  - a path that climbs out of the project (`../../x`)
- **Wrong:**
```rust
use "shapes.nyra"

fn main() {
    print(shapes.area(3))
}
```
- **Fixed:**
```rust
use "./shapes"

fn main() {
    print(shapes.area(3))
}
```
- **Related:** E0300, E0303

## E0306: module has no such item
- **Kind:** compile error · **Since:** planned for v0.6, not in the compiler yet
- **What it means:** `module.name` is used, but the module defines no public function or constant called `name`.
- **Why Nyra has this rule:** A typo after a module name must not turn into a new meaning. The message suggests the closest name.
- **Common causes:**
  - a typo (`math.sqroot`)
  - a function that exists under another name in this module, such as `random.randint(1, 6)`: write `random.range(1, 7)` (the upper bound is excluded)
- **Wrong:**
```rust
use math

fn main() {
    print(math.sqroot(2.0))
}
```
- **Fixed:**
```rust
use math

fn main() {
    print(math.sqrt(2.0))
}
```
- **Related:** E0300, E0301, E0307

## E0307: wrong kind of module item
- **Kind:** compile error · **Since:** planned for v0.6, not in the compiler yet
- **What it means:** A module item is used the wrong way: a function without a call (`math.sqrt`), a constant called like a function (`math.PI()`), or a module name used as a value.
- **Why Nyra has this rule:** Functions are always called and constants are never called, so each use shows what kind of thing it is.
- **Common causes:**
  - forgetting the parentheses on a function
  - adding parentheses to a constant
- **Wrong:**
```rust
use math

fn main() {
    print(math.PI())
}
```
- **Fixed:**
```rust
use math

fn main() {
    print(math.PI)
}
```
- **Related:** E0306, E0236

## E0308: name contains `__`
- **Kind:** compile error · **Since:** planned for v0.6, not in the compiler yet
- **What it means:** An identifier contains two underscores in a row. The compiler also reports this code when two generated names collide (an internal safety net).
- **Why Nyra has this rule:** `__` joins a module name and an item name in the generated code, so user names must never contain it.
- **Common causes:**
  - a name such as `my__helper`
  - Python-style dunder names
- **Wrong:**
```rust
fn my__helper() -> int = 1

fn main() {
    print(my__helper())
}
```
- **Fixed:**
```rust
fn my_helper() -> int = 1

fn main() {
    print(my_helper())
}
```
- **Related:** E0206

## E0310: not available on this target
- **Kind:** compile error · **Since:** planned for v0.6, not in the compiler yet
- **What it means:** The program reaches a function that has no implementation for the chosen target, for example a function declared only with `extern c` when the program is compiled to JavaScript (`--js`), or `fs.read` on a target without files. The message names the target, the targets that do have the function and the chain of calls that reaches it.
- **Why Nyra has this rule:** Nyra refuses to compile code that cannot behave the same way everywhere, instead of failing at run time on one backend. Only code that `main` can reach is checked.
- **Common causes:**
  - a function that has an `extern c` binding but no `extern js` one, compiled with `--js`
  - using files, sleeping or other host features on a target that does not have them
- **Wrong:**
```rust
extern c "math.h" fn cbrt(x: float) -> float

fn main() {
    print(cbrt(27.0))
}
```
- **Fixed:**
```rust
extern c "math.h" fn cbrt(x: float) -> float
extern js fn cbrt(x: float) -> float = "Math.cbrt"

fn main() {
    print(cbrt(27.0))
}
```
- **Related:** E0311, E0314

## E0311: unknown extern host
- **Kind:** compile error · **Since:** planned for v0.6, not in the compiler yet
- **What it means:** An `extern` declaration names a host that does not exist. The hosts are `c`, `js`, `node` and `browser`, written without quotes.
- **Why Nyra has this rule:** A foreign function must say exactly where it exists. Hosts are plain words, not strings, to keep the syntax short.
- **Common causes:**
  - `extern "C" fn` written as in Rust
  - a typo in the host name
- **Wrong:**
```rust
extern "C" fn abs(x: int) -> int

fn main() {
    print(abs(-3))
}
```
- **Fixed:**
```rust
extern c fn abs(x: int) -> int

fn main() {
    print(abs(-3))
}
```
- **Related:** E0310, E0313

## E0312: type cannot cross the FFI boundary
- **Kind:** compile error · **Since:** planned for v0.6, not in the compiler yet
- **What it means:** An `extern` function uses a type that cannot be passed to foreign code. Only `int`, `float`, `bool`, `str` and `ptr` can cross; arrays, structs, `char` and `inout` parameters cannot.
- **Why Nyra has this rule:** Foreign code does not know Nyra's memory layout. Wrap arrays and structs in a small C or JavaScript function that takes plain values.
- **Common causes:**
  - an array parameter or result
  - a struct passed by value
  - a `char`: pass its code, `c.code()`, as an `int`
- **Wrong:**
```rust
extern c fn sum(xs: [int]) -> int

fn main() {
    print(sum([1, 2]))
}
```
- **Fixed:**
```rust
extern c fn sum2(a: int, b: int) -> int

fn main() {
    print(sum2(1, 2))
}
```
- **Related:** E0311, E0316

## E0313: extern function has a body
- **Kind:** compile error · **Since:** planned for v0.6, not in the compiler yet
- **What it means:** An `extern` function is declared with a `{ }` body or `= expression`. A foreign function is only a declaration; its code lives in C or JavaScript.
- **Why Nyra has this rule:** One definition per function. The foreign name, if it differs, goes after `=` as a string.
- **Common causes:**
  - writing a Nyra implementation under an `extern`
  - confusing `=` (foreign name) with `= expression`
- **Wrong:**
```rust
extern c fn twice(x: int) -> int {
    ret x * 2
}
```
- **Fixed:**
```rust
extern c fn twice(x: int) -> int = "twice"
```
- **Related:** E0311, E0314

## E0314: conflicting extern declarations
- **Kind:** compile error · **Since:** planned for v0.6, not in the compiler yet
- **What it means:** The same function name is declared for several hosts with different signatures.
- **Why Nyra has this rule:** A portable wrapper is one signature with one declaration per host, so callers see a single function.
- **Common causes:**
  - one declaration for C and one for JavaScript that differ in a parameter type
  - a copy-and-edit mistake
- **Wrong:**
```rust
extern c fn sqrt(x: float) -> float
extern js fn sqrt(x: int) -> float = "Math.sqrt"
```
- **Fixed:**
```rust
extern c fn sqrt(x: float) -> float
extern js fn sqrt(x: float) -> float = "Math.sqrt"
```
- **Related:** E0310, E0313

## E0315: invalid foreign name or header
- **Kind:** compile error · **Since:** planned for v0.6, not in the compiler yet
- **What it means:** The foreign name after `=` or the header in an `extern` is not valid: a C identifier (not a C keyword) or a JavaScript dotted path is required, and a header may not contain `..`.
- **Why Nyra has this rule:** The name is copied into generated code, so it must be a plain identifier or path and nothing else.
- **Common causes:**
  - a name with spaces or punctuation
  - a C keyword such as `int` as the foreign name
- **Wrong:**
```rust
extern c fn f(x: int) -> int = "a b"
```
- **Fixed:**
```rust
extern c fn f(x: int) -> int = "abs"
```
- **Related:** E0313, E0316

## E0316: `nyrt_` bindings are reserved
- **Kind:** compile error · **Since:** planned for v0.6, not in the compiler yet
- **What it means:** An `extern` binding names a symbol that starts with `nyrt_`. These names belong to the Nyra runtime.
- **Why Nyra has this rule:** Generated code and the runtime use the `nyrt_` prefix, so foreign code must not collide with it.
- **Common causes:**
  - a C function that happens to start with `nyrt_`
- **Wrong:**
```rust
extern c fn f(x: int) -> int = "nyrt_print"
```
- **Fixed:**
```rust
extern c fn f(x: int) -> int = "my_print"
```
- **Related:** E0312, E0315

## E0320: malformed `nyra.toml`
- **Kind:** compile error · **Since:** planned for v0.6, not in the compiler yet
- **What it means:** The project file `nyra.toml` has a syntax error. It uses a small subset of TOML: `[section]` headers, `key = "text"`, `key = 123`, `key = true` and `key = ["a", "b"]` on one line, and `#` comments.
- **Why Nyra has this rule:** A tiny, strict manifest format is simple to write by hand and to check. The message gives the line number.
- **Common causes:**
  - a missing `=`
  - a value split over several lines
  - an unquoted text value
- **Wrong:**
```toml
[package]
name "inventory"
```
- **Fixed:**
```toml
[package]
name = "inventory"
```
- **Related:** E0321, E0324

## E0321: invalid package manifest
- **Kind:** compile error · **Since:** planned for v0.6, not in the compiler yet
- **What it means:** `nyra.toml` parses, but its content is wrong: the `[package]` section has no `name`, or the name contains characters other than lowercase letters, digits and `_`.
- **Why Nyra has this rule:** A package name becomes an identifier in code, so it must be a valid one.
- **Common causes:**
  - a name with spaces or capitals (`My App`)
  - a missing `[package]` section
- **Wrong:**
```toml
[package]
name = "My App"
```
- **Fixed:**
```toml
[package]
name = "my_app"
```
- **Related:** E0320, E0323

## E0322: cannot fetch dependency
- **Kind:** compile error · **Since:** planned for v0.6, not in the compiler yet
- **What it means:** A dependency from `nyra.toml` could not be downloaded or found. Remote dependencies are fetched with `git`; local ones are `path:` directories. The message shows the first line of git's own error.
- **Why Nyra has this rule:** Builds must never silently continue without a dependency.
- **Common causes:**
  - a wrong URL, tag or branch
  - `git` is not installed
  - a `path:` that does not exist
- **Wrong:**
```toml
[dependencies]
json = "https://example.invalid/nyra-json#v1.2.0"
```
- **Fixed:**
```toml
[dependencies]
json = "https://github.com/someone/nyra-json#v1.2.0"
```
- **Related:** E0323, E0324

## E0323: dependency name clash
- **Kind:** compile error · **Since:** planned for v0.6, not in the compiler yet
- **What it means:** A dependency has the name of a standard module, or two dependencies need different versions (different refs) of the same package.
- **Why Nyra has this rule:** A name means one package in a program: there is one version of each, and `use math` always means the standard `math`.
- **Common causes:**
  - naming a dependency `math` or `text`
  - two packages that pin different tags of a third
- **Wrong:**
```toml
[dependencies]
math = "https://github.com/someone/nyra-math"
```
- **Fixed:**
```toml
[dependencies]
fastmath = "https://github.com/someone/nyra-math"
```
- **Related:** E0321, E0322

## E0324: invalid `nyra.lock`
- **Kind:** compile error · **Since:** planned for v0.6, not in the compiler yet
- **What it means:** `nyra.lock` is malformed or does not match `nyra.toml`, for example it lists a dependency that was removed or pins a different source.
- **Why Nyra has this rule:** The lock file pins exact commits so builds are reproducible. A lock that disagrees with the manifest cannot be trusted.
- **Common causes:**
  - editing `nyra.toml` by hand without updating the lock
  - a half-merged lock file
- **Wrong:**
```toml
# nyra.toml no longer lists `json`, but nyra.lock still contains:
[json]
source = "https://github.com/someone/nyra-json"
```
- **Fixed:**
```text
delete nyra.lock: the next build recreates it
```
- **Related:** E0320, E0322

## E0325: package has no library entry
- **Kind:** compile error · **Since:** planned for v0.6, not in the compiler yet
- **What it means:** A dependency is imported with `use name`, but the package has no `src/lib.nyra`, the file that a library exposes to the programs that use it.
- **Why Nyra has this rule:** `use json` has to load something definite: `src/lib.nyra` is the entry point of a library.
- **Common causes:**
  - the package is an application with only `src/main.nyra`
  - the file is in the wrong place or named differently
- **Wrong:**
```text
json/
  nyra.toml
  src/main.nyra
```
- **Fixed:**
```text
json/
  nyra.toml
  src/lib.nyra
```
- **Related:** E0322, E0321

## E0330: `never` outside a return type
- **Kind:** compile error · **Since:** planned for v0.6, not in the compiler yet
- **What it means:** `never` is used as the type of a parameter or variable. It only exists as a return type, for functions that do not come back.
- **Why Nyra has this rule:** A value of type `never` can never exist, so only the end of a function (such as `process.exit`) can be `never`.
- **Common causes:**
  - `let x: never = ...`
  - `never` as a parameter type
- **Wrong:**
```rust
fn stop(x: never) {
}
```
- **Fixed:**
```rust
use process

fn stop() -> never {
    process.exit(1)
}
```
- **Related:** E0207, E0102

## E0331: `const` needs a literal value
- **Kind:** compile error · **Since:** planned for v0.6, not in the compiler yet
- **What it means:** A top-level `const` has an initial value that is not a literal. Only an integer, float, `bool`, `char` or plain string literal (with an optional leading `-`) is allowed, and the type must be written.
- **Why Nyra has this rule:** Constants are fixed when the program is compiled, with no code to run and nothing to initialise in a particular order.
- **Common causes:**
  - a computed value such as `2 * 50`
  - a call to a function
  - a string with `{ }` interpolation
- **Wrong:**
```rust
const LIMIT: int = 2 * 50
```
- **Fixed:**
```rust
const LIMIT: int = 100
```
- **Related:** E0332, E0205

## E0332: `pub` not allowed here
- **Kind:** compile error · **Since:** planned for v0.6, not in the compiler yet
- **What it means:** `pub` is written where it has no meaning: before `use`, before `let`, or inside a function. It goes before `fn`, `const`, `extern` or `struct`.
- **Why Nyra has this rule:** Only definitions can be exported. A module cannot re-export an import: write a small wrapper function instead.
- **Common causes:**
  - `pub use` to re-export a module
  - `pub let` for a global variable (there are none)
- **Wrong:**
```rust
pub use math
```
- **Fixed:**
```rust
use math

pub fn root(x: float) -> float = math.sqrt(x)
```
- **Related:** E0301, E0302

## E0340: file operation failed
- **Kind:** runtime error · **Since:** planned for v0.6, not in the compiler yet
- **What it means:** A function of the `fs` module could not do its job, for example `fs.read: cannot read "x.txt" (not found)`. The reason is one of `not found`, `permission denied`, `is a directory`, `not valid UTF-8` or `io error`. The program stops with exit code 101 and the error points at your call.
- **Why Nyra has this rule:** Nyra has no exceptions and no null, so a call that cannot succeed stops the program with a clear message. Where recovery is plausible there is a probe that never fails, such as `fs.exists`, so a program can check first.
- **Common causes:**
  - a wrong path: paths are relative to the folder the program runs in and use `/` on every system
  - a file or folder without the needed permission
  - reading a folder as if it were a file
  - a file that is not UTF-8 text
- **Wrong:**
```rust
use fs

fn main() {
    print(fs.read("missing.txt"))
}
```
- **Fixed:**
```rust
use fs

fn main() {
    if fs.exists("missing.txt") {
        print(fs.read("missing.txt"))
    } else {
        print("no such file")
    }
}
```
- **Related:** E0341, E0310

## E0341: input is not valid UTF-8
- **Kind:** runtime error · **Since:** planned for v0.6, not in the compiler yet
- **What it means:** Text that enters the program (standard input, files, command-line arguments, environment variables) is not valid UTF-8, for example `io.read_line: input is not valid UTF-8`.
- **Why Nyra has this rule:** A `str` is UTF-8 text on every backend. Both backends check incoming text with the same rule, so a program behaves identically and never holds broken text.
- **Common causes:**
  - binary data piped into the program
  - a text file in an old encoding such as Latin-1 or UTF-16
- **Wrong:**
```rust
use io

fn main() {
    // run it as:  printf '\377\n' | nyra run main.nyra
    print(io.read_line())
}
```
- **Fixed:**
```rust
use io

fn main() {
    // run it as:  printf 'ok\n' | nyra run main.nyra
    print(io.read_line())
}
```
- **Related:** E0340, E0344

## E0342: bad argument value for a standard function
- **Kind:** runtime error · **Since:** planned for v0.6, not in the compiler yet
- **What it means:** A standard library function received an argument that is valid in type but has no sensible result, such as `random.range(5, 5): need lo < hi and hi - lo <= 2^53` or `text.fixed: digits must be 0 to 100`.
- **Why Nyra has this rule:** An empty range or a negative number of digits has no answer, and hosts disagree about what to return. A clear error beats a value that differs by backend.
- **Common causes:**
  - `random.range(lo, hi)` with `hi <= lo`: the upper bound is excluded, so `range(1, 7)` rolls a die
  - `text.fixed(x, d)` with a negative or very large `d`
- **Wrong:**
```rust
use random

fn main() {
    print(random.range(5, 5))
}
```
- **Fixed:**
```rust
use random

fn main() {
    print(random.range(5, 6))
}
```
- **Related:** E0243, E0344

## E0343: foreign function failed
- **Kind:** runtime error · **Since:** planned for v0.6, not in the compiler yet
- **What it means:** A call to an `extern` function failed. The message starts with `ffi:`: the message of a JavaScript exception that escaped the foreign call, `ffi: cbrt returned NULL for a str`, or `ffi: a string with a NUL byte cannot be passed to C`.
- **Why Nyra has this rule:** Foreign code is trusted but can still fail, and Nyra has no `catch`. The failure stops the program with exit code 101 and the original message, the same on every backend.
- **Common causes:**
  - a JavaScript function that throws for the given input
  - a C function that returns NULL where a `str` is expected
  - a string containing a NUL byte passed to a C function
- **Wrong:**
```rust
extern js fn parse(s: str) -> str = "JSON.parse"

fn main() {
    print(parse("not json"))
}
```
- **Fixed:**
```rust
extern js fn parse(s: str) -> str = "JSON.parse"

fn main() {
    print(parse("\"ok\""))
}
```
- **Related:** E0312, E0340

## E0344: no source of randomness
- **Kind:** runtime error · **Since:** planned for v0.6, not in the compiler yet
- **What it means:** `random.random()` or `random.range(lo, hi)` could not get random numbers from the operating system, so the program stops with `random: no randomness source available`.
- **Why Nyra has this rule:** `random` is truly random unless the program seeds it. Nyra never falls back to the clock silently, because those numbers would be predictable. A program that wants a fixed sequence says so with `random.seed(n)`.
- **Common causes:**
  - a container or chroot without `/dev/urandom`
  - an operating system service that is blocked or missing
- **Wrong:**
```rust
use random

fn main() {
    // in an environment without an operating system random source
    print(random.random())
}
```
- **Fixed:**
```rust
use random

fn main() {
    random.seed(42)
    print(random.random())
}
```
- **Related:** E0342, E0341
