<!-- Metadata: this comment is not part of the card (nyra_spec, bench/run.py --spec card and the tests strip it).
TOKENS: 1399 on claude-sonnet-5-5, 1144 on claude-haiku-4-5 (Anthropic count_tokens; python tools/card_tokens.py --write).
BUDGET: hard limit 1,400 tokens on claude-sonnet-5-5. The card has a hard budget: a feature that needs card text must displace something.
The full language is docs/SPEC.md; this card must describe the current compiler exactly (tests/docs.rs runs its example). -->
# Nyra v0.7 agent card
Not Rust, Go, TypeScript or Python: use only what is listed.

```nyra
struct Item { name: str, qty: int }
fn sq(x: int) -> int = x * x   ex sq(3) == 9, sq(-2) == 4
fn total(xs: [Item]) -> int {
    var sum = 0
    for x in xs { sum += x.qty }
    return sum
}
fn bump(inout a: [int], i: int) { a[i] += 1 }  // params are read-only unless `inout` (also at the call)

let items = [Item(name: "pen", qty: 2), Item(name: "ink", qty: 1)]
var hits = [0, 0]
var stock: [str: int] = [:]
bump(inout hits, 1)
stock["pen"] = stock.get("pen", 0) + items[0].qty
for i, x in items { print("{i}: {x.name}") }
print(total(items), hits, stock, "{2.0 / 3.0:.2}")
print([n * n for n in 0..6 if n % 2 == 0], items.map(x => x.qty).sum())
```

## Rules
- A program is top-level statements (a script). Script `let`/`var` are visible in every fn.
- One statement per line, no `;`; `{` on the same line. A line may break inside `( )` `[ ]` or at an operator.
- Params (`b: int = 0` has a default) and the return type are written; `-> T` needs `return` on every path; no `->` returns nothing; `= expr` is the one-line form.
- Named args go last: `f(1, b: 2)`. A `{ }` block after a call, one element per line, is its last `[T]` arg.
- `let` immutable, `var` mutable. No shadowing.
- No implicit conversions: `int() float() str() char(n) c.code()`. `s[i]` is a `char`. Conditions are `bool`; int `/` truncates.
- `a..b` excludes `b`; `step -2`; `for k in map` gives keys. `if`/`match` are values: `if a > b { a } else { b }`, `a > b ? a : b`.
- Assignment, passing and returning copy. Changing an array or map needs a `var`.
- `"{expr}"` interpolates. `print(a, b)` joins with a space; `print(x, end: "")`.
- Tuples `(1, "a")`, `t.0`, `let (a, b) = t`; `int?`, `none`, `x ?? d`, `if let v = x { }`; `enum S { C(float), N }`, `S.C(1.0)`, `match s { S.C(r) => ..., _ => ... }`; `xs[a..b]`; `{s:>8}`; `r.area()` is `area(r)`.
- A lambda `x => e` is only an array-method arg and changes nothing. `ex` takes literals and calls.

## Not in Nyra
`null`, generics, closures, classes and `impl`, `elif`, `and`/`or`/`not` (`&&` `||` `!`), `i++`, `reduce` (`fold`).

## Names
- Methods `s.len()`; modules `math.sqrt(x)`; bare: `print abs min max` and the conversions
- str: `len slice contains starts_with ends_with index_of pad_left pad_right split replace repeat trim upper lower chars codes reversed chunks to_int to_float count any all find_index`
- char: `code upper lower is_digit is_letter is_upper is_lower is_space`
- array: `len push pop insert remove swap contains index_of slice repeat sort reverse reversed join sum min max map filter count any all find find_index fold sort_by sorted sorted_by min_by max_by chunks`
- map: `len has get set remove keys values items`
- modules (`use math`): input `line lines all eof`; os `args env has_env exit`; fs `read write append exists list remove mkdir`; json `str parse`; time `now_ms mono_ms sleep_ms`; random `random range seed`; math `pi e inf sqrt floor ceil round trunc exp log log10 log2 sin cos tan asin acos atan atan2 pow`; text `fixed is_int is_float`
