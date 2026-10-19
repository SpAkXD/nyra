You are a senior front-end developer. Build the website described below as **one single file, `site.html`**, that works when opened directly in a browser.

## Rules
- Everything in one file: HTML, all CSS in `<style>`, all JavaScript in `<script>`. No build step.
- No frameworks or libraries (no React, Tailwind, jQuery, animation libraries or icon fonts). Plain HTML, CSS and JavaScript only. Google Fonts is the only external stylesheet allowed; icons, if you want them, are inline SVG.
- The images are the given URLs; do not use other images.
- It must work: no console errors, every interaction in the brief works.

## The site to build

A one-page website for **Zelená Dental**, a small dental practice of Dr. Petra Zelená in Olomouc (Czech Republic). Use exactly the text, data and image URLs below. The site must look premium, clean and trustworthy (think a well-designed private clinic site in 2026), work on phones and desktops, and be accessible (alt texts, labels, keyboard focus).

**Look:** fresh and calm. Primary color `#0F766E` (teal), accent `#F59E0B` (amber), background `#F7FAF9` (very light mint), text `#14302C`. Headings in **Outfit**, body text in **Inter** (both from Google Fonts). Rounded cards, soft shadows, lots of white space.

**Motion:** the hero text comes in step by step (kicker, title, text, buttons); sections and their cards reveal as you scroll (cards one after another); cards lift slightly on hover; the three numbers count up when they appear. Respect "reduced motion" settings: with reduced motion nothing moves and the numbers show their final value at once.

### Sections, in order

1. **Navigation bar**, sticky at the top: brand "Zelená Dental", links Services (`#services`), Team (`#team`), Prices (`#prices`), FAQ (`#faq`), Contact (`#contact`), and a highlighted button "Book a visit" linking to `#booking`. On phones the links fold into a menu button.

2. **Hero**, a large photo with a dark gradient so the text is readable.
   - Photo: `https://picsum.photos/seed/zelena-hero/1600/1000` (alt: "The bright treatment room of Zelená Dental")
   - Small kicker line: "Olomouc · Horní náměstí 21"
   - Title: "Gentle dentistry for the whole family"
   - Text: "Check-ups, hygiene and modern treatment without the stress. New patients welcome, children from 3 years."
   - Buttons: "Book a visit" (`#booking`, filled) and "See prices" (`#prices`, outlined)
   - A small live badge that says exactly "Open now" or "Closed now", computed in the visitor's browser from the opening hours below (use the visitor's local time).

3. **Numbers**: three numbers that count up: **15** years in Olomouc, **4,800** patients, **98** % would recommend us.

4. **Services** (`#services`), kicker "What we do", title "Care for every smile". Six cards, each with a small inline SVG icon, a name and a sentence:
   - "Check-ups" · "A thorough exam every six months, with digital X-rays only when needed."
   - "Dental hygiene" · "Scaling, polishing and airflow cleaning by our hygienist."
   - "Fillings" · "Tooth-coloured composite fillings that last."
   - "Whitening" · "Safe in-office or at-home whitening for a brighter smile."
   - "Children's dentistry" · "Calm first visits and sealants for kids from 3 years."
   - "Emergency care" · "Toothache? Call before 10 a.m. and we will see you the same day."

5. **Team** (`#team`), kicker "Our team", title "People who take their time". Four people, each with a photo, name, role and one sentence. Photo URL `https://picsum.photos/seed/zelena-team-N/600/600` where N is 1 to 4; the alt text is the person's name.
   1. "Dr. Petra Zelená" · "Dentist, founder" · "Twenty years of practice and a soft spot for nervous patients."
   2. "Dr. Martin Kovář" · "Dentist" · "Fillings, root canals and crowns, done carefully and explained clearly."
   3. "Lucie Horáková" · "Dental hygienist" · "Makes cleanings painless and teaches the tricks that keep teeth healthy."
   4. "Eva Šimková" · "Receptionist" · "Finds you a time that works and answers every question about insurance."

6. **Prices** (`#prices`), kicker "Prices", title "Clear prices, no surprises", subtitle "Prices in CZK. Check-ups are covered by public health insurance." A table with two columns, Treatment and Price:
   - Check-up (insured) · 0 Kč
   - Check-up (self-paying) · 900 Kč
   - Dental hygiene · 1,600 Kč
   - Composite filling · from 1,900 Kč
   - Root canal treatment · from 4,500 Kč
   - In-office whitening · 6,900 Kč
   - Children's check-up · 0 Kč
   - Emergency visit · 1,200 Kč

7. **Testimonials**, title "What our patients say", three quotes:
   - "I used to dread the dentist. Here I actually relax." — Jana P.
   - "They fitted in my son the same morning he broke a tooth." — Tomáš R.
   - "Clear prices and a hygienist who really knows her job." — Markéta V.

8. **Book a visit** (`#booking`), kicker "Book a visit", title "Request an appointment", text "Send us your request and we will call you back within one working day to confirm the time."
   - A form with Name (required), Phone (required), E-mail (required), Treatment (required, a select with: Check-up, Dental hygiene, Filling, Whitening, Children's check-up, Emergency), Preferred date (required, a date field) and Message (optional). Every field has a visible label. The button says "Send request".
   - Sending it opens the visitor's e-mail app with a message addressed to recepce@zelenadental.cz, subject "Appointment request", and all the entered details in the body (no server).

9. **FAQ** (`#faq`), kicker "FAQ", title "Questions we hear often". Clicking a question shows its answer; clicking it again hides it. Five questions:
   - "Do you accept new patients?" · "Yes. We accept new adult patients and children from 3 years. Book a first check-up with the form above."
   - "Which insurance companies do you work with?" · "All Czech public health insurers, including VZP, ČPZP and OZP."
   - "What should I bring to my first visit?" · "Your insurance card, an ID and a list of any medicines you take."
   - "Does dental hygiene hurt?" · "Usually not. If your gums are sensitive, we use a numbing gel at no extra cost."
   - "How do I cancel an appointment?" · "Call us at least 24 hours ahead so that someone else can take your time."

10. **Contact** (`#contact`), kicker "Contact", title "Find us on the main square".
    - Opening hours, one row per day: Monday 8:00–18:00, Tuesday 8:00–18:00, Wednesday 8:00–18:00, Thursday 8:00–18:00, Friday 8:00–14:00, Saturday closed, Sunday closed. Today's row is highlighted, with the same live "Open now"/"Closed now" badge.
    - Address: "Horní náměstí 21, 779 00 Olomouc", with a link "Open in Google Maps" (`https://maps.google.com/?q=Horní+náměstí+21,+Olomouc`).
    - A map: embed `https://www.openstreetmap.org/export/embed.html?bbox=17.2459%2C49.5913%2C17.2559%2C49.5963&layer=mapnik&marker=49.5938%2C17.2509` in a frame (title "Map of Zelená Dental").
    - Phone "+420 585 000 321" (clickable `tel:` link) and e-mail "recepce@zelenadental.cz" (clickable `mailto:` link).

11. **Footer**: "Zelená Dental", the address, the five section links, and "© 2026 Zelená Dental s.r.o.".

Page title: "Zelená Dental – gentle dentistry in Olomouc". Description for search engines: "Family dental practice on Horní náměstí in Olomouc: check-ups, hygiene, fillings, whitening and children's dentistry. New patients welcome."

## Your answer
Reply with the complete `site.html` in one code block and nothing else: no explanation before or after, no "rest of the code here" placeholders.
