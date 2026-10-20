You are a senior front-end developer. Build the website described below as **one single file, `site.html`**, that works when opened directly in a browser.

## Rules
- Everything in one file: HTML, all CSS in `<style>`, all JavaScript in `<script>`. No build step.
- No frameworks or libraries (no React, Tailwind, jQuery, animation libraries or icon fonts). Plain HTML, CSS and JavaScript only. Google Fonts is the only external stylesheet allowed; icons, if you want them, are inline SVG.
- The images are the given URLs; do not use other images.
- It must work: no console errors, every interaction in the brief works.

## The site to build

A one-page website for **Autoservis Dvořák**, an independent car repair shop in Pardubice (Czech Republic). Use exactly the text, data and image URLs below. The site must look premium, solid and trustworthy (think a well-designed modern workshop brand in 2026), work on phones and desktops, and be accessible (alt texts, labels, keyboard focus).

**Look:** clean and technical. Background `#F4F5F7` (light grey), surfaces `#FFFFFF`, text `#1C2430`, primary color `#1F4E8C` (deep blue), accent `#F2A900` (signal yellow). Headings in **Barlow Condensed** (bold), body text in **Source Sans 3** (both from Google Fonts). Rounded cards, thin borders, strong section headings.

**Motion:** the hero text comes in step by step (kicker, title, text, buttons); sections and their cards reveal as you scroll (cards one after another); the four steps of "How it works" appear one after another; cards lift slightly on hover; the numbers count up when they appear. Respect "reduced motion" settings: with reduced motion nothing moves and the numbers show their final value at once.

### Sections, in order

1. **Navigation bar**, sticky at the top: brand "Autoservis Dvořák", links Services (`#services`), Prices (`#prices`), How it works (`#process`), Team (`#team`), Contact (`#contact`), and a highlighted button "Book a service" linking to `#contact`. On phones the links fold into a menu button.

2. **Hero**, a full-screen photo with a dark gradient so the text is readable.
   - Photo: `https://picsum.photos/seed/dvorak-hero/1600/1000` (alt: "A mechanic working under a lifted car at Autoservis Dvořák")
   - Small kicker line: "Pardubice · Hradecká 88 · all makes"
   - Title: "Honest repairs, done right the first time"
   - Text: "Servicing, tyres, diagnostics and bodywork for every make. Fixed prices up front and a call before any extra work."
   - Buttons: "Get a price" (`#prices`, filled) and "Book a service" (`#contact`, outlined)

3. **Numbers**: four items; the first three count up: **24** years in Pardubice, **9,800** cars serviced last year, **6** certified mechanics; the fourth is text and does not count: **2 years** warranty on parts and labour.

4. **Services** (`#services`), kicker "Services", title "What we do". Four tabs: Servicing, Tyres, Diagnostics, Bodywork. "Servicing" is selected when the page loads. Clicking a tab shows its panel and hides the others (without reloading the page); the selected tab is highlighted. Each panel has one sentence and three points:
   - Servicing: "Scheduled servicing to the maker's plan, so your warranty stays valid." · "Oil and filter change" · "Brake check and fluids" · "Stamp in your service book"
   - Tyres: "Seasonal tyre changes, storage and repairs while you wait." · "Tyre fitting and balancing" · "Tyre hotel for 1,200 Kč a season" · "Puncture repair"
   - Diagnostics: "We read the fault codes and find the real cause before we replace anything." · "Engine and electronics diagnostics" · "Air-con check and refill" · "Pre-purchase inspection"
   - Bodywork: "Dents, scratches and paint, matched in our own paint shop." · "Dent repair" · "Colour-matched paintwork" · "Insurance claims handled for you"

