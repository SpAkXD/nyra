You are a senior developer writing **Nyra**, a small programming language made for AI agents. Nyra builds websites: one `.nyra` file with `use web` is compiled by `nyra build site.nyra` into static HTML, CSS and a little JavaScript. Build the website described below as **one single file, `site.nyra`**.

You have not seen Nyra before. Everything you need is in the two reference cards below: the language card and the web card. Use only what they describe; Nyra is not Python, TypeScript, Rust or JSX.

## Rules
- One file, `site.nyra`, that starts with `use web`. No HTML, CSS or JavaScript of your own unless the web card's escape hatches are truly needed: the built-in components, theme and motion words already give a polished look.
- The images are the given URLs; do not use other images.
- It must compile: follow the cards exactly (names, argument order, named options).

## The site to build

A one-page website for **Lípa Bakery & Café**, a small family bakery in Valašské Meziříčí (Czech Republic). Use exactly the text, data and image URLs below. The site must look premium and modern (think a well-designed local business site in 2026), work on phones and desktops, and be accessible (alt texts, labels, keyboard focus).

**Look:** warm and calm. Primary color `#8A4B2A` (crust brown), accent `#E0A13A` (honey), background `#FBF7F1` (cream), text `#2B1D14`. Headings in the serif font **Fraunces**, body text in **Inter** (both from Google Fonts). Rounded cards, soft shadows, generous spacing.

**Motion:** the hero text comes in step by step (kicker, title, text, buttons); sections and their cards reveal as you scroll (cards one after another); cards lift slightly on hover; the four numbers count up when they appear; the product strip scrolls endlessly. Respect "reduced motion" settings.

### Sections, in order

1. **Navigation bar**, sticky at the top: brand "Lípa Bakery", links About (`#about`), Menu (`#menu`), Gallery (`#gallery`), Visit us (`#visit`), and a highlighted button "Order a cake" linking to `#visit`. On phones the links fold into a menu button.

2. **Hero**, full-screen photo with a dark gradient so the text is readable.
   - Photo: `https://picsum.photos/seed/lipa-hero/1600/1000` (alt: "Fresh loaves on the bakery counter")
   - Small kicker line: "Valašské Meziříčí · since 1998"
   - Title: "Bread baked before sunrise"
   - Text: "Sourdough loaves, buttery pastry and celebration cakes from a family bakery on the town square."
   - Buttons: "See the menu" (`#menu`, filled) and "Visit us" (`#visit`, outlined)
   - A small live badge "Open now" or "Closed" computed in the visitor's browser from the opening hours below (Czech time is fine to ignore: use the visitor's local time).

3. **Product strip** (endless horizontal scroll) with these words: Sourdough, Rye bread, Croissants, Kolache, Cinnamon rolls, Wedding cakes, Coffee, Seasonal tarts.

4. **About** (`#about`), kicker "Our story", title "Three generations of early mornings".
   - Paragraph: "Grandma Marie opened Lípa in 1998 with one oven and her own rye recipe. Today her grandchildren still start the ovens at 3 a.m., still use her sourdough starter, and still bake everything in the back of the shop."
   - Four numbers that count up: **28** years of baking, **40** kinds of bread and pastry, **1,200** customers a week, **3 a.m.** ovens on (the last one is text, it does not count).
   - A timeline with four steps: 1998 "Marie opens the bakery with one oven"; 2006 "A second oven and the first wedding cakes"; 2015 "The café opens next door"; 2024 "Organic flour from local mills only".

5. **Menu** (`#menu`), kicker "Menu", title "Baked today", subtitle "Prices in CZK. Ask about allergens at the counter."
   - Filter buttons: All, Bread, Pastry, Cakes. Clicking one shows only that category (without reloading the page); the active button is highlighted.
   - Nine products as cards with photo, name, short description and price. The photo URL is `https://picsum.photos/seed/lipa-N/800/600` where N is the product number:
     1. Bread · "Country sourdough" · "Long-fermented, crackling crust, open crumb." · 89 Kč
     2. Bread · "Dark rye with caraway" · "Grandma Marie's recipe, dense and fragrant." · 72 Kč
     3. Bread · "Seeded spelt loaf" · "Sunflower, pumpkin and flax seeds." · 95 Kč
     4. Pastry · "Butter croissant" · "Laminated for three days, baked every hour." · 45 Kč
     5. Pastry · "Plum kolache" · "Wallachian classic with plum jam and crumble." · 32 Kč
     6. Pastry · "Cinnamon roll" · "Soft dough, brown sugar, a little cardamom." · 49 Kč
     7. Cakes · "Honey cake" · "Ten thin layers, sour cream and honey." · 85 Kč
     8. Cakes · "Chocolate tart" · "Dark chocolate ganache on a salted crust." · 79 Kč
     9. Cakes · "Celebration cake" · "Made to order for 8–30 people. Order 3 days ahead." · from 1,200 Kč

6. **Gallery** (`#gallery`), kicker "Gallery", title "From the oven to your table". Six photos in a grid; clicking a photo opens it large (a lightbox with close and next/previous, also usable with the keyboard). Photos: `https://picsum.photos/seed/lipa-g1/1200/900` up to `lipa-g6`. Alt texts: "The bakery at dawn", "Shaping sourdough loaves", "Croissants fresh from the oven", "The café corner", "A wedding cake on the counter", "Flour sacks from the local mill".

7. **Testimonials**, title "What our neighbours say", three quotes:
   - "The rye bread tastes exactly like my grandmother's. I come every Saturday." — Jana K.
   - "They made our wedding cake in three days and it was perfect." — Tomáš and Eva
   - "Best croissant in Wallachia, and the coffee is great too." — Petr S.

8. **Visit us** (`#visit`), kicker "Visit us", title "Come in for a coffee".
   - Opening hours: Monday–Friday 6:00–18:00, Saturday 7:00–12:00, Sunday closed; today's row highlighted, with the same live "Open now"/"Closed" badge.
   - Address: "Náměstí 12, 757 01 Valašské Meziříčí", with a link "Open in Google Maps" (`https://maps.google.com/?q=Náměstí+12,+Valašské+Meziříčí`).
   - Phone "+420 571 000 123" (clickable `tel:` link) and e-mail "hello@lipa-bakery.cz" (clickable `mailto:` link).
   - A contact form with Name (required), E-mail (required), Message (required). Sending it opens the visitor's e-mail app with the message filled in, addressed to hello@lipa-bakery.cz (no server).

9. **Footer**: "Lípa Bakery & Café", the address, the four section links, and "© 2026 Lípa Bakery & Café".

Page title: "Lípa Bakery & Café – bread baked before sunrise, Valašské Meziříčí". Description for search engines: "Family bakery and café on the town square in Valašské Meziříčí since 1998: sourdough bread, pastry and celebration cakes."

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
site(name, primary: accent: bg: font: display: style: soft|sharp|editorial|dark lang: tz:)
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
gallery([(image, alt, tag)], filters: [(tag, label)])   adds an All tab
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
