You are a senior developer writing **Nyra**, a small programming language made for AI agents. Nyra builds websites: one `.nyra` file with `use web` is compiled by `nyra build site.nyra` into static HTML, CSS and a little JavaScript. Build the website described below as **one single file, `site.nyra`**.

You have not seen Nyra before. Everything you need is in the two reference cards below: the language card and the web card. Use only what they describe; Nyra is not Python, TypeScript, Rust or JSX.

## Rules
- One file, `site.nyra`, that starts with `use web`. No HTML, CSS or JavaScript of your own unless the web card's escape hatches are truly needed: the built-in components, theme and motion words already give a polished look.
- The images are the given URLs; do not use other images.
- It must compile: follow the cards exactly (names, argument order, named options).

## The site to build

A one-page website for **Penzion U Mlýna**, a family-run guesthouse with six rooms in an old mill on the Vltava river in Český Krumlov (Czech Republic). Use exactly the text, data and image URLs below. The site must look premium, warm and welcoming (think a well-designed boutique hotel site in 2026), work on phones and desktops, and be accessible (alt texts, labels, keyboard focus).

**Look:** warm and natural. Background `#FBF8F3` (warm off-white), surfaces `#FFFFFF`, text `#2B2622`, primary color `#7A4E2D` (walnut brown), accent `#5B7F5E` (sage green). Headings in **Fraunces**, body text in **Nunito Sans** (both from Google Fonts). Rounded cards, soft shadows, photos with rounded corners, lots of space.

**Motion:** the hero text comes in step by step (kicker, title, text, buttons); sections and their cards reveal as you scroll (cards one after another); room cards and gallery photos zoom in slightly on hover. Respect "reduced motion" settings: with reduced motion nothing moves.

### Sections, in order

1. **Navigation bar**, sticky at the top: brand "U Mlýna", links Rooms (`#rooms`), Amenities (`#amenities`), Gallery (`#gallery`), Location (`#location`), FAQ (`#faq`), and a highlighted button "Book a stay" linking to `#booking`. On phones the links fold into a menu button.

2. **Hero**, a full-screen photo with a dark gradient so the text is readable.
   - Photo: `https://picsum.photos/seed/mlyn-hero/1600/1000` (alt: "The old mill house of Penzion U Mlýna on the Vltava river")
   - Small kicker line: "Český Krumlov · on the Vltava since 1998"
   - Title: "Sleep by the river, wake up to the castle"
   - Text: "Six quiet rooms in a restored 17th-century mill, five minutes' walk from the old town. Breakfast from our own bakery included."
   - Buttons: "See the rooms" (`#rooms`, filled) and "Book a stay" (`#booking`, outlined)
   - A small live badge that says exactly "Reception open" or "Reception closed", computed in the visitor's browser from the reception hours below (use the visitor's local time).

3. **Rooms** (`#rooms`), kicker "Rooms", title "Find your room", subtitle "Every room has a private bathroom, free Wi-Fi and breakfast included. Prices are per night for the whole room."
   - Filter buttons: All, Double, Family, Suite. "All" is active when the page loads. Clicking a type shows only the rooms of that type (without reloading the page); the active button is highlighted.
   - Six rooms as cards, each with a photo, the name, the type, the size, the number of guests, the price per night and one sentence:
     1. "River Room" · Double · 18 m² · 2 guests · 2,400 Kč per night · "A window seat right above the weir." · photo `https://picsum.photos/seed/mlyn-room-1/800/600` (alt: "The River Room with a window seat")
     2. "Garden Room" · Double · 16 m² · 2 guests · 2,100 Kč per night · "Opens onto the herb garden and is quiet at night." · photo `https://picsum.photos/seed/mlyn-room-2/800/600` (alt: "The Garden Room with a view of the herb garden")
     3. "Miller's Family Room" · Family · 32 m² · 4 guests · 3,600 Kč per night · "A double bed and a bunk bed under old oak beams." · photo `https://picsum.photos/seed/mlyn-room-3/800/600` (alt: "The family room with a bunk bed")
     4. "Attic Family Room" · Family · 36 m² · 5 guests · 3,900 Kč per night · "Two bedrooms under the roof, ideal with older kids." · photo `https://picsum.photos/seed/mlyn-room-4/800/600` (alt: "The attic family room under the roof")
     5. "Castle View Suite" · Suite · 45 m² · 2 guests · 5,200 Kč per night · "A free-standing bath and the best view of the castle tower." · photo `https://picsum.photos/seed/mlyn-room-5/800/600` (alt: "The Castle View Suite with a free-standing bath")
     6. "Mill Wheel Suite" · Suite · 50 m² · 4 guests · 5,900 Kč per night · "A living room with the original mill wheel behind glass." · photo `https://picsum.photos/seed/mlyn-room-6/800/600` (alt: "The Mill Wheel Suite with the old mill wheel")

