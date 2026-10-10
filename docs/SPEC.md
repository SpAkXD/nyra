# Nyra v0.6 — language spec

This file is the whole language. It is short on purpose: paste it into an AI agent's
context and the agent can write Nyra. For common mistakes and complete examples, see
[AI_GUIDE.md](AI_GUIDE.md).

## Rules
- One way to do each thing. No implicit conversions. No shadowing. No null.
- A program is `fn`, `struct` and `enum` definitions and `ex` examples in any order plus `fn main()`, or statements at
  the top level (a script: they run in order, like the body of `main`), or both: the statements run first,
  then `main()` is called. A script's top-level `let`s and `var`s are visible in every function (see Script variables).
- Every function signature is fully typed. Local variable types are inferred.
- One statement per line. There are no semicolons. A line may break inside `( )`, between the
  elements of `[ ]`, after a binary operator, and before a binary operator or a `.` (the next line
  starts with it). `return`, `break` and `continue` may follow a statement on its line: `{ print("no") return }`.
- `return` is written in full; `ret` is accepted as the same keyword, so older programs still work.
- A `fn` written (indented) inside a function body is an ordinary function; it cannot see the locals around it.
- Everything is evaluated left to right: arguments, operands and the parts of a string.
- Names use letters, digits and `_` (`row_count`, `x2`, `_`). Comments: `// to end of line`.
- Code nests at most 256 levels deep: each parenthesis, call, `[ ]`, unary operator, block and `else if`
  link counts, and so does each operator, `.method()` or `[index]` of a chain. Deeper code is E0103.

## Types
| type | values |
|---|---|
| `int` | 64-bit signed: `42`, `-7`; overflow stops the program (E0255), it never wraps |
| `float` | 64-bit: `2.5`, `3.0` (always a dot with digits on both sides; no exponent: `5e-324` is an error, `float("5e-324")` works) |
| `bool` | `true`, `false` |
| `str` | immutable UTF-8 text: `"hi"`, `""`; escapes `\n \t \r \0 \\ \"` |
| `char` | one character: `'a'`, `'é'`, `'\n'`, `'\''` |
| `[T]` | array of `T`: `[1, 2]`, `[[1], []]`; an empty one needs its type: `var xs: [int] = []` |
| `[K: V]` | map from `K` (`int`, `str`, `char`, `bool` or a tuple of those) to `V`: `["a": 1]`; empty: `var m: [str: int] = [:]` |
| `(T, U)` | tuple of two or more values of any types: `(1, "a")`, read with `t.0`, `t.1` |
| `T?` | optional: a `T` or `none`: `int?`, `[str]?`, `(int, str)?` |
| `Point` | a struct you declare |
| `Dir` | an enum you declare: one of its named cases, `Dir.N`, which may carry values, `Shape.Circle(2.0)` |

There are no optional types and no null: `int?` and `Option<int>` are E0262. Return a sentinel (`-1`, `""`) or a `bool`.

## Functions
```nyra
fn add(a: int, b: int) -> int {
    return a + b
}
fn square(x: int) -> int = x * x      // one-line function: the expression is returned
fn greet(name: str) = print("hi {name}")
fn show(n: int) {                     // no `->`: returns nothing
    if n < 0 { return }               // a bare `return` leaves early
    print(n)
}
```
`fn main()` has no parameters and no return type. A function with a return type must `return` on
every path. Functions may call each other in any order, and recurse. Parameters cannot be changed:
declare one `var` (the function works on its own copy: `fn count_down(var n: int)`), or `inout` to change
the caller's variable (see Values).

A function can be called like a method, with its first argument before the dot, when no built-in method has
its name: `r.area()` is `area(r)`, and `p.move_by(2)` is `move_by(inout p, 2)` when the first parameter is `inout`.

## Examples
```nyra
fn sq(x: int) -> int = x * x   ex sq(3) == 9, sq(-2) == 4
fn dist(a: int, b: int) -> int {
    if a > b { return a - b }
    return b - a
}
ex dist(7, 2) == 5, dist(2, 7) == 5
```
`ex` lists `bool` conditions, separated by commas: at the end of a one-line function, or on lines of
their own outside functions (a line may break after a comma). They see no variables, only literals and
calls, and are never compiled into the program: `nyra check`, `run` and `build` evaluate each one while
compiling. A false one is error E0250 with both values (`dist(2, 7)` is -5, not 5), one that stops
with a runtime error is E0251, one that is not a `bool` E0252, and one that runs more than 1,000,000
steps or 10,000 nested calls E0253. `nyra test file.nyra` runs them and counts what passed. `ex` is a
keyword only at the start of an example, so it can still name a variable.