5. **Price calculator** (`#prices`), kicker "Prices", title "Get a price in ten seconds", subtitle "Fixed prices including VAT, parts and labour."
   - The visitor picks the car size: "Small car", "Family car" or "SUV / van". "Family car" is selected when the page loads.
   - Then ticks the services they need (none is ticked when the page loads). Each service shows its price for the selected car size:

     | Service | Small car | Family car | SUV / van |
     |---|---|---|---|
     | Oil service | 1,490 Kč | 1,790 Kč | 2,290 Kč |
     | Front brake pads | 2,190 Kč | 2,590 Kč | 3,190 Kč |
     | Air-con service | 1,290 Kč | 1,290 Kč | 1,590 Kč |
     | Wheel alignment | 990 Kč | 990 Kč | 1,290 Kč |
     | Tyre change (4 wheels) | 800 Kč | 960 Kč | 1,200 Kč |

   - Below, "Estimated total" shows the sum of the ticked services for the selected car size, for example "3,080 Kč" for an oil service and an air-con service on a family car. It updates at once when the car size or a service changes (without reloading the page); with nothing ticked it shows "0 Kč".
   - A button "Book this service" linking to `#contact`.

6. **How it works** (`#process`), kicker "How it works", title "Four steps, no surprises". Four numbered steps along a line:
   1. "Book online or call" · "Pick a time that suits you. Most services are done the same day."
   2. "Drop off your car" · "Leave the key at the desk, or in our key box before 7:00."
   3. "We call before extra work" · "If we find anything else, you decide. Nothing is done without your OK."
   4. "Pick up and pay" · "Pay by card or bank transfer. Every job comes with a 2-year warranty."

7. **Team** (`#team`), kicker "Team", title "The people under the bonnet". Four people, each with a photo, name, role and one sentence. Photo URL `https://picsum.photos/seed/dvorak-team-N/600/700` where N is 1 to 4; the alt text is the person's name.
   1. "Jiří Dvořák" · "Owner, master mechanic" · "Opened the shop in 2002 and still checks every car that leaves it."
   2. "Pavel Svoboda" · "Diagnostics" · "Finds the electrical faults that other garages gave up on."
   3. "Martina Veselá" · "Service advisor" · "Explains every line of your invoice in plain words."
   4. "Lukáš Král" · "Bodywork and paint" · "Matches any colour, from a 1990s Škoda to a brand-new Tesla."

8. **Testimonials**, title "What drivers say", three quotes:
   - "They called before changing anything, and the price was exactly as quoted." — Michal T.
   - "Found an electrical fault that two other garages missed." — Simona K.
   - "Tyres changed in 30 minutes while I had a coffee." — Radek P.

9. **Contact** (`#contact`), kicker "Contact", title "Book your car in".
   - Address: "Hradecká 88, 500 03 Pardubice", with a link "Open in Google Maps" (`https://maps.google.com/?q=Hradecká+88,+Pardubice`).
   - Phone "+420 466 000 245" (clickable `tel:` link) and e-mail "servis@autodvorak.cz" (clickable `mailto:` link).
   - Opening hours: Monday–Friday 7:00–17:00, Saturday 8:00–12:00 (tyres only), Sunday closed.
   - A form with Name (required), Phone (required), E-mail (optional), Car make and model (required), Service (required, a select with: Servicing, Tyres, Diagnostics, Bodywork, Something else), Preferred date (optional, a date field) and Message (optional). Every field has a visible label. The button says "Book service".
   - Sending it opens the visitor's e-mail app with a message addressed to servis@autodvorak.cz, subject "Service booking", and all the entered details in the body (no server).

10. **Footer**: "Autoservis Dvořák", the address, the five section links, and "© 2026 Autoservis Dvořák s.r.o.".

Page title: "Autoservis Dvořák – honest car repairs in Pardubice". Description for search engines: "Independent car repair shop on Hradecká in Pardubice: servicing, tyres, diagnostics and bodywork for all makes, with fixed prices and a 2-year warranty."

## Your answer
Reply with the complete `site.html` in one code block and nothing else: no explanation before or after, no "rest of the code here" placeholders.
