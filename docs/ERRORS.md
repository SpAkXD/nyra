# Nyra error database

Every error the Nyra compiler can report, one entry per code: what it means, why the rule exists, the usual
causes, a wrong program that produces the code and a fixed one. Read an entry with

```
nyra explain E0201            # the entry, for humans
nyra explain E0201 --json     # the same entry as JSON, for tools and AI agents
nyra explain                  # every code the compiler reports, with its title
nyra explain --planned        # the same, with the planned codes of future designs
```

Errors from `nyra check file.nyra --json` carry the code (`"code":"E0201"`) and a `hint` that usually contains the
fix already; this file explains the rule behind it. When the repair is certain (`elif` for `else if`, a `;`, `'text'`, ...)
the error also carries it as a `fix` of text edits, and `nyra check --fix` applies it. The programs under **Wrong** and **Fixed** are tested: for every
code the compiler can emit, the wrong program produces exactly that code and the fixed program compiles and runs.
Codes marked *planned* are described in the design for a future version; the compiler does not emit them yet and the
design may still change.

## Code ranges

| Range | Stage | What goes wrong |
|---|---|---|
| E0001-E0005 | lexer | characters, numbers and strings |
| E0007 | lexer | character literals |
| E0101-E0103 | parser | grammar, type names, nesting depth |
| E0201-E0218 | type checker | names, types, `return`, conditions, lambdas, script variables, map keys |
| E0220-E0239 | type checker | structs, arrays, strings, methods, `inout`, `free` / `keep` / `arena` |
| E0240-E0249, E0255-E0256 | run time | the program stops with exit code 101 |
| E0260-E0269 | type checker, parser (since v0.6) | warnings (E0260) and mistakes taken from other languages: negative positions, optional types, methods in structs, classes |
| E0250-E0254 | examples | an `ex` example is false, stops with a runtime error, is not a `bool`, does not finish or calls a function that uses script variables; checked while compiling |
| E0290-E0293 | capabilities and properties (since v0.6) | a `use` of a module the run does not grant; a malformed or too large `ex for` property example |
| E0300-E0316 | standard modules (since v0.5); files, FFI (planned) | `use`, module items, `json.parse`; `pub`, `extern`, targets |
| E0320-E0325 | packages (planned, v0.6) | `nyra.toml`, dependencies, `nyra.lock` |
| E0330-E0332 | declarations (planned, v0.6) | `never`, `const`, `pub` |
| E0340-E0345 | run time (since v0.5; E0343-E0344 planned) | standard library and foreign function failures |
| E0355-E0359 | run time, interpreter (since v0.6) | a run in the interpreter hit its limit of steps, memory, output, call depth or time; exit codes 120 to 124 |

A code marked **warning** does not stop the build: the compiler prints it to stderr and `--json` lists it under `"warnings"`.

Codes are stable: a number is never reused for another error. E0006 (a bad brace in a string) is retired: since v0.5 a
brace that starts no `{value}` is text. Numbers that are not listed (E0219, E0257-E0259,
E0294-E0299, E0317-E0319, E0326-E0329, E0333-E0339, E0346-E0354, E0360-E0369) are kept free for future errors of the same kind. E0900-E0919 are set aside
for the intermediate representation and the WebAssembly backend (v0.5), which needs no codes of its own so far.

## Entry format

Each entry is a heading `## E0xxx: title` followed by these fields, in this order. The test suite checks the format, and
`nyra explain` reads it, so keep it exactly like this when you add a code (see CONTRIBUTING.md).

```text
## E0201: undefined variable
- **Kind:** compile error · **Since:** v0.1            (runtime error · compile error · warning; or "planned for v0.6, not in the compiler yet")
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
- **What it means:** The source contains a character that cannot start any Nyra token. Nyra's alphabet is small: letters, digits, `_`, strings in double quotes, characters in single quotes (`'a'`), `//` comments and the symbols `( ) { } [ ] , : ? . + - * / % = < > !` plus the two-character forms `-> .. == != <= >= && || += -= *= /= %=`. A `.` is only valid inside a float (`2.5`), in a range (`0..10`) or between a value and a field or method name (`p.x`, `s.len()`), and `&` and `|` only in pairs.
- **Why Nyra has this rule:** A closed alphabet keeps every program unambiguous. Characters that mean something in other languages (`#`, `$`, `@`) are not silently ignored or reinterpreted: you get a precise error at the exact position.
- **Common causes:**
  - a `#` comment: Nyra comments start with `//`
  - backticks or typographic quotes (“ ” ‘ ’): text uses straight double quotes (single quotes hold one character, `'a'`)
  - a single `&` or `|`: write `&&` or `||`
  - `.5` or `5.`: a float needs digits on both sides of the dot (`0.5`, `5.0`)
  - `$`, `@`, `^`, `~` or a backslash outside a string or a character
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
- **Related:** E0004

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
- **What it means:** A backslash inside a string is followed by a character that is not an escape. The escapes are `\n` (new line), `\t` (tab), `\r` (carriage return), `\\` (a backslash) and `\"` (a quote). A character literal such as `'\n'` has the same escapes, plus `\'`, and reports the same error ("unknown escape `\q` in a character").
- **Why Nyra has this rule:** A closed set of escapes means a backslash never silently disappears: `"C:\Users"` would otherwise print `C:Users`. Strings are UTF-8, so any character can be typed directly instead of using `\u` or `\x` codes.
- **Common causes:**
  - a Windows path: write every backslash twice (`"C:\\Users"`)
  - `\u00e9` or `\x41`: type the character itself, as in `"é"`
  - `\{` or `\}`: a brace needs no escape (`"{"` is text; `{{` also prints `{`)
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
- **Related:** E0002

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
- **Why Nyra has this rule:** The grammar is small and strict on purpose: `return` is the only way to return (`ret` is accepted as a short spelling), braces are always required and `{` stays on the line of its `fn`, `if`, `else`, `while` or `for`, and there is one statement per line. So every program has exactly one spelling, and a model that knows another language is corrected at the first deviation.
- **Common causes:**
  - `elif`, `elseif`, `and`, `or`, `not`, `function`, `def`: Nyra spells them `else if`, `&&`, `||`, `!`, `fn`
  - `i++`, `i--`, `2 ** 3`, `0..=9`, `a === b`: these operators do not exist
  - a lambda written as in another language, `lambda x: x * 2`, `|x| x * 2` or `x -> x * 2`: write `x => x * 2`
  - `{` on a line of its own, or a missing `{` or `}`: put `{` on the same line, and close every block
  - two statements on one line (`let a = 1 let b = 2`) or a line that starts with an operator
  - a missing piece: `let x` without `= value`, `fn f(a)` without a type, `for i 0..3` without `in`
  - a conditional value `c ? a : b` that lacks its `:` part, or a `?` anywhere else
  - an `import` or a `class` at the top level of the file
  - a struct written with braces, `Point { x: 1, y: 2 }`: a struct is built like a call, `Point(x: 1, y: 2)`
  - `for (i, x) in xs`: write the two variables without parentheses, `for i, x in xs`
  - `break` or `continue` outside a loop: to leave a function write `return`
  - `0xFF`, `1_000` and `1e5` number forms: write `255`, `1000`, `100000.0`
  - `=` where `==` was meant, as in `if x = 1 {`
  - a format specifier inside a string, as in `"{x:.2f}"`
- **Wrong:**
```rust
fn sign(x: int) -> int {
    if x > 0 {
        return 1
    } elif x < 0 {
        return -1
    }
    return 0
}

fn main() {
    print(sign(4))
}
```
- **Fixed:**
```rust
fn sign(x: int) -> int {
    if x > 0 {
        return 1
    } else if x < 0 {
        return -1
    }
    return 0
}

fn main() {
    print(sign(4))
}
```
- **Related:** E0102, E0212, E0001

## E0102: unknown type
- **Kind:** compile error · **Since:** v0.1
- **What it means:** A type annotation names a type that does not exist. The types are `int` (64-bit integer), `float` (64-bit float), `bool`, `str` (text), `char` (one character), arrays written `[T]` (`[int]`, `[[str]]`) and the structs the program declares. The message for a struct name that is not declared is "unknown type `Vec2`: no struct with this name is defined".
- **Why Nyra has this rule:** Each type has one name, and there are no aliases: a model never has to guess whether the text type is `str`, `string` or `String`.
- **Common causes:**
  - another language's name: `string`, `String`, `i32`, `i64`, `double`, `number`, `boolean`
  - `void` for "returns nothing": leave out the `->` part of the signature
  - maps, sets, tuples and other collection types, which do not exist yet: use an array `[T]` or a struct
  - a typo (`flot`) or a capital letter (`Int`): the built-in type names are lowercase
  - a struct that is not declared, or written differently from its declaration (`Pointt`, `point` for `Point`): the hint suggests the closest declared struct
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