A *property* checks conditions for many inputs: `ex for n in 0..200: f(n) >= 0, f(n) <= f(n + 1)`. The
variable is an `int` that only the conditions see; the range is two whole-number literals (the end is
exclusive), optionally `step k` (`10..0 step -2`), at most 100,000 values (more is E0293; a bound that is
not a literal, or step 0, is E0292). Every condition runs for every value, in the interpreter, while the
program compiles. The first input that fails is reported as E0250 (`example `f(n) >= 0` is false for n = 7:
`f(n)` is -3`, with the actual and expected values), a runtime error for an input as E0251 and a run that
does not finish within 10,000,000 steps as E0253, each saying which `n`.

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

## Script variables
```nyra
var tokens: [str] = []
var pos = 0
let limit = 3
fn peek() -> str = if pos < tokens.len() { tokens[pos] } else { "" }
fn advance() {
    pos += 1                    // a `var` can be changed by any function
}
tokens = "a b c".split(" ")
advance()
print(peek(), limit)            // b 3
```
A `let` or `var` at the top level of a script (not inside a block, and not in `fn main`) is a script
variable: every function may read it, and change it when it is a `var` (a `let` one is E0205). A
function that declares a parameter or variable of the same name does not see it (`fn area(w: int)`
next to `let w = 3` uses its parameter); using both in one function is E0216. A call runs the function,
so every script variable it uses, also through the functions it calls, must be declared before the
call (E0217). Examples cannot call a function that uses script variables (E0254). A script variable
passed `inout` to a function that uses it is E0237; a lambda cannot call a function that changes one
(E0214).

With a `fn main` too, the top-level statements run first, in order, and then `main()` is called (a bare
`main()` line among the statements calls it there instead, and it is not called again). The variables
declared inside `main` stay `main`'s; the script variables are visible in it like in any function.
```nyra
var visits = 0
fn main() {
    visits += 1                 // a script variable
    print("main", visits)       // main 2
}
visits = 1
```

## Control flow
```nyra
let x = 3
var n = 0
if x > 3 { print("big") } else if x == 3 { print("three") } else { print("small") }
while n < 10 { n += 1 }
for i in 0..x + 1 { print(i) }   // 0 to 3: the end is exclusive
for i in 10..0 step -2 { print(i) }  // 10 8 6 4 2: `step` counts by any int but 0
for c in "hi" { print(c) }       // each char; `for v in xs` gives each element of an array
for i, c in "hi" { print(i, c) } // also the position: 0 h, then 1 i
for i in 0..10 {
    if i % 2 == 0 { continue }   // next round of the innermost loop
    if i > 6 { break }           // leave the innermost loop
}
```
Conditions must be `bool` (write `x != 0`, not `x`). `{` stays on the line of its `if`, `else`,
`while`, `for` or `fn`. Range bounds are `int` expressions, evaluated once. Loop variables are
immutable and may go unused. `for v in xs` loops over `xs` as it was when the loop started.

`if` can also be a value. It needs an `else`, and each branch is one expression of the same type:
`let max = if a > b { a } else { b }`. The conditional operator is the same thing: `let max = a > b ? a : b`
(see Operators). So is `if let`: `let n = if let v = m.get(k) { v + 1 } else { 0 }`. A `match` is a value too
(see Enums and `match`).

## Operators (high to low precedence)
| ops | types |
|---|---|
| `f(x)` `p.x` `xs[i]` `s.len()` | call, field, index, method |
| `-x` `!x` | `-` on int/float, `!` on bool |
| `*` `/` `%` | int,int or float,float (`%` int only; int `/` truncates toward zero) |
| `+` `-` | int,int or float,float; `+` also joins two `str`s or two arrays of one type |
| `<` `<=` `>` `>=` | int,int · float,float · str,str · char,char · tuple,tuple of those → bool |
| `==` `!=` | same type on both sides → bool (strings, arrays and structs compare by content) |
| `x in xs` | `bool`: `xs` has the element `x` (`[T]`); `s` has the char or text `x` (`str`); `m` has the key `x` (`[K: V]`) |
| `a ?? b` | `a` is `T?`, `b` is `T` (→ `T`) or `T?` (→ `T?`); between the comparisons and `+`, groups to the right |
| `&&` `\|\|` | bool; the right side runs only when needed |
| `c ? a : b` | `c` is a `bool`; the same as `if c { a } else { b }`: `a` and `b` have one type, and only the chosen one is evaluated |

