You are a senior front-end developer. Build the website described below as **one single file, `site.html`**, that works when opened directly in a browser.

## Rules
- Everything in one file: HTML, all CSS in `<style>`, all JavaScript in `<script>`. No build step.
- No frameworks or libraries (no React, Tailwind, jQuery, animation libraries or icon fonts). Plain HTML, CSS and JavaScript only. Google Fonts is the only external stylesheet allowed; icons, if you want them, are inline SVG.
- The images are the given URLs; do not use other images.
- It must work: no console errors, every interaction in the brief works.

## The site to build

A one-page website for **Forge Fitness Studio**, a small independent gym and group-class studio in Brno (Czech Republic). Use exactly the text, data and image URLs below. The site must look premium, bold and energetic (think a well-designed boutique fitness brand in 2026), work on phones and desktops, and be accessible (alt texts, labels, keyboard focus).

**Look:** dark and strong. Background `#111315` (near black), surfaces `#1B1E22`, text `#F2F2F0`, primary color `#E4572E` (ember orange), accent `#F3C969` (warm yellow). Headings in **Archivo** (bold, uppercase), body text in **Inter** (both from Google Fonts). Rounded cards, subtle borders, generous spacing.

**Motion:** the hero text comes in step by step (kicker, title, text, buttons); sections and their cards reveal as you scroll (cards one after another); cards lift slightly on hover; the numbers count up when they appear. Respect "reduced motion" settings: with reduced motion nothing moves and the numbers show their final value at once.

### Sections, in order

1. **Navigation bar**, sticky at the top: brand "Forge Fitness", links Classes (`#classes`), Trainers (`#trainers`), Membership (`#membership`), Gallery (`#gallery`), Contact (`#contact`), and a highlighted button "Free trial" linking to `#contact`. On phones the links fold into a menu button.

2. **Hero**, a full-screen photo with a dark gradient so the text is readable.
   - Photo: `https://picsum.photos/seed/forge-hero/1600/1000` (alt: "Athletes training with kettlebells at Forge")
   - Small kicker line: "Brno · Veveří 54 · since 2016"
   - Title: "Train hard. Feel at home."
   - Text: "Strength, conditioning and yoga classes in small groups, with coaches who know your name."
   - Buttons: "See the schedule" (`#classes`, filled) and "Free trial" (`#contact`, outlined)

3. **Numbers**: four items; the first three count up: **12** coaches, **45** classes a week, **2,300** members; the fourth is text and does not count: **6 a.m.** first class.

4. **Classes** (`#classes`), kicker "Schedule", title "Classes this week", subtitle "Book through the app or at the front desk. Max 12 people per class."
   - Day filter buttons: All, Mon, Tue, Wed, Thu, Fri, Sat. "All" is active when the page loads. Clicking a day shows only that day's classes (without reloading the page); the active button is highlighted.
   - Twelve classes as cards with the day, time, class name, coach and level:
     1. Mon · 6:00 · "Sunrise Strength" · Coach Adam · All levels
     2. Mon · 18:00 · "Kettlebell Basics" · Coach Klára · Beginner
     3. Tue · 7:00 · "Mobility Flow" · Coach Tereza · All levels
     4. Tue · 19:00 · "Power Circuit" · Coach Ondřej · Advanced
     5. Wed · 6:00 · "Engine Builder" · Coach Adam · Intermediate
     6. Wed · 17:30 · "Vinyasa Yoga" · Coach Tereza · All levels
     7. Thu · 7:00 · "Olympic Lifting" · Coach Ondřej · Advanced
     8. Thu · 18:30 · "Core & Conditioning" · Coach Klára · Intermediate
     9. Fri · 6:00 · "Friday Burn" · Coach Adam · Intermediate
     10. Fri · 17:00 · "Stretch & Restore" · Coach Tereza · Beginner
     11. Sat · 9:00 · "Team WOD" · Coach Ondřej · All levels
     12. Sat · 10:30 · "Weekend Yoga" · Coach Tereza · Beginner

5. **Trainers** (`#trainers`), kicker "Coaches", title "Meet the team". Four coaches, each with a photo, name, specialty and one sentence. Photo URL `https://picsum.photos/seed/forge-coach-N/600/700` where N is 1 to 4; the alt text is the coach's name.
   1. "Adam Novotný" · "Strength & conditioning" · "Former rower who builds engines that last."
   2. "Klára Benešová" · "Kettlebells & core" · "Turns nervous beginners into confident lifters."
   3. "Tereza Malá" · "Yoga & mobility" · "Helps you move better so you can train longer."
   4. "Ondřej Černý" · "Olympic lifting" · "National-level lifter with an eye for technique."

6. **Membership** (`#membership`), kicker "Membership", title "Pick your plan". A switch "Monthly / Yearly" changes the prices of all three plans (without reloading the page); Monthly is selected when the page loads.
   - "Basic" · 790 Kč per month · 7,900 Kč per year · Gym floor access, 2 classes a week, App booking
   - "Unlimited" (marked "Most popular", highlighted) · 1,290 Kč per month · 12,900 Kč per year · Gym floor access, Unlimited classes, App booking, 1 coaching session a month
   - "Premium" · 1,990 Kč per month · 19,900 Kč per year · Everything in Unlimited, Weekly personal training, Towel and locker service
   - Each plan has a button "Join now" linking to `#contact`.

7. **Testimonials**, title "Members say", three quotes:
   - "I came for the free trial and stayed for the people." — Lenka M.
   - "Adam fixed my squat in two sessions after years of knee pain." — Jakub S.
   - "The only gym where the 6 a.m. class feels like a party." — Veronika D.

8. **Gallery** (`#gallery`), kicker "Gallery", title "Inside the studio". Eight photos in a grid; clicking a photo opens it large (a lightbox with a close button and next/previous buttons, also usable with the keyboard: Escape closes, arrow keys move). Photos: `https://picsum.photos/seed/forge-g1/1200/800` up to `forge-g8`. Alt texts: "The main training floor", "Kettlebells lined up", "A yoga class at sunset", "Barbells and plates", "The rowing machines", "A coach helping a member", "The stretching corner", "The front desk and café".

9. **Contact** (`#contact`), kicker "Contact", title "Come for a free trial".
   - Address: "Veveří 54, 602 00 Brno", with a link "Open in Google Maps" (`https://maps.google.com/?q=Veveří+54,+Brno`).
   - Phone "+420 541 000 777" (clickable `tel:` link) and e-mail "hello@forgefitness.cz" (clickable `mailto:` link).
   - Opening hours: Monday–Friday 6:00–22:00, Saturday–Sunday 8:00–20:00.
   - A form with Name (required), E-mail (required), Phone (optional), Interested in (required, a select with: Free trial, Membership, Personal training) and Message (required). Every field has a visible label. The button says "Send".
   - Sending it opens the visitor's e-mail app with a message addressed to hello@forgefitness.cz, subject "Free trial request", and all the entered details in the body (no server).

10. **Footer**: "Forge Fitness Studio", the address, the five section links, and "© 2026 Forge Fitness Studio".

Page title: "Forge Fitness Studio – strength, conditioning and yoga in Brno". Description for search engines: "Independent gym and group-class studio on Veveří in Brno: small classes, expert coaches, monthly or yearly membership and a free trial."

## Your answer
Reply with the complete `site.html` in one code block and nothing else: no explanation before or after, no "rest of the code here" placeholders.
