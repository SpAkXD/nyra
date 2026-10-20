## Method

This is round 4 of the web A/B test. The method, models, settings, repair rule, checklist rules and report format are
those of round 3 (`bench/web/results/2026-10-19/`); only the briefs are new, and Nyra was fixed in between.

**Question.** Given the same client brief, does a model build a better working one-page website as one plain HTML
file (HTML, CSS and JavaScript by hand) or as one Nyra file with `use web` (compiled by `nyra build` into HTML, CSS and
a little JavaScript)? And what does each cost in tokens, time and money?

**Held-out briefs, new for this round.** Round 3's dentist and gym briefs were studied to fix Nyra after round 3, so
re-testing them would be overfitted. This round uses two **new** briefs, written for this test in the same style and
detail as the earlier ones, as a client would write them, before looking at Nyra's web kit, cards or source; they were
committed (with their checklists and prompts, commit `b370ebb`) before the first model call, and Nyra was not changed
for them (compiler at `v0.8` commit `679f69a`, "Merge web fixes from benchmark v3"):

- `briefs/guesthouse.md`: a riverside guesthouse: six rooms as cards with a filter by room type, amenities, a photo
  gallery with a lightbox (buttons and keyboard), reviews, a booking-request form with two date fields and two
  selects, location with a Google Maps link, an FAQ accordion, reception hours with a live "Reception open" /
  "Reception closed" badge.
- `briefs/garage.md`: a car repair shop: counters, services in four tabs, a price calculator (car size x ticked
  services -> live total), a four-step "How it works" timeline, team, testimonials, a booking form with a select and a
  date.