## E0103: code nested too deeply
- **Kind:** compile error · **Since:** v0.5
- **What it means:** An expression or a block is nested more than 256 levels deep. Every pair of parentheses, call, `[ ]`, unary `-` or `!`, block, `else if` link, and every operator, `.method()`, `.field` or `[index]` of a chain adds a level, so `1 + 1 + ... + 1` with 260 terms is too deep as well. The parser stops at the first such place and reports only this error.
- **Why Nyra has this rule:** The compiler works on programs as trees, and every stage walks them recursively. A fixed limit, far above what a person or a model writes, means no input can make the compiler (or `nyra mcp`, which serves many requests) run out of stack: it gets a normal error instead.
- **Common causes:**
  - generated code: a long sum or string concatenation built term by term, or thousands of nested parentheses
  - a long `else if` chain (more than 250 branches): use a map or an array lookup instead
  - deeply nested `if` blocks: return early, or move the inner part into a function
- **Wrong:**
```rust
fn main() {
    print(1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1)
}
```
- **Fixed:**
```rust
fn main() {
    var total = 0
    for i in 0..260 {
        total += 1
    }
    print(total)
}
```
- **Related:** E0101

## E0201: undefined variable
- **Kind:** compile error · **Since:** v0.1
- **What it means:** A name is used as a value, but no variable, parameter or loop variable with that name is visible at that point.
- **Why Nyra has this rule:** Every variable is declared explicitly with `let` or `var`, and names are never created by assignment. A typo can therefore never introduce a new variable silently; it is reported with a "did you mean" hint.
- **Common causes:**
  - a typo in the name (the hint suggests the closest one)
  - the variable is declared later in the function: move its `let` above the use
  - the variable was declared inside an inner `{ }` block and is used after the block ended: declare it before the block
  - a function used without call parentheses: write `limit()`, not `limit`
  - a function that uses a local variable of `fn main`: functions see only the variables declared at the top level of the file (script variables), so declare it there, or pass the value as a parameter (the hint says which)
  - assigning to a variable that was never declared (`count = 1`): declare it first with `var count = 0`
  - words from other languages: `null`, `None`, `self`, `True`
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
- **What it means:** A call `name(...)` refers to a function that is not defined in the file and is not one of the builtins `print`, `int`, `float`, `str`, `char`, `free` and `keep`, and not a struct either.
- **Why Nyra has this rule:** There is no standard library yet, so every helper is written in the program itself. A missing function is reported, with the recipe for common ones, instead of guessing what `sqrt` or `pow` should do.
- **Common causes:**
  - a library function from another language: `pow`, `sqrt`, `floor`, `input` (`abs`, `min` and `max` are builtins)
  - `len(xs)`: the length is a method, `xs.len()`
  - `println`, `printf` or `echo`: the output function is `print(x)`
  - a typo in a function name (the hint suggests the closest one)
  - calling a variable as if it were a function
  - a struct that is not declared (`Vec2(x: 1.0, y: 2.0)` without `struct Vec2`: the hint shows the declaration to write), or a struct called by another spelling (`point(...)` for `Point`)
- **Wrong:**
```rust
fn main() {
    print(pow(2, 10))
}
```
- **Fixed:**
```rust
fn pow(b: int, e: int) -> int {
    var r = 1
    for i in 0..e {
        r *= b
    }
    return r
}

fn main() {
    print(pow(2, 10))
}
```
- **Related:** E0201, E0204

## E0203: type mismatch
- **Kind:** compile error · **Since:** v0.1
- **What it means:** A value has one type where another is required: the initial value of a `let` with a type annotation, an assignment, an argument of a function or method call, a field of a struct construction, a `return` value, the bounds of a `for` range, or the argument of `int()`, `float()` or `char()`. A call to a function that returns nothing cannot be used as a value either.
- **Why Nyra has this rule:** Nothing converts implicitly. Turning an `int` into a `float` (or back) changes the result, so you write it: `float(n)` and `int(x)` (which truncates toward zero). That makes every numeric conversion visible in the code.
- **Common causes:**
  - an `int` where a `float` is needed: write `2.0` for a literal, `float(n)` for a variable
  - a `float` where an `int` is needed, such as a range bound: `int(x)`
  - text where a number is needed (`let n: int = "3"`): write the number, or parse the text with `int(s)` (a runtime error if it is not a number)
  - a `char` where an `int` is needed: its code is `c.code()`; an `int` where a `char` is needed: `char(n)`
  - a number where text is needed: `str(n)` or interpolation, `"{n}"`
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
- **What it means:** A call passes a different number of arguments than the function declares. `print`, `int`, `float`, `str`, `char`, `free` and `keep` take exactly one argument, and a method takes the arguments it needs: `xs.push(1, 2)` reports "`.push()` takes 1 argument but 2 were given".
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
- **What it means:** Something that cannot change is assigned to (`=`, `+=`, `-=`, `*=`, `/=` or `%=`): a variable declared with `let`, a function parameter, or the variable of a `for` loop. The same holds for changing what is inside such a variable: a field or an element (`p.x = 1`, `xs[0] = 1`), a method that changes its receiver (`xs.push(1)`) or an `inout` argument.
- **Why Nyra has this rule:** Values are immutable unless declared with `var`, so reading a function shows exactly where something can change. Parameters and loop variables never change, which keeps loops and calls easy to reason about.
- **Common causes:**
  - `let` was used where `var` was needed: change the declaration
  - a compound assignment such as `count += 1` on a `let` variable
  - `xs.push(...)`, `xs[0] = ...` or `p.x = ...` where the variable was declared with `let`: declare it with `var`
  - modifying a parameter (`n = n / 2`, `xs.push(1)`): copy it first with `var m = n`, or declare the parameter `inout`
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
- **What it means:** A name is declared where Nyra does not allow it: it hides a variable or parameter that is still visible (shadowing), a function is defined twice, a variable has the name of a function, a function or variable is named like a builtin (`print`, `int`, `float`, `str`, `char`, `free`, `keep`), or a struct name is used for a second struct, a function or a variable.
- **Why Nyra has this rule:** One name means one thing. Without shadowing, a model never has to work out which of two `x` is meant, and a rename can never change the meaning of a program. There is no overloading either: every function name is used once.
- **Common causes:**
  - `let x = ...` twice in one function (also in an inner `{ }` block while the outer `x` is visible)
  - a local variable with the name of a parameter
  - a nested `for i in ...` inside another `for i in ...`: name the inner variable `j`
  - two functions with the same name, for example two `max` with different parameter types
  - a variable named like a function that exists (`let add = 1` while `fn add` exists)
  - a parameter or variable named `print`, `int`, `float`, `str`, `char`, `free` or `keep`
  - a variable or function named like a struct (`let Point = 1` while `struct Point` exists), or two structs with one name
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

## E0207: bad or missing `return`
- **Kind:** compile error · **Since:** v0.1
- **What it means:** The use of `return` (or its short spelling `ret`) does not match the function's signature. A function declared `-> T` must end every path with `return value`; a function without `->` cannot return a value; and `return` without a value is only valid in a function that returns nothing.
- **Why Nyra has this rule:** Returning is always explicit, so the end of every path is visible and there is no implicit "last expression is the result" in block functions. The one-line form `fn f(x: int) -> int = x * 2` needs no `return`.
- **Common causes:**
  - the last `if` has no `else` and no `return` follows it: add a final `return` or an `else { return ... }`
  - the last line is a value (`a + b`) written Rust-style without `return`
  - a loop that may run zero times is the last statement: add a `return` after it
  - `return 1` in a function whose signature has no `-> int`
  - `return` with no value in a function that returns a value
- **Wrong:**
```rust
fn sign(x: int) -> int {
    if x > 0 { return 1 }
    if x < 0 { return -1 }
}

fn main() {
    print(sign(5))
}
```
- **Fixed:**
```rust
fn sign(x: int) -> int {
    if x > 0 { return 1 }
    if x < 0 { return -1 }
    return 0
}

fn main() {
    print(sign(5))
}
```
- **Related:** E0203, E0208

## E0208: missing `fn main()`
- **Kind:** compile error · **Since:** v0.1
- **What it means:** The file defines no function called `main`. A program starts running at `fn main()`.
- **Why Nyra has this rule:** One entry point with one shape means every program starts the same way on every backend. A program without `fn main` is a script: its top-level statements are its `main`.
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
- **What it means:** An operator received operands it does not accept. `+ - * /` need two `int`s or two `float`s (and `+` also joins two `str`s or two arrays of one type); `< <= > >=` need two `int`s, `float`s, `str`s or `char`s; `%` needs two `int`s; `==` and `!=` need two values of the same type (arrays and structs are compared by content); `&&`, `||` and `!` need `bool`s; unary `-` needs a number.
- **Why Nyra has this rule:** Operators never convert their operands and cannot be overloaded, so `1 + 2.0` is an error rather than a guess, and `"a" + n` is an error too: `+` joins two strings, so write `"a" + str(n)` or use interpolation, `"a{n}"`.
- **Common causes:**
  - an `int` and a `float` in one expression (`1 + 2.0`, `n * 0.5`): convert with `float(n)`
  - compound assignment with the wrong type, such as `x += 1.5` when `x` is an `int`
  - `"total: " + n`: use interpolation, `"total: {n}"`
  - a `char` joined to a string (`s + c`: write `s + str(c)`) or used in arithmetic (`c + 1`: write `char(c.code() + 1)`)
  - one element added to an array (`xs + 5`): write `xs + [5]` or `xs.push(5)`
  - a chained comparison `a < b < c`: write `a < b && b < c`
  - `%` on floats, `<` on arrays or structs, `!` on an `int`, `-` on a `bool`
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
- **Why Nyra has this rule:** A program has no command-line arguments, input or exit code yet, and `main` behaves identically on every backend. To stop early, write `return` without a value.
- **Common causes:**
  - `fn main() -> int` copied from C or Rust
  - `fn main(args: ...)`: programs are closed, so put the values in the program (`let n = 12`)
  - wanting to return an exit code: `main` always ends normally