Parentheses group: `(a + b) * c`. `?:` binds weaker than `||` and groups to the right: `a ? b : c ? d : e` is
`a ? b : (c ? d : e)`, and `x > 0 || y > 0 ? 1 : 2` tests `x > 0 || y > 0`. A line may break before or after the
`?` and the `:`.

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
| `zip(xs, ys)` · `zip(xs, ys, zs)` | `[(X, Y)]`: the pairs of elements, as many as the shorter array (or string, per char) has |

## Strings and chars
`"{expr}"` inserts any value: `"{name}: {xs.len()} items"`. A brace that starts no value is text:
`"}"`, `"fn main() {"`, `"{}"` (`{{` and `}}` also give one brace). A `$` is ordinary text: `"cost: ${x}"` prints
`cost: $3`, and the compiler warns (E0260) because `${x}` is another language's way to insert a value (see Errors). Strings and chars may appear inside `{ }`: `"{xs.join(", ")}"`.
A format specifier after a colon shapes the text of a value, with the same result on every backend:
```nyra
let name = "ann"
let n = 42
let pi = 3.14159
print("[{name:8}] [{name:>8}] [{name:^8}] [{n:6}] [{name:*<6}]")   // [ann     ] [     ann] [  ann   ] [    42] [ann***]
print("{n:05} {n:+} {pi:.2} {pi:8.3} {1234567:,} {pi:>10.1}")      // 00042 +42 3.14    3.142 1,234,567        3.1
```
The parts, in this order: a fill character and an alignment (`<` left, `>` right, `^` centered; the default is
left for text and right for numbers), `+` (a sign on positive numbers), `0` (zeros after the sign), the width,
`,` (thousands separators), `.N` (N decimals of a float, rounded exactly like `text.fixed`: ties away from zero),
and optionally `f`, `d` or `s`. `{x:.2f}` and `{n:5d}` are accepted as in Python. Width, fill and alignment work
on any value (a tuple or an array is first written as `print` shows it); decimals, `,`, `+` and `0` need a
number (E0271); anything else after the colon is E0270. The colon belongs to the specifier only outside
brackets and quotes, so `{xs[0]}` and `{f("a")}` are unchanged.

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
| `s.len()` · `s.slice(a, b)` | number of characters · characters `a` to `b - 1`; also `s[a..b]`, `s[a..]`, `s[..b]` |
| `s.contains(t)` `s.starts_with(t)` `s.ends_with(t)` | `bool` (`t` is a `str` or a `char`) |
| `s.index_of(t)` | first position of `t` (a `str` or a `char`), or `-1` |
| `s.to_int()` · `s.to_float()` | `int?` · `float?`: the number, or `none` when `int(s)` / `float(s)` would fail |
| `s.pad_left(n)` `s.pad_right(n)` | spaces added until `s` has `n` characters (never shorter); `s.pad_left(n, '0')` pads with a char |
| `s.split(sep)` | `[str]`: `"a,b,,c".split(",")` is `["a", "b", "", "c"]` |
| `s.replace(old, new)` · `s.repeat(n)` | every `old` replaced · `n` copies |
| `s.trim()` · `s.trim(chars)` | without leading and trailing spaces, tabs and newlines · without any of the characters of `chars` (a `str` or a `char`) |
| `s.chunks(n)` | `[str]`: pieces of `n` characters, the last may be shorter (`n` below 1 is E0243) |
| `s.upper()` `s.lower()` · `s.chars()` · `s.codes()` | ASCII case only (`"é".upper()` is `"é"`) · `[char]` · `[int]` |
| `c.code()` · `c.upper()` `c.lower()` | `int` · `char` (ASCII) |
| `c.is_digit()` `c.is_letter()` `c.is_upper()` `c.is_lower()` `c.is_space()` | `bool` (ASCII only) |

Chars compare by code (`'a' < 'b'`) and have no arithmetic: `char(c.code() + 1)`. `int(c)` is an
error: write `c.code()`.

