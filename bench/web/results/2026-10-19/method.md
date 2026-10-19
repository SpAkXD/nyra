## Method

**Question.** Given the same client brief, does a model build a better working one-page website as one plain HTML
file (HTML, CSS and JavaScript by hand) or as one Nyra file with `use web` (compiled by `nyra build` into HTML, CSS and
a little JavaScript)? And what does each cost in tokens, time and money?

**Held-out briefs.** Earlier rounds used the bakery brief (`bench/web/brief.md`), and Nyra's web kit was then fixed
by studying those very outputs, so re-testing the bakery would be overfitted. This round uses two **new** briefs,
written for this test in the same style and detail as the bakery one, as a client would write them, without looking
at Nyra's web kit, and Nyra was not changed to fit them before the run (compiler at `v0.8` commit `fbca6d9`):

- `briefs/dentist.md`: a dental practice: services, team, a prices table, a counter row, testimonials, a booking
  form with a select and a date, an FAQ accordion, opening hours (one row per day) with a live "Open now"/"Closed now"
  badge and today's row highlighted, an OpenStreetMap embed.
- `briefs/gym.md`: a fitness studio: a class schedule with a day filter, trainers, membership plans with a
  monthly/yearly switch, counters, testimonials, a gallery with a lightbox (buttons and keyboard), a contact form.

Both give exact texts, data, picsum.photos image URLs with alt texts, colors, fonts and motion requirements
("respect reduced motion"). The prompts are made by `make_prompts.py` exactly as for the bakery: HTML = brief +
"write one site.html"; Nyra = brief + the language card + the web card (`docs/AGENT_CARD.md`, `docs/WEB_CARD.md`) +
"write one site.nyra". Prompts: `bench/web/prompts/<brief>/`.

**Models and settings.** Anthropic API, `claude-opus-5-5` and `claude-sonnet-5-5` with `output_config.effort:
"medium"` (thinking left to the model: both think adaptively by default), `claude-haiku-4-5-20251001` with its
defaults (it does not accept `effort`; no extended thinking), `max_tokens` 64,000, streaming. The whole prompt is one
user message with prompt caching (so the cards are cached for the 4 runs of a cell). 4 runs per model x language x
brief = 48 first answers.

**One repair turn, Nyra only (the asymmetry).** A Nyra file that does not build gets at most one more turn: the
compiler's message is sent back ("The compiler rejected site.nyra: ... Fix it."), as a developer would paste it. The
tables give "builds first try" and "builds after one repair"; Nyra's tokens, time and cost include the repair turn.
HTML has no compiler, so an HTML file always "builds" and never gets a second turn, even when it is broken: its bugs
only show in the checklist. This favours HTML on cost and Nyra on correctness of the build; the checklist score is
the fair comparison of what the visitor gets.

**Functional checklist (identical for both languages, desktop 1280x900 only, no phone view).** `check.js` loads every
site in headless Brave (served gzip-compressed by `serve.py`, like a production server) with the browser time zone
set to Europe/Prague and the clock fixed by the spec (`checks/<brief>.json`, derived item by item from the brief):