Both give exact texts, data, picsum.photos image URLs with alt texts, colors, fonts and motion requirements ("respect
reduced motion"). The prompts are made by `make_prompts.py` from the **current** `docs/AGENT_CARD.md` and
`docs/WEB_CARD.md` (after the round-3 fixes): HTML = brief + "write one site.html"; Nyra = brief + the language card +
the web card + "write one site.nyra". Prompts: `bench/web/prompts/<brief>/`.

**Models and settings.** Anthropic API only, `claude-opus-5-5` and `claude-sonnet-5-5` with `output_config.effort:
"medium"` (thinking left to the model: both think adaptively by default), `claude-haiku-4-5-20251001` with its
defaults (no `effort`, no extended thinking), `max_tokens` 64,000, streaming. The whole prompt is one user message with
prompt caching. 4 runs per model x language x brief = 48 first answers. Hard spend cap: $12 for the round
(`run_ab.py --briefs guesthouse,garage --cap 12 --parallel 6`).

**One repair turn, Nyra only (the asymmetry).** A Nyra file that does not build gets at most one more turn: the
compiler's message is sent back ("The compiler rejected site.nyra: ... Fix it."), as a developer would paste it. The
tables give "builds first try" and "builds after one repair"; Nyra's tokens, time and cost include the repair turn.
HTML has no compiler, so an HTML file always "builds" and never gets a second turn, even when it is broken: its bugs
only show in the checklist. The checklist score is the fair comparison of what the visitor gets.

**Functional checklist (identical for both languages, desktop 1280x900 only, no phone view).** `check.js` loads every
site in headless Brave (served gzip-compressed by `serve.py`) with the browser time zone set to Europe/Prague and the
clock fixed by the spec (`checks/<brief>.json`, derived item by item from the brief). The items of round 3 are
unchanged (see its report for how each is checked); this round uses:

| item | how it is checked |
|---|---|
| page title and description | `document.title` and `meta[name=description]` equal the brief's (after normalizing dashes, quotes, spaces, thousands separators, case) |
| all brief text present and visible | every text of the brief is in the page and visible after scrolling through (FAQ answers, tab panels and calculator totals are checked by their own items) |
| all brief images present and loaded | every brief image is on the page (by URL or alt text), every rendered image loaded, no failed image request |
| no horizontal scroll at 1280 px | `scrollWidth` <= viewport width |
| phone, e-mail and map links | a `tel:` link with the number, a `mailto:` link with the address, "Open in Google Maps" pointing at Google Maps |
| every nav link scrolls to its section | each nav link (and the highlighted button) is visible in the top bar, its target id exists, and a click brings the section to the top of the view |
| guesthouse: live badge (open) | at Wednesday 10:00 the page shows "Reception open" and not "Reception closed" |
| guesthouse: live badge (closed) | at Saturday 20:30 (open until 21:00 on weekdays but 20:00 at weekends) the page shows "Reception closed" and not "Reception open" |
| guesthouse: room filter | all 6 rooms visible on load; Family shows exactly the two family rooms, Suite the two suites, Double the two doubles, All all 6 (the gym's day-filter check with room types) |
| guesthouse: FAQ items open and close | each answer is hidden, shows after a click on its question, hides after a second click |
| guesthouse: lightbox (4 items) | opens on click; next/previous buttons; close button; ArrowRight/ArrowLeft move and Escape closes (the gym's checks) |
| garage: counters end on the final number | 24, 9,800 and 6 next to their labels 4 s after scrolling to them |
| garage: service tabs (new) | on load only the Servicing panel's 4 texts are visible; after a click on Tyres, Bodywork, Diagnostics, Servicing only that tab's panel texts are visible |
| garage: price calculator (new) | in `#prices`: tick Oil service + Front brake pads (Family car is the default) -> 4,380 shown; choose SUV / van -> 5,480 shown and 4,380 gone; untick the brake pads and tick Air-con service -> 3,880 shown and 5,480 gone; choose Small car -> 2,780 shown and 3,880 gone. The car size may be a select, radio buttons or buttons; a service a checkbox or a toggle button. The brief's own example total (3,080) is not used |
| form fields | every field of the brief has a visible label; the required ones are `required` (or `aria-required`); the selects have the brief's options |
| form opens a filled e-mail | after filling every field and clicking submit, the page navigates to `mailto:` the brief's address, with the brief's subject and every entered value in the body (dates excepted, as in round 3, since they may be reformatted) |
| respects prefers-reduced-motion | with `prefers-reduced-motion: reduce`: no CSS/Web animation or transition longer than 10 ms starts, counters show the final number at once, and nothing visible with normal motion becomes invisible |
| no console errors | no uncaught exception, `console.error` or browser error log entry in any pass (favicon 404 ignored) |
| no CSP violations | no Content-Security-Policy report in any pass |

Score = passed / total (19 items for the guesthouse brief, 14 for the garage brief). A Nyra file that does not build
even after the repair has no site and scores 0 in the headline column; a second column averages built sites only.

**Other metrics.** As in round 3: output tokens (thinking included), input tokens (with cache reads and writes),
estimated thinking tokens, wall time, cost (from the token counts with the prices in `bench/providers.py`), code
tokens of the final file on one tokenizer (`claude-sonnet-5-5`), LCP and transferred KB from `perf.js` (desktop,
unthrottled, median of 3), and a half-scale full-page screenshot per site in `shots/`.

**Checker changes for this round (made before any model call, the same for both languages).** `check.js` got two new
items (tabs, calculator) for the garage brief's interactions; the filter item takes a `group` and a name from the spec
(the gym check is unchanged); "today's row" only runs when the spec asks for one (the guesthouse brief does not). The
new items were tested on two small hand-written pages, a correct one (pass) and a broken one (all panels shown, a total
that ignores the car size: both fail as expected), not on model output. There was no paid pilot this round.

**Checker changes after seeing outputs (disclosed; the same for both languages; all 41 sites checked again with the
final checker, the perf results were kept).** Two rules were changed after the first check pass:
- The live badge texts must now stand as whole words. Before, "Reception open" was found as a substring of a card
  titled "Reception" followed by the kit's "Opening hours" heading, which gave at least one Nyra site (Opus,
  guesthouse run 1) a false pass on a page that actually says "Open now". All 12 HTML sites pass both badge items
  under either rule. This change turned a false pass into a fail.
- In the calculator, a service toggle button whose text gains a leading symbol when selected ("✓ Front brake pads ·
  3,190 Kč") is now found again to untick it. Before, the checker failed a correct calculator (Sonnet, garage Nyra
  run 2, total 5,480 shown, the site worked when driven by hand). No HTML site was affected. This change turned a
  false fail into a pass.

**Known limits.** As in round 3: 4 runs per cell is small (one bad run moves a mean by 25 points; look at min/max);
the checklist tests function, not beauty; desktop only; the checker finds things by the brief's texts (every failure
is listed with its detail below, so a checker mistake can be spotted); remote images and fonts load over the real
network; Nyra's `_headers` CSP file is not applied by the test server; Opus and Sonnet think adaptively and Haiku does
not. New: the two briefs have different numbers of items (19 vs 14), so the per-brief means are not directly
comparable, and the round-3 vs round-4 comparison below compares different briefs.