- **Wrong:**
```rust
fn main() -> int {
    return 0
}
```
- **Fixed:**
```rust
fn main() {
    print("done")
}
```
- **Related:** E0208, E0207

## E0212: bad `if` or `? :` used as a value
- **Kind:** compile error · **Since:** v0.2
- **What it means:** An `if` that is used as a value (`let x = if c { a } else { b }`, also written `let x = c ? a : b`) is incomplete or inconsistent: it has no `else`, a branch is not exactly one expression (an empty branch, a statement, several lines), a branch produces no value, or the two branches have different types.
- **Why Nyra has this rule:** A value must exist on every path and have one type, so the compiler can give it that type. Branches that do things belong in an `if` statement, which has no value.
- **Common causes:**
  - `let x = if c { 1 }` without `else` (the `? :` form always has both parts)
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

## E0213: lambda used as a value
- **Kind:** compile error · **Since:** v0.5
- **What it means:** A lambda (`x => x * 2`) appears somewhere other than as the argument of an array method that takes one: `map`, `filter`, `count`, `any`, `all`, `find_index`, `sort_by` or `fold`. It was stored in a variable, passed to a function of the program, returned or printed.
- **Why Nyra has this rule:** Nyra has no function values. A lambda is compiled into the loop of the method it belongs to, so it costs nothing at run time and can only exist in that place.
- **Common causes:**
  - `let double = x => x * 2`, as a JavaScript arrow function or a Python `lambda` would be stored
  - passing a lambda to a function you wrote, `apply(xs, x => x + 1)`
- **Wrong:**
```rust
fn main() {
    let double = x => x * 2
    print([1, 2, 3].map(double))
}
```
- **Fixed:**
```rust
fn double(x: int) -> int = x * 2

fn main() {
    print([1, 2, 3].map(x => double(x)))
}
```
- **Related:** E0215, E0101

## E0214: a lambda cannot change variables
- **Kind:** compile error · **Since:** v0.5
- **What it means:** Something inside a lambda (or inside the element or condition of a comprehension) would change a variable: a method that changes its receiver (`push`, `pop`, `sort`, ...) or an `inout` argument. Inside a lambda every variable is read-only.
- **Why Nyra has this rule:** A chain such as `xs.filter(...).map(...).sum()` runs as one loop, element by element. If a lambda could change variables, the result would depend on that order and on how often each lambda runs; read-only lambdas give the same answer however the chain is written, and the result says everything the call does.
- **Common causes:**
  - collecting into another array from inside `map`, `ys.push(x)`: use the array that `map` or `filter` returns
  - counting with a variable from inside a lambda: use `count`, `sum` or `fold`
  - calling a function that changes a script variable from inside a lambda: call it in a `for` loop
- **Wrong:**
```rust
fn main() {
    let xs = [1, 2, 3]
    var big: [int] = []
    let n = xs.count(x => big.pop() > x)
    print(n)
}
```
- **Fixed:**
```rust
fn main() {
    let xs = [1, 2, 3]
    let big = xs.filter(x => x > 1)
    print(big, big.len())
}
```
- **Related:** E0205, E0229

## E0215: bad lambda argument
- **Kind:** compile error · **Since:** v0.5
- **What it means:** A method that takes a lambda got something else, or a lambda of the wrong shape: a value or a function name instead of a lambda, the wrong number of parameters (`fold` takes two, the others one), or a body that returns nothing.
- **Why Nyra has this rule:** The method calls the lambda once per element with fixed parameters, and uses its value: a test (`filter`, `count`, `any`, `all`, `find_index`), a new element (`map`), a key (`sort_by`) or the next value (`fold`).
- **Common causes:**
  - `xs.count(3)` as in Python: count with a test, `xs.count(x => x == 3)`
  - a function name, `xs.map(double)`: call it in a lambda, `xs.map(x => double(x))`
  - `xs.fold(0, x => ...)` with one parameter: `fold` passes the value so far and the element, `(acc, x) => acc + x`
  - `xs.map(x => print(x))`: to do something for each element write a `for` loop
- **Wrong:**
```rust
fn main() {
    let xs = [1, 3, 3]
    print(xs.count(3))
}
```
- **Fixed:**
```rust
fn main() {
    let xs = [1, 3, 3]
    print(xs.count(x => x == 3))
}
```
- **Related:** E0213, E0204, E0203

## E0216: a function uses a script variable and declares the same name
- **Kind:** compile error · **Since:** v0.5
- **What it means:** A function uses a script variable (a `let` or `var` at the top level of a script) and also declares a parameter, variable, loop variable or lambda parameter of the same name. A function that declares a name sees only its own variable of that name, so the use of the script variable cannot be resolved: "`pos` is a script variable (line 1), but `f` declares its own `pos` (line 4)".
- **Why Nyra has this rule:** Nyra has no shadowing: inside one function a name means one thing. A function that declares `w` keeps working when the script also has a `w` (the function simply does not see it), but a function where the same name means two things is rejected.
- **Common causes:**
  - reading the script variable at the start of a function and declaring a local of the same name later
  - a lambda parameter or a loop variable named like a script variable the same function uses
- **Wrong:**
```rust
var total = 0
fn add(x: int) {
    total += x
    for total in 0..3 {
        print(total)
    }
}
add(5)
print(total)
```
- **Fixed:**
```rust
var total = 0
fn add(x: int) {
    total += x
    for i in 0..3 {
        print(i)
    }
}
add(5)
print(total)
```
- **Related:** E0206, E0201, E0217

## E0217: a function runs before a script variable it uses exists
- **Kind:** compile error · **Since:** v0.5
- **What it means:** A statement of the script calls a function that uses a script variable (directly, or through the functions it calls) which is declared later in the script, or in this very statement (`var pos = next()` where `next` reads `pos`). The message names the function, the variable and the line of its declaration.
- **Why Nyra has this rule:** The statements of a script run in order, and a script variable has no value before its `let` or `var` has run. Functions may be defined anywhere, but a call runs the function, so every script variable it can reach must already exist.
- **Common causes:**
  - the script variables declared at the end of the file, after the code that uses them
  - a script variable whose initial value is computed by a function that uses the variable itself
- **Wrong:**
```rust
fn advance() {
    pos += 1
}
advance()
var pos = 0
print(pos)
```
- **Fixed:**
```rust
var pos = 0
fn advance() {
    pos += 1
}
advance()
print(pos)
```
- **Related:** E0201, E0216, E0254

## E0218: map key type not allowed
- **Kind:** compile error · **Since:** v0.5
- **What it means:** A map type `[K: V]` has a key type other than `int`, `str`, `char` or `bool`, such as `[float: str]` or `[Point: int]`.
- **Why Nyra has this rule:** A key must compare exactly and hash the same way on every backend. Floats do not (rounding, `NaN`, `-0.0`), and arrays and structs as keys would be compared by content on some hosts and by identity on others.
- **Common causes:**
  - a float key, such as a price or a coordinate
  - a struct or an array as the key, where a name or an id would do