| item | how it is checked |
|---|---|
| page title and description | `document.title` and `meta[name=description]` equal the brief's (after normalizing dashes, quotes, spaces, thousands separators, case) |
| all brief text present and visible | every text of the brief is in the page and visible after scrolling through (FAQ answers are checked by the FAQ item, since a page may only add them when opened) |
| all brief images present and loaded | every brief image is on the page (by URL or alt text), every rendered image loaded, no failed image request |
| no horizontal scroll at 1280 px | `scrollWidth` <= viewport width |
| phone, e-mail and map links | a `tel:` link with the number, a `mailto:` link with the address, "Open in Google Maps" pointing at Google Maps |
| every nav link scrolls to its section | each nav link (and the highlighted button) is visible in the top bar, its target id exists, and a click brings the section to the top of the view |
| counters end on the final number | the numbers next to their labels show the final value 4 s after scrolling to them |
| dentist: live badge (open) | at Wednesday 10:00 the page shows "Open now" and not "Closed now" |
| dentist: live badge (closed) | at Friday 15:00 (open until 14:00 on Fridays) the page shows "Closed now" and not "Open now" |
| dentist: today's row highlighted | Wednesday's hours row is styled differently from Monday's |
| dentist: FAQ items open and close | each answer is hidden, shows after a click on its question, hides after a second click |
| dentist: map embed shows | an iframe with the brief's OpenStreetMap URL, at least 200x150 px, and the frame actually loaded |
| gym: day filter | all 12 classes visible on load; Tue shows exactly Tuesday's two, Sat Saturday's two, All all 12 (visible = rendered, not clipped, not transparent) |
| gym: monthly/yearly switch | monthly prices shown on load and no yearly ones; "Yearly" swaps them; "Monthly" swaps back |
| gym: lightbox opens on click | clicking the first gallery photo shows it large in an overlay |
| gym: lightbox next/previous buttons | next shows photo 2, previous photo 1 |
| gym: lightbox closes with its button | |
| gym: lightbox keyboard | ArrowRight, ArrowLeft move, Escape closes |
| form fields | every field of the brief has a visible label; the required ones are `required` (or `aria-required`); the select has the brief's options |
| form opens a filled e-mail | after filling every field and clicking submit, the page navigates to `mailto:` the brief's address, with the brief's subject and every entered value in the body (parsed like a mail client: `+` stays `+`) |
| respects prefers-reduced-motion | with `prefers-reduced-motion: reduce`: no CSS/Web animation or transition longer than 10 ms starts (CDP Animation domain), counters show the final number at once, and nothing that was visible with normal motion becomes invisible |
| no console errors | no uncaught exception, `console.error` or browser error log entry in any pass (favicon 404 ignored) |
| no CSP violations | no Content-Security-Policy report in any pass |

Score = passed / total (17 items for the dentist brief, 18 for the gym brief). A Nyra file that does not build even
after the repair has no site and scores 0 in the headline column; a second column averages built sites only.

**Other metrics.** Output tokens (API, thinking included), input tokens (with cache reads and writes), an estimate of
thinking tokens (output tokens minus the `count_tokens` of the visible reply, on the same model), wall time of the
calls, cost (computed from the token counts with the prices in `bench/providers.py`), code tokens of the final file
on one tokenizer for all (`claude-sonnet-5-5`, `count_tokens`), LCP and transferred KB from `perf.js` (desktop
1280x900, unthrottled, cache off, fresh profile, median of 3 loads), and a full-page screenshot per site in
`shots/` (half scale).

**Known limits.**
- 4 runs per cell is small: one bad run moves a mean by 25 points. Look at min/max, not only means.
- The checklist tests function, not beauty: two sites with the same score can look very different (see `shots/`).
  Accessibility is only checked through labels, alt texts and reduced motion, not with an audit tool.
- Desktop only; phone layout and the menu button are not tested.
- The checker finds things by the brief's texts. A site that renames a button ("Send message" instead of "Send") is
  still found when it is the form's submit button; a filter whose buttons say neither "Tue" nor "Tuesday" fails.
  Every failure is listed with its detail below so a checker mistake can be spotted.
- Remote images (picsum.photos) and fonts load over the real network, so LCP and KB include their variance.
- Nyra's `_headers` file (a CSP for hosts like Netlify) is not applied by the test server; both sides only get the
  CSP their page declares in a `<meta>` tag.
- Opus and Sonnet think adaptively at medium effort and Haiku does not think; models differ, the languages within a
  model get the same settings.
- Two checker rules were widened after looking at the first outputs of this round, the same for both languages, and
  every affected site was checked again with the final checker: an FAQ answer may be opened by a button of its item
  ("Show answer", seen in an Opus Nyra site) and not only by the question, and the monthly/yearly switch may be a
  plain element without text (seen in Haiku HTML sites). Both changes only turned false failures into passes.
- Pilot: before the round, 6 calls (Haiku both languages, Sonnet Nyra) on the same briefs were used to test the
  harness ($0.39, inside the $20 cap). They found two harness bugs (a reply without an opening code fence was not
  extracted; the mailto parser) that were fixed before the round; their outputs are not part of the results.
