# Nyra v0.4 — language spec

This file is the whole language. It is short on purpose: paste it into an AI agent's
context and the agent can write Nyra. For common mistakes and complete examples, see
[AI_GUIDE.md](AI_GUIDE.md).

## Rules
- One way to do each thing. No implicit conversions. No shadowing. No null.
- A program is `fn` and `struct` definitions in any order plus either `fn main()` or statements at
  the top level (a script: they run in order, like the body of `main`). No global variables:
  functions cannot see the script's variables.
- Every function signature is fully typed. Local variable types are inferred.
- One statement per line. There are no semicolons. A line may break inside `( )`, between the
  elements of `[ ]`, and after a binary operator.
- Everything is evaluated left to right: arguments, operands and the parts of a string.
- Names use letters, digits and `_` (`row_count`, `x2`, `_`). Comments: `// to end of line`.

## Types
| type | values |
|---|---|
| `int` | 64-bit signed: `42`, `-7` |
| `float` | 64-bit: `2.5`, `3.0` (always a dot with digits on both sides) |
| `bool` | `true`, `false` |
| `str` | immutable UTF-8 text: `"hi"`, `""`; escapes `\n \t \r \\ \"` |
| `char` | one character: `'a'`, `'é'`, `'\n'`, `'\''` |
| `[T]` | array of `T`: `[1, 2]`, `[[1], []]`; an empty one needs its type: `var xs: [int] = []` |
| `Point` | a struct you declare |

## Functions
```nyra
fn add(a: int, b: int) -> int {
    ret a + b
}
fn square(x: int) -> int = x * x      // one-line function: the expression is returned
fn greet(name: str) = print("hi {name}")
fn show(n: int) {                     // no `->`: returns nothing
    if n < 0 { ret }                  // a bare `ret` leaves early
    print(n)
}
```
`fn main()` has no parameters and no return type. A function with a return type must `ret` on
every path. Functions may call each other in any order, and recurse. Parameters cannot be changed:
copy one into a `var`, or declare it `inout` (see Values).

## Variables
```nyra
let x = 5               // immutable
let y: float = 2.5      // optional type annotation
var n = 0               // mutable
n = n + 1               // only `var` can be reassigned
n += 1                  // also -= *= /= %=
```
A name cannot be declared again while it is visible (parameters and loop variables included).
A name declared inside `{ }` is gone after the `}`.

## Control flow
```nyra
let x = 3
var n = 0
if x > 3 { print("big") } else if x == 3 { print("three") } else { print("small") }
while n < 10 { n += 1 }
for i in 0..x + 1 { print(i) }   // 0 to 3: the end is exclusive
for i in 10..0 step -2 { print(i) }  // 10 8 6 4 2: `step` counts by any int but 0
for c in "hi" { print(c) }       // each char; `for v in xs` gives each element of an array
for i in 0..10 {
    if i % 2 == 0 { continue }   // next round of the innermost loop
    if i > 6 { break }           // leave the innermost loop
}
```
Conditions must be `bool` (write `x != 0`, not `x`). `{` stays on the line of its `if`, `else`,
`while`, `for` or `fn`. Range bounds are `int` expressions, evaluated once. Loop variables are
immutable and may go unused. `for v in xs` loops over `xs` as it was when the loop started.

`if` can also be a value. It needs an `else`, and each branch is one expression of the same type:
`let max = if a > b { a } else { b }`.

## Operators (high to low precedence)
| ops | types |
|---|---|
| `f(x)` `p.x` `xs[i]` `s.len()` | call, field, index, method |
| `-x` `!x` | `-` on int/float, `!` on bool |
| `*` `/` `%` | int,int or float,float (`%` int only; int `/` truncates toward zero) |
| `+` `-` | int,int or float,float; `+` also joins two `str`s or two arrays of one type |
| `<` `<=` `>` `>=` | int,int · float,float · str,str · char,char → bool |
| `==` `!=` | same type on both sides → bool (strings, arrays and structs compare by content) |
| `&&` `\|\|` | bool; the right side runs only when needed |

Parentheses group: `(a + b) * c`.

## Builtins
| call | meaning |
|---|---|
| `print(x)` · `print(a, b)` | print values of any type, separated by a space, then a newline |
| `print(a, end: "")` | a last `end:` replaces the newline: `print(x, end: " ")` keeps printing on one line |
| `str(x)` | any value → `str`, the text `print` shows |
| `int(x)` | float → int, truncating toward zero (NaN or out of range: E0245); str → int: `int("-42")` (other text: E0244) |
| `float(x)` | int → float; str → float: `float("2.5")`, `float("1e3")` (other text: E0244) |
| `char(n)` | code → char: `char(65)` is `'A'` (invalid code: E0246) |
| `abs(x)` · `min(a, b)` · `max(a, b)` | on `int`s or on `float`s (a program may define its own instead) |