- **Wrong:**
```rust
fn main() {
    var names: [float: str] = [:]
    names[1.5] = "one and a half"
    print(names)
}
```
- **Fixed:**
```rust
fn main() {
    var names: [str: str] = [:]
    names[str(1.5)] = "one and a half"
    print(names)
}
```
- **Related:** E0248, E0102

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
- **What it means:** The left side of an assignment, the receiver of a method that changes its receiver (`push`, `pop`, `insert`, `remove`, `sort`, `reverse`) or an `inout` argument is not something that can change. Only a variable, a field or an element of one can (an element or field inside a map value too: `m[k].push(x)`, `m[k].n += 1`; only `inout` cannot reach into a map). The messages name the case: "cannot assign to a character of a string: strings are immutable" (`name[0] = 'A'`), "cannot call `.push()` on a temporary value: it changes its receiver" (`items().push(3)`), "cannot assign to this expression" (`a + b = 3`), "`inout` needs a variable, a field or an element" (`bump(inout 5)`) and "a value inside a map cannot be passed `inout`" (`bump(inout m["a"])`). A variable that is not a `var` is a different error, E0205.
- **Why Nyra has this rule:** Only variables, and the fields and elements inside them, can change. A string is a value that never changes in place: to change a character, build a new string and assign it. A temporary value has no name, so a change to it would be lost.
- **Common causes:**
  - `s[0] = 'A'` on a string: build the new string with `slice` and `+`
  - `push`, `pop` or `sort` on the result of a function call: store the result in a `var` first
  - an assignment to an expression such as `a + b = 3` or `f() = 1`
  - `inout` with a value that is not a variable, such as `inout 5` or `inout f()`
  - `inout m[k]`, a value inside a map: copy it into a `var`, pass that, and store it back with `m[k] = v`
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
- **What it means:** An empty array literal `[]` appears where nothing says what its element type is. The message is "cannot infer the type of the empty array `[]`" and the hint shows `var xs: [int] = []`. The type is known, and `[]` is fine, where it is declared (`var xs: [int] = []`), assigned to a variable, passed as an argument, put in a struct field, returned with `return`, pushed or inserted into an array of arrays, or compared with or added to an array of known type (`xs == []`, `xs + []`).
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
- **Kind:** compile error · **Since:** v0.3
- **What it means:** `inout` is used inconsistently between a function and its call. Three cases: a parameter declared `inout` is called without `inout` ("argument 1 of `bump` is `inout`: the call must say so"); `inout` is written for an argument whose parameter is not `inout`, or for a builtin ("parameter `n` of `bump` is not `inout`"); or two `inout` arguments of one call start at the same variable, as in `swap(inout xs[0], inout xs[1])` ("`inout` arguments must be different variables").
- **Why Nyra has this rule:** An `inout` parameter lets a function change the caller's variable, and the call says `inout` too, so the line shows what can change. Two `inout` arguments are never the same variable: no function can see a variable changed half-way through another name, so there are no aliasing surprises.
- **Common causes:**
  - `inout` forgotten at the call: `bump(x)` instead of `bump(inout x)`
  - `inout` written at a call for a parameter that is not `inout`, such as `print(inout x)`
  - two elements of one array passed to one call: copy one into a temporary variable, call, then assign it back
  - a script variable passed `inout` to a function that uses that script variable itself (directly or through a call): the function already sees it, so change it there
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
- **Kind:** compile error · **Since:** v0.3
- **What it means:** Two kinds of mistake. `free(x)` or `keep(x)` is used on something that cannot be freed or kept: a value that owns no heap memory ("nothing to free: `n` is an `int`, which owns no heap memory"), a parameter or the variable of a `for` loop ("cannot free parameter `xs`: the caller owns it"), or an expression instead of a variable ("`free` needs a local variable"). Or, inside an `arena { }` block, a string or an array (or a struct that holds one) that was declared outside the block is changed: assigned, extended with `+=`, an element or field stored, a method such as `push` called on it, passed as `inout`, freed or kept ("`best` cannot be changed inside this `arena` block: it was declared outside it"). Numbers, bools and chars declared outside can change freely.
- **Why Nyra has this rule:** Memory is automatic: strings, arrays and structs that hold them are freed when their last owner is gone. `free(x)` gives a local variable's memory back now, `keep(x)` says a value is never freed, and `arena { }` marks the values that belong to one block. All three are checked when the program is compiled, so a program that compiles never uses freed memory, and the same programs are accepted on every backend. In v0.3 an `arena` block is checked by these rules, but its memory is freed by reference counting when the last reference goes, as in any other block: an arena changes when memory is returned and never what a program prints. The rule about outer strings and arrays is checked now, so that the way an arena frees its memory can change in a later version without changing what any program does.
- **Common causes:**
  - `free(n)` or `keep(n)` on an `int`, `float`, `bool` or `char`: there is nothing to free
  - freeing or keeping a parameter or a loop variable: the caller owns it
  - `free(xs[0])` or `free(p.name)`: `free` needs a whole variable, and to drop an element early you assign an empty value (`xs[0] = []`)
  - `names.push(...)` or `best = w` inside an `arena` while the variable was declared before the block: change it after the block, or let a function whose body is the `arena` return the result with `return`
- **Wrong:**
```rust
fn main() {
    var best = ""
    arena {
        let words = "the quick brown fox".split(" ")
        for w in words {
            if w.len() > best.len() {
                best = w
            }
        }
    }
    print(best)
}
```
- **Fixed:**
```rust
fn longest(text: str) -> str {
    arena {
        let words = text.split(" ")
        var best = ""
        for w in words {
            if w.len() > best.len() {
                best = w
            }
        }
        return best
    }
}

fn main() {
    print(longest("the quick brown fox"))
}
```
- **Related:** E0205, E0237, E0239

## E0239: use after free
- **Kind:** compile error · **Since:** v0.3
- **What it means:** A variable is used after `free(x)`, or on a path where it may have been freed: freed in only one branch of an `if`, or in an earlier round of a loop. Every use is an error, including a second `free(x)`, until a `var` is given a new value. The message says where it was freed: "`xs` was freed at line 3 and cannot be used any more", or "`xs` may have been freed (line 4): it is freed on some paths before this use".
- **Why Nyra has this rule:** Freed memory must never be used. The compiler follows every local variable through the function and rejects the program, instead of letting it crash or print garbage. The same programs are rejected on every backend, even where freeing leaves nothing visible.
- **Common causes:**
  - reading the variable after `free(xs)`
  - `free` inside an `if`, with a use after the `if`
  - `free` inside a loop, so the next round uses a variable that was freed in the one before
  - a second `free(xs)` for the same variable
