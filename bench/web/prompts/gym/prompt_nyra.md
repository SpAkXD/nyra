You are a senior developer writing **Nyra**, a small programming language made for AI agents. Nyra builds websites: one `.nyra` file with `use web` is compiled by `nyra build site.nyra` into static HTML, CSS and a little JavaScript. Build the website described below as **one single file, `site.nyra`**.

You have not seen Nyra before. Everything you need is in the two reference cards below: the language card and the web card. Use only what they describe; Nyra is not Python, TypeScript, Rust or JSX.

## Rules
- One file, `site.nyra`, that starts with `use web`. No HTML, CSS or JavaScript of your own unless the web card's escape hatches are truly needed: the built-in components, theme and motion words already give a polished look.
- The images are the given URLs; do not use other images.
- It must compile: follow the cards exactly (names, argument order, named options).

## The site to build

A one-page website for **Forge Fitness Studio**, a small independent gym and group-class studio in Brno (Czech Republic). Use exactly the text, data and image URLs below. The site must look premium, bold and energetic (think a well-designed boutique fitness brand in 2026), work on phones and desktops, and be accessible (alt texts, labels, keyboard focus).

**Look:** dark and strong. Background `#111315` (near black), surfaces `#1B1E22`, text `#F2F2F0`, primary color `#E4572E` (ember orange), accent `#F3C969` (warm yellow). Headings in **Archivo** (bold, uppercase), body text in **Inter** (both from Google Fonts). Rounded cards, subtle borders, generous spacing.

**Motion:** the hero text comes in step by step (kicker, title, text, buttons); sections and their cards reveal as you scroll (cards one after another); cards lift slightly on hover; the numbers count up when they appear. Respect "reduced motion" settings: with reduced motion nothing moves and the numbers show their final value at once.

### Sections, in order

1. **Navigation bar**, sticky at the top: brand "Forge Fitness", links Classes (`#classes`), Trainers (`#trainers`), Membership (`#membership`), Gallery (`#gallery`), Contact (`#contact`), and a highlighted button "Free trial" linking to `#contact`. On phones the links fold into a menu button.

2. **Hero**, a full-screen photo with a dark gradient so the text is readable.
   - Photo: `https://picsum.photos/seed/forge-hero/1600/1000` (alt: "Athletes training with kettlebells at Forge")
   - Small kicker line: "Brno · Veveří 54 · since 2016"
   - Title: "Train hard. Feel at home."
   - Text: "Strength, conditioning and yoga classes in small groups, with coaches who know your name."
   - Buttons: "See the schedule" (`#classes`, filled) and "Free trial" (`#contact`, outlined)

3. **Numbers**: four items; the first three count up: **12** coaches, **45** classes a week, **2,300** members; the fourth is text and does not count: **6 a.m.** first class.

4. **Classes** (`#classes`), kicker "Schedule", title "Classes this week", subtitle "Book through the app or at the front desk. Max 12 people per class."
   - Day filter buttons: All, Mon, Tue, Wed, Thu, Fri, Sat. "All" is active when the page loads. Clicking a day shows only that day's classes (without reloading the page); the active button is highlighted.
   - Twelve classes as cards with the day, time, class name, coach and level:
     1. Mon · 6:00 · "Sunrise Strength" · Coach Adam · All levels
     2. Mon · 18:00 · "Kettlebell Basics" · Coach Klára · Beginner
     3. Tue · 7:00 · "Mobility Flow" · Coach Tereza · All levels
     4. Tue · 19:00 · "Power Circuit" · Coach Ondřej · Advanced
     5. Wed · 6:00 · "Engine Builder" · Coach Adam · Intermediate
     6. Wed · 17:30 · "Vinyasa Yoga" · Coach Tereza · All levels
     7. Thu · 7:00 · "Olympic Lifting" · Coach Ondřej · Advanced
     8. Thu · 18:30 · "Core & Conditioning" · Coach Klára · Intermediate
     9. Fri · 6:00 · "Friday Burn" · Coach Adam · Intermediate
     10. Fri · 17:00 · "Stretch & Restore" · Coach Tereza · Beginner
     11. Sat · 9:00 · "Team WOD" · Coach Ondřej · All levels
     12. Sat · 10:30 · "Weekend Yoga" · Coach Tereza · Beginner

5. **Trainers** (`#trainers`), kicker "Coaches", title "Meet the team". Four coaches, each with a photo, name, specialty and one sentence. Photo URL `https://picsum.photos/seed/forge-coach-N/600/700` where N is 1 to 4; the alt text is the coach's name.
   1. "Adam Novotný" · "Strength & conditioning" · "Former rower who builds engines that last."
   2. "Klára Benešová" · "Kettlebells & core" · "Turns nervous beginners into confident lifters."
   3. "Tereza Malá" · "Yoga & mobility" · "Helps you move better so you can train longer."
   4. "Ondřej Černý" · "Olympic lifting" · "National-level lifter with an eye for technique."

6. **Membership** (`#membership`), kicker "Membership", title "Pick your plan". A switch "Monthly / Yearly" changes the prices of all three plans (without reloading the page); Monthly is selected when the page loads.
   - "Basic" · 790 Kč per month · 7,900 Kč per year · Gym floor access, 2 classes a week, App booking
   - "Unlimited" (marked "Most popular", highlighted) · 1,290 Kč per month · 12,900 Kč per year · Gym floor access, Unlimited classes, App booking, 1 coaching session a month
   - "Premium" · 1,990 Kč per month · 19,900 Kč per year · Everything in Unlimited, Weekly personal training, Towel and locker service
   - Each plan has a button "Join now" linking to `#contact`.