## Arrays
```nyra
let xs = [3, 1, 2]                    // type [int]
var names: [str] = []                 // an empty [] needs a known type
names.push("ann")
print(xs[0] + xs[xs.len() - 1])       // indexes start at 0; no negative indexes (`xs[-1]` is E0261)
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
| `xs.slice(a, b)` · `xs.repeat(n)` | elements `a` to `b - 1`, also `xs[a..b]`, `xs[a..]`, `xs[..b]` (positions as for `slice`) · `n` copies, one after another |
| `xs.chunks(n)` | `[[T]]`: pieces of `n` elements, the last may be shorter (`n` below 1 is E0243) |
| `xs.sort()` · `xs.reverse()` | in place, return nothing (`sort`: `[int]` `[float]` `[str]` `[char]`, or tuples of those) |
| `xs.join(sep)` | `[str]` or `[char]` → one `str` |
| `xs.reversed()` · `s.reversed()` | a reversed copy (`xs.reverse()` reverses in place) |
| `xs.sorted()` | a sorted copy (`sort()` does it in place) |
| `xs.sum()` · `xs.min()` · `xs.max()` | `[int]`/`[float]` sum (0 when empty) · smallest/largest of `[int] [float] [str] [char]`, like `min(a, b)` from left to right (empty: E0247) |

Changing an array (`xs[i] = v`, `+=`, `push pop insert remove swap sort reverse sort_by`) needs a `var`.

## Lambdas and comprehensions
A lambda `x => expr` (or `(a, b) => expr`) is the argument of one of these methods, and nothing else:
it is not a value. Its parameters take the element type; it reads variables but cannot change them
(no `push`, `pop`, ... and no `inout` inside: E0214).
```nyra
let xs = [3, -1, 4]
print(xs.filter(x => x > 0).map(x => x * x).sum())   // 25
print([x * x for x in xs if x > 0], [i * 2 for i in 0..3])   // [9, 16] [0, 2, 4]
```
| method | result |
|---|---|
| `xs.map(x => e)` · `xs.filter(x => test)` | a new array of the results · of the elements that pass |
| `xs.count(x => test)` · `xs.any(...)` · `xs.all(...)` | `int` · `bool` · `bool` (also on a `str`: each char) |
| `xs.find_index(x => test)` | position of the first element that passes, or `-1` (also on a `str`) |
| `xs.find(x => test)` | `T?`: the first element that passes, or `none` |
| `xs.fold(start, (acc, x) => e)` | `acc` starts as `start` and becomes `e` for each element: the last `acc` |
| `xs.sorted_by(x => key)` | a new sorted array, like `sort_by`: `words.sorted_by(w => (-w.count, w.text))` sorts by count descending, then text |
| `xs.min_by(x => key)` · `xs.max_by(x => key)` | the first element with the smallest / largest key (an empty array: E0247) |
| `xs.sort_by(x => key)` | in place, stable, by an `int`/`float`/`str`/`char` key (or a tuple of them) computed once per element |

A chain of `map` and `filter` and the method that ends it run as one loop, element by element (no
array in between); `any`, `all` and `find_index` stop at the answer. `[e for x in src if c]` (the
`if` is optional) is a new array like `src.filter(x => c).map(x => e)`; `src` is an array, a string or
a range `a..b` (with an optional `step`).

## Maps
```nyra
var ages = ["ann": 31, "bob": 27]       // type [str: int]
ages["cid"] = 40                        // add, or replace (an existing key keeps its place)
ages["ann"] += 1
print(ages["bob"], ages.get("dan", 0))  // 27 0: `m[k]` of a missing key is error E0248
for name in ages { print(name) }        // the keys, in insertion order
```
| method | result |
|---|---|
| `m.len()` · `m.has(k)` | number of entries · `bool` |
| `m.get(k)` · `m.get(k, default)` | `V?`: the value or `none` · the value or `default` |
| `m.set(k, v)` · `m.remove(k)` | like `m[k] = v` · removes `k` (nothing happens if it is missing) |
| `m.keys()` · `m.values()` · `m.items()` | arrays, in insertion order; `items` gives the pairs `[(K, V)]`: `for (k, v) in m.items()` |

Maps are values like arrays (`var b = a` copies), compare with `==` by content in any order and print
as `["ann": 31, "bob": 27]` (`[:]` when empty). Changing a map needs a `var`. A value inside a map changes
in place, the same way an element of an array does: `m[k].push(x)`, `m[k].count += 1`, `m[k][i] = v`,
`m[k] += 1`, `m[k].sort()` (the key must exist, else E0248; `inout m[k]` is E0229). A copy of the map never
sees the change.

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
`kids: [Tree]`). Structs have no methods of their own: write `fn area(r: Rect) -> int` and call `area(r)` or `r.area()`. A `fn` inside the
struct's braces or an `impl` block is E0263, and `class` is E0264: write a `struct` and the functions outside it.

## Tuples
```nyra
fn divmod(a: int, b: int) -> (int, int) = (a / b, a % b)

