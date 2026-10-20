<!-- Metadata: this comment is not part of the card (nyra_spec with topic "web" and the tests strip it).
TOKENS: 1394 on claude-sonnet-5-5, 1100 on claude-haiku-4-5 (Anthropic count_tokens; python tools/card_tokens.py --card web --write).
BUDGET: hard limit 1,400 tokens on claude-sonnet-5-5. The card has a hard budget: a feature that needs card text must displace something.
The library is src/std/web.nyra; this card must describe it exactly (tests/docs.rs builds its example). Read with docs/AGENT_CARD.md. -->
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
site(name, primary: accent: bg: font: display: radius: style: "sharp"|"editorial"|"dark" lang: tz:)
page(path, title, desc:) {}   css(".x{color:red}")
section(id, title, kicker: text: bg: "alt"|"dark"|"primary"|"accent") {}
grid(cols, widths: "1fr 2fr") {}   stack {}   row {}
card(title, text, icon: image: tag: items: fill: href: on:) {}
h1 h2 h3(text, icon:)   p small note(text)   link(text, href)   img(src, alt, ratio: "4/3" caption:)
button(text, href, style: "primary"|"accent"|"outline"|"ghost"|"light" on: active: icon:)
list(items, icon:)   table(rows, head:)   address {}   chip(title, text, icon:)   embed(url, title)
quote(text, name, place: stars:)   stat(value, label)   steps([(label, text)])   marquee(words)
navbar(brand, [(label, href)], cta: (label, href))
hero(title, kicker: text: image: alt: buttons: [(label, href)] badge: hoursRows open: closed:) {}
gallery([(image, alt, tag)], filters: [(tag, label)])   accordion([(question, answer)])
hours(hoursRows, open: "Open now" closed:)   hoursRows: [("Mon–Fri", "8:00-16:00"), ("Sun", "")]
map(address)   split { card(...) }   footer(text: links: contact: small:) {}
form(title, mailto: subject: send: | on:) {}   field(name, label, type: "email"|"tel"|"date"|"area"|"select"|"check" req: hint: options:)
```

## Motion
- `anim:` fade rise zoom pop words count stagger; `hover:` lift glow zoom tilt; `loop:` float pulse; `scroll:` parallax; modifiers: `"rise slow 0.2 spring"`. `keyframes(name, css)` adds a word.
- Nothing moves unasked: give sections and the hero `anim: "rise"`.

## Mistakes
- No JSX, tags, `class:`, CSS `style:` or handler fns: `on:` takes a `Msg`.
- E0372: a fn or script variable named like a web function (`page hours map list`).
