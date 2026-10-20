You are a senior developer writing **Nyra**, a small programming language made for AI agents. Nyra builds websites: one `.nyra` file with `use web` is compiled by `nyra build site.nyra` into static HTML, CSS and a little JavaScript. Build the website described below as **one single file, `site.nyra`**.

You have not seen Nyra before. Everything you need is in the two reference cards below: the language card and the web card. Use only what they describe; Nyra is not Python, TypeScript, Rust or JSX.

## Rules
- One file, `site.nyra`, that starts with `use web`. No HTML, CSS or JavaScript of your own unless the web card's escape hatches are truly needed: the built-in components, theme and motion words already give a polished look.
- The images are the given URLs; do not use other images.
- It must compile: follow the cards exactly (names, argument order, named options).

## The site to build

A one-page website for **Autoservis Dvořák**, an independent car repair shop in Pardubice (Czech Republic). Use exactly the text, data and image URLs below. The site must look premium, solid and trustworthy (think a well-designed modern workshop brand in 2026), work on phones and desktops, and be accessible (alt texts, labels, keyboard focus).

**Look:** clean and technical. Background `#F4F5F7` (light grey), surfaces `#FFFFFF`, text `#1C2430`, primary color `#1F4E8C` (deep blue), accent `#F2A900` (signal yellow). Headings in **Barlow Condensed** (bold), body text in **Source Sans 3** (both from Google Fonts). Rounded cards, thin borders, strong section headings.

**Motion:** the hero text comes in step by step (kicker, title, text, buttons); sections and their cards reveal as you scroll (cards one after another); the four steps of "How it works" appear one after another; cards lift slightly on hover; the numbers count up when they appear. Respect "reduced motion" settings: with reduced motion nothing moves and the numbers show their final value at once.

### Sections, in order

1. **Navigation bar**, sticky at the top: brand "Autoservis Dvořák", links Services (`#services`), Prices (`#prices`), How it works (`#process`), Team (`#team`), Contact (`#contact`), and a highlighted button "Book a service" linking to `#contact`. On phones the links fold into a menu button.

2. **Hero**, a full-screen photo with a dark gradient so the text is readable.
   - Photo: `https://picsum.photos/seed/dvorak-hero/1600/1000` (alt: "A mechanic working under a lifted car at Autoservis Dvořák")
   - Small kicker line: "Pardubice · Hradecká 88 · all makes"
   - Title: "Honest repairs, done right the first time"
   - Text: "Servicing, tyres, diagnostics and bodywork for every make. Fixed prices up front and a call before any extra work."
   - Buttons: "Get a price" (`#prices`, filled) and "Book a service" (`#contact`, outlined)

3. **Numbers**: four items; the first three count up: **24** years in Pardubice, **9,800** cars serviced last year, **6** certified mechanics; the fourth is text and does not count: **2 years** warranty on parts and labour.

4. **Services** (`#services`), kicker "Services", title "What we do". Four tabs: Servicing, Tyres, Diagnostics, Bodywork. "Servicing" is selected when the page loads. Clicking a tab shows its panel and hides the others (without reloading the page); the selected tab is highlighted. Each panel has one sentence and three points:
   - Servicing: "Scheduled servicing to the maker's plan, so your warranty stays valid." · "Oil and filter change" · "Brake check and fluids" · "Stamp in your service book"
   - Tyres: "Seasonal tyre changes, storage and repairs while you wait." · "Tyre fitting and balancing" · "Tyre hotel for 1,200 Kč a season" · "Puncture repair"
   - Diagnostics: "We read the fault codes and find the real cause before we replace anything." · "Engine and electronics diagnostics" · "Air-con check and refill" · "Pre-purchase inspection"
   - Bodywork: "Dents, scratches and paint, matched in our own paint shop." · "Dent repair" · "Colour-matched paintwork" · "Insurance claims handled for you"

5. **Price calculator** (`#prices`), kicker "Prices", title "Get a price in ten seconds", subtitle "Fixed prices including VAT, parts and labour."
   - The visitor picks the car size: "Small car", "Family car" or "SUV / van". "Family car" is selected when the page loads.
   - Then ticks the services they need (none is ticked when the page loads). Each service shows its price for the selected car size:

     | Service | Small car | Family car | SUV / van |
     |---|---|---|---|
     | Oil service | 1,490 Kč | 1,790 Kč | 2,290 Kč |
     | Front brake pads | 2,190 Kč | 2,590 Kč | 3,190 Kč |
     | Air-con service | 1,290 Kč | 1,290 Kč | 1,590 Kč |
     | Wheel alignment | 990 Kč | 990 Kč | 1,290 Kč |
     | Tyre change (4 wheels) | 800 Kč | 960 Kč | 1,200 Kč |

   - Below, "Estimated total" shows the sum of the ticked services for the selected car size, for example "3,080 Kč" for an oil service and an air-con service on a family car. It updates at once when the car size or a service changes (without reloading the page); with nothing ticked it shows "0 Kč".
   - A button "Book this service" linking to `#contact`.

