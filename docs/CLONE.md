# Clone a page into Nyra: `nyra clone`

`nyra clone <url>` reads an existing web page and writes `site.nyra`: a `use web` program (see [WEB.md](WEB.md) and
the [web card](WEB_CARD.md)) that recreates the page with the kit's components. The program builds as it is
(`nyra build site.nyra`); an agent then refines it, which costs a few hundred tokens of review instead of thousands of
tokens of HTML and CSS.

```
nyra clone https://example.com                  # writes site.nyra and prints a summary
nyra clone https://spa.example --browser        # a page that JavaScript builds: read the DOM a browser renders
nyra clone saved/index.html --base https://example.com/   # a saved page; its local images go to assets/
nyra clone https://example.com -o shop.nyra --json        # the summary as JSON, for agents
nyra run shop.nyra                              # look at it, then refine it
```

Clone only pages you own or have the right to copy. `clone` reads public pages only: it sends no cookies and does not
log in.

## Options

| Option | Does |
|---|---|
| `-o FILE` | The program to write (default `site.nyra`). Images of a local page are copied into `assets/` next to it |
| `--browser` | Load the page in a headless Chrome, Edge, Brave or Chromium (`--headless=new --dump-dom`) and read the DOM it renders. `NYRA_BROWSER` names the browser's executable when it is not in the usual place |
| `--base URL` | The address of a saved page, so its relative links and images point to the site |
| `--json` | Print the summary as JSON (below) |
| `--no-check` | Do not compile and build the program in memory |
| `--force` | Overwrite a file that `nyra clone` did not write (without it, `clone` only replaces its own output) |

## How a page becomes a program

1. **Fetch.** Nyra has no dependencies, so it has no TLS of its own: the page is fetched with the system's `curl`
   (part of Windows 10 and later, macOS and Linux), following redirects, with a browser's User-Agent, a 20 s time
   limit and a 5 MB size limit. Then its linked stylesheets and their `@import`s (at most 12, 2 MB each), in parallel.
   Google Fonts stylesheets are not fetched: the family names are what matters. With `--browser` the browser gets its
   own profile in the temp folder (kept between runs; its service workers are removed before each run, so a site's
   offline cache never answers instead of the site), and has 30 s.
2. **Parse.** A forgiving HTML reader (`src/clone/dom.rs`): void elements, unclosed `p`, `li`, `td` and friends,
   stray end tags, unquoted attributes, character references, and the raw text of `script` and `style`.
3. **Style.** A small CSS reader (`src/clone/css.rs`) matches the page's rules to its elements well enough to know
   each element's background, text color, font, radius, `display` and alignment as a 1440 px wide desktop window in
   light mode shows them: selectors with tags, classes, ids, attributes and ancestors, specificity and order,
   `!important`, `style=""`, custom properties with `var()`, `@media` widths, and the colors `#hex`, `rgb()`,
   `hsl()`, `oklch()`, `oklab()`, `light-dark()` and color names. Hidden elements (`display: none`, `hidden`,
   `aria-hidden`, screen-reader-only text) are not content.
4. **Understand** (`src/clone/read.rs`): landmarks first (navbar, footer), then the page's sections, split at its
   top headings, then the shape of each part (table below). The theme comes from the styles: the page background and
   text color, the brand colors (the backgrounds of buttons count most, then links, headings and colored bands), the
   body and heading fonts, the corner radius of cards, light or dark, a narrow text column and left-aligned headings.
5. **Write** (`src/clone/write.rs`): `site(...)`, one `page("/", title)` with the components, and for repeated data
   (four or more cards with the same fields) a `struct`, an array and a `for` loop. Text keeps its inline formatting
   as the kit's Markdown (`**bold**`, `*italic*`, `` `code` ``, `[label](url)`); characters Markdown or Nyra would
   read are escaped (`{` is `{{`, `*` is `\\*`, `:word:` is not an icon).
6. **Check.** The program is compiled and its site built in memory, like `nyra build` does. A part (the navbar, a
   section, the footer) that has an error is written again more simply: first with plain elements (`h3`, `p`, `img`,
   `list`, `table`), then as text only. The summary says which parts were simplified; an error that remains is
   reported and the exit code is 1.

## What maps to what

| On the page | In the program |
|---|---|
| `<header>` or `<nav>` at the top: a brand (logo class, home link, or a logo image), its links, the last link that looks like a button | `navbar(brand, [(label, href)], cta: (label, href), logo:)` |
| The first part with the page's `h1`, and a background or large photo | `hero(title, kicker:, text:, image:, alt:, buttons:)`; without a photo or a colored band, a `section` that starts with `h1(...)` |
| Two to four panels side by side at the top, each with an `h1`/`h2` and a photo (a choice between subsites) | `split { card(title, text, image:, tag:, color:) }` |
| A box whose children repeat one shape with a heading or a photo | `grid(n) { card(title, text, image:, href:, tag:, items:) }`, buttons and other content in the card's block |
| Three or more children that are only photos | `gallery([(image, alt, "")])` |
| Children that are a number and a short label ("25+ years", "99.98% uptime") | `grid(n) { stat(value, label) }` |
| `blockquote`, testimonial or review boxes | `quote(text, name, place:)` |
| An `ol` of headed steps, or steps/timeline/process boxes | `steps([(label, text)])` |
| A `form` with fields (`mailto:` or an http(s) action) | `form(title, mailto:, to:, send:) { field(name, label, type:, req:, hint:, options:) }` |
| A table, list or `dl` of week days and times | `hours([(label, "1-5 8:00-16:00")])` |
| A Google Maps iframe | `map(address)` (the address from its query) |
| A YouTube, Vimeo, Spotify or map iframe | `embed(url, title)` |
| `<address>` | `address { p(line) }` |
| `<table>`, `<ul>`/`<ol>`, `<dl>` | `table(rows, head:)`, `list(items, ordered:)` |
| A row of links that look like buttons | `row { button(label, href) button(..., style: "outline") }` |
| `<pre>` | `html("<pre><code>...</code></pre>")` with a small `css(...)` rule for it |
| `<details>` | `h3(summary)` and its content |
| Anything else with text | `section(id, title, kicker:, text:, bg:) { h3 p img ... }` |
| `<footer>` | `footer(brand:, text:, links:, links_title:, contact:, small:)` (`©` lines are the small print, `mailto:`/`tel:` links the contact lines) |

