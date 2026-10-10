# Nyra v0.5 (agent spec)
Nyra is new: use only what is below. Strict types, no implicit conversions, no shadowing, no null.

## Program
A program is top-level statements that run in order (a script, no `fn main` needed), plus `fn`, `struct` and `ex` definitions in any order. A script's top-level `let`/`var` are visible in every fn (a fn may change a `var`); declare them before the first call that uses them. A fn with its own param/local of that name uses its own.
One statement per line, no `;`. A line may break inside `( )` or `[ ]`, after a binary operator, or before a binary operator or `.`. `{` stays on the line of `if`/`else`/`while`/`for`/`fn`. `ret`/`break`/`continue` may follow a statement on a one-line block: `if x < 0 { print("neg") ret }`. Comments `// ...`. Names: letters, digits, `_`. Evaluation is left to right.

## Types
`int` 64-bit: `42` · `float`: `2.5` (digits on both sides of the dot) · `bool` · `str` UTF-8, escapes `\n \t \r \0 \\ \"` · `char` `'a'` `'\n'` `'\''` · `[T]` array `[1, 2]` · `[K: V]` map, K is int/str/char/bool: `["a": 1]` · structs.
Empty collections need a type: `var xs: [int] = []`, `var m: [str: int] = [:]`.

## Functions
```
fn add(a: int, b: int) -> int { ret a + b }   // every param and the return type are written
fn sq(x: int) -> int = x * x                   // one-line form returns the expression
fn show(n: int) { if n < 0 { ret } print(n) }  // no `->`: returns nothing; bare `ret` leaves early
```
A fn with `-> T` must `ret` on every path. Params are immutable: copy into a `var`, or declare `inout`. Recursion and any call order are fine.
`ex` = checked examples, never part of the program: `fn sq(x: int) -> int = x * x   ex sq(3) == 9, sq(-2) == 4` (or on their own lines). Only literals and calls. A false one is error E0250.

## Variables and control flow
```
let x = 5        let y: float = 2.5     var n = 0     n += 1     // also -= *= /= %=
if a > b { ... } else if a == b { ... } else { ... }
let m = if a > b { a } else { b }       // value form: needs else, one expression per branch
while n < 10 { n += 1 }
for i in 0..n { }  for i in 10..0 step -2 { }   // end exclusive; step is any int but 0
for v in xs { }  for c in "hi" { }  for i, v in xs { }  for i, c in "hi" { }  for k in m { }   // map: keys in insertion order
break  continue                         // innermost loop
```
Conditions must be `bool` (`x != 0`, not `x`). A name can be declared once while visible (loop vars, params included). Names declared in `{ }` end at `}`. Loop variables are immutable. Range bounds are evaluated once.

## Operators (high to low)
`f(x)` `p.x` `xs[i]` `s.len()` · unary `-` `!` · `* / %` (`%` int only; int `/` truncates toward 0; int,int or float,float only) · `+ -` (`+` also joins two `str`s or two arrays of one type) · `< <= > >=` (int, float, str, char) · `== !=` (same type; strings, arrays, structs, maps compare by content) · `&&` `||` (short-circuit). No `and`/`or`/`not`, no `?:`, no `x.1`, no tuples.

## Builtins
`print(a, b)` values of any type separated by a space + newline; `print(x, end: "")` keeps the line. `str(x)` any → str (what print shows). `int(x)` float→int truncating, `int("-42")` (bad text: E0244). `float(x)` int/str → float. `char(65)` is `'A'`. `abs(x)` `min(a, b)` `max(a, b)` on ints or on floats.

## Strings and chars
`"{expr}"` interpolates any value: `"{name}: {xs.len()} items"`, strings allowed inside: `"{xs.join(", ")}"`. A brace that starts no value is text (`"}"`, `"{}"`; `{{` `}}` give a brace). `+` joins strings. `s.len()` counts characters; `s[i]` is a `char` (from 0, no negative index); `char` is not `str` and not `int`: `str(c)`, `c.code()`, `char(n)`, `'7'.code() - '0'.code()`.
`s.len() s.slice(a, b)` (chars a..b-1) · `s.contains(t) s.starts_with(t) s.ends_with(t)` (t: str or char) · `s.index_of(t)` or -1 · `s.pad_left(n) s.pad_right(n)` (also `pad_left(n, '0')`) · `s.split(sep)` → [str] · `s.replace(old, new) s.repeat(n) s.trim() s.upper() s.lower() s.reversed()` · `s.chars()` [char] · `s.codes()` [int].
`c.code() c.upper() c.lower()` · `c.is_digit() c.is_letter() c.is_upper() c.is_lower() c.is_space()` (ASCII). Chars compare by code.