- **Wrong:**
```rust
fn main() {
    var xs = [1, 2, 3]
    free(xs)
    print(xs.len())
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
- **Related:** E0201, E0238

## E0240: index out of bounds
- **Kind:** runtime error · **Since:** v0.3
- **What it means:** At run time an index or a range is outside the array or the string. The message gives both numbers: "index 3 is out of bounds for length 3" (reading `xs[3]` or `s[3]`, storing `xs[3] = v`, `remove(i)`, `insert(i, v)`) or "range 2..5 is out of bounds for length 3" (`slice(a, b)`). The valid indexes are 0 up to the length minus 1, and there are no negative indexes: a negative position that is computed (`xs[i - 1]` with `i` at 0) is this error, not the last element, and a negative position written in the program (`xs[-1]`) is stopped earlier, by E0261. `insert(i, v)` also accepts `i` equal to the length, and `slice(a, b)` needs `0 <= a <= b <= len`. The program prints what it printed so far, then the error with the position of the `[` or of the method name, and exits with code 101.
- **Why Nyra has this rule:** Reading past the end would be undefined behaviour in C and `undefined` in JavaScript. Nyra stops with the same error and exit code on every backend.
- **Common causes:**
  - a computed position that goes below 0, such as `xs[i - 1]` when `i` is 0 (the constant `xs[-1]` is E0261)
  - an off-by-one: the last valid index is `xs.len() - 1`, so `xs[xs.len()]` and a loop to `xs.len() + 1` are out
  - an index that comes from data and was not checked against `xs.len()`
  - indexing an empty array
- **Wrong:**
```rust
fn main() {
    let scores = [90, 85, 77]
    let i = 0 - 1
    print("before")
    print(scores[i])
}
```
- **Fixed:**
```rust
fn main() {
    let scores = [90, 85, 77]
    let i = scores.len() - 1
    print("before")
    print(scores[i])
}
```
- **Related:** E0232, E0242

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
    if b == 0 { return 0 }
    return a / b
}

fn main() {
    print(div(6, 3))
    print(div(1, 0))
}
```
- **Related:** E0245

## E0242: pop on an empty array
- **Kind:** runtime error · **Since:** v0.3
- **What it means:** `xs.pop()` was called on an array with no elements, so there is no last element to remove and give back. The message is "pop() on an empty array" with the position of `pop`, and the program exits with code 101. (`remove(i)` on an empty array is E0240, because it names a position.)
- **Why Nyra has this rule:** There is no null to return instead, so the only honest answer is to stop, with the same error and exit code on every backend.
- **Common causes:**
  - popping in a loop that runs more often than the array was filled, such as a stack that is popped once too often
  - an array that is empty because of an earlier branch
- **Wrong:**
```rust
fn main() {
    var stack: [int] = []
    stack.push(1)
    print(stack.pop())
    print(stack.pop())
}
```
- **Fixed:**
```rust
fn main() {
    var stack: [int] = []
    stack.push(1)
    print(stack.pop())
    if stack.len() > 0 {
        print(stack.pop())
    } else {
        print("empty")
    }
}
```
- **Related:** E0240

## E0243: bad argument value
- **Kind:** runtime error · **Since:** v0.3
- **What it means:** A method received an argument that has the right type but a value it cannot work with: `repeat(n)` with a negative `n` ("repeat count must be >= 0, got -1", on strings and on arrays), `replace("", x)` with an empty pattern ("replace() needs a non-empty pattern") or `split("")` with an empty separator ("split() needs a non-empty separator"; the hint says to use `s.chars()` for the characters of a string). A range `for i in a..b step k` with `k` equal to 0 gives "range step must not be 0" (a loop that never ends). The position is the method name (or the step), and the program exits with code 101.
- **Why Nyra has this rule:** These calls have no sensible result, and the host languages disagree about them: JavaScript splits `"abc".split("")` into characters and Python raises an error, and a negative repeat count is an error in JavaScript and an empty string in Python. A clear error beats an answer that depends on the backend.
- **Common causes:**
  - `s.split("")` to get the characters: write `s.chars()`
  - a repeat count that is computed and turns negative, such as `" ".repeat(width - s.len())` when `s` is longer than `width`
  - an empty text passed to `replace`, hoping to insert between characters
- **Wrong:**
```rust
fn main() {
    let word = "hello"
    print("start")
    let letters = word.split("")
    print(letters.len())
}
```
- **Fixed:**
```rust
fn main() {
    let word = "hello"
    print("start")
    let letters = word.chars()
    print(letters.len())
}
```
- **Related:** E0240, E0244, E0249

## E0244: cannot parse text as a number
- **Kind:** runtime error · **Since:** v0.3
- **What it means:** `int(s)` or `float(s)` was given text that is not a number: `cannot parse "five" as int`. `int` accepts digits with an optional leading `-` and the value must fit in 64 bits. `float` accepts the same, with an optional `.` part and an optional exponent (`2`, `-1.5`, `1e3`, `-1.5e-3`), and `"1e999"` is `Infinity`, not an error. The text may not contain spaces, a leading `+`, a `_` or a thousands separator, may not be empty and may not be a word such as `"nan"`. The program exits with code 101.
- **Why Nyra has this rule:** Turning text that is not a number into one silently would hide bugs: there is no `NaN` result and no fallback to 0. A failing conversion stops the program with a precise message. When the text may be invalid, check it first, as `is_number` does below.
- **Common causes:**
  - text with spaces or a unit: `"12 "`, `"3px"`
  - an empty string, for example from `split` on text with nothing in it
  - a decimal point in `int("1.5")`: parse with `float(s)`, then truncate with `int(x)`
  - a number too large for `int`, or a number written with `,` or `_`
- **Wrong:**
```rust
fn main() {
    let parts = "3,4,five".split(",")
    var total = 0
    for p in parts {
        total += int(p)
    }
    print(total)
}
```
- **Fixed:**
```rust
fn is_number(s: str) -> bool {
    if s.len() == 0 {
        return false
    }
    for c in s {
        if !c.is_digit() {
            return false
        }
    }
    return true
}

fn main() {
    let parts = "3,4,five".split(",")
    var total = 0
    for p in parts {
        if is_number(p) {
            total += int(p)
        }
    }
    print(total)
}
```
- **Related:** E0243, E0245

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
- **Kind:** runtime error · **Since:** v0.3
- **What it means:** `char(n)` was called with a number that is not a character: negative, above 1114111, or in the surrogate range 55296 to 57343. The message shows the number: "char(-3): not a valid character code". The position is the `char` call, and the program exits with code 101.
- **Why Nyra has this rule:** A `char` is always one real Unicode character, so a string can never hold invalid text. Turning a number into a character is explicit, and an impossible number is an error, not a replacement character.
- **Common causes:**
  - arithmetic on character codes that leaves the valid range, such as a Caesar shift that goes below `'a'` or past `'z'` without wrapping around
  - a byte or a random number used as a code without checking it
- **Wrong:**
```rust
fn shift(c: char, by: int) -> char = char(c.code() + by)

fn main() {
    print(shift('a', 1))
    print(shift('a', -100))
}
```
- **Fixed:**
```rust
fn shift(c: char, by: int) -> char = char((c.code() - 97 + by) % 26 + 97)

fn main() {
    print(shift('a', 1))
    print(shift('a', 25))
}
```
- **Related:** E0244, E0245

## E0247: min or max of an empty array
- **Kind:** runtime error · **Since:** v0.5
- **What it means:** `xs.min()` or `xs.max()` was called on an array with no elements (also after a `filter` or `map` in the same chain, such as `xs.filter(x => x > 100).min()` when nothing passes), so there is no smallest or largest element. The message is "min() of an empty array" or "max() of an empty array" with the position of the method, and the program exits with code 101.
- **Why Nyra has this rule:** There is no null and no "minus infinity" for every type, so an empty array has no honest answer; the program stops with the same error on every backend.
- **Common causes:**
  - an array that is empty because of an earlier branch or because no line of the input matched
  - a filter that lets nothing through
- **Wrong:**
```rust
fn main() {
    let xs = [3, 8, 5]
    print(xs.max())
    print(xs.filter(x => x > 10).min())
}
```
- **Fixed:**
```rust
fn main() {
    let xs = [3, 8, 5]
    print(xs.max())
    let big = xs.filter(x => x > 10)
    if big.len() > 0 {
        print(big.min())
    } else {
        print("none")
    }
}
```
- **Related:** E0242, E0240

## E0248: key not in the map
- **Kind:** runtime error · **Since:** v0.5
- **What it means:** `m[k]` or `m.get(k)` read a key that the map does not have, for example `key "bob" is not in the map`.
- **Why Nyra has this rule:** Nyra has no null, so a missing key cannot give "nothing". The program stops with a clear message instead of continuing with a made-up value. `m.has(k)` tests first, and `m.get(k, default)` gives a value for a missing key.
- **Common causes:**
  - counting with `m[k] += 1` before the key exists: write `m[k] = m.get(k, 0) + 1`
  - a key with different text (case, spaces) from the one that was stored
- **Wrong:**
```rust
fn main() {
    var counts: [str: int] = [:]
    counts["a"] += 1
    print(counts)
}
```
- **Fixed:**
```rust
fn main() {
    var counts: [str: int] = [:]
    counts["a"] = counts.get("a", 0) + 1
    print(counts)
}
```
- **Related:** E0240, E0218


## E0249: out of memory
- **Kind:** runtime error · **Since:** v0.3
- **What it means:** The program asked for more memory than a string, an array or the machine can have, and it stops with exit code 101: "out of memory", at the operation that asked, or at position 0:0 when that is not known. `repeat` has a size limit on every backend, so a result of more than 536870888 characters (strings) or 100000000 elements (arrays) is reported at once, without trying to allocate it: `"ab".repeat(1000000000000)`. Natively, the error is also reported when the system gives no more memory. On JavaScript it is also reported when the engine runs out of string or array length, for example for a string built up past 536870888 characters. A JavaScript program that fills the whole heap is stopped by Node itself ("JavaScript heap out of memory", exit code 134), which cannot be reported as E0249.
- **Why Nyra has this rule:** With memory managed by the runtime, running out of it has to end the program with a message instead of a crash, and a size that is too big on one backend should be too big on all of them, so `repeat` has one limit everywhere.
- **Common causes:**
  - a repeat count with too many zeros, or one that is computed from a wrong value
  - building a huge array or string in a loop
  - a loop that keeps growing a value and never stops
- **Wrong:**
```rust
fn main() {
    let big = "ab".repeat(1000000000000)
    print(big.len())
}
```
- **Fixed:**
```rust
fn main() {
    let small = "ab".repeat(1000)
    print(small.len())
}
```
- **Related:** E0240, E0243

## E0250: example is false
- **Kind:** compile error · **Since:** v0.5
- **What it means:** An example written with `ex` after a function evaluates to `false`. Examples are run while the program compiles (by `nyra check`, `run`, `build` and `test`), so a function whose logic is wrong for an input of its examples does not compile. For a comparison the message gives the value of each side: "example `dist(2, 7) == 5` is false: `dist(2, 7)` is -5, not 5", and `--json` adds `"actual":"-5","expected":"5"` (values as Nyra code: strings in quotes). When the left side calls a function of the program, the hint names its parameters with the arguments of the example (`a = 2, b = 7`).
- **Why Nyra has this rule:** A function and an example state the same fact in two ways: when they disagree, one of them is wrong, and it is cheaper to learn that before the program runs than from its output. Examples cost nothing at run time: they are never compiled into the program.
- **Common causes:**
  - a bug in the function for one kind of input: the other branch, a negative number, an empty array, the last element
  - an off-by-one in a loop bound or a range
  - an example that expects the wrong value: work it out by hand once more; change the example only when it is the wrong one, never just to match what the function returns
  - comparing floats with `==`: `0.1 + 0.2 == 0.3` is false; compare with a value the function really returns, or test a range (`x > 0.29 && x < 0.31`)
- **Wrong:**
```rust
// the distance between two numbers on a line
fn dist(a: int, b: int) -> int {
    if a > b { return a - b }
    return a - b
}
ex dist(7, 2) == 5, dist(2, 7) == 5

fn main() {
    print(dist(2, 7))
}
```
- **Fixed:**
```rust
// the distance between two numbers on a line
fn dist(a: int, b: int) -> int {
    if a > b { return a - b }
    return b - a
}
ex dist(7, 2) == 5, dist(2, 7) == 5

fn main() {
    print(dist(2, 7))
}
```
- **Related:** E0251, E0252, E0253

## E0251: example stops with a runtime error
- **Kind:** compile error · **Since:** v0.5
- **What it means:** Running an example stops with a runtime error (E0240-E0249): "example `mean([]) == 0` stops with runtime error E0241: division by zero (at line 4:15 in `mean`)". The position in the message is where the error happened, in the function the example calls; the error itself points at the example.
- **Why Nyra has this rule:** An example shows what a function does for an input. If the function cannot handle that input, the program would stop the same way when it meets it, so the compiler reports it before the program runs.
- **Common causes:**
  - a function that divides by a count that can be 0 (an average of an empty array)
  - an index or `slice` past the end for a short or empty input
  - `pop()` on an empty array, `int(s)` of text that is not a number
  - an example that gives the function an input it was never meant to take: give it a valid one
- **Wrong:**
```rust
fn mean(xs: [int]) -> int {
    var total = 0
    for x in xs { total += x }
    return total / xs.len()
}
ex mean([2, 4, 6]) == 4, mean([]) == 0

fn main() {
    print(mean([1, 2, 3]))
}
```
- **Fixed:**
```rust
fn mean(xs: [int]) -> int {
    if xs.len() == 0 { return 0 }
    var total = 0
    for x in xs { total += x }
    return total / xs.len()
}
ex mean([2, 4, 6]) == 4, mean([]) == 0

fn main() {
    print(mean([1, 2, 3]))
}
```
- **Related:** E0250, E0240, E0241

## E0252: example is not a `bool`
- **Kind:** compile error · **Since:** v0.5
- **What it means:** An `ex` example is a condition that must be true, so it must have the type `bool`: "an example must be a `bool` condition, but `sq(3)` is an `int`". A call that returns nothing (`print(1)`) is not an example either.
- **Why Nyra has this rule:** An example states a fact about a value. A value alone states nothing: the compiler would not know what to expect.
- **Common causes:**
  - only the call, without the value it should give: `ex sq(3)` instead of `ex sq(3) == 9`
  - `print(...)` as an example
- **Wrong:**
```rust
fn sq(x: int) -> int = x * x   ex sq(3)

fn main() {
    print(sq(4))
}
```
- **Fixed:**
```rust
fn sq(x: int) -> int = x * x   ex sq(3) == 9

fn main() {
    print(sq(4))
}
```
- **Related:** E0250, E0209

## E0253: example does not finish
- **Kind:** compile error · **Since:** v0.5
- **What it means:** An example ran for more than 1,000,000 steps (statements run, plus the elements and characters it made), or more than 10,000 calls were nested, and the compiler stopped it: "example `digits(1234) == 4` did not finish within 1000000 steps". Usually the function it calls loops forever for that input.
- **Why Nyra has this rule:** Examples run while the program compiles, so each one has a budget: the compiler must always finish, and quickly. A loop that never ends is found before the program runs, with the input that shows it.
- **Common causes:**
  - a `while` loop whose variable never changes (the step was forgotten, or changes another variable)
  - recursion that never reaches its base case for this input (a negative number, an empty array)
  - an example with a very large input: examples should be small; use the program itself for big inputs
- **Wrong:**
```rust
fn digits(n: int) -> int {
    var left = n
    var count = 1
    while left >= 10 {
        count += 1
    }
    return count
}
ex digits(7) == 1, digits(1234) == 4

fn main() {
    print(digits(2026))
}
```
- **Fixed:**
```rust
fn digits(n: int) -> int {
    var left = n
    var count = 1
    while left >= 10 {
        left /= 10
        count += 1
    }
    return count
}
ex digits(7) == 1, digits(1234) == 4

fn main() {
    print(digits(2026))
}
```
- **Related:** E0250, E0251

## E0254: example calls a function that uses script variables
- **Kind:** compile error · **Since:** v0.5
- **What it means:** An `ex` example calls a function that uses a script variable (directly, or through the functions it calls). Examples run while the program compiles, before any statement of the script, so the variable has no value yet.
- **Why Nyra has this rule:** An example states what a function returns for the arguments it shows. A function that also reads a script variable depends on something the example cannot show, and the compiler cannot run the script to give the variable its value.
- **Common causes:**
  - an example for a helper of a parser or a game loop that reads the script's state (`pos`, `tokens`, `board`)
- **Wrong:**
```rust
let rate = 3
fn cost(n: int) -> int = n * rate   ex cost(2) == 6
print(cost(5))
```
- **Fixed:**
```rust
let rate = 3
fn cost(n: int, r: int) -> int = n * r   ex cost(2, 3) == 6
print(cost(5, rate))
```
- **Related:** E0250, E0217

## E0255: int overflow
- **Kind:** runtime error · **Since:** v0.5
- **What it means:** An int `+`, `-`, `*`, `/` (only `-9223372036854775808 / -1`) or negation (including `abs`) gave a result outside the 64-bit range, -9223372036854775808 to 9223372036854775807. The program flushes what it printed so far, reports the operation with its operands (`int overflow: 7696581397574 * 1099511628211 does not fit in 64 bits`) and its position, and exits with code 101. This happens on every backend, at the same operation.
- **Why Nyra has this rule:** Up to v0.4 an int overflow wrapped around silently natively and lost precision in JavaScript: a wrong number that looks right, and a different one on each backend. Stopping makes the bug visible where it happens. Additions, subtractions and multiplications that the compiler can prove stay in range (constants, `for` counters, array indexes, lengths) are compiled without the check.
- **Common causes:**
  - a hash or a random number generator that multiplies without `%` and relied on wrapping (`h = h * 1099511628211`): keep it in range with `%`, e.g. `h = (h * 31 + c.code()) % 1000000007`
  - a factorial, a power or a product that grows past 2^63 (20! is the largest factorial that fits)
  - the negation or `abs` of the smallest int
- **Wrong:**
```rust
fn hash(s: str) -> int {
    var h = 7
    for c in s {
        h = h * 1099511628211 + c.code()
    }
    return h
}

fn main() {
    print(hash("a"))
    print(hash("ab"))
}
```
- **Fixed:**
```rust
fn hash(s: str) -> int {
    var h = 7
    for c in s {
        h = (h * 31 + c.code()) % 1000000007
    }
    return h
}

fn main() {
    print(hash("a"))
    print(hash("ab"))
}
```
- **Related:** E0256, E0245, E0241

## E0256: int too large for JavaScript
- **Kind:** runtime error · **Since:** v0.5
- **What it means:** Only on the JavaScript and TypeScript targets: an int operation gave a result beyond ±9007199254740991 (2^53 - 1), or the program used such an int (a literal, `int(s)`, `int(x)` of a float, a number read by `json.parse`). JavaScript numbers hold ints exactly only up to there, so the program stops instead of going on with a rounded value. The native (C), Rust, Go and Python targets have 64-bit ints and run the same program without this error (they stop with E0255 only beyond 64 bits).
- **Why Nyra has this rule:** Nyra promises the same output on every backend. A rounded int would be a silent difference (`1116302080` instead of `1116302264`); an error says exactly where the JavaScript target cannot follow, and which targets can.
- **Common causes:**
  - a random number generator like `seed = (seed * 1103515245 + 12345) % 2147483648`: the product is about 2^61 before the `%`; use a smaller multiplier, e.g. `seed = seed * 48271 % 2147483647`, or a native target
  - a hash that keeps its value below a large modulus, but multiplies it by a large number first
  - a factorial or product above 9007199254740991, or an int literal with 16 or more digits
- **Wrong:**
```rust
// target: js
fn next(seed: int) -> int = (seed * 1103515245 + 12345) % 2147483648

fn main() {
    var s = 42
    for i in 0..3 {
        s = next(s)
        print(s)
    }
}
```
- **Fixed:**
```rust
fn next(seed: int) -> int = seed * 48271 % 2147483647

fn main() {
    var s = 42
    for i in 0..3 {
        s = next(s)
        print(s)
    }
}
```
- **Related:** E0255

## E0260: `$` before `{value}` in a string
- **Kind:** warning · **Since:** v0.6
- **What it means:** A string has `${x}`: Nyra keeps the `$` as text and inserts the value of `x`, so `"cost: ${x}"` prints `cost: $3`. This is not an error (a dollar sign before a value is sometimes what you want), so the build goes on; the warning is printed to stderr and listed under `"warnings"` in `--json`.
- **Why Nyra has this rule:** `${x}` is how JavaScript, shell and many template engines insert a value. Written in Nyra it compiles and prints a stray `$` without any sign that something is off, which an AI agent or a person coming from those languages does not see in the output. In Nyra `{x}` alone inserts a value.
- **Common causes:**
  - writing a JavaScript template string (`` `total: ${x}` ``) or a shell variable (`"${HOME}/logs"`) in Nyra
  - a price or a currency (`"${price}"`) where the `$` is meant: write `"$" + str(price)` to say so (this also keeps the warning away)
- **Wrong:**
```rust
fn label(x: int) -> str = "cost: ${x}"
print(label(3))
```
- **Fixed:**
```rust
fn label(x: int) -> str = "cost: {x}"
print(label(3))
```
- **Related:** E0201

## E0261: negative index or slice position
- **Kind:** compile error · **Since:** v0.6
- **What it means:** An array or string is indexed (`xs[-1]`, `s[-2]`) or sliced (`xs.slice(-3, 5)`) with a negative number written in the program. Positions start at 0 and Nyra never counts from the end, so the position is out of bounds whatever the length is; the program would stop with a runtime error (E0241) if it ran. Maps are not affected: a map key may be negative.
- **Why Nyra has this rule:** In Python and JavaScript `xs[-1]` is the last element, so people write it by habit. Nyra has one meaning for a position (0 up to len - 1) and no negative shortcut, and the compiler reports the constant case before the program runs.
- **Common causes:**
  - the last element written as `xs[-1]` or `s[-1]`: write `xs[xs.len() - 1]`
  - the last few elements as `xs.slice(-3, xs.len())`: write `xs.slice(xs.len() - 3, xs.len())`
- **Wrong:**
```rust
fn last(xs: [int]) -> int = xs[-1]
print(last([1, 2, 3]))
```
- **Fixed:**
```rust
fn last(xs: [int]) -> int = xs[xs.len() - 1]
print(last([1, 2, 3]))
```
- **Related:** E0241, E0232

## E0262: optional type
- **Kind:** compile error · **Since:** v0.6
- **What it means:** A type is written with a `?` (`int?`, `Point?`, `[str]?`) to say "a value or nothing". Nyra has no optional values and no null yet, so there is no such type.
- **Why Nyra has this rule:** Every Nyra value always exists, so a function never has to be asked whether its result is there. A missing result is an ordinary value that the function documents: a sentinel that cannot be a real answer, or a `bool` next to it.
- **Common causes:**
  - a search that may find nothing and was written `-> int?`: return `-1` (what `index_of` does), or `""` for text
  - a map lookup that may miss: ask `m.has(k)` first, then `m.get(k)`
- **Wrong:**
```rust
fn find(xs: [int], x: int) -> int? {
    ret xs.index_of(x)
}
print(find([1, 2], 2))
```
- **Fixed:**
```rust
fn find(xs: [int], x: int) -> int {
    ret xs.index_of(x)
}
print(find([1, 2], 2))
```
- **Related:** E0001, E0102

## E0263: function inside a struct or `impl` block
- **Kind:** compile error · **Since:** v0.6
- **What it means:** A `fn` is written inside the braces of a `struct`, or in an `impl Name { }` block. A struct holds only fields, and Nyra has no methods and no `impl`: a function is written on its own and takes the struct as a parameter.
- **Why Nyra has this rule:** There is one way to write a function and one way to call it, `area(r)`. Methods and `impl` blocks would add a second way (`r.area()`) and a second place to look for code.
- **Common causes:**
  - a method written the way Rust, Swift, Kotlin or JavaScript classes do
  - an `impl Rect { fn area(self) ... }` block copied from Rust: drop the wrapper, rename `self` to a parameter with a type
- **Wrong:**
```rust
struct Rect {
    w: int
    h: int
    fn area(r: Rect) -> int = r.w * r.h
}
print(1)
```
- **Fixed:**
```rust
struct Rect {
    w: int
    h: int
}
fn area(r: Rect) -> int = r.w * r.h
print(area(Rect(w: 2, h: 3)))
```
- **Related:** E0101, E0227

## E0264: `class`
- **Kind:** compile error · **Since:** v0.6
- **What it means:** The program declares a `class`. Nyra has no classes: data is a `struct`, and the functions that work on it are written outside it.
- **Why Nyra has this rule:** One kind of user-defined type keeps programs short and keeps every function in the same place. A struct has fields and nothing else.
- **Common causes:**
  - a class copied from Python, Java, Kotlin or TypeScript: write `struct`, keep the fields, and move the methods out as functions that take the struct (`fn area(r: Rect)`)
- **Wrong:**
```rust
class Rect {
    w: int
    h: int
}
fn area(r: Rect) -> int = r.w * r.h
print(area(Rect(w: 2, h: 3)))
```
- **Fixed:**
```rust
struct Rect {
    w: int
    h: int
}
fn area(r: Rect) -> int = r.w * r.h
print(area(Rect(w: 2, h: 3)))
```
- **Related:** E0101, E0263

## E0290: capability not granted
- **Kind:** compile error · **Since:** v0.6
- **What it means:** The program has a `use` line for a standard module that needs a capability this run does not grant: "module `fs` needs the capability `fs` (read, write, list and remove files and folders), which is not granted". The modules `fs` (files), `input` (standard input), `os` (arguments, environment variables, `exit`) and later `net` are effectful and need the capability of their own name; `json`, `math`, `text`, `time` and `random` are always available. `nyra run` and `nyra build` grant everything unless `--sandbox` or `--allow` narrow it; the MCP tool `nyra_run` grants only standard input unless its `allow` argument says more.
- **Why Nyra has this rule:** An agent that runs code it wrote itself, or code from someone else, should decide what that code may touch before it runs. The `use` lines already say which modules a program uses, so the check needs no annotations and happens at compile time, with the module, the capability and the flag to add in the message.
- **Common causes:**
  - running with `--sandbox` (nothing is granted but what `--allow` names) a program that reads a file
  - `--allow fs` for a program that also uses `os.args()`: every module needs its own capability
  - `nyra_run` without an `allow` argument for a program that uses `fs` or `os`
- **Wrong:**
```rust
// flags: --sandbox
use fs

fn main() {
    print(fs.exists("notes.txt"))
}
```
- **Fixed:**
```rust
// flags: --sandbox --allow fs
use fs

fn main() {
    print(fs.exists("notes.txt"))
}
```
- **Related:** E0300, E0340

## E0292: malformed property example
- **Kind:** compile error · **Since:** v0.6
- **What it means:** The line after `ex for` is not a property header. A property example is `ex for n in 0..200: f(n) >= 0`: a variable, a range whose bounds are whole-number literals (an optional `step k`, not 0), a colon, and the conditions. The bounds are literals because the compiler runs the example while it compiles.
- **Why Nyra has this rule:** One fixed form keeps properties easy to write and to read back. A range that is computed from variables or functions could not be known before the program runs, and an example sees no variables.
- **Common causes:**
  - a bound that is a name or a calculation (`0..count`, `0..2 * 50`)
  - a missing colon, or a `{` where the colon belongs
  - `step 0`
- **Wrong:**
```rust
fn double(x: int) -> int = x * 2
ex for n in 0..count: double(n) >= 0

fn main() {
    print(double(4))
}
```
- **Fixed:**
```rust
fn double(x: int) -> int = x * 2
ex for n in 0..200: double(n) >= 0

fn main() {
    print(double(4))
}
```
- **Related:** E0250, E0293

## E0293: property example has too many inputs
- **Kind:** compile error · **Since:** v0.6
- **What it means:** The range of a property example has more than 100,000 values: "the property example `for n in 0..1000000` has 1000000 inputs; at most 100000 are run". A property runs its condition for every value of the range while the program compiles.
- **Why Nyra has this rule:** The compiler must always finish quickly. A property is a sample of inputs, not a proof: a few hundred well-chosen values find almost every bug that a million would.
- **Common causes:**
  - a range written with too many zeros
  - a small `step` over a huge range
- **Wrong:**
```rust
fn double(x: int) -> int = x * 2
ex for n in 0..1000000: double(n) >= 0

fn main() {
    print(double(4))
}
```
- **Fixed:**
```rust
fn double(x: int) -> int = x * 2
ex for n in 0..1000000 step 1000: double(n) >= 0

fn main() {
    print(double(4))
}
```
- **Related:** E0250, E0253, E0292

## E0300: module not found
- **Kind:** compile error · **Since:** v0.5
- **What it means:** A `use` line names a module that does not exist. The standard modules are `fs`, `input`, `json`, `math`, `os`, `random`, `text` and `time`; the message suggests the closest one. (Modules from files of the project and dependencies are planned.)
- **Why Nyra has this rule:** Imports must be explicit and checkable before anything runs. The message suggests the closest module name and lists the standard modules.
- **Common causes:**
  - a typo in a module name (`mth` for `math`)
  - a module of another language: `use io` or `use sys` (standard input is `input`, arguments and the exit code are in `os`)
  - `use str`: string methods need no import (`s.split(",")`), and the `text` module has `text.fixed`
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
- **Kind:** compile error · **Since:** v0.5
- **What it means:** An import is not written `use name`: another language's `import math` or `from math import sqrt`, a quoted path, an alias, or a `use` line inside a function. A `use` line names one module and stands at the top level of the file.
- **Why Nyra has this rule:** One import syntax in one place, so the dependencies of a file are visible at the top. Other languages' spellings (`import`, `from ... import`, `use a.{b}`) are not accepted.
- **Common causes:**
  - `import math` instead of `use math` (`nyra check --fix` rewrites it)
  - `from math import sqrt`: import the module and call `math.sqrt(x)`
  - a `use` line inside a function, or several modules on one line
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
- **Kind:** compile error · **Since:** v0.5
- **What it means:** `module.name` is used, but the module has no function or constant called `name`. The hint suggests the closest name, or the Nyra spelling of another language's function (`random.randint` is `random.range`, `json.dumps` is `json.str`).
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
- **Kind:** compile error · **Since:** v0.5
- **What it means:** A module item is used the wrong way: a function without a call (`time.now_ms`), a constant called like a function (`math.pi()`), or a module name used as a value.
- **Why Nyra has this rule:** Functions are always called and constants are never called, so each use shows what kind of thing it is.
- **Common causes:**
  - forgetting the parentheses on a function
  - adding parentheses to a constant
- **Wrong:**
```rust
use math

fn main() {
    print(math.pi())
}
```
- **Fixed:**
```rust
use math

fn main() {
    print(math.pi)
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

## E0309: `json.parse` needs to know the type
- **Kind:** compile error · **Since:** v0.5
- **What it means:** `json.parse(text)` is used where nothing says which type to read. `json.parse` reads the type the value goes to: a `let` with a type, a parameter, a field, a `return` value or an assignment.
- **Why Nyra has this rule:** JSON is read into ordinary Nyra values (ints, floats, strings, arrays and structs), checked against their type, so there is no untyped "JSON value" that every use would have to inspect. The type must therefore be known where the text is read.
- **Common causes:**
  - `let x = json.parse(text)` without a type
  - `print(json.parse(text))`: print takes values of any type
- **Wrong:**
```rust
use json

fn main() {
    let xs = json.parse("[1, 2, 3]")
    print(xs)
}
```
- **Fixed:**
```rust
use json

fn main() {
    let xs: [int] = json.parse("[1, 2, 3]")
    print(xs)
}
```
- **Related:** E0230, E0345

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
    return x * 2
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
- **Kind:** runtime error · **Since:** v0.5
- **What it means:** A function of the `fs` module could not do its job, for example `fs.read: cannot read "x.txt" (not found)`. The reason is one of `not found`, `permission denied`, `is a directory`, `not a directory`, `already exists`, `not empty`, `not valid UTF-8` or `io error`. The program stops with exit code 101 and the error points at your call.
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
- **Kind:** runtime error · **Since:** v0.5
- **What it means:** Text that enters the program (standard input, files, command-line arguments, environment variables) is not valid UTF-8, for example `input.line: the input is not valid UTF-8`.
- **Why Nyra has this rule:** A `str` is UTF-8 text on every backend. Every backend checks incoming text with the same rule, so a program behaves identically and never holds broken text.
- **Common causes:**
  - binary data piped into the program
  - a text file in an old encoding such as Latin-1 or UTF-16
- **Wrong:**
```rust
// stdin: \xff\n
use input

fn main() {
    // run it as:  printf '\377\n' | nyra run main.nyra
    print(input.line())
}
```
- **Fixed:**
```rust
use input

fn main() {
    // run it as:  printf 'ok\n' | nyra run main.nyra
    print(input.line())
}
```
- **Related:** E0340, E0344

## E0342: bad argument value for a standard function
- **Kind:** runtime error · **Since:** v0.5
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

## E0345: JSON text does not fit
- **Kind:** runtime error · **Since:** v0.5
- **What it means:** `json.parse(text)` got text that is not JSON (`json.parse: invalid JSON at line 3: expected `,` or `}``), or JSON whose shape does not match the type it is read into (`json.parse: expected an int at $.items[2].count`, `json.parse: missing field "name" at $`). The path starts at `$`, the whole value.
- **Why Nyra has this rule:** JSON is read straight into typed values, so every field a struct has must be there with the right type; fields the struct does not have are skipped. A mismatch stops the program instead of producing a value with holes, and the message says where.
- **Common causes:**
  - a number written as a string in the JSON (`"age": "12"` for an `int`)
  - a float such as `1.5` where the type says `int`
  - a missing field, or `null` (Nyra has no null)
  - a trailing comma, single quotes or comments, which JSON does not allow
- **Wrong:**
```rust
use json

struct User {
    name: str
    age: int
}

fn main() {
    let u: User = json.parse("{{\"name\": \"Ann\", \"age\": \"12\"}}")
    print(u.age)
}
```
- **Fixed:**
```rust
use json

struct User {
    name: str
    age: int
}

fn main() {
    let u: User = json.parse("{{\"name\": \"Ann\", \"age\": 12}}")
    print(u.age)
}
```
- **Related:** E0309, E0244

## E0355: step limit reached
- **Kind:** runtime error · **Since:** v0.6
- **What it means:** A program that runs in the interpreter (`nyra run --interp`, `--sandbox`, or `nyra_run` with `sandbox: true`) ran more steps than its fuel: "step limit reached: the program ran more than 100000 steps". Every statement costs a step, and so does every element or character an operation makes. The program stops after the output it printed so far, with exit code 120, at the statement that ran when the fuel ran out.
- **Why Nyra has this rule:** Whoever runs a program unsupervised needs it to end. Counting steps instead of seconds makes the limit the same on every machine: the same program with the same fuel always stops at the same place.
- **Common causes:**
  - a `while` loop whose condition never becomes false
  - recursion that never reaches its base case (also E0358)
  - real work that needs more than the fuel: raise it with `--fuel N` (the default is 2,000,000,000; `nyra_run`: `fuel`)
- **Wrong:**
```rust
// flags: --interp --fuel 100000
fn main() {
    var n = 0
    while true {
        n += 1
    }
}
```
- **Fixed:**
```rust
// flags: --interp --fuel 100000
fn main() {
    var n = 0
    while n < 1000 {
        n += 1
    }
    print(n)
}
```
- **Related:** E0253, E0358, E0359

## E0356: memory limit reached
- **Kind:** runtime error · **Since:** v0.6
- **What it means:** A program that runs in the interpreter used more heap memory than its limit: "memory limit reached: the program used more than 16777216 bytes of memory". The interpreter measures the memory the run has added (every string, array, map and struct), and also checks before an operation such as `repeat` that would not fit. The program stops with exit code 121.
- **Why Nyra has this rule:** A program that fills the memory of the machine takes everything else down with it. The limit makes a runaway program an error with a position instead.
- **Common causes:**
  - a loop that keeps adding to an array or a map and never stops
  - a string doubled in a loop (`s = s + s`)
  - a legitimate program that needs more: raise the limit with `--max-memory SIZE` (default 512M; `nyra_run`: `max_memory`)
- **Wrong:**
```rust
// flags: --interp --max-memory 16M
fn main() {
    var xs: [int] = []
    while true {
        xs.push(1)
    }
}
```
- **Fixed:**
```rust
// flags: --interp --max-memory 16M
fn main() {
    var xs: [int] = []
    for i in 0..1000 {
        xs.push(i)
    }
    print(xs.len())
}
```
- **Related:** E0249, E0355

## E0357: output limit reached
- **Kind:** runtime error · **Since:** v0.6
- **What it means:** A program that runs in the interpreter printed more bytes than its limit: "output limit reached: the program printed more than 100 bytes". The output is cut at the limit (the first bytes are written) and the program stops with exit code 122.
- **Why Nyra has this rule:** A print loop that never ends fills a disk or an agent's context in seconds. The cap keeps what a run can say to a size somebody agreed to read.
- **Common causes:**
  - printing in a loop that never ends
  - printing a big array or a long string inside a loop instead of once at the end
  - a legitimate program with a lot of output: raise the limit with `--max-output SIZE` (default 64M; `nyra_run` allows at most 16 KiB)
- **Wrong:**
```rust
// flags: --interp --max-output 100
fn main() {
    for i in 0..1000 {
        print("line {i}")
    }
}
```
- **Fixed:**
```rust
// flags: --interp --max-output 100
fn main() {
    for i in 0..3 {
        print("line {i}")
    }
}
```
- **Related:** E0355

## E0358: call depth limit reached
- **Kind:** runtime error · **Since:** v0.6
- **What it means:** A program that runs in the interpreter nested more calls than its limit: "call depth limit reached: more than 1000 calls were nested". It stops with exit code 123, at the call that went too deep.
- **Why Nyra has this rule:** On the compiled targets a recursion that never ends crashes the program when its stack ends, often without a message. In the interpreter the depth is counted, so the same mistake is an error with a position, the same everywhere.
- **Common causes:**
  - a recursive function that never reaches its base case
  - a recursion that is legitimately deep (a list of a hundred thousand elements): write it as a loop, or raise the limit with `--max-depth N` (default 20,000, at most 100,000)
- **Wrong:**
```rust
// flags: --interp --max-depth 1000
fn depth(n: int) -> int = depth(n + 1) + 1

fn main() {
    print(depth(0))
}
```
- **Fixed:**
```rust
// flags: --interp --max-depth 1000
fn depth(n: int) -> int {
    if n >= 100 {
        ret 0
    }
    ret depth(n + 1) + 1
}

fn main() {
    print(depth(0))
}
```
- **Related:** E0355

## E0359: time limit reached
- **Kind:** runtime error · **Since:** v0.6
- **What it means:** A program that runs in the interpreter ran longer than its time limit: "time limit reached: the program ran longer than 200 ms". The limit is off unless it is given (`--max-time MS`; `nyra_run` always has one, `timeout_ms`, 10 seconds by default). The program stops with exit code 124.
- **Why Nyra has this rule:** Steps (E0355) are the limit that is the same on every machine; real time is the last safety net for a run that has to end at a given moment, for example inside an agent's tool call. `time.sleep_ms` does not wait in the interpreter: it moves a virtual clock and costs steps.
- **Common causes:**
  - an endless loop, when the fuel is very large
  - a heavy program on a slow machine
- **Wrong:**
```rust
// flags: --interp --max-time 200
fn main() {
    var n = 0
    while true {
        n += 1
    }
}
```
- **Fixed:**
```rust
// flags: --interp --max-time 200
fn main() {
    var n = 0
    while n < 1000 {
        n += 1
    }
    print(n)
}
```
- **Related:** E0355