let t = divmod(17, 5)                    // (3, 2)
print(t.0, t.1)                          // 3 2: positions start at 0
let (q, r) = divmod(17, 5)               // take it apart: q is 3, r is 2
var (x, y) = (1, 2)
(x, y) = (y, x)                          // swap: x is 2, y is 1
let (name, _) = ("ann", 31)              // `_` skips a part
for (k, v) in [("a", 1), ("b", 2)] {
    print(k, v)
}
print((1, "a") < (1, "b"))               // true: parts are compared one by one
var pairs = [(3, "c"), (1, "z"), (3, "a")]
pairs.sort()                             // [(1, "z"), (3, "a"), (3, "c")]
print(pairs[0], pairs.len())             // (1, "z") 3
```
A comma makes a tuple (`(a + b) * c` is still a grouping). The parts are fixed by the type: `(int, str)` is
not `(str, int)`, and `t.2` of a pair is E0273. A pattern must name every part (`_` skips one): a wrong
count is E0272. Tuples are values like structs: they are copied, compare with `==` and `!=` by content, and
`<` `<=` `>` `>=` compare the parts in turn when each part is an `int`, `float`, `str`, `char` or `bool`
(or such a tuple). So `sort()` works on an array of them. A tuple whose parts are
ints, strs, chars, bools or such tuples can be a map key (`[(int, int): str]`); `json` writes one as an array.

## Enums and `match`
```nyra
enum Dir { N, E, S, W }                  // a fixed set of named cases; commas or new lines

fn turn_right(d: Dir) -> Dir {
    match d {                            // every case must be handled
        Dir.N => return Dir.E
        Dir.E => return Dir.S
        Dir.S => return Dir.W
        Dir.W => return Dir.N
    }
}

fn is_vertical(d: Dir) -> bool {
    match d {
        Dir.N, Dir.S => return true         // several patterns, one arm
        _ => return false                   // `_` takes everything else
    }
}

let d = Dir.N                            // a value is written with its enum
print(d, turn_right(d), is_vertical(d))  // Dir.N Dir.E true
print(d == Dir.N, Dir.all())             // true [Dir.N, Dir.E, Dir.S, Dir.W]
match 7 % 2 {
    0 => print("even")
    _ => print("odd")
}
```
An enum value is one of its variants, always written `Enum.Variant` (a bare `N` is E0278). It prints as
`Dir.N`, compares with `==` and `!=` by variant, is copied like any value, and can be stored in arrays and
struct fields; it cannot be ordered or be a map key. `Enum.all()` is the array of all variants, in order.

A variant may carry values, written as types after its name. It is built by calling it, and `match` takes
the values out by naming them:
```nyra
enum Shape { Circle(float), Rect(float, float), Empty }

fn area(s: Shape) -> float {
    match s {
        Shape.Circle(r) => return 3.0 * r * r      // `r` is the value of the variant
        Shape.Rect(w, h) => return w * h
        Shape.Empty => return 0.0
    }
}

let c = Shape.Circle(2.0)
print(c, Shape.Rect(1.0, 2.5), c == Shape.Circle(2.0), area(c))   // Shape.Circle(2) Shape.Rect(1, 2.5) true 12
```
A variant that carries values is always written with them (`Shape.Circle` alone is E0286), in the order and
types it declares (a wrong count is E0204, a wrong type E0203). Values compare with `==` by variant and
values, and print like a struct's fields: `Token.Pair(3, "x")`. A pattern names every value, `_` for one that is
not needed (`Shape.Rect(_, h)`; a wrong count or a literal is E0287, and so are names in an arm with several
patterns). The names are `let`s of that arm. An enum cannot contain itself (E0222): keep recursive data in an
array, `Node([Tree])`. `Enum.all()` needs variants without values (E0288).

`match value { pattern => body }` picks the first arm whose pattern equals the value. A pattern is a variant
of the matched enum, or a literal `int`, `str`, `char` or `bool` (a `-` literal cannot start a line:
`4, -1 =>`); several patterns are separated by commas; `_` takes anything. The body is one statement after
`=>`, or a block `{ ... }`. All cases must be covered (E0281): for an enum every variant or a `_` arm, for a
`bool` both values, for an `int`, `str` or `char` a `_` arm. An arm that can never run is E0283, and a
pattern of the wrong type, or a value that cannot be matched (a `float`, an array), is E0279. As a
statement, `match` `return`s a value or assigns it in the arms.

`match` and `if let` also give a value. As a value, each arm is one expression (arms are separated by new
lines or commas), all of one type (E0212), and the cases must still be covered:
```nyra
enum Shape { Circle(float), Rect(float, float), Empty }
fn size(n: int) -> str = match n { 0 => "none", 1, 2, 3 => "few", _ => "many" }

