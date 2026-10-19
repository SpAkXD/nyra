<!-- Metadata: this comment is not part of the card (nyra_spec with topic "web" and the tests strip it).
TOKENS: 1386 on claude-sonnet-5-5, 1105 on claude-haiku-4-5 (estimated: 2,996 characters, scaled from the last measurement of 3,019 characters = 1,397 tokens; no API key at hand when sidebar/media/video/tabs/search were added. Re-measure: python tools/card_tokens.py --card web --write).
BUDGET: hard limit 1,400 tokens on claude-sonnet-5-5. The card has a hard budget: a feature that needs card text must displace something.
The library is src/std/web.nyra; this card must describe it exactly (tests/docs.rs builds its example). Read with docs/AGENT_CARD.md. -->
# Nyra web card
`use web` makes a static site: `nyra build site.nyra -o dist`.

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
site(name, primary: accent: bg: font: display: style: soft|sharp|editorial|dark lang: tz:)
page(path, title, desc:) {}
section(id, title, kicker: text: bg: alt|dark|primary) {}
grid(cols, widths: "1fr 2fr") {}   stack {}   row {}
card(title, text, icon: image: tag: items: fill: href:) {}
h1 h2 h3(text, icon:)   p small note(text)   link(text, href)   img(src, alt, ratio: "4/3" caption: width:)
button(text, href, style: primary|outline|ghost on: active: icon:)
list(items, icon:)   table(rows, head:)   address {}   chip(title, text, icon:)   embed(url, title)
quote(text, name, place: stars:)   stat(value, label)   steps([(label, text)])   marquee(words)
navbar(brand, [(label, href)], cta: (label, href))
hero(title, kicker: text: image: alt: buttons: [(label, href)] badge: rows) {}
gallery([(image, alt, tag)], filters: [(tag, label)])   hours(rows)   rows: [(label, "1-5 8:00-16:00")], days 1-7
map(address)   split { card(...) }   footer(text: links: contact: [str] small: [str]) {}
sidebar([(label, href)]) {}   app nav, ("Group", "") a heading   tabs([label]) {}   search(hint)
media(title, image: href: meta: badge: price:)   video(src, poster: title:)   YouTube/Vimeo play here
form(title, mailto: | on:) {}   field(name, label, type: text|email|tel|date|area|select|check req: hint: options: [str])
css(".x { color: red }")
```

## Motion
- `anim:` fade rise slide zoom pop count stagger; `hover:` lift glow zoom; `loop:` float pulse marquee; `scroll:` parallax; `"rise slow 0.2"`. `keyframes("sway", "50% { rotate: 3deg }")` adds a word.

## Mistakes
- No JSX, tags, `class:`, CSS `style:` or handler fns: `on:` takes a `Msg`.
- E0372: a fn or script var named like a web function (`page map list`).
