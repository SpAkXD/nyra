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
| `--browser` | Load the page in a headless Chrome, Edge, Brave or Chromium and read the DOM it renders (below). `NYRA_BROWSER` names the browser's executable when it is not in the usual place |
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
   offline cache never answers instead of the site). `clone` drives it over the DevTools protocol (`src/clone/cdp.rs`:
   a WebSocket over `std::net`, no crates), as a 1440 x 900 desktop window in light mode:
   - it opens the page and waits until the network is quiet;
   - it **turns down a cookie or consent dialog** with its most private choice: a button labelled exactly "Reject
     all", "Decline", "Only necessary", "Odmítnout vše", "Nur notwendige", "Alle ablehnen", "Tout refuser" (and
     the like in a dozen languages), and only inside a box that talks about cookies, consent or privacy, also in
     frames from other sites and in shadow DOM. "Accept" is never clicked. The summary names the button clicked;
     the choice stays in the profile;
   - it scrolls ten screens down and back, so lazy images and lists load, and watches what moves (Motion, below);
   - it writes what only a browser knows into the DOM for the reader: elements not shown, a column of links down the
     left side of the window (an app's side navigation), the width of each picture, the computed background of the
     page's bands and of the body, links drawn as buttons, and the rules of CSS-in-JS stylesheets (styled-components,
     emotion), which the DOM's `<style>` elements do not hold.

   A browser that does not speak the protocol falls back to `--dump-dom`. `NYRA_CLONE_SAVE=page.html` saves the page as
   `clone` read it (with these hints), for debugging.
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
| A side navigation beside the content (the browser saw a column of links down the left; or a box named `sidebar`, `sidenav`, `docs-nav`, ... that is mostly links and comes before the content) | `sidebar([(label, href)]) { the sections }`: headings and lines between the links become entries with an empty href; links with an icon get the kit's icon for their label (`:house: Home`) |
| Three or more children with one thumbnail, a title and a link, and a video length on the picture, "views", or links to YouTube/Vimeo videos | `grid(4) { media(title, image:, href:, meta: "Channel · 12K views · 2 days ago", badge: "9:56") }` (a struct, an array and a loop from four up); a YouTube or Vimeo thumbnail plays in place |
| The same with a price on most children | `media(..., price: "£24.00", ratio: "1/1")`; when each product names a category (a `category` or `brand` box), buttons filter the grid: `enum Msg { Show(str) }`, `fn update`, and a live view (WEB.md, Interactivity) |
| A `<video>` with a file, a YouTube or Vimeo iframe, a video player box (`<video>` with the player's buttons) on the page of a YouTube or Vimeo video, `og:video` | `video(src, poster:, title:)`: the same video plays (a YouTube or Vimeo player loads only when clicked, from youtube-nocookie.com) |
| `role="tablist"` with its `role="tabpanel"` panels in the page (Bootstrap tabs too) | `tabs([labels]) { a panel per child }`: every panel's content, hidden or not |
| A tab list whose tabs are other pages | `row { button(label, href, style: "primary" or "ghost", size: "sm") }`, the current one filled |
| A search input over a list (`type="search"`, a placeholder "Search"/"Filter"); a site-wide search box in the header when the page has a grid of six or more | `search(hint)` right before the grid: it filters the cards as one types |
| A carousel or slider of pictures (`swiper`, `slick`, `splide`, `carousel`, ...) | `gallery([...])` of all its slides, the loop's copies left out |
| An endless strip (an element whose animation slides it left forever, `<marquee>`) | `marquee([words])`, or `row(loop: "marquee") { img(...) }` for logos |
| A button of the page's scripts with a short label (Subscribe, Share, Save) | `button(label, page URL)`: a static button that goes to the original page |
| A small picture (65 to 400 px wide: an avatar, a logo) | `img(src, alt, width: "160px")` |
| A hero whose picture stands beside the text (the browser drew it narrower than half the window, or it is small) | `section { grid(2, align: "center") { stack { h1 p buttons } img(..., fit: "contain") } }` |
| A table of layout (tables inside it), or a list laid out as a table (no header, many empty cells) | its cells read as boxes; its rows as a `list` |
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
scripts (those with a short label stay as static buttons, counted too), form controls outside a form, navbar links
over eight.

## Motion

`src/clone/motion.rs` finds what moves on the page and gives it the kit's motion words (WEB.md, Motion):

| On the page | Word |
|---|---|
| Scroll-reveal libraries: AOS `data-aos="fade-up"` (`fade-left`, `zoom-in`, `flip-up`, ...), sal.js `data-sal`, `data-animate`, WOW.js and Animate.css classes (`wow animate__fadeInUp`), ScrollReveal (`data-sr`), reveal classes (`reveal`, `fade-up`, `animate-on-scroll`, ...), a server-rendered start state (`style="opacity:0;transform:translateY(40px)"`: Framer Motion, GSAP) | `anim: "rise"`, `"fade"`, `"slide"` (from the left), `"slide-left"` (from the right), `"zoom"`, `"pop"`, `"blur"` |
| With `--browser`: elements the browser saw revealed while scrolling (transparent, moved or scaled before, in place after), with how they came in | the same words |
| CSS `animation` with its `@keyframes`, read from the frames: opacity and a move or scale once; endless rotation, a strip sliding left by half its width, a moving `background-position`, a growing fading ring, a scale or opacity that pulses, a small up and down; with `--browser` also every animation `document.getAnimations()` reports, with its keyframes | `anim:` fade rise slide slide-left zoom pop blur draw; `loop:` spin marquee gradient ping pulse float bob |
| Keyframes no word fits | a word of its own: `keyframes("name", "0% {{ ... }} 50% {{ ... }}")` (checked as the build checks it, E0388; keyframes that animate layout properties, E0375, are left out) |
| `:hover` rules: a move up, a picture that grows inside a card, a shadow, an underline | `hover: "lift"`, `"zoom"`, `"glow"`, `"underline"` |
| Children of a grid that each reveal | `grid(..., anim: "stagger")` |
| Counters (`data-purecounter-end`, `data-count`, `.counter`, `.odometer`; with `--browser` a number that changed while scrolling) | `stat(..., anim: "count")` |
| Parallax (`data-speed`, `data-scroll-speed`, `data-rellax-speed`, `data-parallax`, `.parallax`, `.jarallax`, `background-attachment: fixed`) | `scroll: "parallax"` |
| Typing effects (`.typed`, `.typewriter`), split text (`data-splitting`) | `anim: "letters"`, `"words"` |

The words go where the kit can show them: on a section (its own reveal, or that of its heading), on the hero, on a
grid (stagger, hover), on stats (count), on pictures (loops, parallax) and on strips (marquee). The summary lists what
was found and the word it became (`--json`: `motion`), and how many motion options the program has.

## Server features

A clone is a static site: v0.9 has no server. What the original does on its server cannot be copied, and the clone
says so. The summary has a line `server    needs a server: sign-in and accounts, comments, ...` (`--json`:
`server`), and the program has a `// TODO(server): ...` note where each feature was:

| Feature | Found by | In the clone |
|---|---|---|
| Sign-in and accounts | "Sign in", "Log in", "My account" links and buttons, password fields, links to `/login` or an accounts site | a note before the navbar; the links still go to the original site |
| Subscriptions, likes, follows | "Subscribe", "Like", "Follow" buttons | a note before the navbar; the buttons are static |
| A search over the whole site | a search form in the header (`role="search"`, an action with `search` or `results`) | a note; the page's own grid gets a `search` box that filters it |
| Comments | `#comments`, `.comments`, `ytd-comments`, Disqus | a note at the comments, which stay as a static copy |
| Recommendations | `#related`, YouTube's next videos | a note; the list stays as a static copy |
| A cart and payments | "Add to cart", "Buy now", "Checkout", links to `/cart` | a note before the products; prices are a static copy |
| Uploads | file fields | a note at the form |
| Video streaming | a `<video>` whose stream a script makes (`blob:`), on a page that is not a YouTube or Vimeo video | a note; host the file, or a YouTube or Vimeo copy |

A future `use server` would need, as design notes (nothing of this is built):

- **Routes**: `route("/api/comments", method: "POST") { ... }` functions next to `page(...)`, compiled with the
  native backend into one binary that also serves the built site; `nyra run` would run both.
- **A database**: typed tables from `struct`s (`table Comment { video: str, author: str, text: str, at: int }`), with
  queries written as Nyra expressions; SQLite on disk, migrations from struct changes.
- **Auth**: accounts, password hashing (argon2) and sessions in signed, `HttpOnly` cookies; sign-in with OAuth
  providers as a later step; `on:` messages that need a user would be checked on the server.
- **Uploads**: multipart bodies with a size limit, stored by content hash, images through the build's codec.
- **Video streaming**: files served with HTTP range requests (`206 Partial Content`), later HLS segments; until
  then, `video(...)` with a YouTube or Vimeo address or a file on a CDN.
- **Live data**: the client runtime's `update` would send a `Msg` to a route and get the new state back, so the live
  views of WEB.md work against server state (comments, likes, a cart).

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

With the newer parts of a page, the summary has more lines, for example `nyra clone https://www.youtube.com/@YouTube/videos --browser`:

```
  consent   turned the cookie dialog down (clicked "Reject all")
  navbar                 navbar  "YouTube"
  sidebar                sidebar  "14 links"
  section #page-manager  section: img, stat, button, search, media
  kept      1657 of 1807 words of text (91%), 74 of 74 images
  videos    90 that play here (a player loads from the video site only when clicked)
  server    needs a server: search over the whole site, sign-in and accounts, subscriptions and likes (see the `// TODO(server)` notes)
```

and the JSON has `"server": [...]`, `"motion": [{"found": "data-aos=\"fade-up\"", "word": "anim: \"rise\"", "count": 8}, ...]`,
`"videos": 90` and `"consent": "Reject all"`.

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
- A page that builds its content with JavaScript needs `--browser`; content that appears only after a click (or
  after ten screens of scrolling) is not seen. Shadow DOM is not read.
- Pages that need a login, or that block automated requests, cannot be cloned. A consent dialog that has no button to
  turn everything down on its first screen stays (and is left out as a dialog).
- What a server does (accounts, comments, recommendations, a real search, payments, uploads, streaming) is not copied;
  see Server features.
- Layouts the kit has no component for (two columns of text and a picture, a player beside a list) become a column.

## Tests

`tests/clone.rs` clones the pages in `tests/clone/` (a bakery, a SaaS landing page, a portfolio with a gallery, a page
of nested boxes with inline styles and text full of Markdown and Nyra characters, a WordPress page, an app layout
with a side navigation, a grid of video thumbnails, comments and a site search, a page of videos with a shop and a
carousel, and a page of tabs with custom keyframes) without network access, builds each program with `nyra build`, and checks that the built page has every heading, paragraph, list
item and quote of the original and all of its images, and that each page maps to the components it should. Unit tests
in `src/clone/` cover the HTML reader, the CSS cascade and colors, URL resolution, opening hours, escaping, and the
self-check that simplifies a part that does not build.