## Strings and chars
`"{expr}"` inserts any value: `"{name}: {xs.len()} items"`. Every other `{` or `}` in a string is
doubled: `"{{[]}}"` is the text `{[]}`. Strings and chars may appear inside `{ }`: `"{xs.join(", ")}"`.
`a + b` joins two strings. Lengths and positions count characters (code points): `"héllo".len()`
is 5, and `s[i]` is a `char` (from 0). A char is not a `str` and not an `int`; convert explicitly:
```nyra
print('A'.code())                 // 65
print(char(66))                   // B
let t = "x" + str('y')            // "xy": str(c) makes a str
print("abc"[0] == 'a')            // true: compare chars with chars
print('7'.code() - '0'.code())    // 7: a digit's value
```
| method | result |
|---|---|
| `s.len()` · `s.slice(a, b)` | number of characters · characters `a` to `b - 1` |
| `s.contains(t)` `s.starts_with(t)` `s.ends_with(t)` | `bool` (`t` is a `str` or a `char`) |
| `s.index_of(t)` | first position of `t` (a `str` or a `char`), or `-1` |
| `s.pad_left(n)` `s.pad_right(n)` | spaces added until `s` has `n` characters (never shorter); `s.pad_left(n, '0')` pads with a char |
| `s.split(sep)` | `[str]`: `"a,b,,c".split(",")` is `["a", "b", "", "c"]` |
| `s.replace(old, new)` · `s.repeat(n)` | every `old` replaced · `n` copies |
| `s.trim()` | without leading and trailing spaces, tabs and newlines |
| `s.upper()` `s.lower()` · `s.chars()` · `s.codes()` | ASCII case · `[char]` · `[int]` |
| `c.code()` · `c.upper()` `c.lower()` | `int` · `char` (ASCII) |
| `c.is_digit()` `c.is_letter()` `c.is_upper()` `c.is_lower()` `c.is_space()` | `bool` (ASCII only) |

Chars compare by code (`'a' < 'b'`) and have no arithmetic: `char(c.code() + 1)`. `int(c)` is an
error: write `c.code()`.

## Arrays
```nyra
let xs = [3, 1, 2]                    // type [int]
var names: [str] = []                 // an empty [] needs a known type
names.push("ann")
print(xs[0] + xs[xs.len() - 1])       // indexes start at 0; no negative indexes
var grid = [[0].repeat(3)].repeat(2)  // [[0, 0, 0], [0, 0, 0]]
grid[1][2] = 5
let more = xs + [4, 5]                // a new array; xs is unchanged
```
| method | result |
|---|---|
| `xs.len()` | number of elements |
| `xs.push(v)` · `xs.pop()` | add at the end · remove and return the last |
| `xs.insert(i, v)` · `xs.remove(i)` | insert at index `i` (0 to len) · remove and return element `i` |
| `xs.swap(i, j)` | exchange elements `i` and `j` |
| `xs.contains(v)` · `xs.index_of(v)` | `bool` · first index or `-1` |
| `xs.slice(a, b)` · `xs.repeat(n)` | elements `a` to `b - 1` · `n` copies, one after another |
| `xs.sort()` · `xs.reverse()` | in place, return nothing (`sort`: `[int]` `[float]` `[str]` `[char]`) |
| `xs.join(sep)` | `[str]` or `[char]` → one `str` |

Changing an array (`xs[i] = v`, `+=`, `push pop insert remove swap sort reverse`) needs a `var`.

## Structs
```nyra
struct Point {
    x: int
    y: int
}
struct Line { a: Point, b: Point }    // commas separate fields on one line
fn shifted(p: Point, dx: int) -> Point = Point(x: p.x + dx, y: p.y)

let p = Point(x: 1, y: 2)             // name every field, in declaration order
var q = shifted(p, 10)
q.y += 5                              // fields of a `var` can be changed
print(Line(a: p, b: q))               // Line(a: Point(x: 1, y: 2), b: Point(x: 11, y: 7))
```
Struct names start with an uppercase letter. A struct cannot contain itself (use an array:
`kids: [Tree]`). Structs have no methods: write `fn area(r: Rect) -> int` and call `area(r)`.

## Values and `inout`
Assigning, passing, returning and storing always copy, so two variables never share data (copies
are cheap: data is shared until one side changes it). `var b = a` then `b.push(3)` leaves `a` as it
was. Only an `inout` parameter changes the caller's variable, and the call says `inout` too:
```nyra
fn swap(inout a: int, inout b: int) {
    let t = a
    a = b
    b = t
}
var x = 1
var y = 2
swap(inout x, inout y)                // x is 2, y is 1
```
The argument is a `var`, an `inout` parameter, or a field or element of one (`inout p.x`,
`inout xs[i]`). Two `inout` arguments of one call must be different variables.

## Memory
Memory is freed automatically: reference counting, no garbage collector. Three statements say
when. They are checked at compile time, and a program prints the same with or without them:

| write | effect |
|---|---|
| `free(x)` | frees `x`'s value now; later uses of `x` are error E0239 (a `var` may get a new value) |
| `arena { ... }` | its values are freed at `}`; outer strings, arrays and structs holding them are read-only inside, so a result leaves only through `ret` (copied out) |
| `keep(x)` | `x`'s value is never freed |