let s = Shape.Rect(2.0, 3.0)
let area = match s {
    Shape.Circle(r) => 3.14 * r * r
    Shape.Rect(w, h) => w * h
    Shape.Empty => 0.0
}
let k = 2
print(area, size(k), 10 + match k { 1 => 100, _ => 200 })   // 6 few 210
```
The value matched is computed once, and only the chosen arm runs. A statement that starts with `match` is
the statement form; anywhere else (`let`, `return`, an argument, an operand) it is a value.

## Optional values
```nyra
let ages = ["ann": 31, "bob": 27]
let a: int? = ages.get("ann")                // Some(31); a missing key gives none
print(a, ages.get("cy"))                     // Some(31) none
print(ages.get("cy") ?? 0)                   // 0: the value, or the default
if let n = ages.get("bob") {                 // runs when there is a value, named n
    print("bob is {n}")
} else {
    print("no bob")
}
var best: int? = none                        // none needs the type: `int?`
for x in [4, 9, 2] {
    if best == none || x > (best ?? 0) {     // compare with none, or with a plain value
        best = x
    }
}
print(best, best.is_some(), best.unwrap())   // Some(9) true 9
let first_even = [3, 5, 8, 6].find(x => x % 2 == 0)   // Some(8)
print("12".to_int(), "x".to_int(), "2.5".to_float())  // Some(12) none Some(2.5)
```
`T?` holds a `T` or `none`; a plain `T` goes where a `T?` is expected (`let a: int? = 5`, `ret x` in a function
`-> int?`, an argument). It prints as `Some(31)` or `none`, and `==` and `!=` compare by content (also with a
plain value: `m.get(k) == 3`). `x ?? d` is `x`'s value, or `d` (evaluated only when needed); `d` may itself be
optional. `if let v = x { ... } else { ... }` binds `v` in the first block only. `x.is_some()`, `x.is_none()` and
`x.unwrap()` (E0350 at run time when it holds none) are the only methods; to use a field or a method of the
value, take it out first. `none` needs a known optional type (E0276), `??` and `if let` need an optional on the
left (E0277). Optionals cannot be map keys. In `json` an optional is the value, or `null`.

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
`inout xs[i]`). Two `inout` arguments of one call must be different variables, so
`swap(inout xs[i], inout xs[j])` is an error (E0237): exchange two elements with `xs.swap(i, j)`.

## Memory
Memory is freed automatically: reference counting, no garbage collector. Three statements say
when. They are checked at compile time, and a program prints the same with or without them:

| write | effect |
|---|---|
| `free(x)` | frees `x`'s value now; later uses of `x` are error E0239 (a `var` may get a new value) |
| `arena { ... }` | its values are freed at `}`; outer strings, arrays and structs holding them are read-only inside, so a result leaves only through `return` (copied out) |
| `keep(x)` | `x`'s value is never freed |

`free` and `keep` take a local `let`/`var` holding a `str`, an array or a struct that contains one.

## Printing
`print(x)`, `str(x)` and `"{x}"` show the same text (`"{x:>8}"` adds a format specifier). Arrays, tuples and structs print as Nyra code, with
strings and chars inside them quoted: `["a", "b"]`, `['a', '\n']`, `(1, "a")`, `Point(x: 1, y: 2)`. Numbers print
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
| `json` | `str(v)` any value as JSON (see below) · `parse(text)` reads the type the value goes to: `let p: Point = json.parse(s)` |
| `time` | `now_ms()` int, since 1970 · `mono_ms()` float, a monotonic clock for timing · `sleep_ms(ms)` |
| `random` | `random()` a float from 0 up to 1 · `range(lo, hi)` an int from lo to hi - 1 · `seed(n)` |
| `math` | `pi` `e` `inf` · `sqrt floor ceil round trunc exp log log10 log2 sin cos tan asin acos atan` (float) · `pow(x, y)` · `atan2(y, x)` |
| `text` | `fixed(x, digits)` decimals, `text.fixed(2.0 / 3.0, 2)` is "0.67" · `is_int(s)` · `is_float(s)`: would `int(s)`/`float(s)` work |

A module has a namespace of its own: its functions are reached only as `math.name`, and the names inside it
never clash with the program's (`fn sign`, `let x`, `fn lo` are all fine next to `use math`).

`nyra run main.nyra -- a b` passes the arguments `a b`. Random numbers come from the operating system
unless `random.seed(n)` was called; then they are the same sequence on every backend. `math` gives the
same digits on every backend; `round` rounds halves away from zero. Paths are relative to the folder the
program runs in and use `/`. JSON objects are read into structs by field name (other keys are skipped;
every field must be there), lists into arrays; `json.str` writes infinity and NaN as `null`.

Every type has a JSON form, and `json.parse` reads exactly it:

| Nyra | JSON |
|---|---|
| `int` `float` `bool` `str` `char` | a number, a number, `true`/`false`, a string, a one-character string |
| `[T]` | an array |
| struct | an object with the fields in order: `{"x":1,"y":2}` |
| tuple `(A, B)` | an array of the parts: `[1,"a"]` (reading needs the exact length) |
| optional `T?` | the value, or `null` for `none` |
| enum variant | the name as a string, `"Empty"`, or with values an object with one key and an array: `{"Rect":[1,2]}` |
| map `[str: V]` | an object: `{"a":1,"b":2}`; a repeated key keeps its first place and the last value |
| map with other keys | an array of `[key,value]` pairs: `[[1,"one"],[2,"two"]]` |

```nyra
use json

