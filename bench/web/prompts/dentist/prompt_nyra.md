You are a senior developer writing **Nyra**, a small programming language made for AI agents. Nyra builds websites: one `.nyra` file with `use web` is compiled by `nyra build site.nyra` into static HTML, CSS and a little JavaScript. Build the website described below as **one single file, `site.nyra`**.

You have not seen Nyra before. Everything you need is in the two reference cards below: the language card and the web card. Use only what they describe; Nyra is not Python, TypeScript, Rust or JSX.

## Rules
- One file, `site.nyra`, that starts with `use web`. No HTML, CSS or JavaScript of your own unless the web card's escape hatches are truly needed: the built-in components, theme and motion words already give a polished look.
- The images are the given URLs; do not use other images.
- It must compile: follow the cards exactly (names, argument order, named options).

## The site to build

A one-page website for **Zelená Dental**, a small dental practice of Dr. Petra Zelená in Olomouc (Czech Republic). Use exactly the text, data and image URLs below. The site must look premium, clean and trustworthy (think a well-designed private clinic site in 2026), work on phones and desktops, and be accessible (alt texts, labels, keyboard focus).

**Look:** fresh and calm. Primary color `#0F766E` (teal), accent `#F59E0B` (amber), background `#F7FAF9` (very light mint), text `#14302C`. Headings in **Outfit**, body text in **Inter** (both from Google Fonts). Rounded cards, soft shadows, lots of white space.

**Motion:** the hero text comes in step by step (kicker, title, text, buttons); sections and their cards reveal as you scroll (cards one after another); cards lift slightly on hover; the three numbers count up when they appear. Respect "reduced motion" settings: with reduced motion nothing moves and the numbers show their final value at once.

### Sections, in order

1. **Navigation bar**, sticky at the top: brand "Zelená Dental", links Services (`#services`), Team (`#team`), Prices (`#prices`), FAQ (`#faq`), Contact (`#contact`), and a highlighted button "Book a visit" linking to `#booking`. On phones the links fold into a menu button.

2. **Hero**, a large photo with a dark gradient so the text is readable.
   - Photo: `https://picsum.photos/seed/zelena-hero/1600/1000` (alt: "The bright treatment room of Zelená Dental")
   - Small kicker line: "Olomouc · Horní náměstí 21"
   - Title: "Gentle dentistry for the whole family"
   - Text: "Check-ups, hygiene and modern treatment without the stress. New patients welcome, children from 3 years."
   - Buttons: "Book a visit" (`#booking`, filled) and "See prices" (`#prices`, outlined)
   - A small live badge that says exactly "Open now" or "Closed now", computed in the visitor's browser from the opening hours below (use the visitor's local time).

3. **Numbers**: three numbers that count up: **15** years in Olomouc, **4,800** patients, **98** % would recommend us.

4. **Services** (`#services`), kicker "What we do", title "Care for every smile". Six cards, each with a small inline SVG icon, a name and a sentence:
   - "Check-ups" · "A thorough exam every six months, with digital X-rays only when needed."
   - "Dental hygiene" · "Scaling, polishing and airflow cleaning by our hygienist."
   - "Fillings" · "Tooth-coloured composite fillings that last."
   - "Whitening" · "Safe in-office or at-home whitening for a brighter smile."
   - "Children's dentistry" · "Calm first visits and sealants for kids from 3 years."
   - "Emergency care" · "Toothache? Call before 10 a.m. and we will see you the same day."

5. **Team** (`#team`), kicker "Our team", title "People who take their time". Four people, each with a photo, name, role and one sentence. Photo URL `https://picsum.photos/seed/zelena-team-N/600/600` where N is 1 to 4; the alt text is the person's name.
   1. "Dr. Petra Zelená" · "Dentist, founder" · "Twenty years of practice and a soft spot for nervous patients."
   2. "Dr. Martin Kovář" · "Dentist" · "Fillings, root canals and crowns, done carefully and explained clearly."
   3. "Lucie Horáková" · "Dental hygienist" · "Makes cleanings painless and teaches the tricks that keep teeth healthy."
   4. "Eva Šimková" · "Receptionist" · "Finds you a time that works and answers every question about insurance."

6. **Prices** (`#prices`), kicker "Prices", title "Clear prices, no surprises", subtitle "Prices in CZK. Check-ups are covered by public health insurance." A table with two columns, Treatment and Price:
   - Check-up (insured) · 0 Kč
   - Check-up (self-paying) · 900 Kč
   - Dental hygiene · 1,600 Kč
   - Composite filling · from 1,900 Kč
   - Root canal treatment · from 4,500 Kč
   - In-office whitening · 6,900 Kč
   - Children's check-up · 0 Kč
   - Emergency visit · 1,200 Kč

7. **Testimonials**, title "What our patients say", three quotes:
   - "I used to dread the dentist. Here I actually relax." — Jana P.
   - "They fitted in my son the same morning he broke a tooth." — Tomáš R.
   - "Clear prices and a hygienist who really knows her job." — Markéta V.

8. **Book a visit** (`#booking`), kicker "Book a visit", title "Request an appointment", text "Send us your request and we will call you back within one working day to confirm the time."
   - A form with Name (required), Phone (required), E-mail (required), Treatment (required, a select with: Check-up, Dental hygiene, Filling, Whitening, Children's check-up, Emergency), Preferred date (required, a date field) and Message (optional). Every field has a visible label. The button says "Send request".
   - Sending it opens the visitor's e-mail app with a message addressed to recepce@zelenadental.cz, subject "Appointment request", and all the entered details in the body (no server).

9. **FAQ** (`#faq`), kicker "FAQ", title "Questions we hear often". Clicking a question shows its answer; clicking it again hides it. Five questions:
   - "Do you accept new patients?" · "Yes. We accept new adult patients and children from 3 years. Book a first check-up with the form above."
   - "Which insurance companies do you work with?" · "All Czech public health insurers, including VZP, ČPZP and OZP."
   - "What should I bring to my first visit?" · "Your insurance card, an ID and a list of any medicines you take."
   - "Does dental hygiene hurt?" · "Usually not. If your gums are sensitive, we use a numbing gel at no extra cost."
   - "How do I cancel an appointment?" · "Call us at least 24 hours ahead so that someone else can take your time."

10. **Contact** (`#contact`), kicker "Contact", title "Find us on the main square".
    - Opening hours, one row per day: Monday 8:00–18:00, Tuesday 8:00–18:00, Wednesday 8:00–18:00, Thursday 8:00–18:00, Friday 8:00–14:00, Saturday closed, Sunday closed. Today's row is highlighted, with the same live "Open now"/"Closed now" badge.
    - Address: "Horní náměstí 21, 779 00 Olomouc", with a link "Open in Google Maps" (`https://maps.google.com/?q=Horní+náměstí+21,+Olomouc`).
    - A map: embed `https://www.openstreetmap.org/export/embed.html?bbox=17.2459%2C49.5913%2C17.2559%2C49.5963&layer=mapnik&marker=49.5938%2C17.2509` in a frame (title "Map of Zelená Dental").
    - Phone "+420 585 000 321" (clickable `tel:` link) and e-mail "recepce@zelenadental.cz" (clickable `mailto:` link).

11. **Footer**: "Zelená Dental", the address, the five section links, and "© 2026 Zelená Dental s.r.o.".

Page title: "Zelená Dental – gentle dentistry in Olomouc". Description for search engines: "Family dental practice on Horní náměstí in Olomouc: check-ups, hygiene, fillings, whitening and children's dentistry. New patients welcome."

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