## Arrays
```
let xs = [3, 1, 2]    var names: [str] = []    names.push("ann")    xs[0]    let more = xs + [4]
var grid = [[0].repeat(3)].repeat(2)    grid[1][2] = 5       // [[0,0,0],[0,0,0]]
```
`xs.len() push(v) pop() insert(i, v) remove(i) swap(i, j) contains(v) index_of(v) slice(a, b) repeat(n) sort() reverse() join(sep) reversed() sum() min() max()`. `sort`/`reverse` work in place and return nothing; `sum` of an empty array is 0; `min`/`max` of an empty one are errors; `join` needs `[str]` or `[char]`. Changing an array (`xs[i] = v`, `+=`, push, pop, insert, remove, swap, sort, reverse, sort_by) needs a `var`. Values are copied on assignment, passing and returning (`var b = a` then `b.push(3)` leaves `a`).

## Lambdas and comprehensions
`x => expr` or `(a, b) => expr` only as the argument of these methods (not a value; it cannot change variables): `map(x => e)` `filter(x => c)` `count(x => c)` `any(x => c)` `all(x => c)` `find_index(x => c)` (or -1) `fold(start, (acc, x) => e)` `sort_by(x => key)` (in place, stable, key int/float/str/char). `count any all find_index` also work on a `str` (each char).
`[e for x in src if c]` (`if` optional; `src` is an array, str or range `a..b step s`): `print([x * x for x in xs if x > 0])`.

## Maps
```
var ages = ["ann": 31, "bob": 27]    ages["cid"] = 40    ages["ann"] += 1    ages.get("dan", 0)    for name in ages { }
```
`m.len() has(k) get(k) get(k, default) set(k, v) remove(k) keys() values()` (insertion order). `m[k]` of a missing key is error E0248. A value inside a map cannot change in place (`m[k].x = 1` is an error): copy, change, `m[k] = v`.

## Structs
```
struct Point { x: int, y: int }              // or one field per line
fn shifted(p: Point, dx: int) -> Point = Point(x: p.x + dx, y: p.y)
let p = Point(x: 1, y: 2)  var q = shifted(p, 10)  q.y += 5       // name every field, in order
```
Uppercase names; no methods, no self-containing struct (use an array), no tuples.

## inout
Only an `inout` parameter changes the caller's variable; the call repeats it: `fn swap(inout a: int, inout b: int) { let t = a a = b b = t }` ... `swap(inout x, inout y)`. The argument is a `var`, an inout param, or a field/element of one (`inout p.x`, `inout xs[i]`).

## Standard library
`use math` at the top, then `math.sqrt(2.0)`. Modules: `input` (`line()` `lines()` `all()` `eof()`) · `os` (`args()` `env(n)` `has_env(n)` `exit(code)`) · `fs` (`read write append exists list remove mkdir`) · `json` (`str(v)`, `parse(text)` typed by the variable: `let p: Point = json.parse(s)`) · `time` (`now_ms() mono_ms() sleep_ms(ms)`) · `random` (`random() range(lo, hi) seed(n)`) · `math` (`pi e inf sqrt floor ceil round trunc exp log log10 log2 sin cos tan asin acos atan pow(x, y) atan2(y, x)`, all float) · `text` (`fixed(x, digits)`: `text.fixed(2.0 / 3.0, 2)` is "0.67"; `is_int(s)` `is_float(s)`).
Printing: arrays/structs print as Nyra code (`["a", "b"]`, `Point(x: 1, y: 2)`); floats like JavaScript (`3.0` prints `3`). Memory is automatic (`free(x)`, `arena { }`, `keep(x)` exist but are never required).

## Errors
Compile errors have a code (E0001-E0309), line, col and hint; `nyra check f.nyra --json` lists them, `nyra explain E0201` explains one. A runtime failure stops the program with exit code 101: E0240 index out of bounds · E0241 integer `/` or `%` by zero · E0242 `pop()` on empty · E0243 bad argument (`repeat(-1)`, `split("")`) · E0244 `int(s)`/`float(s)` of non-numeric text (`int("")` too) · E0247 `min()`/`max()` of empty · E0248 missing map key.
