# Nyra on the web (design for v0.8)

Status: **being built.** The two language additions (WP1: default values, named arguments, child blocks; E0370 and
E0371) are built and documented in [SPEC.md](SPEC.md). The build, the dev server and the client runtime (WP5) are
built as described in [Build contract](#build-contract), with their checks E0379, E0380, E0382, E0383, E0386 and
E0387. The web library and the kit (WP2, WP3) are built, with E0372-E0374, E0376-E0378, E0381 and E0384: see
[The web library as built](#the-web-library-as-built-wp2-wp3) for how they meet the build and where they differ
from this design. Motion (WP4) is built as described in [Motion build contract](#motion-build-contract), with its
checks E0374 (motion words), E0375 and E0388 (keyframes). Not built yet: E0385, the per-component motion defaults
and the rest of WP6 (the flora port, the web card, the experiment). The owner's decisions on the open questions are
recorded in [Decisions](#decisions).

Today Nyra cannot make a web page: there is no DOM, HTML, CSS or events. This document designs `use web`: a way for an
agent to write a whole website (pages, theme, sections, motion, interactivity) in Nyra, and get a fast static site out of
`nyra build`. The test case is a real site, [flora.ageofwords.site](https://flora.ageofwords.site) (React 19, Vite,
Tailwind, motion, three.js), and the target is an experiment: an AI builds the same mock site once in plain HTML/CSS/JS
and once in Nyra, and Nyra must cost clearly fewer tokens while the result looks as good and loads as fast or faster.

**The short answer, measured** (details in [Token measurement](#token-measurement)): four real flora sections plus
their shared theme cost **3,479 tokens in Nyra against 18,180 in plain HTML/CSS/JS (0.19x)** and 19,626 in the
React/TSX original (0.18x). About three quarters of that saving comes from the built-in component and motion kit
(the CSS and JS an agent no longer writes), and about one quarter from the syntax (no closing tags, no repeated
classes, data reused, links and icons in text). 70% of the remaining Nyra tokens are the site's own Czech text.

## Contents
1. [How a site is built and served](#how-a-site-is-built-and-served)
2. [Two small language additions](#two-small-language-additions)
3. [The `web` API](#the-web-api)
4. [Motion](#motion)
5. [Interactivity](#interactivity)
6. [Where the savings come from](#where-the-savings-come-from)
7. [Escape hatches, accessibility, SEO, safety](#escape-hatches-accessibility-seo-safety)
8. [Error codes](#error-codes)
9. [The web card](#the-web-card)
10. [Token measurement](#token-measurement)
11. [Clone from a URL (later)](#clone-from-a-url-later)
12. [Build contract](#build-contract) (built: the build, the dev server, the client runtime)
13. [The web library as built](#the-web-library-as-built-wp2-wp3)
14. [Motion build contract](#motion-build-contract) (built: the motion words)
15. [Implementation plan](#implementation-plan)
16. [Open questions and risks](#open-questions-and-risks)

## One example

```nyra
use web

site("Pekárna Hana", lang: "cs", primary: "#7A3E1D", font: "Inter", display: "Fraunces")

struct Bread { name: str, kind: str, price: int, photo: str }
let breads = [Bread(name: "Kváskový chléb", kind: "chléb", price: 89, photo: "kvas.jpg"),
    Bread(name: "Croissant", kind: "sladké", price: 35, photo: "croissant.jpg"),
    Bread(name: "Bageta", kind: "chléb", price: 29, photo: "bageta.jpg")]

enum Msg { Show(str) }
var kind = "vše"
fn update(m: Msg) {
    match m {
        Msg.Show(k) => kind = k
    }
}

fn menu() -> Node = stack {                  // reads `kind`, so it is re-rendered in the browser after each update
    row {
        for k in ["vše", "chléb", "sladké"] { button(k, on: Msg.Show(k), active: k == kind) }
    }
    grid(3, anim: "stagger", hover: "lift") {
        for b in breads.filter(b => kind == "vše" || b.kind == kind) { card(b.name, "{b.price} Kč", image: b.photo) }
    }
}

page("/", "Pekárna Hana – chléb každé ráno") {
    navbar("Pekárna Hana", [("Nabídka", "#nabidka"), ("Kontakt", "#kontakt")])
    hero("Chléb každé ráno", image: "pult.jpg", alt: "Pult s chlebem", buttons: [("Nabídka", "#nabidka")])
    section("nabidka", "Nabídka", kicker: "Dnes upečeno") {
        menu()
    }
    section("kontakt", "Kontakt", bg: "alt") {
        map("Hlavní 1, 602 00 Brno", show: "Zobrazit mapu")
    }
}
```

`nyra build bakery.nyra` writes `dist/index.html` (the whole page prerendered: it shows without JavaScript), one CSS
file with the theme and only the components used, and a small JS file for the filter, the navbar menu and the
animations. The page has a sticky navbar with a mobile menu, a full-screen hero with a parallax photo and an entrance
sequence, staggered cards that lift on hover, a filter whose buttons re-render the grid, a map that loads on click,
and it respects `prefers-reduced-motion`. No styling code was written.

## How a site is built and served

```
nyra build site.nyra          # a program with `use web` -> dist/
nyra run site.nyra            # build, serve it on http://127.0.0.1:8000, rebuild and reload on save
nyra check site.nyra          # types plus web checks (broken #anchors, missing images, contrast), writes nothing
```

`nyra build` sees `use web` and builds a site instead of an executable; no new flag (`-o DIR` changes `dist/`).
What is built so far is described exactly in [Build contract](#build-contract).

**At build time** the whole program runs once in the IR interpreter (the same one `--interp` uses: deterministic, with
limits). Each `page(path, title) { ... }` call renders its node tree and hands the HTML to the build. The build
writes:

| File | What is in it |
|---|---|
| `dist/<path>/index.html` | One per `page`: the prerendered HTML, `<head>` with title, description, Open Graph, canonical, preload of the first image, the font links |
| `dist/site.<hash>.css` | The theme as CSS variables, then the CSS of only the components and motion words the pages used, then any raw `css(...)` |
| `dist/site.<hash>.js` | Only when needed: the kit behaviors the pages used (menu, lightbox, reveal, ...), and the program's own client code (below). Absent for a page with nothing interactive |
| `dist/img/<name>-<width>-<hash>.webp` | Image variants for `srcset`, made by the compiler's own codec (see [Images](#images)) |
| `dist/sitemap.xml`, `dist/robots.txt` | From `site(url: ...)` and the pages |

**In the browser** nothing is rendered on load: the HTML is already there. The JS file only attaches behavior. If the
program has interactive state (an `enum Msg` and `fn update`, see [Interactivity](#interactivity)), the JS backend
compiles `update`, the view functions it affects and what they call into the same JS file, so a click re-renders only
those regions. There is no framework runtime and no hydration of the whole page.

Targets for the flora port (checked by a test on the built `dist/`): first paint with no JS, CSS under 15 KB and JS
under 8 KB gzipped for a subsite, Lighthouse performance 95+ on a mid-range phone profile. The React original ships
React, motion and the Tailwind output; it loads a JS bundle before anything shows (its `index.html` has an empty
`#root`).

**Images.** The compiler decodes and encodes images itself (zero crates, `src/web/img/`), so every photo gets
`width`/`height`, smaller WebP variants with `srcset`, and a tiny blurred preview while it loads, with no tool to
install. See [Images](#images) for what it reads and how the variants are chosen. Images are read only from the
folder `assets/` next to the program; the build needs no `--allow fs`.

**Fonts** come from Google Fonts with `display=swap`, loaded without blocking the first paint (as flora does), or
from `assets/fonts/` when the files are there.

## Two small language additions

Nyra has no closures, no generics, no classes, and lambdas only as array-method arguments. The design keeps all of
that. It needs two additions, both familiar from languages models know well, both lowered away before the IR so no
backend changes:

**1. Default parameter values and named arguments.** `print(x, end: "")` already works this way; this makes it
general.

```nyra
fn price(n: int, currency: str = "Kč", space: bool = true) -> str = ...
price(5)                        // positional arguments first,
price(5, space: false)          // then named ones, in any order
```

A parameter with `= value` may be left out; the value is a constant (a literal, `[]`, `[:]`, `none`, or a struct of
constants), filled in at the call. Any argument may be named. Why: a component has many options, nearly all with a
good default. Without this every call would list every option, in order: `hero(...)` alone has eleven. Structs with
labels cannot help, because every field must be given. Cost: E0226 ("named argument to a function") keeps its number
with a narrower meaning: the function has no parameter of that name. One card line.

**2. Child blocks.** A call whose last parameter is `kids: [Node]` may be followed by a block instead of that
argument:

```nyra
section("sluzby", "Naše služby") {
    p("Úvod")                       // each statement that makes a Node adds it, in order
    "Text také jde"                 // a string on its own line is a text node
    for s in services { card(s.title, s.text) }
    if open { note("Dnes otevřeno") }
}
```

The block runs immediately, in place, like the body of an `if`: it is not a closure and captures nothing. It is
lowered to `var kids: [Node] = []`, one `kids.push(...)` per node statement, and the call with `kids`. Allowed in the
block: node expressions, strings, `let`, `for`, `if`, `match`. Not allowed: assigning or changing variables (E0371);
the block describes a tree, it does not compute. Why: pages are trees, often 6 to 10 levels deep. As nested array
literals (`section(..., [p("Úvod"), grid(3, [card(...), ...])])`) a missing comma or bracket 40 lines down is a
common, hard to place error, and `for`/`if` would need comprehensions everywhere. Blocks are what models write in
SwiftUI, Jetpack Compose, Kotlin builders and Flutter, and braces keep Nyra's rule that a truncated reply is a compile
error.

Both are general features (a user's own component can have defaults and take a child block: `fn panel(title: str,
kids: [Node]) -> Node = card(title, "", kids: kids)`, called as `panel("Hodiny") { ... }`).

**Names.** In a program with `use web`, the web functions are called by their bare names (`hero(...)`, not
`web.hero(...)`), like the builtins. Every line of a page is a web call; we measured the prefix at +2.6% tokens
(3,571 vs 3,479), and the bare form is what models write in every UI framework. A program function or variable with
the name of a web function is E0372 with a hint to rename it. This is a choice for the owner (see open questions): the
alternative keeps the module rule and pays the 2.6%.

## The `web` API

Three levels, from most used to least:

1. **Kit components**: a whole section of a modern site in one call (`hero`, `navbar`, `gallery`, `contact` parts).
   This is where most savings come from.
2. **Layout and content elements**: `section`, `grid`, `stack`, `row`, `card`, `h1`-`h3`, `p`, `img`, `link`,
   `button`, `list`, `table`. For anything the kit does not have.
3. **Escape hatches**: `css(...)`, `html(...)`, `cls:`. For the last 5%.

### Site and pages

| Call | Does |
|---|---|
| `site(name, url:, lang:, tz:, desc:, share:, primary:, accent:, bg:, alt:, ink:, muted:, palette:, font:, display:, radius:, style:, motion:, transition:, say:)` | Once, at the top. Identity and SEO defaults (`desc` for the description, `share` for Open Graph), the theme, motion level, the kit's UI words |
| `page(path, title, desc:, image:, primary:) { ... }` | One HTML file. `primary:` overrides the theme color for this page (flora green vs krby red) |

**Theme, not class soup.** A theme is a handful of values; every component derives the rest (tints, hover shades,
rings, gradients, shadows, dark text on light colors and the reverse) with `color-mix()` from them:

| Option | Meaning | Default |
|---|---|---|
| `primary`, `accent` | Brand color, second color for buttons and highlights | blue-violet, amber |
| `bg`, `alt`, `ink`, `muted` | Page background, alternate section background, text, secondary text | derived from `bg` |
| `palette: ["krby": "#8B2500"]` | Extra named colors, usable anywhere a color is (`color: "krby"`) | none |
| `font`, `display` | Body and heading fonts (Google Fonts names) | Inter, same |
| `radius` | Corner radius in px for cards (buttons are pills, inputs follow) | 16 |
| `style` | A preset of the above: `"soft"` (flora), `"sharp"`, `"editorial"`, `"dark"` | `"soft"` |

A color is a hex string, a palette name, or a theme name (`"primary"`, `"accent"`, `"alt"`). Sections choose a
background with `bg: "alt" | "dark" | "primary" | "white"`; text color follows automatically.

**Kit words** (`say:`). The kit's own labels ("open now", "show map", "close menu", aria labels) are English by
default. `say: ["open": "Nyní otevřeno", ...]` replaces them; the measured samples write them out (they are content).
Built-in word lists for a few languages (`lang: "cs"` picks Czech) are an open question.

### Text

Every text argument is plain text with a small inline Markdown subset and icons:

- `**bold**`, `*italic*`, `` `code` ``, `[label](url)`; `\\*` for a literal star.
- `:name:` is an icon from the built-in set (a subset of Lucide, plus brand icons): `":phone: {s.phone}"`. A name
  starts with a letter and has no spaces, so `8:00 – 16:00` stays text; an unknown name is E0374.
- Interpolation works as everywhere: `"[{c.email}](mailto:{c.email})"`.

Text is always escaped: `"<b>"` shows `<b>`. Markdown is the only markup, and its links are checked (Safety).
`tel:` links drop spaces, external links get `rel="noopener"` and open in a new tab.

### Layout and content elements

| Element | Does | Main options |
|---|---|---|
| `section(id, title) { }` | Full-width band, centered container, vertical rhythm, and a section heading when `title` is given | `kicker:`, `text:` (subtitle), `bg:`, `tone:` (heading accent), `align:` |
| `grid(cols) { }` | Responsive grid: `cols` on desktop, 2 on tablets, 1 on phones | `widths: "1fr 1.15fr"`, `gap:`, `numbers: true` (01, 02, ... on cards) |
| `stack { }`, `row { }` | Vertical / horizontal (wrapping) flow with the theme's gap | `gap:`, `align:` |
| `card(title, text) { }` | Surface with radius and shadow; an `icon:` card is a feature card, an `image:` card a product card | `icon:`, `image:`, `fill: true` (primary background), `href:` |
| `h1`, `h2`, `h3`, `p`, `small` | Text | `icon:`, `sub:` (h3), `size: "lg"` |
| `img(src, alt)` | Responsive image; `alt` is required, `""` marks a decorative one | `ratio: "16/10"`, `caption:`, `fit:`, `width: "160px"` (a small picture: an avatar, a logo) |
| `link(text, href)`, `button(text, href)` | A link; a button-styled link or an action | `style: "primary" | "outline" | "ghost"`, `on:` (action) |
| `list(items)`, `table(rows)`, `address { }`, `note(text)` | Lists, tables, contact block, a small line with a divider | |
| `chip(title, text)` | Pill with an icon, a title and a line | `icon:` |
| `embed(url, title)` | An iframe (video, 3D model viewer), lazy | `ratio:` |

Every element also takes `id:`, `cls:` and the four motion options `anim:`, `hover:`, `loop:`, `scroll:` (Motion).

### Kit components

Each one is the look and behavior of a well-made modern site with no styling code. Their structure is fixed
(a contract with the CSS), their content comes from arguments.

| Component | What it makes | Behavior included |
|---|---|---|
| `navbar(brand, links)` | Sticky, translucent header; links, `back:`, `other:` (switch to another subsite), `cta:` button | Blur and shadow after scrolling, scroll-spy pill on the active link, mobile menu with `aria-expanded`, reading progress bar |
| `hero(title)` | Full-height photo hero: `kicker:`, `text:`, `image:`, `alt:`, `buttons:`, `badge:` | Gradient from the primary color, photo zoom-in and parallax, word reveal of the title, staggered entrance, scroll cue to the next section |
| `split(panels)` | Full-screen selection between two (or more) subsites | Hover zoom, labels, page transition into the chosen site |
| `marquee(items)` | Endless ticker strip in the primary color | CSS-only loop, pauses on hover, static for reduced motion |
| `gallery(photos)` | Masonry gallery: `filters:` | Filter tabs with a sliding pill, lightbox with keys, swipe, counter, focus trap |
| `hours(rows)` | Opening-hours card: rows like `("Sobota", "6 8:30-11:30")` | Live "open now" badge in `site(tz:)`, closed days marked, refreshed each minute |
| `map(address)` | Map card: `show:`, `go:` | Placeholder until clicked (no Google request before consent), then the embed; a directions link |
| `form(title) { field(...) }` | Form with labeled fields: `mailto:` or `to:` (POST URL), `send:`, `note:`, `done:`, `done_text:`, `edit:` | Validation, focus rings in the theme, a mailto composer (message, then a line, then the other fields), a "sent" state with an edit button |
| `steps(items)` | Vertical timeline of (label, text) | Drawn line, staggered items |
| `stat(value, label)` | Big number with a label | Counts up when seen |
| `footer(...)` | Columns of brand text, links, contacts, small print | Back-to-top |
| `sidebar(links) { ... }` | An app layout: a column of links (`(label, href)`; an empty href is a group heading, or a note when long) beside the content of its block | Sticky on desktops, a drawer behind a menu button on phones (a checkbox: no JavaScript needed), closes on a link or Escape |
| `media(title, image:, href:, meta:, badge:, price:)` | A thumbnail card for grids of videos and products: the picture with a corner badge (a video's length), the title, a price and a meta line (`"Channel · 1.2M views · 3 days ago"`); `ratio:`, `tag:` | A YouTube or Vimeo `href:` plays in place when the picture is clicked (youtube-nocookie.com, no request before the click) |
| `video(src, poster:, title:)` | An mp4 or webm file in `<video controls>`, or a YouTube or Vimeo address as a poster with a play button; `ratio:` | The player loads only when clicked; the page's policy allows its frame and media origins |
| `tabs([label]) { ... }` | One panel per child | Without JavaScript the labels are links to the panels, which all show; with it a tab list with arrow keys |
| `search(hint)` | A search box | Filters the cards (or list items, rows) of the element right after it as one types, accents ignored; hidden without JavaScript |

Helpers: `maps(address)` gives a Google Maps search URL, `year()` the current year (at build time).

`hero(..., badge: open_hours)` shows the same live open/closed badge as `hours(...)`.

## Motion

Motion is part of the kit, not an extra library: every component already moves the way flora does, and every element
can say how it moves in one or two words. The rules:

- **CSS first.** Entrances, hovers and loops are CSS transitions and `@keyframes` on `transform`, `opacity`,
  `filter` and `clip-path` only (the properties the compositor animates without layout). Scroll-linked effects use
  CSS scroll-driven animations (`animation-timeline: view()`) where the browser has them.
- **Tiny JS only for triggers.** One `IntersectionObserver` adds a class when an element scrolls into view, and one
  `requestAnimationFrame` loop (asleep when nothing on screen needs it, stopped in hidden tabs) runs counters, tilt,
  and parallax where scroll timelines are missing. About 1.5 KB gzipped, included only if a page uses those words.
- **Reduced motion by default.** Under `prefers-reduced-motion: reduce`, entrances become a short fade or nothing,
  loops and the marquee stop, parallax and tilt are off, counters show the final number. Nothing to write.
  `site(motion: "calm")` halves distances and durations for everyone; `"off"` removes motion.

### One word per element

Four options, on every element, inherited by children when set on a container (`grid(3, hover: "lift")` lifts each
card):

| Option | When | Words |
|---|---|---|
| `anim:` | Once, when the element scrolls into view (or on load above the fold) | `fade` `rise` `slide` `slide-left` `zoom` `blur` `pop` `words` `letters` `draw` `count` `stagger` `none` |
| `hover:` | Pointer over / keyboard focus | `lift` `glow` `zoom` `shine` `tilt` `underline` `fill` `magnetic` |
| `loop:` | Always, while on screen | `bob` `float` `pulse` `ping` `spin` `shimmer` `gradient` `marquee` |
| `scroll:` | Linked to the scroll position | `parallax` `fade-out` `scale` `progress` |

A word can carry modifiers in the same string: a speed (`fast`, `slow`), a delay in seconds, an easing preset:
`anim: "rise slow 0.2"`, `hover: "lift spring"`, `scroll: "parallax 0.3"`. Unknown words are compile errors with the
nearest word (E0374).

What the words do: `rise` fades in from 28 px below (the default for sections and cards), `words` reveals a heading
word by word from clipping boxes (the default for `h1`/`h2` and section titles), `stagger` starts each child 80 ms
after the previous one (the default for grids), `draw` grows a line or SVG stroke, `count` counts a number at the
start of the text up from 0 ("33+" counts 0...33, then "+"), `pop` scales in on a spring. `shine` sweeps a light
sheen across a button (as on nyralang.dev), `tilt` turns a card toward the pointer in 3D, `gradient` slowly moves a
background gradient (an animated `@property` angle), `progress` is a reading-progress bar.

**Defaults** are chosen per component so that most pages need no motion words at all: sections `rise`, headings
`words`, grids `stagger`, cards `hover: "lift"`, buttons `hover: "lift"` with an active press, images in cards
`hover: "zoom"`, the hero its own sequence (photo zoom, then kicker, title, text, buttons, scroll cue),
`marquee` its loop, `stat` `count`, `steps` `draw`. `anim: "none"` turns one off. (Not built yet: today an element
moves only with the words written on it; see [The web library as built](#the-web-library-as-built-wp2-wp3).)

### Easing and springs

Presets: `out` (an expo-out, `cubic-bezier(.16, 1, .3, 1)`, the default), `in-out` (`.76, 0, .24, 1`, for panels),
`spring`, `bounce`, `snappy`, `linear`. A spring is computed at build time: the compiler simulates a damped spring
and writes it as a CSS `linear(...)` easing with enough points, so springs need no JS. `"spring 260 16"` sets
stiffness and damping. Browsers without `linear()` get a close cubic-bezier.

### Sequences

`at:` puts an element on a timeline, in seconds after its parent's trigger (the page load for the hero, the scroll
into view for a section). The hero's sequence, written out, would be:

```nyra
stack(anim: "fade") {
    p(kicker, anim: "rise", at: 0.5)
    h1(title, anim: "words", at: 0.55)
    p(text, anim: "rise", at: 0.85)
    row(anim: "rise", at: 1.0) { ... }
}
```

### Custom keyframes

For anything the words do not cover, `keyframes(name, body)` defines a new word with a CSS keyframes body (the syntax
models already know), and the word works in `anim:` or `loop:`:

```nyra
keyframes("sway", "0%, 100% { rotate: -3deg } 50% { rotate: 3deg }")
img("leaf.png", "", loop: "sway 4s in-out")
```

The body is parsed at build time. Only compositor-friendly properties are accepted silently (`transform` and its
parts `translate`/`rotate`/`scale`, `opacity`, `filter`, `clip-path`, `background-position`); a property that causes
layout (`width`, `top`, `margin`, ...) is the warning E0375, like Nyra's performance warnings.

### Page transitions

`site(transition: "fade" | "slide" | "rise" | "none")` turns on cross-document View Transitions
(`@view-transition { navigation: auto }`) between the site's pages: no JS, and browsers without support simply
navigate. `morph: "flora"` on an element in one page and on one in the next makes the browser animate one into the
other: flora's selection panel growing into the subsite hero is `split` panels and `hero`s with the same `morph`
names.

### What flora and nyralang.dev use, and the word for it

| Effect | flora / nyralang.dev | Nyra |
|---|---|---|
| Scroll reveal, staggered grids | `Reveal`, `Stagger` (motion library) | default; `anim: "rise"`, `"stagger"` |
| Word-by-word heading | `TextReveal` | default for headings; `anim: "words"` |
| Card lift, icon fill, accent bar | Tailwind `group-hover` classes | `card` default; `hover: "lift"` |
| Endless strip | `Marquee` + `@keyframes` | `marquee(items)` |
| Hero parallax and fade-out | `useScroll` + `useTransform` | `hero` default; `scroll: "parallax"`, `"fade-out"` |
| Counters | `CountUp` | `stat` default; `anim: "count"` |
| Timeline line | `scaleY` on view | `steps` default; `anim: "draw"` |
| Button sheen, link underline | nyralang.dev CSS | `hover: "shine"`, `"underline"` |
| Spring nav pill | `layoutId` (flora), CSS spring (nyralang.dev) | `navbar` default; `"spring"` easing |
| Live "open" dot | `animate-ping` | `hours` default; `loop: "ping"` |
| Page change animation | React state machine with timers | `site(transition:)`, `morph:` |
| Reading progress | `useSpring` on scroll | `navbar` default; `scroll: "progress"` |

## Interactivity

Most interactivity on business sites is in the kit (menus, filters, lightbox, forms, maps, badges) and needs no code.
For the rest, a program uses the language it already has: **state in script `var`s, events as values of an enum
named `Msg`, one `fn update(m: Msg)`, and view functions that read the state.** It is the Elm architecture, which
fits a language without closures: an event handler is data, not a function.

```nyra
enum Msg { Add(int), Clear }
var cart: [int] = []
fn update(m: Msg) {
    match m {
        Msg.Add(id) => cart.push(id)
        Msg.Clear => cart = []
    }
}
fn basket() -> Node = row {
    p("V košíku: {cart.len()}")
    button("Vysypat", on: Msg.Clear)
}
```

- `on:` takes a `Msg` value. The value is written into the HTML as its JSON (`data-on='{"Add":[3]}'`, the JSON form
  every Nyra type already has); one delegated listener reads it, calls `update`, and re-renders.
- **Live regions are found by the compiler**, not declared: a call in a page to a function with no parameters that
  returns a `Node` and reads a script `var` that `update` can change is prerendered and wrapped as a live region. After
  each `update`, the browser calls that function again (compiled to JS) and patches the region's DOM (a small keyed
  morph; focus and scroll position are kept). The rest of the page is never touched.
- `form(on: Msg.Send)` with a variant that carries `[str: str]` passes the form's fields (the one place where a
  variant is written without its value). `field(..., on: Msg.Typed)` with a `str` variant fires on input.
- In `update`, browser effects are calls: `go(url)`, `scroll_to(id)`, `after(ms, Msg.Tick)`, `copy(text)`.
  `time.now_ms()` works as it does everywhere.
- A program with `on:` but no `enum Msg` or no `fn update(m: Msg)` is E0373. A live function with parameters is
  E0376 (pass state through script variables instead).

Client code is ordinary Nyra compiled by the JS backend, so it has Nyra's checks and its runtime errors (an index out
of bounds stops the update with E0240 in the console instead of corrupting the page).

## Where the savings come from

Honestly, from two places, and the larger one is not the syntax:

| Source | Share of the saving vs plain HTML/CSS/JS (measured on the samples) |
|---|---|
| **The kit**: CSS and JS the agent does not write (theme, layout, components, motion, behaviors) | about 74% (10,876 of the 14,701 tokens saved) |
| **The syntax and data**: no closing tags, no class lists, one `struct` for two subsites, Markdown links, icons by name, `head` meta from `site` | about 26% (3,825 tokens) |

So if an HTML author had an equally complete ready-made kit (component CSS plus behaviors, used by writing markup with
its class names), Nyra would still be about **0.48x** of that markup (3,479 vs 7,259 tokens), not 0.19x. Tailwind is
not that kit: the same services section with the Tailwind CDN costs 3,716 tokens, more than plain CSS (2,107),
because every card repeats its class list.

What Nyra adds that a CSS kit alone cannot: defaults that compose (a `card` inside a `grid` in a `section` already has
its spacing, reveal, stagger and hover), typed options checked at compile time (a wrong word, a broken `#anchor`, a
missing image or alt text is an error before anything is deployed), one place for data (the two contact sections are
one function and two values), and the interactivity model without a framework.

**Good defaults are the product.** If the kit's output does not look as good as flora with zero styling code, agents
will reach for `css(...)` and the saving disappears. The kit's design quality is therefore its own work package with
the flora site as the bar.

## Escape hatches, accessibility, SEO, safety

**Escape hatches.**
- `css("...")` at the top level adds raw CSS to the site's stylesheet; it may use the theme variables (`var(--primary)`).
- `cls: "name"` adds a class to any element, for that CSS.
- `html("...")` inserts raw HTML. It takes only a string literal (no interpolation, no variables): data never
  becomes markup (E0378).
- `embed(url, title)` for iframes (a video, a 3D model viewer: flora's three.js showroom stays out of scope for v0.8
  and can be embedded).

**Accessibility**, by construction:
- Semantic HTML: `header`, `nav`, `main`, `section` with `aria-labelledby` its heading, `footer`, `address`, lists as
  lists, the lightbox as a `dialog` with a focus trap.
- `img` needs `alt` (a parameter, so a missing one is a type error); `""` marks decorative images explicitly.
- Icon-only buttons need `label:` (E0377). One `h1` per page and no skipped heading levels (warning E0379).
- Visible focus rings in the theme color, a skip link, `aria-expanded` and `aria-current` in the navbar, labels tied
  to fields, required fields marked for screen readers, `lang` from `site`.
- The theme's text and background colors are checked for WCAG AA contrast at build time (warning E0380 with the ratio).
- Reduced motion is respected by default (Motion).

**SEO**: every page has a title (a required parameter) and `lang`; description and Open Graph tags come from `site`
and can be overridden per page; `og:image` defaults to the first hero image; canonical URLs and `sitemap.xml` from
`site(url:)`; the content is in the HTML, not rendered by JS. With `hours` and an address on a page, the build can add
schema.org `LocalBusiness` data (open question).

**Safety.**
- No HTML injection: all text is escaped; the only markup is the Markdown subset, which makes a fixed set of tags.
- URLs are checked: `http`, `https`, `mailto`, `tel`, relative paths and `#anchors` only; `javascript:` and `data:`
  links are E0381 (at compile time for literals, as a build error for computed text).
- A `#anchor` that no element on the page has is E0382, an image missing from `assets/` is E0383.
- The build writes a Content-Security-Policy `<meta>` derived from what the page uses: scripts only from the site,
  frames only from Google Maps when `map` is used, no inline scripts. The policy is computed, not written by hand.
- Capabilities stay as they are: client code (`update` and live views) cannot use `fs`, `os` or `input` (E0384).
  Future network access from the browser (`web.fetch`) will need `--allow net` at build time and a `connect:` list
  that also becomes the CSP `connect-src`; local storage (`web.save`) a new capability `store`.

## Error codes

Reserved: **E0370-E0389** (free in `docs/ERRORS.md`). Also E0226 changes meaning slightly (above).

| Code | Kind | Meaning |
|---|---|---|
| E0370 | compile (built) | A value without a name after a named one, a parameter given twice, a default value that is not a constant, or a default on an `inout` parameter |
| E0371 | compile (built) | A misused child block: after a call whose last parameter is not an array (or is given already), a line that makes no value, or an assignment, `var`, `while`, `return`, `break`, `continue` or `arena` in it |
| E0372 | compile (built) | A program function or variable has the name of a web function (`fn hero`, `let hours`) |
| E0373 | compile (built) | `on:` is used but the program has no `enum Msg` or no `fn update(m: Msg)` |
| E0374 | compile / build (built) | Unknown option word (`hover: "lfit"`), with the nearest word; also an unknown icon name |
| E0375 | warning (built) | A custom keyframe animates a layout property (`width`, `top`, ...) |
| E0376 | compile (built) | A live view function has parameters |
| E0377 | compile (built) | An icon-only button or link has no `label:` |
| E0378 | compile (built) | `html(...)` or `css(...)` with a value that is not a string literal |
| E0379 | warning (built) | Not exactly one `h1` on a page, or a skipped heading level |
| E0380 | warning (built) | Theme colors below WCAG AA contrast (the ratio and the pair are in the message) |
| E0381 | compile / build (built) | A link with a forbidden scheme (`javascript:`, `data:`) |
| E0382 | build (built) | A `#anchor` link to an id that is not on the page |
| E0383 | build (built) | An image that is not in `assets/`, or that cannot be read (a warning in the dev server and MCP) |
| E0384 | compile (built) | Client code (`update`, a live view, what they call) uses `fs`, `os` or `input` |
| E0385 | compile | A node made but not used (a node expression outside a child block, not returned or passed) |
| E0386 | warning (built) | Images copied without `srcset` variants: the built-in codec cannot read the file, or `--image-tool none` |
| E0387 | build (built) | Two pages with the same path, or a path that is not `/...` |
| E0388 | compile / build (built) | A `keyframes(name, body)` that cannot be a word: a bad or taken name, a body that does not parse, an unknown property, a value that is not plain CSS |
| E0389 | | free |

Each gets a full entry (wrong and fixed program) in `docs/ERRORS.md` when it is built, as the error database requires.

## The web card

The agent card has a hard budget of 1,400 tokens and must not grow for web work most programs never do. Plan:

- **Main card**: one line for the two language additions (default parameters, named arguments, child blocks), about
  35 tokens. Something must be shortened to make room; the card's "Names" list is the candidate.
- **Web card**, `docs/WEB_CARD.md`, a second card with the same hard budget as the main card, **1,400 tokens**, served only for web tasks:
  `nyra_spec` (MCP) with `topic: "web"`, `llms.txt` links it, and the experiment's prompt includes it. It holds one
  complete page example, the element and component lists with their main options, the motion words, the `Msg`/`update`
  pattern and the five most likely mistakes. The draft below measures **1,336 tokens** on claude-sonnet-5-5 (1,049 on claude-haiku-4-5).

A web request therefore carries about 2,700 tokens of cards as input (cacheable), against the thousands of output
tokens it saves per section.

The card as built is [`WEB_CARD.md`](WEB_CARD.md) (1,397 tokens; `tests/docs.rs` builds its example and checks every
function and option it names against the library). The draft below, written before the library, is kept as history:
the library differs from it (see [The web library as built](#the-web-library-as-built-wp2-wp3)).

<details>
<summary>Draft of the web card</summary>

````markdown
# Nyra web card (read with the main card)
`use web` makes a site: `nyra build site.nyra` writes `dist/` (HTML per page, one CSS, a small JS).

```nyra
use web
site("Flora", lang: "cs", primary: "#2D5A1B", font: "Inter", display: "Playfair Display")
struct Item { name: str, kind: str, photo: str }
let items = [Item(name: "Seeds", kind: "seed", photo: "seeds.jpg")]
enum Msg { Show(str) }
var kind = "all"
fn update(m: Msg) {
    match m {
        Msg.Show(k) => kind = k
    }
}
fn shop() -> Node = stack {                // reads `kind`: re-rendered after each update
    row {
        for k in ["all", "seed"] { button(k, on: Msg.Show(k), active: k == kind) }
    }
    grid(3, hover: "lift") {
        for p in items.filter(p => kind == "all" || p.kind == kind) { card(p.name, "", image: p.photo) }
    }
}
page("/", "Flora – garden shop") {
    navbar("Flora", [("Shop", "#shop")])
    hero("Flora", text: "Seeds and tools.", image: "hero.jpg", alt: "Shop", buttons: [("Shop", "#shop")])
    section("shop", "Shop", kicker: "Since 1991", bg: "alt") {
        shop()
    }
}
```

## Rules
- Positional arguments, then `name: value` ones in any order; options have defaults.
- A call whose last parameter is `kids: [Node]` takes a `{ }` block: one node or string per line, plus `for`, `if`, `let`; no assignments.
- Text is escaped; inline `**b**`, `[label](url)`, icons `:phone:`. Colors: hex, `primary accent alt`, or `palette` names.
- `img(src, alt)`: alt required (`""` = decorative); files in `assets/`.
- State: script `var`s. Events: `on: Msg.X(..)` and `fn update(m: Msg)`. A no-parameter fn returning `Node` that reads the state re-renders itself.
- Raw: `css("...")`, `html("...")` (literals only), `cls:`.

## Elements
- `site(name, theme...)` `page(path, title) {}`
- `section(id, title, kicker:, text:, bg:) {}` `grid(cols, widths:) {}` `stack {}` `row {}` `card(title, text, icon:, image:, fill:) {}`
- `h1 h2 h3 p small` `img(src, alt, ratio:, caption:)` `link(text, href)` `button(text, href, on:)` `list` `table` `address {}` `note` `chip(title, text, icon:)` `embed(url, title)`
- kit: `navbar(brand, links, back:, cta:)` `hero(title, kicker:, text:, image:, alt:, buttons:, badge:)` `split` `marquee(items)` `gallery(photos, filters:)` `hours(rows)` `map(address)` `form(title, mailto:) { field(name, label, type:, req:, hint:) }` `steps` `stat(value, label)` `footer`

## Motion (any element; a container passes it to its children)
- `anim:` fade rise slide zoom blur pop words draw count stagger none · `hover:` lift glow zoom shine tilt underline
- `loop:` bob float pulse ping spin gradient · `scroll:` parallax fade-out progress
- `"rise slow 0.2 spring"` adds speed, delay, easing; `at: 0.5` places it in the parent's sequence; `keyframes("sway", "50% { rotate: 3deg }")` adds a word.
- Sections, headings, grids, cards and the hero already move; reduced motion is respected.

No class lists, inline styles, JSX or handler functions: a handler is a `Msg` value.
````

</details>

## Token measurement

**What was measured.** Four parts of the real flora site, each written three ways with the same text and the same
features:

| Part | React original (counted as is) | Same look and behavior |
|---|---|---|
| 0. Shared | the theme, base CSS, `index.html`, and the helpers the parts use (`motion.tsx`, `Button`, `SectionHeading`, `OpenBadge`, `SmartImage`, `business.ts`, `scroll.tsx`, `easing.ts`) | theme and colors, fonts, base styles, buttons, section heading, scroll reveal, stagger, word reveal, the live open/closed badge |
| 1. Flora page | `FloraSubsite.tsx`, `SubsiteHero.tsx`, `Marquee.tsx` | page head (title, description, Open Graph, preload, fonts), hero (photo zoom, parallax, entrance sequence, word reveal, badge, two buttons, scroll cue), marquee; the other sections referenced |
| 2. Services | `Services.tsx`, `data/services.ts` | heading, six numbered cards with icons (hover lift, number tint, icon fill, accent bar), stagger, partner chip |
| 3. Contact | `Contact.tsx` (both subsites) | heading, colored info card, opening hours with live badge, click-to-load map, mailto form with validation and a "sent" state; for both subsites |
| 4. About (the motion sample) | `About.tsx` | heading, lead, drawn timeline with staggered milestones, four counters, revealed photo with caption |

The samples are in [`docs/web-samples/`](web-samples): `*.html` (plain HTML/CSS/JS as an AI would write it without a
framework: semantic HTML, one stylesheet, vanilla JS, Lucide icons from a CDN), `*.nyra` (the design above), and
`2-services.tailwind.html` (a sensitivity check). Counts are from Claude's tokenizer (the `count_tokens` endpoint,
`claude-sonnet-5-5`, the per-message overhead subtracted). Each sample's first line, a note for the reader, is not
counted.

**Results (tokens):**

| Part | React/TSX | HTML + CSS + JS | Nyra | Nyra / HTML | Nyra / TSX |
|---|---:|---:|---:|---:|---:|
| 0. Shared (theme, base, helpers) | 8,033 | 3,183 | 509 | 0.16 | 0.06 |
| 1. Flora page: head, hero, marquee | 2,929 | 3,606 | 466 | 0.13 | 0.16 |
| 2. Services | 1,950 | 2,107 | 703 | 0.33 | 0.36 |
| 3. Contact (both subsites) | 4,884 | 6,728 | 1,164 | 0.17 | 0.24 |
| 4. About (motion) | 1,830 | 2,556 | 637 | 0.25 | 0.35 |
| **Parts 1-3 (the brief)** | 9,763 | 12,441 | 2,333 | **0.19** | 0.24 |
| **Parts 1-4** | 11,593 | 14,997 | 2,970 | **0.20** | 0.26 |
| **All, with shared** | 19,626 | 18,180 | 3,479 | **0.19** | **0.18** |

**Checks on the result:**

| Question | Measurement |
|---|---|
| How much of the Nyra is the site's own text? | 2,448 of 3,479 tokens (70%) are inside string literals. The code around the text is about 1,000 tokens for four sections and the theme. Czech text is token-heavy; an English site would show a larger code share |
| Is the contact ratio inflated because HTML repeats the section for the second subsite? | Partly. HTML with only the Flora contact section is 4,979 tokens; Nyra's 1,164 (both subsites) is still 0.23x of that |
| Would Tailwind have been cheaper than plain CSS? | No: the services section with Tailwind classes is 3,716 tokens (plain: 2,107; Nyra: 703) |
| How much is the kit, how much the syntax? | HTML = 10,876 CSS+JS + 706 `<head>` + 6,553 markup. Nyra vs markup and head only: 3,479 vs 7,259, **0.48x** (see Where the savings come from) |
| What does the `web.` prefix cost? | +92 tokens (+2.6%) over the whole sample |
| Is the TSX column fair? | It is the original as it is in the repository, so it is understated in one way (the Tailwind theme and the `motion`, React and lucide libraries are free to it) and overstated in another (TypeScript types, props plumbing, the subsite switching state machine) |

**What is not equal.** The Nyra version writes the kit's Czech UI words once in `site(say: ...)` (counted); the "years
of experience" counter is computed at build time, the React one in the browser (a yearly rebuild or a `since:` option
fixes that); the Nyra navbar call in part 1 is a real call, while the HTML and TSX parts reference the navbar
without its code (this favors HTML and TSX). The blurred image previews (LQIP) of the original are produced by its
build script in both React and Nyra, and left out of the hand-written HTML.

**Cost.** For the experiment the measure is billed tokens, not file size: the agent's output (the program) plus its
input (the prompt, which for Nyra carries the two cards, about 2,700 tokens, cacheable). On these samples the output
difference is about 14,700 tokens for four sections, far larger than the cards. The experiment must still report
both, and first-try correctness, as `docs/DESIGN.md` requires (pass@1 first, tokens second).

## Clone from a URL (later)

The longer-term goal: give an agent only the URL of an existing site and get it rebuilt in Nyra, cheaper and faster
than rewriting HTML/CSS/JS. The `web` API is shaped for this: what an agent sees on a page (a navbar, a hero, a row of
cards, a contact block, the brand colors and fonts) maps one to one onto kit components and `site(...)` options.

A tool, `nyra web clone URL` (also as the MCP tool `nyra_clone`), does the mechanical part and leaves the judgment to
the agent:

1. **Capture**: load the page in a headless browser (the agent's browser tool or a local Chrome), scroll it once so
   lazy content loads, and take a full-page screenshot per breakpoint (mobile, desktop).
2. **Extract** (a script run in the page) into one JSON outline:
   - *theme*: the most used background, text and button colors from computed styles, the font families and their
     Google Fonts names, the median card radius, light or dark;
   - *structure*: landmarks (header, nav, main, footer), then sections split at headings, each with a guess of its
     kit component from patterns (a first section with a large background image and an `h1` is a `hero`; three or
     more siblings with the same shape are a `grid` of `card`s; an animated overflowing row is a `marquee`; a table of
     days and times is `hours`; a Maps iframe is `map`; a form is a `form` with its fields);
   - *content*: the visible text in order with its headings, links (internal ones become `#anchors` or pages), and
     images (the largest `srcset` candidate and its `alt`);
   - *motion*: which elements animate on scroll or hover (classes and computed `transition`/`animation`), mapped to
     the nearest motion words.
3. **Skeleton**: `nyra web skeleton outline.json` writes `site.nyra` using kit components, with the text filled in,
   an `assets/` download list, and `// guess:` comments where the match is weak. Parts that match nothing become a
   `section` of plain elements; nothing is pasted as raw HTML by default.
4. **Refine**: the agent builds, the tool renders `dist/` at the same breakpoints and shows a per-section visual diff
   against the original screenshots; the agent fixes the sections that differ most, a few rounds.

The agent's work is then mostly steps 3 and 4: reviewing a skeleton of a few hundred tokens instead of writing
thousands of tokens of HTML and CSS. Only sites the user owns or has the right to copy may be cloned; the tool says so
and does not fetch through logins or paywalls. Not part of v0.8: it needs the kit first, and its quality depends on
how many real sites the component guesses cover.

## Build contract

What WP5 built (`src/web.rs`, `src/web/*.rs`, `src/web/runtime.js`), and the exact contract between the build
and the web library (WP2), the kit files (WP3) and the motion files (WP4).

### Commands

| Command | Does |
|---|---|
| `nyra build site.nyra` | Runs the program, writes the site to `dist/` next to the program. `-o DIR` writes elsewhere. `--json` prints `{ok, out, pages, files: [{path, bytes}], warnings}` |
| `nyra run site.nyra` | The dev server: builds in memory, serves on `http://127.0.0.1:8000` (`--port N`; `--port 0` lets the system choose, the port is printed), rebuilds when the program, a file it imports or a file below `assets/` changes, and reloads the open pages. A failed build shows its errors in the browser and the terminal |
| `nyra check site.nyra` | Compiles and runs the build in memory for its checks; writes nothing |
| `--image-tool T` | `builtin` (the default: the compiler's own codec), `cwebp`, `magick`, `vips` (or the path of one), or `none` (copy the originals) |
| MCP `nyra_build` | For a program with `use web` (or `target: "web"`): builds in memory and returns `{ok, target: "web", pages, files, warnings}`; a missing image is a warning there, and no image is converted |

A `use web` program gets no capability (`fs`, `os`, `input`, `net`) unless `--allow` names it, and only the build
writes files, and only into the output folder. The output folder must be empty, missing, or written by an earlier
build: the build keeps a list of its files in `DIR/.nyra-build` and removes the ones a new build no longer makes;
files it did not write are never touched.

### Intrinsics

The web library hands everything to the build through intrinsics of the `web` module. They are private: the
library's Nyra source calls them by their short names (`_emit(...)`), and so may a program. In the browser they do
nothing.

| Intrinsic | Meaning |
|---|---|
| `_emit(path: str, html: str)` | One page. `path` is the page's path (`/`, `/about`; E0387 for a bad or repeated one), `html` the whole document (a fragment without `<head>` is wrapped in a minimal document) |
| `_uses(chunk: str)` | The page uses the kit chunk `chunk` (a component or behavior). Call it while rendering the component; repeated calls are fine |
| `_css(text: str)` | CSS for the site's stylesheet, after the kit and motion parts: the theme variables and the program's raw `css(...)` |
| `_keyframes(name: str, body: str)` | A custom motion word: the build checks it (E0388, E0375), writes `@keyframes name { body }` into the stylesheet, and `name` counts as a word of `anim:` and `loop:` |
| `_state(name: str, json: str)` | Written by the build, not the library (see Client code) |

### What the build reads from the pages

The build reads the finished HTML of each page, so the library is free in how it renders:

- **Motion**: the values of `data-anim`, `data-hover`, `data-loop`, `data-scroll`, `data-at`, `data-motion`,
  `data-morph` and `data-transition`, as (attribute, value) pairs, plus the `_keyframes` calls of the run; each value
  is checked (E0374) and picks its parts of the motion CSS and JS (see [Motion build contract](#motion-build-contract)).
- **Events and live regions**: `data-on="<the Msg as JSON>"` on an element (the JSON of `json.str` of the `Msg`
  value), and `data-live="<name of the view function>"` on the element that wraps a live view's HTML. A form or field
  whose `data-on` is only a variant name (`"Send"`, a JSON string) sends that variant with the form's fields (a
  `[str: str]`) or the field's value.
- **Images**: the attributes `src`, `poster`, `data-src`, `data-full`, `data-bg`, `data-image`, the `href` of `a`
  and `link`, and the `content` of `<meta property="og:image">` / `<meta name="twitter:image">`, when the value is
  a relative path with an image extension. The file is looked up in `assets/` next to the program, then next to the
  program (E0383 if missing). CSS `url(...)` of a file in `assets/` is copied too.
- **Checks**: `id` attributes and `#anchor` links (E0382), `h1`-`h6` (E0379), the CSS variables `--ink`, `--muted`,
  `--bg`, `--alt` in `_css` text (E0380, hex colors only).
- **Sitemap**: `<link rel="canonical" href="https://...">`. The site's address is the canonical URL of `/` (else
  the origin of any canonical URL); without one there is no `sitemap.xml`. A page with
  `<meta name="robots" content="noindex">` and the page `/404` are left out.

### What the build adds to each page

- Right after `<meta charset>` (or `<head>`): `<meta http-equiv="Content-Security-Policy">` computed from the page:
  `default-src 'self'`, `script-src 'self'` (or `'none'` without JS), `style-src 'self'` plus `'unsafe-inline'` only
  when the page has a `<style>` or a `style` attribute that cannot be a CSS rule (each `style` attribute becomes a
  class `s-<hash>` of the site's CSS, its declarations `!important`; the kit, the motion script and the live
  regions set styles only through the CSSOM), Google Fonts when linked, `img-src 'self' data:` plus external
  image origins, `frame-src` for iframes, `data-src`/`data-embed`/`data-frame` URLs and Google Maps when the `map`
  chunk is used, `form-action 'self'` plus form targets, `base-uri 'self'`, `object-src 'none'`. No inline scripts.
- Before `</head>`: `<link rel="stylesheet" href="site.<hash>.css">` and `<script src="site.<hash>.js" defer>`
  (relative, so the site also works from a sub-folder), and with `nyra run` the reload script.
- On each `<img>`: `width` and `height` from the file's header when neither is set, `loading="lazy"` (unless
  `loading` is set or `fetchpriority="high"`), `decoding="async"`, and with the variants `srcset` and
  `sizes="100vw"` (unless set).

### Files of the site

| File | Content |
|---|---|
| `<path>/index.html` | Each page: `/` is `index.html`, `/about` and `/about/` are `about/index.html`, `/404` is `404.html`, a path ending in `.html` is that file |
| `site.<hash>.css` | The kit's base CSS (before its first marker), the used kit chunks, the used motion chunks, the keyframes, then the `_css` texts; comments and spaces removed |
| `site.<hash>.js` | Only when needed: the used kit JS chunks (in their own function scope), the used motion parts (one function scope of their own), then the client code |
| `img/<name>-<hash>.<ext>`, `img/<name>-<width>-<hash>.webp`, `img/<name>-og-<hash>.jpg` | The images, their WebP variants (see [Images](#images)) and a JPEG copy for `og:image` when the original does not suit |
| `files/<name>-<hash>.<ext>` | Other files that the CSS refers to (fonts) |
| `sitemap.xml`, `robots.txt` | See above; `robots.txt` names the sitemap |
| `_headers` | The pages' policies joined, plus `frame-ancestors 'self'`, `X-Content-Type-Options` and `Referrer-Policy`, in the format of Netlify and Cloudflare Pages (the dev server sends the same header) |

### Images

The build has its own image codec (`src/web/img/`, zero crates). It reads:

- **JPEG**: baseline and progressive, any chroma subsampling, restart markers, grayscale, Adobe RGB, CMYK and YCCK,
  the EXIF orientation (applied, also to `width`/`height`) and the ICC profile. A photo much larger than needed is
  decoded at 1/2, 1/4 or 1/8 of its size inside the DCT, so a 50-megapixel file never exists at full size in memory.
- **PNG**: every color type and bit depth (16-bit too), palette transparency, interlaced files.
- **GIF** (the first frame), **BMP**, and **WebP** (lossy, lossless, with alpha).
- SVG, AVIF, ICO and animations (a GIF with several frames, an animated WebP) are copied as they are.

For each photo it writes, next to the original:

- **Variants** 480, 800, 1200, 1600 and 2400 px wide that are narrower than the photo (one within 15% of the widest is
  left out), plus one at the photo's own width up to 2560 px. Never larger than the source. A variant that is not
  smaller in bytes than the next wider one is left out, and when the full-size WebP is not smaller than the original
  file, the original is the widest `srcset` entry instead.
- **Resizing**: Lanczos3 in linear light with premultiplied alpha (colors do not darken where they mix, transparent
  pixels do not bleed); very large reductions first halve the image with a box filter.
- **Lossy WebP** at quality 80 (libwebp's scale): modes chosen by rate and distortion, four segments with finer
  quantizers for smooth areas, token probabilities fitted to the image, the loop filter level searched per segment,
  and an exact (lossless) alpha plane. **Graphics** (at most 256 colors, or mostly flat) and small images are also
  encoded as **lossless WebP**, which is used when it is at most 25% larger (no blur on text and edges).
- **A preview**: a WebP of at most 24 px (about 150 bytes), shown behind each lazy-loaded image as a blurred
  `data:` background until the image arrives. The page gets one inline `<style>` with the previews, allowed by its
  SHA-256 hash in the page's `style-src` (or by `'unsafe-inline'` when the page needs that anyway). The first image
  of a page (`fetchpriority="high"`, preloaded) and images with transparency get none.
- **Replacements**: a BMP, or a photo wider than 2560 px, is not copied at all (a 20 MB camera file stays out of
  `dist/`): its widest WebP takes its place. An `og:image` or `twitter:image` that is not a JPEG or PNG, is wider than
  2400 px or larger than 1.5 MB gets a JPEG copy 1200 px wide (quality 85, optimized Huffman tables); an image used only
  there gets nothing else.

The encoded files are cached in the temp folder under the hash of the source (`nyra/web-img`), so a rebuild takes
milliseconds; images are processed on up to 4 threads and the variants of one image in parallel. A file the codec
cannot read (damaged, or a format it does not know) is copied as it is and named in warning E0386.
`--image-tool cwebp`, `magick` or `vips` uses that tool instead (the old way); `--image-tool none` copies the
originals (also E0386).

Quality, measured: file size at equal SSIM against libwebp (Pillow 12.1, libwebp 1.6, `method=4`, its default), on
the flora photos at 1280 and 800 px, qualities 50 to 95:

| Photos | Our size / libwebp's at equal SSIM |
|---|---|
| 11 at 1280 px | 1.01 (0.95 to 1.03) |
| the same at 800 px | 1.01 (0.96 to 1.04) |

Lossless WebP (graphics, screenshots, logos): 0.98 of libwebp's `method=4` size over 25 images. The JPEG decoder
gives the same pixels as libjpeg-turbo; the JPEG encoder is about 5% smaller than libjpeg-turbo with optimized
tables at equal SSIM. Speed on one core: about 0.65 s to encode a 1280 x 914 photo (libwebp `method=4`: 0.58 s). A
50-megapixel, 21 MB JPEG gives its five variants in about 5 s with at most 125 MB of memory (it is decoded at half
size); a 48-megapixel PNG in about 7 s and 470 MB.

### Chunk markers

`src/web/kit.css` and `src/web/kit.js` are split by `/*! chunk: NAME */` lines. A chunk runs to the next marker;
the text before the first marker is the file's base. The kit's base CSS is always included; any other base only when one of its file's
chunks is used. The kit files are embedded into the compiler by `build.rs` when they exist (an absent file is
empty), so the build works at every stage of WP3. The motion files have their own parts and rules; the build calls
`web::motion` for them (see [Motion build contract](#motion-build-contract)).

### Client code

A program with an `enum Msg`, a `fn update(m: Msg)` and live views gets client code in `site.<hash>.js`, made only
when a page has a `data-on`:

- The build lowers the program again without the optimizer (which drops functions the script never calls) and
  compiles, with the JavaScript backend, `update`, each view named in a `data-live`, the function that renders a view's
  value to HTML, and everything they call. A view that returns `str` needs no renderer; for another type the build
  uses the first of `web._render`, `web.render`, `_render`, `render` that takes that type and returns `str`.
- Only the runtime functions the code reaches are included (`client::shake`), then `src/web/runtime.js` (about 1.6 KB
  gzipped): one delegated `click`, `submit` and `input` listener reads `data-on`, calls `update`, then re-renders every
  `data-live` region with a keyed morph (`data-key` or `id`; unkeyed nodes match in order; nodes in place are not
  moved, a focused field keeps what is typed). A runtime error stops that update, is logged, and leaves the state as
  it was.
- The state: the script variables that `update` and the possible live views use (any function of the program
  without parameters that returns a value; the pages say which are live) are recorded at the end of the build's run:
  the build compiles the program once more with `web._state("name", json.str(name))` appended to the script, and puts
  the values into the JS file. Every page starts from the values the variables had when the script ended.
- The test site in `tests/web_build/` (with the stub library `minimal_web.nyra`, made the `web` module by
  `NYRA_WEB_LIB=path`) is built and its client code run in Node.js by `tests/web_build.rs`; outside a page the runtime
  hands out `globalThis.nyra_web = {web, morph, msg}` for such tests. Sizes on that site: the JS file is about 4.5 KB
  gzipped.

Not built yet: the browser effects of `update` (`go`, `scroll_to`, `after`, `copy`), and a
headless-browser click-through test (no browser on the build machine; the Node.js test covers update, views, morph
and messages).

## The web library as built (WP2, WP3)

The library is `src/std/web.nyra` (elements, kit components, text, `<head>`), `src/std/web_icons.nyra` (generated
by `tools/gen_web_icons.py`: 216 Lucide icons, ISC license, plus the Facebook icon from Simple Icons, CC0) and
`src/webcheck.rs` (linking, bare names, compile-time checks, live regions). The kit is `src/web/kit.css` and
`src/web/kit.js`. `examples/web/` has a program per area (elements, kit components, interactivity, escaping) with
its images in `examples/web/assets/`; `tests/web.rs` builds each one and compares the site with
`tests/web/fixtures/<name>/`, which are also the fixture pages to open in a browser.

**How the library meets the build** (the intrinsics above):

- A component returns a `Node`, `struct Node { html: str, kit: [str] }`: its HTML, already rendered and escaped,
  and the kit chunks it needs. `page(path, title) { ... }` puts the document together (head, sprite, skip link,
  the navbar before `<main>`, the footer after it), calls `_uses(chunk)` for the chunks of its children and
  `_emit(path, html)` once. The chunk names are the markers of `kit.css` and `kit.js`: `table chip embed quote nav
  hero hours map form steps stat foot marquee gallery split tabs app media video search` (CSS) and `nav hours map form
  gallery tabs app video search` (JS); the text
  before the first marker of `kit.css` is the base (theme defaults, type, layout, cards, buttons, lists).
- `site(...)` gives the theme to `_css` as one `:root{...}` rule with the variables `--primary`, `--accent`, `--bg`,
  `--alt`, `--ink`, `--muted` (what the contrast check reads), `--c-NAME` for each palette color, `--on-primary`,
  `--on-accent`, `--font`, `--display` and `--radius`. `css("...")` is `_css`, `keyframes(name, body)` is
  `_keyframes`.
- A build error found while rendering, which the compiler cannot see in a literal (a computed `javascript:` link,
  an unknown icon or color in computed text), is given to `_css` as a comment `/*! nyra:error E0381 message */`;
  the build reports it as that error and writes nothing.
- The site lives in the script variable `_web` (a `WebConfig`), which the compiler declares before the program's
  first statement and `site(...)` fills; the web functions read it (for the kit's words, the time zone, the
  palette). A program cannot name it.
- A view's `Node` is rendered for the browser by `web._render(view: Node) -> str`.
- Local images are written as the file's name (`src="counter.jpg"`), for the build to find in `assets/`; the
  hero's photo is `<img fetchpriority="high">` and also the page's preload link and default `og:image`.
- Icons are `<svg class="i"><use href="#i-NAME"/></svg>`; each page starts with a hidden sprite of the symbols it
  uses. So an icon used often is drawn once, and the code compiled for the browser needs the icons' names, not
  their paths. (A live region that shows an icon nothing else on the page uses shows nothing in its place.)

Where the build differs from the design above, and why:

- **E0372 is for functions, types and script variables** (a `let` or `var` at the top level). Variables inside
  functions, loop variables and parameters may have a web function's name (`for p in items` is in the web card's
  own example); a call is always a function, so there is no ambiguity.
- **`gallery(photos)`** takes `(image, alt, tag)` triples (`(image, alt)` pairs too: their tag is `""`) and
  `filters: [(tag, label)]`; the first tab ("all")
  is added, its label from `all:` or the kit words. A tag that is a `palette` name colors its label.
- **`split { ... }`** takes its panels as a child block of `card(title, text, image:, href:, color:)`.
- **`footer(brand:, text:, links:, links_title:, contact:, contact_title:, small:, top:)`**: `contact` and `small`
  are lines of text; the default small print is `© <year> <site name>`.
- **`hours(rows)`**: days are numbers, 1 (Monday) to 7 (Sunday): `"1-5 8:00-16:00"`, `"6 8:30-11:30"`,
  `"1,3 9:00-12:00 13:00-17:00"`; `""` is closed. The open/closed badge is hidden until the script sets it, so a
  page without JavaScript never shows a wrong state.
- **A form's `on: Msg.Send`** (a variant that carries the fields) is written `data-on="&quot;Send&quot;"`, as
  the build expects.
- **Added:** `quote(text, name, place:, tag:, stars:, color:)` (testimonials), `chip`'s `color:`, `card`'s `tag:`,
  `items:` (a two-column list of dots, as flora's product cards) and `ratio:`, `button`'s `icon:` and `label:`,
  `list(icon:, ordered:, cols:)`, `section(width: "narrow")`, `hero(next:, height: "half")`, `site(logo:)`,
  `navbar(logo:)`, and the helpers `maps(address)` and `year()`.
- **The kit's words** (`say:` keys, English and Czech built in): `open closed shut hours more down map show go
  menu close nav skip back all filter zoom prev next dismiss send mail_note done done_text edit top links contact side
  play search none`.
- **Motion**: every element and component takes `anim:`, `hover:`, `loop:`, `scroll:`, `at:` and `morph:`, written
  as the attributes of the [Motion build contract](#motion-build-contract); `site(motion:, transition:)` are
  `data-motion` and `data-transition` on `<html>`. Literal values are checked when the program compiles
  (`webcheck::motion`, which runs `web::motion::lint` on the source: E0374, E0388, E0375 at the value), computed
  ones by the build. Containers: `grid`, `stack`, `row` and `split` write their `hover:` on each child (a child's
  own `hover:` wins), the rest of their options on themselves (`anim: "stagger"` then moves the children).
  `hero(anim: "rise")` is the hero's sequence: its kicker and badge, text and buttons each get the word, 0.15 s
  apart from `at:` (default 0.1). The title and the photo are on screen from the first frame (they are the largest
  things there, so the page's Largest Contentful Paint does not wait for an entrance); the photo settles from a 4%
  zoom. There are no per-component defaults yet: an element moves only with the
  words written on it. Without motion words the kit has only the movement that is part of a component's look:
  hover colors and shadows, the marquee loop, the badge ping, the scroll cue, the mobile menu and lightbox
  openings; `prefers-reduced-motion` stops them.
- **Kit CSS for motion**: boxes that can hold a `scroll:` element clip with `overflow: clip`, not `hidden`; a
  `steps` line follows `--draw` (`scale: 1 var(--draw, 1)`); `hover: "lift"` takes the kit's tinted card shadow
  through `--m-lift-shadow`; the theme defines `--on-primary` for `fill`.
- **`css("...")` is Nyra text**: `{x}` inserts a value and `{{`, `}}` give one brace, but a block written as CSS
  (`css("a { color: red }")`: a space after `{`, then `property: value`) is read as written (see
  [Keyframes in strings](#keyframes-in-strings)).
- **A string on its own line of a block of nodes is a paragraph** (`<p class="t">`), made by `web._text`.
- **Style presets** `soft` (default), `sharp`, `editorial` (Fraunces headings unless `display:` is given) and
  `dark` set defaults on `<html data-style>`; the theme's own values win over them.
- **The JavaScript of a live site** includes the library functions its views call (escaping, Markdown, the
  components): about 35 KB before gzip on `examples/web/live.nyra`.
- One sample was changed: in `3-contact.nyra` the fields of `struct Shop` are declared in the order the two
  values give them (`pay` before `foot`), which Nyra requires (E0225).

## Motion build contract

What WP4 built: `src/web/motion.css`, `src/web/motion.js`, `src/web/motion.rs` (the words, the parts, the checks),
`src/web/ease.rs` (springs) and `src/web/gzip.rs` (sizes). The fixtures are in `tests/web/motion/`.

### Attributes

The renderer writes each motion option as an attribute, with the value as written; CSS and JS key off these only.

| Attribute | From | Where |
|---|---|---|
| `data-anim`, `data-hover`, `data-loop`, `data-scroll` | `anim:`, `hover:`, `loop:`, `scroll:` | the element; a container's value is written on the container. `stagger` and `draw` act on the container's children; hover and loop words act on the element that has them, so the kit writes the `hover:` of `grid`, `stack`, `row` and `split` on each child |
| `data-at` | `at: 0.5` | the element; it starts that many seconds after the nearest ancestor with `data-anim` is revealed (or after the page load when there is none) |
| `data-motion` | `site(motion:)`: `full`, `calm`, `off` | `<html>` |
| `data-transition` | `site(transition:)`: `fade`, `slide`, `rise`, `none` | `<html>` |
| `data-morph` | `morph: "flora"` | the element; it becomes `view-transition-name: m-flora` |

A value is words, a speed (`fast`, `slow`), an easing (`out`, `in-out`, `spring`, `bounce`, `snappy`, `linear`, or
`spring K D` for stiffness and damping), one number (a delay in seconds for `anim` and `loop`, a strength for `hover`,
a rate for `scroll`) and one duration (`4s`, `600ms`). Several words may share a value (`hover: "lift shine"`,
`anim: "stagger zoom"`).

### Rust API (`crate::web::motion`)

| Function | Does |
|---|---|
| `motion_css(used: &[(&str, &str)], custom: &[Keyframes]) -> String` | The motion CSS for a site: the base, the parts of the words used, the springs they need, the `keyframes` words, and one rule per value with numbers (`[data-anim="rise slow 0.2"]{--ma-d:.2s}`). `used` is (attribute without `data-`, value) pairs, each once. Empty when nothing moves |
| `motion_js(used: &[(&str, &str)]) -> String` | The motion script, already in its own function scope and minified. Empty when no word needs one: only `anim` words, `at`, `tilt`, `magnetic`, `marquee` and the scroll fallback do |
| `parse(opt, value, custom) -> Result<Spec, WordError>` | Reads one value; the error has the message and the nearest word (E0374) |
| `check_keyframes(name, body) -> Result<Keyframes, KeyframesError>` | Checks a `keyframes` call (E0388); `Keyframes::layout_warning()` gives E0375 |
| `lint(tokens)`, `lint_source(src)` | The same checks on string literals in a program, before it runs, with positions; the compiler runs it on every `use web` program (`webcheck::motion`) |
| `scan_html(html, &mut pairs)` | The motion attributes of some HTML, for tools and tests (the build uses `assets::motion_attrs` on its parsed tags) |
| `ease::Spring::new(k, d).curve()` | A spring as `linear(...)`, its settle time and its closest `cubic-bezier(...)` |
| `gzip::gzip_size(bytes)` | The size after `gzip -9`, within about 1% on text; for the size budgets |

The compiler checks the literal values first: `anim: "rsie"` is E0374 and a bad `keyframes("rise", ...)` E0388 at
the value, a layout property in a literal keyframes body warning E0375 there. The build (`src/web.rs`) then collects
the pairs from the final HTML of every page plus the `_keyframes(name, body)` calls of the run, checks the keyframes
(E0388 stops the build, E0375 warns unless the compiler did already) and every value (E0374, for computed ones),
and appends `motion_css` to the kit's CSS and `motion_js` to the JS file.

### The parts

`motion.css` and `motion.js` are split by `/*! motion: <part> */` lines: `base`, `mods` (speeds and easings),
`reveal` (the entrance machinery), `ease spring|bounce|snappy` (a spring's `linear()` easing, written by `ease.rs`; a
test keeps the file equal to what it computes), `anim <word>`, `hover <word>`, `loop <word>`, `scroll <word>`,
`transition` and `transition <name>`; in the JS `core`, `words`, `count`, `draw`, `tilt`, `magnetic`, `marquee`,
`scroll` and `end`. Both files work as they are (the fixtures link them directly).

How it moves:
- Entrances are CSS animations on `opacity`, `translate`, `scale`, `rotate`, `filter`, with `animation-fill-mode:
  backwards`, so after the entrance the element's own styles (a hover `transform`) apply again. Loops use the
  individual transform properties too, hover words use `transform`, scroll words use `transform` on a CSS scroll
  timeline. One `animation` list per element holds the three (entrance, loop, scroll), so they never replace each
  other.
- An element waits hidden only where scripts run (`@media (scripting: enabled)`); if the script never starts,
  everything appears after 3 s. With `prefers-reduced-motion: reduce` or `data-motion="off"` nothing waits and nothing
  moves (the fixtures check that no animation or transition is longer than 10 ms, even under the pointer); counters
  show their number. `data-motion="calm"` halves distances and durations.
- Kit CSS can follow an entrance: `draw` animates `--draw` from 0 to 1 (a timeline line uses `scale: 1 var(--draw)`),
  and `lift` takes its shadow from `--m-lift-shadow`. `glow`, `fill` and `gradient` use `--primary`, `--accent` and
  `--on-primary` from the theme.
- `shine` uses the element's `::after`, `shimmer` its `::before`, `ping` its `::after`; `tilt` adds a glare element and
  `marquee` wraps the children in groups (copies are `aria-hidden` and `inert`).
- A box with `overflow: hidden` around a `scroll:` element would become the scroller its CSS timeline follows (and it
  never scrolls), so the parallax would stand still. The kit uses `overflow: clip` (it looks the same);
  `motion.js` also turns `hidden` into `clip` on the ancestors of `scroll:` elements, and the fixtures check that every
  scroll timeline follows the page.

### Budgets (measured)

| What | Size |
|---|---|
| Motion JS of a typical page (`rise`, `words`, `stagger`, `count`, `at`) | 3,026 bytes, **1,485 gzipped** (budget 1,536) |
| All of the motion JS | 2,832 gzipped |
| Motion CSS of the typical page | about 1.4 KB gzipped |
| All of the motion CSS | about 3.3 KB gzipped |

### Keyframes in strings

A body like `"0%, 100% { rotate: -3deg } 50% { rotate: 3deg }"` used to be read as `{value}` insertions. Since v0.8 the
lexer treats `{ ... }` in a string as text when a space follows the `{` and the inside is `property: value` pairs
(with a space after each colon), so CSS in `keyframes(...)` and `css(...)` reads as written. `{x}`, `{x:>8}` and
`{ f(a: 1) }` are still insertions.

## Implementation plan

Six work packages. WP1 is small and goes first (the others write against its syntax but can start with positional
calls); WP2 to WP5 then run in parallel; WP6 closes.

**WP1: Language (defaults, named arguments, child blocks).**
- Default parameter values, named arguments in any order, E0226 narrowed, E0370; child blocks lowered to
  `var kids` + `push` (E0371, E0385); the bare web names (E0372); `fmt` and `--fix` support (a missing comma between
  named arguments, a positional after a named one).
- Files: `src/ast.rs`, `src/parser.rs`, `src/check.rs` (+ `src/check/*`), `src/ir/lower.rs`, `src/fmt.rs`,
  `src/fix.rs`, `src/hints.rs`, `docs/SPEC.md`, `docs/AGENT_CARD.md`, `docs/ERRORS.md`.
- Tests: new examples run on every backend (defaults and blocks are lowered before the IR, so parity is free);
  wrong/fixed programs for each new code; card token test.

**WP2: `web` core (nodes, elements, text, rendering).**
- `Node` type, every element and kit component as a function with defaults, the text pipeline (escaping, the
  Markdown subset, `:icon:` shortcodes, URL checks), rendering to HTML with the `<head>` (SEO, Open Graph, canonical,
  preload), the page registry (an intrinsic `_emit(path, html)`), the icon set (Lucide subset, ISC license; brand
  icons from Simple Icons, CC0), the live-region detection and `Msg` checks (E0373, E0376, E0384).
- Written mostly in Nyra (`src/std/web.nyra`, like `src/std/math.nyra`), so it runs the same on every backend and is
  the code that is compiled to JS for re-rendering.
- Files: `src/std/web.nyra`, `src/stdlib.rs`, `src/web/icons.rs`, `src/check.rs` (hooks).
- Tests: golden HTML for each element and component (`tests/web.rs`, `examples/web/*.nyra` → expected HTML), escaping
  and injection tests (text, attributes, Markdown links, `javascript:`), E0374/E0377/E0378/E0381.

**WP3: Kit look and behaviors.**
- `src/web/kit.css`: theme variables and derivation (`color-mix`), layout, every component in the flora quality bar,
  responsive breakpoints, focus styles, the `style` presets; `src/web/kit.js` per behavior (menu, scroll-spy, lightbox,
  gallery filter, hours badge, map placeholder, mailto form), each a separate chunk the build includes only when used.
- The HTML structure of each component is the contract with WP2 (a fixture file per component).
- Tests: a static fixture page per component (`tests/web/fixtures/`), screenshots in a headless browser at 390 and
  1440 px compared with references, axe-core accessibility run, CSS and JS size budgets.

**WP4: Motion.**
- `src/web/motion.css` (the words, generated per used word), `src/web/motion.js` (one IntersectionObserver, one rAF
  loop: counters, tilt, parallax fallback), `src/web/ease.rs` (spring simulation to CSS `linear()`, easing presets),
  the keyframes parser and checker (E0375), `at:` sequences, reduced-motion and `motion:` levels, View Transitions and
  `morph:`.
- Tests: unit tests for the spring curve and the keyframes parser; fixture pages for each word; a reduced-motion run
  that asserts no animation longer than 10 ms; the JS budget (1.5 KB gzipped).

**WP5: Build, serve, client code.**
- `src/web.rs`: `nyra build` for `use web` programs (run in the interpreter, collect pages, assemble and hash CSS/JS
  from the used parts, `sitemap.xml`, `robots.txt`, the CSP), images (sizes, WebP variants and previews from
  the built-in codec, or `cwebp`/`magick`/`vips`, E0383, E0386, E0387), anchor checks (E0382), contrast and heading
  checks (E0379, E0380); `nyra run` dev server with live reload (`std::net`, zero crates); client code: the JS backend
  compiles `update`, live views and what they call plus `src/web/runtime.js` (event delegation, keyed morph);
  `nyra_build` in MCP.
- Files: `src/web.rs`, `src/main.rs`, `src/mcp.rs`, `src/codegen/js.rs` (entry points only), `src/web/runtime.js`.
- Tests: build `examples/web/` into a temp folder and check the files, a `dist/` size budget test, a headless
  click-through of a `Msg` example.

**WP6: Flora port, docs, experiment (after WP1-WP5).**
- `examples/web/flora/site.nyra`: the whole flora site (both subsites and the selection screen; the 3D showroom as an
  `embed`), with a side-by-side screenshot comparison and Lighthouse numbers against the React original.
- `docs/WEB_CARD.md` (budget test like the main card), the spec section, `llms.txt`, the error entries.
- `bench/web/`: the A/B experiment: a mock-site spec, the two prompts (plain HTML/CSS/JS, Nyra with both cards), the
  same model, several runs each; reported: output and input tokens, cost, first-try build success, screenshot
  similarity to the spec, Lighthouse scores.

## Decisions

Taken by the owner after this design was written:

1. **Bare names.** A program with `use web` calls the web functions by their bare names. A program function or
   variable with the name of a web function is compile error E0372, with a message that names both.
2. **Named arguments for every function**, positional ones still work. E0226 now means "the function has no
   parameter of that name" (built in WP1).
3. **Built-in UI words** for `lang: "en"` and `lang: "cs"` only: a small table (the opening-hours badge, form labels
   and placeholders, the lightbox's close and arrows, the menu, ...), each word overridable by an argument or
   `say:`. Other languages fall back to English.
4. **Images**: the build writes WebP variants and `srcset` with the compiler's own codec (an outside tool only on
   request, `--image-tool`); a file it cannot read is copied with `width`/`height`, `loading="lazy"` and
   `decoding="async"`. The build never fails over images (E0386 is a warning).
5. **The web card's budget is 1,400 tokens**, like the main card's.
6. **The experiment reports both ratios**: 0.19x against plain HTML/CSS/JS and 0.48x against markup over a complete
   component kit.

## Open questions and risks

1. (decided, see above)
2. (decided, see above)
3. (decided, see above)
4. **The kit's look is the whole bet.** Savings come mostly from the kit; a kit that looks generic pushes agents to raw
   CSS. Mitigation: the flora bar, `style` presets, screenshot tests, and the clone workflow as a coverage test of
   real sites.
5. **First-try correctness is unknown.** Models have never seen Nyra web. Named options and words are checked at
   compile time with nearest-match hints, but the benchmark must measure pass@1 before any claim.
6. **Images without crates**: solved by the built-in codec (decoders for JPEG, PNG, GIF, BMP and WebP, a resizer, and
   lossy and lossless WebP and JPEG encoders, see [Images](#images)); an outside tool is optional.
7. **Browser support**: CSS scroll-driven animations and cross-document View Transitions are not in every browser
   (the fallbacks are a JS loop and plain navigation); `linear()` easing has a cubic-bezier fallback.
8. **Values that change with time** (years since 1991) are computed at build time unless the kit offers a live
   option; a static site needs rebuilding for them.
9. **Live-region detection by data flow** may surprise ("why did this re-render?"). `nyra outline --json` will list
   the live regions and the variables each one reads.
10. **The fairness of the experiment**: the HTML side gets no component kit; with one, the measured ratio is 0.48x,
    not 0.19x. The write-up must state both.
