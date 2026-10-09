# Nyra: guide for AI agents

How to write correct **Nyra v0.5** programs. Read it once, top to bottom; it is short on purpose.

Nyra is **not in your training data** and it is **not** Rust, Go, TypeScript or Python, even though
the tokens look familiar (code blocks here are marked `rust` only so GitHub highlights them). Use only
what is shown here or in the [language spec](https://raw.githubusercontent.com/SpAkXD/nyra/main/docs/SPEC.md).
If a feature is not described, it does not exist yet (section 4).

## 1. Workflow

```
nyra check prog.nyra --json    # compile only (and run the `ex` examples); {"ok":true,"errors":[]} when clean
nyra test prog.nyra --json     # run the `ex` examples: {"ok":..,"examples":3,"passed":2,"failed":1,"errors":[..]}
nyra check prog.nyra --json --fix   # the same, after repairing every mistake that has a certain fix
nyra run prog.nyra             # compile and run natively (needs gcc, clang or tcc)
nyra run prog.nyra --js        # or run on Node.js
nyra explain E0201 --json      # what an error code means: why, causes, a wrong and a fixed program
```

1. Write the program to `prog.nyra`, with 1-2 examples after every non-trivial function (below).
2. Run `nyra check prog.nyra --json`. If `"ok"` is `false`, fix **every** entry of `errors`, then check
   again. Each error has a stable `code`, a `message`, `line`, `col` and a `hint`:
   ```json
   {"ok":false,"errors":[{"code":"E0201","message":"undefined variable `cout`","file":"a.nyra","line":4,"col":11,"hint":"did you mean `count`?"}]}
   ```
   If a hint is not enough, `nyra explain <code> --json` returns the whole entry of the error database
   ([ERRORS.md](https://raw.githubusercontent.com/SpAkXD/nyra/main/docs/ERRORS.md)): what the code means, why the rule
   exists, the usual causes, and a wrong and a fixed program. (`nyra explain` needs nyra 0.3 or newer;
   with an older one, read ERRORS.md instead.)
3. Run it and compare the output with what you expect.

**Let the compiler fix simple mistakes first.** An error whose repair is certain carries a `fix`:
`return` -> `ret`, a `;`, `elif` -> `else if`, `and`/`or`/`not` -> `&&`/`||`/`!`, `True` -> `true`,
`'hello'` -> `"hello"`, `xs.length()` -> `xs.len()`, `string`/`i32` -> `str`/`int`, `Point { x: 1 }` ->
`Point(x: 1)`, `Point(1, 2)` -> `Point(x: 1, y: 2)`, `5.` -> `5.0`, `2` -> `2.0` where a `float` is needed,
`let` -> `var` for a variable that changes, `print "hi"` -> `print("hi")`, `{` on its own line, and more.
`--fix` applies all of them, checks again and writes the file back when it then compiles (the edits
are printed to stderr as a diff; `check --json` adds `"fixed":N`); `nyra run prog.nyra --fix` then runs
the program. If an error without a fix remains, the file is not changed and you get the errors as usual.
**Use `--fix` before you spend a model call on a repair.** In the JSON each fix is a list of edits:

```json
{"code":"E0005","message":"unexpected `;`: Nyra has no semicolons","file":"a.nyra","line":2,"col":14,"hint":"delete the `;`: ...","fix":[{"line":2,"col":14,"end_line":2,"end_col":15,"text":""}]}
```

Each edit replaces the text from `line`:`col` up to, not including, `end_line`:`end_col` with `text`
(columns count characters from 1). To apply them yourself, go from the last edit to the first. An error
with several possible repairs has a `hint` and no `fix` (a typo, `null`, `:=`, `n + 0.5` with an `int` n):
those are yours to decide.

**Check your logic with examples, before running anything.** After a function, write what it should
give for one or two inputs, worked out by hand:

```rust
fn sq(x: int) -> int = x * x   ex sq(3) == 9, sq(-2) == 4

fn largest(xs: [int]) -> int {
    var best = xs[0]
    for x in xs {
        if x > best { best = x }
    }
    ret best
}
ex largest([3, 9, 4]) == 9, largest([1, 2, 7]) == 7, largest([-5]) == -5

fn main() {
    print(largest([sq(2), 3]))
}
```

`ex` takes `bool` conditions separated by commas, at the end of a one-line function or on lines after
a function's closing `}`. They may use only literals and calls (no variables), they are never compiled
into the program, and `nyra check` runs every one of them while compiling: a wrong function becomes a
compile error with the value it really gave, at no cost when the program runs. A `largest` that
skips the last element (`for i in 1..xs.len() - 1`) gets:

```json
{"code":"E0250","message":"example `largest([1, 2, 7]) == 7` is false: `largest([1, 2, 7])` is 2, not 7","line":10,"col":29,"hint":"if the example is right, `largest` is wrong for xs = [1, 2, 7]: trace `largest` with these values and fix it; ...","actual":"2","expected":"7"}
```

- Write 1-2 examples for every function with a loop, a branch or arithmetic you could get wrong;
  skip them for one-line wrappers. Include one edge case: 0, an empty array or string, a negative
  number, the last element, two equal values.
- Work the expected value out yourself. Never copy it from the program's output: the example would
  then confirm the bug.
- When an example fails (E0250), assume the function is wrong and trace it with the arguments the
  hint names. Change the example only if you are sure it expects the wrong value.
- E0251 means the example hits a runtime error inside the function (an empty array, a division by
  0): handle that input in the function, or give the example an input the function is meant to take.
  E0253 means it did not finish (a loop that never ends); keep examples small.
- Compare floats with a range, not `==`: `ex mean([1.0, 2.0]) > 1.49, mean([1.0, 2.0]) < 1.51`.

Exit codes: `0` ok, `1` compile errors, `2` usage or tool problem (for example no C compiler: use `--js`),
`101` runtime error (see the bottom of section 5). `nyra run prog.nyra --json` reports runtime errors as JSON too.

**If you have the `nyra` MCP server** (`nyra mcp`, added with `claude mcp add nyra -- nyra mcp`), the
same loop needs no files: `nyra_check {code}` returns the JSON above (failed examples included),
`nyra_test {code}` the result of every example, `nyra_run {code, backend}` returns
`stdout`, `exit` and runtime `errors`, `nyra_explain {code: "E0201"}` an error entry, and `nyra_spec`
the language spec. `nyra_outline`, `nyra_show` and `nyra_edit {path or code, edits}` edit a program by
symbol, as described next.

**Changing a program that already exists: edit by symbol, do not resend the file.** Rewriting a whole
file to change one function costs the whole file in output tokens on every turn. Nyra addresses
functions, structs and fields (`Struct.field`) by name:

```
nyra outline prog.nyra             # one line per symbol: `45-52 fn find(items: [Item], sku: str) -> int`
nyra show prog.nyra find Item.tags # the source of those symbols, exactly as in the file
nyra edit prog.nyra < change.txt   # apply an edit script (below); --json for a JSON summary
nyra edit prog.nyra --set find "fn find(items: [Item], sku: str) -> int = items.len()"
nyra edit prog.nyra --rename find index_of_sku   # the definition and every call
```

The edit script (stdin, or `edits` of the `nyra_edit` tool) is either plain definitions, each of which
replaces the symbol of the same name or is added at the end, or commands, each followed by its code:

```
@replace discount
fn discount(total: int) -> int {
    ret total / 10
}
@add after discount
fn tax(total: int) -> int = total / 5
@delete old_helper
@rename Item.stock in_stock
@add-field Order note: str after customer
```

- Each edit replaces the exact source range of its symbol; every other byte of the file stays as it
  was (line breaks too). The comment lines right above a definition belong to it: `@replace` keeps
  them unless the new code starts with its own `//` lines, `@delete` removes them.
- `@rename` changes the definition and the real references only (calls, constructions, type
  annotations, `x.field` reads and `field:` labels), never strings, comments or a same-named field of
  another struct. The new name must not be used anywhere in the file yet.
- The edits of one script apply in order and the result is checked once: an edit that **adds** errors is
  refused, nothing is written, and you get the errors with the symbol each is in. So a change that
  needs several symbols (a new field and the functions that build the struct) goes in one script.
  `--force` applies anyway; `--fix` first repairs mistakes that have a certain fix (as in `check --fix`).
  Errors that the file already had do not block an edit, so you can repair a broken file one symbol at
  a time.
- The top-level statements of a script are not a symbol: give the program a `fn main` if you will edit it.

What it saves, measured on `tests/edit/inventory.nyra` (209 lines, 5072 bytes, about 1450 tokens):
changing one function (`discount`) by sending the file again costs the whole file; `nyra edit` with the
new function costs 208 bytes sent and a 58-byte reply (about 75 tokens), **95% less**. Reading the
file first as `nyra outline` (1195 bytes) and `nyra show discount` (270 bytes) instead of in full saves
most of the reading side too.

**If you cannot run commands** (you are answering in a chat): follow the rules below, go through the
checklist in section 8, and give the user the code, the output you expect, and the command to run it
(`nyra run prog.nyra`). Binaries: <https://github.com/SpAkXD/nyra/releases/latest>. `nyra --version`
must be 0.3 or newer for chars, string methods, arrays, structs, `inout`, `break` and `continue`
(0.2 has none of them).

## 2. The language at a glance

```rust
// A comment. Files end in .nyra. No semicolons anywhere.

struct Item {                                 // a struct: named fields, uppercase name
    name: str
    price: int
}

fn add(a: int, b: int) -> int = a + b         // one-line function: the expression is the result
fn shout(msg: str) = print("{msg}!")          // no `->`: returns nothing
fn limit() -> int = 100                       // a constant: a function, or a script `let`

fn gcd(a: int, b: int) -> int {               // block body: return with `ret`
    if b == 0 { ret a }
    ret gcd(b, a % b)
}
ex gcd(48, 18) == 6, gcd(7, 0) == 7           // examples: checked while compiling, never run

fn total(items: [Item]) -> int {
    var sum = 0
    for it in items { sum += it.price }       // `for x in xs`: each element of an array
    ret sum
}

fn double_all(inout xs: [int]) {              // `inout`: may change the caller's variable
    for i in 0..xs.len() { xs[i] *= 2 }
}

// the program: statements at the top level run in order (no `fn main` needed)
let x = 7                                 // immutable, type inferred (int)
var count = 0                             // mutable; top-level variables are visible in every function
let ratio: float = 2.5                    // type annotation is optional
count += x                                // also -= *= /= %=  (var only)

if count > 20 && x != 0 {                 // the condition must be a bool
    print("big")
} else if count == 7 {
    print("seven")
} else {
    print("small")
}
while count > 0 { count -= 5 }
for i in 0..3 { print(i) }                // 0, 1, 2: the end is exclusive
let bigger = if x > 3 { x } else { 3 }    // `if` as a value (needs `else`)

var nums = [3, 1, 2]                      // an array of int: [int]
nums.push(10)                             // changing an array needs `var`
nums.sort()                               // [1, 2, 3, 10]
double_all(inout nums)                    // the call says `inout` too
print(nums)                               // [2, 4, 6, 20]
var names: [str] = []                     // an empty array needs its type
names.push("ann")

let words = "red green blue".split(" ")   // ["red", "green", "blue"]
let w = words[2] + "!"                    // `+` joins two strings: "blue!"
print("{w} has {w.len()} chars and starts with {w[0]}")
for c in "hey" {                          // each char of a string
    if c == 'e' { continue }              // chars use single quotes; `break` exists too
    print(c.upper())                      // H, then Y
}

let cart = [Item(name: "pen", price: 3), Item(name: "ink", price: 9)]
print(total(cart))                        // 12
print(cart[0])                            // Item(name: "pen", price: 3)

print("x = {x}, sum = {add(x, 2)}, gcd = {gcd(48, 18)}")
print(float(x) / ratio)                   // 2.8: conversions are explicit, float(...) and int(...)
print(7 / 2)                              // 3: int division truncates
print(bigger + limit())                   // 107
shout("done")
```

Types: `int` (64-bit), `float` (64-bit), `bool`, `str`, `char`, arrays `[T]` and the structs you declare.
Function signatures are always fully typed; local types are inferred. The builtins are `print(x)`,
`str(x)`, `int(x)`, `float(x)`, `char(n)`, `free(x)` and `keep(x)`; everything else on strings, chars
and arrays is a method (`s.len()`, `xs.push(v)`, `c.code()`): the full lists are in the spec.

## 3. Rules: do and don't

| Rule | Don't (error) | Do |
|---|---|---|
| Only `fn` and `struct` at top level | `let max = 10` at top level (E0101) | `fn max() -> int = 10` |
| No semicolons | `let x = 1;` (E0005) | `let x = 1` |
| One statement per line | `let a = 1 let b = 2` (E0101) | two lines |
| `{` on the same line | `fn main()` newline `{` (E0101) | `fn main() {` |
| Braces are required | `if x > 0 print(x)` (E0101) | `if x > 0 { print(x) }` |
| Typed signatures | `fn f(a) -> int` (E0101), `a: string` (E0102) | `fn f(a: int) -> int` |
| No implicit conversion | `1 + 2.0` (E0210), `let f: float = 2` (E0203) | `float(1) + 2.0`, `let f = 2.0` |
| Only `var` changes | `let n = 0` then `n = 1`, `let xs = [1]` then `xs.push(2)` (E0205) | `var n = 0`, `var xs = [1]` |
| Parameters are immutable | `n = n / 2` or `xs.push(1)` on a parameter (E0205) | `var m = n`, then change `m`; or an `inout` parameter |
| `inout` at both ends | `swap(x, y)` for `fn swap(inout a: int, inout b: int)` (E0237) | `swap(inout x, inout y)` |
| Two `inout` arguments, two variables | `swap(inout xs[i], inout xs[j])` (E0237) | `xs.swap(i, j)` |
| No shadowing | `let x = 1` ... `let x = 2` in one function (E0206) | a new name, or `var` and reassign |
| Unique names | `let str = "a"`, `for char in s`, a variable named like a function (E0206) | `let text = "a"`, `for c in s` |
| Bool conditions | `if n {`, `if xs {` (E0209) | `if n != 0 {`, `if xs.len() > 0 {` |
| Return with `ret` | `return x` (E0101) | `ret x` |
| Every path returns | `if x > 0 { ret 1 }` as the last statement (E0207) | add `ret 0` after it, or `else { ret 0 }` |
| Scripts need no `main` | `fn main()` around everything (it works, but costs tokens) | statements at the top level |
| Functions see script variables | `inout pos: int, inout tokens: [Token]` on every helper; or `fn main` locals used in a function (E0201) | `var pos = 0` at the top level, then `fn advance() { pos += 1 }`; declare it before the first call (E0217) |
| Ranges are exclusive | `0..=9` (E0101), `0.0..1.0` (E0203), `'a'..'z'` (E0210) | `0..10` with int bounds |
| `print` joins values with a space | `print(a + " " + b)` | `print(a, b)` |
| `+` joins two strings | `"n=" + 5`, `s + c` with a char `c` (E0210) | `"n=" + str(5)`, `"n={n}"`, `s + str(c)` |
| Chars use single quotes | `s[0] == "a"` (E0210), `'ab'` (E0007) | `s[0] == 'a'`, `"ab"` |
| Char codes are explicit | `int(c)` (E0203), `c + 1` (E0210) | `c.code()`, `char(c.code() + 1)` |
| Length is a method | `len(xs)` (E0202), `xs.length` (E0224), `xs.len` (E0236) | `xs.len()`, `s.len()` |
| Empty arrays need a type | `var xs = []` (E0230) | `var xs: [int] = []` |
| One type per array | `[1, 2.5]`, `[1, "a"]` (E0231) | `[1.0, 2.5]`, or an array of structs |
| No negative indexes, no slice syntax | `xs[-1]` (E0261), `xs[1..3]` (E0101) | `xs[xs.len() - 1]`, `xs.slice(1, 3)` |
| Structs are built with a call | `Point { x: 1, y: 2 }` (E0101), `Point(1, 2)` (E0225) | `Point(x: 1, y: 2)` |
| Struct names are uppercase | `struct point` (E0221) | `struct Point` |
| No methods on structs | `p.area()` (E0227), `fn` inside `struct` or `impl P { }` (E0263), `class` (E0264) | `fn area(p: Point) -> int`, then `area(p)` |
| `{x}` inserts a value, `$` is text | `"cost: ${x}"` prints `cost: $3` (warning E0260) | `"cost: {x}"`; `"$" + str(x)` for a dollar sign |
| No optional types, no null | `int?`, `Option<int>` (E0262) | return `-1` or `""`, or a `bool` flag; `m.has(k)` before `m.get(k)` |
| No quotes inside `{ }` | `print("{f("a")}")` | `let t = f("a")`, then `print("{t}")` |
| Comments | `# note`, `/* note */` (E0001, E0101) | `// note` |
| Examples go outside functions | `ex f(1) == 2` inside a body (E0101), `ex f(1)` (E0252) | after the closing `}`: `ex f(1) == 2` |
| Literals | `.5`, `5.`, `1e5`, `1_000`, `0xFF` (E0001, E0101) | `0.5`, `5.0`, `100000.0`, `1000`, `255` |
| Operators | `and`, `or`, `i++`, `2 ** 3`, `a < b < c`, `c ? a : b` (E0101, E0210, E0001) | `&&`, `i += 1`, `2 * 2 * 2`, `a < b && b < c`, `if c { a } else { b }` |

Good to know:

- Operators, high to low: calls, fields, indexes and methods · `-x` `!x` · `*` `/` `%` · `+` `-` ·
  `<` `<=` `>` `>=` · `==` `!=` · `&&` · `||`. `%` is for ints only. `==` and `!=` need the same type on
  both sides and compare strings, arrays and structs by content. `<` and `>` work on `int`, `float`,
  `str` and `char`. `&&` and `||` skip their right side when the left side decides.
- Int division truncates toward zero: `7 / 2` is `3`, `-7 / 2` is `-3`, `-7 % 3` is `-1`.
- `print` accepts any one value. Floats print in the shortest form that reads back exactly, and whole
  floats have no `.0`: `print(2.0)` prints `2`, `print(0.1 + 0.2)` prints `0.30000000000000004`.
  Arrays and structs print as code, with strings and chars inside them quoted: `["a", "b"]`,
  `['x']`, `Item(name: "pen", price: 3)`. `str(x)` gives the same text as a `str`.
- Float math shows its rounding noise: multiplying 1000.0 by 1.05 four times prints
  `1215.5062500000001`. When you state the expected output of float math, call it approximate.
- Ints are 64-bit and never wrap: an overflow stops the program with E0255. With `--js` and `--ts` an
  int beyond 2^53 - 1 (9007199254740991) stops it with E0256. For a hash or a random number generator,
  keep the value small with `%` at every step: `h = (h * 31 + c.code()) % 1000000007`.
- A name declared inside `{ }` is gone after the closing brace (you may reuse it then). Functions and
  structs can be defined in any order and functions can call each other. `ret` with no value leaves a
  function that returns nothing.
- An `if` used as a value needs an `else`, one expression per branch, and the same type in both
  branches. `else if` chains work and the branches may span lines. A one-line function's expression
  starts on the same line as its `=`.
- Values are copies. `var b = a` copies an array, a string or a struct, and so does passing it to a
  function or storing it in another array; changing the copy never changes the original. Copies are
  cheap (the data is shared until one side changes). To let a function change the caller's variable,
  use an `inout` parameter; otherwise return the new value (`xs = with_item(xs, 5)`).
- `for x in xs` loops over the array as it was when the loop started, and `x` is a copy you cannot
  assign to: to change elements, loop over the indexes, `for i in 0..xs.len() { xs[i] = ... }`.
- Strings count characters, not bytes: `"héllo".len()` is 5 and `"héllo"[1]` is `'é'`. `s[i]` is
  instant on ASCII text; on text with other characters each `s[i]` walks the string natively, so in a
  loop over such text use `for c in s`, or `let cs = s.chars()` once and index `cs[i]`.
- `upper`, `lower` and the `is_*` tests know ASCII letters only: `'é'.is_letter()` is false and
  `"é".upper()` is `"é"`. `s.split(" ")` keeps empty parts: `"a  b".split(" ")` is `["a", "", "b"]`.
- Every `{` in a string starts an interpolation, so text with braces doubles them:
  `"{{[()]}}"` is `{[()]}`. With single braces the compiler reads what is between them as code and
  reports an error about that code (for `"{[]}"` it says it cannot infer the type of `[]`).
- Building text or arrays in a loop is fine: `s += "x"` and `xs.push(v)` change the value in place
  when nothing else shares it.
- An `int` `/` or `%` by zero, an index out of range, `pop()` on an empty array, `int("12x")` and the
  other runtime errors in section 5 stop the program with exit code 101, the same on both backends.
  Float division by zero is fine and prints the same everywhere: `1.0 / 0.0` is `Infinity`,
  `0.0 / 0.0` is `NaN`.
- Recursion is fine to a depth of a few thousand calls. Much deeper recursion crashes without a Nyra
  error (`RangeError` with `--js`; a native program dies and may lose its last output): use a loop.
- Memory is freed automatically. `free(x)`, `arena { }` and `keep(x)` only change when memory is
  returned, never what a program prints, so a correct program never needs them.
- Newlines inside `( )` are ignored and trailing commas are fine; an array literal may also break
  after `[` and after each `,`. You may break a line after a binary operator, never before it, and a
  line cannot start with `.`.
- Style: 4 spaces, `snake_case` for functions and variables, `CamelCase` for structs, short functions,
  `//` comments that say why.

## 4. What does not exist (yet)

Do not use any of these. If the task needs one, say so in a sentence and write the closest program
that works (section 6 has the usual replacements).

- **Network**: no sockets or HTTP. Input, arguments, files, the clock, random numbers, JSON and math
  are in the standard library (section 6b).
- **Types**: no sets, tuples, enums, `Option`, `Result`, generics or type aliases. Use a map
  `[str: bool]` or `contains` for a set, a struct for a tuple. Maps `[K: V]` exist (section 6).
- **Methods you may expect**: arrays have no `reduce` (write `fold`), `find` (`find_index`), `append`
  (`push`) or `flatten`; strings have no `format`, `pad`, `trim_start`, `char_at` or `is_digit` (chars
  have `is_digit`). `sort()` works only on `[int]`, `[float]`, `[str]` and `[char]`, ascending; sort
  anything else with `sort_by(x => key)`.
- **Syntax**: no slices `xs[a..b]` (`xs.slice(a, b)`), no negative indexes, no `match`, `switch`, `?:`,
  `do-while`, `loop`, labeled `break` or `elif`. A comprehension has one `for` and no index.
- **Declarations**: no global variables outside a script (a script's top-level variables are visible in
  functions; in a program with `fn main` nothing is), no closures, function values, overloading, default
  arguments, methods on structs (`impl`, `self`) or modules of your own. One file is one program; it may
  `use` the standard modules (section 6b). A lambda (`x => x * 2`) is only an argument of an array
  method, and it cannot change variables.
- **Library**: `abs`, `min` and `max` are builtins; everything else is a module function, never a
  global one: `math.sqrt(x)` after `use math`, not `sqrt(x)`.
- **Errors**: no exceptions, `null`, `assert` or `panic`. A failing operation stops the program with a
  runtime error (section 5); `os.exit(code)` stops it on purpose. To check a function, write examples:
  `ex f(2) == 4` (section 1).
- **Output**: `print` ends the line unless its last argument is `end:` (`print(x, end: " ")`). There is
  no `printf` and no format specifier (`{x:.2f}` is an error): a float prints in its shortest form; for
  a fixed number of decimals use `text.fixed(x, 2)` (section 6b). Pad text with `s.pad_left(n)`.

## 5. Error codes

Short table. The full database, with the reason for each rule and a wrong and a fixed program for every code, is
[ERRORS.md](https://raw.githubusercontent.com/SpAkXD/nyra/main/docs/ERRORS.md); `nyra explain E0201` prints an entry.

| Code | Meaning | Usual cause and fix |
|---|---|---|
| E0001 | unexpected character | `#`, `?`, `.5`, `5.`, single `&` or `\|`: use `//`, `if` / `else`, `0.5`, `&&` |
| E0002 | unterminated string | strings end on the same line: close with `"`, use `\n` for line breaks |
| E0003 | number too large | `int` max is 9223372036854775807 |
| E0004 | unknown escape | strings know `\n` `\t` `\r` `\\` `\"`; chars also `\'` |
| E0005 | semicolon | delete it |
| E0007 | bad char literal | `'ab'`, `''`: a char holds exactly one character; text uses `"ab"` |
| E0101 | syntax error | `return`, `elif`, `i++`, `0..=n`, `{` on a new line, `Point { x: 1 }`, `xs[1..3]`, two statements on a line, `break` outside a loop: compare with section 2 |
| E0102 | unknown type | `int`, `float`, `bool`, `str`, `char`, `[T]` or a declared struct (not `string`, `i32`, `Char`, `list`, `dict`) |
| E0201 | undefined variable | typo (see `hint`), used before its `let`, or declared in another block |
| E0202 | undefined function | the builtins are `print`, `str`, `int`, `float`, `char`, `abs`, `min`, `max`; `len(xs)` is `xs.len()`, `sqrt(x)` is `math.sqrt(x)` after `use math`; else define it yourself |
| E0300 · E0306 | module not found · module has no such item | the modules are `input os fs json time random math text`; `random.range(1, 7)`, not `randint` |
| E0309 | `json.parse` needs a type | `let p: Point = json.parse(text)` |
| E0203 | type mismatch | wrong argument, return or assigned type: `float(x)` / `int(x)`, write `2.0` not `2`; `int(c)` of a char: `c.code()` |
| E0204 | wrong argument count | functions and methods take a fixed number (`s.split(" ")`, `min(a, b)`); `print(a, b, end: "")` takes any number of values and an optional last `end:` |
| E0205 | changing what is not a `var` | `let` variables, parameters and loop variables are immutable: use `var`, or an `inout` parameter |
| E0206 | name already defined | no shadowing, no duplicate functions or structs, no variable named like a function or a struct |
| E0207 | bad or missing `ret` | end every path of a `->` function with `ret value`; no `ret value` without `->` |
| E0208 | no `fn main()` and no top-level statements | write the program's statements at the top level (a script) |
| E0209 | condition is not `bool` | compare: `if n != 0`, `if xs.len() > 0` |
| E0210 | operator on wrong types | `int + float`, `"a" + 1`, `"a" + 'b'`, `'a' + 1`, `c == "a"`, `xs + 5`, `"a" < 1`, `a < b < c` |
| E0211 | bad `main` | no parameters and no return type |
| E0212 | bad `if` value | an `if` used as a value needs an `else`, exactly one expression per branch, and the same type in both |
| E0213 | lambda used as a value | a lambda is only an argument: `xs.map(x => x * 2)`; to name it, write a `fn` |
| E0214 | lambda changes a variable | variables are read-only inside a lambda: use the method's result, or a `for` loop |
| E0215 | bad lambda argument | `xs.count(x => x == 3)`, not `xs.count(3)`; `fold` takes `(acc, x) => ...` |
| E0216 | script variable hidden by a local | a function that declares `x` cannot use the script's `x`: rename its own one |
| E0217 | script variable used before it exists | a call runs the function: declare the script variables it uses (also through calls) above the call |
| E0220 | field defined twice | rename one of them |
| E0221 | lowercase struct name | `struct Point`, not `struct point` |
| E0222 | struct contains itself | keep the children in an array: `kids: [Node]` |
| E0223 | missing field | give every field: `Point(x: 1, y: 2)` |
| E0224 | unknown field | check the name (see `hint`); arrays and strings have methods, not fields: `xs.len()` |
| E0225 | fields unnamed or out of order | `Point(x: 1, y: 2)`, in declaration order |
| E0226 | named argument to a function | names are only for structs: `add(1, 2)` |
| E0227 | unknown method | the `hint` lists the methods of the type; structs have none: `fn area(r: Rect)` |
| E0228 | method needs another element type | `join` needs `[str]` or `[char]`; `sort`, `min`, `max` and `sort_by` keys need `int`, `float`, `str` or `char`; `sum` needs numbers |
| E0229 | cannot change this | strings are immutable (`s[0] = 'x'`); a temporary (`f().push(1)`); `inout` needs a variable |
| E0230 | type of `[]` unknown | `var xs: [int] = []` |
| E0231 | mixed array elements | one type per array; a struct for mixed data |
| E0232 | index is not an `int` | `xs[int(f)]` |
| E0233 | indexing a non-array | only arrays and strings can be indexed |
| E0234 | looping over a non-array | `for i in 0..n`, or loop over an array or a string |
| E0235 | struct name used as a value | build one: `Point(x: 1, y: 2)` |
| E0236 | method without `()` | `xs.len()` |
| E0237 | wrong `inout` | `inout` at the call exactly when the parameter is `inout`; two `inout` arguments must be different variables |
| E0238 | wrong `free`, `keep` or `arena` | only local `str`, array or struct variables; outer strings and arrays are read-only inside `arena` |
| E0239 | use after `free` | give the `var` a new value first, or move `free` after the last use |
| E0250 | example is false | the message has both values: fix the function for the arguments in the hint (or the example, if it expects the wrong value) |
| E0251 | example stops with a runtime error | the function fails for that input: handle it (an empty array, 0), or use an input it accepts |
| E0252 | example is not a `bool` | `ex sq(3) == 9`, not `ex sq(3)` |
| E0253 | example did not finish | a loop or a recursion that never ends for that input; or an input that is too big |
| E0254 | example calls a function that uses script variables | examples run before the script: pass the value as a parameter, or drop the example |
| E0260 | (warning) `${x}` in a string prints a `$` and the value | `"cost: {x}"`; the build goes on, `--json` lists it under `"warnings"` |
| E0261 | negative constant index or slice position | `xs[xs.len() - 1]`, `xs.slice(xs.len() - 2, xs.len())` |
| E0262 | optional type (`int?`, `Option<int>`) | there is no null: return `-1` or `""`, or a `bool` flag; `m.has(k)` before `m.get(k)` |
| E0263 | `fn` inside a `struct`, or an `impl` block | write the function outside: `fn area(r: Rect)` |
| E0264 | `class` | `struct Rect { w: int }` and plain functions |

**Runtime errors** stop a running program with exit code 101, after everything it printed so far:

```
runtime error[E0240]: index 3 is out of bounds for length 3
  --> prog.nyra:4:13
  = hint: valid indexes are 0 to len - 1; compare with `.len()` first
```

| Code | Meaning | Fix |
|---|---|---|
| E0240 | index or range out of bounds | check `i < xs.len()`; `slice(a, b)` needs `0 <= a <= b <= len` |
| E0241 | integer `/` or `%` by zero | check the divisor first (`if d != 0 { ... }`) |
| E0242 | `pop()` on an empty array | check `xs.len() > 0` first |
| E0243 | bad argument | `repeat(n)` needs `n >= 0`; `split` and `replace` need a non-empty pattern |
| E0244 | text is not a number | `int(s)` takes digits with an optional `-`; `float(s)` also a `.` part and an exponent |
| E0245 | `int(x)` of NaN, infinity or a float too big for an `int` | check the value before converting |
| E0246 | `char(n)` of an invalid code | codes go from 0 to 1114111, except 55296 to 57343 |
| E0247 | `min()` or `max()` of an empty array | check `xs.len() > 0` first, or use `fold` with a start value |
| E0249 | out of memory | `repeat` makes at most 536,870,888 bytes of text (UTF-16 units with `--js`, the same for ASCII) or 100,000,000 elements |

## 6. Recipes

Small helpers you write yourself (`abs`, `min` and `max` are builtins, but a program may define its own):

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

Arrays and text:

```rust
fn sum(xs: [int]) -> int {
    var total = 0
    for x in xs { total += x }
    ret total
}

fn largest(xs: [int]) -> int {                 // xs must not be empty
    var best = xs[0]
    for x in xs {
        if x > best { best = x }
    }
    ret best
}

fn reversed(s: str) -> str {                   // reversed("abc") is "cba"
    var cs = s.chars()
    cs.reverse()
    ret cs.join("")
}

fn pad_left(s: str, width: int) -> str {       // pad_left("7", 3) is "  7"
    if s.len() >= width { ret s }
    ret " ".repeat(width - s.len()) + s
}

fn digit(c: char) -> int = c.code() - '0'.code()   // digit('7') is 7

fn words(text: str) -> [str] {                 // words(" a  b ") is ["a", "b"]
    var out: [str] = []
    for w in text.split(" ") {
        if w != "" { out.push(w) }
    }
    ret out
}
```

Count with a map (`[K: V]`, keys `int`, `str`, `char` or `bool`; reading a missing key with `m[k]` is a
runtime error, so count with `get` and a default):

```rust
fn word_counts(text: str) -> [str: int] {
    var counts: [str: int] = [:]
    for w in text.split(" ") {
        if w != "" { counts[w] = counts.get(w, 0) + 1 }
    }
    ret counts
}

fn main() {
    let counts = word_counts("a b a")
    for w in counts { print("{w}: {counts[w]}") }   // in insertion order: a: 2, b: 1
    print(counts.has("c"), counts.keys())
}
```

A custom sort is an insertion sort (it keeps equal elements in their order):

```rust
struct Count {
    word: str
    n: int
}

fn sort_by_n(inout cs: [Count]) {              // largest n first
    for i in 1..cs.len() {
        let c = cs[i]
        var j = i
        while j > 0 && cs[j - 1].n < c.n {
            cs[j] = cs[j - 1]
            j -= 1
        }
        cs[j] = c
    }
}
```

Leave a loop early with `break` (only the innermost loop) or leave the whole function with `ret`:

```rust
fn first_divisor(n: int) -> int {              // smallest divisor above 1
    var found = n
    for d in 2..n {
        if n % d == 0 {
            found = d
            break
        }
    }
    ret found
}
```

Count down, and build a line of text:

```rust
var i = 3
while i > 0 {
    print(i)                                // 3, 2, 1
    i -= 1
}

var row = ""
for j in 0..5 { row += "*" }
print(row)                                  // *****
print("-".repeat(5))                        // -----
```

## 6b. The standard library

Import a module with `use name` at the top of the file (one per line) and call its functions with the
module's name. The modules: `input`, `os`, `fs`, `json`, `time`, `random`, `math`, `text`; the full list
of functions is in the spec. Read numbers from standard input, one per line, until it ends:

```rust
use input

fn main() {
    var total = 0
    while !input.eof() {
        let line = input.line().trim()
        if line != "" { total += int(line) }
    }
    print("total: {total}")
}
```

Arguments, files and the exit code (`nyra run prog.nyra -- notes.txt` passes `notes.txt`):

```rust
use os
use fs

fn main() {
    let args = os.args()
    if args.len() == 0 {
        print("usage: prog <file>")
        os.exit(2)
    }
    let path = args[0]
    if !fs.exists(path) {
        fs.write(path, "first line\n")
    }
    fs.append(path, "one more line\n")
    print("{fs.read(path).split("\n").len() - 1} lines")
}
```

JSON is read into the type the value goes to (a typed `let`, a parameter, a field, `ret`); a struct
reads an object by field names, an array a list:

```rust
use json

struct Point {
    x: int
    y: int
}

fn main() {
    let ps: [Point] = json.parse("[{{\"x\": 1, \"y\": 2}}, {{\"x\": 3, \"y\": 4}}]")
    print(ps[1].y)                              // 4
    print(json.str(ps[0]))                      // {"x":1,"y":2}
}
```

Random numbers, timing, math and decimals:

```rust
use random
use time
use math
use text

fn main() {
    random.seed(42)                             // the same numbers in every run; leave it out for real randomness
    let die = random.range(1, 7)                // 1 to 6: the upper bound is excluded
    let start = time.mono_ms()
    let r = math.sqrt(2.0) * math.pow(2.0, 10.0)
    print(die >= 1, text.fixed(r, 2))           // true 1448.15
    print("took {text.fixed(time.mono_ms() - start, 1)} ms")
}
```

## 7. Complete programs

FizzBuzz (`for`, `else if`, `%`). Prints `1`, `2`, `Fizz`, `4`, `Buzz`, `Fizz`, `7`, `8`, `Fizz`, `Buzz`,
`11`, `Fizz`, `13`, `14`, `FizzBuzz`, one per line:

```rust
for i in 1..16 {
    if i % 15 == 0 { print("FizzBuzz") }
    else if i % 3 == 0 { print("Fizz") }
    else if i % 5 == 0 { print("Buzz") }
    else { print(i) }
}
```

Word counts (a struct, an array of structs, `split`, early `ret`, changing an element's field).
Prints `the: 4`, `cat: 2`, `saw: 2`, `dog: 2`, then `6 different words`:

```rust
struct Count {
    word: str
    n: int
}

fn index_of_word(counts: [Count], word: str) -> int {
    for i in 0..counts.len() {
        if counts[i].word == word { ret i }
    }
    ret -1
}

let text = "the cat saw the dog and the dog saw the cat run"
var counts: [Count] = []
for w in text.split(" ") {
    let i = index_of_word(counts, w)
    if i == -1 {
        counts.push(Count(word: w, n: 1))
    } else {
        counts[i].n += 1
    }
}
for c in counts {
    if c.n > 1 {
        print("{c.word}: {c.n}")
    }
}
print("{counts.len()} different words")
```

Title case and an acronym (chars, `str(c)`, `slice`, `join`). Prints `Portable Network Graphics`, then
`PNG`:

```rust
fn capitalized(word: str) -> str {
    if word == "" { ret word }
    ret str(word[0].upper()) + word.slice(1, word.len())
}

let name = "portable network graphics"
var title: [str] = []
var acronym = ""
for w in name.split(" ") {
    title.push(capitalized(w))
    acronym += str(w[0].upper())
}
print(title.join(" "))
print(acronym)
```

One step of Conway's Game of Life (a 2D array `[[bool]]`, copies, nested loops). Prints a horizontal
line of three `#` in a 5 by 5 grid of `.`, an empty line, then the same grid with the line vertical:

```rust
fn neighbours(g: [[bool]], r: int, c: int) -> int {
    var n = 0
    for dr in -1..2 {
        for dc in -1..2 {
            let rr = r + dr
            let cc = c + dc
            if (dr != 0 || dc != 0) && rr >= 0 && rr < g.len() && cc >= 0 && cc < g[rr].len() {
                if g[rr][cc] { n += 1 }
            }
        }
    }
    ret n
}

fn step(g: [[bool]]) -> [[bool]] {
    var next = g                               // a copy: changing it leaves g as it was
    for r in 0..g.len() {
        for c in 0..g[r].len() {
            let n = neighbours(g, r, c)
            next[r][c] = n == 3 || (g[r][c] && n == 2)
        }
    }
    ret next
}

fn show(g: [[bool]]) {
    for row in g {
        var line = ""
        for alive in row {
            line += if alive { "#" } else { "." }
        }
        print(line)
    }
}

var grid = [[false].repeat(5)].repeat(5)
grid[2][1] = true
grid[2][2] = true
grid[2][3] = true
show(grid)
print("")
show(step(grid))
```

More programs with expected output live in
[`examples/`](https://github.com/SpAkXD/nyra/tree/main/examples): `strings`, `chars`, `arrays`, `structs`,
`inout`, `loops`, `memory`, `gcd`, `collatz`, `primes`, `math`, `interpolation`, `if_value`.

## 8. Checklist before you answer

1. The program is a script: statements at the top level, `fn` and `struct` definitions anywhere (no `fn main` needed).
   Its top-level variables are visible in every function (declare them before the first call that uses them).
2. No `;`, `return`, `elif`, `++`, `0..=n`, `xs[a..b]`, `Point { x: 1 }`, or Allman-style `{` on its own line.
3. Every name is unique inside its function (parameters, loop variables and locals), and no variable
   shares a name with a function or a struct.
4. Both sides of every operator have the same type; floats are written with a dot (`2.0`);
   conversions are explicit (`float(n)`, `int(x)`, `str(x)`, `c.code()`, `char(n)`).
5. Chars are in single quotes and compared with chars (`s[i] == 'a'`); `+` joins only two strings or two arrays.
6. Everything that changes is a `var` (or an `inout` parameter, written `inout` at the call too); empty
   arrays have a type (`var xs: [int] = []`).
7. Every `if` / `while` condition is a comparison or a `bool`, and every `->` function ends with `ret`
   on all paths (a one-line `=` function needs none).
8. Indexes stay in `0..len`, and literal braces in strings are doubled (`{{` `}}`); nothing inside
   `{ }` contains a `"`.
9. Every function with a loop, a branch or tricky arithmetic has 1-2 `ex` examples with values you
   worked out by hand, one of them an edge case (0, empty, negative, the last element).
10. You stated the output the program should print, and how to run it: `nyra run prog.nyra`.