enum Shape { Circle(float), Empty }

let data = ["a": (1, Shape.Circle(2.5)), "b": (2, Shape.Empty)]
let text = json.str(data)
print(text)                          // {"a":[1,{"Circle":[2.5]}],"b":[2,"Empty"]}
let back: [str: (int, Shape)] = json.parse(text)
print(back == data)                  // true
```
A shape that does not fit is runtime error E0345 with the path, e.g. `expected an array of 2 elements at $.a` or
`expected a variant of Shape: a name, or {"Name": [values]} at $.b`. An optional inside an optional writes `null`
for both `none` and `Some(none)`. Text from
outside (input, files, arguments) must be UTF-8.

## Capabilities and the sandbox
A program says what it touches with its `use` lines, and a run decides what it may touch. The modules
`json`, `math`, `text`, `time` and `random` are always available. The others are *effectful* and need a
capability of the same name; any module that is not listed as pure needs one, so a module added later is
closed by default:

| capability | modules | allows |
|---|---|---|
| `fs` | `fs` | read, write, list and remove files and folders |
| `input` | `input` | read standard input |
| `os` | `os` | arguments, environment variables, `exit` |
| `net` | none yet | network access |

A `use` of a module whose capability the run does not grant is **compile error E0290**: the message names
the module, the capability and the flag to add, and nothing runs. Which capabilities a run grants:

| command | grants |
|---|---|
| `nyra run f.nyra` (also `check`, `build`, `test`) | everything |
| `nyra run f.nyra --allow fs,os` | only those (`--allow all` grants everything, `--allow none` nothing) |
| `nyra run f.nyra --sandbox` | nothing, plus what `--allow` names |
| MCP `nyra_run` | `input`, plus the tool's `allow` list |

Granting `os` also shows the program the environment variables of the process, and `fs` every file the
process can reach (in the sandbox: every file below the working folder): grant only what a task needs.

`nyra outline --json` lists what each function needs, also through the functions it calls: each function
and the script have `"effects": ["fs", "input"]` (the capabilities, sorted; `[]` for a pure function), and
`"capabilities"` at the top is what the whole program needs, which is the `--allow` list that runs it.

`--interp` runs the program in the interpreter of the IR instead of a compiled program: in this process,
with no C compiler or Node.js, and under limits that stop a runaway program with a runtime error that has
a code and an exit code. `--sandbox` is `--interp` with no capabilities but those of `--allow`, and file
paths confined to the working folder (no absolute path, no `..`: E0340). Any limit flag implies `--interp`.

| limit | flag | default | error | exit code |
|---|---|---|---|---|
| steps: statements run, plus the elements and characters operations make | `--fuel N` | 2,000,000,000 | E0355 | 120 |
| heap memory the run adds | `--max-memory SIZE` (`256M`) | 512M | E0356 | 121 |
| bytes printed (the output is cut at the limit) | `--max-output SIZE` | 64M | E0357 | 122 |
| nested calls | `--max-depth N` (at most 100,000) | 20,000 | E0358 | 123 |
| real time | `--max-time MS` | none | E0359 | 124 |

Steps are counted, not timed, so the same program with the same fuel stops at the same statement on every
machine; memory is the heap the interpreter's own thread has added. In the interpreter `time.sleep_ms` does
not wait: it moves a virtual clock that `time.now_ms` and `time.mono_ms` include, and costs one step per
millisecond. The MCP tool `nyra_run` with `sandbox: true` runs a program this way (no child process) and
takes `fuel`, `max_memory`, `max_output` (at most 16 KiB) and `timeout_ms`.

## Modules of your own
```text
// shapes.nyra
pub struct Rect { w: int, h: int }
pub fn area(r: Rect) -> int = r.w * r.h
fn helper(r: Rect) -> int = r.w + r.h     // private: only this file can call it
ex area(Rect(w: 2, h: 5)) == 10