Sections keep the ids that the page's links point to, so `#anchors` in the navbar and the text keep working; a link
to something that was left out becomes its text. A section with its own background becomes `bg: "alt"`,
`"dark"`, `"white"` or `"primary"`. Links are absolute (or `#anchors`, `mailto:`, `tel:`); images stay on the
original site as absolute URLs (the build allows them in the page's content security policy). To host them yourself,
save them into `assets/` and use their file names.

**Left out, and counted in the summary:** scripts, iframes that are not videos or maps, `canvas`, inline SVG
graphics, icons (images of 64 px or less), `data:` images, cookie banners and dialogs, buttons that only run
scripts, form controls outside a form, navbar links over eight.

## The summary

```
nyra clone: tests/clone/bakery.html -> site.nyra: 3.7 KB (about 945 tokens); the page was 5.6 KB of HTML and 0 B of CSS (about 1439 tokens)
  theme     primary #7a3e1d, accent #d99a2b, bg #fbf6ee, ink #2b1d12, font Nunito, display Lora, radius 14px
  navbar                 navbar  "Golden Crust"
  section #home          hero  "Fresh bread every morning"
  section #breads        section: card  "Our breads"
  section #about         section: stat  "About us"
  section #reviews       section: quote  "What our customers say"
  section #visit         section: hours, address, map  "Visit us"
  section #order         section: form  "Order a cake"
  footer                 footer
  kept      227 of 228 words of text (99%), 4 of 4 images
  dropped   2 scripts
  check     compiles and builds
  next      nyra run site.nyra   (images stay on the original site; save them into assets/ to host them)
```

(The CSS of this page is in a `<style>` block, so it counts as HTML.) The four bread cards became a struct, an array
and a loop:

```nyra
struct Bread { title: str, text: str, image: str, alt: str }
let breads = [
    Bread(title: "Sourdough loaf", text: "A crisp crust and a soft crumb, fermented for 24 hours.", image: "https://images.example.com/bakery/sourdough.jpg", alt: "A round sourdough loaf"),
    ...
]
...
    section("breads", "Our breads", text: "Everything is made from organic flour, water, salt and time.") {
        grid(4) {
            for c in breads { card(c.title, c.text, image: c.image, alt: c.alt) }
        }
    }
```

Tokens are estimated as 4 characters per token, the same way for the page and the program. "Words kept" compares
the words of the page's visible text with the words in the program, as runs of letters and digits.

With `--json`:

```json
{"ok":true,"url":"tests/clone/bakery.html","out":"site.nyra",
 "parts":[{"part":"navbar","component":"navbar","id":"","title":"Golden Crust"},
          {"part":"section #home","component":"hero","id":"home","title":"Fresh bread every morning"}, ...],
 "theme":{"primary":"#7a3e1d","accent":"#d99a2b","bg":"#fbf6ee","ink":"#2b1d12","font":"Nunito","display":"Lora","radius":"14px"},
 "kept":{"words":227,"of_words":228,"images":4,"of_images":4},
 "dropped":{"scripts":2},
 "assets":[],
 "simplified":[],
 "errors":[],
 "bytes":{"html":5753,"css":0,"nyra":3778},
 "tokens":{"original":1439,"nyra":945}}
```

`ok` is false when errors remain; an error before anything was written (a fetch that failed) is
`{"ok":false,"error":"..."}` with exit code 2.

## For tools

`src/clone.rs` has the whole conversion of a fetched page as one function, for the MCP tool that will come later:

```rust
pub fn clone_to_nyra(html: &str, base_url: &str, opts: &CloneOptions) -> CloneResult
```

`CloneOptions` holds the page's stylesheets (URL and text), the folder of a local page, whether to check, and the
folder the program will be in; `CloneResult` the program, the parts and their components, the theme, what was
dropped, the words and images kept, the local images to copy, what was simplified and the errors. The module is not
part of the WebAssembly build (it runs `curl` and browsers).

## Limits

- One page. Links to the site's other pages stay links to the original site.
- The look is the kit's, in the page's colors and fonts: layouts the kit has no component for (overlapping boxes,
  asymmetric grids, sliders, canvas and WebGL scenes) become plain sections, and motion is not copied (add `anim:`
  words while refining).
- The styles are read for a desktop window in light mode, without layout: sizes, positions and fonts that only a
  browser would compute are not known. Fonts are named for Google Fonts; a font that is not there falls back to the
  system's.
- A page that builds its content with JavaScript needs `--browser`; content that appears only after a click or a
  scroll is not seen.
- Pages that need a login, or that block automated requests, cannot be cloned.

## Tests

`tests/clone.rs` clones the pages in `tests/clone/` (a bakery, a SaaS landing page, a portfolio with a gallery, a page
of nested boxes with inline styles and text full of Markdown and Nyra characters, a WordPress page) without network
access, builds each program with `nyra build`, and checks that the built page has every heading, paragraph, list
item and quote of the original and all of its images, and that each page maps to the components it should. Unit tests
in `src/clone/` cover the HTML reader, the CSS cascade and colors, URL resolution, opening hours, escaping, and the
self-check that simplifies a part that does not build.
