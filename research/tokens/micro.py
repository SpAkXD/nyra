"""Per-use token cost of each construct: realistic before (Nyra 0.5) / after (proposed) snippet pairs, counted with the
Claude tokenizer.  Output: micro.json and a table on stdout."""
import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import json
from tokcount import count_many

# (id, label, before, after)
P = [
 ("indent_if", "if/else block: braces -> indentation",
  'if x > 0 {\n    print("pos")\n} else if x < 0 {\n    print("neg")\n} else {\n    print("zero")\n}',
  'if x > 0:\n    print("pos")\nelif x < 0:\n    print("neg")\nelse:\n    print("zero")'),
 ("indent_while", "while block: braces -> indentation",
  'while i < n {\n    s += i\n    i += 1\n}',
  'while i < n:\n    s += i\n    i += 1'),
 ("indent_fn", "fn body: braces -> indentation (signature kept)",
  'fn total(xs: [int], k: int) -> int {\n    var s = 0\n    for x in xs {\n        s += x * k\n    }\n    ret s\n}',
  'fn total(xs: [int], k: int) -> int:\n    var s = 0\n    for x in xs:\n        s += x * k\n    ret s'),
 ("oneline_if", "one-line `if c { ret x }` -> `if c: ret x`",
  'if n < 2 { ret n }', 'if n < 2: ret n'),
 ("main_wrapper", "fn main() wrapper around 6 statements",
  'fn main() {\n    let a = 1\n    let b = 2\n    print(a + b)\n    print(a * b)\n}',
  'let a = 1\nlet b = 2\nprint(a + b)\nprint(a * b)'),
 ("ret_type", "return type `-> int` (inferred)",
  'fn sq(x: int) -> int = x * x', 'fn sq(x: int) = x * x'),
 ("ret_type_block", "return type `-> [int]` on a block fn",
  'fn evens(xs: [int]) -> [int] {\n    ret xs.filter(x => x % 2 == 0)\n}',
  'fn evens(xs: [int]) {\n    ret xs.filter(x => x % 2 == 0)\n}'),
 ("param_types", "parameter types `a: int, b: int` (inferred)",
  'fn add(a: int, b: int) -> int = a + b', 'fn add(a, b) = a + b'),
 ("param_types_arr", "parameter type `xs: [int]` + `s: str`",
  'fn count(xs: [int], s: str) -> int {\n    ret xs.len() + s.len()\n}', 'fn count(xs, s) {\n    ret xs.len() + s.len()\n}'),
 ("let_kw", "`let x = 5` -> `x = 5` (first assignment declares)",
  'let n = 5\nlet name = "ann"\nvar total = 0', 'n = 5\nname = "ann"\ntotal = 0'),
 ("local_ann_arr", "`var xs: [int] = []` -> `var xs = []`",
  'var xs: [int] = []', 'var xs = []'),
 ("local_ann_map", "`var m: [str: int] = [:]` -> `var m = [:]`",
  'var m: [str: int] = [:]', 'var m = [:]'),
 ("implicit_ret", "last `ret x` -> `x`",
  'fn f(x: int) -> int {\n    let y = x * 2\n    ret y + 1\n}', 'fn f(x: int) -> int {\n    let y = x * 2\n    y + 1\n}'),
 ("positional_struct", "Item(name: n, price: p, qty: q) -> Item(n, p, q)",
  'items.push(Item(name: "pen", price: 125, qty: 3))', 'items.push(Item("pen", 125, 3))'),
 ("var_param", "copy a parameter into a var -> `var` parameter",
  'fn gcd(a: int, b: int) -> int {\n    var x = a\n    var y = b\n    ret x\n}', 'fn gcd(var a: int, var b: int) -> int {\n    ret a\n}'),
 ("swap_tuple", "swap through a temp -> `a, b = b, a % b`",
  'let t = x % y\nx = y\ny = t', 'x, y = y, x % y'),
 ("multi_decl", "`var lo = 0` + `var hi = n - 1` -> `var lo, hi = 0, n - 1`",
  'var lo = 0\nvar hi = xs.len() - 1', 'var lo, hi = 0, xs.len() - 1'),
 ("destructure_split", "`let p = s.split(\" \")` + 2 index lets -> `let a, b = s.split(\" \")`",
  'let p = line.split(" ")\nlet op = p[0]\nlet name = p[1]', 'let op, name = line.split(" ")'),
 ("tuple_table", "parallel arrays + index loop -> list of tuples",
  'let vals = [1000, 900, 500, 400]\nlet syms = ["M", "CM", "D", "CD"]\nfor i in 0..vals.len() {\n    while n >= vals[i] {\n        out += syms[i]\n        n -= vals[i]\n    }\n}',
  'for v, s in [(1000, "M"), (900, "CM"), (500, "D"), (400, "CD")]:\n    while n >= v:\n        out += s\n        n -= v'),
 ("tuple_return", "struct Res{ok,text} for 2 values -> tuple",
  'struct Res { ok: bool, text: str }\nfn dec(s: str) -> Res {\n    ret Res(ok: false, text: "")\n}\nlet r = dec(c)\nif !r.ok { print(r.text) }',
  'fn dec(s: str) -> (bool, str) {\n    ret (false, "")\n}\nlet ok, text = dec(c)\nif !ok { print(text) }'),
 ("in_op", "`xs.contains(x)` -> `x in xs`", 'if !names.contains(w) { n += 1 }', 'if w not in names { n += 1 }'),
 ("in_map", "`m.has(k)` -> `k in m`", 'if !seen.has(x) { n += 1 }', 'if x not in seen { n += 1 }'),
 ("neg_index", "`xs[xs.len() - 1]` -> `xs[-1]`", 'let last = items[items.len() - 1]', 'let last = items[-1]'),
 ("ternary", "`if c { a } else { b }` -> `a if c else b`",
  'let sign = if n < 0 { "-" } else { "" }', 'let sign = "-" if n < 0 else ""'),
 ("fmt_pad", "`str(x).pad_left(3)` chain -> `{x:>3}`",
  'print(str(i).pad_left(3) + " " + name.pad_right(6) + " " + str(score).pad_left(4))', 'print("{i:>3} {name:<6} {score:>4}")'),
 ("fmt_zero", "`str(r).pad_left(2, '0')` -> `{r:02}`",
  'ret "{d}." + str(r).pad_left(2, \'0\')', 'ret "{d}.{r:02}"'),
 ("fmt_float", "text.fixed(x, 2) -> `{x:.2f}`",
  'use text\nprint("avg " + text.fixed(avg, 2))', 'print("avg {avg:.2f}")'),
 ("fmt_thousands", "manual digit grouping fn -> `{n:,}`",
  'fn group(n: int) -> str {\n    let s = str(n)\n    var out = ""\n    for i, c in s {\n        if i > 0 && (s.len() - i) % 3 == 0 { out += "," }\n        out += str(c)\n    }\n    ret out\n}\nprint(group(total))',
  'print("{total:,}")'),
 ("interp_vs_plus", "\"a\" + str(x) + \"b\" -> \"a{x}b\"",
  'print("n=" + str(n) + " s=" + str(s))', 'print("n={n} s={s}")'),
 ("compr_push", "push loop -> comprehension",
  'var sq: [int] = []\nfor x in xs {\n    if x > 0 {\n        sq.push(x * x)\n    }\n}', 'let sq = [x * x for x in xs if x > 0]'),
 ("sorted_by_tuple", "in-place sort + comparator helper -> sorted_by(tuple key)",
  'fn before(a: Team, b: Team) -> bool {\n    if a.pts != b.pts { ret a.pts > b.pts }\n    ret a.name < b.name\n}\nfor i in 1..n {\n    var j = i\n    while j > 0 && before(ts[j], ts[j - 1]) {\n        ts.swap(j, j - 1)\n        j -= 1\n    }\n}',
  'ts.sort_by(t => (-t.pts, t.name))'),
 ("nested_mut", "map of maps: get/modify/set -> in-place `m[a][b] += n`",
  'var row = acc.get(name, [0].repeat(13))\nrow[m] += n\nacc[name] = row', 'acc[name][m] += n'),
 ("trim_chars", "`.trim(\"'\")` instead of two while loops",
  'var w = cur\nwhile w.len() > 0 && w[0] == \'\\\'\' {\n    w = w.slice(1, w.len())\n}\nwhile w.len() > 0 && w[w.len() - 1] == \'\\\'\' {\n    w = w.slice(0, w.len() - 1)\n}',
  'let w = cur.trim("\'")'),
 ("for_enum", "`for i in 0..xs.len() { xs[i] }` -> `for i, x in xs`",
  'for i in 0..xs.len() {\n    print(i, xs[i])\n}', 'for i, x in xs {\n    print(i, x)\n}'),
 ("bare_lambda", "`.map(x => int(x))` -> `.map(int)`", 'let ns = parts.map(x => int(x))', 'let ns = parts.map(int)'),
 ("print_noparen", "print(x) -> print x (not proposed; for scale)", 'print(total)\nprint(avg)\nprint(name)', 'print total\nprint avg\nprint name'),
 ("tab_indent", "4 spaces -> 1 tab (formatter-enforced)",
  'for i in 0..n {\n    if i % 2 == 0 {\n        if i > 4 {\n            print(i)\n        }\n    }\n}', 'for i in 0..n {\n\tif i % 2 == 0 {\n\t\tif i > 4 {\n\t\t\tprint(i)\n\t\t}\n\t}\n}'),
 ("slice_syntax", "`s.slice(0, 9)` -> `s[:9]`", 'let head = s.slice(0, 9)', 'let head = s[:9]'),
 ("is_digit_int", "digit value: `c.code() - '0'.code()` -> `int(c)`", 'n = n * 10 + (c.code() - \'0\'.code())', 'n = n * 10 + int(c)'),
 ("count_get", "`m[k] = m.get(k, 0) + 1` -> `m[k] += 1`", 'counts[w] = counts.get(w, 0) + 1', 'counts[w] += 1'),
 ("fn_def_kw", "fn -> def (same token count)", 'fn add(a: int) -> int = a', 'def add(a: int) -> int = a'),
]

texts = []
for pid, label, b, a in P:
    texts += [b, a]
c = count_many(texts)
out = []
print(f"{'id':18} {'before':>6} {'after':>6} {'saved':>6}  label")
for i, (pid, label, b, a) in enumerate(P):
    nb, na = c[2 * i], c[2 * i + 1]
    out.append({"id": pid, "label": label, "before": nb, "after": na, "saved": nb - na, "before_text": b, "after_text": a})
    print(f"{pid:18} {nb:6} {na:6} {nb-na:6}  {label}")
json.dump(out, open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "micro.json"), "w"), indent=1)
