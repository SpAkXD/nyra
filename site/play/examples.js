// The example programs of the playground. Each one prints in the browser what `nyra run --sandbox
// --allow input` prints natively (`node tools/check_wasm.mjs --site ...` in the compiler repo checks it).
export const EXAMPLES = [
  {
    id: "tour",
    label: "Tour",
    note: "structs, `ex` checks, inout, lambdas and comprehensions",
    code: `// A quick tour. Press Run (or Ctrl+Enter).
struct Item { name: str, qty: int }

fn sq(x: int) -> int = x * x   ex sq(3) == 9, sq(-2) == 4

fn total(xs: [Item]) -> int {
    var sum = 0
    for x in xs { sum += x.qty }
    return sum
}

fn bump(inout a: [int], i: int) { a[i] += 1 }

let items = [Item(name: "pen", qty: 2), Item(name: "ink", qty: 1)]
var hits = [0, 0]
bump(inout hits, 1)
for i, x in items { print("{i}: {x.name}") }
print(total(items), hits, sq(12))

// lambdas are arguments of array methods; comprehensions build arrays
let xs = [3, -1, 4, -1, 5, 9, -2, 6]
print(xs.filter(x => x > 0).map(x => x * x).sum(), xs.count(x => x < 0))
print(xs.sorted_by(x => -x), xs.fold(0, (acc, x) => acc + x))
print([n * n for n in 0..6 if n % 2 == 0], [c.upper() for c in "nyra" if c != 'y'])
`,
  },
  {
    id: "tuples",
    label: "Tuples",
    note: "return several values, take them apart, sort by them",
    code: `// Tuples: several values in one.
fn divmod(a: int, b: int) -> (int, int) {
    return (a / b, a % b)
}

fn minmax(xs: [int]) -> (int, int) = (xs.min(), xs.max())

let (q, r) = divmod(17, 5)
print("17 = 5 * {q} + {r}")

let (lo, hi) = minmax([4, 1, 9, 3])
print("range {lo}..{hi}")

var (x, y) = (3, 4)
(x, y) = (y, x)
print(x, y)

// tuples compare element by element, so they sort
var scores = [(3, "ann"), (1, "bob"), (3, "abe"), (2, "cy")]
scores.sort()
print(scores)
for i, (n, name) in scores {
    print("{i + 1}. {name} ({n})")
}
`,
  },
  {
    id: "optionals",
    label: "Optionals",
    note: "`T?`, `none`, `??` and `if let` instead of null",
    code: `// No null: a value that may be missing has the type \`T?\`.
struct User { name: str, age: int }

fn find(users: [User], name: str) -> User? {
    for u in users {
        if u.name == name {
            return u
        }
    }
    return none
}

let users = [User(name: "ada", age: 36), User(name: "alan", age: 41)]

if let u = find(users, "ada") {
    print("found {u.name}, {u.age}")
}
print(find(users, "grace"))

// \`??\` gives a default
let age = (find(users, "grace") ?? User(name: "?", age: 0)).age
print("age: {age}")

// text to numbers: to_int() is an int?
for s in ["12", "x", "-7"] {
    print(s, "->", s.to_int() ?? 0)
}
`,
  },
  {
    id: "enums",
    label: "Enums",
    note: "a fixed set of cases, and `match` must cover all of them",
    code: `// Enums and match. Remove a case from \`turn\` and see what the checker says.
enum Dir { N, E, S, W }

fn turn(d: Dir) -> Dir {
    match d {
        Dir.N => return Dir.E
        Dir.E => return Dir.S
        Dir.S => return Dir.W
        Dir.W => return Dir.N
    }
}

fn step(p: (int, int), d: Dir) -> (int, int) {
    match d {
        Dir.N => return (p.0, p.1 + 1)
        Dir.S => return (p.0, p.1 - 1)
        Dir.E => return (p.0 + 1, p.1)
        Dir.W => return (p.0 - 1, p.1)
    }
}

var pos = (0, 0)
var facing = Dir.N
for c in "FFRFFRFL" {
    match c {
        'F' => pos = step(pos, facing)
        'R' => facing = turn(facing)
        _ => facing = turn(turn(turn(facing)))
    }
}
print("at {pos}, facing {facing}")
print(Dir.all())
`,
  },
  {
    id: "format",
    label: "Format specs",
    note: "width, alignment, decimals and thousands, the same on every backend",
    code: `// Format specs inside { }: a subset of Python's.
struct Row { name: str, qty: int, price: float }

let rows = [
    Row(name: "pen", qty: 1200, price: 1.5),
    Row(name: "notebook", qty: 35, price: 4.25),
    Row(name: "ink", qty: 7, price: 12.0),
]

let (item, qty, price, line) = ("item", "qty", "price", "-".repeat(28))
print("{item:<10}{qty:>8}{price:>10}")
print(line)
var total = 0.0
for r in rows {
    print("{r.name:<10}{r.qty:>8,}{r.price:>10.2f}")
    total += float(r.qty) * r.price
}
print(line)
let label = "total"
print("{label:<10}{total:>18,.2f}")

// zeros, signs, precision, centring with a fill character
let name = "mid"
print("{7:03} {42:+} {3.14159:.3} [{name:^9}] [{name:*^9}]")
`,
  },
  {
    id: "maps",
    label: "Maps",
    note: "count words with a map, then sort the entries",
    code: `// Maps: keys to values. Counting words is the classic.
let text = "the cat and the hat and the bat"

var counts: [str: int] = [:]
for w in text.split(" ") {
    counts[w] = counts.get(w, 0) + 1
}
print(counts)
print(counts.has("cat"), counts.has("dog"), counts.len())

// the entries as (word, count) tuples, most frequent first
let top = counts.items().sorted_by(e => -e.1)
for (w, n) in top {
    print("{w:>4} {"#".repeat(n)}")
}
`,
  },
  {
    id: "ex",
    label: "ex checks",
    note: "examples next to a function run at compile time",
    code: `// \`ex\` lines are tests the compiler runs while it checks the program.
// Change a result below (say 55 to 56) and watch the editor flag it.
fn fib(n: int) -> int {
    var a = 0
    var b = 1
    for _ in 0..n {
        (a, b) = (b, a + b)
    }
    return a
}
ex fib(0) == 0, fib(1) == 1, fib(10) == 55

fn is_palindrome(s: str) -> bool = s == s.reversed()   ex is_palindrome("level"), !is_palindrome("nyra")

print([fib(i) for i in 0..12])
print(is_palindrome("racecar"))
`,
  },
  {
    id: "stdin",
    label: "Input",
    note: "`use input` is the one capability the playground grants",
    stdin: "3\n14\nfifteen\n92\n",
    code: `// Reads the stdin box below the editor. \`input\` is a capability:
// a program must say \`use input\` to read it.
use input

var total = 0
var skipped: [str] = []
for line in input.lines() {
    if let n = line.to_int() {
        total += n
    } else {
        skipped.push(line)
    }
}
print("sum: {total}")
print("skipped: {skipped}")
`,
  },
  {
    id: "caps",
    label: "use fs",
    note: "a capability that is not granted fails before anything runs",
    code: `// The playground grants only \`input\`. This program asks for the
// file system, so it stops at compile time with E0290: nothing runs.
use fs

let notes = fs.read("notes.txt")
print(notes)
`,
  },
  {
    id: "fixme",
    label: "Fix me",
    note: "habits from other languages: press Fix",
    code: `// Written with habits from Python and JavaScript. Press Fix:
// every mistake with one certain repair is rewritten in place.
fn average(xs: [float]) -> float {
    return xs.sum() / float(len(xs));
}

fn report(xs: [float]) {
    if average(xs) > 8 and xs.length() > 2 {
        print("great average:", average(xs))
    } elif len(xs) == 0 {
        print("no scores yet")
    } else {
        print("keep going")
    }
}

let scores = [7.5, 9.0, 6.5]
scores.append(10.0)
report(scores)
`,
  },
];