`free` and `keep` take a local `let`/`var` holding a `str`, an array or a struct that contains one.

## Printing
`print(x)`, `str(x)` and `"{x}"` show the same text. Arrays and structs print as Nyra code, with
strings and chars inside them quoted: `["a", "b"]`, `['a', '\n']`, `Point(x: 1, y: 2)`. Numbers print
like JavaScript's `String(x)`: `3.0` prints `3`, `0.1 + 0.2` prints `0.30000000000000004`,
`1.0 / 0.0` prints `Infinity`, never `-0`.

## Standard library
A `use` line at the top of the file imports a standard module; its functions are called with the
module's name. Nothing else is imported or installed.
```nyra
use math
use text

print(text.fixed(math.sqrt(2.0), 3))   // 1.414
```
| module | contents |
|---|---|
| `input` | `line()` the next line of standard input without its line end ("" at the end) · `lines()` all the rest as `[str]` · `all()` the rest as it is · `eof()` |
| `os` | `args()` the program's arguments, `[str]` · `env(name)` a variable ("" when not set) · `has_env(name)` · `exit(code)` stops now |
| `fs` | `read(path)` · `write(path, text)` · `append(path, text)` · `exists(path)` · `list(dir)` names, sorted · `remove(path)` a file or empty folder · `mkdir(path)` |
| `json` | `str(v)` any value as JSON · `parse(text)` reads the type the value goes to: `let p: Point = json.parse(s)` |
| `time` | `now_ms()` int, since 1970 · `mono_ms()` float, a monotonic clock for timing · `sleep_ms(ms)` |
| `random` | `random()` a float from 0 up to 1 · `range(lo, hi)` an int from lo to hi - 1 · `seed(n)` |
| `math` | `pi` `e` `inf` · `sqrt floor ceil round trunc exp log log10 log2 sin cos tan asin acos atan` (float) · `pow(x, y)` · `atan2(y, x)` |
| `text` | `fixed(x, digits)` decimals, `text.fixed(2.0 / 3.0, 2)` is "0.67" · `is_int(s)` · `is_float(s)`: would `int(s)`/`float(s)` work |

`nyra run main.nyra -- a b` passes the arguments `a b`. Random numbers come from the operating system
unless `random.seed(n)` was called; then they are the same sequence on every backend. `math` gives the
same digits on every backend; `round` rounds halves away from zero. Paths are relative to the folder the
program runs in and use `/`. JSON objects are read into structs by field name (other keys are skipped;
every field must be there), lists into arrays; `json.str` writes infinity and NaN as `null`. Text from
outside (input, files, arguments) must be UTF-8.

## Errors
`nyra check file.nyra --json` lists every error with a stable `code` (E0001–E0309), a `message`,
`line`, `col` and a `hint` that says how to fix it. `nyra explain E0201` explains a code with a wrong
and a fixed program; [ERRORS.md](ERRORS.md) has them all.

## Runtime errors
An operation that fails while the program runs stops it with exit code 101, after all earlier
output, and prints (as JSON with `nyra run --json`):
```
runtime error[E0240]: index 3 is out of bounds for length 3
  --> main.nyra:4:13
  = hint: valid indexes are 0 to len - 1; compare with `.len()` first
```
| code | meaning |
|---|---|
| E0240 | index or range out of bounds (`xs[i]`, `s[i]`, `slice`, `insert`, `remove`) |
| E0241 | integer `/` or `%` by zero |
| E0242 | `pop()` on an empty array |
| E0243 | bad argument: `repeat(-1)`, `replace("", t)`, `split("")` |
| E0244 | `int(s)` or `float(s)` of text that is not a number |
| E0245 | `int(x)` of NaN, infinity or a float outside the int range |
| E0246 | `char(n)` of an invalid code (valid: 0 to 1114111, except 55296 to 57343) |
| E0249 | out of memory; `repeat` makes at most 536,870,888 bytes of text or 100,000,000 elements |
| E0340 | a file operation failed: `fs.read: cannot read "x.txt" (not found)` |
| E0341 | input, an argument or a variable is not UTF-8 |
| E0342 | a bad argument to a standard function: `random.range(5, 5)`, `text.fixed(x, -1)` |
| E0345 | `json.parse`: not JSON, or not the shape of the type: `expected an int at $.items[0].count` |

## Known differences between backends
The targets are native (C), `--js`, `--py`, `--ts`, `--rs` and `--go`; everything else, runtime errors
included, is the same on each.
- `int` overflow wraps natively and with `--py`, `--rs` and `--go`; with `--js` and `--ts`, ints are exact
  only up to 2^53 (9007199254740991).
- Deep recursion crashes without a Nyra error: `--js`/`--ts` throw `RangeError` after thousands of
  calls, `--py` raises `RecursionError` after 100,000, a native or `--rs` program dies when its stack
  ends and may lose output it has not written yet; `--go` grows its stack to 1 GB.
- When the system itself runs out of memory (not a `repeat` that is too long, which is E0249
  everywhere), only native and `--js`/`--ts` report E0249; `--py` does too, without a position.