7. **Testimonials**, title "Members say", three quotes:
   - "I came for the free trial and stayed for the people." — Lenka M.
   - "Adam fixed my squat in two sessions after years of knee pain." — Jakub S.
   - "The only gym where the 6 a.m. class feels like a party." — Veronika D.

8. **Gallery** (`#gallery`), kicker "Gallery", title "Inside the studio". Eight photos in a grid; clicking a photo opens it large (a lightbox with a close button and next/previous buttons, also usable with the keyboard: Escape closes, arrow keys move). Photos: `https://picsum.photos/seed/forge-g1/1200/800` up to `forge-g8`. Alt texts: "The main training floor", "Kettlebells lined up", "A yoga class at sunset", "Barbells and plates", "The rowing machines", "A coach helping a member", "The stretching corner", "The front desk and café".

9. **Contact** (`#contact`), kicker "Contact", title "Come for a free trial".
   - Address: "Veveří 54, 602 00 Brno", with a link "Open in Google Maps" (`https://maps.google.com/?q=Veveří+54,+Brno`).
   - Phone "+420 541 000 777" (clickable `tel:` link) and e-mail "hello@forgefitness.cz" (clickable `mailto:` link).
   - Opening hours: Monday–Friday 6:00–22:00, Saturday–Sunday 8:00–20:00.
   - A form with Name (required), E-mail (required), Phone (optional), Interested in (required, a select with: Free trial, Membership, Personal training) and Message (required). Every field has a visible label. The button says "Send".
   - Sending it opens the visitor's e-mail app with a message addressed to hello@forgefitness.cz, subject "Free trial request", and all the entered details in the body (no server).

10. **Footer**: "Forge Fitness Studio", the address, the five section links, and "© 2026 Forge Fitness Studio".

Page title: "Forge Fitness Studio – strength, conditioning and yoga in Brno". Description for search engines: "Independent gym and group-class studio on Veveří in Brno: small classes, expert coaches, monthly or yearly membership and a free trial."

---

# Reference 1: the Nyra language card

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

---

# Reference 2: the Nyra web card

# Nyra web card
`use web` makes a static site: `nyra build site.nyra -o dist`.

```nyra
use web
site("Brew & Co", primary: "#3B2A1A", display: "Playfair Display")
let drinks = [("Latte", "hot", "latte.jpg"), ("Cold brew", "cold", "cold.jpg")]
enum Msg { Show(str) }
var kind = "all"
fn update(m: Msg) { match m { Msg.Show(k) => kind = k } }
fn menu() -> Node = stack {
    row {
        for k in ["all", "hot", "cold"] { button(k, on: Msg.Show(k), active: k == kind) }
    }
    grid(3, anim: "stagger", hover: "lift") {
        for d in drinks.filter(x => kind == "all" || x.1 == kind) { card(d.0, d.1, image: d.2) }
    }
}
page("/", "Brew & Co") {
    navbar("Brew & Co", [("Menu", "#menu")])
    hero("Good coffee", image: "hero.jpg", alt: "Latte art", buttons: [("Menu", "#menu")], anim: "rise")
    section("menu", "Our menu", bg: "alt", anim: "rise") {
        menu()
    }
}
```

## Rules
- Params before the first `x:` are positional, the rest named. All elements take `id: cls:` and motion.
- `{}` takes a child block (`for` `if` `let` allowed); a string line is a paragraph.
- `on: Msg.X(..)` calls `fn update(m: Msg)`; then a parameterless `Node` fn that reads a changed `var` redraws.
- Text: `**b**` `[label](url)` `:icon:` (Lucide). Images: `assets/` files or `https://` URLs.

## Elements
```
site(name, primary: accent: bg: font: display: radius: 16 style: soft|sharp|editorial|dark lang: tz:)
page(path, title, desc:) {}
section(id, title, kicker: text: bg: alt|dark|white|primary|accent) {}
grid(cols, widths: "1fr 2fr") {}   stack {}   row {}
card(title, text, icon: image: tag: items: fill: href:) {}
h1 h2 h3(text, icon:)   p small note(text)   link(text, href)   img(src, alt, ratio: "4/3" caption:)
button(text, href, style: primary|accent|outline|ghost|light on: active: icon:)
list(items, icon:)   table(rows, head:)   address {}   chip(title, text, icon:)   embed(url, title)
quote(text, name, place: stars:)   stat(value, label)   steps([(label, text)])   marquee(words)
navbar(brand, [(label, href)], cta: (label, href))
hero(title, kicker: text: image: alt: buttons: [(label, href)] badge: rows) {}   badge: hours rows, shows open/closed now
gallery([(image, alt, tag)], filters: [(tag, label)])
hours(rows)   rows: [(label, "1-5 8:00-16:00")], days 1-7 = Mon-Sun, "" closed
map(address)   split { card(...) }   footer(text: links: contact: [str] small: [str]) {}
form(title, mailto: | on:) {}   field(name, label, type: text|email|tel|date|area|select|check req: hint: options: [str])
css(".x { color: red }")
```

## Motion
- `anim:` fade rise zoom pop words count stagger; `hover:` lift glow zoom tilt; `loop:` float pulse; `scroll:` parallax; modifiers: `"rise slow 0.2 spring"`. `keyframes("sway", "50% { rotate: 3deg }")` adds a word.
- Nothing moves unasked: give sections and the hero `anim: "rise"`.

## Mistakes
- No JSX, tags, `class:`, CSS `style:` or handler fns: `on:` takes a `Msg`.
- E0372: a fn or script variable named like a web function (`page hours map list`).

---

## Your answer
Reply with the complete `site.nyra` in one code block and nothing else: no explanation before or after, no "rest of the code here" placeholders.