// main.nyra
use ./shapes                              // the file shapes.nyra next to this one
let r = Rect(w: 3, h: 4)                  // types keep their plain names
print(shapes.area(r))                     // 12: functions are called with the file's name
```
`use ./name` (or `use ../util/name`, `use ./folder/name`; `use "./name"` is the same) imports the file `name.nyra`
from the folder of the importing file. The functions marked `pub` are called `name.f(x)`; a function
without `pub` is private to its file (E0301), and a file's helpers call each other by their plain names. The
`pub` structs and enums are used without a prefix (all type names share one program-wide namespace, so a
clash is E0206). A module holds only definitions: `fn`, `struct`, `enum`, `ex`, and its own `use` lines
(E0285 for statements); its examples run with the program's. Two files are never imported under one name
(E0304, also for a file named like a standard module), files may not import each other (E0303), and
`pub` goes right before `fn`, `struct` or `enum` (E0332). A mistake inside an imported file is reported
with that file's name and line. A program given as text (`nyra mcp`) has no folder and cannot import files.

## Errors
`nyra check file.nyra --json` lists every error with a stable `code` (E0001–E0359), a `message`,
`line`, `col` and a `hint` that says how to fix it. `nyra explain E0201` explains a code with a wrong
and a fixed program; [ERRORS.md](ERRORS.md) has them all. Positions are always in your file.

A **warning** is a likely mistake that the language allows: the build goes on and the exit code is
unchanged. It prints to stderr as `warning[E0260]: ...` and `--json` lists it under `"warnings"` (next to
`"errors"`, with the same fields). E0260 is the only one so far: `"${x}"` prints a `$` and then the value;
write `"{x}"`, or `"$" + str(x)` when the dollar sign is meant.

An error that has exactly one certain repair (it carries a `fix`) is repaired by `nyra check`, `run`,
`build` and `test` in memory: the program goes on, each repair is a warning on stderr (in `--json`
a `warnings` array with `code`, `line`, `col`, `message`, `applied` and `fix` for each), and the file is
not written. `--fix` writes the repaired file back, `--strict` keeps every error an error. Runtime error
positions refer to the repaired text. `nyra fmt file.nyra` rewrites a program into canonical form: it
applies those repairs, writes `return` (for `ret`) and indents by four spaces; it never changes what the
program does.

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
| E0247 | `min()` or `max()` of an empty array |
| E0248 | `m[k]` of a key the map does not have |
| E0249 | out of memory; `repeat` makes at most 536,870,888 bytes of text or 100,000,000 elements |
| E0255 | int overflow: `+`, `-`, `*`, negation, `abs` or `/` (only `MIN / -1`) outside -2^63 to 2^63 - 1 |
| E0256 | `--js`/`--ts` only: an int beyond 2^53 - 1 (9007199254740991), which JavaScript would round |
| E0350 | `opt.unwrap()` of `none` |
| E0340 | a file operation failed: `fs.read: cannot read "x.txt" (not found)` |
| E0341 | input, an argument or a variable is not UTF-8 |
| E0342 | a bad argument to a standard function: `random.range(5, 5)`, `text.fixed(x, -1)` |
| E0345 | `json.parse`: not JSON, or not the shape of the type: `expected an int at $.items[0].count` |
| E0355-E0359 | `--interp` / `--sandbox` only: a limit of the interpreter was reached (steps, memory, output, call depth, time); exit codes 120 to 124, see Capabilities and the sandbox |

## Known differences between backends
The supported targets are native (C) and `--js`; `--py`, `--ts`, `--rs` and `--go` are experimental. Everything
else, runtime errors included, is the same on each.
- Ints: an overflow of the 64-bit range is E0255 on every target. With `--js` and `--ts` an int is a
  JavaScript number, exact only up to 2^53 - 1 (9007199254740991): a program that goes beyond (an
  operation, a literal, `int(s)`, `int(x)`, `json.parse`) stops with E0256 there, where the other
  targets go on. Programs that stay below print the same everywhere. (Operations the compiler proves
  in range, such as constants, `for` counters, indexes and lengths, and a `var` that only changes by
  `+= 1` or `-= 1` from a small start, are not checked: such a counter would need months to get there.)
- Deep recursion crashes without a Nyra error: `--js`/`--ts` throw `RangeError` after thousands of
  calls, `--py` raises `RecursionError` after 100,000, a native or `--rs` program dies when its stack
  ends and may lose output it has not written yet; `--go` grows its stack to 1 GB.
- When the system itself runs out of memory (not a `repeat` that is too long, which is E0249
  everywhere), only native and `--js`/`--ts` report E0249; `--py` does too, without a position.
- The interpreter (`--interp`, `--sandbox`) prints what the native target prints, ints included (no E0256),
  with these differences: `time.sleep_ms` does not wait (a virtual clock), a recursion that is too deep is
  E0358 instead of a crash, and the sandbox confines file paths to the working folder.