4. **Amenities** (`#amenities`), kicker "Amenities", title "Everything for a slow weekend". Eight items, each with a small icon and a label: "Breakfast from our bakery", "Free Wi-Fi", "Free private parking", "Riverside garden terrace", "Bike and canoe storage", "Pets welcome (300 Kč per night)", "Sauna in the old cellar", "Early check-in on request".

5. **Gallery** (`#gallery`), kicker "Gallery", title "A look around". Six photos in a grid; clicking a photo opens it large (a lightbox with a close button and next/previous buttons, also usable with the keyboard: Escape closes, arrow keys move). Photos: `https://picsum.photos/seed/mlyn-g1/1200/800` up to `mlyn-g6`. Alt texts: "The mill house at dusk", "Breakfast on the terrace", "The weir below the River Room", "The reading corner by the fireplace", "Canoes on the riverbank", "The castle tower from our garden".

6. **Reviews**, title "What our guests say", three reviews, each with five stars:
   - "The quietest night we have had in years, and the croissants alone are worth the trip." — Anna & Tom, United Kingdom
   - "Our kids loved the bunk bed and the ducks on the river." — The Huber family, Austria
   - "Five minutes from the old town, yet it feels like the countryside." — Petr K., Prague

7. **Book a stay** (`#booking`), kicker "Book a stay", title "Ask for your dates", text "Tell us when you would like to come. We reply within a few hours with availability and the price."
   - A form with Arrival (required, a date field), Departure (required, a date field), Guests (required, a select with: 1, 2, 3, 4, 5), Room type (required, a select with: Double, Family, Suite, Any), Name (required), E-mail (required), Phone (optional) and Message (optional). Every field has a visible label. The button says "Check availability".
   - Sending it opens the visitor's e-mail app with a message addressed to stay@umlyna.cz, subject "Booking request", and all the entered details in the body (no server).

8. **Location** (`#location`), kicker "Location", title "Five minutes from the old town".
   - Address: "Linecká 12, 381 01 Český Krumlov", with a link "Open in Google Maps" (`https://maps.google.com/?q=Linecká+12,+Český+Krumlov`).
   - How far things are: Old town square · 5 min on foot; Castle · 10 min on foot; Train station · free pick-up on request.
   - Phone "+420 380 000 512" (clickable `tel:` link) and e-mail "stay@umlyna.cz" (clickable `mailto:` link).
   - Reception hours: Monday–Friday 7:00–21:00, Saturday–Sunday 8:00–20:00, with the same live "Reception open"/"Reception closed" badge. Below them: "Check-in from 14:00, check-out until 11:00."

9. **FAQ** (`#faq`), kicker "FAQ", title "Good to know". Clicking a question shows its answer; clicking it again hides it. Four questions:
   - "Is breakfast included?" · "Yes. Breakfast is served from 7:30 to 10:00 in the mill room or on the terrace."
   - "Can I arrive late in the evening?" · "Yes. Tell us your arrival time and we will leave your key in a code box at the door."
   - "Do you have parking?" · "We have eight free spaces in our courtyard. There is no need to book one."
   - "Can I cancel my booking?" · "Free cancellation up to 7 days before arrival. After that we charge the first night."

10. **Footer**: "Penzion U Mlýna", the address, the five section links, and "© 2026 Penzion U Mlýna".

Page title: "Penzion U Mlýna – riverside guesthouse in Český Krumlov". Description for search engines: "Family-run guesthouse in a restored mill on the Vltava, five minutes from the old town of Český Krumlov: six rooms, breakfast included, free parking."

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