6. **How it works** (`#process`), kicker "How it works", title "Four steps, no surprises". Four numbered steps along a line:
   1. "Book online or call" · "Pick a time that suits you. Most services are done the same day."
   2. "Drop off your car" · "Leave the key at the desk, or in our key box before 7:00."
   3. "We call before extra work" · "If we find anything else, you decide. Nothing is done without your OK."
   4. "Pick up and pay" · "Pay by card or bank transfer. Every job comes with a 2-year warranty."

7. **Team** (`#team`), kicker "Team", title "The people under the bonnet". Four people, each with a photo, name, role and one sentence. Photo URL `https://picsum.photos/seed/dvorak-team-N/600/700` where N is 1 to 4; the alt text is the person's name.
   1. "Jiří Dvořák" · "Owner, master mechanic" · "Opened the shop in 2002 and still checks every car that leaves it."
   2. "Pavel Svoboda" · "Diagnostics" · "Finds the electrical faults that other garages gave up on."
   3. "Martina Veselá" · "Service advisor" · "Explains every line of your invoice in plain words."
   4. "Lukáš Král" · "Bodywork and paint" · "Matches any colour, from a 1990s Škoda to a brand-new Tesla."

8. **Testimonials**, title "What drivers say", three quotes:
   - "They called before changing anything, and the price was exactly as quoted." — Michal T.
   - "Found an electrical fault that two other garages missed." — Simona K.
   - "Tyres changed in 30 minutes while I had a coffee." — Radek P.

9. **Contact** (`#contact`), kicker "Contact", title "Book your car in".
   - Address: "Hradecká 88, 500 03 Pardubice", with a link "Open in Google Maps" (`https://maps.google.com/?q=Hradecká+88,+Pardubice`).
   - Phone "+420 466 000 245" (clickable `tel:` link) and e-mail "servis@autodvorak.cz" (clickable `mailto:` link).
   - Opening hours: Monday–Friday 7:00–17:00, Saturday 8:00–12:00 (tyres only), Sunday closed.
   - A form with Name (required), Phone (required), E-mail (optional), Car make and model (required), Service (required, a select with: Servicing, Tyres, Diagnostics, Bodywork, Something else), Preferred date (optional, a date field) and Message (optional). Every field has a visible label. The button says "Book service".
   - Sending it opens the visitor's e-mail app with a message addressed to servis@autodvorak.cz, subject "Service booking", and all the entered details in the body (no server).

10. **Footer**: "Autoservis Dvořák", the address, the five section links, and "© 2026 Autoservis Dvořák s.r.o.".

Page title: "Autoservis Dvořák – honest car repairs in Pardubice". Description for search engines: "Independent car repair shop on Hradecká in Pardubice: servicing, tyres, diagnostics and bodywork for all makes, with fixed prices and a 2-year warranty."

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
`use web` makes a static site.

```nyra
use web
site("Brew & Co", primary: "#3B2A1A")
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
site(name, primary: accent: bg: font: display: radius: 16 style: "soft"|"sharp"|"editorial"|"dark" lang: tz:)
page(path, title, desc:) {}   css(".x { color: red }") anywhere
section(id, title, kicker: text: bg: "alt"|"dark"|"white"|"primary"|"accent") {}
grid(cols, widths: "1fr 2fr") {}   stack {}   row {}
card(title, text, icon: image: tag: items: fill: true href: on:) {}
h1 h2 h3(text, icon:)   p small note(text)   link(text, href)   img(src, alt, ratio: "4/3" caption:)
button(text, href, style: "primary"|"accent"|"outline"|"ghost"|"light" on: active: icon:)
list(items, icon:)   table(rows, head:)   address {}   chip(title, text, icon:)   embed(url, title)
quote(text, name, place: stars:)   stat(value, label)   steps([(label, text)])   marquee(words)
navbar(brand, [(label, href)], cta: (label, href))
hero(title, kicker: text: image: alt: buttons: [(label, href)] badge: hoursRows) {}
gallery([(image, alt, tag)], filters: [(tag, label)])
hours(hoursRows)   hoursRows: [("Mon–Fri", "8:00-16:00"), ("Sun", "")]
map(address)   split { card(...) }   footer(text: links: contact: [str] small: [str]) {}
form(title, mailto: subject: send: | on:) {}   field(name, label, type: "text"|"email"|"tel"|"date"|"area"|"select"|"check" req: hint: options: [str])
```

## Motion
- `anim:` fade rise zoom pop words count stagger; `hover:` lift glow zoom tilt; `loop:` float pulse; `scroll:` parallax; modifiers: `"rise slow 0.2 spring"`. `keyframes(name, css)` adds a word.
- Nothing moves unasked: give sections and the hero `anim: "rise"`.

## Mistakes
- No JSX, tags, `class:`, CSS `style:` or handler fns: `on:` takes a `Msg`.
- E0372: a fn or script variable named like a web function (`page hours map list`).

---

## Your answer
Reply with the complete `site.nyra` in one code block and nothing else: no explanation before or after, no "rest of the code here" placeholders.
